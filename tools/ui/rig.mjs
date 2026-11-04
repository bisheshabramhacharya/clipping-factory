// Shared browser plumbing for the shot rig and the deterministic checks.
// One context per (state, viewport): clean storage, frozen clock, fixed
// locale and timezone, animations off, no caret. Nothing here touches
// static/ or the server; it only drives the fixture studio.

import { chromium } from "playwright-core";
import { PROJECT_ID, SOURCE_NAME, STATES as FIXTURE_STATES, FIXED_NOW, CLIP_FRAME_INDEX } from "./fixtures/state.mjs";

export const VIEWPORTS = [
  { width: 1280, height: 800 },
  { width: 1440, height: 900 },
  { width: 1920, height: 1080 },
];

export const STATE_NAMES = FIXTURE_STATES.map((state) => state.name);

export const DISABLE_MOTION_CSS = `
  *, *::before, *::after {
    animation: none !important;
    transition: none !important;
    caret-color: transparent !important;
    scroll-behavior: auto !important;
  }
  html { scrollbar-width: none; }
  .preview .rig-poster {
    display: block; aspect-ratio: 9 / 16;
    border-radius: 6px; background: #000;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, .08), 0 18px 48px rgba(0, 0, 0, .16);
  }
`;

function determinismScript({ fixedNow, projectId, seed }) {
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

async function setStagedFile(page) {
  await page.setInputFiles("#file-input", {
    name: SOURCE_NAME,
    mimeType: "video/mp4",
    buffer: Buffer.alloc(2_411_724, 7),
  });
  await page.locator('#upload-caption-style label:has-text("Pop")').click();
  await page.locator('label:has(input[name="framing-mode"][value="background"])').click();
  await page.locator('.upload-swatch[data-color="#FF4F4F"]').click();
  await page.selectOption("#upload-language", "es");
  await page.selectOption("#upload-platform", "tiktok");
  await page.fill("#focus-prompt", "clips about compute and safety");
  await page.waitForSelector("#drop-staged:not(.hidden)");
}

async function openRejected(page) {
  await page.locator("#rejected-details > summary").click();
  await page.waitForSelector("#rejected-details[open]");
  await page.evaluate(() => {
    // Expanding must not move the rail: a person who opens a section expects
    // the clips above it to stay where they were. The rig used to scroll to the
    // bottom here, which made the shot look like the list had been replaced.
    const rail = document.querySelector(".clip-rail, .rail-scroll");
    if (rail) rail.scrollTop = 0;
  });
}

async function openTheater(page) {
  await page.locator("#review-clips-btn").click();
  await page.waitForSelector("#review-theater:not([hidden])");
  await page.waitForFunction(() => (document.querySelector("#review-title")?.textContent || "").length > 0);
}

// Six keeps walk the whole set, which is how the payoff card is reached.
async function finishReview(page) {
  await openTheater(page);
  for (let i = 0; i < 6; i += 1) {
    await page.keyboard.press("1");
    await page.waitForTimeout(30);
  }
  await page.waitForSelector("#review-done:not([hidden])");
}

async function openLibrary(page) {
  await page.locator("#library-btn").click();
  await page.waitForSelector("#library-drawer.open");
  await page.waitForFunction(() => document.querySelectorAll("#library-list .library-card").length >= 5);
}

async function openRestyle(page) {
  await page.locator(".rail-item").nth(2).click();
  await page.waitForFunction(() => document.querySelectorAll(".rail-item")[2]?.classList.contains("active") === true);
  await page.locator(".clip.is-active .caption-text-wrap > summary").click();
  await page.waitForSelector(".clip.is-active .caption-text");
  await page.fill(".clip.is-active .caption-text", "A hiring bar that hides the real problem.");
  await page.locator(".clip.is-active .seg-btn", { hasText: "Clean" }).click();
  await page.waitForFunction(() =>
    (document.querySelector(".clip.is-active .restyle-status")?.textContent || "").includes("Unsaved"));
}

const ACTIONS = {
  "staged-file": setStagedFile,
  "review-theater": openTheater,
  "review-done": finishReview,
  "results-rejected-expanded": openRejected,
  "library-open": openLibrary,
  "clip-detail-restyle": openRestyle,
};

const READY = {
  "empty-upload": () => document.querySelector("#upload-state:not(.hidden)") !== null,
  "staged-file": () => document.querySelector("#drop-staged:not(.hidden)") !== null,
  "processing-mid-run": () =>
    document.querySelector("#processing-state:not(.hidden)") !== null &&
    document.querySelector(".step.active .mini-fill") !== null &&
    document.querySelector("#overall:not(.hidden)") !== null &&
    document.querySelector("#current-op:not(.hidden)") !== null,
  "processing-error": () => document.querySelector("#error-box:not(.hidden)") !== null,
  "results-6-clips": () =>
    document.querySelector("#results-state:not(.hidden)") !== null &&
    document.querySelectorAll("#clip-rail-list .rail-item").length === 6 &&
    document.querySelectorAll(".clip.is-active").length === 1,
  "results-rejected-expanded": () =>
    document.querySelector("#rejected-details[open]") !== null &&
    document.querySelectorAll("#rejected-list .rejected-item").length === 4,
  "library-open": () =>
    document.querySelector("#library-drawer.open") !== null &&
    document.querySelectorAll("#library-list .library-card").length >= 5,
  "clip-detail-restyle": () =>
    document.querySelectorAll(".rail-item.active").length === 1 &&
    document.querySelector(".restyle") !== null &&
    document.querySelector(".caption-text") !== null,
  "review-theater": () =>
    document.querySelector("#review-theater:not([hidden])") !== null &&
    document.querySelector("#review-done[hidden]") !== null,
  "review-done": () => document.querySelector("#review-done:not([hidden])") !== null,
  "setup-missing-runtime": () => document.querySelector("#setup-banner:not(.hidden)") !== null,
};

export async function launchBrowser() {
  return chromium.launch({
    channel: "chrome",
    args: [
      "--force-color-profile=srgb",
      "--disable-lcd-text",
      "--disable-gpu",
      "--font-render-hinting=none",
      "--disable-font-subpixel-positioning",
      "--disable-skia-runtime-opts",
      "--disable-partial-raster",
      "--run-all-compositor-stages-before-draw",
    ],
  });
}

// Rewrite every preview <video> to a still poster. The fixture server has no
// media bytes on purpose: the real app shows a decoded frame, the rig shows a
// fake frame built for UI review.
export async function pinPosters(page) {
  await page.evaluate(async (frameIndex) => {
    for (const image of document.querySelectorAll("#clip-rail-list .rail-thumb img")) {
      const item = image.closest(".rail-item");
      const key = item && item.dataset.reviewKey;
      const id = key ? key.split("/").pop() : null;
      const index = id ? frameIndex[id] : null;
      if (index) image.src = `/fixtures/frames/thumb-${index}.png`;
    }
    const videos = [...document.querySelectorAll(".preview video"), ...document.querySelectorAll("#review-video")];
    await Promise.all(videos.map(async (video) => {
      const match = /Preview clip (\d+)/.exec(video.getAttribute("aria-label") || "");
      // The theater opens on the best-ranked clip, which is frame 1.
      const url = match ? `/fixtures/frames/frame-${match[1]}.png` : `/fixtures/frames/frame-1.png`;
      // Draw the still at the exact box the player occupies. A scaled raster
      // inside a rounded clip is the one Chrome path that is not byte-stable
      // between runs; a 1:1 canvas is.
      const rect = video.getBoundingClientRect();
      const width = Math.max(1, Math.round(rect.width));
      const height = Math.max(1, Math.round(rect.height));
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
  }, CLIP_FRAME_INDEX);
}

export async function settle(page) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    const images = [...document.images];
    for (const image of images) image.loading = "eager";
    await Promise.all(images.map((image) => image.decode().catch(() => {})));
  });
  await page.waitForFunction(() => [...document.images].every((image) => image.complete));
  await page.waitForTimeout(400);
}

export async function openState(browser, base, state, viewport) {
  const projectId = PROJECT_STATES.has(state) ? PROJECT_ID : null;
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
    locale: "en-US",
    timezoneId: "UTC",
    colorScheme: "light",
    reducedMotion: "reduce",
    extraHTTPHeaders: { "X-UI-State": state },
  });
  await context.addInitScript(determinismScript, {
    fixedNow: FIXED_NOW,
    projectId,
    seed: 0x5eed,
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => {
    if (process.env.UI_RIG_DEBUG) console.error(`[${state}] pageerror: ${error.message}`);
  });
  await page.goto(`${base}/`, { waitUntil: "domcontentloaded" });
  await page.addStyleTag({ content: DISABLE_MOTION_CSS });
  const action = ACTIONS[state];
  if (action) await action(page);
  const ready = READY[state];
  if (ready) await page.waitForFunction(ready, null, { timeout: 15_000 });
  await pinPosters(page);
  await page.evaluate(() => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  });
  await settle(page);
  return { context, page };
}

export async function closeState(handle) {
  await handle.context.close();
}

export function shotName(state, width, platform) {
  return `${state}@${width}.${platform}.png`;
}
