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
  duration_ms: 288_182, // 04:48, as measured by the studio
  width: 1280,
  height: 720,
  fps: 23.976,
  video_codec: "h264",
  audio_codec: "aac",
  size_bytes: 23_020_602,
  scene_boundaries_ms: [37_700, 79_300, 114_700, 160_300, 203_700, 251_100],
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
    id: "e94045ebf9",
    rank: 1,
    headline: "There's a question of how much of that goes to the pharmaceutical company, to the AI…",
    filename: "01-there-s-a-question-of-how-much-of-that-goes-to-t.mp4",
    start_ms: 37745,
    end_ms: 79280,
    duration_ms: 41535,
    selection_reason:
      "The excerpt opens on a direct question, sets up a tension or misconception, contains a quick question-and-answer turn, repeats its central claim for emphasis, resolves at a natural sentence boundary. Selected by local ranking across the full transcript.",
    scores: SCORES,
    score: 37.2,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "impact",
    accent_color: "#FFDD00",
    caption_font: "Inter",
    caption_text:
      "How long does it take to cure all the diseases? That's one of the ways that drives a huge amount of economic value. You cure every disease. There's a question of how much of that goes to the pharmaceutical company, to the AI company, but there's an enormous consumer surplus because I assume we can get access for everyone, which I care about greatly. We cure all of these diseases. How long does it take? You have to do the biological discovery, you have to manufacture the new drug, you have to go through the regulatory process. We saw this with vaccines and COVID. There's just this, we got the vaccine out to everyone, but it took a year and a half.",
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
    id: "b4d136b518",
    rank: 2,
    headline: "How long from when that AI first exists in the lab to when diseases have actually been…",
    filename: "02-how-long-from-when-that-ai-first-exists-in-the-l.mp4",
    start_ms: 79280,
    end_ms: 114745,
    duration_ms: 35465,
    selection_reason:
      "The excerpt opens on a direct question, sets up a tension or misconception, grounds the point in specifics, contains a quick question-and-answer turn, resolves at a natural sentence boundary. Selected by local ranking across the full transcript.",
    scores: SCORES,
    score: 35.7,
    layout: PAD,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "clean",
    accent_color: "#7CFF4F",
    caption_font: "Inter",
    caption_text:
      "My question is, how long does it take to get the cure for everything, which AI is the genius that can in theory invent out to everyone? How long from when that AI first exists in the lab to when diseases have actually been cured for everyone? We've had a polio vaccine for 50 years. We're still trying to eradicate it in the most remote corners of Africa. The Gates Foundation is trying as hard as they can. Others are trying as hard as they can, but that's difficult. Again, I don't expect most of the economic diffusion to be as difficult as that.",
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
    id: "f311c0d6c7",
    rank: 3,
    headline: "Basically, I'm saying in 2027, how much compute do I get?",
    filename: "03-basically-i-m-saying-in-2027-how-much-compute-do.mp4",
    start_ms: 160300,
    end_ms: 203710,
    duration_ms: 43410,
    selection_reason:
      "The excerpt opens on a direct question, sets up a tension or misconception, grounds the point in specifics, contains a quick question-and-answer turn, resolves at a natural sentence boundary. Selected by local ranking across the full transcript.",
    scores: SCORES,
    score: 35.7,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "pop",
    accent_color: "#FF4F4F",
    caption_font: "Inter",
    caption_text:
      "Basically, I'm saying in 2027, how much compute do I get? I could assume that the revenue will continue growing 10x a year, so it will be $100 billion at the end of 2026, and $1 trillion at the end of 2027. I could buy a trillion dollars, actually it would be like $5 trillion of compute, because it would be a trillion dollar a year for five years. I could buy a trillion dollars of compute that starts at the end of 2027. If my revenue is not a trillion dollars, if it's even $800 billion, there's no foreson earth.",
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
    id: "a5a49340c6",
    rank: 4,
    headline: "We know it's coming, but with the way you buy these data centers, if you're off by a…",
    filename: "04-we-know-it-s-coming-but-with-the-way-you-buy-the.mp4",
    start_ms: 560,
    end_ms: 45660,
    duration_ms: 45100,
    selection_reason:
      "The excerpt opens on a complete thought, sets up a tension or misconception, grounds the point in specifics, repeats its central claim for emphasis, resolves at a natural sentence boundary. Selected by local ranking across the full transcript.",
    scores: SCORES,
    score: 32.8,
    layout: PAD,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "cinema",
    accent_color: "#4FB5FF",
    caption_font: "Inter",
    caption_text:
      "Even if the technology goes as fast as I suspect that it will, we don't know exactly how fast it's going to drive revenue. We know it's coming, but with the way you buy these data centers, if you're off by a couple years, that can be ruinous. It is just like how I wrote in machines of loving grace, I said, \"Look, I think we might get this powerful AI, this country of genius in the data center, that description you gave comes from the machines of loving grace.\" I said, \"We'll get that 2026, maybe 2027 again. That is my hunch. Wouldn't be surprised if I'm off by a year or two, but that is my hunch.\" Let's say that happens, that's the starting gun. How long does it take to cure all the diseases? That's one of the ways that drives a huge amount of economic value.",
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
    id: "3da71eb51b",
    rank: 5,
    headline: "$800 billion, there's no foreson earth",
    filename: "05-800-billion-there-s-no-foreson-earth.mp4",
    start_ms: 196260,
    end_ms: 251140,
    duration_ms: 54880,
    selection_reason:
      "The excerpt opens on a complete thought, sets up a tension or misconception, grounds the point in specifics, lands on a stated takeaway before it ends. Selected by local ranking across the full transcript.",
    scores: SCORES,
    score: 32.8,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "impact",
    accent_color: "#FF9F1C",
    caption_font: "Inter",
    caption_text:
      "If my revenue is not a trillion dollars, if it's even $800 billion, there's no foreson earth. There's no hedge on earth that could stop me from going bankrupt if I buy that much compute. Even though a part of my brain wonders if it's going to keep growing 10x, I can't buy a trillion dollars a year of compute in 2027. If I'm just off by a year in that rate of growth, or if the growth rate is 5x a year instead of 10x a year, then you go bankrupt. And so, you end up in a world where you're supporting hundreds of billions, not trillions, and you accept some risk that there's so much demand that you can't support the revenue, and you accept still some risk that you got it wrong and it's still slow.",
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
    id: "49982d7c1c",
    rank: 6,
    headline: "That's the most difficult case, but there's a real dilemma here",
    filename: "06-that-s-the-most-difficult-case-but-there-s-a-rea.mp4",
    start_ms: 104190,
    end_ms: 149410,
    duration_ms: 45220,
    selection_reason:
      "The excerpt opens on a complete thought, sets up a tension or misconception, grounds the point in specifics, resolves at a natural sentence boundary. Selected by local ranking across the full transcript.",
    scores: SCORES,
    score: 29.7,
    layout: FACE,
    status: "ready",
    error: null,
    low_confidence: false,
    caption_style: "clean",
    accent_color: "#C77DFF",
    caption_font: "Inter",
    caption_text:
      "The Gates Foundation is trying as hard as they can. Others are trying as hard as they can, but that's difficult. Again, I don't expect most of the economic diffusion to be as difficult as that. That's the most difficult case, but there's a real dilemma here. Where I've settled on it is, it will be faster than anything we've seen in the world, but it still has its limits. Then, when we go to buying data centers, again, the curve I'm looking at is we've had a 10x a year increase every year. Main of this year, we're looking at $10 billion in rate of annualized revenue at the beginning of the year.",
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
    start_ms: 258_000,
    end_ms: 271_000,
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
    clips_total: 0,
    clips_ready: 0,
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
  data_dir: process.env.CF_UI_DATA_DIR || "~/.clipping-factory",
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
