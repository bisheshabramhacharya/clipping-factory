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
  const download = document.getElementById("review-download");
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
  let items = [], index = 0, decisions = load(), lastFocused = null;
  let announced = false, toastTimer = null;

  function load() {
    try { return JSON.parse(localStorage.getItem(STORE) || "{}") || {}; } catch (_) { return {}; }
  }
  function save() {
    try { localStorage.setItem(STORE, JSON.stringify(decisions)); } catch (_) {}
  }
  function key(src) {
    try { return new URL(src, location.href).pathname; } catch (_) { return src; }
  }
  // .rank also hosts the decision badge; keep only its own label text.
  function rankText(card) {
    const rank = card.querySelector(".rank");
    if (!rank) return "";
    return [...rank.childNodes]
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent).join("").trim();
  }
  function collect() {
    items = [...root.querySelectorAll("article.clip")].flatMap((card) => {
      const player = card.querySelector(".preview video");
      if (!player) return [];
      const dl = card.querySelector(".actions .action-button[download]");
      return [{
        card,
        player,
        key: key(player.src),
        title: (card.querySelector("h3") || {}).textContent || "Untitled clip",
        rank: rankText(card),
        reason: (card.querySelector(".why") || {}).textContent || "",
        score: (card.querySelector(".badge.score") || {}).textContent || "",
        downloadHref: dl ? dl.getAttribute("href") : "",
        downloadName: dl ? dl.getAttribute("download") : "",
      }];
    });
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
  }
  function updateCounts() {
    const keys = new Set(items.map((i) => i.key));
    const total = root.querySelectorAll("article.clip").length;
    const t = { keep: 0, maybe: 0, skip: 0 };
    for (const [k, v] of Object.entries(decisions)) if (keys.has(k) && v in t) t[v]++;
    const readyText = total > items.length ? `${items.length} of ${total} ready` : `${items.length} ready`;
    counts.textContent = ` · ${readyText} · ${t.keep} keep · ${t.maybe} maybe · ${t.skip} skip`;
  }
  function show(autoplay = true) {
    const item = items[index];
    if (!item) return;
    if (video.src !== item.player.src) {
      video.pause();
      video.src = item.player.src;
    }
    title.textContent = item.title;
    meta.textContent = [item.rank, item.score].filter(Boolean).join(" · ");
    reason.textContent = item.reason;
    if (item.downloadHref) {
      download.href = item.downloadHref;
      download.download = item.downloadName || "";
      download.classList.remove("hidden");
    } else {
      download.classList.add("hidden");
    }
    progress.textContent = `${index + 1} / ${items.length}`;
    prevBtn.disabled = index === 0;
    nextBtn.disabled = index === items.length - 1;
    updateCounts();
    for (const b of buttons) {
      const selected = b.dataset.reviewDecision === decisions[item.key];
      b.classList.toggle("active", selected);
      b.setAttribute("aria-pressed", String(selected));
    }
    if (autoplay) video.play().catch(() => {});
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
    theater.hidden = true;
    shell.inert = false;
    aiBackdrop.inert = false;
    document.body.style.overflow = "";
    paint();
    if (lastFocused && lastFocused.isConnected) lastFocused.focus();
  }
  function decide(value) {
    const item = items[index];
    if (!item) return;
    decisions[item.key] = value;
    save();
    paint();
    if (index < items.length - 1) { index++; show(true); }
    else show(false);
  }
  function move(delta) {
    const next = Math.max(0, Math.min(items.length - 1, index + delta));
    if (next !== index) { index = next; show(true); }
  }
  function focusables() {
    return [...theater.querySelectorAll("button, video, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])")]
      .filter((element) => !element.disabled && element.getClientRects().length > 0);
  }
  function typing(target) {
    return target instanceof Element && (target.matches("input,select,textarea") || target.isContentEditable);
  }

  openBtn.addEventListener("click", openReview);
  closeBtn.addEventListener("click", closeReview);
  prevBtn.addEventListener("click", () => move(-1));
  nextBtn.addEventListener("click", () => move(1));
  toastOpen.addEventListener("click", openReview);
  for (const b of buttons) b.addEventListener("click", () => decide(b.dataset.reviewDecision));

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
      const elements = focusables();
      if (!elements.length) return;
      const first = elements[0], last = elements[elements.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault(); last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault(); first.focus();
      }
      return;
    }
    if (typing(event.target)) return;
    if (event.key === "Escape") closeReview();
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
