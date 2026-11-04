// Regenerate the fake clip stills used as video posters in the capture rig.
// The rig never decodes video: every preview shows one of these frames, which
// is the fixture's stand-in for "the first frame of the rendered clip".
//
//   node fixtures/make-frames.mjs
//
// Requires the installed Chrome (same requirement as the shot rig).

import { chromium } from "playwright-core";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OUT = path.join(path.dirname(fileURLToPath(import.meta.url)), "frames");

const FRAMES = [
  { hue: "#2B3440", warm: "#5B4636", accent: "#FFDD00", time: "12:04", lines: [["The", "pricing"], ["floor", "nobody"], ["says", "out loud"]] },
  { hue: "#33402F", warm: "#4A3A2E", accent: "#7CFF4F", time: "28:19", lines: [["Why the", "launch"], ["slipped", "twice"], ["", ""]] },
  { hue: "#3A2E38", warm: "#5B3B32", accent: "#FF4F4F", time: "41:07", lines: [["A hiring", "bar"], ["that hides"], ["the", "problem"]] },
  { hue: "#24333F", warm: "#4C3B33", accent: "#4FB5FF", time: "55:03", lines: [["The demo", "broke"], ["right", "here"], ["", ""]] },
  { hue: "#2E2A3A", warm: "#4A3630", accent: "#FF9F1C", time: "63:18", lines: [["One", "metric"], ["turned", "the"], ["room"]] },
  { hue: "#33302A", warm: "#523B31", accent: "#C77DFF", time: "77:40", lines: [["Where the", "advice"], ["stops", "working"], ["", ""]] },
];

function html() {
  return `<!doctype html><meta charset="utf-8">
  <style>
    * { box-sizing: border-box; margin: 0; }
    body { background: #111; }
    .frame {
      position: relative; width: 360px; height: 640px; overflow: hidden;
      font-family: -apple-system, "Segoe UI", sans-serif; color: #fff;
      display: flex; align-items: flex-end;
    }
    .bg { position: absolute; inset: 0; }
    .vignette { position: absolute; inset: 0; background: radial-gradient(120% 70% at 50% 30%, transparent 20%, rgba(0,0,0,.55) 100%); }
    .person { position: absolute; left: 50%; bottom: 0; width: 300px; height: 430px; transform: translateX(-50%); }
    .head { position: absolute; left: 50%; top: 0; width: 128px; height: 150px; border-radius: 50% 50% 46% 46%; transform: translateX(-50%); background: rgba(0,0,0,.34); box-shadow: inset -18px -10px 40px rgba(0,0,0,.28); }
    .body { position: absolute; left: 50%; bottom: 0; width: 300px; height: 300px; border-radius: 150px 150px 24px 24px; transform: translateX(-50%); background: rgba(0,0,0,.38); }
    .neck { position: absolute; left: 50%; top: 118px; width: 54px; height: 70px; transform: translateX(-50%); background: rgba(0,0,0,.34); }
    .rim { position: absolute; left: 50%; top: 8px; width: 120px; height: 142px; border-radius: 50%; transform: translateX(-50%); box-shadow: inset 0 0 0 2px rgba(255,255,255,.09); }
    .top { position: absolute; top: 18px; left: 18px; right: 18px; display: flex; justify-content: space-between; align-items: center; font-size: 10px; letter-spacing: .16em; font-weight: 700; color: rgba(255,255,255,.72); }
    .cap { position: relative; width: 100%; padding: 0 22px 74px; }
    .line { font-size: 38px; line-height: 1.02; font-weight: 800; letter-spacing: -.02em; text-shadow: 0 3px 14px rgba(0,0,0,.55); }
    .accent { color: var(--accent); }
    .safe { position: absolute; left: 14px; right: 14px; bottom: 14px; height: 2px; background: rgba(255,255,255,.16); border-radius: 2px; }
    .safe span { display: block; height: 2px; width: 34%; background: var(--accent); border-radius: 2px; }
  </style>
  <body>
  ${FRAMES.map((f) => `
    <div class="frame" style="--accent:${f.accent}">
      <div class="bg" style="background:
        radial-gradient(90% 60% at 62% 18%, ${f.warm} 0%, transparent 58%),
        linear-gradient(180deg, ${f.hue} 0%, #15161A 100%)"></div>
      <div class="vignette"></div>
      <div class="person"><div class="body"></div><div class="neck"></div><div class="head"></div><div class="rim"></div></div>
      <div class="top"><span>EPISODE 12</span><span>${f.time}</span></div>
      <div class="cap">
        ${f.lines.map(([a, b]) => `<div class="line">${a}${b ? ` <span class="accent">${b}</span>` : ""}</div>`).join("")}
      </div>
      <div class="safe"><span></span></div>
    </div>`).join("")}
  </body>`;
}

const browser = await chromium.launch({ channel: "chrome" });
const page = await browser.newPage({ viewport: { width: 400, height: 700 }, deviceScaleFactor: 1 });
await page.setContent(html());
const frames = await page.locator(".frame").all();
for (let i = 0; i < frames.length; i += 1) {
  await frames[i].screenshot({ path: path.join(OUT, `frame-${i + 1}.png`), animations: "disabled" });
}
// The rail thumbnail is 54x96 CSS px. Chrome's decode-at-scale path for a
// scaled image inside a rounded clip is not byte-stable, so the rig serves a
// pre-sized thumbnail instead: one fake frame, two resolutions, same look.
for (let i = 0; i < frames.length; i += 1) {
  const source = (await readFile(path.join(OUT, `frame-${i + 1}.png`))).toString("base64");
  const thumb = await page.evaluate(async (base64) => {
    const image = new Image();
    image.src = `data:image/png;base64,${base64}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = 54;
    canvas.height = 96;
    canvas.getContext("2d").drawImage(image, 0, 0, 54, 96);
    return canvas.toDataURL("image/png").split(",")[1];
  }, source);
  await writeFile(path.join(OUT, `thumb-${i + 1}.png`), Buffer.from(thumb, "base64"));
}
await browser.close();
console.log(`wrote ${frames.length} frames and thumbnails to ${OUT}`);
