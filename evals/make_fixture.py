#!/usr/bin/env python3
"""Generate a deterministic synthetic "episode" for the eval harness.

The golden set wants real media, but that asset lives on the maintainer's
machine. This fixture lets every gate in evals/ run anywhere ffmpeg exists:
distinct visual scenes (so scdet has boundaries to find) plus intelligible
speech (so whisper has words to time). With no TTS in ffmpeg the script
degrades to tone audio — the pipeline still completes, producing an honest
zero-clip run.

Output: an mp4 plus a <name>.truth.json sidecar recording the known scene
boundaries, so checkers can compare detector output against ground truth.

Usage: python3 evals/make_fixture.py [--out PATH] [--seconds N]
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys

# Podcast-shaped sentences; hook words up front so the heuristic selector has
# something to find in every scene.
SCRIPT = [
    "The one thing nobody tells you about pricing is that it is never about the number.",
    "We spent three months building the wrong feature and it nearly killed the company.",
    "Here is the secret that took me ten years to learn about growing an audience.",
    "Everyone said we would fail, and for a while they were completely right about it.",
    "The biggest mistake I ever made was hiring for skill instead of for curiosity.",
    "This is the part where the story gets weird, because the money ran out on a Tuesday.",
    "If you only remember one thing from this, remember that consistency beats talent.",
    "Nobody talks about the boring middle, and the boring middle is where everything happens.",
]

# Structurally different test patterns per scene — a hard cut between them
# scores well above the scdet threshold (a hue-rotated identical pattern
# does not, because luma barely changes).
SCENE_SOURCES = ["testsrc2", "smptehdbars", "rgbtestsrc", "gradients",
                 "yuvtestsrc", "pal75bars", "testsrc", "pal100bars"]


def have_flite(ffmpeg: str) -> bool:
    p = subprocess.run([ffmpeg, "-hide_banner", "-filters"],
                       capture_output=True, text=True)
    return any(l.split()[1] == "flite" for l in p.stdout.splitlines() if " flite " in l)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=None,
                    help="output mp4 (default evals/fixtures/synthetic-episode.mp4)")
    ap.add_argument("--seconds", type=int, default=150)
    ap.add_argument("--scene-seconds", type=int, default=30)
    ap.add_argument("--fps", type=int, default=30)
    args = ap.parse_args()
    if args.seconds <= 0 or args.scene_seconds <= 0:
        ap.error("--seconds and --scene-seconds must be positive")

    out = args.out
    if out is None:
        import os
        here = os.path.dirname(os.path.abspath(__file__))
        out = os.path.join(here, "fixtures", "synthetic-episode.mp4")
    import os
    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        print("error: ffmpeg not found on PATH (needed to render the fixture)",
              file=sys.stderr)
        return 1
    speech = have_flite(ffmpeg)
    if not speech:
        print("note: ffmpeg lacks the flite filter; generating tone audio "
              "(run completes, zero clips is the honest result)", file=sys.stderr)

    n_scenes = max(1, -(-args.seconds // args.scene_seconds))
    parts = []
    boundaries_ms = []
    for i in range(n_scenes):
        dur = min(args.scene_seconds, args.seconds - i * args.scene_seconds)
        pattern = SCENE_SOURCES[i % len(SCENE_SOURCES)]
        label = f"Scene {i + 1}"
        video = (f"{pattern}=size=1280x720:rate={args.fps}:duration={dur},"
                 f"drawtext=text='{label}':fontsize=72:fontcolor=white:"
                 f"x=(w-text_w)/2:y=(h-text_h)/2:box=1:boxcolor=black@0.5")
        if speech:
            text = SCRIPT[i % len(SCRIPT)]
            audio = f"flite=text='{text}':voice=slt,aresample=16000,apad=whole_dur={dur},atrim=0:{dur}"
        else:
            audio = f"sine=frequency={220 + i * 110}:duration={dur},volume=0.3"
        parts.append((video, audio))
        if i > 0:
            boundaries_ms.append(i * args.scene_seconds * 1000)

    inputs = []
    for v, a in parts:
        inputs += ["-f", "lavfi", "-i", v, "-f", "lavfi", "-i", a]
    filter_parts = "".join(f"[{2*i}:v][{2*i+1}:a]" for i in range(n_scenes))
    fc = f"{filter_parts}concat=n={n_scenes}:v=1:a=1[v][a]"
    cmd = ([ffmpeg, "-y", "-hide_banner", *inputs,
            "-filter_complex", fc, "-map", "[v]", "-map", "[a]",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac",
            "-movflags", "+faststart", out])
    p = subprocess.run(cmd, capture_output=True, text=True)
    if p.returncode != 0:
        print(p.stderr[-3000:], file=sys.stderr)
        return 1

    truth = {
        "schema_version": 1,
        "generator": "evals/make_fixture.py",
        "speech": speech,
        "scenes": n_scenes,
        "scene_seconds": args.scene_seconds,
        "duration_ms": n_scenes * args.scene_seconds * 1000,
        "scene_boundaries_ms": boundaries_ms,
        "note": ("Synthetic fixture; not a substitute for the canonical "
                 "real-episode asset (issue #48 handoff)."),
    }
    sidecar = out.rsplit(".", 1)[0] + ".truth.json"
    with open(sidecar, "w", encoding="utf-8") as f:
        json.dump(truth, f, indent=2, sort_keys=True)
        f.write("\n")
    print(f"wrote {out} ({n_scenes} scenes, {'speech' if speech else 'tone'} audio)")
    print(f"wrote {sidecar}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
