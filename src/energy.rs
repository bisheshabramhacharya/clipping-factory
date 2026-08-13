//! Per-second audio energy profile (PRD §9 feature: energetic moments).
//!
//! A z-score-normalized loudness profile is measured from the same 16 kHz WAV
//! that whisper.cpp transcribes (ffmpeg `astats`, no new dependency). The
//! heuristic selector uses it to give high-energy windows — raised voices,
//! laughs, dramatic peaks — a modest composite boost, mirroring what
//! reaction-aware clippers (podcli/YAMNet, OpusClip-style rubrics) treat as a
//! "something is happening" signal. It deliberately never *penalizes* quiet
//! windows; it only adds evidence in favor of loud ones.
//!
//! ponytail: this is a single scalar per ~1s bucket, not a frequency analysis.
//! Laughter/cheering classification (YAMNet-class events) would be a stronger
//! signal but adds an ONNX model dependency; the loudness boost is the cheap
//! 80% and upgrades in place later.

use crate::config::Config;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// Roughly one `astats` window per second at 16 kHz mono (frame = 1024
/// samples, so 16 frames ≈ 1.024 s). Kept small enough that loudness changes
/// are visible, big enough that a window is not a single 64 ms frame.
const RESET_FRAMES: u32 = 16;

/// RMS loudness (dBFS) per second of source audio.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EnergyProfile {
    pub per_second_db: Vec<f32>,
}

impl EnergyProfile {}

/// Measure the per-second RMS profile of a WAV with ffmpeg `astats`.
/// Returns an error on any failure; callers degrade to "no signal".
pub async fn measure(
    cfg: &Config,
    wav: &Path,
    cancel: &CancellationToken,
) -> Result<EnergyProfile> {
    if !wav.exists() {
        return Err(anyhow!("audio file missing: {}", wav.display()));
    }
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-i".into(),
        wav.to_string_lossy().into_owned(),
        "-af".into(),
        format!("astats=metadata=1:reset={RESET_FRAMES},ametadata=print:key=lavfi.astats.Overall.RMS_level"),
        "-f".into(),
        "null".into(),
        "-".into(),
    ];
    let out = run_astats_capture(&cfg.ffmpeg, &args, cancel).await?;
    let per_second_db = parse_rms_lines(&out);
    if per_second_db.len() < 2 {
        return Err(anyhow!(
            "energy profile too short ({} samples)",
            per_second_db.len()
        ));
    }
    Ok(EnergyProfile { per_second_db })
}

/// Run ffmpeg capturing BOTH streams — `ametadata=print` writes to stderr on
/// this ffmpeg build — while keeping cancellation control over the child.
async fn run_astats_capture(
    bin: &str,
    args: &[String],
    cancel: &CancellationToken,
) -> Result<String> {
    let bin = bin.to_string();
    let args = args.to_vec();
    let mut task = tokio::spawn(async move {
        let out = Command::new(&bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await
            .with_context(|| format!("failed to start `{}`", bin))?;
        let mut all = String::from_utf8_lossy(&out.stderr).into_owned();
        all.push_str(&String::from_utf8_lossy(&out.stdout));
        if !out.status.success() {
            bail!(
                "`{}` exited with {} — {}",
                bin,
                out.status.code().unwrap_or(-1),
                all.lines().last().unwrap_or("").trim()
            );
        }
        Ok(all)
    });

    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            task.abort();
            let _ = task.await;
            bail!("cancelled");
        }
        result = &mut task => Ok(result.context("astats capture failed")??),
    }
}

/// Parse the per-window RMS lines emitted by `ametadata=print`, bucketed by
/// whole second using the frame's `pts_time`. Handles both observed formats:
///
/// ```text
/// [Parsed_ametadata_1 @ 0x…] frame:0    pts:0       pts_time:0
/// [Parsed_ametadata_1 @ 0x…] lavfi.astats.Overall.RMS_level=-21.087600
/// ```
///
/// and the older `frame:N|key:…RMS_level|value:-24.53` pipe format. Missing or
/// `-inf` values (digital silence) map to the quietest observed level; empty
/// buckets stay at that floor.
pub fn parse_rms_lines(output: &str) -> Vec<f32> {
    let mut samples: Vec<(f32, f32)> = Vec::new();
    let mut pending_sec: Option<f32> = None;
    // Quietest finite level observed so far; starts at 0 dBFS (the loudest
    // possible) and descends as real samples arrive.
    let mut floor = 0.0f32;
    let mut seen_finite = false;

    for line in output.lines() {
        if let Some(sec) = line
            .split("pts_time:")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<f32>().ok())
        {
            pending_sec = Some(sec);
        }
        let value = line
            .split("RMS_level=")
            .nth(1)
            .or_else(|| line.split('|').find_map(|part| part.strip_prefix("value:")))
            .and_then(|s| s.trim().parse::<f32>().ok());
        if let Some(v) = value {
            if v.is_finite() {
                seen_finite = true;
                if v < floor {
                    floor = v;
                }
                samples.push((pending_sec.unwrap_or(samples.len() as f32), v));
            } else if seen_finite {
                // `-inf` (digital silence) -> quietest level observed so far.
                samples.push((pending_sec.unwrap_or(samples.len() as f32), floor));
            }
            // Leading silence (all `-inf` before the first finite sample) is
            // skipped: those buckets stay empty and resolve to the final
            // floor, so an intro of exact zeros is scored as quiet, never as
            // the initial 0 dBFS sentinel.
        }
    }
    if samples.is_empty() {
        return Vec::new();
    }

    let buckets = samples.iter().map(|(s, _)| *s as usize).max().unwrap_or(0) + 1;
    let mut acc: Vec<(f32, u32)> = vec![(0.0, 0); buckets];
    for (sec, v) in samples {
        let (sum, count) = &mut acc[sec as usize];
        *sum += v;
        *count += 1;
    }
    acc.into_iter()
        .map(|(sum, count)| {
            if count == 0 {
                floor
            } else {
                sum / count as f32
            }
        })
        .collect()
}

/// Z-score boost (0..=4) for a window's mean loudness vs the episode baseline.
/// Only positive excursions add points; quiet windows get exactly 0.
pub fn window_boost(profile: &EnergyProfile, start_ms: u64, end_ms: u64) -> f32 {
    let db = &profile.per_second_db;
    if db.len() < 2 {
        return 0.0;
    }
    let mean = db.iter().sum::<f32>() / db.len() as f32;
    let var = db.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / db.len() as f32;
    let std = var.sqrt();
    if std < 1.0 {
        return 0.0; // flat audio: no signal to exploit
    }

    let lo = ((start_ms / 1000) as usize).min(db.len());
    let hi = ((end_ms.div_ceil(1000)) as usize).min(db.len());
    if hi <= lo {
        return 0.0;
    }
    let window_mean = db[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
    let z = (window_mean - mean) / std;
    (z.max(0.0) * 1.5).min(4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exact stderr shape observed from this machine's ffmpeg.
    const SAMPLE: &str = "\
[Parsed_ametadata_1 @ 0x0] frame:0    pts:0       pts_time:0
[Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-24.531234
[Parsed_ametadata_1 @ 0x0] frame:1    pts:1024    pts_time:1.024
[Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-18.000000
[Parsed_ametadata_1 @ 0x0] frame:2    pts:2048    pts_time:2.048
[Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-inf
noise";

    #[test]
    fn parses_real_ffmpeg_format_and_buckets_by_second() {
        let out = parse_rms_lines(SAMPLE);
        assert_eq!(out.len(), 3);
        assert!((out[0] + 24.531234).abs() < 1e-4);
        assert!((out[1] + 18.0).abs() < 1e-4);
        assert_eq!(out[2], out[0]); // -inf -> quietest observed
    }

    #[test]
    fn parses_legacy_pipe_format() {
        let out = parse_rms_lines(
            "frame:0|key:lavfi.astats.Overall.RMS_level|value:-30.0\n\
             frame:1|key:lavfi.astats.Overall.RMS_level|value:-20.0",
        );
        assert_eq!(out.len(), 2);
        assert!((out[1] + 20.0).abs() < 1e-4);
    }

    #[test]
    fn leading_digital_silence_is_scored_quiet_not_loud() {
        // An intro of exact zeros emits `-inf` before any finite sample. The
        // initial floor (0 dBFS, the loudest possible) must not leak into
        // those buckets: they resolve to the final floor instead.
        let out = parse_rms_lines(
            "[Parsed_ametadata_1 @ 0x0] frame:0    pts:0       pts_time:0\n\
             [Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-inf\n\
             [Parsed_ametadata_1 @ 0x0] frame:1    pts:1024    pts_time:1.024\n\
             [Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-inf\n\
             [Parsed_ametadata_1 @ 0x0] frame:2    pts:2048    pts_time:2.048\n\
             [Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-24.0\n\
             [Parsed_ametadata_1 @ 0x0] frame:3    pts:3072    pts_time:3.072\n\
             [Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-18.0",
        );
        assert_eq!(out.len(), 4);
        // Leading-silence buckets (0-1) resolve to the final floor, not 0 dBFS.
        assert_eq!(out[0], -24.0);
        assert_eq!(out[1], -24.0);
        assert!((out[2] + 24.0).abs() < 1e-4);
        assert!((out[3] + 18.0).abs() < 1e-4);
    }

    #[test]
    fn loud_window_gets_boost_quiet_window_gets_zero() {
        let mut db = vec![-40.0f32; 120];
        for v in db.iter_mut().skip(60).take(30) {
            *v = -15.0;
        }
        let profile = EnergyProfile { per_second_db: db };
        let boost = window_boost(&profile, 60_000, 90_000);
        assert!(boost > 2.0 && boost <= 4.0, "boost was {boost}");
        assert_eq!(window_boost(&profile, 0, 30_000), 0.0);
        assert_eq!(window_boost(&profile, 90_000, 120_000), 0.0);
    }

    #[test]
    fn flat_audio_and_tiny_profiles_never_boost() {
        let flat = EnergyProfile {
            per_second_db: vec![-22.0; 50],
        };
        assert_eq!(window_boost(&flat, 0, 50_000), 0.0);
        let tiny = EnergyProfile {
            per_second_db: vec![-22.0],
        };
        assert_eq!(window_boost(&tiny, 0, 10_000), 0.0);
        assert_eq!(window_boost(&EnergyProfile::default(), 0, 10_000), 0.0);
    }
}
