//! First-run sample episode, synthesized on demand so no media ships in the
//! repo. Narration uses macOS `say` when available so the transcriber gets
//! real speech; otherwise a sine tone stands in. The video track is ffmpeg's
//! generated testsrc2 pattern. Everything is procedurally produced at runtime:
//! no third-party or copyrighted media is involved.

use std::path::{Path, PathBuf};
use tokio::process::Command;

use anyhow::{anyhow, Context};

const SCRIPT: &str = "\
Here is what makes a moment worth clipping. A strong clip opens clean, makes one point, and lands its payoff before your thumb can scroll away.\n\
Most podcast video fails for a boring reason. The cut starts mid-thought, or it runs long past the punchline, and the viewer is gone.\n\
Clipping Factory reads the full transcript first. Every sentence gets timed word by word, so nothing is guessed and nothing is invented later.\n\
The selector looks for tension, surprise, and small complete stories. Conflict outranks a hot take. A confession outranks a joke.\n\
A deterministic validator has the final word. If a candidate depends on missing context, or overlaps a stronger moment, it gets rejected.\n\
That gate is why the shortlist stays short. Ten honest candidates beat fifty forgettable ones.\n\
Framing comes next. When a face is detected, the crop follows it through the frame. When none is found, the clip falls back to a soft blurred pad instead of guessing.\n\
Captions are burned in word accurately, in the style and accent color you picked, so the clip posts without another editing pass.\n\
Everything runs on this machine. The video never leaves it, the transcript never leaves it, and no account exists anywhere.\n\
Review is one keypress per clip. Keep the strong ones, skip the rest, and download everything you kept as a single archive.\n\
That is the entire workflow. One episode in, a stack of postable clips out. Drop your own podcast whenever you are ready.";

/// Where the synthesized sample lives; cached across runs and projects.
pub fn sample_path(data_dir: &Path) -> PathBuf {
    data_dir.join("sample").join("sample-episode.mp4")
}

/// Return the cached sample, synthesizing it first if absent.
pub async fn ensure_sample(data_dir: &Path, ffmpeg: &str) -> anyhow::Result<PathBuf> {
    let dest = sample_path(data_dir);
    if tokio::fs::metadata(&dest)
        .await
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
        return Ok(dest);
    }
    tokio::fs::create_dir_all(dest.parent().expect("sample path has a parent"))
        .await
        .context("creating sample dir")?;

    let work = std::env::temp_dir().join(format!("cf-sample-{}", crate::util::short_id()));
    tokio::fs::create_dir_all(&work)
        .await
        .context("creating sample work dir")?;
    let result = match narrate(&work).await {
        Some(aiff) => {
            render(
                ffmpeg,
                &["-i", aiff.to_string_lossy().as_ref()],
                &dest,
                true,
            )
            .await
        }
        None => {
            tracing::warn!("`say` unavailable; synthesizing sample with a sine tone");
            render(
                ffmpeg,
                &["-f", "lavfi", "-i", "sine=frequency=200:duration=24"],
                &dest,
                false,
            )
            .await
        }
    };
    tokio::fs::remove_dir_all(&work).await.ok();
    result?;
    Ok(dest)
}

/// Produce narration via macOS speech synthesis; None when unavailable.
async fn narrate(dir: &Path) -> Option<PathBuf> {
    let say = PathBuf::from("/usr/bin/say");
    if !say.exists() {
        return None;
    }
    let script = dir.join("script.txt");
    tokio::fs::write(&script, SCRIPT).await.ok()?;
    let aiff = dir.join("narration.aiff");
    let status = Command::new(&say)
        .arg("-f")
        .arg(&script)
        .arg("-o")
        .arg(&aiff)
        .status()
        .await
        .ok()?;
    status.success().then_some(aiff)
}

async fn render(
    ffmpeg: &str,
    audio_args: &[&str],
    dest: &Path,
    shortest: bool,
) -> anyhow::Result<()> {
    let mut args: Vec<&str> = vec!["-y", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30"];
    args.extend_from_slice(audio_args);
    if shortest {
        args.push("-shortest");
    }
    args.extend_from_slice(&[
        "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-c:a", "aac",
    ]);
    let out = Command::new(ffmpeg)
        .args(&args)
        .arg(dest)
        .output()
        .await
        .context("running ffmpeg for sample synthesis")?;
    if !out.status.success() {
        return Err(anyhow!(
            "ffmpeg failed synthesizing the sample: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn existing_sample_is_reused_without_synthesis() {
        let tmp = std::env::temp_dir().join(format!("cf-sample-{}", crate::util::short_id()));
        let data_dir = tmp.join("data");
        tokio::fs::create_dir_all(sample_path(&data_dir).parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(sample_path(&data_dir), b"cached")
            .await
            .unwrap();

        let got = ensure_sample(&data_dir, "/nonexistent-ffmpeg")
            .await
            .expect("cached sample should short-circuit");

        assert_eq!(got, sample_path(&data_dir));
        let bytes = tokio::fs::read(&got).await.unwrap();
        assert_eq!(bytes, b"cached");
        tokio::fs::remove_dir_all(tmp).await.ok();
    }
}
