// The comparative-only view of one round: the citation, then the verdict, per
// category, with the order-check result and the credit cost.
//
//   node judges/aggregate-comparative.mjs --round 3
//
// judges/aggregate-round.mjs is the whole-round view; this one exists so a
// round can be read without the absolute scores in the way.

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

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const roundDir = path.join(ROUNDS, `round-${args.round}`);
  const map = await readJson(path.join(roundDir, "comparative-map.json"));
  if (!map) throw new Error(`missing ${path.join(roundDir, "comparative-map.json")}; run judges/prepare.mjs --role comparative first`);

  const categories = {};
  const tally = { win: 0, loss: 0, tie: 0, not_judged: 0 };
  let credits = 0;
  let calls = 0;

  for (const category of CATEGORIES) {
    const record = await readCategoryVerdict(roundDir, map, category.key, category.label);
    categories[category.key] = record;
    tally[record.verdict] += 1;
    for (const key of [category.key, `${category.key}-swapped`]) {
      const recorded = await readJson(path.join(roundDir, "comparative", key, "calls.json"));
      for (const call of recorded?.calls || []) {
        calls += 1;
        credits += Number(call.credits || 0);
      }
    }
  }

  const result = {
    round: args.round,
    categories,
    summary: tally,
    cost: { credits: Math.round(credits * 10000) / 10000, calls },
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
  console.log(`judge calls: ${calls} · cost: ${result.cost.credits} credits`);
  console.log(`wrote ${path.join(roundDir, "comparative.json")}`);
}

await main();
