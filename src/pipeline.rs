//! Pipeline orchestrator (PRD §7.2, §12, §14.2).
//!
//! Runs the stage sequence, persists every transition to project.json,
//! broadcasts SSE events, honors cancellation (killing the active subprocess),
//! and supports stage/clip-level retry by skipping work whose artifacts
//! already exist on disk.

use crate::captions::{accent_bgr_for, build_ass, CaptionInput, CaptionStyle};
use crate::domain::*;
use crate::state::{AppState, ProjectHandle};
use crate::util::{promote_atomic, slugify, unique_temp_path};
use crate::validate::interval_confidence;
use anyhow::Result;
use chrono::Utc;
use futures::FutureExt;
use serde_json::json;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
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

struct Ctx {
    state: AppState,
    handle: Arc<ProjectHandle>,
    cancel: CancellationToken,
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

    async fn begin(&self, p: &mut Project, stage: &str) -> Result<()> {
        p.status = JobState::from_stage(stage);
        let rec = p.stage_mut(stage);
        rec.started_at = Some(Utc::now());
        rec.completed_at = None;
        rec.error = None;
        rec.progress = Some(0.0);
        self.state.store.save_project(p).await?;
        self.handle.set_live(stage, 0.0, None);
        self.emit_stage(p, stage, "running");
        Ok(())
    }

    async fn complete(&self, p: &mut Project, stage: &str, detail: String) -> Result<()> {
        let rec = p.stage_mut(stage);
        rec.completed_at = Some(Utc::now());
        rec.progress = Some(1.0);
        rec.detail = Some(detail);
        self.state.store.save_project(p).await?;
        self.handle.clear_live();
        self.emit_stage(p, stage, "done");
        Ok(())
    }

    async fn skip(&self, p: &mut Project, stage: &str, detail: &str) -> Result<()> {
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

    /// Throttled live-progress reporter for a stage.
    fn progress_fn(&self, stage: &'static str) -> impl FnMut(f32, Option<String>) + '_ {
        let handle = self.handle.clone();
        let mut last = Instant::now() - Duration::from_secs(10);
        let mut last_pct = -1.0f32;
        move |pct: f32, detail: Option<String>| {
            handle.set_live(stage, pct, detail.clone());
            if last.elapsed() >= Duration::from_millis(400) && (pct - last_pct).abs() >= 0.01 {
                last = Instant::now();
                last_pct = pct;
                handle.emit(json!({
                    "type": "progress",
                    "stage": stage,
                    "progress": pct,
                    "detail": detail,
                }));
            }
        }
    }
}

fn is_cancelled(e: &anyhow::Error, token: &CancellationToken) -> bool {
    token.is_cancelled() || e.to_string().contains("cancelled")
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
    let ctx = Ctx {
        state: state.clone(),
        handle,
        cancel: cancel.clone(),
    };
    let store = &state.store;
    let cfg = &state.cfg;
    let mut p = store.load_project(&id).await?;
    let src = store.source_path(&id);

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
                Ok(info) => {
                    let detail = format!(
                        "{}×{} · {} · {}/{}",
                        info.width,
                        info.height,
                        fmt_ms(info.duration_ms),
                        info.video_codec,
                        info.audio_codec
                    );
                    p.source = Some(info);
                    Ok(detail)
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
        let mut prog = ctx.progress_fn("transcribing");
        stage!("transcribing", {
            match crate::transcribe::transcribe(cfg, &wav, &ctx.cancel, |pct| prog(pct, None)).await
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
                    Ok(format!(
                        "{} words · avg confidence {:.0}%",
                        t.words.len(),
                        t.avg_confidence * 100.0
                    ))
                }
                Err(e) => Err(e),
            }
        });
    }
    let transcript = store.load_transcript(&id).await?;

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
            let energy = store.load_energy(&id).await;
            stage!("selecting_candidates", {
                let proposed = tokio::select! {
                    biased;
                    _ = ctx.cancel.cancelled() => Err(anyhow::anyhow!("cancelled")),
                    result = crate::select::propose(
                        &settings,
                        &transcript,
                        &source,
                        energy.as_ref(),
                    ) => result,
                };
                match proposed {
                    Ok(outcome) => {
                        store.save_raw_candidates(&id, &outcome.candidates).await?;
                        p.selector = Some(outcome.selector.clone());
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
            let report = crate::validate::validate(raw, &transcript, source.duration_ms, selector);
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
            let mut clips: Vec<ClipRecord> = Vec::new();
            let total = report.accepted.len();
            let mut result: anyhow::Result<String> = Ok(String::new());
            for (i, vc) in report.accepted.iter().enumerate() {
                prog(
                    i as f32 / total as f32,
                    Some(format!("Analyzing framing for clip {} of {}", i + 1, total)),
                );
                let frames_dir = store.frames_dir(&id);
                let analyzed_layout = match crate::frame::analyze_layout(
                    cfg,
                    &src,
                    &source,
                    vc.candidate.start_ms,
                    vc.candidate.end_ms,
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
                    layout,
                    status: ClipStatus::Pending,
                    error: None,
                    low_confidence: interval_confidence(&transcript, c.start_ms, c.end_ms)
                        < LOW_CONFIDENCE,
                    caption_style: None,
                    accent_color: None,
                    caption_font: None,
                    caption_text: Some(words_to_text(&crate::captions::words_in_interval(
                        &transcript.words,
                        c.start_ms,
                        c.end_ms,
                    ))),
                    audio_qc: None,
                });
            }
            match result {
                Ok(_) => {
                    let face_crops = clips
                        .iter()
                        .filter(|c| matches!(c.layout, LayoutPlan::FaceCrop { .. }))
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
    let output_dir = state
        .cfg
        .output_root
        .join(slugify(source.filename.trim_end_matches(".mp4"), 60));
    let total = manifest.clips.len();
    tokio::fs::create_dir_all(store.base_dir(&id)).await?;

    for i in 0..manifest.clips.len() {
        if ctx.cancel.is_cancelled() {
            ctx.mark_cancelled(&mut p, "rendering").await?;
            return Ok(());
        }
        let clip = manifest.clips[i].clone();
        let out_path = store.clips_dir(&id).join(&clip.filename);
        if clip.status == ClipStatus::Ready
            && store.final_is_ready(&id, &clip.id, &clip.filename).await?
        {
            continue;
        }

        manifest.clips[i].status = ClipStatus::Rendering;
        store.save_manifest(&id, &manifest).await?;
        ctx.handle
            .emit(json!({"type": "clip", "clip": manifest.clips[i]}));
        let mut prog = ctx.progress_fn("rendering");

        let done_label = format!("Rendering clip {} of {}", i + 1, total);
        let caption_label = format!("Burning captions for clip {} of {}", i + 1, total);
        let base_path = store.base_clip_path(&id, &clip.id);
        let ass_path: PathBuf =
            unique_temp_path(&store.clips_dir(&id).join(format!("{}.ass", clip.id)));
        let base_temp = unique_temp_path(&base_path);
        let out_temp = unique_temp_path(&out_path);
        let base_ready = store.base_is_ready(&id, &clip.id).await?;
        // Measure the source segment first so the base render can apply a
        // linear two-pass loudnorm correction. Advisory: a failed measurement
        // renders unnormalized rather than failing the clip.
        let loudness = if base_ready {
            None
        } else {
            match crate::audio::measure(
                cfg,
                &src,
                Some(clip.start_ms),
                Some(clip.end_ms - clip.start_ms),
                &ctx.cancel,
            )
            .await
            {
                Ok(m) => Some(m),
                // Advisory: a failed measurement renders unnormalized rather
                // than failing the clip. Cancellation is caught by the check
                // at the top of the next loop iteration.
                Err(e) => {
                    tracing::warn!("loudness measurement failed, rendering unnormalized: {e:#}");
                    None
                }
            }
        };
        let audio_filter = loudness.as_ref().map(crate::audio::loudnorm_filter);

        let render_result: anyhow::Result<()> = async {
            // Pass 1 — framed, uncaptioned base. Kept on disk so captions can
            // be restyled later without re-doing the expensive framing work
            // (and reused as-is when retrying a failed caption burn).
            if !base_ready {
                tokio::fs::remove_file(&base_path).await.ok();
                store.clear_base_ready(&id, &clip.id).await;
                crate::render::render_base_clip(
                    cfg,
                    &src,
                    &source,
                    &clip.layout,
                    clip.start_ms,
                    clip.end_ms,
                    audio_filter.as_deref(),
                    &base_temp,
                    &ctx.cancel,
                    |pct| prog(pct * 0.85, Some(done_label.clone())),
                )
                .await?;
                if ctx.cancel.is_cancelled() {
                    return Err(anyhow::anyhow!("cancelled"));
                }
                promote_atomic(&base_temp, &base_path).await?;
                store.mark_base_ready(&id, &clip.id).await?;
            }
            // Pass 2 — word-accurate captions burned onto the base.
            let words = crate::captions::with_caption_text(
                &crate::captions::words_in_interval(&transcript.words, clip.start_ms, clip.end_ms),
                clip.caption_text.as_deref(),
            );
            let ass = build_ass(
                &CaptionInput {
                    words: &words,
                    clip_start_ms: clip.start_ms,
                    clip_end_ms: clip.end_ms,
                    headline: &clip.headline,
                    font: &cfg.caption_font,
                    accent_bgr: accent_bgr.clone(),
                },
                caption_style,
            );
            tokio::fs::write(&ass_path, &ass).await?;
            if ctx.cancel.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled"));
            }
            let burn = crate::render::burn_captions(
                cfg,
                &base_path,
                &ass_path,
                &out_temp,
                clip.end_ms.saturating_sub(clip.start_ms),
                &ctx.cancel,
                |pct| prog(0.85 + pct * 0.15, Some(caption_label.clone())),
            )
            .await;
            burn?;
            if ctx.cancel.is_cancelled() {
                return Err(anyhow::anyhow!("cancelled"));
            }
            promote_atomic(&out_temp, &out_path).await?;
            store.mark_final_ready(&id, &clip.id).await?;
            // QC: measure what actually shipped. Never fails the clip.
            if let Ok(final_qc) =
                crate::audio::measure(cfg, &out_path, None, None, &ctx.cancel).await
            {
                if let Some(source_qc) = &loudness {
                    manifest.clips[i].audio_qc = Some(AudioQc {
                        target_i: crate::audio::TARGET_I,
                        source_i: source_qc.input_i,
                        source_tp: source_qc.input_tp,
                        output_i: final_qc.input_i,
                        output_tp: final_qc.input_tp,
                    });
                }
            }
            Ok(())
        }
        .await;

        tokio::fs::remove_file(&base_temp).await.ok();
        tokio::fs::remove_file(&out_temp).await.ok();
        tokio::fs::remove_file(&ass_path).await.ok();

        match render_result {
            Ok(()) => {
                manifest.clips[i].status = ClipStatus::Ready;
                manifest.clips[i].error = None;
                manifest.clips[i].caption_style = Some(caption_style.label().to_string());
                manifest.clips[i].accent_color = Some(accent_hex.clone());
                manifest.clips[i].caption_font = Some(cfg.caption_font.clone());
                // Copy into the user-facing output folder (best-effort).
                if tokio::fs::create_dir_all(&output_dir).await.is_ok() {
                    let dest = output_dir.join(&clip.filename);
                    if tokio::fs::copy(&out_path, &dest).await.is_ok() {
                        manifest.output_dir = Some(output_dir.to_string_lossy().into_owned());
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
                    store.clear_base_ready(&id, &clip.id).await;
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
                    store.clear_base_ready(&id, &clip.id).await;
                }
                tokio::fs::remove_file(&out_path).await.ok();
                store.clear_final_ready(&id, &clip.id).await;
                manifest.clips[i].status = ClipStatus::Failed;
                manifest.clips[i].error = Some(e.to_string());
                store.save_manifest(&id, &manifest).await?;
                ctx.handle
                    .emit(json!({"type": "clip", "clip": manifest.clips[i]}));
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
}
