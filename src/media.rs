//! Media inspection (ffprobe) and audio extraction (ffmpeg) — PRD §7.2 steps
//! "Inspect source" and "Extract audio", with the validation and edge cases
//! from §15.

use crate::config::Config;
use crate::domain::SourceInfo;
use crate::util::run_streaming;
use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use tokio_util::sync::CancellationToken;

pub const MAX_SOURCE_MS: u64 = 4 * 3600 * 1000;

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
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(|| anyhow!("No video stream found. Attach an MP4 that contains video."))?;
    let audio = streams
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .ok_or_else(|| {
            anyhow!(
                "This MP4 has no audio stream. Clipping Factory needs speech audio to work with."
            )
        })?;

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
        bail!("This video is over 4 hours. The MVP supports sources up to 4 hours.");
    }

    let width = video["width"].as_u64().unwrap_or(0) as u32;
    let height = video["height"].as_u64().unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        bail!("Could not read the video dimensions. The file may be corrupted.");
    }

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
    let audio_codec = audio["codec_name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    ensure_decodable(cfg, [&video_codec, &audio_codec], cancel).await?;

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
async fn ensure_decodable(
    cfg: &Config,
    codecs: [&str; 2],
    cancel: &CancellationToken,
) -> Result<()> {
    let args: Vec<String> = vec!["-hide_banner".into(), "-decoders".into()];
    let list = crate::util::run_capture_cancellable(&cfg.ffmpeg, &args, cancel).await?;
    for codec in codecs {
        if codec != "unknown" && !decoder_list_has(&list, codec) {
            bail!(
                "This source uses the {codec} codec, which this FFmpeg build cannot decode. \
                 Convert it to an MP4 with H.264 video and AAC audio, then upload it again."
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

/// The 0–1 fraction for an ffmpeg `-progress` line's `out_time_ms`
/// (microseconds), clamped to `dur_us`; `None` for other lines or when the
/// duration is not yet known.
pub(crate) fn out_time_fraction(line: &str, dur_us: f64) -> Option<f32> {
    let us: f64 = line.strip_prefix("out_time_ms=")?.parse().ok()?;
    (dur_us > 0.0).then(|| (us / dur_us).clamp(0.0, 1.0) as f32)
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
        "-i".into(),
        src.to_string_lossy().into_owned(),
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
    let dur_us = (duration_ms as f64) * 1000.0;
    run_streaming(&cfg.ffmpeg, &args, cancel, |is_err, line| {
        if !is_err {
            if let Some(pct) = out_time_fraction(line, dur_us) {
                on_progress(pct);
            }
            return;
        }
        if let Some(ms) = parse_scdet_time_ms(line) {
            boundaries.push(ms);
        }
    })
    .await?;
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
    fn decoder_list_matches_by_name_only() {
        let list = " V....D h264                 H.264 / AVC\n A....D aac                  AAC\n";
        assert!(decoder_list_has(list, "h264"));
        assert!(decoder_list_has(list, "aac"));
        assert!(!decoder_list_has(list, "vp9"));
        // A codec named only in a description is not a decoder.
        assert!(!decoder_list_has(list, "H.264"));
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
}
