//! Provenance & QC reporting: machine-checkable proof that a delivery's
//! clips are faithful excerpts of a named source, produced by named tools.
//!
//! Every field that cannot be established locally is serialized as `null` —
//! a provenance report must never fabricate a value. The same report is
//! served from `GET /api/projects/{id}/provenance.json` and written as a
//! `provenance.json` sidecar next to the delivered clips after a successful
//! render. Sidecar failures are logged, never fatal: provenance must not
//! break rendering.

use crate::config::Config;
use crate::domain::{Project, RenderManifest, SelectionReport};
use crate::store::Store;
use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use tokio::io::AsyncReadExt;

/// Streaming SHA-256 of a file, hex-encoded lowercase. Safe for multi-GB
/// sources: the file is read in fixed chunks, never held in memory.
pub async fn sha256_file(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Tool identities recorded in the report. `None` means "could not be
/// established", not "not used".
#[derive(Serialize, Clone, Debug, Default)]
pub struct ToolsInfo {
    /// First line of `ffmpeg -version`.
    pub ffmpeg_version: Option<String>,
    /// File name of the ggml model used for transcription.
    pub whisper_model: Option<String>,
}

/// SHA-256 of each rendered clip MP4, keyed by clip id. Ready clips only.
pub struct ClipHashes(pub HashMap<String, String>);

#[derive(Serialize, Clone, Debug)]
struct SourceProvenance {
    filename: Option<String>,
    size_bytes: Option<u64>,
    duration_ms: Option<u64>,
    source_sha256: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
struct SelectionSummary {
    selector: Option<String>,
    accepted: Option<usize>,
    rejected: Option<usize>,
}

#[derive(Serialize, Clone, Debug)]
struct ClipProvenance {
    rank: usize,
    headline: String,
    filename: String,
    start_ms: u64,
    end_ms: u64,
    duration_ms: u64,
    layout: &'static str,
    caption_style: Option<String>,
    accent_color: Option<String>,
    sha256: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProvenanceReport {
    generated_at: String,
    project_id: String,
    source: SourceProvenance,
    tools: ToolsInfo,
    selection: SelectionSummary,
    clips: Vec<ClipProvenance>,
}

/// Assemble the report from persisted project state. Pure aside from reading
/// nothing: all expensive work (hashing, tool probing) happens in [`gather`],
/// so this is directly unit-testable without real media.
pub fn build_report(
    project: &Project,
    manifest: Option<&RenderManifest>,
    selection: Option<&SelectionReport>,
    source_sha256: Option<String>,
    tools: ToolsInfo,
    hashes: &ClipHashes,
) -> ProvenanceReport {
    let source = match &project.source {
        Some(s) => SourceProvenance {
            filename: Some(s.filename.clone()),
            size_bytes: Some(s.size_bytes),
            duration_ms: Some(s.duration_ms),
            source_sha256,
        },
        None => SourceProvenance {
            filename: None,
            size_bytes: None,
            duration_ms: None,
            source_sha256: None,
        },
    };
    let selection = match selection {
        Some(r) => SelectionSummary {
            selector: Some(r.selector.clone()),
            accepted: Some(r.accepted.len()),
            rejected: Some(r.rejected.len()),
        },
        None => SelectionSummary {
            selector: project.selector.clone(),
            accepted: None,
            rejected: None,
        },
    };
    let mut clips: Vec<ClipProvenance> = manifest
        .map(|m| {
            m.clips
                .iter()
                .map(|c| ClipProvenance {
                    rank: c.rank,
                    headline: c.headline.clone(),
                    filename: c.filename.clone(),
                    start_ms: c.start_ms,
                    end_ms: c.end_ms,
                    duration_ms: c.duration_ms,
                    layout: c.layout.label(),
                    caption_style: c.caption_style.clone(),
                    accent_color: c.accent_color.clone(),
                    sha256: hashes.0.get(&c.id).cloned(),
                })
                .collect()
        })
        .unwrap_or_default();
    clips.sort_by_key(|c| c.rank);
    ProvenanceReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        project_id: project.id.clone(),
        source,
        tools,
        selection,
        clips,
    }
}

/// Hash every ready clip and probe tool versions, then assemble the report.
/// All probing is best-effort: unavailable facts become `null`s.
pub async fn gather(
    cfg: &Config,
    store: &Store,
    project: &Project,
    manifest: Option<&RenderManifest>,
    selection: Option<&SelectionReport>,
) -> ProvenanceReport {
    let source_sha256 = sha256_file(&store.source_path(&project.id)).await.ok();
    let ffmpeg_version = crate::util::run_capture(&cfg.ffmpeg, &["-version".into()])
        .await
        .ok()
        .and_then(|out| out.lines().next().map(str::to_string));
    let whisper_model = cfg.whisper_model.as_ref().map(|p| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    let tools = ToolsInfo {
        ffmpeg_version,
        whisper_model,
    };

    let mut by_clip_id = HashMap::new();
    if let Some(m) = manifest {
        for clip in m
            .clips
            .iter()
            .filter(|c| c.status == crate::domain::ClipStatus::Ready)
        {
            let path = store.clips_dir(&project.id).join(&clip.filename);
            if let Ok(hash) = sha256_file(&path).await {
                by_clip_id.insert(clip.id.clone(), hash);
            }
        }
    }

    build_report(
        project,
        manifest,
        selection,
        source_sha256,
        tools,
        &ClipHashes(by_clip_id),
    )
}

/// Write the report as `provenance.json` inside `dir` (the delivered output
/// folder). Best-effort: failures are logged and swallowed.
pub async fn write_sidecar(report: &ProvenanceReport, dir: &Path) {
    let body = match serde_json::to_vec_pretty(report) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("provenance sidecar serialization failed: {e}");
            return;
        }
    };
    if let Err(e) = crate::util::atomic_write_bytes(&dir.join("provenance.json"), &body).await {
        tracing::warn!("provenance sidecar write failed: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Candidate, ClipRecord, ClipStatus, LayoutPlan, Scores, ValidatedCandidate,
    };

    fn test_project() -> Project {
        Project::new("prov-test".into(), "/tmp/source.mp4".into())
    }

    fn test_clip(id: &str, rank: usize, layout: LayoutPlan) -> ClipRecord {
        ClipRecord {
            id: id.into(),
            rank,
            headline: format!("Clip {rank}"),
            filename: format!("{rank:02}-clip.mp4"),
            start_ms: 1_000,
            end_ms: 31_000,
            duration_ms: 30_000,
            selection_reason: "strong moment".into(),
            scores: Scores::default(),
            layout,
            status: ClipStatus::Ready,
            error: None,
            low_confidence: false,
            caption_style: Some("impact".into()),
            accent_color: Some("#FF5500".into()),
            caption_font: None,
            caption_text: None,
        }
    }

    fn test_manifest(clips: Vec<ClipRecord>) -> RenderManifest {
        RenderManifest {
            clips,
            output_dir: None,
        }
    }

    fn test_candidate() -> Candidate {
        Candidate {
            start_ms: 0,
            end_ms: 1,
            headline: "h".into(),
            opening_quote: String::new(),
            closing_quote: String::new(),
            selection_reason: String::new(),
            scores: Scores::default(),
        }
    }

    #[tokio::test]
    async fn sha256_matches_known_vector() {
        let path = std::env::temp_dir().join(format!("cf-prov-{}", crate::util::short_id()));
        tokio::fs::write(&path, b"abc").await.unwrap();
        let hash = sha256_file(&path).await.unwrap();
        tokio::fs::remove_file(&path).await.ok();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn missing_pieces_serialize_as_null() {
        let project = test_project();
        let report = build_report(
            &project,
            None,
            None,
            None,
            ToolsInfo::default(),
            &ClipHashes(HashMap::new()),
        );
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["source"]["filename"], serde_json::Value::Null);
        assert_eq!(v["source"]["source_sha256"], serde_json::Value::Null);
        assert_eq!(v["tools"]["ffmpeg_version"], serde_json::Value::Null);
        assert_eq!(v["selection"]["accepted"], serde_json::Value::Null);
        assert_eq!(v["clips"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn clips_are_ordered_by_rank_with_layout_and_hash_passthrough() {
        let mut project = test_project();
        project.source = Some(crate::domain::SourceInfo {
            filename: "show.mp4".into(),
            duration_ms: 600_000,
            width: 1920,
            height: 1080,
            fps: 30.0,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            size_bytes: 123,
        });
        let manifest = test_manifest(vec![
            test_clip("b", 2, LayoutPlan::BlurPad),
            test_clip("a", 1, LayoutPlan::FaceCrop { keyframes: vec![] }),
        ]);
        let selection = SelectionReport {
            selector: "offline heuristic".into(),
            accepted: vec![ValidatedCandidate {
                candidate: test_candidate(),
                rank: 1,
                composite: 0.8,
                duration_exception: false,
            }],
            rejected: Vec::new(),
        };
        let mut hashes = HashMap::new();
        hashes.insert("a".to_string(), "hash-a".to_string());
        let report = build_report(
            &project,
            Some(&manifest),
            Some(&selection),
            Some("hash-src".into()),
            ToolsInfo {
                ffmpeg_version: Some("ffmpeg v7".into()),
                whisper_model: Some("ggml-base.en.bin".into()),
            },
            &ClipHashes(hashes),
        );
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["source"]["filename"], "show.mp4");
        assert_eq!(v["source"]["source_sha256"], "hash-src");
        assert_eq!(v["selection"]["selector"], "offline heuristic");
        assert_eq!(v["selection"]["accepted"], 1);
        assert_eq!(v["selection"]["rejected"], 0);
        let clips = v["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0]["rank"], 1);
        assert_eq!(clips[1]["rank"], 2);
        assert_eq!(clips[0]["layout"], "face_crop");
        assert_eq!(clips[1]["layout"], "blur_pad");
        assert_eq!(clips[0]["sha256"], "hash-a");
        assert_eq!(clips[1]["sha256"], serde_json::Value::Null);
        assert_eq!(clips[0]["caption_style"], "impact");
    }
}
