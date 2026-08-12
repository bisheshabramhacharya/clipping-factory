//! Filesystem JSON project store — no database, per PRD §13/§14.
//!
//! Layout:
//! ```text
//! <data_dir>/projects/<project-id>/
//!   project.json
//!   transcript.json
//!   candidates-raw.json      (selector proposals, pre-validation)
//!   candidates.json          (validated SelectionReport)
//!   render-manifest.json
//!   source.mp4
//!   audio.wav                (temporary; deleted after transcription)
//!   clips/
//! ```

use crate::domain::*;
use crate::util::{atomic_write_bytes, atomic_write_json};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(data_dir: &Path) -> Store {
        Store {
            root: data_dir.join("projects"),
        }
    }

    pub fn project_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }
    pub fn project_json(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("project.json")
    }
    pub fn source_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("source.mp4")
    }
    pub fn audio_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("audio.wav")
    }
    pub fn transcript_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("transcript.json")
    }
    pub fn raw_candidates_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("candidates-raw.json")
    }
    pub fn energy_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("energy.json")
    }
    pub async fn save_energy(&self, id: &str, e: &crate::energy::EnergyProfile) -> Result<()> {
        atomic_write_json(&self.energy_path(id), e).await
    }
    pub async fn load_energy(&self, id: &str) -> Option<crate::energy::EnergyProfile> {
        let bytes = tokio::fs::read(self.energy_path(id)).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }
    pub fn candidates_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("candidates.json")
    }
    pub fn manifest_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("render-manifest.json")
    }
    pub fn clips_dir(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("clips")
    }
    /// Uncaptioned framed intermediates, kept so captions can be restyled
    /// without re-doing the expensive framing render.
    pub fn base_dir(&self, id: &str) -> PathBuf {
        self.clips_dir(id).join("base")
    }
    pub fn base_clip_path(&self, id: &str, clip_id: &str) -> PathBuf {
        self.base_dir(id).join(format!("{clip_id}.mp4"))
    }
    pub fn base_ready_marker(&self, id: &str, clip_id: &str) -> PathBuf {
        self.base_dir(id).join(format!("{clip_id}.ready"))
    }
    pub fn final_ready_marker(&self, id: &str, clip_id: &str) -> PathBuf {
        self.clips_dir(id).join(format!("{clip_id}.ready"))
    }
    pub fn frames_dir(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("frames")
    }

    pub fn exists(&self, id: &str) -> bool {
        self.project_json(id).is_file()
    }

    pub async fn create_dirs(&self, id: &str) -> Result<()> {
        tokio::fs::create_dir_all(self.base_dir(id)).await?;
        Ok(())
    }

    /// A base clip is reusable only when its completed media and promotion
    /// marker both exist. This keeps old direct-to-final partials out of retry.
    pub async fn base_is_ready(&self, id: &str, clip_id: &str) -> Result<bool> {
        let media = tokio::fs::metadata(self.base_clip_path(id, clip_id)).await;
        let marker = tokio::fs::metadata(self.base_ready_marker(id, clip_id)).await;
        Ok(media.map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
            && marker.map(|m| m.is_file() && m.len() > 0).unwrap_or(false))
    }

    pub async fn mark_base_ready(&self, id: &str, clip_id: &str) -> Result<()> {
        atomic_write_bytes(&self.base_ready_marker(id, clip_id), b"ready\n").await
    }

    pub async fn clear_base_ready(&self, id: &str, clip_id: &str) {
        tokio::fs::remove_file(self.base_ready_marker(id, clip_id))
            .await
            .ok();
    }

    pub async fn final_is_ready(&self, id: &str, clip_id: &str, filename: &str) -> Result<bool> {
        let media = tokio::fs::metadata(self.clips_dir(id).join(filename)).await;
        let marker = tokio::fs::metadata(self.final_ready_marker(id, clip_id)).await;
        Ok(media.map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
            && marker.map(|m| m.is_file() && m.len() > 0).unwrap_or(false))
    }

    pub async fn mark_final_ready(&self, id: &str, clip_id: &str) -> Result<()> {
        atomic_write_bytes(&self.final_ready_marker(id, clip_id), b"ready\n").await
    }

    pub async fn clear_final_ready(&self, id: &str, clip_id: &str) {
        tokio::fs::remove_file(self.final_ready_marker(id, clip_id))
            .await
            .ok();
    }

    /// Remove artifacts that are not proven complete before a retry. Completed
    /// final clips and marked base clips remain available for incremental retry.
    pub async fn cleanup_partial_files(&self, id: &str) -> Result<()> {
        let project_dir = self.project_dir(id);
        if !project_dir.is_dir() {
            return Ok(());
        }

        let mut ready_names = HashSet::new();
        let mut ready_ids = HashSet::new();
        if let Ok(manifest) = self.load_manifest(id).await {
            for clip in manifest.clips {
                if clip.status != ClipStatus::Ready {
                    continue;
                }
                let media = self.clips_dir(id).join(&clip.filename);
                let legacy_media_is_complete = tokio::fs::metadata(&media)
                    .await
                    .map(|m| m.is_file() && m.len() > 0)
                    .unwrap_or(false);
                if legacy_media_is_complete && !self.final_ready_marker(id, &clip.id).is_file() {
                    self.mark_final_ready(id, &clip.id).await?;
                }
                if self.final_is_ready(id, &clip.id, &clip.filename).await? {
                    ready_names.insert(clip.filename);
                    ready_ids.insert(clip.id);
                }
            }
        }

        let mut root = tokio::fs::read_dir(&project_dir).await?;
        while let Some(entry) = root.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                if entry.file_name() == "frames" {
                    tokio::fs::remove_dir_all(path).await.ok();
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "audio.wav"
                || name.contains(".part-")
                || name.starts_with(".whisper-")
                || name.starts_with("audio.whisper")
            {
                tokio::fs::remove_file(path).await.ok();
            }
        }

        let clips_dir = self.clips_dir(id);
        if clips_dir.is_dir() {
            let mut clips = tokio::fs::read_dir(&clips_dir).await?;
            while let Some(entry) = clips.next_entry().await? {
                let path = entry.path();
                if entry.file_type().await?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".ass")
                    || name.contains(".part-")
                    || (name.ends_with(".mp4") && !ready_names.contains(&name))
                {
                    tokio::fs::remove_file(path).await.ok();
                } else if let Some(clip_id) = name.strip_suffix(".ready") {
                    if !ready_ids.contains(clip_id) {
                        tokio::fs::remove_file(path).await.ok();
                    }
                }
            }
        }

        let base_dir = self.base_dir(id);
        if base_dir.is_dir() {
            let mut bases = tokio::fs::read_dir(&base_dir).await?;
            while let Some(entry) = bases.next_entry().await? {
                let path = entry.path();
                if entry.file_type().await?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.contains(".part-") {
                    tokio::fs::remove_file(path).await.ok();
                } else if let Some(clip_id) = name.strip_suffix(".mp4") {
                    if ready_ids.contains(clip_id)
                        && tokio::fs::metadata(&path)
                            .await
                            .map(|m| m.is_file() && m.len() > 0)
                            .unwrap_or(false)
                        && !self.base_ready_marker(id, clip_id).is_file()
                    {
                        self.mark_base_ready(id, clip_id).await?;
                    }
                    if !self.base_is_ready(id, clip_id).await? {
                        tokio::fs::remove_file(&path).await.ok();
                        self.clear_base_ready(id, clip_id).await;
                    }
                } else if let Some(clip_id) = name.strip_suffix(".ready") {
                    if !self.base_clip_path(id, clip_id).is_file() {
                        tokio::fs::remove_file(path).await.ok();
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn save_project(&self, p: &Project) -> Result<()> {
        atomic_write_json(&self.project_json(&p.id), p).await
    }

    pub async fn load_project(&self, id: &str) -> Result<Project> {
        let bytes = tokio::fs::read(self.project_json(id))
            .await
            .with_context(|| format!("project {} not found", id))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn save_transcript(&self, id: &str, t: &Transcript) -> Result<()> {
        atomic_write_json(&self.transcript_path(id), t).await
    }
    pub async fn load_transcript(&self, id: &str) -> Result<Transcript> {
        let bytes = tokio::fs::read(self.transcript_path(id)).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn save_raw_candidates(&self, id: &str, c: &Vec<Candidate>) -> Result<()> {
        atomic_write_json(&self.raw_candidates_path(id), c).await
    }
    pub async fn load_raw_candidates(&self, id: &str) -> Result<Vec<Candidate>> {
        let bytes = tokio::fs::read(self.raw_candidates_path(id)).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn save_selection(&self, id: &str, r: &SelectionReport) -> Result<()> {
        atomic_write_json(&self.candidates_path(id), r).await
    }
    pub async fn load_selection(&self, id: &str) -> Result<SelectionReport> {
        let bytes = tokio::fs::read(self.candidates_path(id)).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn save_manifest(&self, id: &str, m: &RenderManifest) -> Result<()> {
        atomic_write_json(&self.manifest_path(id), m).await
    }
    pub async fn load_manifest(&self, id: &str) -> Result<RenderManifest> {
        let bytes = tokio::fs::read(self.manifest_path(id)).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PRD §16.3: project-state recovery must be covered by tests.
    #[tokio::test]
    async fn project_roundtrip_survives_reload() {
        let tmp = std::env::temp_dir().join(format!("cf-test-{}", crate::util::short_id()));
        let store = Store::new(&tmp);
        let id = "testproj01".to_string();
        store.create_dirs(&id).await.unwrap();

        let mut p = Project::new(id.clone(), store.source_path(&id));
        p.status = JobState::Transcribing;
        p.stage_mut("transcribing").progress = Some(0.42);
        p.stage_mut("transcribing").detail = Some("12:00 of 28:00".into());
        store.save_project(&p).await.unwrap();

        let mut loaded = store.load_project(&id).await.unwrap();
        assert_eq!(loaded.status, JobState::Transcribing);
        assert_eq!(loaded.stages.len(), STAGES.len());
        assert_eq!(loaded.stage_mut("transcribing").progress, Some(0.42));

        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn manifest_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("cf-test-{}", crate::util::short_id()));
        let store = Store::new(&tmp);
        let id = "testproj02".to_string();
        store.create_dirs(&id).await.unwrap();

        let m = RenderManifest {
            clips: vec![ClipRecord {
                id: "c1".into(),
                rank: 1,
                headline: "A test".into(),
                filename: "01-a-test.mp4".into(),
                start_ms: 1000,
                end_ms: 31000,
                duration_ms: 30000,
                selection_reason: "why".into(),
                scores: Scores::default(),
                layout: LayoutPlan::BlurPad,
                status: ClipStatus::Ready,
                error: None,
                low_confidence: false,
                caption_style: Some("impact".into()),
                accent_color: Some("#FFDD00".into()),
                caption_font: Some("Inter".into()),
                caption_text: None,
            }],
            output_dir: Some("/tmp/out".into()),
        };
        store.save_manifest(&id, &m).await.unwrap();
        let loaded = store.load_manifest(&id).await.unwrap();
        assert_eq!(loaded.clips.len(), 1);
        assert_eq!(loaded.clips[0].layout, LayoutPlan::BlurPad);
        assert_eq!(loaded.clips[0].status, ClipStatus::Ready);

        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn retry_cleanup_removes_unproven_artifacts_but_keeps_completed_clips() {
        let tmp = std::env::temp_dir().join(format!("cf-cleanup-{}", crate::util::short_id()));
        let store = Store::new(&tmp);
        let id = "cleanup01";
        store.create_dirs(id).await.unwrap();

        let ready = ClipRecord {
            id: "ready1".into(),
            rank: 1,
            headline: "Ready".into(),
            filename: "01-ready.mp4".into(),
            start_ms: 0,
            end_ms: 20_000,
            duration_ms: 20_000,
            selection_reason: "test".into(),
            scores: Scores::default(),
            layout: LayoutPlan::BlurPad,
            status: ClipStatus::Ready,
            error: None,
            low_confidence: false,
            caption_style: None,
            caption_text: None,
            accent_color: None,
            caption_font: None,
        };
        store
            .save_manifest(
                id,
                &RenderManifest {
                    clips: vec![ready],
                    output_dir: None,
                },
            )
            .await
            .unwrap();

        tokio::fs::write(store.audio_path(id), b"partial")
            .await
            .unwrap();
        tokio::fs::write(store.project_dir(id).join(".whisper-old.json"), b"partial")
            .await
            .unwrap();
        tokio::fs::write(store.project_dir(id).join("stale.part-old.mp4"), b"partial")
            .await
            .unwrap();
        tokio::fs::write(store.clips_dir(id).join("01-ready.mp4"), b"complete")
            .await
            .unwrap();
        store.mark_final_ready(id, "ready1").await.unwrap();
        tokio::fs::write(store.clips_dir(id).join("02-stale.mp4"), b"partial")
            .await
            .unwrap();
        tokio::fs::write(store.clips_dir(id).join("ready1.ass"), b"partial")
            .await
            .unwrap();
        tokio::fs::write(store.base_clip_path(id, "ready1"), b"complete")
            .await
            .unwrap();
        store.mark_base_ready(id, "ready1").await.unwrap();
        tokio::fs::write(store.base_clip_path(id, "stale1"), b"partial")
            .await
            .unwrap();

        store.cleanup_partial_files(id).await.unwrap();

        assert!(store.clips_dir(id).join("01-ready.mp4").is_file());
        assert!(store
            .final_is_ready(id, "ready1", "01-ready.mp4")
            .await
            .unwrap());
        assert!(store.base_is_ready(id, "ready1").await.unwrap());
        assert!(!store.audio_path(id).is_file());
        assert!(!store.project_dir(id).join(".whisper-old.json").is_file());
        assert!(!store.clips_dir(id).join("02-stale.mp4").is_file());
        assert!(!store.clips_dir(id).join("ready1.ass").is_file());
        assert!(!store.base_clip_path(id, "stale1").is_file());
        tokio::fs::remove_dir_all(&tmp).await.ok();
    }

    #[tokio::test]
    async fn retry_cleanup_preserves_and_marks_legacy_ready_clip_and_base() {
        let tmp =
            std::env::temp_dir().join(format!("cf-legacy-cleanup-{}", crate::util::short_id()));
        let store = Store::new(&tmp);
        let id = "legacy-cleanup01";
        store.create_dirs(id).await.unwrap();

        store
            .save_manifest(
                id,
                &RenderManifest {
                    clips: vec![ClipRecord {
                        id: "legacy-ready".into(),
                        rank: 1,
                        headline: "Legacy Ready".into(),
                        filename: "01-legacy-ready.mp4".into(),
                        start_ms: 0,
                        end_ms: 20_000,
                        duration_ms: 20_000,
                        selection_reason: "test".into(),
                        scores: Scores::default(),
                        layout: LayoutPlan::BlurPad,
                        status: ClipStatus::Ready,
                        error: None,
                        low_confidence: false,
                        caption_style: None,
                        caption_text: None,
                        accent_color: None,
                        caption_font: None,
                    }],
                    output_dir: None,
                },
            )
            .await
            .unwrap();

        let final_path = store.clips_dir(id).join("01-legacy-ready.mp4");
        let base_path = store.base_clip_path(id, "legacy-ready");
        tokio::fs::write(&final_path, b"legacy-final")
            .await
            .unwrap();
        tokio::fs::write(&base_path, b"legacy-base").await.unwrap();
        assert!(!store.final_ready_marker(id, "legacy-ready").exists());
        assert!(!store.base_ready_marker(id, "legacy-ready").exists());

        store.cleanup_partial_files(id).await.unwrap();

        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), b"legacy-final");
        assert_eq!(tokio::fs::read(&base_path).await.unwrap(), b"legacy-base");
        assert!(store
            .final_is_ready(id, "legacy-ready", "01-legacy-ready.mp4")
            .await
            .unwrap());
        assert!(store.base_is_ready(id, "legacy-ready").await.unwrap());

        tokio::fs::remove_dir_all(&tmp).await.ok();
    }
}
