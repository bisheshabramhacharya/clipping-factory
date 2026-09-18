//! Speaker diarization — who speaks when, fully offline (PRD framing ticket:
//! speaker-aware framing). No cloud, no speech-text model: the pipeline is
//!
//! 1. **VAD by transcript**: whisper word timings already mark speech —
//!    words merged over short gaps give utterance spans, and because the
//!    same transcript drives captions, a word always lands inside exactly
//!    one turn (crop switches and caption labels can never disagree).
//! 2. **Embeddings**: each span is sliced into fixed windows, each window
//!    runs through an ONNX speaker-embedding model ([`Config::speaker_model`];
//!    16 kHz mono waveform in, vector out — e.g. a SpeechBrain ECAPA-TDNN
//!    export), and the span's speaker vector is the mean of its windows.
//! 3. **Clustering**: greedy centroid clustering on cosine similarity, in
//!    time order; labels are assigned in first-appearance order ("S1" is
//!    whoever talks first). Short backchannels ("yeah", "right") too small
//!    to embed get absorbed into the surrounding turn instead of stealing it.
//!
//! Everything after step 2's model call is deterministic and unit-tested on
//! synthetic fixtures. Without a configured model the stage never runs and
//! the project simply has no `speakers.json` — framing falls back to the
//! speaker-free paths (split-screen for two-face shots, single-face lock
//! otherwise).

use crate::config::Config;
use crate::domain::{Diarization, SpeakerTurn, Word};
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Whisper word gap that splits two utterances. Shorter pauses stay inside
/// one span (and one turn's reach); longer ones mark a possible handoff.
const SPAN_GAP_MS: u64 = 320;
/// A span must be at least this long to carry a trustworthy embedding.
/// Shorter spans are too little signal; they inherit the turn around them.
const MIN_EMBED_SPAN_MS: u64 = 700;
/// Embedding window length and hop inside a span.
const WINDOW_MS: u64 = 1500;
const WINDOW_HOP_MS: u64 = 600;
/// Long spans cap their window count — embedding cost stays linear in
/// speech time and uniform coverage beats dense sampling at the head.
const MAX_SPAN_WINDOWS: usize = 6;
/// Global cap on embedding calls — worst-case diarization stays bounded on
/// multi-hour sources.
const MAX_WINDOWS: usize = 1200;
/// Cosine-similarity floor for folding a span into an existing speaker.
/// Speaker-embedding models separate voices comfortably above 0.3–0.5;
/// the midpoint keeps same-speaker spans together without absorbing
/// genuinely different voices.
const SPEAKER_SIM: f32 = 0.55;
/// Two-host podcasts are the target; the clustering can surface more, but
/// framing and labels only ever name the loudest voices — cap so a noisy
/// source can't spray labels.
const MAX_SPEAKERS: usize = 4;
/// PCM rate the embedder consumes (matches `media::extract_audio`).
const SAMPLE_RATE: usize = 16_000;
/// Tolerance for locating a speech span inside the decoded PCM — the WAV
/// is produced by us at exactly 16 kHz mono, so this is purely defensive.
const PCM_SLACK_SAMPLES: usize = SAMPLE_RATE; // ±1 s

/// Run diarization over the project's extracted 16 kHz mono WAV.
/// `Ok(None)` when there is no model configured — callers treat None as
/// "no speaker info" and continue with speaker-free layouts.
pub async fn diarize(
    cfg: &Config,
    wav: &Path,
    words: &[Word],
    cancel: &CancellationToken,
) -> Result<Option<Diarization>> {
    let Some(model) = cfg.speaker_model.clone() else {
        return Ok(None);
    };

    let spans = speech_spans(words);
    let embeddable: Vec<usize> = (0..spans.len())
        .filter(|&i| spans[i].1 - spans[i].0 >= MIN_EMBED_SPAN_MS)
        .collect();
    if embeddable.len() < 2 {
        // One voice in a monotone stretch (or pure silence) — nothing to
        // separate. Not an error: the pipeline still labels "S1".
        return Ok(single_speaker(&spans, words));
    }

    let pcm = read_wav_f32(wav).await?;
    if pcm.is_empty() {
        return Ok(None);
    }

    let windows = plan_windows(&spans, &embeddable);
    if windows.is_empty() {
        return Ok(None);
    }

    let cancelled = Arc::new(AtomicBool::new(cancel.is_cancelled()));
    let watcher_flag = cancelled.clone();
    let watcher_token = cancel.clone();
    let watcher = tokio::spawn(async move {
        watcher_token.cancelled().await;
        watcher_flag.store(true, Ordering::Relaxed);
    });
    let worker_flag = cancelled.clone();
    let result =
        tokio::task::spawn_blocking(move || embed_spans(&model, &pcm, &windows, &worker_flag))
            .await;
    watcher.abort();
    let span_embeddings: Vec<(usize, Vec<f32>)> = result??;
    if cancelled.load(Ordering::Relaxed) || cancel.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    let labels = cluster_embeddings(&span_embeddings);
    Ok(Some(build_diarization(&spans, &labels, words)))
}

/// Speech spans from word timings: consecutive words separated by less than
/// `SPAN_GAP_MS` share a span. Returns `(start_ms, end_ms)` pairs — the
/// first word's onset to the last word's offset, per span.
pub fn speech_spans(words: &[Word]) -> Vec<(u64, u64)> {
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for w in words {
        match spans.last_mut() {
            Some(last) if w.start_ms.saturating_sub(last.1) < SPAN_GAP_MS => {
                last.1 = last.1.max(w.end_ms);
            }
            _ => spans.push((w.start_ms, w.end_ms)),
        }
    }
    spans
}

/// Sample points for embedding: `(span_idx, start_ms, len_ms)` windows.
/// A span ≥ WINDOW_MS gets evenly placed windows; a shorter embeddable
/// span gets exactly one covering its whole length.
fn plan_windows(spans: &[(u64, u64)], embeddable: &[usize]) -> Vec<(usize, u64, u64)> {
    let mut windows: Vec<(usize, u64, u64)> = Vec::new();
    for &si in embeddable {
        let (s, e) = spans[si];
        let dur = e - s;
        if dur <= WINDOW_MS {
            windows.push((si, s, dur));
            continue;
        }
        let n = ((dur - WINDOW_MS) / WINDOW_HOP_MS + 1).min(MAX_SPAN_WINDOWS as u64) as usize;
        // Even spread so windows cover the span's full extent, not its head.
        let hop = if n > 1 {
            (dur - WINDOW_MS) / (n as u64 - 1)
        } else {
            0
        };
        for i in 0..n {
            windows.push((si, s + hop * i as u64, WINDOW_MS));
        }
    }
    // Bound total inference cost on very talkative sources.
    if windows.len() > MAX_WINDOWS {
        let stride = windows.len() as f64 / MAX_WINDOWS as f64;
        windows = (0..MAX_WINDOWS)
            .map(|i| windows[(i as f64 * stride) as usize])
            .collect();
    }
    windows
}

/// Decode our own `pcm_s16le` 16 kHz mono WAV into f32 samples in [-1, 1].
async fn read_wav_f32(path: &Path) -> Result<Vec<f32>> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    // Minimal WAV walk: find the `data` chunk (fmt chunks vary in size).
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" {
        return Err(anyhow!("not a RIFF/WAV file"));
    }
    let mut cursor = 12usize;
    while cursor + 8 <= bytes.len() {
        let tag = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let body = cursor + 8;
        if tag == b"data" {
            let end = (body + size).min(bytes.len());
            return Ok(bytes[body..end]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                .collect());
        }
        cursor = body + size + (size % 2); // chunks are word-aligned
    }
    Err(anyhow!("WAV file has no data chunk"))
}

/// Embed every planned window through the ONNX model and mean-pool per
/// span. Synchronous CPU inference — callers wrap this in `spawn_blocking`.
fn embed_spans(
    model: &Path,
    pcm: &[f32],
    windows: &[(usize, u64, u64)],
    cancelled: &AtomicBool,
) -> Result<Vec<(usize, Vec<f32>)>> {
    let mut session = ort::session::Session::builder()
        .and_then(|mut b| b.commit_from_file(model))
        .map_err(|e| anyhow!("speaker model failed to load: {e}"))?;
    let input_name = session
        .inputs()
        .first()
        .map(|o| o.name().to_string())
        .ok_or_else(|| anyhow!("speaker model declares no inputs"))?;

    let mut sums: std::collections::HashMap<usize, Vec<f32>> = std::collections::HashMap::new();
    let mut counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &(si, start_ms, len_ms) in windows {
        if cancelled.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        let from = start_ms as usize * SAMPLE_RATE / 1000;
        let to = ((start_ms + len_ms) as usize * SAMPLE_RATE / 1000).min(pcm.len());
        if to <= from + PCM_SLACK_SAMPLES / 16 {
            continue; // window fell outside the decoded audio entirely
        }
        let slice = &pcm[from.min(pcm.len())..to.max(from + 1).min(pcm.len())];
        let emb = embed_window(&mut session, &input_name, slice)?;
        let sum = sums.entry(si).or_insert_with(|| vec![0.0; emb.len()]);
        for (a, b) in sum.iter_mut().zip(emb.iter()) {
            *a += b;
        }
        *counts.entry(si).or_insert(0) += 1;
    }
    Ok(sums
        .into_iter()
        .filter_map(|(si, sum)| {
            let n = *counts.get(&si).unwrap_or(&0);
            (n > 0).then(|| (si, l2_normalize(&sum)))
        })
        .collect())
}

fn embed_window(
    session: &mut ort::session::Session,
    input_name: &str,
    pcm: &[f32],
) -> Result<Vec<f32>> {
    let tensor = ort::value::Tensor::from_array((vec![1i64, pcm.len() as i64], pcm.to_vec()))
        .context("building input tensor")?;
    let outputs = session
        .run(ort::inputs![input_name => tensor])
        .context("speaker embedding inference")?;
    let (_shape, data) = outputs[0]
        .try_extract_tensor::<f32>()
        .context("speaker model output is not an f32 tensor")?;
    Ok(l2_normalize(data))
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        v.iter().map(|x| x / norm).collect()
    } else {
        v.to_vec()
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Greedy centroid clustering over `(span_idx, embedding)` in time order:
/// a span joins the nearest centroid above `SPEAKER_SIM`, otherwise founds
/// a new cluster. Deterministic; labels come out in clustering order and
/// get renumbered by first appearance afterwards.
/// Returns `Option<u8>` per span index — `None` for unembedded spans.
pub fn cluster_embeddings(span_embeddings: &[(usize, Vec<f32>)]) -> Vec<Option<u8>> {
    let max_span = span_embeddings
        .iter()
        .map(|(i, _)| *i)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);
    let mut labels: Vec<Option<u8>> = vec![None; max_span];
    let mut centroids: Vec<Vec<f32>> = Vec::new();
    let mut masses: Vec<usize> = Vec::new();

    let mut ordered = span_embeddings.to_vec();
    ordered.sort_by_key(|(i, _)| *i);
    for (si, emb) in ordered {
        let best = centroids
            .iter()
            .enumerate()
            .map(|(ci, c)| (ci, cosine(&emb, c)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        match best {
            Some((ci, sim)) if sim >= SPEAKER_SIM || centroids.len() >= MAX_SPEAKERS => {
                labels[si] = Some(ci as u8);
                masses[ci] += 1;
                // Running centroid: weight by mass so early outliers fade.
                let n = masses[ci] as f32;
                for (c, e) in centroids[ci].iter_mut().zip(emb.iter()) {
                    *c = *c * (n - 1.0) / n + e / n;
                }
                centroids[ci] = l2_normalize(&centroids[ci]);
            }
            _ => {
                labels[si] = Some(centroids.len() as u8);
                centroids.push(emb.clone());
                masses.push(1);
            }
        }
    }
    labels
}

/// Fold per-span labels into speaker turns: consecutive same-speaker spans
/// merge, gaps between different speakers split at the gap midpoint, and
/// unlabeled spans (too short to embed) attach to the surrounding turn —
/// a backchannel doesn't steal the camera.
pub fn build_diarization(
    spans: &[(u64, u64)],
    labels: &[Option<u8>],
    words: &[Word],
) -> Diarization {
    // Resolve each span's speaker, absorbing unlabeled spans into the
    // previous labeled turn when the next labeled span disagrees or is
    // unknown. A leading run of unlabeled spans takes the next label.
    let mut resolved: Vec<Option<u8>> = labels.to_vec();
    if let Some(next) = resolved.iter().position(|l| l.is_some()) {
        for l in resolved.iter_mut().take(next) {
            *l = labels[next];
        }
    }
    for i in 0..resolved.len() {
        if resolved[i].is_none() {
            resolved[i] = resolved.get(i.wrapping_sub(1)).copied().flatten();
        }
    }

    // Renumber by first appearance so "S1" is whoever talks first.
    let mut order: Vec<u8> = Vec::new();
    for l in resolved.iter().flatten() {
        if !order.contains(l) {
            order.push(*l);
        }
    }
    let rename = |s: u8| order.iter().position(|&o| o == s).unwrap_or(0) as u8;

    let mut turns: Vec<SpeakerTurn> = Vec::new();
    for (i, spk) in resolved.iter().enumerate() {
        let Some(spk) = spk.map(rename) else { continue };
        let (s, e) = spans[i];
        match turns.last_mut() {
            Some(t) if t.speaker == spk => t.end_ms = e,
            _ => {
                // Boundary sits at the midpoint of the silence between the
                // previous turn's last word and this span's first word.
                let start = turns
                    .last()
                    .map(|t| t.end_ms + (s.saturating_sub(t.end_ms)) / 2)
                    .unwrap_or(s);
                turns.push(SpeakerTurn {
                    start_ms: start,
                    end_ms: e,
                    speaker: spk,
                });
            }
        }
    }

    let n = order.len().max(1);
    Diarization {
        labels: (0..n).map(|i| format!("S{}", i + 1)).collect(),
        turns,
    }
    .with_words_covered(words)
}

impl Diarization {
    /// Ensure every transcript word lands inside some turn: extend the
    /// nearest turn over orphans (they can appear in unlabeled gaps at the
    /// edges of short spans). Boundaries otherwise stand.
    fn with_words_covered(mut self, words: &[Word]) -> Self {
        for w in words {
            if self.word_speaker(w).is_none() {
                let mid = w.start_ms + w.end_ms.saturating_sub(w.start_ms) / 2;
                // Extend whichever turn is closest.
                if let Some(t) = self
                    .turns
                    .iter_mut()
                    .min_by_key(|t| t.start_ms.abs_diff(mid).min(t.end_ms.abs_diff(mid)))
                {
                    t.start_ms = t.start_ms.min(mid);
                    t.end_ms = t.end_ms.max(mid);
                }
            }
        }
        self
    }
}

/// A source where nothing separable was heard: label the whole speech
/// region S1 — honest, and lets captions/tagging stay uniform.
fn single_speaker(spans: &[(u64, u64)], words: &[Word]) -> Option<Diarization> {
    let (first, last) = (words.first()?, words.last()?);
    let _ = spans;
    Some(Diarization {
        labels: vec!["S1".into()],
        turns: vec![SpeakerTurn {
            start_ms: first.start_ms,
            end_ms: last.end_ms,
            speaker: 0,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(start_ms: u64, end_ms: u64) -> Word {
        Word {
            text: "x".into(),
            start_ms,
            end_ms,
            p: 0.9,
        }
    }

    fn emb(base: f32, seed: f32) -> Vec<f32> {
        // Two well-separated synthetic voices live near unit vectors that
        // only share one coordinate axis faintly.
        l2_normalize(&[base, 1.0 - base, seed])
    }

    // ---- speech spans ----

    #[test]
    fn speech_spans_merge_short_gaps_and_split_long_ones() {
        let words = vec![
            w(0, 300),
            w(450, 700),   // 150 ms gap — same utterance
            w(1500, 1800), // 800 ms gap — new span
            w(2000, 2300), // 200 ms gap — same span
        ];
        assert_eq!(speech_spans(&words), vec![(0, 700), (1500, 2300)]);
    }

    #[test]
    fn speech_spans_empty_for_empty_words() {
        assert!(speech_spans(&[]).is_empty());
    }

    // ---- clustering ----

    #[test]
    fn two_voices_cluster_into_two_speakers() {
        let embeds: Vec<(usize, Vec<f32>)> = (0..8)
            .map(|i| {
                let voice = if i % 2 == 0 {
                    emb(0.95, 0.02)
                } else {
                    emb(0.05, 0.95)
                };
                (i, voice)
            })
            .collect();
        let labels = cluster_embeddings(&embeds);
        // Alternating voices → alternating stable labels.
        for (i, l) in labels.iter().enumerate() {
            assert_eq!(*l, Some(if i % 2 == 0 { 0 } else { 1 }), "span {i}");
        }
    }

    #[test]
    fn one_voice_stays_one_cluster() {
        let embeds: Vec<(usize, Vec<f32>)> =
            (0..5).map(|i| (i, emb(0.9, i as f32 * 0.01))).collect();
        let labels = cluster_embeddings(&embeds);
        assert!(labels.iter().all(|l| *l == Some(0)), "{labels:?}");
    }

    // ---- turn building ----

    #[test]
    fn turns_merge_same_speaker_and_split_at_gap_midpoints() {
        // spans: A talks 0–2s, pause, B 3–4s, pause, A 5–6s.
        let spans = vec![(0u64, 2000u64), (3000, 4000), (5000, 6000)];
        let labels = vec![Some(0u8), Some(1u8), Some(0u8)];
        let words = vec![w(0, 500), w(3100, 3500), w(5100, 5500)];
        let d = build_diarization(&spans, &labels, &words);
        assert_eq!(d.turns.len(), 3);
        assert_eq!(d.turns[0].speaker, 0);
        assert_eq!(d.turns[1].speaker, 1);
        // Boundary = midpoint of the 2000–3000 gap → 2500.
        assert_eq!(d.turns[1].start_ms, 2500);
        assert_eq!(d.turns[0].end_ms, 2000);
        assert_eq!(d.turns[2].speaker, 0);
        assert_eq!(d.labels, vec!["S1", "S2"]);
    }

    #[test]
    fn unlabeled_spans_inherit_the_turn_around_them() {
        // B's short "yeah" (unlabeled) between A's long turns stays A.
        let spans = vec![(0u64, 2000u64), (2200, 2400), (3000, 5000)];
        let labels = vec![Some(0u8), None, Some(1u8)];
        let words = vec![w(0, 500), w(2200, 2400), w(3100, 3500)];
        let d = build_diarization(&spans, &labels, &words);
        // The middle span takes the previous turn's speaker → turn[0]
        // extends through it; turn[1] starts at midpoint of 2400..3000.
        assert_eq!(d.turns[0].speaker, 0);
        assert_eq!(d.turns[0].end_ms, 2400);
        assert_eq!(d.turns[1].start_ms, 2700);
        // Every word is covered by some turn.
        assert!(words.iter().all(|w| d.word_speaker(w).is_some()));
    }

    #[test]
    fn labels_follow_first_appearance_not_cluster_index() {
        // Speaker B (cluster index 1) talks first → becomes S1.
        let spans = vec![(0u64, 1000u64), (2000, 3000)];
        let labels = vec![Some(1u8), Some(0u8)];
        let words = vec![w(0, 500), w(2100, 2500)];
        let d = build_diarization(&spans, &labels, &words);
        assert_eq!(d.turns[0].speaker, 0); // was cluster 1 → now S1
        assert_eq!(d.turns[1].speaker, 1);
    }

    #[test]
    fn word_speaker_maps_midpoints() {
        let d = Diarization {
            labels: vec!["S1".into(), "S2".into()],
            turns: vec![
                SpeakerTurn {
                    start_ms: 0,
                    end_ms: 2000,
                    speaker: 0,
                },
                SpeakerTurn {
                    start_ms: 2500,
                    end_ms: 5000,
                    speaker: 1,
                },
            ],
        };
        assert_eq!(d.word_speaker(&w(100, 400)), Some(0));
        assert_eq!(d.word_speaker(&w(2600, 2900)), Some(1));
        // A word midpoint in the silence gap between turns → None (caller
        // decides); word inside the second turn → S2.
        assert_eq!(d.speaker_at(2250), None);
    }

    #[test]
    fn every_word_gets_a_turn_via_extension() {
        // A word stranded in an unlabeled edge gap still ends up labeled.
        let spans = vec![(1000u64, 2000u64)];
        let labels = vec![Some(0u8)];
        let words = vec![w(500, 800), w(1100, 1500)];
        let d = build_diarization(&spans, &labels, &words);
        assert!(d.word_speaker(&words[0]).is_some());
    }
}
