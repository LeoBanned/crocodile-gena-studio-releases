(() => {
  "use strict";

  const tauri = window.__TAURI__;
  const invoke = tauri?.core?.invoke;
  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];

  const state = {
    gameDir: "",
    appid: null,
    gameName: "",
    banner: "",
    engine: "—",
    exes: [],
    steamApis: [],
    dlcs: {},
    status: "clean",
    hasEos: false,
    hasEosBackup: false,
    hasEpicfix: false,
    scanning: false,
    saveLocations: [],
    savesScanned: false,
    dlc: {
      gameDir: "",
      appid: null,
      name: "",
      banner: "",
      source: "",
      targets: [],
      installed: false,
      catalog: [],
      loaded: false,
      scanning: false,
      games: [],
      libraryScanned: false,
      libraryScanning: false,
      extraLibrary: localStorage.getItem("dlcExtraLibrary") || "",
      detail: false,
      method: "cream",
      exes: [],
      exePath: "",
      proxy: "version.dll",
      requestId: 0,
      catalogLoading: false,
      catalogError: "",
      catalogStatus: "",
      operating: false,
      configuredDlcs: [],
      configUnlockAll: true,
      coverObjectUrls: [],
    },
    lua: {
      steamPath: "",
      luaFolder: "",
      loaderStatus: "unknown",
      loaderName: "",
      loaderIssue: "",
      games: [],
      names: {},
      loaded: false,
      service: "ryuu",
      hubcapKey: "",
      hubcapUsage: { dailyUsage: 0, dailyLimit: 25, remaining: 25 },
      hubcapUsageError: "",
      hubcapUsageLoading: false,
      millennium: {
        status: "checking",
        message: "Проверяем Millennium и установленную версию плагина.",
        millenniumInstalled: false,
        pluginInstalled: false,
        pluginCurrent: false,
        pluginVersion: "",
        bundledVersion: "0.5.2",
        pluginPath: "",
      },
      millenniumPrompted: false,
    },
    updates: {
      checking: false,
      promptedVersion: "",
      latest: null,
    },
  };

  const viewMeta = {
    home: ["ОБЗОР", "Главная"],
    fix: ["РАБОЧАЯ ОБЛАСТЬ", "Online Fix"],
    dlc: ["МОДУЛЬ 03", "DLC Unlocker"],
    lua: ["МОДУЛЬ 04", "CrocLua"],
    hypervisor: ["МОДУЛЬ 05", "Гипервизор"],
    files: ["МОДУЛЬ 06", "Кряки"],
    utilities: ["МОДУЛЬ 07", "Утилиты"],
    saves: ["МОДУЛЬ 02", "Менеджер сохранений"],
    settings: ["ПРИЛОЖЕНИЕ", "Настройки"],
  };

  const moduleNames = ["fix", "saves", "dlc", "lua", "hypervisor", "files", "utilities"];
  let hiddenModules = new Set();
  const platformHiddenModules = new Set();
  try {
    const stored = JSON.parse(localStorage.getItem("cgs-hidden-modules") || "[]");
    if (Array.isArray(stored)) hiddenModules = new Set(stored.filter((name) => moduleNames.includes(name)));
  } catch (_) { /* Ignore an invalid saved preference. */ }

  function applyModuleVisibility() {
    $$(".nav-item[data-view], [data-open-view]").forEach((item) => {
      const name = item.dataset.view || item.dataset.openView;
      item.classList.toggle("module-hidden", hiddenModules.has(name) || platformHiddenModules.has(name));
    });
    $$("[data-module-toggle]").forEach((input) => {
      const unavailable = platformHiddenModules.has(input.dataset.moduleToggle);
      input.checked = !hiddenModules.has(input.dataset.moduleToggle) && !unavailable;
      input.disabled = unavailable;
      input.closest("label")?.classList.toggle("platform-unavailable", unavailable);
    });
    const active = $("[data-view-panel].active")?.dataset.viewPanel;
    if (hiddenModules.has(active) || platformHiddenModules.has(active)) switchView("home");
  }

  function setModuleVisible(name, visible) {
    if (!moduleNames.includes(name) || platformHiddenModules.has(name)) return;
    if (visible) hiddenModules.delete(name);
    else hiddenModules.add(name);
    localStorage.setItem("cgs-hidden-modules", JSON.stringify([...hiddenModules]));
    applyModuleVisibility();
  }

  async function loadPlatformInfo() {
    if (!invoke) return;
    try {
      const info = await invoke("platform_info");
      if (!info?.os) return;
      document.body.dataset.platform = info.os;
      (info.windowsOnlyModules || []).forEach((name) => platformHiddenModules.add(name));
      if (!info.windows) {
        $$(".windows-only").forEach((element) => element.classList.add("hidden"));
        $("#save-format option[value='rar']")?.remove();
      }
    } catch (error) {
      log(`Платформа: ${errorText(error)}`, "error");
    }
  }

  function now() {
    return new Date().toLocaleTimeString(window.cgsI18n?.locale?.() || "ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  function log(message, type = "info") {
    const container = $("#activity-log");
    const entry = document.createElement("div");
    entry.className = `log-entry ${type}`;
    const time = document.createElement("time");
    time.textContent = now();
    const text = document.createElement("span");
    text.textContent = String(message);
    entry.append(time, text);
    container.appendChild(entry);
    container.scrollTop = container.scrollHeight;
    window.dispatchEvent(new CustomEvent("cgs:activity", { detail: { type } }));
  }

  function toast(message, type = "success") {
    const item = document.createElement("div");
    item.className = `toast ${type}`;
    item.textContent = message;
    $("#toast-stack").appendChild(item);
    window.dispatchEvent(new CustomEvent("cgs:activity", { detail: { type } }));
    setTimeout(() => {
      item.style.opacity = "0";
      item.style.transform = "translateX(15px)";
      setTimeout(() => item.remove(), 250);
    }, 3600);
  }

  function errorText(error) {
    if (typeof error === "string") return error;
    return error?.message || "Неизвестная ошибка";
  }

  async function call(command, args = {}) {
    if (!invoke) throw new Error("Системный модуль недоступен. Перезапустите приложение.");
    return invoke(command, args);
  }

  window.cgsInvokePrivileged = async (command, args = {}, view = "hypervisor") => {
    if (!invoke) throw new Error("Системный модуль недоступен. Перезапустите приложение.");
    if (await invoke("process_is_elevated")) return invoke(command, args);
    const confirmed = await showModal(
      "Нужны права администратора",
      "Для этой операции программа перезапустится с повышенными правами. Подтвердите запрос Windows и повторите действие.",
      { confirm: true, confirmText: "Перезапустить", symbol: "🛡" },
    );
    if (!confirmed) throw new Error("Операция отменена");
    localStorage.setItem("cgs-admin-reopen-view", view);
    try {
      await invoke("relaunch_as_admin");
    } catch (error) {
      localStorage.removeItem("cgs-admin-reopen-view");
      throw error;
    }
    setTimeout(() => invoke("close_window").catch(() => window.close()), 80);
    return new Promise(() => {});
  };

  function setBusy(button, busy, label) {
    if (!button) return;
    if (busy) {
      button.dataset.original = button.innerHTML;
      button.classList.add("busy");
      button.disabled = true;
      if (label) button.innerHTML = `<span>${label}</span>`;
    } else {
      button.classList.remove("busy");
      button.disabled = false;
      if (button.dataset.original) button.innerHTML = button.dataset.original;
      delete button.dataset.original;
    }
    window.dispatchEvent(new CustomEvent("cgs:busy-changed", { detail: { id: button.id, busy } }));
  }

  function switchView(name) {
    if (!viewMeta[name]) return;
    if (hiddenModules.has(name) || platformHiddenModules.has(name)) name = "home";
    $$("[data-view-panel]").forEach((panel) => panel.classList.toggle("active", panel.dataset.viewPanel === name));
    $$(".nav-item").forEach((item) => item.classList.toggle("active", item.dataset.view === name));
    $("#page-eyebrow").textContent = viewMeta[name][0];
    $("#page-title").textContent = viewMeta[name][1];
    if (name === "lua") luaRefresh();
    window.dispatchEvent(new CustomEvent("cgs:view-changed", { detail: name }));
    $(".main-area").scrollTo({ top: 0, behavior: window.cgsPersonalization?.motionEnabled() === false ? "auto" : "smooth" });
  }

  function showModal(title, message, options = {}) {
    const { confirm = false, confirmText = "Продолжить", cancelText = "Отмена", symbol = confirm ? "?" : "✓" } = options;
    const backdrop = $("#modal-backdrop");
    $("#modal-title").textContent = title;
    $("#modal-message").textContent = message;
    $("#modal-symbol").textContent = symbol;
    $("#modal-cancel").classList.toggle("hidden", !confirm);
    $("#modal-cancel").textContent = cancelText;
    $("#modal-confirm").querySelector("span")?.remove();
    $("#modal-confirm").textContent = confirm ? confirmText : "Понятно";
    backdrop.classList.remove("hidden");
    return new Promise((resolve) => {
      const finish = (value) => {
        backdrop.classList.add("hidden");
        $("#modal-confirm").onclick = null;
        $("#modal-cancel").onclick = null;
        resolve(value);
      };
      $("#modal-confirm").onclick = () => finish(true);
      $("#modal-cancel").onclick = () => finish(false);
    });
  }

  window.cgsShowModal = showModal;

  function basename(path) {
    return String(path || "").split(/[\\/]/).filter(Boolean).pop() || "Неизвестная игра";
  }

  function pluralPackages(count) {
    const mod10 = count % 10;
    const mod100 = count % 100;
    if (mod10 === 1 && mod100 !== 11) return `${count} пакет`;
    if (mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14)) return `${count} пакета`;
    return `${count} пакетов`;
  }

  function formatBytes(bytes) {
    const value = Number(bytes) || 0;
    if (value < 1024) return `${value} Б`;
    const units = ["КБ", "МБ", "ГБ", "ТБ"];
    let amount = value / 1024;
    let unit = 0;
    while (amount >= 1024 && unit < units.length - 1) {
      amount /= 1024;
      unit += 1;
    }
    return `${amount >= 10 ? amount.toFixed(1) : amount.toFixed(2)} ${units[unit]}`;
  }

  function updateSaveSelection() {
    const checkboxes = $$("#save-list input[type='checkbox']");
    const selected = checkboxes.filter((item) => item.checked);
    const button = $("#create-save-archive");
    button.disabled = selected.length === 0;
    const label = button.querySelector("span");
    if (label) label.textContent = selected.length ? `Создать архив · ${selected.length}` : "Создать архив";
    const toggle = $("#save-toggle-selection");
    toggle.disabled = checkboxes.length === 0;
    toggle.textContent = checkboxes.length > 0 && selected.length === checkboxes.length
      ? "Снять выделение"
      : "Выделить все";
  }

  function renderSaveLocations() {
    const locations = state.saveLocations || [];
    const fileCount = locations.reduce((sum, item) => sum + (Number(item.fileCount) || 0), 0);
    const totalBytes = locations.reduce((sum, item) => sum + (Number(item.totalBytes) || 0), 0);
    $("#save-location-count").textContent = locations.length.toLocaleString(window.cgsI18n?.locale?.() || "ru-RU");
    $("#save-file-count").textContent = fileCount.toLocaleString(window.cgsI18n?.locale?.() || "ru-RU");
    $("#save-total-size").textContent = formatBytes(totalBytes);

    const list = $("#save-list");
    const empty = $("#save-empty");
    list.replaceChildren();
    list.classList.toggle("hidden", locations.length === 0);
    empty.classList.toggle("hidden", locations.length > 0);
    if (!locations.length) {
      $("#save-empty h3").textContent = state.savesScanned ? "Сохранения не найдены" : "Сканирование ещё не запускалось";
      $("#save-empty p").textContent = state.savesScanned
        ? "Популярные расположения проверены. Возможно, игра хранит прогресс в собственной папке."
        : "Нажмите «Найти сохранения», чтобы проверить популярные расположения.";
      updateSaveSelection();
      return;
    }

    locations.forEach((location) => {
      const row = document.createElement("label");
      row.className = "save-location";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.checked = true;
      checkbox.dataset.path = location.path;
      checkbox.addEventListener("change", updateSaveSelection);

      const icon = document.createElement("span");
      icon.className = "save-location-icon";
      icon.textContent = "SV";

      const copy = document.createElement("span");
      copy.className = "save-location-copy";
      const title = document.createElement("strong");
      title.textContent = location.label;
      const path = document.createElement("code");
      path.textContent = location.path;
      path.title = location.path;
      copy.append(title, path);

      const meta = document.createElement("span");
      meta.className = "save-location-meta";
      const numbers = document.createElement("b");
      numbers.textContent = `${Number(location.fileCount).toLocaleString(window.cgsI18n?.locale?.() || "ru-RU")} файлов · ${formatBytes(location.totalBytes)}`;
      const category = document.createElement("span");
      category.textContent = location.category;
      category.title = location.category;
      meta.append(numbers, category);
      row.append(checkbox, icon, copy, meta);
      list.appendChild(row);
    });
    updateSaveSelection();
  }

  async function scanSaveLocations() {
    const button = $("#scan-saves-button");
    setBusy(button, true, "Сканируем…");
    log("Запущен поиск папок с игровыми сохранениями", "system");
    try {
      const result = await call("scan_save_locations");
      state.saveLocations = Array.isArray(result) ? result : [];
      state.savesScanned = true;
      renderSaveLocations();
      const files = state.saveLocations.reduce((sum, item) => sum + (Number(item.fileCount) || 0), 0);
      log(`Найдено папок сохранений: ${state.saveLocations.length}; файлов: ${files}`, "success");
      toast(state.saveLocations.length ? `Найдено папок: ${state.saveLocations.length}` : "Сохранения не найдены", state.saveLocations.length ? "success" : "error");
    } catch (error) {
      log(`Ошибка поиска сохранений: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function createSaveArchive() {
    const paths = $$("#save-list input[type='checkbox']:checked").map((item) => item.dataset.path).filter(Boolean);
    if (!paths.length) return toast("Выберите хотя бы одну папку", "error");
    const format = $("#save-format").value === "rar" ? "rar" : "zip";
    let destination;
    try {
      destination = await call("pick_save_archive", { format });
    } catch (error) {
      return toast(errorText(error), "error");
    }
    if (!destination) return;
    const accepted = await showModal(
      "Создать архив сохранений?",
      `Будет скопировано выбранных папок: ${paths.length}. Оригинальные файлы останутся без изменений. Формат архива: ${format.toUpperCase()}.`,
      { confirm: true, confirmText: "Создать архив", cancelText: "Отмена", symbol: "↓" },
    );
    if (!accepted) return;

    const button = $("#create-save-archive");
    setBusy(button, true, "Архивация…");
    log(`Создание ${format.toUpperCase()}-архива сохранений`, "system");
    try {
      const result = await call("create_save_archive", { paths, destination, format });
      if (consumeResult(result)) {
        await showModal("Архив готов", `Резервная копия успешно создана:\n${result.archivePath || destination}`, { symbol: "✓" });
      }
    } catch (error) {
      log(`Ошибка архивации: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
      updateSaveSelection();
    }
  }

  async function restoreSaveArchive() {
    let archive;
    try {
      archive = await call("pick_save_restore_archive");
    } catch (error) {
      return toast(errorText(error), "error");
    }
    if (!archive) return;
    const accepted = await showModal(
      "Восстановить сохранения?",
      "Программа распознает исходные папки и автоматически разложит файлы. Существующие версии файлов будут предварительно сохранены в отдельную резервную папку. Перед продолжением закройте игры.",
      { confirm: true, confirmText: "Восстановить", cancelText: "Отмена", symbol: "↺" },
    );
    if (!accepted) return;

    const button = $("#restore-save-archive");
    setBusy(button, true, "Восстановление…");
    log(`Восстановление сохранений из архива: ${archive}`, "system");
    try {
      const result = await call("restore_save_archive", { archive });
      if (consumeResult(result)) {
        await showModal(
          "Сохранения восстановлены",
          "Файлы автоматически разложены по исходным папкам. Можно запускать игры и проверять прогресс.",
          { symbol: "✓" },
        );
        await scanSaveLocations();
      }
    } catch (error) {
      log(`Ошибка восстановления сохранений: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  function dlcExe() {
    return state.dlc.exes.find((exe) => exe.path === state.dlc.exePath);
  }

  function dlcInstalled() {
    return state.dlc.method === "smaki" ? Boolean(dlcExe()?.proxy) : state.dlc.installed;
  }

  function dlcShowDetail(detail) {
    state.dlc.detail = detail;
    $("#view-dlc").classList.toggle("dlc-detail", detail);
    $("#dlc-library-panel").classList.toggle("hidden", detail);
    $("#dlc-manual-panel").classList.toggle("hidden", detail);
    $("#dlc-workspace").classList.toggle("hidden", !detail);
    $("#dlc-library-scan").classList.toggle("hidden", detail);
  }

  function renderDlcLibrary() {
    const list = $("#dlc-game-list");
    const filter = $("#dlc-library-search").value.trim().toLowerCase();
    const games = state.dlc.games.filter((game) => !filter || game.name.toLowerCase().includes(filter) || String(game.appid).includes(filter));
    state.dlc.coverObjectUrls.forEach((url) => URL.revokeObjectURL(url));
    state.dlc.coverObjectUrls = [];
    list.replaceChildren();
    list.classList.toggle("hidden", !games.length);
    $("#dlc-library-empty").classList.toggle("hidden", games.length > 0);
    $("#dlc-library-count").textContent = state.dlc.games.length;
    $("#dlc-library-empty h3").textContent = state.dlc.libraryScanned ? "Игры не найдены" : "Ваша библиотека игр";
    for (const game of games) {
      const button = document.createElement("button");
      button.className = "dlc-library-game";
      button.disabled = state.dlc.scanning || state.dlc.libraryScanning;
      const cover = document.createElement("div"); cover.className = "dlc-library-cover";
      const placeholder = document.createElement("span"); placeholder.textContent = "🎮";
      const image = document.createElement("img"); image.src = game.headerImage; image.alt = ""; image.loading = "lazy";
      image.onerror = async () => {
        image.onerror = null;
        try {
          const cover = await call("steam_library_cover", { appid: Number(game.appid), steamPath: state.lua.steamPath || null });
          if (!cover?.bytes?.length || !image.isConnected) return image.remove();
          const url = URL.createObjectURL(new Blob([new Uint8Array(cover.bytes)], { type: cover.mime || "image/jpeg" }));
          state.dlc.coverObjectUrls.push(url);
          image.src = url;
        } catch (_) { image.remove(); }
      };
      cover.append(placeholder, image);
      const copy = document.createElement("div"); copy.className = "dlc-library-copy";
      const name = document.createElement("strong"); name.textContent = game.name;
      const id = document.createElement("small"); id.textContent = `AppID ${game.appid}`;
      const path = document.createElement("code"); path.textContent = game.gameDir; path.title = game.gameDir;
      copy.append(name, id, path);
      const arrow = document.createElement("span"); arrow.className = "dlc-library-arrow"; arrow.textContent = "→";
      button.append(cover, copy, arrow);
      button.addEventListener("click", () => scanDlcDirectory(game.gameDir, game));
      list.append(button);
    }
  }

  async function scanDlcLibrary() {
    if (state.dlc.libraryScanning || state.dlc.operating) return;
    state.dlc.libraryScanning = true;
    const button = $("#dlc-library-scan");
    setBusy(button, true, "Сканирование…");
    $("#dlc-library-folder").disabled = true;
    $("#dlc-library-status").textContent = "Проверяем библиотеки Steam…";
    renderDlcLibrary();
    try {
      const result = await call("scan_installed_steam_games", { steamPath: state.lua.steamPath || null, libraryPath: state.dlc.extraLibrary || null });
      state.dlc.games = result.games || [];
      state.dlc.libraryScanned = true;
      $("#dlc-library-status").textContent = `Установлено игр: ${state.dlc.games.length} · Библиотек: ${result.libraries.length}`;
      for (const warning of result.warnings || []) log(warning, "error");
      if (result.warnings?.length) $("#dlc-library-status").textContent += " · Часть библиотек не удалось прочитать";
    } catch (error) {
      $("#dlc-library-status").textContent = errorText(error);
      toast(errorText(error), "error");
    } finally {
      state.dlc.libraryScanning = false;
      setBusy(button, false);
      $("#dlc-library-folder").disabled = false;
      renderDlcLibrary();
    }
  }

  function renderDlcMethod() {
    const smaki = state.dlc.method === "smaki";
    $$("[data-dlc-method]").forEach((button) => {
      const active = button.dataset.dlcMethod === state.dlc.method;
      button.classList.toggle("active", active);
      button.setAttribute("aria-pressed", String(active));
      button.disabled = state.dlc.operating;
    });
    $$("[data-dlc-proxy]").forEach((button) => {
      const active = button.dataset.dlcProxy === state.dlc.proxy;
      button.classList.toggle("active", active); button.setAttribute("aria-pressed", String(active));
      button.disabled = state.dlc.operating;
    });
    $("#dlc-exe-select").disabled = state.dlc.operating;
    $("#dlc-back").disabled = state.dlc.operating;
    $("#dlc-smaki-options").classList.toggle("hidden", !smaki);
    $("#dlc-cream-options").classList.toggle("hidden", smaki);
    const exe = dlcExe();
    const destination = exe ? exe.path.replace(/[^\\/]+$/, state.dlc.proxy) : "—";
    $("#dlc-proxy-destination").textContent = destination;
    const supportedSmaki = exe && (exe.bitness === "x64" || exe.bitness === "x86");
    $("#dlc-smaki-note").textContent = !exe ? "EXE игры не найден. Установка недоступна." : !supportedSmaki ? "SmokeAPI поддерживает только x64 и x86 EXE. Для этой игры выберите CreamAPI." : `Будет использована ${exe.bitness}-версия SmokeAPI. Если прокси не загружается игрой, попробуйте другое имя DLL.`;
    $("#dlc-install-note").textContent = smaki ? "Прокси и конфигурация устанавливаются рядом с выбранным EXE. Steam API не изменяется." : "Оригинальная библиотека Steam сохраняется и восстанавливается при удалении.";
    const installed = dlcInstalled();
    const conflict = smaki ? state.dlc.installed : state.dlc.exes.some((item) => item.proxy);
    $("#dlc-method-conflict").classList.toggle("hidden", !conflict);
    $("#dlc-method-conflict").textContent = smaki ? "Установлен CreamAPI. Выберите CreamAPI и удалите его, затем установите SmokeAPI." : "Установлен SmokeAPI. Выберите SmokeAPI и удалите его, затем установите CreamAPI.";
    $("#dlc-state").textContent = installed ? "UNLOCKED" : "CLEAN";
    $("#dlc-state").classList.toggle("installed", installed);
    $("#dlc-uninstall-button").classList.toggle("hidden", !installed);
    $("#dlc-uninstall-button").disabled = state.dlc.operating;
    $("#dlc-target-count").textContent = smaki ? (exe ? 1 : 0) : state.dlc.targets.length;
    $("#dlc-target-count").parentElement.querySelector("small").textContent = smaki ? "ПРОКСИ РЯДОМ С EXE" : "БИБЛИОТЕК";
    $("#dlc-target-count").parentElement.querySelector("span").textContent = smaki ? state.dlc.proxy : "steam_api для подмены";
    updateDlcSelection();
  }

  function restoreDlcSelection() {
    const exe = dlcExe();
    const installed = dlcInstalled();
    if (!installed) return;
    const configured = state.dlc.method === "smaki" ? Object.entries(exe.configuredDlcs || {}).map(([id, name]) => ({ id, name })) : state.dlc.configuredDlcs;
    const ids = new Set(configured.map((item) => item.id));
    $("#dlc-unlock-all").checked = state.dlc.method === "smaki" ? exe.unlockAll : state.dlc.configUnlockAll;
    for (const item of configured) if (!state.dlc.catalog.some((entry) => entry.id === item.id)) state.dlc.catalog.push({ ...item, enabled: true });
    state.dlc.catalog.forEach((item) => { item.enabled = ids.has(item.id); });
  }

  function updateDlcSelection() {
    const catalog = state.dlc.catalog;
    const selected = catalog.filter((item) => item.enabled);
    const unlockAll = $("#dlc-unlock-all").checked;
    $("#dlc-total-count").textContent = catalog.length.toLocaleString(window.cgsI18n?.locale?.() || "ru-RU");
    $("#dlc-count-value").textContent = catalog.length ? catalog.length : (state.dlc.appid ? "0" : "—");
    $("#dlc-selected-count").textContent = unlockAll ? "ВСЕ" : selected.length.toLocaleString(window.cgsI18n?.locale?.() || "ru-RU");
    const toggle = $("#dlc-toggle-selection");
    toggle.disabled = catalog.length === 0 || unlockAll;
    toggle.textContent = catalog.length > 0 && selected.length === catalog.length ? "Снять выделение" : "Выделить все";
    $("#dlc-catalog").classList.toggle("dimmed", unlockAll);

    const install = $("#dlc-install-button");
    const compatible = state.dlc.method === "smaki" ? ["x64", "x86"].includes(dlcExe()?.bitness) : state.dlc.targets.length > 0;
    const otherInstalled = state.dlc.method === "smaki" ? state.dlc.installed : state.dlc.exes.some((exe) => exe.proxy);
    install.disabled = state.dlc.operating || state.dlc.scanning || state.dlc.catalogLoading || !state.dlc.gameDir || !state.dlc.appid || !compatible || otherInstalled || (!unlockAll && !selected.length);
    const label = install.querySelector("span");
    if (label) label.textContent = dlcInstalled() ? "Переустановить анлокер" : "Установить DLC Unlocker";
    $("#dlc-catalog-retry").disabled = state.dlc.operating || state.dlc.scanning || state.dlc.catalogLoading;
    $("#dlc-rescan-button").disabled = state.dlc.operating || state.dlc.scanning || state.dlc.catalogLoading;
    $("#dlc-manual-button").disabled = state.dlc.operating || state.dlc.scanning || state.dlc.catalogLoading;
  }

  function renderDlcCatalog(filter = "") {
    const container = $("#dlc-catalog");
    const empty = $("#dlc-empty");
    const catalog = state.dlc.catalog;
    $("#dlc-catalog-status").textContent = state.dlc.catalogLoading ? "Сканируем DLC…" : state.dlc.catalogError || state.dlc.catalogStatus;
    $("#dlc-catalog-status").classList.toggle("warning", Boolean(state.dlc.catalogError));
    container.replaceChildren();
    container.classList.toggle("hidden", catalog.length === 0);
    empty.classList.toggle("hidden", catalog.length > 0);
    if (!catalog.length) {
      $("#dlc-empty h3").textContent = state.dlc.catalogLoading ? "Сканируем DLC…" : state.dlc.catalogError ? "Каталог недоступен" : state.dlc.loaded ? "Дополнения не найдены" : "Каталог ещё не загружен";
      $("#dlc-empty p").textContent = state.dlc.catalogLoading ? "Проверяем каталоги Steam и локальный кэш клиента." : state.dlc.catalogError ? "Не удалось проверить список DLC. Повторите сканирование; ошибка сети не означает, что дополнений нет." : state.dlc.loaded ? "Доступные источники не вернули DLC. Для некоторых игр полный каталог может быть недоступен." : "Определите игру, чтобы загрузить список её дополнений.";
      updateDlcSelection();
      return;
    }

    const normalized = filter.trim().toLowerCase();
    const visible = catalog.filter((item) => !normalized || item.id.includes(normalized) || item.name.toLowerCase().includes(normalized));
    if (!visible.length) {
      const inline = document.createElement("div");
      inline.className = "empty-inline";
      inline.textContent = "Ничего не найдено";
      container.appendChild(inline);
      updateDlcSelection();
      return;
    }

    visible.forEach((entry) => {
      const row = document.createElement("label");
      row.className = "save-location dlc-row";

      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.checked = entry.enabled;
      checkbox.addEventListener("change", () => {
        entry.enabled = checkbox.checked;
        updateDlcSelection();
      });

      const icon = document.createElement("span");
      icon.className = "save-location-icon";
      icon.textContent = "DLC";

      const copy = document.createElement("span");
      copy.className = "save-location-copy";
      const title = document.createElement("strong");
      title.textContent = entry.name;
      title.title = entry.name;
      const id = document.createElement("code");
      id.textContent = `AppID ${entry.id}`;
      copy.append(title, id);

      row.append(checkbox, icon, copy);
      container.appendChild(row);
    });
    updateDlcSelection();
  }

  function renderDlcGame() {
    dlcShowDetail(true);
    $("#dlc-detail-path").textContent = state.dlc.gameDir;
    $("#dlc-detail-path").title = state.dlc.gameDir;
    const { name, gameDir, appid, targets, installed, catalog, banner, source } = state.dlc;
    $("#dlc-title").textContent = name || basename(gameDir);
    $("#dlc-appid-value").textContent = appid || "Не определён";
    const apis = [...new Set(targets.map((item) => item.bitness))];
    $("#dlc-api-value").textContent = targets.length ? (targets.length > 1 ? `${targets.length} × ${apis.join("/")}` : apis[0] || "—") : "Не найдена";
    $("#dlc-count-value").textContent = catalog.length ? catalog.length : (appid ? "0" : "—");
    $("#dlc-target-count").textContent = targets.length.toLocaleString(window.cgsI18n?.locale?.() || "ru-RU");
    $("#dlc-source-value").textContent = source || "—";
    $("#dlc-source-value").title = source || "";

    const chip = $("#dlc-config-chip");
    chip.textContent = installed ? "АНЛОКЕР УСТАНОВЛЕН" : targets.length ? "БИБЛИОТЕКА НАЙДЕНА" : "STEAM API НЕ НАЙДЕН";
    chip.classList.toggle("installed", installed);

    const badge = $("#dlc-state");
    badge.classList.toggle("installed", installed);
    badge.textContent = installed ? "UNLOCKED" : "CLEAN";
    $("#dlc-uninstall-button").classList.toggle("hidden", !installed);

    const image = $("#dlc-banner");
    const fallback = $("#dlc-cover-fallback");
    image.classList.remove("loaded");
    fallback.classList.remove("hidden");
    if (banner) {
      image.onload = () => { image.classList.add("loaded"); fallback.classList.add("hidden"); };
      image.onerror = () => { image.classList.remove("loaded"); fallback.classList.remove("hidden"); };
      image.src = banner;
    } else {
      image.removeAttribute("src");
    }
    renderDlcMethod();
  }

  async function loadDlcCatalog(appid, token = state.dlc.requestId) {
    if (!appid) {
      state.dlc.catalog = [];
      state.dlc.loaded = false;
      renderDlcCatalog();
      return;
    }
    state.dlc.catalogLoading = true;
    state.dlc.catalogError = "";
    renderDlcCatalog();
    try {
      const result = await call("scan_dlc_catalog", { appid, steamPath: state.lua.steamPath || null });
      if (token !== state.dlc.requestId) return;
      const previous = new Map(state.dlc.catalog.map((item) => [item.id, item]));
      for (const item of result.entries || []) {
        const old = previous.get(item.id);
        previous.set(item.id, { ...item, enabled: old ? old.enabled : !(dlcInstalled() && !$("#dlc-unlock-all").checked) });
      }
      state.dlc.catalog = [...previous.values()]
        .sort((left, right) => Number(left.id) - Number(right.id));
      state.dlc.loaded = true;
      state.dlc.catalogStatus = (result.warnings || []).join(" ");
      renderDlcCatalog($("#dlc-search").value);
      log(`Каталог DLC загружен: ${state.dlc.catalog.length} дополнений`, state.dlc.catalog.length ? "success" : "info");
    } catch (error) {
      if (token !== state.dlc.requestId) return;
      state.dlc.catalogError = errorText(error);
      state.dlc.loaded = false;
      renderDlcCatalog();
      log(`Каталог DLC не загружен: ${errorText(error)}`, "error");
    } finally {
      if (token === state.dlc.requestId) {
        state.dlc.catalogLoading = false;
        renderDlcCatalog($("#dlc-search").value);
      }
    }
  }

  async function scanDlcDirectory(rawPath, selectedGame = null, operationRefresh = false) {
    const path = String(rawPath || "").trim().replace(/^"|"$/g, "");
    if (!path || state.dlc.scanning || (state.dlc.operating && !operationRefresh)) {
      if (!path) toast("Сначала укажите папку игры", "error");
      return;
    }
    state.dlc.scanning = true;
    const token = ++state.dlc.requestId;
    state.dlc.catalog = [];
    state.dlc.loaded = false;
    state.dlc.catalogError = "";
    state.dlc.catalogStatus = "";
    $("#dlc-search").value = "";
    renderDlcLibrary();
    if (state.dlc.detail) updateDlcSelection();
    $("#dlc-library-status").textContent = "Анализируем выбранную игру…";
    localStorage.setItem("lastDlcPath", path);
    $("#dlc-path").value = path;
    const button = $("#dlc-scan-button");
    setBusy(button, true, "Определяем…");
    log(`DLC Unlocker · определение игры: ${path}`, "system");
    try {
      const result = await call("scan_dlc_target", { gameDir: path });
      if (token !== state.dlc.requestId) return;
      state.dlc.gameDir = result.gameDir;
      state.dlc.appid = result.appid || null;
      state.dlc.name = selectedGame?.name || (result.detected ? result.name : basename(path));
      state.dlc.banner = result.headerImage || "";
      state.dlc.source = result.source || "";
      state.dlc.targets = result.targets || [];
      state.dlc.installed = Boolean(result.installed);
      state.dlc.exes = result.exes || [];
      const currentExe = state.dlc.exes.find((exe) => exe.path === state.dlc.exePath);
      const exe = currentExe || state.dlc.exes.find((exe) => exe.proxy) || state.dlc.exes[0];
      state.dlc.exePath = exe?.path || "";
      state.dlc.proxy = exe?.proxy || state.dlc.proxy;
      state.dlc.configuredDlcs = result.configuredDlcs || [];
      state.dlc.configUnlockAll = Boolean(result.unlockAll);
      if (!currentExe) state.dlc.method = result.installed ? "cream" : state.dlc.exes.some((item) => item.proxy) ? "smaki" : "cream";
      const selector = $("#dlc-exe-select"); selector.replaceChildren();
      for (const item of state.dlc.exes) {
        const option = document.createElement("option"); option.value = item.path;
        option.textContent = `${item.relativePath} · ${item.bitness}${item.recommended ? " ★" : ""}`;
        selector.append(option);
      }
      selector.value = state.dlc.exePath;
      $("#dlc-unlock-all").checked = true;
      restoreDlcSelection();
      $("#dlc-path").value = result.gameDir;
      renderDlcGame();

      log(`Библиотек steam_api найдено: ${state.dlc.targets.length}`, state.dlc.targets.length ? "info" : "error");
      if (!state.dlc.targets.length) toast("В папке не найдена библиотека steam_api", "error");
      if (result.detected) log(`Игра определена: ${state.dlc.name} · AppID ${state.dlc.appid}`, "success");
      else log("Steam AppID не определён автоматически. Используйте ручной поиск.", "error");
      if (state.dlc.installed) log("В игре уже установлен DLC-анлокер", "info");
      await loadDlcCatalog(state.dlc.appid, token);
      if (token !== state.dlc.requestId) return;
      toast("Готово: игра проанализирована");
    } catch (error) {
      if (token !== state.dlc.requestId) return;
      $("#dlc-library-status").textContent = errorText(error);
      log(`Определение не выполнено: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      if (token === state.dlc.requestId) {
        setBusy(button, false);
        state.dlc.scanning = false;
        renderDlcLibrary();
        updateDlcSelection();
      }
    }
  }

  async function chooseDlcDirectory() {
    const button = $("#dlc-browse-button");
    setBusy(button, true, "Открываем…");
    try {
      const path = await call("pick_game_directory");
      if (path) await scanDlcDirectory(path);
    } catch (error) {
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function dlcManualSearch() {
    if (state.dlc.operating || state.dlc.scanning || state.dlc.catalogLoading) return;
    const query = $("#dlc-manual-query").value.trim();
    if (!query) return toast("Введите название игры", "error");
    const button = $("#dlc-manual-button");
    const token = ++state.dlc.requestId;
    setBusy(button, true, "Поиск…");
    log(`DLC Unlocker · поиск в Steam: «${query}»`, "system");
    try {
      const game = await call("search_game", { query });
      if (token !== state.dlc.requestId) return;
      if (!game) throw new Error("Игра не найдена в Steam Store");
      state.dlc.appid = game.appid;
      state.dlc.name = game.name;
      state.dlc.banner = game.headerImage || state.dlc.banner;
      state.dlc.source = `Steam Store · ${query}`;
      state.dlc.catalog = [];
      state.dlc.loaded = false;
      state.dlc.configuredDlcs = [];
      renderDlcGame();
      log(`Выбрана игра: ${game.name} · AppID ${game.appid}`, "success");
      await loadDlcCatalog(game.appid, token);
      if (token !== state.dlc.requestId) return;
      toast("Метаданные игры обновлены");
    } catch (error) {
      if (token !== state.dlc.requestId) return;
      log(`Ошибка поиска: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      if (token === state.dlc.requestId) setBusy(button, false);
    }
  }

  async function installDlcUnlocker() {
    if (state.dlc.operating || state.dlc.scanning || state.dlc.catalogLoading) return;
    if (!state.dlc.gameDir) return toast("Сначала выберите папку игры", "error");
    if (!state.dlc.appid) return toast("Сначала определите Steam AppID (ручной поиск)", "error");
    const smaki = state.dlc.method === "smaki";
    if (smaki && !["x64", "x86"].includes(dlcExe()?.bitness)) return toast("SmokeAPI поддерживает только x64 и x86 EXE", "error");
    if (!smaki && !state.dlc.targets.length) return toast("В папке не найдена библиотека steam_api", "error");
    const unlockAll = $("#dlc-unlock-all").checked;
    const selected = unlockAll ? state.dlc.catalog.slice() : state.dlc.catalog.filter((item) => item.enabled);
    if (!unlockAll && !selected.length) return toast("Выберите хотя бы одно DLC или включите «Разблокировать все»", "error");

    const target = { ...state.dlc, exePath: state.dlc.exePath };
    const button = $("#dlc-install-button");
    state.dlc.operating = true;
    renderDlcMethod();
    setBusy(button, true, "Устанавливаем…");
    log(`Установка DLC Unlocker · AppID ${state.dlc.appid} · ${unlockAll ? "все DLC" : selected.length + " DLC"}`, "system");
    try {
      const accepted = await showModal(
        dlcInstalled() ? "Переустановить DLC Unlocker?" : "Установить DLC Unlocker?",
        smaki ? `SmokeAPI: ${target.proxy}\n${target.exePath.replace(/[^\\/]+$/, target.proxy)}\nБиблиотеки Steam не изменяются. DLC-файлы должны уже присутствовать в игре.` : `CreamAPI: библиотека Steam будет подменена в ${target.targets.length} директориях. Оригиналы сохраняются и восстанавливаются при удалении.`,
        { confirm: true, confirmText: dlcInstalled() ? "Переустановить" : "Установить", symbol: "↓" },
      );
      if (!accepted) return;
      const request = {
        gameDir: target.gameDir,
        appid: Number(target.appid),
        dlcs: Object.fromEntries(selected.map((item) => [item.id, item.name])),
        unlockAll,
        extraProtection: $("#dlc-extra-protection").checked,
      };
      if (smaki) { request.exePath = target.exePath; request.proxy = target.proxy; }
      const result = await call(smaki ? "install_smaki" : "install_dlc_unlocker", { request });
      if (consumeResult(result)) {
        await scanDlcDirectory(target.gameDir, null, true);
        await showModal("Анлокер установлен", smaki ? "Прокси установлен рядом с EXE. Библиотеки Steam не изменялись." : "Библиотека Steam подменена, оригинал сохранён. Запустите игру и проверьте доступ к DLC.", { symbol: "✓" });
      }
    } catch (error) {
      log(`Ошибка установки: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
      state.dlc.operating = false;
      renderDlcMethod();
    }
  }

  async function uninstallDlcUnlocker() {
    if (!state.dlc.gameDir || !dlcInstalled() || state.dlc.operating || state.dlc.scanning || state.dlc.catalogLoading) return;
    const smaki = state.dlc.method === "smaki";
    const target = { gameDir: state.dlc.gameDir, exePath: state.dlc.exePath };
    const button = $("#dlc-uninstall-button");
    state.dlc.operating = true;
    renderDlcMethod();
    setBusy(button, true, "Восстановление…");
    log("Удаление DLC Unlocker и восстановление оригинала", "system");
    try {
      const accepted = await showModal("Удалить DLC Unlocker?", smaki ? "Будут удалены только установленные программой прокси и конфигурация SmokeAPI рядом с выбранным EXE. Steam API не изменяется." : "Оригинальная библиотека Steam будет восстановлена из резервной копии, а cream_api.ini удалён.", { confirm: true, confirmText: "Удалить и восстановить", symbol: "↺" });
      if (!accepted) return;
      const result = await call(smaki ? "uninstall_smaki" : "uninstall_dlc_unlocker", smaki ? target : { gameDir: target.gameDir });
      if (consumeResult(result)) await scanDlcDirectory(target.gameDir, null, true);
    } catch (error) {
      log(`Ошибка удаления: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
      state.dlc.operating = false;
      renderDlcMethod();
    }
  }

  function steamHeaderUrls(appid) {
    return [
      `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/header.jpg`,
      `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/header.jpg`,
      `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${appid}/header.jpg`,
    ];
  }

  function loadCover(cover, img, appid) {
    const urls = steamHeaderUrls(appid);
    let index = 0;
    img.onload = () => cover.classList.add("loaded");
    img.onerror = () => {
      index += 1;
      if (index < urls.length) img.src = urls[index];
      else cover.classList.remove("loaded");
    };
    img.src = urls[0];
  }

  function renderLuaGames() {
    const container = $("#lua-games");
    const empty = $("#lua-empty");
    const games = state.lua.games;
    const renderId = ++luaDrmRenderId;
    luaDrmObserver?.disconnect();
    luaDrmQueue.clear();
    clearTimeout(luaDrmQueueTimer);
    container.replaceChildren();
    container.classList.toggle("hidden", games.length === 0);
    empty.classList.toggle("hidden", games.length > 0);
    if (!games.length) return;

    luaDrmObserver = "IntersectionObserver" in window
      ? new IntersectionObserver((entries) => {
          entries.forEach((entry) => {
            if (!entry.isIntersecting) return;
            luaDrmObserver?.unobserve(entry.target);
            const badge = entry.target.querySelector(".lua-drm-badge");
            if (badge) queueDrmBadge(entry.target.dataset.appid, badge, renderId);
          });
        }, { root: null, rootMargin: "120px 0px" })
      : null;

    games.forEach((game) => {
      const card = document.createElement("div");
      const needsAttention = game.status === "missing_manifests" || game.status === "invalid_lua";
      card.className = `lua-game${game.enabled ? "" : " disabled"}${needsAttention ? " needs-attention" : ""}`;
      card.dataset.appid = String(game.appid);

      const cover = document.createElement("div");
      cover.className = "lua-game-cover";
      const img = document.createElement("img");
      const fallback = document.createElement("span");
      fallback.className = "lua-cover-fallback";
      fallback.textContent = game.appid;
      cover.append(img, fallback);
      loadCover(cover, img, game.appid);
      cover.style.cursor = "pointer";
      cover.title = "Подробнее";
      cover.addEventListener("click", () => luaOpenDetail(game.appid));

      const info = document.createElement("div");
      info.className = "lua-game-info";
      const title = document.createElement("strong");
      title.textContent = state.lua.names[game.appid] || `AppID ${game.appid}`;
      title.title = title.textContent;
      const status = document.createElement("span");
      const statusKey = game.status || (game.enabled ? "ready" : "disabled");
      status.className = `lua-game-status ${statusKey}`;
      if (statusKey === "missing_manifests") {
        status.textContent = `Нет manifest: ${game.missingManifestCount || 0}`;
        status.title = `Установлено: ${game.manifestCount || 0}. Отсутствует: ${game.missingManifestCount || 0}.`;
      } else if (statusKey === "invalid_lua") {
        status.textContent = "Lua повреждён";
        status.title = "Lua-файл пустой, недоступен или не содержит addappid";
      } else if (statusKey === "disabled") {
        status.textContent = "Отключена";
      } else {
        status.textContent = game.manifestCount ? `Готова · ${game.manifestCount} manifest` : "Готова";
      }
      const infoMain = document.createElement("div");
      infoMain.className = "lua-game-info-main";
      infoMain.append(title, status);
      const drmBadge = document.createElement("span");
      drmBadge.className = "lua-drm-badge loading";
      drmBadge.textContent = "Защита…";
      drmBadge.title = "Получаем сведения о защите из Steam";
      info.append(infoMain, drmBadge);

      const actions = document.createElement("div");
      actions.className = "lua-game-actions";
      const details = document.createElement("button");
      details.className = "ghost-button";
      details.textContent = "Подробнее";
      details.addEventListener("click", () => luaOpenDetail(game.appid));
      const toggle = document.createElement("button");
      toggle.className = "ghost-button";
      toggle.textContent = game.enabled ? "Отключить" : "Включить";
      toggle.addEventListener("click", () => luaToggle(game.appid, !game.enabled));
      const remove = document.createElement("button");
      remove.className = "ghost-button danger-ghost";
      remove.textContent = "Удалить";
      remove.addEventListener("click", () => luaRemove(game.appid));
      actions.append(details, toggle, remove);

      card.append(cover, info, actions);
      container.appendChild(card);
      const cachedDrm = luaDrmCache.get(String(game.appid));
      if (cachedDrm) applyDrmBadge(drmBadge, cachedDrm);
      else if (luaDrmObserver) luaDrmObserver.observe(card);
      else queueDrmBadge(game.appid, drmBadge, renderId);
    });
  }

  async function luaLoadNames() {
    const missing = state.lua.games.map((g) => g.appid).filter((id) => !state.lua.names[id]);
    if (!missing.length) return;
    try {
      const names = await call("luatools_names", { appids: missing.map(Number) });
      Object.assign(state.lua.names, names || {});
      renderLuaGames();
    } catch (error) {
      log(`Не удалось получить названия игр: ${errorText(error)}`, "info");
    }
  }

  function luaDetailDate(unix) {
    if (!unix) return "";
    return new Date(Number(unix) * 1000).toLocaleDateString(window.cgsI18n?.locale?.() || "ru-RU", { day: "2-digit", month: "short", year: "numeric" });
  }

  function renderDepotList(container, items) {
    container.replaceChildren();
    if (!items.length) {
      const empty = document.createElement("div");
      empty.className = "empty-inline";
      empty.textContent = "Ничего нет";
      container.appendChild(empty);
      return;
    }
    items.forEach((item) => {
      const row = document.createElement("div");
      row.className = "lua-depot";
      const copy = document.createElement("div");
      copy.className = "lua-depot-copy";
      const name = document.createElement("strong");
      name.textContent = item.name;
      name.title = item.name;
      const id = document.createElement("code");
      id.textContent = item.id;
      copy.append(name, id);
      row.append(copy);
      const badge = document.createElement("span");
      badge.className = "lua-depot-badge";
      badge.textContent = "DLC";
      row.append(badge);
      container.appendChild(row);
    });
  }

  async function luaOpenDetail(appid) {
    const backdrop = $("#lua-detail-backdrop");
    $("#lua-detail-name").textContent = state.lua.names[appid] || `AppID ${appid}`;
    $("#lua-detail-appid").textContent = appid;
    $("#lua-detail-added").textContent = "";
    $("#lua-detail-released").textContent = "";
    $("#lua-detail-fallback").textContent = appid;
    const banner = $("#lua-detail-banner");
    banner.removeAttribute("src");
    banner.parentElement.classList.remove("loaded");
    loadCover(banner.parentElement, banner, appid);
    const loading = () => { const d = document.createElement("div"); d.className = "empty-inline"; d.textContent = "Загрузка из Steam…"; return d; };
    $("#lua-detail-in").replaceChildren(loading());
    $("#lua-detail-missing").replaceChildren();
    $("#lua-detail-in-count").textContent = "(…)";
    $("#lua-detail-missing-count").textContent = "(…)";
    backdrop.classList.remove("hidden");

    try {
      const detail = await call("luatools_game_detail", { appid, steamPath: state.lua.steamPath || null });
      if (detail.name) { $("#lua-detail-name").textContent = detail.name; state.lua.names[appid] = detail.name; }
      $("#lua-detail-added").textContent = detail.addedUnix ? `Добавлено ${luaDetailDate(detail.addedUnix)}` : "";
      $("#lua-detail-released").textContent = detail.releaseDate ? `Вышла ${detail.releaseDate}` : "";
      $("#lua-detail-in-count").textContent = `(${detail.inLua.length})`;
      $("#lua-detail-missing-count").textContent = `(${detail.missing.length})`;
      renderDepotList($("#lua-detail-in"), detail.inLua);
      renderDepotList($("#lua-detail-missing"), detail.missing);
    } catch (error) {
      $("#lua-detail-in-count").textContent = "(0)";
      $("#lua-detail-missing-count").textContent = "(0)";
      toast(errorText(error), "error");
    }
  }

  function renderLuaStatus() {
    const { steamPath, luaFolder, loaderStatus, loaderName, loaderIssue, games } = state.lua;
    $("#lua-count").textContent = games.length.toLocaleString(window.cgsI18n?.locale?.() || "ru-RU");
    $("#lua-steam-path").value = steamPath || "";
    if (loaderIssue) {
      $("#lua-steam-hint").textContent = loaderIssue;
    } else if (steamPath && luaFolder) {
      const relative = luaFolder.toLowerCase().startsWith(steamPath.toLowerCase())
        ? luaFolder.slice(steamPath.length).replace(/^[\\/]+/, "")
        : luaFolder;
      $("#lua-steam-hint").textContent = `${loaderName || "SteamTools"} · Lua: ${relative}`;
    } else {
      $("#lua-steam-hint").textContent = steamPath
        ? `Загрузчик: ${loaderStatus}`
        : "Steam не найден — укажите папку вручную";
    }
    renderMillenniumStatus();
    renderLuaGames();
  }

  function renderMillenniumStatus() {
    const info = state.lua.millennium || {};
    const badge = $("#millennium-plugin-badge");
    const installButton = $("#millennium-install-button");
    const installLabel = installButton.querySelector("span");
    const siteButton = $("#millennium-site-button");
    const removeButton = $("#millennium-remove-button");

    const labels = {
      checking: "Проверка…",
      steam_missing: "Steam не найден",
      steam_invalid: "Неверная папка Steam",
      millennium_missing: "Millennium не установлен",
      plugin_missing: "Плагин не установлен",
      update_available: `Обновление · ${info.bundledVersion || ""}`,
      ready: `Установлен · ${info.pluginVersion || info.bundledVersion || ""}`,
    };
    badge.textContent = labels[info.status] || "Неизвестный статус";
    badge.className = "millennium-plugin-badge";
    if (info.status === "ready") badge.classList.add("ready");
    else if (["plugin_missing", "update_available"].includes(info.status)) badge.classList.add("warning");
    else if (["steam_missing", "steam_invalid", "millennium_missing"].includes(info.status)) badge.classList.add("error");
    else badge.classList.add("checking");

    const canInstall = Boolean(info.millenniumInstalled) && !["checking", "steam_invalid", "steam_missing"].includes(info.status);
    installButton.disabled = !canInstall;
    installButton.classList.toggle("hidden", !["plugin_missing", "update_available"].includes(info.status));
    siteButton.classList.toggle("hidden", info.status !== "millennium_missing");
    removeButton.classList.toggle("hidden", !info.pluginInstalled);
    if (installLabel) {
      installLabel.textContent = info.status === "ready"
        ? "Переустановить"
        : info.status === "update_available"
          ? "Обновить плагин"
          : "Установить плагин";
    }
  }

  async function downloadMillenniumInstaller() {
    const button = $("#millennium-site-button");
    setBusy(button, true, "Устанавливаем…");
    try {
      const result = await call("millennium_install_bundled", { steamPath: state.lua.steamPath || null });
      if (!consumeResult(result)) return;
      await luaRefresh();
      if (!state.lua.millennium?.pluginInstalled) {
        const installPlugin = await showModal(
          "Millennium установлен",
          "Файлы размещены прямо в папке Steam. Установить Crocodile Manifest сейчас? После этого потребуется полностью перезапустить Steam.",
          { confirm: true, confirmText: "Установить плагин", cancelText: "Позже", symbol: "🐊" },
        );
        if (installPlugin) await installMillenniumPlugin();
      }
    } catch (error) {
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
      renderMillenniumStatus();
    }
  }

  async function offerMillenniumInstall() {
    const accepted = await showModal(
      "Для плагина нужен Millennium",
      "CrocLua установит встроенные файлы Millennium напрямую в выбранную папку Steam — без отдельного установщика. После установки потребуется полностью перезапустить Steam.",
      { confirm: true, confirmText: "Установить", cancelText: "Позже", symbol: "M" },
    );
    if (accepted) await downloadMillenniumInstaller();
  }

  async function luaRefresh(options = {}) {
    try {
      const [status, millennium] = await Promise.all([
        call("luatools_status", { steamPath: state.lua.steamPath || null }),
        call("millennium_plugin_status", { steamPath: state.lua.steamPath || null }),
      ]);
      state.lua.steamPath = status.steamPath || state.lua.steamPath || "";
      state.lua.luaFolder = status.luaFolder || "";
      state.lua.loaderStatus = status.loaderStatus || "unknown";
      state.lua.loaderName = status.loaderName || "";
      state.lua.loaderIssue = status.loaderIssue || "";
      state.lua.games = status.games || [];
      state.lua.millennium = millennium || state.lua.millennium;
      state.lua.loaded = true;
      if (state.lua.steamPath) localStorage.setItem("luaSteamPath", state.lua.steamPath);
      renderLuaStatus();
      luaLoadNames();
      if (document.querySelector('#view-lua.active')) window.cgsLoaderDialog?.check({steamPath:state.lua.steamPath || null});
      if (options.offerMillennium !== false && millennium?.status === "millennium_missing" && !state.lua.millenniumPrompted) {
        state.lua.millenniumPrompted = true;
        await offerMillenniumInstall();
      }
    } catch (error) {
      log(`LuaTools: ошибка статуса — ${errorText(error)}`, "error");
    }
  }

  let millenniumStatusRefreshing = false;

  async function refreshMillenniumStatus() {
    if (millenniumStatusRefreshing || !invoke) return;
    millenniumStatusRefreshing = true;
    try {
      const millennium = await call("millennium_plugin_status", { steamPath: state.lua.steamPath || null });
      state.lua.millennium = millennium || state.lua.millennium;
      renderMillenniumStatus();
    } catch (_) {
      // Полный luaRefresh покажет ошибку при следующем ручном обновлении.
    } finally {
      millenniumStatusRefreshing = false;
    }
  }

  let luaLoaderUpdating = false;

  async function luaUpdateDlls() {
    if (luaLoaderUpdating || luaInstalling) return;
    luaLoaderUpdating = true;
    const button = $("#lua-update-dll-button");
    setBusy(button, true, "Выбираем…");
    try {
      const result = await window.cgsLoaderDialog.open({ steamPath: state.lua.steamPath || null });
      if (result && consumeResult(result)) await luaRefresh({ offerMillennium: false });
    } catch (error) {
      toast(errorText(error), "error");
    } finally {
      luaLoaderUpdating = false;
      setBusy(button, false);
      // Keep a Russian source label for live translation after busy HTML is restored.
      if (button) { button.textContent = "Обновить DLL"; button.focus({preventScroll:true}); }
    }
  }

  let luaInstalling = false;

  async function luaInstall(paths) {
    const files = (paths || []).filter(Boolean);
    if (!files.length || luaInstalling || luaLoaderUpdating) return;
    luaInstalling = true;
    $("#lua-drop-zone").classList.add("busy");
    log(`CrocLua: добавление файлов — ${files.length}`, "system");
    try {
      const result = await call("luatools_install", {
        paths: files,
        steamPath: state.lua.steamPath || null,
        autoUpdate: $("#lua-auto-update").checked,
      });
      if (consumeResult(result)) {
        await luaRefresh();
        await showModal("Игра добавлена", "Файлы размещены в папках Steam. Нажмите «Перезапустить Steam», чтобы игра появилась в библиотеке.", { symbol: "✓" });
      }
    } catch (error) {
      log(`Ошибка добавления: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      luaInstalling = false;
      $("#lua-drop-zone").classList.remove("busy");
    }
  }

  async function luaBrowseFiles() {
    const button = $("#lua-browse-button");
    setBusy(button, true, "Открываем…");
    try {
      const files = await call("luatools_pick_files");
      if (files && files.length) await luaInstall(files);
    } catch (error) {
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function luaToggle(appid, enable) {
    try {
      const result = await call("luatools_toggle", { appid, enable, steamPath: state.lua.steamPath || null });
      if (consumeResult(result)) await luaRefresh();
    } catch (error) {
      toast(errorText(error), "error");
    }
  }

  async function luaRemove(appid) {
    const accepted = await showModal("Удалить игру из загрузчика?", `Файл lua для AppID ${appid} будет удалён. После этого перезапустите Steam.`, { confirm: true, confirmText: "Удалить", symbol: "↺" });
    if (!accepted) return;
    try {
      const result = await call("luatools_remove", { appid, steamPath: state.lua.steamPath || null });
      if (consumeResult(result)) await luaRefresh();
    } catch (error) {
      toast(errorText(error), "error");
    }
  }

  async function luaRestartSteam() {
    const accepted = await showModal("Перезапустить Steam?", "Клиент Steam будет закрыт и запущен заново, чтобы применить изменения. Незавершённые загрузки могут прерваться.", { confirm: true, confirmText: "Перезапустить", symbol: "↻" });
    if (!accepted) return;
    const button = $("#lua-restart-button");
    setBusy(button, true, "Перезапуск…");
    try {
      const result = await call("luatools_restart_steam", { steamPath: state.lua.steamPath || null });
      consumeResult(result);
    } catch (error) {
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function installMillenniumPlugin() {
    if (!state.lua.millennium?.millenniumInstalled) {
      await offerMillenniumInstall();
      return;
    }
    const button = $("#millennium-install-button");
    setBusy(button, true, state.lua.millennium.pluginInstalled ? "Обновляем…" : "Устанавливаем…");
    try {
      const result = await call("millennium_install_plugin", { steamPath: state.lua.steamPath || null });
      if (!consumeResult(result)) return;
      await luaRefresh();
      const restart = await showModal(
        "Crocodile Manifest установлен",
        "Плагин размещён в папке Millennium. Перезапустить Steam сейчас, чтобы он появился в настройках плагинов?",
        { confirm: true, confirmText: "Перезапустить Steam", cancelText: "Позже", symbol: "🐊" },
      );
      if (restart) {
        const restartResult = await call("luatools_restart_steam", { steamPath: state.lua.steamPath || null });
        consumeResult(restartResult);
      }
    } catch (error) {
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
      renderMillenniumStatus();
    }
  }

  async function removeMillenniumPlugin() {
    const accepted = await showModal(
      "Удалить Crocodile Manifest?",
      "Папка плагина будет полностью удалена из Millennium. Сохранённая история операций останется на компьютере и будет доступна после повторной установки.",
      { confirm: true, confirmText: "Удалить плагин", cancelText: "Отмена", symbol: "↺" },
    );
    if (!accepted) return;

    const button = $("#millennium-remove-button");
    setBusy(button, true, "Удаляем…");
    try {
      const result = await call("millennium_remove_plugin", { steamPath: state.lua.steamPath || null });
      if (!consumeResult(result)) return;
      await luaRefresh();
      const restart = await showModal(
        "Crocodile Manifest удалён",
        "Перезапустить Steam сейчас, чтобы полностью выгрузить плагин?",
        { confirm: true, confirmText: "Перезапустить Steam", cancelText: "Позже", symbol: "✓" },
      );
      if (restart) {
        const restartResult = await call("luatools_restart_steam", { steamPath: state.lua.steamPath || null });
        consumeResult(restartResult);
      }
    } catch (error) {
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
      renderMillenniumStatus();
    }
  }

  let luaSearchTimer = null;
  let hubcapKeyTimer = null;
  let luaDrmRenderId = 0;
  let luaDrmObserver = null;
  let luaDrmQueueTimer = null;
  const luaDrmCache = new Map();
  const luaDrmQueue = new Map();

  function applyDrmBadge(badge, info) {
    const allowedKinds = new Set(["denuvo", "launcher", "steam", "third-party", "unknown"]);
    const kind = allowedKinds.has(info?.kind) ? info.kind : "unknown";
    badge.className = `lua-drm-badge ${kind}`;
    badge.textContent = info?.label || "Не указано";
    badge.title = info?.notice || "Steam не указывает сведения о защите для этой игры.";
  }

  async function flushDrmQueue() {
    luaDrmQueueTimer = null;
    const entries = [...luaDrmQueue.values()];
    luaDrmQueue.clear();
    const active = entries.filter((entry) => entry.badge.isConnected);
    if (!active.length) return;
    const appids = [...new Set(active.map((entry) => entry.appid))];
    try {
      const batches = [];
      for (let index = 0; index < appids.length; index += 12) {
        batches.push(appids.slice(index, index + 12));
      }
      const infos = (await Promise.all(
        batches.map((batch) => call("steam_drm_info", { appids: batch }))
      )).flat();
      const byAppid = new Map();
      (infos || []).forEach((info) => {
        const appid = String(info.appid);
        luaDrmCache.set(appid, info);
        byAppid.set(appid, info);
      });
      active.forEach((entry) => {
        if (!entry.badge.isConnected || entry.renderId !== luaDrmRenderId) return;
        applyDrmBadge(entry.badge, byAppid.get(entry.appid) || {
          label: "Нет данных",
          kind: "unknown",
          notice: "Не удалось получить сведения о защите из Steam. Они появятся при следующем обновлении списка.",
        });
      });
    } catch {
      active.forEach((entry) => {
        if (!entry.badge.isConnected || entry.renderId !== luaDrmRenderId) return;
        applyDrmBadge(entry.badge, {
          label: "Нет данных",
          kind: "unknown",
          notice: "Не удалось получить сведения о защите из Steam. Они появятся при следующем обновлении списка.",
        });
      });
    }
  }

  function queueDrmBadge(appid, badge, renderId) {
    const normalized = String(appid);
    const cached = luaDrmCache.get(normalized);
    if (cached) {
      applyDrmBadge(badge, cached);
      return;
    }
    luaDrmQueue.set(normalized, { appid: normalized, badge, renderId });
    clearTimeout(luaDrmQueueTimer);
    luaDrmQueueTimer = setTimeout(flushDrmQueue, 90);
  }

  function renderSearchDropdown(results) {
    const dropdown = $("#lua-search-dropdown");
    dropdown.replaceChildren();
    if (!results.length) {
      const empty = document.createElement("div");
      empty.className = "lua-search-empty";
      empty.textContent = "Ничего не найдено";
      dropdown.append(empty);
      dropdown.classList.remove("hidden");
      return;
    }
    results.forEach((game) => {
      const row = document.createElement("button");
      row.className = "lua-search-item";
      row.type = "button";
      const img = document.createElement("img");
      img.loading = "lazy";
      img.src = game.headerImage || steamHeaderUrls(game.appid)[0];
      img.onerror = () => { img.style.visibility = "hidden"; };
      const copy = document.createElement("span");
      copy.className = "lua-search-copy";
      const name = document.createElement("strong");
      name.textContent = game.name;
      name.title = game.name;
      const id = document.createElement("code");
      id.textContent = `AppID ${game.appid}`;
      copy.append(name, id);
      const get = document.createElement("span");
      get.className = "lua-search-get";
      get.textContent = "Получить";
      row.append(img, copy, get);
      row.addEventListener("click", () => luaGet(game.appid, game.name));
      dropdown.append(row);
    });
    dropdown.classList.remove("hidden");
  }

  function luaDropdownMessage(text) {
    const dropdown = $("#lua-search-dropdown");
    dropdown.replaceChildren();
    const item = document.createElement("div");
    item.className = "lua-search-empty";
    item.textContent = text;
    dropdown.append(item);
    dropdown.classList.remove("hidden");
  }

  async function luaSearch(query) {
    const dropdown = $("#lua-search-dropdown");
    const q = String(query || "").trim();
    const hubcap = state.lua.service === "hubcap";
    const minLen = hubcap ? 3 : 2;
    if (q.length < minLen) {
      dropdown.classList.add("hidden");
      dropdown.replaceChildren();
      return;
    }
    if (hubcap && !state.lua.hubcapKey.trim()) {
      luaDropdownMessage("Укажите API-ключ HubCap в настройках");
      return;
    }
    $("#lua-search-spin").classList.remove("hidden");
    try {
      const results = hubcap
        ? await call("hubcap_search", { query: q, apiKey: state.lua.hubcapKey })
        : await call("ryuu_search", { query: q });
      renderSearchDropdown(results || []);
    } catch (error) {
      dropdown.replaceChildren();
      const err = document.createElement("div");
      err.className = "lua-search-empty";
      err.textContent = errorText(error);
      dropdown.append(err);
      dropdown.classList.remove("hidden");
    } finally {
      $("#lua-search-spin").classList.add("hidden");
    }
  }

  async function luaGet(appid, name) {
    $("#lua-search-dropdown").classList.add("hidden");
    const label = name || `AppID ${appid}`;
    const hubcap = state.lua.service === "hubcap";
    log(`Получение из ${hubcap ? "HubCap" : "генератора"}: ${label} (${appid})`, "system");
    toast("Загрузка манифестов…");
    try {
      const result = hubcap
        ? await call("hubcap_get", {
            appid: String(appid),
            apiKey: state.lua.hubcapKey,
            steamPath: state.lua.steamPath || null,
            autoUpdate: $("#lua-auto-update").checked,
          })
        : await call("ryuu_get", {
            appid: String(appid),
            steamPath: state.lua.steamPath || null,
            autoUpdate: $("#lua-auto-update").checked,
          });
      if (consumeResult(result)) {
        if (!state.lua.names[appid] && name) state.lua.names[appid] = name;
        await luaRefresh();
        await showModal("Игра получена", `${label} добавлена. Нажмите «Перезапустить Steam», чтобы она появилась в библиотеке.`, { symbol: "✓" });
      }
    } catch (error) {
      log(`Ошибка получения: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      if (hubcap) refreshHubcapUsage();
    }
  }

  function renderDlcs(filter = "") {
    const container = $("#dlc-list");
    const entries = Object.entries(state.dlcs || {});
    $("#dlc-count").textContent = pluralPackages(entries.length);
    container.replaceChildren();
    const normalized = filter.trim().toLowerCase();
    const visible = entries.filter(([id, name]) => !normalized || id.includes(normalized) || String(name).toLowerCase().includes(normalized));
    if (!visible.length) {
      const empty = document.createElement("div");
      empty.className = "empty-inline";
      empty.textContent = entries.length ? "Ничего не найдено" : "У этой игры DLC не обнаружены";
      container.appendChild(empty);
      return;
    }
    visible.forEach(([id, name]) => {
      const chip = document.createElement("div");
      chip.className = "dlc-chip";
      const code = document.createElement("b");
      code.textContent = id;
      chip.append(code, document.createTextNode(name));
      chip.title = `${id} · ${name}`;
      container.appendChild(chip);
    });
  }

  function renderGame() {
    $("#game-workspace").classList.remove("hidden");
    $("#game-title").textContent = state.gameName || basename(state.gameDir);
    $("#game-appid").textContent = state.appid || "Нужно определить";
    $("#game-engine").textContent = state.engine || "Другой";
    $("#game-exe").textContent = state.exes.length > 1 ? `${state.exes.length} файлов` : (state.exes[0] || "Не найден");
    $("#game-exe").title = state.exes.join("\n");
    $("#game-steam").textContent = state.steamApis.length > 1 ? `${state.steamApis.length} библиотек` : (state.steamApis[0] || "Не найден");
    $("#game-steam").title = state.steamApis.join("\n");

    const status = $("#game-status");
    status.classList.toggle("installed", state.status !== "clean");
    status.textContent = state.status === "installed" ? "FIX INSTALLED" : state.status === "backup_exists" ? "BACKUP READY" : "CLEAN BUILD";
    $("#uninstall-button").classList.toggle("hidden", state.status === "clean");

    const image = $("#game-banner");
    const fallback = $("#cover-fallback");
    image.classList.remove("loaded");
    fallback.classList.remove("hidden");
    if (state.banner) {
      image.onload = () => { image.classList.add("loaded"); fallback.classList.add("hidden"); };
      image.onerror = () => { image.classList.remove("loaded"); fallback.classList.remove("hidden"); };
      image.src = state.banner;
    } else {
      image.removeAttribute("src");
    }

    const eosAvailable = state.hasEos || state.hasEpicfix;
    const canInstallEpicfix = state.hasEos && !state.hasEpicfix;
    const canRestoreEos = state.hasEpicfix && state.hasEosBackup;
    const eosStatus = $("#eos-status");
    eosStatus.textContent = !eosAvailable
      ? "EOS не обнаружен"
      : canRestoreEos
        ? "EpicFix установлен · можно восстановить EOS"
        : state.hasEpicfix
          ? "Оригинальный EOS активен"
          : "EOS обнаружен";
    eosStatus.classList.toggle("offline", !eosAvailable);
    $("#epicfix-button").disabled = !canInstallEpicfix;
    $("#restore-eos-button").disabled = !canRestoreEos;
    renderDlcs();
  }

  async function chooseDirectory() {
    const button = $("#browse-button");
    setBusy(button, true, "Открываем…");
    try {
      const path = await call("pick_game_directory");
      if (path) {
        $("#game-path").value = path;
        localStorage.setItem("lastGamePath", path);
        await scanDirectory(path);
      }
    } catch (error) {
      log(`Ошибка выбора папки: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function scanDirectory(rawPath) {
    const path = String(rawPath || "").trim().replace(/^"|"$/g, "");
    if (!path || state.scanning) {
      if (!path) toast("Сначала укажите папку игры", "error");
      return;
    }
    state.scanning = true;
    localStorage.setItem("lastGamePath", path);
    $("#game-path").value = path;
    $("#scan-progress").classList.remove("hidden");
    $("#game-workspace").classList.add("hidden");
    const scanButton = $("#scan-button");
    setBusy(scanButton, true, "Анализ…");
    log(`Запущено сканирование: ${path}`, "system");
    const messages = ["Ищем исполняемые файлы и сетевые библиотеки…", "Определяем игровой движок…", "Запрашиваем метаданные Steam и DLC…"];
    let index = 0;
    const ticker = setInterval(() => { index = (index + 1) % messages.length; $("#scan-progress-text").textContent = messages[index]; }, 1800);

    try {
      const result = await call("scan_game_directory", { path });
      state.gameDir = result.gameDir;
      state.exes = result.exes || [];
      state.steamApis = result.steamApiPaths || [];
      state.engine = result.engine;
      state.status = result.status;
      state.hasEos = Boolean(result.hasEos);
      state.hasEosBackup = Boolean(result.hasEosBackup);
      state.hasEpicfix = Boolean(result.hasEpicfix);
      const detected = result.detectedGame || {};
      state.appid = detected.detected ? detected.appid : null;
      state.gameName = detected.detected ? detected.name : basename(path);
      state.banner = detected.headerImage || "";
      state.dlcs = detected.dlcs || {};
      renderGame();
      log(`Найдено EXE: ${state.exes.length}; Steam API: ${state.steamApis.length}`, "info");
      if (detected.detected) log(`Игра определена: ${state.gameName} · AppID ${state.appid}`, "success");
      else log("Steam AppID не определён. Используйте ручной поиск.", "error");
      if (state.hasEos) log("Обнаружена библиотека Epic Online Services", "info");
      toast("Сканирование завершено");
    } catch (error) {
      const message = errorText(error);
      log(`Сканирование не выполнено: ${message}`, "error");
      toast(message, "error");
    } finally {
      clearInterval(ticker);
      $("#scan-progress").classList.add("hidden");
      setBusy(scanButton, false);
      state.scanning = false;
    }
  }

  async function manualSearch() {
    const query = $("#manual-query").value.trim();
    if (!query) return toast("Введите название игры", "error");
    const button = $("#manual-search-button");
    setBusy(button, true, "Поиск…");
    log(`Ручной поиск в Steam: «${query}»`, "system");
    try {
      const game = await call("search_game", { query });
      if (!game) throw new Error("Игра не найдена в Steam Store");
      const details = await call("get_app_details", { appid: game.appid });
      state.appid = game.appid;
      state.gameName = game.name;
      state.banner = details?.headerImage || game.headerImage || "";
      state.dlcs = details?.dlcs || {};
      renderGame();
      log(`Выбрана игра: ${game.name} · AppID ${game.appid}`, "success");
      toast("Метаданные игры обновлены");
    } catch (error) {
      log(`Ошибка поиска: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  function consumeResult(result) {
    (result.logs || []).forEach((message) => log(message, String(message).startsWith("⚠") ? "error" : "info"));
    log(result.message, result.success ? "success" : "error");
    toast(result.message, result.success ? "success" : "error");
    return result.success;
  }

  async function installFix() {
    if (!state.gameDir) return toast("Сначала просканируйте папку игры", "error");
    if (!state.appid) return toast("Сначала определите Steam AppID через ручной поиск", "error");
    const button = $("#install-button");
    setBusy(button, true, "Устанавливаем…");
    log(`Установка Online Fix для AppID ${state.appid}`, "system");
    try {
      const request = {
        gameDir: state.gameDir,
        realAppid: Number(state.appid),
        fakeAppid: Number($("#fake-appid").value) || 480,
        dlcs: state.dlcs,
        installEpicfix: false,
        unlockAllDlcs: $("#unlock-dlc").checked,
      };
      const result = await call("install_fix", { request });
      if (consumeResult(result)) {
        state.status = "installed";
        if (state.hasEos) state.hasEosBackup = true;
        renderGame();
        await showModal("Установка завершена", "Компоненты размещены рядом с исполняемым файлом игры. Оригинальные файлы сохранены автоматически.", { symbol: "✓" });
      }
    } catch (error) {
      log(`Ошибка установки: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function uninstallFix() {
    if (!state.gameDir) return;
    const accepted = await showModal("Удалить Online Fix?", "Компоненты фикса будут удалены, а сохранённые оригинальные файлы — возвращены на свои места.", { confirm: true, confirmText: "Удалить и восстановить", symbol: "↺" });
    if (!accepted) return;
    const button = $("#uninstall-button");
    setBusy(button, true, "Восстановление…");
    log("Запущено удаление фикса и восстановление оригиналов", "system");
    try {
      const result = await call("uninstall_fix", { gameDir: state.gameDir });
      if (consumeResult(result)) {
        state.status = "clean";
        state.hasEosBackup = false;
        state.hasEpicfix = false;
        renderGame();
      }
    } catch (error) {
      log(`Ошибка удаления: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function installEpicfix() {
    if (!state.gameDir || !state.hasEos || state.hasEpicfix) return;
    const accepted = await showModal(
      "Установить EpicFix?",
      "Используйте этот шаг, если сразу после установки Online Fix игра не запускается или не создаётся лобби. После установки EpicFix запустите игру и снова проверьте подключение. Если проблема останется, нажмите восстановление оригинального EOS - обычно это решает проблему.",
      { confirm: true, confirmText: "Установить", cancelText: "Не устанавливать", symbol: "!" },
    );
    if (!accepted) return;
    const button = $("#epicfix-button");
    setBusy(button, true, "Установка…");
    log("Отдельная установка компонентов EpicFix", "system");
    try {
      const result = await call("install_epicfix_only", { gameDir: state.gameDir });
      if (consumeResult(result)) {
        state.hasEpicfix = true;
        renderGame();
      }
    } catch (error) {
      log(`Ошибка EpicFix: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  async function restoreEos() {
    if (!state.gameDir || !state.hasEpicfix || !state.hasEosBackup) return;
    const accepted = await showModal("Восстановить оригинальный EOS?", "Если после установки EpicFix игра всё ещё не запускается или лобби не создаётся, восстановите оригинальную библиотеку EOS. В большинстве случаев после этого игра начинает работать корректно.", { confirm: true, confirmText: "Восстановить", cancelText: "Не восстанавливать", symbol: "↺" });
    if (!accepted) return;
    const button = $("#restore-eos-button");
    setBusy(button, true, "Восстановление…");
    try {
      const result = await call("restore_eos_only", { gameDir: state.gameDir });
      if (consumeResult(result)) {
        state.hasEosBackup = false;
        renderGame();
      }
    } catch (error) {
      log(`Ошибка восстановления EOS: ${errorText(error)}`, "error");
      toast(errorText(error), "error");
    } finally {
      setBusy(button, false);
    }
  }

  function applyAccent(name) {
    const values = {
      mint: ["#63f5c2", "99, 245, 194"],
      violet: ["#a987ff", "169, 135, 255"],
      blue: ["#5ca8ff", "92, 168, 255"],
      rose: ["#ff739d", "255, 115, 157"],
    };
    const accent = values[name] || values.mint;
    document.documentElement.style.setProperty("--accent", accent[0]);
    document.documentElement.style.setProperty("--accent-rgb", accent[1]);
    $$(".accent").forEach((button) => button.classList.toggle("active", button.dataset.accent === name));
    localStorage.setItem("accent", name);
  }

  function applyManifestService(service) {
    const svc = service === "hubcap" ? "hubcap" : "ryuu";
    state.lua.service = svc;
    $$(".svc-option").forEach((button) => button.classList.toggle("active", button.dataset.service === svc));
    $("#svc-key-block").classList.toggle("hidden", svc !== "hubcap");
    $("#hubcap-limit-badge").classList.toggle("hidden", svc !== "hubcap");
    localStorage.setItem("manifestService", svc);
    const input = $("#lua-search-input");
    if (input) input.placeholder = svc === "hubcap"
      ? "Поиск через HubCap — название или AppID…"
      : "Введите название игры или AppID…";
    renderHubcapUsage();
    if (svc === "hubcap" && state.lua.hubcapKey.trim()) refreshHubcapUsage();
  }

  function updateHubcapKeyStatus() {
    const status = $("#hubcap-key-status");
    if (status) status.textContent = state.lua.hubcapKey.trim() ? "Ключ сохранён" : "Ключ не указан";
  }

  function renderHubcapUsage() {
    const usage = state.lua.hubcapUsage || { dailyUsage: 0, dailyLimit: 25, remaining: 25 };
    const unlimited = usage.dailyLimit >= 999999;
    const limit = unlimited ? "∞" : usage.dailyLimit;
    const remaining = unlimited ? "∞" : usage.remaining;
    const badge = $("#hubcap-limit-badge");
    const status = $("#hubcap-usage-status");
    badge.textContent = state.lua.hubcapUsageError
      ? "HubCap: лимит недоступен"
      : `HubCap: ${usage.dailyUsage}/${limit} · осталось ${remaining}`;
    badge.classList.toggle("error", Boolean(state.lua.hubcapUsageError));
    $("#hubcap-usage-used").textContent = `${usage.dailyUsage}/${limit}`;
    $("#hubcap-usage-remaining").textContent = String(remaining);
    status.classList.toggle("error", Boolean(state.lua.hubcapUsageError));
    status.textContent = state.lua.hubcapUsageError
      || (state.lua.hubcapUsageLoading
        ? "Обновление данных HubCap…"
        : state.lua.hubcapKey.trim()
          ? "Данные HubCap · обновляются каждые 30 секунд"
          : "Добавьте ключ для синхронизации");
  }

  async function refreshHubcapUsage() {
    if (!invoke || state.lua.hubcapUsageLoading) return;
    if (!state.lua.hubcapKey.trim()) {
      state.lua.hubcapUsage = { dailyUsage: 0, dailyLimit: 25, remaining: 25 };
      state.lua.hubcapUsageError = "";
      renderHubcapUsage();
      return;
    }
    state.lua.hubcapUsageLoading = true;
    state.lua.hubcapUsageError = "";
    renderHubcapUsage();
    try {
      state.lua.hubcapUsage = await call("hubcap_usage", { apiKey: state.lua.hubcapKey });
    } catch (error) {
      state.lua.hubcapUsageError = errorText(error);
    } finally {
      state.lua.hubcapUsageLoading = false;
      renderHubcapUsage();
    }
  }

  function renderUpdateDownloads(update = null) {
    state.updates.latest = update;
    const actions = $("#update-download-actions");
    const portable = $("#download-portable-button");
    const installer = $("#download-installer-button");
    portable.classList.toggle("hidden", !update?.portableUrl);
    installer.classList.toggle("hidden", !update?.installerUrl);
    actions.classList.toggle("hidden", !update?.portableUrl && !update?.installerUrl);
  }

  async function openUpdateDownload(kind) {
    const update = state.updates.latest;
    if (!update) return;
    const url = kind === "portable" ? update.portableUrl : update.installerUrl;
    await call("open_external", { url: url || update.releaseUrl });
  }

  async function checkForUpdates(interactive = false) {
    if (state.updates.checking || !invoke) return;
    state.updates.checking = true;
    const button = $("#check-update-button");
    const status = $("#update-status");
    setBusy(button, true, "Проверяем…");
    status.classList.remove("available", "error");
    status.textContent = "Подключение к GitHub…";

    try {
      const update = await call("check_for_update");
      $("#update-current-version").textContent = update.currentVersion;
      if (!update.available) {
        renderUpdateDownloads();
        status.textContent = `Установлена актуальная версия ${update.currentVersion}`;
        if (interactive) toast("У вас установлена последняя версия");
        return;
      }

      renderUpdateDownloads(update);
      status.textContent = `Доступна версия ${update.latestVersion}`;
      status.classList.add("available");
      if (!interactive && state.updates.promptedVersion === update.latestVersion) return;
      state.updates.promptedVersion = update.latestVersion;

      const notes = String(update.notes || "").trim();
      const shortNotes = notes.length > 700 ? `${notes.slice(0, 700).trim()}…` : notes;
      const preferredUrl = update.portableUrl || update.installerUrl || update.releaseUrl;
      const downloadLabel = update.portableUrl ? "Скачать EXE" : update.installerUrl ? "Скачать Setup" : "Открыть релиз";
      const formatsHint = update.portableUrl && update.installerUrl
        ? "\n\nВ настройках доступны два варианта: прямой EXE и установщик Setup."
        : "";
      const accepted = await showModal(
        `Доступна версия ${update.latestVersion}`,
        `${shortNotes || "Опубликована новая версия Crocodile Gena Studio."}${formatsHint}`,
        { confirm: true, confirmText: downloadLabel, cancelText: "Позже", symbol: "↑" },
      );
      if (accepted) {
        await call("open_external", { url: preferredUrl });
      }
    } catch (error) {
      const message = errorText(error);
      status.textContent = message;
      status.classList.add("error");
      log(`Проверка обновлений: ${message}`, "error");
      if (interactive) toast(message, "error");
    } finally {
      state.updates.checking = false;
      setBusy(button, false);
    }
  }

  function bindEvents() {
    document.addEventListener("contextmenu", (event) => event.preventDefault(), { capture: true });
    document.addEventListener("auxclick", (event) => {
      if (event.button === 2) event.preventDefault();
    }, { capture: true });

    const titlebar = $(".titlebar");
    titlebar.addEventListener("mousedown", (event) => {
      if (event.button !== 0 || event.target.closest(".window-actions")) return;
      call("start_dragging_window").catch(() => {});
    });
    titlebar.addEventListener("dblclick", (event) => {
      if (event.target.closest(".window-actions")) return;
      call("toggle_maximize_window").catch(() => {});
    });

    $$(".nav-item").forEach((item) => item.addEventListener("click", () => switchView(item.dataset.view)));
    $$('[data-open-view]').forEach((item) => item.addEventListener("click", () => switchView(item.dataset.openView)));
    $$("[data-module-toggle]").forEach((input) => input.addEventListener("change", () => {
      setModuleVisible(input.dataset.moduleToggle, input.checked);
    }));
    $("#show-all-modules").addEventListener("click", () => {
      hiddenModules.clear();
      localStorage.setItem("cgs-hidden-modules", "[]");
      applyModuleVisibility();
    });
    $("#browse-button").addEventListener("click", chooseDirectory);
    $("#drop-zone").addEventListener("dblclick", chooseDirectory);
    $("#scan-button").addEventListener("click", () => scanDirectory($("#game-path").value));
    $("#rescan-button").addEventListener("click", () => scanDirectory(state.gameDir));
    $("#game-path").addEventListener("keydown", (event) => { if (event.key === "Enter") scanDirectory(event.currentTarget.value); });
    $("#manual-query").addEventListener("keydown", (event) => { if (event.key === "Enter") manualSearch(); });
    $("#manual-search-button").addEventListener("click", manualSearch);
    $("#dlc-filter").addEventListener("input", (event) => renderDlcs(event.currentTarget.value));
    $("#install-button").addEventListener("click", installFix);
    $("#uninstall-button").addEventListener("click", uninstallFix);
    $("#epicfix-button").addEventListener("click", installEpicfix);
    $("#restore-eos-button").addEventListener("click", restoreEos);
    $("#scan-saves-button").addEventListener("click", scanSaveLocations);
    $("#save-toggle-selection").addEventListener("click", () => {
      const checkboxes = $$("#save-list input[type='checkbox']");
      const shouldSelect = checkboxes.some((item) => !item.checked);
      checkboxes.forEach((item) => { item.checked = shouldSelect; });
      updateSaveSelection();
    });
    $("#create-save-archive").addEventListener("click", createSaveArchive);
    $("#restore-save-archive").addEventListener("click", restoreSaveArchive);

    $("#dlc-browse-button").addEventListener("click", chooseDlcDirectory);
    $("#dlc-library-scan").addEventListener("click", scanDlcLibrary);
    $("#dlc-library-search").addEventListener("input", renderDlcLibrary);
    $("#dlc-library-folder").addEventListener("click", async () => {
      if (state.dlc.libraryScanning || state.dlc.operating) return;
      try {
        const path = await call("pick_game_directory");
        if (!path) return;
        state.dlc.extraLibrary = path;
        // Validate and scan before persisting, so an incorrect folder does not
        // permanently break the next scan.
        const result = await call("scan_installed_steam_games", { steamPath: state.lua.steamPath || null, libraryPath: path });
        state.dlc.games = result.games || []; state.dlc.libraryScanned = true;
        localStorage.setItem("dlcExtraLibrary", path);
        $("#dlc-library-status").textContent = `Установлено игр: ${state.dlc.games.length} · Библиотек: ${result.libraries.length}`;
        renderDlcLibrary();
      } catch (error) { state.dlc.extraLibrary = localStorage.getItem("dlcExtraLibrary") || ""; toast(errorText(error), "error"); }
    });
    $("#dlc-back").addEventListener("click", () => {
      if (state.dlc.operating) return;
      ++state.dlc.requestId;
      state.dlc.scanning = false; state.dlc.catalogLoading = false;
      setBusy($("#dlc-scan-button"), false);
      dlcShowDetail(false); renderDlcLibrary();
      $("#dlc-library-status").textContent = `Установлено игр: ${state.dlc.games.length}`;
    });
    $$("[data-dlc-method]").forEach((button) => button.addEventListener("click", () => {
      if (state.dlc.operating) return;
      state.dlc.method = button.dataset.dlcMethod;
      restoreDlcSelection(); renderDlcMethod(); renderDlcCatalog($("#dlc-search").value);
    }));
    $$("[data-dlc-proxy]").forEach((button) => button.addEventListener("click", () => {
      if (state.dlc.operating) return;
      state.dlc.proxy = button.dataset.dlcProxy; renderDlcMethod();
    }));
    $("#dlc-exe-select").addEventListener("change", (event) => {
      if (state.dlc.operating) return;
      state.dlc.exePath = event.currentTarget.value;
      if (dlcExe()?.proxy) state.dlc.proxy = dlcExe().proxy;
      restoreDlcSelection(); renderDlcMethod(); renderDlcCatalog($("#dlc-search").value);
    });
    $("#dlc-catalog-retry").addEventListener("click", () => {
      if (!state.dlc.operating && !state.dlc.scanning && !state.dlc.catalogLoading) loadDlcCatalog(state.dlc.appid);
    });
    $("#dlc-drop-zone").addEventListener("dblclick", chooseDlcDirectory);
    $("#dlc-scan-button").addEventListener("click", () => scanDlcDirectory($("#dlc-path").value));
    $("#dlc-rescan-button").addEventListener("click", () => scanDlcDirectory(state.dlc.gameDir));
    $("#dlc-path").addEventListener("keydown", (event) => { if (event.key === "Enter") scanDlcDirectory(event.currentTarget.value); });
    $("#dlc-manual-query").addEventListener("keydown", (event) => { if (event.key === "Enter") dlcManualSearch(); });
    $("#dlc-manual-button").addEventListener("click", dlcManualSearch);
    $("#dlc-search").addEventListener("input", (event) => renderDlcCatalog(event.currentTarget.value));
    $("#dlc-unlock-all").addEventListener("change", updateDlcSelection);
    $("#dlc-toggle-selection").addEventListener("click", () => {
      const shouldSelect = state.dlc.catalog.some((item) => !item.enabled);
      state.dlc.catalog.forEach((item) => { item.enabled = shouldSelect; });
      renderDlcCatalog($("#dlc-search").value);
    });
    $("#dlc-install-button").addEventListener("click", installDlcUnlocker);
    $("#dlc-uninstall-button").addEventListener("click", uninstallDlcUnlocker);

    $("#lua-browse-button").addEventListener("click", luaBrowseFiles);
    $("#lua-drop-zone").addEventListener("dblclick", luaBrowseFiles);
    $("#lua-refresh-button").addEventListener("click", luaRefresh);
    $("#lua-update-dll-button").addEventListener("click", luaUpdateDlls);
    $("#lua-restart-button").addEventListener("click", luaRestartSteam);
    $("#millennium-site-button").addEventListener("click", downloadMillenniumInstaller);
    $("#millennium-install-button").addEventListener("click", installMillenniumPlugin);
    $("#millennium-remove-button").addEventListener("click", removeMillenniumPlugin);
    $("#lua-steam-browse").addEventListener("click", async () => {
      try {
        const path = await call("pick_game_directory");
        if (path) { state.lua.steamPath = path; localStorage.setItem("luaSteamPath", path); await luaRefresh(); }
      } catch (error) { toast(errorText(error), "error"); }
    });
    $("#lua-steam-path").addEventListener("change", (event) => {
      state.lua.steamPath = event.currentTarget.value.trim();
      localStorage.setItem("luaSteamPath", state.lua.steamPath);
      luaRefresh();
    });
    const luaDrop = $("#lua-drop-zone");
    ["dragenter", "dragover"].forEach((name) => luaDrop.addEventListener(name, (event) => { event.preventDefault(); luaDrop.classList.add("dragover"); }));
    ["dragleave", "drop"].forEach((name) => luaDrop.addEventListener(name, (event) => { event.preventDefault(); luaDrop.classList.remove("dragover"); }));
    luaDrop.addEventListener("drop", (event) => {
      const files = [...(event.dataTransfer?.files || [])].map((file) => file.path).filter(Boolean);
      handleFileDrop(files);
    });
    const luaDetail = $("#lua-detail-backdrop");
    $("#lua-detail-close").addEventListener("click", () => luaDetail.classList.add("hidden"));
    luaDetail.addEventListener("click", (event) => { if (event.target === luaDetail) luaDetail.classList.add("hidden"); });

    const luaSearchInput = $("#lua-search-input");
    luaSearchInput.addEventListener("input", (event) => {
      clearTimeout(luaSearchTimer);
      const value = event.currentTarget.value;
      luaSearchTimer = setTimeout(() => luaSearch(value), 280);
    });
    luaSearchInput.addEventListener("focus", () => {
      if ($("#lua-search-dropdown").childElementCount) $("#lua-search-dropdown").classList.remove("hidden");
    });
    document.addEventListener("click", (event) => {
      if (!event.target.closest(".lua-search-wrap")) $("#lua-search-dropdown").classList.add("hidden");
    });
    const dlcDrop = $("#dlc-drop-zone");
    ["dragenter", "dragover"].forEach((name) => dlcDrop.addEventListener(name, (event) => { event.preventDefault(); dlcDrop.classList.add("dragover"); }));
    ["dragleave", "drop"].forEach((name) => dlcDrop.addEventListener(name, (event) => { event.preventDefault(); dlcDrop.classList.remove("dragover"); }));
    dlcDrop.addEventListener("drop", (event) => {
      handleFileDrop([event.dataTransfer?.files?.[0]?.path]);
    });

    $("#clear-log").addEventListener("click", () => { $("#activity-log").replaceChildren(); log("Журнал очищен", "system"); });
    $("#open-personal-channel").addEventListener("click", async () => {
      try { await call("open_external", { url: "https://t.me/crocodilehub2" }); }
      catch (error) { toast(errorText(error), "error"); }
    });
    $("#open-community-channel").addEventListener("click", async () => {
      try { await call("open_external", { url: "https://t.me/piratestationn" }); }
      catch (error) { toast(errorText(error), "error"); }
    });
    $("#window-minimize").addEventListener("click", () => call("minimize_window").catch(() => {}));
    $("#window-maximize").addEventListener("click", () => call("toggle_maximize_window").catch(() => {}));
    $("#window-close").addEventListener("click", () => call("close_window").catch(() => {}));
    $$(".accent").forEach((button) => button.addEventListener("click", () => applyAccent(button.dataset.accent)));

    $$(".svc-option").forEach((button) => button.addEventListener("click", () => {
      applyManifestService(button.dataset.service);
      updateHubcapKeyStatus();
    }));
    $("#hubcap-key").addEventListener("input", (event) => {
      state.lua.hubcapKey = event.currentTarget.value.trim();
      localStorage.setItem("hubcapKey", state.lua.hubcapKey);
      updateHubcapKeyStatus();
      clearTimeout(hubcapKeyTimer);
      hubcapKeyTimer = setTimeout(() => refreshHubcapUsage(), 650);
    });
    $("#hubcap-key-get").addEventListener("click", async () => {
      try { await call("open_external", { url: "https://hubcapmanifest.com/api-keys/stats" }); }
      catch (error) { toast(errorText(error), "error"); }
    });
    $("#check-update-button").addEventListener("click", () => checkForUpdates(true));
    $("#download-portable-button").addEventListener("click", () => openUpdateDownload("portable").catch((error) => toast(errorText(error), "error")));
    $("#download-installer-button").addEventListener("click", () => openUpdateDownload("installer").catch((error) => toast(errorText(error), "error")));

    const dropZone = $("#drop-zone");
    ["dragenter", "dragover"].forEach((name) => dropZone.addEventListener(name, (event) => { event.preventDefault(); dropZone.classList.add("dragover"); }));
    ["dragleave", "drop"].forEach((name) => dropZone.addEventListener(name, (event) => { event.preventDefault(); dropZone.classList.remove("dragover"); }));
    dropZone.addEventListener("drop", (event) => {
      handleFileDrop([event.dataTransfer?.files?.[0]?.path]);
    });

    if (tauri?.event?.listen) {
      tauri.event.listen("tauri://drag-drop", (event) => handleFileDrop(event.payload?.paths))
        .catch((error) => log(`Перетаскивание: ${errorText(error)}`, "error"));
      tauri.event.listen("tauri://drag-enter", () => {
        const zone = activeDropZone();
        if (zone) zone.classList.add("dragover");
      }).catch(() => {});
      tauri.event.listen("tauri://drag-leave", clearDropHighlights).catch(() => {});
    }
  }

  function activeDropZone() {
    if ($("#view-lua").classList.contains("active")) return $("#lua-drop-zone");
    if ($("#view-dlc").classList.contains("active")) return $("#dlc-drop-zone");
    if ($("#view-fix").classList.contains("active")) return $("#drop-zone");
    return null;
  }

  function clearDropHighlights() {
    ["#drop-zone", "#lua-drop-zone", "#dlc-drop-zone"].forEach((selector) => $(selector).classList.remove("dragover"));
  }

  let lastDropKey = "";
  let lastDropAt = 0;

  async function handleFileDrop(input) {
    clearDropHighlights();
    const paths = [...new Set((input || []).filter((path) => typeof path === "string" && path.trim()))];
    if (!paths.length) return;
    const active = $("[data-view-panel].active")?.dataset.viewPanel;
    const key = `${active}:${paths.map((path) => path.toLowerCase()).sort().join("\n")}`;
    const stamp = Date.now();
    if (key === lastDropKey && stamp - lastDropAt < 1000) return;
    lastDropKey = key;
    lastDropAt = stamp;
    if (active === "lua") {
      await luaInstall(paths);
    } else if (active === "dlc") {
      await scanDlcDirectory(paths[0]);
    } else if ((active === "fix" || active === "home") && !hiddenModules.has("fix")) {
      switchView("fix");
      await scanDirectory(paths[0]);
    }
  }

  async function boot() {
    bindEvents();
    await loadPlatformInfo();
    applyModuleVisibility();
    applyAccent(localStorage.getItem("accent") || "mint");
    const requestedView = new URLSearchParams(window.location.search).get("view");
    const adminReopenView = localStorage.getItem("cgs-admin-reopen-view");
    localStorage.removeItem("cgs-admin-reopen-view");
    const initialView = requestedView && viewMeta[requestedView]
      ? requestedView
      : adminReopenView && viewMeta[adminReopenView]
        ? adminReopenView
        : "home";
    switchView(initialView);
    const lastPath = localStorage.getItem("lastGamePath");
    if (lastPath) $("#game-path").value = lastPath;
    state.lua.steamPath = localStorage.getItem("luaSteamPath") || "";
    state.lua.service = localStorage.getItem("manifestService") === "hubcap" ? "hubcap" : "ryuu";
    state.lua.hubcapKey = localStorage.getItem("hubcapKey") || "";
    applyManifestService(state.lua.service);
    $("#hubcap-key").value = state.lua.hubcapKey;
    updateHubcapKeyStatus();
    renderHubcapUsage();
    if (state.lua.hubcapKey.trim()) refreshHubcapUsage();
    setInterval(() => {
      if (state.lua.hubcapKey.trim()) refreshHubcapUsage();
    }, 30_000);
    setInterval(() => {
      if ($("#view-lua")?.classList.contains("active")) refreshMillenniumStatus();
      call("luatools_restore_manifests", { steamPath: state.lua.steamPath || null }).catch(() => {});
    }, 3_000);
    log("Crocodile Gena Studio 5.5.7 инициализирован", "system");
    log(invoke ? "Все компоненты готовы к работе" : "Запустите установленное приложение", invoke ? "success" : "error");
    if (invoke) setTimeout(() => checkForUpdates(false), 1800);
  }

  document.addEventListener("DOMContentLoaded", boot);
})();
