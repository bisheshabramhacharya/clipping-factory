import { CATEGORY_KEYS } from "./categories.mjs";

const isScore = (value) => Number.isInteger(value) && value >= 1 && value <= 10;
const PANEL_REFERENCE = /[AB]\/\d{2}\.png/;

function checkCategories(value, where, errors) {
  if (!value || typeof value !== "object") {
    errors.push(`${where}: missing categories`);
    return;
  }
  const keys = Object.keys(value).sort();
  const expected = [...CATEGORY_KEYS].sort();
  if (keys.join(",") !== expected.join(",")) {
    errors.push(`${where}: categories must be exactly ${CATEGORY_KEYS.join(", ")}`);
  }
  for (const key of CATEGORY_KEYS) {
    const entry = value[key];
    if (!entry || typeof entry !== "object") {
      errors.push(`${where}.${key}: missing`);
      continue;
    }
    if (!isScore(entry.score)) errors.push(`${where}.${key}.score: must be an integer 1-10`);
    if (typeof entry.evidence !== "string" || !entry.evidence.trim()) {
      errors.push(`${where}.${key}.evidence: must be a non-empty string`);
    }
  }
}

export function validateAnswer(role, value) {
  const errors = [];
  if (!value || typeof value !== "object") return ["answer is not a JSON object"];
  if (value.role !== role) errors.push(`role must be "${role}"`);
  if (!Number.isInteger(value.round)) errors.push("round must be an integer");

  if (role === "absolute") {
    if (typeof value.notes !== "string") errors.push("notes must be a string");
    checkCategories(value.categories, "categories", errors);
    if (!Array.isArray(value.top_issues)) {
      errors.push("top_issues must be an array");
    } else {
      value.top_issues.forEach((issue, index) => {
        for (const key of ["state", "viewport", "expected", "actual"]) {
          if (typeof issue?.[key] !== "string" || !issue[key].trim()) {
            errors.push(`top_issues[${index}].${key}: must be a non-empty string`);
          }
        }
      });
    }
  }

  if (role === "comparative") {
    if (!CATEGORY_KEYS.includes(value.category)) {
      errors.push(`category must be one of ${CATEGORY_KEYS.join(", ")}`);
    }
    if (!["A", "B", "tie"].includes(value.winner)) errors.push('winner must be "A", "B" or "tie"');
    if (!["clear", "slight"].includes(value.confidence)) errors.push('confidence must be "clear" or "slight"');
    if (typeof value.citation !== "string" || !value.citation.trim()) {
      errors.push("citation: a win or a tie needs a citation naming a panel file");
    } else {
      if (!PANEL_REFERENCE.test(value.citation)) {
        errors.push("citation: must name the panel it came from, as A/01.png or B/01.png");
      }
      if (!/\d/.test(value.citation)) {
        errors.push("citation: must carry a specific value, not just a description");
      }
    }
    if (typeof value.why !== "string" || !value.why.trim()) {
      errors.push("why: must be a non-empty string");
    }
  }

  if (role === "defect-hunter") {
    if (typeof value.notes !== "string") errors.push("notes must be a string");
    if (!Array.isArray(value.defects) || value.defects.length === 0) {
      errors.push("defects: a non-empty array is required");
    } else {
      value.defects.forEach((defect, index) => {
        for (const key of ["state", "viewport", "expected", "actual"]) {
          if (typeof defect?.[key] !== "string" || !defect[key].trim()) {
            errors.push(`defects[${index}].${key}: must be a non-empty string`);
          }
        }
        if (!["defect", "taste"].includes(defect?.kind)) {
          errors.push(`defects[${index}].kind: must be "defect" or "taste"`);
        }
        if (!["high", "medium", "low"].includes(defect?.severity)) {
          errors.push(`defects[${index}].severity: must be high, medium or low`);
        }
      });
    }
  }

  return errors;
}
