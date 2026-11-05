// Build the prompt packet for one judge role.
//
//   node judges/prepare.mjs --round 0 --role absolute
//   node judges/prepare.mjs --round 5 --role absolute --viewport 1440 --states a,b,c
//     (there are more states than one call may carry; --states names the subset)
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

const DEFAULT_PANEL_WIDTHS = [1280];
const HARD_CATEGORY_CAP = 3;
const MAX_IMAGES_PER_CALL = 10;

function parseArgs(argv) {
  const args = { round: 0, role: null, category: null, platforms: null, viewport: null, widths: null, states: null, tag: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
    else if (argv[i] === "--role") args.role = argv[++i];
    else if (argv[i] === "--category") args.category = argv[++i];
    else if (argv[i] === "--platforms") args.platforms = argv[++i].split(",").map((value) => value.trim());
    else if (argv[i] === "--viewport") args.viewport = Number(argv[++i]);
    else if (argv[i] === "--widths") args.widths = argv[++i].split(",").map(Number).filter(Boolean);
    else if (argv[i] === "--states") args.states = argv[++i].split(",").map((value) => value.trim()).filter(Boolean);
    else if (argv[i] === "--tag") args.tag = argv[++i];
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
    if (args.category === "all") {
      throw new Error(`at most ${HARD_CATEGORY_CAP} categories per round; name them, or use --category worst`);
    }
    if (args.category !== "worst") {
      const list = args.category.split(",").map((value) => value.trim()).filter(Boolean);
      if (!list.length || list.some((key) => !CATEGORY_KEYS.includes(key))) {
        throw new Error(`--category must be worst, or one to ${MAX_CATEGORIES_PER_ROUND} of ${CATEGORY_KEYS.join(", ")}`);
      }
      if (list.length > HARD_CATEGORY_CAP) {
        throw new Error(`at most ${HARD_CATEGORY_CAP} categories per round; got ${list.length}`);
      }
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
    .map((entry) => `${entry.panelPath || entry.absolute}\n  state: ${entry.state} · viewport: ${entry.width} · platform: ${entry.platform}`)
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


// The three weakest categories by absolute score, so a round spends its calls
// where the work is. Absolute answers are filed per viewport; the category score
// here is the median across them, same rule as the round file.
async function comparativePolicy() {
  try {
    return JSON.parse(await readFile(path.join(ROOT, "judges.json"), "utf8")).comparative || {};
  } catch {
    return {};
  }
}

async function worstCategories(round, count) {
  const dir = path.join(REPO, "tools", "ui", "rounds", `round-${round}`, "raw");
  const parts = [];
  for (const width of [1280, 1440, 1920]) {
    try {
      parts.push(JSON.parse(await readFile(path.join(dir, `absolute-${width}.json`), "utf8")));
    } catch {
      /* viewport not judged; the others still count */
    }
  }
  if (!parts.length) {
    throw new Error(`round ${round} has no absolute answers; run --role absolute --viewport <width> first, or name categories`);
  }
  const scored = CATEGORIES.map((category) => {
    const values = parts
      .map((part) => part.categories?.[category.key]?.score)
      .filter((value) => typeof value === "number")
      .sort((a, b) => a - b);
    if (!values.length) return null;
    const middle = Math.floor(values.length / 2);
    const median = values.length % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2;
    return { key: category.key, median };
  }).filter(Boolean);
  return scored.sort((a, b) => a.median - b.median).slice(0, count).map((entry) => entry.key);
}

async function buildComparative({ round, categories, entries, widths }) {
  const { scored, aspirational } = await loadReferenceManifest();
  const inboxRoot = path.join(ROOT, "inbox", `round-${round}`, "comparative");
  const mapPath = path.join(REPO, "tools", "ui", "rounds", `round-${round}`, "comparative-map.json");
  await mkdir(path.dirname(mapPath), { recursive: true });

  const existingMap = await readFile(mapPath, "utf8").then((text) => JSON.parse(text)).catch(() => ({ round, categories: {} }));
  const mapping = { round, categories: { ...existingMap.categories } };
  const built = [];

  for (const category of categories) {
    const state = CATEGORY_STATE[category];
    const studioShots = widths.map((width) => {
      const candidates = entries.filter((entry) => entry.state === state && entry.width === width);
      if (!candidates.length) throw new Error(`no ${state}@${width} shot; run npm run shots first`);
      return candidates.find((entry) => entry.platform === "macos") || candidates[0];
    });
    const references = widths.map((width) => {
      const match = scored.find((file) => file.viewport === String(width));
      if (!match) throw new Error(`reference/manifest.json has no role "reference" file at viewport ${width}`);
      return match;
    });

    const sides = shuffle(["studio", "reference"], hashKey(`round-${round}:${category}`));
    const categoryLabel = CATEGORIES.find((entry) => entry.key === category).label;

    // Two orientations of the same pair. The second swaps A and B so the judge
    // cannot win a category by preferring whatever sits on the left.
    const orientations = [
      { key: category, labels: { A: sides[0], B: sides[1] } },
      { key: `${category}-swapped`, labels: { A: sides[1], B: sides[0] }, swapOf: category },
    ];

    for (const orientation of orientations) {
      const dir = path.join(inboxRoot, orientation.key);
      const poolRoot = path.join(dir, "pool");
      await rm(poolRoot, { recursive: true, force: true });

      const files = {};
      for (const label of ["A", "B"]) {
        const side = orientation.labels[label];
        const outDir = path.join(poolRoot, label);
        await mkdir(outDir, { recursive: true });
        for (const [index, width] of widths.entries()) {
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
        throw new Error(`${orientation.key} panel has ${imageCount} images, over the ${MAX_IMAGES_PER_CALL} limit`);
      }

      mapping.categories[orientation.key] = {
        labels: orientation.labels,
        files,
        ...(orientation.swapOf ? { swap_of: orientation.swapOf } : {}),
      };

      const answerPath = path.join(dir, "answer.json");
      const pools = ["A", "B"]
        .map((label) => `${label}:\n${widths.map((_, index) => `  ${path.join(poolRoot, label, `${String(index + 1).padStart(2, "0")}.png`)}`).join("\n")}`)
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
        orientation: orientation.swapOf ? "swapped" : "normal",
        images: imageCount,
        widths,
        pool: poolRoot,
        answer: answerPath,
      }, null, 2)}\n`);

      built.push({ category: orientation.key, dir, answerPath, imageCount });
    }
  }

  await writeFile(mapPath, `${JSON.stringify(mapping, null, 2)}\n`);
  return { built, mapPath, aspirational: aspirational.length, scored: scored.length };
}

// A panel must not change under a judge while it reads. The shots directory is
// live, so every image a panel names is copied into the panel directory first
// and the prompt points at the copy.
async function freezeImages(entries, panelDir) {
  for (const entry of entries) {
    const name = `${entry.state}@${entry.width}.${entry.platform}.png`;
    const target = path.join(panelDir, name);
    await copyFile(entry.absolute, target);
    entry.panelPath = target;
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const entries = await discoverShots(args.platforms);
  if (!entries.length) throw new Error("no shots found; run npm run shots first");

  if (args.role === "comparative") {
    const policy = await comparativePolicy();
    const categories = args.category === "worst"
      ? await worstCategories(args.round, Math.min(policy.max_categories_per_round || 2, HARD_CATEGORY_CAP))
      : args.category.split(",").map((value) => value.trim()).filter(Boolean);
    const result = await buildComparative({ round: args.round, categories, entries, widths: args.widths || DEFAULT_PANEL_WIDTHS });
    console.log(`role: comparative · round ${args.round} · ${categories.length} category call(s)`);
    for (const call of result.built) {
      console.log(`  ${call.category}: ${call.imageCount} images → ${call.dir}`);
    }
    console.log(`map: ${result.mapPath}`);
    console.log(`scored references: ${result.scored}; aspiration-only files excluded: ${result.aspirational}`);
    return;
  }

  const selected = entries.filter((entry) =>
    entry.width === args.viewport && (!args.states || args.states.includes(entry.state)));
  if (!selected.length) throw new Error(`no ${args.viewport} shots; run npm run shots first`);
  if (args.states) {
    const missing = args.states.filter((name) => !selected.some((entry) => entry.state === name));
    if (missing.length) throw new Error(`no ${args.viewport} shots for: ${missing.join(", ")}`);
  }
  if (selected.length > MAX_IMAGES_PER_CALL) {
    throw new Error(`${args.role} would carry ${selected.length} images, over the ${MAX_IMAGES_PER_CALL} cap; split the call further`);
  }

  // --tag splits one round into panels: a call may carry at most ten images,
  // and a panel of four is cheap enough for a judge to read carefully.
  const panelName = `${args.role}-${args.viewport}${args.tag ? `-${args.tag}` : ""}`;
  const inbox = path.join(ROOT, "inbox", `round-${args.round}`, panelName);
  await mkdir(inbox, { recursive: true });
  const answerPath = path.join(inbox, "answer.json");
  const template = await readFile(path.join(ROOT, "prompts", `${args.role}.md`), "utf8");
  await freezeImages(selected, inbox);

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
