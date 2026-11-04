// Validate one judge answer and file it into the round.
//
//   node judges/ingest.mjs --round 0 --role absolute --file answer.txt
//   node judges/ingest.mjs --round 1 --role comparative --category typography --file raw.txt
//   node judges/ingest.mjs --round 1 --role comparative --category typography --failed "returned prose twice"
//
// Malformed JSON or a failed schema check exits 1 and files nothing. There is
// no invented score: a rejected answer stays rejected until the judge is run
// again, and a call that stays broken is recorded as a failure instead of a
// result.

import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORY_KEYS } from "./categories.mjs";
import { validateAnswer } from "./schema.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(ROOT, "..", "..", "..");
const ROUNDS = path.join(REPO, "tools", "ui", "rounds");

function parseArgs(argv) {
  const args = { round: 0, role: null, category: null, viewport: null, file: null, failed: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
    else if (argv[i] === "--role") args.role = argv[++i];
    else if (argv[i] === "--category") args.category = argv[++i];
    else if (argv[i] === "--viewport") args.viewport = Number(argv[++i]);
    else if (argv[i] === "--file") args.file = argv[++i];
    else if (argv[i] === "--failed") args.failed = argv[++i];
  }
  if (!["absolute", "comparative", "defect-hunter"].includes(args.role)) {
    throw new Error("--role must be absolute, comparative or defect-hunter");
  }
  if (args.role === "comparative" && !CATEGORY_KEYS.includes(args.category)) {
    throw new Error(`comparative needs --category, one of ${CATEGORY_KEYS.join(", ")}`);
  }
  if (args.role !== "comparative" && !Number.isInteger(args.viewport)) {
    throw new Error(`${args.role} needs --viewport <width>: answers are filed per viewport`);
  }
  if (!args.failed && !args.file) throw new Error("--file is required (the judge's answer), or --failed with a reason");
  return args;
}

function extractJson(text) {
  const fenced = /```(?:json)?\s*\n([\s\S]*?)\n```/.exec(text);
  const candidate = fenced ? fenced[1] : text;
  return JSON.parse(candidate.trim());
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const roundDir = path.join(ROUNDS, `round-${args.round}`);
  const targetDir = args.role === "comparative"
    ? path.join(roundDir, "comparative", args.category)
    : path.join(roundDir, "raw");
  await mkdir(targetDir, { recursive: true });
  const stem = args.role === "comparative" ? "answer" : `${args.role}-${args.viewport}`;
  const failureStem = args.role === "comparative" ? "failed" : `${stem}.failed`;

  if (args.failed) {
    const record = {
      round: args.round,
      role: args.role,
      category: args.category,
      reason: args.failed,
      recorded: new Date().toISOString().slice(0, 10),
    };
    await writeFile(path.join(targetDir, `${failureStem}.json`), `${JSON.stringify(record, null, 2)}\n`);
    await rm(path.join(targetDir, `${stem}.json`), { force: true });
    console.error(`recorded failure for ${args.role}${args.category ? `/${args.category}` : ""}: ${args.failed}`);
    process.exitCode = 1;
    return;
  }

  const text = await readFile(path.resolve(args.file), "utf8");
  let value;
  try {
    value = extractJson(text);
  } catch (error) {
    console.error(`rejected: not valid JSON (${error.message})`);
    process.exitCode = 1;
    return;
  }
  const errors = validateAnswer(args.role, value);
  if (value.round !== args.round) errors.push(`round must be ${args.round}`);
  if (args.role === "comparative" && value.category !== args.category) {
    errors.push(`category must be "${args.category}"`);
  }
  if (errors.length) {
    console.error(`rejected: ${errors.length} schema problem(s)`);
    for (const error of errors) console.error(`  - ${error}`);
    process.exitCode = 1;
    return;
  }
  await writeFile(path.join(targetDir, `${stem}.json`), `${JSON.stringify(value, null, 2)}\n`);
  await writeFile(path.join(targetDir, `${stem}.txt`), text);
  await rm(path.join(targetDir, `${failureStem}.json`), { force: true });
  console.log(`filed ${args.role}${args.category ? `/${args.category}` : ""} → ${path.join(targetDir, `${stem}.json`)}`);
}

await main();
