// Fold one round's raw judge answers and deterministic checks into
// rounds/round-<n>.json and rounds/round-<n>.md.
//
//   node judges/aggregate.mjs --round 0
//
// The median is computed across the judges that actually scored the studio
// (absolute + comparative). The defect hunter does not score, so it is not in
// the median. Reference gaps come from the comparative pass only, because
// those sources were scored in the same session on the same scale.

import { readFile, writeFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORIES } from "./categories.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(ROOT, "..", "..", "..");
const ROUNDS = path.join(REPO, "tools", "ui", "rounds");
const SOURCE_LABEL = { "t3-code": "T3 Code", "opus-clip": "Opus Clip", descript: "Descript", studio: "This studio" };

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

function poolKey(poolPath) {
  const parts = String(poolPath || "").split(path.sep).join("/").split("/");
  return parts.slice(-2).join("/");
}

function stateFromPool(map, poolPath) {
  const record = map.files?.[poolKey(poolPath)];
  return record?.state || "cross-screen";
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const dir = path.join(ROUNDS, `round-${args.round}`);
  const [absolute, comparative, defects, map, deterministic] = await Promise.all([
    readJson(path.join(dir, "raw", "absolute.json")),
    readJson(path.join(dir, "raw", "comparative.json")),
    readJson(path.join(dir, "raw", "defect-hunter.json")),
    readJson(path.join(dir, "comparative-map.json")),
    readJson(path.join(dir, "deterministic.json")),
  ]);
  if (!absolute || !comparative || !defects) {
    throw new Error(`round ${args.round} needs absolute.json, comparative.json and defect-hunter.json in ${path.join(dir, "raw")}`);
  }
  if (!map) throw new Error(`missing ${path.join(dir, "comparative-map.json")}; rerun judges/prepare.mjs --role comparative`);

  const studioLabel = Object.entries(map.labels).find(([, source]) => source === "studio")?.[0];
  if (!studioLabel) throw new Error("comparative map has no studio label");

  const scores = {};
  for (const category of CATEGORIES) {
    const absoluteScore = absolute.categories[category.key].score;
    const comparativeScore = comparative.sources[studioLabel].categories[category.key].score;
    scores[category.key] = {
      label: category.label,
      absolute: absoluteScore,
      comparative: comparativeScore,
      median: median([absoluteScore, comparativeScore]),
    };
  }

  const references = [...new Set(Object.values(map.labels).filter((source) => source !== "studio"))];
  const referenceGap = {};
  for (const reference of references) {
    const label = Object.entries(map.labels).find(([, source]) => source === reference)[0];
    referenceGap[reference] = {};
    for (const category of CATEGORIES) {
      const ours = comparative.sources[studioLabel].categories[category.key].score;
      const theirs = comparative.sources[label].categories[category.key].score;
      referenceGap[reference][category.key] = Math.round((ours - theirs) * 100) / 100;
    }
  }

  const findings = [];
  for (const issue of absolute.top_issues || []) {
    findings.push({
      role: "absolute",
      state: issue.state,
      viewport: issue.viewport,
      expected: issue.expected,
      actual: issue.actual,
      severity: null,
    });
  }
  for (const defect of defects.defects || []) {
    findings.push({
      role: "defect-hunter",
      state: defect.state,
      viewport: defect.viewport,
      expected: defect.expected,
      actual: defect.actual,
      severity: defect.severity,
      kind: defect.kind,
    });
  }
  const strengths = [];
  for (const pair of comparative.pairs || []) {
    if (pair.lower === studioLabel) {
      findings.push({
        role: "comparative",
        state: stateFromPool(map, pair.screens?.[0]),
        viewport: "cross-screen",
        expected: `match ${SOURCE_LABEL[map.labels[pair.higher]] || map.labels[pair.higher]} (${pair.higher})`,
        actual: pair.note,
        severity: null,
        category: pair.category,
      });
    } else if (pair.higher === studioLabel) {
      strengths.push({
        state: stateFromPool(map, pair.screens?.[1]),
        category: pair.category,
        note: pair.note,
      });
    }
  }

  const byState = {};
  for (const finding of findings) {
    const key = finding.state || "unknown";
    byState[key] = byState[key] || [];
    byState[key].push(finding);
  }

  const confirmation = {};
  for (const [state, rows] of Object.entries(byState)) {
    confirmation[state] = {
      absolute: rows.filter((row) => row.role === "absolute").length,
      comparative: rows.filter((row) => row.role === "comparative").length,
      defect_hunter: rows.filter((row) => row.role === "defect-hunter").length,
      roles: [...new Set(rows.map((row) => row.role))],
    };
  }

  const platforms = [...new Set(
    Object.values(map.files).filter((file) => file.source === "studio").map((file) => file.platform),
  )].sort();

  const round = {
    round: args.round,
    platforms,
    judges: {
      absolute: { notes: absolute.notes, top_issues: absolute.top_issues },
      comparative: {
        notes: comparative.notes,
        studio_label: studioLabel,
        sources: comparative.sources,
        pairs: comparative.pairs,
      },
      defect_hunter: { notes: defects.notes, defects: defects.defects },
    },
    scores,
    reference_gap: referenceGap,
    findings,
    strengths,
    confirmation,
    deterministic: deterministic || null,
    comparative_map: map,
  };

  await mkdir(dir, { recursive: true });
  await writeFile(path.join(ROUNDS, `round-${args.round}.json`), `${JSON.stringify(round, null, 2)}\n`);

  const lines = [];
  const missingPlatforms = ["macos", "windows"].filter((platform) => !platforms.includes(platform));
  lines.push(`# Round ${args.round} — design review`);
  lines.push("");
  lines.push(`Platforms: ${platforms.join(", ") || "none"}${missingPlatforms.length ? ` · not present: ${missingPlatforms.join(", ")}` : ""}.`);
  lines.push("");
  lines.push(`${absolute.top_issues?.length || 0} absolute issues, ${defects.defects?.length || 0} hunted defects, ${findings.length} findings after mapping the comparative losses.`);
  lines.push("");
  lines.push("## Scores");
  lines.push("");
  lines.push("| Category | Absolute | Comparative | Median | " + references.map((reference) => SOURCE_LABEL[reference] || reference).join(" | ") + " |");
  lines.push("| --- | ---: | ---: | ---: | " + references.map(() => "---:").join(" | ") + " |");
  for (const category of CATEGORIES) {
    const row = scores[category.key];
    const gaps = references.map((reference) => {
      const gap = referenceGap[reference][category.key];
      return gap === 0 ? "0" : gap > 0 ? `+${gap}` : `${gap}`;
    });
    lines.push(`| ${category.label} | ${row.absolute} | ${row.comparative} | **${row.median}** | ${gaps.join(" | ")} |`);
  }
  lines.push("");
  lines.push("Reference columns are gaps: the comparative judge's score for this studio minus its score for that reference product. Positive means the reference was scored lower.");
  lines.push("");
  lines.push("## Deterministic checks");
  lines.push("");
  if (deterministic) {
    lines.push(`- ${deterministic.wcag?.count ?? 0} WCAG AA pairs (${deterministic.wcag?.unique ?? 0} unique)`);
    lines.push(`- Distinct values: ${deterministic.distinct_values?.radius ?? "?"} border-radius, ${deterministic.distinct_values?.font_sizes ?? "?"} font-size, ${deterministic.distinct_values?.shadows ?? "?"} shadow, ${deterministic.distinct_values?.colors ?? "?"} color`);
    lines.push(`- Horizontal overflow: ${deterministic.overflow?.count ?? 0} shots`);
    lines.push(`- Clipped text: ${deterministic.clipped_text?.count ?? 0} elements`);
    const unique = new Map();
    for (const failure of deterministic.wcag?.failures || []) {
      const key = `${failure.selector}|${failure.foreground}|${failure.background}|${failure.required}`;
      if (!unique.has(key)) unique.set(key, { ...failure, states: new Set() });
      unique.get(key).states.add(`${failure.state}@${failure.viewport}`);
    }
    if (unique.size) {
      lines.push("");
      lines.push("Unique WCAG AA failures:");
      lines.push("");
      for (const failure of unique.values()) {
        const states = [...failure.states];
        lines.push(`- \`${failure.selector}\` — ${failure.foreground} on ${failure.background} = ${failure.ratio}:1 (needs ${failure.required}:1), ${states.length} shot${states.length === 1 ? "" : "s"}, first ${states[0]}`);
      }
    }
  } else {
    lines.push("No deterministic report filed for this round.");
  }
  lines.push("");
  lines.push("## Findings");
  lines.push("");
  for (const state of Object.keys(byState).sort()) {
    lines.push(`### ${state}`);
    lines.push("");
    for (const finding of byState[state]) {
      const where = finding.viewport && finding.viewport !== "cross-screen" ? `@${finding.viewport}` : "";
      const severity = finding.severity ? ` (${finding.severity})` : "";
      lines.push(`- **${finding.role}${severity}** ${where}: expected ${finding.expected} — actual ${finding.actual}`);
    }
    lines.push("");
  }
  lines.push("## Comparative evidence on this studio");
  lines.push("");
  const lostPairs = (comparative.pairs || []).filter((pair) => pair.lower === studioLabel);
  if (!lostPairs.length) {
    lines.push(`The comparative judge recorded no pairwise loss: it scored ${SOURCE_LABEL.studio} at or above every reference in all eight categories. Its category evidence still names weaknesses, quoted verbatim below.`);
  } else {
    lines.push(`The comparative judge recorded ${lostPairs.length} pairwise loss${lostPairs.length === 1 ? "" : "es"} where ${SOURCE_LABEL.studio} scored below a reference; they are in the findings above.`);
  }
  lines.push("");
  for (const category of CATEGORIES) {
    const evidence = comparative.sources[studioLabel].categories[category.key].evidence;
    lines.push(`- **${category.label}** (${comparative.sources[studioLabel].categories[category.key].score}): ${evidence}`);
  }
  lines.push("");
  if (strengths.length) {
    lines.push("## Where this studio scored above a reference");
    lines.push("");
    for (const strength of strengths) {
      lines.push(`- ${strength.category}: ${strength.note}`);
    }
    lines.push("");
  }
  lines.push("## Confirmation");
  lines.push("");
  lines.push("A finding raised by two or more roles is treated as real; a single-role finding stays a lead until a second role confirms it.");
  lines.push("");
  lines.push("| State | Absolute | Comparative | Defect hunter | Roles |");
  lines.push("| --- | ---: | ---: | ---: | --- |");
  for (const [state, row] of Object.entries(confirmation).sort()) {
    lines.push(`| ${state} | ${row.absolute} | ${row.comparative} | ${row.defect_hunter} | ${row.roles.join(", ")} |`);
  }
  lines.push("");
  lines.push("## Judge notes");
  lines.push("");
  lines.push(`**Absolute:** ${absolute.notes}`);
  lines.push("");
  lines.push(`**Comparative:** ${comparative.notes}`);
  lines.push("");
  lines.push(`**Defect hunter:** ${defects.notes}`);
  lines.push("");
  await writeFile(path.join(ROUNDS, `round-${args.round}.md`), `${lines.join("\n")}\n`);

  console.log(`round ${args.round}: ${Object.keys(scores).length} categories, ${findings.length} findings`);
  console.log(`wrote ${path.join(ROUNDS, `round-${args.round}.json`)}`);
  console.log(`wrote ${path.join(ROUNDS, `round-${args.round}.md`)}`);
}

await main();
