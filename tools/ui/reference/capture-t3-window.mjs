// Capture the running T3 Code desktop app's main window at native viewport
// sizes, straight from its own Chromium renderer over the DevTools Protocol.
//
//   open -na "/Applications/T3 Code (Nightly).app" --args \
//     --remote-debugging-port=9333
//   node tools/ui/reference/capture-t3-window.mjs 9333 tools/ui/reference
//
// Why CDP and not screencapture: the capture has to be the app's own pixels at
// an exact CSS size, with no window shadow, no desktop behind it and no
// macOS title bar (the title bar would name the product and break the blind
// panel). screencapture needs Screen Recording consent that a shell session
// does not have, and it returns whatever the window happens to be sized to.
//
// The renderer is Chromium either way, so this is the same rendering path the
// studio's own shots are taken with: device scale factor 1, CSS pixels 1:1.

import { writeFile } from "node:fs/promises";
import path from "node:path";

const SIZES = [
  { width: 1280, height: 800 },
  { width: 1440, height: 900 },
];

const port = Number(process.argv[2] ?? 9333);
const outDir = process.argv[3];
if (!outDir) throw new Error("usage: node capture-t3-window.mjs <port> <out-dir>");

const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const page = targets.find((target) => target.type === "page" && target.url && !target.url.startsWith("devtools://"));
if (!page) throw new Error(`no page target on port ${port}: ${JSON.stringify(targets.map((t) => [t.type, t.url]))}`);

const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});

let nextId = 0;
const pending = new Map();
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  const entry = pending.get(message.id);
  if (!entry) return;
  pending.delete(message.id);
  if (message.error) entry.reject(new Error(`${entry.method}: ${message.error.message}`));
  else entry.resolve(message.result);
});

function send(method, params = {}) {
  const id = ++nextId;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject, method }));
}

await send("Page.enable");
const url = (await send("Runtime.evaluate", { expression: "location.href", returnByValue: true })).result.value;
console.log(`page: ${url}`);

// Transient chrome is not the app's design: an update toast and any open
// popover would be captured forever into a reference a judge scores against.
// Escape handles the toast; a click in empty workspace closes the project menu,
// and the click's focus is dropped because a caret is not part of the design.
const BLANK_SPOT = { x: 640, y: 620 };
await send("Runtime.evaluate", {
  expression: `
    for (const button of document.querySelectorAll('button[aria-label="Dismiss notification"]')) button.click();
    true;
  `,
});
for (const type of ["mousePressed", "mouseReleased"]) {
  await send("Input.dispatchMouseEvent", { type, x: BLANK_SPOT.x, y: BLANK_SPOT.y, button: "left", clickCount: 1 });
}
await send("Runtime.evaluate", { expression: "document.activeElement?.blur(); true" });
await new Promise((resolve) => setTimeout(resolve, 500));

const written = [];
for (const size of SIZES) {
  await send("Emulation.setDeviceMetricsOverride", {
    width: size.width,
    height: size.height,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await send("Runtime.evaluate", {
    expression: "new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))",
    awaitPromise: true,
  });
  const { data } = await send("Page.captureScreenshot", {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: false,
  });
  const file = path.join(outDir, `t3-code-window@${size.width}.png`);
  await writeFile(file, Buffer.from(data, "base64"));
  written.push(file);
  console.log(`wrote ${file} (${size.width}x${size.height})`);
}

await send("Emulation.clearDeviceMetricsOverride");
socket.close();
console.log(JSON.stringify({ url, files: written }, null, 2));
