//! Framing analysis (PRD §11.2, ADR-0004): split a Clip into camera shots,
//! decide who should be on screen in each, and emit one static view per
//! shot.
//!
//! - Shots come from ffmpeg `scdet` over the clip interval at the source
//!   frame rate, so a camera switch in the Source becomes a hard cut in the
//!   Clip on the same frame.
//! - Faces come from rustface on frames sampled at SAMPLE_FPS. The camera
//!   holds still within a shot, so detections are pooled per shot: a person
//!   the detector misses on some frames is still framed for the whole shot.
//! - One person in the shot → a crop on them. Several → the Active speaker:
//!   the face whose mouth moves most while words are spoken, chosen per bin
//!   with a switching cost so the view only cuts between people on
//!   sustained turns.
//! - Nobody usable → the whole frame over a blurred copy, never a crop aimed
//!   at an empty chair or the table.
//!
//! Within a view the crop never moves; between views it hard-cuts.
//! If the model file is missing or detection fails, callers degrade to
//! BlurPad — framing never fails a render.

use crate::config::Config;
use crate::domain::{CropKey, LayoutPlan, SourceInfo, Word};
use crate::util::run_streaming;
use anyhow::{bail, Context, Result};
use rustface::ImageData;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tokio_util::sync::CancellationToken;

const SAMPLE_FPS: f64 = 6.0;
const SAMPLE_WIDTH: u32 = 640;
const DETECT_THREADS: usize = 3;
/// scdet threshold for shot cuts. Lower than the Source-wide scan: a false
/// cut only splits one shot into two that frame the same way and merge.
const CUT_THRESHOLD: u32 = 6;
/// Cuts closer than this collapse; no shot is shorter.
const MIN_SHOT_MS: u64 = 300;
/// Smallest face the detector looks for, in sampled pixels (~3% of width).
const MIN_FACE_PX: u32 = 20;
/// A person must be detected in this share of a shot's frames (or in
/// SHOT_PRESENCE_CAP of them, so someone who fades in late in a long shot
/// still counts)…
const SHOT_PRESENCE: f64 = 0.3;
const SHOT_PRESENCE_CAP: usize = 9;
/// …and at least once with this detector score. Spurious hits sit near
/// 2–4 and never persist; profile faces in wide shots peak around 5–7.
const STRONG_SCORE: f32 = 5.0;
/// Faces under this fraction of the shot's largest face are background.
const MIN_FACE_REL: f32 = 0.45;
/// Face-box height as a share of the output height the crop aims for: head
/// and shoulders, neither a forehead close-up nor a room.
const TARGET_FACE_H: f32 = 0.20;
/// The crop may tighten past the full-height window by at most this much…
const MAX_ZOOM: f32 = 1.6;
/// …and never to a window shorter than this many source pixels.
const MIN_WINDOW_PX: f32 = 540.0;
/// Where the eyes land, as a share of the output height from the top: clear
/// of the hook-title band above and the caption band below.
const EYE_LINE: f32 = 0.36;
/// Eyes sit about 15% of the face-box height above its center.
const EYE_FACE_OFFSET: f32 = 0.15;
/// Speaker evidence is pooled into bins of this length.
const TURN_BIN_MS: u64 = 1000;
/// Cost of cutting between people, in bins of one-sided evidence: turns
/// shorter than ~2 s are not worth a cut.
const SWITCH_COST: f32 = 1.2;
/// Mild preference for the shot's largest face when evidence is flat, e.g.
/// a reaction shot while someone off camera talks.
const LARGEST_FACE_PRIOR: f32 = 0.3;
/// Cost, per bin, of a view on someone not seen in it while someone else is.
const ABSENT_COST: f32 = 2.0;

/// One face detection in a sampled frame, normalized to the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceDet {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
}

/// Everything the planner reads from the sampled frames.
struct Samples {
    faces: Vec<Vec<FaceDet>>,
    /// Half-resolution grayscale frames for mouth motion.
    small: Vec<Vec<u8>>,
    small_w: usize,
    small_h: usize,
}

pub async fn analyze_layout(
    cfg: &Config,
    src: &Path,
    source: &SourceInfo,
    start_ms: u64,
    end_ms: u64,
    words: &[Word],
    cancel: &CancellationToken,
) -> Result<LayoutPlan> {
    // Portrait-ish sources can't be face-cropped to 9:16 — pad them.
    if (source.width as f64) / (source.height as f64) < 1.05 {
        return Ok(LayoutPlan::BlurPad);
    }
    let Some(model_path) = cfg.face_model.clone() else {
        return Ok(LayoutPlan::BlurPad);
    };
    let dur_ms = end_ms.saturating_sub(start_ms);

    let cuts = shot_cuts(cfg, src, start_ms, dur_ms, cancel)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("shot detection failed, framing as one shot: {e:#}");
            Vec::new()
        });
    if cancel.is_cancelled() {
        return Err(crate::util::cancelled());
    }

    let sample_h =
        (((SAMPLE_WIDTH as u64 * source.height as u64) / source.width as u64) & !1) as u32;
    let cancelled = Arc::new(AtomicBool::new(false));
    let watcher_flag = cancelled.clone();
    let watcher_token = cancel.clone();
    let watcher = tokio::spawn(async move {
        watcher_token.cancelled().await;
        watcher_flag.store(true, Ordering::Relaxed);
    });
    let (ffmpeg, src_path, flag) = (cfg.ffmpeg.clone(), src.to_path_buf(), cancelled.clone());
    let sampled = tokio::task::spawn_blocking(move || {
        sample_and_detect(
            &ffmpeg,
            &src_path,
            &model_path,
            start_ms,
            dur_ms,
            (SAMPLE_WIDTH, sample_h),
            &flag,
        )
    })
    .await;
    watcher.abort();
    let samples = sampled??;
    if cancelled.load(Ordering::Relaxed) || cancel.is_cancelled() {
        return Err(crate::util::cancelled());
    }

    let speech: Vec<(u64, u64)> = words
        .iter()
        .filter(|w| w.end_ms > start_ms && w.start_ms < end_ms)
        .map(|w| {
            (
                w.start_ms.saturating_sub(start_ms),
                w.end_ms.min(end_ms) - start_ms,
            )
        })
        .collect();
    let keys = plan_shots(
        &samples,
        &cuts,
        dur_ms,
        &speech,
        (source.width, source.height),
    );
    let plan = if keys.iter().all(|k| k.pad) {
        LayoutPlan::BlurPad
    } else {
        LayoutPlan::FaceCrop { keyframes: keys }
    };
    tracing::info!(
        frames = samples.faces.len(),
        shots = cuts.len() + 1,
        layout = plan.label(),
        views = match &plan {
            LayoutPlan::FaceCrop { keyframes } => keyframes.len(),
            LayoutPlan::BlurPad => 1,
        },
        "framing analysis complete"
    );
    Ok(plan)
}

/// Faces detected in every sampled frame of a video, as JSON
/// `{"width","height","frames":[{"t_ms","faces":[{cx,cy,w,h,score}]}]}`.
/// Used by the eval harness to check rendered clips.
pub async fn probe_faces(cfg: &Config, video: &Path) -> Result<serde_json::Value> {
    let model = cfg
        .face_model
        .clone()
        .context("no face model found (set CF_FACE_MODEL)")?;
    let info = crate::media::probe(cfg, video, "", &CancellationToken::new()).await?;
    let h = (((SAMPLE_WIDTH as u64 * info.height as u64) / info.width.max(1) as u64) & !1) as u32;
    let (ffmpeg, path) = (cfg.ffmpeg.clone(), video.to_path_buf());
    let samples = tokio::task::spawn_blocking(move || {
        sample_and_detect(
            &ffmpeg,
            &path,
            &model,
            0,
            info.duration_ms,
            (SAMPLE_WIDTH, h),
            &AtomicBool::new(false),
        )
    })
    .await??;
    let frames: Vec<serde_json::Value> = samples
        .faces
        .iter()
        .enumerate()
        .map(|(i, faces)| {
            serde_json::json!({
                "t_ms": frame_ms(i),
                "faces": faces.iter().map(|f| serde_json::json!({
                    "cx": f.cx, "cy": f.cy, "w": f.w, "h": f.h, "score": f.score,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "width": info.width,
        "height": info.height,
        "frames": frames,
    }))
}

/// Shot cuts inside the clip, in ms relative to its start.
async fn shot_cuts(
    cfg: &Config,
    src: &Path,
    start_ms: u64,
    dur_ms: u64,
    cancel: &CancellationToken,
) -> Result<Vec<u64>> {
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-ss".into(),
        format!("{:.3}", start_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", dur_ms as f64 / 1000.0),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-an".into(),
        "-vf".into(),
        // Same zeroed timeline the render trims on (render.rs resets PTS).
        format!("setpts=PTS-STARTPTS,scale=320:-2,scdet=threshold={CUT_THRESHOLD}"),
        "-f".into(),
        "null".into(),
        "-".into(),
    ];
    let mut raw = Vec::new();
    run_streaming(&cfg.ffmpeg, &args, cancel, |is_err, line| {
        if is_err {
            if let Some(ms) = crate::media::parse_scdet_time_ms(line) {
                raw.push(ms);
            }
        }
    })
    .await?;
    Ok(clean_cuts(raw, dur_ms))
}

/// Sort, drop cuts too close to the clip edges, and collapse near-duplicates.
fn clean_cuts(mut raw: Vec<u64>, dur_ms: u64) -> Vec<u64> {
    raw.sort_unstable();
    let mut cuts: Vec<u64> = Vec::new();
    for t in raw {
        if t < MIN_SHOT_MS || t + MIN_SHOT_MS > dur_ms {
            continue;
        }
        if cuts.last().is_some_and(|&p| t < p + MIN_SHOT_MS) {
            continue;
        }
        cuts.push(t);
    }
    cuts
}

fn new_detector(model: &Path) -> Result<Box<dyn rustface::Detector>> {
    let mut d = rustface::create_detector(&model.to_string_lossy())
        .map_err(|e| anyhow::anyhow!("face detector init failed: {}", e))?;
    d.set_min_face_size(MIN_FACE_PX);
    d.set_score_thresh(2.0);
    d.set_pyramid_scale_factor(0.8);
    d.set_slide_window_step(4, 4);
    Ok(d)
}

/// Decode the clip at SAMPLE_FPS as raw grayscale, keep a half-resolution
/// copy of every frame, and detect faces on a small worker pool. Blocking;
/// runs inside `spawn_blocking`.
fn sample_and_detect(
    ffmpeg: &str,
    src: &Path,
    model: &Path,
    start_ms: u64,
    dur_ms: u64,
    (w, h): (u32, u32),
    cancelled: &AtomicBool,
) -> Result<Samples> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(crate::util::cancelled());
    }
    drop(new_detector(model)?);
    let mut child = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{:.3}", start_ms as f64 / 1000.0),
            "-t",
            &format!("{:.3}", dur_ms as f64 / 1000.0),
            "-i",
            &src.to_string_lossy(),
            "-an",
            "-vf",
            &format!("setpts=PTS-STARTPTS,fps={SAMPLE_FPS},scale={w}:{h},format=gray"),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start `{ffmpeg}`"))?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let frame_len = (w * h) as usize;
    let (tx, rx) = mpsc::sync_channel::<(usize, Vec<u8>)>(DETECT_THREADS * 2);
    let rx = Mutex::new(rx);
    let (done_tx, done_rx) = mpsc::channel::<(usize, Vec<FaceDet>)>();
    let mut small = Vec::new();

    let read = std::thread::scope(|s| -> Result<()> {
        for _ in 0..DETECT_THREADS {
            let (rx, done_tx) = (&rx, done_tx.clone());
            s.spawn(move || {
                // Init was checked above; a worker that still fails to load
                // keeps draining so the reader never blocks on a full queue.
                let mut det = new_detector(model).ok();
                loop {
                    let next = rx.lock().expect("frame queue").recv();
                    let Ok((i, frame)) = next else { break };
                    let faces = det
                        .as_mut()
                        .map(|d| detect(d.as_mut(), &frame, w, h))
                        .unwrap_or_default();
                    if done_tx.send((i, faces)).is_err() {
                        break;
                    }
                }
            });
        }
        let mut buf = vec![0u8; frame_len];
        let mut i = 0;
        let outcome = loop {
            if cancelled.load(Ordering::Relaxed) {
                break Err(crate::util::cancelled());
            }
            match stdout.read_exact(&mut buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break Ok(()),
                Err(e) => break Err(e.into()),
            }
            small.push(half_res(&buf, w as usize, h as usize));
            if tx.send((i, buf.clone())).is_err() {
                break Ok(());
            }
            i += 1;
        };
        drop(tx);
        outcome
    });
    drop(done_tx);
    if read.is_err() {
        child.kill().ok();
    }
    child.wait().ok();
    read?;

    let mut faces = vec![Vec::new(); small.len()];
    for (i, f) in done_rx.try_iter() {
        faces[i] = f;
    }
    if faces.is_empty() {
        bail!("no frames sampled from the clip");
    }
    Ok(Samples {
        faces,
        small,
        small_w: (w / 2) as usize,
        small_h: (h / 2) as usize,
    })
}

fn detect(det: &mut dyn rustface::Detector, frame: &[u8], w: u32, h: u32) -> Vec<FaceDet> {
    det.detect(&ImageData::new(frame, w, h))
        .into_iter()
        .map(|f| {
            let b = f.bbox();
            FaceDet {
                cx: (b.x() as f32 + b.width() as f32 / 2.0) / w as f32,
                cy: (b.y() as f32 + b.height() as f32 / 2.0) / h as f32,
                w: b.width() as f32 / w as f32,
                h: b.height() as f32 / h as f32,
                score: f.score() as f32,
            }
        })
        .collect()
}

/// 2×2 box downsample of a grayscale frame.
fn half_res(frame: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (hw, hh) = (w / 2, h / 2);
    let mut out = Vec::with_capacity(hw * hh);
    for y in 0..hh {
        let (r0, r1) = (&frame[2 * y * w..], &frame[(2 * y + 1) * w..]);
        for x in 0..hw {
            let sum =
                r0[2 * x] as u16 + r0[2 * x + 1] as u16 + r1[2 * x] as u16 + r1[2 * x + 1] as u16;
            out.push((sum / 4) as u8);
        }
    }
    out
}

fn frame_ms(i: usize) -> u64 {
    (i as f64 * 1000.0 / SAMPLE_FPS).round() as u64
}

/// One view per shot (several when the Active speaker changes inside a
/// shot), merged where consecutive views frame the same way.
fn plan_shots(
    s: &Samples,
    cuts: &[u64],
    dur_ms: u64,
    speech: &[(u64, u64)],
    src: (u32, u32),
) -> Vec<CropKey> {
    let mut bounds = vec![0];
    bounds.extend_from_slice(cuts);
    bounds.push(dur_ms);
    let mut keys = Vec::new();
    for shot in bounds.windows(2) {
        let (a, b) = (shot[0], shot[1]);
        let idx: Vec<usize> = (0..s.faces.len())
            .filter(|&i| (a..b).contains(&frame_ms(i)))
            .collect();
        let people = shot_people(&s.faces, &idx);
        match people.len() {
            0 => keys.push(CropKey::pad(a)),
            1 => keys.push(frame_face(&people[0].face, a, src)),
            _ => {
                let activity = mouth_activity(s, &idx, &people);
                let seen: Vec<Vec<u64>> = people
                    .iter()
                    .map(|p| p.dets.iter().map(|(i, _)| frame_ms(*i)).collect())
                    .collect();
                for (t, p) in speaker_turns(&activity, &seen, a, b, speech) {
                    keys.push(frame_face(&people[p].face, t, src));
                }
            }
        }
    }
    merge_keys(keys)
}

/// A person seen in one shot: the median of their detections, plus the
/// detections themselves keyed by frame.
struct Person {
    face: FaceDet,
    dets: Vec<(usize, FaceDet)>,
}

/// Cluster a shot's detections into people. Keeps those present in enough
/// of the shot's frames and seen clearly at least once, drops background
/// faces, and orders the rest largest first.
fn shot_people(faces: &[Vec<FaceDet>], idx: &[usize]) -> Vec<Person> {
    let mut clusters: Vec<Vec<(usize, FaceDet)>> = Vec::new();
    for &i in idx {
        for &d in &faces[i] {
            let found = clusters.iter_mut().find(|cl| {
                let m = mean_face(cl);
                let ratio = d.w / m.w;
                (m.cx - d.cx).abs() < 0.5 * m.w.max(d.w) + 0.02
                    && (m.cy - d.cy).abs() < 0.5 * m.h.max(d.h) + 0.02
                    && (0.6..=1.67).contains(&ratio)
            });
            match found {
                Some(cl) => cl.push((i, d)),
                None => clusters.push(vec![(i, d)]),
            }
        }
    }
    let need = ((SHOT_PRESENCE * idx.len() as f64).ceil() as usize).clamp(1, SHOT_PRESENCE_CAP);
    let mut people: Vec<Person> = clusters
        .into_iter()
        .filter(|cl| {
            let mut frames: Vec<usize> = cl.iter().map(|(i, _)| *i).collect();
            frames.dedup();
            frames.len() >= need && cl.iter().any(|(_, d)| d.score >= STRONG_SCORE)
        })
        .map(|dets| Person {
            face: median_face(&dets),
            dets,
        })
        .collect();
    let largest = people.iter().map(|p| p.face.w).fold(0.0, f32::max);
    people.retain(|p| p.face.w >= MIN_FACE_REL * largest);
    people.sort_by(|a, b| b.face.w.total_cmp(&a.face.w));
    people
}

fn mean_face(cl: &[(usize, FaceDet)]) -> FaceDet {
    let n = cl.len() as f32;
    let sum = |f: fn(&FaceDet) -> f32| cl.iter().map(|(_, d)| f(d)).sum::<f32>() / n;
    FaceDet {
        cx: sum(|d| d.cx),
        cy: sum(|d| d.cy),
        w: sum(|d| d.w),
        h: sum(|d| d.h),
        score: 0.0,
    }
}

fn median_face(cl: &[(usize, FaceDet)]) -> FaceDet {
    let med = |f: fn(&FaceDet) -> f32| median(cl.iter().map(|(_, d)| f(d)));
    FaceDet {
        cx: med(|d| d.cx),
        cy: med(|d| d.cy),
        w: med(|d| d.w),
        h: med(|d| d.h),
        score: med(|d| d.score),
    }
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut xs: Vec<f32> = values.collect();
    xs.sort_by(f32::total_cmp);
    let n = xs.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

/// Per sampled frame of the shot (from its second on), each person's mouth
/// motion: mean frame difference over the mouth minus the same over the
/// eyes and forehead, so nodding and leaning cancel out and talking stays.
fn mouth_activity(s: &Samples, idx: &[usize], people: &[Person]) -> Vec<(u64, Vec<f32>)> {
    let (w, h) = (s.small_w, s.small_h);
    idx.windows(2)
        .map(|pair| {
            let (prev, cur) = (&s.small[pair[0]], &s.small[pair[1]]);
            let scores = people
                .iter()
                .map(|p| {
                    let f = p
                        .dets
                        .iter()
                        .find(|(i, _)| *i == pair[1])
                        .map(|(_, d)| *d)
                        .unwrap_or(p.face);
                    let mouth = region_diff(prev, cur, w, h, f, 0.15, 0.5);
                    let upper = region_diff(prev, cur, w, h, f, -0.45, -0.1);
                    (mouth - upper).max(0.0)
                })
                .collect();
            (frame_ms(pair[1]), scores)
        })
        .collect()
}

/// Mean absolute difference between two frames over the face's middle half
/// width, between `top` and `bottom` face-heights from its center.
fn region_diff(a: &[u8], b: &[u8], w: usize, h: usize, f: FaceDet, top: f32, bottom: f32) -> f32 {
    let px = |v: f32, n: usize| ((v * n as f32).round().max(0.0) as usize).min(n);
    let (x0, x1) = (px(f.cx - 0.25 * f.w, w), px(f.cx + 0.25 * f.w, w));
    let (y0, y1) = (px(f.cy + top * f.h, h), px(f.cy + bottom * f.h, h));
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += (a[y * w + x] as i16 - b[y * w + x] as i16).unsigned_abs() as u64;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum as f32 / n as f32
    }
}

/// Who holds the view across one multi-person shot `[a, b)`: a Viterbi pass
/// over speech bins where each person's cost is one minus their share of
/// the bin's mouth motion, plus ABSENT_COST in bins where they aren't seen
/// but someone else is, and every change of person costs SWITCH_COST.
/// `seen` holds each person's detection times. Returns (start ms, person)
/// per turn; person 0 is the largest face.
fn speaker_turns(
    activity: &[(u64, Vec<f32>)],
    seen: &[Vec<u64>],
    a: u64,
    b: u64,
    speech: &[(u64, u64)],
) -> Vec<(u64, usize)> {
    let n = seen.len();
    let present = |p: usize, s: u64, e: u64| seen[p].iter().any(|t| (s..e).contains(t));
    let mut edges: Vec<u64> = (a..b).step_by(TURN_BIN_MS as usize).collect();
    // A short tail folds into the previous bin.
    if edges.len() > 1 && b - edges[edges.len() - 1] < TURN_BIN_MS / 2 {
        edges.pop();
    }
    edges.push(b);
    let costs: Vec<Vec<f32>> = edges
        .windows(2)
        .map(|bin| {
            let (s, e) = (bin[0], bin[1]);
            let spoken: u64 = speech
                .iter()
                .map(|&(ws, we)| we.min(e).saturating_sub(ws.max(s)))
                .sum();
            let mut motion = vec![0.0f32; n];
            for (_, scores) in activity.iter().filter(|(t, _)| (s..e).contains(t)) {
                for (m, v) in motion.iter_mut().zip(scores) {
                    *m += v;
                }
            }
            let total: f32 = motion.iter().sum();
            let mut cost = if spoken * 10 < (e - s) * 3 || total <= f32::EPSILON {
                vec![0.0; n]
            } else {
                motion.iter().map(|m| 1.0 - m / total).collect()
            };
            // Someone the camera isn't showing can't hold the view: an
            // unflagged cut or a dissolve swaps who is in frame.
            let here: Vec<bool> = (0..n).map(|p| present(p, s, e)).collect();
            if here.iter().any(|&h| h) {
                for (c, h) in cost.iter_mut().zip(&here) {
                    if !h {
                        *c += ABSENT_COST;
                    }
                }
            }
            cost
        })
        .collect();

    // Viterbi: best[p] is the cheapest path ending on person p.
    let mut best: Vec<f32> = (0..n)
        .map(|p| if p == 0 { 0.0 } else { LARGEST_FACE_PRIOR })
        .collect();
    let mut back: Vec<Vec<usize>> = Vec::with_capacity(costs.len());
    for (k, c) in costs.iter().enumerate() {
        if k == 0 {
            best.iter_mut().zip(c).for_each(|(b, c)| *b += c);
            back.push((0..n).collect());
            continue;
        }
        let mut next = vec![0.0; n];
        let mut from = vec![0; n];
        for p in 0..n {
            let (q, cost) = (0..n)
                .map(|q| (q, best[q] + if q == p { 0.0 } else { SWITCH_COST }))
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .expect("at least one person");
            next[p] = cost + c[p];
            from[p] = q;
        }
        best = next;
        back.push(from);
    }
    let mut p = (0..n)
        .min_by(|&x, &y| best[x].total_cmp(&best[y]))
        .expect("at least one person");
    let mut path = vec![p; costs.len()];
    for k in (1..costs.len()).rev() {
        p = back[k][p];
        path[k - 1] = p;
    }

    let mut turns: Vec<(u64, usize)> = vec![(a, path[0])];
    for k in 1..path.len() {
        let (old, new) = (path[k - 1], path[k]);
        if old == new {
            continue;
        }
        // A swap of who is on screen cuts between the last sighting of one
        // and the first of the other; a turn between people who are both
        // visible cuts on the new speaker's first word.
        let last_old = seen[old]
            .iter()
            .copied()
            .filter(|&t| t < edges[k + 1])
            .max();
        let first_new = seen[new]
            .iter()
            .copied()
            .filter(|&t| t >= edges[k - 1] && last_old.is_none_or(|l| t > l))
            .min();
        let t = match (last_old, first_new) {
            (Some(l), Some(f)) if !present(old, edges[k], edges[k + 1]) => (l + f) / 2,
            _ => snap_to_word(edges[k], a, b, speech),
        };
        turns.push((t, new));
    }
    turns
}

/// Move a turn change onto the nearest word start within half a bin — ideally
/// one after a pause — so the view cuts as the new speaker starts talking.
fn snap_to_word(t: u64, a: u64, b: u64, speech: &[(u64, u64)]) -> u64 {
    let reach = TURN_BIN_MS / 2;
    let lo = (a + MIN_SHOT_MS).max(t.saturating_sub(reach));
    let hi = b.saturating_sub(MIN_SHOT_MS).min(t + reach);
    let gap_before = |i: usize| -> u64 {
        match i {
            0 => u64::MAX,
            _ => speech[i].0.saturating_sub(speech[i - 1].1),
        }
    };
    (0..speech.len())
        .filter(|&i| (lo..=hi).contains(&speech[i].0))
        .min_by_key(|&i| {
            let paused = gap_before(i) >= 150;
            (!paused, speech[i].0.abs_diff(t))
        })
        .map(|i| speech[i].0)
        .unwrap_or(t)
}

/// The static view for one face, starting at `t_ms`: tighten past the
/// full-height window until the face box is TARGET_FACE_H of the frame
/// (within the zoom limits), center it horizontally, and put the eyes on
/// EYE_LINE as far as the window allows.
fn frame_face(f: &FaceDet, t_ms: u64, (sw, sh): (u32, u32)) -> CropKey {
    let (sw, sh) = (sw as f32, sh as f32);
    let max_zoom = (sh / MIN_WINDOW_PX).clamp(1.0, MAX_ZOOM);
    let zoom = if f.h > 0.0 {
        (TARGET_FACE_H / f.h).clamp(1.0, max_zoom)
    } else {
        1.0
    };
    let win_h = 1.0 / zoom;
    let win_w = (win_h * sh * 9.0 / 16.0 / sw).min(1.0);
    let cx = f.cx.clamp(win_w / 2.0, 1.0 - win_w / 2.0);
    let eye = f.cy - EYE_FACE_OFFSET * f.h;
    let cy = (eye - EYE_LINE * win_h + win_h / 2.0).clamp(win_h / 2.0, 1.0 - win_h / 2.0);
    CropKey::crop(t_ms, cx, cy, zoom)
}

/// Drop views that repeat the previous one — a false cut, or the same
/// person across a speaker change that framed identically.
fn merge_keys(keys: Vec<CropKey>) -> Vec<CropKey> {
    let mut out: Vec<CropKey> = Vec::with_capacity(keys.len());
    for k in keys {
        let same = out.last().is_some_and(|p| {
            (p.pad && k.pad)
                || (!p.pad
                    && !k.pad
                    && (p.cx - k.cx).abs() < 0.03
                    && (p.cy - k.cy).abs() < 0.03
                    && (p.zoom - k.zoom).abs() < 0.08)
        });
        if !same {
            out.push(k);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: (u32, u32) = (1920, 1080);

    fn face(cx: f32, w: f32) -> FaceDet {
        FaceDet {
            cx,
            cy: 0.35,
            w,
            h: w * 16.0 / 9.0,
            score: 20.0,
        }
    }

    fn samples(faces: Vec<Vec<FaceDet>>) -> Samples {
        let n = faces.len();
        Samples {
            faces,
            small: vec![vec![0; 4]; n],
            small_w: 2,
            small_h: 2,
        }
    }

    /// n frames at SAMPLE_FPS spanning `n / SAMPLE_FPS` seconds.
    fn dur(n: usize) -> u64 {
        frame_ms(n)
    }

    #[test]
    fn a_shot_with_no_face_shows_the_full_frame() {
        let s = samples(vec![vec![]; 30]);
        assert_eq!(
            plan_shots(&s, &[], dur(30), &[], SRC),
            vec![CropKey::pad(0)]
        );
    }

    #[test]
    fn one_person_gets_one_static_view_for_the_shot() {
        let s = samples(
            (0..30)
                .map(|i| vec![face(0.40 + i as f32 * 0.002, 0.12)])
                .collect(),
        );
        let keys = plan_shots(&s, &[], dur(30), &[], SRC);
        assert_eq!(keys.len(), 1);
        assert!(!keys[0].pad);
        assert!((keys[0].cx - 0.429).abs() < 0.005, "cx={}", keys[0].cx);
    }

    #[test]
    fn a_camera_cut_switches_the_view_on_the_cut() {
        // Close-up on the left for 3 s, then a cut to someone on the right.
        let s = samples(
            (0..36)
                .map(|i| vec![face(if i < 18 { 0.3 } else { 0.7 }, 0.12)])
                .collect(),
        );
        let keys = plan_shots(&s, &[3000], dur(36), &[], SRC);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].t_ms, 0);
        assert!((keys[0].cx - 0.3).abs() < 0.01);
        assert_eq!(keys[1].t_ms, 3000);
        assert!((keys[1].cx - 0.7).abs() < 0.01);
    }

    #[test]
    fn a_faceless_cutaway_pads_instead_of_holding_the_old_crop() {
        // The bug this design exists for: after a cut to a shot with nobody
        // in it, the old locked crop stared at the table.
        let s = samples(
            (0..36)
                .map(|i| {
                    if i < 18 {
                        vec![face(0.3, 0.12)]
                    } else {
                        vec![]
                    }
                })
                .collect(),
        );
        let keys = plan_shots(&s, &[3000], dur(36), &[], SRC);
        assert_eq!(keys.len(), 2);
        assert!(!keys[0].pad);
        assert_eq!(keys[1], CropKey::pad(3000));
    }

    #[test]
    fn a_detector_that_misses_most_frames_still_frames_the_person() {
        // Profile turns drop detections; 40% presence is plenty.
        let s = samples(
            (0..30)
                .map(|i| {
                    if i % 5 < 2 {
                        vec![face(0.6, 0.12)]
                    } else {
                        vec![]
                    }
                })
                .collect(),
        );
        let keys = plan_shots(&s, &[], dur(30), &[], SRC);
        assert_eq!(keys.len(), 1);
        assert!(!keys[0].pad);
    }

    #[test]
    fn weak_one_off_detections_are_ignored() {
        let mut faint = face(0.6, 0.12);
        faint.score = 3.0;
        let s = samples(vec![vec![faint]; 30]);
        assert_eq!(
            plan_shots(&s, &[], dur(30), &[], SRC),
            vec![CropKey::pad(0)]
        );
    }

    #[test]
    fn background_faces_do_not_compete_for_the_view() {
        let s = samples(vec![vec![face(0.3, 0.14), face(0.8, 0.04)]; 30]);
        let people = shot_people(&s.faces, &(0..30).collect::<Vec<_>>());
        assert_eq!(people.len(), 1);
        assert!((people[0].face.cx - 0.3).abs() < 1e-4);
    }

    #[test]
    fn people_are_ordered_largest_first() {
        let s = samples(vec![vec![face(0.3, 0.10), face(0.7, 0.14)]; 30]);
        let people = shot_people(&s.faces, &(0..30).collect::<Vec<_>>());
        assert_eq!(people.len(), 2);
        assert!((people[0].face.cx - 0.7).abs() < 1e-4);
    }

    fn activity(n: usize, who: impl Fn(u64) -> usize) -> Vec<(u64, Vec<f32>)> {
        (1..n)
            .map(|i| {
                let t = frame_ms(i);
                let mut v = vec![1.0, 1.0];
                v[who(t)] = 6.0;
                (t, v)
            })
            .collect()
    }

    #[test]
    fn a_sustained_turn_cuts_to_the_new_speaker() {
        // Person 1 talks for 5 s, then person 0 for 5 s, words throughout.
        let speech: Vec<(u64, u64)> = (0..20).map(|k| (k * 500 + 20, k * 500 + 400)).collect();
        let act = activity(60, |t| if t < 5000 { 1 } else { 0 });
        let turns = speaker_turns(&act, &both_seen(10_000), 0, 10_000, &speech);
        assert_eq!(turns.len(), 2, "{turns:?}");
        assert_eq!(turns[0], (0, 1));
        assert_eq!(turns[1].1, 0);
        assert!(turns[1].0.abs_diff(5000) <= 500, "{turns:?}");
    }

    #[test]
    fn a_one_second_interjection_does_not_cut() {
        let speech: Vec<(u64, u64)> = (0..20).map(|k| (k * 500 + 20, k * 500 + 400)).collect();
        let act = activity(60, |t| if (4000..5000).contains(&t) { 1 } else { 0 });
        let turns = speaker_turns(&act, &both_seen(10_000), 0, 10_000, &speech);
        assert_eq!(turns, vec![(0, 0)]);
    }

    #[test]
    fn silent_bins_hold_the_largest_face() {
        // No words: mouth motion is chewing or noise, not a turn.
        let act = activity(60, |_| 1);
        assert_eq!(
            speaker_turns(&act, &both_seen(10_000), 0, 10_000, &[]),
            vec![(0, 0)]
        );
    }

    fn both_seen(dur: u64) -> Vec<Vec<u64>> {
        let all: Vec<u64> = (0..).map(frame_ms).take_while(|&t| t < dur).collect();
        vec![all.clone(), all]
    }

    #[test]
    fn the_view_follows_who_is_on_screen_across_an_unflagged_cut() {
        // A dissolve scdet misses: the larger face (0) only appears from
        // 6.1 s; before that only person 1 is in frame. No speech.
        let times: Vec<u64> = (0..60).map(frame_ms).collect();
        let seen = vec![
            times.iter().copied().filter(|&t| t >= 6100).collect(),
            times.iter().copied().filter(|&t| t < 6000).collect(),
        ];
        let act = activity(60, |_| 0);
        let turns = speaker_turns(&act, &seen, 0, 10_000, &[]);
        assert_eq!(turns.len(), 2, "{turns:?}");
        assert_eq!(turns[0], (0, 1));
        assert_eq!(turns[1].1, 0);
        assert!(turns[1].0.abs_diff(6_000) <= 170, "{turns:?}");
    }

    #[test]
    fn turn_changes_snap_to_the_word_after_a_pause() {
        let speech = vec![(0, 3800), (4200, 4500), (4550, 6000)];
        assert_eq!(snap_to_word(4000, 0, 10_000, &speech), 4200);
        // Nothing nearby: keep the bin edge.
        assert_eq!(snap_to_word(8000, 0, 10_000, &speech), 8000);
    }

    #[test]
    fn a_close_up_keeps_the_full_height_window() {
        // Face box already 23% of the frame height: zooming would crop it.
        let k = frame_face(&face(0.5, 0.13), 0, SRC);
        assert_eq!(k.zoom, 1.0);
    }

    #[test]
    fn a_wide_shot_tightens_toward_head_and_shoulders() {
        let k = frame_face(&face(0.5, 0.07), 0, SRC);
        // 0.07 * 16/9 = 0.124 of the height → 0.20 / 0.124 = 1.61, capped.
        assert!((k.zoom - MAX_ZOOM).abs() < 1e-4, "zoom={}", k.zoom);
        let k = frame_face(&face(0.5, 0.10), 0, SRC);
        assert!((k.zoom - 1.125).abs() < 0.01, "zoom={}", k.zoom);
    }

    #[test]
    fn low_resolution_sources_never_zoom() {
        let k = frame_face(&face(0.5, 0.05), 0, (640, 360));
        assert_eq!(k.zoom, 1.0);
    }

    #[test]
    fn a_zoomed_window_puts_the_eyes_on_the_eye_line() {
        let f = face(0.5, 0.08);
        let k = frame_face(&f, 0, SRC);
        let win_h = 1.0 / k.zoom;
        let eye = f.cy - EYE_FACE_OFFSET * f.h;
        let top = k.cy - win_h / 2.0;
        assert!(((eye - top) / win_h - EYE_LINE).abs() < 1e-3);
    }

    #[test]
    fn views_stay_inside_the_frame() {
        for cx in [0.0, 0.02, 0.98, 1.0] {
            for cy in [0.0, 0.95] {
                let mut f = face(cx, 0.06);
                f.cy = cy;
                let k = frame_face(&f, 0, SRC);
                let win_h = 1.0 / k.zoom;
                let win_w = win_h * 1080.0 * 9.0 / 16.0 / 1920.0;
                assert!(k.cx - win_w / 2.0 >= -1e-5 && k.cx + win_w / 2.0 <= 1.0 + 1e-5);
                assert!(k.cy - win_h / 2.0 >= -1e-5 && k.cy + win_h / 2.0 <= 1.0 + 1e-5);
            }
        }
    }

    #[test]
    fn cuts_near_edges_or_each_other_collapse() {
        assert_eq!(
            clean_cuts(vec![5000, 100, 5100, 9900, 3000], 10_000),
            vec![3000, 5000]
        );
    }

    #[test]
    fn repeated_views_merge() {
        let keys = vec![
            CropKey::crop(0, 0.5, 0.5, 1.0),
            CropKey::crop(2000, 0.51, 0.5, 1.0),
            CropKey::pad(4000),
            CropKey::pad(5000),
            CropKey::crop(6000, 0.5, 0.5, 1.0),
        ];
        assert_eq!(
            merge_keys(keys),
            vec![
                CropKey::crop(0, 0.5, 0.5, 1.0),
                CropKey::pad(4000),
                CropKey::crop(6000, 0.5, 0.5, 1.0),
            ]
        );
    }

    #[test]
    fn mouth_motion_ignores_whole_head_movement() {
        let (w, h) = (40, 40);
        let f = FaceDet {
            cx: 0.5,
            cy: 0.5,
            w: 0.5,
            h: 0.5,
            score: 20.0,
        };
        let still = vec![100u8; w * h];
        // Talking: only the mouth band changes.
        let mut talking = still.clone();
        for y in 25..34 {
            for x in 15..25 {
                talking[y * w + x] = 140;
            }
        }
        // Nodding: the whole face changes.
        let nodding = vec![140u8; w * h];
        let s = Samples {
            faces: vec![vec![f]; 3],
            small: vec![still.clone(), talking, nodding],
            small_w: w,
            small_h: h,
        };
        let people = vec![Person {
            face: f,
            dets: vec![],
        }];
        let act = mouth_activity(&s, &[0, 1], &people);
        assert!(act[0].1[0] > 20.0, "{act:?}");
        let act = mouth_activity(&s, &[0, 2], &people);
        assert!(act[0].1[0] < 1.0, "{act:?}");
    }

    #[test]
    fn half_res_averages_two_by_two_blocks() {
        let frame = [0, 4, 8, 8, 4, 0, 8, 8];
        assert_eq!(half_res(&frame, 4, 2), vec![2, 8]);
    }

    #[test]
    fn face_detection_checks_cancellation_before_blocking_work() {
        let cancelled = AtomicBool::new(true);
        let result = sample_and_detect(
            "ffmpeg",
            Path::new("missing.mp4"),
            Path::new("missing-model"),
            0,
            1000,
            (64, 36),
            &cancelled,
        );
        assert!(crate::util::is_cancelled(&result.err().unwrap()));
    }
}
