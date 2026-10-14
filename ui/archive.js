(() => {
  "use strict";
  const invoke = window.__TAURI__?.core?.invoke;
  const listen = window.__TAURI__?.event?.listen;
  const escape = value => String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
  // Windows extended paths remain intact for native I/O; only their display is simplified.
  function displayPath(value) {
    const path = String(value ?? "");
    if (path.slice(0, 8).toUpperCase() === "\\\\?\\UNC\\") return "\\\\" + path.slice(8);
    return path.startsWith("\\\\?\\") ? path.slice(4) : path;
  }
  // Display only the selected folder's first level. Never change native extraction/copy plans.
  function listedFiles(files) {
    const rows = new Map();
    for (const file of files) {
      const path = String(file.path ?? "").replaceAll("\\", "/");
      if (!path || /\.(jpe?g)$/i.test(path)) continue;
      const parts = path.split("/");
      const folder = parts.length > 1;
      const name = parts[0];
      const key = `${folder ? "folder" : "file"}:${name.toLowerCase()}`;
      if (!rows.has(key)) rows.set(key, { path: name, folder });
    }
    return [...rows.values()].sort((a, b) => Number(b.folder) - Number(a.folder)
      || a.path.localeCompare(b.path, undefined, { numeric: true, sensitivity: "base" }));
  }
  function renderRows(rows) {
    const folderIcon = '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M2 5h6l2 2h8v10H2zM2 5V3h6l2 2"/></svg>';
    const fileIcon = '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5 2h7l4 4v12H5zM12 2v5h4"/></svg>';
    return rows.map(row => `<li data-archive-kind="${row.folder ? "folder" : "file"}"><span class="archive-item-name">${row.folder ? folderIcon : fileIcon}<span>${escape(row.path)}</span></span></li>`).join("");
  }
  let tail = Promise.resolve();
  const queued = new Set();
  const progressHandlers = new Map();
  listen?.("files-archive-progress", event => {
    const p = event.payload ?? event;
    progressHandlers.get(p.id)?.(p);
  }).catch(() => {});

  async function open(download) {
    if (!invoke) return;
    const previousFocus = document.activeElement;
    const overlay = document.createElement("div");
    overlay.className = "archive-backdrop";
    overlay.innerHTML = `<section class="premium-panel archive-dialog" role="dialog" aria-modal="true" aria-labelledby="archive-title">
      <span class="hero-kicker">MODULE 06</span><h3 id="archive-title">Распаковать в игру</h3>
      <p>${escape(download.fileName || download.path)}</p>
      <div class="archive-form hidden">
        <label class="archive-field"><span>Папка внутри архива</span><select id="archive-source"></select></label>
        <div class="archive-preview archive-contents hidden"><header><span>Содержимое папки</span><small id="archive-content-count"></small></header><ul></ul></div>
        <div class="archive-field"><span>Папка игры</span><div class="archive-destination"><span id="archive-destination">Не выбрана</span><button class="secondary-button" id="archive-pick">Выбрать папку</button></div></div>
      </div>
      <details class="personalization-details"><summary><span>Пароль архива</span></summary><label class="archive-field"><input id="archive-password" type="password" autocomplete="off" aria-label="Пароль архива" /></label></details>
      <div class="archive-status" role="status" aria-live="polite"></div>
      <p class="archive-backup-note">Перед заменой оригиналы будут сохранены в .cgs-backups внутри папки игры. Скачанный архив останется в Загрузках.</p>
      <div class="archive-footer"><button class="secondary-button" id="archive-cancel">Отмена</button><button class="primary-button" id="archive-next">Распаковать</button></div>
    </section>`;
    document.body.appendChild(overlay);
    const $ = selector => overlay.querySelector(selector);
    $("#archive-password").value = download.password || "";
    let id = null, destination = "", busy = false, done = false, passwordDirty = false, contentsLoading = false;
    let contentsRequest = 0;
    let contentsSource = "";
    let finish;
    const completed = new Promise(resolve => { finish = resolve; });
    const status = (text, error = false) => { $(".archive-status").textContent = text; $(".archive-status").classList.toggle("error", error); };
    function updateAction() {
      const needsRetry = !id || passwordDirty;
      $("#archive-next").disabled = busy || contentsLoading || (!done && !needsRetry && (!destination || !$("#archive-source").value));
    }
    function setBusy(value) {
      busy = value;
      overlay.querySelectorAll("button,input,select").forEach(el => { el.disabled = value; });
      updateAction();
      $(".archive-dialog").setAttribute("aria-busy", String(value));
      window.dispatchEvent(new CustomEvent("cgs:busy-changed", { detail: { id: "archive", busy: value } }));
    }
    function invalidate() {
      $(".archive-contents").classList.toggle("hidden", !contentsSource || contentsSource !== $("#archive-source").value);
      $("#archive-next").textContent = "Распаковать";
      updateAction();
    }
    async function showContents() {
      const request = ++contentsRequest;
      const source = $("#archive-source").value;
      contentsSource = "";
      $(".archive-contents").classList.add("hidden");
      $(".archive-contents ul").replaceChildren();
      contentsLoading = Boolean(id && source);
      updateAction();
      if (!contentsLoading) return;
      try {
        const files = await invoke("files_archive_contents", { id, source });
        if (request !== contentsRequest) return;
        const visibleFiles = listedFiles(files);
        $(".archive-contents ul").innerHTML = renderRows(visibleFiles);
        $("#archive-content-count").textContent = String(visibleFiles.length);
        contentsSource = source;
        $(".archive-contents").classList.remove("hidden");
      } catch (e) {
        if (request === contentsRequest) status(String(e), true);
      } finally {
        if (request === contentsRequest) { contentsLoading = false; updateAction(); }
      }
    }
    async function close() {
      if (busy) return;
      ++contentsRequest;
      if (id) { progressHandlers.delete(id); await invoke("files_archive_cancel", { id }).catch(() => {}); }
      document.removeEventListener("keydown", keydown);
      overlay.remove(); previousFocus?.focus(); finish();
    }
    function keydown(event) {
      if (event.key === "Escape") { event.preventDefault(); close(); }
      if (event.key === "Tab") {
        const targets = [...overlay.querySelectorAll("button:not(:disabled),select:not(:disabled),input:not(:disabled),summary")].filter(el => el.getClientRects().length);
        const first = targets[0], last = targets.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    }
    document.addEventListener("keydown", keydown);
    async function inspect() {
      setBusy(true); invalidate(); status("Проверяем архив…");
      ++contentsRequest; contentsLoading = false; $(".archive-contents").classList.add("hidden");
      contentsSource = "";
      try {
        if (id) { progressHandlers.delete(id); await invoke("files_archive_cancel", { id }); id = null; }
        const result = await invoke("files_archive_inspect", { path: download.path, password: $("#archive-password").value });
        id = result.id; passwordDirty = false;
        progressHandlers.set(id, p => status(`${p.phase === "copy" ? "Копирование файлов" : "Распаковка файлов"} · ${p.completed}/${p.total}`));
        $("#archive-source").innerHTML = (result.recognized && result.folders.length === 1 ? "" : '<option value="">Выберите папку</option>')
          + result.folders.map(folder => `<option value="${escape(folder.path)}">${escape(folder.path)}</option>`).join("");
        $(".archive-form").classList.remove("hidden");
        await showContents();
        status(!result.recognized ? "DenuvOwO и ARTIFACT не найдены. Выберите папку вручную."
          : download.password ? (download.passwordCopied === false ? "Пароль подставлен. Не удалось скопировать его в буфер обмена." : "Пароль подставлен автоматически.") : "Выберите папку игры для распаковки.");
      } catch (e) { status(String(e), true); $("#archive-next").textContent = "Повторить"; }
      finally { setBusy(false); }
    }
    async function pick() {
      setBusy(true);
      try {
        const picked = await invoke("pick_game_directory");
        if (picked) { destination = picked; $("#archive-destination").textContent = displayPath(destination); invalidate(); }
      } catch (e) { status(String(e), true); }
      finally { setBusy(false); }
    }
    $("#archive-pick").onclick = pick;
    $("#archive-cancel").onclick = close;
    $("#archive-source").onchange = () => { invalidate(); showContents(); };
    $("#archive-password").oninput = () => { passwordDirty = true; invalidate(); };
    $("#archive-next").onclick = async () => {
      if (busy || contentsLoading) return;
      if (done) return close();
      if (!id || passwordDirty) return inspect();
      if (!$("#archive-source").value) return status("Выберите папку внутри архива.", true);
      if (!destination) return pick();
      let installing = false;
      setBusy(true);
      try {
        // One explicit action: native staging/validation, backup and copying remain intact.
        status("Распаковываем во временную папку…");
        await invoke("files_archive_prepare", { id, source: $("#archive-source").value, destination });
        installing = true;
        status("Копирование файлов…");
        const result = await invoke("files_archive_install", { id });
        progressHandlers.delete(id); id = null; done = true;
        overlay.querySelectorAll(".archive-form,details,.archive-backup-note,#archive-cancel").forEach(el => el.classList.add("hidden"));
        $("#archive-next").textContent = "Готово";
        status(`Файлы распакованы в папку игры. (${result.count})${result.backupPath ? `\nРезервная копия: ${displayPath(result.backupPath)}` : ""}`);
        window.dispatchEvent(new CustomEvent("cgs:activity", { detail: { type: "success" } }));
      } catch (e) {
        if (installing) { progressHandlers.delete(id); id = null; }
        invalidate(); status(String(e), true);
        if (!id) $("#archive-next").textContent = "Повторить";
        window.dispatchEvent(new CustomEvent("cgs:activity", { detail: { type: "error" } }));
      } finally { setBusy(false); }
    };
    $("#archive-cancel").focus();
    await inspect();
    await completed;
  }
  window.cgsOpenArchive = download => {
    if (!download?.path || queued.has(download.path)) return;
    queued.add(download.path);
    tail = tail.catch(() => {}).then(() => open(download)).finally(() => queued.delete(download.path));
    return tail;
  };
})();
