//! Audio loudness normalization and QC (EBU R128).
//!
//! Social platforms normalize playback loudness themselves; delivering clips
//! near a consistent target (-16 LUFS integrated, -1.5 dBTP) keeps them from
//! being turned up or down unpredictably and makes a batch of clips sound
//! uniform. Two-pass `loudnorm` measures the source segment first, then
//! applies a *linear* gain correction during the base render — no dynamic
//! processing, so the speech keeps its natural dynamics. The final file is
//! measured again as a QC check that travels with the clip record.

use crate::config::Config;
use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio_util::sync::CancellationToken;

/// Delivery targets (streaming/spocial-video convention): -16 LUFS
/// integrated, -1.5 dBTP ceiling, LRA left open for speech.
pub const TARGET_I: f64 = -16.0;
pub const TARGET_TP: f64 = -1.5;
const TARGET_LRA: f64 = 11.0;

/// One pass of `loudnorm` measurement over a source segment (or whole file
/// when `dur_ms` is `None`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct LoudnessMeasure {
    #[serde(rename = "input_i")]
    pub input_i: f64,
    #[serde(rename = "input_tp")]
    pub input_tp: f64,
    #[serde(rename = "input_lra")]
    pub input_lra: f64,
    #[serde(rename = "input_thresh")]
    pub input_thresh: f64,
    #[serde(rename = "target_offset")]
    pub target_offset: f64,
}

/// Second-pass loudnorm filter: linear gain correction toward the target.
pub fn loudnorm_filter(m: &LoudnessMeasure) -> String {
    format!(
        "loudnorm=I={}:TP={}:LRA={}:measured_I={:.2}:measured_TP={:.2}:\
measured_LRA={:.2}:measured_thresh={:.2}:offset={:.2}:linear=true:print_format=none",
        TARGET_I,
        TARGET_TP,
        TARGET_LRA,
        m.input_i,
        m.input_tp,
        m.input_lra,
        m.input_thresh,
        m.target_offset
    )
}

/// Measure integrated loudness / true peak of a file (or a time-boxed slice
/// of it) with `loudnorm`'s JSON printout on stderr.
pub async fn measure(
    cfg: &Config,
    input: &Path,
    start_ms: Option<u64>,
    dur_ms: Option<u64>,
    cancel: &CancellationToken,
) -> Result<LoudnessMeasure> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-loglevel".into(),
        "info".into(),
    ];
    if let Some(s) = start_ms {
        args.extend(["-ss".into(), format!("{:.3}", s as f64 / 1000.0)]);
    }
    if let Some(d) = dur_ms {
        args.extend(["-t".into(), format!("{:.3}", d as f64 / 1000.0)]);
    }
    args.extend([
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-map".into(),
        "a:0".into(),
        "-af".into(),
        format!(
            "loudnorm=I={}:TP={}:LRA={}:print_format=json",
            TARGET_I, TARGET_TP, TARGET_LRA
        ),
        "-f".into(),
        "null".into(),
        "-".into(),
    ]);

    // loudnorm prints its JSON report on stderr, so this mirrors
    // `util::run_capture_cancellable` but captures stderr.
    let bin = cfg.ffmpeg.clone();
    let mut task = tokio::spawn(async move {
        tokio::process::Command::new(&bin)
            .args(&args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .with_context(|| format!("failed to start `{}`", bin))
    });

    let out = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            task.abort();
            let _ = task.await;
            bail!("cancelled");
        }
        result = &mut task => result.context("loudness measurement task failed")??,
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "`{}` exited with {} — {}",
            cfg.ffmpeg,
            out.status.code().unwrap_or(-1),
            err.lines().last().unwrap_or("").trim()
        );
    }

    parse_loudnorm_json(&String::from_utf8_lossy(&out.stderr))
}

/// Extract the last JSON object from ffmpeg's stderr (loudnorm prints exactly
/// one at the end of the run). Real ffmpeg quotes every value ("-23.40"), so
/// numeric fields accept strings or bare numbers.
fn parse_loudnorm_json(stderr: &str) -> Result<LoudnessMeasure> {
    let open = stderr
        .rfind('{')
        .ok_or_else(|| anyhow!("loudnorm produced no JSON report"))?;
    let close = stderr[open..]
        .find('}')
        .ok_or_else(|| anyhow!("loudnorm JSON report was truncated"))?;
    let json = &stderr[open..=open + close];
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| anyhow!("could not parse loudnorm report: {e}"))?;
    let field = |name: &str| -> Result<f64> {
        match &value[name] {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.parse::<f64>().ok(),
            _ => None,
        }
        .ok_or_else(|| anyhow!("loudnorm report was missing {name}"))
    };
    let parsed = LoudnessMeasure {
        input_i: field("input_i")?,
        input_tp: field("input_tp")?,
        input_lra: field("input_lra")?,
        input_thresh: field("input_thresh")?,
        target_offset: field("target_offset")?,
    };
    // Silence measures around -70/-inf; a linear correction would be garbage.
    if parsed.input_i < -70.0 {
        bail!("segment is effectively silent; skipping loudness normalization");
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(i: &str, tp: &str, lra: &str, thresh: &str, offset: &str) -> String {
        format!(
            r#"some ffmpeg noise above
[Parsed_loudnorm] 
{{
	"input_i" : "{i}",
	"input_tp" : "{tp}",
	"input_lra" : "{lra}",
	"input_thresh" : "{thresh}",
	"output_i" : "-16.05",
	"output_tp" : "-1.54",
	"output_lra" : "8.40",
	"output_thresh" : "-26.11",
	"normalization_type" : "dynamic",
	"target_offset" : "{offset}"
}}
"#
        )
    }

    #[test]
    fn parses_quoted_values_exactly_as_ffmpeg_emits_them() {
        let m = parse_loudnorm_json(&report("-23.4", "-1.2", "8.1", "-33.9", "-0.05")).unwrap();
        assert_eq!(m.input_i, -23.4);
        assert_eq!(m.input_tp, -1.2);
        assert_eq!(m.input_lra, 8.1);
        assert_eq!(m.input_thresh, -33.9);
        assert_eq!(m.target_offset, -0.05);
    }

    #[test]
    fn parses_bare_numbers_too() {
        let m = parse_loudnorm_json(
            r#"{ "input_i" : -23.4, "input_tp" : -1.2, "input_lra" : 8.1,
                 "input_thresh" : -33.9, "target_offset" : -0.05 }"#,
        )
        .unwrap();
        assert_eq!(m.input_i, -23.4);
    }

    #[test]
    fn missing_fields_are_an_error_not_a_default() {
        let broken = report("-23.4", "-1.2", "8.1", "-33.9", "-0.05").replacen(
            "\"input_i\"",
            "\"input_missing\"",
            1,
        );
        assert!(parse_loudnorm_json(&broken).is_err());
    }

    #[test]
    fn missing_report_is_an_error() {
        assert!(parse_loudnorm_json("no json here").is_err());
    }

    #[test]
    fn silence_is_rejected_instead_of_gain_stacked() {
        let err =
            parse_loudnorm_json(&report("-91.0", "-91.0", "0.0", "-100.0", "0.0")).unwrap_err();
        assert!(err.to_string().contains("silent"));
    }

    #[test]
    fn second_pass_filter_is_linear_and_carries_measurements() {
        let m = LoudnessMeasure {
            input_i: -23.4,
            input_tp: -1.2,
            input_lra: 8.1,
            input_thresh: -33.9,
            target_offset: -0.05,
        };
        let f = loudnorm_filter(&m);
        assert!(f.starts_with("loudnorm=I=-16:TP=-1.5:LRA=11:"));
        assert!(f.contains("measured_I=-23.40"));
        assert!(f.contains("measured_TP=-1.20"));
        assert!(f.contains("linear=true"));
    }
}
