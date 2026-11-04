// Fold one round of the current method into rounds/round-<n>.json and .md.
//
//   node judges/aggregate-round.mjs --round 2
//
// Inputs, all optional except one:
//   raw/absolute-<width>.json   one per viewport, nine shots each
//   raw/defect-hunter-<width>.json
//   comparative/<category>/answer.json  one blind pair per category
//   deterministic.json          from npm run checks
//
// The absolute score per category is the median across viewports, so no single
// width can carry the round. The comparative verdict per category comes from
// the blind pair; a category with no answer is not_judged, never a win.
// judges/aggregate.mjs still reproduces round 0's four-source method.

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

function median(values) {
  const nums = values.filter((value) => typeof value === "number").sort((a, b) => a - b);
  if (!nums.length) return null;
  const middle = Math.floor(nums.length / 2);
  return nums.length % 2 ? nums[middle] : Math.round(((nums[middle - 1] + nums[middle]) / 2) * 100) / 100;
}

async function readViewportParts(dir, role) {
  const widths = [1280, 1440, 1920];
  const parts = [];
  for (const width of widths) {
    const value = await readJson(path.join(dir, `${role}-${width}.json`));
    if (value) parts.push({ width, value });
  }
  const single = await readJson(path.join(dir, `${role}.json`));
  if (single) parts.push({ width: null, value: single });
  return parts;
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
    const values = Object.values(perWidth).map((entry) => entry.score);
    scores[category.key] = { label: category.label, per_viewport: perWidth, median: median(values) };
  }

  const verdicts = {};
  const tally = { win: 0, loss: 0, tie: 0, not_judged: 0 };
  for (const category of CATEGORIES) {
    const panel = map?.categories?.[category.key];
    const answer = panel ? await readJson(path.join(dir, "comparative", category.key, "answer.json")) : null;
    const failed = panel ? await readJson(path.join(dir, "comparative", category.key, "failed.json")) : null;
    const call = panel ? await readJson(path.join(dir, "comparative", category.key, "call.json")) : null;
    if (!answer) {
      verdicts[category.key] = { label: category.label, verdict: "not_judged", failure: failed?.reason || null };
      tally.not_judged += 1;
      continue;
    }
    const studioLabel = Object.entries(panel.labels).find(([, side]) => side === "studio")?.[0];
    const verdict = answer.winner === "tie" ? "tie" : answer.winner === studioLabel ? "win" : "loss";
    tally[verdict] += 1;
    verdicts[category.key] = {
      label: category.label,
      verdict,
      winner_label: answer.winner,
      studio_label: studioLabel,
      citation: answer.citation,
      why: answer.why,
      confidence: answer.confidence,
      judge: call?.judge || null,
      model: call?.attempts?.at(-1)?.model || null,
    };
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
    scores,
    verdicts,
    tally,
    findings,
    deterministic: deterministic || null,
  };
  await mkdir(dir, { recursive: true });
  await writeFile(path.join(ROUNDS, `round-${args.round}.json`), `${JSON.stringify(round, null, 2)}\n`);

  const lines = [`# Round ${args.round} — design review`, ""];
  if (round.viewports.length) {
    lines.push(`## Absolute score by category`, "");
    lines.push(`Viewports judged: ${round.viewports.join(", ")}. A category's score is the median across them.`, "");
    lines.push("| Category | " + round.viewports.map((width) => `${width}`).join(" | ") + " | Median |");
    lines.push("| --- |" + round.viewports.map(() => " ---:").join("") + " ---: |");
    for (const category of CATEGORIES) {
      const row = scores[category.key];
      const cells = round.viewports.map((width) => row.per_viewport[width]?.score ?? "—");
      lines.push(`| ${category.label} | ${cells.join(" | ")} | **${row.median ?? "—"}** |`);
    }
    lines.push("");
  }
  lines.push("## Blind comparison");
  lines.push("");
  lines.push("One call per category, blind pair, one winner with a citation or a tie.");
  lines.push("");
  lines.push("| Category | Verdict | Confidence |");
  lines.push("| --- | --- | --- |");
  for (const category of CATEGORIES) {
    const row = verdicts[category.key];
    lines.push(`| ${category.label} | ${row.verdict === "not_judged" ? "not judged" : row.verdict} | ${row.confidence || (row.failure ? "failed" : "—")} |`);
  }
  lines.push("");
  lines.push(`Wins ${tally.win} · losses ${tally.loss} · ties ${tally.tie} · not judged ${tally.not_judged}`, "");
  for (const category of CATEGORIES) {
    const row = verdicts[category.key];
    if (row.verdict === "not_judged") {
      if (row.failure) lines.push(`- **${category.label}**: not judged, ${row.failure}`);
      continue;
    }
    lines.push(`### ${category.label} — ${row.verdict} (${row.confidence})`, "");
    lines.push(`Citation, winner is side ${row.winner_label}:`, "", `> ${row.citation}`, "");
    lines.push(`Why: ${row.why}`, "");
    if (row.model) lines.push(`Judge: ${row.judge}, ${row.model}.`, "");
  }
  lines.push(`Findings: ${findings.length}.`);
  await writeFile(path.join(ROUNDS, `round-${args.round}.md`), `${lines.join("\n")}\n`);

  console.log(`round ${args.round}: ${round.viewports.length} viewport(s), ${tally.win} wins / ${tally.loss} losses / ${tally.tie} ties / ${tally.not_judged} not judged`);
  console.log(`wrote ${path.join(ROUNDS, `round-${args.round}.json`)} and .md`);
}

await main();
