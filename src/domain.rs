//! Core domain types shared across the pipeline, storage, and API layers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Ordered pipeline stages, matching PRD §14.2.
pub const STAGES: &[&str] = &[
    "inspecting",
    "extracting_audio",
    "transcribing",
    "selecting_candidates",
    "validating_candidates",
    "analyzing_layout",
    "rendering",
];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Created,
    Inspecting,
    ExtractingAudio,
    Transcribing,
    SelectingCandidates,
    ValidatingCandidates,
    AnalyzingLayout,
    Rendering,
    Complete,
    Cancelled,
    Failed,
}

impl JobState {
    pub fn from_stage(stage: &str) -> JobState {
        match stage {
            "inspecting" => JobState::Inspecting,
            "extracting_audio" => JobState::ExtractingAudio,
            "transcribing" => JobState::Transcribing,
            "selecting_candidates" => JobState::SelectingCandidates,
            "validating_candidates" => JobState::ValidatingCandidates,
            "analyzing_layout" => JobState::AnalyzingLayout,
            "rendering" => JobState::Rendering,
            _ => JobState::Created,
        }
    }

    /// True while a pipeline run is (or should be) actively working.
    pub fn is_active(&self) -> bool {
        !matches!(
            self,
            JobState::Created | JobState::Complete | JobState::Cancelled | JobState::Failed
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StageRecord {
    pub name: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// 0.0 – 1.0 when a meaningful percentage exists.
    pub progress: Option<f32>,
    /// Human-readable description of the current operation.
    pub detail: Option<String>,
    pub error: Option<String>,
}

impl StageRecord {
    pub fn new(name: &str) -> Self {
        StageRecord {
            name: name.to_string(),
            started_at: None,
            completed_at: None,
            progress: None,
            detail: None,
            error: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SourceInfo {
    pub filename: String,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub video_codec: String,
    pub audio_codec: String,
    pub size_bytes: u64,
    /// Scene-boundary timestamps (ms) detected once during inspection via
    /// ffmpeg `scdet`. Empty when detection ran before this field existed or
    /// failed — the validator then applies no transition guard.
    #[serde(default)]
    pub scene_boundaries_ms: Vec<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub status: JobState,
    pub source: Option<SourceInfo>,
    pub source_path: PathBuf,
    pub stages: Vec<StageRecord>,
    /// Top-level error message when status == Failed.
    pub error: Option<String>,
    /// Which selector produced candidates: "openai" | "anthropic" | "local" | "offline heuristic".
    pub selector: Option<String>,
    /// Non-fatal warning surfaced in the UI (e.g. low transcription confidence).
    pub warning: Option<String>,
    /// Caption style for this project: "impact" (default), "clean", "pop",
    /// or "cinema".
    #[serde(default)]
    pub caption_style: Option<String>,
    /// Accent color for the active caption word, as #RRGGBB.
    #[serde(default)]
    pub accent_color: Option<String>,
    /// Opt-in emoji accents flashed above the caption block.
    #[serde(default)]
    pub emoji_overlay: Option<bool>,
    /// Output composition selected before upload.
    #[serde(default)]
    pub framing_mode: FramingMode,
    /// Whisper language code picked at upload (`None`/`"auto"` = auto-detect).
    #[serde(default)]
    pub language: Option<String>,
    /// Free-text focus from project setup ("clips about pricing"). Steers
    /// candidate selection toward the topic; `None` keeps generic ranking.
    #[serde(default)]
    pub focus_prompt: Option<String>,
}

impl Project {
    pub fn new(id: String, source_path: PathBuf) -> Self {
        Project {
            id,
            created_at: Utc::now(),
            status: JobState::Created,
            source: None,
            source_path,
            stages: STAGES.iter().map(|s| StageRecord::new(s)).collect(),
            error: None,
            selector: None,
            warning: None,
            caption_style: None,
            accent_color: None,
            emoji_overlay: None,
            framing_mode: FramingMode::default(),
            language: None,
            focus_prompt: None,
        }
    }

    pub fn stage_mut(&mut self, name: &str) -> &mut StageRecord {
        let idx = self
            .stages
            .iter()
            .position(|s| s.name == name)
            .expect("unknown stage name");
        &mut self.stages[idx]
    }
}

// ---------------------------------------------------------------------------
// Transcript
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Word {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Mean token probability (0–1) for this word.
    pub p: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sentence {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Inclusive start / exclusive end indexes into `Transcript::words`.
    pub word_start: usize,
    pub word_end: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transcript {
    pub language: String,
    pub words: Vec<Word>,
    pub sentences: Vec<Sentence>,
    pub avg_confidence: f32,
}

// ---------------------------------------------------------------------------
// Candidates (editorial selection)
// ---------------------------------------------------------------------------

/// 1–5 rubric scores per PRD §9.2. For `context_dependency` and `slop_risk`,
/// 1 is safest and 5 is worst.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct Scores {
    pub self_contained: u8,
    pub opening_strength: u8,
    pub specificity: u8,
    pub tension_or_novelty: u8,
    pub payoff: u8,
    pub clarity: u8,
    pub context_dependency: u8,
    pub slop_risk: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Candidate {
    pub start_ms: u64,
    pub end_ms: u64,
    pub headline: String,
    pub opening_quote: String,
    pub closing_quote: String,
    pub selection_reason: String,
    pub scores: Scores,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ValidatedCandidate {
    pub candidate: Candidate,
    pub rank: usize,
    pub composite: f32,
    pub duration_exception: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RejectedCandidate {
    pub candidate: Candidate,
    pub reasons: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SelectionReport {
    pub selector: String,
    pub accepted: Vec<ValidatedCandidate>,
    pub rejected: Vec<RejectedCandidate>,
}

// ---------------------------------------------------------------------------
// Layout & rendering
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FramingMode {
    /// Fill the 9:16 canvas, following a face when one can be tracked.
    #[default]
    Fill,
    /// Preserve the full source over a blurred background.
    Background,
}

impl FramingMode {
    pub fn apply(self, analyzed: LayoutPlan) -> LayoutPlan {
        match (self, analyzed) {
            (FramingMode::Fill, tracked @ LayoutPlan::FaceCrop { .. }) => tracked,
            // Speaker-aware layouts still fill the canvas, just around two
            // faces — a user asking to fill never wants them flattened to
            // BlurPad or collapsed to one face.
            (FramingMode::Fill, planned @ LayoutPlan::Split { .. }) => planned,
            (FramingMode::Fill, planned @ LayoutPlan::SpeakerCrop { .. }) => planned,
            (FramingMode::Fill, LayoutPlan::BlurPad) => LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0,
                }],
            },
            (FramingMode::Background, _) => LayoutPlan::BlurPad,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum LayoutPlan {
    /// Smoothed vertical crop that follows one persistent face.
    FaceCrop { keyframes: Vec<CropKey> },
    /// Two-person interview shot: the frame split into two stacked panels,
    /// each a crop centered on one face. `top`/`bottom` are the face anchors
    /// in normalized source coordinates (left face renders on top).
    Split { top: FaceAnchor, bottom: FaceAnchor },
    /// Locked crop that cuts between faces at speaker-turn boundaries.
    /// Unlike FaceCrop keyframes (piecewise-linear legacy pans), every
    /// keyframe here applies instantly at its `t_ms` — a hard cut, matching
    /// the no-camera-motion rule (ADR-0001, amended by ADR-0004).
    SpeakerCrop { keyframes: Vec<CropKey> },
    /// Uncropped source centered over a blurred, darkened background.
    BlurPad,
}

/// A face position in normalized source coordinates — the anchor a crop
/// window or split panel is centered on.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct FaceAnchor {
    /// Normalized horizontal center (0–1).
    pub cx: f32,
    /// Normalized vertical center (0–1).
    pub cy: f32,
    /// The panel's eye-line offset in normalized panel heights: >0 slides
    /// the column down (blurred underlay fills above), <0 lifts it, 0 keeps
    /// the column centered (the framing used before eye-line anchoring).
    #[serde(default)]
    pub dy: f32,
}

impl LayoutPlan {
    pub fn label(&self) -> &'static str {
        match self {
            LayoutPlan::FaceCrop { .. } => "face_crop",
            LayoutPlan::Split { .. } => "split",
            LayoutPlan::SpeakerCrop { .. } => "speaker_crop",
            LayoutPlan::BlurPad => "blur_pad",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CropKey {
    /// Milliseconds relative to clip start.
    pub t_ms: u64,
    /// Normalized horizontal face center in the source frame (0–1).
    pub cx: f32,
    /// Eye-line offset in normalized canvas heights: >0 slides the crop down
    /// over a blurred underlay (blurred band fills above the frame), <0 lifts
    /// it, 0 keeps the frame centered (the framing used before eye-line
    /// anchoring and whenever face metadata lacks a usable vertical extent.
    #[serde(default)]
    pub dy: f32,
}

// ---------------------------------------------------------------------------
// Speaker diarization
// ---------------------------------------------------------------------------

/// One diarized speaker turn: the span of audio assigned to one voice.
/// `speaker` indexes `Diarization::labels`. Turn boundaries sit in silence
/// gaps — a turn never starts mid-word, so crop switches keyed to turns
/// cannot land mid-sentence.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeakerTurn {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Index into `Diarization::labels`.
    pub speaker: u8,
}

/// Project-level diarization persisted as `speakers.json`: who speaks when,
/// across the whole source. Produced once per project (not per clip) so
/// every clip sees the same voice → name mapping.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Diarization {
    /// Display names indexed by `SpeakerTurn::speaker`, in first-appearance
    /// order ("S1" is whoever talks first).
    pub labels: Vec<String>,
    /// Speaker turns sorted by `start_ms`, non-overlapping.
    pub turns: Vec<SpeakerTurn>,
}

impl Diarization {
    /// The speaker whose turn covers `t_ms`, when one does.
    pub fn speaker_at(&self, t_ms: u64) -> Option<u8> {
        self.turns
            .iter()
            .find(|t| t.start_ms <= t_ms && t_ms < t.end_ms)
            .map(|t| t.speaker)
    }

    /// Speaker label for a word: the turn covering the word's midpoint.
    /// Words sit inside speech spans by construction, and turn boundaries
    /// fall in the gaps between spans — so a word can never straddle a
    /// boundary. Midpoint lookup is the whole attribution rule.
    pub fn word_speaker(&self, word: &Word) -> Option<u8> {
        self.speaker_at(word.start_ms + word.end_ms.saturating_sub(word.start_ms) / 2)
    }

    /// Distinct speakers heard inside `[start_ms, end_ms)`.
    pub fn speakers_in(&self, start_ms: u64, end_ms: u64) -> Vec<u8> {
        let mut seen: Vec<u8> = self
            .turns
            .iter()
            .filter(|t| t.start_ms < end_ms && t.end_ms > start_ms)
            .map(|t| t.speaker)
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    }

    /// Turns overlapping `[start_ms, end_ms)`, in order.
    pub fn turns_in(&self, start_ms: u64, end_ms: u64) -> Vec<SpeakerTurn> {
        self.turns
            .iter()
            .filter(|t| t.start_ms < end_ms && t.end_ms > start_ms)
            .copied()
            .collect()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ClipStatus {
    Pending,
    Rendering,
    Ready,
    Failed,
}

/// One removed source interval, absolute milliseconds. Auto-cut stores the
/// spans it actually removed so restyle/retry can reproduce the same cut
/// without re-detecting silence.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CutSpan {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl CutSpan {
    pub fn len_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// One zoom keyframe: magnification `z` at `t_ms` on the clip's output
/// (post-cut) timeline. `z` is 1.0 at rest; a beat bumps it to a small peak
/// and returns to 1.0, so there is never net motion between beats.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct ZoomKey {
    pub t_ms: u64,
    pub z: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClipRecord {
    pub id: String,
    pub rank: usize,
    pub headline: String,
    pub filename: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub duration_ms: u64,
    pub selection_reason: String,
    pub scores: Scores,
    /// The validator's composite quality score for the Candidate behind this
    /// Clip — the same number that set `rank`. `None` on caption-only
    /// projects and manifests written before scores were surfaced.
    #[serde(default)]
    pub score: Option<f32>,
    pub layout: LayoutPlan,
    pub status: ClipStatus,
    pub error: Option<String>,
    /// True when transcription confidence inside this interval was low (PRD §10).
    pub low_confidence: bool,
    /// Caption style burned into the current render: "impact", "clean",
    /// "pop", or "cinema". `None` on manifests written before post-render
    /// restyling existed.
    #[serde(default)]
    pub caption_style: Option<String>,
    /// Accent color burned into the current render, as `#RRGGBB`.
    #[serde(default)]
    pub accent_color: Option<String>,
    /// Caption font burned into the current render.
    #[serde(default)]
    pub caption_font: Option<String>,
    /// Editable caption wording. Word timings are preserved when possible.
    #[serde(default)]
    pub caption_text: Option<String>,
    /// Whether the emoji accent overlay was burned into the current render.
    /// `None` on manifests written before the overlay existed.
    #[serde(default)]
    pub emoji_overlay: Option<bool>,
    /// Rendered output size of this clip (ADR-0002). `None` on manifests
    /// written before downscale-only output — those bases are fixed
    /// 1080×1920, which is what the render and restyle paths assume for them.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Opt-in auto-cut: remove silence gaps and filler words at render time.
    /// Default off — a Clip is otherwise one continuous faithful excerpt.
    #[serde(default)]
    pub auto_cut: bool,
    /// The removed spans the current base was rendered with. `None` while
    /// auto-cut is on but the cut list has not been computed yet; `Some([])`
    /// means detection ran and found nothing to remove.
    #[serde(default)]
    pub cut_spans: Option<Vec<CutSpan>>,
    /// Opt-in zoom cuts: subtle punch-in/out on emphasis beats at render
    /// time. Default off — the Locked crop never moves on its own.
    #[serde(default)]
    pub zoom_cuts: bool,
    /// The zoom keyframes the current base was rendered with, on the
    /// post-cut output timeline. `None` while zoom cuts are on but the key
    /// list has not been planned yet; `Some([])` means planning ran and
    /// found no beats.
    #[serde(default)]
    pub zoom_keys: Option<Vec<ZoomKey>>,
    /// Opt-in end card: a short "Made with Clipping Factory" tail appended
    /// after the clip's audio fade. Default off.
    #[serde(default)]
    pub end_card: bool,
    /// Opt-in progress bar: a thin accent-colored strip along the bottom
    /// edge filling over the clip's duration. Default off.
    #[serde(default)]
    pub progress_bar: bool,
    /// Opt-in hook title: the clip's headline burned as a title card over
    /// the opening beat, upper-third. Default off.
    #[serde(default)]
    pub hook_title: bool,
}

impl ClipRecord {
    /// The removals that apply to the current render: the stored cut list
    /// when auto-cut is on, otherwise nothing.
    pub fn effective_removals(&self) -> &[CutSpan] {
        if self.auto_cut {
            self.cut_spans.as_deref().unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// The zoom keyframes that apply to the current render: the stored
    /// key list when zoom cuts are on, otherwise nothing.
    pub fn effective_zoom_keys(&self) -> &[ZoomKey] {
        if self.zoom_cuts {
            self.zoom_keys.as_deref().unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// The base-intermediate key for this clip's current render state. Each
    /// render-affecting variant gets its own base (`<id>.cut`, `<id>.zoom`)
    /// so toggling never destroys the plain base — and a feature that
    /// planned nothing shares it, since the frames are identical.
    pub fn base_key(&self) -> String {
        let mut key = self.id.clone();
        if !self.effective_removals().is_empty() {
            key.push_str(".cut");
        }
        if !self.effective_zoom_keys().is_empty() {
            key.push_str(".zoom");
        }
        if self.end_card {
            key.push_str(".card");
        }
        if self.progress_bar {
            key.push_str(".bar");
        }
        if self.hook_title {
            key.push_str(".hook");
        }
        key
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RenderManifest {
    pub clips: Vec<ClipRecord>,
    /// Final user-facing output directory once at least one clip copied there.
    pub output_dir: Option<String>,
}

/// Format a millisecond offset as `MM:SS` (or `H:MM:SS` above one hour).
pub fn fmt_ms(ms: u64) -> String {
    let total_s = ms / 1000;
    let (h, m, s) = (total_s / 3600, (total_s % 3600) / 60, total_s % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_framing_keeps_face_tracking_when_available() {
        let tracked = LayoutPlan::FaceCrop {
            keyframes: vec![CropKey {
                t_ms: 0,
                cx: 0.42,
                dy: 0.0,
            }],
        };
        assert_eq!(FramingMode::Fill.apply(tracked.clone()), tracked);
    }

    #[test]
    fn fill_framing_uses_center_crop_when_tracking_is_unavailable() {
        assert_eq!(
            FramingMode::Fill.apply(LayoutPlan::BlurPad),
            LayoutPlan::FaceCrop {
                keyframes: vec![CropKey {
                    t_ms: 0,
                    cx: 0.5,
                    dy: 0.0
                }],
            }
        );
    }

    #[test]
    fn background_framing_always_preserves_the_full_source() {
        let tracked = LayoutPlan::FaceCrop {
            keyframes: vec![CropKey {
                t_ms: 0,
                cx: 0.42,
                dy: 0.0,
            }],
        };
        assert_eq!(FramingMode::Background.apply(tracked), LayoutPlan::BlurPad);
    }

    /// Manifests written before per-clip caption styling must still load.
    #[test]
    fn old_manifest_without_caption_fields_deserializes() {
        let old = r#"{
            "clips": [{
                "id": "c1", "rank": 1, "headline": "A test",
                "filename": "01-a-test.mp4",
                "start_ms": 1000, "end_ms": 31000, "duration_ms": 30000,
                "selection_reason": "why",
                "scores": {"self_contained":5,"opening_strength":4,"specificity":4,
                            "tension_or_novelty":4,"payoff":4,"clarity":5,
                            "context_dependency":1,"slop_risk":1},
                "layout": {"mode": "blur_pad"},
                "status": "ready", "error": null, "low_confidence": false
            }],
            "output_dir": null
        }"#;
        let m: RenderManifest = serde_json::from_str(old).expect("old manifest must load");
        assert_eq!(m.clips.len(), 1);
        assert_eq!(m.clips[0].score, None);
        assert_eq!(m.clips[0].caption_style, None);
        assert_eq!(m.clips[0].accent_color, None);
        assert_eq!(m.clips[0].caption_font, None);
        assert_eq!(m.clips[0].caption_text, None);
        assert_eq!(m.clips[0].emoji_overlay, None);
        assert_eq!(m.clips[0].width, None);
        assert_eq!(m.clips[0].height, None);
        // Auto-cut and zoom cuts default off for manifests written before
        // they existed.
        assert!(!m.clips[0].auto_cut);
        assert_eq!(m.clips[0].cut_spans, None);
        assert!(!m.clips[0].zoom_cuts);
        assert_eq!(m.clips[0].zoom_keys, None);
        assert_eq!(m.clips[0].base_key(), "c1");
    }
}
