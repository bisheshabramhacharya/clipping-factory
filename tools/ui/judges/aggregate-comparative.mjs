// Fold one round's blind comparative calls into rounds/round-<n>/comparative.json
// and print the verdict table.
//
//   node judges/aggregate-comparative.mjs --round 1
//
// Each category is one blind pair, so the result is a verdict per category, not
// a score: win, loss or tie against the real reference window. A category with
// no answer and no recorded failure is listed as not judged, never as a win.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORIES } from "./categories.mjs";

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

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const roundDir = path.join(ROUNDS, `round-${args.round}`);
  const map = await readJson(path.join(roundDir, "comparative-map.json"));
  if (!map) throw new Error(`missing ${path.join(roundDir, "comparative-map.json")}; run judges/prepare.mjs --role comparative first`);

  const manifest = await readJson(path.join(REPO, "tools", "ui", "reference", "manifest.json"));
  const sourceOfFile = new Map((manifest?.files || []).map((file) => [`reference/${file.file}`, file.source]));

  const categories = {};
  const tally = { win: 0, loss: 0, tie: 0, not_judged: 0 };
  let spentUsd = 0;
  let calls = 0;

  for (const category of CATEGORIES) {
    const panel = map.categories?.[category.key];
    if (!panel) {
      categories[category.key] = { label: category.label, verdict: "not_judged" };
      tally.not_judged += 1;
      continue;
    }
    const answer = await readJson(path.join(roundDir, "comparative", category.key, "answer.json"));
    const failed = await readJson(path.join(roundDir, "comparative", category.key, "failed.json"));
    const call = await readJson(path.join(roundDir, "comparative", category.key, "call.json"));
    for (const attempt of call?.attempts || []) {
      calls += 1;
      spentUsd += Number(attempt.cost_usd || 0);
    }
    if (!answer) {
      categories[category.key] = {
        label: category.label,
        verdict: "not_judged",
        failure: failed?.reason || null,
      };
      tally.not_judged += 1;
      continue;
    }

    const studioLabel = Object.entries(panel.labels).find(([, side]) => side === "studio")?.[0];
    const referenceLabel = Object.entries(panel.labels).find(([, side]) => side === "reference")?.[0];
    const verdict = answer.winner === "tie" ? "tie" : answer.winner === studioLabel ? "win" : "loss";
    tally[verdict] += 1;

    const referenceFile = Object.entries(panel.files).find(([, file]) => file.side === "reference")?.[1];
    categories[category.key] = {
      label: category.label,
      verdict,
      winner_label: answer.winner,
      studio_label: studioLabel,
      reference_label: referenceLabel,
      reference_source: sourceOfFile.get(referenceFile?.origin) || "reference",
      citation: answer.citation,
      why: answer.why,
      confidence: answer.confidence,
      judge: call?.judge || null,
      model: call?.attempts?.at(-1)?.model || null,
    };
  }

  const result = {
    round: args.round,
    categories,
    summary: tally,
    cost: { usd: Math.round(spentUsd * 10000) / 10000, calls },
  };
  await mkdir(roundDir, { recursive: true });
  await writeFile(path.join(roundDir, "comparative.json"), `${JSON.stringify(result, null, 2)}\n`);

  console.log(`round ${args.round} · blind comparative`);
  for (const category of CATEGORIES) {
    const record = categories[category.key];
    const verdict = record.verdict === "not_judged" ? "NOT JUDGED" : record.verdict.toUpperCase();
    console.log(`  ${category.label.padEnd(30)} ${verdict.padEnd(11)} ${record.citation ? `"${record.citation.slice(0, 90)}…"` : record.failure || ""}`);
  }
  console.log(`wins: ${tally.win} · losses: ${tally.loss} · ties: ${tally.tie} · not judged: ${tally.not_judged}`);
  console.log(`judge calls: ${calls} · cost: $${result.cost.usd}`);
  console.log(`wrote ${path.join(roundDir, "comparative.json")}`);
}

await main();
