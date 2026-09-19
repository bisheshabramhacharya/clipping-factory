<div align="center">

# Clipping Factory

### One podcast in. Every strong, faithful clip out.

A local-first podcast clipping studio with full-transcript ranking, face-aware reframing, and word-accurate captions. Built entirely in Rust.

[![CI](https://github.com/bisheshabramhacharya/clipping-factory/actions/workflows/ci.yml/badge.svg)](https://github.com/bisheshabramhacharya/clipping-factory/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![Local first](https://img.shields.io/badge/processing-local--first-1f6feb)
![Output](https://img.shields.io/badge/output-1080%C3%971920-7c3aed)
![Tests](https://img.shields.io/badge/tests-256%20passing-238636)

</div>

![Clipping Factory results showing six rendered vertical clips](docs/assets/studio-results.png)

Clipping Factory turns a podcast MP4 into a set of strong, distinct vertical clips. Every result is one continuous excerpt with word-timed captions and clean, face-aware framing.

The goal is simple: find moments worth posting without rewriting the speaker, inventing context, or hiding weak edits behind effects.

No account. No cloud upload. No required AI model. The built-in ranker scans the full transcript, and a deterministic validator rejects clips that depend on missing context or overlap stronger moments.

## What you get

| | |
|---|---|
| **More useful candidates** | Keeps every strong, distinct moment instead of stopping at an arbitrary quota. |
| **Faithful excerpts** | Never rewrites, reorders, splices, or invents speech. |
| **Feed-ready video** | Produces H.264/AAC MP4s at 1080×1920. |
| **Word-accurate captions** | Impact, Clean, Pop, and Cinema styles with per-clip restyling in seconds, plus optional emoji accents. |
| **Clip controls** | Opt-in per clip: auto-cut silence/filler, zoom cuts on emphasis beats, hook title, progress bar, end card. |
| **Export pack** | Every clip ships with .srt, .vtt, .meta.json (title/description/hashtags), and a poster still. |
| **Honest ranking** | Composite score and selection reason on every clip, plus what was rejected and why. |
| **~99 languages** | Whisper transcription auto-detects the language, or you pick it per project. |
| **Private by default** | Keeps video, audio, transcripts, project state, and rendering on your machine. |

<p align="center">
  <img src="docs/assets/clip-details.png" alt="Rendered clip cards with captions and face tracking" width="620">
</p>

```text
Drop MP4 → Inspect → Extract audio → Transcribe → Find moments
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

The studio opens at [http://localhost:4571](http://localhost:4571). Drop in one MP4 and the pipeline starts — or hit **Try the sample episode** to run the bundled 150 s sample end-to-end without finding a file first. The optional **Focus** field steers selection toward a topic ("clips about pricing", "where they argue"): it reaches the configured provider as an editorial directive, and under local ranking it falls back to keyword matching. Left blank, selection stays the generic best-moments ranking.

### Linux

Install FFmpeg through your package manager and build [whisper.cpp](https://github.com/ggml-org/whisper.cpp):

```bash
cmake -B build
cmake --build build -j --target whisper-cli
```

Set `CF_WHISPER_BIN` to the resulting `whisper-cli` path if it is not already on your `PATH`.

### Better transcription

`ggml-base` is the fast default and covers ~99 languages. For tougher audio, put `ggml-small.bin` in `~/.clipping-factory/models/` or set `CF_WHISPER_MODEL` to another compatible ggml model. English-only weights (`ggml-*.en.bin`) are also supported and slightly stronger on English — but then only English (and no auto-detection) is available.

Language is chosen per project at upload: **Auto-detect** is the default, or pick a specific language to skip detection. Non-English sources need a multilingual model.

## Local by default. AI optional.

Local ranking is the default and needs no API key. If you want model-assisted selection, open the provider control in the studio and connect OpenAI or Anthropic.

| Provider | Default model | Notes |
|---|---|---|
| Local ranking | — | Scans the full transcript locally. No key required. |
| Local endpoint | — | Ollama, llama.cpp, or LM Studio. Model-assisted, fully offline. |
| OpenAI | `gpt-4o-mini` | Accepts another chat-completions model name. |
| Anthropic | `claude-sonnet-4-5` | Optional alternative provider. |

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

Finding a possible moment is not enough. Every proposed clip must pass the same deterministic validator before rendering:

- The excerpt must meet minimum scores for self-containment, payoff, and clarity.
- Context dependency and slop risk must stay below fixed limits.
- The opening and closing quotes must appear in the transcript near the proposed boundaries.
- Boundaries snap to real word timestamps instead of trusting model-generated milliseconds.
- A clip cannot overlap more than 30% with a higher-ranked result.
- Timestamps must stay inside the source duration.
- A clip may not span a detected scene transition; cuts near one snap to word boundaries.
- A clip may not open on a greeting or filler word, or close on outro/CTA bait ("like and subscribe").
- Normal duration is 20–90 seconds, with a narrow exception for unusually strong moments; clips in the 25–60s short-form sweet spot rank ahead of equal-scored longer ones.

Zero clips is a valid result. The studio shows what it considered, what it rejected, and which rule rejected it.

## Captions you choose after rendering

Clips render with the default style first. Each finished clip can then be restyled without repeating the expensive framing pass.

- **Impact** uses tight, kinetic stacks with one dominant word and a restrained active-word accent.
- **Clean** uses compact conversational groups in the lower safe area with a softer active-word accent.
- **Pop** pops each active word up and holds a keyword accent for the whole phrase.
- **Cinema** sets lowercase letterspaced lines that fade in like subtitle cards.

You can switch styles, choose an accent color, tune the caption text, and apply the change from the result card. Clipping Factory re-burns captions from the cached base render in seconds.

## Per-clip garnishes

Every rendered clip carries opt-in toggles — each re-renders just that clip:

- **Auto-cut** removes silence gaps and filler words ("um", "uh") via a keep-list concat; captions stay in sync.
- **Zoom cuts** add `zoompan` punch-ins on energy and emphasis beats, inside the locked crop.
- **Hook title** burns the clip's headline as an ALL-CAPS card over the first ~1.8 s.
- **Progress bar** draws a thin accent-colored fill along the bottom edge.
- **End card** appends a 1.2 s "Made with Clipping Factory" tail.

## Speaker-aware framing

When a speaker-embedding model is configured (`CF_SPEAKER_MODEL`), two-person sources get extra layouts: **Split** stacks both faces when two people share a wide shot, and **SpeakerCrop** hard-cuts the locked crop between faces at speaker-turn boundaries. Captions get `S1:`/`S2:` tags and the .srt sidecar is speaker-labeled. Without the model nothing changes — the layouts stay single-face.

## House rendering rules

- H.264/AAC output at the native crop size, capped at 1080×1920 — never upscaled.
- Source frame rate is preserved, with a 30 fps fallback.
- One persistent face gets a locked 9:16 crop that never pans or eases.
- Multiple faces or no reliable face gets a centered source over a darkened blur background.
- Captions use short conversational groups in the lower safe area.
- Audio is loudness-normalized to −16 LUFS with short edge fades on every clip.
- Defaults stay clean: no B-roll, music, or transitions — motion options (zoom cuts, emoji) are opt-in per clip.

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
| Transcription | whisper.cpp with word timestamps |
| Editorial selection | Local ranker, optional local endpoint (Ollama/llama.cpp/LM Studio), optional OpenAI, optional Anthropic |
| Quality gate | Pure Rust deterministic validator |
| Framing | rustface detections with smoothing and crop clamping |
| Captions | Generated ASS subtitles burned by libass |
| State | Atomic filesystem JSON writes |

```text
src/
  main.rs          startup and first-run checks
  api.rs           HTTP API and static studio
  pipeline.rs      stage orchestration
  media.rs         probing and audio extraction
  transcribe.rs    whisper.cpp integration
  select/          local and optional model-assisted selection
  validate.rs      deterministic quality gate
  frame.rs         face detection and layout decisions
  captions.rs      caption grouping and ASS generation
  render.rs        FFmpeg filter graphs
  store.rs         project persistence

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
| `CF_CAPTION_STYLE` | Default style: `impact` or `clean` |
| `CF_NO_OPEN=1` | Do not open the browser on startup |

The studio has no authentication because it is designed for localhost. The server always binds to `127.0.0.1`; there is no supported setting to expose it on other interfaces.

For build, run, keep-alive (launchd), and troubleshooting details, see the [runbook](docs/RUNBOOK.md).

## Testing

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --locked   # 123 tests: every validator rule, caption pagination, crop math,
                       # face-track smoothing & pan clamping, layout decisions, restyle
                       # plumbing, selector parsing, state recovery
```

Unit tests cover validation rules, selector parsing, caption timing and pagination, framing decisions, crop smoothing, rendering filters, restyling, persistence, and recovery.

Selection and rendering quality also need real media. The [evaluation harness](evals/README.md) defines the golden-set workflow used to catch regressions that unit tests cannot see.

## Contributing

Small, focused changes are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

For security reports, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## Credits and license

Clipping Factory uses [FFmpeg](https://ffmpeg.org), [whisper.cpp](https://github.com/ggml-org/whisper.cpp), [rustface](https://github.com/atomashpolskiy/rustface), and the [Inter](https://rsms.me/inter/) typeface.

The source code is available under the [MIT License](LICENSE). Bundled fonts and models are covered by the notices in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); tools and user-provided media remain subject to their own licenses.

Only process media you own or have permission to use.
