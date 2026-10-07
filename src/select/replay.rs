//! Offline replay of a saved project's selection, for eval runs.
//!
//! `select-replay <project-dir>` loads the transcript, energy profile, and
//! project record the pipeline already wrote to disk, then re-runs the same
//! `select::propose` → `validate::validate` calls the pipeline makes on its
//! local-ranking path — never a copy of that logic — and prints the
//! `SelectionReport` as JSON on stdout.

use crate::domain::{Project, SelectionReport, Transcript};
use crate::energy::EnergyProfile;
use crate::settings::AiSettings;
use anyhow::{anyhow, Context, Result};
use std::path::Path;

fn read_json<T: serde::de::DeserializeOwned>(dir: &Path, name: &str) -> Result<T> {
    let path = dir.join(name);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

/// Re-run the pipeline's local ranking and validation for one project.
/// Energy is optional input: like `store.load_energy`, a missing or
/// unparsable file degrades to "no signal" rather than an error.
pub async fn replay(project_dir: &Path) -> Result<SelectionReport> {
    let project: Project = read_json(project_dir, "project.json")?;
    let transcript: Transcript = read_json(project_dir, "transcript.json")?;
    let energy: Option<EnergyProfile> = read_json(project_dir, "energy.json").ok();
    let source = project
        .source
        .ok_or_else(|| anyhow!("project has no inspected source yet"))?;

    // The pipeline calls `select::propose` with the stored settings; forcing
    // the offline provider exercises the same local-ranking branch it takes
    // when no provider is configured.
    let settings = AiSettings {
        provider: crate::settings::PROVIDER_OFFLINE.into(),
        ..AiSettings::default()
    };
    let outcome = crate::select::propose(
        &settings,
        &transcript,
        &source,
        energy.as_ref(),
        project.focus_prompt.as_deref(),
        project.platform,
        |_| {},
    )
    .await?;
    Ok(crate::validate::validate(
        outcome.candidates,
        &transcript,
        source.duration_ms,
        outcome.selector,
        &source.scene_boundaries_ms,
        project.platform,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SourceInfo, Word};
    use crate::transcribe::build_sentences;
    use std::path::PathBuf;

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

    fn write_project(dir: &Path, transcript: &Transcript, duration_ms: u64) {
        std::fs::create_dir_all(dir).unwrap();
        let mut project = Project::new("replaytest".into(), PathBuf::from("source.mp4"));
        project.source = Some(SourceInfo {
            filename: "Replay Test - A Good Interview.mp4".into(),
            duration_ms,
            width: 1280,
            height: 720,
            fps: 30.0,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            size_bytes: 1_000_000,
            scene_boundaries_ms: Vec::new(),
        });
        std::fs::write(
            dir.join("project.json"),
            serde_json::to_vec_pretty(&project).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("transcript.json"),
            serde_json::to_vec_pretty(transcript).unwrap(),
        )
        .unwrap();
    }

    /// A saved project replays deterministically through the pipeline's own
    /// local-ranking path: same inputs in, same report bytes out.
    #[tokio::test]
    async fn replay_is_deterministic() {
        let script = [
            ("Welcome back to the show.", 0),
            ("Here is the surprising thing nobody expected.", 900),
            ("We tested the pump for ninety days straight.", 1100),
            ("It failed on day forty one in a way no one predicted.", 800),
            ("The fix took one bolt and twenty minutes of work.", 900),
            ("So the lesson is simple, test it until it breaks.", 1200),
            (
                "Exactly, and that is why we changed the whole checklist.",
                1400,
            ),
            ("Thanks for listening.", 2000),
        ];
        let t = transcript_from(&script);
        let dir = std::env::temp_dir().join(format!("cf-replay-{}", crate::util::short_id()));
        let last = t.words.last().unwrap().end_ms + 1_000;
        write_project(&dir, &t, last);

        let first = replay(&dir).await.unwrap();
        let second = replay(&dir).await.unwrap();
        assert_eq!(first.selector, "local ranking");
        assert_eq!(
            serde_json::to_string_pretty(&first).unwrap(),
            serde_json::to_string_pretty(&second).unwrap()
        );
        for vc in &first.accepted {
            assert!(vc.candidate.start_ms < vc.candidate.end_ms);
            assert!(vc.candidate.end_ms <= last + 500);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A project dir with no inspected source reports the honest error
    /// instead of panicking.
    #[tokio::test]
    async fn replay_rejects_missing_source() {
        let dir = std::env::temp_dir().join(format!("cf-replay-{}", crate::util::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = Project::new("replaytest".into(), PathBuf::from("source.mp4"));
        std::fs::write(
            dir.join("project.json"),
            serde_json::to_vec_pretty(&project).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("transcript.json"),
            serde_json::to_vec_pretty(&transcript_from(&[("hello.", 0)])).unwrap(),
        )
        .unwrap();
        let err = replay(&dir).await.unwrap_err().to_string();
        assert!(err.contains("no inspected source"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
