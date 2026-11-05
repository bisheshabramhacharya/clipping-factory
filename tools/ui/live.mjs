// The live tour: recorded against a studio that is actually running, with the
// clips it actually rendered.
//
//   npm run live -- --project 945e180cd2
//   npm run live -- --project <id> --base http://127.0.0.1:4571 --width 1440
//
// This is the end-to-end check the fixture tour cannot be: the page is the
// Rust server, the clips are real MP4s served from it, the transcript is the
// one whisper produced, and every click changes real project state. Nothing is
// pinned except the viewport.
//
// Output: tour/live.mp4, tour/live/*.png, tour/live.md.

import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser } from "./rig.mjs";
import {
  determinismScript, overlayScript, caption, moveTo, clickAt, pressKey, transcode,
} from "./tourkit.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(ROOT, "..", "..");
const OUT = path.join(ROOT, "tour");
const VIDEO_DIR = path.join(OUT, "video-live");
const STILLS = path.join(OUT, "live");

function parseArgs(argv) {
  const args = { base: "http://127.0.0.1:4571", project: null, width: 1440, height: 900 };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--base") args.base = argv[++i].replace(/\/$/, "");
    else if (arg === "--project") args.project = argv[++i];
    else if (arg === "--width") args.width = Number(argv[++i]);
    else if (arg === "--height") args.height = Number(argv[++i]);
  }
  if (!args.project) throw new Error("--project is required (the studio project id)");
  if (!args.height) args.height = Math.round(((args.width * 9) / 16) / 2) * 2;
  return args;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  await rm(VIDEO_DIR, { recursive: true, force: true });
  await rm(STILLS, { recursive: true, force: true });
  await mkdir(VIDEO_DIR, { recursive: true });
  await mkdir(STILLS, { recursive: true });

  const browser = await launchBrowser();
  const context = await browser.newContext({
    viewport: { width: args.width, height: args.height },
    deviceScaleFactor: 1,
    colorScheme: "light",
    recordVideo: { dir: VIDEO_DIR, size: { width: args.width, height: args.height } },
  });
  await context.addInitScript(determinismScript, { fixedNow: Date.now(), seed: 0x5eed, projectId: args.project });
  await context.addInitScript(overlayScript);
  const page = await context.newPage();

  const steps = [];
  const still = async (index, name) => {
    const file = path.join(STILLS, `${String(index).padStart(2, "0")}-${name}.png`);
    await page.screenshot({ path: file, animations: "disabled" });
    return path.relative(REPO, file);
  };

  try {
    await page.goto(args.base, { waitUntil: "domcontentloaded" });
    await page.waitForSelector("#results-state:not(.hidden) .rail-item", { timeout: 30_000 });
    await page.waitForTimeout(1500);

    const total = await page.locator(".rail-item").count();
    await caption(page, {
      index: 1,
      total: 5,
      title: `${total} clips from the real episode`,
      detail: "Rendered by the real pipeline, playing from the real server.",
    });
    await page.waitForTimeout(1200);
    await moveTo(page, ".rail-item");
    steps.push({ index: 1, title: "Results", still: await still(1, "results") });

    // Let the selected clip actually play for a moment before moving on.
    await page.evaluate(() => {
      const video = document.querySelector(".clip.is-active .preview video");
      if (video) {
        video.muted = true;
        video.currentTime = 2;
        video.play().catch(() => {});
      }
    });
    await page.waitForTimeout(2600);

    await caption(page, {
      index: 2,
      total: 5,
      title: "The clip plays in place",
      detail: "9:16, captions burned in, the same file the download serves.",
    });
    await clickAt(page, ".rail-item:nth-child(3)");
    await page.waitForTimeout(1200);
    await page.evaluate(() => {
      const video = document.querySelector(".clip.is-active .preview video");
      if (video) {
        video.muted = true;
        video.play().catch(() => {});
      }
    });
    await page.waitForTimeout(2600);
    steps.push({ index: 2, title: "A second clip plays", still: await still(2, "second-clip") });

    await caption(page, {
      index: 3,
      total: 5,
      title: "Restyling a clip re-burns from the cached base render",
      detail: "Seconds, not a whole render.",
    });
    await clickAt(page, '.clip.is-active .seg-btn:has-text("Clean")');
    await clickAt(page, ".clip.is-active .swatch >> nth=2");
    await page.waitForTimeout(1600);
    steps.push({ index: 3, title: "Restyle", still: await still(3, "restyle") });

    await caption(page, {
      index: 4,
      total: 5,
      title: "Keyboard triage over the rendered clips",
      detail: "1 keep, 2 maybe, 3 skip.",
    });
    await clickAt(page, "#review-clips-btn");
    await page.waitForSelector("#review-theater:not([hidden])");
    await page.waitForTimeout(1200);
    await pressKey(page, "1");
    await pressKey(page, "1");
    await pressKey(page, "2");
    await page.waitForTimeout(800);
    steps.push({ index: 4, title: "Review theater", still: await still(4, "theater") });

    for (let i = 0; i < 3; i += 1) await pressKey(page, "1");
    await page.waitForSelector("#review-done:not([hidden])");
    await caption(page, {
      index: 5,
      total: 5,
      title: "Kept, maybe, skipped",
      detail: "One download for the whole run.",
    });
    await page.waitForTimeout(1600);
    await moveTo(page, "#review-done-download");
    steps.push({ index: 5, title: "Review complete", still: await still(5, "review-done") });
  } finally {
    const video = page.video();
    await context.close();
    await browser.close();
    if (video && video.path) {
      const webm = await video.path();
      const mp4 = path.join(OUT, "live.mp4");
      transcode(webm, mp4);
      await rm(webm, { force: true });
      console.log(`video: ${path.relative(REPO, mp4)}`);
    }
  }

  const md = [
    "# Live tour",
    "",
    `Studio: ${args.base} · project \`${args.project}\` · ${args.width}x${args.height}`,
    "",
    "Recorded against the running Rust studio, not the fixture server: real clips, real transcript, real project state.",
    "",
    ...steps.map((step) => `${step.index}. ${step.title} (${step.still})`),
    "",
  ].join("\n");
  await writeFile(path.join(OUT, "live.md"), md);
  console.log(`steps: ${path.relative(REPO, path.join(OUT, "live.md"))}`);
}

await main();
