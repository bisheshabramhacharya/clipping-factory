// Build the prompt packet for one judge role.
//
//   node judges/prepare.mjs --round 0 --role absolute
//   node judges/prepare.mjs --round 0 --role comparative
//   node judges/prepare.mjs --round 0 --role defect-hunter
//
// The comparative pass copies every screenshot into an opaque pool (A-D) and
// keeps the label -> source map in rounds/round-<n>/comparative-map.json. The
// judge never learns which pool is this repository's interface.

import { copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORIES } from "./categories.mjs";
import { STATES as FIXTURE_STATES } from "../fixtures/state.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const RIG = path.resolve(ROOT, "..");
const REPO = path.resolve(RIG, "..", "..");
const SHOT_PATTERN = /^(.+)@(\d+)\.([a-z0-9]+)\.png$/;
const STATE_ORDER = FIXTURE_STATES.map((state) => state.name);

const REFERENCES = [
  { source: "t3-code", file: "t3-code-desktop.png" },
  { source: "opus-clip", file: "opus-clip-editor.png" },
  { source: "opus-clip", file: "opus-clip-result.png" },
  { source: "opus-clip", file: "opus-clip-grid.png" },
  { source: "descript", file: "descript-editor.png" },
  { source: "descript", file: "descript-underlord-clips.png" },
  { source: "descript", file: "descript-underlord-edit.png" },
];

function parseArgs(argv) {
  const args = { round: 0, role: null, platforms: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
    else if (argv[i] === "--role") args.role = argv[++i];
    else if (argv[i] === "--platforms") args.platforms = argv[++i].split(",").map((value) => value.trim());
  }
  if (!["absolute", "comparative", "defect-hunter"].includes(args.role)) {
    throw new Error("--role must be absolute, comparative or defect-hunter");
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

async function buildComparative({ round, entries, inbox }) {
  const poolRoot = path.join(inbox, "pool");
  await rm(poolRoot, { recursive: true, force: true });
  await mkdir(poolRoot, { recursive: true });

  const sources = ["studio", "t3-code", "opus-clip", "descript"];
  const order = shuffle(sources, 0xc0ffee + round * 7919);
  const labels = ["A", "B", "C", "D"];
  const labelOf = Object.fromEntries(order.map((source, index) => [source, labels[index]]));

  const mapping = { round, labels: {}, files: {} };
  for (const [source, label] of Object.entries(labelOf)) mapping.labels[label] = source;

  for (const label of labels) {
    const dir = path.join(poolRoot, label);
    await mkdir(dir, { recursive: true });
    const source = mapping.labels[label];
    const picks = source === "studio"
      ? entries
      : REFERENCES.filter((reference) => reference.source === source);
    let index = 0;
    for (const pick of picks) {
      index += 1;
      const name = `${String(index).padStart(2, "0")}.png`;
      if (source === "studio") {
        await copyFile(pick.absolute, path.join(dir, name));
        mapping.files[`${label}/${name}`] = {
          source: "studio",
          state: pick.state,
          viewport: String(pick.width),
          platform: pick.platform,
          origin: pick.relative,
        };
      } else {
        await copyFile(path.join(RIG, "reference", pick.file), path.join(dir, name));
        mapping.files[`${label}/${name}`] = {
          source,
          reference: pick.file,
          origin: path.join("reference", pick.file),
        };
      }
    }
  }

  const pools = labels.map((label) => {
    const files = Object.entries(mapping.files)
      .filter(([key]) => key.startsWith(`${label}/`))
      .map(([key]) => path.join(poolRoot, key));
    return `${label}:\n${files.map((file) => `  ${file}`).join("\n")}`;
  }).join("\n\n");

  return { labelOf, mapping, pools };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const entries = await discoverShots(args.platforms);
  if (!entries.length) throw new Error("no shots found; run npm run shots first");

  const inbox = path.join(ROOT, "inbox", `round-${args.round}`, args.role);
  await mkdir(inbox, { recursive: true });
  const answerPath = path.join(inbox, "answer.json");
  const template = await readFile(path.join(ROOT, "prompts", `${args.role}.md`), "utf8");

  let images = imageList(entries);
  let pools = "";
  if (args.role === "comparative") {
    const built = await buildComparative({ round: args.round, entries, inbox });
    pools = built.pools;
    const roundDir = path.join(REPO, "tools", "ui", "rounds", `round-${args.round}`);
    await mkdir(roundDir, { recursive: true });
    await writeFile(
      path.join(roundDir, "comparative-map.json"),
      `${JSON.stringify({ ...built.mapping, label_of: built.labelOf }, null, 2)}\n`,
    );
  }

  const prompt = template
    .replaceAll("{{ROUND}}", String(args.round))
    .replaceAll("{{RUBRIC}}", path.join(RIG, "rubric.md"))
    .replaceAll("{{IMAGES}}", images)
    .replaceAll("{{POOLS}}", pools)
    .replaceAll("{{ANSWER}}", answerPath);

  await writeFile(path.join(inbox, "prompt.md"), prompt);
  await writeFile(path.join(inbox, "images.json"), `${JSON.stringify({
    role: args.role,
    round: args.round,
    shots: entries.map((entry) => ({ ...entry, absolute: undefined })),
  }, null, 2)}\n`);

  const words = CATEGORIES.length;
  console.log(`role: ${args.role}`);
  console.log(`prompt: ${path.join(inbox, "prompt.md")}`);
  console.log(`answer: ${answerPath}`);
  console.log(`images: ${entries.length} shots, ${words} categories`);
  if (args.role === "comparative") console.log(`pool: ${path.join(inbox, "pool")} (labels A-D, map in rounds/round-${args.round}/comparative-map.json)`);
}

await main();
