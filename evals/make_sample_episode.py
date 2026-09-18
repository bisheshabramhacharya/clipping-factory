#!/usr/bin/env python3
"""Regenerate assets/sample-episode.mp4 — the bundled first-run sample.

Unlike evals/make_fixture.py (one sentence per scene, silence-heavy — built
to exercise detectors, not to demo), the sample episode is shaped like a
real podcast excerpt: longer scenes and several back-to-back hook-y
sentences per scene, so the selector can find 20-40s clips that sit
entirely inside a scene and produce a non-empty, watchable result.

Output: the mp4 plus assets/sample-episode.truth.json recording the scene
boundaries, for the eval harness to compare against.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys

# Hook-first dialogue, grouped per scene: each scene reads as one thought,
# so a clip cut wholly inside it still sounds like a complete idea.
SCENES = [
    {
        "pattern": "testsrc2",
        "label": "Scene 1",
        "lines": [
            "The one thing nobody tells you about pricing is that it is never about the number.",
            "We charged five dollars for two years, then doubled it overnight and lost nobody.",
            "That is when I learned the price is just a story you tell about confidence.",
            "Cheap is not a strategy, it is a confession that you do not believe in the product.",
            "So here is the rule: price it so you are slightly embarrassed to say it out loud.",
            "Because the number you are scared of is usually the number the work is actually worth.",
            "And every founder who undercharged regrets it louder than anyone who overcharged.",
        ],
    },
    {
        "pattern": "smptehdbars",
        "label": "Scene 2",
        "lines": [
            "Here is the secret that took me ten years to learn about growing an audience.",
            "Everyone says post every day, but consistency without a point of view is just noise.",
            "The boring middle is where everything happens, and nobody talks about it.",
            "My first hundred videos got twelve views each, and that was the entire point.",
            "You are not building an audience at the start, you are building a habit.",
            "The audience shows up later for whoever kept going through the empty room.",
            "That is the whole secret. There is no hack hiding underneath it.",
        ],
    },
    {
        "pattern": "gradients",
        "label": "Scene 3",
        "lines": [
            "The biggest mistake I ever made was hiring for skill instead of for curiosity.",
            "We spent three months building the wrong feature and it nearly killed the company.",
            "If you only remember one thing, remember that the story gets weird when money runs out.",
            "A curious person learns the job in a month. A skilled one defends it for a year.",
            "So now I hire for the questions people ask in the interview, not the answers.",
            "That one change rebuilt the team faster than any rewrite ever could.",
            "And it is the single best decision hiding inside that whole disaster.",
        ],
    },
]

SCENE_SECONDS = 50
FPS = 30
SIZE = "1280x720"


def run(ffmpeg: str, args: list[str]) -> None:
    p = subprocess.run([ffmpeg, "-hide_banner", *args],
                       capture_output=True, text=True)
    if p.returncode != 0:
        sys.stderr.write(p.stderr[-4000:])
        sys.exit(1)


def have_flite(ffmpeg: str) -> bool:
    p = subprocess.run([ffmpeg, "-hide_banner", "-filters"],
                       capture_output=True, text=True)
    return any(l.split()[1] == "flite" for l in p.stdout.splitlines() if " flite " in l)


def main() -> int:
    ap = argparse.ArgumentParser()
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--out",
                    default=os.path.join(here, "..", "assets", "sample-episode.mp4"))
    args = ap.parse_args()
    out = os.path.abspath(args.out)

    ffmpeg = "ffmpeg"
    if not have_flite(ffmpeg):
        print("error: ffmpeg lacks the flite filter; cannot regenerate the sample",
              file=sys.stderr)
        return 1

    dur = SCENE_SECONDS
    inputs: list[str] = []
    n_inputs = 0

    def add_input(spec: str) -> int:
        nonlocal n_inputs
        inputs.extend(["-f", "lavfi", "-i", spec])
        idx = n_inputs
        n_inputs += 1
        return idx

    conv = []
    # Per scene: one video input + one audio input per line; each line's
    # speech runs at its natural length, then the scene pads to SCENE_SECONDS
    # of room tone (apad) — the silence tail is intentional dead air.
    for si, scene in enumerate(SCENES):
        v_idx = add_input(
            f"{scene['pattern']}=size={SIZE}:rate={FPS}:duration={dur},"
            f"drawtext=text='{scene['label']}':fontsize=72:fontcolor=white:"
            f"x=(w-text_w)/2:y=(h-text_h)/2:box=1:boxcolor=black@0.5")
        line_labels = []
        for li, line in enumerate(scene["lines"]):
            safe = line.replace("\\", "\\\\").replace(":", "\\:").replace("'", "\\'")
            a_idx = add_input(f"flite=text='{safe}':voice=slt,aresample=16000")
            line_labels.append(f"[{a_idx}:a]")
        joined = f"sc{si}join"
        conv.append("".join(line_labels) +
                    f"concat=n={len(line_labels)}:v=0:a=1,"
                    f"apad=whole_dur={dur},atrim=0:{dur},asetpts=PTS-STARTPTS[{joined}]")
        conv.append(f"[{v_idx}:v][{joined}]concat=n=1:v=1:a=1[sv{si}][sa{si}]")

    scene_labels = "".join(f"[sv{i}][sa{i}]" for i in range(len(SCENES)))
    conv.append(f"{scene_labels}concat=n={len(SCENES)}:v=1:a=1[v][a]")
    fc = ";".join(conv)

    run(ffmpeg, ["-y", *inputs, "-filter_complex", fc,
                 "-map", "[v]", "-map", "[a]",
                 "-c:v", "libx264", "-crf", "26", "-preset", "slow",
                 "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "96k",
                 "-movflags", "+faststart", out])

    truth = {
        "schema_version": 1,
        "generator": "evals/make_sample_episode.py",
        "speech": True,
        "scenes": len(SCENES),
        "scene_seconds": SCENE_SECONDS,
        "duration_ms": len(SCENES) * SCENE_SECONDS * 1000,
        "scene_boundaries_ms": [SCENE_SECONDS * 1000 * i
                                for i in range(1, len(SCENES))],
        "note": "First-run sample: hook-dense speech, one hard cut per 50 s "
                "scene so candidate windows can sit inside a single scene.",
    }
    sidecar = out.rsplit(".", 1)[0] + ".truth.json"
    with open(sidecar, "w", encoding="utf-8") as f:
        json.dump(truth, f, indent=2, sort_keys=True)
        f.write("\n")
    print(f"wrote {out}")
    print(f"wrote {sidecar}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
