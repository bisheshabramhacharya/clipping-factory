// One-off layout probe: does a full spoken-sentence headline fit?
//
//   node probe-clamp.mjs
//
// Prints, for the results screen at every viewport, the measured height of the
// clip title against its scroll height, and the same for the rail titles. A
// clamped title is not a bug by itself; a clamped title with no way to read the
// rest is, so this reports the numbers the design has to answer to.

import { createFixtureServer } from "./server.mjs";
import { launchBrowser, openState, closeState, VIEWPORTS } from "./rig.mjs";

const server = await createFixtureServer();
const browser = await launchBrowser();
try {
  for (const viewport of VIEWPORTS) {
    const handle = await openState(browser, server.url, "results-6-clips", viewport);
    try {
      const rows = await handle.page.evaluate(() => {
        const box = (element) => {
          const rect = element.getBoundingClientRect();
          return {
            text: (element.textContent || "").trim().slice(0, 44),
            clientHeight: Math.round(rect.height),
            scrollHeight: Math.round(element.scrollHeight),
            clamped: element.scrollHeight > element.clientHeight + 1,
            fontSize: getComputedStyle(element).fontSize,
          };
        };
        return {
          titles: [...document.querySelectorAll(".clip.is-active .clip-info h3")].map(box),
          rail: [...document.querySelectorAll(".rail-item .rail-title")].map(box),
          inspector: (() => {
            const element = document.querySelector(".clip.is-active .inspector");
            if (!element) return null;
            return { clientHeight: Math.round(element.clientHeight), scrollHeight: Math.round(element.scrollHeight) };
          })(),
        };
      });
      console.log(`\n${viewport.width}x${viewport.height}`);
      for (const title of rows.titles) console.log("  title", title.fontSize, `${title.clientHeight}/${title.scrollHeight}`, title.clamped ? "CLAMPED" : "fits", "|", title.text);
      for (const item of rows.rail) console.log("  rail ", item.fontSize, `${item.clientHeight}/${item.scrollHeight}`, item.clamped ? "clamped" : "fits", "|", item.text);
      if (rows.inspector) console.log("  inspector", `${rows.inspector.clientHeight}/${rows.inspector.scrollHeight}`, rows.inspector.scrollHeight > rows.inspector.clientHeight + 1 ? "OVERFLOWS" : "fits");
    } finally {
      await closeState(handle);
    }
  }
} finally {
  await browser.close();
  await server.close();
}
