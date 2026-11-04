// Run one comparative judge call on a Capy subagent, and file what it answered.
//
//   node judges/ask-judge.mjs --stage   --round 1 --category typography
//   node judges/ask-judge.mjs --collect --round 1 --category typography \
//     --subagent 3 --credits 0.42 --subagents 0
//
// The judge is pinned in judges/judges.json. A call is a subagent on this
// machine, one per call, and the caps are enforced here rather than asked for:
// a panel over ten images is refused, a third attempt is refused, a call whose
// judge spawned a helper is void and never filed, and a win without a checkable
// citation is filed as a tie.
//
// --stage writes task.md (the subagent's instructions) and prints the exact
// tools.subagent.create payload to use. --collect reads the answer the subagent
// wrote, validates it, and files it under rounds/round-<n>/comparative/<cat>/.
// --collect --as repeat files a second independent call as repeat.json, which is
// how the method is checked for stability without overwriting the round record.

import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CATEGORY_KEYS } from "./categories.mjs";
import { normalizeComparative, validateAnswer } from "./schema.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const RIG = path.resolve(ROOT, "..");
const REPO = path.resolve(RIG, "..", "..");
const ROUNDS = path.join(REPO, "tools", "ui", "rounds");

function parseArgs(argv) {
  const args = {
    round: 0, category: null, stage: false, collect: false, failed: null,
    as: "answer", subagent: null, credits: null, tokens: null, subagents: 0, model: null,
    from: null, reasoning: null, maxReplyTokens: 300, swap: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--round") args.round = Number(argv[++i]);
    else if (arg === "--category") args.category = argv[++i];
    else if (arg === "--stage") args.stage = true;
    else if (arg === "--collect") args.collect = true;
    else if (arg === "--failed") args.failed = argv[++i];
    else if (arg === "--as") args.as = argv[++i];
    else if (arg === "--subagent") args.subagent = argv[++i];
    else if (arg === "--credits") args.credits = Number(argv[++i]);
    else if (arg === "--tokens") args.tokens = Number(argv[++i]);
    else if (arg === "--subagents") args.subagents = Number(argv[++i]);
    else if (arg === "--model") args.model = argv[++i];
    else if (arg === "--from") args.from = argv[++i];
    else if (arg === "--reasoning") args.reasoning = argv[++i];
    else if (arg === "--max-reply-tokens") args.maxReplyTokens = Number(argv[++i]);
    else if (arg === "--swap") args.swap = true;
  }
  if (!CATEGORY_KEYS.includes(args.category)) throw new Error(`--category must be one of ${CATEGORY_KEYS.join(", ")}`);
  if (!["answer", "repeat"].includes(args.as)) throw new Error('--as must be "answer" or "repeat"');
  if (!args.stage && !args.collect && !args.failed) throw new Error("pass --stage, --collect or --failed <reason>");
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

async function readJson(file) {
  try {
    return JSON.parse(await readFile(file, "utf8"));
  } catch {
    return null;
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const policy = (await readJson(path.join(ROOT, "judges.json")))?.comparative;
  if (!policy) throw new Error("judges/judges.json has no comparative policy");

  const panelKey = args.swap ? `${args.category}-swapped` : args.category;
  const panelDir = path.join(ROOT, "inbox", `round-${args.round}`, "comparative", panelKey);
  const panel = await readJson(path.join(panelDir, "panel.json"));
  if (!panel) throw new Error(`no panel at ${panelDir}; run judges/prepare.mjs --role comparative first`);
  const outDir = path.join(ROUNDS, `round-${args.round}`, "comparative", panelKey);
  await mkdir(outDir, { recursive: true });
  const attemptsPath = path.join(panelDir, "attempts.json");
  const attempts = (await readJson(attemptsPath)) || { attempts: [] };

  if (args.stage) {
    if (panel.images > policy.max_images) {
      throw new Error(`panel has ${panel.images} images, over the ${policy.max_images} cap`);
    }
    if (attempts.attempts.filter((attempt) => !attempt.repeat).length >= policy.max_attempts) {
      throw new Error(`${policy.max_attempts} attempts already used for ${args.category}; record a failure instead of calling again`);
    }
    await rm(path.join(panelDir, "answer.json"), { force: true });
    await rm(path.join(panelDir, "reply.txt"), { force: true });
    const task = `# Judge call: ${args.category} (round ${args.round})

You are the only judge for this call. Work alone.

1. Read \`${path.join(panelDir, "prompt.md")}\` and follow every rule in it.
2. Read the rubric it names, then look at the ${panel.images} images it lists.
3. Reply with one JSON object and nothing else, at most ${args.maxReplyTokens}
   tokens in total. Do not write any file, do not run any command, and do not
   list directories.

Read the prompt, the rubric and each image exactly once. Do not re-read a file
you have already read. Answer in one pass.

Do not spawn subagents or helpers. Do not run any application. Do not read
\`rounds/\`, any \`comparative-map.json\`, or anything that names which side is
which. Use ${policy.model} as your model; you are running on ${policy.machine}.
`;
    await writeFile(path.join(panelDir, "task.md"), task);
    attempts.attempts.push({ attempt: attempts.attempts.length + 1, repeat: args.as === "repeat", staged: new Date().toISOString() });
    await writeFile(attemptsPath, `${JSON.stringify(attempts, null, 2)}\n`);
    console.log(`staged ${panelKey}: ${panel.images} images, attempt ${attempts.attempts.length} of ${policy.max_attempts}`);
    console.log(`task: ${path.join(panelDir, "task.md")}`);
    console.log(`start with: machine "${policy.machine}", model ${policy.model}, reasoning ${policy.reasoning_mode}`);
    return;
  }

  const filedStem = args.as === "repeat" ? "repeat" : "answer";

  if (args.failed) {
    await writeFile(path.join(outDir, "failed.json"), `${JSON.stringify({
      round: args.round,
      role: "comparative",
      category: args.category,
      reason: args.failed,
      attempts: attempts.attempts,
      recorded: new Date().toISOString().slice(0, 10),
    }, null, 2)}\n`);
    await rm(path.join(outDir, `${filedStem}.json`), { force: true });
    console.error(`recorded failure for comparative/${args.category}: ${args.failed}`);
    process.exitCode = 1;
    return;
  }

  if (args.subagents > 0) {
    await writeFile(path.join(outDir, "void.json"), `${JSON.stringify({
      round: args.round,
      category: args.category,
      reason: `judge spawned ${args.subagents} subagent(s); the call is void`,
      recorded: new Date().toISOString().slice(0, 10),
    }, null, 2)}\n`);
    console.error(`void: judge spawned ${args.subagents} subagent(s); nothing filed`);
    process.exitCode = 1;
    return;
  }
  if (args.model && args.model !== policy.model) {
    console.error(`refused: call ran on ${args.model}, the pinned judge is ${policy.model}`);
    process.exitCode = 1;
    return;
  }

  const raw = (args.from ? await readFile(path.resolve(args.from), "utf8").catch(() => null) : null)
    ?? (await readFile(path.join(panelDir, "answer.json"), "utf8").catch(() => null))
    ?? (await readFile(path.join(panelDir, "reply.txt"), "utf8").catch(() => null));
  if (raw === null) {
    console.error(`no answer at ${args.from || path.join(panelDir, "answer.json")} or reply.txt`);
    process.exitCode = 1;
    return;
  }

  const errors = [];
  const approxTokens = Math.ceil(raw.trim().length / 4);
  if (approxTokens > args.maxReplyTokens) {
    errors.push(`reply is about ${approxTokens} tokens, over the ${args.maxReplyTokens} cap`);
  }
  let value;
  try {
    value = normalizeComparative(extractJson(raw));
    if (value.round !== args.round) errors.push(`round must be ${args.round}`);
    if (value.category !== args.category) errors.push(`category must be "${args.category}"`);
    errors.push(...validateAnswer("comparative", value));
  } catch (error) {
    errors.push(`not valid JSON: ${error.message}`);
  }
  if (value?.original_winner) {
    console.error(`note: ${value.original_winner} won without a checkable citation, filed as a tie`);
  }
  if (errors.length) {
    console.error(`rejected ${args.category}: ${errors.length} problem(s)`);
    for (const error of errors) console.error(`  - ${error}`);
    process.exitCode = 1;
    return;
  }

  const call = {
    run: filedStem,
    judge: "capy-subagent",
    model: args.model || policy.model,
    machine: policy.machine,
    reasoning: args.reasoning || policy.reasoning_mode,
    subagent: args.subagent,
    credits: args.credits,
    tokens: args.tokens,
    images: panel.images,
    staged_at: attempts.attempts.at(-1)?.staged || null,
    collected_at: new Date().toISOString(),
  };
  await writeFile(path.join(outDir, `${filedStem}.json`), `${JSON.stringify(value, null, 2)}\n`);
  await writeFile(path.join(outDir, `${filedStem}.txt`), raw);
  const calls = (await readJson(path.join(outDir, "calls.json"))) || { calls: [] };
  calls.calls = calls.calls.filter((entry) => entry.run !== filedStem).concat(call);
  await writeFile(path.join(outDir, "calls.json"), `${JSON.stringify(calls, null, 2)}\n`);
  await rm(path.join(outDir, "failed.json"), { force: true });
  console.log(`filed comparative/${panelKey} as ${filedStem} (${call.model}, ${call.credits ?? "?"} credits, ${panel.images} images)`);
  console.log(JSON.stringify(value));
}

await main();
