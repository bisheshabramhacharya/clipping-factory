//! Export pack (spec #54, ticket #60): every rendered Clip gets three
//! sidecars next to its MP4 — `{clip}.srt`, `{clip}.vtt`, and
//! `{clip}.meta.json` — so posting a clip is copy-paste, not a blank
//! caption box. Deliberately files, not feeds: nothing auto-publishes.
//!
//! - `.srt` / `.vtt` carry the caption words the viewer actually sees
//!   (including any `caption_text` edit), timed to the clip's own clock.
//! - `.meta.json` carries title/description/hashtags plus the clip's
//!   timing and Source provenance. Copy is written by the configured AI
//!   provider when one is connected; anything else — offline tier, missing
//!   key, provider error — falls back to a deterministic template.

use crate::captions::{is_stopword, paginate, with_caption_text, words_in_interval};
use crate::domain::{fmt_ms, ClipRecord, SourceInfo, Transcript, Word};
use crate::settings::{AiSettings, Provider};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Sidecar filenames
// ---------------------------------------------------------------------------

fn stem(clip_filename: &str) -> &str {
    clip_filename.strip_suffix(".mp4").unwrap_or(clip_filename)
}

pub fn srt_name(clip_filename: &str) -> String {
    format!("{}.srt", stem(clip_filename))
}
pub fn vtt_name(clip_filename: &str) -> String {
    format!("{}.vtt", stem(clip_filename))
}
pub fn meta_name(clip_filename: &str) -> String {
    format!("{}.meta.json", stem(clip_filename))
}
pub fn meta_path(dir: &Path, clip_filename: &str) -> PathBuf {
    dir.join(meta_name(clip_filename))
}

/// The filename stem shared by a clip's MP4 and its export sidecars — used by
/// the retry sweep to keep or drop the whole pack as one unit.
pub fn pack_stem(name: &str) -> Option<&str> {
    name.strip_suffix(".mp4")
        .or_else(|| name.strip_suffix(".srt"))
        .or_else(|| name.strip_suffix(".vtt"))
        .or_else(|| name.strip_suffix(".meta.json"))
}

// ---------------------------------------------------------------------------
// Caption sidecars (.srt / .vtt)
// ---------------------------------------------------------------------------

/// One subtitle cue: clip-relative timing plus plain spoken text.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Group the clip's words into subtitle cues on the clip's own clock. Word
/// grouping mirrors the caption pagination, and each cue breathes into the
/// gap before the next one — capped by the next cue's start and the clip end.
pub fn caption_cues(words: &[Word], clip_start_ms: u64, clip_end_ms: u64) -> Vec<Cue> {
    let rel: Vec<Word> = words
        .iter()
        .map(|w| Word {
            text: w.text.clone(),
            start_ms: w.start_ms.saturating_sub(clip_start_ms),
            end_ms: w.end_ms.saturating_sub(clip_start_ms),
            p: w.p,
        })
        .collect();
    let clip_len = clip_end_ms.saturating_sub(clip_start_ms);
    let pages = paginate(&rel);
    let mut cues = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let (Some(first), Some(last)) = (page.first(), page.last()) else {
            continue;
        };
        let next_start = pages
            .get(i + 1)
            .and_then(|p| p.first())
            .map(|w| w.start_ms)
            .unwrap_or(u64::MAX);
        let start = first.start_ms;
        let end = (last.end_ms + 200)
            .min(next_start)
            .min(clip_len.max(last.end_ms));
        if end <= start {
            continue;
        }
        let text = page
            .iter()
            .map(|w| clean_text(&w.text))
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        cues.push(Cue {
            start_ms: start,
            end_ms: end,
            text,
        });
    }
    cues
}

/// Subtitle files are plaintext: flatten anything that would break a cue line.
fn clean_text(s: &str) -> String {
    s.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The clip's caption words as burned into the MP4 — transcript words inside
/// the interval with any `caption_text` edit applied on the original timings.
pub fn caption_words(transcript: &Transcript, clip: &ClipRecord) -> Vec<Word> {
    with_caption_text(
        &words_in_interval(&transcript.words, clip.start_ms, clip.end_ms),
        clip.caption_text.as_deref(),
    )
}

fn srt_ts(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn vtt_ts(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            srt_ts(cue.start_ms),
            srt_ts(cue.end_ms),
            cue.text
        ));
    }
    out
}

pub fn to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            vtt_ts(cue.start_ms),
            vtt_ts(cue.end_ms),
            cue.text
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// meta.json — posting copy plus provenance
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct ClipTiming {
    pub filename: String,
    pub rank: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub duration_ms: u64,
    /// Human-readable position inside the Source, e.g. "12:34".
    pub start: String,
    pub end: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct SourceRef {
    pub filename: String,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
}

/// `{clip}.meta.json` — the copy a creator pastes when posting, plus where
/// the clip came from.
#[derive(Serialize, Clone, Debug)]
pub struct ClipMeta {
    pub title: String,
    pub description: String,
    /// Post-ready hashtags, each prefixed with `#`.
    pub hashtags: Vec<String>,
    /// What wrote the copy: the provider label (`openai · gpt-4o-mini`,
    /// `anthropic · …`) or `"template"` when no AI provider wrote it.
    pub generated_by: String,
    pub clip: ClipTiming,
    pub source: SourceRef,
    pub layout: String,
    pub caption_style: Option<String>,
    /// How the clip was chosen ("local ranking", "openai · gpt-4o-mini",
    /// "caption-only") — provenance for the posted excerpt.
    pub selector: Option<String>,
    pub project_id: String,
    pub generated_at: DateTime<Utc>,
}

/// Everything metadata generation needs, bundled so pipeline and restyle
/// callers share one shape.
pub struct MetaInput<'a> {
    pub clip: &'a ClipRecord,
    pub source: &'a SourceInfo,
    /// Caption words as burned (see [`caption_words`]); the template and the
    /// provider prompt both describe what the viewer reads.
    pub words: &'a [Word],
    pub project_id: &'a str,
    pub selector: Option<&'a str>,
    pub caption_style: Option<&'a str>,
}

/// Title/description/hashtags for one clip. The configured provider writes
/// the copy when it can; otherwise the deterministic template does. This
/// never fails and never throws — a clip render is never hostage to a
/// metadata call.
pub async fn clip_metadata(
    settings: &AiSettings,
    input: &MetaInput<'_>,
    cancel: &CancellationToken,
) -> ClipMeta {
    let fallback = template_copy(input);
    let (title, description, hashtags, generated_by) = match ai_copy(settings, input, cancel).await
    {
        Some(ai) => (ai.0, ai.1, ai.2, ai.3),
        None => (fallback.0, fallback.1, fallback.2, "template".to_string()),
    };
    ClipMeta {
        title,
        description,
        hashtags,
        generated_by,
        clip: ClipTiming {
            filename: input.clip.filename.clone(),
            rank: input.clip.rank,
            start_ms: input.clip.start_ms,
            end_ms: input.clip.end_ms,
            duration_ms: input.clip.duration_ms,
            start: fmt_ms(input.clip.start_ms),
            end: fmt_ms(input.clip.end_ms),
        },
        source: SourceRef {
            filename: input.source.filename.clone(),
            duration_ms: input.source.duration_ms,
            width: input.source.width,
            height: input.source.height,
        },
        layout: input.clip.layout.label().to_string(),
        caption_style: input
            .caption_style
            .map(str::to_string)
            .or_else(|| input.clip.caption_style.clone()),
        selector: input.selector.map(str::to_string),
        project_id: input.project_id.to_string(),
        generated_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Provider-generated copy (with per-field fallback to the template)
// ---------------------------------------------------------------------------

const META_SYSTEM: &str = r#"You write the posting copy for one short vertical clip cut from a podcast. Return ONLY a JSON object, no markdown fences:
{"title":"…","description":"…","hashtags":["tag","tag",…]}
Rules:
- title: under 90 characters, sentence case, no surrounding quotes, faithful to what is actually said.
- description: 1–3 plain sentences a creator can paste under the video. No hashtags inside it, no invented links.
- hashtags: 3–8 lowercase tags, letters/digits/underscore only, no '#' prefix.
- Never invent claims, numbers, names, or emotion that is not in the transcript."#;

fn meta_prompt(input: &MetaInput<'_>) -> String {
    let clip = input.clip;
    let spoken = input
        .words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "Source: \"{}\" · this clip runs {}–{} ({} ms) · selected because: {}\nHeadline: \"{}\"\n\nTranscript:\n{}\n\nReturn only the JSON object.",
        input.source.filename,
        fmt_ms(clip.start_ms),
        fmt_ms(clip.end_ms),
        clip.duration_ms,
        if clip.selection_reason.trim().is_empty() {
            "n/a"
        } else {
            clip.selection_reason.trim()
        },
        clip.headline.trim(),
        spoken
    )
}

/// The configured provider writes copy when it is connected and is a real
/// text model — anything else returns None so the template takes over. The
/// offline tier has no text model by definition.
async fn ai_copy(
    settings: &AiSettings,
    input: &MetaInput<'_>,
    cancel: &CancellationToken,
) -> Option<(String, String, Vec<String>, String)> {
    if cancel.is_cancelled() || !settings.connected() {
        return None;
    }
    let provider = Provider::parse(&settings.provider)?;
    let model = settings.effective_model();
    let prompt = meta_prompt(input);
    let raw = match provider {
        Provider::OpenAi => {
            let key = settings
                .api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())?;
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return None,
                r = crate::select::openai::complete(key, &model, META_SYSTEM, &prompt) => r,
            }
        }
        Provider::Anthropic => {
            let key = settings
                .api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())?;
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return None,
                r = crate::select::anthropic::complete(key, &model, META_SYSTEM, &prompt) => r,
            }
        }
        Provider::Local => {
            let base = settings.effective_base_url();
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return None,
                r = crate::select::local::complete(&base, &model, META_SYSTEM, &prompt) => r,
            }
        }
        Provider::Offline => return None,
    };
    let parsed = raw
        .and_then(|text| parse_meta(&text))
        .map_err(|e| {
            tracing::warn!(
                "clip metadata via {} failed, using template: {e:#}",
                provider.as_str()
            );
            e
        })
        .ok()?;

    // Merge field by field: a thin provider answer never erases the
    // deterministic value underneath it.
    let mut meta = template_copy(input);
    if let Some(title) = clean_sentence(&parsed.title, 90) {
        meta.0 = title;
    }
    if let Some(description) = clean_sentence(&parsed.description, 600) {
        meta.1 = description;
    }
    let tags = clean_hashtags(&parsed.hashtags);
    if !tags.is_empty() {
        meta.2 = tags;
    }
    Some((
        meta.0,
        meta.1,
        meta.2,
        format!("{} · {}", provider.as_str(), model),
    ))
}

#[derive(serde::Deserialize, Default)]
struct MetaIn {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    hashtags: Vec<String>,
}

/// Same lenient envelope as the selector's parser: tolerate code fences and
/// prose around the JSON object.
fn parse_meta(raw: &str) -> Result<MetaIn> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let start = cleaned.find('{');
    let end = cleaned.rfind('}');
    let json = match (start, end) {
        (Some(s), Some(e)) if e > s => &cleaned[s..=e],
        _ => return Err(anyhow!("provider returned no JSON object")),
    };
    serde_json::from_str(json).map_err(|e| anyhow!("provider returned malformed JSON ({e})"))
}

/// A one-line field: trimmed, unwrapped from quotes, hard-capped at a word
/// boundary so provider output can never blow up a caption box.
fn clean_sentence(s: &str, max_chars: usize) -> Option<String> {
    let t = s.trim().trim_matches('"').trim().replace(['\r', '\n'], " ");
    if t.is_empty() {
        return None;
    }
    Some(truncate_words(&t, max_chars))
}

fn truncate_words(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let mut cut = s[..max_chars].to_string();
    if let Some(space) = cut.rfind(' ') {
        cut.truncate(space);
    }
    format!("{}…", cut.trim_end())
}

/// Normalize provider/model tags into post-ready `#tag` strings.
fn clean_hashtags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let cleaned: String = tag
            .trim()
            .trim_start_matches('#')
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if cleaned.len() >= 2 && !out.iter().any(|t| t == &cleaned) {
            out.push(cleaned);
        }
        if out.len() >= 8 {
            break;
        }
    }
    out.iter().map(|t| format!("#{t}")).collect()
}

// ---------------------------------------------------------------------------
// Deterministic template copy
// ---------------------------------------------------------------------------

/// Copy built entirely from what's already on disk: the validator's
/// headline, the spoken excerpt, and the clip's timing in the Source.
fn template_copy(input: &MetaInput<'_>) -> (String, String, Vec<String>) {
    let clip = input.clip;
    let spoken: Vec<&str> = input
        .words
        .iter()
        .map(|w| w.text.trim())
        .filter(|t| !t.is_empty())
        .collect();

    let title = if !clip.headline.trim().is_empty() {
        truncate_words(clip.headline.trim(), 90)
    } else {
        let hook = spoken.iter().take(8).copied().collect::<Vec<_>>().join(" ");
        if hook.is_empty() {
            format!("Clip {}", clip.rank)
        } else {
            truncate_words(&hook, 90)
        }
    };

    let excerpt = truncate_words(&spoken.join(" "), 220);
    let provenance = format!(
        "Clip from \"{}\" · {}–{}.",
        input.source.filename,
        fmt_ms(clip.start_ms),
        fmt_ms(clip.end_ms)
    );
    let description = if excerpt.is_empty() {
        provenance
    } else {
        format!("{excerpt}\n\n{provenance}")
    };

    (title, description, keyword_tags(input))
}

/// Deterministic hashtags from the words the clip actually says: content
/// words (skip stopwords, ≥4 letters), most frequent first, ties in spoken
/// order.
fn keyword_tags(input: &MetaInput<'_>) -> Vec<String> {
    let mut counts: HashMap<String, (u32, usize)> = HashMap::new();
    let mut order = 0usize;
    let mut feed = |text: &str| {
        for token in text.split_whitespace() {
            let word: String = token
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .flat_map(|c| c.to_lowercase())
                .collect();
            if word.len() < 4 || is_stopword(token) {
                order += 1;
                continue;
            }
            let entry = counts.entry(word).or_insert((0, order));
            entry.0 += 1;
            order += 1;
        }
    };
    feed(&input.clip.headline);
    for w in input.words {
        feed(&w.text);
    }
    let mut ranked: Vec<(&String, &(u32, usize))> = counts.iter().collect();
    ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.1 .1.cmp(&b.1 .1)));
    let tags: Vec<String> = ranked
        .into_iter()
        .take(6)
        .map(|(tag, _)| format!("#{tag}"))
        .collect();
    if tags.is_empty() {
        vec!["#podcast".into(), "#clips".into()]
    } else {
        tags
    }
}

// ---------------------------------------------------------------------------
// Writers
// ---------------------------------------------------------------------------

/// Write the full pack (srt + vtt + meta.json) next to the clip's MP4 in
/// `dir` — the clips dir inside the project, where the render stage lands it.
pub async fn write_export_pack(
    dir: &Path,
    input: &MetaInput<'_>,
    settings: &AiSettings,
    cancel: &CancellationToken,
) -> Result<()> {
    let cues = caption_cues(input.words, input.clip.start_ms, input.clip.end_ms);
    let meta = clip_metadata(settings, input, cancel).await;
    write_all(dir, &input.clip.filename, &cues, Some(&meta)).await
}

/// Rewrite just the caption sidecars — used after a restyle, when the burned
/// words may have changed but the posting copy has not.
pub async fn write_caption_files(
    dir: &Path,
    clip_filename: &str,
    words: &[Word],
    clip_start_ms: u64,
    clip_end_ms: u64,
) -> Result<()> {
    let cues = caption_cues(words, clip_start_ms, clip_end_ms);
    write_all(dir, clip_filename, &cues, None).await
}

/// Backfill whichever sidecars are missing for an already-rendered clip —
/// e.g. clips rendered before export packs existed, resumed on retry.
pub async fn ensure_export_pack(
    dir: &Path,
    input: &MetaInput<'_>,
    settings: &AiSettings,
    cancel: &CancellationToken,
) -> Result<()> {
    let filename = &input.clip.filename;
    let need_srt = !dir.join(srt_name(filename)).is_file();
    let need_vtt = !dir.join(vtt_name(filename)).is_file();
    let need_meta = !meta_path(dir, filename).is_file();
    if !need_srt && !need_vtt && !need_meta {
        return Ok(());
    }
    let cues = caption_cues(input.words, input.clip.start_ms, input.clip.end_ms);
    if need_srt {
        crate::util::atomic_write_bytes(&dir.join(srt_name(filename)), to_srt(&cues).as_bytes())
            .await?;
    }
    if need_vtt {
        crate::util::atomic_write_bytes(&dir.join(vtt_name(filename)), to_vtt(&cues).as_bytes())
            .await?;
    }
    if need_meta {
        write_meta_file(dir, input, settings, cancel).await?;
    }
    Ok(())
}

/// Write only `{clip}.meta.json`.
pub async fn write_meta_file(
    dir: &Path,
    input: &MetaInput<'_>,
    settings: &AiSettings,
    cancel: &CancellationToken,
) -> Result<()> {
    let meta = clip_metadata(settings, input, cancel).await;
    crate::util::atomic_write_json(&meta_path(dir, &input.clip.filename), &meta).await
}

async fn write_all(
    dir: &Path,
    clip_filename: &str,
    cues: &[Cue],
    meta: Option<&ClipMeta>,
) -> Result<()> {
    crate::util::atomic_write_bytes(&dir.join(srt_name(clip_filename)), to_srt(cues).as_bytes())
        .await?;
    crate::util::atomic_write_bytes(&dir.join(vtt_name(clip_filename)), to_vtt(cues).as_bytes())
        .await?;
    if let Some(meta) = meta {
        crate::util::atomic_write_json(&meta_path(dir, clip_filename), meta).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LayoutPlan, Scores};
    use crate::settings::PROVIDER_OFFLINE;

    fn words(s: &str, step: u64) -> Vec<Word> {
        s.split_whitespace()
            .enumerate()
            .map(|(i, t)| Word {
                text: t.into(),
                start_ms: i as u64 * step,
                end_ms: i as u64 * step + 280,
                p: 0.9,
            })
            .collect()
    }

    fn clip() -> ClipRecord {
        ClipRecord {
            id: "clip1".into(),
            rank: 1,
            headline: "Why discipline beats motivation".into(),
            filename: "01-why-discipline.mp4".into(),
            start_ms: 60_000,
            end_ms: 90_000,
            duration_ms: 30_000,
            selection_reason: "self-contained reveal".into(),
            scores: Scores::default(),
            layout: LayoutPlan::BlurPad,
            status: crate::domain::ClipStatus::Ready,
            error: None,
            low_confidence: false,
            caption_style: Some("impact".into()),
            accent_color: None,
            caption_font: None,
            caption_text: None,
            emoji_overlay: None,
            width: Some(608),
            height: Some(1080),
            auto_cut: false,
            cut_spans: None,
            zoom_cuts: false,
            zoom_keys: None,
            end_card: false,
            score: None,
        }
    }

    fn source() -> SourceInfo {
        SourceInfo {
            filename: "episode.mp4".into(),
            duration_ms: 3_600_000,
            width: 1920,
            height: 1080,
            fps: 30.0,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            size_bytes: 1,
            scene_boundaries_ms: Vec::new(),
        }
    }

    #[test]
    fn sidecar_names_share_the_clip_stem() {
        assert_eq!(srt_name("01-x.mp4"), "01-x.srt");
        assert_eq!(vtt_name("01-x.mp4"), "01-x.vtt");
        assert_eq!(meta_name("01-x.mp4"), "01-x.meta.json");
        assert_eq!(srt_name("noext"), "noext.srt");
        assert_eq!(pack_stem("01-x.meta.json"), Some("01-x"));
        assert_eq!(pack_stem("01-x.srt"), Some("01-x"));
        assert_eq!(pack_stem("other.ready"), None);
    }

    #[test]
    fn cues_are_clip_relative_and_breathe_into_gaps() {
        // Two pages: "…pause." ends, then a >700ms gap to "after".
        let mut ws = words("before pause", 350);
        ws.push(Word {
            text: "after".into(),
            start_ms: 4000,
            end_ms: 4300,
            p: 0.9,
        });
        let cues = caption_cues(&ws, 100, 10_000);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 0);
        // First cue breathes 200ms past the last word, capped by the next cue.
        assert_eq!(cues[0].end_ms, 730);
        assert_eq!(cues[1].start_ms, 3900);
        // Last cue is capped by the clip end.
        assert!(cues[1].end_ms <= 10_000);
        assert_eq!(cues[0].text, "before pause");
        assert_eq!(cues[1].text, "after");
    }

    #[test]
    fn srt_and_vtt_render_standard_shapes() {
        let cues = vec![
            Cue {
                start_ms: 0,
                end_ms: 1500,
                text: "hello world".into(),
            },
            Cue {
                start_ms: 61_230,
                end_ms: 62_000,
                text: "second line".into(),
            },
        ];
        let srt = to_srt(&cues);
        assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:01,500\nhello world\n"));
        assert!(srt.contains("2\n00:01:01,230 --> 00:01:02,000\nsecond line\n"));
        let vtt = to_vtt(&cues);
        assert!(vtt.starts_with("WEBVTT\n\n"));
        assert!(vtt.contains("00:00:00.000 --> 00:00:01.500\nhello world\n"));
        assert!(!vtt.contains(','));
    }

    #[test]
    fn empty_caption_text_yields_header_only_files() {
        let cues = caption_cues(&[], 0, 30_000);
        assert!(cues.is_empty());
        assert_eq!(to_vtt(&cues), "WEBVTT\n\n");
        assert_eq!(to_srt(&cues), "");
    }

    fn offline_settings() -> AiSettings {
        AiSettings {
            provider: PROVIDER_OFFLINE.into(),
            model: String::new(),
            api_key: None,
            base_url: String::new(),
        }
    }

    #[tokio::test]
    async fn template_meta_carries_copy_and_provenance() {
        let clip = clip();
        let source = source();
        let ws = words(
            "discipline beats motivation because motivation fades every single evening",
            300,
        );
        let input = MetaInput {
            clip: &clip,
            source: &source,
            words: &ws,
            project_id: "proj1",
            selector: Some("local ranking"),
            caption_style: Some("impact"),
        };
        let meta = clip_metadata(&offline_settings(), &input, &CancellationToken::new()).await;
        assert_eq!(meta.generated_by, "template");
        assert_eq!(meta.title, "Why discipline beats motivation");
        assert!(meta.description.contains("discipline beats motivation"));
        assert!(meta.description.contains("episode.mp4"));
        assert!(meta.description.contains("01:00–01:30"));
        assert!(meta.hashtags.iter().all(|t| t.starts_with('#')));
        assert!(meta.hashtags.contains(&"#discipline".to_string()));
        assert_eq!(meta.clip.start_ms, 60_000);
        assert_eq!(meta.clip.start, "01:00");
        assert_eq!(meta.clip.filename, "01-why-discipline.mp4");
        assert_eq!(meta.source.filename, "episode.mp4");
        assert_eq!(meta.source.width, 1920);
        assert_eq!(meta.layout, "blur_pad");
        assert_eq!(meta.selector.as_deref(), Some("local ranking"));
        assert_eq!(meta.project_id, "proj1");
    }

    #[tokio::test]
    async fn unconnected_openai_settings_still_get_template_copy() {
        let settings = AiSettings {
            provider: "openai".into(),
            model: String::new(),
            api_key: None, // configured provider, no key — not connected
            base_url: String::new(),
        };
        let clip = clip();
        let source = source();
        let ws = words("hello there", 300);
        let input = MetaInput {
            clip: &clip,
            source: &source,
            words: &ws,
            project_id: "p",
            selector: None,
            caption_style: None,
        };
        let meta = clip_metadata(&settings, &input, &CancellationToken::new()).await;
        assert_eq!(meta.generated_by, "template");
        assert!(!meta.title.is_empty());
    }

    #[test]
    fn parse_meta_tolerates_fences_and_prose() {
        let raw = "Here you go!\n```json\n{\"title\":\"T\",\"description\":\"D\",\"hashtags\":[\"#One\",\"two\"]}\n```\nThanks";
        let m = parse_meta(raw).unwrap();
        assert_eq!(m.title, "T");
        assert_eq!(m.hashtags, vec!["#One", "two"]);
        assert!(parse_meta("no json").is_err());
    }

    #[test]
    fn hashtag_cleanup_normalizes_and_dedupes() {
        let tags = clean_hashtags(&[
            "#Podcast".into(),
            "pod-cast!".into(),
            "podcast".into(),
            "a".into(),
            "Growth2024".into(),
        ]);
        assert_eq!(tags, vec!["#podcast", "#growth2024"]);
    }

    #[test]
    fn template_title_falls_back_to_spoken_hook() {
        let mut clip = clip();
        clip.headline = "   ".into();
        let source = source();
        let ws = words("the real trick is designing the environment once", 300);
        let input = MetaInput {
            clip: &clip,
            source: &source,
            words: &ws,
            project_id: "p",
            selector: None,
            caption_style: None,
        };
        let (title, _, _) = template_copy(&input);
        assert_eq!(title, "the real trick is designing the environment once");
    }

    #[test]
    fn keyword_tags_rank_content_words_first() {
        let clip = clip();
        let source = source();
        let ws = words(
            "discipline discipline discipline and the rest is just noise",
            300,
        );
        let input = MetaInput {
            clip: &clip,
            source: &source,
            words: &ws,
            project_id: "p",
            selector: None,
            caption_style: None,
        };
        let (_, _, tags) = template_copy(&input);
        assert_eq!(tags.first().map(String::as_str), Some("#discipline"));
        // Stopwords and short tokens never become tags.
        assert!(tags
            .iter()
            .all(|t| t != "#the" && t != "#and" && t != "#is"));
    }

    #[tokio::test]
    async fn export_pack_writes_all_three_sidecars() {
        let tmp = std::env::temp_dir().join(format!("cf-export-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        let clip = clip();
        let source = source();
        let ws = words("some words said here", 300);
        let input = MetaInput {
            clip: &clip,
            source: &source,
            words: &ws,
            project_id: "p",
            selector: None,
            caption_style: None,
        };
        write_export_pack(&tmp, &input, &offline_settings(), &CancellationToken::new())
            .await
            .unwrap();
        assert!(tmp.join("01-why-discipline.srt").is_file());
        assert!(tmp.join("01-why-discipline.vtt").is_file());
        let meta_bytes = tokio::fs::read(tmp.join("01-why-discipline.meta.json"))
            .await
            .unwrap();
        let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).unwrap();
        assert_eq!(meta["generated_by"], "template");
        assert_eq!(meta["clip"]["filename"], "01-why-discipline.mp4");

        // ensure_export_pack fills only what's missing.
        tokio::fs::remove_file(tmp.join("01-why-discipline.srt"))
            .await
            .unwrap();
        ensure_export_pack(&tmp, &input, &offline_settings(), &CancellationToken::new())
            .await
            .unwrap();
        assert!(tmp.join("01-why-discipline.srt").is_file());
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }
}
