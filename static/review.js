// Swipe Review theater — keyboard-first triage over rendered clips.
// Decisions live in localStorage, keyed by clip pathname. They never leave the browser.
(() => {
  const STORE = "cf-review-decisions-v1";
  const root = document.getElementById("clips");
  const results = document.getElementById("results-state");
  const openBtn = document.getElementById("review-clips-btn");
  const exportBtn = document.getElementById("export-kept-btn");
  const theater = document.getElementById("review-theater");
  const video = document.getElementById("review-video");
  const title = document.getElementById("review-title");
  const reason = document.getElementById("review-reason");
  const progress = document.getElementById("review-progress");
  const counts = document.getElementById("review-counts");
  const buttons = [...theater.querySelectorAll("[data-review-decision]")];
  const closeBtn = document.getElementById("review-close");
  let items = [], index = 0, decisions = load(), lastFocused = null, syncTimer = null;

  // Server-side persistence: decisions survive the browser and drive the
  // kept-clips ZIP export. UI verbs (keep/skip) map to server verdicts.
  function projectId() { return localStorage.getItem("cf-project"); }
  function toServer(map) {
    const out = {};
    for (const [k, v] of Object.entries(map)) {
      if (v === "keep" || v === "maybe" || v === "skip") {
        out[k] = { verdict: v === "keep" ? "kept" : v === "skip" ? "skipped" : "maybe", updated_at: new Date().toISOString() };
      }
    }
    return { decisions: out };
  }
  function sync() {
    clearTimeout(syncTimer);
    syncTimer = setTimeout(() => {
      const pid = projectId();
      if (!pid) return;
      fetch(`/api/projects/${pid}/decisions`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(toServer(decisions)),
      }).catch(() => {});
    }, 400);
  }
  async function pullServerDecisions() {
    const pid = projectId();
    if (!pid) return;
    try {
      const res = await fetch(`/api/projects/${pid}/decisions`);
      if (!res.ok) return;
      const data = await res.json();
      let changed = false;
      for (const [k, entry] of Object.entries(data.decisions || {})) {
        if (!decisions[k]) {
          decisions[k] = entry.verdict === "kept" ? "keep" : entry.verdict === "skipped" ? "skip" : "maybe";
          changed = true;
        }
      }
      if (changed) { save(); paint(); updateCounts(); }
    } catch (_) {}
  }
  function updateExportButton(keptCount) {
    if (!exportBtn) return;
    exportBtn.hidden = !items.length || !keptCount || !projectId();
    if (!exportBtn.hidden) exportBtn.textContent = `Download kept (${keptCount})`;
  }

  function load() {
    try { return JSON.parse(localStorage.getItem(STORE) || "{}") || {}; } catch (_) { return {}; }
  }
  function save() {
    try { localStorage.setItem(STORE, JSON.stringify(decisions)); } catch (_) {}
  }
  function key(src) {
    try { return new URL(src, location.href).pathname; } catch (_) { return src; }
  }
  function collect() {
    items = [...root.querySelectorAll("article.clip")].flatMap((card) => {
      const player = card.querySelector(".preview video");
      return player ? [{
        card,
        player,
        key: key(player.src),
        title: (card.querySelector("h3") || {}).textContent || "Untitled clip",
        reason: (card.querySelector(".why") || {}).textContent || "",
      }] : [];
    });
    openBtn.hidden = !items.length;
    paint();
    updateCounts();
    if (!theater.hidden) {
      if (!items.length) closeReview();
      else { index = Math.min(index, items.length - 1); show(false); }
    }
  }
  function paint() {
    for (const item of items) {
      const decision = decisions[item.key];
      item.card.classList.toggle("review-skip", decision === "skip");
      const old = item.card.querySelector(".review-badge");
      if (!decision) { if (old) old.remove(); continue; }
      if (old && old.textContent === decision) continue;
      if (old) old.remove();
      const badge = document.createElement("span");
      badge.className = "review-badge";
      badge.textContent = decision;
      badge.dataset.decision = decision; // visual-state only: lets CSS color per decision
      const rank = item.card.querySelector(".rank");
      if (rank) rank.appendChild(badge);
    }
  }
  function updateCounts() {
    const keys = new Set(items.map((i) => i.key));
    const t = { keep: 0, maybe: 0, skip: 0 };
    for (const [k, v] of Object.entries(decisions)) if (keys.has(k) && v in t) t[v]++;
    counts.textContent = ` · ${t.keep} keep · ${t.maybe} maybe · ${t.skip} skip`;
    updateExportButton(t.keep);
  }
  function show(autoplay = true) {
    const item = items[index];
    if (!item) return;
    if (video.src !== item.player.src) video.src = item.player.src;
    title.textContent = item.title;
    reason.textContent = item.reason;
    progress.textContent = `${index + 1} / ${items.length}`;
    updateCounts();
    for (const b of buttons) b.classList.toggle("active", b.dataset.reviewDecision === decisions[item.key]);
    if (autoplay) video.play().catch(() => {});
  }
  function openReview() {
    collect();
    if (!items.length) return;
    lastFocused = document.activeElement;
    const first = items.findIndex((i) => !decisions[i.key]);
    index = first < 0 ? 0 : first;
    theater.hidden = false;
    document.body.style.overflow = "hidden";
    closeBtn.focus();
    show(true);
  }
  function closeReview() {
    video.pause();
    theater.hidden = true;
    document.body.style.overflow = "";
    paint();
    if (lastFocused && lastFocused.isConnected) lastFocused.focus();
  }
  function decide(value) {
    const item = items[index];
    if (!item) return;
    decisions[item.key] = value;
    save();
    sync();
    paint();
    if (index < items.length - 1) { index++; show(true); }
    else show(false);
  }
  function move(delta) {
    const next = Math.max(0, Math.min(items.length - 1, index + delta));
    if (next !== index) { index = next; show(true); }
  }
  function typing(target) {
    return target instanceof Element && (target.matches("input,select,textarea") || target.isContentEditable);
  }

  openBtn.addEventListener("click", openReview);
  closeBtn.addEventListener("click", closeReview);
  for (const b of buttons) b.addEventListener("click", () => decide(b.dataset.reviewDecision));
  if (exportBtn) exportBtn.addEventListener("click", () => {
    const pid = projectId();
    if (pid) window.location.href = `/api/projects/${pid}/export?verdicts=kept`;
  });
  pullServerDecisions();

  document.addEventListener("keydown", (event) => {
    if (typing(event.target)) return;
    if (theater.hidden) {
      if ((event.key === "r" || event.key === "R") && !results.classList.contains("hidden") && items.length) {
        event.preventDefault();
        openReview();
      }
      return;
    }
    if (event.key === "Escape") closeReview();
    else if (event.key === "ArrowLeft") { event.preventDefault(); move(-1); }
    else if (event.key === "ArrowRight") { event.preventDefault(); move(1); }
    else if (event.key === " ") { event.preventDefault(); video.paused ? video.play().catch(() => {}) : video.pause(); }
    else if (event.key === "1") decide("keep");
    else if (event.key === "2") decide("maybe");
    else if (event.key === "3") decide("skip");
  });

  // Keep the button in sync when app.js re-renders the results list.
  const observer = new MutationObserver(collect);
  observer.observe(root, { childList: true, subtree: true });
})();
