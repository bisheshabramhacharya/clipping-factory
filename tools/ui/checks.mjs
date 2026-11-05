// Deterministic UI facts, no model involved.
//
//   npm run checks            # report, always exit 0
//   npm run checks -- --strict  # exit 1 when a WCAG AA pair or overflow fails
//
// What it measures on every state at every viewport:
//   - WCAG AA contrast of every visible text/background pair it can resolve
//   - distinct border-radius, font-size, shadow and color values in use
//   - page-level horizontal overflow
//   - clipped text (scroll width larger than client width under overflow:hidden)
//
// Writes tools/ui/checks.json. Facts here outrank any judge opinion.

import { writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createFixtureServer } from "./server.mjs";
import { launchBrowser, openState, closeState, VIEWPORTS, STATE_NAMES } from "./rig.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.join(ROOT, "checks.json");

function auditScript() {
  const visible = (el) => {
    const rect = el.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return false;
    const cs = getComputedStyle(el);
    if (cs.visibility === "hidden" || cs.display === "none" || Number(cs.opacity) < 0.05) return false;
    return true;
  };

  const selectorOf = (el) => {
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && parts.length < 5) {
      let part = node.tagName.toLowerCase();
      if (node.id) part += `#${node.id}`;
      else if (node.classList.length) part += `.${[...node.classList].slice(0, 2).join(".")}`;
      const parent = node.parentElement;
      if (parent) {
        const siblings = [...parent.children].filter((child) => child.tagName === node.tagName);
        if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(" > ");
  };

  const parseColor = (value) => {
    const match = /^rgba?\(([^)]+)\)$/.exec(value || "");
    if (!match) return { r: 0, g: 0, b: 0, a: 0 };
    const bits = match[1].split(/[ ,/]+/).filter(Boolean).map(Number);
    return { r: bits[0] || 0, g: bits[1] || 0, b: bits[2] || 0, a: bits.length > 3 ? bits[3] : 1 };
  };

  const over = (front, back) => {
    const a = front.a + back.a * (1 - front.a);
    if (a === 0) return { r: 255, g: 255, b: 255, a: 1 };
    const mix = (f, b) => (f * front.a + b * back.a * (1 - front.a)) / a;
    return { r: mix(front.r, back.r), g: mix(front.g, back.g), b: mix(front.b, back.b), a };
  };

  // Resolve the painted background behind an element. Returns null when a
  // gradient or image sits inside the chain: a flat ratio would be a lie.
  const backgroundOf = (el) => {
    const layers = [];
    let node = el;
    while (node && node.nodeType === 1) {
      const cs = getComputedStyle(node);
      if (cs.backgroundImage && cs.backgroundImage !== "none") return null;
      const color = parseColor(cs.backgroundColor);
      if (color.a > 0) {
        layers.push(color);
        if (color.a >= 0.999) break;
      }
      node = node.parentElement;
    }
    if (!layers.length) return { r: 246, g: 245, b: 241, a: 1 };
    let base = { r: 246, g: 245, b: 241, a: 1 };
    for (let i = layers.length - 1; i >= 0; i -= 1) base = over(layers[i], base);
    return base;
  };

  const channel = (value) => {
    const c = value / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const luminance = (color) => 0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b);
  const contrast = (a, b) => {
    const la = luminance(a);
    const lb = luminance(b);
    return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
  };
  const round = (value) => Math.round(value * 100) / 100;
  const colorText = (color) => `rgb(${Math.round(color.r)}, ${Math.round(color.g)}, ${Math.round(color.b)})`;

  const contrastPairs = [];
  let textPairsChecked = 0;
  const clipped = [];
  const overflowing = [];
  const filled = [];
  const titleSizes = {};
  const caps = {};
  const textLefts = [];
  const rhythm = {};
  const marks = [];
  const radius = {};
  const fontSizes = {};
  const shadows = {};
  const colors = {};
  const fonts = {};
  const bump = (bucket, key) => {
    if (!key || key === "none" || key === "normal") return;
    bucket[key] = (bucket[key] || 0) + 1;
  };

  for (const el of document.body.querySelectorAll("*")) {
    if (!visible(el)) continue;
    const cs = getComputedStyle(el);
    if (el.classList.contains("sr-only")) continue;
    bump(radius, cs.borderRadius);
    bump(fontSizes, cs.fontSize);
    bump(shadows, cs.boxShadow);
    bump(fonts, cs.fontFamily);
    if (cs.color) bump(colors, cs.color);

    const ownText = [...el.childNodes]
      .filter((node) => node.nodeType === 3 && node.textContent.trim())
      .map((node) => node.textContent.trim())
      .join(" ");
    if (ownText) {
      const background = backgroundOf(el);
      if (background) {
        textPairsChecked += 1;
        const foreground = parseColor(cs.color);
        const painted = foreground.a < 1 ? over(foreground, background) : foreground;
        const size = parseFloat(cs.fontSize);
        const weight = Number(cs.fontWeight) || 400;
        const large = size >= 24 || (size >= 18.66 && weight >= 700);
        const required = large ? 3 : 4.5;
        const ratio = round(contrast(painted, background));
        if (ratio < required) {
          contrastPairs.push({
            selector: selectorOf(el),
            text: ownText.slice(0, 80),
            foreground: colorText(painted),
            background: colorText(background),
            ratio,
            required,
            font_size: cs.fontSize,
            font_weight: cs.fontWeight,
          });
        }
      }
    }

    if (cs.overflowX === "hidden" && el.scrollWidth > el.clientWidth + 1) {
      const text = (el.textContent || "").trim().replace(/\s+/g, " ");
      if (text) {
        clipped.push({
          selector: selectorOf(el),
          text: text.slice(0, 90),
          scroll_width: el.scrollWidth,
          client_width: el.clientWidth,
          delta: el.scrollWidth - el.clientWidth,
          ellipsis: cs.textOverflow === "ellipsis",
        });
      }
    }
  }

  const doc = document.documentElement;
  for (const el of document.body.querySelectorAll("*")) {
    if (!visible(el)) continue;
    const rect = el.getBoundingClientRect();
    if (rect.right > window.innerWidth + 1) {
      overflowing.push({
        selector: selectorOf(el),
        right: Math.round(rect.right),
        width: Math.round(rect.width),
        overflow: Math.round(rect.right - window.innerWidth),
      });
    }
  }

  // System invariants. Each one is a rule a blind panel has cited at least
  // once, expressed so a machine can refuse the regression instead of a judge
  // happening to notice it.
  for (const el of document.body.querySelectorAll("*")) {
    if (!visible(el)) continue;
    const cs = getComputedStyle(el);
    const own = (el.textContent || "").trim();
    const rect = el.getBoundingClientRect();

    // An element behind an overlay is still laid out, so a rect alone would
    // count the view under the theater. Ask the hit test instead: a control or
    // a title only counts if it is the thing a person would actually see.
    const size = parseFloat(cs.fontSize) || 0;
    const weight = Number(cs.fontWeight) || 400;
    const letters = own.replace(/[^A-Za-z]/g, "");
    const isControl = el.tagName === "BUTTON" || el.getAttribute("role") === "button";
    const matters = (isControl && cs.backgroundColor === "rgb(10, 10, 10)")
      || (size >= 24 && weight >= 600 && letters.length > 3)
      || (cs.textTransform === "uppercase" && letters.length > 1)
      || (size >= 12 && own.length >= 8);
    if (!matters) continue;
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), window.innerWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), window.innerHeight - 1);
    const hit = document.elementFromPoint(x, y);
    const exposed = Boolean(hit && (hit === el || el.contains(hit) || el.contains(hit.parentElement)));
    if (!exposed) continue;

    if (isControl && cs.backgroundColor === "rgb(10, 10, 10)") {
      filled.push({ label: (own || el.id || "?").slice(0, 24), selector: selectorOf(el) });
    }

    if (own && el.children.length === 0) {
      if (size >= 24 && weight >= 600 && letters.length > 3) bump(titleSizes, `${cs.fontSize}|${el.className || el.tagName}`);
      if (cs.textTransform === "uppercase" && size > 0 && letters.length > 1) bump(caps, `${cs.fontSize}|${cs.letterSpacing}`);
      if (size >= 12 && own.length >= 8 && !el.closest("header")) textLefts.push(Math.round(rect.left));
      if (size >= 10) bump(rhythm, `${cs.fontSize}|${cs.lineHeight}`);
    }

    if (el.classList.contains("library-card")) {
      marks.push({ row: (own || "").slice(0, 30), marked: Boolean(el.querySelector(".library-mark")) });
    }
  }

  return {
    contrast: contrastPairs,
    text_pairs_checked: textPairsChecked,
    clipped,
    filled,
    title_sizes: Object.keys(titleSizes),
    caps: Object.keys(caps),
    rhythm: Object.keys(rhythm),
    text_left: textLefts.length ? textLefts.reduce((min, value) => Math.min(min, value), Infinity) : null,
    library_rows: marks,
    overflow: {
      document_scroll_width: doc.scrollWidth,
      body_scroll_width: document.body.scrollWidth,
      viewport_width: window.innerWidth,
      has_horizontal_overflow: doc.scrollWidth > window.innerWidth + 1,
      offenders: overflowing.slice(0, 12),
    },
    values: { radius, fontSizes, shadows, colors, fonts },
  };
}

function sumValues(accumulator, bucket) {
  for (const [value, count] of Object.entries(bucket)) {
    accumulator[value] = (accumulator[value] || 0) + count;
  }
}

function table(bucket) {
  return Object.entries(bucket)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([value, count]) => ({ value, count }));
}

async function main() {
  const strict = process.argv.includes("--strict");
  const server = await createFixtureServer();
  const browser = await launchBrowser();
  const run = {
    rig: "tools/ui",
    states: STATE_NAMES,
    viewports: VIEWPORTS.map((viewport) => `${viewport.width}x${viewport.height}`),
    results: [],
    values: { radius: {}, fontSizes: {}, shadows: {}, colors: {}, fonts: {} },
  };
  try {
    for (const state of STATE_NAMES) {
      for (const viewport of VIEWPORTS) {
        const handle = await openState(browser, server.url, state, viewport);
        try {
          const audit = await handle.page.evaluate(auditScript);
          run.results.push({ state, viewport: viewport.width, ...audit });
          for (const key of Object.keys(run.values)) {
            if (audit.values[key]) sumValues(run.values[key], audit.values[key]);
          }
        } finally {
          await closeState(handle);
        }
      }
    }
  } finally {
    await browser.close();
    await server.close();
  }

  const contrastFailures = run.results.flatMap((result) =>
    result.contrast.map((pair) => ({ state: result.state, viewport: result.viewport, ...pair })));
  const clippedText = run.results.flatMap((result) =>
    result.clipped.map((item) => ({ state: result.state, viewport: result.viewport, ...item })));
  const overflow = run.results
    .filter((result) => result.overflow.has_horizontal_overflow)
    .map((result) => ({ state: result.state, viewport: result.viewport, ...result.overflow }));

  // Gates: each rule below is a system invariant a blind panel has cited.
  // A gate failure is a regression, not a taste question.
  const gates = [];
  const sizeOf = (entry) => entry.split("|")[0];
  const filledOver = run.results
    .filter((result) => (result.filled || []).length > 1)
    .map((result) => ({ gate: "one-filled-control-per-view", state: result.state, viewport: result.viewport, found: result.filled.map((item) => item.label) }));
  gates.push(...filledOver);

  const titleSpread = run.results
    .map((result) => ({ state: result.state, viewport: result.viewport, sizes: [...new Set((result.title_sizes || []).map(sizeOf))] }))
    .filter((result) => result.sizes.length > 1);
  gates.push(...titleSpread.map((result) => ({ gate: "one-display-size-per-view", ...result })));

  // One line-height per size. A size that needs two rhythms needs a second
  // size or a documented exception, not a value chosen per element.
  const rhythm = {};
  for (const result of run.results) {
    for (const entry of result.rhythm || []) {
      const [size, lineHeight] = entry.split("|");
      rhythm[size] = rhythm[size] || {};
      rhythm[size][lineHeight] = (rhythm[size][lineHeight] || 0) + 1;
    }
  }
  for (const [size, heights] of Object.entries(rhythm)) {
    if (Object.keys(heights).length > 1) {
      gates.push({ gate: "one-line-height-per-size", found: `${size}: ${Object.entries(heights).map(([h, n]) => `${h}×${n}`).join(", ")}` });
    }
  }

  const capsEntries = run.results.flatMap((result) => result.caps || []);
  const capsSizes = [...new Set(capsEntries.map(sizeOf))];
  if (capsSizes.length > 1) gates.push({ gate: "one-caps-size", found: capsSizes });
  const capsTracking = [...new Set(capsEntries.map((entry) => entry.split("|")[1]))];
  if (capsTracking.length > 1) gates.push({ gate: "one-caps-tracking", found: capsTracking });

  // Each family of screens is one page with one content column: the page
  // family (upload, run, setup) shares an edge, and so does the studio family.
  const FAMILY = {
    "empty-upload": "page",
    "staged-file": "page",
    "processing-mid-run": "page",
    "processing-error": "page",
    "setup-missing-runtime": "page",
    "results-6-clips": "studio",
    "results-rejected-expanded": "studio",
    "clip-detail-restyle": "studio",
    "review-theater": "studio",
    "review-done": "studio",
    "library-open": "studio",
  };
  const edges = {};
  for (const result of run.results) {
    if (result.text_left == null) continue;
    const family = FAMILY[result.state] || "other";
    const key = `${family}@${result.viewport}`;
    edges[key] = edges[key] || {};
    edges[key][result.text_left] = (edges[key][result.text_left] || []).concat(result.state);
  }
  // Edges are reported, not gated: a card's inset text and a drawer's own
  // column are legitimate differences, so the machine lists them and a person
  // decides which ones are drift.
  const edgeReport = Object.entries(edges)
    .map(([key, lefts]) => ({ where: key, found: Object.keys(lefts).map(Number).sort((a, b) => a - b).map((value) => `${value}:${[...new Set(lefts[String(value)])].join("/")}`) }))
    .filter((entry) => entry.found.length > 1);

  const unmarked = run.results.flatMap((result) => (result.library_rows || []).filter((row) => !row.marked).map((row) => ({ gate: "every-library-row-marked", state: result.state, viewport: result.viewport, row: row.row })));
  gates.push(...unmarked);

  const summary = {
    radius: table(run.values.radius),
    fontSizes: table(run.values.fontSizes),
    shadows: table(run.values.shadows),
    colors: table(run.values.colors),
    fonts: table(run.values.fonts),
  };

  const report = {
    checked: {
      states: run.states.length,
      viewports: run.viewports,
      shots: run.results.length,
      text_pairs: run.results.reduce((total, result) => total + (result.text_pairs_checked || 0), 0),
    },
    wcag: {
      failures: contrastFailures,
      count: contrastFailures.length,
      unique: new Set(contrastFailures.map((item) =>
        `${item.selector}|${item.foreground}|${item.background}|${item.required}`)).size,
    },
    distinct_values: {
      radius: summary.radius.length,
      font_sizes: summary.fontSizes.length,
      shadows: summary.shadows.length,
      colors: summary.colors.length,
      font_families: summary.fonts.length,
    },
    values: summary,
    overflow: { failing_states: overflow, count: overflow.length },
    clipped_text: { count: clippedText.length, items: clippedText },
    gates: { count: gates.length, failing: gates },
    edges: edgeReport,
  };

  await writeFile(OUT, `${JSON.stringify(report, null, 2)}\n`);

  console.log(`checked ${report.checked.shots} shots (${report.checked.states} states x ${VIEWPORTS.length} viewports)`);
  console.log(`WCAG AA: ${report.wcag.count} failures (${report.wcag.unique} unique pairs)`);
  for (const failure of contrastFailures.slice(0, 10)) {
    console.log(`  ${failure.state}@${failure.viewport} ${failure.selector} "${failure.text}" ${failure.ratio}:1 < ${failure.required}:1`);
  }
  if (contrastFailures.length > 10) console.log(`  ... ${contrastFailures.length - 10} more in checks.json`);
  console.log(`distinct values: ${report.distinct_values.radius} border-radius, ${report.distinct_values.font_sizes} font-size, ${report.distinct_values.shadows} shadow, ${report.distinct_values.colors} color, ${report.distinct_values.font_families} font-family`);
  console.log(`  radius: ${summary.radius.map((item) => `${item.value}×${item.count}`).join(", ")}`);
  console.log(`horizontal overflow: ${report.overflow.count} failing shots`);
  for (const item of overflow) console.log(`  ${item.state}@${item.viewport} content ${item.document_scroll_width}px in ${item.viewport_width}px`);
  console.log(`clipped text: ${report.clipped_text.count} elements (${clippedText.filter((item) => item.ellipsis).length} with ellipsis)`);
  for (const item of clippedText.slice(0, 8)) {
    console.log(`  ${item.state}@${item.viewport} ${item.selector} +${item.delta}px "${item.text.slice(0, 50)}"`);
  }
  console.log(`system gates: ${report.gates.count} failing`);
  for (const gate of report.gates.failing.slice(0, 12)) {
    const where = gate.state ? `${gate.state}@${gate.viewport}` : "";
    console.log(`  ${gate.gate} ${where} ${JSON.stringify(gate.found || gate.sizes || gate.row || "")}`);
  }
  if (report.gates.count > 12) console.log(`  ... ${report.gates.count - 12} more in checks.json`);
  console.log(`report: ${OUT}`);

  if (strict && (report.wcag.count > 0 || report.overflow.count > 0 || report.gates.count > 0)) process.exitCode = 1;
}

await main();
