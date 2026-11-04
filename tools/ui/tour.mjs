// A guided tour of the whole studio, recorded, so a person can watch the
// product work instead of reading a diff.
//
//   npm run tour
//   npm run tour -- --width 1440 --platform windows
//
// The tour drives the real static/ bundle in the installed Chrome against
// fixture API data, same as the shot rig. It walks every screen in product
// order, names each step in an overlay, moves a drawn cursor to the control it
// is about to use, and actually performs the interaction: clicks a rail row,
// flips a switch, presses the review keys. Every step also lands a still in
// tour/stills/.
//
// Output: tour/tour.mp4 (h264, yuv420p), tour/stills/*.png, tour/steps.md.

import { execFileSync } from "node:child_process";
import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createFixtureServer } from "./server.mjs";
import { launchBrowser, pinPosters, settle } from "./rig.mjs";
import { FIXED_NOW, PROJECT_ID, SOURCE_NAME } from "./fixtures/state.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(ROOT, "..", "..");
const OUT = path.join(ROOT, "tour");
const VIDEO_DIR = path.join(OUT, "video");
const STILLS = path.join(OUT, "stills");

// States that need a project on screen. Mirrors PROJECT_STATES in rig.mjs.
const PROJECT_STATES = new Set([
  "processing-mid-run",
  "processing-error",
  "results-6-clips",
  "results-rejected-expanded",
  "library-open",
  "clip-detail-restyle",
  "review-theater",
  "review-done",
]);

function parseArgs(argv) {
  const args = { width: 1440, height: 900, platform: "windows" };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--width") args.width = Number(argv[++i]);
    else if (arg === "--height") args.height = Number(argv[++i]);
    else if (arg === "--platform") args.platform = argv[++i].toLowerCase();
  }
  if (!args.height) args.height = Math.round(((args.width * 9) / 16) / 2) * 2;
  return args;
}

// The overlay is the only thing added to the page: a caption and a drawn
// cursor. Every other pixel is the product.
const OVERLAY = `
  #tour-layer { position: fixed; inset: 0; z-index: 2147483000; pointer-events: none; }
  #tour-cursor {
    position: absolute; left: 0; top: 0; width: 18px; height: 18px; margin: -9px 0 0 -9px;
    border-radius: 50%; background: #0A0A0A;
    box-shadow: 0 0 0 2px #fff, 0 0 0 3px rgba(0, 0, 0, .22);
    transition: transform .5s cubic-bezier(.32, .72, 0, 1);
  }
  #tour-cursor.click::after {
    content: ""; position: absolute; inset: -10px; border-radius: 50%;
    border: 2px solid rgba(10, 10, 10, .55);
    animation: tour-ripple .5s ease-out both;
  }
  @keyframes tour-ripple { from { transform: scale(.4); opacity: 1; } to { transform: scale(1.5); opacity: 0; } }
  #tour-caption {
    position: absolute; left: 20px; bottom: 20px; max-width: 460px;
    padding: 10px 14px; border-radius: 8px;
    background: #0A0A0A; color: #fff;
    font: 500 13px/1.45 Inter, -apple-system, "Segoe UI", Roboto, sans-serif;
    box-shadow: 0 8px 28px rgba(0, 0, 0, .22);
  }
  #tour-caption b { display: block; font-weight: 600; letter-spacing: -.01em; }
  #tour-caption span { display: block; margin-top: 2px; color: #C9C9C9; font-weight: 400; }
  #tour-caption i {
    display: block; margin-bottom: 4px; font-style: normal;
    font-size: 10px; letter-spacing: .08em; text-transform: uppercase; color: #9A9A9A;
  }
`;

function overlayScript() {
  const mount = () => {
    const style = document.createElement("style");
    style.textContent = OVERLAY;
    document.head.appendChild(style);
    const layer = document.createElement("div");
    layer.id = "tour-layer";
    layer.innerHTML = '<div id="tour-cursor"></div><div id="tour-caption"><i></i><b></b><span></span></div>';
    document.body.appendChild(layer);
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", mount);
  else mount();
}

// The app boots from localStorage, not from the fixture state alone: without a
// stored project id it renders the empty screen no matter which state the
// server is answering.
function determinismScript({ fixedNow, seed, projectId }) {
  const NativeDate = Date;
  class FixedDate extends NativeDate {
    constructor(...args) {
      if (args.length === 0) super(fixedNow);
      else super(...args);
    }
    static now() {
      return fixedNow;
    }
  }
  window.Date = FixedDate;
  let value = seed >>> 0;
  Math.random = () => {
    value = (value * 1664525 + 1013904223) >>> 0;
    return value / 4294967296;
  };
  try {
    localStorage.clear();
    if (projectId) {
      localStorage.setItem("cf-project", projectId);
      localStorage.setItem("cf-caption-style", "impact");
      localStorage.setItem("cf-accent-color", "#FFDD00");
    }
  } catch {
    /* about:blank has no storage; the real document does. */
  }
}

async function caption(page, { index, total, title, detail }) {
  await page.evaluate(({ index, total, title, detail }) => {
    const box = document.getElementById("tour-caption");
    if (!box) return;
    box.querySelector("i").textContent = `${index} / ${total}`;
    box.querySelector("b").textContent = title;
    box.querySelector("span").textContent = detail || "";
  }, { index, total, title, detail });
}

async function moveTo(page, selector) {
  const box = await page.locator(selector).first().boundingBox().catch(() => null);
  if (!box) {
    console.warn(`  (no ${selector} to point at)`);
    return false;
  }
  await page.evaluate(({ x, y }) => {
    const cursor = document.getElementById("tour-cursor");
    if (cursor) cursor.style.transform = `translate(${x}px, ${y}px)`;
  }, { x: box.x + box.width / 2, y: box.y + box.height / 2 });
  await page.waitForTimeout(420);
  return true;
}

async function ripple(page) {
  await page.evaluate(() => {
    const cursor = document.getElementById("tour-cursor");
    if (!cursor) return;
    cursor.classList.remove("click");
    void cursor.offsetWidth;
    cursor.classList.add("click");
  });
  await page.waitForTimeout(400);
  await page.evaluate(() => document.getElementById("tour-cursor")?.classList.remove("click"));
}

async function clickAt(page, selector) {
  const target = page.locator(selector).first();
  if (!(await target.count())) {
    console.warn(`  (skipped: no ${selector})`);
    return;
  }
  await moveTo(page, selector);
  await ripple(page);
  await target.click({ timeout: 5000 }).catch((error) => {
    console.warn(`  (click failed on ${selector}: ${String(error.message).split("\n")[0]})`);
  });
  await page.waitForTimeout(280);
}

async function pressKey(page, key) {
  await page.keyboard.press(key);
  await page.waitForTimeout(260);
}

const TOUR = [
  {
    state: "empty-upload",
    title: "Nothing loaded yet",
    detail: "One drop surface, one format sentence, one action.",
    async run(page) {
      await moveTo(page, "#choose-btn");
    },
  },
  {
    state: "staged-file",
    ready: "#drop-staged:not(.hidden)",
    title: "A file is staged",
    detail: SOURCE_NAME,
    async prepare(page) {
      await page.setInputFiles("#file-input", {
        name: SOURCE_NAME,
        mimeType: "video/mp4",
        buffer: Buffer.alloc(2_411_724, 7),
      });
      await page.waitForSelector("#drop-staged:not(.hidden)");
    },
    async run(page, { captionFor }) {
      await captionFor("Captions, framing and accent are chosen before the run", "Pop, Clean, Cinema, Impact: one control, four styles.");
      await clickAt(page, '#upload-caption-style label:has-text("Clean")');
      await clickAt(page, '#upload-caption-style label:has-text("Pop")');
      await clickAt(page, '.upload-swatch[data-color="#FF4F4F"]');
      await clickAt(page, 'label:has(input[name="framing-mode"][value="background"])');
      await moveTo(page, "#start-btn");
    },
  },
  {
    state: "processing-mid-run",
    ready: "#processing-state:not(.hidden) .step.active",
    title: "Seven stages, one progress model",
    detail: "Each stage reports its own facts; the overall bar carries the whole run.",
    async run(page) {
      await moveTo(page, ".step.active");
    },
  },
  {
    state: "processing-error",
    ready: "#error-box:not(.hidden)",
    title: "A failure keeps its context",
    detail: "The stage is named, the cause is quoted, retry is the one filled action.",
    async run(page) {
      await moveTo(page, "#retry-btn");
    },
  },
  {
    state: "results-6-clips",
    ready: "#results-state:not(.hidden) .rail-item",
    title: "Six clips, best first",
    detail: "Click a row and the stage follows it.",
    async run(page, { captionFor }) {
      await moveTo(page, ".rail-item");
      await captionFor("Working down the list", "Rank, score and duration sit on one line; the still stays a still.");
      await clickAt(page, ".rail-item:nth-child(3)");
      await clickAt(page, ".rail-item:nth-child(5)");
      await clickAt(page, ".rail-item:nth-child(1)");
    },
  },
  {
    state: "clip-detail-restyle",
    ready: ".clip.is-active .seg-btn",
    title: "The inspector owns the selected clip",
    detail: "Caption style, accent, extras, and one Apply.",
    async run(page, { captionFor }) {
      await clickAt(page, '.clip.is-active .seg-btn:has-text("Pop")');
      await clickAt(page, ".clip.is-active .swatch >> nth=2");
      await clickAt(page, '.clip.is-active .switch-row:has-text("Zoom cuts") input');
      await captionFor("One Apply, and the preview reloads in place", "Seconds, not a full re-render.");
      await moveTo(page, ".apply-captions");
      await clickAt(page, ".apply-captions");
    },
  },
  {
    state: "review-theater",
    ready: "#review-theater:not([hidden])",
    title: "Keyboard triage over the rendered clip",
    detail: "1 keep, 2 maybe, 3 skip, arrows to move.",
    async prepare(page) {
      await page.locator("#review-clips-btn").click();
      await page.waitForSelector("#review-theater:not([hidden])");
    },
    async run(page, { captionFor }) {
      await moveTo(page, ".review-decisions");
      await pressKey(page, "1");
      await pressKey(page, "1");
      await pressKey(page, "2");
      await captionFor("Every decision is one key and one dot", "The dots across the top are the whole run.");
      await pressKey(page, "3");
    },
  },
  {
    state: "review-done",
    ready: "#review-done:not([hidden])",
    title: "The run ends on a decision, not a dead end",
    detail: "Kept, maybe, skipped, then one download.",
    async prepare(page) {
      await page.locator("#review-clips-btn").click();
      await page.waitForSelector("#review-theater:not([hidden])");
      for (let i = 0; i < 6; i += 1) {
        await page.keyboard.press("1");
        await page.waitForTimeout(60);
      }
      await page.waitForSelector("#review-done:not([hidden])");
    },
    async run(page) {
      await moveTo(page, "#review-done-download");
    },
  },
  {
    state: "library-open",
    ready: "#library-drawer.open",
    title: "Every project on this machine",
    detail: "Open, failed and cancelled keep their own words.",
    async prepare(page) {
      await page.locator("#library-btn").click();
      await page.waitForSelector("#library-drawer.open");
    },
    async run(page) {
      await moveTo(page, "#library-list .library-card:nth-child(2)");
    },
  },
  {
    state: "setup-missing-runtime",
    ready: "#setup-banner:not(.hidden)",
    title: "Missing dependencies are named one by one",
    detail: "Each line is a thing to install, not a generic failure.",
    async run(page) {
      await moveTo(page, ".banner");
    },
  },
];

function transcode(webm, mp4) {
  execFileSync("ffmpeg", [
    "-y", "-v", "error",
    "-i", webm,
    "-c:v", "libx264", "-preset", "medium", "-crf", "26",
    "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-an",
    mp4,
  ], { stdio: ["ignore", "pipe", "pipe"] });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  await rm(VIDEO_DIR, { recursive: true, force: true });
  await rm(STILLS, { recursive: true, force: true });
  await mkdir(VIDEO_DIR, { recursive: true });
  await mkdir(STILLS, { recursive: true });

  const server = await createFixtureServer();
  const browser = await launchBrowser();
  const context = await browser.newContext({
    viewport: { width: args.width, height: args.height },
    deviceScaleFactor: 1,
    locale: "en-US",
    timezoneId: "UTC",
    colorScheme: "light",
    recordVideo: { dir: VIDEO_DIR, size: { width: args.width, height: args.height } },
  });
  await context.addInitScript(overlayScript);
  const page = await context.newPage();

  const steps = [];
  try {
    for (let i = 0; i < TOUR.length; i += 1) {
      const step = TOUR[i];
      const meta = { index: i + 1, total: TOUR.length, title: step.title, detail: step.detail };

      // The app reads the project id at boot, so it is written per step.
      await context.addInitScript(determinismScript, {
        fixedNow: FIXED_NOW,
        seed: 0x5eed,
        projectId: PROJECT_STATES.has(step.state) ? PROJECT_ID : null,
      });

      await page.goto(`${server.url}/?state=${encodeURIComponent(step.state)}`, { waitUntil: "domcontentloaded" });
      await page.goto(`${server.url}/`, { waitUntil: "domcontentloaded" });
      await page.waitForSelector("header");
      await page.waitForTimeout(500);
      // A step that has to open something first does it before anyone waits
      // for the screen it opens.
      if (step.prepare) await step.prepare(page);
      if (step.ready) {
        await page.waitForSelector(step.ready, { timeout: 15_000 })
          .catch(() => console.warn(`  (ready selector never appeared: ${step.ready})`));
      }
      await pinPosters(page);
      await settle(page);
      await caption(page, meta);
      await page.waitForTimeout(900);

      const captionFor = async (title, detail) => {
        meta.title = title;
        meta.detail = detail;
        await caption(page, { ...meta, index: i + 1 });
        await page.waitForTimeout(700);
      };

      await step.run(page, { captionFor, meta });
      await page.waitForTimeout(1100);

      const still = path.join(STILLS, `${String(i + 1).padStart(2, "0")}-${step.state}.png`);
      await page.screenshot({ path: still, animations: "disabled" });
      steps.push({ ...meta, state: step.state, still: path.relative(REPO, still) });
      console.log(`step ${i + 1}/${TOUR.length} ${step.state}`);
    }
  } finally {
    const video = page.video();
    await context.close();
    await browser.close();
    await server.close();
    if (video && video.path) {
      const webm = await video.path();
      const mp4 = path.join(OUT, "tour.mp4");
      transcode(webm, mp4);
      await rm(webm, { force: true });
      console.log(`video: ${path.relative(REPO, mp4)}`);
    }
  }

  const md = [
    "# Studio tour",
    "",
    `Recorded at ${args.width}x${args.height} from the real static bundle, in the installed Chrome, against fixture API data.`,
    "",
    ...steps.map((step) => `${step.index}. **${step.title}**, \`${step.state}\`${step.detail ? `: ${step.detail}` : ""}`),
    "",
  ].join("\n");
  await writeFile(path.join(OUT, "steps.md"), md);
  console.log(`steps: ${path.relative(REPO, path.join(OUT, "steps.md"))}`);
}

await main();
