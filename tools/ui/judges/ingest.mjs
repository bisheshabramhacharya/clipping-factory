// Validate one judge answer and file it under rounds/round-<n>/raw/.
//
//   node judges/ingest.mjs --round 0 --role absolute --file answer.json
//
// Malformed JSON or a failed schema check exits 1 and files nothing. There is
// no invented score: a rejected answer stays rejected until the judge is run
// again.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { validateAnswer } from "./schema.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(ROOT, "..", "..", "..");

function parseArgs(argv) {
  const args = { round: 0, role: null, file: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
    else if (argv[i] === "--role") args.role = argv[++i];
    else if (argv[i] === "--file") args.file = argv[++i];
  }
  if (!["absolute", "comparative", "defect-hunter"].includes(args.role)) {
    throw new Error("--role must be absolute, comparative or defect-hunter");
  }
  if (!args.file) throw new Error("--file is required (the judge's answer.json)");
  return args;
}

function extractJson(text) {
  const fenced = /```(?:json)?\s*\n([\s\S]*?)\n```/.exec(text);
  const candidate = fenced ? fenced[1] : text;
  return JSON.parse(candidate.trim());
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
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
  if (errors.length) {
    console.error(`rejected: ${errors.length} schema problem(s)`);
    for (const error of errors) console.error(`  - ${error}`);
    process.exitCode = 1;
    return;
  }
  const dir = path.join(REPO, "tools", "ui", "rounds", `round-${args.round}`, "raw");
  await mkdir(dir, { recursive: true });
  await writeFile(path.join(dir, `${args.role}.json`), `${JSON.stringify(value, null, 2)}\n`);
  await writeFile(path.join(dir, `${args.role}.txt`), text);
  console.log(`filed ${args.role} → ${path.join(dir, `${args.role}.json`)}`);
}

await main();
