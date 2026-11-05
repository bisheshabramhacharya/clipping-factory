// Capture any build that serves the studio UI, for directions that do not share
// the current class names.
//
//   node shoot.mjs --base http://127.0.0.1:4611 --out C:/work/direction-a --width 1440,1280
//
// The strict rig waits on selectors from the current markup, so a new direction
// cannot use it before it is adopted. This waits on the page instead: fonts,
// images, network, one settle. Every <video> is replaced by a still poster drawn
// at its own box, the same rule the strict rig uses to stay byte-stable.

import { mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { launchBrowser, settle, DISABLE_MOTION_CSS } from "./rig.mjs";
import { STATE_NAMES } from "./rig.mjs";

function parseArgs(argv) {
  const args = { base: null, out: null, states: null, widths: [1440, 1280], platform: process.platform === "win32" ? "windows" : "macos" };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--base") args.base = argv[++i];
    else if (arg === "--out") args.out = argv[++i];
    else if (arg === "--states") args.states = argv[++i].split(",").map((value) => value.trim()).filter(Boolean);
    else if (arg === "--width") args.widths = argv[++i].split(",").map(Number).filter(Boolean);
    else if (arg === "--platform") args.platform = argv[++i];
  }
  if (!args.base || !args.out) throw new Error("--base and --out are required");
  return args;
}

const pinVideos = async (page) => {
  await page.evaluate(async () => {
    const videos = [...document.querySelectorAll("video")];
    await Promise.all(videos.map(async (video) => {
      const rect = video.getBoundingClientRect();
      const width = Math.max(1, Math.round(rect.width));
      const height = Math.max(1, Math.round(rect.height));
      const match = /(\d+)/.exec(video.getAttribute("aria-label") || "");
      const url = `/fixtures/frames/frame-${match ? match[1] : 1}.png`;
      const frame = new Image();
      frame.src = url;
      await frame.decode().catch(() => {});
      const canvas = document.createElement("canvas");
      canvas.className = "rig-poster";
      canvas.width = width;
      canvas.height = height;
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      canvas.getContext("2d").drawImage(frame, 0, 0, width, height);
      video.style.display = "none";
      video.after(canvas);
    }));
  });
};

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const states = args.states || STATE_NAMES;
  await mkdir(args.out, { recursive: true });
  const existing = new Set(await import("node:fs/promises").then((fs) => fs.readdir(args.out).catch(() => [])));
  for (const name of existing) if (name.endsWith(`.${args.platform}.png`)) await rm(path.join(args.out, name), { force: true });

  const browser = await launchBrowser();
  try {
    for (const width of args.widths) {
      for (const state of states) {
        const context = await browser.newContext({
          viewport: { width, height: width === 1440 ? 900 : 800 },
          deviceScaleFactor: 1,
          locale: "en-US",
          timezoneId: "UTC",
          colorScheme: "light",
          reducedMotion: "reduce",
          extraHTTPHeaders: { "X-UI-State": state },
        });
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (error) => errors.push(error.message));
        try {
          await page.goto(`${args.base}/`, { waitUntil: "networkidle" });
          await page.addStyleTag({ content: DISABLE_MOTION_CSS });
          await page.waitForTimeout(600);
          await pinVideos(page);
          await settle(page);
          const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
          await page.screenshot({ path: path.join(args.out, `${state}@${width}.${args.platform}.png`), animations: "disabled", caret: "hide", scale: "css" });
          console.log(`${state}@${width} overflow=${overflow}${errors.length ? ` errors=${errors.length}` : ""}`);
          for (const message of errors.slice(0, 3)) console.log(`  pageerror: ${message}`);
        } finally {
          await context.close();
        }
      }
    }
  } finally {
    await browser.close();
  }
}

await main();
