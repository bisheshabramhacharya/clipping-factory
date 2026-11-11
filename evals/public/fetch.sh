#!/usr/bin/env bash
# Fetch the public eval set into evals/sources/ and write a filled
# evals/manifest.json that evals/run.sh can consume.
#
#   bash evals/public/fetch.sh
#
# Media stays outside git (evals/sources/ is gitignored). Audio-only
# sources are wrapped in an MP4 with a black frame so the studio's
# video ingest path accepts them; the audio track is untouched.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
PUB_MANIFEST="$ROOT/manifest.json"
OUT_DIR="$ROOT/../sources"
RUN_MANIFEST="$ROOT/../manifest.json"
mkdir -p "$OUT_DIR"

for cmd in curl yt-dlp ffmpeg ffprobe jq; do
  command -v "$cmd" >/dev/null || { echo "$cmd is required" >&2; exit 1; }
done

slug() {
  # Lowercase, alnum runs -> dashes; stable file name per source id.
  python3 -c 'import re,sys; print(re.sub(r"[^a-z0-9]+","-",sys.argv[1].lower()).strip("-"))' "$1"
}

fetch_audio() {
  local url="$1" dest="$2"
  local tmp="${dest%.mp4}.tmp.mp3"
  curl -fL --retry 3 -o "$tmp" "$url"
  # Loop a black frame under the untouched audio; -shortest ends at audio end.
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=black:s=1280x720:r=30" \
    -i "$tmp" \
    -c:v libx264 -preset veryfast -crf 32 -pix_fmt yuv420p \
    -c:a aac -b:a 96k -movflags +faststart -shortest "$dest"
  rm -f "$tmp"
}

fetch_video() {
  local url="$1" dest="$2"
  local tmp="${dest%.mp4}.tmp.mkv"
  # Cap at 720p: enough for framing checks, small enough to keep local.
  yt-dlp -f "bv*[height<=720]+ba/b[height<=720]" \
    --merge-output-format mkv -o "$tmp" "$url"
  # Remux to a boring h264/aac MP4 only when the codecs already match;
  # otherwise re-encode so the studio's ffmpeg path reads it anywhere.
  local vcodec acodec
  vcodec="$(ffprobe -v error -select_streams v:0 -show_entries stream=codec_name -of csv=p=0 "$tmp")"
  acodec="$(ffprobe -v error -select_streams a:0 -show_entries stream=codec_name -of csv=p=0 "$tmp")"
  if [[ "$vcodec" == "h264" && "$acodec" == "aac" ]]; then
    ffmpeg -hide_banner -loglevel error -y -i "$tmp" -c copy -movflags +faststart "$dest"
  else
    ffmpeg -hide_banner -loglevel error -y -i "$tmp" \
      -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
      -c:a aac -b:a 128k -movflags +faststart "$dest"
  fi
  rm -f "$tmp"
}

COUNT="$(jq '.sources | length' "$PUB_MANIFEST")"
for ((i=0; i<COUNT; i++)); do
  ID="$(jq -r ".sources[$i].id" "$PUB_MANIFEST")"
  TITLE="$(jq -r ".sources[$i].title" "$PUB_MANIFEST")"
  URL="$(jq -r ".sources[$i].url" "$PUB_MANIFEST")"
  MEDIA="$(jq -r ".sources[$i].media" "$PUB_MANIFEST")"
  DEST="$OUT_DIR/$(slug "$TITLE").mp4"
  if [[ -f "$DEST" ]]; then
    echo "↷ $ID already fetched"
  else
    echo "── $ID: $TITLE"
    case "$MEDIA" in
      audio) fetch_audio "$URL" "$DEST" ;;
      video) fetch_video "$URL" "$DEST" ;;
      *) echo "unknown media type $MEDIA for $ID" >&2; exit 1 ;;
    esac
  fi
done

# Write the run.sh manifest beside evals/ with real durations probed from disk.
python3 - "$PUB_MANIFEST" "$OUT_DIR" "$RUN_MANIFEST" <<'PY'
import json, subprocess, sys

pub_path, out_dir, run_path = sys.argv[1:]
pub = json.load(open(pub_path, encoding="utf-8"))
sources = []
for item in pub["sources"]:
    name = item["title"].lower()
    name = "".join(c if c.isalnum() else "-" for c in name)
    name = "-".join(part for part in name.split("-") if part)
    path = f"sources/{name}.mp4"
    full = f"{out_dir}/{name}.mp4"
    try:
        probe = subprocess.run(
            [
                "ffprobe", "-v", "error", "-show_entries", "format=duration",
                "-of", "csv=p=0", full,
            ],
            text=True, capture_output=True, check=True,
        )
        seconds = float(probe.stdout.strip() or 0)
    except (OSError, subprocess.CalledProcessError, ValueError):
        seconds = 0
    if seconds <= 0:
        raise SystemExit(f"missing or unreadable media for {item['id']}: {full}")
    entry = {
        "id": item["id"],
        "path": path,
        "category": item["category"],
        "duration_seconds": round(seconds),
        "source": item["page"],
        "license": item["license"],
    }
    sources.append(entry)

manifest = {"schema_version": 1, "sources": sources}
with open(run_path, "w", encoding="utf-8") as fh:
    json.dump(manifest, fh, indent=2)
    fh.write("\n")
print(f"wrote {run_path} with {len(sources)} sources")
PY
