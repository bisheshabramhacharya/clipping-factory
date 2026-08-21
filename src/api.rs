//! HTTP API (PRD §14.1) + the studio's static assets.
//!
//! POST   /api/settings/ai            set provider/model/key
//! GET    /api/settings/ai            public status (never the key)
//! POST   /api/settings/ai/test       verify connectivity
//! GET    /api/setup                  first-run environment checks
//! POST   /api/projects               multipart MP4 upload → project (auto-starts)
//! GET    /api/projects/{id}          full project view
//! GET    /api/projects/{id}/events   SSE progress stream
//! POST   /api/projects/{id}/process  start/resume
//! POST   /api/projects/{id}/cancel   stop subprocesses, keep finished clips
//! POST   /api/projects/{id}/retry    re-run failed stage / failed clips only
//! GET    /api/projects/{id}/clips/{clipId}           inline MP4 (Range-aware)
//! GET    /api/projects/{id}/clips/{clipId}/download  attachment
//! POST   /api/projects/{id}/clips/{clipId}/restyle   re-burn captions (style/color)
//! GET    /api/projects/{id}/decisions                review triage map
//! PUT    /api/projects/{id}/decisions                replace the triage map
//! GET    /api/projects/{id}/export?verdicts=kept     ZIP of clips matching verdicts
//! POST   /api/projects/{id}/open-output-folder

use crate::domain::*;
use crate::pipeline;
use crate::settings::AiSettings;
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::{self, Stream, StreamExt};
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio_stream::wrappers::BroadcastStream;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_html))
        .route("/styles.css", get(styles_css))
        .route("/app.js", get(app_js))
        .route("/review.js", get(review_js))
        .route("/api/setup", get(setup_status))
        .route("/api/settings/ai", get(get_settings).post(set_settings))
        .route("/api/settings/ai/test", post(test_settings))
        .route("/api/projects", post(create_project))
        .route("/api/projects/{id}", get(get_project))
        .route("/api/projects/{id}/events", get(project_events))
        .route("/api/projects/{id}/process", post(process_project))
        .route("/api/projects/{id}/cancel", post(cancel_project))
        .route("/api/projects/{id}/retry", post(retry_project))
        .route("/api/projects/{id}/clips/{clip}", get(serve_clip_inline))
        .route(
            "/api/projects/{id}/clips/{clip}/download",
            get(serve_clip_download),
        )
        .route(
            "/api/projects/{id}/clips/{clip}/restyle",
            post(restyle_clip),
        )
        .route(
            "/api/projects/{id}/decisions",
            get(get_decisions).put(put_decisions),
        )
        .route("/api/projects/{id}/export", get(export_clips))
        .route(
            "/api/projects/{id}/open-output-folder",
            post(open_output_folder),
        )
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}
fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
fn not_found(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, msg.into())
}

// ---------------------------------------------------------------------------
// Static studio assets: prefer ./static on disk (dev), fall back to embedded.
// ---------------------------------------------------------------------------

macro_rules! static_asset {
    ($fn_name:ident, $file:literal, $ct:literal) => {
        async fn $fn_name() -> Response {
            let disk = std::path::Path::new("static").join($file);
            let body = tokio::fs::read_to_string(&disk)
                .await
                .unwrap_or_else(|_| include_str!(concat!("../static/", $file)).to_string());
            ([(header::CONTENT_TYPE, $ct)], body).into_response()
        }
    };
}
static_asset!(styles_css, "styles.css", "text/css; charset=utf-8");
static_asset!(app_js, "app.js", "application/javascript; charset=utf-8");
static_asset!(
    review_js,
    "review.js",
    "application/javascript; charset=utf-8"
);

async fn index_html() -> Html<String> {
    let disk = std::path::Path::new("static").join("index.html");
    Html(
        tokio::fs::read_to_string(&disk)
            .await
            .unwrap_or_else(|_| include_str!("../static/index.html").to_string()),
    )
}

// ---------------------------------------------------------------------------
// Setup & settings
// ---------------------------------------------------------------------------

async fn setup_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = &state.cfg;
    let ffmpeg_ok = crate::util::run_capture(&cfg.ffmpeg, &["-version".into()])
        .await
        .is_ok();
    let ffmpeg_ass = crate::util::ffmpeg_has_ass(&cfg.ffmpeg).await;
    let ffprobe_ok = crate::util::run_capture(&cfg.ffprobe, &["-version".into()])
        .await
        .is_ok();
    let model_size = cfg
        .whisper_model
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() / 1_000_000);
    let disk = crate::util::disk_free_gb(&cfg.data_dir).await;
    Json(json!({
        "ffmpeg": ffmpeg_ok,
        "ffmpeg_ass": ffmpeg_ass,
        "ffprobe": ffprobe_ok,
        "whisper_cli": cfg.whisper_bin.as_ref().map(|p| p.to_string_lossy()).unwrap_or_default(),
        "whisper_ok": cfg.whisper_bin.is_some(),
        "model_ok": cfg.whisper_model.is_some(),
        "model_mb": model_size,
        "face_model_ok": cfg.face_model.is_some(),
        "caption_font": cfg.caption_font,
        "caption_fonts": crate::captions::CAPTION_FONTS,
        "accent_palette": crate::accent::ACCENT_PALETTE,
        "disk_free_gb": disk,
        "data_dir": cfg.data_dir.to_string_lossy(),
        "output_root": cfg.output_root.to_string_lossy(),
    }))
}

async fn get_settings(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.settings.read().unwrap().public();
    Json(serde_json::to_value(s).unwrap_or_default())
}

#[derive(serde::Deserialize)]
struct SettingsIn {
    provider: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    api_key: String,
}

async fn set_settings(
    State(state): State<AppState>,
    Json(body): Json<SettingsIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !["openai", "anthropic", "offline"].contains(&body.provider.as_str()) {
        return Err(bad_request(
            "provider must be openai, anthropic, or offline",
        ));
    }
    let updated = {
        let mut candidate = state.settings.read().unwrap().clone();
        candidate.provider = body.provider;
        candidate.model = body.model;
        // Empty key = keep the existing one (lets users switch model without retyping).
        if !body.api_key.trim().is_empty() {
            candidate.api_key = Some(body.api_key.trim().to_string());
        }
        candidate
    };
    crate::select::test_connection(&updated)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    // `settings::save` touches the filesystem synchronously (write + chmod
    // 0600) — keep that work off the async workers.
    let data_dir = state.cfg.data_dir.clone();
    let to_save = updated.clone();
    tokio::task::spawn_blocking(move || crate::settings::save(&data_dir, &to_save))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    *state.settings.write().unwrap() = updated.clone();
    Ok(Json(
        serde_json::to_value(updated.public()).unwrap_or_default(),
    ))
}

async fn test_settings(State(state): State<AppState>) -> Json<serde_json::Value> {
    let settings: AiSettings = state.settings.read().unwrap().clone();
    match crate::select::test_connection(&settings).await {
        Ok(msg) => Json(json!({ "ok": true, "message": msg })),
        Err(e) => Json(json!({ "ok": false, "message": e.to_string() })),
    }
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

struct UploadFields {
    original_name: String,
    caption_style: Option<String>,
    accent_color: Option<String>,
    framing_mode: FramingMode,
}

const UPLOAD_DISK_RESERVE_BYTES: u64 = 1024 * 1024 * 1024;

fn upload_capacity_bytes(free_gb: Option<f64>) -> Option<u64> {
    free_gb.map(|gb| {
        let free_bytes = (gb.max(0.0) * 1024.0 * 1024.0 * 1024.0) as u64;
        free_bytes.saturating_sub(UPLOAD_DISK_RESERVE_BYTES)
    })
}

async fn cleanup_upload(state: &AppState, id: &str) {
    tokio::fs::remove_dir_all(state.store.project_dir(id))
        .await
        .ok();
}

struct UploadCleanupGuard {
    path: PathBuf,
    armed: bool,
}

impl UploadCleanupGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for UploadCleanupGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let path = self.path.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                tokio::fs::remove_dir_all(path).await.ok();
            });
        }
    }
}

async fn receive_upload(
    state: &AppState,
    id: &str,
    mut multipart: Multipart,
) -> Result<UploadFields, ApiError> {
    state.store.create_dirs(id).await.map_err(ApiError::from)?;
    let dest = state.store.source_path(id);
    let upload_capacity =
        upload_capacity_bytes(crate::util::disk_free_gb(&state.cfg.data_dir).await);

    let mut original_name = String::new();
    let mut wrote_bytes: u64 = 0;
    let mut caption_style: Option<String> = None;
    let mut accent_color: Option<String> = None;
    let mut accent_mode = crate::accent::AccentMode::default();
    let mut framing_mode = FramingMode::default();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(format!("upload error: {e}")))?
    {
        if field.name() == Some("caption_style") {
            if let Ok(v) = field.text().await {
                let v = v.trim().to_lowercase();
                if v == "impact" || v == "clean" {
                    caption_style = Some(v);
                }
            }
            continue;
        }
        if field.name() == Some("accent_color") {
            if let Ok(v) = field.text().await {
                let v = v.trim().to_string();
                if crate::captions::hex_to_ass_bgr(&v).is_some() {
                    accent_color = Some(if v.starts_with('#') {
                        v
                    } else {
                        format!("#{v}")
                    });
                }
            }
            continue;
        }
        if field.name() == Some("accent_mode") {
            if let Ok(v) = field.text().await {
                accent_mode = crate::accent::AccentMode::parse(&v).ok_or_else(|| {
                    bad_request("accent_mode must be manual, random, or optimized")
                })?;
            }
            continue;
        }
        if field.name() == Some("framing_mode") {
            if let Ok(v) = field.text().await {
                framing_mode = match v.trim() {
                    "background" => FramingMode::Background,
                    _ => FramingMode::Fill,
                };
            }
            continue;
        }
        if field.name() != Some("file") {
            continue;
        }
        original_name = field.file_name().unwrap_or("source.mp4").to_string();
        let lower = original_name.to_lowercase();
        if !(lower.ends_with(".mp4") || lower.ends_with(".m4v")) {
            return Err(bad_request(
                "Attach an .mp4 file. Other containers are post-MVP.",
            ));
        }
        // Stream to disk without buffering the whole video in memory (PRD §7.2).
        let upload_temp = crate::util::unique_temp_path(&dest);
        let mut file = tokio::fs::File::create(&upload_temp)
            .await
            .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| bad_request(format!("upload interrupted: {e}")))?
        {
            let next_size = wrote_bytes.saturating_add(chunk.len() as u64);
            if upload_capacity.is_some_and(|capacity| next_size > capacity) {
                return Err(bad_request(
                    "Not enough free disk space for this video. Free up space and try again.",
                ));
            }
            wrote_bytes = next_size;
            file.write_all(&chunk)
                .await
                .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
        }
        file.flush()
            .await
            .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
        crate::util::promote_atomic(&upload_temp, &dest)
            .await
            .map_err(ApiError::from)?;
    }

    if wrote_bytes == 0 {
        return Err(bad_request(
            "No file received. Drop one MP4 into the studio.",
        ));
    }

    accent_color = match accent_mode {
        crate::accent::AccentMode::Random => Some(crate::accent::random_accent().to_string()),
        crate::accent::AccentMode::Optimized => match crate::accent::optimized_accent_for_video(
            &state.cfg.ffmpeg,
            &state.cfg.ffprobe,
            &dest,
        )
        .await
        {
            Ok(color) => Some(color.to_string()),
            Err(error) => {
                tracing::warn!("video color optimization failed, using default accent: {error:#}");
                Some(
                    crate::captions::default_accent_hex(crate::captions::CaptionStyle::Impact)
                        .to_string(),
                )
            }
        },
        crate::accent::AccentMode::Manual => accent_color,
    };

    Ok(UploadFields {
        original_name,
        caption_style,
        accent_color,
        framing_mode,
    })
}

async fn create_project(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = crate::util::short_id();
    let mut cleanup = UploadCleanupGuard::new(state.store.project_dir(&id));
    let fields = match receive_upload(&state, &id, multipart).await {
        Ok(fields) => fields,
        Err(error) => {
            cleanup_upload(&state, &id).await;
            cleanup.disarm();
            return Err(error);
        }
    };

    let mut project = Project::new(id.clone(), state.store.source_path(&id));
    project.caption_style = fields.caption_style;
    project.accent_color = fields.accent_color;
    project.framing_mode = fields.framing_mode;
    if let Err(error) = state.store.save_project(&project).await {
        cleanup_upload(&state, &id).await;
        cleanup.disarm();
        return Err(ApiError::from(error));
    }
    cleanup.disarm();
    // The original filename lives in a sidecar file; the inspect stage and
    // output-folder naming read it from there.
    tokio::fs::write(
        state.store.project_dir(&id).join("original-name.txt"),
        &fields.original_name,
    )
    .await
    .ok();

    // Processing begins automatically (PRD §7.2).
    pipeline::start(state.clone(), id.clone()).await.ok();

    let view = project_view(&state, &id).await.map_err(ApiError::from)?;
    Ok(Json(view))
}

async fn get_project(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state.store.exists(&id) {
        return Err(not_found("Project not found."));
    }
    // Detect interrupted runs (server restarted mid-processing).
    let mut p = state
        .store
        .load_project(&id)
        .await
        .map_err(ApiError::from)?;
    let handle = state.handle(&id);
    if p.status.is_active() && !handle.is_running() {
        p.status = JobState::Failed;
        p.error = Some("Processing was interrupted (the server restarted). Retry to resume from the last completed stage.".into());
        state.store.save_project(&p).await.ok();
        // A hard interruption can also strand manifest clips in `rendering`;
        // reset them so the next run re-renders instead of skipping them.
        if let Ok(mut m) = state.store.load_manifest(&id).await {
            let mut changed = false;
            for c in &mut m.clips {
                if c.status == ClipStatus::Rendering {
                    c.status = ClipStatus::Pending;
                    changed = true;
                }
            }
            if changed {
                state.store.save_manifest(&id, &m).await.ok();
            }
        }
    }
    let view = project_view(&state, &id).await.map_err(ApiError::from)?;
    Ok(Json(view))
}

async fn project_view(state: &AppState, id: &str) -> anyhow::Result<serde_json::Value> {
    let p = state.store.load_project(id).await?;
    let handle = state.handle(id);
    let live = handle.live.lock().unwrap().clone();
    let selection = state.store.load_selection(id).await.ok();
    let manifest = state.store.load_manifest(id).await.ok();
    let original_name =
        tokio::fs::read_to_string(state.store.project_dir(id).join("original-name.txt"))
            .await
            .unwrap_or_default();

    let rejected_summary: Vec<serde_json::Value> = selection
        .as_ref()
        .map(|s| {
            s.rejected
                .iter()
                .take(12)
                .map(|r| {
                    json!({
                        "headline": r.candidate.headline,
                        "start_ms": r.candidate.start_ms,
                        "end_ms": r.candidate.end_ms,
                        "reasons": r.reasons,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "project": p,
        "original_name": original_name.trim(),
        "running": handle.is_running(),
        "live": live,
        "accepted": selection.as_ref().map(|s| s.accepted.len()).unwrap_or(0),
        "rejected": selection.as_ref().map(|s| s.rejected.len()).unwrap_or(0),
        "rejected_summary": rejected_summary,
        "selector": p.selector,
        "caption_only": p.source.as_ref().is_some_and(|source| pipeline::is_caption_only(source.duration_ms)),
        "clips": manifest.as_ref().map(|m| m.clips.clone()).unwrap_or_default(),
        "output_dir": manifest.as_ref().and_then(|m| m.output_dir.clone()),
    }))
}

async fn process_project(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state.store.exists(&id) {
        return Err(not_found("Project not found."));
    }
    match pipeline::start(state.clone(), id).await {
        Ok(()) => Ok(Json(json!({ "started": true }))),
        Err(msg) => Err(ApiError(StatusCode::CONFLICT, msg)),
    }
}

async fn cancel_project(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state.store.exists(&id) {
        return Err(not_found("Project not found."));
    }
    match pipeline::cancel(&state, &id)
        .await
        .map_err(|msg| ApiError(StatusCode::INTERNAL_SERVER_ERROR, msg))?
    {
        pipeline::CancelOutcome::Cancelled => Ok(Json(json!({
            "cancelling": false,
            "cancelled": true,
            "status": "cancelled"
        }))),
        pipeline::CancelOutcome::Status(status) => Ok(Json(json!({
            "cancelling": false,
            "cancelled": false,
            "status": status
        }))),
    }
}

async fn retry_project(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state.store.exists(&id) {
        return Err(not_found("Project not found."));
    }
    match pipeline::retry(state.clone(), id).await {
        Ok(()) => Ok(Json(json!({ "restarted": true }))),
        Err(msg) => Err(ApiError(StatusCode::CONFLICT, msg)),
    }
}

// ---------------------------------------------------------------------------
// SSE
// ---------------------------------------------------------------------------

async fn project_events(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    if !state.store.exists(&id) {
        return Err(not_found("Project not found."));
    }
    let handle = state.handle(&id);
    let rx = handle.events.subscribe();

    let snapshot = project_view(&state, &id)
        .await
        .map(|v| json!({ "type": "snapshot", "view": v }).to_string())
        .unwrap_or_else(|_| json!({ "type": "snapshot" }).to_string());

    let first = stream::once(async move { Ok(Event::default().data(snapshot)) });
    let rest = BroadcastStream::new(rx)
        .filter_map(|msg| async move { msg.ok().map(|data| Ok(Event::default().data(data))) });
    Ok(Sse::new(first.chain(rest)).keep_alive(KeepAlive::default()))
}

// ---------------------------------------------------------------------------
// Post-render caption restyling
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct RestyleIn {
    /// "impact" | "clean". Omitted = keep the clip's current style.
    #[serde(default)]
    style: Option<String>,
    /// `#RRGGBB`. Omitted = keep the clip's current accent.
    #[serde(default)]
    accent_color: Option<String>,
    /// Curated caption font. Omitted = keep the clip's current font.
    #[serde(default)]
    font: Option<String>,
    /// Replacement caption wording. Omitted = keep the current text.
    #[serde(default)]
    caption_text: Option<String>,
}

/// Releases the per-clip restyle lock on every exit path.
struct RestyleGuard {
    state: AppState,
    key: String,
}
impl Drop for RestyleGuard {
    fn drop(&mut self) {
        self.state.end_restyle(&self.key);
    }
}

/// Re-burn one rendered clip's captions with a new style and/or accent color.
/// Fast path: re-encode from the framed, uncaptioned base intermediate.
/// Older projects without a base rebuild it from the source first (one time).
async fn restyle_clip(
    State(state): State<AppState>,
    AxPath((id, clip_id)): AxPath<(String, String)>,
    Json(body): Json<RestyleIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use crate::captions::{
        accent_bgr_for, build_ass, caption_font_name, default_accent_hex, hex_to_ass_bgr,
        with_caption_text, words_in_interval, CaptionInput, CaptionStyle,
    };

    if !state.store.exists(&id) {
        return Err(not_found("Project not found."));
    }
    let handle = state.handle(&id);
    let _operation = handle.operation.lock().await;
    if handle.is_running() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Processing is still running. Restyle clips once rendering finishes.".into(),
        ));
    }
    let key = format!("{id}/{clip_id}");
    if !state.try_begin_restyle(&key) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "A restyle is already running for this clip.".into(),
        ));
    }
    let _guard = RestyleGuard {
        state: state.clone(),
        key,
    };

    let mut manifest = state
        .store
        .load_manifest(&id)
        .await
        .map_err(|_| not_found("No rendered clips for this project yet."))?;
    let idx = manifest
        .clips
        .iter()
        .position(|c| c.id == clip_id)
        .ok_or_else(|| not_found("Clip not found."))?;
    let clip = manifest.clips[idx].clone();
    if clip.status != ClipStatus::Ready {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Only rendered clips can be restyled.".into(),
        ));
    }

    let cfg = &state.cfg;

    let style = match body.style.as_deref() {
        Some(s) => CaptionStyle::parse_strict(s)
            .ok_or_else(|| bad_request("style must be \"impact\" or \"clean\""))?,
        None => CaptionStyle::from_str(clip.caption_style.as_deref().unwrap_or("impact")),
    };
    let accent_hex = match body.accent_color.as_deref() {
        Some(c) => {
            hex_to_ass_bgr(c).ok_or_else(|| bad_request("accent_color must be #RRGGBB"))?;
            let c = c.trim();
            if c.starts_with('#') {
                c.to_string()
            } else {
                format!("#{c}")
            }
        }
        None => clip
            .accent_color
            .clone()
            .unwrap_or_else(|| default_accent_hex(style).to_string()),
    };
    let caption_font = match body.font.as_deref() {
        Some(font) => caption_font_name(font)
            .ok_or_else(|| bad_request("font must be one of the curated caption fonts"))?
            .to_string(),
        None => clip
            .caption_font
            .clone()
            .unwrap_or_else(|| cfg.caption_font.clone()),
    };
    let caption_text = body
        .caption_text
        .map(|text| text.trim().to_string())
        .or_else(|| clip.caption_text.clone());

    let p = state
        .store
        .load_project(&id)
        .await
        .map_err(ApiError::from)?;
    let transcript = state.store.load_transcript(&id).await.map_err(|_| {
        ApiError(
            StatusCode::CONFLICT,
            "The transcript is no longer on disk, so captions cannot be rebuilt.".into(),
        )
    })?;
    let cancel = tokio_util::sync::CancellationToken::new();

    // Ensure the framed, uncaptioned base exists (projects rendered before
    // base intermediates existed rebuild it here from the source, one time).
    let base_path = state.store.base_clip_path(&id, &clip.id);
    let mut base_ready = state
        .store
        .base_is_ready(&id, &clip.id)
        .await
        .map_err(ApiError::from)?;
    if !base_ready
        && tokio::fs::metadata(&base_path)
            .await
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false)
    {
        state
            .store
            .mark_base_ready(&id, &clip.id)
            .await
            .map_err(ApiError::from)?;
        base_ready = true;
    }
    if !base_ready {
        let source = p.source.clone().ok_or_else(|| {
            ApiError(
                StatusCode::CONFLICT,
                "Source metadata is missing; re-run this project.".into(),
            )
        })?;
        if !p.source_path.is_file() {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "The original video is no longer on disk, so this clip cannot be restyled.".into(),
            ));
        }
        tokio::fs::create_dir_all(state.store.base_dir(&id))
            .await
            .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
        tokio::fs::remove_file(&base_path).await.ok();
        state.store.clear_base_ready(&id, &clip.id).await;
        let base_temp = crate::util::unique_temp_path(&base_path);
        crate::render::render_base_clip(
            cfg,
            &p.source_path,
            &source,
            &clip.layout,
            clip.start_ms,
            clip.end_ms,
            &base_temp,
            &cancel,
            |_| {},
        )
        .await
        .map_err(ApiError::from)?;
        crate::util::promote_atomic(&base_temp, &base_path)
            .await
            .map_err(ApiError::from)?;
        state
            .store
            .mark_base_ready(&id, &clip.id)
            .await
            .map_err(ApiError::from)?;
    }

    // Build the new captions and burn them onto the base.
    let words = with_caption_text(
        &words_in_interval(&transcript.words, clip.start_ms, clip.end_ms),
        caption_text.as_deref(),
    );
    let ass = build_ass(
        &CaptionInput {
            words: &words,
            clip_start_ms: clip.start_ms,
            clip_end_ms: clip.end_ms,
            headline: &clip.headline,
            font: &caption_font,
            accent_bgr: accent_bgr_for(style, Some(&accent_hex)),
        },
        style,
    );
    let clips_dir = state.store.clips_dir(&id);
    let final_path = clips_dir.join(&clip.filename);
    let ass_path =
        crate::util::unique_temp_path(&clips_dir.join(format!("{}.restyle.ass", clip.id)));
    let tmp_out = crate::util::unique_temp_path(&final_path);
    tokio::fs::write(&ass_path, &ass)
        .await
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    let burn = crate::render::burn_captions(
        cfg,
        &base_path,
        &ass_path,
        &tmp_out,
        clip.end_ms.saturating_sub(clip.start_ms),
        &cancel,
        |_| {},
    )
    .await;
    tokio::fs::remove_file(&ass_path).await.ok();
    if let Err(e) = burn {
        tokio::fs::remove_file(&tmp_out).await.ok();
        return Err(ApiError::from(e));
    }

    // Atomically promote the restyled clip into place, then refresh the copy in the
    // user-facing output folder (best-effort, mirroring the render stage).
    crate::util::promote_atomic(&tmp_out, &final_path)
        .await
        .map_err(ApiError::from)?;
    state
        .store
        .mark_final_ready(&id, &clip.id)
        .await
        .map_err(ApiError::from)?;
    if let Some(dir) = manifest.output_dir.clone() {
        tokio::fs::copy(&final_path, std::path::Path::new(&dir).join(&clip.filename))
            .await
            .ok();
    }

    manifest.clips[idx].caption_style = Some(style.label().to_string());
    manifest.clips[idx].accent_color = Some(accent_hex);
    manifest.clips[idx].caption_font = Some(caption_font);
    manifest.clips[idx].caption_text = caption_text;
    state
        .store
        .save_manifest(&id, &manifest)
        .await
        .map_err(ApiError::from)?;
    handle.emit(json!({"type": "clip", "clip": manifest.clips[idx]}));
    Ok(Json(
        serde_json::to_value(&manifest.clips[idx]).unwrap_or_default(),
    ))
}

// ---------------------------------------------------------------------------
// Review decisions + batch export
// ---------------------------------------------------------------------------

async fn require_project(state: &AppState, id: &str) -> Result<(), ApiError> {
    if state.store.exists(id) {
        Ok(())
    } else {
        Err(not_found("Project not found."))
    }
}

async fn get_decisions(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<ReviewDecisions>, ApiError> {
    require_project(&state, &id).await?;
    Ok(Json(state.store.load_decisions(&id).await?))
}

/// Full-map replace. Keys must be this project's canonical clip paths; the
/// verdict enum is enforced by deserialization, so unknown verdicts never
/// reach this handler.
async fn put_decisions(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
    Json(body): Json<ReviewDecisions>,
) -> Result<Json<ReviewDecisions>, ApiError> {
    require_project(&state, &id).await?;
    let prefix = format!("/api/projects/{id}/clips/");
    if let Some(key) = body.decisions.keys().find(|k| !k.starts_with(&prefix)) {
        return Err(bad_request(format!(
            "decision key `{key}` does not belong to project {id}"
        )));
    }
    state.store.save_decisions(&id, &body).await?;
    Ok(Json(body))
}

#[derive(serde::Deserialize)]
struct ExportQuery {
    verdicts: Option<String>,
}

fn build_zip(zip_path: &std::path::Path, entries: &[(String, PathBuf)]) -> anyhow::Result<()> {
    let file = std::fs::File::create(zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, path) in entries {
        zip.start_file(name.as_str(), opts)?;
        std::io::copy(&mut std::fs::File::open(path)?, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

async fn export_clips(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let filter = match &query.verdicts {
        None => vec![ReviewVerdict::Kept],
        Some(spec) => ReviewVerdict::parse_list(spec).ok_or_else(|| {
            bad_request("verdicts must be a comma-separated list of kept|maybe|skipped")
        })?,
    };
    require_project(&state, &id).await?;
    let manifest = state
        .store
        .load_manifest(&id)
        .await
        .map_err(|_| not_found("No rendered clips for this project yet."))?;
    let decisions = state.store.load_decisions(&id).await?;

    let mut selected: Vec<(String, PathBuf)> = Vec::new();
    for clip in &manifest.clips {
        if clip.status != ClipStatus::Ready {
            continue;
        }
        let path = state.store.clips_dir(&id).join(&clip.filename);
        let on_disk = tokio::fs::metadata(&path)
            .await
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false);
        if !on_disk {
            continue;
        }
        let Some(entry) = decisions.decisions.get(&clip_decision_key(&id, &clip.id)) else {
            continue;
        };
        if filter.contains(&entry.verdict) {
            selected.push((clip.filename.clone(), path));
        }
    }
    if selected.is_empty() {
        return Err(not_found("No clips match this verdict filter."));
    }

    let tag = filter
        .iter()
        .map(|v| format!("{v:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join("-");
    let zip_path = state
        .store
        .project_dir(&id)
        .join(format!(".export-{tag}.zip"));
    tokio::task::spawn_blocking(move || build_zip(&zip_path, &selected))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;

    let file = tokio::fs::File::open(
        state
            .store
            .project_dir(&id)
            .join(format!(".export-{tag}.zip")),
    )
    .await
    .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    let len = file
        .metadata()
        .await
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))?
        .len();
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_LENGTH, len.to_string())
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"clips-{tag}.zip\""),
        )
        .body(axum::body::Body::from_stream(
            tokio_util::io::ReaderStream::new(file),
        ))
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))
}

// ---------------------------------------------------------------------------
// Clip serving (Range-aware so <video> scrubbing works, esp. Safari)
// ---------------------------------------------------------------------------

async fn find_clip(
    state: &AppState,
    id: &str,
    clip_id: &str,
) -> Result<(ClipRecord, std::path::PathBuf), ApiError> {
    let manifest = state
        .store
        .load_manifest(id)
        .await
        .map_err(|_| not_found("No rendered clips for this project yet."))?;
    let clip = manifest
        .clips
        .iter()
        .find(|c| c.id == clip_id)
        .cloned()
        .ok_or_else(|| not_found("Clip not found."))?;
    let path = state.store.clips_dir(id).join(&clip.filename);
    if !path.is_file() {
        return Err(not_found(
            "Clip file is not on disk (render may have failed).",
        ));
    }
    Ok((clip, path))
}

async fn serve_clip_inline(
    State(state): State<AppState>,
    AxPath((id, clip_id)): AxPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (_clip, path) = find_clip(&state, &id, &clip_id).await?;
    serve_video(&path, &headers, None).await
}

async fn serve_clip_download(
    State(state): State<AppState>,
    AxPath((id, clip_id)): AxPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (clip, path) = find_clip(&state, &id, &clip_id).await?;
    serve_video(&path, &headers, Some(clip.filename)).await
}

async fn serve_video(
    path: &std::path::Path,
    headers: &HeaderMap,
    download_name: Option<String>,
) -> Result<Response, ApiError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    use tokio_util::io::ReaderStream;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    let len = file
        .metadata()
        .await
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))?
        .len();

    // Parse a simple `bytes=start-end` range.
    let range = parse_byte_range(headers.get(header::RANGE), len);

    let mut builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        // Clip bytes change in place when captions are restyled — never let
        // the browser reuse a cached copy.
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_TYPE, "video/mp4");
    if let Some(name) = &download_name {
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", name),
        );
    }

    let (start, end, status) = match range {
        Ok(Some((s, e))) => (s, e, StatusCode::PARTIAL_CONTENT),
        Ok(None) if len > 0 => (0, len - 1, StatusCode::OK),
        _ => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(axum::body::Body::empty())
                .map_err(|e| ApiError::from(anyhow::Error::from(e)));
        }
    };
    let read_len = end - start + 1;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;

    builder = builder
        .status(status)
        .header(header::CONTENT_LENGTH, read_len.to_string());
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, len),
        );
    }
    builder
        .body(axum::body::Body::from_stream(ReaderStream::new(
            file.take(read_len),
        )))
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))
}

fn parse_byte_range(
    value: Option<&axum::http::HeaderValue>,
    len: u64,
) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = value else { return Ok(None) };
    if len == 0 {
        return Err(());
    }
    let spec = value
        .to_str()
        .map_err(|_| ())?
        .strip_prefix("bytes=")
        .ok_or(())?;
    if spec.contains(',') {
        return Err(());
    }
    let (start, end) = spec.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok(Some((len.saturating_sub(suffix.min(len)), len - 1)));
    }
    let start: u64 = start.parse().map_err(|_| ())?;
    if start >= len {
        return Err(());
    }
    let end = if end.is_empty() {
        len - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(len - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(Some((start, end)))
}

// ---------------------------------------------------------------------------

async fn open_output_folder(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let manifest = state.store.load_manifest(&id).await.ok();
    let dir = manifest
        .and_then(|m| m.output_dir)
        .unwrap_or_else(|| state.cfg.output_root.to_string_lossy().into_owned());
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let opened = std::process::Command::new(opener).arg(&dir).spawn().is_ok();
    Ok(Json(json!({ "opened": opened, "path": dir })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, Bytes};
    use axum::extract::FromRequest;
    use axum::http::Request;

    #[test]
    fn byte_ranges_support_open_ended_and_suffix_requests() {
        let open = axum::http::HeaderValue::from_static("bytes=10-");
        let suffix = axum::http::HeaderValue::from_static("bytes=-20");
        assert_eq!(parse_byte_range(Some(&open), 100), Ok(Some((10, 99))));
        assert_eq!(parse_byte_range(Some(&suffix), 100), Ok(Some((80, 99))));
    }

    #[test]
    fn byte_ranges_reject_unsatisfiable_and_multiple_requests() {
        let beyond = axum::http::HeaderValue::from_static("bytes=100-");
        let multiple = axum::http::HeaderValue::from_static("bytes=0-1,4-5");
        assert!(parse_byte_range(Some(&beyond), 100).is_err());
        assert!(parse_byte_range(Some(&multiple), 100).is_err());
    }

    #[test]
    fn upload_capacity_keeps_one_gibibyte_free() {
        assert_eq!(
            upload_capacity_bytes(Some(3.0)),
            Some(2 * 1024 * 1024 * 1024)
        );
        assert_eq!(upload_capacity_bytes(Some(0.5)), Some(0));
        assert_eq!(upload_capacity_bytes(None), None);
    }

    #[tokio::test]
    async fn video_range_response_caps_end_and_streams_only_requested_bytes() {
        let tmp = std::env::temp_dir().join(format!("cf-range-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let path = tmp.join("clip.mp4");
        tokio::fs::write(&path, b"0123456789").await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, "bytes=4-99".parse().unwrap());

        let response = match serve_video(&path, &headers, None).await {
            Ok(response) => response,
            Err(_) => panic!("range response should be served"),
        };

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 4-9/10");
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "6");
        let body = axum::body::to_bytes(response.into_body(), 6).await.unwrap();
        assert_eq!(&body[..], b"456789");

        tokio::fs::remove_dir_all(tmp).await.ok();
    }

    #[tokio::test]
    async fn interrupted_multipart_upload_removes_the_project_directory() {
        let boundary = "cf-upload-boundary";
        let prefix = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"source.mp4\"\r\nContent-Type: video/mp4\r\n\r\npartial"
        );
        let body = futures::stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(prefix)),
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "client disconnected",
            )),
        ]);
        let request = Request::builder()
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from_stream(body))
            .unwrap();
        let multipart = Multipart::from_request(request, &()).await.unwrap();

        let tmp = std::env::temp_dir().join(format!("cf-upload-{}", crate::util::short_id()));
        let mut cfg = crate::config::Config::resolve();
        cfg.data_dir = tmp.join("data");
        cfg.output_root = tmp.join("output");
        let projects_dir = cfg.data_dir.join("projects");
        let state = AppState::new(cfg);

        assert!(create_project(State(state.clone()), multipart)
            .await
            .is_err());
        let mut projects = tokio::fs::read_dir(projects_dir).await.unwrap();
        assert!(projects.next_entry().await.unwrap().is_none());
        tokio::fs::remove_dir_all(tmp).await.ok();
    }

    #[test]
    fn verdict_filters_parse_strictly() {
        assert_eq!(
            ReviewVerdict::parse_list("kept, maybe"),
            Some(vec![ReviewVerdict::Kept, ReviewVerdict::Maybe])
        );
        assert_eq!(ReviewVerdict::parse_list("kept,bogus"), None);
    }

    fn decision(verdict: ReviewVerdict) -> DecisionEntry {
        DecisionEntry {
            verdict,
            updated_at: chrono::Utc::now(),
        }
    }

    async fn export_fixture_state(tag: &str) -> (AppState, String) {
        let tmp = std::env::temp_dir().join(format!("cf-export-{tag}-{}", crate::util::short_id()));
        let mut cfg = crate::config::Config::resolve();
        cfg.data_dir = tmp.join("data");
        cfg.output_root = tmp.join("output");
        let state = AppState::new(cfg);
        let id = "exportproj1";
        state.store.create_dirs(id).await.unwrap();
        state
            .store
            .save_project(&Project::new(id.to_string(), state.store.source_path(id)))
            .await
            .unwrap();

        let clip = |cid: &str, name: &str| ClipRecord {
            id: cid.into(),
            rank: 1,
            headline: "H".into(),
            filename: name.into(),
            start_ms: 0,
            end_ms: 20_000,
            duration_ms: 20_000,
            selection_reason: "test".into(),
            scores: Scores::default(),
            layout: LayoutPlan::BlurPad,
            status: ClipStatus::Ready,
            error: None,
            low_confidence: false,
            caption_style: None,
            caption_text: None,
            accent_color: None,
            caption_font: None,
        };
        state
            .store
            .save_manifest(
                id,
                &RenderManifest {
                    clips: vec![clip("keepme", "01-keep.mp4"), clip("skipme", "02-skip.mp4")],
                    output_dir: None,
                },
            )
            .await
            .unwrap();
        tokio::fs::write(state.store.clips_dir(id).join("01-keep.mp4"), b"kept-bytes")
            .await
            .unwrap();
        tokio::fs::write(
            state.store.clips_dir(id).join("02-skip.mp4"),
            b"skipped-bytes",
        )
        .await
        .unwrap();
        let mut d = ReviewDecisions::default();
        d.decisions.insert(
            clip_decision_key(id, "keepme"),
            decision(ReviewVerdict::Kept),
        );
        d.decisions.insert(
            clip_decision_key(id, "skipme"),
            decision(ReviewVerdict::Skipped),
        );
        state.store.save_decisions(id, &d).await.unwrap();
        (state, tmp.to_string_lossy().into_owned())
    }

    #[tokio::test]
    async fn decisions_roundtrip_rejects_foreign_keys_and_unknown_projects() {
        let (state, tmp) = export_fixture_state("api-decisions").await;
        let id = "exportproj1";

        let got = match get_decisions(State(state.clone()), AxPath(id.to_string())).await {
            Ok(got) => got,
            Err(e) => panic!("decisions should load: {:?}", e.0),
        };
        assert_eq!(got.decisions.len(), 2);

        let mut foreign = ReviewDecisions::default();
        foreign.decisions.insert(
            "/api/projects/other/clips/c1".into(),
            decision(ReviewVerdict::Kept),
        );
        let err = match put_decisions(State(state.clone()), AxPath(id.to_string()), Json(foreign))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("foreign decision key should be rejected"),
        };
        assert_eq!(err.0, StatusCode::BAD_REQUEST);

        let missing = match get_decisions(State(state.clone()), AxPath("nope".into())).await {
            Err(e) => e,
            Ok(_) => panic!("unknown project should 404"),
        };
        assert_eq!(missing.0, StatusCode::NOT_FOUND);

        tokio::fs::remove_dir_all(tmp).await.ok();
    }

    #[tokio::test]
    async fn export_zips_only_clips_matching_the_verdict_filter() {
        let (state, tmp) = export_fixture_state("api-export").await;
        let id = "exportproj1";

        let response = match export_clips(
            State(state.clone()),
            AxPath(id.to_string()),
            Query(ExportQuery { verdicts: None }),
        )
        .await
        {
            Ok(response) => response,
            Err(e) => panic!("kept export should succeed: {:?}", e.0),
        };
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/zip");
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let reader = zip::ZipArchive::new(std::io::Cursor::new(&body[..])).unwrap();
        assert_eq!(reader.file_names().collect::<Vec<_>>(), vec!["01-keep.mp4"]);

        let empty = match export_clips(
            State(state.clone()),
            AxPath(id.to_string()),
            Query(ExportQuery {
                verdicts: Some("maybe".into()),
            }),
        )
        .await
        {
            Err(e) => e,
            Ok(_) => panic!("empty filter result should 404"),
        };
        assert_eq!(empty.0, StatusCode::NOT_FOUND);

        let bad = match export_clips(
            State(state.clone()),
            AxPath(id.to_string()),
            Query(ExportQuery {
                verdicts: Some("nope".into()),
            }),
        )
        .await
        {
            Err(e) => e,
            Ok(_) => panic!("invalid verdict token should be rejected"),
        };
        assert_eq!(bad.0, StatusCode::BAD_REQUEST);

        tokio::fs::remove_dir_all(tmp).await.ok();
    }
}
