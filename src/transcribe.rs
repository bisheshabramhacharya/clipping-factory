//! Local word-timestamp transcription via whisper.cpp (PRD §10).
//!
//! Word onsets come from whisper.cpp's DTW token alignment (`--dtw`), not its
//! token `offsets`: offsets are interpolated from segment timestamps and were
//! measured 150–350 ms off on median (up to ~900 ms at p90), often collapsing
//! several words onto one instant. Word ends are the next word's onset,
//! pulled back over any sustained silence measured on the same WAV. The
//! offsets path remains as a fallback when DTW output is unavailable.
//! Sentence/segment offsets are never treated as word boundaries because
//! they absorb pauses.

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
    let model_dirs =
        crate::config::model_search_dirs(&cfg.data_dir, &cwd, dirs::home_dir().as_deref());
    let (model, whisper_lang) = resolve_language(cfg, language.unwrap_or("auto"), &model_dirs)?;

    // Never let whisper write directly to a retry-visible name. A cancelled
    // process may leave a JSON prefix that looks parseable on the next run.
    let out_prefix = wav.with_file_name(format!(".whisper-{}", crate::util::short_id()));
    let json_candidates = [
        out_prefix.with_extension("json"),
        out_prefix.with_extension("whisper.json"),
    ];
    let mut args: Vec<String> = vec![
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
    let preset = dtw_preset(&model);
    let base_args = args.clone();
    if let Some(preset) = preset {
        // whisper.cpp silently drops DTW when flash attention is on.
        args.extend(["-nfa".into(), "--dtw".into(), preset.into()]);
    }

    let result: Result<Transcript> = async {
        let bin = bin.to_string_lossy();
        let mut outcome = run_whisper(&bin, &args, cancel, &mut on_progress).await;
        if preset.is_some() && !cancel.is_cancelled() {
            if let Err(e) = &outcome {
                // Older whisper-cli builds reject `-nfa`/`--dtw`; fall back
                // to plain token offsets rather than failing the stage.
                tracing::warn!("whisper-cli with DTW failed, retrying without it: {e:#}");
                outcome = run_whisper(&bin, &base_args, cancel, &mut on_progress).await;
            }
        }
        outcome.map_err(|e| {
            if crate::util::is_cancelled(&e) {
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

        let speech = {
            let wav = wav.to_path_buf();
            tokio::task::spawn_blocking(move || speech_frames(&wav))
                .await
                .ok()
                .flatten()
        };
        let words = parse_words(&parsed, preset.map(calibration), speech.as_deref());
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

async fn run_whisper<F>(
    bin: &str,
    args: &[String],
    cancel: &CancellationToken,
    on_progress: &mut F,
) -> Result<()>
where
    F: FnMut(f32),
{
    run_streaming(bin, args, cancel, |_is_err, line| {
        // whisper.cpp prints `whisper_print_progress_callback: progress = 35%`
        if let Some(idx) = line.find("progress =") {
            let tail = &line[idx + 10..];
            if let Ok(pct) = tail.trim().trim_end_matches('%').parse::<f32>() {
                on_progress((pct / 100.0).clamp(0.0, 1.0));
            }
        }
    })
    .await
}

/// whisper.cpp's `--dtw` alignment-head preset for a ggml model file
/// (`ggml-base.en.bin` → `base.en`, `ggml-large-v3-turbo-q5_0.bin` →
/// `large.v3.turbo`). `None` for names whisper.cpp has no preset for.
fn dtw_preset(model: &Path) -> Option<&'static str> {
    let name = model.file_name()?.to_str()?.to_ascii_lowercase();
    let stem = name.strip_prefix("ggml-")?.strip_suffix(".bin")?;
    // Quantized weights keep the same heads: drop a `-q5_0`-style suffix.
    let stem = match stem.rsplit_once('-') {
        Some((head, q))
            if q.starts_with('q') && q[1..].starts_with(|c: char| c.is_ascii_digit()) =>
        {
            head
        }
        _ => stem,
    };
    const PRESETS: &[(&str, &str)] = &[
        ("tiny", "tiny"),
        ("tiny.en", "tiny.en"),
        ("base", "base"),
        ("base.en", "base.en"),
        ("small", "small"),
        ("small.en", "small.en"),
        ("medium", "medium"),
        ("medium.en", "medium.en"),
        ("large-v1", "large.v1"),
        ("large-v2", "large.v2"),
        ("large-v3", "large.v3"),
        ("large-v3-turbo", "large.v3.turbo"),
    ];
    PRESETS
        .iter()
        .find(|(file, _)| *file == stem)
        .map(|(_, preset)| *preset)
}

/// How far before its DTW timestamp a word actually starts. whisper.cpp
/// stamps a token when the alignment path enters it, which lands late in the
/// token; the true onset sits between the previous token's stamp and this
/// one. Measured against forced alignment on real speech: tiny/base heads
/// run ~160 ms late, larger models ~320 ms. The cap keeps a pause before a
/// word from pulling its onset into the silence.
#[derive(Clone, Copy, Debug)]
struct Calibration {
    lead_frac: f64,
    lead_cap_ms: u64,
}

fn calibration(preset: &str) -> Calibration {
    if preset.starts_with("tiny") || preset.starts_with("base") {
        Calibration {
            lead_frac: 0.75,
            lead_cap_ms: 160,
        }
    } else {
        Calibration {
            lead_frac: 0.9,
            lead_cap_ms: 320,
        }
    }
}

/// Speech/silence per 10 ms frame of a 16-bit PCM WAV, or `None` when the
/// file can't be read or has too little level contrast to tell them apart.
fn speech_frames(wav: &Path) -> Option<Vec<bool>> {
    use std::io::Read;
    let mut file = std::io::BufReader::new(std::fs::File::open(wav).ok()?);
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    loop {
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk).ok()?;
        let size = u32::from_le_bytes(chunk[4..8].try_into().ok()?) as usize;
        if &chunk[0..4] == b"data" {
            break;
        }
        let mut body = vec![0u8; size + size % 2];
        file.read_exact(&mut body).ok()?;
        if &chunk[0..4] == b"fmt " && body.len() >= 16 {
            let format = u16::from_le_bytes([body[0], body[1]]);
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            bits = u16::from_le_bytes([body[14], body[15]]);
            if format != 1 {
                return None;
            }
        }
    }
    if bits != 16 || channels == 0 || rate < 1000 {
        return None;
    }
    let frame_bytes = (rate as usize / 100) * channels as usize * 2;
    let mut buf = vec![0u8; frame_bytes];
    let mut db = Vec::new();
    loop {
        let mut filled = 0;
        while filled < frame_bytes {
            match file.read(&mut buf[filled..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => filled += n,
            }
        }
        if filled < 2 {
            break;
        }
        let samples = filled / 2;
        let energy: f64 = buf[..samples * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| {
                let s = i16::from_le_bytes(*b) as f64;
                s * s
            })
            .sum();
        let rms = (energy / samples as f64).sqrt().max(1.0);
        db.push(20.0 * rms.log10());
        if filled < frame_bytes {
            break;
        }
    }
    speech_mask(&db)
}

/// Frames at least 30% of the way from the noise floor (10th percentile) to
/// the speech level (95th percentile) count as speech.
fn speech_mask(db: &[f64]) -> Option<Vec<bool>> {
    if db.len() < 50 {
        return None;
    }
    let mut sorted = db.to_vec();
    sorted.sort_by(f64::total_cmp);
    let floor = sorted[sorted.len() / 10];
    let level = sorted[sorted.len() * 95 / 100];
    if level - floor < 12.0 {
        return None;
    }
    let threshold = floor + 0.3 * (level - floor);
    Some(db.iter().map(|&d| d >= threshold).collect())
}

#[derive(Default)]
struct PendingWord {
    text: String,
    start_ms: u64,
    end_ms: u64,
    p_sum: f64,
    p_count: usize,
    /// DTW stamp of the word's first lexical token, and of the token before it.
    dtw_ms: Option<u64>,
    prev_dtw_ms: Option<u64>,
}

/// A word parsed from whisper tokens, before its final timing is chosen.
struct TokenWord {
    word: Word,
    dtw_ms: Option<u64>,
    prev_dtw_ms: Option<u64>,
}

fn finish_word(words: &mut Vec<TokenWord>, pending: &mut Option<PendingWord>) {
    let Some(word) = pending.take() else { return };
    if !word.text.chars().any(char::is_alphanumeric) {
        return;
    }
    words.push(TokenWord {
        word: Word {
            text: word.text,
            start_ms: word.start_ms,
            end_ms: word.end_ms.max(word.start_ms.saturating_add(10)),
            p: if word.p_count == 0 {
                0.5
            } else {
                (word.p_sum / word.p_count as f64) as f32
            },
        },
        dtw_ms: word.dtw_ms,
        prev_dtw_ms: word.prev_dtw_ms,
    });
}

/// whisper.cpp writes `t_dtw` in 10 ms units, `-1` when DTW was off.
fn token_dtw_ms(token: &serde_json::Value) -> Option<u64> {
    token["t_dtw"]
        .as_i64()
        .filter(|t| *t >= 0)
        .map(|t| t as u64 * 10)
}

fn parse_token_words(segments: &[serde_json::Value]) -> Vec<TokenWord> {
    let mut words = Vec::new();
    let mut saw_timed_token = false;
    let mut last_dtw: Option<u64> = None;

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
            let dtw = token_dtw_ms(token);
            let prev_dtw = last_dtw;
            last_dtw = dtw.or(last_dtw);
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
                    dtw_ms: dtw,
                    prev_dtw_ms: prev_dtw,
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
    words
}

/// Offset-based timing, the fallback when no DTW stamps are available.
fn offset_timing(words: Vec<TokenWord>) -> Vec<Word> {
    let mut words: Vec<Word> = words.into_iter().map(|w| w.word).collect();
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

/// Onsets never closer than this, so every word holds the highlight for at
/// least one frame at 25+ fps.
const MIN_ONSET_GAP_MS: u64 = 40;
/// A trailing silence at least this long ends the word before the next onset.
const MIN_TRAILING_SILENCE_MS: u64 = 150;
/// How much speech before the next onset may belong to that (late) onset.
const ONSET_SLACK_MS: u64 = 150;
/// Upper bounds for a single word's span (drawn-out words included).
const MAX_WORD_MS: u64 = 1500;
const LAST_WORD_MS: u64 = 800;

/// DTW timing: each onset is its token's stamp minus a calibrated lead, and
/// each word ends at the next onset unless the audio goes silent first.
fn dtw_timing(words: Vec<TokenWord>, calib: Calibration, speech: Option<&[bool]>) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::with_capacity(words.len());
    for tw in words {
        let at = tw.dtw_ms.unwrap_or(tw.word.start_ms);
        let gap = tw
            .prev_dtw_ms
            .map(|prev| at.saturating_sub(prev))
            .unwrap_or(calib.lead_cap_ms);
        let lead = ((gap as f64 * calib.lead_frac) as u64).min(calib.lead_cap_ms);
        let mut start_ms = at.saturating_sub(lead);
        if let Some(prev) = out.last() {
            start_ms = start_ms.max(prev.start_ms + MIN_ONSET_GAP_MS);
        }
        out.push(Word {
            start_ms,
            end_ms: start_ms,
            ..tw.word
        });
    }
    for i in 0..out.len() {
        let start = out[i].start_ms;
        let limit = out
            .get(i + 1)
            .map(|next| next.start_ms)
            .unwrap_or(start + LAST_WORD_MS)
            .min(start + MAX_WORD_MS);
        let mut end = limit;
        if let Some(mask) = speech {
            // Walk back from the limit over silent frames (past the end of
            // the audio counts as silent), never into the word's first 80 ms.
            // The next onset can land a little after its speech resumes, so
            // a short stretch of speech right before the limit is stepped
            // over first.
            let floor = ((start + 80) / 10) as usize;
            let speaking = |frame: usize| mask.get(frame).copied().unwrap_or(false);
            let mut frame = (limit / 10) as usize;
            let mut stepped = 0u64;
            while frame > floor && stepped < ONSET_SLACK_MS && speaking(frame - 1) {
                frame -= 1;
                stepped += 10;
            }
            let mut run = 0u64;
            while frame > floor && !speaking(frame - 1) {
                frame -= 1;
                run += 10;
            }
            if run >= MIN_TRAILING_SILENCE_MS {
                end = frame as u64 * 10 + 30;
            }
        }
        out[i].end_ms = end.max(start + 60).min(limit);
    }
    out
}

fn parse_words(
    v: &serde_json::Value,
    calib: Option<Calibration>,
    speech: Option<&[bool]>,
) -> Vec<Word> {
    let mut words = Vec::new();
    let Some(segments) = v["transcription"].as_array() else {
        return words;
    };
    let token_words = parse_token_words(segments);
    if !token_words.is_empty() {
        return match calib {
            Some(calib) if token_words.iter().all(|w| w.dtw_ms.is_some()) => {
                dtw_timing(token_words, calib, speech)
            }
            _ => offset_timing(token_words),
        };
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

/// Does this word's text end a sentence (terminal punctuation, allowing
/// closing quotes/brackets after the mark)? Abbreviations like "U.S." or
/// "Dr." don't: splitting there broke "three times U.S. electricity output"
/// into two fragments.
pub fn terminal_word(text: &str) -> bool {
    let t = text.trim_end_matches(['"', '\'', ')', ']']);
    t.ends_with(['.', '?', '!', '…']) && !is_title(t) && !is_initialism(t)
}

fn is_title(word: &str) -> bool {
    const TITLES: &[&str] = &[
        "mr.", "mrs.", "ms.", "dr.", "st.", "vs.", "jr.", "sr.", "prof.",
    ];
    TITLES.contains(&word.to_lowercase().as_str())
}

/// Dotted initialisms: "U.S.", "e.g.", "A.I." — letters alternating with dots.
fn is_initialism(word: &str) -> bool {
    word.len() >= 4
        && word.chars().enumerate().all(|(i, c)| {
            if i % 2 == 0 {
                c.is_alphabetic()
            } else {
                c == '.'
            }
        })
}

/// Whether a sentence ends after `words[i]`. An initialism ends one only
/// when the next word is capitalized: "…out of the U.S. Reading between the
/// lines" breaks, "three times U.S. electricity output" doesn't.
pub fn ends_sentence(words: &[Word], i: usize) -> bool {
    let text = words[i].text.trim_end_matches(['"', '\'', ')', ']']);
    terminal_word(text)
        || is_initialism(text)
            && words
                .get(i + 1)
                .and_then(|n| n.text.chars().next())
                .is_some_and(char::is_uppercase)
}

/// Group words into sentence-like segments: break after terminal punctuation,
/// on long pauses, or when a segment grows unreasonably large.
pub fn build_sentences(words: &[Word]) -> Vec<Sentence> {
    let mut sentences = Vec::new();
    let mut start_idx = 0usize;
    let mut char_len = 0usize;

    for i in 0..words.len() {
        char_len += words[i].text.len() + 1;
        let terminal = ends_sentence(words, i);
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

    #[test]
    fn an_initialism_ends_a_sentence_only_before_a_capital() {
        let w = |text: &str, start_ms: u64| Word {
            text: text.into(),
            start_ms,
            end_ms: start_ms + 200,
            p: 1.0,
        };
        let words = vec![
            w("three", 0),
            w("times", 300),
            w("U.S.", 600),
            w("output", 900),
            w("in", 1200),
            w("the", 1500),
            w("U.S.", 1800),
            w("Reading", 2100),
            w("on.", 2400),
        ];
        let texts: Vec<String> = build_sentences(&words)
            .into_iter()
            .map(|s| s.text)
            .collect();
        assert_eq!(
            texts,
            vec!["three times U.S. output in the U.S.", "Reading on."]
        );
    }

    #[test]
    fn abbreviations_do_not_end_a_sentence() {
        assert!(!terminal_word("U.S."));
        assert!(!terminal_word("Dr."));
        assert!(!terminal_word("e.g."));
        assert!(terminal_word("output."));
        assert!(terminal_word("US."));
        assert!(terminal_word("why?"));
    }

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

        let words = parse_words(&parsed, None, None);
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

        let words = parse_words(&parsed, None, None);
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
        let words = parse_words(&parsed, None, None);
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

        let words = parse_words(&parsed, None, None);
        assert_eq!((words[0].start_ms, words[0].end_ms), (100, 175));
        assert_eq!((words[1].start_ms, words[1].end_ms), (175, 300));
    }

    #[test]
    fn dtw_presets_follow_the_model_file_name() {
        let preset = |name: &str| dtw_preset(Path::new(name));
        assert_eq!(preset("models/ggml-base.en.bin"), Some("base.en"));
        assert_eq!(preset("ggml-small.en.bin"), Some("small.en"));
        assert_eq!(preset("ggml-base.bin"), Some("base"));
        assert_eq!(preset("ggml-large-v3-turbo.bin"), Some("large.v3.turbo"));
        assert_eq!(
            preset("ggml-large-v3-turbo-q5_0.bin"),
            Some("large.v3.turbo")
        );
        assert_eq!(preset("ggml-medium.en-q8_0.bin"), Some("medium.en"));
        assert_eq!(preset("custom-finetune.bin"), None);
        assert_eq!(preset("ggml-distil-whatever.bin"), None);
    }

    fn dtw_tok(text: &str, from: u64, to: u64, dtw: i64) -> serde_json::Value {
        serde_json::json!({"text": text, "offsets": {"from": from, "to": to}, "p": 0.9, "t_dtw": dtw})
    }

    /// The offsets below collapse "within the bubble" onto one instant, as
    /// whisper.cpp did on a real podcast; DTW stamps keep the words apart.
    fn collapsed_offsets_fixture() -> serde_json::Value {
        serde_json::json!({"transcription": [{
            "offsets": {"from": 7000, "to": 9500},
            "tokens": [
                dtw_tok("[_BEG_]", 7000, 7000, -1),
                dtw_tok(" within", 7680, 7680, 780),
                dtw_tok(" the", 7680, 7680, 802),
                dtw_tok(" bubble", 7680, 7680, 822),
                dtw_tok(",", 7680, 7700, 840),
                dtw_tok(" outside", 7780, 8510, 900),
            ]
        }]})
    }

    #[test]
    fn dtw_onsets_replace_collapsed_offsets() {
        let words = parse_words(
            &collapsed_offsets_fixture(),
            Some(calibration("base.en")),
            None,
        );
        let starts: Vec<u64> = words.iter().map(|w| w.start_ms).collect();
        // Onset = stamp − min(0.75 × gap to the previous stamp, 160 ms); the
        // first word has no previous stamp and takes 0.75 × 160.
        assert_eq!(starts, vec![7680, 7860, 8070, 8840]);
        assert_eq!(words[2].text, "bubble,");
        for pair in words.windows(2) {
            assert!(pair[0].end_ms <= pair[1].start_ms, "{words:?}");
            assert!(pair[0].end_ms > pair[0].start_ms, "{words:?}");
        }
    }

    #[test]
    fn onsets_stay_a_frame_apart() {
        let parsed = serde_json::json!({"transcription": [{"tokens": [
            dtw_tok(" a", 0, 0, 100),
            dtw_tok(" b", 0, 0, 101),
            dtw_tok(" c", 0, 0, 102),
        ]}]});
        let words = parse_words(&parsed, Some(calibration("small.en")), None);
        let starts: Vec<u64> = words.iter().map(|w| w.start_ms).collect();
        // "c" would start at 1011, only 10 ms after "b".
        assert_eq!(starts, vec![712, 1001, 1041]);
    }

    #[test]
    fn dtw_output_is_ignored_without_a_calibration_or_when_stamps_are_missing() {
        let offsets = parse_words(&collapsed_offsets_fixture(), None, None);
        assert_eq!(offsets[0].start_ms, 7680);
        assert_eq!(offsets[1].start_ms, offsets[0].start_ms + 10);

        let no_dtw = serde_json::json!({"transcription": [{"tokens": [
            dtw_tok(" hello", 100, 400, -1),
            dtw_tok(" there", 400, 800, -1),
        ]}]});
        let words = parse_words(&no_dtw, Some(calibration("base.en")), None);
        assert_eq!(
            words
                .iter()
                .map(|w| (w.start_ms, w.end_ms))
                .collect::<Vec<_>>(),
            vec![(100, 400), (400, 800)]
        );
    }

    #[test]
    fn a_word_before_a_pause_ends_where_the_voice_stops() {
        let parsed = serde_json::json!({"transcription": [{"tokens": [
            dtw_tok(" first", 0, 0, 20),
            dtw_tok(" second", 0, 0, 40),
            dtw_tok(" third", 0, 0, 210),
        ]}]});
        // 10 ms frames: speech to 0.6 s, silence to 1.9 s, then speech. The
        // "third" onset (1.94 s) lands just after its speech resumes.
        let mask: Vec<bool> = (0..300).map(|f| !(60..190).contains(&f)).collect();
        let calib = calibration("base.en");
        let words = parse_words(&parsed, Some(calib), Some(&mask));
        assert_eq!(words[1].text, "second");
        assert_eq!((words[1].start_ms, words[1].end_ms), (250, 630));
        assert_eq!(words[2].start_ms, 1940);
        // Without audio the word runs toward the next onset, capped at 1.5 s.
        let words = parse_words(&parsed, Some(calib), None);
        assert_eq!(words[1].end_ms, 250 + MAX_WORD_MS);
        assert_eq!(words[0].end_ms, words[1].start_ms);
    }

    #[test]
    fn speech_mask_needs_level_contrast() {
        assert!(speech_mask(&[-30.0; 200]).is_none());
        let mut db = vec![-70.0; 100];
        db.extend([-20.0; 100]);
        let mask = speech_mask(&db).unwrap();
        assert!(!mask[10] && mask[150]);
    }

    #[test]
    fn speech_frames_reads_16_bit_pcm_wavs() {
        let rate = 16_000u32;
        let mut samples: Vec<i16> = vec![0; rate as usize / 2];
        samples.extend((0..rate as usize / 2).map(|i| ((i as f64 * 0.3).sin() * 8000.0) as i16));
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + data.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(rate.to_le_bytes());
        wav.extend((rate * 2).to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data.len() as u32).to_le_bytes());
        wav.extend(&data);
        let path = std::env::temp_dir().join(format!("cf-speech-{}.wav", crate::util::short_id()));
        std::fs::write(&path, wav).unwrap();
        let mask = speech_frames(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(mask.len(), 100);
        assert!(mask[..50].iter().all(|s| !s));
        assert!(mask[50..].iter().all(|s| *s));
    }
}
