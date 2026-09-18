//! Deterministic candidate validator (PRD §9.3).
//!
//! Every rule here is pure and unit-tested: score thresholds, duration bounds
//! (with the explicit exception path), timestamp bounds, word-boundary
//! snapping, verbatim quote matching, >30% overlap suppression against
//! higher-ranked candidates, and the scene-transition guard. The validator is
//! the final authority — the LLM only proposes.

use crate::domain::*;

const MIN_MS: u64 = 20_000;
const MAX_MS: u64 = 90_000;
/// Exception envelope (PRD §9.3 "without an explicit validator exception"):
/// slightly-out-of-range candidates pass only when unusually strong.
const EXC_MIN_MS: u64 = 15_000;
const EXC_MAX_MS: u64 = 110_000;
const MAX_OVERLAP: f64 = 0.30;
/// Ranking sweet spot (not a bound): durations platforms actually reward.
const SWEET_MIN_MS: u64 = 25_000;
const SWEET_MAX_MS: u64 = 60_000;
/// Half-width of the transition around a detected scene boundary (ms): a cut
/// inside this window lands on the crossfade itself.
const TRANSITION_HALF_MS: u64 = 500;
/// A closing word without terminal punctuation is a mid-sentence cut only
/// when speech resumes quickly — a ≥700 ms gap reads as an intended stop.
const TRAILING_SPEECH_MS: u64 = 700;
/// Room tone kept around the snapped boundary words so no clip opens or
/// closes on a hard sample edge.
const LEAD_PAD_MS: u64 = 80;
const TAIL_PAD_MS: u64 = 250;
/// The pad never reaches all the way to the neighboring word — eating the
/// neighbor's first/last samples would be worse than cutting tight.
const PAD_MARGIN_MS: u64 = 20;

/// Cold-open defense: a clip may not open on greetings, housekeeping, or a
/// non-lexical filler run — the canonical "AI clip" tells that announce the
/// cut wasn't editorial. Openers already mid-thought (connectives like
/// "so", "and") stay fine; only the tells get rejected.
const GREETING_OPENERS: &[&str] = &[
    "welcome to",
    "welcome back",
    "hey everybody",
    "hey everyone",
    "hey guys",
    "hello everyone",
    "hello everybody",
    "good morning",
    "good afternoon",
    "good evening",
    "today we are going to",
    "today we re going to",
    "in this video",
    "in this episode",
    "before we get started",
    "thanks for tuning in",
    "thanks for watching",
    "thanks for joining",
];
const NON_LEXICAL_OPENERS: &[&str] = &["um", "uh", "er", "ah", "hmm", "mhm"];

/// Outro-bait closers — a clip ending on a channel CTA reads as an ad for the
/// source, not a standalone moment. Normalized forms (no apostrophes).
const CTA_CLOSERS: &[&str] = &[
    "like and subscribe",
    "like comment and subscribe",
    "smash that like",
    "hit the bell",
    "hit that subscribe",
    "hit subscribe",
    "link in the description",
    "links in the description",
    "comment below",
    "let me know in the comments",
    "follow for more",
    "subscribe for more",
    "see you next time",
    "until next time",
    "thanks for watching",
    "thanks for tuning in",
    "thank you for watching",
    "dont forget to subscribe",
    "don t forget to subscribe",
    "dont forget to like",
    "don t forget to like",
];

/// Returns a reason when the clip's last words are an outro CTA.
fn cta_close_reason(last_words: &[crate::domain::Word]) -> Option<String> {
    let tail: Vec<&str> = last_words
        .iter()
        .rev()
        .take(10)
        .map(|w| w.text.as_str())
        .collect();
    let joined = normalize(&tail.into_iter().rev().collect::<Vec<_>>().join(" "));
    CTA_CLOSERS
        .iter()
        .find(|c| joined.ends_with(*c))
        .map(|c| format!("closes on outro/CTA '{c}'"))
}

fn cold_open_reason(first_words: &[crate::domain::Word]) -> Option<String> {
    let joined = normalize(
        &first_words
            .iter()
            .take(8)
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
    if let Some(g) = GREETING_OPENERS.iter().find(|g| joined.starts_with(*g)) {
        return Some(format!("opens on greeting/housekeeping '{g}'"));
    }
    let first_norm = normalize(&first_words.first()?.text);
    if NON_LEXICAL_OPENERS.contains(&first_norm.as_str()) {
        return Some(format!("opens on filler word '{first_norm}'"));
    }
    None
}

pub fn validate(
    candidates: Vec<Candidate>,
    transcript: &Transcript,
    source_duration_ms: u64,
    selector: String,
    scene_boundaries: &[u64],
) -> SelectionReport {
    let mut evaluated: Vec<Result<(Candidate, bool, f32), RejectedCandidate>> = Vec::new();
    let mut scene_bounds = scene_boundaries.to_vec();
    scene_bounds.sort_unstable();
    scene_bounds.dedup();

    for mut cand in candidates {
        let mut reasons: Vec<String> = Vec::new();

        // --- Timestamp bounds (PRD §15) ---------------------------------
        if cand.start_ms >= cand.end_ms {
            reasons.push("start time is not before end time".into());
        }
        if cand.end_ms > source_duration_ms + 500 {
            reasons.push(format!(
                "end timestamp {} is outside the source duration {}",
                fmt_ms(cand.end_ms),
                fmt_ms(source_duration_ms)
            ));
        }
        if !reasons.is_empty() {
            evaluated.push(Err(RejectedCandidate {
                candidate: cand,
                reasons,
            }));
            continue;
        }

        // --- Snap to real word timestamps (PRD §9.1) ---------------------
        if let Some((s, e)) = snap_to_words(transcript, cand.start_ms, cand.end_ms) {
            cand.start_ms = s;
            cand.end_ms = e;
        } else {
            evaluated.push(Err(RejectedCandidate {
                candidate: cand,
                reasons: vec!["interval contains no transcribed words".into()],
            }));
            continue;
        }

        // --- Scene-transition guard ---------------------------------------
        if let Err(reason) = clear_scene_transitions(transcript, &scene_bounds, &mut cand) {
            evaluated.push(Err(RejectedCandidate {
                candidate: cand,
                reasons: vec![reason],
            }));
            continue;
        }

        // --- Boundary completeness ----------------------------------------
        // The clip may not close mid-sentence while speech continues: a
        // non-terminal closing word followed by speech within 700 ms means
        // the cut lands inside a thought. (Mid-thought cold OPENS are
        // intentional; the start side stays loose on purpose.)
        let words = &transcript.words;
        let fi = words.iter().position(|w| w.end_ms > cand.start_ms);
        let li = words.iter().rposition(|w| w.start_ms < cand.end_ms);
        if let (Some(fi), Some(li)) = (fi, li) {
            let first = &words[fi];
            let last = &words[li];
            if let Some(reason) = cold_open_reason(&words[fi..]) {
                reasons.push(reason);
            }
            if let Some(reason) = cta_close_reason(&words[..li + 1]) {
                reasons.push(reason);
            }
            if !crate::transcribe::terminal_word(&last.text) {
                let continues = words
                    .get(li + 1)
                    .map(|n| n.start_ms.saturating_sub(last.end_ms) < TRAILING_SPEECH_MS)
                    .unwrap_or(false);
                if continues {
                    reasons
                        .push("ends mid-sentence with speech continuing under 0.7s later".into());
                }
            }
            // Breathing-room pads applied to the final (post-scene-guard)
            // boundary: a little room tone so the clip never opens or closes
            // on a hard sample edge, capped by the actual silence gap so the
            // pad can never swallow a neighboring word's samples.
            let lead_gap = fi
                .checked_sub(1)
                .map(|p| first.start_ms.saturating_sub(words[p].end_ms))
                .unwrap_or(first.start_ms);
            cand.start_ms = first
                .start_ms
                .saturating_sub(lead_gap.saturating_sub(PAD_MARGIN_MS).min(LEAD_PAD_MS));
            let tail_gap = words
                .get(li + 1)
                .map(|n| n.start_ms.saturating_sub(last.end_ms))
                .unwrap_or(source_duration_ms.saturating_sub(last.end_ms));
            cand.end_ms = (last.end_ms + tail_gap.saturating_sub(PAD_MARGIN_MS).min(TAIL_PAD_MS))
                .min(source_duration_ms);
        }

        // --- Score thresholds (PRD §9.3) ---------------------------------
        let s = cand.scores;
        if s.self_contained < 4 {
            reasons.push(format!("self_contained {} is below 4", s.self_contained));
        }
        if s.opening_strength < 4 {
            reasons.push(format!(
                "opening_strength {} is below 4",
                s.opening_strength
            ));
        }
        if s.payoff < 3 {
            reasons.push(format!("payoff {} is below 3", s.payoff));
        }
        if s.clarity < 4 {
            reasons.push(format!("clarity {} is below 4", s.clarity));
        }
        if s.context_dependency > 2 {
            reasons.push(format!(
                "context_dependency {} is above 2",
                s.context_dependency
            ));
        }
        if s.slop_risk > 2 {
            reasons.push(format!("slop_risk {} is above 2", s.slop_risk));
        }

        // --- Duration with explicit exception path ------------------------
        let dur = cand.end_ms - cand.start_ms;
        let mut duration_exception = false;
        if !(MIN_MS..=MAX_MS).contains(&dur) {
            let exceptional = s.payoff >= 4 && s.self_contained >= 5 && s.clarity >= 4;
            if (EXC_MIN_MS..=EXC_MAX_MS).contains(&dur) && exceptional {
                duration_exception = true;
            } else {
                reasons.push(format!(
                    "duration {}s falls outside 20–90 seconds",
                    dur / 1000
                ));
            }
        }

        // --- Verbatim quote matching (PRD §9.3) ---------------------------
        let excerpt = excerpt_text(transcript, cand.start_ms, cand.end_ms);
        let excerpt_norm = normalize(&excerpt);
        for (label, quote, near_start) in [
            ("opening", &cand.opening_quote, true),
            ("closing", &cand.closing_quote, false),
        ] {
            let qn = normalize(quote);
            if qn.is_empty() {
                reasons.push(format!("{} quote is empty", label));
                continue;
            }
            let position = if near_start {
                excerpt_norm.find(&qn)
            } else {
                excerpt_norm.rfind(&qn)
            };
            match position {
                None => reasons.push(format!(
                    "{} quote cannot be found in the transcribed excerpt",
                    label
                )),
                Some(pos) => {
                    let len = excerpt_norm.len().max(1);
                    let zone = (len / 4).min(160);
                    let ok = if near_start {
                        pos <= zone
                    } else {
                        pos + qn.len() >= len.saturating_sub(zone)
                    };
                    if !ok {
                        reasons.push(format!(
                            "{} quote is not near the {} of the excerpt",
                            label,
                            if near_start { "start" } else { "end" }
                        ));
                    }
                }
            }
        }

        if reasons.is_empty() {
            // Ranking nudge only: 25–60s is the short-form sweet spot
            // (Shorts cap 60s; viral clips cluster under ~45s). Bounds
            // and the exception path above are untouched.
            let duration_bonus = if (SWEET_MIN_MS..=SWEET_MAX_MS).contains(&dur) {
                0.75
            } else {
                0.0
            };
            let composite = composite_score(&s) + duration_bonus;
            evaluated.push(Ok((cand, duration_exception, composite)));
        } else {
            evaluated.push(Err(RejectedCandidate {
                candidate: cand,
                reasons,
            }));
        }
    }

    // --- Rank survivors, then suppress >30% overlaps ----------------------
    let mut rejected: Vec<RejectedCandidate> = Vec::new();
    let mut passing: Vec<(Candidate, bool, f32)> = Vec::new();
    for item in evaluated {
        match item {
            Ok(v) => passing.push(v),
            Err(r) => rejected.push(r),
        }
    }
    passing.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut accepted: Vec<ValidatedCandidate> = Vec::new();
    for (cand, duration_exception, composite) in passing {
        let candidate_dur = (cand.end_ms - cand.start_ms).max(1) as f64;
        let too_much_overlap = accepted.iter().any(|a| {
            let inter = crate::select::overlap_ms(
                a.candidate.start_ms,
                a.candidate.end_ms,
                cand.start_ms,
                cand.end_ms,
            ) as f64;
            let contains_higher_ranked =
                cand.start_ms <= a.candidate.start_ms && cand.end_ms >= a.candidate.end_ms;
            contains_higher_ranked || inter / candidate_dur > MAX_OVERLAP
        });
        if too_much_overlap {
            rejected.push(RejectedCandidate {
                candidate: cand,
                reasons: vec!["overlaps more than 30% with a higher-ranked clip".into()],
            });
            continue;
        }
        accepted.push(ValidatedCandidate {
            rank: accepted.len() + 1,
            candidate: cand,
            composite,
            duration_exception,
        });
    }

    SelectionReport {
        selector,
        accepted,
        rejected,
    }
}

pub fn composite_score(s: &Scores) -> f32 {
    s.self_contained as f32 * 2.0
        + s.payoff as f32 * 1.6
        + s.opening_strength as f32 * 1.4
        + s.clarity as f32 * 1.2
        + s.tension_or_novelty as f32 * 1.0
        + s.specificity as f32 * 0.8
        - s.context_dependency as f32 * 1.5
        - s.slop_risk as f32 * 2.0
}

/// Snap a proposed interval to real word timestamps: the start of the word
/// containing (or nearest after) `start`, and the end of the word containing
/// (or nearest before) `end`.
pub fn snap_to_words(t: &Transcript, start: u64, end: u64) -> Option<(u64, u64)> {
    let words = &t.words;
    if words.is_empty() {
        return None;
    }
    let first = words
        .iter()
        .find(|w| w.end_ms > start)
        .map(|w| w.start_ms)?;
    let last = words
        .iter()
        .rev()
        .find(|w| w.start_ms < end)
        .map(|w| w.end_ms)?;
    if first >= last {
        return None;
    }
    Some((first, last))
}

/// Scene-transition guard: a Clip may neither open/close inside a detected
/// transition nor span a boundary mid-interval. A cut inside a transition
/// window is moved to the nearest word boundary clear of it — the same
/// word-timestamp snapping rules as `snap_to_words` (open on a word start,
/// close on a word end). Returns the rejection reason when the interval
/// cannot be cleared.
fn clear_scene_transitions(
    t: &Transcript,
    boundaries: &[u64],
    cand: &mut Candidate,
) -> Result<(), String> {
    // Nudge the opening cut past any transition containing it, landing on the
    // first word that starts after the transition ends.
    while let Some(&b) = boundaries
        .iter()
        .find(|&&b| cand.start_ms.abs_diff(b) <= TRANSITION_HALF_MS)
    {
        let after = b.saturating_add(TRANSITION_HALF_MS);
        match t.words.iter().find(|w| w.start_ms > after) {
            Some(w) if w.start_ms < cand.end_ms => cand.start_ms = w.start_ms,
            _ => {
                return Err(format!("opens inside a scene transition at {}", fmt_ms(b)));
            }
        }
    }
    // Nudge the closing cut before any transition containing it, landing on
    // the last word that ends before the transition starts.
    while let Some(&b) = boundaries
        .iter()
        .find(|&&b| cand.end_ms.abs_diff(b) <= TRANSITION_HALF_MS)
    {
        let before = b.saturating_sub(TRANSITION_HALF_MS);
        match t.words.iter().rev().find(|w| w.end_ms < before) {
            Some(w) if w.end_ms > cand.start_ms => cand.end_ms = w.end_ms,
            _ => {
                return Err(format!("closes inside a scene transition at {}", fmt_ms(b)));
            }
        }
    }
    if let Some(&b) = boundaries
        .iter()
        .find(|&&b| cand.start_ms < b && b < cand.end_ms)
    {
        return Err(format!("spans a scene transition at {}", fmt_ms(b)));
    }
    Ok(())
}

pub fn excerpt_text(t: &Transcript, start: u64, end: u64) -> String {
    t.words
        .iter()
        .filter(|w| w.start_ms >= start && w.end_ms <= end)
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lowercase, alphanumeric + single spaces — tolerant matching for
/// punctuation/casing differences while preserving wording.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim_end().to_string()
}

/// Average word confidence inside an interval — used to surface the PRD §10
/// low-confidence warning on affected clips.
pub fn interval_confidence(t: &Transcript, start: u64, end: u64) -> f32 {
    let mut sum = 0.0f32;
    let mut n = 0usize;
    for w in &t.words {
        if w.start_ms >= start && w.end_ms <= end {
            sum += w.p;
            n += 1;
        }
    }
    if n == 0 {
        1.0
    } else {
        sum / n as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(n_words: usize, word_ms: u64) -> Transcript {
        let mut words = Vec::new();
        for i in 0..n_words {
            let t0 = i as u64 * word_ms;
            words.push(Word {
                text: format!("word{}.", i),
                start_ms: t0,
                end_ms: t0 + word_ms - 50,
                p: 0.9,
            });
        }
        let sentences = crate::transcribe::build_sentences(&words);
        Transcript {
            language: "en".into(),
            words,
            sentences,
            avg_confidence: 0.9,
        }
    }

    fn good_scores() -> Scores {
        Scores {
            self_contained: 5,
            opening_strength: 4,
            specificity: 4,
            tension_or_novelty: 4,
            payoff: 5,
            clarity: 5,
            context_dependency: 1,
            slop_risk: 1,
        }
    }

    fn cand(t: &Transcript, start: u64, end: u64, scores: Scores) -> Candidate {
        Candidate {
            start_ms: start,
            end_ms: end,
            headline: "A test headline".into(),
            opening_quote: excerpt_head(t, start, end, 5),
            closing_quote: excerpt_tail(t, start, end, 5),
            selection_reason: "test".into(),
            scores,
        }
    }

    fn excerpt_head(t: &Transcript, s: u64, e: u64, n: usize) -> String {
        excerpt_text(t, s, e)
            .split_whitespace()
            .take(n)
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn excerpt_tail(t: &Transcript, s: u64, e: u64, n: usize) -> String {
        let text = excerpt_text(t, s, e);
        let v: Vec<&str> = text.split_whitespace().collect();
        v[v.len().saturating_sub(n)..].join(" ")
    }

    const SRC: u64 = 600_000;

    #[test]
    fn accepts_a_good_candidate() {
        let t = transcript(1500, 400); // 600s of words
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "test".into(), &[]);
        assert_eq!(r.accepted.len(), 1);
        assert_eq!(r.rejected.len(), 0);
        assert_eq!(r.accepted[0].rank, 1);
    }

    #[test]
    fn rejects_each_score_threshold() {
        let t = transcript(1500, 400);
        for (field, value) in [
            ("self_contained", 3u8),
            ("opening_strength", 3),
            ("payoff", 2),
            ("clarity", 3),
            ("context_dependency", 3),
            ("slop_risk", 3),
        ] {
            let mut s = good_scores();
            match field {
                "self_contained" => s.self_contained = value,
                "opening_strength" => s.opening_strength = value,
                "payoff" => s.payoff = value,
                "clarity" => s.clarity = value,
                "context_dependency" => s.context_dependency = value,
                _ => s.slop_risk = value,
            }
            let r = validate(
                vec![cand(&t, 10_000, 50_000, s)],
                &t,
                SRC,
                "test".into(),
                &[],
            );
            assert_eq!(r.accepted.len(), 0, "{} should reject", field);
            assert!(
                r.rejected[0].reasons[0].contains(field),
                "reason should name {}: {:?}",
                field,
                r.rejected[0].reasons
            );
        }
    }

    #[test]
    fn rejects_out_of_range_duration() {
        let t = transcript(1500, 400);
        // 10s — too short even for the exception.
        let r = validate(
            vec![cand(&t, 10_000, 20_000, good_scores())],
            &t,
            SRC,
            "t".into(),
            &[],
        );
        assert_eq!(r.accepted.len(), 0);
        // 150s — too long even for the exception.
        let r = validate(
            vec![cand(&t, 10_000, 160_000, good_scores())],
            &t,
            SRC,
            "t".into(),
            &[],
        );
        assert_eq!(r.accepted.len(), 0);
    }

    #[test]
    fn duration_exception_requires_exceptional_scores() {
        let t = transcript(1500, 400);
        // 17s, exceptional scores → accepted with the exception flag.
        let r = validate(
            vec![cand(&t, 10_000, 27_000, good_scores())],
            &t,
            SRC,
            "t".into(),
            &[],
        );
        assert_eq!(r.accepted.len(), 1);
        assert!(r.accepted[0].duration_exception);
        // 17s, mediocre payoff → rejected.
        let mut s = good_scores();
        s.payoff = 3;
        let r = validate(vec![cand(&t, 10_000, 27_000, s)], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
    }

    #[test]
    fn rejects_timestamps_outside_source() {
        let t = transcript(1500, 400);
        let c = cand(&t, 590_000, 640_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0].reasons[0].contains("outside the source"));
    }

    #[test]
    fn rejects_inverted_interval() {
        let t = transcript(1500, 400);
        let c = cand(&t, 50_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
    }

    #[test]
    fn snaps_to_word_boundaries() {
        let t = transcript(1500, 400);
        // Propose an interval starting mid-word: word at 10_000..10_350.
        let c = cand(&t, 10_133, 50_177, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 1);
        let a = &r.accepted[0].candidate;
        assert_eq!(
            (a.start_ms + 30) % 400,
            0,
            "start snapped to a word start minus lead pad"
        );
        assert_eq!(
            (a.end_ms - 30 + 50) % 400,
            0,
            "end snapped to a word end plus tail pad"
        );
    }

    #[test]
    fn rejects_unfindable_quotes() {
        let t = transcript(1500, 400);
        let mut c = cand(&t, 10_000, 50_000, good_scores());
        c.opening_quote = "words that were never spoken".into();
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0].reasons[0].contains("opening quote"));
    }

    #[test]
    fn closing_quote_uses_the_occurrence_nearest_the_excerpt_end() {
        let mut t = transcript(200, 400);
        for start in [10usize, 190] {
            for (offset, text) in ["we", "finally", "got", "there"].iter().enumerate() {
                t.words[start + offset].text = (*text).into();
            }
        }
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let mut c = cand(&t, 0, 80_000, good_scores());
        c.closing_quote = "we finally got there".into();
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 1, "reasons: {:?}", r.rejected);
    }

    #[test]
    fn closing_quote_in_the_middle_is_not_close_enough_to_the_excerpt_end() {
        let mut t = transcript(200, 400);
        for (offset, text) in ["we", "finally", "got", "there"].iter().enumerate() {
            t.words[150 + offset].text = (*text).into();
        }
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let mut c = cand(&t, 0, 80_000, good_scores());
        c.closing_quote = "we finally got there".into();
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("closing quote is not near the end")));
    }

    #[test]
    fn suppresses_overlap_above_30_percent() {
        let t = transcript(1500, 400);
        let strong = cand(&t, 10_000, 70_000, good_scores());
        let mut weaker_scores = good_scores();
        weaker_scores.tension_or_novelty = 3;
        // 40s candidate overlapping 30s with `strong` → 75% overlap → rejected.
        let overlapping = cand(&t, 40_000, 80_000, weaker_scores);
        // Distant candidate survives.
        let distant = cand(&t, 200_000, 250_000, weaker_scores);
        let r = validate(vec![strong, overlapping, distant], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 2);
        assert_eq!(r.rejected.len(), 1);
        assert!(r.rejected[0].reasons[0].contains("overlaps"));
        // Ranks are 1..n in composite order.
        assert_eq!(r.accepted[0].rank, 1);
        assert_eq!(r.accepted[1].rank, 2);
    }

    #[test]
    fn overlap_at_25_percent_is_allowed() {
        let t = transcript(1500, 400);
        let a = cand(&t, 10_000, 70_000, good_scores()); // 60s
        let mut s2 = good_scores();
        s2.tension_or_novelty = 3;
        // 60s candidate sharing 10s with `a` → 16% overlap → allowed.
        let b = cand(&t, 60_000, 120_000, s2);
        let r = validate(vec![a, b], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 2);
    }

    #[test]
    fn a_candidate_containing_a_higher_ranked_clip_is_rejected_as_overlap() {
        let t = transcript(1500, 400);
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../evals/fixtures/overlap_containment.json"))
                .unwrap();
        let interval = |name: &str| {
            (
                fixture[name]["start_ms"].as_u64().unwrap(),
                fixture[name]["end_ms"].as_u64().unwrap(),
            )
        };
        assert_eq!(fixture["expected"], "reject-lower-ranked-containing-clip");
        let (strong_start, strong_end) = interval("higher_ranked");
        let (containing_start, containing_end) = interval("lower_ranked");
        let strong = cand(&t, strong_start, strong_end, good_scores());
        let mut weaker_scores = good_scores();
        weaker_scores.tension_or_novelty = 3;
        let containing = cand(&t, containing_start, containing_end, weaker_scores);
        let r = validate(vec![strong, containing], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 1);
        assert_eq!(r.rejected.len(), 1);
        assert!(r.rejected[0].reasons[0].contains("overlaps"));
    }

    #[test]
    fn small_overlap_with_a_shorter_higher_ranked_clip_is_allowed() {
        let t = transcript(1500, 400);
        let strong = cand(&t, 10_000, 30_000, good_scores());
        let mut weaker_scores = good_scores();
        weaker_scores.tension_or_novelty = 3;
        let mostly_distinct = cand(&t, 23_000, 113_000, weaker_scores);
        let r = validate(vec![strong, mostly_distinct], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 2, "reasons: {:?}", r.rejected);
    }

    #[test]
    fn normalize_is_punctuation_and_case_tolerant() {
        assert_eq!(normalize("Hello,   WORLD!"), "hello world");
        assert_eq!(normalize("don't-stop"), "don t stop");
    }

    #[test]
    fn sweet_spot_duration_ranks_above_equal_scored_long_clip() {
        let t = transcript(1500, 400);
        let short = cand(&t, 10_000, 40_000, good_scores()); // 30s
        let long = cand(&t, 200_000, 280_000, good_scores()); // 80s, still in bounds
        let r = validate(vec![long, short], &t, SRC, "test".into(), &[]);
        assert_eq!(r.accepted.len(), 2);
        let dur = |i: usize| r.accepted[i].candidate.end_ms - r.accepted[i].candidate.start_ms;
        assert!(
            dur(0) < 50_000,
            "rank 1 should be the ~30s clip, got {}ms",
            dur(0)
        );
        assert!(r.accepted[0].composite > r.accepted[1].composite);
    }

    #[test]
    fn zero_candidates_yields_clean_empty_report() {
        let t = transcript(100, 400);
        let r = validate(vec![], &t, SRC, "t".into(), &[]);
        assert!(r.accepted.is_empty());
        assert!(r.rejected.is_empty());
    }

    // --- Scene-transition guard -------------------------------------------

    #[test]
    fn a_candidate_spanning_a_scene_boundary_is_rejected() {
        let t = transcript(1500, 400);
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[30_000]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("spans a scene transition")));
    }

    #[test]
    fn an_opening_cut_inside_a_transition_snaps_to_the_next_word() {
        let t = transcript(1500, 400);
        // Boundary at 10_100 → window 9_600–10_600; the proposed cut at
        // 10_000 (word 25) lands inside, so the guard re-snaps to the first
        // word starting after the window: word 27 at 10_800.
        let mut c = cand(&t, 10_000, 50_000, good_scores());
        c.opening_quote = excerpt_head(&t, 10_800, 50_000, 5);
        let r = validate(vec![c], &t, SRC, "t".into(), &[10_100]);
        assert_eq!(r.accepted.len(), 1, "reasons: {:?}", r.rejected);
        let a = &r.accepted[0].candidate;
        assert_eq!(a.start_ms, 10_800 - 30);
        assert_eq!(
            (a.start_ms + 30) % 400,
            0,
            "start snapped to a word start minus lead pad"
        );
        assert_eq!(
            (a.end_ms - 30 + 50) % 400,
            0,
            "end snapped to a word end plus tail pad"
        );
    }

    #[test]
    fn a_closing_cut_inside_a_transition_snaps_to_the_prior_word() {
        let t = transcript(1500, 400);
        // Boundary at 49_500 → window 49_000–50_000; the word-snapped cut at
        // 49_950 lands inside, so the guard re-snaps to the last word ending
        // before the window: word 121 ends at 48_750.
        let mut c = cand(&t, 10_000, 50_000, good_scores());
        c.closing_quote = excerpt_tail(&t, 10_000, 48_750, 5);
        let r = validate(vec![c], &t, SRC, "t".into(), &[49_500]);
        assert_eq!(r.accepted.len(), 1, "reasons: {:?}", r.rejected);
        assert_eq!(r.accepted[0].candidate.end_ms, 48_750 + 30);
    }

    #[test]
    fn a_cut_inside_a_transition_that_cannot_clear_is_rejected() {
        let t = transcript(1500, 400);
        // Only 700 ms of room before the transition: no word past it can keep
        // the interval non-empty.
        let c = cand(&t, 200, 700, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[300]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("opens inside a scene transition")));

        // Symmetric case on the closing cut: the last word ending before the
        // window (word 23 at 9_550) is not past the already-placed start.
        let c = cand(&t, 9_600, 10_300, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[10_300]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("closes inside a scene transition")));
    }

    #[test]
    fn boundaries_outside_transition_windows_leave_candidates_alone() {
        let t = transcript(1500, 400);
        // 50_600 is just past the snapped end (49_950) and 300_000 is far away;
        // unsorted input is fine.
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[300_000, 50_600]);
        assert_eq!(r.accepted.len(), 1, "reasons: {:?}", r.rejected);
        assert_eq!(r.accepted[0].candidate.start_ms, 10_000 - 30);
        assert_eq!(r.accepted[0].candidate.end_ms, 49_950 + 30);
    }

    // --- Boundary completeness --------------------------------------------

    #[test]
    fn rejects_a_clip_ending_mid_sentence_while_speech_continues() {
        let mut t = transcript(1500, 400);
        // Strip the period off the closing word; the next word starts 50 ms
        // later, so the cut lands inside a live sentence.
        t.words[124].text = "word124".into();
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("mid-sentence")));
    }

    #[test]
    fn rejects_a_clip_opening_on_a_greeting() {
        let mut t = transcript(1500, 400);
        let greeting = ["welcome", "back", "to", "the", "show", "everybody."];
        for (i, text) in greeting.iter().enumerate() {
            t.words[25 + i].text = (*text).into();
        }
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("greeting")));
    }

    #[test]
    fn rejects_a_clip_opening_on_filler() {
        let mut t = transcript(1500, 400);
        t.words[25].text = "um".into();
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("filler")));
    }

    #[test]
    fn a_mid_thought_connective_opener_is_allowed() {
        let mut t = transcript(1500, 400);
        // "So" reads as mid-thought, not housekeeping — the intended opener.
        t.words[25].text = "so".into();
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 1, "reasons: {:?}", r.rejected);
    }

    #[test]
    fn rejects_a_clip_closing_on_outro_cta() {
        let mut t = transcript(1500, 400);
        let outro = [
            "if",
            "you",
            "enjoyed",
            "this",
            "make",
            "sure",
            "to",
            "like",
            "and",
            "subscribe.",
        ];
        for (i, text) in outro.iter().enumerate() {
            t.words[114 + i].text = (*text).into(); // word 123 ("subscribe.") at 49.2s
        }
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let c = cand(&t, 10_000, 49_600, good_scores()); // ends on the CTA word
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 0);
        assert!(r.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("outro/CTA")));
    }

    #[test]
    fn a_content_word_close_is_allowed() {
        let t = transcript(1500, 400);
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 1, "reasons: {:?}", r.rejected);
    }

    #[test]
    fn a_non_terminal_end_is_allowed_after_a_natural_pause() {
        let mut t = transcript(1500, 400);
        // The closing word (ends 48_350) has no period, but the next word
        // starts 800 ms later — a real pause reads as an intended stop.
        t.words[120].text = "word120".into();
        for w in &mut t.words[121..] {
            w.start_ms += 750;
            w.end_ms += 750;
        }
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let c = cand(&t, 10_000, 48_400, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted.len(), 1, "reasons: {:?}", r.rejected);
    }

    #[test]
    fn boundaries_keep_a_little_room_tone() {
        // 50 ms inter-word gaps clamp both pads to 30 ms.
        let t = transcript(1500, 400);
        let c = cand(&t, 10_000, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        let a = &r.accepted[0].candidate;
        assert_eq!(a.start_ms, 10_000 - 30);
        assert_eq!(a.end_ms, 49_950 + 30);
    }

    #[test]
    fn pads_take_the_full_allowance_inside_real_silence() {
        let mut t = transcript(1500, 400);
        // 400+ ms of silence after the clip's last word (word 74, shifted to
        // end 30_450, next word at 30_900) and 500 ms before its first
        // (word 25 starts 10_500).
        for w in &mut t.words[25..] {
            w.start_ms += 500;
            w.end_ms += 500;
        }
        for w in &mut t.words[75..] {
            w.start_ms += 400;
            w.end_ms += 400;
        }
        t.sentences = crate::transcribe::build_sentences(&t.words);
        let c = cand(&t, 10_400, 30_350, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        let a = &r.accepted[0].candidate;
        assert_eq!(a.start_ms, 10_500 - 80);
        assert_eq!(a.end_ms, 30_450 + 250);
    }

    #[test]
    fn the_first_word_of_the_source_gets_no_leading_pad_before_zero() {
        let t = transcript(1500, 400);
        let c = cand(&t, 0, 50_000, good_scores());
        let r = validate(vec![c], &t, SRC, "t".into(), &[]);
        assert_eq!(r.accepted[0].candidate.start_ms, 0);
    }
}
