/* Clipping Factory studio — one screen, three states, SSE-driven. */
(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const STAGE_LABELS = {
    inspecting: "1. Inspect",
    extracting_audio: "2. Extract audio",
    transcribing: "3. Transcribe",
    selecting_candidates: "4. Find candidates",
    validating_candidates: "5. Validate",
    analyzing_layout: "6. Analyze framing",
    rendering: "7. Render",
  };
  const STAGE_ORDER = Object.keys(STAGE_LABELS);

  const PROJECT_ID_RE = /^[0-9a-f]{10}$/;
  const restoredProjectId = localStorage.getItem("cf-project");
  let projectId = restoredProjectId && PROJECT_ID_RE.test(restoredProjectId) ? restoredProjectId : null;
  if (restoredProjectId && !projectId) localStorage.removeItem("cf-project");
  let view = null;
  let sse = null;
  let refetchTimer = null;
  let elapsedTimer = null;
  let uploadXhr = null;
  let uploadCancelRequested = false;
  // A chosen MP4 waits here until "Make clips", so the caption and framing
  // options can still change before any upload or transcription starts.
  let stagedFile = null;
  let cancellationPending = false;
  let retryPending = false;
  let actionMessageKind = null;
  let pendingDeleteId = null;
  let deleteBusy = false;
  let library = []; // LibraryEntry rows from GET /api/projects
  let librarySig = ""; // last-rendered signature — skips pointless rebuilds
  let liveProgress = null; // LiveStage + receivedAt (client receipt time)
  // Progress samples for the active stage — the ETA's rolling-rate window.
  let liveSamples = { stage: null, pts: [] };
  let firstClipAnnounced = false; // jump to the first ready clip once per run
  let selectedClipId = null; // the clip shown in the studio player
  // Last style/color the user applied — the starting point for new restyles.
  let captionStyle = localStorage.getItem("cf-caption-style") || "impact";
  let accentColor = localStorage.getItem("cf-accent-color") || "#FFDD00";
  let captionFonts = [];
  let captionStyles = [];
  let captionDefaultFont = "Inter";
  const ACCENT_SWATCHES = [
    { name: "Sun yellow", color: "#FFDD00" },
    { name: "Lime", color: "#7CFF4F" },
    { name: "Coral", color: "#FF4F4F" },
    { name: "Sky blue", color: "#4FB5FF" },
    { name: "Violet", color: "#C77DFF" },
    { name: "Orange", color: "#FF9F1C" },
  ];
  const clipRev = {}; // clip id → cache-busting token after a restyle
  const restyleState = {}; // clip id → {busy, kind, message, draft}
  // clip id → {sig, row}: lets a refetch rebuild only the cards whose clip
  // changed instead of remounting the whole list.
  const clipRowCache = {};

  function isProcessing(status) { return STAGE_ORDER.includes(status); }
  function apiPath(...segments) { return `/api/${segments.map((segment) => encodeURIComponent(String(segment))).join("/")}`; }

  function fmtBytes(n) {
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = Number(n) || 0, i = 0;
    while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
    return `${v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
  }

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
    const swatch = ACCENT_SWATCHES.find((entry) => entry.color === normalized);
    return swatch ? `${swatch.name} (${swatch.color})` : `Custom color (${normalized})`;
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
      if (!s.model_ok) problems.push(`Transcription model missing (~148 MB). Download ggml-base.bin into ${s.data_dir}/models/`);
      if (s.disk_free_gb !== null && s.disk_free_gb < 2) problems.push(`Low disk space: ${s.disk_free_gb.toFixed(1)} GB free.`);
      const banner = $("setup-banner");
      captionDefaultFont = s.caption_font || captionDefaultFont;
      if (Array.isArray(s.caption_fonts) && s.caption_fonts.length) captionFonts = s.caption_fonts;
      if (Array.isArray(s.caption_styles) && s.caption_styles.length) captionStyles = s.caption_styles;
      populateLanguagePicker(s);
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

  // The language list comes from the backend so the picker always matches what
  // the installed whisper.cpp understands. English-only ggml-*.en.bin weights
  // can't do detection or other languages — grey those options out.
  function populateLanguagePicker(setup) {
    const select = $("upload-language");
    const note = $("upload-language-note");
    if (!select || select.dataset.populated) {
      if (note && setup.model_ok && setup.model_multilingual === false) note.textContent =
        "The installed transcription model is English-only. Add a multilingual model (e.g. ggml-base.bin) to transcribe other languages.";
      return;
    }
    const langs = Array.isArray(setup.whisper_languages) ? setup.whisper_languages : [];
    if (!langs.length) return;
    for (const lang of langs) {
      const opt = document.createElement("option");
      opt.value = lang.code;
      opt.textContent = lang.name;
      if (setup.model_ok && setup.model_multilingual === false && lang.code !== "en") {
        opt.disabled = true;
      }
      select.appendChild(opt);
    }
    select.dataset.populated = "1";
    if (note && setup.model_ok && setup.model_multilingual === false) {
      note.textContent =
        "The installed transcription model is English-only. Add a multilingual model (e.g. ggml-base.bin) to transcribe other languages.";
    }
  }

  async function loadSettings() {
    try {
      const s = await requestJson("/api/settings/ai", {}, "Couldn't reconnect to the local server.");
      $("provider").value = s.provider || "openai";
      $("model").value = s.model || "";
      $("base-url").value = s.base_url || "";
      syncModalRows();
      clearActionMessage("reconnect");
    } catch {
      showActionMessage("Couldn't reconnect to the local server. Refresh to try again.", "reconnect");
    }
  }

  // ------------------------------------------------------------------ upload
  function wireUpload() {
    const drop = $("drop");
    $("choose-btn").addEventListener("click", () => $("file-input").click());
    $("change-file-btn").addEventListener("click", () => $("file-input").click());
    $("file-input").addEventListener("change", (e) => {
      if (e.target.files[0]) stageFile(e.target.files[0]);
    });
    $("start-btn").addEventListener("click", () => {
      if (stagedFile) uploadFile(stagedFile);
    });
    ["dragenter", "dragover"].forEach((ev) =>
      drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.add("dragover"); })
    );
    ["dragleave", "drop"].forEach((ev) =>
      drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.remove("dragover"); })
    );
  }

  // The upload box only exists on the empty state, so the window itself is the
  // drop target — dropping a file from any view offers a new project.
  function wireGlobalDrop() {
    const overlay = $("drop-overlay");
    const isFileDrag = (e) => e.dataTransfer && [...e.dataTransfer.types].includes("Files");
    const theaterOpen = () => !$("review-theater").hidden;
    const hide = () => overlay.classList.add("hidden");

    window.addEventListener("dragenter", (e) => {
      if (!isFileDrag(e)) return;
      e.preventDefault();
      if (!theaterOpen()) overlay.classList.remove("hidden");
    });
    window.addEventListener("dragover", (e) => {
      if (isFileDrag(e)) e.preventDefault();
    });
    window.addEventListener("dragleave", (e) => {
      if (isFileDrag(e) && e.relatedTarget === null) hide();
    });
    window.addEventListener("drop", (e) => {
      hide();
      if (!isFileDrag(e)) return;
      e.preventDefault();
      if (theaterOpen()) return;
      if (uploadXhr) {
        showActionMessage("Wait for the current upload to finish.", "notice");
        return;
      }
      const f = e.dataTransfer.files && e.dataTransfer.files[0];
      if (!f) return;
      if (view && isProcessing(view.project.status)) {
        showActionMessage("Cancel or finish the current project before dropping a new MP4.", "notice");
        return;
      }
      if (Object.values(restyleState).some((state) => state.busy)) {
        showActionMessage("Wait for the caption update to finish before starting another project.", "notice");
        return;
      }
      if (view) resetToEmpty();
      stageFile(f);
    });
  }

  function stageFile(file) {
    if (!/\.(mp4|m4v)$/i.test(file.name)) {
      showActionMessage("Attach an .mp4 file. Other containers are not supported yet.");
      return;
    }
    stagedFile = file;
    clearActionMessage();
    $("staged-name").textContent = file.name;
    $("staged-size").textContent = fmtBytes(file.size);
    syncStagedFile();
    $("start-btn").focus();
  }

  function clearStagedFile() {
    stagedFile = null;
    $("file-input").value = "";
    syncStagedFile();
  }

  function syncStagedFile() {
    $("drop-empty").classList.toggle("hidden", !!stagedFile);
    $("drop-staged").classList.toggle("hidden", !stagedFile);
  }

  function wireUploadOptions() {
    const swatchGroup = $("upload-swatches");
    const swatches = ACCENT_SWATCHES.map(({ name, color }) => {
      const swatch = document.createElement("button");
      swatch.type = "button";
      swatch.className = "upload-swatch";
      swatch.dataset.color = color;
      swatch.style.setProperty("--swatch", color);
      swatch.setAttribute("aria-label", `Use ${name} accent color, ${color}`);
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
    // Random / Auto replace the swatch pick: show no swatch as chosen.
    for (const auto of document.querySelectorAll('input[name="accent-mode"]:not([value="manual"])')) {
      auto.addEventListener("change", () => {
        for (const swatch of swatches) {
          swatch.classList.remove("active");
          swatch.setAttribute("aria-pressed", "false");
        }
        $("upload-accent-hex").textContent = auto.value === "random" ? "random color" : "matched to the video";
      });
    }
    selectColor(accentColor);
    const styleInputs = [...document.querySelectorAll('input[name="caption-style"]')];
    const savedStyle = styleInputs.find((input) => input.value === captionStyle);
    if (savedStyle) savedStyle.checked = true;
    for (const input of styleInputs) {
      input.addEventListener("change", () => {
        if (!input.checked) return;
        captionStyle = input.value;
        localStorage.setItem("cf-caption-style", captionStyle);
      });
    }
    // Platform target is a UI pref like caption style: restore the last pick.
    const platformSelect = $("upload-platform");
    const savedPlatform = localStorage.getItem("cf-platform");
    if (savedPlatform && [...platformSelect.options].some((o) => o.value === savedPlatform)) {
      platformSelect.value = savedPlatform;
    }
    platformSelect.addEventListener("change", () => {
      localStorage.setItem("cf-platform", platformSelect.value);
    });
  }

  function uploadFile(file) {
    if (!/\.(mp4|m4v)$/i.test(file.name)) {
      showActionMessage("Attach an .mp4 file. Other containers are not supported yet.");
      return;
    }
    if (uploadXhr) return;
    resetProjectState();
    $("drop").classList.add("hidden");
    $("upload-progress").classList.remove("hidden");
    $("cancel-upload-btn").disabled = false;
    $("cancel-upload-btn").textContent = "Cancel upload";
    setUploadPhase(`Uploading ${file.name}…`, 0);

    const form = new FormData();
    const framingMode = document.querySelector('input[name="framing-mode"]:checked').value;
    const accentMode = document.querySelector('input[name="accent-mode"]:checked').value;
    const styleInput = document.querySelector('input[name="caption-style"]:checked');
    if (styleInput) form.append("caption_style", styleInput.value);
    form.append("framing_mode", framingMode);
    form.append("accent_mode", accentMode);
    form.append("accent_color", $("upload-accent-color").value.toUpperCase());
    form.append("language", $("upload-language").value || "auto");
    form.append("platform", $("upload-platform").value || "any");
    const focusPrompt = $("focus-prompt").value.trim();
    if (focusPrompt) form.append("focus_prompt", focusPrompt);
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
          if (!v.project || !PROJECT_ID_RE.test(v.project.id)) throw new Error("invalid project id");
          projectId = v.project.id;
          localStorage.setItem("cf-project", projectId);
          view = v;
          clearStagedFile();
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
      $("upload-bar").style.transform = "scaleX(" + (percent / 100) + ")";
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

  // Every per-project runtime field, cleared in one place: the empty state,
  // switching projects, and a fresh upload all start from the same slate.
  function resetProjectState() {
    projectId = null;
    view = null;
    liveProgress = null;
    liveSamples = { stage: null, pts: [] };
    cancellationPending = false;
    retryPending = false;
    uploadCancelRequested = false;
    firstClipAnnounced = false;
    selectedClipId = null;
    clearTimeout(refetchTimer);
    refetchTimer = null;
    if (sse) { sse.close(); sse = null; }
    for (const key of Object.keys(restyleState)) delete restyleState[key];
    for (const key of Object.keys(clipRev)) delete clipRev[key];
    for (const key of Object.keys(clipRowCache)) delete clipRowCache[key];
  }

  function resetToEmpty({ message = null, kind = "error" } = {}) {
    resetProjectState();
    localStorage.removeItem("cf-project");
    const warning = $("warning-banner");
    warning.textContent = "";
    warning.classList.add("hidden");
    clearActionMessage();
    $("drop").classList.remove("hidden");
    $("upload-progress").classList.add("hidden");
    $("upload-bar").style.transform = "scaleX(0)";
    $("upload-bar").parentElement.setAttribute("aria-valuenow", "0");
    clearStagedFile();
    render();
    if (message) showActionMessage(message, kind);
    loadLibrary();
  }

  // ------------------------------------------------------------------ data
  async function refetch() {
    if (!projectId) return;
    const requestProjectId = projectId;
    try {
      const res = await fetch(apiPath("projects", requestProjectId));
      if (projectId !== requestProjectId) return;
      if (res.status === 404) {
        resetToEmpty({ message: "This project is no longer available. Choose another MP4." });
        return;
      }
      let payload = null;
      try { payload = await res.json(); } catch { throw new Error("The server returned an invalid project response."); }
      if (!res.ok) throw new Error(formatApiError(payload, res.status, "Couldn't refresh project status."));
      if (projectId !== requestProjectId) return;
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
      if (projectId !== requestProjectId) return;
      showActionMessage(`${err.message} Live progress will retry.`, "reconnect");
    }
  }

  function scheduleRefetch() {
    clearTimeout(refetchTimer);
    refetchTimer = setTimeout(refetch, 180);
  }

  // ------------------------------------------------------------------ library
  async function loadLibrary() {
    try {
      const entries = await requestJson("/api/projects", {}, "Couldn't load the library.");
      library = Array.isArray(entries) ? entries : [];
    } catch {
      library = [];
    }
    renderLibrary();
  }

  const LIBRARY_STATUS = {
    created: "Queued",
    complete: "Complete",
    cancelled: "Cancelled",
    failed: "Failed",
  };
  function libraryStatus(entry) {
    if (LIBRARY_STATUS[entry.status]) return LIBRARY_STATUS[entry.status];
    const stage = STAGE_LABELS[entry.status];
    return stage ? stage.replace(/^\d+\.\s*/, "") + "…" : entry.status;
  }

  function renderLibrary() {
    const restyleBusy = Object.values(restyleState).some((s) => s.busy);
    const sig = JSON.stringify([library, projectId, restyleBusy]);
    if (sig === librarySig) return;
    librarySig = sig;

    const list = $("library-list");
    const count = $("library-count");
    count.textContent = String(library.length);
    count.classList.toggle("hidden", library.length === 0);
    $("library-empty").classList.toggle("hidden", library.length > 0);
    list.innerHTML = "";
    for (const entry of library) {
      const isOpen = entry.id === projectId;
      const name = entry.source_filename || `project ${entry.id}`;
      const date = entry.created_at
        ? new Date(entry.created_at).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })
        : "";
      const bits = [date, libraryStatus(entry)];
      if (entry.clips_ready) bits.push(`${entry.clips_ready} clip${entry.clips_ready === 1 ? "" : "s"}`);
      bits.push(fmtBytes(entry.size_bytes));

      const card = document.createElement("div");
      card.className = `library-card${isOpen ? " open" : ""}`;

      const open = document.createElement("button");
      open.type = "button";
      open.className = "library-open";
      open.title = isOpen ? `${name} is open` : `Open ${name}`;
      const nameEl = document.createElement("span");
      nameEl.className = "name";
      nameEl.textContent = name;
      const meta = document.createElement("span");
      meta.className = "meta";
      meta.textContent = bits.filter(Boolean).join(" · ") + (isOpen ? " " : "");
      if (isOpen) {
        const tag = document.createElement("span");
        tag.className = "open-tag";
        tag.textContent = "· open";
        meta.appendChild(tag);
      }
      open.appendChild(nameEl);
      open.appendChild(meta);
      open.addEventListener("click", () => openProject(entry.id));

      const del = document.createElement("button");
      del.type = "button";
      del.className = "library-delete";
      del.textContent = "Delete";
      del.setAttribute("aria-label", `Delete ${name} from this computer`);
      const blocked = isProcessing(entry.status);
      const styling = isOpen && restyleBusy;
      del.disabled = blocked || styling || deleteBusy;
      del.title = blocked
        ? "Processing is running — cancel it before deleting."
        : styling
          ? "Caption update is applying — wait for it to finish."
          : `Delete ${name} from this computer`;
      del.addEventListener("click", () => askDelete(entry));

      card.appendChild(open);
      card.appendChild(del);
      list.appendChild(card);
    }
  }

  // Switch the screen to a library project.
  function openProject(id) {
    if (!id || id === projectId || uploadXhr) return;
    resetProjectState();
    projectId = id;
    localStorage.setItem("cf-project", projectId);
    closeLibrary();
    clearActionMessage();
    render();
    refetch().then(() => connectSse());
  }

  function openLibrary() {
    loadLibrary();
    const drawer = $("library-drawer");
    drawer.inert = false;
    drawer.setAttribute("aria-hidden", "false");
    drawer.classList.add("open");
    $("library-backdrop").classList.remove("hidden");
    $("library-btn").setAttribute("aria-expanded", "true");
    requestAnimationFrame(() => $("library-close").focus());
  }

  function closeLibrary() {
    const drawer = $("library-drawer");
    if (!drawer.classList.contains("open")) return;
    drawer.classList.remove("open");
    drawer.setAttribute("aria-hidden", "true");
    drawer.inert = true;
    $("library-backdrop").classList.add("hidden");
    $("library-btn").setAttribute("aria-expanded", "false");
    if (drawer.contains(document.activeElement)) $("library-btn").focus();
  }

  function wireLibrary() {
    $("library-btn").addEventListener("click", () =>
      $("library-drawer").classList.contains("open") ? closeLibrary() : openLibrary());
    $("library-close").addEventListener("click", closeLibrary);
    $("library-backdrop").addEventListener("click", closeLibrary);
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && $("delete-backdrop").classList.contains("hidden")) closeLibrary();
    });
  }

  function openDeleteModal() { deleteDialog.open(); }

  function closeDeleteModal() { deleteDialog.close(); }

  function askDelete(entry) {
    if (deleteBusy) return;
    pendingDeleteId = entry.id;
    const name = entry.source_filename || `project ${entry.id}`;
    const files = entry.file_count || 0;
    $("delete-modal-description").textContent =
      `Delete "${name}" — ${files} file${files === 1 ? "" : "s"}, ${fmtBytes(entry.size_bytes)} — from this computer? This can't be undone.`;
    openDeleteModal();
  }

  async function confirmDelete() {
    if (!pendingDeleteId || deleteBusy) return;
    const id = pendingDeleteId;
    deleteBusy = true;
    const btn = $("delete-confirm");
    btn.disabled = true;
    btn.textContent = "Deleting…";
    try {
      await requestJson(apiPath("projects", id), { method: "DELETE" }, "Couldn't delete the project.");
      closeDeleteModal();
      if (id === projectId) {
        resetToEmpty({ message: "Project deleted from this computer.", kind: "notice" });
      } else {
        // cf-project only ever points at the open project, but clear it if a
        // stale value happened to name the deleted id.
        if (localStorage.getItem("cf-project") === id) localStorage.removeItem("cf-project");
        showActionMessage("Project deleted from this computer.", "notice");
      }
      await loadLibrary();
    } catch (err) {
      closeDeleteModal();
      showActionMessage(err.message);
      await loadLibrary();
    } finally {
      deleteBusy = false;
      btn.disabled = false;
      btn.textContent = "Delete";
    }
  }

  function wireDeleteModal() {
    $("delete-cancel").addEventListener("click", closeDeleteModal);
    $("delete-confirm").addEventListener("click", confirmDelete);
  }

  function connectSse() {
    if (sse) sse.close();
    if (!projectId) return;
    const sourceProjectId = projectId;
    const source = new EventSource(apiPath("projects", sourceProjectId, "events"));
    sse = source;
    source.onmessage = (e) => {
      if (sse !== source || projectId !== sourceProjectId) return;
      let msg = {};
      try { msg = JSON.parse(e.data); } catch { return; }
      if (msg.type === "snapshot" && msg.view) { view = msg.view; clearActionMessage("reconnect"); render(); return; }
      if (msg.type === "progress") {
        liveProgress = {
          stage: msg.stage,
          progress: msg.progress,
          detail: msg.detail,
          elapsed_ms: msg.elapsed_ms,
          stage_estimate_ms: msg.stage_estimate_ms,
          overall_progress: msg.overall_progress,
          pending_ms: msg.pending_ms,
          receivedAt: Date.now(),
        };
        if (liveSamples.stage !== msg.stage) liveSamples = { stage: msg.stage, pts: [] };
        liveSamples.pts.push({ t: Date.now(), p: msg.progress || 0 });
        if (liveSamples.pts.length > 600) liveSamples.pts.splice(0, liveSamples.pts.length - 600);
        clearActionMessage("reconnect");
        renderLive();
        return;
      }
      // stage / clip / done → authoritative refetch
      liveProgress = null;
      liveSamples = { stage: null, pts: [] };
      scheduleRefetch();
    };
    source.onerror = () => {
      if (sse !== source || projectId !== sourceProjectId) return;
      showActionMessage("Live progress disconnected. Reconnecting…", "reconnect");
    };
  }

  // ------------------------------------------------------------------ render
  function render() {
    const p = view && view.project;
    // Keep the open project's library card in step with live status so its
    // Delete disables while a run is active.
    if (p) {
      const entry = library.find((e) => e.id === p.id);
      if (entry && entry.status !== p.status) entry.status = p.status;
    }
    renderLibrary();
    $("upload-state").classList.toggle("hidden", !!p);
    $("processing-state").classList.toggle("hidden", !p || p.status === "complete");
    // Once clips exist the studio owns the screen; progress shrinks to a strip.
    $("processing-state").classList.toggle("compact", !!p && (view.clips || []).length > 0);
    if (!p) { $("results-state").classList.add("hidden"); stopElapsed(); return; }
    if (!isProcessing(p.status)) {
      if (cancellationPending) {
        cancellationPending = false;
        clearActionMessage("cancel");
      }
    }

    // Source line
    const src = p.source;
    $("source-name").textContent = view.source_name || "source.mp4";
    $("source-name").title = view.source_name || "source.mp4";
    $("source-meta").textContent = src
      ? `${src.width}×${src.height} · ${fmtMs(src.duration_ms)} · ${src.video_codec}/${src.audio_codec}`
      : "";
    $("source-focus").textContent = p.focus_prompt ? ` · focus: "${p.focus_prompt}"` : "";

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
        (st === "active"
          ? `<span class="step-meta muted"></span><div class="mini-bar"><div class="mini-fill"></div></div>`
          : "");
      div.querySelector(".status").textContent = status;
      div.setAttribute("aria-label", `${STAGE_LABELS[name]}${status ? `: ${status}` : ": pending"}`);
      wrap.appendChild(div);
    }
    renderLive();
  }

  // "~45s" / "~3m" / "~1h 04m" — compact ETA text for a millisecond count.
  function fmtEta(ms) {
    const s = Math.max(0, Math.round(ms / 1000));
    if (s < 90) return `${s}s`;
    const m = Math.round(s / 60);
    if (m < 60) return `${m}m`;
    return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
  }

  // Rolling rate: the slope of progress across the last ~60s of samples,
  // measured against *now* — so a stage that stalls drags its own rate
  // toward zero and its ETA visibly grows instead of freezing.
  function recentRate() {
    const pts = liveSamples.pts;
    if (!liveProgress || !pts.length) return 0;
    const cutoff = Date.now() - 60_000;
    let first = pts[0];
    for (const s of pts) {
      if (s.t >= cutoff) break;
      first = s;
    }
    const dt = Date.now() - first.t;
    const dp = Math.max(0, (liveProgress.progress || 0) - first.p);
    return dt > 0 ? dp / dt : 0;
  }

  // Estimated ms left in the active stage. Elapsed time is aged past the
  // last SSE event; while almost nothing is measured, the backend's
  // calibrated stage estimate is the honest figure.
  function liveEtaMs() {
    if (!liveProgress) return null;
    const p = Math.min(Math.max(liveProgress.progress || 0, 0), 0.995);
    const elapsed =
      (liveProgress.elapsed_ms || 0) + (Date.now() - (liveProgress.receivedAt || Date.now()));
    if ((p < 0.005 || elapsed < 3_000) && liveProgress.stage_estimate_ms != null) {
      return liveProgress.stage_estimate_ms;
    }
    if (p <= 0 || elapsed <= 0) return null;
    const rate = recentRate() || p / elapsed;
    if (rate <= 0) return null;
    return (1 - p) / rate;
  }

  function renderOverall(live, etaMs) {
    const box = $("overall");
    if (!box) return;
    const show = !!(live && live.overall_progress != null);
    box.classList.toggle("hidden", !show);
    if (!show) return;
    const pct = Math.min(100, Math.round(live.overall_progress * 100));
    $("overall-bar").style.transform = `scaleX(${pct / 100})`;
    $("overall-bar").parentElement.setAttribute("aria-valuenow", String(pct));
    const left =
      etaMs != null && live.pending_ms != null
        ? ` · ~${fmtEta(etaMs + live.pending_ms)} left`
        : "";
    $("overall-text").textContent = `${pct}%${left}`;
  }

  function renderLive() {
    if (!liveProgress && view && view.live) {
      liveProgress = { ...view.live, receivedAt: Date.now() };
      if (liveSamples.stage !== liveProgress.stage) {
        liveSamples = { stage: liveProgress.stage, pts: [] };
      }
    }
    if (!liveProgress) { renderOverall(null); return; }
    const stage = liveProgress.stage;
    const percent = Math.round((liveProgress.progress || 0) * 100);
    const etaMs = liveEtaMs();
    const etaText = etaMs != null ? ` · ~${fmtEta(etaMs)}` : "";
    const step = document.querySelector(`.step[data-stage="${stage}"]`);
    if (step) {
      const fill = step.querySelector(".mini-fill");
      if (fill) fill.style.transform = `scaleX(${percent / 100})`;
      const meta = step.querySelector(".step-meta");
      if (meta) meta.textContent = `${percent}%${etaText ? `${etaText} left` : ""}`;
      const status = step.querySelector(".status");
      if (status && liveProgress.detail) {
        status.textContent = liveProgress.detail;
        step.setAttribute(
          "aria-label",
          `${STAGE_LABELS[stage] || stage}: ${liveProgress.detail} · ${percent}%`
        );
      }
    }
    if (!cancellationPending) {
      const label =
        liveProgress.detail || `${STAGE_LABELS[stage] || stage}`;
      $("current-op-text").textContent =
        `${label} · ${percent}%${etaText ? `${etaText} remaining` : ""}`;
    }
    renderOverall(liveProgress, etaMs);
  }

  function renderCurrentOp(p) {
    const active = isProcessing(p.status);
    $("current-op").classList.toggle("hidden", !active);
    $("cancel-btn").classList.toggle("hidden", !active);
    $("cancel-btn").disabled = cancellationPending;
    $("cancel-btn").textContent = cancellationPending ? "Cancelling…" : "Cancel";
    if (active) {
      // A live progress report already carries percent + ETA — don't
      // clobber it with the generic stage label between SSE events.
      const liveCovers = liveProgress && liveProgress.stage === p.status;
      if (cancellationPending) $("current-op-text").textContent = "Cancelling…";
      else if (!liveCovers) {
        const label = STAGE_LABELS[p.status] || p.status;
        $("current-op-text").textContent = label.replace(/^\d+\.\s*/, "") + "…";
      }
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

  // Highest validator score first; scoreless rows (caption-only, old
  // manifests) keep their manifest order at the end.
  function rankClips(clips) {
    return clips.slice().sort((a, b) =>
      (typeof b.score === "number" ? b.score : -Infinity) -
      (typeof a.score === "number" ? a.score : -Infinity));
  }

  function renderResults(p) {
    const section = $("results-state");
    const clips = (view.clips || []);
    const ready = clips.filter((c) => c.status === "ready");
    // The first finished clip lands while later ones still render: pull the
    // results into view once so the user notices without hunting for it.
    if (ready.length > 0 && !firstClipAnnounced && isProcessing(p.status)) {
      firstClipAnnounced = true;
      selectedClipId = ready[0].id;
    }
    const failed = clips.filter((c) => c.status === "failed");
    const showResults = clips.length > 0 || p.status === "complete";
    section.classList.toggle("hidden", !showResults);
    if (!showResults) return;

    const total = clips.length;
    if (view.caption_only === true) {
      $("results-title").textContent = p.status === "complete"
        ? "Captioned video ready"
        : "Captioning full video";
      $("results-sub").textContent = view.source_name || "";
    } else {
      $("results-title").textContent =
        total === 0 ? "No clips produced" :
        failed.length > 0
          ? `${ready.length} of ${total} clips ready · ${failed.length} failed`
          : p.status === "complete"
          ? `${ready.length} clip${ready.length === 1 ? "" : "s"}`
          : `${ready.length} of ${total} clips ready`;

      $("results-sub").textContent = view.source_name || "";
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
    const ranked = rankClips(clips);
    // Reconcile row-by-row instead of remounting the list: clips land
    // one at a time while later ones still render, and remounting a card
    // whose data didn't change would reset its <video>'s playback and any
    // open caption controls.
    const seen = new Set();
    const kept = new Set();
    const rows = ranked.map((c) => {
      const prev = clipRowCache[c.id];
      let sig = clipSignature(c);
      let row;
      if (prev && prev.sig === sig) {
        row = prev.row;
      } else {
        row = clipRow(c);
        // Building a ready clip's controls seeds restyleState[c.id], which
        // the signature reads — retake it after the build.
        sig = clipSignature(c);
      }
      clipRowCache[c.id] = { sig, row };
      seen.add(c.id);
      kept.add(row);
      return row;
    });
    for (const id of Object.keys(clipRowCache)) if (!seen.has(id)) delete clipRowCache[id];
    for (const child of [...wrap.children]) if (!kept.has(child)) child.remove();
    rows.forEach((row, i) => {
      const current = wrap.children[i];
      if (current !== row) wrap.insertBefore(row, current || null);
    });
    if (!ranked.some((c) => c.id === selectedClipId)) {
      const firstReady = ranked.find((c) => c.status === "ready");
      selectedClipId = (firstReady || ranked[0] || {}).id || null;
    }
    ranked.forEach((c, i) => {
      const active = c.id === selectedClipId;
      const row = rows[i];
      if (row.classList.contains("is-active") !== active) {
        row.classList.toggle("is-active", active);
        // A hidden clip must not keep playing behind the selected one.
        if (!active) row.querySelectorAll("video").forEach((v) => v.pause());
      }
    });
    wrap.classList.toggle("hidden", ranked.length === 0);
    renderRail(ranked);

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
        d.children[0].textContent = `“${r.headline || "(untitled)"}” · ${fmtMs(r.start_ms)}-${fmtMs(r.end_ms)}` +
          (typeof r.score === "number" ? ` · score ${r.score.toFixed(1)}` : "");
        d.children[1].textContent = (r.reasons || []).join("; ");
        list.appendChild(d);
      }
    }
  }

  function selectClip(id) {
    if (id === selectedClipId || !view) return;
    selectedClipId = id;
    renderResults(view.project);
  }

  // Left rail: one compact card per clip — poster, rank, headline, score.
  function renderRail(ranked) {
    const list = $("clip-rail-list");
    list.innerHTML = "";
    const captionOnly = view.caption_only === true;
    ranked.forEach((c) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = `rail-item${c.id === selectedClipId ? " active" : ""} ${c.status}`;
      b.dataset.reviewKey = apiPath("projects", projectId, "clips", c.id);
      b.setAttribute("aria-current", c.id === selectedClipId ? "true" : "false");
      const thumb = document.createElement("span");
      thumb.className = "rail-thumb";
      if (c.status === "ready") {
        const img = document.createElement("img");
        img.alt = "";
        img.loading = "lazy";
        img.src = apiPath("projects", projectId, "clips", c.id, "export", "poster") +
          (clipRev[c.id] ? `?rev=${clipRev[c.id]}` : "");
        img.addEventListener("error", () => img.remove(), { once: true });
        thumb.appendChild(img);
      } else {
        thumb.innerHTML = c.status === "rendering" ? `<span class="spinner"></span>` : "";
      }
      const dur = document.createElement("span");
      dur.className = "rail-dur";
      dur.textContent = fmtMs(c.duration_ms);
      thumb.appendChild(dur);
      const text = document.createElement("span");
      text.className = "rail-text";
      const top = document.createElement("span");
      top.className = "rail-rank";
      top.textContent = captionOnly ? "Full video" : `#${c.rank}` +
        (typeof c.score === "number" ? ` · ${c.score.toFixed(1)}` : "") +
        (c.status === "rendering" ? " · rendering" : c.status === "failed" ? " · failed" : c.status === "ready" ? "" : " · queued");
      const title = document.createElement("span");
      title.className = "rail-title";
      title.textContent = captionOnly ? "Captioned video" : c.headline;
      text.appendChild(top);
      text.appendChild(title);
      b.appendChild(thumb);
      b.appendChild(text);
      b.addEventListener("click", () => selectClip(c.id));
      list.appendChild(b);
    });
    document.dispatchEvent(new CustomEvent("cf-rail-rendered"));
  }

  // Everything a clip card renders, as one string: the clip record itself,
  // the restyle-preview cache buster, the retry button's pending label, the
  // font/style catalogs (they land via loadSetup after first paint), and the
  // restyle "applying" flag — its failure path relies on a rebuild to restore
  // the controls. Draft edits and status text already update via sync()
  // inside the row, so they stay out — keeping them out is what lets a
  // mid-edit card survive a sibling clip's SSE-driven re-render.
  function clipSignature(c) {
    const r = restyleState[c.id] || {};
    return JSON.stringify([
      c,
      view.caption_only === true,
      clipRev[c.id] || 0,
      retryPending,
      captionFonts,
      captionStyles,
      Boolean(r.busy),
    ]);
  }

  // Card copy shared with the review theater, which reads it through the
  // bridge below instead of scraping this DOM.
  function clipRankText(c) {
    if (view.caption_only === true) return `Full video · ${fmtMs(c.duration_ms)}`;
    return `${c.rank === 1 ? "Best clip" : `Clip ${c.rank}`} · ${fmtMs(c.duration_ms)}`;
  }

  function clipTitleText(c) {
    return view.caption_only === true ? "Captioned video" : `“${c.headline}”`;
  }

  function clipWhyText(c) {
    if (c.status === "failed" && c.error) return `Render error: ${c.error}`;
    return view.caption_only === true ? "Captions cover the entire video." : c.selection_reason;
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
      v.src = apiPath("projects", projectId, "clips", c.id) +
        (clipRev[c.id] ? `?rev=${clipRev[c.id]}` : "");
      const pendingPreview = restyleState[c.id];
      if (pendingPreview && pendingPreview.awaitingPreview) {
        v.addEventListener("loadedmetadata", () => {
          const state = restyleState[c.id];
          if (!state || !state.awaitingPreview) return;
          state.awaitingPreview = false;
          state.kind = "success";
          state.message = "Applied";
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
      preview.innerHTML = `<div class="rendering-note"><span class="spinner"></span><span>Rendering…</span></div>`;
    } else if (c.status === "failed") {
      preview.textContent = "render failed";
    } else {
      preview.textContent = "queued";
    }

    const body = document.createElement("div");
    body.className = "clip-info";
    const captionOnly = view.caption_only === true;
    const badges = [];
    if (typeof c.score === "number") badges.push(`<span class="badge score">score ${c.score.toFixed(1)}</span>`);
    if (c.low_confidence) badges.push(`<span class="badge warn">unclear audio</span>`);
    if (c.status === "rendering") badges.push(`<span class="badge">rendering…</span>`);
    if (c.status === "failed") badges.push(`<span class="badge bad">failed</span>`);
    body.innerHTML = `
      <div class="rank"></div>
      <h3></h3>
      <p class="times"></p>
      <div class="badges">${badges.join("")}</div>
      <details class="why-wrap"><summary>Why this clip</summary><p class="why"></p></details>`;
    body.querySelector(".rank").textContent = clipRankText(c);
    body.querySelector("h3").textContent = clipTitleText(c);
    body.querySelector(".times").textContent =
      captionOnly
        ? `Full ${fmtMs(c.duration_ms)} video`
        : `${fmtMs(c.start_ms)} – ${fmtMs(c.end_ms)}`;
    body.querySelector(".why").textContent = clipWhyText(c);
    if (c.status === "failed") body.querySelector(".why-wrap").open = true;

    const actions = document.createElement("div");
    actions.className = "actions";
    if (c.status === "ready") {
      const a = document.createElement("a");
      a.className = "primary action-button";
      a.href = apiPath("projects", projectId, "clips", c.id, "download");
      a.download = c.filename;
      a.setAttribute("aria-label", `Download clip ${c.rank}: ${c.headline}`);
      a.textContent = "Download MP4";
      actions.appendChild(a);
      // Export pack: the .srt/.vtt/.meta.json sidecars next to every MP4.
      const stem = (c.filename || "clip").replace(/\.mp4$/i, "");
      const pack = document.createElement("div");
      pack.className = "export-links";
      pack.setAttribute("aria-label", "Export pack files");
      for (const [label, kind, ext] of [["SRT", "srt", "srt"], ["VTT", "vtt", "vtt"], ["Meta", "meta", "meta.json"], ["Poster", "poster", "jpg"]]) {
        const link = document.createElement("a");
        link.className = "export-link";
        link.href = apiPath("projects", projectId, "clips", c.id, "export", kind);
        link.download = `${stem}.${ext}`;
        link.title = `Download ${stem}.${ext}`;
        link.textContent = label;
        pack.appendChild(link);
      }
      actions.appendChild(pack);
    } else if (c.status === "failed") {
      const b = document.createElement("button");
      b.textContent = retryPending ? "Retrying…" : "Retry failed clips";
      b.disabled = retryPending;
      b.addEventListener("click", retry);
      actions.appendChild(b);
    }

    const inspector = document.createElement("div");
    inspector.className = "inspector";
    inspector.appendChild(body);
    inspector.appendChild(actions);
    if (c.status === "ready") inspector.appendChild(restyleControls(c));

    row.appendChild(preview);
    row.appendChild(inspector);
    return row;
  }

  // The caption/garnish bundle travels clip → applied → draft → payload; one
  // conversion per direction keeps a new field to a single edit here.
  function captionBundle(clip) {
    return {
      style: clip.caption_style || captionStyle,
      color: (clip.accent_color || accentColor).toUpperCase(),
      font: clip.caption_font || captionDefaultFont,
      text: clip.caption_text ?? "",
      textPresent: clip.caption_text !== null && clip.caption_text !== undefined,
      autoCut: Boolean(clip.auto_cut),
      zoomCuts: Boolean(clip.zoom_cuts),
      progressBar: Boolean(clip.progress_bar),
      hookTitle: Boolean(clip.hook_title),
    };
  }

  function captionBundleChanged(draft, applied) {
    return (
      draft.style !== applied.style ||
      draft.color !== applied.color ||
      draft.font !== applied.font ||
      draft.textPresent !== applied.textPresent ||
      (draft.textPresent && draft.text !== applied.text) ||
      draft.autoCut !== applied.autoCut ||
      draft.zoomCuts !== applied.zoomCuts ||
      draft.progressBar !== applied.progressBar ||
      draft.hookTitle !== applied.hookTitle
    );
  }

  function captionBundlePayload(draft, applied) {
    const payload = {
      style: draft.style,
      accent_color: draft.color,
      font: draft.font,
    };
    if (draft.textPresent) payload.caption_text = draft.text;
    if (draft.autoCut !== applied.autoCut) payload.auto_cut = draft.autoCut;
    if (draft.zoomCuts !== applied.zoomCuts) payload.zoom_cuts = draft.zoomCuts;
    if (draft.progressBar !== applied.progressBar) payload.progress_bar = draft.progressBar;
    if (draft.hookTitle !== applied.hookTitle) payload.hook_title = draft.hookTitle;
    return payload;
  }

  // One builder for every restyle switch row: the text label leads, the
  // switch stays last.
  function switchRow({ title, aria, strong, hint, checked, onChange }) {
    const row = document.createElement("label");
    row.className = "switch-row compact";
    row.title = title;
    const text = document.createElement("span");
    text.innerHTML = `<strong>${strong}</strong><small>${hint}</small>`;
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "switch";
    box.checked = checked;
    box.setAttribute("aria-label", aria);
    box.addEventListener("change", () => onChange(box.checked));
    row.appendChild(text);
    row.appendChild(box);
    return { row, box };
  }

  // Per-clip caption restyle: pick style + accent color, re-burn from the
  // cached base render (seconds, not a full re-render), reload the preview.
  function restyleControls(c) {
    const box = document.createElement("div");
    box.className = "restyle";
    box.setAttribute("role", "group");
    box.setAttribute("aria-label", `Caption settings for ${c.headline}`);
    const applied = captionBundle(c);
    const state = restyleState[c.id] || { draft: { ...applied } };
    state.draft = state.draft || { ...applied };
    restyleState[c.id] = state;

    // The one place a caption or garnish edit is recorded as unsaved.
    function markDirty(message = "Unsaved changes") {
      state.kind = "dirty";
      state.message = message;
      sync();
    }

    const captionText = document.createElement("textarea");
    captionText.className = "caption-text";
    captionText.rows = 3;
    captionText.value = state.draft.text;
    captionText.placeholder = "Edit caption text";
    captionText.setAttribute("aria-label", "Caption text");
    captionText.addEventListener("input", () => {
      state.draft.text = captionText.value;
      state.draft.textPresent = true;
      markDirty();
    });

    const seg = document.createElement("div");
    seg.className = "seg";
    seg.setAttribute("role", "group");
    seg.setAttribute("aria-label", "Caption style");
    const STYLE_LABELS = { impact: "Impact", clean: "Clean", pop: "Pop", cinema: "Cinema" };
    const styleBtns = (captionStyles.length ? captionStyles : Object.keys(STYLE_LABELS)).map((s) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "seg-btn";
      b.textContent = STYLE_LABELS[s] || s;
      b.setAttribute("aria-pressed", "false");
      b.addEventListener("click", () => {
        state.draft.style = s;
        markDirty();
      });
      seg.appendChild(b);
      return [s, b];
    });

    const swatches = document.createElement("div");
    swatches.className = "swatches";
    swatches.setAttribute("role", "group");
    swatches.setAttribute("aria-label", "Caption accent color");
    const swatchBtns = ACCENT_SWATCHES.map(({ name, color }) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "swatch";
      b.style.background = color;
      b.setAttribute("aria-label", `Use ${name} accent color, ${color}`);
      b.setAttribute("aria-pressed", "false");
      b.title = `${name} (${color})`;
      b.addEventListener("click", () => {
        state.draft.color = color;
        markDirty();
      });
      swatches.appendChild(b);
      return [color, b];
    });
    const customPicker = document.createElement("label");
    customPicker.className = "custom-color-picker";
    const customLabel = document.createElement("span");
    customLabel.textContent = "";
    const custom = document.createElement("input");
    custom.type = "color";
    custom.className = "custom-color";
    custom.setAttribute("aria-label", "Custom caption accent color");
    custom.title = "Custom caption accent color";
    custom.setAttribute("aria-pressed", "false");
    custom.addEventListener("input", (e) => {
      state.draft.color = e.target.value.toUpperCase();
      markDirty();
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
      markDirty();
    });
    const fontPicker = document.createElement("label");
    fontPicker.className = "font-picker";
    const fontLabel = document.createElement("span");
    fontLabel.textContent = "Font";
    fontPicker.appendChild(fontLabel);
    fontPicker.appendChild(font);

    const toggle = (key, { title, aria, strong, hint, message = "Re-renders this clip" }) => switchRow({
      title,
      aria,
      strong,
      hint,
      checked: Boolean(state.draft[key]),
      onChange(checked) {
        state.draft[key] = checked;
        markDirty(message);
      },
    });

    // Opt-in auto-cut: removes silence gaps and filler words at render.
    // Default off — a clip is otherwise one continuous faithful excerpt.
    const { row: autoCut, box: autoCutBox } = toggle("autoCut", {
      title: "Remove silence gaps and filler words (um, uh) — re-renders this clip",
      aria: `Auto-cut silences and filler words for ${c.headline}`,
      strong: "Auto-cut",
      hint: "Remove silences &amp; ums",
    });
    // Opt-in zoom cuts: subtle punch-in/out on emphasis beats. Default off —
    // the framing otherwise never moves.
    const { row: zoomCuts, box: zoomCutsBox } = toggle("zoomCuts", {
      title: "Punch in slightly on loud beats and stressed words — re-renders this clip",
      aria: `Zoom cuts on emphasis beats for ${c.headline}`,
      strong: "Zoom cuts",
      hint: "Punch in on emphasis",
    });
    const { row: progBar, box: progBarBox } = toggle("progressBar", {
      title: "Draw a thin accent-colored progress bar along the bottom edge — re-renders this clip",
      aria: `Draw a progress bar for ${c.headline}`,
      strong: "Progress bar",
      hint: "Thin bar along the bottom",
    });
    // Opt-in hook title: the clip's headline burned over the opening beat.
    // Default off — the clip opens on content.
    const { row: hookTitle, box: hookTitleBox } = toggle("hookTitle", {
      title: "Burn the clip headline as a hook title over the first ~1.8s — re-renders this clip",
      aria: `Show a hook title at the start of ${c.headline}`,
      strong: "Hook title",
      hint: "Headline over the opening",
    });

    const apply = document.createElement("button");
    apply.type = "button";
    apply.className = "apply-captions";
    apply.textContent = "Apply changes";
    const status = document.createElement("span");
    status.className = "muted small restyle-status";
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");

    function sync() {
      state.dirty = captionBundleChanged(state.draft, applied);
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
      const customSelected = !ACCENT_SWATCHES.some((entry) => entry.color === state.draft.color);
      custom.classList.toggle("active", customSelected);
      custom.setAttribute("aria-pressed", String(customSelected));
      custom.value = state.draft.color;
      font.value = state.draft.font;
      autoCutBox.checked = Boolean(state.draft.autoCut);
      zoomCutsBox.checked = Boolean(state.draft.zoomCuts);
      progBarBox.checked = Boolean(state.draft.progressBar);
      hookTitleBox.checked = Boolean(state.draft.hookTitle);
      if (captionText.value !== state.draft.text) captionText.value = state.draft.text;
      apply.disabled = Boolean(state.busy) || !state.dirty;
      apply.textContent = state.busy ? "Applying…" : "Apply changes";
      status.className = `small restyle-status ${state.kind || ""}`;
      status.textContent = state.message || "";
    }

    apply.addEventListener("click", async () => {
      if (state.busy || !state.dirty) return;
      state.busy = true;
      state.kind = "busy";
      state.message = "Applying…";
      sync();
      try {
        const requestProjectId = projectId;
        const payload = captionBundlePayload(state.draft, applied);
        const updated = await requestJson(apiPath("projects", requestProjectId, "clips", c.id, "restyle"), {
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
        state.message = "Loading preview…";
        state.draft = captionBundle(updated);
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

    // Grouped: look (style, color, font) · edits (render toggles) · text.
    const section = (title, ...children) => {
      const el = document.createElement("div");
      el.className = "restyle-section";
      const h = document.createElement("span");
      h.className = "restyle-label";
      h.textContent = title;
      el.appendChild(h);
      for (const child of children) el.appendChild(child);
      return el;
    };
    const toggles = document.createElement("div");
    toggles.className = "toggle-grid";
    for (const t of [hookTitle, progBar, autoCut, zoomCuts]) toggles.appendChild(t);
    const textWrap = document.createElement("details");
    textWrap.className = "caption-text-wrap";
    const textSummary = document.createElement("summary");
    textSummary.textContent = "Edit text";
    textWrap.appendChild(textSummary);
    textWrap.appendChild(captionText);
    const footer = document.createElement("div");
    footer.className = "restyle-footer";
    footer.appendChild(status);
    footer.appendChild(apply);

    const scroll = document.createElement("div");
    scroll.className = "restyle-scroll";
    scroll.appendChild(section("Captions", seg, swatches, fontPicker));
    scroll.appendChild(section("Extras", toggles));
    scroll.appendChild(textWrap);
    box.appendChild(scroll);
    box.appendChild(footer);
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
      // Ages the displayed ETA between SSE events — a stalled stage's
      // remaining estimate keeps growing instead of freezing.
      renderLive();
    }, 1000);
  }
  function stopElapsed() { clearInterval(elapsedTimer); }

  // ------------------------------------------------------------------ actions
  async function cancel() {
    if (!projectId || cancellationPending || !isProcessing(view && view.project && view.project.status)) return false;
    const requestProjectId = projectId;
    cancellationPending = true;
    render();
    showActionMessage("Cancellation requested. Waiting for the active stage to stop…", "cancel");
    try {
      const result = await requestJson(
        apiPath("projects", requestProjectId, "cancel"),
        { method: "POST" },
        "Couldn't cancel processing."
      );
      if (projectId !== requestProjectId) return false;
      if (result.cancelled) showActionMessage("Processing cancelled. Finished clips were kept.", "notice");
      else showActionMessage("Processing had already stopped. Refreshing its status…", "notice");
      scheduleRefetch();
      return true;
    } catch (err) {
      if (projectId !== requestProjectId) return false;
      cancellationPending = false;
      render();
      showActionMessage(err.message);
      return false;
    }
  }

  async function retry() {
    if (!projectId || retryPending) return;
    const requestProjectId = projectId;
    retryPending = true;
    render();
    try {
      await requestJson(apiPath("projects", requestProjectId, "retry"), { method: "POST" }, "Couldn't retry processing.");
      if (projectId !== requestProjectId) return;
      showActionMessage("Retry started. Completed work will be kept.", "notice");
      await refetch();
      retryPending = false;
      render();
    } catch (err) {
      if (projectId !== requestProjectId) return;
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
    const requestProjectId = projectId;
    try {
      const result = await requestJson(
        apiPath("projects", requestProjectId, "open-output-folder"),
        { method: "POST" },
        "Couldn't open the output folder."
      );
      if (projectId !== requestProjectId) return;
      if (result.opened) clearActionMessage();
      else showActionMessage(result.path ? `Open the clips manually at ${result.path}.` : "The output folder could not be opened.", "notice");
    } catch (err) {
      if (projectId !== requestProjectId) return;
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
    const provider = $("provider").value;
    const offline = provider === "offline";
    const local = provider === "local";
    $("key-row").classList.toggle("hidden", offline || local);
    $("model-row").classList.toggle("hidden", offline);
    $("base-url-row").classList.toggle("hidden", !local);
    $("offline-note").classList.toggle("hidden", !offline);
    $("local-note").classList.toggle("hidden", !local);
    $("model").placeholder = provider === "anthropic" ? "claude-opus-5" : local ? "qwen2.5:7b" : "gpt-4o-mini";
  }

  // One focus trap + open/close implementation shared by every dialog: the
  // two modals here and the review theater in review.js (via the bridge).
  function dialogFocusables(root) {
    return [...root.querySelectorAll("button, input, select, textarea, video, [href], [tabindex]:not([tabindex='-1'])")]
      .filter((el) => !el.disabled && el.getClientRects().length > 0);
  }

  function trapTabWithin(event, root) {
    const focusables = dialogFocusables(root);
    if (!focusables.length) return false;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
      return true;
    }
    if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
      return true;
    }
    return false;
  }

  function createDialog(backdropId, { focus, onOpen, onClose } = {}) {
    const backdrop = $(backdropId);
    let returnFocus = null;
    function open() {
      returnFocus = document.activeElement;
      backdrop.classList.remove("hidden");
      backdrop.setAttribute("aria-hidden", "false");
      document.body.classList.add("modal-open");
      if (onOpen) onOpen();
      requestAnimationFrame(() => {
        const target = focus && backdrop.querySelector(focus);
        if (target) target.focus();
      });
    }
    function close() {
      backdrop.classList.add("hidden");
      backdrop.setAttribute("aria-hidden", "true");
      document.body.classList.remove("modal-open");
      if (onClose) onClose();
      if (returnFocus && typeof returnFocus.focus === "function") returnFocus.focus();
      returnFocus = null;
    }
    document.addEventListener("keydown", (e) => {
      if (backdrop.classList.contains("hidden")) return;
      if (e.key === "Escape") {
        e.preventDefault();
        close();
        return;
      }
      if (e.key === "Tab") trapTabWithin(e, backdrop);
    });
    backdrop.addEventListener("click", (e) => {
      if (e.target === backdrop) close();
    });
    return { open, close };
  }

  const aiDialog = createDialog("modal-backdrop", {
    focus: "#provider",
    onOpen: syncModalRows,
    onClose: () => {
      $("test-result").classList.add("hidden");
      $("api-key").value = "";
    },
  });
  const deleteDialog = createDialog("delete-backdrop", {
    focus: "#delete-cancel",
    onClose: () => { pendingDeleteId = null; },
  });

  function openModal() { aiDialog.open(); }
  function closeModal() { aiDialog.close(); }

  function wireModal() {
    $("ai-btn").addEventListener("click", openModal);
    $("modal-close").addEventListener("click", closeModal);
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
            base_url: $("base-url").value.trim(),
            api_key: $("api-key").value.trim(),
          }),
        }, "Could not save AI settings.");
        const out = $("test-result");
        out.textContent = saved.provider === "offline"
          ? "Local ranking is ready. No API key needed."
          : saved.provider === "local"
            ? `Local endpoint verified. Using model ${saved.model}.`
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

  // ------------------------------------------------------------------ review bridge
  // review.js is a separate script: it reads the live clip records here and
  // reuses the dialog focus trap instead of scraping the card DOM.
  function reviewItems() {
    const clips = (view && view.clips) || [];
    const items = [];
    for (const c of rankClips(clips)) {
      const cached = clipRowCache[c.id];
      const row = cached && cached.row;
      const player = row && row.querySelector(".preview video");
      if (!player) continue; // only a ready clip has a playable card
      items.push({
        card: row,
        player,
        key: apiPath("projects", projectId, "clips", c.id),
        title: clipTitleText(c),
        rank: clipRankText(c),
        reason: clipWhyText(c) || "",
        score: typeof c.score === "number" ? `score ${c.score.toFixed(1)}` : "",
        downloadHref: apiPath("projects", projectId, "clips", c.id, "download"),
        downloadName: c.filename || "",
      });
    }
    return { items, total: clips.length };
  }

  window.cfStudio = { trapTab: trapTabWithin, reviewItems };

  // ------------------------------------------------------------------ boot
  function boot() {
    wireUpload();
    wireGlobalDrop();
    wireUploadOptions();
    wireModal();
    wireDeleteModal();
    wireLibrary();
    loadLibrary();
    $("cancel-upload-btn").addEventListener("click", cancelUpload);
    $("cancel-btn").addEventListener("click", cancel);
    $("retry-btn").addEventListener("click", retry);
    $("choose-another-btn").addEventListener("click", resetToEmpty);
    $("empty-choose-btn").addEventListener("click", resetToEmpty);
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
