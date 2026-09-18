//! Framing analysis (PRD §11.2): sample frames across the candidate interval,
//! detect faces, and decide the layout.
//!
//! - A dominant face  → locked vertical crop centered on it (ADR-0001: the
//!   camera never moves).
//! - No reliable face → uncropped source over a blurred background.
//!
//! The face track reduces to a single position: the median x-center of the
//! dominant cluster — the persistent cluster with the most detections
//! (tiebreaks: larger mean face size, then earliest first-detection).
//! Single-face clips lock that face; BlurPad is only for clips with no
//! reliable face at all.
//!
//! Two-person interviews (ADR-0004): when a project diarization proves two
//! voices share the clip, two persistent face clusters change the plan —
//! faces that co-exist in the same frames get a stacked Split panel each;
//! faces that appear in alternating shots (multi-cam) get a SpeakerCrop
//! that hard-cuts to whichever face's mouth moves during each speaker turn.
//! Without a diarization the single-face behavior is unchanged.
//!
//! Face detection uses rustface (SeetaFace, pure Rust). If the model file is
//! missing or detection fails, we degrade gracefully to BlurPad — never crash
//! a render over framing.

use crate::config::Config;
use crate::domain::{CropKey, Diarization, FaceAnchor, LayoutPlan, SourceInfo};
use crate::util::run_streaming;
use anyhow::{bail, Result};
use rustface::ImageData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const SAMPLE_FPS: f64 = 1.0;
/// Dense sampling rate for the mouth-motion pass. Only runs for two-face
/// multi-cam candidates, so it doesn't tax ordinary clips.
const DENSE_FPS: f64 = 4.0;
const SAMPLE_WIDTH: u32 = 480;
/// A face cluster must appear in at least this fraction of sampled frames to
/// count as persistent.
const PERSISTENCE: f64 = 0.5;
/// Cluster width as a fraction of frame width.
const CLUSTER_EPS: f32 = 0.18;
/// Opening face gate: the dominant face must appear within this many
/// leading sampled frames (~2s at SAMPLE_FPS = 1). Otherwise the clip
/// opens on an empty room and BlurPad is the honest framing.
const OPENING_FACE_GATE_FRAMES: usize = 2;
/// The second face must be substantial — at least this fraction of the
/// dominant cluster's detections, and at least this fraction of its mean
/// face size — to qualify as an interview partner rather than a background
/// face passing through.
const SECOND_FACE_COUNT_RATIO: f64 = 0.35;
const SECOND_FACE_SIZE_RATIO: f32 = 0.5;
/// Two persistent clusters detected in the same frame this often means a
/// wide two-shot (both hosts on camera at once) rather than alternating
/// single-person shots.
const CO_PRESENT_TAU: f64 = 0.5;
/// A speaker claims a face when this share of the mouth motion during
/// their turns lands on it. Below that, the correlation is noise and the
/// speaker keeps the dominant face.
const MOUTH_MOTION_SHARE: f32 = 0.55;

/// One face detection in a sampled frame.
#[derive(Clone, Copy, Debug)]
pub struct FaceDet {
    /// Normalized horizontal center (0–1) of the face bounding box.
    pub cx: f32,
    /// Normalized vertical center (0–1) — needed to place split panels.
    pub cy: f32,
    /// Normalized face width (bbox width / frame width) — the "size" input
    /// to the dominant-cluster tiebreak.
    pub w: f32,
}

#[allow(clippy::too_many_arguments)]
pub async fn analyze_layout(
    cfg: &Config,
    src: &Path,
    source: &SourceInfo,
    start_ms: u64,
    end_ms: u64,
    diarization: Option<&Diarization>,
    frames_dir: &Path,
    cancel: &CancellationToken,
) -> Result<LayoutPlan> {
    // Portrait-ish sources can't be face-cropped to 9:16 — pad them.
    if (source.width as f64) / (source.height as f64) < 1.05 {
        return Ok(LayoutPlan::BlurPad);
    }
    let Some(model_path) = cfg.face_model.as_ref() else {
        return Ok(LayoutPlan::BlurPad);
    };

    // 1. Sample frames with ffmpeg.
    tokio::fs::remove_dir_all(frames_dir).await.ok();
    tokio::fs::create_dir_all(frames_dir).await?;
    let dur_s = (end_ms - start_ms) as f64 / 1000.0;
    let args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-ss".into(),
        format!("{:.3}", start_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", dur_s),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-vf".into(),
        format!("fps={},scale={}:-2", SAMPLE_FPS, SAMPLE_WIDTH),
        "-q:v".into(),
        "6".into(),
        frames_dir.join("f%04d.jpg").to_string_lossy().into_owned(),
    ];
    run_streaming(&cfg.ffmpeg, &args, cancel, |_, _| {}).await?;

    // 2. List frames + detect faces (blocking CPU/fs work off the runtime).
    let model_path = model_path.clone();
    let dir = frames_dir.to_path_buf();
    let cancelled = Arc::new(AtomicBool::new(cancel.is_cancelled()));
    let watcher_flag = cancelled.clone();
    let watcher_token = cancel.clone();
    let watcher = tokio::spawn(async move {
        watcher_token.cancelled().await;
        watcher_flag.store(true, Ordering::Relaxed);
    });
    let worker_flag = cancelled.clone();
    let detection_result =
        tokio::task::spawn_blocking(move || detect_all(&model_path, &dir, &worker_flag)).await;
    watcher.abort();
    tokio::fs::remove_dir_all(frames_dir).await.ok();
    let detections: Vec<Vec<FaceDet>> = detection_result??;
    if cancelled.load(Ordering::Relaxed) || cancel.is_cancelled() {
        bail!("cancelled");
    }
    if detections.is_empty() {
        return Ok(LayoutPlan::BlurPad);
    }

    // 3. Decide the layout. Two substantial persistent faces + a two-voice
    //    diarization unlock the speaker-aware layouts; everything else is
    //    the pre-existing single-face path.
    let n_frames = detections.len();
    let clusters = persistent_clusters(&detections, n_frames);
    let mut motion: Option<Vec<Vec<f32>>> = None;
    if needs_mouth_motion(&clusters, n_frames, diarization, start_ms, end_ms) {
        motion = mouth_motion_scores(
            cfg,
            src,
            &clusters,
            start_ms,
            end_ms,
            diarization.unwrap(),
            frames_dir,
            cancel,
        )
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "mouth-motion pass failed; falling back");
            None
        });
    }
    let plan = decide_layout_full(
        &clusters,
        n_frames,
        diarization,
        start_ms,
        end_ms,
        motion.as_deref(),
    );
    tracing::info!(
        frames = n_frames,
        layout = plan.label(),
        "framing analysis complete"
    );
    Ok(plan)
}

/// Per frame, return the detected faces (normalized center x + width).
/// Runs inside `spawn_blocking`: directory listing and detection are
/// synchronous CPU/fs work that must stay off the async runtime.
fn detect_all(
    model_path: &Path,
    frames_dir: &Path,
    cancelled: &AtomicBool,
) -> Result<Vec<Vec<FaceDet>>> {
    if cancelled.load(Ordering::Relaxed) {
        bail!("cancelled");
    }
    let mut frames: Vec<PathBuf> = std::fs::read_dir(frames_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "jpg").unwrap_or(false))
        .collect();
    frames.sort();

    let mut detector = rustface::create_detector(&model_path.to_string_lossy())
        .map_err(|e| anyhow::anyhow!("face detector init failed: {}", e))?;
    detector.set_min_face_size(24);
    detector.set_score_thresh(2.0);
    detector.set_pyramid_scale_factor(0.8);
    detector.set_slide_window_step(4, 4);

    let mut out = Vec::with_capacity(frames.len());
    for path in &frames {
        if cancelled.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let faces = match image::open(path) {
            Ok(img) => {
                let gray = img.to_luma8();
                let (w, h) = (gray.width(), gray.height());
                let data = ImageData::new(&gray, w, h);
                detector
                    .detect(&data)
                    .into_iter()
                    .filter(|f| f.score() > 2.0)
                    .map(|f| {
                        let b = f.bbox();
                        FaceDet {
                            cx: (b.x() as f32 + b.width() as f32 / 2.0) / w as f32,
                            cy: (b.y() as f32 + b.height() as f32 / 2.0) / h as f32,
                            w: b.width() as f32 / w as f32,
                        }
                    })
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        out.push(faces);
    }
    Ok(out)
}

/// Cluster detections across frames and return the persistent clusters,
/// ordered dominant-first (most detections; ties: larger mean face, then
/// earliest first detection).
pub fn persistent_clusters(
    detections: &[Vec<FaceDet>],
    n_frames: usize,
) -> Vec<Vec<(usize, FaceDet)>> {
    let mut clusters: Vec<Vec<(usize, FaceDet)>> = Vec::new(); // (frame_idx, det)
    for (fi, faces) in detections.iter().enumerate() {
        for &d in faces {
            match clusters.iter_mut().find(|cl| {
                let mean = cl.iter().map(|(_, d)| d.cx).sum::<f32>() / cl.len() as f32;
                (mean - d.cx).abs() < CLUSTER_EPS
            }) {
                Some(cl) => cl.push((fi, d)),
                None => clusters.push(vec![(fi, d)]),
            }
        }
    }
    clusters.retain(|cl| {
        let distinct: std::collections::HashSet<usize> = cl.iter().map(|(fi, _)| *fi).collect();
        distinct.len() as f64 / n_frames.max(1) as f64 >= PERSISTENCE
    });
    clusters.sort_by(|a, b| {
        b.len()
            .cmp(&a.len())
            .then_with(|| {
                mean_face_size(b)
                    .partial_cmp(&mean_face_size(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| first_frame(a).cmp(&first_frame(b)))
    });
    clusters
}

/// Cluster detections across frames, pick the dominant one, and emit a
/// single locked crop position (ADR-0001). BlurPad only when no cluster is
/// persistent. Speaker-free entry point — exercised by the tests; the
/// pipeline always goes through [`decide_layout_full`].
#[cfg(test)]
pub fn decide_layout(detections: &[Vec<FaceDet>], n_frames: usize) -> LayoutPlan {
    let clusters = persistent_clusters(detections, n_frames);
    decide_layout_full(&clusters, n_frames, None, 0, 0, None)
}

/// Full layout decision (ADR-0004):
/// - 0 persistent clusters, or a dominant face absent from the opening
///   frames → BlurPad.
/// - 2+ persistent clusters, a substantial second face, and ≥2 speakers in
///   the clip's diarization:
///   * faces co-present in the same frames (wide two-shot) → Split;
///   * faces in alternating frames (multi-cam) + confident mouth-motion
///     correlation → SpeakerCrop cutting at turn boundaries;
///   * correlation missing or unconvinced → single-face lock as before.
/// - otherwise → FaceCrop on the dominant cluster.
///
/// `motion` is `mouth_motion_scores` output: `motion[cluster_rank][speaker]`
/// mean luma-diff in the face's mouth region during that speaker's turns.
pub fn decide_layout_full(
    persistent: &[Vec<(usize, FaceDet)>],
    n_frames: usize,
    diarization: Option<&Diarization>,
    clip_start_ms: u64,
    clip_end_ms: u64,
    motion: Option<&[Vec<f32>]>,
) -> LayoutPlan {
    // No reliable face → BlurPad. One or more → lock the dominant cluster.
    let Some(dominant) = persistent.first() else {
        return LayoutPlan::BlurPad;
    };

    // Two-person path: the second face must be substantial and the
    // diarization must actually hear two voices in this clip.
    let speakers = diarization
        .map(|d| d.speakers_in(clip_start_ms, clip_end_ms))
        .unwrap_or_default();
    if let (Some(second), true) = (persistent.get(1), speakers.len() >= 2) {
        let substantial = second.len() as f64 >= SECOND_FACE_COUNT_RATIO * dominant.len() as f64
            && mean_face_size(second) >= SECOND_FACE_SIZE_RATIO * mean_face_size(dominant);
        if substantial {
            if co_presence(dominant, second, n_frames) >= CO_PRESENT_TAU {
                // Wide two-shot: both hosts are on camera together —
                // stack them, left face on top.
                let (top, bottom) = if median_cx(dominant) <= median_cx(second) {
                    (dominant, second)
                } else {
                    (second, dominant)
                };
                return LayoutPlan::Split {
                    top: cluster_anchor(top),
                    bottom: cluster_anchor(bottom),
                };
            }
            if let Some(motion) = motion {
                if let Some(plan) = speaker_crop_plan(
                    persistent,
                    motion,
                    diarization.unwrap(),
                    clip_start_ms,
                    clip_end_ms,
                ) {
                    return plan;
                }
            }
        }
    }

    // Opening face gate: if the dominant face isn't in frame within the
    // clip's first ~2 sampled seconds (frames 0 and 1 at SAMPLE_FPS = 1),
    // a locked crop would open on an empty room aimed at the face's future
    // position — pad instead.
    if first_frame(dominant) >= OPENING_FACE_GATE_FRAMES {
        return LayoutPlan::BlurPad;
    }

    LayoutPlan::FaceCrop {
        keyframes: vec![CropKey {
            t_ms: 0,
            cx: median_cx(dominant),
        }],
    }
}

/// Does this clip warrant the dense mouth-motion pass? Only two substantial
/// faces in alternating frames + a two-voice diarization need it — the wide
/// two-shot answers itself, and single-face clips never ask.
fn needs_mouth_motion(
    clusters: &[Vec<(usize, FaceDet)>],
    n_frames: usize,
    diarization: Option<&Diarization>,
    clip_start_ms: u64,
    clip_end_ms: u64,
) -> bool {
    let (Some(dominant), Some(second), Some(d)) = (clusters.first(), clusters.get(1), diarization)
    else {
        return false;
    };
    if d.speakers_in(clip_start_ms, clip_end_ms).len() < 2 {
        return false;
    }
    let substantial = second.len() as f64 >= SECOND_FACE_COUNT_RATIO * dominant.len() as f64
        && mean_face_size(second) >= SECOND_FACE_SIZE_RATIO * mean_face_size(dominant);
    substantial && co_presence(dominant, second, n_frames) < CO_PRESENT_TAU
}

/// Fraction of sampled frames where both clusters have a detection.
fn co_presence(a: &[(usize, FaceDet)], b: &[(usize, FaceDet)], n_frames: usize) -> f64 {
    let fa: std::collections::HashSet<usize> = a.iter().map(|(fi, _)| *fi).collect();
    let fb: std::collections::HashSet<usize> = b.iter().map(|(fi, _)| *fi).collect();
    fa.intersection(&fb).count() as f64 / n_frames.max(1) as f64
}

/// A cluster's split-panel anchor: median horizontal and vertical centers.
fn cluster_anchor(cluster: &[(usize, FaceDet)]) -> FaceAnchor {
    FaceAnchor {
        cx: median_cx(cluster),
        cy: median_val(cluster.iter().map(|(_, d)| d.cy)),
    }
}

/// Speaker → face-cluster correlation (offline surrogate for lip-sync):
/// the mouth region of the face that's actually talking moves during that
/// speaker's turns. Each speaker claims the cluster holding ≥
/// MOUTH_MOTION_SHARE of the motion on their turns; unclaimed or ambiguous
/// speakers hold the dominant face.
fn speaker_face_map(motion: &[Vec<f32>], n_speakers: usize) -> Vec<usize> {
    (0..n_speakers)
        .map(|s| {
            let total: f32 = motion
                .iter()
                .map(|c| c.get(s).copied().unwrap_or(0.0))
                .sum();
            (0..motion.len())
                .find(|&c| {
                    total > f32::EPSILON
                        && motion[c].get(s).copied().unwrap_or(0.0) / total >= MOUTH_MOTION_SHARE
                })
                .unwrap_or(0)
        })
        .collect()
}

/// Build the active-speaker locked crop: one CropKey per speaker turn,
/// each a hard cut to that speaker's correlated face at the turn's
/// boundary — turns start in silence gaps, so the cut never lands
/// mid-sentence. Returns None when the correlation collapses to a single
/// face (nothing to cut to).
fn speaker_crop_plan(
    persistent: &[Vec<(usize, FaceDet)>],
    motion: &[Vec<f32>],
    diarization: &Diarization,
    clip_start_ms: u64,
    clip_end_ms: u64,
) -> Option<LayoutPlan> {
    let face_map = speaker_face_map(motion, diarization.labels.len());
    let cluster_cx: Vec<f32> = persistent.iter().map(|c| median_cx(c)).collect();
    let speaker_face =
        |spk: u8| -> f32 { cluster_cx[face_map.get(spk as usize).copied().unwrap_or(0)] };

    let mut keyframes: Vec<CropKey> = Vec::new();
    for turn in diarization.turns_in(clip_start_ms, clip_end_ms) {
        let cx = speaker_face(turn.speaker);
        let t = turn.start_ms.saturating_sub(clip_start_ms);
        if keyframes.last().map(|k| k.cx) == Some(cx) {
            continue; // same face — no cut
        }
        keyframes.push(CropKey { t_ms: t, cx });
    }
    if keyframes.first().map(|k| k.t_ms) != Some(0) {
        // Crop must hold something before the first observed turn.
        keyframes.insert(
            0,
            CropKey {
                t_ms: 0,
                cx: keyframes.first().map(|k| k.cx).unwrap_or(cluster_cx[0]),
            },
        );
    }
    let distinct: std::collections::HashSet<u32> =
        keyframes.iter().map(|k| k.cx.to_bits()).collect();
    (distinct.len() >= 2).then_some(LayoutPlan::SpeakerCrop { keyframes })
}

// ---------------------------------------------------------------------------
// Dense mouth-motion pass
// ---------------------------------------------------------------------------

/// A dense-pass face detection pinned to a persistent cluster.
#[derive(Clone, Copy)]
struct DenseFace {
    cluster: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Second ffmpeg pass at DENSE_FPS: correlate mouth-region pixel motion
/// with speaker turns. Returns `motion[cluster_rank][speaker]` — mean luma
/// difference inside each face's mouth strip over frame pairs inside that
/// speaker's turns.
#[allow(clippy::too_many_arguments)]
async fn mouth_motion_scores(
    cfg: &Config,
    src: &Path,
    clusters: &[Vec<(usize, FaceDet)>],
    start_ms: u64,
    end_ms: u64,
    diarization: &Diarization,
    frames_dir: &Path,
    cancel: &CancellationToken,
) -> Result<Option<Vec<Vec<f32>>>> {
    let Some(model_path) = cfg.face_model.clone() else {
        return Ok(None);
    };
    let dense_dir = frames_dir.join("dense");
    tokio::fs::create_dir_all(&dense_dir).await?;
    let dur_s = (end_ms - start_ms) as f64 / 1000.0;
    let args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-ss".into(),
        format!("{:.3}", start_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", dur_s),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-vf".into(),
        format!("fps={},scale={}:-2", DENSE_FPS, SAMPLE_WIDTH),
        "-q:v".into(),
        "6".into(),
        dense_dir.join("d%04d.jpg").to_string_lossy().into_owned(),
    ];
    run_streaming(&cfg.ffmpeg, &args, cancel, |_, _| {}).await?;

    let cancelled = Arc::new(AtomicBool::new(cancel.is_cancelled()));
    let watcher_flag = cancelled.clone();
    let watcher_token = cancel.clone();
    let watcher = tokio::spawn(async move {
        watcher_token.cancelled().await;
        watcher_flag.store(true, Ordering::Relaxed);
    });
    let centers: Vec<f32> = clusters.iter().map(|c| median_cx(c)).collect();
    let n_speakers = diarization.labels.len();
    let turns = diarization.turns.clone();
    let worker_flag = cancelled.clone();
    let blocking_dir = dense_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        dense_motion_blocking(
            &model_path,
            &blocking_dir,
            &centers,
            start_ms,
            n_speakers,
            &turns,
            &worker_flag,
        )
    })
    .await;
    watcher.abort();
    tokio::fs::remove_dir_all(&dense_dir).await.ok();
    if cancelled.load(Ordering::Relaxed) {
        bail!("cancelled");
    }
    result?
}

/// Blocking half of `mouth_motion_scores`: decode each dense frame once,
/// assign its faces to clusters, and diff each face's mouth strip against
/// the previous frame's same face.
fn dense_motion_blocking(
    model_path: &Path,
    dense_dir: &Path,
    centers: &[f32],
    clip_start_ms: u64,
    n_speakers: usize,
    turns: &[crate::domain::SpeakerTurn],
    cancelled: &AtomicBool,
) -> Result<Option<Vec<Vec<f32>>>> {
    let mut frames: Vec<PathBuf> = std::fs::read_dir(dense_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "jpg").unwrap_or(false))
        .collect();
    frames.sort();
    if frames.len() < 2 {
        return Ok(None);
    }

    let mut detector = rustface::create_detector(&model_path.to_string_lossy())
        .map_err(|e| anyhow::anyhow!("face detector init failed: {}", e))?;
    detector.set_min_face_size(24);
    detector.set_score_thresh(2.0);
    detector.set_pyramid_scale_factor(0.8);
    detector.set_slide_window_step(4, 4);

    let frame_ms = 1000.0 / DENSE_FPS;
    let mut energy = vec![vec![0.0f32; n_speakers]; centers.len()];
    let mut pairs = vec![vec![0u32; n_speakers]; centers.len()];
    let mut prev: Option<(image::GrayImage, Vec<DenseFace>)> = None;

    for (fi, path) in frames.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let Ok(img) = image::open(path) else { continue };
        let gray = img.to_luma8();
        let (w, h) = (gray.width(), gray.height());
        let data = ImageData::new(&gray, w, h);
        let faces: Vec<DenseFace> = detector
            .detect(&data)
            .into_iter()
            .filter(|f| f.score() > 2.0)
            .filter_map(|f| {
                let b = f.bbox();
                let cx = (b.x() as f32 + b.width() as f32 / 2.0) / w as f32;
                centers
                    .iter()
                    .enumerate()
                    .map(|(ci, &c)| (ci, (c - cx).abs()))
                    .filter(|(_, d)| *d < CLUSTER_EPS)
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(ci, _)| DenseFace {
                        cluster: ci,
                        x: b.x().max(0) as u32,
                        y: b.y().max(0) as u32,
                        w: b.width(),
                        h: b.height(),
                    })
            })
            .collect();

        if let Some((prev_gray, prev_faces)) = &prev {
            let mid = clip_start_ms + ((fi as f64 - 0.5) * frame_ms) as u64;
            if let Some(speaker) = turns
                .iter()
                .find(|t| t.start_ms <= mid && mid < t.end_ms)
                .map(|t| t.speaker as usize)
            {
                for face in &faces {
                    if prev_faces.iter().any(|p| p.cluster == face.cluster) {
                        let diff = mouth_diff(prev_gray, &gray, *face);
                        energy[face.cluster][speaker] += diff;
                        pairs[face.cluster][speaker] += 1;
                    }
                }
            }
        }
        prev = Some((gray, faces));
    }

    let motion: Vec<Vec<f32>> = energy
        .into_iter()
        .zip(pairs)
        .map(|(e, p)| {
            e.into_iter()
                .zip(p)
                .map(|(sum, n)| if n > 0 { sum / n as f32 } else { 0.0 })
                .collect()
        })
        .collect();
    // Require at least one measured pair — otherwise every share is 0/0.
    if motion.iter().flatten().all(|v| *v <= 0.0) {
        return Ok(None);
    }
    Ok(Some(motion))
}

/// Mean absolute luma difference inside the face's mouth strip between two
/// consecutive dense frames. The mouth strip is the lower third of the
/// bounding box, horizontally centered — where talking changes pixels and
/// blinking doesn't.
fn mouth_diff(a: &image::GrayImage, b: &image::GrayImage, f: DenseFace) -> f32 {
    // Use the current frame's bbox for the strip; a face drifts little in
    // the 250 ms between dense samples.
    let fb = f;
    let (w_img, h_img) = (b.width(), b.height());
    let x0 = fb.x + (fb.w as f32 * 0.2) as u32;
    let x1 = (fb.x + (fb.w as f32 * 0.8) as u32).min(w_img);
    let y0 = fb.y + (fb.h as f32 * 0.65) as u32;
    let y1 = (fb.y + (fb.h as f32 * 0.95) as u32).min(h_img);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let pa = a.get_pixel(x.min(a.width() - 1), y.min(a.height() - 1))[0] as i32;
            let pb = b.get_pixel(x, y)[0] as i32;
            sum += (pa - pb).unsigned_abs() as u64;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum as f32 / n as f32
    }
}

/// Mean normalized face width across a cluster's detections.
fn mean_face_size(cluster: &[(usize, FaceDet)]) -> f32 {
    cluster.iter().map(|(_, d)| d.w).sum::<f32>() / cluster.len() as f32
}

/// Index of the first sampled frame the cluster appears in.
fn first_frame(cluster: &[(usize, FaceDet)]) -> usize {
    cluster
        .iter()
        .map(|(fi, _)| *fi)
        .min()
        .unwrap_or(usize::MAX)
}

/// Median x-center of a cluster's detections — the locked crop position.
fn median_cx(cluster: &[(usize, FaceDet)]) -> f32 {
    median_val(cluster.iter().map(|(_, d)| d.cx))
}

/// Median of any f32 stream over the cluster.
fn median_val(values: impl Iterator<Item = f32>) -> f32 {
    let mut xs: Vec<f32> = values.collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(cx: f32, w: f32) -> FaceDet {
        FaceDet { cx, cy: 0.45, w }
    }

    fn turn(start_ms: u64, end_ms: u64, speaker: u8) -> crate::domain::SpeakerTurn {
        crate::domain::SpeakerTurn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    fn diar(turns: Vec<crate::domain::SpeakerTurn>) -> Diarization {
        Diarization {
            labels: vec!["S1".into(), "S2".into()],
            turns,
        }
    }

    fn locked_cx(plan: LayoutPlan) -> f32 {
        match plan {
            LayoutPlan::FaceCrop { keyframes } => {
                assert_eq!(keyframes.len(), 1, "locked crop emits one keyframe");
                assert_eq!(keyframes[0].t_ms, 0);
                keyframes[0].cx
            }
            other => panic!("expected FaceCrop, got {:?}", other),
        }
    }

    #[test]
    fn no_faces_means_blur_pad() {
        let det: Vec<Vec<FaceDet>> = vec![vec![]; 30];
        assert_eq!(decide_layout(&det, 30), LayoutPlan::BlurPad);
    }

    #[test]
    fn flickering_detection_below_persistence_is_blur_pad() {
        // Face appears in only 30% of frames.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| {
                if i % 10 < 3 {
                    vec![det(0.5, 0.1)]
                } else {
                    vec![]
                }
            })
            .collect();
        assert_eq!(decide_layout(&det, 30), LayoutPlan::BlurPad);
    }

    #[test]
    fn single_face_locks_to_one_static_keyframe() {
        // Face drifts slowly from x=0.40 to x=0.46; the crop locks at the
        // median of the cluster's detections and never moves.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| vec![det(0.40 + i as f32 * 0.002, 0.1)])
            .collect();
        let cx = locked_cx(decide_layout(&det, 30));
        assert!(
            (cx - 0.429).abs() < 0.005,
            "expected median lock ~0.429, got {cx}"
        );
    }

    #[test]
    fn dominant_cluster_is_the_one_with_most_detections() {
        // A at 0.3 in all 30 frames; B at 0.7 in only 20 → A is dominant.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| {
                let mut v = vec![det(0.3, 0.1)];
                if i >= 10 {
                    v.push(det(0.7, 0.1));
                }
                v
            })
            .collect();
        let cx = locked_cx(decide_layout(&det, 30));
        assert!((cx - 0.3).abs() < 1e-4, "expected lock on 0.3, got {cx}");
    }

    #[test]
    fn equal_detection_counts_pick_the_larger_face() {
        // Both faces appear in every frame; the larger one wins the tiebreak.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|_| vec![det(0.3, 0.10), det(0.7, 0.20)])
            .collect();
        let cx = locked_cx(decide_layout(&det, 30));
        assert!((cx - 0.7).abs() < 1e-4, "expected lock on 0.7, got {cx}");
    }

    #[test]
    fn fully_tied_clusters_pick_the_earliest_first_detection() {
        // A spans frames 0..25, B spans 5..30 — same count (25) and size,
        // but A was seen first.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| {
                let mut v = Vec::new();
                if i < 25 {
                    v.push(det(0.3, 0.1));
                }
                if i >= 5 {
                    v.push(det(0.7, 0.1));
                }
                v
            })
            .collect();
        let cx = locked_cx(decide_layout(&det, 30));
        assert!((cx - 0.3).abs() < 1e-4, "expected lock on 0.3, got {cx}");
    }

    #[test]
    fn two_face_shot_locks_the_dominant_face() {
        // A two-person shot used to fall back to BlurPad; now it locks the
        // dominant (here: larger) face.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|_| vec![det(0.3, 0.22), det(0.7, 0.12)])
            .collect();
        let cx = locked_cx(decide_layout(&det, 30));
        assert!((cx - 0.3).abs() < 1e-4, "expected lock on 0.3, got {cx}");
    }

    #[test]
    fn single_frame_outlier_does_not_move_the_crop() {
        // One bad in-cluster detection (0.65 among steady 0.50) is just one
        // vote in the median — the lock stays at 0.50.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| vec![det(if i == 15 { 0.65 } else { 0.5 }, 0.1)])
            .collect();
        let cx = locked_cx(decide_layout(&det, 30));
        assert!(
            (cx - 0.5).abs() < 1e-4,
            "outlier leaked into the locked crop: cx={cx}"
        );
    }

    #[test]
    fn in_frame_detections_median_to_the_central_face() {
        // Three same-frame detections in one cluster: 0.44/0.50/0.56 → 0.50.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|_| vec![det(0.44, 0.1), det(0.50, 0.1), det(0.56, 0.1)])
            .collect();
        let cx = locked_cx(decide_layout(&det, 30));
        assert!((cx - 0.5).abs() < 1e-4, "expected lock on 0.5, got {cx}");
    }

    // ---- speaker-aware layouts (ADR-0004) ----

    /// Two-face multi-cam clip: faces never share a frame; each face's
    /// mouth moves during its own speaker's turns.
    fn multi_cam_fixture() -> (Vec<Vec<FaceDet>>, Diarization, Vec<Vec<f32>>) {
        // 30 sampled frames: face A alone for the first 15, face B for the rest.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| {
                if i < 15 {
                    vec![det(0.35, 0.14)]
                } else {
                    vec![det(0.7, 0.14)]
                }
            })
            .collect();
        // Speaker 1 talks 0–15 s (frames 0–15 ≈ clip seconds), speaker 2 after.
        let d = diar(vec![turn(0, 15_000, 0), turn(15_500, 30_000, 1)]);
        // Mouth motion correlates: cluster 0 (left face) moves for S1,
        // cluster 1 (right face) for S2.
        let motion = vec![vec![9.0, 1.0], vec![1.0, 8.0]];
        (det, d, motion)
    }

    #[test]
    fn two_person_wide_shot_splits_into_stacked_panels() {
        // Both faces on camera together in every frame → Split.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|_| vec![det(0.3, 0.14), det(0.7, 0.14)])
            .collect();
        let d = diar(vec![turn(0, 15_000, 0), turn(15_500, 30_000, 1)]);
        let clusters = persistent_clusters(&det, 30);
        let plan = decide_layout_full(&clusters, 30, Some(&d), 0, 30_000, None);
        match plan {
            LayoutPlan::Split { top, bottom } => {
                assert!((top.cx - 0.3).abs() < 1e-4, "left face on top");
                assert!((bottom.cx - 0.7).abs() < 1e-4, "right face below");
                assert!(top.cy > 0.0 && top.cy < 1.0);
            }
            other => panic!("expected Split, got {:?}", other),
        }
    }

    #[test]
    fn multi_cam_with_correlated_mouths_becomes_speaker_crop() {
        let (det, d, motion) = multi_cam_fixture();
        let clusters = persistent_clusters(&det, 30);
        let plan = decide_layout_full(&clusters, 30, Some(&d), 0, 30_000, Some(&motion));
        match plan {
            LayoutPlan::SpeakerCrop { keyframes } => {
                // Cut at the turn boundary (~15.5 s) from left to right face.
                assert_eq!(keyframes.len(), 2, "{keyframes:?}");
                assert_eq!(keyframes[0].t_ms, 0);
                assert!((keyframes[0].cx - 0.35).abs() < 1e-4);
                assert_eq!(keyframes[1].t_ms, 15_500);
                assert!((keyframes[1].cx - 0.7).abs() < 1e-4);
            }
            other => panic!("expected SpeakerCrop, got {:?}", other),
        }
    }

    #[test]
    fn multi_cam_without_diarization_stays_dominant_locked() {
        // Same alternating faces, but no speaker data → old behavior.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| {
                if i < 15 {
                    vec![det(0.35, 0.14)]
                } else {
                    vec![det(0.7, 0.14)]
                }
            })
            .collect();
        let clusters = persistent_clusters(&det, 30);
        let plan = decide_layout_full(&clusters, 30, None, 0, 30_000, None);
        let cx = locked_cx(plan);
        assert!(
            (cx - 0.35).abs() < 1e-4,
            "expected lock on first face, got {cx}"
        );
    }

    #[test]
    fn multi_cam_uncorrelated_motion_falls_back_to_dominant() {
        let (det, d, _motion) = multi_cam_fixture();
        // Motion evenly split — no face can claim either speaker.
        let motion = vec![vec![5.0, 5.0], vec![5.0, 5.0]];
        let clusters = persistent_clusters(&det, 30);
        let plan = decide_layout_full(&clusters, 30, Some(&d), 0, 30_000, Some(&motion));
        // Both speakers map to cluster 0 → single-face keys collapse to FaceCrop.
        let cx = locked_cx(plan);
        assert!((cx - 0.35).abs() < 1e-4);
    }

    #[test]
    fn background_face_below_substantiality_keeps_single_lock() {
        // Second face is real but tiny and rare — not an interview partner.
        let det: Vec<Vec<FaceDet>> = (0..30)
            .map(|i| {
                let mut v = vec![det(0.4, 0.16)];
                if i % 5 == 0 {
                    v.push(det(0.85, 0.05)); // 6 frames, small
                }
                v
            })
            .collect();
        let d = diar(vec![turn(0, 15_000, 0), turn(15_500, 30_000, 1)]);
        let clusters = persistent_clusters(&det, 30);
        let plan = decide_layout_full(&clusters, 30, Some(&d), 0, 30_000, None);
        let cx = locked_cx(plan);
        assert!(
            (cx - 0.4).abs() < 1e-4,
            "background face must not split, got {cx}"
        );
    }

    #[test]
    fn speaker_crop_keys_never_start_mid_turn() {
        // Turn boundaries define the only legal cut points.
        let (det, d, motion) = multi_cam_fixture();
        let clusters = persistent_clusters(&det, 30);
        let plan = decide_layout_full(&clusters, 30, Some(&d), 0, 30_000, Some(&motion));
        if let LayoutPlan::SpeakerCrop { keyframes } = plan {
            for k in keyframes.iter().skip(1) {
                assert!(
                    d.turns.iter().any(|t| t.start_ms == k.t_ms),
                    "keyframe at {}ms is not a turn boundary",
                    k.t_ms
                );
            }
        } else {
            panic!("expected SpeakerCrop");
        }
    }

    #[test]
    fn face_detection_checks_cancellation_before_blocking_work() {
        let cancelled = AtomicBool::new(true);
        let result = detect_all(
            Path::new("missing-model"),
            Path::new("missing-frames"),
            &cancelled,
        );
        assert!(result.unwrap_err().to_string().contains("cancelled"));
    }
}
