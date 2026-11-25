//! Media inspection (ffprobe) and audio extraction (ffmpeg) — PRD §7.2 steps
//! "Inspect source" and "Extract audio", with the validation and edge cases
//! from §15.

use crate::config::Config;
use crate::domain::SourceInfo;
use crate::util::run_streaming;
use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Long-form ceiling for one uploaded source. Twelve hours covers normal
/// podcast episodes as well as full Twitch/YouTube stream VODs; the local
/// transcription pass is still the practical cost center, so this is a
/// guardrail rather than a target.
pub const MAX_SOURCE_MS: u64 = 12 * 3600 * 1000;

/// Browser/client prefilter for common containers ffmpeg can inspect. The
/// ffprobe decode check remains authoritative — extension support only keeps
/// obviously wrong files out before upload.
const SOURCE_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "ts", "m2ts", "mpg", "mpeg",
];

pub fn is_supported_source_filename(filename: &str) -> bool {
    filename
        .rsplit_once('.')
        .map(|(_, ext)| SOURCE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Validate one user-supplied video URL. Only plain HTTP(S) links are valid;
/// credentials are rejected so a pasted link cannot smuggle basic-auth secret
/// material into yt-dlp's process environment or project metadata.
pub fn validate_source_url(input: &str) -> Result<reqwest::Url> {
    let url = input.trim();
    if url.len() > 2_048
        || url.chars().any(char::is_whitespace)
        || !(url.starts_with("https://") || url.starts_with("http://"))
    {
        bail!("Paste one valid http:// or https:// video URL.");
    }
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| anyhow!("Paste one valid http:// or https:// video URL."))?;
    if parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("Paste one valid http:// or https:// video URL.");
    }
    Ok(parsed)
}

/// The display filename for a URL import. Only well-known public video IDs
/// are copied into the name; arbitrary query strings are deliberately left
/// out so signed URLs do not leak tokens into project labels or output paths.
pub fn source_name_for_url(url: &reqwest::Url) -> String {
    let host = url.host_str().unwrap_or("video").to_lowercase();
    let segments: Vec<&str> = url
        .path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).collect())
        .unwrap_or_default();
    let platform_id = if host.ends_with("youtube.com") {
        url.query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned())
    } else if host == "youtu.be" {
        segments.first().map(|part| (*part).to_string())
    } else if host.ends_with("twitch.tv") {
        if segments.first() == Some(&"videos") {
            segments.get(1).map(|part| (*part).to_string())
        } else {
            segments.first().map(|part| (*part).to_string())
        }
    } else {
        segments.last().map(|part| (*part).to_string())
    };
    let basis = platform_id
        .as_deref()
        .map(|id| id.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(id))
        .filter(|id| !id.is_empty() && id.len() <= 96)
        .map(|id| format!("{host}-{id}"))
        .unwrap_or_else(|| host.clone());
    let slug = crate::util::slugify(&basis, 72);
    format!(
        "url-{}.mp4",
        if slug.is_empty() {
            "video".to_string()
        } else {
            slug
        }
    )
}

/// Remove HTTP(S) URLs from subprocess errors before they are persisted or
/// shown through the local API. Signed CDN links often carry bearer tokens in
/// query parameters; the error text around the URL remains useful without
/// retaining the credential.
fn redact_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let https = rest.find("https://");
        let http = rest.find("http://");
        let Some(pos) = [https, http].into_iter().flatten().min() else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        out.push_str("<redacted-url>");
        rest = &after[end..];
    }
    out
}

/// Parse yt-dlp's normal `[download]  42.0%` progress line.
pub(crate) fn ytdlp_progress_percent(line: &str) -> Option<f32> {
    let rest = line.trim().strip_prefix("[download]")?.trim_start();
    let end = rest.find('%')?;
    let pct: f32 = rest[..end].trim().parse().ok()?;
    (0.0..=100.0).contains(&pct).then_some(pct)
}

static URL_DOWNLOAD_SLOTS: OnceLock<Semaphore> = OnceLock::new();

async fn directory_file_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return 0;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await {
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn ytdlp_rejection_reason(line: &str) -> Option<&'static str> {
    if line.contains("larger than max-filesize") {
        Some("The video is larger than the current disk and upload limit.")
    } else if line.contains("does not pass filter") {
        Some("That link is a livestream or playlist, not one downloadable video.")
    } else {
        None
    }
}

fn is_probable_ytdlp_video(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return true;
    };
    !matches!(
        ext.to_ascii_lowercase().as_str(),
        "json"
            | "description"
            | "jpg"
            | "jpeg"
            | "png"
            | "webp"
            | "srt"
            | "vtt"
            | "ass"
            | "ttml"
            | "xml"
    )
}

/// Download one validated HTTP(S) video URL with yt-dlp. The URL is passed as
/// plain argv (never shell-interpolated), playlists and livestreams are
/// rejected, concurrent downloads are serialized, and a merge-aware byte budget
/// is enforced both by yt-dlp metadata and by watching real bytes as they land.
/// The source is promoted into place only after a successful exit.
pub async fn download_source_url<F>(
    cfg: &Config,
    download_dir: &Path,
    dest: &Path,
    input: &str,
    upload_capacity: u64,
    cancel: &CancellationToken,
    mut on_line: F,
) -> Result<String>
where
    F: FnMut(bool, &str),
{
    let url = validate_source_url(input)?;
    let Some(bin) = cfg.ytdlp_bin.as_ref() else {
        bail!("URL import needs yt-dlp. Install yt-dlp or set CF_YTDLP, then try again.");
    };
    // URL ingestion is serialized because every download receives a disk budget
    // derived from currently free space. Running several at once would multiply
    // that budget and defeat the reserve.
    let slots = URL_DOWNLOAD_SLOTS.get_or_init(|| Semaphore::new(1));
    let _permit = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(crate::util::cancelled()),
        permit = slots.acquire() => permit.context("URL download queue closed")?,
    };
    if tokio::fs::try_exists(download_dir).await.unwrap_or(false) {
        tokio::fs::remove_dir_all(download_dir).await.ok();
    }
    tokio::fs::create_dir_all(download_dir).await?;
    let output_template = download_dir.join("source.%(ext)s");
    let batch_path = download_dir.join("batch-url.txt");
    crate::util::atomic_write_bytes_private(&batch_path, format!("{}\n", url.as_str()).as_bytes())
        .await?;
    let mut args = vec![
        "--no-playlist".into(),
        "--newline".into(),
        // A standalone playlist exposes `playlist_id` even before entries are
        // processed. Rejecting it there prevents a whole collection from being
        // treated as one source; `--no-playlist` still permits a video URL that
        // merely happens to carry a playlist query parameter.
        "--match-filters".into(),
        "!is_live & !playlist_id".into(),
        "--no-warnings".into(),
        "--socket-timeout".into(),
        "30".into(),
        "--retries".into(),
        "3".into(),
    ];
    // yt-dlp shells out to FFmpeg for split-track merges and otherwise searches
    // PATH. Give it the same configured binary the rest of the studio uses;
    // a plain `ffmpeg` command is already discoverable through PATH.
    let ffmpeg_location = if cfg.ffmpeg.contains('/') || cfg.ffmpeg.contains('\\') {
        cfg.ffmpeg.clone()
    } else {
        crate::util::which(&cfg.ffmpeg)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| cfg.ffmpeg.clone())
    };
    args.push("--ffmpeg-location".into());
    args.push(ffmpeg_location);
    args.extend([
        "-f".into(),
        "bv*[height<=1080]+ba/b[height<=1080]/best".into(),
        "--merge-output-format".into(),
        "mp4".into(),
        "--max-filesize".into(),
        upload_capacity.to_string(),
        "-o".into(),
        output_template.to_string_lossy().into_owned(),
        "--batch-file".into(),
        batch_path.to_string_lossy().into_owned(),
    ]);
    // The metadata-based `--max-filesize` check is not enough: sizes can be
    // unknown, and split tracks coexist with ffmpeg's merged output. Watch real
    // bytes in the private download directory and stop before the calculated
    // peak budget is exceeded. A child token lets this watchdog be distinguished
    // from the user's own cancellation.
    let download_cancel = cancel.child_token();
    let exceeded = Arc::new(AtomicBool::new(false));
    let peak_limit = upload_capacity.saturating_mul(2);
    let watcher = {
        let download_cancel = download_cancel.clone();
        let exceeded = exceeded.clone();
        let download_dir = download_dir.to_path_buf();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                if download_cancel.is_cancelled() {
                    break;
                }
                if directory_file_bytes(&download_dir).await > peak_limit {
                    exceeded.store(true, Ordering::SeqCst);
                    download_cancel.cancel();
                    break;
                }
            }
        })
    };
    let mut rejection = None;
    let result = run_streaming(
        &bin.to_string_lossy(),
        &args,
        &download_cancel,
        |is_err, line| {
            if rejection.is_none() {
                rejection = ytdlp_rejection_reason(line);
            }
            on_line(is_err, line);
        },
    )
    .await;
    watcher.abort();
    let _ = watcher.await;
    if exceeded.load(Ordering::SeqCst) {
        tokio::fs::remove_dir_all(download_dir).await.ok();
        bail!("Downloaded video exceeds the upload limit or available disk space.");
    }
    if let Err(e) = result {
        tokio::fs::remove_dir_all(download_dir).await.ok();
        if crate::util::is_cancelled(&e) {
            return Err(e);
        }
        return Err(anyhow!(
            "Could not download that video URL. Check the link and try again. {}",
            redact_urls(&e.to_string())
        ));
    }
    // yt-dlp can skip an oversized split audio/video track yet still exit 0.
    // Accepting the remaining track would silently turn a sounded source into
    // a video-only one, so any explicit filter/size rejection is fatal even
    // when a playable partial file remains.
    if let Some(reason) = rejection {
        tokio::fs::remove_dir_all(download_dir).await.ok();
        bail!(reason);
    }

    let mut downloaded = None;
    let mut entries = tokio::fs::read_dir(download_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("source.")
            && !name.ends_with(".part")
            && is_probable_ytdlp_video(&name)
            && path.is_file()
        {
            downloaded = Some(path);
            break;
        }
    }
    let Some(downloaded) = downloaded else {
        tokio::fs::remove_dir_all(download_dir).await.ok();
        if let Some(reason) = rejection {
            bail!(reason);
        }
        bail!("The URL download finished without a video file.");
    };
    let size = tokio::fs::metadata(&downloaded)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if size == 0 || size > upload_capacity {
        tokio::fs::remove_dir_all(download_dir).await.ok();
        bail!("Downloaded video exceeds the upload limit or available disk space.");
    }
    crate::util::promote_atomic(&downloaded, dest).await?;
    tokio::fs::remove_dir_all(download_dir).await.ok();
    Ok(source_name_for_url(&url))
}

/// Inspect and validate the uploaded source. Errors are user-actionable.
pub async fn probe(
    cfg: &Config,
    src: &Path,
    source_filename: &str,
    cancel: &CancellationToken,
) -> Result<SourceInfo> {
    let args: Vec<String> = vec![
        "-v".into(),
        "error".into(),
        "-print_format".into(),
        "json".into(),
        "-show_format".into(),
        "-show_streams".into(),
        src.to_string_lossy().into_owned(),
    ];
    let out = crate::util::run_capture_cancellable(&cfg.ffprobe, &args, cancel)
        .await
        .map_err(|e| {
            if crate::util::is_cancelled(&e) {
                e
            } else {
                anyhow!("This file could not be read as a video. {}", e)
            }
        })?;
    let v: serde_json::Value =
        serde_json::from_str(&out).context("ffprobe returned unparseable output")?;

    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let video = streams
        .iter()
        .find(|s| is_primary_video_stream(s))
        .ok_or_else(|| {
            anyhow!("No video stream found. Attach a video file that contains video.")
        })?;
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");

    let duration_s: f64 = v["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .or_else(|| video["duration"].as_str().and_then(|d| d.parse().ok()))
        .ok_or_else(|| {
            anyhow!("Could not determine the video duration. The file may be corrupted.")
        })?;
    let duration_ms = (duration_s * 1000.0) as u64;

    if duration_ms == 0 {
        bail!("This video has no measurable duration.");
    }
    if duration_ms > MAX_SOURCE_MS {
        bail!("This video is over 12 hours. Clipping Factory supports sources up to 12 hours.");
    }

    let coded_width = video["width"].as_u64().unwrap_or(0) as u32;
    let coded_height = video["height"].as_u64().unwrap_or(0) as u32;
    if coded_width == 0 || coded_height == 0 {
        bail!("Could not read the video dimensions. The file may be corrupted.");
    }
    // ffmpeg autorotates decoded frames before the filter graph, so crop
    // coordinates must use display dimensions rather than the coded matrix.
    // Phone MOVs commonly store 1920×1080 pixels plus a ±90° rotation.
    let rotation = display_rotation_degrees(video);
    let (width, height) = display_dimensions(coded_width, coded_height, rotation);

    let fps = parse_rate(video["avg_frame_rate"].as_str().unwrap_or(""))
        .or_else(|| parse_rate(video["r_frame_rate"].as_str().unwrap_or("")))
        .unwrap_or(30.0);

    let size_bytes = v["format"]["size"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| std::fs::metadata(src).map(|m| m.len()).unwrap_or(0));

    let video_codec = video["codec_name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let audio_codec = audio
        .and_then(|stream| stream["codec_name"].as_str())
        .unwrap_or("none")
        .to_string();
    let mut codecs = vec![video_codec.as_str()];
    if audio_codec != "none" {
        codecs.push(audio_codec.as_str());
    }
    ensure_decodable(cfg, &codecs, cancel).await?;

    Ok(SourceInfo {
        filename: source_filename.to_string(),
        duration_ms,
        width,
        height,
        fps,
        video_codec,
        audio_codec,
        size_bytes,
        scene_boundaries_ms: Vec::new(),
    })
}

/// A stream whose codec this FFmpeg build has no decoder for must fail here:
/// extract and render would otherwise fail mid-run with a less actionable
/// message. The decoder list is read without touching the source, so the
/// check stays constant-time on long files.
async fn ensure_decodable(cfg: &Config, codecs: &[&str], cancel: &CancellationToken) -> Result<()> {
    let args: Vec<String> = vec!["-hide_banner".into(), "-decoders".into()];
    let list = crate::util::run_capture_cancellable(&cfg.ffmpeg, &args, cancel).await?;
    for codec in codecs {
        if *codec != "unknown" && !decoder_list_has(&list, codec) {
            bail!(
                "This source uses the {codec} codec, which this FFmpeg build cannot decode. \
                 Convert it to H.264 video and AAC audio, then upload it again."
            );
        }
    }
    Ok(())
}

/// Whether an `ffmpeg -decoders` listing names `codec` (the second
/// whitespace-separated field on each decoder row).
fn decoder_list_has(list: &str, codec: &str) -> bool {
    list.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next();
        fields.next() == Some(codec)
    })
}

fn is_primary_video_stream(stream: &serde_json::Value) -> bool {
    stream["codec_type"] == "video" && stream["disposition"]["attached_pic"].as_u64() != Some(1)
}

#[derive(Debug)]
pub struct TruncatedSource;

impl std::fmt::Display for TruncatedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "This video appears truncated or incomplete. Download or upload the complete file, then try again.",
        )
    }
}

impl std::error::Error for TruncatedSource {}

pub fn is_truncated_source(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| cause.is::<TruncatedSource>())
}

/// Demuxers report physically short reads in format-specific wording: MP4
/// says `stream … offset …: partial file`, while Matroska/WebM says
/// `File ended prematurely`. Require ffmpeg's demuxer log context so input
/// metadata/titles containing those words cannot trigger a false positive.
fn is_demuxer_truncation_line(line: &str) -> bool {
    let demuxer_line = line.starts_with('[') && line.contains(" @ 0x");
    demuxer_line
        && ((line.contains("stream ")
            && line.contains(" offset 0x")
            && line.trim_end().ends_with(": partial file"))
            || line.contains("] File ended prematurely")
            || line.contains("] File is truncated"))
}

/// Decode the video stream that inspection models to completion. Scene
/// detection already performs this decode in the normal path; this stricter
/// fallback distinguishes a scan/filter failure from damaged media bytes.
async fn validate_video_decode(cfg: &Config, src: &Path, cancel: &CancellationToken) -> Result<()> {
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-v".into(),
        "error".into(),
        "-nostats".into(),
        "-xerror".into(),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-map".into(),
        "0:V:0".into(),
        "-an".into(),
        "-f".into(),
        "null".into(),
        "-".into(),
    ];
    crate::util::run_streaming(&cfg.ffmpeg, &args, cancel, |_, _| {}).await
}

/// The 0–1 fraction for an ffmpeg `-progress` line's `out_time_ms`
/// (microseconds), clamped to `dur_us`; `None` for other lines or when the
/// duration is not yet known.
pub(crate) fn out_time_fraction(line: &str, dur_us: f64) -> Option<f32> {
    let us: f64 = line.strip_prefix("out_time_ms=")?.parse().ok()?;
    (dur_us > 0.0).then(|| (us / dur_us).clamp(0.0, 1.0) as f32)
}

async fn video_stream_duration_ms(
    cfg: &Config,
    src: &Path,
    cancel: &CancellationToken,
) -> Result<Option<u64>> {
    let args: Vec<String> = vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "V:0".into(),
        "-show_entries".into(),
        "stream=duration".into(),
        "-of".into(),
        "json".into(),
        src.to_string_lossy().into_owned(),
    ];
    let out = crate::util::run_capture_cancellable(&cfg.ffprobe, &args, cancel).await?;
    let json: serde_json::Value = match serde_json::from_str(&out) {
        Ok(json) => json,
        Err(_) => return Ok(None),
    };
    let Some(stream) = json["streams"]
        .as_array()
        .and_then(|streams| streams.first())
    else {
        return Ok(None);
    };
    Ok(stream["duration"]
        .as_str()
        .and_then(|duration| duration.parse::<f64>().ok())
        .map(|seconds| (seconds * 1000.0).round() as u64))
}

/// Detect scene-boundary timestamps once per Source during inspection.
///
/// Runs ffmpeg `scdet` over a downscaled copy of the video — cheap enough
/// for a multi-hour source yet still catches the hard cuts and crossfades a
/// Clip must not open or close on. Advisory like the energy profile:
/// callers degrade to "no boundaries".
///
/// `on_progress` gets the real fraction of the source scanned: `-progress`
/// reports `out_time_ms` on stdout while the scdet detections arrive on
/// stderr.
pub async fn scene_boundaries<F>(
    cfg: &Config,
    src: &Path,
    duration_ms: u64,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<Vec<u64>>
where
    F: FnMut(f32),
{
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-xerror".into(),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-map".into(),
        "0:V:0".into(),
        "-an".into(),
        "-vf".into(),
        "scale=320:-2,scdet".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-f".into(),
        "null".into(),
        "-".into(),
    ];
    let mut boundaries: Vec<u64> = Vec::new();
    // Container duration can include a longer audio track, while this scan
    // decodes video only (`-an`). Compare decode coverage with the video
    // stream's own duration when one is authoritatively available. Matroska
    // duration tags are user metadata and can be stale, so when the numeric
    // stream duration is missing, skip the shortfall heuristic rather than
    // measuring video against audio time or untrusted tags.
    let video_scan_duration_ms = match video_stream_duration_ms(cfg, src, cancel).await {
        Ok(Some(ms)) if ms > 0 => Some(ms),
        Ok(_) => None,
        Err(e) if crate::util::is_cancelled(&e) => return Err(e),
        Err(_) => None,
    };
    // Progress still needs some scale when the video duration is unknown.
    // That fallback is display-only and is deliberately not used for the
    // truncation verdict.
    let progress_duration_ms = video_scan_duration_ms.unwrap_or(duration_ms);
    let dur_us = (progress_duration_ms as f64) * 1000.0;
    let mut decoded_fraction = 0.0f32;
    let mut demuxer_reported_partial_file = false;
    let mut stream_mapping_started = false;
    let result = run_streaming(&cfg.ffmpeg, &args, cancel, |is_err, line| {
        if !is_err {
            if let Some(pct) = out_time_fraction(line, dur_us) {
                decoded_fraction = decoded_fraction.max(pct);
                on_progress(pct);
            }
            return;
        }
        if is_demuxer_truncation_line(line) {
            demuxer_reported_partial_file = true;
        }
        if line.starts_with("Stream mapping:") {
            stream_mapping_started = true;
        }
        if let Some(ms) = parse_scdet_time_ms(line) {
            boundaries.push(ms);
        }
    })
    .await;
    // A truncated faststart MP4 can still probe at its nominal duration and
    // ffmpeg can exit 0 after stopping at the last available packet. Treat a
    // demuxer partial-file report or a large decode shortfall as source
    // corruption rather than silently clipping only the first part.
    match result {
        Err(e) => {
            if crate::util::is_cancelled(&e) {
                return Err(e);
            }
            if demuxer_reported_partial_file {
                return Err(anyhow::Error::new(TruncatedSource));
            }
            // Once decoding has entered stream mapping, `-xerror` normally
            // means damaged media. Confirm with the filter-free decode so a
            // missing/broken scdet capability remains advisory instead of
            // being misreported as corruption.
            if stream_mapping_started {
                match validate_video_decode(cfg, src, cancel).await {
                    Ok(()) => return Err(e),
                    Err(validation_error) if crate::util::is_cancelled(&validation_error) => {
                        return Err(validation_error)
                    }
                    Err(_) => return Err(anyhow::Error::new(TruncatedSource)),
                }
            }
            return Err(e);
        }
        Ok(())
            if demuxer_reported_partial_file
                || video_scan_duration_ms
                    .map(|ms| ms >= 5_000 && decoded_fraction < 0.90)
                    .unwrap_or(false) =>
        {
            return Err(anyhow::Error::new(TruncatedSource));
        }
        Ok(()) => {}
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    Ok(boundaries)
}

/// Parse one scdet detection line into milliseconds. ffmpeg ≥5 reports
/// `lavfi.scdet.time=12.34` (metadata=print) or `lavfi.scdet.time: 12.34`
/// (the filter's own log line); 4.x names the same key `lavfi.scd.time`.
pub(crate) fn parse_scdet_time_ms(line: &str) -> Option<u64> {
    let value = ["lavfi.scdet.time", "lavfi.scd.time"]
        .iter()
        .find_map(|key| line.find(key).map(|i| &line[i + key.len()..]))?;
    let secs: f64 = value
        .trim_start_matches(['=', ':'])
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    if secs.is_finite() && secs >= 0.0 {
        Some((secs * 1000.0).round() as u64)
    } else {
        None
    }
}

fn display_dimensions(coded_width: u32, coded_height: u32, rotation: i32) -> (u32, u32) {
    if matches!(rotation.rem_euclid(360), 90 | 270) {
        (coded_height, coded_width)
    } else {
        (coded_width, coded_height)
    }
}

fn display_rotation_degrees(video: &serde_json::Value) -> i32 {
    let parse = |value: Option<&serde_json::Value>| -> Option<i32> {
        let v = value?;
        if let Some(n) = v.as_i64() {
            return Some(n as i32);
        }
        if let Some(text) = v.as_str() {
            return text.parse::<i32>().ok();
        }
        v.as_f64().map(|n| n as i32)
    };
    let raw = parse(Some(&video["tags"]["rotate"]))
        .or_else(|| {
            video["side_data_list"].as_array().and_then(|items| {
                items.iter().find_map(|item| {
                    item["side_data_type"]
                        .as_str()
                        .is_some_and(|kind| kind == "Display Matrix")
                        .then(|| parse(Some(&item["rotation"])))
                        .flatten()
                })
            })
        })
        .unwrap_or(0);
    raw.rem_euclid(360)
}

fn parse_rate(s: &str) -> Option<f64> {
    let (num, den) = s.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 || num == 0.0 {
        None
    } else {
        Some(num / den)
    }
}

/// Extract mono 16kHz WAV for transcription, reporting progress 0–1.
pub async fn extract_audio<F>(
    cfg: &Config,
    src: &Path,
    out_wav: &Path,
    duration_ms: u64,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(f32),
{
    let args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-vn".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-progress".into(),
        "pipe:1".into(),
        out_wav.to_string_lossy().into_owned(),
    ];
    let dur_us = (duration_ms as f64) * 1000.0;
    run_streaming(&cfg.ffmpeg, &args, cancel, |is_err, line| {
        // ffmpeg -progress emits `out_time_ms=<microseconds>` lines on stdout.
        if !is_err {
            if let Some(pct) = out_time_fraction(line, dur_us) {
                on_progress(pct);
            }
        }
    })
    .await
    .map_err(|e| {
        if crate::util::is_cancelled(&e) {
            e
        } else {
            anyhow!("Audio extraction failed. {}", e)
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media_tools_available(cfg: &crate::config::Config) -> bool {
        fn available(bin: &str) -> bool {
            if bin.contains('/') || bin.contains('\\') {
                Path::new(bin).is_file()
            } else {
                crate::util::which(bin).is_some()
            }
        }
        available(&cfg.ffmpeg) && available(&cfg.ffprobe)
    }

    #[tokio::test]
    async fn scene_scan_rejects_truncated_faststart_mp4() {
        let tmp = std::env::temp_dir().join(format!("cf-trunc-test-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let full = tmp.join("full.mp4");
        let half = tmp.join("half.mp4");
        let cfg = crate::config::Config::resolve();
        if !media_tools_available(&cfg) {
            return;
        }
        let status = std::process::Command::new(&cfg.ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "5",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
            ])
            .arg(&full)
            .status()
            .unwrap();
        assert!(status.success());
        let bytes = std::fs::read(&full).unwrap();
        std::fs::write(&half, &bytes[..bytes.len() / 2]).unwrap();
        let error = scene_boundaries(&cfg, &half, 5_000, &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(is_truncated_source(&error), "{error:#}");
        assert!(
            scene_boundaries(&cfg, &full, 5_000, &CancellationToken::new(), |_| {})
                .await
                .is_ok()
        );
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn scene_scan_accepts_complete_video_with_longer_audio() {
        let tmp = std::env::temp_dir().join(format!("cf-long-audio-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let video = tmp.join("long-audio.mp4");
        let cfg = crate::config::Config::resolve();
        if !media_tools_available(&cfg) {
            return;
        }
        let status = std::process::Command::new(&cfg.ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100:duration=8",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        scene_boundaries(&cfg, &video, 8_000, &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn scene_scan_rejects_corrupt_video_payload() {
        let tmp = std::env::temp_dir().join(format!("cf-corrupt-test-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let video = tmp.join("corrupt.mp4");
        let cfg = crate::config::Config::resolve();
        if !media_tools_available(&cfg) {
            return;
        }
        let status = std::process::Command::new(&cfg.ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100:duration=5",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let mut bytes = std::fs::read(&video).unwrap();
        let mdat = bytes
            .windows(4)
            .position(|window| window == b"mdat")
            .expect("generated mp4 has an mdat box")
            + 4;
        let corrupt_end = (bytes.len() - 1024).min(mdat + 80_000);
        for byte in &mut bytes[mdat + 1024..corrupt_end] {
            *byte = 0;
        }
        std::fs::write(&video, bytes).unwrap();
        let error = scene_boundaries(&cfg, &video, 5_000, &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(is_truncated_source(&error), "{error:#}");
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn scene_scan_accepts_webm_video_with_longer_audio() {
        let tmp = std::env::temp_dir().join(format!("cf-webm-audio-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let video = tmp.join("long-audio.webm");
        let cfg = crate::config::Config::resolve();
        if !media_tools_available(&cfg) {
            return;
        }
        let status = std::process::Command::new(&cfg.ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100:duration=10",
                "-c:v",
                "libvpx",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "libvorbis",
            ])
            .arg(&video)
            .status()
            .unwrap();
        if !status.success() {
            tokio::fs::remove_dir_all(&tmp).await.ok();
            return;
        }
        scene_boundaries(&cfg, &video, 10_000, &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn scene_scan_rejects_truncated_webm() {
        let tmp = std::env::temp_dir().join(format!("cf-webm-trunc-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let full = tmp.join("full.webm");
        let half = tmp.join("half.webm");
        let cfg = crate::config::Config::resolve();
        if !media_tools_available(&cfg) {
            return;
        }
        let status = std::process::Command::new(&cfg.ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100:duration=5",
                "-c:v",
                "libvpx",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "libvorbis",
            ])
            .arg(&full)
            .status()
            .unwrap();
        if !status.success() {
            tokio::fs::remove_dir_all(&tmp).await.ok();
            return;
        }
        let bytes = std::fs::read(&full).unwrap();
        std::fs::write(&half, &bytes[..bytes.len() / 2]).unwrap();
        let error = scene_boundaries(&cfg, &half, 5_000, &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(is_truncated_source(&error), "{error:#}");
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn scene_scan_accepts_multi_track_video() {
        let tmp = std::env::temp_dir().join(format!("cf-multi-video-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let video = tmp.join("multi.mp4");
        let cfg = crate::config::Config::resolve();
        if !media_tools_available(&cfg) {
            return;
        }
        let status = std::process::Command::new(&cfg.ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30:duration=10",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30:duration=5",
                "-map",
                "0:v",
                "-map",
                "1:v",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        scene_boundaries(&cfg, &video, 10_000, &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[test]
    fn cover_art_is_not_the_primary_video_stream() {
        let cover = serde_json::json!({
            "codec_type": "video",
            "codec_name": "mjpeg",
            "disposition": {"attached_pic": 1}
        });
        let footage = serde_json::json!({
            "codec_type": "video",
            "codec_name": "h264",
            "disposition": {"attached_pic": 0}
        });
        assert!(!is_primary_video_stream(&cover));
        assert!(is_primary_video_stream(&footage));
    }

    #[tokio::test]
    async fn scene_scan_selects_non_cover_art_video() {
        let tmp = std::env::temp_dir().join(format!("cf-cover-args-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let src = tmp.join("source.mp4");
        let ffmpeg = tmp.join("ffmpeg");
        let ffprobe = tmp.join("ffprobe");
        let args_file = tmp.join("args.txt");
        tokio::fs::write(&src, b"fake").await.unwrap();
        tokio::fs::write(
            &ffmpeg,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'out_time_ms=5000000\\n'\nexit 0\n",
                args_file.display()
            ),
        )
        .await
        .unwrap();
        tokio::fs::write(
            &ffprobe,
            r#"#!/bin/sh
printf '%s\n' '{"streams":[{"duration":"5.0"}]}'
"#,
        )
        .await
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(&ffprobe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut cfg = crate::config::Config::resolve();
        cfg.ffmpeg = ffmpeg.to_string_lossy().into_owned();
        cfg.ffprobe = ffprobe.to_string_lossy().into_owned();
        scene_boundaries(&cfg, &src, 5_000, &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        let args = tokio::fs::read_to_string(&args_file).await.unwrap();
        assert!(args.contains("0:V:0"), "{args}");
        assert!(!args.contains("0:v:0"), "{args}");
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[test]
    fn truncation_detection_requires_demuxer_context() {
        assert!(is_demuxer_truncation_line(
            "[mov,mp4,m4a,3gp,3g2,mj2 @ 0x0] stream 0, offset 0x2b7a1: partial file"
        ));
        assert!(is_demuxer_truncation_line(
            "[matroska,webm @ 0x0] File ended prematurely"
        ));
        assert!(is_demuxer_truncation_line(
            "[matroska,webm @ 0x0] File ended prematurely at pos. 100 (0x64)"
        ));
        assert!(!is_demuxer_truncation_line(
            "    title           : partial file"
        ));
        assert!(!is_demuxer_truncation_line(
            "    title           : File ended prematurely"
        ));
        assert!(!is_demuxer_truncation_line(
            "partial file downloaded successfully"
        ));
    }

    #[test]
    fn parses_scdet_time_across_ffmpeg_versions() {
        // ffmpeg 4.x scdet log line (metadata key `lavfi.scd.time`).
        assert_eq!(
            parse_scdet_time_ms("[scdet @ 0x0] lavfi.scd.score: 15.625, lavfi.scd.time: 4.2"),
            Some(4_200)
        );
        // ffmpeg ≥5 metadata=print output (metadata key `lavfi.scdet.time`).
        assert_eq!(
            parse_scdet_time_ms("[Parsed_metadata_2 @ 0x0] lavfi.scdet.time=12.345"),
            Some(12_345)
        );
        assert_eq!(parse_scdet_time_ms("lavfi.scdet.time: 0"), Some(0));
        assert_eq!(parse_scdet_time_ms("frame=  100 fps=30"), None);
        assert_eq!(parse_scdet_time_ms("lavfi.scdet.time=abc"), None);
    }

    #[test]
    fn display_dimensions_swap_for_quarter_turns_only() {
        assert_eq!(display_dimensions(1920, 1080, 90), (1080, 1920));
        assert_eq!(display_dimensions(1920, 1080, 270), (1080, 1920));
        assert_eq!(display_dimensions(1920, 1080, 180), (1920, 1080));
        assert_eq!(display_dimensions(1920, 1080, 0), (1920, 1080));
    }

    #[test]
    fn display_rotation_comes_from_tags_or_display_matrix() {
        assert_eq!(
            display_rotation_degrees(&serde_json::json!({"tags":{"rotate":"90"}})),
            90
        );
        assert_eq!(
            display_rotation_degrees(&serde_json::json!({
                "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]
            })),
            270
        );
        assert_eq!(display_rotation_degrees(&serde_json::json!({})), 0);
    }

    #[test]
    fn decoder_list_matches_by_name_only() {
        let list = " V....D h264                 H.264 / AVC\n A....D aac                  AAC\n";
        assert!(decoder_list_has(list, "h264"));
        assert!(decoder_list_has(list, "aac"));
        assert!(!decoder_list_has(list, "vp9"));
        // A codec named only in a description is not a decoder.
        assert!(!decoder_list_has(list, "H.264"));
    }

    #[test]
    fn source_filename_prefilter_accepts_common_video_containers() {
        for name in [
            "show.mp4",
            "show.m4v",
            "demo.mov",
            "vod.mkv",
            "gameplay.webm",
            "camera.avi",
            "capture.ts",
            "archive.m2ts",
            "old.mpg",
            "old.mpeg",
        ] {
            assert!(
                is_supported_source_filename(name),
                "{name} should be accepted"
            );
        }
        assert!(is_supported_source_filename("UPPER.MKV"));
        assert!(!is_supported_source_filename("notes.txt"));
        assert!(!is_supported_source_filename("no-extension"));
    }

    #[test]
    fn progress_fraction_parses_and_clamps() {
        assert_eq!(
            out_time_fraction("out_time_ms=2500000", 10_000_000.0),
            Some(0.25)
        );
        assert_eq!(
            out_time_fraction("out_time_ms=20000000", 10_000_000.0),
            Some(1.0)
        );
        assert_eq!(out_time_fraction("out_time_ms=abc", 10_000_000.0), None);
        assert_eq!(out_time_fraction("frame=3", 10_000_000.0), None);
        assert_eq!(out_time_fraction("out_time_ms=5", 0.0), None);
    }

    #[test]
    fn source_urls_are_strict_and_reject_credentials() {
        assert!(validate_source_url("https://www.youtube.com/watch?v=RJKDbEbxpCY").is_ok());
        assert!(validate_source_url("http://example.com/video.mp4").is_ok());
        for bad in [
            "ftp://example.com/video.mp4",
            "javascript:alert(1)",
            "https://user:pass@example.com/video.mp4",
            "https://example.com/a b.mp4",
            "not-a-url",
        ] {
            assert!(validate_source_url(bad).is_err(), "{bad} should fail");
        }
    }

    #[test]
    fn url_source_names_keep_public_ids_but_not_query_tokens() {
        let youtube =
            validate_source_url("https://www.youtube.com/watch?v=RJKDbEbxpCY&sig=secret-token")
                .unwrap();
        let name = source_name_for_url(&youtube);
        assert!(name.contains("rjkdbebxpcy"), "{name}");
        assert!(!name.contains("secret-token"), "{name}");

        let twitch = validate_source_url("https://www.twitch.tv/videos/2631890024").unwrap();
        assert!(source_name_for_url(&twitch).contains("2631890024"));

        let direct = validate_source_url("http://127.0.0.1:48711/cf-url-e2e-source.mp4").unwrap();
        assert!(source_name_for_url(&direct).contains("cf-url-e2e-source"));
    }

    #[test]
    fn ytdlp_progress_lines_parse_percentages() {
        assert_eq!(
            ytdlp_progress_percent("[download]  42.5% of 10.00MiB at 1.00MiB/s"),
            Some(42.5)
        );
        assert_eq!(
            ytdlp_progress_percent("[download] 100% of 10.00MiB"),
            Some(100.0)
        );
        assert_eq!(ytdlp_progress_percent("[youtube] extracting info"), None);
    }

    #[test]
    fn ytdlp_media_discovery_excludes_sidecars_not_valid_media_extensions() {
        assert!(is_probable_ytdlp_video("source.unknown_video"));
        assert!(is_probable_ytdlp_video("source.flv"));
        assert!(!is_probable_ytdlp_video("source.info.json"));
        assert!(!is_probable_ytdlp_video("source.jpg"));
        assert!(!is_probable_ytdlp_video("source.vtt"));
    }

    #[test]
    fn ytdlp_rejections_become_actionable_messages() {
        assert_eq!(
            ytdlp_rejection_reason(
                "[download] File is larger than max-filesize (1 > 2). Aborting."
            ),
            Some("The video is larger than the current disk and upload limit.")
        );
        assert_eq!(
            ytdlp_rejection_reason(
                "video does not pass filter (!is_live & !playlist_id), skipping .."
            ),
            Some("That link is a livestream or playlist, not one downloadable video.")
        );
        assert_eq!(ytdlp_rejection_reason("[download] 10%"), None);
    }

    #[test]
    fn download_errors_redact_signed_urls() {
        let redacted = redact_urls(
            "unable to download https://cdn.example/video.mp4?sig=secret retry http://x.test/a?token=two",
        );
        assert!(!redacted.contains("secret"), "{redacted}");
        assert!(!redacted.contains("token=two"), "{redacted}");
        assert_eq!(redacted.matches("<redacted-url>").count(), 2);
        assert!(redacted.contains("unable to download"), "{redacted}");
    }

    #[tokio::test]
    async fn url_download_rejects_partial_tracks_even_with_success_exit() {
        let tmp = std::env::temp_dir().join(format!("cf-url-partial-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let fake = tmp.join("fake-yt-dlp");
        let script = r#"#!/bin/bash
out=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then shift; out="$1"; fi
  shift
done
echo '[download] File is larger than max-filesize (10 bytes > 5 bytes). Aborting.'
printf 'video-without-audio' > "${out//%(ext)s/mp4}"
"#;
        tokio::fs::write(&fake, script).await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }
        let mut cfg = crate::config::Config::resolve();
        cfg.ytdlp_bin = Some(fake);
        let error = download_source_url(
            &cfg,
            &tmp.join("download"),
            &tmp.join("source.mp4"),
            "https://example.com/video",
            1024,
            &CancellationToken::new(),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("larger than the current disk"));
        assert!(!tmp.join("source.mp4").exists());
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn url_download_promotes_only_successful_ytdlp_output() {
        let tmp = std::env::temp_dir().join(format!("cf-url-test-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let fake = tmp.join("fake-yt-dlp");
        let args_log = tmp.join("argv-url.txt");
        let all_args_log = tmp.join("argv-all.txt");
        let script = r#"#!/bin/bash
out=""
printf '%s\n' "$@" > "__ALL_ARGS_LOG__"
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then shift; out="$1"; fi
  if [ "$1" = "--batch-file" ]; then shift; url=$(cat "$1"); else url="$1"; fi
  shift
done
printf '%s\n' "$url" > "__ARGS_LOG__"
echo '[download]  50.0% of 1.00MiB'
printf 'metadata' > "${out//%(ext)s/info.json}"
printf 'thumbnail' > "${out//%(ext)s/jpg}"
printf 'fake-video' > "${out//%(ext)s/mp4}"
"#
        .replace("__ARGS_LOG__", &args_log.to_string_lossy())
        .replace("__ALL_ARGS_LOG__", &all_args_log.to_string_lossy());
        tokio::fs::write(&fake, script).await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }

        let mut cfg = crate::config::Config::resolve();
        cfg.ytdlp_bin = Some(fake);
        cfg.ffmpeg = "/opt/custom/bin/ffmpeg".into();
        let download_dir = tmp.join("download");
        let dest = tmp.join("source.mp4");
        let url = "https://www.youtube.com/watch?v=RJKDbEbxpCY&sig=secret";
        let mut saw_progress = false;
        let name = download_source_url(
            &cfg,
            &download_dir,
            &dest,
            url,
            1024,
            &CancellationToken::new(),
            |_, line| {
                saw_progress |= ytdlp_progress_percent(line) == Some(50.0);
            },
        )
        .await
        .unwrap();
        assert!(name.contains("rjkdbebxpcy"));
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), b"fake-video");
        assert_eq!(
            tokio::fs::read_to_string(&args_log).await.unwrap().trim(),
            url
        );
        let all_args = tokio::fs::read_to_string(&all_args_log).await.unwrap();
        assert!(all_args.contains("--newline"));
        assert!(all_args.contains("--match-filters\n!is_live & !playlist_id"));
        assert!(all_args.contains("--ffmpeg-location\n/opt/custom/bin/ffmpeg"));
        assert!(all_args.contains("--max-filesize\n1024"));
        assert!(all_args.contains("--batch-file"));
        assert!(!all_args.contains("sig=secret"));
        assert!(saw_progress);
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }
}
