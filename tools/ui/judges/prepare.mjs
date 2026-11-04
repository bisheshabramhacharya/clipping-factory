// Build the prompt packet for one judge role.
//
//   node judges/prepare.mjs --round 0 --role absolute
//   node judges/prepare.mjs --round 0 --role defect-hunter
//   node judges/prepare.mjs --round 1 --role comparative --category typography
//   node judges/prepare.mjs --round 1 --role comparative --category all
//
// The comparative pass is one call per category. Each call gets a blind pair:
// this studio's screen for that category and the real T3 Code window, both at
// 1280 and 1440, copied into A/ and B/ in a per-category shuffled order. The
// label map lands in rounds/round-<n>/comparative-map.json, outside the judge's
// input directory, so the judge cannot read which side is which.

import { copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORIES, CATEGORY_KEYS } from "./categories.mjs";
import { STATES as FIXTURE_STATES } from "../fixtures/state.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const RIG = path.resolve(ROOT, "..");
const REPO = path.resolve(RIG, "..", "..");
const SHOT_PATTERN = /^(.+)@(\d+)\.([a-z0-9]+)\.png$/;
const STATE_ORDER = FIXTURE_STATES.map((state) => state.name);

// The screen that carries each category's craft. Picking the screen per
// category keeps the comparison about one thing at a time instead of asking a
// judge to average a whole app.
const CATEGORY_STATE = {
  typography: "results-6-clips",
  hierarchy: "results-6-clips",
  spacing_rhythm: "results-6-clips",
  color_contrast: "results-6-clips",
  component_consistency: "clip-detail-restyle",
  iconography_detail: "results-6-clips",
  primary_action: "staged-file",
  empty_error_states: "processing-error",
};

const PANEL_WIDTHS = [1280, 1440];
const MAX_IMAGES_PER_CALL = 10;

function parseArgs(argv) {
  const args = { round: 0, role: null, category: null, platforms: null, viewport: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
    else if (argv[i] === "--role") args.role = argv[++i];
    else if (argv[i] === "--category") args.category = argv[++i];
    else if (argv[i] === "--platforms") args.platforms = argv[++i].split(",").map((value) => value.trim());
    else if (argv[i] === "--viewport") args.viewport = Number(argv[++i]);
  }
  if (!["absolute", "comparative", "defect-hunter"].includes(args.role)) {
    throw new Error("--role must be absolute, comparative or defect-hunter");
  }
  if (args.role !== "comparative") {
    if (!Number.isInteger(args.viewport)) {
      throw new Error(`${args.role} needs --viewport <width>: a call carries one viewport's shots, never the whole set`);
    }
  }
  if (args.role === "comparative") {
    if (!args.category) throw new Error("comparative needs --category <key> or --category all");
    if (args.category !== "all" && !CATEGORY_KEYS.includes(args.category)) {
      throw new Error(`--category must be one of ${CATEGORY_KEYS.join(", ")} or all`);
    }
  }
  return args;
}

async function discoverShots(platforms) {
  const dirs = (await readdir(RIG)).filter((name) => name === "shots" || name.startsWith("shots-"));
  const entries = [];
  for (const dir of dirs) {
    const absolute = path.join(RIG, dir);
    if (!(await stat(absolute)).isDirectory()) continue;
    for (const file of await readdir(absolute)) {
      const match = SHOT_PATTERN.exec(file);
      if (!match) continue;
      const [, state, width, platform] = match;
      if (platforms && !platforms.includes(platform)) continue;
      entries.push({
        state,
        width: Number(width),
        platform,
        file,
        dir,
        absolute: path.join(absolute, file),
        relative: path.join(dir, file),
      });
    }
  }
  entries.sort((a, b) => {
    const stateDelta = STATE_ORDER.indexOf(a.state) - STATE_ORDER.indexOf(b.state);
    if (stateDelta !== 0) return stateDelta;
    if (a.state !== b.state) return a.state.localeCompare(b.state);
    if (a.width !== b.width) return a.width - b.width;
    if (a.platform !== b.platform) return a.platform.localeCompare(b.platform);
    return a.file.localeCompare(b.file);
  });
  return entries;
}

function imageList(entries) {
  return entries
    .map((entry) => `${entry.absolute}\n  state: ${entry.state} · viewport: ${entry.width} · platform: ${entry.platform}`)
    .join("\n\n");
}

function hashKey(text) {
  let hash = 0x811c9dc5;
  for (const character of String(text)) {
    hash ^= character.codePointAt(0);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

function shuffle(values, seed) {
  const result = [...values];
  let state = seed >>> 0;
  const random = () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  for (let i = result.length - 1; i > 0; i -= 1) {
    const j = Math.floor(random() * (i + 1));
    [result[i], result[j]] = [result[j], result[i]];
  }
  return result;
}

async function loadReferenceManifest() {
  const manifest = JSON.parse(await readFile(path.join(RIG, "reference", "manifest.json"), "utf8"));
  const scored = manifest.files.filter((file) => file.role === "reference");
  if (!scored.length) throw new Error("reference/manifest.json has no file with role \"reference\"");
  const aspirational = manifest.files.filter((file) => file.role === "aspiration");
  return { scored, aspirational };
}

async function buildComparative({ round, categories, entries }) {
  const { scored, aspirational } = await loadReferenceManifest();
  const inboxRoot = path.join(ROOT, "inbox", `round-${round}`, "comparative");
  const mapPath = path.join(REPO, "tools", "ui", "rounds", `round-${round}`, "comparative-map.json");
  await mkdir(path.dirname(mapPath), { recursive: true });

  const existingMap = await readFile(mapPath, "utf8").then((text) => JSON.parse(text)).catch(() => ({ round, categories: {} }));
  const mapping = { round, categories: { ...existingMap.categories } };
  const built = [];

  for (const category of categories) {
    const state = CATEGORY_STATE[category];
    const studioShots = PANEL_WIDTHS.map((width) => {
      const candidates = entries.filter((entry) => entry.state === state && entry.width === width);
      if (!candidates.length) throw new Error(`no ${state}@${width} shot; run npm run shots first`);
      return candidates.find((entry) => entry.platform === "macos") || candidates[0];
    });
    const references = PANEL_WIDTHS.map((width) => {
      const match = scored.find((file) => file.viewport === String(width));
      if (!match) throw new Error(`reference/manifest.json has no role "reference" file at viewport ${width}`);
      return match;
    });

    const sides = shuffle(["studio", "reference"], hashKey(`round-${round}:${category}`));
    const labels = { A: sides[0], B: sides[1] };
    const dir = path.join(inboxRoot, category);
    const poolRoot = path.join(dir, "pool");
    await rm(poolRoot, { recursive: true, force: true });

    const files = {};
    for (const label of ["A", "B"]) {
      const side = labels[label];
      const outDir = path.join(poolRoot, label);
      await mkdir(outDir, { recursive: true });
      for (const [index, width] of PANEL_WIDTHS.entries()) {
        const name = `${String(index + 1).padStart(2, "0")}.png`;
        const target = path.join(outDir, name);
        if (side === "studio") {
          const shot = studioShots[index];
          await copyFile(shot.absolute, target);
          files[`${label}/${name}`] = { side, viewport: String(width), origin: shot.relative };
        } else {
          const reference = references[index];
          await copyFile(path.join(RIG, "reference", reference.file), target);
          files[`${label}/${name}`] = { side, viewport: String(width), origin: `reference/${reference.file}` };
        }
      }
    }

    const imageCount = Object.keys(files).length;
    if (imageCount > MAX_IMAGES_PER_CALL) {
      throw new Error(`${category} panel has ${imageCount} images, over the ${MAX_IMAGES_PER_CALL} limit`);
    }

    mapping.categories[category] = { labels, files };
    const categoryLabel = CATEGORIES.find((entry) => entry.key === category).label;
    const answerPath = path.join(dir, "answer.json");
    const pools = ["A", "B"]
      .map((label) => `${label}:\n${PANEL_WIDTHS.map((_, index) => `  ${path.join(poolRoot, label, `${String(index + 1).padStart(2, "0")}.png`)}`).join("\n")}`)
      .join("\n\n");

    const prompt = (await readFile(path.join(ROOT, "prompts", "comparative.md"), "utf8"))
      .replaceAll("{{ROUND}}", String(round))
      .replaceAll("{{CATEGORY}}", category)
      .replaceAll("{{CATEGORY_LABEL}}", categoryLabel)
      .replaceAll("{{RUBRIC}}", path.join(RIG, "rubric.md"))
      .replaceAll("{{POOLS}}", pools)
      .replaceAll("{{IMAGE_COUNT}}", String(imageCount))
      .replaceAll("{{ANSWER}}", answerPath);
    await writeFile(path.join(dir, "prompt.md"), prompt);
    await writeFile(path.join(dir, "panel.json"), `${JSON.stringify({
      round,
      category,
      images: imageCount,
      widths: PANEL_WIDTHS,
      pool: poolRoot,
      answer: answerPath,
    }, null, 2)}\n`);

    built.push({ category, dir, answerPath, imageCount });
  }

  await writeFile(mapPath, `${JSON.stringify(mapping, null, 2)}\n`);
  return { built, mapPath, aspirational: aspirational.length, scored: scored.length };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const entries = await discoverShots(args.platforms);
  if (!entries.length) throw new Error("no shots found; run npm run shots first");

  if (args.role === "comparative") {
    const categories = args.category === "all" ? CATEGORY_KEYS : [args.category];
    const result = await buildComparative({ round: args.round, categories, entries });
    console.log(`role: comparative · round ${args.round} · ${categories.length} category call(s)`);
    for (const call of result.built) {
      console.log(`  ${call.category}: ${call.imageCount} images → ${call.dir}`);
    }
    console.log(`map: ${result.mapPath}`);
    console.log(`scored references: ${result.scored}; aspiration-only files excluded: ${result.aspirational}`);
    return;
  }

  const selected = entries.filter((entry) => entry.width === args.viewport);
  if (!selected.length) throw new Error(`no ${args.viewport} shots; run npm run shots first`);
  if (selected.length > MAX_IMAGES_PER_CALL) {
    throw new Error(`${args.role} would carry ${selected.length} images, over the ${MAX_IMAGES_PER_CALL} cap; split the call further`);
  }

  const inbox = path.join(ROOT, "inbox", `round-${args.round}`, `${args.role}-${args.viewport}`);
  await mkdir(inbox, { recursive: true });
  const answerPath = path.join(inbox, "answer.json");
  const template = await readFile(path.join(ROOT, "prompts", `${args.role}.md`), "utf8");
  const prompt = template
    .replaceAll("{{ROUND}}", String(args.round))
    .replaceAll("{{RUBRIC}}", path.join(RIG, "rubric.md"))
    .replaceAll("{{VIEWPORT}}", String(args.viewport))
    .replaceAll("{{IMAGE_COUNT}}", String(selected.length))
    .replaceAll("{{IMAGES}}", imageList(selected))
    .replaceAll("{{ANSWER}}", answerPath);

  await writeFile(path.join(inbox, "prompt.md"), prompt);
  await writeFile(path.join(inbox, "images.json"), `${JSON.stringify({
    role: args.role,
    round: args.round,
    viewport: args.viewport,
    shots: selected.map((entry) => ({ ...entry, absolute: undefined })),
  }, null, 2)}\n`);

  console.log(`role: ${args.role} · viewport ${args.viewport}`);
  console.log(`prompt: ${path.join(inbox, "prompt.md")}`);
  console.log(`answer: ${answerPath}`);
  console.log(`images: ${selected.length} shots, ${CATEGORIES.length} categories`);
}

await main();
