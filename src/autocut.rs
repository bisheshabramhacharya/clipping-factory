//! Auto-cut — the per-clip opt-in that tightens pacing by removing silence
//! gaps and filler words ("um", "uh", …) at render time. Default off: a Clip
//! is otherwise one continuous faithful excerpt, and that promise holds.
//!
//! Two removal sources combine into one cut list:
//! - **Silence gaps**: ffmpeg `silencedetect` run over the clip's audio span
//!   (precise, millisecond-accurate). When detection fails — e.g. an ffmpeg
//!   build without the filter — the stored per-second [`EnergyProfile`]
//!   degrades the feature to coarser cuts, and a clip with neither simply
//!   loses the silence part and still drops its filler words.
//! - **Filler words**: transcript word timestamps. Deliberately narrow list —
//!   words like "like" or "just" carry real meaning too often to cut safely.
//!
//! The result is a removal list in absolute ms: inverted by
//! [`keeps_from_removals`] into the spans the render graph keeps, and
//! persisted on the ClipRecord so restyle/retry reproduce the identical cut
//! without re-running detection. Captions follow the cut via
//! [`retime_words`], which maps word timings onto the output timeline and
//! drops words that landed inside a removal.

use crate::config::Config;
use crate::domain::{CutSpan, Word};
use crate::energy::EnergyProfile;
use anyhow::Result;
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// Non-lexical fillers removed without hesitation. Matching normalizes case
/// and strips punctuation, so whisper's `"uh,"` or `"Um."` still match.
const FILLER_WORDS: &[&str] = &[
    "um", "uh", "umm", "uhh", "er", "erm", "ah", "eh", "hmm", "hm",
];

/// silencedetect noise floor (dBFS) and minimum pause length. A podcast's
/// room tone sits below -45 dB, so -35 dB separates real pauses from quiet
/// speech without eating soft syllables.
const SILENCE_DB: &str = "-35dB";
const MIN_SILENCE_MS: u64 = 500;

/// Breath kept at each edge of a removed silence, so a cut lands between
/// phrases rather than mid-phoneme.
const SILENCE_EDGE_PAD_MS: u64 = 100;

/// Two removals separated by less than this merge when no word is spoken
/// between them — otherwise the sliver renders as a few-frame flash.
const MIN_KEEP_SLIVER_MS: u64 = 80;

/// The energy fallback only trusts a profile whose quietest second is
/// genuinely quiet — otherwise nothing in the file is evidence of silence.
const ENERGY_SILENCE_FLOOR_DB: f32 = -30.0;
/// A bucket counts as silent when within this of the file's quietest level.
const ENERGY_SILENCE_BAND_DB: f32 = 1.0;

/// Compute the cut list for a clip: silence detection on the source audio,
/// filler words from the transcript, merged into sorted removal spans.
pub async fn plan_for_clip(
    cfg: &Config,
    src: &Path,
    words: &[Word],
    clip_start_ms: u64,
    clip_end_ms: u64,
    energy: Option<&EnergyProfile>,
    cancel: &CancellationToken,
) -> Result<Vec<CutSpan>> {
    let silences = match detect_silences(cfg, src, clip_start_ms, clip_end_ms, cancel).await {
        Ok(spans) => spans,
        Err(e) => {
            if cancel.is_cancelled() || e.to_string().contains("cancelled") {
                return Err(e);
            }
            tracing::warn!("silencedetect failed, falling back to energy profile: {e:#}");
            energy
                .map(|p| energy_silences(p, clip_start_ms, clip_end_ms))
                .unwrap_or_default()
        }
    };
    Ok(build_plan(clip_start_ms, clip_end_ms, words, silences))
}

/// Combine filler-word and silence removals into one merged list (sorted,
/// non-overlapping, clamped to the clip). `words` is the full transcript;
/// only words overlapping the clip matter. A list that would consume the
/// whole clip collapses to empty — never render an empty file.
pub fn build_plan(
    clip_start_ms: u64,
    clip_end_ms: u64,
    words: &[Word],
    silences: Vec<CutSpan>,
) -> Vec<CutSpan> {
    let mut removals: Vec<CutSpan> = Vec::new();

    for w in words {
        if w.start_ms >= clip_end_ms || w.end_ms <= clip_start_ms {
            continue;
        }
        if is_filler(&w.text) {
            removals.push(CutSpan {
                start_ms: w.start_ms.max(clip_start_ms),
                end_ms: w.end_ms.min(clip_end_ms),
            });
        }
    }

    for s in silences {
        let start = s.start_ms.max(clip_start_ms);
        let end = s.end_ms.min(clip_end_ms);
        if end.saturating_sub(start) < MIN_SILENCE_MS {
            continue;
        }
        // Keep a breath at each edge so the cut feels intentional.
        let start = start.saturating_add(SILENCE_EDGE_PAD_MS);
        let end = end.saturating_sub(SILENCE_EDGE_PAD_MS);
        if start < end {
            removals.push(CutSpan {
                start_ms: start,
                end_ms: end,
            });
        }
    }

    removals.sort_by_key(|r| r.start_ms);
    let merged = merge_removals(removals, words);
    if keeps_from_removals(clip_start_ms, clip_end_ms, &merged).is_empty() {
        return Vec::new();
    }
    merged
}

/// The kept intervals inside `[start_ms, end_ms)` once `removals` (already
/// sorted + merged, absolute ms) are taken out.
pub fn keeps_from_removals(start_ms: u64, end_ms: u64, removals: &[CutSpan]) -> Vec<CutSpan> {
    let mut keeps = Vec::new();
    let mut cursor = start_ms;
    for r in removals {
        if r.start_ms > cursor {
            keeps.push(CutSpan {
                start_ms: cursor,
                end_ms: r.start_ms.min(end_ms),
            });
        }
        cursor = cursor.max(r.end_ms);
    }
    if cursor < end_ms {
        keeps.push(CutSpan {
            start_ms: cursor,
            end_ms,
        });
    }
    keeps
}

/// Merge sorted removals: fold overlaps, and fold two removals separated by a
/// sub-`MIN_KEEP_SLIVER_MS` gap when no word is spoken inside the gap (the
/// sliver would render as a flash). A word bridging the gap keeps the cut
/// apart — real speech is never removed just for being short.
fn merge_removals(removals: Vec<CutSpan>, words: &[Word]) -> Vec<CutSpan> {
    let mut merged: Vec<CutSpan> = Vec::new();
    for r in removals {
        if r.end_ms <= r.start_ms {
            continue;
        }
        match merged.last_mut() {
            Some(last)
                if r.start_ms <= last.end_ms
                    || (r.start_ms - last.end_ms < MIN_KEEP_SLIVER_MS
                        && !word_in_gap(words, last.end_ms, r.start_ms)) =>
            {
                last.end_ms = last.end_ms.max(r.end_ms);
            }
            _ => merged.push(r),
        }
    }
    merged
}

/// True when a word covers any part of the open interval `(from, to)`.
fn word_in_gap(words: &[Word], from: u64, to: u64) -> bool {
    words.iter().any(|w| w.start_ms < to && w.end_ms > from)
}

/// Non-lexical filler check on a normalized token.
fn is_filler(text: &str) -> bool {
    let clean: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect::<String>()
        .to_lowercase();
    FILLER_WORDS.contains(&clean.as_str())
}

/// Map a source timestamp onto the output timeline: subtract everything
/// removed before `t`. A point inside a removal maps to the removal's start —
/// both edges of a removed span collapse to the same output instant.
pub(crate) fn map_to_output(t: u64, removals: &[CutSpan]) -> u64 {
    let mut removed = 0u64;
    for r in removals {
        if r.end_ms <= t {
            removed += r.len_ms();
        } else if r.start_ms < t {
            removed += t - r.start_ms;
            break;
        } else {
            break;
        }
    }
    t - removed
}

/// Shift caption words onto the output timeline. Words fully inside a
/// removal are dropped (the "um" is gone from audio and caption alike); a
/// word spanning a removal keeps only its surviving time.
pub fn retime_words(words: &[Word], removals: &[CutSpan]) -> Vec<Word> {
    if removals.is_empty() {
        return words.to_vec();
    }
    words
        .iter()
        .filter_map(|w| {
            let start_ms = map_to_output(w.start_ms, removals);
            let end_ms = map_to_output(w.end_ms, removals);
            if end_ms <= start_ms {
                None
            } else {
                Some(Word {
                    text: w.text.clone(),
                    start_ms,
                    end_ms,
                    p: w.p,
                })
            }
        })
        .collect()
}

/// Shift speaker turns onto the output timeline — the same map as
/// [`retime_words`], so a caption's speaker label and a speaker-crop's cut
/// always agree about who is on screen. Turns fully inside a removal
/// collapse and drop out.
pub fn retime_turns(
    turns: &[crate::domain::SpeakerTurn],
    removals: &[CutSpan],
) -> Vec<crate::domain::SpeakerTurn> {
    if removals.is_empty() {
        return turns.to_vec();
    }
    turns
        .iter()
        .filter_map(|t| {
            let start_ms = map_to_output(t.start_ms, removals);
            let end_ms = map_to_output(t.end_ms, removals);
            (end_ms > start_ms).then_some(crate::domain::SpeakerTurn {
                start_ms,
                end_ms,
                speaker: t.speaker,
            })
        })
        .collect()
}

/// ffmpeg `silencedetect` over the clip's audio span → absolute-ms spans.
/// `-ss`/`-t` bound the read so detection is clip-scoped, not file-wide.
async fn detect_silences(
    cfg: &Config,
    src: &Path,
    clip_start_ms: u64,
    clip_end_ms: u64,
    cancel: &CancellationToken,
) -> Result<Vec<CutSpan>> {
    let dur_s = clip_end_ms.saturating_sub(clip_start_ms) as f64 / 1000.0;
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-ss".into(),
        format!("{:.3}", clip_start_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", dur_s),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-vn".into(),
        "-af".into(),
        format!(
            "silencedetect=noise={}:d={:.3}",
            SILENCE_DB,
            MIN_SILENCE_MS as f64 / 1000.0
        ),
        "-f".into(),
        "null".into(),
        "-".into(),
    ];
    let out = crate::util::run_capture_cancellable_all(&cfg.ffmpeg, &args, cancel).await?;
    Ok(parse_silencedetect(&out, clip_start_ms))
}

/// Parse silencedetect's log lines into spans offset to absolute source ms:
///
/// ```text
/// [silencedetect @ 0x…] silence_start: 12.345
/// [silencedetect @ 0x…] silence_end: 15.678 | silence_duration: 3.333
/// ```
///
/// A silence running to the end of the probed span has no `silence_end`;
/// it is emitted with `u64::MAX` and the caller's clamp bounds it.
pub fn parse_silencedetect(output: &str, offset_ms: u64) -> Vec<CutSpan> {
    let field = |line: &str, key: &str| -> Option<f64> {
        line.split(key)
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|v| v.parse::<f64>().ok())
    };
    let to_ms = |s: f64| offset_ms.saturating_add((s * 1000.0).round().max(0.0) as u64);

    let mut spans = Vec::new();
    let mut pending: Option<f64> = None;
    for line in output.lines() {
        if let Some(start) = field(line, "silence_start:") {
            pending = Some(start);
        } else if let Some(end) = field(line, "silence_end:") {
            if let Some(start) = pending.take() {
                spans.push(CutSpan {
                    start_ms: to_ms(start),
                    end_ms: to_ms(end),
                });
            }
        }
    }
    if let Some(start) = pending {
        spans.push(CutSpan {
            start_ms: to_ms(start),
            end_ms: u64::MAX,
        });
    }
    spans
}

/// Energy-profile fallback: contiguous runs of per-second buckets within
/// `ENERGY_SILENCE_BAND_DB` of the file's quietest level — but only when that
/// level is itself genuinely quiet. Coarse (one-second buckets), so a span
/// overlapping any word is dropped rather than risking clipped speech.
pub fn energy_silences(
    profile: &EnergyProfile,
    clip_start_ms: u64,
    clip_end_ms: u64,
) -> Vec<CutSpan> {
    let db = &profile.per_second_db;
    let floor = db.iter().cloned().fold(f32::INFINITY, f32::min);
    if db.len() < 2 || !floor.is_finite() || floor > ENERGY_SILENCE_FLOOR_DB {
        return Vec::new();
    }
    let clip_secs = clip_end_ms.div_ceil(1000) as usize;
    let mut spans = Vec::new();
    let mut run_start: Option<usize> = None;
    for (i, &v) in db.iter().enumerate().take(clip_secs) {
        let silent = v <= floor + ENERGY_SILENCE_BAND_DB;
        match (run_start, silent) {
            (None, true) => run_start = Some(i),
            (Some(s), false) => {
                spans.push(CutSpan {
                    start_ms: s as u64 * 1000,
                    end_ms: (i as u64) * 1000,
                });
                run_start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = run_start {
        spans.push(CutSpan {
            start_ms: s as u64 * 1000,
            end_ms: u64::MAX,
        });
    }
    spans
        .into_iter()
        .map(|s| CutSpan {
            start_ms: s.start_ms.max(clip_start_ms),
            end_ms: s.end_ms.min(clip_end_ms),
        })
        .filter(|s| s.end_ms > s.start_ms)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start_ms: u64, end_ms: u64) -> Word {
        Word {
            text: text.into(),
            start_ms,
            end_ms,
            p: 0.9,
        }
    }

    fn span(start_ms: u64, end_ms: u64) -> CutSpan {
        CutSpan { start_ms, end_ms }
    }

    // ---- filler detection ----

    #[test]
    fn filler_words_match_normalized_tokens() {
        assert!(is_filler("um"));
        assert!(is_filler("Uh,"));
        assert!(is_filler("ERM"));
        assert!(is_filler("uhh"));
        assert!(!is_filler("like")); // carries meaning too often to cut
        assert!(!is_filler("human")); // substring is not a match
        assert!(!is_filler(""));
    }

    #[test]
    fn plan_removes_filler_words_from_the_clip() {
        let words = vec![
            word("welcome", 0, 400),
            word("um", 400, 700),
            word("back", 700, 1000),
        ];
        let removals = build_plan(0, 10_000, &words, vec![]);
        assert_eq!(removals, vec![span(400, 700)]);
        let keeps = keeps_from_removals(0, 10_000, &removals);
        assert_eq!(keeps, vec![span(0, 400), span(700, 10_000)]);
        assert_eq!(keeps.iter().map(|k| k.len_ms()).sum::<u64>(), 10_000 - 300);
    }

    // ---- silencedetect parsing ----

    #[test]
    fn parses_silencedetect_log_and_offsets_to_clip_start() {
        let log = "[silencedetect @ 0x6000037] silence_start: 2.040\n\
                   [silencedetect @ 0x6000037] silence_end: 3.290 | silence_duration: 1.250\n\
                   [silencedetect @ 0x6000037] silence_start: 8.5\n";
        let spans = parse_silencedetect(log, 60_000);
        assert_eq!(
            spans,
            vec![
                span(62_040, 63_290),
                span(68_500, u64::MAX), // trailing silence: caller clamps
            ]
        );
    }

    #[test]
    fn silence_shorter_than_the_floor_or_only_padding_is_ignored() {
        let removals = build_plan(
            0,
            10_000,
            &[],
            vec![
                span(1000, 1400), // 400 ms < 500 ms floor: keep
                span(3000, 3500), // 500 ms → cut interior after padding
                span(5000, 5180), // 180 ms < floor: keep
            ],
        );
        assert_eq!(removals, vec![span(3100, 3400)]);
        assert_eq!(
            keeps_from_removals(0, 10_000, &removals),
            vec![span(0, 3100), span(3400, 10_000)]
        );
    }

    // ---- merging ----

    #[test]
    fn overlapping_and_wordless_sliver_removals_merge() {
        // Two fillers 60 ms apart with nothing said between → the sub-
        // sliver keep would flash, so they merge into one removal.
        let removals = build_plan(
            0,
            10_000,
            &[word("um", 1000, 1100), word("uh", 1160, 1300)],
            vec![],
        );
        assert_eq!(removals, vec![span(1000, 1300)]);
        // Overlapping filler + padded silence folds too.
        let removals = build_plan(0, 10_000, &[word("um", 1100, 1200)], vec![span(1000, 2000)]);
        assert_eq!(removals, vec![span(1100, 1900)]);
    }

    #[test]
    fn a_word_in_the_gap_keeps_the_cuts_apart() {
        // Same two fillers, but the word "I" is spoken inside the 60 ms
        // sliver — merging would cut real speech, so they stay separate.
        let removals = build_plan(
            0,
            10_000,
            &[
                word("um", 1000, 1100),
                word("I", 1120, 1150),
                word("uh", 1160, 1300),
            ],
            vec![],
        );
        assert_eq!(removals, vec![span(1000, 1100), span(1160, 1300)]);
        assert_eq!(
            keeps_from_removals(0, 10_000, &removals),
            vec![span(0, 1000), span(1100, 1160), span(1300, 10_000)]
        );
    }

    #[test]
    fn removals_covering_everything_fall_back_to_uncut() {
        let removals = build_plan(0, 10_000, &[word("um", 0, 10_000)], vec![]);
        assert!(removals.is_empty());
        assert_eq!(
            keeps_from_removals(0, 10_000, &removals),
            vec![span(0, 10_000)]
        );
    }

    #[test]
    fn removals_outside_the_clip_are_clamped_to_it() {
        let removals = build_plan(
            60_000,
            90_000,
            &[word("uh", 59_000, 60_500)],
            vec![span(55_000, 65_000)],
        );
        // Silence clamped to [60000,65000] pads to [60100,64900]; the filler
        // clipped at the edge [60000,60500] folds in → one merged removal.
        assert_eq!(removals, vec![span(60_000, 64_900)]);
        assert_eq!(
            keeps_from_removals(60_000, 90_000, &removals),
            vec![span(64_900, 90_000)]
        );
    }

    // ---- caption retiming ----

    #[test]
    fn retime_shifts_words_by_removed_time() {
        let removals = vec![span(1000, 2000)];
        let words = vec![
            word("a", 0, 500),      // fully before → unchanged
            word("um", 1200, 1600), // inside removal → dropped
            word("b", 2500, 3000),  // after → shifted -1000
            word("c", 500, 2500),   // spans removal → compresses to boundary
        ];
        let out = retime_words(&words, &removals);
        assert_eq!(out.len(), 3);
        assert_eq!((out[0].start_ms, out[0].end_ms), (0, 500));
        assert_eq!(out[0].text, "a");
        assert_eq!((out[1].start_ms, out[1].end_ms), (1500, 2000));
        assert_eq!(out[1].text, "b");
        // "c" is audible 500–1000 (500 ms) then 2000–2500 joins at the cut:
        // both edges of the removal map to output 1000, so 500–2500 → 500–1500.
        assert_eq!((out[2].start_ms, out[2].end_ms), (500, 1500));
        assert_eq!(out[2].text, "c");
    }

    #[test]
    fn retime_with_no_removals_is_identity() {
        let words = vec![word("a", 10, 20)];
        let out = retime_words(&words, &[]);
        assert_eq!(out[0].start_ms, 10);
        assert_eq!(out[0].end_ms, 20);
    }

    #[test]
    fn retime_drops_zero_length_survivors() {
        // A word exactly covering a removal collapses to a point → dropped.
        let removals = vec![span(100, 200)];
        let words = vec![word("um", 100, 200)];
        assert!(retime_words(&words, &removals).is_empty());
    }

    // ---- energy fallback ----

    #[test]
    fn energy_profile_finds_quiet_runs() {
        let mut db = vec![-20.0f32; 60];
        for v in db.iter_mut().skip(10).take(3) {
            *v = -55.0; // 3 s of near-silence at 10–13 s
        }
        let profile = EnergyProfile { per_second_db: db };
        let spans = energy_silences(&profile, 0, 60_000);
        assert_eq!(spans, vec![span(10_000, 13_000)]);
    }

    #[test]
    fn energy_profile_with_no_real_quiet_floor_reports_nothing() {
        // Quietest bucket is -20 dB — loud throughout; silence cannot be told
        // from speech at this granularity, so the fallback stays out.
        let profile = EnergyProfile {
            per_second_db: vec![-20.0f32; 30],
        };
        assert!(energy_silences(&profile, 0, 30_000).is_empty());
    }

    #[test]
    fn energy_spans_are_clipped_to_the_interval() {
        let mut db = vec![-20.0f32; 60];
        for v in db.iter_mut().skip(10).take(50) {
            *v = -55.0;
        }
        let profile = EnergyProfile { per_second_db: db };
        let spans = energy_silences(&profile, 30_000, 50_000);
        assert_eq!(spans, vec![span(30_000, 50_000)]);
    }

    #[test]
    fn retime_turns_shift_and_drop_with_removals() {
        use crate::domain::SpeakerTurn;
        let turns = vec![
            SpeakerTurn {
                start_ms: 10_000,
                end_ms: 20_000,
                speaker: 0,
            },
            SpeakerTurn {
                start_ms: 21_000,
                end_ms: 23_000,
                speaker: 1,
            },
            SpeakerTurn {
                start_ms: 25_000,
                end_ms: 30_000,
                speaker: 0,
            },
        ];
        let removals = vec![span(20_000, 24_000)];
        let out = retime_turns(&turns, &removals);
        // Turn 1 lands inside the removal → dropped; turn 3 shifts back 4 s.
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].start_ms, 21_000);
        assert_eq!(out[1].end_ms, 26_000);
        assert_eq!(out[1].speaker, 0);
        // No removals → identical copy.
        assert_eq!(retime_turns(&turns, &[]), turns);
    }
}
