//! Caption generation — four house styles, rendered by libass via ffmpeg:
//!
//! - **Impact** (default): kinetic stacked lockups. Each spoken phrase becomes
//!   a tight, ragged stack of words at different sizes — connective words
//!   small, the key word HUGE in caps — popping in mid-frame, with the
//!   currently spoken word tinted. The short-form-native look.
//! - **Clean**: the original restrained PRD §11.3 treatment — 3–7 word groups
//!   in the lower safe area, one accent color on the active word.
//! - **Pop**: one spoken word at a time, dead center, popping in on a scale
//!   transform. Keyword words render uppercase in the accent color.
//! - **Cinema**: a minimal lower-third line — the whole page fades in
//!   letterspaced lowercase; only the keyword carries the accent color.

use crate::domain::{Diarization, Word};
use crate::render::{OUT_H, OUT_W};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptionStyle {
    Impact,
    Clean,
    Pop,
    Cinema,
}

/// Style labels accepted by the API and surfaced to the UI pickers.
pub const CAPTION_STYLES: [&str; 4] = ["impact", "clean", "pop", "cinema"];

impl CaptionStyle {
    pub fn from_str(s: &str) -> CaptionStyle {
        match s.trim().to_lowercase().as_str() {
            "clean" | "minimal" => CaptionStyle::Clean,
            "pop" | "bounce" => CaptionStyle::Pop,
            "cinema" | "cinematic" => CaptionStyle::Cinema,
            _ => CaptionStyle::Impact,
        }
    }
    /// Strict parse for API input: unknown names are an error, not a default.
    pub fn parse_strict(s: &str) -> Option<CaptionStyle> {
        match s.trim().to_lowercase().as_str() {
            "impact" => Some(CaptionStyle::Impact),
            "clean" => Some(CaptionStyle::Clean),
            "pop" => Some(CaptionStyle::Pop),
            "cinema" => Some(CaptionStyle::Cinema),
            _ => None,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            CaptionStyle::Impact => "impact",
            CaptionStyle::Clean => "clean",
            CaptionStyle::Pop => "pop",
            CaptionStyle::Cinema => "cinema",
        }
    }
    /// True when the style shouts its display text in caps (Impact's
    /// emphasis word, Pop's keywords) — the hook title matches that voice;
    /// Clean and Cinema keep the headline's own casing.
    pub fn uses_caps(&self) -> bool {
        matches!(self, CaptionStyle::Impact | CaptionStyle::Pop)
    }
    /// The display-face name this style writes for `family` — Impact and
    /// Pop wear Inter's ExtraBold face; the rest name the family itself.
    /// Used when a hook title must resolve a font name without a file.
    pub fn face<'a>(&self, family: &'a str) -> &'a str {
        match self {
            CaptionStyle::Impact | CaptionStyle::Pop if family == "Inter" => "Inter ExtraBold",
            _ => family,
        }
    }
}

/// Curated for caption legibility. Keep this list strict: every option is a
/// sturdy display, sans-serif, or highly readable serif face available on the
/// target desktop rather than a decorative/script font. Inter and Anton ship
/// in `assets/fonts/` (OFL) so the heavy-condensed look never depends on what
/// the user's machine has installed.
pub const CAPTION_FONTS: [&str; 7] = [
    "Inter",
    "Anton",
    "Arial",
    "Helvetica Neue",
    "Avenir Next",
    "Verdana",
    "Georgia",
];

pub fn caption_font_name(input: &str) -> Option<&'static str> {
    CAPTION_FONTS
        .iter()
        .copied()
        .find(|font| font.eq_ignore_ascii_case(input.trim()))
}

/// Default accent as `#RRGGBB`, mirroring the ASS BGR constants below
/// (consistency is asserted by a unit test).
pub fn default_accent_hex(style: CaptionStyle) -> &'static str {
    match style {
        CaptionStyle::Impact | CaptionStyle::Pop => "#FFDD00",
        CaptionStyle::Clean | CaptionStyle::Cinema => "#FFB224",
    }
}

/// The words fully inside a clip interval, for caption generation.
pub fn words_in_interval(words: &[Word], start_ms: u64, end_ms: u64) -> Vec<Word> {
    words
        .iter()
        .filter(|w| w.start_ms >= start_ms && w.end_ms <= end_ms)
        .cloned()
        .collect()
}

/// Apply edited caption wording while retaining the transcription's timing.
/// When the word count changes, spread the replacement words evenly across
/// the original caption interval.
pub fn with_caption_text(words: &[Word], caption_text: Option<&str>) -> Vec<Word> {
    let Some(text) = caption_text.map(str::trim) else {
        return words.to_vec();
    };
    let replacements: Vec<&str> = text.split_whitespace().collect();
    if replacements.is_empty() {
        return Vec::new();
    }
    if words.is_empty() {
        return words.to_vec();
    }
    if replacements.len() == words.len() {
        return words
            .iter()
            .zip(replacements)
            .map(|(word, text)| Word {
                text: text.to_string(),
                ..word.clone()
            })
            .collect();
    }

    let start_ms = words.first().unwrap().start_ms;
    let end_ms = words.last().unwrap().end_ms.max(start_ms + 1);
    let duration = end_ms - start_ms;
    let count = replacements.len() as u64;
    replacements
        .into_iter()
        .enumerate()
        .map(|(index, text)| Word {
            text: text.to_string(),
            start_ms: start_ms + duration * index as u64 / count,
            end_ms: start_ms + duration * (index as u64 + 1) / count,
            p: 1.0,
        })
        .collect()
}

/// Accent (currently spoken word). ASS colors are &HBBGGRR.
const ACCENT_BGR: &str = "00DDFF"; // #FFDD00 vivid yellow
const CLEAN_ACCENT_BGR: &str = "24B2FF"; // #FFB224 warm amber
const WHITE_BGR: &str = "FFFFFF";

pub struct CaptionInput<'a> {
    /// Words fully inside the clip, with absolute source timestamps.
    pub words: &'a [Word],
    pub clip_start_ms: u64,
    pub clip_end_ms: u64,
    pub headline: &'a str,
    pub font: &'a str,
    /// Accent color in ASS BGR order (see `accent_bgr_for`).
    pub accent_bgr: String,
    /// Opt-in emoji accent: a large glyph flashes above the caption block at
    /// each page's keyword timestamp. Rendered as ASS text, so the system's
    /// color-emoji font (Noto Color Emoji, Apple Color Emoji, Segoe UI Emoji)
    /// must be installed for glyphs to appear.
    pub emoji_overlay: bool,
    /// The clip's rendered output size the captions burn onto (ADR-0002).
    /// ASS PlayRes and all geometry derive from this — the constants below
    /// are authored against the OUT_W×OUT_H reference canvas and scaled.
    pub out_w: u32,
    pub out_h: u32,
    /// Speaker turns on the SAME timeline as `words` (post-auto-cut output
    /// timeline — callers pass `autocut::retime_turns` output). Captions
    /// get an "S1:"/"S2:" tag only when the clip genuinely holds two
    /// voices; a monologue never shows one.
    pub diarization: Option<&'a Diarization>,
}

/// The clip's per-word speaker ids (parallel to `input.words`), or None
/// when fewer than two voices appear — labels only exist to tell people
/// apart.
fn speaker_ids(input: &CaptionInput) -> Option<Vec<Option<u8>>> {
    let d = input.diarization?;
    let ids: Vec<Option<u8>> = input.words.iter().map(|w| d.word_speaker(w)).collect();
    let distinct: std::collections::HashSet<u8> = ids.iter().flatten().copied().collect();
    (distinct.len() >= 2).then_some(ids)
}

/// The speaker label for a page of clip-relative words, using the first
/// word that lands inside a turn. `ids` parallels `rel` — the relative
/// words are shifted by `clip_start_ms`, so an id lookup indexes into the
/// absolute list by position, not time.
fn page_tag(
    input: &CaptionInput,
    ids: &Option<Vec<Option<u8>>>,
    abs_index: usize,
) -> Option<String> {
    let ids = ids.as_ref()?;
    let spk = ids.get(abs_index).copied().flatten()?;
    let label = input
        .diarization?
        .labels
        .get(spk as usize)
        .cloned()
        .unwrap_or_else(|| format!("S{}", spk + 1));
    Some(format!(
        "{{\\c&H{}&}}{}:{{\\c&H{}&}} ",
        input.accent_bgr, label, WHITE_BGR
    ))
}

/// Resolve the accent color: a user-picked #RRGGBB wins, otherwise each style
/// has its default (vivid yellow for Impact, warm amber for Clean).
pub fn accent_bgr_for(style: CaptionStyle, user_hex: Option<&str>) -> String {
    user_hex
        .and_then(hex_to_ass_bgr)
        .unwrap_or_else(|| match style {
            CaptionStyle::Impact | CaptionStyle::Pop => ACCENT_BGR.to_string(),
            CaptionStyle::Clean | CaptionStyle::Cinema => CLEAN_ACCENT_BGR.to_string(),
        })
}

/// `#RRGGBB` (hash optional) → ASS `BBGGRR` hex, or None if malformed.
pub fn hex_to_ass_bgr(hex: &str) -> Option<String> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("{}{}{}", &h[4..6], &h[2..4], &h[0..2]).to_uppercase())
}

pub fn build_ass(input: &CaptionInput, style: CaptionStyle) -> String {
    match style {
        CaptionStyle::Impact => build_impact(input),
        CaptionStyle::Clean => build_clean(input),
        CaptionStyle::Pop => build_pop(input),
        CaptionStyle::Cinema => build_cinema(input),
    }
}

fn relative_words(input: &CaptionInput) -> (Vec<Word>, u64) {
    let mut rel: Vec<Word> = input
        .words
        .iter()
        .map(|w| Word {
            text: w.text.clone(),
            start_ms: w.start_ms.saturating_sub(input.clip_start_ms),
            end_ms: w.end_ms.saturating_sub(input.clip_start_ms),
            p: w.p,
        })
        .collect();
    // Old heuristic transcripts can contain zero-length words. Give each one
    // a single ASS tick and move the following onset forward by that tick so
    // every spoken token can receive a non-overlapping active window.
    for i in 0..rel.len() {
        if rel[i].end_ms <= rel[i].start_ms {
            rel[i].end_ms = rel[i].start_ms + 10;
            let repaired_end = rel[i].end_ms;
            if let Some(next) = rel.get_mut(i + 1) {
                next.start_ms = next.start_ms.max(repaired_end);
                next.end_ms = next.end_ms.max(next.start_ms + 10);
            }
        }
    }
    let clip_len = input.clip_end_ms.saturating_sub(input.clip_start_ms);
    (rel, clip_len)
}

// ===========================================================================
// IMPACT STYLE — stacked lockups: small connectives, one HUGE emphasis word
// ===========================================================================

/// Sizes for the two tiers. The emphasis word is fit-clamped to the frame.
const SMALL_FS: f32 = 70.0;
const EMPH_FS: f32 = 150.0;
const EMPH_FS_FLOOR: f32 = 92.0;
/// Uppercase Inter ExtraBold ≈ 0.62 em/char; lowercase ≈ 0.55.
const CHAR_EM_UPPER: f32 = 0.62;
const CHAR_EM_LOWER: f32 = 0.55;
const MAX_LINE_W: f32 = 940.0;
/// Stroke/drop-shadow references at the 1080×1920 canvas — the punchy looks
/// sit inside the ~8–12px pro short-form band; `border_scale` shrinks them
/// with the clip's real output size.
const PRO_OUTLINE: f32 = 10.0;
const PRO_SHADOW: f32 = 4.0;
/// Vertical center of the lockup and its allowed band.
const BLOCK_ANCHOR_Y: f32 = 1270.0;
const BLOCK_TOP_MIN: f32 = 920.0;
const BLOCK_BOTTOM_MAX: f32 = 1640.0;
/// Line pitch relative to font size — snug but collision-free.
const LINE_PITCH: f32 = 1.04;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "so", "of", "to", "in", "on", "at", "is", "are", "was",
    "were", "be", "been", "it", "its", "it's", "that", "that's", "this", "these", "if", "you",
    "your", "you're", "we", "we're", "i", "i'm", "he", "she", "they", "them", "their", "there",
    "there's", "like", "just", "really", "very", "what", "what's", "when", "how", "why", "would",
    "could", "can", "can't", "will", "won't", "because", "about", "for", "with", "as", "do", "did",
    "does", "don't", "have", "has", "had", "not", "no", "yes", "my", "me", "us", "our", "than",
    "then", "get", "got", "go", "going", "gonna", "all", "any", "some", "one", "out", "up", "down",
    "now", "well",
];

pub(crate) fn is_stopword(w: &str) -> bool {
    let clean: String = w
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect();
    STOPWORDS.contains(&clean.to_lowercase().as_str())
}

fn alnum_len(w: &str) -> usize {
    w.chars().filter(|c| c.is_alphanumeric()).count()
}

/// A word worth coloring ahead of time: the same tier-1 rule the emphasis
/// picker uses (substantial and not a stopword).
fn is_keyword(w: &str) -> bool {
    alnum_len(w) >= 5 && !is_stopword(w)
}

/// The word that gets blasted huge: the last substantial content word, then
/// any content word, then the longest word.
pub fn pick_emphasis(words: &[Word]) -> usize {
    let mut pick: Option<usize> = None;
    for (i, w) in words.iter().enumerate() {
        if alnum_len(&w.text) >= 5 && !is_stopword(&w.text) {
            pick = Some(i);
        }
    }
    if pick.is_none() {
        for (i, w) in words.iter().enumerate() {
            if alnum_len(&w.text) >= 3 && !is_stopword(&w.text) {
                pick = Some(i);
            }
        }
    }
    pick.unwrap_or_else(|| {
        words
            .iter()
            .enumerate()
            .max_by_key(|(_, w)| alnum_len(&w.text))
            .map(|(i, _)| i)
            .unwrap_or(0)
    })
}

#[derive(Debug, PartialEq)]
pub struct LockupLine {
    /// Indexes into the page's words.
    pub word_idx: Vec<usize>,
    pub emphasis: bool,
    pub fs: f32,
    pub x: f32,
    pub y: f32,
}

/// Lay out a page as a 1–3 line lockup: pre-words small, emphasis word huge,
/// post-words small — with mild alternating offsets and safe-band clamping.
/// `(out_w, out_h)` is the clip's real output size; all metrics scale from
/// the OUT_W×OUT_H reference canvas (ADR-0002).
pub fn layout_lockup(words: &[Word], page_no: usize, out_w: u32, out_h: u32) -> Vec<LockupLine> {
    let sx = out_w as f32 / OUT_W as f32;
    let sy = out_h as f32 / OUT_H as f32;
    let e = pick_emphasis(words);
    let mut lines: Vec<LockupLine> = Vec::new();

    let small_line = |idxs: Vec<usize>| -> Option<(Vec<usize>, f32)> {
        if idxs.is_empty() {
            return None;
        }
        let chars: usize =
            idxs.iter().map(|&i| words[i].text.len()).sum::<usize>() + idxs.len().saturating_sub(1);
        let mut fs = SMALL_FS * sy;
        if chars as f32 * CHAR_EM_LOWER * fs > MAX_LINE_W * sx {
            fs = (MAX_LINE_W * sx / (chars as f32 * CHAR_EM_LOWER)).max(46.0 * sy);
        }
        Some((idxs, fs))
    };

    if let Some((idxs, fs)) = small_line((0..e).collect()) {
        lines.push(LockupLine {
            word_idx: idxs,
            emphasis: false,
            fs,
            x: 0.0,
            y: 0.0,
        });
    }
    {
        let chars = words[e].text.len();
        let mut fs = EMPH_FS * sy;
        if chars as f32 * CHAR_EM_UPPER * fs > MAX_LINE_W * sx {
            fs = (MAX_LINE_W * sx / (chars as f32 * CHAR_EM_UPPER)).max(EMPH_FS_FLOOR * sy);
        }
        lines.push(LockupLine {
            word_idx: vec![e],
            emphasis: true,
            fs,
            x: 0.0,
            y: 0.0,
        });
    }
    if let Some((idxs, fs)) = small_line(((e + 1)..words.len()).collect()) {
        lines.push(LockupLine {
            word_idx: idxs,
            emphasis: false,
            fs,
            x: 0.0,
            y: 0.0,
        });
    }

    // Vertical stack centered on the anchor, clamped to the safe band.
    let total_h: f32 = lines.iter().map(|l| l.fs * LINE_PITCH).sum();
    let mut top = BLOCK_ANCHOR_Y * sy - total_h / 2.0;
    if top < BLOCK_TOP_MIN * sy {
        top = BLOCK_TOP_MIN * sy;
    }
    if top + total_h > BLOCK_BOTTOM_MAX * sy {
        top = BLOCK_BOTTOM_MAX * sy - total_h;
    }
    let mut cursor = top;
    let cx = out_w as f32 / 2.0;
    for (li, line) in lines.iter_mut().enumerate() {
        let lh = line.fs * LINE_PITCH;
        line.y = cursor + lh / 2.0;
        cursor += lh;
        line.x = if line.emphasis {
            cx
        } else {
            // Mild raggedness that always stays on-canvas.
            let chars: usize = line
                .word_idx
                .iter()
                .map(|&i| words[i].text.len())
                .sum::<usize>()
                + line.word_idx.len().saturating_sub(1);
            let w = chars as f32 * CHAR_EM_LOWER * line.fs;
            let max_dx = ((out_w as f32 - w) / 2.0 - 50.0 * sx).max(0.0);
            let dx: f32 = if (li + page_no).is_multiple_of(2) {
                -34.0 * sx
            } else {
                34.0 * sx
            };
            cx + dx.clamp(-max_dx, max_dx)
        };
    }
    lines
}

fn build_impact(input: &CaptionInput) -> String {
    let (rel, clip_len) = relative_words(input);
    let blur = 0.6 * input.out_h as f32 / OUT_H as f32;
    let mut ass = String::new();
    ass.push_str(&impact_header(
        input.font,
        input.out_w,
        input.out_h,
        input.emoji_overlay,
    ));

    let ids = speaker_ids(input);
    let pages = paginate_impact(&rel);
    let mut abs_idx = 0usize; // pages consume `rel` in order
    for (page_no, page) in pages.iter().enumerate() {
        if page.is_empty() {
            continue;
        }
        let tag = page_tag(input, &ids, abs_idx).unwrap_or_default();
        abs_idx += page.len();
        // Hard ceiling: never outlive the next page's first word.
        let next_start = pages
            .get(page_no + 1)
            .and_then(|p| p.first())
            .map(|w| w.start_ms)
            .unwrap_or(u64::MAX);
        let last = page.last().unwrap();
        let page_end = (last.end_ms + 200)
            .min(next_start)
            .min(clip_len.max(last.end_ms));

        let lines = layout_lockup(page, page_no, input.out_w, input.out_h);

        if let Some(emoji) = emoji_event(
            input,
            &page[pick_emphasis(page)],
            lines.first().map(|l| l.y - l.fs * LINE_PITCH * 0.62),
        ) {
            ass.push_str(&emoji);
        }

        for (k, word) in page.iter().enumerate() {
            let start = word.start_ms;
            let gap_end = if k + 1 < page.len() {
                page[k + 1].start_ms
            } else {
                page_end
            };
            let end = word.end_ms.max(start + 10).min(gap_end);
            if end <= start {
                continue;
            }
            for (li, line) in lines.iter().enumerate() {
                let pop = if k == 0 {
                    let from = if line.emphasis { 85 } else { 90 };
                    format!("\\fscx{f}\\fscy{f}\\t(0,110,\\fscx100\\fscy100)", f = from)
                } else {
                    String::new()
                };
                let mut text = format!(
                    "{{\\an5\\pos({:.0},{:.0})\\fs{:.0}\\blur{:.1}{}}}{}",
                    line.x,
                    line.y,
                    line.fs,
                    blur,
                    pop,
                    if li == 0 { tag.as_str() } else { "" }
                );
                for (j, &wi) in line.word_idx.iter().enumerate() {
                    if j > 0 {
                        text.push(' ');
                    }
                    let raw = escape(&page[wi].text);
                    let shown = if line.emphasis {
                        raw.to_uppercase()
                    } else {
                        raw.to_lowercase()
                    };
                    if wi == k || line.emphasis {
                        text.push_str(&format!(
                            "{{\\c&H{}&}}{}{{\\c&H{}&}}",
                            input.accent_bgr, shown, WHITE_BGR
                        ));
                    } else {
                        text.push_str(&shown);
                    }
                }
                ass.push_str(&format!(
                    "Dialogue: 0,{},{},Impact,,0,0,0,,{}\n",
                    ass_time(start),
                    ass_time(end),
                    text
                ));
                if gap_end > end {
                    let mut neutral = format!(
                        "{{\\an5\\pos({:.0},{:.0})\\fs{:.0}\\blur{:.1}}}{}",
                        line.x,
                        line.y,
                        line.fs,
                        blur,
                        if li == 0 { tag.as_str() } else { "" }
                    );
                    for (j, &wi) in line.word_idx.iter().enumerate() {
                        if j > 0 {
                            neutral.push(' ');
                        }
                        let raw = escape(&page[wi].text);
                        let shown = if line.emphasis {
                            raw.to_uppercase()
                        } else {
                            raw.to_lowercase()
                        };
                        if line.emphasis {
                            neutral.push_str(&format!(
                                "{{\\c&H{}&}}{}{{\\c&H{}&}}",
                                input.accent_bgr, shown, WHITE_BGR
                            ));
                        } else {
                            neutral.push_str(&shown);
                        }
                    }
                    ass.push_str(&format!(
                        "Dialogue: 0,{},{},Impact,,0,0,0,,{}\n",
                        ass_time(end),
                        ass_time(gap_end),
                        neutral
                    ));
                }
            }
        }
    }
    ass
}

fn impact_header(font: &str, out_w: u32, out_h: u32, emoji: bool) -> String {
    let face = if font == "Inter" {
        "Inter ExtraBold".to_string()
    } else {
        font.to_string()
    };
    let s = out_h as f32 / OUT_H as f32;
    let b = border_scale(out_w, out_h);
    format!(
        "[Script Info]\n\
         Title: Clipping Factory captions (impact)\n\
         ScriptType: v4.00+\n\
         PlayResX: {out_w}\n\
         PlayResY: {out_h}\n\
         WrapStyle: 2\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Impact,{face},{fs:.1},&H00FFFFFF,&H00FFFFFF,&H00000000,&H9C000000,-1,0,0,0,100,100,1,0,1,{outline:.1},{shadow:.1},5,{ml:.0},{mr:.0},{mv:.0},1\n{emoji_style}\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        face = face,
        out_w = out_w,
        out_h = out_h,
        fs = 84.0 * s,
        outline = PRO_OUTLINE * b,
        shadow = PRO_SHADOW * b,
        ml = 60.0 * s,
        mr = 60.0 * s,
        mv = 60.0 * s,
        emoji_style = emoji_style_line(&face, s, emoji),
    )
}

/// Character budget for a page (keeps the small tier comfortably wide).
const PAGE_CHAR_BUDGET: usize = 20;

/// Punchy karaoke cadence: never more than four words on a page.
const IMPACT_MAX_WORDS: usize = 4;

/// Impact pages: 1–4 words with a look-ahead break so no page overflows.
pub fn paginate_impact(words: &[Word]) -> Vec<Vec<Word>> {
    let mut pages: Vec<Vec<Word>> = Vec::new();
    let mut page: Vec<Word> = Vec::new();
    let mut chars = 0usize;

    for (i, w) in words.iter().enumerate() {
        let w_len = w.text.len();
        if !page.is_empty() && chars + 1 + w_len > PAGE_CHAR_BUDGET {
            pages.push(std::mem::take(&mut page));
            chars = 0;
        }
        chars += if page.is_empty() { w_len } else { w_len + 1 };
        page.push(w.clone());

        let terminal = w
            .text
            .trim_end_matches(['"', '\'', ')', ']'])
            .ends_with(['.', '?', '!', '…', ',']);
        let gap = words
            .get(i + 1)
            .map(|n| n.start_ms.saturating_sub(w.end_ms))
            .unwrap_or(u64::MAX);

        let full = page.len() >= IMPACT_MAX_WORDS;
        let punct = terminal && page.len() >= 2;
        let pause = gap >= 600;

        if full || punct || pause {
            pages.push(std::mem::take(&mut page));
            chars = 0;
        }
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}

// ===========================================================================
// CLEAN STYLE — the original restrained treatment
// ===========================================================================

const MAX_WORDS_PER_PAGE: usize = 7;
const MIN_WORDS_BEFORE_PUNCT_BREAK: usize = 3;
const MAX_CHARS_PER_PAGE: usize = 30;
const PAGE_GAP_MS: u64 = 700;
/// Platform UIs overlay roughly the bottom fifth of the frame; Clean's lower
/// margin (25% of the reference height) keeps the caption baseline above it.
const CLEAN_BOTTOM_SAFE: f32 = 480.0;

fn build_clean(input: &CaptionInput) -> String {
    let (rel, clip_len) = relative_words(input);

    let mut ass = String::new();
    ass.push_str(&clean_header(
        input.font,
        input.out_w,
        input.out_h,
        input.emoji_overlay,
    ));

    // Headline: only when it adds context beyond the opening caption.
    if show_headline(input.headline, &rel) {
        ass.push_str(&format!(
            "Dialogue: 0,{},{},Headline,,0,0,0,,{}\n",
            ass_time(0),
            ass_time(3500.min(clip_len)),
            escape(input.headline)
        ));
    }

    let ids = speaker_ids(input);
    let pages = paginate(&rel);
    let mut abs_idx = 0usize;
    for (page_no, page) in pages.iter().enumerate() {
        if page.is_empty() {
            continue;
        }
        let keyword = pick_emphasis(page);
        if let Some(emoji) = emoji_event(input, &page[keyword], Some(input.out_h as f32 * 0.62)) {
            ass.push_str(&emoji);
        }
        let tag = page_tag(input, &ids, abs_idx).unwrap_or_default();
        abs_idx += page.len();
        let next_start = pages
            .get(page_no + 1)
            .and_then(|p| p.first())
            .map(|w| w.start_ms)
            .unwrap_or(u64::MAX);
        let page_end = page
            .last()
            .map(|w| (w.end_ms + 160).min(next_start).min(clip_len.max(w.end_ms)))
            .unwrap_or(0);
        for (i, word) in page.iter().enumerate() {
            let start = word.start_ms;
            let gap_end = page.get(i + 1).map(|n| n.start_ms).unwrap_or(page_end);
            let end = word.end_ms.max(start + 10).min(gap_end);
            if end <= start {
                continue;
            }
            let mut line = tag.clone();
            for (j, w) in page.iter().enumerate() {
                if j > 0 {
                    line.push(' ');
                }
                if j == i || j == keyword {
                    line.push_str(&format!(
                        "{{\\c&H{}&}}{}{{\\c&H{}&}}",
                        input.accent_bgr,
                        escape(&w.text),
                        WHITE_BGR
                    ));
                } else {
                    line.push_str(&escape(&w.text));
                }
            }
            ass.push_str(&format!(
                "Dialogue: 0,{},{},Caption,,0,0,0,,{}\n",
                ass_time(start),
                ass_time(end),
                line
            ));
            if gap_end > end {
                let mut neutral = tag.clone();
                for (j, word) in page.iter().enumerate() {
                    if j > 0 {
                        neutral.push(' ');
                    }
                    if j == keyword {
                        neutral.push_str(&format!(
                            "{{\\c&H{}&}}{}{{\\c&H{}&}}",
                            input.accent_bgr,
                            escape(&word.text),
                            WHITE_BGR
                        ));
                    } else {
                        neutral.push_str(&escape(&word.text));
                    }
                }
                ass.push_str(&format!(
                    "Dialogue: 0,{},{},Caption,,0,0,0,,{}\n",
                    ass_time(end),
                    ass_time(gap_end),
                    neutral
                ));
            }
        }
    }
    ass
}

fn clean_header(font: &str, out_w: u32, out_h: u32, emoji: bool) -> String {
    let s = out_h as f32 / OUT_H as f32;
    let b = border_scale(out_w, out_h);
    format!(
        "[Script Info]\n\
         Title: Clipping Factory captions (clean)\n\
         ScriptType: v4.00+\n\
         PlayResX: {out_w}\n\
         PlayResY: {out_h}\n\
         WrapStyle: 2\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Caption,{font},{cfs:.1},&H00FFFFFF,&H00FFFFFF,&H00141414,&H7A000000,-1,0,0,0,100,100,0,0,1,{co:.1},{cs:.1},2,{cml:.0},{cmr:.0},{cmv:.0},1\n\
         Style: Headline,{font},{hfs:.1},&H00F2F2F2,&H00FFFFFF,&H00141414,&H7A000000,-1,0,0,0,100,100,0,0,1,{ho:.1},{hs:.1},8,{hml:.0},{hmr:.0},{hmv:.0},1\n{emoji_style}\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        font = font,
        out_w = out_w,
        out_h = out_h,
        cfs = 66.0 * s,
        co = 6.4 * b,
        cs = 1.8 * b,
        cml = 90.0 * s,
        cmr = 90.0 * s,
        cmv = CLEAN_BOTTOM_SAFE * s,
        hfs = 42.0 * s,
        ho = 3.4 * b,
        hs = 1.2 * b,
        hml = 110.0 * s,
        hmr = 110.0 * s,
        hmv = 110.0 * s,
        emoji_style = emoji_style_line(font, s, emoji),
    )
}

/// Clean-style pages: 3–7 word groups.
pub fn paginate(words: &[Word]) -> Vec<Vec<Word>> {
    let mut pages: Vec<Vec<Word>> = Vec::new();
    let mut page: Vec<Word> = Vec::new();
    let mut chars = 0usize;

    for (i, w) in words.iter().enumerate() {
        page.push(w.clone());
        chars += w.text.len() + 1;

        let terminal = w
            .text
            .trim_end_matches(['"', '\'', ')', ']'])
            .ends_with(['.', '?', '!', '…', ',']);
        let gap = words
            .get(i + 1)
            .map(|n| n.start_ms.saturating_sub(w.end_ms))
            .unwrap_or(u64::MAX);

        let full = page.len() >= MAX_WORDS_PER_PAGE;
        let wide = chars >= MAX_CHARS_PER_PAGE && page.len() >= MIN_WORDS_BEFORE_PUNCT_BREAK;
        let punct = terminal && page.len() >= MIN_WORDS_BEFORE_PUNCT_BREAK;
        let pause = gap >= PAGE_GAP_MS;

        if full || wide || punct || pause {
            pages.push(std::mem::take(&mut page));
            chars = 0;
        }
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}

/// Plain-text export sidecar (`<clip>.srt`): one cue per caption page —
/// the same words and timing the burned captions show, tagged with the
/// speaker when the clip holds two voices. Style-agnostic: SRT viewers
/// reflow text anyway, so cues use the restrained pagination.
pub fn build_srt(input: &CaptionInput) -> String {
    let (rel, clip_len) = relative_words(input);
    let ids = speaker_ids(input);
    let mut abs_idx = 0usize;
    let mut out = String::new();
    let mut cue = 0usize;
    for page in paginate(&rel) {
        if page.is_empty() {
            continue;
        }
        let start = page[0].start_ms;
        let end = (page.last().unwrap().end_ms + 160).min(clip_len.max(start + 10));
        // Speaker name in plain text — SRT has no styling to borrow.
        let tag = ids
            .as_ref()
            .and_then(|ids| ids.get(abs_idx).copied().flatten())
            .and_then(|spk| {
                input
                    .diarization
                    .and_then(|d| d.labels.get(spk as usize))
                    .cloned()
            })
            .map(|name| format!("{name}: "))
            .unwrap_or_default();
        abs_idx += page.len();
        cue += 1;
        let text = page
            .iter()
            .map(|w| w.text.replace('\n', " "))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "{cue}\n{} --> {}\n{tag}{text}\n\n",
            srt_time(start),
            srt_time(end)
        ));
    }
    out
}

/// `HH:MM:SS,mmm` — the SRT timestamp shape.
fn srt_time(ms: u64) -> String {
    let (h, rem) = (ms / 3_600_000, ms % 3_600_000);
    let (m, rem) = (rem / 60_000, rem % 60_000);
    let (s, frac) = (rem / 1000, rem % 1000);
    format!("{h:02}:{m:02}:{s:02},{frac:03}")
}

/// Skip the headline overlay when it (nearly) duplicates the opening words.
fn show_headline(headline: &str, words: &[Word]) -> bool {
    if headline.trim().is_empty() {
        return false;
    }
    let norm = |s: &str| -> Vec<String> {
        s.split_whitespace()
            .map(|w| {
                w.chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(|c| c.to_lowercase())
                    .collect::<String>()
            })
            .filter(|w| !w.is_empty())
            .collect()
    };
    let h = norm(headline);
    if h.is_empty() {
        return false;
    }
    let opening: std::collections::HashSet<String> =
        words.iter().take(14).flat_map(|w| norm(&w.text)).collect();
    let contained = h.iter().filter(|w| opening.contains(*w)).count();
    (contained as f32 / h.len() as f32) < 0.7
}

// ===========================================================================
// POP STYLE — one word at a time, center frame, scale-popping in
// ===========================================================================

const POP_FS: f32 = 118.0;
const POP_ANCHOR_Y: f32 = 1180.0;
/// Minimum spacing between emoji flashes so they read as accents, not noise.
const POP_EMOJI_GAP_MS: u64 = 2500;

fn build_pop(input: &CaptionInput) -> String {
    let (rel, _clip_len) = relative_words(input);
    let s = input.out_h as f32 / OUT_H as f32;
    let sx = input.out_w as f32 / OUT_W as f32;
    let cx = input.out_w as f32 / 2.0;
    let blur = 0.6 * s;
    let mut ass = String::new();
    ass.push_str(&pop_header(
        input.font,
        input.out_w,
        input.out_h,
        input.emoji_overlay,
    ));

    let mut last_emoji: Option<u64> = None;
    for word in &rel {
        let start = word.start_ms;
        let end = word.end_ms.max(start + 10);
        let keyword = is_keyword(&word.text);
        let shown = if keyword {
            escape(&word.text).to_uppercase()
        } else {
            escape(&word.text).to_lowercase()
        };
        // Clamp the word's size to the frame, same rule as the lockup lines.
        let em = if keyword {
            CHAR_EM_UPPER
        } else {
            CHAR_EM_LOWER
        };
        let mut fs = POP_FS * s;
        if shown.len() as f32 * em * fs > MAX_LINE_W * sx {
            fs = (MAX_LINE_W * sx / (shown.len() as f32 * em)).max(34.0 * s);
        }
        let text = if keyword {
            format!(
                "{{\\an5\\pos({cx:.0},{y:.0})\\fs{fs:.0}\\blur{blur:.1}\\fscx82\\fscy82\\t(0,110,\\fscx100\\fscy100)\\c&H{}&}}{}",
                input.accent_bgr,
                shown,
                cx = cx,
                y = POP_ANCHOR_Y * s,
            )
        } else {
            format!(
                "{{\\an5\\pos({cx:.0},{y:.0})\\fs{fs:.0}\\blur{blur:.1}\\fscx82\\fscy82\\t(0,110,\\fscx100\\fscy100)}}{shown}",
                cx = cx,
                y = POP_ANCHOR_Y * s,
            )
        };
        ass.push_str(&format!(
            "Dialogue: 0,{},{},Pop,,0,0,0,,{}\n",
            ass_time(start),
            ass_time(end),
            text
        ));

        if keyword
            && last_emoji
                .map(|t| start.saturating_sub(t) >= POP_EMOJI_GAP_MS)
                .unwrap_or(true)
        {
            if let Some(emoji) = emoji_event(input, word, Some((POP_ANCHOR_Y - 240.0) * s)) {
                ass.push_str(&emoji);
                last_emoji = Some(start);
            }
        }
    }
    ass
}

fn pop_header(font: &str, out_w: u32, out_h: u32, emoji: bool) -> String {
    let face = if font == "Inter" {
        "Inter ExtraBold".to_string()
    } else {
        font.to_string()
    };
    let s = out_h as f32 / OUT_H as f32;
    let b = border_scale(out_w, out_h);
    format!(
        "[Script Info]\n\
         Title: Clipping Factory captions (pop)\n\
         ScriptType: v4.00+\n\
         PlayResX: {out_w}\n\
         PlayResY: {out_h}\n\
         WrapStyle: 2\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Pop,{face},{fs:.1},&H00FFFFFF,&H00FFFFFF,&H00000000,&H9C000000,-1,0,0,0,100,100,1,0,1,{outline:.1},{shadow:.1},5,{ml:.0},{mr:.0},{mv:.0},1\n{emoji_style}\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        face = face,
        out_w = out_w,
        out_h = out_h,
        fs = POP_FS * s,
        outline = PRO_OUTLINE * b,
        shadow = PRO_SHADOW * b,
        ml = 60.0 * s,
        mr = 60.0 * s,
        mv = 60.0 * s,
        emoji_style = emoji_style_line(&face, s, emoji),
    )
}

// ===========================================================================
// CINEMA STYLE — a minimal letterspaced lower-third line per page
// ===========================================================================

const CINEMA_FS: f32 = 54.0;
const CINEMA_Y: f32 = 1660.0;
const CINEMA_TRACKING: f32 = 5.0;

fn build_cinema(input: &CaptionInput) -> String {
    let (rel, _clip_len) = relative_words(input);
    let s = input.out_h as f32 / OUT_H as f32;
    let cx = input.out_w as f32 / 2.0;
    let mut ass = String::new();
    ass.push_str(&cinema_header(
        input.font,
        input.out_w,
        input.out_h,
        input.emoji_overlay,
    ));

    let pages = paginate(&rel);
    for page in pages.iter() {
        if page.is_empty() {
            continue;
        }
        let keyword = pick_emphasis(page);
        if let Some(emoji) = emoji_event(input, &page[keyword], Some((CINEMA_Y - 260.0) * s)) {
            ass.push_str(&emoji);
        }
        let start = page.first().unwrap().start_ms;
        let end = page.last().unwrap().end_ms.max(start + 10);
        let mut line = String::new();
        for (j, w) in page.iter().enumerate() {
            if j > 0 {
                line.push(' ');
            }
            let shown = escape(&w.text).to_lowercase();
            if j == keyword {
                line.push_str(&format!(
                    "{{\\c&H{}&}}{}{{\\c&H{}&}}",
                    input.accent_bgr, shown, WHITE_BGR
                ));
            } else {
                line.push_str(&shown);
            }
        }
        ass.push_str(&format!(
            "Dialogue: 0,{},{},Cinema,,0,0,0,,{{\\an2\\pos({cx:.0},{y:.0})\\fs{fs:.0}\\fsp{fsp:.1}\\fad(90,140)\\blur0.4}}{line}\n",
            ass_time(start),
            ass_time(end),
            cx = cx,
            y = CINEMA_Y * s,
            fs = CINEMA_FS * s,
            fsp = CINEMA_TRACKING * s,
        ));
    }
    ass
}

fn cinema_header(font: &str, out_w: u32, out_h: u32, emoji: bool) -> String {
    let s = out_h as f32 / OUT_H as f32;
    let b = border_scale(out_w, out_h);
    format!(
        "[Script Info]\n\
         Title: Clipping Factory captions (cinema)\n\
         ScriptType: v4.00+\n\
         PlayResX: {out_w}\n\
         PlayResY: {out_h}\n\
         WrapStyle: 2\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Cinema,{font},{fs:.1},&H00F2F2F2,&H00FFFFFF,&H00141414,&H7A000000,0,0,0,0,100,100,0,0,1,{outline:.1},{shadow:.1},2,{ml:.0},{mr:.0},{mv:.0},1\n{emoji_style}\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        font = font,
        out_w = out_w,
        out_h = out_h,
        fs = CINEMA_FS * s,
        outline = 3.2 * b,
        shadow = 1.4 * b,
        ml = 90.0 * s,
        mr = 90.0 * s,
        mv = 60.0 * s,
        emoji_style = emoji_style_line(font, s, emoji),
    )
}

// ===========================================================================
// EMOJI ACCENT — opt-in glyph flash at each page's keyword timestamp
// ===========================================================================

/// Curated keyword → glyph map; longer hints first so specific wins over
/// generic. Fallback is deterministic on the word so the same keyword always
/// lands the same emoji.
const EMOJI_MAP: &[(&[&str], &str)] = &[
    (
        &[
            "money", "cash", "profit", "selling", "sales", "revenue", "rich", "paid",
        ],
        "💰",
    ),
    (&["win", "best", "champion", "goat", "top"], "🏆"),
    (
        &[
            "growth", "growing", "scale", "compound", "market", "business",
        ],
        "📈",
    ),
    (&["secret", "hidden", "nobody", "actually"], "🤫"),
    (&["afraid", "scared", "fear", "panic", "terrifying"], "😱"),
    (&["brain", "mind", "think", "idea", "smart", "learn"], "🧠"),
    (&["fire", "hot", "insane", "crazy", "wild"], "🔥"),
    (&["laugh", "funny", "joke", "hilarious"], "😂"),
    (&["sleep", "tired", "dream"], "😴"),
    (&["work", "hustle", "grind", "build"], "💪"),
    (&["dead", "death", "kill"], "💀"),
    (&["food", "hungry", "eating"], "🍔"),
    (&["code", "computer", "phone", "tech", "internet"], "💻"),
    (&["time", "clock", "hours", "minutes"], "⏰"),
    (&["mistake", "wrong", "never", "stop"], "🚫"),
    (&["love", "heart"], "❤️"),
    (&["question"], "❓"),
    (&["danger", "risk", "warning"], "⚠️"),
    (&["music", "song"], "🎵"),
    (&["world", "earth", "everyone"], "🌍"),
    (&["future", "tomorrow"], "🔮"),
    (&["sad", "cry"], "😢"),
];
const EMOJI_FALLBACK: &[&str] = &["⚡", "💡", "🚀", "⭐"];

/// Deterministic glyph for an emphasized word.
fn emoji_for(word: &str) -> &'static str {
    let cleaned: String = word
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    for (hints, emoji) in EMOJI_MAP {
        if hints.iter().any(|h| cleaned.contains(h)) {
            return emoji;
        }
    }
    let hash: usize = cleaned.bytes().map(|b| b as usize).sum();
    EMOJI_FALLBACK[hash % EMOJI_FALLBACK.len()]
}

/// A large glyph flash centered above the caption block for the keyword's
/// spoken window (clamped to ~0.6–1.4 s so it reads as a beat, not a frame
/// pop). `None` when the overlay is off.
fn emoji_event(input: &CaptionInput, word: &Word, y: Option<f32>) -> Option<String> {
    if !input.emoji_overlay {
        return None;
    }
    let s = input.out_h as f32 / OUT_H as f32;
    let cx = input.out_w as f32 / 2.0;
    let y = y.unwrap_or(input.out_h as f32 * 0.60).max(140.0 * s);
    let start = word.start_ms;
    let end = word.end_ms.clamp(start + 600, start + 1400);
    Some(format!(
        "Dialogue: 0,{},{},Emoji,,0,0,0,,{{\\an5\\pos({cx:.0},{y:.0})\\fs{fs:.0}\\fad(60,120)\\fscx70\\fscy70\\t(0,120,\\fscx100\\fscy100)}}{}\n",
        ass_time(start),
        ass_time(end),
        emoji_for(&word.text),
        cx = cx,
        y = y,
        fs = 120.0 * s,
    ))
}

/// The Emoji style line, appended under a style's own line only when the
/// overlay is enabled so unused style rows stay out of the ASS.
fn emoji_style_line(font: &str, s: f32, enabled: bool) -> String {
    if !enabled {
        return String::new();
    }
    format!(
        "         Style: Emoji,{font},{fs:.1},&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n",
        font = font,
        fs = 120.0 * s,
    )
}

// ===========================================================================
// Shared plumbing
// ===========================================================================

/// Border scale for ASS Outline/Shadow: borders follow the clip's output
/// geometry (the smaller axis wins) so a 608×1080 render keeps the same
/// visual stroke weight as the 1080×1920 reference canvas.
fn border_scale(out_w: u32, out_h: u32) -> f32 {
    (out_w as f32 / OUT_W as f32).min(out_h as f32 / OUT_H as f32)
}

/// ASS timestamp: `H:MM:SS.CS` (centiseconds).
fn ass_time(ms: u64) -> String {
    let cs = (ms / 10) % 100;
    let s = (ms / 1000) % 60;
    let m = (ms / 60_000) % 60;
    let h = ms / 3_600_000;
    format!("{}:{:02}:{:02}.{:02}", h, m, s, cs)
}

/// Keep user/model text inside one ASS event line. ASS treats `{}` as
/// override blocks and `\` as escapes; CR/LF and other controls can inject or
/// corrupt event records.
fn escape(s: &str) -> String {
    s.chars()
        .map(|ch| match ch {
            '{' => '(',
            '}' => ')',
            '\\' => '/',
            '\r' | '\n' => ' ',
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, start: u64) -> Word {
        Word {
            text: text.into(),
            start_ms: start,
            end_ms: start + 280,
            p: 0.9,
        }
    }

    // ---- Clean style ----

    #[test]
    fn clean_pages_stay_within_3_to_7_words() {
        let words: Vec<Word> = "this is a longer test sentence that should split into several caption pages cleanly without any single page growing too large"
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| w(t, i as u64 * 320))
            .collect();
        let pages = paginate(&words);
        assert!(pages.len() >= 2);
        for p in &pages {
            assert!(p.len() <= 7, "page too long: {}", p.len());
        }
        assert_eq!(pages.iter().map(|p| p.len()).sum::<usize>(), words.len());
    }

    #[test]
    fn pause_forces_page_break() {
        let words = vec![w("before", 0), w("pause", 350), w("after", 3000)];
        let pages = paginate(&words);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].len(), 2);
    }

    #[test]
    fn ass_time_formats_centiseconds() {
        assert_eq!(ass_time(0), "0:00:00.00");
        assert_eq!(ass_time(61_230), "0:01:01.23");
        assert_eq!(ass_time(3_600_000), "1:00:00.00");
    }

    #[test]
    fn headline_skipped_when_duplicating_opening() {
        let words: Vec<Word> = "most people misunderstand what discipline is"
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| w(t, i as u64 * 300))
            .collect();
        assert!(!show_headline(
            "Most people misunderstand what discipline is",
            &words
        ));
        assert!(show_headline(
            "A totally different framing of the idea",
            &words
        ));
    }

    #[test]
    fn clean_karaoke_events_cover_every_word_once() {
        let words: Vec<Word> = (0..5).map(|i| w("word", i * 400)).collect();
        let input = CaptionInput {
            words: &words,
            clip_start_ms: 0,
            clip_end_ms: 3000,
            headline: "",
            font: "Inter",
            accent_bgr: accent_bgr_for(CaptionStyle::Clean, None),
            emoji_overlay: false,
            out_w: OUT_W,
            out_h: OUT_H,
            diarization: None,
        };
        let ass = build_ass(&input, CaptionStyle::Clean);
        // Every spoken word gets an accent window; the page keyword also keeps
        // its accent through the trailing neutral window.
        for w in [0u64, 400, 800, 1200, 1600] {
            assert!(
                parse_accent_events(&ass, CLEAN_ACCENT_BGR)
                    .iter()
                    .any(|&(s, _)| s == w),
                "missing accent window starting at {w}"
            );
        }
    }

    #[test]
    fn escapes_ass_control_characters() {
        assert_eq!(escape("a{b}c\\d\r\ne\u{0007}f"), "a(b)c/d  e f");
    }

    #[test]
    fn multiline_headline_and_caption_text_cannot_inject_ass_events() {
        let words = vec![Word {
            text: "first\nDialogue: 9,0:00:00.00,bad\rline".into(),
            start_ms: 0,
            end_ms: 1000,
            p: 0.9,
        }];
        let ass = build_ass(
            &CaptionInput {
                words: &words,
                clip_start_ms: 0,
                clip_end_ms: 1000,
                headline: "headline\r\nDialogue: injected",
                font: "Inter",
                accent_bgr: accent_bgr_for(CaptionStyle::Clean, None),
                emoji_overlay: false,
                out_w: OUT_W,
                out_h: OUT_H,
                diarization: None,
            },
            CaptionStyle::Clean,
        );
        assert!(!ass.contains("\nDialogue: injected"), "{ass}");
        assert!(!ass.contains("\nDialogue: 9"), "{ass}");
        assert!(ass.lines().all(|line| !line.contains('\r')));
    }

    // ---- Impact style ----

    fn words_from(s: &str) -> Vec<Word> {
        s.split_whitespace()
            .enumerate()
            .map(|(i, t)| w(t, i as u64 * 330))
            .collect()
    }

    fn input<'a>(words: &'a [Word], end_ms: u64) -> CaptionInput<'a> {
        CaptionInput {
            words,
            clip_start_ms: 0,
            clip_end_ms: end_ms,
            headline: "",
            font: "Inter",
            accent_bgr: accent_bgr_for(CaptionStyle::Impact, None),
            emoji_overlay: false,
            out_w: OUT_W,
            out_h: OUT_H,
            diarization: None,
        }
    }

    // ---- Speaker labels ----

    fn two_speaker_diar() -> Diarization {
        Diarization {
            labels: vec!["S1".into(), "S2".into()],
            turns: vec![
                crate::domain::SpeakerTurn {
                    start_ms: 0,
                    end_ms: 2_000,
                    speaker: 0,
                },
                crate::domain::SpeakerTurn {
                    start_ms: 2_000,
                    end_ms: 10_000,
                    speaker: 1,
                },
            ],
        }
    }

    #[test]
    fn clean_captions_tag_pages_with_the_speaker() {
        // S1 words, then S2 words (each cluster pages separately on the gap).
        let d = two_speaker_diar();
        let mut words: Vec<Word> = (0..4).map(|i| w("alpha", i * 300)).collect();
        words.extend((0..4).map(|i| w("beta", 3_000 + i * 300)));
        let mut inp = input(&words, 6_000);
        inp.diarization = Some(&d);
        let ass = build_ass(&inp, CaptionStyle::Clean);
        assert!(ass.contains("S1:"), "first page tagged S1: {ass}");
        assert!(ass.contains("S2:"), "second page tagged S2: {ass}");
    }

    #[test]
    fn monologue_never_gets_speaker_tags() {
        let d = Diarization {
            labels: vec!["S1".into()],
            turns: vec![crate::domain::SpeakerTurn {
                start_ms: 0,
                end_ms: 10_000,
                speaker: 0,
            }],
        };
        let words: Vec<Word> = (0..6).map(|i| w("alpha", i * 300)).collect();
        let mut inp = input(&words, 6_000);
        inp.diarization = Some(&d);
        let ass = build_ass(&inp, CaptionStyle::Clean);
        assert!(!ass.contains("S1:"), "one voice → no labels: {ass}");
    }

    #[test]
    fn srt_export_carries_speaker_names() {
        let d = two_speaker_diar();
        let mut words: Vec<Word> = (0..3).map(|i| w("alpha", i * 300)).collect();
        words.extend((0..3).map(|i| w("beta", 3_000 + i * 300)));
        let mut inp = input(&words, 6_000);
        inp.diarization = Some(&d);
        let srt = build_srt(&inp);
        assert!(srt.contains("S1: alpha alpha alpha"), "{srt}");
        assert!(srt.contains("S2: beta beta beta"), "{srt}");
        assert!(srt.contains("00:00:00,000 -->"), "{srt}");
    }

    /// Parse "Dialogue: 0,H:MM:SS.CS,H:MM:SS.CS,..." start/end back to ms.
    fn parse_events(ass: &str) -> Vec<(u64, u64)> {
        fn t(s: &str) -> u64 {
            let parts: Vec<&str> = s.split(':').collect();
            let (h, m, rest) = (parts[0], parts[1], parts[2]);
            let (sec, cs) = rest.split_once('.').unwrap();
            h.parse::<u64>().unwrap() * 3_600_000
                + m.parse::<u64>().unwrap() * 60_000
                + sec.parse::<u64>().unwrap() * 1000
                + cs.parse::<u64>().unwrap() * 10
        }
        ass.lines()
            .filter(|l| l.starts_with("Dialogue:"))
            .map(|l| {
                let f: Vec<&str> = l.splitn(4, ',').collect();
                (t(f[1]), t(f[2]))
            })
            .collect()
    }

    fn parse_accent_events(ass: &str, accent_bgr: &str) -> Vec<(u64, u64)> {
        let mut events = ass
            .lines()
            .filter(|line| line.starts_with("Dialogue:") && line.contains(accent_bgr))
            .flat_map(parse_events)
            .collect::<Vec<_>>();
        events.sort_unstable();
        events.dedup();
        events
    }

    #[test]
    fn hex_conversion_is_bgr() {
        assert_eq!(hex_to_ass_bgr("#FFDD00").unwrap(), "00DDFF");
        assert_eq!(hex_to_ass_bgr("4fb5ff").unwrap(), "FFB54F");
        assert!(hex_to_ass_bgr("#nope").is_none());
        assert!(hex_to_ass_bgr("#FFF").is_none());
    }

    #[test]
    fn accent_defaults_per_style_and_user_wins() {
        assert_eq!(accent_bgr_for(CaptionStyle::Impact, None), "00DDFF");
        assert_eq!(accent_bgr_for(CaptionStyle::Clean, None), "24B2FF");
        assert_eq!(
            accent_bgr_for(CaptionStyle::Impact, Some("#7CFF4F")),
            "4FFF7C"
        );
        assert_eq!(
            accent_bgr_for(CaptionStyle::Impact, Some("garbage")),
            "00DDFF"
        );
    }

    #[test]
    fn emphasis_prefers_last_substantial_content_word() {
        let words = words_from("when silence feels like strength");
        assert_eq!(pick_emphasis(&words), 4);
        let words = words_from("if this would work with 300");
        assert_eq!(words[pick_emphasis(&words)].text, "300");
    }

    #[test]
    fn lockup_has_a_dominant_emphasis_line_in_the_safe_band() {
        let words = words_from("when silence feels like strength");
        let lines = layout_lockup(&words, 0, OUT_W, OUT_H);
        // Every word appears in exactly one line.
        let mut covered: Vec<usize> = lines.iter().flat_map(|l| l.word_idx.clone()).collect();
        covered.sort();
        assert_eq!(covered, vec![0, 1, 2, 3, 4]);
        let emph = lines
            .iter()
            .find(|l| l.emphasis)
            .expect("has emphasis line");
        for l in &lines {
            if !l.emphasis {
                assert!(
                    emph.fs > l.fs * 1.5,
                    "emphasis dominates: {} vs {}",
                    emph.fs,
                    l.fs
                );
            }
            assert!(l.y > BLOCK_TOP_MIN - 1.0 && l.y < BLOCK_BOTTOM_MAX + 1.0);
        }
        // Lines never collide: centers are at least the smaller half-pitch apart.
        for pair in lines.windows(2) {
            let min_gap = (pair[0].fs + pair[1].fs) / 2.0 * 0.9;
            assert!(pair[1].y - pair[0].y >= min_gap * 0.9, "lines too close");
        }
    }

    #[test]
    fn long_emphasis_words_clamp_to_frame() {
        let words = words_from("this is counterintuitive");
        let lines = layout_lockup(&words, 0, OUT_W, OUT_H);
        let emph = lines.iter().find(|l| l.emphasis).unwrap();
        let w = "counterintuitive".len() as f32 * CHAR_EM_UPPER * emph.fs;
        assert!(w <= MAX_LINE_W + 1.0, "emphasis width {} exceeds frame", w);
        assert!(
            emph.fs >= EMPH_FS_FLOOR - 26.0,
            "still reads big: {}",
            emph.fs
        );
    }

    #[test]
    fn impact_pages_stay_small_and_lose_no_words() {
        let words = words_from(
            "most people think reselling is about finding cheap stuff and flipping it for profit online every day",
        );
        let pages = paginate_impact(&words);
        for p in &pages {
            assert!(
                p.len() <= IMPACT_MAX_WORDS,
                "impact page too long: {}",
                p.len()
            );
            if p.len() > 1 {
                let chars: usize = p.iter().map(|w| w.text.len()).sum::<usize>() + p.len() - 1;
                assert!(chars <= PAGE_CHAR_BUDGET, "page over budget: {}", chars);
            }
        }
        assert_eq!(pages.iter().map(|p| p.len()).sum::<usize>(), words.len());
    }

    /// Regression: no two caption windows may partially overlap. (Lines of the
    /// same lockup legitimately share an identical window.)
    #[test]
    fn impact_windows_never_partially_overlap_in_fast_speech() {
        let words: Vec<Word> =
            "this is very fast speech with no pauses at all between any words here honestly"
                .split_whitespace()
                .enumerate()
                .map(|(i, t)| Word {
                    text: t.into(),
                    start_ms: i as u64 * 180,
                    end_ms: (i as u64 + 1) * 180,
                    p: 0.9,
                })
                .collect();
        let ass = build_ass(&input(&words, 20_000), CaptionStyle::Impact);
        let mut windows = parse_events(&ass);
        windows.sort();
        windows.dedup();
        for pair in windows.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0,
                "windows overlap: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn active_word_windows_follow_variable_speech_spans_exactly() {
        let words = vec![
            Word {
                text: "fast".into(),
                start_ms: 100,
                end_ms: 180,
                p: 0.9,
            },
            Word {
                text: "slowly".into(),
                start_ms: 400,
                end_ms: 900,
                p: 0.9,
            },
            Word {
                text: "now".into(),
                start_ms: 950,
                end_ms: 1050,
                p: 0.9,
            },
        ];
        for style in [CaptionStyle::Impact, CaptionStyle::Clean] {
            let accent = accent_bgr_for(style, None);
            let mut caption_input = input(&words, 1400);
            caption_input.accent_bgr = accent.clone();
            let ass = build_ass(&caption_input, style);
            let windows = parse_accent_events(&ass, &accent);
            // Keyword tint adds accent windows around the spoken ones; every
            // spoken window must still be exactly the word's own span.
            for w in [(100, 180), (400, 900), (950, 1050)] {
                assert!(windows.contains(&w), "{style:?} missing {w:?}: {windows:?}");
            }
        }
    }

    #[test]
    fn impact_uppercases_emphasis_and_tints_active_word() {
        let words = words_from("when silence feels like strength");
        let ass = build_ass(&input(&words, 4000), CaptionStyle::Impact);
        assert!(ass.contains("STRENGTH"), "emphasis uppercased");
        assert!(ass.contains("silence"), "small words lowercase");
        assert!(ass.contains(&accent_bgr_for(CaptionStyle::Impact, None)));
        let pops = ass.matches("\\t(0,").count();
        assert!(pops >= paginate_impact(&words).len(), "pop-in on each page");
    }

    #[test]
    fn custom_accent_flows_into_the_ass() {
        let words = words_from("when silence feels like strength");
        let mut inp = input(&words, 4000);
        inp.accent_bgr = accent_bgr_for(CaptionStyle::Impact, Some("#7CFF4F"));
        let ass = build_ass(&inp, CaptionStyle::Impact);
        assert!(ass.contains("4FFF7C"), "custom green accent present");
        assert!(!ass.contains("00DDFF"), "default yellow fully replaced");
    }

    /// Positions of the ASS `Style:` fields this module emits (per the
    /// `Format:` row in each header).
    const ASS_OUTLINE_FIELD: usize = 16;
    const ASS_SHADOW_FIELD: usize = 17;
    const ASS_MARGINV_FIELD: usize = 21;

    /// A numeric field from a `Style: <name>,...` row in a generated header.
    fn style_field(ass: &str, style: &str, index: usize) -> f32 {
        ass.lines()
            .find(|l| l.starts_with(&format!("Style: {style},")))
            .unwrap_or_else(|| panic!("missing Style: {style}: {ass}"))
            .split(',')
            .nth(index)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or_else(|| panic!("{style} field {index} not numeric: {ass}"))
    }

    /// Spec A3: the pro short-form stroke (~8–12px at 1080×1920) is a function
    /// of the output geometry — a 608×1080 render keeps the same visual weight
    /// by shrinking with the canvas instead of burning a fixed 1080p border.
    #[test]
    fn outline_and_shadow_scale_with_output_geometry() {
        let (sw, sh) = (608u32, 1080u32);
        let scale = border_scale(sw, sh);
        for (ass_style, header) in [
            ("Impact", impact_header("Inter", OUT_W, OUT_H, false)),
            ("Pop", pop_header("Inter", OUT_W, OUT_H, false)),
            ("Caption", clean_header("Inter", OUT_W, OUT_H, false)),
            ("Cinema", cinema_header("Inter", OUT_W, OUT_H, false)),
        ] {
            let small = match ass_style {
                "Impact" => impact_header("Inter", sw, sh, false),
                "Pop" => pop_header("Inter", sw, sh, false),
                "Caption" => clean_header("Inter", sw, sh, false),
                _ => cinema_header("Inter", sw, sh, false),
            };
            for field in [ASS_OUTLINE_FIELD, ASS_SHADOW_FIELD] {
                let full = style_field(&header, ass_style, field);
                let shrunk = style_field(&small, ass_style, field);
                assert!(
                    (shrunk - full * scale).abs() < 0.05,
                    "{ass_style} field {field}: {full}px at 1080×1920 → {shrunk}px at 608×1080 (expected {:.2})",
                    full * scale
                );
            }
        }
        // The punchy looks land inside the ~8–12px band at full size.
        for (style, ass) in [
            ("Impact", impact_header("Inter", OUT_W, OUT_H, false)),
            ("Pop", pop_header("Inter", OUT_W, OUT_H, false)),
        ] {
            let outline = style_field(&ass, style, ASS_OUTLINE_FIELD);
            assert!(
                (8.0..=12.0).contains(&outline),
                "{style} outline {outline}px outside the pro band at 1080×1920"
            );
        }
    }

    /// Spec A3: platform UIs overlay roughly the bottom 20% of the frame, so
    /// Clean's bottom margin clears that zone at any output size.
    #[test]
    fn clean_captions_clear_the_bottom_risk_zone() {
        for (w, h) in [(OUT_W, OUT_H), (608, 1080)] {
            let mv = style_field(
                &clean_header("Inter", w, h, false),
                "Caption",
                ASS_MARGINV_FIELD,
            );
            assert!(
                mv > 0.2 * h as f32,
                "clean baseline {mv}px sits inside the bottom UI zone at {w}×{h}"
            );
        }
    }

    /// Every caption style must be authored against the exact render canvas,
    /// so libass scales coordinates the way the renderer crops them.
    #[test]
    fn ass_headers_pin_the_output_canvas() {
        for ass in [
            impact_header("Inter", OUT_W, OUT_H, false),
            clean_header("Inter", OUT_W, OUT_H, false),
            pop_header("Inter", OUT_W, OUT_H, false),
            cinema_header("Inter", OUT_W, OUT_H, false),
        ] {
            assert!(ass.contains("PlayResX: 1080\n"), "{ass}");
            assert!(ass.contains("PlayResY: 1920\n"), "{ass}");
        }
    }

    /// ADR-0002: PlayRes and font sizes derive from the clip's actual output
    /// size, not the 1080×1920 ceiling — a 608×1080 clip gets a 608×1080 ASS
    /// canvas and proportionally smaller fonts.
    #[test]
    fn ass_headers_track_the_clip_output_size() {
        let (w, h) = (608, 1080);
        for ass in [
            impact_header("Inter", w, h, false),
            clean_header("Inter", w, h, false),
            pop_header("Inter", w, h, false),
            cinema_header("Inter", w, h, false),
        ] {
            assert!(ass.contains("PlayResX: 608\n"), "{ass}");
            assert!(ass.contains("PlayResY: 1080\n"), "{ass}");
        }
        let s = h as f32 / OUT_H as f32;
        let clean = clean_header("Inter", w, h, false);
        assert!(
            clean.contains(&format!("Style: Caption,Inter,{:.1}", 66.0 * s)),
            "{clean}"
        );
        let impact = impact_header("Inter", w, h, false);
        assert!(
            impact.contains(&format!("Style: Impact,Inter ExtraBold,{:.1}", 84.0 * s)),
            "{impact}"
        );
        let pop = pop_header("Inter", w, h, false);
        assert!(
            pop.contains(&format!("Style: Pop,Inter ExtraBold,{:.1}", POP_FS * s)),
            "{pop}"
        );
        let cinema = cinema_header("Inter", w, h, false);
        assert!(
            cinema.contains(&format!("Style: Cinema,Inter,{:.1}", CINEMA_FS * s)),
            "{cinema}"
        );
    }

    #[test]
    fn lockup_geometry_scales_with_output_size() {
        let words = words_from("when silence feels like strength");
        let full = layout_lockup(&words, 0, OUT_W, OUT_H);
        let half = layout_lockup(&words, 0, OUT_W / 2, OUT_H / 2);
        assert_eq!(full.len(), half.len());
        for (a, b) in full.iter().zip(half.iter()) {
            assert!((b.fs - a.fs * 0.5).abs() < 0.01, "fs scales");
            assert!((b.y - a.y * 0.5).abs() < 0.5, "y scales");
            assert!((b.x - a.x * 0.5).abs() < 0.5, "x scales");
        }
    }

    #[test]
    fn style_parses_from_string() {
        assert_eq!(CaptionStyle::from_str("clean"), CaptionStyle::Clean);
        assert_eq!(CaptionStyle::from_str("impact"), CaptionStyle::Impact);
        assert_eq!(CaptionStyle::from_str("anything"), CaptionStyle::Impact);
    }

    #[test]
    fn curated_caption_fonts_parse_to_canonical_names() {
        assert_eq!(caption_font_name("inter"), Some("Inter"));
        assert_eq!(caption_font_name("anton"), Some("Anton"));
        assert_eq!(caption_font_name("Helvetica Neue"), Some("Helvetica Neue"));
        assert_eq!(caption_font_name("avenir next"), Some("Avenir Next"));
    }

    /// The condensed black weight ships with the repo (OFL), so the pro look
    /// never depends on what the user's machine has installed.
    #[test]
    fn bundled_anton_is_listed_and_on_disk() {
        assert!(CAPTION_FONTS.contains(&"Anton"));
        let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
        assert!(
            fonts.join("Anton-Regular.ttf").is_file(),
            "Anton-Regular.ttf missing from assets/fonts"
        );
        assert!(
            fonts.join("OFL-Anton.txt").is_file(),
            "OFL-Anton.txt license missing from assets/fonts"
        );
    }

    #[test]
    fn arbitrary_or_decorative_fonts_are_rejected() {
        assert_eq!(caption_font_name("Comic Sans MS"), None);
        assert_eq!(caption_font_name("Brush Script MT"), None);
        assert_eq!(caption_font_name(""), None);
    }

    #[test]
    fn selected_font_is_written_to_the_caption_style() {
        let words = words_from("readable captions");
        let mut caption_input = input(&words, 2_000);
        caption_input.font = "Georgia";
        let ass = build_ass(&caption_input, CaptionStyle::Impact);
        assert!(ass.contains("Style: Impact,Georgia,"));
    }

    // ---- Pop + Cinema styles + emoji overlay ----

    #[test]
    fn style_names_round_trip_through_strict_parse() {
        for label in CAPTION_STYLES {
            let style = CaptionStyle::parse_strict(label).expect(label);
            assert_eq!(style.label(), label);
        }
        assert_eq!(CaptionStyle::from_str("bounce"), CaptionStyle::Pop);
        assert_eq!(CaptionStyle::from_str("cinematic"), CaptionStyle::Cinema);
        assert_eq!(
            CaptionStyle::parse_strict("Cinema"),
            Some(CaptionStyle::Cinema)
        );
        assert_eq!(CaptionStyle::parse_strict("karaoke"), None);
    }

    #[test]
    fn pop_emits_one_event_per_word_and_marks_keywords() {
        let words = words_from("when silence feels like strength");
        let mut inp = input(&words, 4000);
        inp.accent_bgr = accent_bgr_for(CaptionStyle::Pop, None);
        let ass = build_ass(&inp, CaptionStyle::Pop);
        // One dialogue event per spoken word.
        assert_eq!(
            ass.lines().filter(|l| l.starts_with("Dialogue:")).count(),
            words.len()
        );
        // Keywords shout in caps inside the accent color.
        assert!(ass.contains("STRENGTH"), "{ass}");
        assert!(
            ass.contains(&format!("&H{}&}}{}", ACCENT_BGR, "STRENGTH")),
            "keyword wears the accent: {ass}"
        );
        // Stopwords stay lowercase and untinted.
        let like_line = ass
            .lines()
            .find(|l| l.trim_end().ends_with("}like"))
            .expect("pop event for 'like'");
        assert!(
            !like_line.contains(&format!("&H{}&}}", ACCENT_BGR)),
            "{like_line}"
        );
        // Every pop event pops in on a scale transform.
        assert_eq!(ass.matches("\\fscx82").count(), words.len(), "{ass}");
    }

    #[test]
    fn pop_keeps_long_words_on_canvas() {
        let words = words_from("a antidisestablishmentarianism moment");
        let mut inp = input(&words, 3000);
        inp.accent_bgr = accent_bgr_for(CaptionStyle::Pop, None);
        let ass = build_ass(&inp, CaptionStyle::Pop);
        // The giant word shrinks to fit instead of running off the edges.
        let fs: f32 = ass
            .lines()
            .find(|l| l.contains("ANTIDISESTABLISHMENTARIANISM"))
            .and_then(|l| l.split("\\fs").nth(1))
            .and_then(|s| s.split('\\').next())
            .and_then(|s| s.parse().ok())
            .expect("pop event carries a font size");
        assert!(
            "ANTIDISESTABLISHMENTARIANISM".len() as f32 * CHAR_EM_UPPER * fs <= MAX_LINE_W + 1.0,
            "keyword overruns frame at fs {fs}"
        );
    }

    #[test]
    fn cinema_emits_one_fading_line_per_page_with_accented_keyword() {
        let words: Vec<Word> = "most people think discipline means waking early every day"
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| w(t, i as u64 * 400))
            .collect();
        let mut inp = input(&words, 5000);
        inp.accent_bgr = accent_bgr_for(CaptionStyle::Cinema, None);
        let ass = build_ass(&inp, CaptionStyle::Cinema);
        let events: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(events.len(), paginate(&words).len(), "{ass}");
        for e in &events {
            assert!(e.contains("\\fad("), "cinema lines fade: {e}");
            assert!(e.contains("\\fsp"), "cinema lines are letterspaced: {e}");
            assert!(e.contains("Cinema,"), "{e}");
        }
        // The page's keyword is accent-colored and everything renders lowercase.
        assert!(
            ass.contains("discipline") && !ass.contains("Discipline"),
            "{ass}"
        );
        let accent = accent_bgr_for(CaptionStyle::Cinema, None);
        for page in paginate(&words) {
            let kw = page[pick_emphasis(&page)].text.to_lowercase();
            assert!(
                ass.contains(&format!("&H{accent}&}}{kw}")),
                "keyword '{kw}' carries the accent: {ass}"
            );
        }
    }

    /// "i made money selling systems" at 1s/word: page styles pick each
    /// page's emphasis word (Impact: "selling" → 💰; Clean/Cinema single-page
    /// "systems"); Pop flashes each keyword (money → 💰, systems) under its
    /// cooldown rule.
    #[test]
    fn emoji_overlay_is_opt_in_and_lands_on_the_keyword() {
        let words: Vec<Word> = "i made money selling systems"
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| Word {
                text: t.into(),
                start_ms: i as u64 * 1000,
                end_ms: i as u64 * 1000 + 280,
                p: 0.9,
            })
            .collect();
        let expected_anchor: &[(CaptionStyle, u64)] = &[
            (CaptionStyle::Impact, 3000),
            (CaptionStyle::Clean, 4000),
            (CaptionStyle::Cinema, 4000),
            (CaptionStyle::Pop, 2000),
        ];
        for (style, anchor) in expected_anchor {
            let mut inp = input(&words, 6000);
            inp.accent_bgr = accent_bgr_for(*style, None);
            inp.emoji_overlay = false;
            let off = build_ass(&inp, *style);
            assert!(
                !off.contains("Emoji"),
                "{style:?} leaked Emoji style: {off}"
            );

            inp.emoji_overlay = true;
            let on = build_ass(&inp, *style);
            assert!(
                on.contains("Style: Emoji,"),
                "{style:?} missing Emoji style: {on}"
            );
            let emoji_events: Vec<&str> = on
                .lines()
                .filter(|l| l.starts_with("Dialogue:") && l.contains("Emoji,"))
                .collect();
            assert!(
                !emoji_events.is_empty(),
                "{style:?} overlay on but no emoji events: {on}"
            );
            let windows = emoji_events
                .iter()
                .flat_map(|e| parse_events(e))
                .collect::<Vec<_>>();
            assert!(
                windows.iter().any(|(s, _)| *s == *anchor),
                "{style:?} emoji not anchored to keyword @{anchor}: {windows:?}"
            );
        }
        // Impact and Pop both surface the mapped glyph for their keyword.
        let mut inp = input(&words, 6000);
        inp.emoji_overlay = true;
        assert!(build_ass(&inp, CaptionStyle::Impact).contains("💰"));
        inp.accent_bgr = accent_bgr_for(CaptionStyle::Pop, None);
        assert!(build_ass(&inp, CaptionStyle::Pop).contains("💰"));
    }

    #[test]
    fn emoji_for_is_deterministic_and_keyword_mapped() {
        assert_eq!(emoji_for("money"), "💰");
        assert_eq!(emoji_for("Profits!"), "💰");
        assert_eq!(emoji_for("zzz-custom"), emoji_for("zzz-custom"));
        assert!(EMOJI_FALLBACK.contains(&emoji_for("persimmon")));
    }

    #[test]
    fn pop_emoji_flashes_respect_the_cooldown() {
        // Ten keywords in 2s: only the first flash fits inside the gap rule.
        let words: Vec<Word> = (0..10)
            .map(|i| Word {
                text: format!("keyword{i}"),
                start_ms: i as u64 * 200,
                end_ms: i as u64 * 200 + 180,
                p: 0.9,
            })
            .collect();
        let mut inp = input(&words, 4000);
        inp.emoji_overlay = true;
        inp.accent_bgr = accent_bgr_for(CaptionStyle::Pop, None);
        let ass = build_ass(&inp, CaptionStyle::Pop);
        let flashes = ass
            .lines()
            .filter(|l| l.starts_with("Dialogue:") && l.contains("Emoji,"))
            .count();
        assert!(
            flashes <= 2,
            "emoji spam: {flashes} flashes across 2s of keywords\n{ass}"
        );
    }
}

#[cfg(test)]
mod restyle_support_tests {
    use super::*;

    #[test]
    fn default_hex_matches_ass_bgr_constants() {
        assert_eq!(
            hex_to_ass_bgr(default_accent_hex(CaptionStyle::Impact)).as_deref(),
            Some(ACCENT_BGR)
        );
        assert_eq!(
            hex_to_ass_bgr(default_accent_hex(CaptionStyle::Clean)).as_deref(),
            Some(CLEAN_ACCENT_BGR)
        );
    }

    #[test]
    fn parse_strict_rejects_unknown_styles() {
        assert_eq!(
            CaptionStyle::parse_strict("impact"),
            Some(CaptionStyle::Impact)
        );
        assert_eq!(
            CaptionStyle::parse_strict(" Clean "),
            Some(CaptionStyle::Clean)
        );
        assert_eq!(CaptionStyle::parse_strict("comic-sans"), None);
        assert_eq!(CaptionStyle::parse_strict(""), None);
    }

    #[test]
    fn words_in_interval_keeps_only_fully_contained_words() {
        let w = |s: u64, e: u64| Word {
            text: "w".into(),
            start_ms: s,
            end_ms: e,
            p: 1.0,
        };
        let words = vec![w(900, 1100), w(1000, 1500), w(1500, 2000), w(1900, 2100)];
        let inside = words_in_interval(&words, 1000, 2000);
        assert_eq!(inside.len(), 2);
        assert_eq!(inside[0].start_ms, 1000);
        assert_eq!(inside[1].end_ms, 2000);
    }

    #[test]
    fn edited_caption_text_preserves_matching_word_timings() {
        let words = vec![
            Word {
                text: "old".into(),
                start_ms: 100,
                end_ms: 400,
                p: 0.8,
            },
            Word {
                text: "words".into(),
                start_ms: 500,
                end_ms: 900,
                p: 0.8,
            },
        ];
        let edited = with_caption_text(&words, Some("new text"));
        assert_eq!(edited[0].text, "new");
        assert_eq!(edited[0].start_ms, 100);
        assert_eq!(edited[1].text, "text");
        assert_eq!(edited[1].end_ms, 900);
    }

    #[test]
    fn edited_caption_text_with_new_word_count_spans_original_interval() {
        let words = vec![
            Word {
                text: "old".into(),
                start_ms: 100,
                end_ms: 400,
                p: 0.8,
            },
            Word {
                text: "words".into(),
                start_ms: 500,
                end_ms: 900,
                p: 0.8,
            },
        ];
        let edited = with_caption_text(&words, Some("three new words"));
        assert_eq!(edited.len(), 3);
        assert_eq!(edited.first().unwrap().start_ms, 100);
        assert_eq!(edited.last().unwrap().end_ms, 900);
    }

    #[test]
    fn empty_edited_caption_text_removes_all_captions() {
        let words = vec![Word {
            text: "remove".into(),
            start_ms: 100,
            end_ms: 900,
            p: 0.8,
        }];
        assert!(with_caption_text(&words, Some("   ")).is_empty());
    }
}
