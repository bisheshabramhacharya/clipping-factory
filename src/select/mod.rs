//! Editorial selection engine (PRD §9): builds transcript windows, asks the
//! configured provider for candidate intervals, and merges the results.
//!
//! Providers:
//! - `openai`    — the PRD's primary provider (user's key)
//! - `anthropic` — optional alternative provider
//! - `local`     — OpenAI-compatible endpoint on this machine (Ollama,
//!   llama.cpp, LM Studio); falls back to `offline` on failure
//! - `offline`   — deterministic heuristic; also the automatic fallback when
//!   no key is configured, clearly labeled in the UI.

pub mod anthropic;
pub mod heuristic;
pub mod local;
pub mod openai;
pub mod replay;

use crate::domain::{fmt_ms, Candidate, Platform, Scores, SourceInfo, Transcript};
use crate::settings::{AiSettings, Provider};
use anyhow::{anyhow, Result};

/// Planning targets from PRD §9.1.
pub fn plan_counts(source_duration_ms: u64) -> (usize, usize) {
    let minutes = source_duration_ms as f64 / 60_000.0;
    let target = ((minutes / 10.0).round() as usize).max(1);
    let candidate_count = ((target as f64 * 1.5).ceil() as usize).max(3);
    (target, candidate_count)
}

/// Local ranking is cheap, so keep every strong, distinct candidate the
/// validator can reasonably use instead of applying the AI candidate quota.
pub fn local_candidate_limit(source_duration_ms: u64) -> usize {
    (source_duration_ms.div_ceil(30_000) as usize).clamp(6, 30)
}

/// The offline selector's full-source ranking: the offline tier's result and
/// the fallback when a window's provider request fails.
fn local_ranking(
    transcript: &Transcript,
    source: &SourceInfo,
    energy: Option<&crate::energy::EnergyProfile>,
    focus: Option<&str>,
) -> Vec<Candidate> {
    heuristic::propose(
        transcript,
        source.duration_ms,
        local_candidate_limit(source.duration_ms),
        energy,
        focus,
        Some(&source.filename),
    )
}

pub struct SelectionOutcome {
    pub candidates: Vec<Candidate>,
    pub selector: String,
    /// Non-fatal caveat surfaced in the UI (e.g. the local endpoint failed
    /// and the heuristic tier ranked instead).
    pub warning: Option<String>,
}

/// `focus` is the project's optional free-text steering prompt ("clips about
/// pricing"). Providers receive it as a rubric directive; the offline tier
/// falls back to keyword matching. Blank or absent focus keeps generic
/// best-candidate ranking. `platform` re-centers the preferred clip length in
/// the window prompt; `Platform::Generic` adds no hint.
pub async fn propose(
    settings: &AiSettings,
    transcript: &Transcript,
    source: &SourceInfo,
    energy: Option<&crate::energy::EnergyProfile>,
    focus: Option<&str>,
    platform: Platform,
    on_progress: impl FnMut(f32),
) -> Result<SelectionOutcome> {
    let (target, candidate_count) = plan_counts(source.duration_ms);
    let focus = focus.map(str::trim).filter(|f| !f.is_empty());

    // An unconnected setup always ranks locally, whatever its stored provider says.
    let provider = if settings.connected() {
        Provider::parse(&settings.provider)
            .ok_or_else(|| anyhow!("Unknown AI provider `{}`.", settings.provider))?
    } else {
        Provider::Offline
    };

    if provider == Provider::Offline {
        // `heuristic::propose` ranks the whole source in one deterministic
        // pass, so its shortlist is already finally ranked (PRD §9.1) — only
        // the windowed tiers below need a separate ranking pass.
        return Ok(SelectionOutcome {
            candidates: local_ranking(transcript, source, energy, focus),
            selector: "local ranking".into(),
            warning: None,
        });
    }

    let key = match provider {
        Provider::Local => String::new(),
        _ => settings
            .api_key
            .clone()
            .ok_or_else(|| anyhow!("AI key missing. Open AI connection and add your key."))?,
    };
    let base_url = settings.effective_base_url();
    let model = settings.effective_model();
    let windows = build_windows(transcript, source.duration_ms);
    let per_window = ((candidate_count as f64) / (windows.len() as f64)).ceil() as usize;
    // Every tier asks the same question per window; only the endpoint differs.
    let mut completion = |user: String| {
        let (key, base_url, model) = (key.clone(), base_url.clone(), model.clone());
        async move {
            match provider {
                Provider::Anthropic => {
                    anthropic::complete(&key, &model, SYSTEM_PROMPT, &user).await
                }
                Provider::Local => local::complete(&base_url, &model, SYSTEM_PROMPT, &user).await,
                _ => openai::complete(&key, &model, SYSTEM_PROMPT, &user).await,
            }
        }
    };
    let results = complete_windows(
        &windows,
        |win| window_prompt(win, source, target, per_window.max(2), focus, platform),
        on_progress,
        &mut completion,
        provider == Provider::Local,
    )
    .await;
    let failures = results.iter().filter(|r| r.is_err()).count();

    // A down or misbehaving local endpoint must never wedge the pipeline:
    // rank the whole source locally and say so in the UI.
    if provider == Provider::Local && failures > 0 {
        return Ok(SelectionOutcome {
            candidates: local_ranking(transcript, source, energy, focus),
            selector: "local ranking (local endpoint failed)".into(),
            warning: Some(format!(
                "The local endpoint failed ({}) — ranked locally instead.",
                first_error(&results)
            )),
        });
    }
    // Every window failing is the one case the stage cannot survive: there is
    // no partial result to degrade onto.
    if failures == windows.len() {
        return Err(anyhow!(first_error(&results)));
    }

    let local_fallback = (failures > 0).then(|| local_ranking(transcript, source, energy, focus));
    let (mut all, warning) = merge_windows(
        &windows,
        results,
        local_fallback.as_deref().unwrap_or_default(),
    );
    if windows.len() > 1 {
        // Overlapping windows can propose the same Candidate twice; keep the
        // copy the validator would rank higher.
        all = dedupe_similar(all);
        // PRD §9.1's final ranking pass: per-window order says nothing about
        // cross-window quality, so one more completion orders the shortlist.
        if all.len() > 1 {
            match completion(ranking_prompt(&all))
                .await
                .and_then(|raw| apply_ranking(&all, &raw))
            {
                Ok(ranked) => all = ranked,
                // An unusable reply keeps the merge order; candidates are
                // never dropped or invented.
                Err(e) => tracing::warn!("final ranking pass failed: {e:#}"),
            }
        }
    }
    Ok(SelectionOutcome {
        candidates: all,
        selector: format!("{} · {}", provider.as_str(), model),
        warning,
    })
}

/// One window's provider outcome: its candidates, or the error that failed it.
type WindowResult = Result<Vec<Candidate>, anyhow::Error>;

/// One provider request per window — the loop count is the work. Outcomes are
/// kept per window so a single failure can degrade instead of failing the run.
/// `stop_on_error` is for a local endpoint that is down: asking the remaining
/// windows only multiplies the wait.
async fn complete_windows<F, Fut>(
    windows: &[Window],
    prompt_for: impl Fn(&Window) -> String,
    mut on_progress: impl FnMut(f32),
    mut complete: F,
    stop_on_error: bool,
) -> Vec<WindowResult>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let mut results = Vec::with_capacity(windows.len());
    for (i, win) in windows.iter().enumerate() {
        on_progress(i as f32 / windows.len() as f32);
        let outcome = complete(prompt_for(win))
            .await
            .and_then(|raw| parse_candidates(&raw));
        let failed = outcome.is_err();
        results.push(outcome);
        if failed && stop_on_error {
            break;
        }
    }
    results
}

/// Fold one provider outcome per window into a single shortlist. Successful
/// windows keep their candidates; a failed window is covered by the local
/// ranking's candidates inside that window's span, so one provider error
/// degrades the run instead of failing the stage.
fn merge_windows(
    windows: &[Window],
    results: Vec<WindowResult>,
    local_ranking: &[Candidate],
) -> (Vec<Candidate>, Option<String>) {
    let mut merged: Vec<Candidate> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (win, result) in windows.iter().zip(results) {
        match result {
            Ok(cands) => merged.extend(cands),
            Err(e) => {
                merged.extend(
                    local_ranking
                        .iter()
                        .filter(|c| c.start_ms < win.end_ms && c.end_ms > win.start_ms)
                        .cloned(),
                );
                failures.push(format!("{e:#}"));
            }
        }
    }
    let warning = failures.first().map(|first| {
        format!(
            "{} of {} transcript windows failed ({}); those spans were ranked locally instead.",
            failures.len(),
            windows.len(),
            first
        )
    });
    (merged, warning)
}

/// The first window failure, as the message the stage surfaces.
fn first_error(results: &[WindowResult]) -> String {
    results
        .iter()
        .find_map(|r| r.as_ref().err())
        .map(|e| format!("{e:#}"))
        .unwrap_or_else(|| "the provider failed every window".into())
}

/// Test connectivity for the configured provider (PRD §14.1 `/api/settings/ai/test`).
pub async fn test_connection(settings: &AiSettings) -> Result<String> {
    let provider = Provider::parse(&settings.provider)
        .ok_or_else(|| anyhow!("Unknown provider `{}`.", settings.provider))?;
    match provider {
        Provider::Offline => Ok("Local ranking is ready — no API key needed.".into()),
        Provider::OpenAi => {
            let key = settings
                .api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())
                .ok_or_else(|| anyhow!("Enter an OpenAI API key first."))?;
            openai::test(key).await?;
            Ok(format!(
                "OpenAI connection verified. Using model {}.",
                settings.effective_model()
            ))
        }
        Provider::Anthropic => {
            let key = settings
                .api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())
                .ok_or_else(|| anyhow!("Enter an Anthropic API key first."))?;
            anthropic::test(key).await?;
            Ok(format!(
                "Anthropic connection verified. Using model {}.",
                settings.effective_model()
            ))
        }
        Provider::Local => {
            let model = settings.effective_model();
            if model.is_empty() {
                return Err(anyhow!(
                    "Enter the model name your local endpoint serves (e.g. qwen2.5:7b)."
                ));
            }
            let base_url = settings.effective_base_url();
            local::test(&base_url, &model).await?;
            Ok(format!(
                "Local endpoint verified at {}. Using model {}.",
                base_url, model
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Windowing (PRD §9.1: overlapping windows for long transcripts)
// ---------------------------------------------------------------------------

pub struct Window {
    pub start_ms: u64,
    pub end_ms: u64,
    pub lines: String,
}

const WINDOW_MS: u64 = 12 * 60_000;
const OVERLAP_MS: u64 = 2 * 60_000;

pub fn build_windows(t: &Transcript, source_duration_ms: u64) -> Vec<Window> {
    let mut windows = Vec::new();
    let mut win_start: u64 = 0;
    loop {
        let win_end = (win_start + WINDOW_MS).min(source_duration_ms);
        let mut lines = String::new();
        for s in &t.sentences {
            if s.end_ms < win_start || s.start_ms > win_end {
                continue;
            }
            lines.push_str(&format!(
                "[{} --> {}] {}\n",
                ts_precise(s.start_ms),
                ts_precise(s.end_ms),
                s.text
            ));
        }
        if !lines.is_empty() {
            windows.push(Window {
                start_ms: win_start,
                end_ms: win_end,
                lines,
            });
        }
        if win_end >= source_duration_ms {
            break;
        }
        win_start = win_end - OVERLAP_MS;
    }
    if windows.is_empty() {
        windows.push(Window {
            start_ms: 0,
            end_ms: source_duration_ms,
            lines: String::new(),
        });
    }
    windows
}

fn ts_precise(ms: u64) -> String {
    format!("{}.{:03}", fmt_ms(ms), ms % 1000)
}

// ---------------------------------------------------------------------------
// Prompting & parsing
// ---------------------------------------------------------------------------

const SYSTEM_PROMPT: &str = r#"You are the editorial selector inside Clipping Factory, a tool that turns one long podcast into a few faithful vertical clips. You choose which continuous candidates of the source recording deserve to stand alone. You are a demanding editor: quality over quota.

HARD RULES
- Each candidate is ONE continuous interval of the source. You choose only start_ms and end_ms.
- Never rewrite, reorder, splice, or invent speech.
- opening_quote and closing_quote must be VERBATIM text from the transcript near the start and end of your interval.
- Clips normally run 20–90 seconds. Start on a natural sentence boundary; end after the idea resolves.
- The headline summarizes the excerpt in sentence case, under 90 characters, supported directly by what the speaker says. Never invent numbers, certainty, or conflict.

A PASSING CLIP MUST
- Make sense without the preceding conversation.
- Establish its subject within the first sentence or few seconds.
- Contain a specific insight, story, disagreement, reveal, joke, or useful explanation.
- Build toward a payoff or clear conclusion, and end cleanly.
- Preserve the speaker's actual meaning.
- Avoid unresolved references like "like I said earlier".
- Avoid sponsor reads, housekeeping, introductions, and generic agreement.

WHAT MAKES A CANDIDATE WORTHY (in priority order)
1. Conflict, tension, or a strong disagreement — people stop scrolling for friction.
2. A surprising claim, counter-intuitive insight, or a reveal that reframes something.
3. A complete micro-story: setup → buildup → payoff.
4. A personal admission, vulnerability, or strong opinion stated plainly.
5. A quotable line that works as a standalone hook.
6. A loud, emotional, high-energy exchange (raised voices, laughter, excitement).

When in doubt between two candidates, prefer the one with more emotion or conflict over neutral-but-correct explanation. Never invent emotion that is not in the transcript — the energy must be audible in the words themselves.

SCORING (1–5 integers)
self_contained, opening_strength, specificity, tension_or_novelty, payoff, clarity: 5 is best.
context_dependency, slop_risk: these are penalties — 1 is safest, 5 is worst.
Score honestly; weak candidates should score low so the validator can reject them. opening_strength must reflect whether the FIRST few seconds would stop a viewer from scrolling.

OUTPUT
Return ONLY a JSON object, no markdown fences, shaped exactly like:
{"candidates":[{"start_ms":1122000,"end_ms":1188000,"headline":"...","opening_quote":"...","closing_quote":"...","selection_reason":"...","scores":{"self_contained":5,"opening_strength":4,"specificity":4,"tension_or_novelty":4,"payoff":5,"clarity":5,"context_dependency":1,"slop_risk":1}}]}
Propose fewer candidates than asked rather than padding with weak ones. If nothing qualifies, return {"candidates":[]}."#;

fn window_prompt(
    win: &Window,
    source: &SourceInfo,
    target: usize,
    per_window: usize,
    focus: Option<&str>,
    platform: Platform,
) -> String {
    let mut directive = focus
        .map(|f| f.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|f| !f.is_empty())
        .map(|f| {
            format!(
                "\n\nEDITORIAL FOCUS\nThe editor who set up this project asked for clips about: \"{f}\". Topical relevance to that request is now the top selection priority — a candidate that clearly addresses it outranks a generically stronger one. Only propose off-topic candidates when they are exceptional. Every hard rule still applies; never pad the quota with off-topic filler."
            )
        })
        .unwrap_or_default();
    if platform != Platform::Generic {
        let (min_ms, max_ms) = platform.sweet_spot_ms();
        directive.push_str(&format!(
            "\n\nPLATFORM TARGET\nThese clips are destined for {} — prefer candidates of {}–{} s when they are otherwise comparable. The hard duration rules are unchanged; never stretch or pad a candidate to fit the window.",
            platform.label(),
            min_ms / 1000,
            max_ms / 1000
        ));
    }
    format!(
        "Source: \"{}\" — total duration {} ({} ms). Planning target for the whole source: about {} clip(s); this is guidance, not a quota.{}\n\nTranscript window ({} → {}), one sentence per line as [start --> end] text:\n\n{}\n\nPropose up to {} strong candidates from THIS window only. Timestamps are absolute source milliseconds. Remember: return only the JSON object.",
        source.filename,
        fmt_ms(source.duration_ms),
        source.duration_ms,
        target,
        directive,
        fmt_ms(win.start_ms),
        fmt_ms(win.end_ms),
        win.lines,
        per_window
    )
}

#[derive(serde::Deserialize)]
struct CandidatesWrapper {
    candidates: Vec<CandidateIn>,
}

#[derive(serde::Deserialize)]
struct CandidateIn {
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    #[serde(default)]
    headline: String,
    #[serde(default)]
    opening_quote: String,
    #[serde(default)]
    closing_quote: String,
    #[serde(default)]
    selection_reason: String,
    scores: Option<ScoresIn>,
}

#[derive(serde::Deserialize, Default)]
struct ScoresIn {
    #[serde(default)]
    self_contained: f64,
    #[serde(default)]
    opening_strength: f64,
    #[serde(default)]
    specificity: f64,
    #[serde(default)]
    tension_or_novelty: f64,
    #[serde(default)]
    payoff: f64,
    #[serde(default)]
    clarity: f64,
    #[serde(default)]
    context_dependency: f64,
    #[serde(default)]
    slop_risk: f64,
}

fn clamp_score(v: f64) -> u8 {
    (v.round() as i64).clamp(1, 5) as u8
}

/// Slice the JSON object out of a provider reply, tolerating markdown fences
/// and surrounding prose.
fn json_payload(raw: &str) -> Option<&str> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match (cleaned.find('{'), cleaned.rfind('}')) {
        (Some(s), Some(e)) if e > s => Some(&cleaned[s..=e]),
        _ => None,
    }
}

/// Parse a provider response into candidates. Malformed JSON is a named,
/// retryable error (PRD §15).
pub fn parse_candidates(raw: &str) -> Result<Vec<Candidate>> {
    let json = json_payload(raw)
        .ok_or_else(|| anyhow!("The AI returned malformed JSON. Retry the stage."))?;
    let wrapper: CandidatesWrapper = serde_json::from_str(json)
        .map_err(|e| anyhow!("The AI returned malformed JSON ({}). Retry the stage.", e))?;

    let mut out = Vec::new();
    for c in wrapper.candidates {
        let (Some(start_ms), Some(end_ms)) = (c.start_ms, c.end_ms) else {
            continue;
        };
        if start_ms < 0 || end_ms <= start_ms {
            continue;
        }
        let s = c.scores.unwrap_or_default();
        out.push(Candidate {
            start_ms: start_ms as u64,
            end_ms: end_ms as u64,
            headline: c.headline.trim().to_string(),
            opening_quote: c.opening_quote.trim().to_string(),
            closing_quote: c.closing_quote.trim().to_string(),
            selection_reason: c.selection_reason.trim().to_string(),
            scores: Scores {
                self_contained: clamp_score(s.self_contained),
                opening_strength: clamp_score(s.opening_strength),
                specificity: clamp_score(s.specificity),
                tension_or_novelty: clamp_score(s.tension_or_novelty),
                payoff: clamp_score(s.payoff),
                clarity: clamp_score(s.clarity),
                context_dependency: clamp_score(s.context_dependency),
                slop_risk: clamp_score(s.slop_risk),
            },
        });
    }
    Ok(out)
}

/// The final ranking pass (PRD §9.1): the merged shortlist is sent back once
/// more and the reply must order every candidate by index.
fn ranking_prompt(shortlist: &[Candidate]) -> String {
    let mut lines = String::from(
        "You are the editorial selector inside Clipping Factory. You receive the merged shortlist of candidate clips from one long podcast and order them best first. Return ONLY a JSON object shaped exactly like {\"order\":[2,0,1]}: every candidate index appears exactly once, best first, and no index is added or dropped.\n\nShortlist:\n",
    );
    for (i, c) in shortlist.iter().enumerate() {
        lines.push_str(&format!(
            "[{i}] {:.1}–{:.1}s — {}\n",
            c.start_ms as f64 / 1000.0,
            c.end_ms as f64 / 1000.0,
            c.headline
        ));
    }
    lines
        .push_str("\nOrder every candidate index from best to worst. Return only the JSON object.");
    lines
}

#[derive(serde::Deserialize)]
struct RankingWrapper {
    order: Vec<usize>,
}

/// Apply a `{"order":[…]}` reply to the shortlist. The reply must name every
/// candidate exactly once; anything else is an error so the caller keeps the
/// merge order instead of dropping or inventing candidates.
fn apply_ranking(shortlist: &[Candidate], raw: &str) -> Result<Vec<Candidate>> {
    let json = json_payload(raw).ok_or_else(|| anyhow!("The AI returned a malformed ranking."))?;
    let wrapper: RankingWrapper = serde_json::from_str(json)
        .map_err(|e| anyhow!("The AI returned a malformed ranking ({}).", e))?;
    if wrapper.order.len() != shortlist.len() {
        return Err(anyhow!(
            "The AI ranking listed {} of {} candidates.",
            wrapper.order.len(),
            shortlist.len()
        ));
    }
    let mut seen = vec![false; shortlist.len()];
    let mut ranked = Vec::with_capacity(shortlist.len());
    for &idx in &wrapper.order {
        if idx >= shortlist.len() || seen[idx] {
            return Err(anyhow!("The AI ranking repeated or misplaced a candidate."));
        }
        seen[idx] = true;
        ranked.push(shortlist[idx].clone());
    }
    Ok(ranked)
}

/// Merge near-duplicate candidates from overlapping windows: keep the higher
/// scoring of any pair whose intervals overlap more than 55%. The LLM tiers
/// run their final ranking completion on the merged shortlist (PRD §9.1);
/// the offline tier's `heuristic::propose` has already ranked the whole
/// source deterministically, so this merge order is its final order.
fn dedupe_similar(mut cands: Vec<Candidate>) -> Vec<Candidate> {
    // The documented Composite score is the ranking measure; the platform
    // duration nudge is added later, after the merge, so it plays no part here.
    let composite = |c: &Candidate| crate::validate::composite_score(&c.scores);
    cands.sort_by(|a, b| {
        composite(b)
            .partial_cmp(&composite(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<Candidate> = Vec::new();
    for c in cands {
        let dup = kept.iter().any(|k| {
            let inter = overlap_ms(k.start_ms, k.end_ms, c.start_ms, c.end_ms) as f64;
            let dur = (c.end_ms - c.start_ms).max(1) as f64;
            inter / dur > 0.55
        });
        if !dup {
            kept.push(c);
        }
    }
    kept
}

pub fn overlap_ms(a0: u64, a1: u64, b0: u64, b1: u64) -> u64 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    hi.saturating_sub(lo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_counts_match_prd() {
        // 60-minute source → target 6, candidate count 9.
        assert_eq!(plan_counts(60 * 60_000), (6, 9));
        // 3-minute source → target 1, candidate count 3 (floor).
        assert_eq!(plan_counts(3 * 60_000), (1, 3));
        // 25 minutes → round(2.5) = 3 (round-half-up), candidate count 5.
        let (t, p) = plan_counts(25 * 60_000);
        assert_eq!(t, 3);
        assert_eq!(p, 5);
    }

    #[test]
    fn local_ranking_keeps_many_more_candidates() {
        assert_eq!(local_candidate_limit(3 * 60_000), 6);
        assert_eq!(local_candidate_limit(366_805), 13);
        assert_eq!(local_candidate_limit(60 * 60_000), 30);
    }

    #[test]
    fn parses_fenced_json() {
        let raw = "```json\n{\"candidates\":[{\"start_ms\":1000,\"end_ms\":31000,\"headline\":\"H\",\"opening_quote\":\"a\",\"closing_quote\":\"b\",\"selection_reason\":\"r\",\"scores\":{\"self_contained\":5,\"opening_strength\":4,\"specificity\":4,\"tension_or_novelty\":4,\"payoff\":5,\"clarity\":5,\"context_dependency\":1,\"slop_risk\":1}}]}\n```";
        let c = parse_candidates(raw).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].scores.self_contained, 5);
    }

    #[test]
    fn malformed_json_is_error() {
        assert!(parse_candidates("no json here").is_err());
    }

    // ------------------------------------------------------------------
    // local endpoint (OpenAI-compatible) — end-to-end against a stub server
    // ------------------------------------------------------------------

    fn tiny_fixture() -> (Transcript, SourceInfo) {
        let text = "The real trick with discipline is designing the environment once so the default action is the right one every single day.";
        let mut words = Vec::new();
        let mut t = 0u64;
        for token in text.split_whitespace() {
            words.push(crate::domain::Word {
                text: token.into(),
                start_ms: t,
                end_ms: t + 300,
                p: 0.92,
            });
            t += 360;
        }
        let duration_ms = words.last().unwrap().end_ms + 500;
        let sentences = crate::transcribe::build_sentences(&words);
        (
            Transcript {
                language: "en".into(),
                words,
                sentences,
                avg_confidence: 0.92,
            },
            SourceInfo {
                filename: "episode.mp4".into(),
                duration_ms,
                width: 1920,
                height: 1080,
                fps: 30.0,
                video_codec: "h264".into(),
                audio_codec: "aac".into(),
                scene_boundaries_ms: Vec::new(),
                size_bytes: 1,
            },
        )
    }

    #[tokio::test]
    async fn local_endpoint_produces_candidates() {
        let content = "{\"candidates\":[{\"start_ms\":1000,\"end_ms\":31000,\"headline\":\"H\",\"opening_quote\":\"a\",\"closing_quote\":\"b\",\"selection_reason\":\"r\",\"scores\":{\"self_contained\":5,\"opening_strength\":4,\"specificity\":4,\"tension_or_novelty\":4,\"payoff\":5,\"clarity\":5,\"context_dependency\":1,\"slop_risk\":1}}]}";
        let base_url = local::test_server::spawn(vec!["qwen2.5:7b".into()], content.into()).await;
        let settings = AiSettings {
            provider: crate::settings::PROVIDER_LOCAL.into(),
            model: "qwen2.5:7b".into(),
            base_url,
            api_key: None,
        };
        let (t, src) = tiny_fixture();
        let outcome = propose(&settings, &t, &src, None, None, Platform::Generic, |_| {})
            .await
            .unwrap();
        assert_eq!(outcome.selector, "local · qwen2.5:7b");
        assert!(outcome.warning.is_none());
        assert_eq!(outcome.candidates.len(), 1);
        assert_eq!(outcome.candidates[0].start_ms, 1000);
        assert_eq!(outcome.candidates[0].scores.payoff, 5);
    }

    #[tokio::test]
    async fn local_endpoint_failure_falls_back_to_local_ranking() {
        // Bind then drop: a port guaranteed to refuse connections.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let settings = AiSettings {
            provider: crate::settings::PROVIDER_LOCAL.into(),
            model: "qwen2.5:7b".into(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: None,
        };
        let (t, src) = tiny_fixture();
        let outcome = propose(&settings, &t, &src, None, None, Platform::Generic, |_| {})
            .await
            .unwrap();
        assert_eq!(outcome.selector, "local ranking (local endpoint failed)");
        assert!(outcome.warning.is_some());
    }

    #[test]
    fn window_prompt_carries_the_focus_directive() {
        let (t, src) = tiny_fixture();
        let windows = build_windows(&t, src.duration_ms);
        let focused = window_prompt(
            &windows[0],
            &src,
            1,
            2,
            Some("clips about pricing"),
            Platform::Generic,
        );
        assert!(focused.contains("EDITORIAL FOCUS"));
        assert!(focused.contains("clips about pricing"));
        let plain = window_prompt(&windows[0], &src, 1, 2, None, Platform::Generic);
        assert!(!plain.contains("EDITORIAL FOCUS"));
        let blank = window_prompt(&windows[0], &src, 1, 2, Some("   "), Platform::Generic);
        assert_eq!(blank, plain);
    }

    #[test]
    fn window_prompt_carries_the_platform_target() {
        let (t, src) = tiny_fixture();
        let windows = build_windows(&t, src.duration_ms);
        let tiktok = window_prompt(&windows[0], &src, 1, 2, None, Platform::TikTok);
        assert!(tiktok.contains("PLATFORM TARGET"));
        assert!(tiktok.contains("TikTok"));
        assert!(tiktok.contains("25–35 s"));
        // Generic keeps the prompt identical to no platform line at all.
        let generic = window_prompt(&windows[0], &src, 1, 2, None, Platform::Generic);
        assert!(!generic.contains("PLATFORM TARGET"));
        let reels = window_prompt(&windows[0], &src, 1, 2, None, Platform::Reels);
        assert!(reels.contains("35–45 s"));
    }

    #[tokio::test]
    async fn local_endpoint_receives_the_focus_directive() {
        let (base_url, requests) = local::test_server::spawn_with_requests(
            vec!["qwen2.5:7b".into()],
            "{\"candidates\":[]}".into(),
        )
        .await;
        let settings = AiSettings {
            provider: crate::settings::PROVIDER_LOCAL.into(),
            model: "qwen2.5:7b".into(),
            base_url,
            api_key: None,
        };
        let (t, src) = tiny_fixture();
        let outcome = propose(
            &settings,
            &t,
            &src,
            None,
            Some("clips about pricing"),
            Platform::Generic,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(outcome.selector, "local · qwen2.5:7b");
        let sent = requests.lock().await;
        assert!(
            sent.iter().any(|r| r.contains("clips about pricing")),
            "provider request must carry the focus prompt"
        );
    }

    // ------------------------------------------------------------------
    // window failures and the final ranking pass — fake providers, no network
    // ------------------------------------------------------------------

    fn a_window(start_ms: u64, end_ms: u64) -> Window {
        Window {
            start_ms,
            end_ms,
            lines: String::new(),
        }
    }

    fn a_candidate(start_ms: u64, end_ms: u64) -> Candidate {
        Candidate {
            start_ms,
            end_ms,
            headline: "H".into(),
            opening_quote: "o".into(),
            closing_quote: "c".into(),
            selection_reason: "r".into(),
            scores: Scores::default(),
        }
    }

    #[test]
    fn apply_ranking_reorders_without_dropping_or_inventing() {
        let shortlist = vec![
            a_candidate(0, 20_000),
            a_candidate(30_000, 50_000),
            a_candidate(60_000, 80_000),
        ];
        let ranked = apply_ranking(&shortlist, "```json\n{\"order\":[2,0,1]}\n```").unwrap();
        let spans: Vec<(u64, u64)> = ranked.iter().map(|c| (c.start_ms, c.end_ms)).collect();
        assert_eq!(spans, vec![(60_000, 80_000), (0, 20_000), (30_000, 50_000)]);
    }

    #[test]
    fn a_ranking_that_is_not_a_permutation_is_an_error() {
        let shortlist = vec![a_candidate(0, 20_000), a_candidate(30_000, 50_000)];
        for raw in [
            "no json here",
            "{\"order\":[0]}",
            "{\"order\":[0,0]}",
            "{\"order\":[0,2]}",
        ] {
            assert!(apply_ranking(&shortlist, raw).is_err(), "accepted {raw}");
        }
    }

    #[tokio::test]
    async fn a_single_window_failure_is_reported_not_fatal() {
        let windows = vec![a_window(0, 10_000), a_window(10_000, 20_000)];
        let results = complete_windows(
            &windows,
            |w| w.start_ms.to_string(),
            |_| {},
            |prompt: String| async move {
                if prompt == "10000" {
                    Err(anyhow!("rate limit reached (429)"))
                } else {
                    Ok("{\"candidates\":[]}".to_string())
                }
            },
            false,
        )
        .await;
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
    }

    #[tokio::test]
    async fn a_local_endpoint_error_stops_the_window_loop() {
        let windows = vec![a_window(0, 10_000), a_window(10_000, 20_000)];
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = calls.clone();
        let results = complete_windows(
            &windows,
            |_| String::new(),
            |_| {},
            |_prompt: String| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err::<String, _>(anyhow!("connection refused"))
                }
            },
            true,
        )
        .await;
        assert_eq!(results.len(), 1);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_window_degrades_to_local_ranking_for_its_span() {
        let windows = vec![a_window(0, 30_000), a_window(30_000, 90_000)];
        let results = vec![
            Ok(vec![a_candidate(1_000, 20_000)]),
            Err(anyhow!("rate limit reached (429)")),
        ];
        let local = vec![a_candidate(40_000, 55_000), a_candidate(100_000, 130_000)];
        let (merged, warning) = merge_windows(&windows, results, &local);
        let spans: Vec<(u64, u64)> = merged.iter().map(|c| (c.start_ms, c.end_ms)).collect();
        // The successful window's candidate survives; the failed window's span
        // is covered by local ranking, and nothing outside it is invented.
        assert_eq!(spans, vec![(1_000, 20_000), (40_000, 55_000)]);
        assert!(warning.unwrap().contains("ranked locally"));
    }

    #[test]
    fn a_clean_run_keeps_every_candidate_and_warns_about_nothing() {
        let windows = vec![a_window(0, 30_000), a_window(30_000, 60_000)];
        let results = vec![
            Ok(vec![a_candidate(1_000, 20_000)]),
            Ok(vec![a_candidate(40_000, 55_000)]),
        ];
        let (merged, warning) = merge_windows(&windows, results, &[]);
        assert_eq!(merged.len(), 2);
        assert!(warning.is_none());
    }
}
