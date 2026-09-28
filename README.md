<div align="center">

# Clipping Factory

### One podcast in. Every strong, faithful clip out.

A local-first podcast clipping studio with full-transcript ranking, face-aware reframing, and word-accurate captions. Built entirely in Rust.

[![CI](https://github.com/bisheshabramhacharya/clipping-factory/actions/workflows/ci.yml/badge.svg)](https://github.com/bisheshabramhacharya/clipping-factory/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![Local first](https://img.shields.io/badge/processing-local--first-1f6feb)
![Output](https://img.shields.io/badge/output-up%20to%201080%C3%971920-7c3aed)

</div>

![Clipping Factory results showing six rendered vertical clips](docs/assets/studio-results.png)

Clipping Factory turns a podcast MP4 into a set of strong, distinct vertical clips. Every result is one continuous excerpt with word-timed captions and clean, face-aware framing.

The goal is simple: find clips worth posting without rewriting the speaker, inventing context, or hiding weak edits behind effects.

No account. No cloud upload. No required AI model. The built-in ranker scans the full transcript, and a deterministic validator rejects clips that depend on missing context or overlap stronger ones.

## What you get

| | |
|---|---|
| **More useful candidates** | Keeps every strong, distinct candidate instead of stopping at an arbitrary quota. |
| **Faithful excerpts** | Never rewrites, reorders, splices, or invents speech. |
| **Feed-ready video** | Produces 9:16 H.264/AAC MP4s at the source's native window size, up to 1080×1920. |
| **Word-accurate captions** | The highlighted word is the one being spoken, timed from whisper.cpp's DTW alignment. Impact, Clean, Pop, and Cinema styles with per-clip restyling in seconds. |
| **Clip controls** | Opt-in per clip: auto-cut silence/filler, zoom cuts on emphasis beats, hook title, progress bar. |
| **Export pack** | Every clip ships with .srt, .vtt, .meta.json (title/description/hashtags), and a poster still. |
| **Honest ranking** | Composite score and selection reason on every clip, plus what was rejected and why. |
| **~99 languages** | Whisper transcription auto-detects the language, or you pick it per project. |
| **Private by default** | Keeps video, audio, transcripts, project state, and rendering on your machine. |

<p align="center">
  <img src="docs/assets/clip-details.png" alt="Rendered clip cards with captions and speaker framing" width="620">
</p>

```text
Drop MP4 → Inspect → Extract audio → Transcribe → Find candidates
         → Validate → Analyze framing → Render → Preview and download
```

## Quickstart

The first supported setup is macOS on Apple Silicon.

```bash
# Media and transcription runtimes
brew install ffmpeg-full whisper-cpp

# Rust toolchain — skip this if Rust is already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Download the base transcription model once (multilingual)
mkdir -p ~/.clipping-factory/models
curl -L -o ~/.clipping-factory/models/ggml-base.bin \
  "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"

# Run from the repository root
cargo run --release
```

The studio opens at [http://localhost:4571](http://localhost:4571). Drop in one MP4 and the pipeline starts. The optional **Focus** field steers selection toward a topic ("clips about pricing", "where they argue"): it reaches the configured provider as an editorial directive, and under local ranking it falls back to keyword matching. Left blank, selection stays the generic best-clips ranking.

### Linux

Install FFmpeg through your package manager and build [whisper.cpp](https://github.com/ggml-org/whisper.cpp):

```bash
cmake -B build
cmake --build build -j --target whisper-cli
```

Set `CF_WHISPER_BIN` to the resulting `whisper-cli` path if it is not already on your `PATH`.

### Better transcription

`ggml-base` is the fast default and covers ~99 languages. For English podcasts, `ggml-small.en.bin` gives noticeably cleaner transcripts at about 2.4× the transcription time. Models are found in `~/.clipping-factory/models/`, `./models/`, and `~/.cache/whisper.cpp/`; the largest one wins, and English-only weights (`ggml-*.en.bin`, up to `medium.en`) win over multilingual ones of any size. A project that needs another language switches to the best multilingual model found. `CF_WHISPER_MODEL` overrides discovery.

Language is chosen per project at upload: **Auto-detect** is the default, or pick a specific language to skip detection. Non-English sources need a multilingual model.

## Local by default. AI optional.

Local ranking is the default and needs no API key. If you want model-assisted selection, open the provider control in the studio and connect OpenAI or Anthropic.

| Provider | Default model | Notes |
|---|---|---|
| Local ranking | — | Scans the full transcript locally. No key required. |
| Local endpoint | — | Ollama, llama.cpp, or LM Studio. Model-assisted, fully offline. |
| OpenAI | `gpt-4o-mini` | Accepts another chat-completions model name. |
| Anthropic | `claude-opus-5` | Optional alternative provider. |

When a provider is enabled, only transcript text is sent to it. The source video stays on your machine.

### Local endpoint

Point the studio at any OpenAI-compatible server on your machine: pick **Local endpoint** in the AI connection control, enter the base URL and a model the server already has, and test & save. Ollama is the shortest path:

```sh
ollama pull qwen2.5:7b   # any 7–8B instruct GGUF works
# base URL: http://localhost:11434/v1 (the default)
```

LM Studio serves at `http://localhost:1234/v1`, llama.cpp's `llama-server` at `http://localhost:8080/v1`. No API key is needed. If the endpoint is down or misbehaves during a run, selection falls back to local ranking with a warning — the pipeline never stalls on it.

API keys are stored in `~/.clipping-factory/settings.json` with user-only `0600` permissions. Keys are never logged or returned by the settings API.

## The anti-slop gate

Finding a possible candidate is not enough. Every candidate must pass the same deterministic validator before rendering:

- The excerpt must meet minimum scores for self-containment, payoff, clarity, and opening strength.
- Context dependency and slop risk must stay below fixed limits.
- The opening and closing quotes must appear in the transcript near the candidate's boundaries.
- Boundaries snap to real word timestamps instead of trusting model-generated milliseconds.
- A clip cannot overlap more than 30% with a higher-ranked result, or contain one entirely.
- Timestamps must stay inside the source duration.
- A clip may not open or close on a detected scene transition; cuts near one snap to word boundaries.
- A clip may not open on a greeting or filler word, or close on outro/CTA bait ("like and subscribe").
- Normal duration is 20–90 seconds, with a narrow exception for unusually strong candidates; clips in the 25–60s short-form sweet spot rank ahead of equal-scored longer ones.

Zero clips is a valid result. The studio shows what it considered, what it rejected, and which rule rejected it.

Sources shorter than 20 seconds skip selection: the whole video becomes one captioned clip (the **caption-only** path).

## Captions you choose after rendering

Clips render with the default style first. Each finished clip can then be restyled without repeating the expensive framing pass.

- **Impact** uses tight, kinetic stacks with one oversized key word and a restrained active-word accent.
- **Clean** uses compact conversational groups in the lower safe area with a softer active-word accent.
- **Pop** shows one word at a time, dead center, with keywords in caps.
- **Cinema** sets lowercase letterspaced lines that fade in like subtitle cards.

In every style the accent color sits only on the word being spoken, so exactly one word is ever highlighted.

You can switch styles, choose an accent color, tune the caption text, and apply the change from the result card. Clipping Factory re-burns captions from the cached base render in seconds.

## Per-clip garnishes

Every rendered clip carries opt-in toggles — each re-renders just that clip:

- **Auto-cut** removes silence gaps and filler words ("um", "uh") via a keep-list concat; captions stay in sync.
- **Zoom cuts** add `zoompan` punch-ins on energy and emphasis beats.
- **Hook title** burns the clip's headline over the first ~1.8 s: ALL-CAPS in Impact and Pop, headline case in Clean and Cinema.
- **Progress bar** draws a thin accent-colored fill along the bottom edge.

## House rendering rules

- H.264/AAC output at the native 9:16 window size, capped at 1080×1920. The only upscaling is a zoomed-in view, at most 1.6×.
- Source frame rate is preserved between 20 and 60 fps; anything outside that range renders at 30 fps.
- Each camera shot gets one static view that never pans or eases, and the clip hard-cuts where the source cuts.
- The view frames the person speaking at head-and-shoulders size. When several people share a shot, it cuts to whoever is talking on turns longer than about 2 s.
- A shot with nobody usable in it shows the full frame over a darkened blur, never a crop aimed at empty space.
- Captions use short conversational groups in the lower safe area.
- Audio is loudness-normalized to −16 LUFS with short edge fades on every clip.
- Defaults stay clean: no B-roll, music, or transitions — zoom cuts and the other extras are opt-in per clip.

## Outputs and local state

```text
~/Downloads/Clipping Factory/<source-name>/
  01-headline-slug.mp4

~/.clipping-factory/projects/<project-id>/
  project.json
  transcript.json
  candidates.json
  render-manifest.json
  clips/
```

Project state is plain JSON. Temporary audio is deleted after transcription. Finished clips survive retries, and interrupted projects can resume from the last completed stage.

## Architecture

Clipping Factory is a browser-based studio backed by one Rust binary. There is no Node or Python application runtime.

| Area | Implementation |
|---|---|
| Web server and API | axum, tokio, server-sent events, streaming multipart uploads |
| Media inspection and rendering | FFmpeg and FFprobe subprocesses |
| Transcription | whisper.cpp with DTW word timestamps, word ends trimmed to the audio |
| Editorial selection | Local ranker, optional local endpoint (Ollama/llama.cpp/LM Studio), optional OpenAI, optional Anthropic |
| Quality gate | Pure Rust deterministic validator |
| Framing | ffmpeg `scdet` shot cuts, rustface detections pooled per shot, mouth-motion active speaker |
| Captions | Generated ASS subtitles burned by libass |
| State | Atomic filesystem JSON writes |

```text
src/
  main.rs          startup and first-run checks
  config.rs        environment and tool discovery
  api.rs           HTTP API and static studio
  state.rs         shared app state and per-project run handles
  settings.rs      AI provider settings
  pipeline.rs      stage orchestration and clip rendering
  domain.rs        shared types (projects, candidates, clips, layouts)
  media.rs         probing and audio extraction
  transcribe.rs    whisper.cpp integration and word timing
  energy.rs        audio energy profile
  select/          local and optional model-assisted selection
  validate.rs      deterministic quality gate
  frame.rs         shot detection, active speaker, and framing views
  captions.rs      caption grouping and ASS generation
  accent.rs        accent color picking
  autocut.rs       silence and filler removal
  zoom.rs          zoom-cut planning
  render.rs        FFmpeg filter graphs
  export.rs        export pack (.srt, .vtt, .meta.json, poster)
  store.rs         project persistence
  util.rs          subprocesses, atomic writes, helpers

static/            browser studio
evals/             golden-set evaluation harness
```

The original product decisions live in the [PRD](docs/PRD.md). Current priorities and working agreements live in the [roadmap](docs/ROADMAP.md).

### Configuration

| Variable | Purpose |
|---|---|
| `CF_PORT` | Studio port; defaults to `4571` |
| `CF_DATA_DIR` | Project state directory |
| `CF_OUTPUT_DIR` | Finished clip directory |
| `CF_FFMPEG`, `CF_FFPROBE` | Media binary overrides |
| `CF_WHISPER_BIN`, `CF_WHISPER_MODEL` | Transcription overrides |
| `CF_FONTS_DIR`, `CF_FACE_MODEL` | Bundled asset overrides |
| `CF_THREADS` | Transcription thread count |
| `CF_CAPTION_STYLE` | Default style: `impact`, `clean`, `pop`, or `cinema` |
| `CF_NO_OPEN=1` | Do not open the browser on startup |

The studio has no authentication because it is designed for localhost. The server always binds to `127.0.0.1`; there is no supported setting to expose it on other interfaces.

For build, run, keep-alive (launchd), and troubleshooting details, see the [runbook](docs/RUNBOOK.md).

## Testing

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
bash -n evals/run.sh evals/verify_clip_quality.sh evals/bless_baseline.sh
python3 -m unittest discover -s evals/tests
```

Unit tests cover validation rules, selector parsing, caption timing and pagination, shot framing and speaker turns, rendering filters, restyling, persistence, and recovery.

`bash evals/verify_clip_quality.sh --source <episode.mp4>` renders a real episode in an isolated studio and checks every clip: output size, encoder, cut edges, burned captions, and whether each crop view actually shows a centered face at head-and-shoulders size.

Selection and rendering quality also need real media. The [evaluation harness](evals/README.md) defines the golden-set workflow used to catch regressions that unit tests cannot see.

## Contributing

Small, focused changes are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

For security reports, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## Credits and license

Clipping Factory uses [FFmpeg](https://ffmpeg.org), [whisper.cpp](https://github.com/ggml-org/whisper.cpp), [rustface](https://github.com/atomashpolskiy/rustface), and the [Inter](https://rsms.me/inter/) typeface.

The source code is available under the [MIT License](LICENSE). Bundled fonts and models are covered by the notices in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); tools and user-provided media remain subject to their own licenses.

Only process media you own or have permission to use.
