// Shared pieces of the recorded tours: the overlay that draws the cursor and
// the caption, the frozen clock, and the small input helpers. Both tours (the
// fixture tour and the live one against a running studio) use exactly this, so
// the two recordings look like the same hand.

import { execFileSync } from "node:child_process";

export const OVERLAY = `
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

export function overlayScript() {
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

export function determinismScript({ fixedNow, seed, projectId }) {
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

export async function caption(page, { index, total, title, detail }) {
  await page.evaluate(({ index, total, title, detail }) => {
    const box = document.getElementById("tour-caption");
    if (!box) return;
    box.querySelector("i").textContent = `${index} / ${total}`;
    box.querySelector("b").textContent = title;
    box.querySelector("span").textContent = detail || "";
  }, { index, total, title, detail });
}

export async function moveTo(page, selector) {
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

export async function ripple(page) {
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

export async function clickAt(page, selector) {
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

export async function pressKey(page, key) {
  await page.keyboard.press(key);
  await page.waitForTimeout(260);
}

export function transcode(webm, mp4) {
  execFileSync("ffmpeg", [
    "-y", "-v", "error",
    "-i", webm,
    "-c:v", "libx264", "-preset", "medium", "-crf", "26",
    "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-an",
    mp4,
  ], { stdio: ["ignore", "pipe", "pipe"] });
}
