//! Offline heuristic selector.
//!
//! Runs when no AI key is configured (and in tests). It is deliberately
//! conservative: it looks for sentence windows that open with a hook, develop
//! one idea, and close on a resolution cue, then scores them honestly on the
//! same 1–5 rubric so the deterministic validator applies identical rules.
//! Quotes are exact transcript substrings, so faithfulness is guaranteed.

use crate::domain::{Candidate, Scores, Sentence, Transcript};
use crate::select::overlap_ms;

const HOOK_STARTS: &[&str] = &[
    "what",
    "why",
    "how",
    "here's",
    "heres",
    "the biggest",
    "the problem",
    "the thing",
    "most people",
    "nobody",
    "everyone",
    "everybody",
    "if you",
    "let me tell",
    "the truth",
    "people think",
    "you know what",
    "the mistake",
    "one thing",
    "my favorite",
    "the best",
    "the worst",
    "i learned",
    "i realized",
    "the secret",
    "stop",
    "never",
    "always",
];

const CONTRAST_CUES: &[&str] = &[
    "but ",
    "actually",
    "the truth is",
    "turns out",
    "instead",
    "wrong",
    "mistake",
    "nobody talks",
    "don't realize",
    "dont realize",
    "counterintuitive",
    "surprised",
    "the opposite",
    "not what you think",
    "myth",
    "lie",
];

const PAYOFF_CUES: &[&str] = &[
    "so ",
    "that's why",
    "thats why",
    "which means",
    "the lesson",
    "at the end of the day",
    "that is what",
    "and that's",
    "and thats",
    "the point is",
    "that changed",
    "ever since",
    "now i",
    "the answer",
    "it works because",
    "that's how",
    "thats how",
];

const REFERENCE_CUES: &[&str] = &[
    "as i said",
    "like i said",
    "as i mentioned",
    "like i mentioned",
    "we talked about",
    "going back to",
    "earlier i",
    "mentioned earlier",
    "said earlier",
    "as we discussed",
];

const PRONOUN_OPENERS: &[&str] = &[
    "that", "this", "it", "he", "she", "they", "which", "those", "and", "so",
    // Connectives and reactions lean on the line before them.
    "but", "or", "because", "yeah", "like", "um", "uh", "well",
];

const FILLER_WORDS: &[&str] = &["um", "uh", "like", "you know", "kind of", "sort of"];

// These are production/editorial blockers, not quality signals. A local
// selector should never turn routine show housekeeping or an ad read into a
// clip merely because the window happens to be long enough.
const HOUSEKEEPING_OR_SPONSOR_CUES: &[&str] = &[
    "welcome back to the show",
    "today we are going to talk about",
    "before we begin",
    "subscribe and leave a review",
    "we will be right back",
    "we'll be right back",
    "thanks for listening",
    "now let us get into the conversation",
    "now let's get into the conversation",
    "this episode is brought to you by",
    "brought to you by",
    "sponsored by",
    "use the code",
    "discount at checkout",
    "in the episode notes",
    "in the show notes",
    "sponsor link",
    "thanks to our sponsor",
    "support for the show",
    // Channel outros.
    "if you enjoyed this",
    "watch the full episode",
    "full episode here",
    "and subscribe",
];

const MIN_MS: u64 = 20_000;
const MAX_MS: u64 = 90_000;
/// Upper bounds of the clip-length bands each start keeps a best end in.
const END_BANDS: [u64; 3] = [40_000, 60_000, MAX_MS];

/// Function words dropped from the focus prompt before keyword matching, plus
/// the request boilerplate users naturally type ("clips about", "the part
/// where"). Topical words survive — they are the signal.
const FOCUS_STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "for", "with", "about", "at",
    "by", "is", "are", "was", "were", "be", "been", "it", "its", "that", "this", "these", "those",
    "they", "them", "their", "where", "when", "what", "who", "how", "why", "do", "does", "did",
    "i", "we", "you", "me", "my", "our", "your", "his", "her", "he", "she", "from", "into", "any",
    "all", "get", "find", "keep", "make", "show", "clip", "clips", "moment", "moments", "part",
    "parts", "section", "bit", "video",
];

/// Keywords extracted from the free-text focus prompt: lowercased,
/// punctuation-stripped, deduplicated, stopwords and stray short tokens
/// removed. A term needs ≥3 letters or a digit to count as topical.
fn focus_terms(focus: Option<&str>) -> Vec<String> {
    let Some(focus) = focus.map(str::trim).filter(|f| !f.is_empty()) else {
        return Vec::new();
    };
    let mut terms: Vec<String> = Vec::new();
    for word in normalized_claim(focus).split_whitespace() {
        let keep = !FOCUS_STOPWORDS.contains(&word)
            && (word.chars().count() >= 3 || word.chars().any(|c| c.is_ascii_digit()));
        if keep && !terms.iter().any(|t| t == word) {
            terms.push(word.to_string());
        }
    }
    terms
}

/// The episode title's topical terms and its adjacent topical word pairs
/// ("utterly dominate"), from a filename like
/// "_China Will Utterly Dominate_ Without This – Elon Musk (1080p).mp4".
fn title_terms(title: Option<&str>) -> (Vec<String>, Vec<String>) {
    let Some(title) = title else {
        return (Vec::new(), Vec::new());
    };
    let stem = title.rsplit_once('.').map(|(a, _)| a).unwrap_or(title);
    let mut clean = String::new();
    let mut depth = 0usize;
    for c in stem.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => clean.push(if c == '_' { ' ' } else { c }),
            _ => {}
        }
    }
    let words: Vec<String> = normalized_claim(&clean)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let topical = |w: &str| w.chars().count() >= 3 && !FOCUS_STOPWORDS.contains(&w);
    let pairs = words
        .windows(2)
        .filter(|p| topical(&p[0]) && topical(&p[1]))
        .map(|p| format!("{} {}", p[0], p[1]))
        .collect();
    (focus_terms(Some(&clean)), pairs)
}

/// Stem-lite match: a term hits a window word on exact match, when the
/// shorter token is a prefix of the longer ("cat"~"cats"), or when it is one
/// letter shy of a prefix ("price"~"pricing", "argue"~"arguing"). The shared
/// stem must be ≥4 chars so stray short words like "art"~"party" don't match.
fn term_hits_word(term: &str, word: &str) -> bool {
    if term == word {
        return true;
    }
    let (short, long) = if term.len() <= word.len() {
        (term, word)
    } else {
        (word, term)
    };
    long.starts_with(short) && short.len() >= 3
        || short.len() >= 5 && long.starts_with(&short[..short.len() - 1])
}

/// Propose candidates; an optional loudness profile adds a modest composite
/// boost to high-energy windows (see [`crate::energy`]). An optional focus
/// prompt ("clips about pricing") steers ranking toward windows whose
/// transcript text matches its keywords — topical windows also pass the
/// editorial-signal gate, since the user asked for the topic directly.
/// Blank or absent focus keeps generic best-moments ranking unchanged.
/// `title` is the uploaded filename: episodes are usually named for their
/// best moment, so windows that say the title's words get a moderate boost.
pub fn propose(
    t: &Transcript,
    source_duration_ms: u64,
    proposal_count: usize,
    energy: Option<&crate::energy::EnergyProfile>,
    focus: Option<&str>,
    title: Option<&str>,
) -> Vec<Candidate> {
    let sentences = &t.sentences;
    if sentences.is_empty() {
        return Vec::new();
    }
    let focus_terms = focus_terms(focus);
    let (title_terms, title_pairs) = title_terms(title);

    let mut scored: Vec<(f32, Candidate)> = Vec::new();

    for start_idx in 0..sentences.len() {
        let opener = &sentences[start_idx];
        let opener_lower = opener.text.to_lowercase();

        // Grow the window sentence by sentence; consider every end point that
        // lands in the 20–90s range and keep the best close in each length
        // band, so a start whose best long cut collides with a neighbor can
        // still place a shorter one.
        let mut best_end: [Option<(f32, usize)>; END_BANDS.len()] = [None; END_BANDS.len()];
        for end_idx in start_idx..sentences.len() {
            let dur = sentences[end_idx].end_ms.saturating_sub(opener.start_ms);
            if dur < MIN_MS {
                continue;
            }
            if dur > MAX_MS {
                break;
            }
            let closer = &sentences[end_idx];
            let closer_lower = closer.text.to_lowercase();
            let mut end_score = 0.0f32;
            if closer
                .text
                .split_whitespace()
                .last()
                .is_some_and(crate::transcribe::terminal_word)
            {
                end_score += 1.0;
            }
            if PAYOFF_CUES.iter().any(|c| closer_lower.contains(c)) {
                end_score += 1.4;
            }
            // Reward a pause after the closing sentence (natural resolution).
            if let Some(next) = sentences.get(end_idx + 1) {
                if next.start_ms.saturating_sub(closer.end_ms) >= 700 {
                    end_score += 0.8;
                }
            } else {
                end_score += 0.5;
            }
            // Mild preference for the 30–70s sweet spot.
            let dur_s = dur as f32 / 1000.0;
            end_score += 1.0 - ((dur_s - 45.0).abs() / 45.0).min(1.0) * 0.6;

            let band = END_BANDS
                .iter()
                .position(|&max| dur <= max)
                .unwrap_or(END_BANDS.len() - 1);
            if best_end[band].is_none_or(|(s, _)| end_score > s) {
                best_end[band] = Some((end_score, end_idx));
            }
        }
        for (end_score, end_idx) in best_end.into_iter().flatten() {
            let closer = &sentences[end_idx];
            let window_text: String = sentences[start_idx..=end_idx]
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let window_lower = window_text.to_lowercase();
            let window = &sentences[start_idx..=end_idx];

            // --- Feature detection -> honest rubric scores -------------------
            let first_word = opener_lower.split_whitespace().next().unwrap_or("");
            // "That's" and "It's" lean on context as much as "that" and "it".
            let first_word = first_word.split(['\'', '’']).next().unwrap_or(first_word);
            // Whisper sometimes breaks on a pause or length instead of
            // punctuation; a window opening there starts mid-sentence.
            let mid_sentence_open = opener.word_start > 0
                && !crate::transcribe::ends_sentence(&t.words, opener.word_start - 1);
            let vague_open = mid_sentence_open || is_vague_opener(&opener_lower);
            let pronoun_open = PRONOUN_OPENERS.contains(&first_word) || vague_open;
            let hook = HOOK_STARTS.iter().any(|h| opener_lower.starts_with(h))
                || opener.text.contains('?');
            let contrast = CONTRAST_CUES.iter().any(|c| window_lower.contains(c));
            let payoff_cue = PAYOFF_CUES.iter().any(|c| {
                sentences[end_idx.saturating_sub(1)..=end_idx]
                    .iter()
                    .any(|s| s.text.to_lowercase().contains(c))
            });
            let reference = REFERENCE_CUES.iter().any(|c| window_lower.contains(c));
            let has_number = window_text.chars().any(|c| c.is_ascii_digit());
            let word_count = window_text.split_whitespace().count().max(1);
            let normalized_window = normalized_claim(&window_lower);
            let normalized_words: Vec<&str> = normalized_window.split_whitespace().collect();
            // Distinct focus terms present in the window, plus how much of the
            // window is actually on-topic: a stray topical tail inside a long
            // generic stretch shouldn't count, so the boost requires at least a
            // third of the window's sentences to hit.
            let focus_hits = focus_terms
                .iter()
                .filter(|term| normalized_words.iter().any(|w| term_hits_word(term, w)))
                .count();
            let focus_hit_sentences = window
                .iter()
                .filter(|s| {
                    let normalized = normalized_claim(&s.text);
                    focus_terms.iter().any(|term| {
                        normalized
                            .split_whitespace()
                            .any(|w| term_hits_word(term, w))
                    })
                })
                .count();
            let focus_density = focus_hit_sentences as f32 / window.len() as f32;
            // Decisive but not absolute: the user asked for the topic directly, so
            // a topical window outweighs a generically stronger one, and only an
            // exceptional off-topic window can still outrank it.
            let focus_boost = if focus_hits == 0 || focus_density < 0.3 {
                0.0
            } else {
                (14.0 + 6.0 * focus_density + 2.0 * focus_hits as f32).min(24.0)
            };
            let title_hits = title_terms
                .iter()
                .filter(|term| normalized_words.iter().any(|w| term_hits_word(term, w)))
                .count();
            let title_phrase = title_pairs
                .iter()
                .any(|p| format!(" {normalized_window} ").contains(&format!(" {p} ")));
            let title_boost = if title_hits >= 2 {
                (1.5 * title_hits as f32).min(6.0)
            } else {
                0.0
            } + if title_phrase { 4.0 } else { 0.0 };
            let filler_count = count_fillers(&window_text);
            let filler_rate = filler_count as f32 / word_count as f32;
            let question_open = opener.text.contains('?')
                || ["what", "why", "how", "who", "when"].contains(&first_word);
            let repeated_claim = has_repeated_claim(window);
            let exchange = has_reaction_exchange(window);
            let absolute_claim = contains_absolute_claim(&window_lower);

            // Keep routine housekeeping, sponsor reads, and filler-heavy windows
            // out of the candidate set before ranking can reward their length.
            // The signal gate below remains deliberately narrow so a specific,
            // standalone thought with no magic phrase can still be proposed.
            if has_housekeeping_or_sponsor_cue(&window_lower)
                || is_filler_dominated(filler_count, filler_rate)
            {
                continue;
            }
            // No cue-phrase gate: plain, substantive talk is most of a good
            // podcast. Every window is ranked; the validator's bar decides.
            let substantive_open = !pronoun_open && is_substantive_opener(&opener.text);
            let cohesion = topic_cohesion(&normalized_words);
            let names = named_terms(window);

            let self_contained: u8 = match (pronoun_open, reference) {
                (false, false) => {
                    if hook {
                        5
                    } else {
                        4
                    }
                }
                (true, false) => 3,
                (_, true) => 2,
            };
            let opening_strength: u8 = if vague_open {
                3
            } else if hook && question_open || absolute_claim {
                5
            } else if hook || substantive_open {
                4
            } else {
                3
            };
            let specificity: u8 = if repeated_claim || has_number && contrast {
                5
            } else if absolute_claim || has_number || contrast || names >= 2 {
                4
            } else {
                3
            };
            let tension: u8 = if exchange || contrast && question_open {
                5
            } else if contrast || question_open {
                4
            } else {
                3
            };
            let payoff: u8 = if repeated_claim || payoff_cue && end_score >= 2.5 {
                5
            } else if exchange || payoff_cue || end_score >= 2.2 {
                4
            } else {
                3
            };
            let clarity: u8 = if filler_rate > 0.12 {
                3
            } else if filler_rate > 0.06 {
                4
            } else {
                5
            };
            let context_dependency: u8 = if reference {
                4
            } else if pronoun_open {
                3
            } else {
                1
            };
            let slop_risk: u8 = 1; // continuous faithful excerpt, no effects

            let scores = Scores {
                self_contained,
                opening_strength,
                specificity,
                tension_or_novelty: tension,
                payoff,
                clarity,
                context_dependency,
                slop_risk,
            };

            // Don't propose what the validator will reject: a doomed window
            // would still win the overlap check against a good neighbor.
            if !crate::validate::score_reasons(&scores).is_empty()
                || crate::validate::cold_open_reason(&t.words[opener.word_start..]).is_some()
            {
                continue;
            }

            let dur_s = (closer.end_ms - opener.start_ms) as f32 / 1000.0;
            let composite = self_contained as f32 * 2.0
                + payoff as f32 * 1.6
                + opening_strength as f32 * 1.4
                + clarity as f32 * 1.2
                + tension as f32 * 1.0
                + specificity as f32 * 0.8
                - context_dependency as f32 * 1.5
                + end_score
                + focus_boost
                + title_boost
                + if repeated_claim { 4.0 } else { 0.0 }
                + if exchange { 3.0 } else { 0.0 }
                + if absolute_claim { 1.5 } else { 0.0 }
                - if vague_open { 4.0 } else { 0.0 }
                // One idea developed beats a ramble across topics.
                + 6.0 * cohesion
                + 0.5 * names.min(4) as f32
                - 25.0 * filler_rate
                // Short-form lands best under a minute: a longer cut must
                // earn its extra seconds.
                - 0.1 * (dur_s - 50.0).max(0.0)
                + energy
                    .map(|e| crate::energy::window_boost(e, opener.start_ms, closer.end_ms))
                    .unwrap_or(0.0);

            let headline = make_headline(best_headline_sentence(window));
            let opening_quote = quote_head(&opener.text, 12);
            let closing_quote = quote_tail(&closer.text, 12);
            let mut selection_reason = make_reason(
                hook,
                question_open,
                contrast,
                payoff_cue,
                has_number,
                repeated_claim,
                exchange,
            );
            if focus_boost > 0.0 {
                selection_reason.push_str(" It matches your focus prompt.");
            }

            scored.push((
                composite,
                Candidate {
                    start_ms: opener.start_ms,
                    end_ms: closer.end_ms,
                    headline,
                    opening_quote,
                    closing_quote,
                    selection_reason,
                    scores,
                },
            ));
        }
    }

    // Rank, then keep a diverse, non-overlapping set spread across the source.
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept: Vec<Candidate> = Vec::new();
    for (_, cand) in scored {
        if kept.len() >= proposal_count {
            break;
        }
        let overlaps = kept.iter().any(|k| {
            let inter = overlap_ms(k.start_ms, k.end_ms, cand.start_ms, cand.end_ms) as f64;
            inter / ((cand.end_ms - cand.start_ms).max(1) as f64) > 0.25
        });
        if overlaps {
            continue;
        }
        // Positional diversity: don't let one hot region eat every slot.
        let third = (source_duration_ms / 3).max(1);
        let region = (cand.start_ms / third).min(2);
        let region_count = kept
            .iter()
            .filter(|k| (k.start_ms / third).min(2) == region)
            .count();
        if region_count >= (proposal_count / 2).max(2) {
            continue;
        }
        kept.push(cand);
    }
    kept
}

/// A clean statement to open on: a full sentence of 6–60 words (interview
/// questions run long), light on
/// filler, carrying at least three content words. Scores a 4 on opening
/// strength without needing a stock hook phrase.
fn is_substantive_opener(text: &str) -> bool {
    let normalized = normalized_claim(text);
    let words: Vec<&str> = normalized.split_whitespace().collect();
    let fillers = count_fillers(text);
    let content = words.iter().filter(|w| is_content_word(w)).count();
    (6..=60).contains(&words.len()) && fillers * 10 <= words.len() && content >= 3
}

fn is_content_word(w: &str) -> bool {
    w.chars().count() >= 4 && !FOCUS_STOPWORDS.contains(&w) && !FILLER_WORDS.contains(&w)
}

/// Share of the window's content-word tokens whose stem recurs in it: a
/// window that keeps returning to its subject scores high, a ramble low.
fn topic_cohesion(words: &[&str]) -> f32 {
    let stems: Vec<String> = words
        .iter()
        .filter(|w| is_content_word(w))
        .map(|w| w.chars().take(5).collect())
        .collect();
    if stems.len() < 8 {
        return 0.0;
    }
    let repeated = stems
        .iter()
        .filter(|s| stems.iter().filter(|o| o == s).count() >= 2)
        .count();
    repeated as f32 / stems.len() as f32
}

/// Distinct capitalized words that aren't sentence-initial or "I": names,
/// places, products — concrete detail.
fn named_terms(window: &[Sentence]) -> usize {
    let mut names: Vec<String> = Vec::new();
    for s in window {
        for w in s.text.split_whitespace().skip(1) {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric());
            let capital = w.chars().next().is_some_and(char::is_uppercase);
            if capital && w != "I" && !w.starts_with("I'") && !names.iter().any(|n| n == w) {
                names.push(w.to_string());
            }
        }
    }
    names.len()
}

fn is_vague_opener(text: &str) -> bool {
    let words = text.split_whitespace().count();
    words <= 9
        && (text.contains("that")
            || text.contains("this")
            || text.contains("those")
            || text.contains(" it ")
            || text.starts_with("it "))
}

fn has_housekeeping_or_sponsor_cue(text: &str) -> bool {
    HOUSEKEEPING_OR_SPONSOR_CUES
        .iter()
        .any(|cue| text.contains(cue))
}

fn is_filler_dominated(filler_count: usize, filler_rate: f32) -> bool {
    filler_count >= 5 && filler_rate >= 0.15
}

/// Verbal filler in raw transcript text. Punctuation tells filler from
/// meaning: "like," and "you know," are filler, "I like electricity" and
/// "you know the answer" are not.
fn count_fillers(text: &str) -> usize {
    let mut count = 0;
    let mut prev = String::new();
    for token in text.split_whitespace() {
        let lower = token.to_lowercase();
        let bare: String = lower.chars().filter(|c| c.is_alphabetic()).collect();
        let comma = lower.ends_with(',');
        let filler = matches!(bare.as_str(), "um" | "uh" | "er" | "ah" | "hmm")
            || bare == "like" && comma
            || bare == "know" && prev == "you" && comma
            || bare == "mean" && prev == "i" && comma
            || bare == "of" && (prev == "kind" || prev == "sort");
        if filler {
            count += 1;
        }
        prev = bare;
    }
    count
}

fn normalized_claim(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_repeated_claim(sentences: &[Sentence]) -> bool {
    for (i, a) in sentences.iter().enumerate() {
        let a = normalized_claim(&a.text);
        for b in sentences.iter().skip(i + 1) {
            let b = normalized_claim(&b.text);
            let shorter = if a.len() <= b.len() { &a } else { &b };
            let longer = if a.len() <= b.len() { &b } else { &a };
            if shorter.split_whitespace().count() >= 3 && longer.contains(shorter) {
                return true;
            }
        }
    }
    false
}

fn has_reaction_exchange(sentences: &[Sentence]) -> bool {
    sentences.windows(2).any(|pair| {
        pair[0].text.contains('?')
            && pair[1].text.split_whitespace().count() <= 12
            && pair[1].start_ms.saturating_sub(pair[0].end_ms) <= 1_500
    })
}

fn contains_absolute_claim(text: &str) -> bool {
    let lower = text.to_lowercase();
    let single_word_cue = lower.split_whitespace().any(|word| {
        matches!(
            word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''),
            "everyone" | "everybody" | "nobody" | "never" | "always"
        )
    });
    let normalized = format!(" {} ", normalized_claim(text));
    single_word_cue || normalized.contains(" no one ") || normalized.contains(" all of us ")
}

fn repeats_elsewhere(sentence: &Sentence, sentences: &[Sentence]) -> bool {
    let claim = normalized_claim(&sentence.text);
    claim.split_whitespace().count() >= 3
        && sentences.iter().any(|other| {
            !std::ptr::eq(sentence, other) && normalized_claim(&other.text).contains(&claim)
        })
}

fn best_headline_sentence(sentences: &[Sentence]) -> &Sentence {
    sentences
        .iter()
        .take(5)
        .max_by_key(|s| {
            let lower = s.text.to_lowercase();
            let words = s.text.split_whitespace().count();
            let repeated = repeats_elsewhere(s, sentences) as i32;
            let absolute = contains_absolute_claim(&lower) as i32;
            let question = s.text.contains('?') as i32;
            let concise = (3..=16).contains(&words) as i32;
            repeated * 5 + absolute * 3 + question * 2 + concise
                - is_vague_opener(&lower) as i32 * 3
        })
        .unwrap_or(&sentences[0])
}

fn make_headline(opener: &Sentence) -> String {
    let mut text = opener.text.trim().to_string();
    // Strip weak leading connectives for a cleaner headline.
    for lead in [
        "so ", "and ", "but ", "um ", "uh ", "well ", "yeah ", "okay ", "ok ",
    ] {
        let lower = text.to_lowercase();
        if lower.starts_with(lead) {
            text = text[lead.len()..].trim_start().to_string();
        }
    }
    let mut headline = text.trim_end_matches(['.', ',']).to_string();
    if headline.chars().count() > 90 {
        let prefix: String = headline.chars().take(90).collect();
        let cut = prefix
            .rfind(' ')
            .or_else(|| prefix.char_indices().nth(87).map(|(i, _)| i))
            .unwrap_or(prefix.len());
        headline = format!("{}…", prefix[..cut].trim_end());
    }
    // Sentence case.
    let mut chars = headline.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => headline,
    }
}

fn quote_head(text: &str, words: usize) -> String {
    text.split_whitespace()
        .take(words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_tail(text: &str, words: usize) -> String {
    let all: Vec<&str> = text.split_whitespace().collect();
    let start = all.len().saturating_sub(words);
    all[start..].join(" ")
}

fn make_reason(
    hook: bool,
    question: bool,
    contrast: bool,
    payoff: bool,
    number: bool,
    repeated_claim: bool,
    exchange: bool,
) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if question {
        parts.push("opens on a direct question");
    } else if hook {
        parts.push("opens by naming its subject immediately");
    } else {
        parts.push("opens on a complete thought");
    }
    if contrast {
        parts.push("sets up a tension or misconception");
    }
    if number {
        parts.push("grounds the point in specifics");
    }
    if exchange {
        parts.push("contains a quick question-and-answer turn");
    }
    if repeated_claim {
        parts.push("repeats its central claim for emphasis");
    }
    if payoff {
        parts.push("lands on a stated takeaway before it ends");
    } else {
        parts.push("resolves at a natural sentence boundary");
    }
    let mut reason = parts.join(", ");
    reason = format!(
        "The excerpt {}. Selected by local ranking across the full transcript.",
        reason
    );
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dry run of local ranking + validation on real projects:
    /// `CF_DRY_PROJECTS=dir1:dir2 cargo test --release dry_run -- --ignored --nocapture`
    /// where each dir holds a studio project's transcript.json and project.json.
    #[test]
    #[ignore]
    fn dry_run_on_real_projects() {
        let dirs = std::env::var("CF_DRY_PROJECTS").expect("set CF_DRY_PROJECTS");
        for dir in dirs.split(':') {
            let dir = std::path::Path::new(dir);
            let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
            let mut t: Transcript =
                serde_json::from_str(&read("transcript.json").unwrap()).unwrap();
            t.sentences = crate::transcribe::build_sentences(&t.words);
            let project: serde_json::Value =
                serde_json::from_str(&read("project.json").unwrap()).unwrap();
            let src = &project["source"];
            let dur = src["duration_ms"].as_u64().unwrap();
            let scenes: Vec<u64> =
                serde_json::from_value(src["scene_boundaries_ms"].clone()).unwrap_or_default();
            let energy: Option<crate::energy::EnergyProfile> =
                read("energy.json").and_then(|e| serde_json::from_str(&e).ok());
            let limit = crate::select::local_proposal_limit(dur);
            let cands = propose(
                &t,
                dur,
                limit,
                energy.as_ref(),
                None,
                src["filename"].as_str(),
            );
            let raw = cands.len();
            let report = crate::validate::validate(
                cands,
                &t,
                dur,
                "local".into(),
                &scenes,
                crate::domain::Platform::default(),
            );
            println!(
                "\n=== {} ({:.1} min): {} proposed, {} accepted",
                src["filename"].as_str().unwrap_or("?"),
                dur as f64 / 60_000.0,
                raw,
                report.accepted.len()
            );
            for a in &report.accepted {
                let c = &a.candidate;
                println!(
                    "  ACCEPT {:>5.1}-{:>5.1}s c={:.1} {}",
                    c.start_ms as f64 / 1000.0,
                    c.end_ms as f64 / 1000.0,
                    a.composite,
                    crate::validate::excerpt_text(&t, c.start_ms, c.end_ms)
                        .chars()
                        .take(300)
                        .collect::<String>()
                );
            }
            for r in &report.rejected {
                let c = &r.candidate;
                println!(
                    "  reject {:>5.1}-{:>5.1}s {:?} {}",
                    c.start_ms as f64 / 1000.0,
                    c.end_ms as f64 / 1000.0,
                    r.reasons,
                    c.opening_quote
                );
            }
        }
    }
    use crate::domain::Word;
    use crate::transcribe::build_sentences;

    fn transcript_from(script: &[(&str, u64)]) -> Transcript {
        // (sentence text, gap_ms before it); words spaced ~360ms.
        let mut words: Vec<Word> = Vec::new();
        let mut t = 0u64;
        for (text, gap) in script {
            t += gap;
            for token in text.split_whitespace() {
                words.push(Word {
                    text: token.into(),
                    start_ms: t,
                    end_ms: t + 300,
                    p: 0.92,
                });
                t += 360;
            }
        }
        let sentences = build_sentences(&words);
        Transcript {
            language: "en".into(),
            words,
            sentences,
            avg_confidence: 0.92,
        }
    }

    #[test]
    fn finds_a_hooked_window_in_plausible_speech() {
        // ~40 words per sentence-group ≈ 14s each; three groups ≈ 43s total.
        let long = "Most people completely misunderstand what discipline actually is and I want to explain the real mechanics behind it because once you see it you cannot unsee it at all.";
        let mid = "The mistake is thinking discipline is about motivation when really it is about designing your environment so the default action is the right one every single day without fail.";
        let close = "So the lesson is simple: stop negotiating with yourself every morning and build the system once. That's why the habit finally sticks.";
        let t = transcript_from(&[(long, 0), (mid, 400), (close, 400)]);
        let cands = propose(
            &t,
            t.words.last().unwrap().end_ms + 500,
            3,
            None,
            None,
            None,
        );
        assert!(!cands.is_empty(), "expected at least one candidate");
        let c = &cands[0];
        assert!(c.end_ms - c.start_ms >= MIN_MS);
        assert!(c.end_ms - c.start_ms <= MAX_MS);
        assert!(c.scores.self_contained >= 4);
        assert!(!c.headline.is_empty() && c.headline.len() <= 92);
    }

    #[test]
    fn empty_transcript_yields_nothing() {
        let t = Transcript {
            language: "en".into(),
            words: vec![],
            sentences: vec![],
            avg_confidence: 0.0,
        };
        assert!(propose(&t, 60_000, 3, None, None, None).is_empty());
    }

    #[test]
    fn repeated_claim_and_reaction_surface_as_top_candidate() {
        let t = transcript_from(&[
            ("I believe the solution to making everybody happy is to give them what they want.", 0),
            ("Let's get them all rich.", 200),
            ("Let's get them all fit and healthy, and then let's get them all happy.", 200),
            ("Are those things even possible?", 200),
            ("Can everyone be rich?", 100),
            ("Everyone can be rich.", 100),
            ("Here's my thought exercise for you.", 200),
            ("Everyone can be rich.", 200),
            ("Everything I have created about making money is free because charging would ruin the point.", 200),
            ("Yes, everybody can be rich, and the reason is that knowledge and productive tools can spread.", 200),
        ]);
        let duration = t.words.last().unwrap().end_ms + 500;
        let cands = propose(&t, duration, 3, None, None, None);
        assert!(!cands.is_empty());
        let headline = cands[0].headline.to_lowercase();
        assert!(
            headline.contains("everyone") && headline.contains("rich"),
            "unexpected top candidate: {}",
            cands[0].headline
        );
    }

    #[test]
    fn short_demonstrative_question_is_not_self_contained() {
        assert!(is_vague_opener("how would that work?"));
        assert!(is_vague_opener("are those things possible?"));
        assert!(!is_vague_opener("can everyone be rich?"));
    }

    #[test]
    fn accented_headline_truncation_is_utf8_safe() {
        let text = format!("What {}", "é ".repeat(120));
        let t = Transcript {
            language: "en".into(),
            words: vec![],
            sentences: vec![Sentence {
                text,
                start_ms: 0,
                end_ms: 40_000,
                word_start: 0,
                word_end: 0,
            }],
            avg_confidence: 0.92,
        };

        let cands = propose(&t, 60_000, 1, None, None, None);
        assert_eq!(cands.len(), 1);
        assert!(cands[0].headline.ends_with('…'));
        assert!(cands[0].headline.chars().count() <= 91);
    }

    #[test]
    fn filler_count_reads_punctuation_to_tell_filler_from_meaning() {
        assert_eq!(count_fillers("I like electricity output as a proxy."), 0);
        assert_eq!(count_fillers("It's, like, you know, um, huge."), 3);
        assert_eq!(count_fillers("Do you know the answer?"), 0);
        assert_eq!(count_fillers("I mean, it's kind of big."), 2);
    }

    #[test]
    fn a_window_never_opens_mid_sentence() {
        // The first "sentence" broke on a pause, not punctuation, so the
        // second starts mid-thought and must not open a clip.
        let t = transcript_from(&[
            ("The biggest mistake founders make with pricing is", 0),
            (
                "charging too little for years because nobody tells them otherwise. So the lesson is to raise prices early. That's why it works.",
                1_200,
            ),
        ]);
        let duration = t.words.last().unwrap().end_ms + 500;
        let mid = t.sentences[1].start_ms;
        assert!(propose(&t, duration, 6, None, None, None)
            .iter()
            .filter(|c| c.start_ms == mid)
            .all(|c| c.scores.opening_strength < 4));
    }

    #[test]
    fn a_contracted_pronoun_opener_needs_context() {
        let t = transcript_from(&[
            (
                "That's why the biggest mistake founders make is charging too little for years.",
                0,
            ),
            (
                "Nobody tells them to raise prices, so the lesson is simple: raise them early and often.",
                400,
            ),
            ("That's how the good companies actually grow.", 400),
        ]);
        let duration = t.words.last().unwrap().end_ms + 500;
        assert!(propose(&t, duration, 6, None, None, None)
            .iter()
            .all(|c| c.start_ms != t.sentences[0].start_ms));
    }

    #[test]
    fn numbers_alone_do_not_turn_housekeeping_into_a_candidate() {
        let t = transcript_from(&[
            ("Episode 42 covers our 3 schedule changes.", 0),
            (
                "The first item starts at 9 and the second starts at 10.",
                500,
            ),
            ("We also have 2 reminders for next week's recording.", 500),
            ("That is the full schedule for episode 42.", 500),
        ]);
        let duration = t.words.last().unwrap().end_ms + 500;
        assert!(propose(&t, duration, 3, None, None, None).is_empty());
    }

    #[derive(serde::Deserialize)]
    struct EditorialFixture {
        name: String,
        expected: String,
        sentences: Vec<String>,
    }

    #[test]
    fn synthetic_editorial_fixtures_match_selector_expectations() {
        let fixtures: Vec<EditorialFixture> =
            serde_json::from_str(include_str!("../../evals/fixtures/editorial_cases.json"))
                .unwrap();

        for fixture in fixtures {
            let script: Vec<(&str, u64)> = fixture
                .sentences
                .iter()
                .enumerate()
                .map(|(idx, sentence)| (sentence.as_str(), if idx == 0 { 0 } else { 400 }))
                .collect();
            let t = transcript_from(&script);
            let duration = t.words.last().map(|w| w.end_ms + 500).unwrap_or(60_000);
            let candidates = propose(&t, duration, 3, None, None, None);
            let observed = if candidates.is_empty() {
                "reject"
            } else {
                "accept"
            };
            println!(
                "{}: {} candidate(s) -> {}",
                fixture.name,
                candidates.len(),
                observed
            );
            assert_eq!(
                observed, fixture.expected,
                "fixture {} produced the wrong selector outcome",
                fixture.name
            );
        }
    }

    #[test]
    fn loud_windows_outrank_quiet_ones_all_else_equal() {
        // Two structurally identical story groups (~60s of speech each, so the
        // merged whole-episode window exceeds the 90s cap and never forms).
        // Only loudness differs: the second group is an intense loud stretch.
        let a = "Most people completely misunderstand what discipline actually is and I want to explain the real mechanics behind it because once you see it you cannot unsee it and the whole thing comes down to designing your environment so the default action is the right one every single day without fail and that is the entire secret of lasting change and it applies to money health and relationships equally and the reason most people struggle is that they rely on motivation instead of systems and motivation is a feeling that comes and goes while systems run on their own.";
        let b = "Most people also misunderstand how tiny systems compound and why small daily actions beat big annual plans every single time without exception and the reason is that momentum quietly builds when nobody is watching and then it shows up as results that look like overnight success but never are and the compounding curve always looks flat until it suddenly does not and that is when everyone calls you lucky and the truth is that the winners were just boring enough to keep going.";
        let t = transcript_from(&[(a, 0), (b, 400)]);
        let duration = t.words.last().unwrap().end_ms + 500;

        let quiet = propose(&t, duration, 3, None, None, None);
        assert!(!quiet.is_empty());
        assert!(
            quiet[0].start_ms < 10_000,
            "quiet ranking should favor the first group, got {}",
            quiet[0].start_ms
        );

        // Second group sits in an intense loud stretch; the rest is quiet.
        let mut db = vec![-45.0f32; 300];
        for v in db.iter_mut().skip(40).take(32) {
            *v = -12.0;
        }
        let energy = crate::energy::EnergyProfile { per_second_db: db };
        let boosted = propose(&t, duration, 3, Some(&energy), None, None);
        assert!(!boosted.is_empty());
        assert!(
            boosted[0].start_ms >= 35_000,
            "loud second group should rank first, got {}",
            boosted[0].start_ms
        );
    }

    // A hooky discipline cluster (~43s), then a plainly-worded pricing stretch
    // (~30s) that carries no editorial cues at all — topicality alone must
    // surface it when a focus prompt asks for it.
    fn focused_fixture() -> (Transcript, u64, u64, u64) {
        let d1 = "Most people completely misunderstand what discipline actually is and I want to explain the real mechanics behind it because once you see it you cannot unsee it at all.";
        let d2 = "The mistake is thinking discipline is about motivation when really it is about designing your environment so the default action is the right one every single day without fail.";
        let d3 = "So the lesson is simple: stop negotiating with yourself every morning and build the system once. That's why the habit finally sticks.";
        let p1 = "The pricing page lists three tiers and each tier adds seats for larger teams.";
        let p2 =
            "Monthly billing runs on the first business day and receipts go out automatically.";
        let p3 = "Enterprise contracts include a custom quote and a named account manager.";
        let p4 = "The pricing experiment ran for six weeks across two cohorts last year.";
        let p5 = "Seat counts update on renewal and the invoice total follows the plan.";
        let t = transcript_from(&[
            (d1, 0),
            (d2, 400),
            (d3, 400),
            (p1, 60_000),
            (p2, 400),
            (p3, 400),
            (p4, 400),
            (p5, 400),
        ]);
        let duration = t.words.last().unwrap().end_ms + 500;
        let pricing_start = t
            .sentences
            .iter()
            .find(|s| s.text.contains("pricing"))
            .unwrap()
            .start_ms;
        let pricing_end = t.sentences.last().unwrap().end_ms;
        (t, duration, pricing_start, pricing_end)
    }

    #[test]
    fn focus_prompt_pulls_matching_windows_to_the_top() {
        let (t, duration, pricing_start, pricing_end) = focused_fixture();
        let covers_pricing = |c: &Candidate| {
            overlap_ms(c.start_ms, c.end_ms, pricing_start, pricing_end) * 2
                >= pricing_end - pricing_start
        };

        // Without a focus, the stronger generic window ranks first.
        let generic = propose(&t, duration, 6, None, None, None);
        assert!(!generic.is_empty());
        assert!(
            !covers_pricing(&generic[0]),
            "generic ranking should not lead with the plain pricing stretch"
        );

        // With the focus, matching windows outrank stronger generic ones.
        let focused = propose(&t, duration, 6, None, Some("clips about pricing"), None);
        assert!(!focused.is_empty());
        assert!(
            covers_pricing(&focused[0]),
            "top candidate should cover the pricing stretch, got {}..{}",
            focused[0].start_ms,
            focused[0].end_ms
        );
        assert!(focused[0].selection_reason.contains("focus"));
    }

    #[test]
    fn blank_or_unmatched_focus_is_generic_ranking() {
        let (t, duration, _, _) = focused_fixture();
        let intervals =
            |c: Vec<Candidate>| c.iter().map(|c| (c.start_ms, c.end_ms)).collect::<Vec<_>>();
        let plain = intervals(propose(&t, duration, 6, None, None, None));
        assert_eq!(
            plain,
            intervals(propose(&t, duration, 6, None, Some("   "), None))
        );
        // A focus whose keywords appear nowhere changes nothing.
        assert_eq!(
            plain,
            intervals(propose(
                &t,
                duration,
                6,
                None,
                Some("zebra crossings"),
                None
            ))
        );
    }

    #[test]
    fn focus_terms_drop_boilerplate_and_keep_keywords() {
        assert_eq!(focus_terms(Some("clips about pricing")), ["pricing"]);
        assert_eq!(
            focus_terms(Some("the part where they argue about pricing")),
            ["argue", "pricing"]
        );
        assert!(focus_terms(Some("the clips")).is_empty());
        assert!(focus_terms(Some("   ")).is_empty());
        assert!(focus_terms(None).is_empty());
    }

    #[test]
    fn focus_terms_match_by_stem_not_substring() {
        assert!(term_hits_word("pricing", "price"));
        assert!(term_hits_word("argue", "arguing"));
        assert!(term_hits_word("argue", "argue"));
        assert!(term_hits_word("cat", "cats"));
        assert!(!term_hits_word("them", "thesis"));
        assert!(!term_hits_word("art", "party"));
    }
}
