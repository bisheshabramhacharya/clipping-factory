//! Local word-timestamp transcription via whisper.cpp (PRD §10).
//!
//! We consume lexical token offsets from whisper.cpp's full JSON output and
//! rebuild sentence-level segments deterministically. Sentence/segment offsets
//! are deliberately not treated as word boundaries because they absorb pauses.

use crate::config::Config;
use crate::domain::{Sentence, Transcript, Word};
use crate::util::run_streaming;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Languages whisper.cpp's multilingual models understand
/// (`whisper_lang_str` order): `(code, display name)`. The picker treats the
/// stored value `auto` (not a code) as whisper's own language detection.
pub const WHISPER_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("zh", "Chinese"),
    ("de", "German"),
    ("es", "Spanish"),
    ("ru", "Russian"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("ja", "Japanese"),
    ("pt", "Portuguese"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("ca", "Catalan"),
    ("nl", "Dutch"),
    ("ar", "Arabic"),
    ("sv", "Swedish"),
    ("it", "Italian"),
    ("id", "Indonesian"),
    ("hi", "Hindi"),
    ("fi", "Finnish"),
    ("vi", "Vietnamese"),
    ("he", "Hebrew"),
    ("uk", "Ukrainian"),
    ("el", "Greek"),
    ("ms", "Malay"),
    ("cs", "Czech"),
    ("ro", "Romanian"),
    ("da", "Danish"),
    ("hu", "Hungarian"),
    ("ta", "Tamil"),
    ("no", "Norwegian"),
    ("th", "Thai"),
    ("ur", "Urdu"),
    ("hr", "Croatian"),
    ("bg", "Bulgarian"),
    ("lt", "Lithuanian"),
    ("la", "Latin"),
    ("mi", "Maori"),
    ("ml", "Malayalam"),
    ("cy", "Welsh"),
    ("sk", "Slovak"),
    ("te", "Telugu"),
    ("fa", "Persian"),
    ("lv", "Latvian"),
    ("bn", "Bengali"),
    ("sr", "Serbian"),
    ("az", "Azerbaijani"),
    ("sl", "Slovenian"),
    ("kn", "Kannada"),
    ("et", "Estonian"),
    ("mk", "Macedonian"),
    ("br", "Breton"),
    ("eu", "Basque"),
    ("is", "Icelandic"),
    ("hy", "Armenian"),
    ("ne", "Nepali"),
    ("mn", "Mongolian"),
    ("bs", "Bosnian"),
    ("kk", "Kazakh"),
    ("sq", "Albanian"),
    ("sw", "Swahili"),
    ("gl", "Galician"),
    ("mr", "Marathi"),
    ("pa", "Punjabi"),
    ("si", "Sinhala"),
    ("km", "Khmer"),
    ("sn", "Shona"),
    ("yo", "Yoruba"),
    ("so", "Somali"),
    ("af", "Afrikaans"),
    ("oc", "Occitan"),
    ("ka", "Georgian"),
    ("be", "Belarusian"),
    ("tg", "Tajik"),
    ("sd", "Sindhi"),
    ("gu", "Gujarati"),
    ("am", "Amharic"),
    ("yi", "Yiddish"),
    ("lo", "Lao"),
    ("uz", "Uzbek"),
    ("fo", "Faroese"),
    ("ht", "Haitian Creole"),
    ("ps", "Pashto"),
    ("tk", "Turkmen"),
    ("nn", "Nynorsk"),
    ("mt", "Maltese"),
    ("sa", "Sanskrit"),
    ("lb", "Luxembourgish"),
    ("my", "Myanmar"),
    ("bo", "Tibetan"),
    ("tl", "Tagalog"),
    ("mg", "Malagasy"),
    ("as", "Assamese"),
    ("tt", "Tatar"),
    ("haw", "Hawaiian"),
    ("ln", "Lingala"),
    ("ha", "Hausa"),
    ("ba", "Bashkir"),
    ("jw", "Javanese"),
    ("su", "Sundanese"),
    ("yue", "Cantonese"),
];

pub fn language_name(code: &str) -> Option<&'static str> {
    WHISPER_LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
}

/// Pick the `-l` argument and, when the requested language outgrows an
/// English-only model, the model to run with. Returns the resolved model path
/// and language code ("auto" = whisper's own detection, which needs
/// multilingual weights).
fn resolve_language(
    cfg: &Config,
    requested: &str,
    model_dirs: &[PathBuf],
) -> Result<(PathBuf, String)> {
    let model = cfg
        .whisper_model
        .as_ref()
        .ok_or_else(|| anyhow!("Transcription model missing. Download ggml-base.bin (~148MB) into <data-dir>/models or set CF_WHISPER_MODEL."))?;
    let code = requested.trim().to_lowercase();
    let code = code.as_str();
    if code != "auto" && language_name(code).is_none() {
        return Err(anyhow!(
            "Unknown transcription language \"{code}\". Pick a language from the list."
        ));
    }
    if crate::config::model_is_multilingual(model) {
        return Ok((model.clone(), code.to_string()));
    }
    // English-only weights can neither detect a language nor transcribe
    // non-English — swap to a multilingual model on disk when one exists.
    if code != "en" {
        if let Some(alt) = crate::config::find_multilingual_model(model_dirs) {
            return Ok((alt, code.to_string()));
        }
        if code != "auto" {
            let name = language_name(code).unwrap_or(code);
            return Err(anyhow!(
                "Transcribing in {name} needs a multilingual whisper model (e.g. ggml-base.bin in <data-dir>/models or CF_WHISPER_MODEL). The configured model is English-only."
            ));
        }
    }
    // `auto` with no multilingual model on disk keeps the historical `-l en`.
    Ok((model.clone(), "en".into()))
}

pub async fn transcribe<F>(
    cfg: &Config,
    wav: &Path,
    language: Option<&str>,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> Result<Transcript>
where
    F: FnMut(f32),
{
    let bin = cfg
        .whisper_bin
        .as_ref()
        .ok_or_else(|| anyhow!("whisper-cli not found. Install whisper.cpp (macOS: `brew install whisper-cpp`) or set CF_WHISPER_BIN."))?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let model_dirs = crate::config::model_search_dirs(&cfg.data_dir, &cwd);
    let (model, whisper_lang) = resolve_language(cfg, language.unwrap_or("auto"), &model_dirs)?;

    // Never let whisper write directly to a retry-visible name. A cancelled
    // process may leave a JSON prefix that looks parseable on the next run.
    let out_prefix = wav.with_file_name(format!(".whisper-{}", crate::util::short_id()));
    let json_candidates = [
        out_prefix.with_extension("json"),
        out_prefix.with_extension("whisper.json"),
    ];
    let args: Vec<String> = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-l".into(),
        whisper_lang,
        "-t".into(),
        cfg.threads.to_string(),
        "--output-json-full".into(),
        "--output-file".into(),
        out_prefix.to_string_lossy().into_owned(),
        "--print-progress".into(),
    ];

    let result: Result<Transcript> = async {
        run_streaming(&bin.to_string_lossy(), &args, cancel, |_is_err, line| {
            // whisper.cpp prints `whisper_print_progress_callback: progress = 35%`
            if let Some(idx) = line.find("progress =") {
                let tail = &line[idx + 10..];
                if let Ok(pct) = tail.trim().trim_end_matches('%').parse::<f32>() {
                    on_progress((pct / 100.0).clamp(0.0, 1.0));
                }
            }
        })
        .await
        .map_err(|e| {
            if e.to_string().contains("cancelled") {
                e
            } else {
                anyhow!("Transcription failed. {}", e)
            }
        })?;

        // whisper.cpp appends `.json` to the output prefix. Keep a fallback
        // for versions that insert `.whisper` before that suffix.
        let json_path = json_candidates
            .iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                anyhow!(
                    "whisper output not found at {}",
                    json_candidates[0].display()
                )
            })?;
        let bytes = tokio::fs::read(json_path)
            .await
            .with_context(|| format!("reading whisper output at {}", json_path.display()))?;
        let parsed: serde_json::Value = serde_json::from_slice(&bytes)?;

        let words = parse_words(&parsed);
        if words.is_empty() {
            return Err(anyhow!(
                "No speech was detected in this video. Clipping Factory needs clear spoken audio."
            ));
        }
        let sentences = build_sentences(&words);
        let avg_confidence = words.iter().map(|w| w.p).sum::<f32>() / (words.len().max(1) as f32);
        let language = parsed["result"]["language"]
            .as_str()
            .unwrap_or("en")
            .to_string();

        Ok(Transcript {
            language,
            words,
            sentences,
            avg_confidence,
        })
    }
    .await;

    for path in &json_candidates {
        tokio::fs::remove_file(path).await.ok();
    }
    result
}

#[derive(Default)]
struct PendingWord {
    text: String,
    start_ms: u64,
    end_ms: u64,
    p_sum: f64,
    p_count: usize,
}

fn finish_word(words: &mut Vec<Word>, pending: &mut Option<PendingWord>) {
    let Some(word) = pending.take() else { return };
    if !word.text.chars().any(char::is_alphanumeric) {
        return;
    }
    words.push(Word {
        text: word.text,
        start_ms: word.start_ms,
        end_ms: word.end_ms.max(word.start_ms.saturating_add(10)),
        p: if word.p_count == 0 {
            0.5
        } else {
            (word.p_sum / word.p_count as f64) as f32
        },
    });
}

fn parse_token_words(segments: &[serde_json::Value]) -> Vec<Word> {
    let mut words = Vec::new();
    let mut saw_timed_token = false;

    for segment in segments {
        let mut pending: Option<PendingWord> = None;
        // Non-lexical symbols before the next word (¿, ¡, quotes, dashes) —
        // attached to the word they open so captions keep them.
        let mut prefix = String::new();
        let Some(tokens) = segment["tokens"].as_array() else {
            continue;
        };
        for token in tokens {
            let raw = token["text"].as_str().unwrap_or("");
            let part = raw.trim();
            if part.is_empty() || part.starts_with("[_") {
                continue;
            }
            let starts_word = raw.chars().next().is_some_and(char::is_whitespace);
            if starts_word {
                finish_word(&mut words, &mut pending);
            }

            let lexical = part.chars().any(char::is_alphanumeric);
            let annotation = (part.starts_with('[') && part.ends_with(']'))
                || (part.starts_with('(') && part.ends_with(')'));
            if annotation {
                continue;
            }
            if !lexical && pending.is_none() {
                prefix.push_str(part);
                continue;
            }

            let from = token["offsets"]["from"].as_u64();
            let to = token["offsets"]["to"].as_u64();
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            saw_timed_token = true;

            if pending.is_none() {
                if !lexical {
                    continue;
                }
                pending = Some(PendingWord {
                    text: std::mem::take(&mut prefix) + part,
                    start_ms: from,
                    end_ms: to,
                    p_sum: token["p"].as_f64().unwrap_or(0.5),
                    p_count: 1,
                });
                continue;
            }

            let word = pending.as_mut().unwrap();
            word.text.push_str(part);
            // Punctuation has no spoken duration. Keep it visible, but do not
            // let its timestamp absorb the silence after the lexical word.
            if lexical {
                word.end_ms = word.end_ms.max(to);
                word.p_sum += token["p"].as_f64().unwrap_or(0.5);
                word.p_count += 1;
            }
        }
        finish_word(&mut words, &mut pending);
    }

    if !saw_timed_token {
        return Vec::new();
    }
    // Token heuristics can overlap at a boundary. Split only those overlaps;
    // genuine silence gaps remain untouched.
    for i in 0..words.len().saturating_sub(1) {
        if words[i].end_ms <= words[i + 1].start_ms {
            continue;
        }
        let min_boundary = words[i].start_ms.saturating_add(10);
        let max_boundary = words[i + 1].end_ms.saturating_sub(10);
        let midpoint =
            words[i + 1].start_ms + words[i].end_ms.saturating_sub(words[i + 1].start_ms) / 2;
        let boundary = if min_boundary <= max_boundary {
            midpoint.clamp(min_boundary, max_boundary)
        } else {
            min_boundary
        };
        words[i].end_ms = boundary;
        words[i + 1].start_ms = boundary;
        words[i + 1].end_ms = words[i + 1].end_ms.max(boundary.saturating_add(10));
    }
    words
}

fn parse_words(v: &serde_json::Value) -> Vec<Word> {
    let mut words = Vec::new();
    let Some(segments) = v["transcription"].as_array() else {
        return words;
    };
    let token_words = parse_token_words(segments);
    if !token_words.is_empty() {
        return token_words;
    }
    for seg in segments {
        let text = seg["text"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        // Skip non-speech annotations like [BLANK_AUDIO], (music), ♪ etc.
        if (text.starts_with('[') && text.ends_with(']'))
            || (text.starts_with('(') && text.ends_with(')'))
            || text.chars().all(|c| !c.is_alphanumeric())
        {
            continue;
        }
        let from = seg["offsets"]["from"].as_u64().unwrap_or(0);
        let to = seg["offsets"]["to"].as_u64().unwrap_or(from);
        // Mean probability over real tokens (skip specials like [_BEG_]).
        let mut p_sum = 0.0f64;
        let mut p_n = 0usize;
        if let Some(tokens) = seg["tokens"].as_array() {
            for tok in tokens {
                let tt = tok["text"].as_str().unwrap_or("");
                if tt.starts_with("[_") {
                    continue;
                }
                if let Some(p) = tok["p"].as_f64() {
                    p_sum += p;
                    p_n += 1;
                }
            }
        }
        let p = if p_n > 0 {
            (p_sum / p_n as f64) as f32
        } else {
            0.5
        };
        let lexical_words = text
            .split_whitespace()
            .filter(|word| word.chars().any(char::is_alphanumeric))
            .collect::<Vec<_>>();
        let end = to.max(from);
        let duration = end.saturating_sub(from);
        let count = lexical_words.len() as u64;
        for (index, word) in lexical_words.into_iter().enumerate() {
            let index = index as u64;
            words.push(Word {
                text: word.to_string(),
                start_ms: from.saturating_add(duration.saturating_mul(index) / count),
                end_ms: from.saturating_add(duration.saturating_mul(index + 1) / count),
                p,
            });
        }
    }
    words
}

/// Group words into sentence-like segments: break after terminal punctuation,
/// on long pauses, or when a segment grows unreasonably large.
pub fn build_sentences(words: &[Word]) -> Vec<Sentence> {
    let mut sentences = Vec::new();
    let mut start_idx = 0usize;
    let mut char_len = 0usize;

    for i in 0..words.len() {
        char_len += words[i].text.len() + 1;
        let terminal = words[i]
            .text
            .trim_end_matches(['"', '\'', ')', ']'])
            .ends_with(['.', '?', '!', '…']);
        let long_pause = words
            .get(i + 1)
            .map(|next| next.start_ms.saturating_sub(words[i].end_ms) >= 1000)
            .unwrap_or(false);
        let too_long = char_len >= 260;
        let last = i + 1 == words.len();

        if terminal || long_pause || too_long || last {
            let slice = &words[start_idx..=i];
            let text = slice
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            sentences.push(Sentence {
                text,
                start_ms: slice[0].start_ms,
                end_ms: slice[slice.len() - 1].end_ms,
                word_start: start_idx,
                word_end: i + 1,
            });
            start_idx = i + 1;
            char_len = 0;
        }
    }
    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, start: u64, end: u64) -> Word {
        Word {
            text: text.into(),
            start_ms: start,
            end_ms: end,
            p: 0.9,
        }
    }

    #[test]
    fn sentences_break_on_punctuation_and_pauses() {
        let words = vec![
            w("Hello", 0, 300),
            w("there.", 350, 700),
            w("Second", 900, 1200),
            w("idea", 1250, 1500),
            // 1.5s pause here
            w("after", 3000, 3300),
            w("pause", 3350, 3700),
        ];
        let s = build_sentences(&words);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].text, "Hello there.");
        assert_eq!(s[1].word_start, 2);
        assert_eq!(s[2].start_ms, 3000);
    }

    #[test]
    fn token_offsets_preserve_variable_rate_word_spans_and_silence() {
        let parsed = serde_json::json!({
            "transcription": [{
                "offsets": {"from": 0, "to": 1200},
                "text": "Fast slowly now.",
                "tokens": [
                    {"text": "[_BEG_]", "offsets": {"from": 0, "to": 0}, "p": 1.0, "t_dtw": -1},
                    {"text": " Fast", "offsets": {"from": 100, "to": 180}, "p": 0.9, "t_dtw": 14},
                    {"text": " slowly", "offsets": {"from": 400, "to": 900}, "p": 0.9, "t_dtw": 62},
                    {"text": " now", "offsets": {"from": 950, "to": 1050}, "p": 0.9, "t_dtw": 100},
                    {"text": ".", "offsets": {"from": 1050, "to": 1180}, "p": 0.8, "t_dtw": 106}
                ]
            }]
        });

        let words = parse_words(&parsed);
        assert_eq!(
            words
                .iter()
                .map(|word| (word.text.as_str(), word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            vec![
                ("Fast", 100, 180),
                ("slowly", 400, 900),
                ("now.", 950, 1050),
            ]
        );
    }

    #[test]
    fn segment_timing_is_distributed_when_tokens_have_no_usable_offsets() {
        let parsed = serde_json::json!({
            "transcription": [{
                "offsets": {"from": 1000, "to": 2200},
                "text": "Three lexical words.",
                "tokens": [
                    {"text": " Three", "p": 0.9},
                    {"text": " lexical", "p": 0.8},
                    {"text": " words.", "p": 0.7}
                ]
            }]
        });

        let words = parse_words(&parsed);
        assert_eq!(
            words
                .iter()
                .map(|word| (word.text.as_str(), word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            vec![
                ("Three", 1000, 1400),
                ("lexical", 1400, 1800),
                ("words.", 1800, 2200),
            ]
        );
    }

    fn cfg_with_model(path: &Path) -> Config {
        let mut cfg = Config::resolve();
        cfg.whisper_model = Some(path.to_path_buf());
        cfg
    }

    #[test]
    fn language_table_is_unique_and_covers_whisper_languages() {
        assert_eq!(WHISPER_LANGUAGES.len(), 100);
        let mut codes: Vec<&str> = WHISPER_LANGUAGES.iter().map(|(c, _)| *c).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), WHISPER_LANGUAGES.len());
        assert_eq!(language_name("es"), Some("Spanish"));
        assert_eq!(language_name("yue"), Some("Cantonese"));
        assert_eq!(language_name("xx"), None);
    }

    #[test]
    fn auto_language_detects_only_on_multilingual_weights() {
        let multi = cfg_with_model(Path::new("models/ggml-base.bin"));
        let en_only = cfg_with_model(Path::new("models/ggml-base.en.bin"));
        let no_dirs: &[PathBuf] = &[];
        // English-only weights keep the historical `-l en` for auto-detect.
        assert_eq!(resolve_language(&en_only, "auto", no_dirs).unwrap().1, "en");
        assert_eq!(resolve_language(&en_only, "en", no_dirs).unwrap().1, "en");
        assert_eq!(
            resolve_language(&en_only, "es", no_dirs).unwrap_err().to_string(),
            "Transcribing in Spanish needs a multilingual whisper model (e.g. ggml-base.bin in <data-dir>/models or CF_WHISPER_MODEL). The configured model is English-only."
        );
        // ...unless a multilingual model is on disk to swap in.
        let dir = std::env::temp_dir().join(format!("cf-lang-test-{}", crate::util::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let alt = dir.join("ggml-base.bin");
        std::fs::write(&alt, b"multi").unwrap();
        assert_eq!(
            resolve_language(&en_only, "auto", std::slice::from_ref(&dir)).unwrap(),
            (alt.clone(), "auto".into())
        );
        assert_eq!(
            resolve_language(&en_only, "es", std::slice::from_ref(&dir)).unwrap(),
            (alt, "es".into())
        );
        std::fs::remove_dir_all(dir).ok();

        assert_eq!(resolve_language(&multi, "auto", no_dirs).unwrap().1, "auto");
        assert_eq!(resolve_language(&multi, "es", no_dirs).unwrap().1, "es");
        assert_eq!(resolve_language(&multi, "EN", no_dirs).unwrap().1, "en");
        assert!(resolve_language(&multi, "klingon", no_dirs).is_err());
    }

    /// Spanish fixture: whisper token offsets survive parsing and the words
    /// reach the ASS untouched — accents, inverted punctuation and all.
    #[test]
    fn spanish_words_render_into_captions() {
        let parsed = serde_json::json!({
            "result": {"language": "es"},
            "transcription": [{
                "offsets": {"from": 0, "to": 3000},
                "text": " ¿Qué hacemos con los niños esta noche?",
                "tokens": [
                    {"text": "[_BEG_]", "offsets": {"from": 0, "to": 0}, "p": 1.0},
                    {"text": " ¿", "offsets": {"from": 100, "to": 160}, "p": 0.95},
                    {"text": "Qu", "offsets": {"from": 160, "to": 260}, "p": 0.95},
                    {"text": "é", "offsets": {"from": 260, "to": 340}, "p": 0.95},
                    {"text": " hacemos", "offsets": {"from": 400, "to": 900}, "p": 0.92},
                    {"text": " con", "offsets": {"from": 900, "to": 1100}, "p": 0.94},
                    {"text": " los", "offsets": {"from": 1100, "to": 1300}, "p": 0.94},
                    {"text": " niños", "offsets": {"from": 1300, "to": 1700}, "p": 0.96},
                    {"text": " esta", "offsets": {"from": 1700, "to": 2000}, "p": 0.93},
                    {"text": " noche", "offsets": {"from": 2000, "to": 2500}, "p": 0.95},
                    {"text": "?", "offsets": {"from": 2500, "to": 2600}, "p": 0.9}
                ]
            }]
        });
        let words = parse_words(&parsed);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        // "¿" is non-lexical punctuation: it attaches to the word it opens,
        // and sub-word tokens ("Qu" + "é") merge into one word.
        assert!(texts.contains(&"¿Qué"), "{texts:?}");
        assert!(texts.contains(&"niños"), "{texts:?}");
        assert!(texts.contains(&"hacemos"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains('_')), "{texts:?}");

        let sentences = build_sentences(&words);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "¿Qué hacemos con los niños esta noche?");

        let input = crate::captions::CaptionInput {
            words: &words,
            clip_start_ms: 0,
            clip_end_ms: 3000,
            headline: "",
            font: "Inter",
            accent_bgr: crate::captions::accent_bgr_for(
                crate::captions::CaptionStyle::Impact,
                None,
            ),
            emoji_overlay: false,
            out_w: crate::render::OUT_W,
            out_h: crate::render::OUT_H,
        };
        let impact = crate::captions::build_ass(&input, crate::captions::CaptionStyle::Impact);
        assert!(impact.contains("NIÑOS"), "{impact}");
        let clean = crate::captions::build_ass(&input, crate::captions::CaptionStyle::Clean);
        assert!(clean.contains("niños"), "{clean}");
        assert!(clean.contains("noche"), "{clean}");
    }

    #[test]
    fn token_offsets_repair_reversed_and_overlapping_spans() {
        let parsed = serde_json::json!({
            "transcription": [{
                "tokens": [
                    {"text": " First", "offsets": {"from": 200, "to": 100}, "p": 0.9},
                    {"text": " second", "offsets": {"from": 150, "to": 300}, "p": 0.9}
                ]
            }]
        });

        let words = parse_words(&parsed);
        assert_eq!((words[0].start_ms, words[0].end_ms), (100, 175));
        assert_eq!((words[1].start_ms, words[1].end_ms), (175, 300));
    }
}
