// Fixture state for the Clipping Factory studio.
//
// Every payload here mirrors the shapes in src/domain.rs and src/api.rs:
// Project, StageRecord, SourceInfo, ClipRecord, Scores, LayoutPlan,
// LibraryEntry, /api/setup and /api/settings/ai. All content is deliberately
// fake and stable ("Example Episode", 00:47). If a field changes in the Rust
// structs, change it here too or a state will stop rendering truthfully.

export const FIXED_NOW = Date.UTC(2026, 2, 12, 16, 26, 0); // 2026-03-12T16:26:00Z
export const PROJECT_ID = "a1b2c3d4e5";
export const SOURCE_NAME = "Example Episode.mp4";
export const STAGE_NAMES = [
  "inspecting",
  "extracting_audio",
  "transcribing",
  "selecting_candidates",
  "validating_candidates",
  "analyzing_layout",
  "rendering",
];

const SCORES = {
  self_contained: 5,
  opening_strength: 4,
  specificity: 4,
  tension_or_novelty: 4,
  payoff: 5,
  clarity: 5,
  context_dependency: 1,
  slop_risk: 1,
};

const SOURCE = {
  filename: SOURCE_NAME,
  duration_ms: 2_832_000, // 47:12
  width: 1920,
  height: 1080,
  fps: 29.97,
  video_codec: "h264",
  audio_codec: "aac",
  size_bytes: 1_284_000_000,
  scene_boundaries_ms: [41_000, 122_500, 344_000, 612_000, 901_000],
};

function crop(t_ms, cx, cy = 0.5, zoom = 1.0) {
  return { t_ms, cx, cy, zoom, pad: false };
}

const FACE = { mode: "face_crop", keyframes: [crop(0, 0.42), crop(9_400, 0.58), crop(21_000, 0.46, 0.46, 1.18)] };
const PAD = { mode: "blur_pad" };

const CLIPS = [
  {
    id: "c1a2b3c4d5",
    rank: 1,
    headline: "The pricing floor nobody says out loud",
    filename: "01-the-pricing-floor-nobody-says-out-loud.mp4",
    start_ms: 724_000,
    end_ms: 761_000,
    duration_ms: 37_000,
    selection_reason:
      "Names the number everyone avoids, then walks through the two deals that set it. Stands alone without the rest of the episode.",
    scores: SCORES,
    score: 8.7,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "impact",
    accent_color: "#FFDD00",
    caption_font: "Inter",
    caption_text: null,
    width: 1080,
    height: 1920,
    auto_cut: false,
    cut_spans: null,
    zoom_cuts: false,
    zoom_keys: null,
    progress_bar: false,
    hook_title: false,
  },
  {
    id: "c2b3c4d5e6",
    rank: 2,
    headline: "Why the launch slipped twice",
    filename: "02-why-the-launch-slipped-twice.mp4",
    start_ms: 1_699_000,
    end_ms: 1_732_000,
    duration_ms: 33_000,
    selection_reason:
      "A clean before-and-after: the date, the reason it moved, and what changed the second time. Opens mid-thought.",
    scores: SCORES,
    score: 8.2,
    layout: PAD,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "clean",
    accent_color: "#7CFF4F",
    caption_font: "Inter",
    caption_text: null,
    width: 1080,
    height: 1920,
    auto_cut: true,
    cut_spans: [],
    zoom_cuts: false,
    zoom_keys: null,
    progress_bar: true,
    hook_title: false,
  },
  {
    id: "c3c4d5e6f7",
    rank: 3,
    headline: "A hiring bar that hides the real problem",
    filename: "03-a-hiring-bar-that-hides-the-real-problem.mp4",
    start_ms: 2_467_000,
    end_ms: 2_504_000,
    duration_ms: 37_000,
    selection_reason:
      "Turns a familiar complaint into a concrete diagnosis in the first ten seconds, then pays it off with one example.",
    scores: SCORES,
    score: 7.6,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "pop",
    accent_color: "#FF4F4F",
    caption_font: "Anton",
    caption_text: null,
    width: 1080,
    height: 1920,
    auto_cut: false,
    cut_spans: null,
    zoom_cuts: true,
    zoom_keys: [],
    progress_bar: false,
    hook_title: false,
  },
  {
    id: "c4d5e6f7a8",
    rank: 4,
    headline: "The part of the demo that broke",
    filename: "04-the-part-of-the-demo-that-broke.mp4",
    start_ms: 3_303_000,
    end_ms: 3_329_000,
    duration_ms: 26_000,
    selection_reason:
      "A short, self-contained story with a real failure in it. The laughter at the end lands without context.",
    scores: SCORES,
    score: 7.1,
    layout: PAD,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "cinema",
    accent_color: "#4FB5FF",
    caption_font: "Georgia",
    caption_text: null,
    width: 1080,
    height: 1920,
    auto_cut: false,
    cut_spans: null,
    zoom_cuts: false,
    zoom_keys: null,
    progress_bar: false,
    hook_title: true,
  },
  {
    id: "c5e6f7a8b9",
    rank: 5,
    headline: "One metric that turned the room",
    filename: "05-one-metric-that-turned-the-room.mp4",
    start_ms: 3_798_000,
    end_ms: 3_835_000,
    duration_ms: 37_000,
    selection_reason:
      "The number does the work: one chart, one reaction, one decision. Audio is rough in the middle but the words are clear.",
    scores: SCORES,
    score: 6.8,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: true,
    caption_style: "impact",
    accent_color: "#FF9F1C",
    caption_font: "Inter",
    caption_text: null,
    width: 1080,
    height: 1920,
    auto_cut: false,
    cut_spans: null,
    zoom_cuts: false,
    zoom_keys: null,
    progress_bar: false,
    hook_title: false,
  },
  {
    id: "c6f7a8b9c0",
    rank: 6,
    headline: "Where the advice stops working",
    filename: "06-where-the-advice-stops-working.mp4",
    start_ms: 4_660_000,
    end_ms: 4_692_000,
    duration_ms: 32_000,
    selection_reason:
      "Pushes back on the episode's own conclusion. Slightly dependent on the earlier segment, but the caveat is fully stated.",
    scores: SCORES,
    score: 6.2,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "clean",
    accent_color: "#C77DFF",
    caption_font: "Inter",
    caption_text: null,
    width: 1080,
    height: 1920,
    auto_cut: false,
    cut_spans: null,
    zoom_cuts: false,
    zoom_keys: null,
    progress_bar: false,
    hook_title: false,
  },
];

const REJECTED = [
  {
    headline: "Welcome back to the show",
    start_ms: 2_000,
    end_ms: 19_000,
    reasons: ["Cold-open guard: opens on a greeting", "Context dependency: needs the previous sentence"],
    score: 2.4,
  },
  {
    headline: "So yeah, that was pretty much it",
    start_ms: 2_831_000,
    end_ms: 2_858_000,
    reasons: ["Payoff: no conclusion", "Slop risk: filler-heavy, nothing at stake"],
    score: 3.1,
  },
  {
    headline: "Um, I mean, I guess",
    start_ms: 542_000,
    end_ms: 560_000,
    reasons: ["Cold-open guard: lone filler word", "Self-contained: no claim to stand on"],
    score: 1.8,
  },
  {
    headline: "Anyway, back to the topic",
    start_ms: 3_750_000,
    end_ms: 3_778_000,
    reasons: ["Scene guard: opens inside a camera transition", "Clarity: refers to an earlier thread"],
    score: 3.9,
  },
];

function stage(name, { started = null, completed = null, progress = null, detail = null, error = null } = {}) {
  return { name, started_at: started, completed_at: completed, progress, detail, error };
}

const DONE_STAGES = [
  stage("inspecting", { started: "2026-03-12T14:02:03Z", completed: "2026-03-12T14:02:06Z", progress: 1, detail: "1920×1080 · h264 / aac · 47:12" }),
  stage("extracting_audio", { started: "2026-03-12T14:02:06Z", completed: "2026-03-12T14:02:39Z", progress: 1, detail: "48 kHz mono · 263 MB" }),
  stage("transcribing", { started: "2026-03-12T14:02:39Z", completed: "2026-03-12T14:08:14Z", progress: 1, detail: "1,284 words · confidence 0.94" }),
  stage("selecting_candidates", { started: "2026-03-12T14:08:14Z", completed: "2026-03-12T14:08:19Z", progress: 1, detail: "12 candidates" }),
  stage("validating_candidates", { started: "2026-03-12T14:08:19Z", completed: "2026-03-12T14:08:21Z", progress: 1, detail: "6 kept · 4 rejected" }),
  stage("analyzing_layout", { started: "2026-03-12T14:08:21Z", completed: "2026-03-12T14:09:12Z", progress: 1, detail: "6 of 6 clips framed" }),
  stage("rendering", { started: "2026-03-12T14:09:12Z", completed: "2026-03-12T14:14:02Z", progress: 1, detail: "6 of 6 clips rendered" }),
];

function project(overrides = {}) {
  return {
    id: PROJECT_ID,
    created_at: "2026-03-12T14:02:00Z",
    status: "complete",
    source: SOURCE,
    source_path: `/fixtures/${SOURCE_NAME}`,
    stages: DONE_STAGES,
    error: null,
    selector: "local",
    warning: null,
    caption_style: "impact",
    accent_color: "#FFDD00",
    framing_mode: "fill",
    language: "auto",
    focus_prompt: "pricing",
    platform: "generic",
    ...overrides,
  };
}

function view(overrides = {}) {
  return {
    project: project(),
    source_name: SOURCE_NAME,
    running: false,
    live: null,
    accepted: 6,
    rejected: 4,
    rejected_summary: REJECTED,
    selector: "local",
    caption_only: false,
    clips: CLIPS,
    output_dir: "/Users/example/Movies/Clipping Factory/Example Episode",
    ...overrides,
  };
}

const PROCESSING_VIEW = view({
  project: project({
    status: "transcribing",
    stages: [
      DONE_STAGES[0],
      DONE_STAGES[1],
      stage("transcribing", { started: "2026-03-12T16:19:38Z", progress: 0.38, detail: null }),
      ...STAGE_NAMES.slice(3).map((name) => stage(name)),
    ],
    focus_prompt: "pricing",
  }),
  running: true,
  accepted: 0,
  rejected: 0,
  rejected_summary: [],
  clips: [],
  output_dir: null,
  live: {
    stage: "transcribing",
    progress: 0.38,
    detail: null,
    elapsed_ms: 382_000,
    stage_estimate_ms: 900_000,
    overall_progress: 0.34,
    pending_ms: 1_440_000,
  },
});

const ERROR_VIEW = view({
  project: project({
    status: "failed",
    stages: [
      DONE_STAGES[0],
      DONE_STAGES[1],
      stage("transcribing", {
        started: "2026-03-12T14:02:39Z",
        progress: 0.31,
        error: "whisper-cli exited with code 134 (SIGABRT)",
      }),
      ...STAGE_NAMES.slice(3).map((name) => stage(name)),
    ],
    error: "Transcription failed: whisper-cli exited with code 134. Check that the model file is intact, then retry.",
  }),
  running: false,
  accepted: 0,
  rejected: 0,
  rejected_summary: [],
  clips: [],
  output_dir: null,
  live: null,
});

const LIBRARY = [
  {
    id: PROJECT_ID,
    created_at: "2026-03-12T14:02:00Z",
    status: "complete",
    source_filename: SOURCE_NAME,
    width: 1920,
    height: 1080,
    duration_ms: 2_832_000,
    file_count: 46,
    size_bytes: 1_284_000_000,
    clips_total: 6,
    clips_ready: 6,
    clips_failed: 0,
  },
  {
    id: "b2c3d4e5f6",
    created_at: "2026-03-11T18:40:00Z",
    status: "complete",
    source_filename: "Design review with the founders.mp4",
    width: 1920,
    height: 1080,
    duration_ms: 1_920_000,
    file_count: 24,
    size_bytes: 642_000_000,
    clips_total: 3,
    clips_ready: 3,
    clips_failed: 0,
  },
  {
    id: "c3d4e5f6a7",
    created_at: "2026-03-10T15:12:00Z",
    status: "failed",
    source_filename: "Customer call — Northwind.mp4",
    width: 1280,
    height: 720,
    duration_ms: 2_460_000,
    file_count: 8,
    size_bytes: 118_000_000,
    clips_total: 0,
    clips_ready: 0,
    clips_failed: 0,
  },
  {
    id: "d4e5f6a7b8",
    created_at: "2026-03-09T21:05:00Z",
    status: "cancelled",
    source_filename: "Q1 all hands.mp4",
    width: 1920,
    height: 1080,
    duration_ms: 3_640_000,
    file_count: 12,
    size_bytes: 902_000_000,
    clips_total: 2,
    clips_ready: 2,
    clips_failed: 0,
  },
  {
    id: "e5f6a7b8c9",
    created_at: "2026-03-08T17:20:00Z",
    status: "transcribing",
    source_filename: "Webinar rehearsal.mp4",
    width: 1920,
    height: 1080,
    duration_ms: 2_100_000,
    file_count: 4,
    size_bytes: 310_000_000,
    clips_total: 0,
    clips_ready: 0,
    clips_failed: 0,
  },
];

const LANGUAGES = [
  { code: "en", name: "English" },
  { code: "es", name: "Spanish" },
  { code: "fr", name: "French" },
  { code: "de", name: "German" },
  { code: "pt", name: "Portuguese" },
  { code: "ja", name: "Japanese" },
  { code: "ko", name: "Korean" },
  { code: "zh", name: "Chinese" },
];

const GOOD_SETUP = {
  ffmpeg: true,
  ffmpeg_ass: true,
  ffprobe: true,
  whisper_cli: "/opt/homebrew/bin/whisper-cli",
  whisper_ok: true,
  model_ok: true,
  model_mb: 148,
  model_multilingual: true,
  whisper_languages: LANGUAGES,
  face_model_ok: true,
  caption_font: "Inter",
  caption_fonts: ["Inter", "Anton", "Arial", "Helvetica Neue", "Avenir Next", "Verdana", "Georgia"],
  caption_styles: ["impact", "clean", "pop", "cinema"],
  accent_palette: ["#FFDD00", "#7CFF4F", "#FF4F4F", "#4FB5FF", "#C77DFF", "#FF9F1C"],
  disk_free_gb: 84.2,
  data_dir: "/Users/example/.clipping-factory",
  output_root: "/Users/example/Movies/Clipping Factory",
};

const MISSING_RUNTIME_SETUP = {
  ...GOOD_SETUP,
  ffmpeg: false,
  ffmpeg_ass: false,
  ffprobe: false,
  whisper_cli: "",
  whisper_ok: false,
  model_ok: false,
  model_mb: 0,
  disk_free_gb: 1.4,
};

const SETTINGS = { provider: "openai", model: "gpt-4o-mini", base_url: "", connected: false };

const BASE = {
  setup: GOOD_SETUP,
  settings: SETTINGS,
  library: LIBRARY,
  view: null,
  live: null,
  project: false,
};

export const STATES = [
  { name: "empty-upload", ...BASE },
  { name: "staged-file", ...BASE },
  { name: "processing-mid-run", ...BASE, setup: GOOD_SETUP, library: LIBRARY, view: PROCESSING_VIEW, live: PROCESSING_VIEW.live, project: true },
  { name: "processing-error", ...BASE, view: ERROR_VIEW, project: true },
  { name: "results-6-clips", ...BASE, view: view(), project: true },
  { name: "results-rejected-expanded", ...BASE, view: view(), project: true },
  { name: "library-open", ...BASE, view: view(), project: true },
  { name: "clip-detail-restyle", ...BASE, view: view(), project: true },
  { name: "setup-missing-runtime", ...BASE, setup: MISSING_RUNTIME_SETUP },
];

const BY_NAME = new Map(STATES.map((state) => [state.name, state]));

export function fixtureFor(name) {
  return BY_NAME.get(name) || BY_NAME.get("empty-upload");
}

export function clipFor(id) {
  return CLIPS.find((clip) => clip.id === id) || null;
}

export const CLIP_FRAME_INDEX = Object.fromEntries(CLIPS.map((clip, index) => [clip.id, index + 1]));

export function frameIndexFor(id) {
  const index = CLIPS.findIndex((clip) => clip.id === id);
  return index >= 0 ? index + 1 : 1;
}
