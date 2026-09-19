//! Pipeline orchestrator (PRD §7.2, §12, §14.2).
//!
//! Runs the stage sequence, persists every transition to project.json,
//! broadcasts SSE events, honors cancellation (killing the active subprocess),
//! and supports stage/clip-level retry by skipping work whose artifacts
//! already exist on disk.

use crate::captions::{accent_bgr_for, build_ass, CaptionInput, CaptionStyle};
use crate::domain::*;
use crate::state::{AppState, LiveStage, ProjectHandle};
use crate::util::{promote_atomic, slugify, unique_temp_path};
use crate::validate::interval_confidence;
use anyhow::Result;
use chrono::Utc;
use futures::FutureExt;
use serde_json::json;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const LOW_CONFIDENCE: f32 = 0.66;
const CANCEL_ACK_TIMEOUT: Duration = Duration::from_secs(5);
const SHORT_VIDEO_MS: u64 = 20_000;

pub fn is_caption_only(duration_ms: u64) -> bool {
    duration_ms < SHORT_VIDEO_MS
}

/// Start (or resume) processing for a project. Returns an error string if a
/// run is already active.
pub async fn start(state: AppState, id: String) -> Result<(), String> {
    let handle = state.handle(&id);
    let _operation = handle.operation.lock().await;
    start_locked(state, id, handle.clone())
}

fn start_locked(state: AppState, id: String, handle: Arc<ProjectHandle>) -> Result<(), String> {
    let lease = handle
        .try_start()
        .ok_or_else(|| "Processing is already running for this project.".to_string())?;
    let token = lease.token;
    let generation = lease.generation;
    handle.clear_live();

    tokio::spawn(async move {
        let hid = id.clone();
        let result = AssertUnwindSafe(run(state.clone(), id, handle.clone(), token))
            .catch_unwind()
            .await;
        let failure = match result {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(_) => Some(anyhow::anyhow!("pipeline task panicked")),
        };
        if let Some(e) = failure {
            tracing::error!(project = %hid, "pipeline task error: {e:#}");
            let message = format!("Processing failed unexpectedly. {e}");
            if let Ok(mut project) = state.store.load_project(&hid).await {
                project.status = JobState::Failed;
                project.error = Some(message.clone());
                if let Some(stage) = project
                    .stages
                    .iter_mut()
                    .find(|stage| stage.started_at.is_some() && stage.completed_at.is_none())
                {
                    stage.error = Some(message.clone());
                }
                state.store.save_project(&project).await.ok();
            }
            handle.emit(json!({"type": "done", "status": "failed"}));
        }
        handle.finish(generation);
        handle.clear_live();
    });
    Ok(())
}

pub enum CancelOutcome {
    Cancelled,
    Status(JobState),
}

async fn wait_for_cancel_ack(
    done: &mut watch::Receiver<bool>,
    timeout: Duration,
) -> Result<(), String> {
    let wait = async {
        while !*done.borrow() {
            done.changed().await.map_err(|_| {
                "Processing run ended before cancellation was acknowledged.".to_string()
            })?;
        }
        Ok::<(), String>(())
    };
    tokio::time::timeout(timeout, wait)
        .await
        .map_err(|_| {
            format!(
                "Cancellation did not reach a terminal state within {} ms; no cancellation acknowledgement was issued.",
                timeout.as_millis()
            )
        })??;
    Ok(())
}

pub async fn cancel(state: &AppState, id: &str) -> Result<CancelOutcome, String> {
    let handle = state.handle(id);
    let mut done = {
        let _operation = handle.operation.lock().await;
        handle.request_cancel()
    };
    if let Some(done) = done.as_mut() {
        wait_for_cancel_ack(done, CANCEL_ACK_TIMEOUT).await?;
    }

    let project = state
        .store
        .load_project(id)
        .await
        .map_err(|e| e.to_string())?;
    if project.status == JobState::Cancelled {
        Ok(CancelOutcome::Cancelled)
    } else {
        Ok(CancelOutcome::Status(project.status))
    }
}

/// Reset failure markers so a new run resumes from persisted artifacts, then start.
pub async fn retry(state: AppState, id: String) -> Result<(), String> {
    let handle = state.handle(&id);
    let _operation = handle.operation.lock().await;
    if handle.is_running() {
        return Err("Processing is already running for this project.".into());
    }
    state
        .store
        .cleanup_partial_files(&id)
        .await
        .map_err(|e| e.to_string())?;

    let mut p = state
        .store
        .load_project(&id)
        .await
        .map_err(|e| e.to_string())?;
    for s in &mut p.stages {
        if s.error.is_some() || s.detail.as_deref() == Some("Cancelled") {
            s.error = None;
            s.started_at = None;
            s.completed_at = None;
            s.progress = None;
            s.detail = None;
        }
    }
    p.error = None;
    p.status = JobState::Created;
    if let Ok(mut manifest) = state.store.load_manifest(&id).await {
        let mut changed = false;
        for c in &mut manifest.clips {
            if c.status == ClipStatus::Failed || c.status == ClipStatus::Rendering {
                c.status = ClipStatus::Pending;
                c.error = None;
                changed = true;
            }
        }
        if changed {
            state.store.save_manifest(&id, &manifest).await.ok();
        }
    }
    state
        .store
        .save_project(&p)
        .await
        .map_err(|e| e.to_string())?;
    start_locked(state, id, handle.clone())
}

// ---------------------------------------------------------------------------
// Honest progress model
// ---------------------------------------------------------------------------

/// Stage-cost calibration: seconds of work per second of source (or per
/// second of rendered output for layout/render), from observed stage runtimes
/// on a mid-range CPU. These set the *weights* of the overall bar; the per
/// stage fraction is always measured, never modelled.
const INSPECT_PER_SRC_S: f64 = 0.02; // scdet scan ≈ 50× realtime
const EXTRACT_PER_SRC_S: f64 = 0.012; // audio decode ≈ 80× realtime
const TRANSCRIBE_PER_SRC_S: f64 = 0.35; // whisper.cpp base ≈ 3× realtime
const DIARIZE_PER_SRC_S: f64 = 0.12; // speaker embeddings ≈ 8× realtime
const ANALYZE_PER_OUT_S: f64 = 0.8; // frame sampling + face detect
const RENDER_PER_OUT_S: f64 = 1.9; // base encode ≈1.1× + caption burn ≈0.8×
const RENDER_PER_CLIP_S: f64 = 3.0; // cut/zoom planning + export pack + copies
const SELECT_REMOTE_BASE_S: f64 = 4.0; // one LLM roundtrip…
const SELECT_REMOTE_PER_WINDOW_S: f64 = 8.0; // …per 12-min transcript window
const SELECT_WINDOW_S: f64 = 12.0 * 60.0;
const SELECT_LOCAL_S: f64 = 1.5;
const VALIDATE_S: f64 = 0.4;
/// Base-encode share of one clip's render cost (1.1× realtime out of the
/// 1.9× total — the caption burn is the cheaper second pass).
const RENDER_BASE_SHARE: f64 = 1.1 / 1.9;

/// Per-run progress model for the overall bar. Each stage's weight is an
/// estimated cost in seconds derived from the real inputs: source duration
/// for inspect/extract/transcribe/speaker analysis, expected output duration
/// for layout and render. A stage that is skipped or already complete on a
/// resumed run carries zero weight — the bar measures only the work this run
/// actually performs, so zero-clip projects and retries never fake progress.
struct ProgressModel {
    /// Resolved seconds-per-stage: 0.0 once a stage proves free this run.
    weights: HashMap<&'static str, f64>,
    current: Option<&'static str>,
    /// Measured fraction of the current stage (monotonic within the stage).
    fraction: f32,
    started: Option<Instant>,
    source_ms: Option<u64>,
    caption_only: bool,
    /// Expected rendered output across all clips (ms) — selection estimate
    /// first, real manifest durations once known.
    expected_output_ms: Option<u64>,
    clip_count: usize,
    diarize_planned: bool,
    remote_selection: bool,
}

impl ProgressModel {
    fn new(p: &Project) -> ProgressModel {
        let source_ms = p.source.as_ref().map(|s| s.duration_ms);
        ProgressModel {
            weights: HashMap::new(),
            current: None,
            fraction: 0.0,
            started: None,
            source_ms,
            caption_only: source_ms.map(is_caption_only).unwrap_or(false),
            expected_output_ms: None,
            clip_count: 0,
            diarize_planned: false,
            remote_selection: false,
        }
    }

    fn set_source(&mut self, source: &SourceInfo) {
        self.source_ms = Some(source.duration_ms);
        self.caption_only = is_caption_only(source.duration_ms);
        // The stage in flight when the duration first lands (inspect on a
        // fresh upload) gets its weight re-resolved against the real input.
        if let Some(cur) = self.current {
            let w = self.estimate(cur);
            self.weights.insert(cur, w);
        }
    }

    fn set_expected_output(&mut self, output_ms: u64, clips: usize) {
        self.expected_output_ms = Some(output_ms.max(1));
        self.clip_count = clips;
    }

    /// Seconds of rendered output the layout/render stages will work through.
    /// Before selection resolves, assume ~5% of the source becomes clips
    /// (a few 30–60s moments), capped at 5 minutes.
    fn expected_output_s(&self) -> f64 {
        if let Some(ms) = self.expected_output_ms {
            return ms as f64 / 1000.0;
        }
        let src_s = self.source_ms.unwrap_or(0) as f64 / 1000.0;
        if self.caption_only {
            src_s
        } else {
            (src_s * 0.05).clamp(30.0, 300.0)
        }
    }

    /// Diarization's share of the layout stage's own fraction: the
    /// embedding pass runs inside `analyzing_layout`, so its modelled cost
    /// divides that stage's progress between "who speaks when" and
    /// per-clip framing analysis.
    fn diarize_share(&self) -> f32 {
        if !self.diarize_planned {
            return 0.0;
        }
        let src_s = self.source_ms.unwrap_or(0) as f64 / 1000.0;
        let d = src_s * DIARIZE_PER_SRC_S;
        let a = self.expected_output_s() * ANALYZE_PER_OUT_S;
        (d / (d + a).max(f64::EPSILON)) as f32
    }

    /// Estimated seconds for a stage under this run's real inputs.
    fn estimate(&self, stage: &str) -> f64 {
        let src_s = self.source_ms.unwrap_or(0) as f64 / 1000.0;
        let out_s = self.expected_output_s();
        match stage {
            "inspecting" => 1.0 + src_s * INSPECT_PER_SRC_S,
            "extracting_audio" => src_s * EXTRACT_PER_SRC_S,
            "transcribing" => src_s * TRANSCRIBE_PER_SRC_S,
            "selecting_candidates" => {
                if self.caption_only {
                    0.1
                } else if self.remote_selection {
                    SELECT_REMOTE_BASE_S
                        + (src_s / SELECT_WINDOW_S).ceil() * SELECT_REMOTE_PER_WINDOW_S
                } else {
                    SELECT_LOCAL_S
                }
            }
            "validating_candidates" => {
                if self.caption_only {
                    0.05
                } else {
                    VALIDATE_S
                }
            }
            "analyzing_layout" => {
                out_s * ANALYZE_PER_OUT_S
                    + if self.diarize_planned {
                        src_s * DIARIZE_PER_SRC_S
                    } else {
                        0.0
                    }
            }
            "rendering" => {
                out_s * RENDER_PER_OUT_S + self.clip_count.max(1) as f64 * RENDER_PER_CLIP_S
            }
            _ => 1.0,
        }
    }

    /// Mark a stage as costing this run nothing (skipped or already done).
    fn mark_free(&mut self, stage: &'static str) {
        self.weights.insert(stage, 0.0);
    }

    fn begin(&mut self, stage: &'static str) -> LiveStage {
        let w = self.estimate(stage);
        self.weights.insert(stage, w);
        self.current = Some(stage);
        self.fraction = 0.0;
        self.started = Some(Instant::now());
        self.live(None)
    }

    fn finish(&mut self, stage: &str) {
        if self.current == Some(stage) {
            self.fraction = 1.0;
        }
    }

    /// Update the current stage's measured fraction and snapshot the model.
    fn report(&mut self, stage: &'static str, pct: f32, detail: Option<String>) -> LiveStage {
        if self.current != Some(stage) {
            let w = self.estimate(stage);
            self.weights.insert(stage, w);
            self.current = Some(stage);
            self.started = Some(Instant::now());
            self.fraction = 0.0;
        }
        // Stage fractions are cumulative by construction; the max keeps a
        // stray out-of-order sample from dragging the bar backwards.
        self.fraction = self.fraction.max(pct.clamp(0.0, 1.0));
        self.live(detail)
    }

    /// (overall fraction 0–1, estimated seconds of work after the current
    /// stage). `None` until the source duration is known — before inspection
    /// finishes there is nothing honest to scale weights by.
    fn overall(&self) -> Option<(f32, f64)> {
        self.source_ms?;
        let cur = self
            .current
            .and_then(|c| STAGES.iter().position(|s| *s == c));
        let mut num = 0.0;
        let mut den = 0.0;
        let mut pending = 0.0;
        for (i, &s) in STAGES.iter().enumerate() {
            let past = cur.is_some_and(|c| i < c);
            let w = match self.weights.get(s) {
                Some(w) => *w,
                // A stage behind the current one that never ran is free.
                None if past => 0.0,
                None => self.estimate(s),
            };
            den += w;
            if Some(s) == self.current {
                num += w * f64::from(self.fraction);
            } else if past {
                num += w;
            } else {
                pending += w;
            }
        }
        if den <= 0.0 {
            return Some((1.0, 0.0));
        }
        Some((((num / den).min(1.0)) as f32, pending))
    }

    fn live(&self, detail: Option<String>) -> LiveStage {
        let (overall_progress, pending_ms) = match self.overall() {
            Some((o, p)) => (Some(o), Some((p * 1000.0).round() as u64)),
            None => (None, None),
        };
        LiveStage {
            stage: self.current.unwrap_or_default().to_string(),
            progress: self.fraction,
            detail,
            elapsed_ms: self
                .started
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0),
            stage_estimate_ms: self
                .current
                .and_then(|c| self.weights.get(c))
                .map(|w| (w * 1000.0).round() as u64),
            overall_progress,
            pending_ms,
        }
    }
}

/// A `progress` SSE event is the serialized LiveStage plus its event type —
/// additive only, so older clients keep working.
fn live_json(live: &LiveStage) -> serde_json::Value {
    let mut v = serde_json::to_value(live).unwrap_or_else(|_| json!({}));
    if let Some(m) = v.as_object_mut() {
        m.insert("type".into(), json!("progress"));
    }
    v
}

// ---------------------------------------------------------------------------

struct Ctx {
    state: AppState,
    handle: Arc<ProjectHandle>,
    cancel: CancellationToken,
    model: Arc<Mutex<ProgressModel>>,
}

impl Ctx {
    fn emit_stage(&self, p: &Project, stage: &str, status: &str) {
        let rec = p.stages.iter().find(|s| s.name == stage);
        self.handle.emit(json!({
            "type": "stage",
            "stage": stage,
            "status": status,
            "detail": rec.and_then(|r| r.detail.clone()),
            "error": rec.and_then(|r| r.error.clone()),
            "project_status": p.status,
        }));
    }

    async fn begin(&self, p: &mut Project, stage: &'static str) -> Result<()> {
        p.status = JobState::from_stage(stage);
        let rec = p.stage_mut(stage);
        rec.started_at = Some(Utc::now());
        rec.completed_at = None;
        rec.error = None;
        rec.progress = Some(0.0);
        self.state.store.save_project(p).await?;
        let live = self.model.lock().unwrap().begin(stage);
        self.handle.set_live(live);
        self.emit_stage(p, stage, "running");
        Ok(())
    }

    async fn complete(&self, p: &mut Project, stage: &'static str, detail: String) -> Result<()> {
        let rec = p.stage_mut(stage);
        rec.completed_at = Some(Utc::now());
        rec.progress = Some(1.0);
        rec.detail = Some(detail);
        self.state.store.save_project(p).await?;
        self.model.lock().unwrap().finish(stage);
        self.handle.clear_live();
        self.emit_stage(p, stage, "done");
        Ok(())
    }

    async fn skip(&self, p: &mut Project, stage: &'static str, detail: &str) -> Result<()> {
        self.model.lock().unwrap().mark_free(stage);
        let rec = p.stage_mut(stage);
        if rec.completed_at.is_none() {
            rec.started_at = Some(Utc::now());
            rec.completed_at = Some(Utc::now());
            rec.progress = Some(1.0);
            rec.detail = Some(detail.to_string());
            self.state.store.save_project(p).await?;
        }
        Ok(())
    }

    async fn fail(&self, p: &mut Project, stage: &str, msg: String) {
        p.status = JobState::Failed;
        p.error = Some(msg.clone());
        let rec = p.stage_mut(stage);
        rec.error = Some(msg);
        self.state.store.save_project(p).await.ok();
        self.handle.clear_live();
        self.emit_stage(p, stage, "failed");
        self.handle
            .emit(json!({"type": "done", "status": "failed"}));
    }

    async fn mark_cancelled(&self, p: &mut Project, stage: &str) -> Result<()> {
        p.status = JobState::Cancelled;
        p.error = None;
        let rec = p.stage_mut(stage);
        let now = Utc::now();
        if rec.started_at.is_none() {
            rec.started_at = Some(now);
        }
        rec.completed_at = Some(now);
        rec.progress = Some(rec.progress.unwrap_or(0.0));
        rec.detail = Some("Cancelled".into());
        rec.error = None;
        self.state.store.save_project(p).await?;
        self.handle.clear_live();
        self.emit_stage(p, stage, "cancelled");
        self.handle
            .emit(json!({"type": "done", "status": "cancelled"}));
        Ok(())
    }

    /// Throttled live-progress reporter for a stage. The fraction is the
    /// stage's measured share of its own work; the model attaches elapsed
    /// time, the stage's cost estimate, and the weighted overall figure.
    fn progress_fn(&self, stage: &'static str) -> impl FnMut(f32, Option<String>) + Send + 'static {
        let handle = self.handle.clone();
        let model = self.model.clone();
        let mut last = Instant::now() - Duration::from_secs(10);
        let mut last_pct = -1.0f32;
        move |pct: f32, detail: Option<String>| {
            let live = model.lock().unwrap().report(stage, pct, detail);
            handle.set_live(live.clone());
            if last.elapsed() >= Duration::from_millis(400)
                && (live.progress - last_pct).abs() >= 0.01
            {
                last = Instant::now();
                last_pct = live.progress;
                handle.emit(live_json(&live));
            }
        }
    }
}

fn is_cancelled(e: &anyhow::Error, token: &CancellationToken) -> bool {
    token.is_cancelled() || e.to_string().contains("cancelled")
}

/// Load the project's diarization, or produce it once. Advisory by design:
/// no speaker model → `Ok(None)`; a model/diarization failure logs and
/// also yields `None` — framing falls back to speaker-free layouts. Only a
/// cancellation propagates (as `Err`).
///
/// Needs 16 kHz mono PCM — the same `audio.wav` whisper consumed. That file
/// is deleted after transcription (PRD §13), so a resumed project
/// re-extracts it to a temp sibling and cleans up afterwards.
///
/// `on_progress` gets a 0–1 fraction of the diarization work: the audio
/// re-extract covers the first 10%, speaker embeddings the rest.
#[allow(clippy::too_many_arguments)]
async fn ensure_diarization<F>(
    cfg: &crate::config::Config,
    store: &crate::store::Store,
    id: &str,
    src: &std::path::Path,
    source: &SourceInfo,
    transcript: &Transcript,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<Option<Diarization>>
where
    F: FnMut(f32) + Send + 'static,
{
    /// Share of the diarization work the audio re-extract accounts for
    /// (embedding inference dominates).
    const EXTRACT_SHARE: f32 = 0.1;
    if cfg.speaker_model.is_none() {
        return Ok(None);
    }
    if let Some(d) = store.load_diarization(id).await {
        return Ok(Some(d));
    }

    let wav = store.audio_path(id);
    let mut temp_wav: Option<PathBuf> = None;
    let wav_path = if wav.is_file() {
        on_progress(EXTRACT_SHARE);
        wav
    } else {
        let tmp = unique_temp_path(&wav);
        if let Err(e) =
            crate::media::extract_audio(cfg, src, &tmp, source.duration_ms, cancel, |p| {
                on_progress(p * EXTRACT_SHARE)
            })
            .await
        {
            tokio::fs::remove_file(&tmp).await.ok();
            if is_cancelled(&e, cancel) {
                return Err(e);
            }
            tracing::warn!("diarization skipped: audio re-extract failed: {e:#}");
            return Ok(None);
        }
        temp_wav = Some(tmp.clone());
        tmp
    };

    let out = match crate::diarize::diarize(cfg, &wav_path, &transcript.words, cancel, move |p| {
        on_progress(EXTRACT_SHARE + p * (1.0 - EXTRACT_SHARE))
    })
    .await
    {
        Ok(Some(d)) => {
            if let Err(e) = store.save_diarization(id, &d).await {
                tracing::warn!("diarization not persisted: {e:#}");
            }
            Some(d)
        }
        Ok(None) => None,
        Err(e) => {
            if is_cancelled(&e, cancel) {
                if let Some(t) = &temp_wav {
                    tokio::fs::remove_file(t).await.ok();
                }
                return Err(e);
            }
            tracing::warn!("diarization failed; continuing without speakers: {e:#}");
            None
        }
    };
    if let Some(t) = temp_wav {
        tokio::fs::remove_file(t).await.ok();
    }
    Ok(out)
}

fn full_video_candidate(transcript: &Transcript, duration_ms: u64) -> Candidate {
    let caption = words_to_text(&transcript.words);
    Candidate {
        start_ms: 0,
        end_ms: duration_ms,
        headline: String::new(),
        opening_quote: caption.clone(),
        closing_quote: caption,
        selection_reason: "Full short video captioned without clipping.".into(),
        scores: Scores::default(),
    }
}

fn words_to_text(words: &[Word]) -> String {
    words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

async fn run(
    state: AppState,
    id: String,
    handle: Arc<ProjectHandle>,
    cancel: CancellationToken,
) -> Result<()> {
    let store = &state.store;
    let cfg = &state.cfg;
    let mut p = store.load_project(&id).await?;
    let src = store.source_path(&id);
    let ctx = Ctx {
        state: state.clone(),
        handle,
        cancel: cancel.clone(),
        model: Arc::new(Mutex::new(ProgressModel::new(&p))),
    };

    macro_rules! stage {
        ($name:literal, $body:expr) => {{
            if ctx.cancel.is_cancelled() {
                ctx.mark_cancelled(&mut p, $name).await?;
                return Ok(());
            }
            ctx.begin(&mut p, $name).await?;
            match $body {
                Ok(detail) => {
                    let detail: String = detail;
                    ctx.complete(&mut p, $name, detail).await?;
                }
                Err(e) => {
                    let e: anyhow::Error = e;
                    if is_cancelled(&e, &ctx.cancel) {
                        ctx.mark_cancelled(&mut p, $name).await?;
                    } else {
                        ctx.fail(&mut p, $name, e.to_string()).await;
                    }
                    return Ok(());
                }
            }
        }};
    }

    // ---- 1. Inspect -------------------------------------------------------
    if p.source.is_some() {
        ctx.skip(&mut p, "inspecting", "Already inspected").await?;
    } else {
        let original = tokio::fs::read_to_string(store.project_dir(&id).join("original-name.txt"))
            .await
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "source.mp4".into());
        stage!("inspecting", {
            match crate::media::probe(cfg, &src, &original, &ctx.cancel).await {
                Ok(mut info) => {
                    let detail = format!(
                        "{}×{} · {} · {}/{}",
                        info.width,
                        info.height,
                        fmt_ms(info.duration_ms),
                        info.video_codec,
                        info.audio_codec
                    );
                    // Scene detection is the slow half of inspection: the
                    // scdet scan reports real decode progress, so a 3-hour
                    // source shows a proportionally slower bar than a clip.
                    let mut prog = ctx.progress_fn("inspecting");
                    prog(0.05, Some("Scanning for scene boundaries".into()));
                    // Scene detection is advisory like the energy profile: a
                    // failed pass degrades to no boundaries — but a
                    // cancellation must still fail the stage.
                    match crate::media::scene_boundaries(
                        cfg,
                        &src,
                        info.duration_ms,
                        &ctx.cancel,
                        |pct| prog(0.05 + pct * 0.95, None),
                    )
                    .await
                    {
                        Ok(boundaries) => {
                            info.scene_boundaries_ms = boundaries;
                            ctx.model.lock().unwrap().set_source(&info);
                            p.source = Some(info);
                            Ok(detail)
                        }
                        Err(e) if is_cancelled(&e, &ctx.cancel) => Err(e),
                        Err(e) => {
                            tracing::warn!(
                                "scene detection failed; continuing without boundaries: {e:#}"
                            );
                            ctx.model.lock().unwrap().set_source(&info);
                            p.source = Some(info);
                            Ok(detail)
                        }
                    }
                }
                Err(e) => Err(e),
            }
        });
    }
    let source = p.source.clone().expect("source set after inspect");

    let transcript_exists = store.transcript_path(&id).is_file();

    // ---- 2. Extract audio -------------------------------------------------
    if transcript_exists {
        ctx.skip(&mut p, "extracting_audio", "Transcript already on disk")
            .await?;
    } else {
        let wav = store.audio_path(&id);
        // A previous run may have been killed after FFmpeg created the final
        // path. Always rebuild from a unique sibling and promote only after
        // FFmpeg has exited successfully.
        tokio::fs::remove_file(&wav).await.ok();
        let wav_temp = unique_temp_path(&wav);
        let mut prog = ctx.progress_fn("extracting_audio");
        stage!("extracting_audio", {
            let result = async {
                crate::media::extract_audio(
                    cfg,
                    &src,
                    &wav_temp,
                    source.duration_ms,
                    &ctx.cancel,
                    |pct| prog(pct, None),
                )
                .await?;
                let metadata = tokio::fs::metadata(&wav_temp).await?;
                if !metadata.is_file() || metadata.len() == 0 {
                    return Err(anyhow::anyhow!("Audio extraction produced an empty file."));
                }
                if ctx.cancel.is_cancelled() {
                    return Err(anyhow::anyhow!("cancelled"));
                }
                promote_atomic(&wav_temp, &wav).await?;
                Ok::<String, anyhow::Error>("16 kHz mono audio ready".to_string())
            }
            .await;
            if result.is_err() {
                tokio::fs::remove_file(&wav_temp).await.ok();
            }
            result
        });
    }

    // ---- 3. Transcribe ----------------------------------------------------
    if transcript_exists {
        ctx.skip(&mut p, "transcribing", "Transcript already on disk")
            .await?;
    } else {
        let wav = store.audio_path(&id);
        let language = p.language.clone();
        let mut prog = ctx.progress_fn("transcribing");
        stage!("transcribing", {
            match crate::transcribe::transcribe(
                cfg,
                &wav,
                language.as_deref(),
                &ctx.cancel,
                |pct| prog(pct, None),
            )
            .await
            {
                Ok(t) => {
                    store.save_transcript(&id, &t).await?;
                    // Energy profile is advisory: measure before the WAV is
                    // deleted (PRD §13), but never fail the stage on it.
                    if let Ok(profile) = crate::energy::measure(cfg, &wav, &ctx.cancel).await {
                        let _ = store.save_energy(&id, &profile).await;
                    }
                    // PRD §13: delete temporary audio after successful transcription.
                    tokio::fs::remove_file(&wav).await.ok();
                    if t.avg_confidence < 0.68 {
                        p.warning = Some(format!(
                            "Transcription confidence was low ({:.0}%). Captions may contain errors.",
                            t.avg_confidence * 100.0
                        ));
                    }
                    let lang = crate::transcribe::language_name(&t.language)
                        .unwrap_or(t.language.as_str());
                    Ok(format!(
                        "{} words · {} · avg confidence {:.0}%",
                        t.words.len(),
                        lang,
                        t.avg_confidence * 100.0
                    ))
                }
                Err(e) => Err(e),
            }
        });
    }
    let transcript = store.load_transcript(&id).await?;
    // A speaker model with no persisted diarization means the layout stage
    // will run embedding inference — fold its cost into the overall weights.
    ctx.model.lock().unwrap().diarize_planned =
        cfg.speaker_model.is_some() && store.load_diarization(&id).await.is_none();

    // ---- 4–5. Select and validate -----------------------------------------
    // Short videos are caption-only jobs: preserve the entire source instead
    // of sending it through editorial selection and minimum clip validation.
    if is_caption_only(source.duration_ms) {
        let candidate = full_video_candidate(&transcript, source.duration_ms);
        stage!("selecting_candidates", {
            store
                .save_raw_candidates(&id, &vec![candidate.clone()])
                .await?;
            p.selector = Some("caption-only".into());
            Ok::<String, anyhow::Error>("Short video · using the full duration".into())
        });
        stage!("validating_candidates", {
            let report = SelectionReport {
                selector: "caption-only".into(),
                accepted: vec![ValidatedCandidate {
                    candidate,
                    rank: 1,
                    composite: 0.0,
                    duration_exception: true,
                }],
                rejected: Vec::new(),
            };
            store.save_selection(&id, &report).await?;
            Ok::<String, anyhow::Error>("Full video accepted for captioning".into())
        });
    } else {
        if store.raw_candidates_path(&id).is_file() {
            ctx.skip(&mut p, "selecting_candidates", "Proposals already on disk")
                .await?;
        } else {
            let settings = state.settings.read().unwrap().clone();
            // Networked providers cost one LLM request per transcript
            // window; the offline/local heuristic is nearly free.
            ctx.model.lock().unwrap().remote_selection = settings.connected()
                && crate::settings::Provider::parse(&settings.provider)
                    .is_some_and(|pr| pr != crate::settings::Provider::Offline);
            let energy = store.load_energy(&id).await;
            stage!("selecting_candidates", {
                let mut prog = ctx.progress_fn("selecting_candidates");
                let proposed = tokio::select! {
                    biased;
                    _ = ctx.cancel.cancelled() => Err(anyhow::anyhow!("cancelled")),
                    result = crate::select::propose(
                        &settings,
                        &transcript,
                        &source,
                        energy.as_ref(),
                        p.focus_prompt.as_deref(),
                        |pct| prog(pct, None),
                    ) => result,
                };
                match proposed {
                    Ok(outcome) => {
                        store.save_raw_candidates(&id, &outcome.candidates).await?;
                        p.selector = Some(outcome.selector.clone());
                        if let Some(warning) = outcome.warning {
                            p.warning = Some(match p.warning.take() {
                                Some(existing) => format!("{existing} {warning}"),
                                None => warning,
                            });
                        }
                        Ok(format!(
                            "{} proposal(s) from {}",
                            outcome.candidates.len(),
                            outcome.selector
                        ))
                    }
                    Err(e) => Err(e),
                }
            });
        }

        let raw = store.load_raw_candidates(&id).await?;
        let selector = p.selector.clone().unwrap_or_else(|| "unknown".into());
        stage!("validating_candidates", {
            let report = crate::validate::validate(
                raw,
                &transcript,
                source.duration_ms,
                selector,
                &source.scene_boundaries_ms,
            );
            let detail = format!(
                "{} passed · {} rejected",
                report.accepted.len(),
                report.rejected.len()
            );
            store.save_selection(&id, &report).await?;
            Ok::<String, anyhow::Error>(detail)
        });
    }
    let report = store.load_selection(&id).await?;

    // Selection is resolved: the accepted durations are the model's best
    // output estimate until the render manifest replaces them.
    let accepted_ms: u64 = report
        .accepted
        .iter()
        .map(|vc| vc.candidate.end_ms.saturating_sub(vc.candidate.start_ms))
        .sum();
    ctx.model
        .lock()
        .unwrap()
        .set_expected_output(accepted_ms, report.accepted.len());

    // No passing moments is a valid, honest outcome (PRD §6.2, §8.3).
    if report.accepted.is_empty() {
        if ctx.cancel.is_cancelled() {
            ctx.mark_cancelled(&mut p, "validating_candidates").await?;
            return Ok(());
        }
        ctx.skip(
            &mut p,
            "analyzing_layout",
            "No moments passed the quality bar",
        )
        .await?;
        ctx.skip(&mut p, "rendering", "Nothing to render").await?;
        store.save_manifest(&id, &RenderManifest::default()).await?;
        p.status = JobState::Complete;
        store.save_project(&p).await?;
        ctx.handle
            .emit(json!({"type": "done", "status": "complete"}));
        return Ok(());
    }

    // ---- 6. Analyze framing -------------------------------------------------
    let existing = store.load_manifest(&id).await.ok();
    let manifest_matches = existing
        .as_ref()
        .map(|m| m.clips.len() == report.accepted.len())
        .unwrap_or(false);
    if manifest_matches {
        ctx.skip(&mut p, "analyzing_layout", "Layouts already planned")
            .await?;
    } else {
        let mut prog = ctx.progress_fn("analyzing_layout");
        stage!("analyzing_layout", {
            // Speaker diarization is per-project and advisory: it runs once
            // here (the only stage that consumes it), persists to
            // speakers.json, and any absence simply means speaker-free
            // layouts and unlabeled captions. Its share of the stage's own
            // fraction matches its modelled cost share — on a long source
            // the embedding pass is most of this stage.
            let diar_share = ctx.model.lock().unwrap().diarize_share();
            let mut diar_prog = ctx.progress_fn("analyzing_layout");
            let diarization = ensure_diarization(
                cfg,
                store,
                &id,
                &src,
                &source,
                &transcript,
                &ctx.cancel,
                move |p| {
                    diar_prog(
                        p * diar_share,
                        Some("Identifying speakers from audio".into()),
                    )
                },
            )
            .await?;
            let mut clips: Vec<ClipRecord> = Vec::new();
            let total = report.accepted.len();
            let total_ms: u64 = report
                .accepted
                .iter()
                .map(|vc| vc.candidate.end_ms.saturating_sub(vc.candidate.start_ms))
                .sum();
            // Frame sampling + face detection cost scales with clip length,
            // so a clip's share of the stage is its duration — not 1/N.
            let mut analyzed_ms: u64 = 0;
            let mut result: anyhow::Result<String> = Ok(String::new());
            for (i, vc) in report.accepted.iter().enumerate() {
                let clip_frac =
                    diar_share + (analyzed_ms as f32 / total_ms.max(1) as f32) * (1.0 - diar_share);
                prog(
                    clip_frac,
                    Some(format!("Analyzing framing for clip {} of {}", i + 1, total)),
                );
                analyzed_ms += vc.candidate.end_ms.saturating_sub(vc.candidate.start_ms);
                let frames_dir = store.frames_dir(&id);
                let analyzed_layout = match crate::frame::analyze_layout(
                    cfg,
                    &src,
                    &source,
                    vc.candidate.start_ms,
                    vc.candidate.end_ms,
                    diarization.as_ref(),
                    &frames_dir,
                    &ctx.cancel,
                )
                .await
                {
                    Ok(l) => l,
                    Err(e) if is_cancelled(&e, &ctx.cancel) => {
                        result = Err(e);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("framing analysis failed, using blur-pad: {e:#}");
                        LayoutPlan::BlurPad
                    }
                };
                let layout = p.framing_mode.apply(analyzed_layout);
                let (out_w, out_h) = crate::render::output_size(&source, &layout);
                let c = &vc.candidate;
                clips.push(ClipRecord {
                    id: crate::util::short_id(),
                    rank: vc.rank,
                    headline: c.headline.clone(),
                    filename: format!("{:02}-{}.mp4", vc.rank, slugify(&c.headline, 48)),
                    start_ms: c.start_ms,
                    end_ms: c.end_ms,
                    duration_ms: c.end_ms - c.start_ms,
                    selection_reason: c.selection_reason.clone(),
                    scores: c.scores,
                    // Caption-only jobs run no ranking, so their synthetic
                    // composite (0.0) is not a score worth showing.
                    score: (!is_caption_only(source.duration_ms)).then_some(vc.composite),
                    layout,
                    width: Some(out_w),
                    height: Some(out_h),
                    status: ClipStatus::Pending,
                    error: None,
                    low_confidence: interval_confidence(&transcript, c.start_ms, c.end_ms)
                        < LOW_CONFIDENCE,
                    caption_style: None,
                    accent_color: None,
                    caption_font: None,
                    emoji_overlay: None,
                    caption_text: Some(words_to_text(&crate::captions::words_in_interval(
                        &transcript.words,
                        c.start_ms,
                        c.end_ms,
                    ))),
                    auto_cut: false,
                    cut_spans: None,
                    zoom_cuts: false,
                    zoom_keys: None,
                    end_card: false,
                    progress_bar: false,
                    hook_title: false,
                });
            }
            match result {
                Ok(_) => {
                    let face_crops = clips
                        .iter()
                        .filter(|c| {
                            matches!(
                                c.layout,
                                LayoutPlan::FaceCrop { .. }
                                    | LayoutPlan::Split { .. }
                                    | LayoutPlan::SpeakerCrop { .. }
                            )
                        })
                        .count();
                    store
                        .save_manifest(
                            &id,
                            &RenderManifest {
                                clips,
                                output_dir: None,
                            },
                        )
                        .await?;
                    Ok(format!(
                        "{} layout(s) planned · {} face-tracked",
                        total, face_crops
                    ))
                }
                Err(e) => Err(e),
            }
        });
    }

    // ---- 7. Render (sequential, incremental, per-clip isolation) ------------
    let mut manifest = store.load_manifest(&id).await?;
    // The manifest's real clip durations replace the selection estimate —
    // they drive both the render stage's overall weight and its per-clip
    // shares below.
    let manifest_ms: u64 = manifest
        .clips
        .iter()
        .map(|c| c.end_ms.saturating_sub(c.start_ms))
        .sum();
    ctx.model
        .lock()
        .unwrap()
        .set_expected_output(manifest_ms, manifest.clips.len());
    // Speaker labels for captions/SRT ride on the same diarization the
    // layout pass produced; it may not exist — that's fine.
    let diarization = store.load_diarization(&id).await;
    if ctx.cancel.is_cancelled() {
        ctx.mark_cancelled(&mut p, "rendering").await?;
        return Ok(());
    }
    ctx.begin(&mut p, "rendering").await?;
    match crate::util::ffmpeg_has_ass_cancellable(&cfg.ffmpeg, &ctx.cancel).await {
        Ok(true) => {}
        Ok(false) => {
            ctx.fail(
                &mut p,
                "rendering",
                "This FFmpeg build cannot burn captions because the ASS filter is missing. On macOS, install `ffmpeg-full` with Homebrew and restart.".into(),
            )
            .await;
            return Ok(());
        }
        Err(e) if is_cancelled(&e, &ctx.cancel) => {
            ctx.mark_cancelled(&mut p, "rendering").await?;
            return Ok(());
        }
        Err(e) => {
            ctx.fail(&mut p, "rendering", e.to_string()).await;
            return Ok(());
        }
    }
    let caption_style =
        CaptionStyle::from_str(p.caption_style.as_deref().unwrap_or(&cfg.caption_style));
    let accent_hex = p
        .accent_color
        .clone()
        .unwrap_or_else(|| crate::captions::default_accent_hex(caption_style).to_string());
    let accent_bgr = accent_bgr_for(caption_style, Some(&accent_hex));
    let emoji_overlay = p.emoji_overlay.unwrap_or(false);
    let output_dir = state
        .cfg
        .output_root
        .join(slugify(source.filename.trim_end_matches(".mp4"), 60));
    let settings = state.settings.read().unwrap().clone();
    let total = manifest.clips.len();
    tokio::fs::create_dir_all(store.base_dir(&id)).await?;

    // Stage fraction = cumulative share of clip work done. A clip's share
    // is its output duration (frames written is the cost driver), so a
    // 90 s clip moves the bar three times further than a 30 s one — and on
    // resume, clips already on disk count as done from the start.
    let clip_weight = |c: &ClipRecord| -> f64 {
        c.end_ms.saturating_sub(c.start_ms).max(1) as f64
            + crate::render::end_card_ms(cfg, c.end_card) as f64
    };
    let total_w: f64 = manifest
        .clips
        .iter()
        .map(&clip_weight)
        .sum::<f64>()
        .max(1.0);
    let mut done_w: f64 = 0.0;

    for i in 0..manifest.clips.len() {
        if ctx.cancel.is_cancelled() {
            ctx.mark_cancelled(&mut p, "rendering").await?;
            return Ok(());
        }
        let mut clip = manifest.clips[i].clone();
        let w_i = clip_weight(&clip);
        let mut prog = ctx.progress_fn("rendering");
        let out_path = store.clips_dir(&id).join(&clip.filename);
        if clip.status == ClipStatus::Ready
            && store.final_is_ready(&id, &clip.id, &clip.filename).await?
        {
            done_w += w_i;
            prog(
                (done_w / total_w) as f32,
                Some(format!("Clip {} of {} already rendered", i + 1, total)),
            );
            // Clips rendered before export packs existed get their sidecars
            // backfilled on resume — no re-render needed.
            let words = crate::export::caption_words(&transcript, &clip);
            let input = crate::export::MetaInput {
                clip: &clip,
                source: &source,
                words: &words,
                project_id: &id,
                selector: p.selector.as_deref(),
                caption_style: clip.caption_style.as_deref(),
            };
            if let Err(e) = crate::export::ensure_export_pack(
                &store.clips_dir(&id),
                &input,
                &settings,
                &ctx.cancel,
            )
            .await
            {
                tracing::warn!(clip = %clip.id, "export pack backfill failed: {e:#}");
            }
            continue;
        }

        // Auto-cut (opt-in): compute the cut list once and persist it on the
        // clip so restyle/retry reproduce the identical cut without
        // re-running silence detection.
        if clip.auto_cut && clip.cut_spans.is_none() {
            let energy = store.load_energy(&id).await;
            match crate::autocut::plan_for_clip(
                cfg,
                &src,
                &transcript.words,
                clip.start_ms,
                clip.end_ms,
                energy.as_ref(),
                &ctx.cancel,
            )
            .await
            {
                Ok(removals) => {
                    clip.cut_spans = Some(removals);
                    manifest.clips[i].cut_spans = clip.cut_spans.clone();
                    store.save_manifest(&id, &manifest).await?;
                }
                Err(e) if is_cancelled(&e, &ctx.cancel) => {
                    ctx.mark_cancelled(&mut p, "rendering").await?;
                    return Ok(());
                }
                Err(e) => {
                    manifest.clips[i].status = ClipStatus::Failed;
                    manifest.clips[i].error = Some(e.to_string());
                    store.save_manifest(&id, &manifest).await?;
                    ctx.handle
                        .emit(json!({"type": "clip", "clip": manifest.clips[i]}));
                    done_w += w_i;
                    prog(
                        (done_w / total_w) as f32,
                        Some(format!("Clip {} of {} failed", i + 1, total)),
                    );
                    continue;
                }
            }
        }
        let removals = clip.effective_removals();
        let keeps = crate::autocut::keeps_from_removals(clip.start_ms, clip.end_ms, removals);
        let out_dur_ms = keeps.iter().map(|k| k.len_ms()).sum::<u64>();
        // The end card lengthens the file but not the caption timeline —
        // keep it out of zoom planning, count it in progress and duration.
        let card_ms = crate::render::end_card_ms(cfg, clip.end_card);
        // Zoom cuts (opt-in): plan the keyframes once and persist them on
        // the clip so restyle/retry reproduce the identical zoom without
        // re-running beat detection. Beats land on the post-cut timeline,
        // so this runs after auto-cut's removals are known.
        if clip.zoom_cuts && clip.zoom_keys.is_none() {
            let energy = store.load_energy(&id).await;
            let words = crate::captions::with_caption_text(
                &crate::captions::words_in_interval(&transcript.words, clip.start_ms, clip.end_ms),
                clip.caption_text.as_deref(),
            );
            let keys = crate::zoom::plan(
                clip.start_ms,
                clip.end_ms,
                &words,
                energy.as_ref(),
                removals,
                out_dur_ms,
            );
            clip.zoom_keys = Some(keys);
            manifest.clips[i].zoom_keys = clip.zoom_keys.clone();
            store.save_manifest(&id, &manifest).await?;
        }
        let base_key = clip.base_key();

        manifest.clips[i].status = ClipStatus::Rendering;
        store.save_manifest(&id, &manifest).await?;
        ctx.handle
            .emit(json!({"type": "clip", "clip": manifest.clips[i]}));

        let done_label = format!("Rendering clip {} of {}", i + 1, total);
        let caption_label = format!("Burning captions for clip {} of {}", i + 1, total);
        let base_path = store.base_clip_path(&id, &base_key);
        let ass_path: PathBuf =
            unique_temp_path(&store.clips_dir(&id).join(format!("{}.ass", clip.id)));
        let base_temp = unique_temp_path(&base_path);
        let out_temp = unique_temp_path(&out_path);
        let base_ready = store.base_is_ready(&id, &base_key).await?;
        // Captions are authored against the base clip's real size: a fresh
        // base renders at output_size, while a pre-ADR-0002 base already on
        // disk is fixed 1080×1920 (manifests then carry no dims).
        let (out_w, out_h) = if base_ready {
            (
                clip.width.unwrap_or(crate::render::OUT_W),
                clip.height.unwrap_or(crate::render::OUT_H),
            )
        } else {
            crate::render::output_size(&source, &clip.layout)
        };

        let render_result: anyhow::Result<()> = async {
            // Pass 1 — framed, uncaptioned base. Kept on disk so captions can
            // be restyled later without re-doing the expensive framing work
            // (and reused as-is when retrying a failed caption burn).
            if !base_ready {
                tokio::fs::remove_file(&base_path).await.ok();
                store.clear_base_ready(&id, &base_key).await;
                crate::render::render_base_clip(
                    cfg,
                    &src,
                    &source,
                    &clip.layout,
                    clip.start_ms,
                    clip.end_ms,
                    &keeps,
                    clip.effective_zoom_keys(),
                    clip.end_card,
                    clip.progress_bar.then_some(accent_hex.as_str()),
                    clip.hook_title.then_some(crate::render::HookSpec {
                        headline: &clip.headline,
                        font: cfg.caption_font.as_str(),
                        face: caption_style.face(&cfg.caption_font),
                        caps: caption_style.uses_caps(),
                    }),
                    &base_temp,
                    &ctx.cancel,
                    |pct| {
                        prog(
                            ((done_w + w_i * pct as f64 * RENDER_BASE_SHARE) / total_w) as f32,
                            Some(done_label.clone()),
                        )
                    },
                )
                .await?;
                if ctx.cancel.is_cancelled() {
                    return Err(anyhow::anyhow!("cancelled"));
                }
                promote_atomic(&base_temp, &base_path).await?;
                store.mark_base_ready(&id, &base_key).await?;
            }
            // Pass 2 — word-accurate captions burned onto the base. With
            // auto-cut the words move onto the output timeline (dropped
            // fillers vanish from captions too); with it off this is the
            // same interval text as before.
            let words = crate::autocut::retime_words(
                &crate::export::caption_words(&transcript, &clip),
                clip.effective_removals(),
            );
            // Speaker turns move onto the output timeline alongside the
            // words so a caption tag and a speaker-crop cut agree.
            let clip_diar = diarization.as_ref().map(|d| Diarization {
                labels: d.labels.clone(),
                turns: crate::autocut::retime_turns(&d.turns, clip.effective_removals()),
            });
            let caption_input = CaptionInput {
                words: &words,
                clip_start_ms: clip.start_ms,
                clip_end_ms: clip.start_ms + out_dur_ms,
                headline: &clip.headline,
                font: &cfg.caption_font,
                accent_bgr: accent_bgr.clone(),
                emoji_overlay,
                out_w,
                out_h,
                diarization: clip_diar.as_ref(),
            };
            let ass = build_ass(&caption_input, caption_style);
            tokio::fs::write(&ass_path, &ass).await?;
            // Speaker-labeled subtitle sidecar beside the clip.
            let srt = crate::captions::build_srt(&caption_input);
            tokio::fs::write(out_path.with_extension("srt"), srt)
                .await
                .ok();
            if ctx.cancel.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled"));
            }
            let burn = crate::render::burn_captions(
                cfg,
                &base_path,
                &ass_path,
                &out_temp,
                out_dur_ms + card_ms,
                &ctx.cancel,
                |pct| {
                    prog(
                        ((done_w
                            + w_i * (RENDER_BASE_SHARE + pct as f64 * (1.0 - RENDER_BASE_SHARE)))
                            / total_w) as f32,
                        Some(caption_label.clone()),
                    )
                },
            )
            .await;
            burn?;
            if ctx.cancel.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled"));
            }
            // The export pack lands before the clip is marked ready — a
            // Ready clip always carries its .srt/.vtt/.meta.json sidecars.
            crate::export::write_export_pack(
                &store.clips_dir(&id),
                &crate::export::MetaInput {
                    clip: &clip,
                    source: &source,
                    words: &words,
                    project_id: &id,
                    selector: p.selector.as_deref(),
                    caption_style: Some(caption_style.label()),
                },
                &settings,
                &ctx.cancel,
            )
            .await?;
            promote_atomic(&out_temp, &out_path).await?;
            // Poster frame beside the export pack: a shareable still pulled
            // ~1s in so it carries the hook title when that's on. Best-effort —
            // a poster failure never un-marks a rendered clip.
            let poster = store
                .clips_dir(&id)
                .join(crate::export::poster_name(&clip.filename));
            let _ = crate::util::run_streaming(
                &cfg.ffmpeg,
                &[
                    "-y".into(),
                    "-ss".into(),
                    "0.9".into(),
                    "-i".into(),
                    out_path.to_string_lossy().into_owned(),
                    "-frames:v".into(),
                    "1".into(),
                    "-q:v".into(),
                    "3".into(),
                    poster.to_string_lossy().into_owned(),
                ],
                &ctx.cancel,
                |_, _| {},
            )
            .await;
            store.mark_final_ready(&id, &clip.id).await?;
            Ok(())
        }
        .await;

        tokio::fs::remove_file(&base_temp).await.ok();
        tokio::fs::remove_file(&out_temp).await.ok();
        tokio::fs::remove_file(&ass_path).await.ok();

        match render_result {
            Ok(()) => {
                done_w += w_i;
                prog((done_w / total_w) as f32, Some(done_label.clone()));
                manifest.clips[i].status = ClipStatus::Ready;
                manifest.clips[i].error = None;
                manifest.clips[i].caption_style = Some(caption_style.label().to_string());
                manifest.clips[i].accent_color = Some(accent_hex.clone());
                manifest.clips[i].caption_font = Some(cfg.caption_font.clone());
                manifest.clips[i].emoji_overlay = Some(emoji_overlay);
                manifest.clips[i].width = Some(out_w);
                manifest.clips[i].height = Some(out_h);
                // Auto-cut shortens the clip — the manifest reports the
                // rendered length, not the source interval.
                manifest.clips[i].duration_ms = out_dur_ms + card_ms;
                // Copy into the user-facing output folder (best-effort).
                if tokio::fs::create_dir_all(&output_dir).await.is_ok() {
                    let dest = output_dir.join(&clip.filename);
                    if tokio::fs::copy(&out_path, &dest).await.is_ok() {
                        manifest.output_dir = Some(output_dir.to_string_lossy().into_owned());
                        let srt = out_path.with_extension("srt");
                        if srt.is_file() {
                            tokio::fs::copy(&srt, dest.with_extension("srt")).await.ok();
                        }
                    }
                    // The export pack travels with the MP4 — same best-effort copy.
                    for name in [
                        crate::export::srt_name(&clip.filename),
                        crate::export::vtt_name(&clip.filename),
                        crate::export::meta_name(&clip.filename),
                        crate::export::poster_name(&clip.filename),
                    ] {
                        let sidecar = store.clips_dir(&id).join(&name);
                        tokio::fs::copy(&sidecar, output_dir.join(&name)).await.ok();
                    }
                }
                store.save_manifest(&id, &manifest).await?;
                ctx.handle
                    .emit(json!({"type": "clip", "clip": manifest.clips[i]}));
                if ctx.cancel.is_cancelled() {
                    ctx.mark_cancelled(&mut p, "rendering").await?;
                    return Ok(());
                }
            }
            Err(e) if is_cancelled(&e, &ctx.cancel) => {
                tokio::fs::remove_file(&out_path).await.ok();
                store.clear_final_ready(&id, &clip.id).await;
                if !base_ready {
                    tokio::fs::remove_file(&base_path).await.ok();
                    store.clear_base_ready(&id, &base_key).await;
                }
                manifest.clips[i].status = ClipStatus::Pending;
                store.save_manifest(&id, &manifest).await?;
                ctx.mark_cancelled(&mut p, "rendering").await?;
                return Ok(());
            }
            Err(e) => {
                // A failed render must not discard successful outputs (PRD §12).
                // Drop this clip's base so retry rebuilds it from scratch — a
                // truncated base from a crash would poison every re-burn.
                if !base_ready {
                    tokio::fs::remove_file(&base_path).await.ok();
                    store.clear_base_ready(&id, &base_key).await;
                }
                tokio::fs::remove_file(&out_path).await.ok();
                store.clear_final_ready(&id, &clip.id).await;
                manifest.clips[i].status = ClipStatus::Failed;
                manifest.clips[i].error = Some(e.to_string());
                store.save_manifest(&id, &manifest).await?;
                ctx.handle
                    .emit(json!({"type": "clip", "clip": manifest.clips[i]}));
                done_w += w_i;
                prog(
                    (done_w / total_w) as f32,
                    Some(format!("Clip {} of {} failed", i + 1, total)),
                );
            }
        }
    }

    let ready = manifest
        .clips
        .iter()
        .filter(|c| c.status == ClipStatus::Ready)
        .count();
    let failed = manifest
        .clips
        .iter()
        .filter(|c| c.status == ClipStatus::Failed)
        .count();

    if ctx.cancel.is_cancelled() {
        ctx.mark_cancelled(&mut p, "rendering").await?;
        return Ok(());
    }

    if ready > 0 {
        ctx.complete(
            &mut p,
            "rendering",
            format!("{} of {} clip(s) rendered", ready, total),
        )
        .await?;
        if failed > 0 {
            p.warning = Some(format!(
                "{} clip(s) failed to render. Use Retry to re-run just the failed clip(s).",
                failed
            ));
        }
        p.status = JobState::Complete;
        p.error = None;
        store.save_project(&p).await?;
        ctx.handle
            .emit(json!({"type": "done", "status": "complete"}));
    } else {
        ctx.fail(
            &mut p,
            "rendering",
            "All clip renders failed. Review the clip errors and retry.".into(),
        )
        .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn cancellation_persists_terminal_stage_metadata() {
        let tmp = std::env::temp_dir().join(format!("cf-cancel-{}", crate::util::short_id()));
        let mut cfg = Config::resolve();
        cfg.data_dir = tmp.join("data");
        cfg.output_root = tmp.join("output");
        let state = AppState::new(cfg);
        let id = "cancel-test".to_string();

        state.store.create_dirs(&id).await.unwrap();
        state
            .store
            .save_project(&Project::new(id.clone(), state.store.source_path(&id)))
            .await
            .unwrap();

        let token = CancellationToken::new();
        token.cancel();
        run(state.clone(), id.clone(), state.handle(&id), token)
            .await
            .unwrap();

        let mut project = state.store.load_project(&id).await.unwrap();
        assert_eq!(project.status, JobState::Cancelled);
        let stage = project.stage_mut("inspecting");
        assert!(stage.started_at.is_some());
        assert!(stage.completed_at.is_some());
        assert_eq!(stage.detail.as_deref(), Some("Cancelled"));

        tokio::fs::remove_dir_all(tmp).await.ok();
    }

    #[tokio::test]
    async fn cancellation_ack_timeout_returns_an_error_instead_of_hanging() {
        let (_sender, mut done) = watch::channel(false);
        let result = wait_for_cancel_ack(&mut done, Duration::from_millis(10)).await;
        let message = result.unwrap_err();
        assert!(message.contains("no cancellation acknowledgement"));
    }

    #[test]
    fn short_video_candidate_preserves_the_full_source() {
        let transcript = Transcript {
            language: "en".into(),
            words: vec![Word {
                text: "hello".into(),
                start_ms: 250,
                end_ms: 900,
                p: 0.9,
            }],
            sentences: Vec::new(),
            avg_confidence: 0.9,
        };
        let candidate = full_video_candidate(&transcript, 12_345);
        assert_eq!(candidate.start_ms, 0);
        assert_eq!(candidate.end_ms, 12_345);
        assert!(candidate.headline.is_empty());
    }

    #[test]
    fn caption_only_threshold_excludes_exactly_twenty_seconds() {
        assert!(is_caption_only(19_999));
        assert!(!is_caption_only(20_000));
    }

    // ---- progress model ---------------------------------------------------

    fn model_with_source(source_ms: u64) -> ProgressModel {
        let mut m = ProgressModel::new(&Project::new("t".into(), PathBuf::new()));
        m.set_source(&SourceInfo {
            filename: "t.mp4".into(),
            duration_ms: source_ms,
            width: 1920,
            height: 1080,
            fps: 30.0,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            size_bytes: 1,
            scene_boundaries_ms: Vec::new(),
        });
        m
    }

    #[test]
    fn overall_is_unknown_until_the_source_is() {
        let mut m = ProgressModel::new(&Project::new("t".into(), PathBuf::new()));
        let live = m.begin("inspecting");
        // No duration yet — the UI must show no overall figure rather than
        // invent one.
        assert!(live.overall_progress.is_none());
        assert!(live.pending_ms.is_none());
        assert_eq!(live.stage, "inspecting");
    }

    #[test]
    fn overall_scales_with_source_duration() {
        // The same stage at the same measured fraction must carry a very
        // different estimate for a 10-minute source vs a 3-hour one —
        // that estimate is what the ETA falls back on while progress ≈ 0.
        let est_for = |ms: u64| {
            let mut m = model_with_source(ms);
            m.begin("transcribing");
            m.report("transcribing", 0.5, None)
                .stage_estimate_ms
                .unwrap()
        };
        let short = est_for(10 * 60_000);
        let long = est_for(3 * 3_600_000);
        assert_eq!(short, 210_000);
        assert_eq!(long, 3_780_000);
        // And the overall fractions differ in the expected direction: on the
        // long source transcribe outweighs the later stages more heavily.
        let frac_for = |ms: u64| {
            let mut m = model_with_source(ms);
            m.begin("transcribing");
            m.report("transcribing", 0.5, None);
            m.overall().unwrap().0
        };
        assert!(frac_for(3 * 3_600_000) > frac_for(10 * 60_000));
    }

    #[test]
    fn skipped_and_absent_stages_cost_nothing() {
        let mut m = model_with_source(60_000);
        m.set_expected_output(30_000, 1);
        m.begin("inspecting");
        m.finish("inspecting");
        m.mark_free("extracting_audio");
        m.mark_free("transcribing");
        m.mark_free("selecting_candidates");
        m.mark_free("validating_candidates");
        m.mark_free("analyzing_layout");
        m.begin("rendering");
        let live = m.report("rendering", 0.0, None);
        // With every earlier stage free, the overall bar is driven by
        // render alone — but it is not fabricated ahead of the fraction.
        let (o, pending) = m.overall().unwrap();
        assert!(o < 0.6, "overall {o} — render-only run at 0% of render");
        assert_eq!(pending, 0.0);
        assert_eq!(live.stage, "rendering");
    }

    #[test]
    fn report_never_moves_backwards() {
        let mut m = model_with_source(60_000);
        m.begin("extracting_audio");
        m.report("extracting_audio", 0.5, None);
        let live = m.report("extracting_audio", 0.2, None);
        assert_eq!(live.progress, 0.5);
    }

    #[test]
    fn elapsed_and_estimate_ride_on_the_stage() {
        let mut m = model_with_source(3 * 3_600_000);
        m.begin("transcribing");
        let live = m.report("transcribing", 0.1, None);
        // 3 h source → transcribe estimate ≈ 3780 s.
        let est = live.stage_estimate_ms.unwrap();
        assert!((est as f64 - 3_780_000.0).abs() < 1.0, "{est}");
        assert!(live.elapsed_ms < 1_000);
    }
}
