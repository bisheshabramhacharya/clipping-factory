// Build a blind four-way panel so one screen from four independent designs can
// be ranked in a single call.
//
//   node judges/prepare-variants.mjs --round 6 --state results-6-clips \
//     --dirs shots-a,shots-b,shots-c,shots-d --tag results
//
// Each candidate directory holds `<state>@<width>.<platform>.png`. The files are
// copied into A.png..D.png under a deterministic shuffle and the label map is
// written outside the judge's reading path, so the answer cannot be traced back
// to a name while it is being written.

import { copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const RIG = path.resolve(ROOT, "..");
const INBOX = path.join(ROOT, "inbox");

const LETTERS = ["A", "B", "C", "D", "E", "F"];

function parseArgs(argv) {
  const args = { round: 6, state: null, width: 1280, dirs: [], tag: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--round") args.round = Number(argv[++i]);
    else if (arg === "--state") args.state = argv[++i];
    else if (arg === "--width") args.width = Number(argv[++i]);
    else if (arg === "--dirs") args.dirs = argv[++i].split(",").map((value) => value.trim()).filter(Boolean);
    else if (arg === "--tag") args.tag = argv[++i];
  }
  if (!args.state) throw new Error("--state is required");
  if (args.dirs.length < 2) throw new Error("--dirs needs at least two shot directories");
  return args;
}

// Same input, same order: the shuffle is a seed over the tag, not Math.random.
function shuffle(items, seedText) {
  let seed = 0x811c9dc5;
  for (const char of seedText) seed = ((seed ^ char.charCodeAt(0)) * 0x01000193) >>> 0;
  const out = [...items];
  for (let i = out.length - 1; i > 0; i -= 1) {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    const j = seed % (i + 1);
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

async function findShot(dir, state, width) {
  const absolute = path.isAbsolute(dir) ? dir : path.join(RIG, dir);
  const names = await readdir(absolute).catch(() => []);
  const pattern = new RegExp(`^${state}@${width}\\.[a-z0-9]+\\.png$`);
  const match = names.filter((name) => pattern.test(name)).sort()[0];
  if (!match) throw new Error(`no ${state}@${width} shot in ${absolute}`);
  return path.join(absolute, match);
}

function promptText({ images, state, width, rubric, answer }) {
  return `# Blind ranking: four designs of one screen

Four independent designers each restyled the same product. This call shows their
version of the same screen, \`${state}\` at ${width}px, one per letter. None of
them is a reference and none is the original. Rank them.

Read the rubric at ${rubric} first: it defines the eight qualities the ranking
should weigh. Then read each image once.

${images.map((letter) => `${letter}. ${path.join(path.dirname(answer), `${letter}.png`)}`).join("\n")}

## Rules

- Rank all four, best to worst: \`"ranking": ["B", "D", "A", "C"]\`.
- For each one, say what it does better and what it does worse than the others,
  citing what you can see in the image. Specific beats general.
- Judge the interface, not the fixture content. The episode, clip titles and
  video stills are placeholders in all four.
- No design is expected to win. If two are effectively tied, say so and explain
  what separates them.
- Do not open any other file, do not run the application, do not spawn helpers.
- One pass. Never re-read a file.

## Output

Write this object to ${answer} with a bash heredoc, then reply with the ranking
and one line per design:

\`\`\`json
{
  "role": "variants",
  "screen": "${state}@${width}",
  "ranking": ["B", "D", "A", "C"],
  "notes": {
    "A": { "better": "", "worse": "" },
    "B": { "better": "", "worse": "" },
    "C": { "better": "", "worse": "" },
    "D": { "better": "", "worse": "" }
  },
  "decisive": "the one difference that decided first from last"
}
\`\`\`
`;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const tag = args.tag || `${args.state}@${args.width}`;
  const out = path.join(INBOX, `round-${args.round}`, `variants-${tag}`);
  await mkdir(out, { recursive: true });

  const shots = [];
  for (const dir of args.dirs) shots.push({ dir, file: await findShot(dir, args.state, args.width) });
  if (shots.length > LETTERS.length) throw new Error(`at most ${LETTERS.length} candidates per call`);

  const order = shuffle(shots, tag);
  const map = {};
  for (let i = 0; i < order.length; i += 1) {
    const letter = LETTERS[i];
    map[letter] = { dir: order[i].dir, source: path.basename(order[i].file) };
    await copyFile(order[i].file, path.join(out, `${letter}.png`));
  }

  const answer = path.join(out, "answer.json");
  // The map stays outside the panel directory: a judge may read anything it
  // can see, and the letters must not resolve to a name until after the call.
  await writeFile(path.join(INBOX, `round-${args.round}`, `map-${tag}.json`), `${JSON.stringify(map, null, 2)}\n`);
  await writeFile(
    path.join(out, "prompt.md"),
    promptText({
      images: LETTERS.slice(0, order.length),
      state: args.state,
      width: args.width,
      rubric: path.join(RIG, "rubric.md"),
      answer,
    }),
  );
  await writeFile(
    path.join(out, "task.md"),
    `# Ranking call: ${tag}\n\nYou are the only judge for this call. Work alone.\n\n`
    + `1. Read \`${path.join(out, "prompt.md")}\` and follow it exactly.\n`
    + `2. Rank the ${order.length} images it lists. One pass, read each once.\n`
    + `3. Write the answer object to \`${answer}\` with the heredoc it shows.\n`
    + `4. Reply with the ranking and one line per design.\n\n`
    + `Do not open any other folder, do not run the application, and do not spawn subagents.\n`,
  );
  console.log(`panel: ${tag} · ${order.length} designs → ${out}`);
  console.log(`map:   ${JSON.stringify(Object.fromEntries(Object.entries(map).map(([letter, value]) => [letter, value.dir])))}`);
}

await main();
