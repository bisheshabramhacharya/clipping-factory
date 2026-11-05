// Real clip stills, cut from an actual episode with ffmpeg.
//
//   node fixtures/frames-from-video.mjs --source "C:\path\to\episode.mp4"
//   node fixtures/frames-from-video.mjs            # first MP4 in evals/sources
//
// The rig never decodes video. It draws one still into the exact box the
// player occupies, so the stills have to be real frames or the screenshots
// are a drawing of the product instead of the product. Stills are derived
// from licensed media and stay out of Git (`fixtures/frames/` is ignored);
// `make-frames.mjs` builds synthetic stand-ins for machines without the
// episode, and this script overwrites them when the episode is present.
//
// Fixed timestamps, fixed crop, fixed encoder settings: two runs of this
// script on the same source produce byte-identical PNGs.

import { execFileSync } from "node:child_process";
import { mkdir, readdir, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..", "..", "..");
const OUT = path.join(HERE, "frames");

// One still per clip rank, spread across the episode so the rail reads like a
// real project instead of six frames of the same shot.
// One per clip rank, two seconds into that clip, so each still is a frame the
// clip actually contains.
const TIMES = [39, 81, 162, 2, 198, 106];

function parseArgs(argv) {
  const args = { source: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--source") args.source = argv[++i];
  }
  return args;
}

async function defaultSource() {
  for (const dir of ["evals/sources", "promo-videos", "."]) {
    const full = path.join(REPO, dir);
    const names = await readdir(full).catch(() => []);
    const mp4 = names.filter((name) => name.toLowerCase().endsWith(".mp4")).sort()[0];
    if (mp4) return path.join(full, mp4);
  }
  return null;
}

function run(bin, args) {
  try {
    return execFileSync(bin, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  } catch (error) {
    const detail = error.stderr ? String(error.stderr).trim().split("\n").slice(-3).join(" ") : error.message;
    throw new Error(`${bin} failed: ${detail}`);
  }
}

function probe(file) {
  const raw = run("ffprobe", [
    "-v", "error",
    "-select_streams", "v:0",
    "-show_entries", "stream=width,height,duration",
    "-of", "json",
    file,
  ]);
  const stream = JSON.parse(raw).streams[0];
  return { width: stream.width, height: stream.height, duration: Number(stream.duration) };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const source = path.resolve(args.source || (await defaultSource()) || "");
  if (!args.source && !(await stat(source).catch(() => null))) {
    throw new Error("no source MP4: pass --source <file>, or put one in evals/sources/");
  }
  const info = probe(source);
  const ratio = 9 / 16;
  // Centre crop to the clip's aspect, then one scale to the still size.
  const sourceRatio = info.width / info.height;
  const cropWidth = sourceRatio > ratio ? Math.round(info.height * ratio) : info.width;
  const cropHeight = sourceRatio > ratio ? info.height : Math.round(info.width / ratio);
  const x = Math.round((info.width - cropWidth) / 2);
  const y = Math.round((info.height - cropHeight) / 2);

  await mkdir(OUT, { recursive: true });
  const late = TIMES.filter((time) => time > info.duration);
  if (late.length) throw new Error(`timestamps ${late.join(", ")}s are past the ${info.duration.toFixed(1)}s source`);

  TIMES.forEach((time, index) => {
    const frame = path.join(OUT, `frame-${index + 1}.png`);
    run("ffmpeg", [
      "-y", "-v", "error",
      "-ss", String(time),
      "-i", source,
      "-frames:v", "1",
      "-vf", `crop=${cropWidth}:${cropHeight}:${x}:${y},scale=540:960:flags=lanczos`,
      frame,
    ]);
    // The rail thumbnail is 40x71 CSS px. Chrome's decode-at-scale path for a
    // scaled image inside a rounded clip is not byte-stable, so the rig serves
    // a pre-sized thumbnail: one still, two resolutions, same look.
    run("ffmpeg", [
      "-y", "-v", "error",
      "-i", frame,
      "-frames:v", "1",
      "-vf", "scale=54:96:flags=lanczos",
      path.join(OUT, `thumb-${index + 1}.png`),
    ]);
  });

  console.log(`wrote ${TIMES.length} stills from ${path.basename(source)} (${info.duration.toFixed(1)}s) to ${OUT}`);
}

await main();
