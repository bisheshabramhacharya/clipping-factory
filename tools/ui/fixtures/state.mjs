// Fixture state for the Clipping Factory studio.
//
// Every payload here mirrors the shapes in src/domain.rs and src/api.rs:
// Project, StageRecord, SourceInfo, ClipRecord, Scores, LayoutPlan,
// LibraryEntry, /api/setup and /api/settings/ai. All content is deliberately
// fake and stable (fixture titles over real stills of a 04:48 episode). If a field changes in the Rust
// structs, change it here too or a state will stop rendering truthfully.

export const FIXED_NOW = Date.UTC(2026, 2, 12, 16, 26, 0); // 2026-03-12T16:26:00Z
export const PROJECT_ID = "a1b2c3d4e5";
export const SOURCE_NAME = "Avoiding Bankruptcy Before AGI – Dario Amodei - Dwarkesh Clips (720p).mp4";
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
  duration_ms: 288_182, // 04:48
  width: 1280,
  height: 720,
  fps: 23.976,
  video_codec: "h264",
  audio_codec: "aac",
  size_bytes: 23_020_602,
  scene_boundaries_ms: [41_000, 122_500, 171_000, 211_000, 251_000],
};

function crop(t_ms, cx, cy = 0.5, zoom = 1.0) {
  return { t_ms, cx, cy, zoom, pad: false };
}

const FACE = { mode: "face_crop", keyframes: [crop(0, 0.42), crop(9_400, 0.58), crop(21_000, 0.46, 0.46, 1.18)] };
const PAD = { mode: "blur_pad" };

// Titles are fixture copy, not transcript lines. The frames are real stills
// from the source named above; the words are the rig's stand-in for what a
// selector would have written.
const CLIPS = [
  {
    id: "c1a2b3c4d5",
    rank: 1,
    headline: "The window closes faster than you think",
    filename: "01-the-window-closes-faster-than-you-think.mp4",
    start_ms: 21_000,
    end_ms: 52_000,
    duration_ms: 31_000,
    selection_reason:
      "Names a deadline, then gives the two numbers behind it. Stands alone without the rest of the episode.",
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
    headline: "The compute number is the whole argument",
    filename: "02-the-compute-number-is-the-whole-argument.mp4",
    start_ms: 64_000,
    end_ms: 98_000,
    duration_ms: 34_000,
    selection_reason:
      "A clean before-and-after: the figure, where it comes from, and what changes if it holds. Opens mid-thought but recovers in two sentences.",
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
    headline: "What nobody says about the safety race",
    filename: "03-what-nobody-says-about-the-safety-race.mp4",
    start_ms: 112_000,
    end_ms: 139_000,
    duration_ms: 27_000,
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
    caption_font: "Inter",
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
    headline: "One chart ended that objection",
    filename: "04-one-chart-ended-that-objection.mp4",
    start_ms: 146_000,
    end_ms: 181_000,
    duration_ms: 35_000,
    selection_reason:
      "A short, self-contained answer with a real turn in it. The payoff lands without context.",
    scores: SCORES,
    score: 7.1,
    layout: PAD,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "cinema",
    accent_color: "#4FB5FF",
    caption_font: "Inter",
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
    headline: "The part everyone clips, and why it misses",
    filename: "05-the-part-everyone-clips-and-why-it-misses.mp4",
    start_ms: 192_000,
    end_ms: 224_000,
    duration_ms: 32_000,
    selection_reason:
      "Pushes back on the most quoted line of the episode. Audio is rough in the middle but the words are clear.",
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
    headline: "Where the optimistic case breaks",
    filename: "06-where-the-optimistic-case-breaks.mp4",
    start_ms: 238_000,
    end_ms: 264_000,
    duration_ms: 26_000,
    selection_reason:
      "Concedes the strongest counter-argument and then answers it. Slightly dependent on the earlier segment, but the caveat is fully stated.",
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
    start_ms: 271_000,
    end_ms: 283_000,
    reasons: ["Payoff: no conclusion", "Slop risk: filler-heavy, nothing at stake"],
    score: 3.1,
  },
  {
    headline: "Um, I mean, I guess",
    start_ms: 47_000,
    end_ms: 51_000,
    reasons: ["Cold-open guard: lone filler word", "Self-contained: no claim to stand on"],
    score: 1.8,
  },
  {
    headline: "Anyway, back to the topic",
    start_ms: 226_000,
    end_ms: 231_000,
    reasons: ["Scene guard: opens inside a camera transition", "Clarity: refers to an earlier thread"],
    score: 3.9,
  },
];

function stage(name, { started = null, completed = null, progress = null, detail = null, error = null } = {}) {
  return { name, started_at: started, completed_at: completed, progress, detail, error };
}

const DONE_STAGES = [
  stage("inspecting", { started: "2026-03-12T14:02:03Z", completed: "2026-03-12T14:02:06Z", progress: 1, detail: "1280×720 · h264 / aac · 04:48" }),
  stage("extracting_audio", { started: "2026-03-12T14:02:06Z", completed: "2026-03-12T14:02:39Z", progress: 1, detail: "48 kHz mono · 13 MB" }),
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
  { name: "review-theater", ...BASE, view: view(), project: true },
  { name: "review-done", ...BASE, view: view(), project: true },
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
