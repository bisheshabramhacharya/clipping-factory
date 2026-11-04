// Ask one judge one question, enforce the call budget, and file the answer.
//
//   node judges/ask-judge.mjs --round 1 --judge claude --category typography
//
// The budget is part of the method, not a preference: a call gets the panel the
// prompt names (never more than ten images), one retry when the reply cannot be
// parsed, and then it is recorded as a failure. A reply that spawns a subagent
// is a failed call, because the helper would be scoring the panel on the
// judge's behalf.
//
// Supported judges: claude (Claude Code CLI, prints a JSON envelope with cost
// and subagent counts) and codex (Codex CLI, plain text). Both read the images
// from the absolute paths in the prompt.

import { execFile } from "node:child_process";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { CATEGORY_KEYS } from "./categories.mjs";
import { normalizeComparative, validateAnswer } from "./schema.mjs";

const run = promisify(execFile);
const ROOT = path.dirname(fileURLToPath(import.meta.url));
const RIG = path.resolve(ROOT, "..");
const REPO = path.resolve(RIG, "..", "..");
const ROUNDS = path.join(REPO, "tools", "ui", "rounds");
const MAX_IMAGES = 10;
const ATTEMPTS = 2;

function parseArgs(argv) {
  const args = { round: 0, judge: "claude", category: null, timeout: 900 };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--round") args.round = Number(argv[++i]);
    else if (argv[i] === "--judge") args.judge = argv[++i];
    else if (argv[i] === "--category") args.category = argv[++i];
    else if (argv[i] === "--timeout") args.timeout = Number(argv[++i]);
  }
  if (!CATEGORY_KEYS.includes(args.category)) {
    throw new Error(`--category must be one of ${CATEGORY_KEYS.join(", ")}`);
  }
  if (!["claude", "codex"].includes(args.judge)) throw new Error("--judge must be claude or codex");
  return args;
}

function extractJson(text) {
  const fenced = /```(?:json)?\s*\n([\s\S]*?)\n```/.exec(text);
  const candidate = fenced ? fenced[1] : text;
  const start = candidate.indexOf("{");
  const end = candidate.lastIndexOf("}");
  if (start === -1 || end === -1) throw new Error("no JSON object in the reply");
  return JSON.parse(candidate.slice(start, end + 1));
}

async function askClaude(prompt) {
  const { stdout } = await run("claude", ["-p", prompt, "--output-format", "json"], {
    cwd: RIG,
    maxBuffer: 32 * 1024 * 1024,
  });
  const envelope = JSON.parse(stdout);
  return {
    text: String(envelope.result ?? ""),
    model: envelope.modelUsage ? Object.keys(envelope.modelUsage)[0] : undefined,
    cost_usd: envelope.total_cost_usd,
    turns: envelope.num_turns,
    subagents: envelope.subagent_stats?.spawned ?? 0,
  };
}

async function askCodex(prompt) {
  const { stdout } = await run("codex", ["exec", "--skip-git-repo-check", "--", prompt], {
    cwd: RIG,
    maxBuffer: 32 * 1024 * 1024,
  });
  return { text: stdout, model: "codex", cost_usd: null, turns: null, subagents: 0 };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const panelDir = path.join(ROOT, "inbox", `round-${args.round}`, "comparative", args.category);
  const panel = JSON.parse(await readFile(path.join(panelDir, "panel.json"), "utf8"));
  const prompt = await readFile(path.join(panelDir, "prompt.md"), "utf8");
  if (panel.images > MAX_IMAGES) throw new Error(`panel has ${panel.images} images, over the ${MAX_IMAGES} cap`);

  const outDir = path.join(ROUNDS, `round-${args.round}`, "comparative", args.category);
  await mkdir(outDir, { recursive: true });
  const attempts = [];

  for (let attempt = 1; attempt <= ATTEMPTS; attempt += 1) {
    const started = Date.now();
    let reply;
    try {
      reply = args.judge === "claude" ? await askClaude(prompt) : await askCodex(prompt);
    } catch (error) {
      attempts.push({ attempt, error: `judge command failed: ${error.message}` });
      continue;
    }
    const call = {
      attempt,
      judge: args.judge,
      model: reply.model,
      cost_usd: reply.cost_usd,
      turns: reply.turns,
      subagents: reply.subagents,
      images: panel.images,
      seconds: Math.round((Date.now() - started) / 100) / 10,
    };
    if (reply.subagents > 0) {
      await writeFile(path.join(outDir, "failed.json"), `${JSON.stringify({
        round: args.round,
        role: "comparative",
        category: args.category,
        reason: `judge spawned ${reply.subagents} subagent(s); the call is void`,
        attempts: [...attempts, call],
      }, null, 2)}\n`);
      await rm(path.join(outDir, "answer.json"), { force: true });
      console.error(`failed: ${call.reason}`);
      process.exitCode = 1;
      return;
    }

    let value;
    const errors = [];
    try {
      value = normalizeComparative(extractJson(reply.text));
      if (value.round !== args.round) errors.push(`round must be ${args.round}`);
      if (value.category !== args.category) errors.push(`category must be "${args.category}"`);
      errors.push(...validateAnswer("comparative", value));
      if (value.original_winner) console.error(`note: ${value.original_winner} won without a checkable citation, recorded as a tie`);
    } catch (error) {
      errors.push(`not valid JSON: ${error.message}`);
    }

    if (!errors.length) {
      await writeFile(path.join(outDir, "answer.json"), `${JSON.stringify(value, null, 2)}\n`);
      await writeFile(path.join(outDir, "answer.txt"), reply.text);
      await writeFile(path.join(outDir, "call.json"), `${JSON.stringify({
        round: args.round,
        category: args.category,
        judge: args.judge,
        attempts: [...attempts, call],
      }, null, 2)}\n`);
      await rm(path.join(outDir, "failed.json"), { force: true });
      console.log(`filed comparative/${args.category} (${call.model}, $${call.cost_usd ?? "?"}, ${call.images} images)`);
      return;
    }

    attempts.push({ ...call, errors });
    console.error(`attempt ${attempt} rejected: ${errors.join("; ")}`);
  }

  await writeFile(path.join(outDir, "failed.json"), `${JSON.stringify({
    round: args.round,
    role: "comparative",
    category: args.category,
    reason: `${ATTEMPTS} attempts, no valid answer`,
    attempts,
  }, null, 2)}\n`);
  await rm(path.join(outDir, "answer.json"), { force: true });
  console.error(`recorded failure for comparative/${args.category} after ${ATTEMPTS} attempts`);
  process.exitCode = 1;
}

await main();
