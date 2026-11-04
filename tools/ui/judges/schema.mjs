import { CATEGORY_KEYS } from "./categories.mjs";

const isScore = (value) => Number.isInteger(value) && value >= 1 && value <= 10;

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
  if (typeof value.notes !== "string") errors.push("notes must be a string");

  if (role === "absolute") {
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
    for (const label of ["A", "B", "C", "D"]) {
      if (!value.sources?.[label]) {
        errors.push(`sources.${label}: missing`);
        continue;
      }
      checkCategories(value.sources[label].categories, `sources.${label}.categories`, errors);
    }
    if (!Array.isArray(value.pairs) || value.pairs.length < 6) {
      errors.push("pairs: at least six pair entries are required");
    } else {
      value.pairs.forEach((pair, index) => {
        for (const key of ["higher", "lower", "category", "note"]) {
          if (typeof pair?.[key] !== "string" || !pair[key].trim()) {
            errors.push(`pairs[${index}].${key}: must be a non-empty string`);
          }
        }
        if (!CATEGORY_KEYS.includes(pair?.category)) {
          errors.push(`pairs[${index}].category: must be one of ${CATEGORY_KEYS.join(", ")}`);
        }
        if (!["A", "B", "C", "D"].includes(pair?.higher) || !["A", "B", "C", "D"].includes(pair?.lower)) {
          errors.push(`pairs[${index}]: higher/lower must be A-D`);
        }
      });
    }
  }

  if (role === "defect-hunter") {
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
