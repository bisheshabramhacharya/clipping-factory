// Merge several absolute-pass panels into one round.
//
//   node judges/aggregate-panels.mjs --round 5
//
// One call may carry ten images, which is more than a careful read wants, so a
// round is split into panels of two or three screens. Each panel still scores
// all eight categories; this merges them by median and keeps every individual
// score next to the panel that gave it, so a category that disagrees with
// itself is visible instead of averaged away.
//
// Writes rounds/round-<n>.json and prints the table.

import { readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORY_KEYS } from "./categories.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const RIG = path.resolve(ROOT, "..");
const INBOX = path.join(ROOT, "inbox");
const ROUNDS = path.join(RIG, "rounds");

function parseArgs(argv) {
  const args = { round: 5 };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
  }
  return args;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  if (!sorted.length) return null;
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const root = path.join(INBOX, `round-${args.round}`);
  const dirs = (await readdir(root, { withFileTypes: true }).catch(() => []))
    .filter((entry) => entry.isDirectory() && entry.name.startsWith("absolute-"))
    .map((entry) => entry.name)
    .sort();

  const panels = [];
  for (const dir of dirs) {
    let answer;
    try {
      answer = JSON.parse(await readFile(path.join(root, dir, "answer.json"), "utf8"));
    } catch {
      continue;
    }
    const images = JSON.parse(await readFile(path.join(root, dir, "images.json"), "utf8").catch(() => "[]"));
    panels.push({ panel: dir, images: Array.isArray(images) ? images.map((image) => image.state || image) : [], answer });
  }
  if (!panels.length) throw new Error(`no answers under ${root}`);

  const categories = {};
  for (const key of CATEGORY_KEYS) {
    const entries = panels
      .map((panel) => ({ panel: panel.panel, ...(panel.answer.categories?.[key] || {}) }))
      .filter((entry) => typeof entry.score === "number");
    const scores = entries.map((entry) => entry.score);
    categories[key] = {
      median: median(scores),
      low: scores.length ? Math.min(...scores) : null,
      high: scores.length ? Math.max(...scores) : null,
      calls: entries.map((entry) => ({ panel: entry.panel.replace("absolute-", ""), score: entry.score, evidence: entry.evidence || "" })),
    };
  }

  const issues = panels.flatMap((panel) =>
    (panel.answer.top_issues || []).map((issue) => ({ ...issue, panel: panel.panel.replace("absolute-", "") })));
  const notes = panels.map((panel) => ({ panel: panel.panel.replace("absolute-", ""), notes: panel.answer.notes || "" }));

  const record = {
    round: args.round,
    panels: panels.map((panel) => ({ panel: panel.panel, screens: panel.images, role: panel.answer.role })),
    categories,
    panel_medians: Object.fromEntries(Object.entries(categories).map(([key, value]) => [key, value.median])),
    issues,
    notes,
  };
  await writeFile(path.join(ROUNDS, `round-${args.round}.json`), `${JSON.stringify(record, null, 2)}\n`);

  console.log(`round ${args.round} · ${panels.length} panels: ${panels.map((panel) => panel.panel.replace("absolute-", "")).join(", ")}\n`);
  console.log("| category | median | range | per panel |");
  console.log("|---|---|---|---|");
  for (const key of CATEGORY_KEYS) {
    const value = categories[key];
    const calls = value.calls.map((call) => `${call.score} ${call.panel}`).join(" · ");
    console.log(`| ${key} | ${value.median ?? "-"} | ${value.low ?? "-"}–${value.high ?? "-"} | ${calls} |`);
  }
  const medians = Object.values(categories).map((value) => value.median).filter((value) => typeof value === "number");
  console.log(`\nmedian of medians: ${median(medians)} · lowest category: ${Object.entries(categories).sort((a, b) => (a[1].median ?? 99) - (b[1].median ?? 99))[0][0]}`);
  console.log(`\n${issues.length} issues cited; record at rounds/round-${args.round}.json`);
}

await main();
