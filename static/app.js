/* Clipping Factory studio — one screen, three states, SSE-driven. */
(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const STAGE_LABELS = {
    inspecting: "1. Inspect",
    extracting_audio: "2. Extract audio",
    transcribing: "3. Transcribe",
    selecting_candidates: "4. Find moments",
    validating_candidates: "5. Validate",
    analyzing_layout: "6. Analyze framing",
    rendering: "7. Render",
  };
  const STAGE_ORDER = Object.keys(STAGE_LABELS);

  let projectId = localStorage.getItem("cf-project") || null;
  let view = null;
  let sse = null;
  let refetchTimer = null;
  let elapsedTimer = null;
  let uploadXhr = null;
  let uploadCancelRequested = false;
  let cancellationPending = false;
  let retryPending = false;
  let actionMessageKind = null;
  let modalReturnFocus = null;
  let liveProgress = null; // {stage, progress, detail}
  // Last style/color the user applied — the starting point for new restyles.
  let captionStyle = localStorage.getItem("cf-caption-style") || "impact";
  let accentColor = localStorage.getItem("cf-accent-color") || "#FFDD00";
  let captionFonts = [];
  let captionDefaultFont = "Inter";
  const ACCENT_PRESETS = [
    { name: "Sun yellow", color: "#FFDD00" },
    { name: "Lime", color: "#7CFF4F" },
    { name: "Coral", color: "#FF4F4F" },
    { name: "Sky blue", color: "#4FB5FF" },
    { name: "Violet", color: "#C77DFF" },
    { name: "Orange", color: "#FF9F1C" },
  ];
  const clipRev = {}; // clip id → cache-busting token after a restyle
  const restyleState = {}; // clip id → {busy, kind, message, draft}

  function isProcessing(status) { return STAGE_ORDER.includes(status); }

  function formatApiError(payload, status, fallback) {
    const message = payload && (payload.error || payload.message);
    if (message) return message;
    return status ? `${fallback} (${status})` : fallback;
  }

  async function requestJson(url, options = {}, fallback = "Request failed.") {
    const res = await fetch(url, options);
    let payload = null;
    try {
      payload = await res.json();
    } catch {
      if (res.ok) throw new Error(fallback);
    }
    if (!res.ok) throw new Error(formatApiError(payload, res.status, fallback));
    return payload;
  }

  function xhrError(xhr, fallback) {
    let payload = null;
    try { payload = JSON.parse(xhr.responseText); } catch {}
    return formatApiError(payload, xhr.status, fallback);
  }

  function showActionMessage(message, kind = "error") {
    const banner = $("action-banner");
    if (!banner) return;
    actionMessageKind = kind;
    banner.textContent = message;
    banner.className = `banner action ${kind === "cancel" ? "notice" : kind}`;
    banner.classList.remove("hidden");
  }

  function clearActionMessage(kind = null) {
    if (kind && actionMessageKind !== kind) return;
    const banner = $("action-banner");
    if (!banner) return;
    actionMessageKind = null;
    banner.classList.add("hidden");
  }

  function accentLabel(color) {
    const normalized = String(color || "").toUpperCase();
    const preset = ACCENT_PRESETS.find((entry) => entry.color === normalized);
    return preset ? `${preset.name} (${preset.color})` : `Custom color (${normalized})`;
  }

  // ------------------------------------------------------------------ setup
  async function loadSetup() {
    try {
      const s = await requestJson("/api/setup", {}, "Couldn't reconnect to the local server.");
      const problems = [];
      if (!s.ffmpeg) problems.push("FFmpeg was not found. Install it and restart.");
      else if (!s.ffmpeg_ass) problems.push("This FFmpeg build cannot burn captions. macOS: brew install ffmpeg-full, then restart.");
      if (!s.ffprobe) problems.push("FFprobe was not found. It ships with FFmpeg.");
      if (!s.whisper_ok) problems.push("whisper-cli was not found. macOS: brew install whisper-cpp, or set CF_WHISPER_BIN.");
      if (!s.model_ok) problems.push(`Transcription model missing (~148 MB). Download ggml-base.en.bin into ${s.data_dir}/models/`);
      if (s.disk_free_gb !== null && s.disk_free_gb < 2) problems.push(`Low disk space: ${s.disk_free_gb.toFixed(1)} GB free.`);
      const banner = $("setup-banner");
      captionDefaultFont = s.caption_font || captionDefaultFont;
      if (Array.isArray(s.caption_fonts) && s.caption_fonts.length) captionFonts = s.caption_fonts;
      if (problems.length) {
        banner.textContent = problems.join("\n");
        banner.classList.remove("hidden");
      } else {
        banner.classList.add("hidden");
      }
      clearActionMessage("reconnect");
      if (view) render();
    } catch {
      showActionMessage("Couldn't reconnect to the local server. Refresh to try again.", "reconnect");
    }
  }

  async function loadSettings() {
    try {
      const s = await requestJson("/api/settings/ai", {}, "Couldn't reconnect to the local server.");
      const dot = $("ai-dot");
      dot.className = "dot";
      if (s.provider === "offline") { dot.classList.add("offline"); $("ai-label").textContent = "Local ranking"; }
      else if (s.connected) { dot.classList.add("on"); $("ai-label").textContent = `${s.provider} · ${s.model}`; }
      else { $("ai-label").textContent = "AI connection"; }
      $("provider").value = s.provider || "openai";
      $("model").value = s.model || "";
      syncModalRows();
      clearActionMessage("reconnect");
    } catch {
      $("ai-label").textContent = "AI connection unavailable";
      showActionMessage("Couldn't reconnect to the local server. Refresh to try again.", "reconnect");
    }
  }

  // ------------------------------------------------------------------ upload
  function wireUpload() {
    const drop = $("drop");
    $("choose-btn").addEventListener("click", () => $("file-input").click());
    $("file-input").addEventListener("change", (e) => {
      if (e.target.files[0]) uploadFile(e.target.files[0]);
    });
    ["dragenter", "dragover"].forEach((ev) =>
      drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.add("dragover"); })
    );
    ["dragleave", "drop"].forEach((ev) =>
      drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.remove("dragover"); })
    );
    drop.addEventListener("drop", (e) => {
      const f = e.dataTransfer.files && e.dataTransfer.files[0];
      if (f) uploadFile(f);
    });
  }

  function wireUploadOptions() {
    const swatchGroup = $("upload-swatches");
    const swatches = ACCENT_PRESETS.map(({ name, color }) => {
      const swatch = document.createElement("button");
      swatch.type = "button";
      swatch.className = "upload-swatch";
      swatch.dataset.color = color;
      swatch.style.setProperty("--swatch", color);
      swatch.setAttribute("aria-label", `Use ${name} caption highlight, ${color}`);
      swatch.title = `${name} (${color})`;
      swatchGroup.appendChild(swatch);
      return swatch;
    });
    if (!swatches.some((swatch) => swatch.dataset.color === accentColor)) {
      accentColor = "#FFDD00";
    }
    function selectColor(color) {
      accentColor = color;
      $("upload-accent-color").value = color;
      $("upload-accent-hex").textContent = `${accentLabel(color)} selected`;
      for (const swatch of swatches) {
        const selected = swatch.dataset.color === color;
        swatch.classList.toggle("active", selected);
        swatch.setAttribute("aria-pressed", String(selected));
      }
      document.querySelector('input[name="accent-mode"][value="manual"]').checked = true;
    }
    for (const swatch of swatches) {
      swatch.addEventListener("click", () => selectColor(swatch.dataset.color));
    }
    selectColor(accentColor);
  }

  function uploadFile(file) {
    if (!/\.(mp4|m4v)$/i.test(file.name)) {
      showActionMessage("Attach an .mp4 file. Other containers are not supported yet.");
      return;
    }
    if (uploadXhr) return;
    $("drop").classList.add("hidden");
    $("upload-progress").classList.remove("hidden");
    $("cancel-upload-btn").disabled = false;
    $("cancel-upload-btn").textContent = "Cancel upload";
    uploadCancelRequested = false;
    setUploadPhase(`Uploading ${file.name}…`, 0);

    const form = new FormData();
    const framingMode = document.querySelector('input[name="framing-mode"]:checked').value;
    const accentMode = document.querySelector('input[name="accent-mode"]:checked').value;
    form.append("framing_mode", framingMode);
    form.append("accent_mode", accentMode);
    form.append("accent_color", $("upload-accent-color").value.toUpperCase());
    form.append("file", file, file.name);
    const xhr = new XMLHttpRequest();
    uploadXhr = xhr;
    xhr.open("POST", "/api/projects");
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) {
        const percent = Math.round((e.loaded / e.total) * 100);
        setUploadPhase(`Uploading ${file.name}… ${percent}%`, percent);
      }
    };
    xhr.upload.onload = () => {
      if (uploadXhr === xhr) setUploadPhase(`Preparing ${file.name}…`, 100);
    };
    xhr.onload = () => {
      if (uploadXhr !== xhr) return;
      uploadXhr = null;
      if (xhr.status >= 200 && xhr.status < 300) {
        try {
          const v = JSON.parse(xhr.responseText);
          projectId = v.project.id;
          localStorage.setItem("cf-project", projectId);
          view = v;
          clearActionMessage();
          connectSse();
          render();
        } catch {
          resetToEmpty({ message: "The server returned an invalid project response. Try again." });
        }
      } else {
        resetToEmpty({ message: xhrError(xhr, "Upload failed.") });
      }
    };
    xhr.onabort = () => {
      if (uploadXhr !== xhr) return;
      uploadXhr = null;
      const message = uploadCancelRequested
        ? "Upload cancelled. Choose another MP4."
        : "Upload stopped before it finished. Try again.";
      resetToEmpty({ message, kind: uploadCancelRequested ? "notice" : "error" });
    };
    xhr.onerror = () => {
      if (uploadXhr !== xhr) return;
      uploadXhr = null;
      resetToEmpty({ message: "Upload failed. Check that the local server is still running." });
    };
    xhr.send(form);
  }

  function setUploadPhase(label, percent) {
    $("upload-label").textContent = label;
    if (percent != null) {
      $("upload-bar").style.width = `${percent}%`;
      $("upload-bar").parentElement.setAttribute("aria-valuenow", String(percent));
    }
  }

  function cancelUpload() {
    if (!uploadXhr) return;
    uploadCancelRequested = true;
    $("cancel-upload-btn").disabled = true;
    $("cancel-upload-btn").textContent = "Cancelling…";
    setUploadPhase("Cancelling upload…", null);
    uploadXhr.abort();
  }

  function resetToEmpty({ message = null, kind = "error" } = {}) {
    projectId = null;
    view = null;
    liveProgress = null;
    cancellationPending = false;
    retryPending = false;
    uploadCancelRequested = false;
    localStorage.removeItem("cf-project");
    if (sse) { sse.close(); sse = null; }
    $("drop").classList.remove("hidden");
    $("upload-progress").classList.add("hidden");
    $("upload-bar").style.width = "0%";
    $("upload-bar").parentElement.setAttribute("aria-valuenow", "0");
    $("file-input").value = "";
    render();
    if (message) showActionMessage(message, kind);
  }

  // ------------------------------------------------------------------ data
  async function refetch() {
    if (!projectId) return;
    try {
      const res = await fetch(`/api/projects/${projectId}`);
      if (res.status === 404) {
        resetToEmpty({ message: "This project is no longer available. Choose another MP4." });
        return;
      }
      let payload = null;
      try { payload = await res.json(); } catch { throw new Error("The server returned an invalid project response."); }
      if (!res.ok) throw new Error(formatApiError(payload, res.status, "Couldn't refresh project status."));
      view = payload;
      if (!isProcessing(view.project.status)) {
        if (cancellationPending) {
          cancellationPending = false;
          clearActionMessage("cancel");
        }
      }
      clearActionMessage("reconnect");
      render();
    } catch (err) {
      showActionMessage(`${err.message} Live progress will retry.`, "reconnect");
    }
  }

  function scheduleRefetch() {
    clearTimeout(refetchTimer);
    refetchTimer = setTimeout(refetch, 180);
  }

  function connectSse() {
    if (sse) sse.close();
    if (!projectId) return;
    sse = new EventSource(`/api/projects/${projectId}/events`);
    sse.onmessage = (e) => {
      let msg = {};
      try { msg = JSON.parse(e.data); } catch { return; }
      if (msg.type === "snapshot" && msg.view) { view = msg.view; clearActionMessage("reconnect"); render(); return; }
      if (msg.type === "progress") {
        liveProgress = { stage: msg.stage, progress: msg.progress, detail: msg.detail };
        clearActionMessage("reconnect");
        renderLive();
        return;
      }
      // stage / clip / done → authoritative refetch
      liveProgress = null;
      scheduleRefetch();
    };
    sse.onerror = () => {
      showActionMessage("Live progress disconnected. Reconnecting…", "reconnect");
    };
  }

  // ------------------------------------------------------------------ render
  function render() {
    const p = view && view.project;
    $("upload-state").classList.toggle("hidden", !!p);
    $("processing-state").classList.toggle("hidden", !p);
    if (!p) { $("results-state").classList.add("hidden"); stopElapsed(); return; }
    if (!isProcessing(p.status)) {
      if (cancellationPending) {
        cancellationPending = false;
        clearActionMessage("cancel");
      }
    }

    // Source line
    const src = p.source;
    $("source-name").textContent = view.original_name || "source.mp4";
    $("source-name").title = view.original_name || "source.mp4";
    $("source-meta").textContent = src
      ? `${src.width}×${src.height} · ${fmtMs(src.duration_ms)} · ${src.video_codec}/${src.audio_codec}`
      : "";

    // Warning banner
    const warn = $("warning-banner");
    if (p.warning) { warn.textContent = p.warning; warn.classList.remove("hidden"); }
    else warn.classList.add("hidden");

    renderStages(p);
    renderCurrentOp(p);
    renderError(p);
    renderResults(p);
    startElapsed(p);
  }

  function stageState(p, name) {
    const rec = p.stages.find((s) => s.name === name) || {};
    if (rec.error) return "failed";
    if (rec.completed_at) return "done";
    if (rec.started_at) return "active";
    return "pending";
  }

  function renderStages(p) {
    const wrap = $("stages");
    wrap.innerHTML = "";
    for (const name of STAGE_ORDER) {
      const rec = p.stages.find((s) => s.name === name) || {};
      const st = stageState(p, name);
      const div = document.createElement("div");
      div.className = `step ${st === "pending" ? "" : st}`;
      div.dataset.stage = name;
      div.setAttribute("role", "listitem");
      const status =
        st === "failed" ? "Failed" :
        st === "done" ? (rec.detail || "Done") :
        st === "active" ? (rec.detail || "Working…") : "";
      div.innerHTML = `<strong>${STAGE_LABELS[name]}</strong><span class="status"></span>` +
        (st === "active" ? `<div class="mini-bar"><div class="mini-fill"></div></div>` : "");
      div.querySelector(".status").textContent = status;
      div.setAttribute("aria-label", `${STAGE_LABELS[name]}${status ? `: ${status}` : ": pending"}`);
      wrap.appendChild(div);
    }
    renderLive();
  }

  function renderLive() {
    if (!liveProgress && view && view.live) liveProgress = view.live;
    if (!liveProgress) return;
    const step = document.querySelector(`.step[data-stage="${liveProgress.stage}"] .mini-fill`);
    if (step) step.style.width = `${Math.round((liveProgress.progress || 0) * 100)}%`;
    const status = document.querySelector(`.step[data-stage="${liveProgress.stage}"] .status`);
    if (status && liveProgress.detail) {
      status.textContent = liveProgress.detail;
      status.parentElement.setAttribute("aria-label", `${STAGE_LABELS[liveProgress.stage] || liveProgress.stage}: ${liveProgress.detail}`);
    }
    if (liveProgress.detail) $("current-op-text").textContent = liveProgress.detail;
    else if (liveProgress.progress != null)
      $("current-op-text").textContent =
        `${STAGE_LABELS[liveProgress.stage] || liveProgress.stage} · ${Math.round(liveProgress.progress * 100)}%`;
  }

  function renderCurrentOp(p) {
    const active = isProcessing(p.status);
    $("current-op").classList.toggle("hidden", !active);
    $("cancel-btn").classList.toggle("hidden", !active);
    $("cancel-btn").disabled = cancellationPending;
    $("cancel-btn").textContent = cancellationPending ? "Cancelling…" : "Cancel";
    if (active) {
      const label = STAGE_LABELS[p.status] || p.status;
      $("current-op-text").textContent = cancellationPending
        ? "Cancelling…"
        : label.replace(/^\d+\.\s*/, "") + "…";
    }
  }

  function renderError(p) {
    const box = $("error-box");
    const clips = view.clips || [];
    const chooseAnother = $("choose-another-btn");
    const retryButton = $("retry-btn");
    const zeroClipFailure = clips.length === 0 && (p.status === "failed" || p.status === "cancelled");
    chooseAnother.classList.toggle("hidden", !zeroClipFailure);
    retryButton.disabled = retryPending;
    retryButton.textContent = retryPending
      ? "Retrying…"
      : p.status === "cancelled" ? "Retry processing" : "Retry stage";
    if (p.status === "failed") {
      const failedStage = p.stages.find((s) => s.error);
      $("error-stage").textContent = failedStage
        ? `${STAGE_LABELS[failedStage.name] || failedStage.name} failed`
        : "Processing failed";
      $("error-text").textContent = p.error || "Processing failed. Try again or choose another MP4.";
      box.classList.remove("hidden");
    } else if (p.status === "cancelled") {
      $("error-stage").textContent = "Cancelled";
      $("error-text").textContent = "Processing was stopped. Completed clips are kept. Retry resumes from the last completed stage.";
      box.classList.remove("hidden");
    } else {
      box.classList.add("hidden");
    }
  }

  function renderResults(p) {
    const section = $("results-state");
    const clips = (view.clips || []);
    const ready = clips.filter((c) => c.status === "ready");
    const failed = clips.filter((c) => c.status === "failed");
    const showResults = clips.length > 0 || p.status === "complete";
    section.classList.toggle("hidden", !showResults);
    if (!showResults) return;

    const total = clips.length;
    if (view.caption_only === true) {
      $("results-title").textContent = p.status === "complete"
        ? "Captioned video ready"
        : "Captioning full video";
      $("results-sub").textContent = "The full video is preserved without selecting or cutting clips.";
    } else {
      $("results-title").textContent =
        total === 0 ? "No clips produced" :
        failed.length > 0
          ? `${ready.length} of ${total} clips ready · ${failed.length} failed`
          : p.status === "complete"
          ? `${ready.length} strong clip${ready.length === 1 ? "" : "s"} found`
          : `${ready.length} of ${total} clips ready`;

      const sel = view.selector ? ` Selected by ${view.selector}.` : "";
      const count = total > 0 ? ` ${ready.length} ready${failed.length ? `, ${failed.length} failed` : ""}.` : "";
      $("results-sub").textContent =
        `Ranked by self-contained opening, tension, payoff, and clarity.${count}${sel}`;
    }

    const openFolder = $("open-folder-btn");
    const outputAvailable = typeof view.output_dir === "string" && view.output_dir.trim().length > 0;
    openFolder.disabled = !outputAvailable || ready.length === 0;
    openFolder.title = openFolder.disabled
      ? "Available after at least one clip is ready and saved."
      : "Open the folder containing the ready clips";
    const newProject = $("new-project-btn");
    const active = isProcessing(p.status);
    const restyleBusy = Object.values(restyleState).some((state) => state.busy);
    newProject.disabled = cancellationPending || restyleBusy;
    newProject.textContent = restyleBusy
      ? "Applying captions…"
      : active
      ? cancellationPending ? "Cancelling…" : "Cancel & start over"
      : "New project";
    newProject.title = active
      ? "Cancel processing first. Completed clips will stay available."
      : "Leave this project and choose another MP4";

    // Empty (quality bar) state
    $("empty-results").classList.toggle("hidden", !(p.status === "complete" && total === 0));

    const wrap = $("clips");
    wrap.innerHTML = "";
    for (const c of clips) {
      wrap.appendChild(clipRow(c));
    }

    // Rejected transparency
    const rej = view.rejected_summary || [];
    $("rejected-details").classList.toggle("hidden", rej.length === 0);
    if (rej.length) {
      const list = $("rejected-list");
      list.innerHTML = "";
      for (const r of rej) {
        const d = document.createElement("div");
        d.className = "rejected-item";
        d.innerHTML = `<div></div><div class="reasons"></div>`;
        d.children[0].textContent = `“${r.headline || "(untitled)"}” — ${fmtMs(r.start_ms)}–${fmtMs(r.end_ms)}`;
        d.children[1].textContent = (r.reasons || []).join("; ");
        list.appendChild(d);
      }
    }
  }

  function clipRow(c) {
    const row = document.createElement("article");
    row.className = "clip";

    const preview = document.createElement("div");
    preview.className = "preview";
    if (c.status === "ready") {
      const v = document.createElement("video");
      v.controls = true;
      v.preload = "metadata";
      v.playsInline = true;
      v.setAttribute("aria-label", `Preview clip ${c.rank}: ${c.headline}`);
      v.src = `/api/projects/${projectId}/clips/${c.id}` +
        (clipRev[c.id] ? `?rev=${clipRev[c.id]}` : "");
      const pendingPreview = restyleState[c.id];
      if (pendingPreview && pendingPreview.awaitingPreview) {
        v.addEventListener("loadedmetadata", () => {
          const state = restyleState[c.id];
          if (!state || !state.awaitingPreview) return;
          state.awaitingPreview = false;
          state.kind = "success";
          state.message = "Captions applied";
          const status = row.querySelector(".restyle-status");
          if (status) {
            status.className = "small restyle-status success";
            status.textContent = state.message;
          }
        }, { once: true });
        v.addEventListener("error", () => {
          const state = restyleState[c.id];
          if (!state || !state.awaitingPreview) return;
          state.awaitingPreview = false;
          state.kind = "status-error";
          state.message = "Captions saved, but the preview could not reload";
          const status = row.querySelector(".restyle-status");
          if (status) {
            status.className = "small restyle-status status-error";
            status.textContent = state.message;
          }
        }, { once: true });
      }
      preview.appendChild(v);
    } else if (c.status === "rendering") {
      preview.innerHTML = `<span class="spinner"></span>`;
    } else if (c.status === "failed") {
      preview.textContent = "render failed";
    } else {
      preview.textContent = "queued";
    }

    const body = document.createElement("div");
    const captionOnly = view.caption_only === true;
    const rankLabel = captionOnly
      ? "Full video"
      : (c.rank === 1 ? "Best candidate" : `Candidate ${c.rank}`);
    const badges = [];
    if (c.layout && c.layout.mode === "face_crop") badges.push(`<span class="badge">face-tracked crop</span>`);
    else badges.push(`<span class="badge">blur-pad layout</span>`);
    if (c.low_confidence) badges.push(`<span class="badge warn">low transcription confidence</span>`);
    if (c.status === "failed") badges.push(`<span class="badge bad">failed</span>`);
    body.innerHTML = `
      <div class="rank"></div>
      <h3></h3>
      <p class="times"></p>
      <p class="why"></p>
      <div class="badges">${badges.join("")}</div>`;
    body.querySelector(".rank").textContent = `${rankLabel} · ${fmtMs(c.duration_ms)}`;
    body.querySelector("h3").textContent = captionOnly ? "Captioned video" : `“${c.headline}”`;
    body.querySelector(".times").textContent =
      captionOnly
        ? `The complete ${fmtMs(c.duration_ms)} video is preserved without clipping.`
        : `Starts at ${fmtMs(c.start_ms)} and ends at ${fmtMs(c.end_ms)}. One continuous excerpt from the podcast.`;
    body.querySelector(".why").textContent = c.status === "failed" && c.error
      ? `Render error: ${c.error}`
      : (captionOnly ? "Captions cover the entire video." : `Why it works: ${c.selection_reason}`);
    if (c.status === "ready") body.appendChild(restyleControls(c));

    const actions = document.createElement("div");
    actions.className = "actions";
    if (c.status === "ready") {
      const a = document.createElement("a");
      a.className = "primary action-button";
      a.href = `/api/projects/${projectId}/clips/${c.id}/download`;
      a.download = c.filename;
      a.setAttribute("aria-label", `Download clip ${c.rank}: ${c.headline}`);
      a.textContent = "Download MP4";
      actions.appendChild(a);
    } else if (c.status === "failed") {
      const b = document.createElement("button");
      b.textContent = retryPending ? "Retrying…" : "Retry failed clips";
      b.disabled = retryPending;
      b.addEventListener("click", retry);
      actions.appendChild(b);
    }

    row.appendChild(preview);
    row.appendChild(body);
    row.appendChild(actions);
    return row;
  }

  // Per-clip caption restyle: pick style + accent color, re-burn from the
  // cached base render (seconds, not a full re-render), reload the preview.
  function restyleControls(c) {
    const box = document.createElement("div");
    box.className = "restyle";
    box.setAttribute("role", "group");
    box.setAttribute("aria-label", `Caption settings for ${c.headline}`);
    const applied = {
      style: c.caption_style || captionStyle,
      color: (c.accent_color || accentColor).toUpperCase(),
      font: c.caption_font || captionDefaultFont,
    };
    const state = restyleState[c.id] || { draft: { ...applied } };
    state.draft = state.draft || { ...applied };
    restyleState[c.id] = state;

    const captionText = document.createElement("textarea");
    captionText.className = "caption-text";
    captionText.rows = 3;
    captionText.value = c.caption_text || "";
    captionText.placeholder = "Edit caption text";
    captionText.setAttribute("aria-label", "Caption text");

    const label = document.createElement("span");
    label.className = "muted small restyle-label";
    label.textContent = "Caption settings";

    const seg = document.createElement("div");
    seg.className = "seg";
    seg.setAttribute("role", "group");
    seg.setAttribute("aria-label", "Caption style");
    const styleBtns = ["impact", "clean"].map((s) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "seg-btn";
      b.textContent = s === "impact" ? "Impact" : "Clean";
      b.setAttribute("aria-pressed", "false");
      b.addEventListener("click", () => {
        state.draft.style = s;
        state.kind = "dirty";
        state.message = "Unsaved caption changes";
        sync();
      });
      seg.appendChild(b);
      return [s, b];
    });

    const swatches = document.createElement("div");
    swatches.className = "swatches";
    swatches.setAttribute("role", "group");
    swatches.setAttribute("aria-label", "Caption highlight color");
    const swatchBtns = ACCENT_PRESETS.map(({ name, color }) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "swatch";
      b.style.background = color;
      b.setAttribute("aria-label", `Use ${name} caption highlight, ${color}`);
      b.setAttribute("aria-pressed", "false");
      b.title = `${name} (${color})`;
      b.addEventListener("click", () => {
        state.draft.color = color;
        state.kind = "dirty";
        state.message = "Unsaved caption changes";
        sync();
      });
      swatches.appendChild(b);
      return [color, b];
    });
    const customPicker = document.createElement("label");
    customPicker.className = "custom-color-picker";
    const customLabel = document.createElement("span");
    customLabel.textContent = "Custom";
    const custom = document.createElement("input");
    custom.type = "color";
    custom.className = "custom-color";
    custom.setAttribute("aria-label", "Custom caption highlight color");
    custom.title = "Custom caption highlight color";
    custom.setAttribute("aria-pressed", "false");
    custom.addEventListener("input", (e) => {
      state.draft.color = e.target.value.toUpperCase();
      state.kind = "dirty";
      state.message = "Unsaved caption changes";
      sync();
    });
    customPicker.appendChild(customLabel);
    customPicker.appendChild(custom);
    swatches.appendChild(customPicker);

    const font = document.createElement("select");
    font.className = "font-select";
    font.setAttribute("aria-label", "Caption font");
    font.title = "Caption font";
    const availableFonts = captionFonts.includes(state.draft.font)
      ? captionFonts
      : [state.draft.font, ...captionFonts];
    for (const name of availableFonts) {
      const option = document.createElement("option");
      option.value = name;
      option.textContent = name;
      option.selected = name === state.draft.font;
      font.appendChild(option);
    }
    font.addEventListener("change", () => {
      state.draft.font = font.value;
      state.kind = "dirty";
      state.message = "Unsaved caption changes";
      sync();
    });
    const fontPicker = document.createElement("label");
    fontPicker.className = "font-picker";
    const fontLabel = document.createElement("span");
    fontLabel.textContent = "Font";
    fontPicker.appendChild(fontLabel);
    fontPicker.appendChild(font);

    const apply = document.createElement("button");
    apply.type = "button";
    apply.className = "apply-captions";
    apply.textContent = "Apply captions";
    const status = document.createElement("span");
    status.className = "muted small restyle-status";
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");

    function sync() {
      state.dirty = (
        state.draft.style !== applied.style ||
        state.draft.color !== applied.color ||
        state.draft.font !== applied.font
      );
      if (!state.dirty && state.kind === "dirty") {
        state.kind = null;
        state.message = "";
      }
      for (const [s, b] of styleBtns) {
        const selected = s === state.draft.style;
        b.classList.toggle("active", selected);
        b.setAttribute("aria-pressed", String(selected));
      }
      for (const [color, b] of swatchBtns) {
        const selected = color === state.draft.color;
        b.classList.toggle("active", selected);
        b.setAttribute("aria-pressed", String(selected));
      }
      const customSelected = !ACCENT_PRESETS.some((entry) => entry.color === state.draft.color);
      custom.classList.toggle("active", customSelected);
      custom.setAttribute("aria-pressed", String(customSelected));
      custom.value = state.draft.color;
      font.value = state.draft.font;
      apply.disabled = Boolean(state.busy) || !state.dirty;
      apply.textContent = state.busy ? "Applying…" : "Apply captions";
      status.className = `small restyle-status ${state.kind || ""}`;
      status.textContent = state.message || "";
    }

    apply.addEventListener("click", async () => {
      if (state.busy || !state.dirty) return;
      state.busy = true;
      state.kind = "busy";
      state.message = "Applying captions…";
      sync();
      try {
        const requestProjectId = projectId;
        const payload = {
          style: state.draft.style,
          accent_color: state.draft.color,
          font: state.draft.font,
        };
        if (view.caption_only === true) payload.caption_text = captionText.value;
        const updated = await requestJson(`/api/projects/${projectId}/clips/${c.id}/restyle`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        }, "Captions could not be updated.");
        captionStyle = state.draft.style;
        accentColor = state.draft.color;
        localStorage.setItem("cf-caption-style", captionStyle);
        localStorage.setItem("cf-accent-color", accentColor);
        clipRev[c.id] = Date.now();
        state.busy = false;
        if (!view || projectId !== requestProjectId) return;
        const i = (view.clips || []).findIndex((x) => x.id === c.id);
        if (i >= 0) view.clips[i] = updated;
        state.dirty = false;
        state.awaitingPreview = true;
        state.kind = "busy";
        state.message = "Captions saved. Refreshing preview…";
        state.draft = {
          style: updated.caption_style || state.draft.style,
          color: (updated.accent_color || state.draft.color).toUpperCase(),
          font: updated.caption_font || state.draft.font,
        };
        render();
      } catch (err) {
        state.busy = false;
        state.dirty = true;
        state.kind = "status-error";
        state.message = err.message;
        render();
        showActionMessage(`Captions for “${c.headline}” failed: ${err.message}`);
      }
    });

    box.appendChild(label);
    if (view.caption_only === true) box.appendChild(captionText);
    box.appendChild(seg);
    box.appendChild(swatches);
    box.appendChild(fontPicker);
    box.appendChild(apply);
    box.appendChild(status);
    sync();
    return box;
  }

  // ------------------------------------------------------------------ elapsed
  function startElapsed(p) {
    stopElapsed();
    const activeStage = p.stages.find((s) => s.started_at && !s.completed_at && !s.error);
    if (!activeStage) { $("elapsed").textContent = ""; return; }
    const started = new Date(activeStage.started_at).getTime();
    elapsedTimer = setInterval(() => {
      const s = Math.max(0, Math.floor((Date.now() - started) / 1000));
      $("elapsed").textContent = `· ${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")} elapsed`;
    }, 1000);
  }
  function stopElapsed() { clearInterval(elapsedTimer); }

  // ------------------------------------------------------------------ actions
  async function cancel() {
    if (!projectId || cancellationPending || !isProcessing(view && view.project && view.project.status)) return false;
    cancellationPending = true;
    render();
    showActionMessage("Cancellation requested. Waiting for the active stage to stop…", "cancel");
    try {
      const result = await requestJson(
        `/api/projects/${projectId}/cancel`,
        { method: "POST" },
        "Couldn't cancel processing."
      );
      if (result.cancelled) showActionMessage("Processing cancelled. Finished clips were kept.", "notice");
      else showActionMessage("Processing had already stopped. Refreshing its status…", "notice");
      scheduleRefetch();
      return true;
    } catch (err) {
      cancellationPending = false;
      render();
      showActionMessage(err.message);
      return false;
    }
  }

  async function retry() {
    if (!projectId || retryPending) return;
    retryPending = true;
    render();
    try {
      await requestJson(`/api/projects/${projectId}/retry`, { method: "POST" }, "Couldn't retry processing.");
      showActionMessage("Retry started. Completed work will be kept.", "notice");
      await refetch();
      retryPending = false;
      render();
    } catch (err) {
      retryPending = false;
      render();
      showActionMessage(err.message);
    }
  }

  async function openFolder() {
    const ready = (view && view.clips || []).filter((c) => c.status === "ready");
    if (!projectId || !view || !view.output_dir || ready.length === 0) {
      showActionMessage("The output folder becomes available after a clip is ready and saved.");
      return;
    }
    try {
      const result = await requestJson(
        `/api/projects/${projectId}/open-output-folder`,
        { method: "POST" },
        "Couldn't open the output folder."
      );
      if (result.opened) clearActionMessage();
      else showActionMessage(result.path ? `Open the clips manually at ${result.path}.` : "The output folder could not be opened.", "notice");
    } catch (err) {
      showActionMessage(err.message);
    }
  }

  async function handleNewProject() {
    if (uploadXhr) {
      cancelUpload();
      return;
    }
    if (view && isProcessing(view.project.status)) {
      if (await cancel()) {
        resetToEmpty({ message: "Processing cancelled. Finished clips remain on disk.", kind: "notice" });
      }
      return;
    }
    if (Object.values(restyleState).some((state) => state.busy)) {
      showActionMessage("Wait for the caption update to finish before starting another project.", "notice");
      return;
    }
    resetToEmpty();
  }

  // ------------------------------------------------------------------ modal
  function syncModalRows() {
    const offline = $("provider").value === "offline";
    $("key-row").classList.toggle("hidden", offline);
    $("model-row").classList.toggle("hidden", offline);
    $("offline-note").classList.toggle("hidden", !offline);
    $("model").placeholder = $("provider").value === "anthropic" ? "claude-sonnet-4-5" : "gpt-4o-mini";
  }

  function modalFocusables() {
    return [...$("modal-backdrop").querySelectorAll("button, input, select, [href]")]
      .filter((el) => !el.disabled && el.getClientRects().length > 0);
  }

  function openModal() {
    modalReturnFocus = document.activeElement;
    $("modal-backdrop").classList.remove("hidden");
    $("modal-backdrop").setAttribute("aria-hidden", "false");
    document.body.classList.add("modal-open");
    syncModalRows();
    requestAnimationFrame(() => $("provider").focus());
  }

  function closeModal() {
    $("modal-backdrop").classList.add("hidden");
    $("modal-backdrop").setAttribute("aria-hidden", "true");
    document.body.classList.remove("modal-open");
    $("test-result").classList.add("hidden");
    $("api-key").value = "";
    if (modalReturnFocus && typeof modalReturnFocus.focus === "function") modalReturnFocus.focus();
    modalReturnFocus = null;
  }

  function wireModal() {
    $("ai-btn").addEventListener("click", openModal);
    $("modal-close").addEventListener("click", closeModal);
    $("modal-backdrop").addEventListener("click", (e) => {
      if (e.target === $("modal-backdrop")) closeModal();
    });
    document.addEventListener("keydown", (e) => {
      if ($("modal-backdrop").classList.contains("hidden")) return;
      if (e.key === "Escape") {
        e.preventDefault();
        closeModal();
        return;
      }
      if (e.key !== "Tab") return;
      const focusables = modalFocusables();
      if (!focusables.length) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    });
    $("provider").addEventListener("change", syncModalRows);
    $("test-save").addEventListener("click", async () => {
      const btn = $("test-save");
      btn.disabled = true;
      btn.textContent = "Testing…";
      try {
        const saved = await requestJson("/api/settings/ai", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            provider: $("provider").value,
            model: $("model").value.trim(),
            api_key: $("api-key").value.trim(),
          }),
        }, "Could not save AI settings.");
        const out = $("test-result");
        out.textContent = saved.provider === "offline"
          ? "Local ranking is ready — no API key needed."
          : `${saved.provider === "anthropic" ? "Anthropic" : "OpenAI"} connection verified. Using model ${saved.model}.`;
        out.className = "small ok";
        out.classList.remove("hidden");
        $("api-key").value = "";
        await loadSettings();
      } catch (err) {
        const out = $("test-result");
        out.textContent = err.message;
        out.className = "small bad";
        out.classList.remove("hidden");
        showActionMessage(err.message);
      } finally {
        btn.disabled = false;
        btn.textContent = "Test & save";
      }
    });
  }

  // ------------------------------------------------------------------ misc
  function fmtMs(ms) {
    const t = Math.floor((ms || 0) / 1000);
    const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
    return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
                 : `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }

  // ------------------------------------------------------------------ boot
  function boot() {
    wireUpload();
    wireUploadOptions();
    wireModal();
    $("cancel-upload-btn").addEventListener("click", cancelUpload);
    $("cancel-btn").addEventListener("click", cancel);
    $("retry-btn").addEventListener("click", retry);
    $("choose-another-btn").addEventListener("click", resetToEmpty);
    $("open-folder-btn").addEventListener("click", openFolder);
    $("new-project-btn").addEventListener("click", handleNewProject);
    loadSetup();
    loadSettings();
    if (projectId) {
      refetch().then(() => connectSse());
    }
  }
  boot();
})();
