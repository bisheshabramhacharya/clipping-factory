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
//! Multi-face clips lock the dominant face; BlurPad is only for clips with
//! no reliable face at all.
//!
//! Face detection uses rustface (SeetaFace, pure Rust). If the model file is
//! missing or detection fails, we degrade gracefully to BlurPad — never crash
//! a render over framing.

use crate::config::Config;
use crate::domain::{CropKey, LayoutPlan, SourceInfo};
use crate::util::run_streaming;
use anyhow::{bail, Result};
use rustface::ImageData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const SAMPLE_FPS: f64 = 1.0;
const SAMPLE_WIDTH: u32 = 480;
/// A face cluster must appear in at least this fraction of sampled frames to
/// count as persistent.
const PERSISTENCE: f64 = 0.5;
/// Cluster width as a fraction of frame width.
const CLUSTER_EPS: f32 = 0.18;

/// One face detection in a sampled frame.
#[derive(Clone, Copy, Debug)]
pub struct FaceDet {
    /// Normalized horizontal center (0–1) of the face bounding box.
    pub cx: f32,
    /// Normalized face width (bbox width / frame width) — the "size" input
    /// to the dominant-cluster tiebreak.
    pub w: f32,
}

pub async fn analyze_layout(
    cfg: &Config,
    src: &Path,
    source: &SourceInfo,
    start_ms: u64,
    end_ms: u64,
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

    // 3. Decide the layout.
    let n_frames = detections.len();
    let plan = decide_layout(&detections, n_frames);
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

/// Cluster detections across frames, pick the dominant one, and emit a
/// single locked crop position (ADR-0001). BlurPad only when no cluster is
/// persistent.
pub fn decide_layout(detections: &[Vec<FaceDet>], n_frames: usize) -> LayoutPlan {
    if n_frames == 0 {
        return LayoutPlan::BlurPad;
    }
    // 1D clustering of face centers across all frames.
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
    let persistent: Vec<&Vec<(usize, FaceDet)>> = clusters
        .iter()
        .filter(|cl| {
            let distinct: std::collections::HashSet<usize> = cl.iter().map(|(fi, _)| *fi).collect();
            distinct.len() as f64 / n_frames as f64 >= PERSISTENCE
        })
        .collect();

    // No reliable face → BlurPad. One or more → lock the dominant cluster.
    let Some(&dominant) = persistent.iter().max_by(|a, b| {
        a.len()
            .cmp(&b.len())
            .then_with(|| {
                mean_face_size(a)
                    .partial_cmp(&mean_face_size(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            // Earliest first-detection wins: smaller frame index is "greater".
            .then_with(|| first_frame(b).cmp(&first_frame(a)))
    }) else {
        return LayoutPlan::BlurPad;
    };

    LayoutPlan::FaceCrop {
        keyframes: vec![CropKey {
            t_ms: 0,
            cx: median_cx(dominant),
        }],
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
    let mut xs: Vec<f32> = cluster.iter().map(|(_, d)| d.cx).collect();
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
        FaceDet { cx, w }
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
