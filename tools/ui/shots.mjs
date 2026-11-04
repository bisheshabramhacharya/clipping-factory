// Capture every studio state at every viewport, twice when asked, and build
// the contact sheet.
//
//   npm run shots
//   npm run shots -- --out tools/ui/shots-windows --platform windows
//   npm run shots -- --verify
//   npm run shots -- --sheet-only
//
// Filenames carry the platform: <state>@<width>.<platform>.png. Nothing in
// this file reads macOS-only paths or launches a shell.

import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, rm, writeFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createFixtureServer } from "./server.mjs";
import { launchBrowser, openState, closeState, VIEWPORTS, STATE_NAMES, shotName } from "./rig.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_OUT = path.join(ROOT, "shots");
const SHEET_STATES = STATE_NAMES;

function parseArgs(argv) {
  const args = { out: DEFAULT_OUT, platform: null, verify: false, sheetOnly: false, only: null, widths: null, quiet: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--out") args.out = path.resolve(ROOT, "..", "..", argv[++i]);
    else if (arg === "--platform") args.platform = argv[++i].toLowerCase();
    else if (arg === "--verify") args.verify = true;
    else if (arg === "--sheet-only") args.sheetOnly = true;
    else if (arg === "--only") args.only = argv[++i].split(",").map((value) => value.trim()).filter(Boolean);
    else if (arg === "--width") args.widths = argv[++i].split(",").map(Number).filter(Boolean);
    else if (arg === "--quiet") args.quiet = true;
  }
  return args;
}

function defaultPlatform() {
  if (process.platform === "darwin") return "macos";
  if (process.platform === "win32") return "windows";
  return process.platform;
}

async function sha256(file) {
  return createHash("sha256").update(await readFile(file)).digest("hex");
}

async function captureAll({ out, platform, only, widths }) {
  const states = only ? STATE_NAMES.filter((state) => only.includes(state)) : STATE_NAMES;
  const viewports = VIEWPORTS.filter((viewport) => !widths || widths.includes(viewport.width));
  await mkdir(out, { recursive: true });

  const existing = await readdir(out).catch(() => []);
  await Promise.all(existing
    .filter((name) => name.endsWith(`.${platform}.png`))
    .map((name) => rm(path.join(out, name), { force: true })));

  const server = await createFixtureServer();
  const browser = await launchBrowser();
  const files = [];
  const started = Date.now();
  try {
    for (const state of states) {
      for (const viewport of viewports) {
        const handle = await openState(browser, server.url, state, viewport);
        try {
          const name = shotName(state, viewport.width, platform);
          const target = path.join(out, name);
          if (state === "ai-connection-disabled") {
            await handle.page.locator("header").screenshot({ path: target, animations: "disabled", caret: "hide", scale: "css" });
          } else {
            await handle.page.screenshot({ path: target, animations: "disabled", caret: "hide", scale: "css" });
          }
          files.push(name);
        } finally {
          await closeState(handle);
        }
      }
    }
  } finally {
    await browser.close();
    await server.close();
  }
  files.sort();
  return { files, seconds: Math.round((Date.now() - started) / 100) / 10, states, widths: viewports.map((viewport) => viewport.width) };
}

async function digestRun(out, names) {
  const entries = {};
  for (const name of names) entries[name] = await sha256(path.join(out, name));
  return entries;
}

async function writeContactSheet({ output = path.join(ROOT, "index.html") } = {}) {
  const dirs = (await readdir(ROOT))
    .filter((name) => name === "shots" || name.startsWith("shots-"));
  const shotDirs = [];
  for (const name of dirs) {
    const absolute = path.join(ROOT, name);
    if ((await stat(absolute)).isDirectory()) shotDirs.push({ name, absolute });
  }
  shotDirs.sort((a, b) => a.name.localeCompare(b.name));

  const shotPattern = /^(.+)@(\d+)\.([a-z0-9]+)\.png$/;
  const byState = new Map();
  const platforms = new Set();
  let total = 0;
  for (const dir of shotDirs) {
    for (const file of await readdir(dir.absolute)) {
      const match = shotPattern.exec(file);
      if (!match) continue;
      const [, state, width, platform] = match;
      platforms.add(platform);
      if (!byState.has(state)) byState.set(state, new Map());
      const widths = byState.get(state);
      const key = Number(width);
      if (!widths.has(key)) widths.set(key, new Map());
      widths.get(key).set(platform, `${dir.name}/${file}`);
      total += 1;
    }
  }

  const orderedStates = [
    ...SHEET_STATES.filter((state) => byState.has(state)),
    ...[...byState.keys()].filter((state) => !SHEET_STATES.includes(state)).sort(),
  ];
  const allWidths = [...new Set(VIEWPORTS.map((viewport) => viewport.width))];
  const platformList = [...platforms].sort();
  const missing = ["macos", "windows"].filter((platform) => !platforms.has(platform));

  const rows = orderedStates.map((state) => {
    const widths = byState.get(state);
    const cells = allWidths
      .filter((width) => widths.has(width))
      .map((width) => {
        const entries = widths.get(width);
        const figures = platformList.map((platform) => {
          const file = entries.get(platform);
          if (!file) {
            return `<figure class="missing"><div class="absent">no ${platform} shot</div><figcaption>${platform}</figcaption></figure>`;
          }
          return `<figure><a href="${file}"><img loading="lazy" src="${file}" alt="${state} at ${width} on ${platform}"></a><figcaption>${platform}</figcaption></figure>`;
        }).join("");
        return `<div class="width-row"><h3>${width}px</h3><div class="frames">${figures}</div></div>`;
      }).join("");
    return `<section class="state"><h2>${state}</h2>${cells}</section>`;
  }).join("\n");

  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clipping Factory UI shots</title>
<style>
  :root { color-scheme: light; }
  * { box-sizing: border-box; }
  body { margin: 0; padding: 32px; background: #f2f1ee; color: #1b1a19;
    font: 14px/1.5 -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif; }
  h1 { font-size: 20px; margin: 0 0 4px; }
  .meta { color: #6b6860; margin-bottom: 24px; }
  .banner { background: #fff; border-left: 3px solid #b7791f; padding: 10px 14px;
    border-radius: 10px; margin-bottom: 20px; max-width: 900px; }
  .state { margin: 0 0 36px; }
  .state > h2 { font-size: 15px; margin: 0 0 8px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .width-row { display: grid; grid-template-columns: 70px minmax(0, 1fr); gap: 12px; align-items: start;
    padding: 10px 0; border-top: 1px solid rgba(27,26,25,.1); }
  .width-row h3 { font-size: 12px; color: #6b6860; margin: 4px 0 0; font-weight: 600; }
  .frames { display: flex; gap: 14px; flex-wrap: wrap; align-items: flex-start; }
  figure { margin: 0; }
  figure img { display: block; width: min(620px, 46vw); max-width: 100%; height: auto; border-radius: 8px;
    box-shadow: 0 2px 10px rgba(27,26,25,.14); background: #fff; }
  figcaption { font-size: 11px; color: #6b6860; margin-top: 4px; letter-spacing: .04em; text-transform: uppercase; }
  .missing .absent { width: 320px; height: 180px; border: 1px dashed #b9b5ab; border-radius: 8px;
    display: grid; place-items: center; color: #9a968d; font-size: 12px; }
  @media (min-width: 1800px) { figure img { width: 760px; } }
</style>
</head>
<body>
<h1>Clipping Factory UI shots</h1>
<p class="meta">${total} shot${total === 1 ? "" : "s"} across ${platformList.length ? platformList.join(", ") : "no platforms"} · generated by <code>npm run shots</code>. Do not edit by hand.</p>
${missing.length ? `<div class="banner"><strong>${missing.join(" and ")} shot${missing.length === 1 ? "" : "s"} not found.</strong> Put them in ${missing.map((platform) => `<code>tools/ui/shots-${platform}/</code>`).join(" or ")} and run <code>npm run shots -- --sheet-only</code>. Missing platforms are skipped, never invented.</div>` : ""}
${rows || "<p>No shots found. Run <code>npm run shots</code>.</p>"}
</body>
</html>`;
  await writeFile(output, html);
  return { output, total, platforms: platformList, states: orderedStates.length, missing };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const platform = args.platform || defaultPlatform();
  if (!args.sheetOnly) {
    const target = args.out;
    const first = await captureAll({ out: target, platform, only: args.only, widths: args.widths });
    let diffCount = null;
    if (args.verify) {
      const secondDir = path.join(target, `.verify-${process.pid}`);
      const second = await captureAll({ out: secondDir, platform, only: args.only, widths: args.widths });
      const a = await digestRun(target, first.files);
      const b = await digestRun(secondDir, second.files);
      const names = [...new Set([...Object.keys(a), ...Object.keys(b)])].sort();
      const diffs = names.filter((name) => a[name] !== b[name]);
      diffCount = diffs.length;
      await rm(secondDir, { recursive: true, force: true });
      const digestFile = path.join(target, `digests.${platform}.json`);
      await writeFile(digestFile, `${JSON.stringify({
        platform,
        viewport_widths: first.widths,
        states: first.states,
        files: a,
        verify: { diff_count: diffCount, compared: names.length },
      }, null, 2)}\n`);
      if (!args.quiet) {
        console.log(`shots: ${first.files.length} files on ${platform} in ${first.seconds}s → ${target}`);
        console.log(`determinism: ${diffCount === 0 ? "identical" : "DIFFERENT"} · diff count ${diffCount} of ${names.length}`);
      }
      if (diffCount !== 0) {
        console.log(`diffs: ${diffs.join(", ")}`);
        process.exitCode = 1;
      }
    } else {
      const digestFile = path.join(target, `digests.${platform}.json`);
      await writeFile(digestFile, `${JSON.stringify({
        platform,
        viewport_widths: first.widths,
        states: first.states,
        files: await digestRun(target, first.files),
      }, null, 2)}\n`);
      if (!args.quiet) console.log(`shots: ${first.files.length} files on ${platform} in ${first.seconds}s → ${target}`);
    }
  }
  const sheet = await writeContactSheet();
  if (!args.quiet) {
    console.log(`contact sheet: ${sheet.output} (${sheet.total} shots, platforms: ${sheet.platforms.join(", ") || "none"})`);
    if (sheet.missing.length) console.log(`note: no ${sheet.missing.join(" or ")} shots yet; sheet skips them.`);
  }
}

await main();
