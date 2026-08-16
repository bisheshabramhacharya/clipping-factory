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

/// Fixed level for digital silence when the file has no quiet finite
/// reference; `-inf` resolves to the quieter of this and the file-wide floor,
/// keeping a contrast signal in silence-dominated files.
const SILENCE_REFERENCE_DB: f32 = -60.0;

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
/// and the older `frame:N|key:…RMS_level|value:-24.53` pipe format. `-inf`
/// values (digital silence) and empty buckets map to the quieter of the
/// file-wide floor and [`SILENCE_REFERENCE_DB`] — computed in a second pass so
/// leading silence cannot masquerade as a loud peak (a one-pass "quietest so
/// far" floor starts at 0 dBFS).
///
/// The fixed reference only bites when the file has no quiet finite level: a
/// mostly-silent recording whose only sound is loud would otherwise resolve
/// silence to that loud level and collapse to a flat profile.
pub fn parse_rms_lines(output: &str) -> Vec<f32> {
    let mut samples: Vec<(f32, Option<f32>)> = Vec::new();
    let mut pending_sec: Option<f32> = None;
    // Quietest finite level anywhere in the file.
    let mut floor = f32::INFINITY;

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
                floor = floor.min(v);
                samples.push((pending_sec.unwrap_or(samples.len() as f32), Some(v)));
            } else {
                // `-inf` (digital silence) -> resolved to the file-wide floor
                // in the second pass below.
                samples.push((pending_sec.unwrap_or(samples.len() as f32), None));
            }
        }
    }
    if samples.is_empty() {
        return Vec::new();
    }
    // Resolve silence to the quieter of the file-wide floor or the fixed
    // reference. All-silence input falls out of the same expression
    // (INFINITY.min(-60.0) = -60.0), yielding a flat profile that the
    // flat-audio guard in `window_boost` neutralizes.
    let silence_level = floor.min(SILENCE_REFERENCE_DB);

    let buckets = samples.iter().map(|(s, _)| *s as usize).max().unwrap_or(0) + 1;
    let mut acc: Vec<(f32, u32)> = vec![(0.0, 0); buckets];
    for (sec, v) in samples {
        let (sum, count) = &mut acc[sec as usize];
        *sum += v.unwrap_or(silence_level);
        *count += 1;
    }
    acc.into_iter()
        .map(|(sum, count)| {
            if count == 0 {
                silence_level
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
        assert_eq!(out[2], SILENCE_REFERENCE_DB); // -inf -> quietest reference, quieter than any content
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
    fn leading_silence_maps_to_a_quiet_reference_not_to_max_loudness() {
        // A recording that OPENS with digital silence must not look like a
        // 0 dBFS climax (one-pass "quietest so far" started at 0.0).
        let mut out = String::new();
        for i in 0..6u32 {
            let pts = i as f32 * 1.024;
            out.push_str(&format!(
                "[Parsed_ametadata_1 @ 0x0] frame:{i}    pts_time:{pts}\n"
            ));
            if i < 3 {
                out.push_str("[Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-inf\n");
            } else {
                out.push_str(
                    "[Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-18.000000\n",
                );
            }
        }
        let out = parse_rms_lines(&out);
        assert_eq!(out.len(), 6);
        for (i, v) in out.iter().enumerate() {
            let expected = if i < 3 { SILENCE_REFERENCE_DB } else { -18.0 };
            assert!(
                (v - expected).abs() < 1e-4,
                "bucket {i} should be {expected}, got {v}"
            );
        }
        // The silent intro must NOT look loud relative to the file floor...
        let profile = EnergyProfile { per_second_db: out };
        assert_eq!(window_boost(&profile, 0, 3_000), 0.0);
        // ...while a genuinely loud stretch still gets its boost.
        assert!(window_boost(&profile, 3_000, 5_000) > 0.0);
    }

    #[test]
    fn all_silence_stays_flat_and_never_boosts() {
        // No finite RMS level anywhere: every bucket resolves to the fixed
        // silence reference, giving a flat profile that the flat-audio guard
        // in `window_boost` neutralizes.
        let mut out = String::new();
        for i in 0..4u32 {
            let pts = i as f32 * 1.024;
            out.push_str(&format!(
                "[Parsed_ametadata_1 @ 0x0] frame:{i}    pts_time:{pts}\n"
            ));
            out.push_str("[Parsed_ametadata_1 @ 0x0] lavfi.astats.Overall.RMS_level=-inf\n");
        }
        let out = parse_rms_lines(&out);
        assert_eq!(out.len(), 4);
        for v in &out {
            assert!(
                (v - SILENCE_REFERENCE_DB).abs() < 1e-4,
                "bucket should be the silence reference, got {v}"
            );
        }
        let profile = EnergyProfile { per_second_db: out };
        assert_eq!(window_boost(&profile, 0, 4_000), 0.0);
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
