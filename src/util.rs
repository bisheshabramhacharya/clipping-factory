//! Small shared utilities: subprocess streaming with cancellation, atomic
//! JSON persistence, slugs, ids, and disk space checks.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Run a subprocess, streaming stdout/stderr lines to `on_line(is_stderr, line)`.
/// Kills the child immediately if `cancel` fires. Fails on non-zero exit with
/// the last few stderr lines included in the error message.
pub async fn run_streaming<F>(
    bin: &str,
    args: &[String],
    cancel: &CancellationToken,
    mut on_line: F,
) -> Result<()>
where
    F: FnMut(bool, &str),
{
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to start `{}`", bin))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let (tx, mut rx) = mpsc::channel::<(bool, String)>(256);

    let tx_out = tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx_out.send((false, line)).await.is_err() {
                break;
            }
        }
    });
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send((true, line)).await.is_err() {
                break;
            }
        }
    });

    let mut stderr_tail: VecDeque<String> = VecDeque::with_capacity(8);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                bail!("cancelled");
            }
            msg = rx.recv() => match msg {
                Some((is_err, line)) => {
                    if is_err && !line.trim().is_empty() {
                        if stderr_tail.len() == 8 { stderr_tail.pop_front(); }
                        stderr_tail.push_back(line.clone());
                    }
                    on_line(is_err, &line);
                }
                None => break, // both streams closed
            }
        }
    }

    let status = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            bail!("cancelled");
        }
        s = child.wait() => s.context("waiting for subprocess")?,
    };

    if !status.success() {
        bail!(
            "`{}` exited with {} — {}",
            bin,
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into()),
            stderr_tail.iter().cloned().collect::<Vec<_>>().join(" | ")
        );
    }
    Ok(())
}

/// Run a subprocess to completion, capturing stdout. For quick, quiet tools
/// like ffprobe.
pub async fn run_capture(bin: &str, args: &[String]) -> Result<String> {
    let out = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to start `{}`", bin))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "`{}` exited with {} — {}",
            bin,
            out.status.code().unwrap_or(-1),
            err.lines().last().unwrap_or("").trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run a quiet subprocess while retaining cancellation control over the child.
/// `Command::output` owns its child, so it runs in a task that can be aborted;
/// `kill_on_drop` then terminates the child when cancellation wins the race.
pub async fn run_capture_cancellable(
    bin: &str,
    args: &[String],
    cancel: &CancellationToken,
) -> Result<String> {
    let bin = bin.to_string();
    let args = args.to_vec();
    let task_bin = bin.clone();
    let mut task = tokio::spawn(async move {
        Command::new(&task_bin)
            .args(&args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .with_context(|| format!("failed to start `{}`", task_bin))
    });

    let out = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            task.abort();
            let _ = task.await;
            bail!("cancelled");
        }
        result = &mut task => result.context("cancellable subprocess task failed")??,
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "`{}` exited with {} — {}",
            bin,
            out.status.code().unwrap_or(-1),
            err.lines().last().unwrap_or("").trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether this FFmpeg build can burn the generated ASS captions.
pub async fn ffmpeg_has_ass(bin: &str) -> bool {
    let args = ["-hide_banner".into(), "-filters".into()];
    run_capture(bin, &args)
        .await
        .map(|output| filter_list_has(&output, "ass"))
        .unwrap_or(false)
}

/// Cancellable variant used inside a pipeline run, where an FFmpeg capability
/// check must not hold cancellation acknowledgement open indefinitely.
pub async fn ffmpeg_has_ass_cancellable(bin: &str, cancel: &CancellationToken) -> Result<bool> {
    let args = ["-hide_banner".into(), "-filters".into()];
    let output = run_capture_cancellable(bin, &args, cancel).await?;
    Ok(filter_list_has(&output, "ass"))
}

fn filter_list_has(output: &str, name: &str) -> bool {
    output.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next();
        fields.next() == Some(name)
    })
}

/// Serialize JSON to a temp file, then atomically rename into place.
pub async fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_vec_pretty(value)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    let tmp = unique_temp_path(path);
    tokio::fs::write(&tmp, &json)
        .await
        .with_context(|| format!("writing {}", tmp.display()))?;
    tokio::fs::rename(&tmp, path)
        .await
        .with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

/// Return a unique sibling path that preserves the final extension so tools
/// such as FFmpeg still infer the intended container format.
pub fn unique_temp_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("artifact");
    let suffix = short_id();
    let name = match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!(".{stem}.part-{suffix}.{ext}"),
        None => format!(".{stem}.part-{suffix}"),
    };
    path.with_file_name(name)
}

/// Atomically promote a completed sibling artifact into its public path.
pub async fn promote_atomic(temp: &Path, final_path: &Path) -> Result<()> {
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::rename(temp, final_path)
        .await
        .with_context(|| format!("promoting {} to {}", temp.display(), final_path.display()))?;
    Ok(())
}

/// Atomically write a small marker or other byte payload.
pub async fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    let temp = unique_temp_path(path);
    tokio::fs::write(&temp, bytes)
        .await
        .with_context(|| format!("writing {}", temp.display()))?;
    promote_atomic(&temp, path).await
}

/// Short, URL-safe project/clip id.
pub fn short_id() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    id[..10].to_string()
}

/// Filesystem-safe slug for output filenames: `Why Discipline Fails!` → `why-discipline-fails`.
pub fn slugify(s: &str, max_len: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = true; // suppress leading dash
    for ch in s.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= max_len {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "clip".to_string()
    } else {
        trimmed
    }
}

/// Best-effort free disk space in GB for the filesystem containing `path`.
/// Uses `df -Pk`, which behaves consistently on macOS and Linux.
pub async fn disk_free_gb(path: &Path) -> Option<f64> {
    let out = Command::new("df")
        .arg("-Pk")
        .arg(path)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let avail_kb: f64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail_kb / 1024.0 / 1024.0)
}

/// Locate a binary on PATH.
pub fn which(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Why Discipline Fails!", 48), "why-discipline-fails");
        assert_eq!(slugify("  ---  ", 48), "clip");
        assert_eq!(
            slugify("The $5 mistake — that ended it", 48),
            "the-5-mistake-that-ended-it"
        );
    }

    #[test]
    fn slugify_truncates() {
        let s = slugify(
            "a very long headline that keeps going and going and going",
            20,
        );
        assert!(s.len() <= 20, "got {} ({})", s.len(), s);
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn detects_named_ffmpeg_filter() {
        let filters = " T. crop V->V Crop video.\n ... ass V->V Render ASS subtitles.\n";
        assert!(filter_list_has(filters, "ass"));
        assert!(!filter_list_has(filters, "subtitles"));
    }

    #[tokio::test]
    async fn cancellable_capture_stops_a_slow_child() {
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let result = run_capture_cancellable("/bin/sleep", &["5".into()], &cancel).await;
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
        canceller.await.unwrap();
    }

    #[tokio::test]
    async fn atomic_promotion_replaces_only_after_the_temp_is_complete() {
        let dir = std::env::temp_dir().join(format!("cf-atomic-{}", short_id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let final_path = dir.join("clip.mp4");
        let temp = unique_temp_path(&final_path);
        tokio::fs::write(&temp, b"complete").await.unwrap();
        promote_atomic(&temp, &final_path).await.unwrap();
        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), b"complete");
        assert!(!temp.exists());
        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
