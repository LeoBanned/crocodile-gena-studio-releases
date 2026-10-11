(() => {
  "use strict";

  const invoke = window.__TAURI__?.core?.invoke;
  const listen = window.__TAURI__?.event?.listen;
  const $ = (selector, root = document) => root.querySelector(selector);
  const PAGE_SIZE = 12;

  const state = {
    files: [],
    page: 1,
    query: "",
    loaded: false,
    loading: false,
    refreshing: false,
    cached: false,
    error: "",
    progress: new Map(),
    downloads: new Map(),
  };

  function escapeHtml(value) {
    return String(value ?? "")
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#039;");
  }

  function filteredFiles() {
    const query = state.query.trim().toLocaleLowerCase("ru-RU");
    return state.files.filter((item) => {
      if (isFeatured(item)) return false;
      return !query || `${item.name} ${item.build}`.toLocaleLowerCase("ru-RU").includes(query);
    });
  }

  function isFeatured(item) {
    return item?.pinned === true;
  }

  function featuredFiles() {
    return state.files.filter(isFeatured);
  }

  function fileNameFromUrl(url) {
    try {
      return decodeURIComponent(new URL(url).pathname.split("/").filter(Boolean).pop() || "Архив");
    } catch (_) {
      return "Архив";
    }
  }

  function pageNumbers(current, total) {
    if (total <= 5) return Array.from({ length: total }, (_, index) => index + 1);
    let start = Math.max(1, current - 2);
    let end = Math.min(total, start + 4);
    start = Math.max(1, end - 4);
    return Array.from({ length: end - start + 1 }, (_, index) => start + index);
  }

  function renderCard(item, position, featured = false) {
    const password = item.password || "Не требуется";
    const progress = state.progress.get(item.url);
    const downloaded = state.downloads.get(item.url);
    const url = escapeHtml(item.url);
    const newest = !featured && !state.query && position < 3;
    const marker = featured ? "ЗАКРЕПЛЕНО" : `ФАЙЛ ${String(position + 1).padStart(3, "0")}`;
    return `
      <article class="file-card ${featured ? "featured" : ""}">
        <div class="file-card-top">
          <span class="file-icon">📦</span>
          <div class="file-title">
            <div><span>${marker}</span>${newest ? "<b>НОВОЕ</b>" : ""}</div>
            <h3 title="${escapeHtml(item.name)}">${escapeHtml(item.name)}</h3>
          </div>
        </div>
        <div class="file-details">
          <div><small>БИЛД</small><strong>${escapeHtml(item.build || "Не указан")}</strong></div>
          <div><small>ПАРОЛЬ</small><strong class="file-password ${item.password ? "" : "empty"}">${escapeHtml(password)}</strong></div>
        </div>
        <div class="file-actions">
          ${item.password ? `<button class="file-copy" data-files-action="copy" data-files-url="${url}"><span>⧉</span> Копировать пароль</button>` : '<span class="file-no-password">Без пароля</span>'}
          ${downloaded
            ? `<button class="file-download downloaded" data-files-action="reveal" data-files-url="${url}"><span>↗</span> Открыть папку</button>`
            : progress
              ? `<button class="file-cancel" data-files-action="cancel" data-files-url="${url}" ${progress.cancelling ? "disabled" : ""}><span>×</span> ${progress.cancelling ? "Отменяем…" : `Отменить ${progress.percent == null ? "" : `${progress.percent}%`}`}</button>`
              : `<button class="file-download" data-files-action="download" data-files-url="${url}"><span>↓</span> Скачать</button>`}
        </div>
      </article>`;
  }

  function render() {
    const pinned = featuredFiles();
    const all = filteredFiles();
    const totalPages = Math.max(1, Math.ceil(all.length / PAGE_SIZE));
    state.page = Math.min(Math.max(1, state.page), totalPages);
    const start = (state.page - 1) * PAGE_SIZE;
    const pageItems = all.slice(start, start + PAGE_SIZE);

    $("#files-total").textContent = String(state.files.length || 0);
    $("#files-loading").classList.toggle("hidden", !state.loading);
    $("#files-featured").classList.toggle("hidden", state.loading || pinned.length === 0);
    $("#files-regular-divider").classList.toggle("hidden", state.loading || pinned.length === 0);
    $("#files-empty").classList.toggle("hidden", state.loading || pageItems.length > 0);
    $("#files-grid").classList.toggle("hidden", state.loading || pageItems.length === 0);
    $("#files-pagination").classList.toggle("hidden", state.loading || all.length <= PAGE_SIZE);

    $("#files-featured-grid").innerHTML = pinned
      .map((item, index) => renderCard(item, index, true))
      .join("");
    $("#files-grid").innerHTML = pageItems
      .map((item, pageIndex) => renderCard(item, start + pageIndex))
      .join("");

    $("#files-pages").innerHTML = pageNumbers(state.page, totalPages)
      .map((page) => `<button class="${page === state.page ? "active" : ""}" data-files-page="${page}">${page}</button>`)
      .join("");
    $("#files-prev").disabled = state.page <= 1;
    $("#files-next").disabled = state.page >= totalPages;
    $("#files-page-state").textContent = `Страница ${state.page} из ${totalPages}`;

    if (!state.loading && state.error) {
      $("#files-status").textContent = state.error;
    } else if (!state.loading) {
      const range = all.length ? `${start + 1}–${start + pageItems.length} из ${all.length}` : "0 файлов";
      const source = state.cached ? " · локальная копия" : "";
      $("#files-status").textContent = `${range}${source}`;
    }
  }

  async function loadCatalog(force = false, silent = false) {
    if (state.refreshing || (state.loaded && !force)) return;
    state.refreshing = true;
    state.loading = !silent;
    state.error = "";
    if (!silent) $("#files-status").textContent = "Загрузка списка из Gist…";
    $("#files-refresh").disabled = true;
    if (!silent) render();

    try {
      if (!invoke) throw new Error("Системный модуль недоступен");
      const catalog = await invoke("files_catalog");
      state.files = Array.isArray(catalog?.files) ? catalog.files : [];
      state.cached = Boolean(catalog?.cached);
      state.error = "";
      state.loaded = true;
      if (!silent) state.page = 1;
    } catch (error) {
      if (!silent) {
        state.files = [];
        state.loaded = false;
        state.error = `Не удалось получить каталог: ${String(error)}`;
      }
    } finally {
      state.loading = false;
      state.refreshing = false;
      $("#files-refresh").disabled = false;
      render();
    }
  }

  async function copyPassword(item, button) {
    if (!item?.password) return;
    try {
      await navigator.clipboard.writeText(item.password);
      const original = button.innerHTML;
      button.innerHTML = "<span>✓</span> Скопировано";
      window.setTimeout(() => { button.innerHTML = original; }, 1400);
    } catch (error) {
      $("#files-status").textContent = `Не удалось скопировать пароль: ${String(error)}`;
    }
  }

  async function download(item) {
    if (!item?.url || !invoke) return;
    state.progress.set(item.url, { percent: 0 });
    render();
    try {
      const result = await invoke("files_download", {
        url: item.url,
        suggestedName: fileNameFromUrl(item.url),
      });
      state.downloads.set(item.url, result);
      state.progress.delete(item.url);
      render();
      $("#files-status").textContent = `Файл «${result.fileName}» сохранён в папку «Загрузки»`;
    } catch (error) {
      state.progress.delete(item.url);
      render();
      const message = String(error);
      $("#files-status").textContent = message.includes("Загрузка отменена")
        ? "Скачивание отменено — временный файл удалён"
        : `Не удалось скачать файл: ${message}`;
    }
  }

  async function cancelDownload(item) {
    const progress = state.progress.get(item?.url);
    if (!progress || !invoke) return;
    state.progress.set(item.url, { ...progress, cancelling: true });
    render();
    try {
      await invoke("files_cancel_download", { url: item.url });
    } catch (error) {
      state.progress.set(item.url, { ...progress, cancelling: false });
      render();
      $("#files-status").textContent = `Не удалось отменить скачивание: ${String(error)}`;
    }
  }

  async function reveal(item) {
    const downloaded = state.downloads.get(item?.url);
    if (!downloaded?.path || !invoke) return;
    try {
      await invoke("files_reveal", { path: downloaded.path });
    } catch (error) {
      $("#files-status").textContent = `Не удалось открыть папку: ${String(error)}`;
    }
  }

  function updateProgress(payload) {
    if (!payload?.url || !state.progress.has(payload.url)) return;
    state.progress.set(payload.url, { percent: Number.isFinite(payload.percent) ? payload.percent : null });
    document.querySelectorAll('[data-files-action="cancel"]').forEach((button) => {
      if (button.dataset.filesUrl !== payload.url) return;
      const progress = state.progress.get(payload.url);
      if (progress?.cancelling) return;
      const label = payload.percent == null ? "Отменить" : `Отменить ${payload.percent}%`;
      button.innerHTML = `<span>×</span> ${label}`;
    });
  }

  function bindEvents() {
    $("#files-refresh")?.addEventListener("click", () => loadCatalog(true));
    $("#files-search")?.addEventListener("input", (event) => {
      state.query = event.currentTarget.value;
      state.page = 1;
      render();
    });
    $("#files-prev")?.addEventListener("click", () => { state.page -= 1; render(); });
    $("#files-next")?.addEventListener("click", () => { state.page += 1; render(); });
    $("#files-pages")?.addEventListener("click", (event) => {
      const button = event.target.closest("[data-files-page]");
      if (!button) return;
      state.page = Number(button.dataset.filesPage) || 1;
      render();
      document.querySelector("#view-files")?.scrollIntoView({ block: "start", behavior: "smooth" });
    });
    $("#view-files")?.addEventListener("click", (event) => {
      const button = event.target.closest("[data-files-action]");
      if (!button) return;
      const item = state.files.find((entry) => entry.url === button.dataset.filesUrl);
      if (button.dataset.filesAction === "copy") copyPassword(item, button);
      if (button.dataset.filesAction === "download") download(item);
      if (button.dataset.filesAction === "cancel") cancelDownload(item);
      if (button.dataset.filesAction === "reveal") reveal(item);
    });
    window.addEventListener("cgs:view-changed", (event) => {
      if (event.detail === "files") loadCatalog(true);
    });
    if (listen) {
      listen("files-download-progress", (event) => updateProgress(event.payload ?? event)).catch(() => {});
    }
    window.setInterval(() => {
      if ($("#view-files")?.classList.contains("active")) loadCatalog(true, true);
    }, 3000);
  }

  bindEvents();
})();
