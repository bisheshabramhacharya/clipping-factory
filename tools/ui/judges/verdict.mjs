// Read one comparative category and say whether the verdict survived the
// swapped panel.
//
// Every comparative call runs twice, once with A and B as built and once with
// the sides exchanged. A judge that prefers whatever is on the left will flip
// when the panel is swapped; that is a position effect, not a design finding,
// so a flipped verdict is recorded as a tie. A category with only one of the two
// calls is not judged, because the method did not finish.

import { readFile } from "node:fs/promises";
import path from "node:path";

async function readJson(file) {
  try {
    return JSON.parse(await readFile(file, "utf8"));
  } catch {
    return null;
  }
}

function sideOf(panel, winner) {
  if (!panel || (winner !== "A" && winner !== "B")) return null;
  return panel.labels?.[winner] || null;
}

export async function readCategoryVerdict(roundDir, map, categoryKey, label) {
  const panel = map?.categories?.[categoryKey];
  const swappedPanel = map?.categories?.[`${categoryKey}-swapped`];
  const base = {
    label,
    verdict: "not_judged",
    citation: null,
    why: null,
    confidence: null,
    order_checked: false,
    swapped_winner: null,
    judge: null,
    model: null,
  };
  if (!panel) return { ...base, failure: "no panel built" };

  const answer = await readJson(path.join(roundDir, "comparative", categoryKey, "answer.json"));
  const swapped = await readJson(path.join(roundDir, "comparative", `${categoryKey}-swapped`, "answer.json"));
  const failed = await readJson(path.join(roundDir, "comparative", categoryKey, "failed.json"));
  const calls = await readJson(path.join(roundDir, "comparative", categoryKey, "calls.json"));

  const record = {
    ...base,
    citation: answer?.citation || null,
    why: answer?.why || null,
    confidence: answer?.confidence || null,
    judge: calls?.calls?.at(-1)?.judge || null,
    model: calls?.calls?.at(-1)?.model || null,
    order_checked: Boolean(answer && swapped),
    swapped_winner: swapped ? sideOf(swappedPanel, swapped.winner) : null,
  };

  if (!answer) return { ...record, failure: failed?.reason || "no answer" };
  if (!swapped) {
    return { ...record, failure: "the swapped call is missing, so the order check did not run" };
  }

  const normalSide = sideOf(panel, answer.winner);
  const swappedSide = sideOf(swappedPanel, swapped.winner);
  if (!normalSide || !swappedSide) {
    return { ...record, verdict: "tie", why: `${answer.why || ""} (one call returned a tie)`.trim() };
  }
  if (normalSide !== swappedSide) {
    return {
      ...record,
      verdict: "tie",
      why: `Verdict flipped when the panel was swapped: ${normalSide} won as built, ${swappedSide} won swapped. Recorded as a tie.`,
    };
  }
  return { ...record, verdict: normalSide === "studio" ? "win" : "loss" };
}
