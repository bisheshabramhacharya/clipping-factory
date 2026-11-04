// Fold one round into rounds/round-<n>.json and .md.
//
//   node judges/aggregate-round.mjs --round 3
//
// Inputs, all optional except one:
//   raw/absolute-<width>.json   one per viewport, nine shots each
//   raw/defect-hunter-<width>.json
//   comparative/<category>/answer.json        the panel as built
//   comparative/<category>-swapped/answer.json  the same pair with A and B exchanged
//   deterministic.json          from npm run checks
//
// The round file leads with citations, because a reader adjudicates evidence,
// not a score. The absolute score per category is the median across viewports.
// A comparative verdict only counts when both orientations were judged: a
// verdict that flips with position is recorded as a tie.
//
// judges/aggregate.mjs still reproduces round 0's four-source method.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORIES } from "./categories.mjs";
import { readCategoryVerdict } from "./verdict.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(ROOT, "..", "..", "..");
const ROUNDS = path.join(REPO, "tools", "ui", "rounds");

function parseArgs(argv) {
  const args = { round: 0 };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
  }
  return args;
}

async function readJson(file) {
  try {
    return JSON.parse(await readFile(file, "utf8"));
  } catch {
    return null;
  }
}

function median(values) {
  const nums = values.filter((value) => typeof value === "number").sort((a, b) => a - b);
  if (!nums.length) return null;
  const middle = Math.floor(nums.length / 2);
  return nums.length % 2 ? nums[middle] : Math.round(((nums[middle - 1] + nums[middle]) / 2) * 100) / 100;
}

async function readViewportParts(dir, role) {
  const parts = [];
  for (const width of [1280, 1440, 1920]) {
    const value = await readJson(path.join(dir, `${role}-${width}.json`));
    if (value) parts.push({ width, value });
  }
  const single = await readJson(path.join(dir, `${role}.json`));
  if (single) parts.push({ width: null, value: single });
  return parts;
}

async function callCredits(dir, key) {
  const calls = await readJson(path.join(dir, "comparative", key, "calls.json"));
  return { credits: (calls?.calls || []).reduce((sum, call) => sum + Number(call.credits || 0), 0), calls: calls?.calls?.length || 0 };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const dir = path.join(ROUNDS, `round-${args.round}`);
  const [absoluteParts, defectParts, map, deterministic] = await Promise.all([
    readViewportParts(dir, "absolute"),
    readViewportParts(dir, "defect-hunter"),
    readJson(path.join(dir, "comparative-map.json")),
    readJson(path.join(dir, "deterministic.json")),
  ]);
  if (!absoluteParts.length && !map) {
    throw new Error(`round ${args.round} has no absolute answers and no comparative map`);
  }

  const scores = {};
  for (const category of CATEGORIES) {
    const perWidth = {};
    for (const part of absoluteParts) {
      const entry = part.value.categories?.[category.key];
      if (!entry) continue;
      perWidth[part.width ?? "all"] = { score: entry.score, evidence: entry.evidence };
    }
    scores[category.key] = { label: category.label, per_viewport: perWidth, median: median(Object.values(perWidth).map((entry) => entry.score)) };
  }

  const verdicts = {};
  const tally = { win: 0, loss: 0, tie: 0, not_judged: 0 };
  let credits = 0;
  let calls = 0;
  for (const category of CATEGORIES) {
    const record = await readCategoryVerdict(dir, map, category.key, category.label);
    const normal = await callCredits(dir, category.key);
    const swapped = await callCredits(dir, `${category.key}-swapped`);
    credits += normal.credits + swapped.credits;
    calls += normal.calls + swapped.calls;
    verdicts[category.key] = record;
    tally[record.verdict] += 1;
  }

  const findings = [];
  for (const part of absoluteParts) {
    for (const issue of part.value.top_issues || []) findings.push({ role: "absolute", ...issue });
  }
  for (const part of defectParts) {
    for (const defect of part.value.defects || []) findings.push({ role: "defect-hunter", ...defect });
  }

  const round = {
    round: args.round,
    viewports: absoluteParts.map((part) => part.width).filter(Boolean),
    verdicts,
    tally,
    scores,
    findings,
    cost: { credits: Math.round(credits * 10000) / 10000, calls },
    deterministic: deterministic || null,
  };
  await mkdir(dir, { recursive: true });
  await writeFile(path.join(ROUNDS, `round-${args.round}.json`), `${JSON.stringify(round, null, 2)}\n`);

  const lines = [`# Round ${args.round}: design review`, ""];
  lines.push("## Blind comparison", "");
  const policy = await readJson(path.join(ROOT, "judges.json"));
  const scope = policy?.comparative?.max_categories_per_round ?? 2;
  lines.push(`At most ${scope} categories, one blind pair each, every pair judged twice with the sides swapped. Wins ${tally.win} · losses ${tally.loss} · ties ${tally.tie} · not judged ${tally.not_judged}. ${calls} calls, ${round.cost.credits} credits.`, "");

  for (const category of CATEGORIES) {
    const row = verdicts[category.key];
    lines.push(`### ${category.label}`, "");
    if (row.verdict === "not_judged") {
      lines.push(`Not judged: ${row.failure || "no call filed"}.`, "");
      continue;
    }
    lines.push(`Citation, from side ${row.citation?.startsWith("A") ? "A" : "B"}, confidence ${row.confidence || "unknown"}:`, "");
    lines.push(`> ${row.citation}`, "");
    if (row.why) lines.push(`Judge's reading: ${row.why}`, "");
    const order = row.order_checked
      ? (row.verdict === "tie" && row.swapped_winner ? "the verdict flipped when the panel was swapped, so it is a tie" : "survived the swap")
      : "not order checked";
    lines.push(`Verdict: **${row.verdict}**. Order check: ${order}.`, "");
  }

  if (round.viewports.length) {
    lines.push("## Absolute score by category", "");
    lines.push(`Viewports judged: ${round.viewports.join(", ")}. A category's score is the median across them.`, "");
    lines.push("| Category | " + round.viewports.map((width) => `${width}`).join(" | ") + " | Median |");
    lines.push("| --- |" + round.viewports.map(() => " ---:").join("") + " ---: |");
    for (const category of CATEGORIES) {
      const row = scores[category.key];
      lines.push(`| ${category.label} | ${round.viewports.map((width) => row.per_viewport[width]?.score ?? "n/a").join(" | ")} | **${row.median ?? "n/a"}** |`);
    }
    lines.push("");
  }
  lines.push(`Findings: ${findings.length}.`);
  await writeFile(path.join(ROUNDS, `round-${args.round}.md`), `${lines.join("\n")}\n`);

  console.log(`round ${args.round}: ${tally.win} wins / ${tally.loss} losses / ${tally.tie} ties / ${tally.not_judged} not judged, ${round.cost.credits} credits in ${calls} calls`);
  console.log(`wrote ${path.join(ROUNDS, `round-${args.round}.json`)} and .md`);
}

await main();
