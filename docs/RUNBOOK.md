# Clipping Factory Runbook

Build it, run it, configure it, keep it alive, and fix it when it breaks. This is the
operator's guide for running the studio as a local service on macOS. For what the app
does and how to use it, start with the [README](../README.md).

## Prerequisites

The first supported setup is macOS on Apple Silicon.

| Tool | Why | Install |
|---|---|---|
| Rust toolchain | Builds the binary | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` |
| `ffmpeg-full` | Media inspection and rendering (with libass for captions) | `brew install ffmpeg-full` |
| `whisper-cpp` | Local transcription with word timestamps | `brew install whisper-cpp` |
| A ggml model | Speech-to-text weights, downloaded once | See below |

Two accuracy notes before you install:

- **Use `ffmpeg-full`, not plain `ffmpeg`.** Homebrew's regular FFmpeg build omits
  libass, which the caption renderer needs. The app prefers
  `/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg` when it exists and checks for ASS
  caption support at startup.
- **The transcription model is a separate download.** `brew install whisper-cpp`
  gives you the `whisper-cli` binary but no weights.

Download the base English model once:

```bash
mkdir -p ~/.clipping-factory/models
curl -L -o ~/.clipping-factory/models/ggml-base.en.bin \
  "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
```

`ggml-base.en` is the fast default. For tougher audio, put `ggml-small.en.bin` in
`~/.clipping-factory/models/` or point `CF_WHISPER_MODEL` at another compatible
ggml model.

## Build and run

From the repository root:

```bash
cargo run --release
```

Or build once and run the binary directly (useful for launchd, see below):

```bash
cargo build --release
./target/release/clipping-factory
```

The studio starts at [http://localhost:4571](http://localhost:4571) and opens your
browser automatically (set `CF_NO_OPEN=1` to suppress that). Drop in one MP4 and the
pipeline runs: inspect, extract audio, transcribe, find moments, validate, frame,
render.

On startup the server prints a first-run report that verifies every dependency:

```text
  Clipping Factory — first-run checks
  ├─ ffmpeg        ok
  ├─ ASS captions  ok
  ├─ ffprobe       ok
  ├─ whisper-cli   MISSING (brew install whisper-cpp, or set CF_WHISPER_BIN)
  ├─ whisper model MISSING — download ggml-base.en.bin (~148 MB) to ~/.clipping-factory/models/
  ├─ face model    missing (optional — clips fall back to blur-pad layout)
  ├─ caption font  ...
  ├─ disk free     ...
  ├─ projects dir  /Users/you/.clipping-factory
  └─ output dir    /Users/you/Downloads/Clipping Factory
```

Anything showing `MISSING` will fail later in the pipeline, so fix it before uploading.

Incremental rebuilds are fast (seconds when only a few files changed):

```bash
cargo build --release
```

## Configuration reference

All configuration is via environment variables. There is no config file.

| Variable | Default | Purpose |
|---|---|---|
| `CF_PORT` | `4571` | HTTP port for the studio |
| `CF_DATA_DIR` | `~/.clipping-factory` | Project state, models, settings, logs root |
| `CF_OUTPUT_DIR` | `~/Downloads/Clipping Factory` | Where finished clips are written |
| `CF_FFMPEG` | `ffmpeg-full` if present, else `ffmpeg` on PATH | FFmpeg binary override |
| `CF_FFPROBE` | `ffprobe` on PATH | FFprobe binary override |
| `CF_WHISPER_BIN` | `whisper-cli`/`whisper-cpp` on PATH, then common build locations | Transcription binary override |
| `CF_WHISPER_MODEL` | `ggml-base.en.bin` in the data dir's `models/` | ggml model path |
| `CF_FONTS_DIR` | bundled `assets/fonts` | Directory containing caption fonts |
| `CF_FACE_MODEL` | bundled model | rustface seeta model path (optional) |
| `CF_THREADS` | physical cores | Transcription thread count |
| `CF_CAPTION_STYLE` | `impact` | Default style: `impact` or `clean` |
| `CF_NO_OPEN=1` | unset (browser opens) | Do not open the browser on startup |

Example:

```bash
CF_PORT=4572 CF_CAPTION_STYLE=clean CF_NO_OPEN=1 ./target/release/clipping-factory
```

The whisper binary is located in this order: `CF_WHISPER_BIN`, then `whisper-cli`
or `whisper-cpp` on PATH, then common local build locations. The model is located
in this order: `CF_WHISPER_MODEL`, then the data dir's `models/` folder, then
common local locations.

Security note: the studio has **no authentication** because it is designed for
localhost. API keys for the optional AI providers are stored in
`~/.clipping-factory/settings.json` with user-only `0600` permissions and are never
logged. The server is loopback-only and always binds `127.0.0.1`; there is no
supported setting to listen on other interfaces. Do not expose the studio through
a reverse proxy or port forward without adding authentication and a deliberate
network security model.

## Keeping it alive (launchd)

To run the studio as an always-on background service, use a LaunchAgent with
`KeepAlive` so it restarts after crashes and after reboots. The plist runs the
binary through a `bash -lc` wrapper so launchd gets a sane PATH (including
Homebrew's `/opt/homebrew/bin`).

Create `~/Library/LaunchAgents/com.clipping-factory.server.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.clipping-factory.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>/bin/bash</string>
        <string>-lc</string>
        <string>export PATH="/opt/homebrew/bin:$PATH"; exec /path/to/clipping-factory/target/release/clipping-factory</string>
    </array>
    <key>WorkingDirectory</key>
    <string>/path/to/clipping-factory</string>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/you/.clipping-factory/logs/server.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/you/.clipping-factory/logs/server.log</string>
</dict>
</plist>
```

Replace `/path/to/clipping-factory` and `/Users/you` with real paths. The binary
path must exist; a plist pointing at a missing binary makes launchd churn a failed
spawn every 10 seconds, so verify the path before loading.

Load the agent and confirm it is running:

```bash
mkdir -p ~/.clipping-factory/logs
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.clipping-factory.server.plist
launchctl list | grep clipping-factory
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:4571/   # expect 200
```

Restart the service after a rebuild (this is the normal deploy step):

```bash
launchctl kickstart -k gui/$(id -u)/com.clipping-factory.server
```

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `address already in use` on startup | Something else owns the port. Find it with `lsof -nP -iTCP:4571 -sTCP:LISTEN`, or run on another port with `CF_PORT=4572`. |
| First-run report shows `whisper model MISSING` | No model weights installed. Download `ggml-base.en.bin` to `~/.clipping-factory/models/` (or set `CF_WHISPER_MODEL`). |
| First-run report shows `whisper-cli MISSING` | `brew install whisper-cpp`, or set `CF_WHISPER_BIN` to your `whisper-cli` build. |
| First-run report shows `ASS captions MISSING` | Plain `ffmpeg` lacks libass. `brew install ffmpeg-full`, or point `CF_FFMPEG` at a build with libass. |
| Rendering fails mid-project | Check the log first; the most common cause is a full disk. Uploads reserve 1 GiB based on free space at upload start. Free space and retry. |
| A project shows failed with "Retry to resume" | An interruption (crash, kill, power loss) stopped a render. Retry from the studio UI re-renders from the last completed stage; finished clips are kept. Project state lives under `~/.clipping-factory/projects/<project-id>/`. |
| Studio unreachable after a rebuild | The running process is the old binary. `launchctl kickstart -k gui/$(id -u)/com.clipping-factory.server` to pick up the new build. |
| Where are the logs? | Under the launchd example above: `~/.clipping-factory/logs/server.log`. Run manually in a terminal and logs go to that terminal's stderr instead. |

## Related

- [README](../README.md) — product overview, quickstart, testing
- [PRD](PRD.md) — original product decisions
- [ROADMAP](ROADMAP.md) — current priorities and working agreements
