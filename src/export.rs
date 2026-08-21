//! Text-artifact exports (SRT captions, transcript JSON, edit-decision
//! manifest). Pure builders here; the HTTP handlers in `api.rs` only load
//! persisted state and attach filenames.
//!
//! SRT cue grouping is deterministic: words accumulate into one cue until it
//! reaches 38 characters, 7 words, or a sentence-ending word. Timing comes
//! from the transcript's word offsets, never from the model's milliseconds.

use crate::domain::{Project, RenderManifest, Transcript, Word};
use chrono::Utc;
use serde_json::json;

/// Maximum characters accumulated before a cue must close.
const MAX_CUE_CHARS: usize = 38;
/// Maximum words per cue.
const MAX_CUE_WORDS: usize = 7;
/// Minimum on-screen duration for any cue, so flashes never render.
const MIN_CUE_MS: u64 = 500;

/// Format a millisecond offset as an SRT timestamp `HH:MM:SS,mmm`.
pub fn srt_timestamp(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let mmm = ms % 1000;
    format!("{h:02}:{m:02}:{s:02},{mmm:03}")
}

fn ends_sentence(text: &str) -> bool {
    text.ends_with(['.', '!', '?'])
}

/// Build the SRT subtitle file for one clip. `words` are the clip's word
/// interval; timings are rebased to the clip start and clamped to its span.
pub fn clip_srt(words: &[Word], clip_start_ms: u64, clip_end_ms: u64) -> String {
    let span = clip_end_ms.saturating_sub(clip_start_ms);
    let mut out = String::new();
    let mut index = 1usize;

    let mut cue_words: Vec<&Word> = Vec::new();
    for word in words {
        cue_words.push(word);
        let chars: usize = cue_words.iter().map(|w| w.text.chars().count() + 1).sum();
        if chars >= MAX_CUE_CHARS || cue_words.len() >= MAX_CUE_WORDS || ends_sentence(&word.text) {
            push_cue(&mut out, &mut index, &cue_words, clip_start_ms, span);
            cue_words.clear();
        }
    }
    push_cue(&mut out, &mut index, &cue_words, clip_start_ms, span);
    out
}

/// Append one numbered cue block and advance `index`.
fn push_cue(out: &mut String, index: &mut usize, cue: &[&Word], clip_start_ms: u64, span: u64) {
    if cue.is_empty() {
        return;
    }
    let start = cue[0].start_ms.saturating_sub(clip_start_ms);
    let end = cue[cue.len() - 1].end_ms.saturating_sub(clip_start_ms);
    let end = end.clamp(start + MIN_CUE_MS, span.max(start + MIN_CUE_MS));
    let text = cue
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    out.push_str(&index.to_string());
    out.push('\n');
    out.push_str(&format!(
        "{} --> {}\n",
        srt_timestamp(start),
        srt_timestamp(end)
    ));
    out.push_str(&text);
    out.push_str("\n\n");
    *index += 1;
}

/// The full transcript as pretty-printed JSON.
pub fn transcript_json(t: &Transcript) -> String {
    serde_json::to_string_pretty(t).unwrap_or_else(|_| "{}".to_string())
}

/// The edit-decision manifest: which source intervals became clips, with the
/// presentation choices burned into each render. Machine-readable handoff for
/// other tools.
pub fn edl_manifest(p: &Project, m: &RenderManifest) -> String {
    let clips: Vec<serde_json::Value> = m
        .clips
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "rank": c.rank,
                "headline": c.headline,
                "filename": c.filename,
                "start_ms": c.start_ms,
                "end_ms": c.end_ms,
                "duration_ms": c.duration_ms,
                "layout": c.layout.label(),
                "caption_style": c.caption_style,
                "accent_color": c.accent_color,
            })
        })
        .collect();
    let doc = json!({
        "project_id": p.id,
        "source_filename": p.source.as_ref().map(|s| s.filename.clone()),
        "generated_at": Utc::now().to_rfc3339(),
        "clips": clips,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::LayoutPlan;

    fn word(text: &str, start_ms: u64, end_ms: u64) -> Word {
        Word {
            text: text.to_string(),
            start_ms,
            end_ms,
            p: 0.9,
        }
    }

    #[test]
    fn srt_timestamp_formats_hours_rollover() {
        assert_eq!(srt_timestamp(0), "00:00:00,000");
        assert_eq!(srt_timestamp(901_234), "00:15:01,234");
        assert_eq!(srt_timestamp(3_600_000), "01:00:00,000");
        assert_eq!(srt_timestamp(3_661_001), "01:01:01,001");
    }

    #[test]
    fn cues_close_on_sentence_punctuation() {
        let words = vec![
            word("Hello", 0, 300),
            word("world.", 300, 700),
            word("More", 800, 1_000),
            word("words", 1_000, 1_400),
        ];
        let srt = clip_srt(&words, 0, 5_000);
        // First cue closes after "world."; second holds the remainder.
        assert_eq!(srt.matches("-->").count(), 2);
        assert!(srt.contains("00:00:00,000 --> "));
        assert!(srt.contains("Hello world."));
        assert!(srt.contains("More words"));
    }

    #[test]
    fn cue_timings_rebase_to_clip_start_and_stay_inside_the_span() {
        let words = vec![
            word("One", 10_000, 10_200),
            word("two", 10_200, 10_500),
            word("three.", 10_500, 11_000),
        ];
        let srt = clip_srt(&words, 10_000, 12_000);
        assert!(
            srt.contains("00:00:00,000 --> 00:00:01,000"),
            "rebased to clip start: {srt}"
        );
        // Nothing beyond clip_end_ms - clip_start_ms may appear.
        assert!(!srt.contains("00:00:02,"));
    }

    #[test]
    fn short_cues_are_held_on_screen_for_at_least_half_a_second() {
        let words = vec![word("Yes.", 1_000, 1_050)];
        let srt = clip_srt(&words, 0, 9_000);
        assert!(
            srt.contains("00:00:01,000 --> 00:00:01,500"),
            "end clamped to start + 500ms: {srt}"
        );
    }

    #[test]
    fn empty_word_list_produces_an_empty_srt() {
        assert_eq!(clip_srt(&[], 0, 1_000), "");
    }

    #[test]
    fn edl_manifest_carries_project_and_clip_fields() {
        let mut p = Project::new("proj-1".into(), "/tmp/src.mp4".into());
        p.source = Some(crate::domain::SourceInfo {
            filename: "src.mp4".into(),
            duration_ms: 60_000,
            width: 1920,
            height: 1080,
            fps: 30.0,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            size_bytes: 1,
        });
        let manifest = RenderManifest {
            clips: vec![crate::domain::ClipRecord {
                id: "clip-1".into(),
                rank: 1,
                headline: "A strong take".into(),
                filename: "01-a-strong-take.mp4".into(),
                start_ms: 1_000,
                end_ms: 31_000,
                duration_ms: 30_000,
                selection_reason: String::new(),
                scores: Default::default(),
                layout: LayoutPlan::BlurPad,
                status: crate::domain::ClipStatus::Ready,
                error: None,
                low_confidence: false,
                caption_style: Some("impact".into()),
                accent_color: Some("#FFDD00".into()),
                caption_font: None,
                caption_text: None,
            }],
            output_dir: None,
        };
        let doc: serde_json::Value = serde_json::from_str(&edl_manifest(&p, &manifest)).unwrap();
        assert_eq!(doc["project_id"], "proj-1");
        assert_eq!(doc["source_filename"], "src.mp4");
        assert!(doc["generated_at"].as_str().is_some());
        let clip = &doc["clips"][0];
        assert_eq!(clip["id"], "clip-1");
        assert_eq!(clip["start_ms"], 1_000);
        assert_eq!(clip["duration_ms"], 30_000);
        assert_eq!(clip["layout"], "blur_pad");
        assert_eq!(clip["caption_style"], "impact");
    }

    #[test]
    fn transcript_json_is_pretty_printed_json() {
        let t = Transcript {
            language: "en".into(),
            words: vec![word("hello", 250, 900)],
            sentences: Vec::new(),
            avg_confidence: 0.9,
        };
        let out = transcript_json(&t);
        let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(doc["language"], "en");
        assert!(out.contains("\n  "), "pretty-printed: {out}");
    }
}
