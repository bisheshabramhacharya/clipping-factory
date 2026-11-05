// Swipe Review theater: keyboard-first triage over rendered clips.
// Decisions live in localStorage, keyed by clip pathname. They never leave the browser.
(() => {
  const STORE = "cf-review-decisions-v1";
  const root = document.getElementById("clips");
  const results = document.getElementById("results-state");
  const cancelBtn = document.getElementById("cancel-btn");
  const openBtn = document.getElementById("review-clips-btn");
  const theater = document.getElementById("review-theater");
  const video = document.getElementById("review-video");
  const title = document.getElementById("review-title");
  const meta = document.getElementById("review-meta");
  const reason = document.getElementById("review-reason");
  const caption = document.getElementById("review-caption");
  const captionLabel = document.getElementById("review-caption-label");
  const captionMore = document.getElementById("review-caption-more");
  const copy = document.querySelector(".review-copy");
  const download = document.getElementById("review-download");
  const playBtn = document.getElementById("review-play");
  const progress = document.getElementById("review-progress");
  const counts = document.getElementById("review-counts");
  const buttons = [...theater.querySelectorAll("[data-review-decision]")];
  const prevBtn = document.getElementById("review-prev");
  const nextBtn = document.getElementById("review-next");
  const closeBtn = document.getElementById("review-close");
  const toast = document.getElementById("review-toast");
  const toastText = document.getElementById("review-toast-text");
  const toastOpen = document.getElementById("review-toast-open");
  const shell = document.querySelector(".shell");
  const aiBackdrop = document.getElementById("modal-backdrop");
  const dots = document.getElementById("review-dots");
  const done = document.getElementById("review-done");
  const doneTitle = document.getElementById("review-done-title");
  const doneStats = document.getElementById("review-done-stats");
  const doneKept = document.getElementById("review-done-kept");
  const doneDownload = document.getElementById("review-done-download");
  const doneClose = document.getElementById("review-done-close");
  let items = [], clipTotal = 0, index = 0, decisions = load(), lastFocused = null;
  let announced = false, toastTimer = null;

  function load() {
    try { return JSON.parse(localStorage.getItem(STORE) || "{}") || {}; } catch (_) { return {}; }
  }
  function save() {
    try { localStorage.setItem(STORE, JSON.stringify(decisions)); } catch (_) {}
  }
  // The clip records and their card elements come from app.js's bridge, so
  // the theater never has to parse this file's own DOM.
  function collect() {
    const bridge = window.cfStudio;
    const data = bridge && bridge.reviewItems ? bridge.reviewItems() : { items: [], total: 0 };
    items = data.items;
    clipTotal = data.total;
    openBtn.hidden = !items.length;
    paint();
    if (!items.length) {
      announced = false;
      hideToast();
      if (!theater.hidden) closeReview();
      return;
    }
    // First clip landing mid-run: offer the theater without stealing focus.
    // cancel-btn is visible exactly while isProcessing(project.status) holds.
    const running = !cancelBtn.classList.contains("hidden");
    if (!announced && theater.hidden && running) {
      announced = true;
      showToast("Clip 1 ready · press R to review");
    }
    if (!theater.hidden) {
      index = Math.min(index, items.length - 1);
      show(false);
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
    // Mirror decisions onto the studio's clip rail.
    for (const rail of document.querySelectorAll(".rail-item[data-review-key]")) {
      const decision = decisions[rail.dataset.reviewKey];
      if (decision) rail.dataset.decision = decision;
      else delete rail.dataset.decision;
    }
    for (const group of document.querySelectorAll(".rail-triage")) {
      const decision = decisions[group.dataset.reviewKey] || null;
      if (decision) group.dataset.decision = decision;
      else delete group.dataset.decision;
      for (const button of group.querySelectorAll("[data-rail-decision]")) {
        const selected = button.dataset.railDecision === decision;
        button.classList.toggle("on", selected);
        button.setAttribute("aria-pressed", String(selected));
      }
    }
  }
  function tally() {
    const t = { keep: 0, maybe: 0, skip: 0 };
    for (const item of items) { const d = decisions[item.key]; if (d in t) t[d]++; }
    return t;
  }
  function paintDots() {
    dots.innerHTML = "";
    items.forEach((item, i) => {
      const dot = document.createElement("span");
      dot.className = "review-dot" + (i === index ? " current" : "");
      if (decisions[item.key]) dot.dataset.decision = decisions[item.key];
      dots.appendChild(dot);
    });
  }
  function updateCounts() {
    const keys = new Set(items.map((i) => i.key));
    const t = { keep: 0, maybe: 0, skip: 0 };
    for (const [k, v] of Object.entries(decisions)) if (keys.has(k) && v in t) t[v]++;
    const readyText = clipTotal > items.length ? `${items.length} of ${clipTotal} ready · ` : "";
    counts.innerHTML = `${readyText}<b>${t.keep}</b> keep · <b>${t.maybe}</b> maybe · <b>${t.skip}</b> skip`;
    paintDots();
  }
  function show(autoplay = true) {
    const item = items[index];
    if (!item) return;
    if (video.src !== item.player.src) {
      video.pause();
      video.src = item.player.src;
    }
    title.textContent = item.title;
    const captionText = item.caption || "";
    caption.textContent = captionText;
    caption.hidden = !captionText;
    captionLabel.hidden = !captionText;
    copy.classList.remove("expanded");
    captionMore.textContent = "Show full transcript";
    captionMore.hidden = true;
    if (captionText) {
      requestAnimationFrame(() => {
        captionMore.hidden = caption.scrollHeight <= caption.clientHeight + 1;
      });
    }
    meta.textContent = "";
    if (item.rank) meta.append(`${item.rank} `);
    if (item.score) {
      const lockup = document.createElement("span");
      lockup.className = "score";
      const score = document.createElement("span");
      score.className = "score-value";
      score.textContent = item.score;
      lockup.append(score);
      if (item.scoreScale) {
        const scale = document.createElement("span");
        scale.className = "score-scale";
        scale.textContent = item.scoreScale;
        lockup.append(scale);
      }
      meta.append(lockup);
    }
    theater.classList.toggle("decided", Boolean(decisions[item.key]));
    reason.textContent = item.reason;
    if (item.downloadHref) {
      download.href = item.downloadHref;
      download.download = item.downloadName || "";
      download.classList.remove("hidden");
    } else {
      download.classList.add("hidden");
    }
    progress.textContent = `Clip ${index + 1} of ${items.length}`;
    paintTransport();
    prevBtn.disabled = index === 0;
    nextBtn.disabled = index === items.length - 1;
    updateCounts();
    for (const b of buttons) {
      const selected = b.dataset.reviewDecision === decisions[item.key];
      b.classList.toggle("active", selected);
      b.setAttribute("aria-pressed", String(selected));
    }
    theater.classList.toggle("decided", Boolean(decisions[item.key]));
    if (autoplay) video.play().catch(() => {});
  }
  function paintTransport() {
    const playing = !video.paused && !video.ended;
    playBtn.setAttribute("aria-label", playing ? "Pause" : "Play");
    playBtn.title = playing ? "Pause" : "Play";
    playBtn.replaceChildren(window.cfStudio.icon(playing ? "pause" : "play"));
  }
  function showToast(text) {
    toastText.textContent = text;
    toast.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(hideToast, 12000);
  }
  function hideToast() {
    toast.hidden = true;
    clearTimeout(toastTimer);
    toastTimer = null;
  }
  function openReview() {
    if (!aiBackdrop.classList.contains("hidden")) return;
    collect();
    if (!items.length) return;
    hideToast();
    hideDone();
    lastFocused = document.activeElement;
    const first = items.findIndex((i) => !decisions[i.key]);
    index = first < 0 ? 0 : first;
    theater.hidden = false;
    shell.inert = true;
    aiBackdrop.inert = true;
    document.body.style.overflow = "hidden";
    closeBtn.focus();
    show(true);
  }
  function closeReview() {
    video.pause();
    hideDone();
    theater.hidden = true;
    shell.inert = false;
    aiBackdrop.inert = false;
    document.body.style.overflow = "";
    paint();
    if (lastFocused && lastFocused.isConnected) lastFocused.focus();
  }
  function setDecision(key, value) {
    decisions[key] = value;
    save();
    paint();
  }
  function decide(value) {
    const item = items[index];
    if (!item) return;
    setDecision(item.key, value);
    // Next undecided clip after this one, wrapping; none left → summary.
    const order = items.map((_, i) => (index + 1 + i) % items.length);
    const next = order.find((i) => !decisions[items[i].key]);
    if (next === undefined) { show(false); showDone(); return; }
    index = next;
    show(true);
  }
  function showDone() {
    video.pause();
    const t = tally();
    const kept = items.filter((i) => decisions[i.key] === "keep");
    doneTitle.textContent = kept.length
      ? `${kept.length} clip${kept.length === 1 ? "" : "s"} worth posting`
      : "Nothing kept this time";
    doneStats.innerHTML = "";
    for (const [label, n] of [["Keep", t.keep], ["Maybe", t.maybe], ["Skip", t.skip]]) {
      const stat = document.createElement("div");
      stat.className = "review-done-stat";
      stat.dataset.decision = label.toLowerCase();
      stat.classList.toggle("zero", n === 0);
      stat.innerHTML = "<strong></strong><span></span>";
      stat.firstChild.textContent = String(n);
      stat.lastChild.textContent = label;
      doneStats.appendChild(stat);
    }
    doneKept.innerHTML = "";
    for (const item of kept) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "review-done-row";
      row.append(window.cfStudio.icon("check"), document.createTextNode(item.fullTitle || item.title));
      row.setAttribute("aria-label", `Open clip: ${item.fullTitle || item.title}`);
      row.addEventListener("click", () => {
        hideDone();
        index = items.indexOf(item);
        show(true);
      });
      doneKept.appendChild(row);
    }
    doneDownload.hidden = kept.length === 0;
    done.hidden = false;
    (kept.length ? doneDownload : doneClose).focus();
  }
  function hideDone() { done.hidden = true; }
  function downloadKept() {
    const kept = items.filter((i) => decisions[i.key] === "keep" && i.downloadHref);
    kept.forEach((item, n) => setTimeout(() => {
      const a = document.createElement("a");
      a.href = item.downloadHref;
      a.download = item.downloadName || "";
      document.body.appendChild(a);
      a.click();
      a.remove();
    }, n * 400));
  }
  function move(delta) {
    const next = Math.max(0, Math.min(items.length - 1, index + delta));
    if (next !== index) { index = next; show(true); }
  }
  function typing(target) {
    return target instanceof Element && (target.matches("input,select,textarea") || target.isContentEditable);
  }

  captionMore.addEventListener("click", () => {
    const expanded = copy.classList.toggle("expanded");
    captionMore.textContent = expanded ? "Show less" : "Show full transcript";
  });
  openBtn.addEventListener("click", openReview);
  playBtn.addEventListener("click", () => { video.paused ? video.play().catch(() => {}) : video.pause(); });
  video.addEventListener("play", paintTransport);
  video.addEventListener("pause", paintTransport);
  closeBtn.addEventListener("click", closeReview);
  prevBtn.addEventListener("click", () => move(-1));
  nextBtn.addEventListener("click", () => move(1));
  toastOpen.addEventListener("click", openReview);
  for (const b of buttons) b.addEventListener("click", () => decide(b.dataset.reviewDecision));
  doneDownload.addEventListener("click", downloadKept);
  doneClose.addEventListener("click", closeReview);
  document.addEventListener("cf-rail-rendered", paint);
  document.getElementById("clip-rail-list").addEventListener("click", (event) => {
    const button = event.target.closest("[data-rail-decision]");
    const group = button && button.closest(".rail-triage");
    if (group) setDecision(group.dataset.reviewKey, button.dataset.railDecision);
  });

  document.addEventListener("keydown", (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey) return; // don't eat browser shortcuts (Ctrl+R refresh, etc.)
    if (theater.hidden) {
      if (typing(event.target)) return;
      if ((event.key === "r" || event.key === "R") && aiBackdrop.classList.contains("hidden") && !results.classList.contains("hidden") && items.length) {
        event.preventDefault();
        openReview();
      }
      return;
    }
    if (event.key === "Tab") {
      // app.js owns the shared dialog focus trap.
      if (window.cfStudio && window.cfStudio.trapTab) window.cfStudio.trapTab(event, theater);
      return;
    }
    if (typing(event.target)) return;
    if (event.key === "Escape") { closeReview(); return; }
    if (!done.hidden) return;
    else if (event.key === "ArrowLeft" || event.key === "k" || event.key === "K") { event.preventDefault(); move(-1); }
    else if (event.key === "ArrowRight" || event.key === "j" || event.key === "J") { event.preventDefault(); move(1); }
    else if (event.key === " ") { event.preventDefault(); video.paused ? video.play().catch(() => {}) : video.pause(); }
    else if (event.key === "1") decide("keep");
    else if (event.key === "2") decide("maybe");
    else if (event.key === "3") decide("skip");
  });

  // Keep the button in sync when app.js re-renders the results list.
  const observer = new MutationObserver(collect);
  observer.observe(root, { childList: true, subtree: true });
})();
