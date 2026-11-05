// Build one blind panel of products, hand it to judges, and file their answers.
//
//   node judges/panel.mjs --stage  --spec judges/panels/round-13.json
//   node judges/panel.mjs --collect --spec judges/panels/round-13.json --judge 1
//   node judges/panel.mjs --report  --spec judges/panels/round-13.json
//
// A panel is a set of products shown as the same screens. Every product is
// copied under an opaque code, the order is shuffled once with the round as the
// seed, and the code map is written outside the judges' input directory. Every
// judge sees every product, scores all eight categories, and gets a different
// lens. Nothing in a prompt names the product under construction, the target,
// past scores, or that any redesign is happening.

import { copyFile, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORIES, CATEGORY_KEYS } from "./categories.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const RIG = path.resolve(ROOT, "..");
const REPO = path.resolve(RIG, "..", "..");
const ROUNDS = path.join(REPO, "tools", "ui", "rounds");

function parseArgs(argv) {
  const args = { stage: false, collect: false, report: false, spec: null, judge: null, out: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--stage") args.stage = true;
    else if (arg === "--collect") args.collect = true;
    else if (arg === "--report") args.report = true;
    else if (arg === "--spec") args.spec = argv[++i];
    else if (arg === "--judge") args.judge = Number(argv[++i]);
    else if (arg === "--judges") args.judges = Number(argv[++i]);
    else if (arg === "--out") args.out = argv[++i];
  }
  if (!args.spec) throw new Error("--spec <panel.json> is required");
  if (!args.stage && !args.collect && !args.report) throw new Error("pass --stage, --collect or --report");
  return args;
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

export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  if (!sorted.length) return null;
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

const readJson = async (file) => JSON.parse(await readFile(file, "utf8"));

async function loadPolicy() {
  const policy = await readJson(path.join(ROOT, "judges.json"));
  if (!policy?.panel) throw new Error("judges/judges.json has no panel policy");
  return policy.panel;
}

async function stage({ spec, policy, out }) {
  const panelDir = out ? path.resolve(out) : path.join(ROOT, "inbox", `round-${spec.round}`, "panel");
  const imagesDir = path.join(panelDir, "images");
  await rm(imagesDir, { recursive: true, force: true });
  await mkdir(imagesDir, { recursive: true });

  const live = spec.products.filter((product) => Object.keys(product.files || {}).length > 0);
  const order = shuffle(live, hashKey(`order-${spec.round}`));
  const codeFor = new Map(order.map((product, index) => [product.code, `P${index + 1}`]));
  const productFor = new Map(order.map((product) => [codeFor.get(product.code), product]));

  const map = { round: spec.round, built: new Date().toISOString(), codes: {} };
  for (const product of order) {
    const code = codeFor.get(product.code);
    map.codes[code] = { source: product.code, label: product.label, note: product.note || null };
    for (const [stateKey, file] of Object.entries(product.files)) {
      const stateDir = path.join(imagesDir, stateKey);
      await mkdir(stateDir, { recursive: true });
      await copyFile(path.resolve(REPO, file), path.join(stateDir, `${code}.png`));
    }
  }
  const mapPath = path.join(ROUNDS, `round-${spec.round}`, "panel-map.json");
  await mkdir(path.dirname(mapPath), { recursive: true });
  await writeFile(mapPath, `${JSON.stringify(map, null, 2)}\n`);

  const codes = [...productFor.keys()].sort();
  const states = spec.states.filter((state) => [...productFor.values()].some((product) => product.files?.[state.key]));
  const rubric = await readFile(path.join(RIG, "rubric.md"), "utf8");

  const judgeCount = args.judges || policy.lenses.length;
  for (let index = 0; index < judgeCount; index += 1) {
    const lens = policy.lenses[index % policy.lenses.length];
    const imageList = states
      .map((state) => {
        const files = codes
          .filter((code) => productFor.get(code).files?.[state.key])
          .map((code) => `- ${imagesDir}/${state.key}/${code}.png`);
        return `**${state.key}**: ${state.label}\n\n${files.join("\n")}`;
      })
      .join("\n\n");

    const shape = JSON.stringify({
      judge: lens.key,
      products: Object.fromEntries([["P1", Object.fromEntries(CATEGORY_KEYS.map((key) => [key, 0]))]]),
      issues: [{ product: "P1", state: states[0]?.key || "results", category: "typography", issue: "one checkable sentence with the location" }],
      notes: "one or two sentences, including anything the panel could not show you",
    }, null, 2);

    const prompt = `# Blind panel, round ${spec.round}

You are ${lens.title.toLowerCase()} judging a blind panel.

${lens.text}

## What you are looking at

${codes.length} products, each shown as the same set of screens. The codes carry no meaning and the order is shuffled: P1 is not necessarily the first product anyone would list. You are not told which product is which, who made any of them, or what any of them is competing for. Do not try to guess. Score what you see.

States in this panel:

${states.map((state) => `- **${state.key}**: ${state.label}`).join("\n")}

## The rubric

${rubric}

## Images

${imageList}

## What to return

Write one JSON object to \`answer-${index + 1}.json\` in this directory. Reply with nothing else, and keep the reply under ${policy.max_reply_tokens} tokens.

${shape}

Every product code appears in \`products\` with all eight categories. Scores are 1 to 10, half points allowed. At most three issues per product, ranked by what they cost that product, so every product gets its own three. Do not write any other file.

Do not spawn subagents or helpers. Do not read \`rounds/\`, any map file, or anything outside this panel directory.
`;

    const judgeDir = path.join(panelDir, `judge-${index + 1}`);
    await mkdir(judgeDir, { recursive: true });
    await writeFile(path.join(judgeDir, "prompt.md"), prompt);
    await rm(path.join(judgeDir, `answer-${index + 1}.json`), { force: true });
  }

  console.log(`staged round ${spec.round}: ${codes.length} products, ${states.length} states, ${judgeCount} judges`);
  console.log(`panel: ${panelDir}`);
  console.log(`map:   ${mapPath}`);
}

async function collect({ spec, policy, judge, out }) {
  const lens = policy.lenses[(judge - 1) % policy.lenses.length];
  if (!lens) throw new Error(`no lens ${judge}`);
  const panelDir = out ? path.resolve(out) : path.join(ROOT, "inbox", `round-${spec.round}`, "panel");
  const answer = await readJson(path.join(panelDir, `judge-${judge}`, `answer-${judge}.json`));
  if (answer.judge !== lens.key) throw new Error(`answer names judge ${answer.judge}, expected ${lens.key}`);
  const codes = Object.keys(answer.products || {});
  if (codes.length < 2) throw new Error("fewer than two products in the answer");
  for (const [code, scores] of Object.entries(answer.products)) {
    for (const key of CATEGORY_KEYS) {
      const value = scores[key];
      if (typeof value !== "number" || value < 1 || value > 10) throw new Error(`${code}.${key} is not a score between 1 and 10`);
    }
  }
  const perProduct = new Map();
  answer.issues = (answer.issues || []).filter((issue) => {
    const count = perProduct.get(issue.product) || 0;
    if (count >= 3) return false;
    perProduct.set(issue.product, count + 1);
    return true;
  });
  const outDir = path.join(ROUNDS, `round-${spec.round}`, "panel");
  await mkdir(outDir, { recursive: true });
  await writeFile(path.join(outDir, `${lens.key}-${judge}.json`), `${JSON.stringify(answer, null, 2)}\n`);
  console.log(`filed ${lens.key}: ${codes.length} products`);
}

async function report({ spec }) {
  const panelDir = path.join(ROUNDS, `round-${spec.round}`, "panel");
  const files = (await readdir(panelDir).catch(() => [])).filter((name) => name.endsWith(".json"));
  if (!files.length) throw new Error(`no filed answers in ${panelDir}`);
  const map = await readJson(path.join(ROUNDS, `round-${spec.round}`, "panel-map.json"));
  const answers = await Promise.all(files.map(async (name) => ({ name: name.replace(".json", ""), ...(await readJson(path.join(panelDir, name))) })));
  const label = (code) => map.codes[code]?.label || code;

  const rows = [];
  for (const code of Object.keys(map.codes).sort()) {
    const scores = answers.map((answer) => answer.products?.[code]).filter(Boolean);
    if (!scores.length) continue;
    const cells = CATEGORIES.map((category) => median(scores.map((value) => value[category.key])));
    rows.push({
      code,
      label: label(code),
      cells,
      overall: median(cells),
      perJudge: answers.map((answer) => median(CATEGORY_KEYS.map((key) => answer.products[code][key]))),
    });
  }
  rows.sort((a, b) => b.overall - a.overall);

  console.log(`| product | ${CATEGORIES.map((c) => c.label).join(" | ")} | median |`);
  console.log(`| --- | ${CATEGORIES.map(() => "---").join(" | ")} | --- |`);
  for (const row of rows) console.log(`| ${row.code} ${row.label} | ${row.cells.map((v) => v ?? "-").join(" | ")} | ${row.overall} |`);

  console.log("");
  console.log(`| product | ${answers.map((answer) => answer.judge).join(" | ")} |`);
  console.log(`| --- | ${answers.map(() => "---").join(" | ")} |`);
  for (const row of rows) console.log(`| ${row.code} ${row.label} | ${row.perJudge.map((v) => v ?? "-").join(" | ")} |`);

  const issues = answers.flatMap((answer) => (answer.issues || []).map((issue) => ({ ...issue, judge: answer.judge })));
  console.log(`\n### Issues (${issues.length})`);
  for (const issue of issues) console.log(`- ${issue.judge} | ${issue.product} ${label(issue.product)} ${issue.state} | ${issue.category} | ${issue.issue}`);
}

const args = parseArgs(process.argv.slice(2));
const policy = await loadPolicy();
const spec = await readJson(path.resolve(args.spec));
if (args.stage) await stage({ spec, policy, out: args.out });
else if (args.collect) await collect({ spec, policy, judge: args.judge, out: args.out });
else await report({ spec });
