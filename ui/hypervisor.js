(() => {
  "use strict";

  const invoke = window.__TAURI__?.core?.invoke;
  const listen = window.__TAURI__?.event?.listen;
  const $ = (selector, root = document) => root.querySelector(selector);

  const steps = [
    ["Проверка системы", "Core Isolation и права администратора"],
    ["Подготовка профиля", "Выбранный профиль KVC или DRV"],
    ["Служебные файлы", "Проверка загрузчика и BAT-файла"],
    ["Запуск сценария", "Системная подготовка Windows"],
    ["Запуск игры", "Старт выбранного EXE"],
    ["Возврат DSE", "Восстановление после запуска"],
    ["Очистка", "Завершение сценария"],
  ];

  const state = {
    selectedId: null,
    launchers: [],
    games: [],
    running: false,
    launching: false,
    gameRunning: false,
    processMonitorToken: 0,
    initialized: false,
    confirmingRemove: "",
    profileSwitching: false,
    pendingProfile: null,
  };

  const call = (command, args = {}) => {
    if (!invoke) return Promise.reject(new Error("Системный модуль недоступен"));
    return invoke(command, args);
  };

  function escapeHtml(value) {
    return String(value ?? "")
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#039;");
  }

  function timeNow() {
    return new Date().toLocaleTimeString(window.cgsI18n?.locale?.() || "ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  function appendLog(kind, message) {
    const output = $("#hv-log");
    if (!output) return;
    const line = document.createElement("div");
    line.className = `hv-log-line ${kind || "info"}`;
    const time = document.createElement("time");
    time.textContent = timeNow();
    const text = document.createElement("span");
    text.textContent = String(message || "");
    line.append(time, text);
    output.appendChild(line);
    output.scrollTop = output.scrollHeight;
  }

  function setBusy(busy, label = "") {
    state.running = busy;
    ["#hv-add-game", "#hv-selected-verify"].forEach((selector) => {
      const button = $(selector);
      if (!button) return;
      button.disabled = busy || (selector === "#hv-selected-verify" && !state.selectedId);
    });
    updateLaunchButton();
    if (label) $("#hv-run-state").textContent = label;
    renderGames();
  }

  function updateLaunchButton() {
    const button = $("#hv-launch-button");
    if (!button) return;
    button.disabled = state.running || state.gameRunning || !state.selectedId;
    button.classList.toggle("game-running", state.gameRunning);
    button.classList.toggle("launching", state.launching);
    const label = button.querySelector("span:last-child");
    if (label) label.textContent = state.gameRunning ? "Игра запущена" : state.launching ? "Запуск..." : "Запустить";
  }

  function setGameRunning(running) {
    state.gameRunning = Boolean(running);
    updateLaunchButton();
  }

  const wait = (milliseconds) => new Promise((resolve) => window.setTimeout(resolve, milliseconds));

  async function monitorGameProcess(waitForStart = false) {
    const token = ++state.processMonitorToken;
    const selectedId = state.selectedId;
    let processSeen = !waitForStart && state.gameRunning;
    let startMisses = 0;

    while (token === state.processMonitorToken && selectedId === state.selectedId) {
      let running;
      try {
        running = await call("is_selected_game_running");
      } catch (error) {
        appendLog("warning", `Не удалось проверить процесс игры: ${error}`);
        setGameRunning(false);
        return;
      }

      if (running) {
        processSeen = true;
        setGameRunning(true);
      } else if (processSeen) {
        setGameRunning(false);
        appendLog("info", "Процесс игры завершён. Кнопка запуска снова доступна.");
        return;
      } else {
        startMisses += 1;
        if (startMisses >= 10) {
          setGameRunning(false);
          return;
        }
      }

      await wait(1500);
    }
  }

  function renderSteps() {
    $("#hv-steps").innerHTML = steps.map(([title, detail], index) => `
      <article class="hv-step" data-hv-step="${index + 1}">
        <span>${index + 1}</span><div><strong>${title}</strong><p>${detail}</p></div>
      </article>`).join("");
  }

  function resetSteps() {
    document.querySelectorAll("[data-hv-step]").forEach((item) => item.classList.remove("active", "done", "error"));
  }

  function activateStep(step) {
    const number = Number(step) || 1;
    document.querySelectorAll("[data-hv-step]").forEach((item) => {
      const current = Number(item.dataset.hvStep);
      item.classList.toggle("done", current < number);
      item.classList.toggle("active", current === number);
      item.classList.remove("error");
    });
  }

  function completeSteps() {
    document.querySelectorAll("[data-hv-step]").forEach((item) => {
      item.classList.remove("active", "error");
      item.classList.add("done");
    });
  }

  function markStepError() {
    const current = $(".hv-step.active") || $("[data-hv-step='1']");
    current?.classList.add("error");
    current?.classList.remove("active");
  }

  function pluralGames(count) {
    const n10 = count % 10;
    const n100 = count % 100;
    if (n10 === 1 && n100 !== 11) return `${count} игра`;
    if (n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14)) return `${count} игры`;
    return `${count} игр`;
  }

  function selectedGame() {
    return state.games.find((game) => game.id === state.selectedId) || null;
  }

  function applyLibrary(library) {
    state.selectedId = library?.selected_id || null;
    state.launchers = Array.isArray(library?.launchers) ? library.launchers : [];
    state.games = Array.isArray(library?.games) ? library.games : [];
    renderGames();
  }

  function renderGames() {
    const list = $("#hv-game-list");
    const empty = $("#hv-empty");
    $("#hv-library-count").textContent = pluralGames(state.games.length);
    $("#hv-games-count").textContent = String(state.games.length);
    empty.classList.toggle("hidden", state.games.length > 0);
    list.classList.toggle("hidden", state.games.length === 0);

    list.innerHTML = state.games.map((game) => {
      const selected = game.id === state.selectedId;
      const confirm = state.confirmingRemove === game.id;
      return `
        <article class="hv-game-card ${selected ? "selected" : ""}" data-game-id="${escapeHtml(game.id)}">
          <button class="hv-game-symbol" data-hv-action="select" data-game-id="${escapeHtml(game.id)}" title="Выбрать игру" ${state.running ? "disabled" : ""}>${selected ? "✓" : "🎮"}</button>
          <div class="hv-game-main">
            <div class="hv-game-heading"><strong>${escapeHtml(game.name)}</strong><b class="${game.ready ? "ready" : ""}"><i></i>${game.ready ? "Готова к запуску" : "Нужна проверка"}</b></div>
            <div class="hv-game-path" title="${escapeHtml(game.exe_path)}">${escapeHtml(game.exe_path)}</div>
            <div class="hv-game-bottom">
              <div class="hv-game-profile"><span>ПРОФИЛЬ</span><strong>${escapeHtml(game.launcher_name || "HV-Launcher")}</strong></div>
              <div class="hv-game-actions">
                ${selected ? '<span class="hv-active-game"><i></i>Активна</span>' : `<button class="hv-game-action select" data-hv-action="select" data-game-id="${escapeHtml(game.id)}" ${state.running ? "disabled" : ""}>Выбрать</button>`}
                <button class="hv-game-action" data-hv-action="verify" data-game-id="${escapeHtml(game.id)}" ${state.running ? "disabled" : ""}><span>↻</span> Проверить</button>
                <button class="hv-game-action danger ${confirm ? "confirm" : ""}" data-hv-action="remove" data-game-id="${escapeHtml(game.id)}" ${state.running ? "disabled" : ""}>${confirm ? "Точно удалить?" : "Удалить"}</button>
              </div>
            </div>
          </div>
        </article>`;
    }).join("");
  }

  function renderStatus(status) {
    const game = status?.selected_game || selectedGame();
    $("#hv-admin-status").textContent = status?.admin ? "Admin" : "Через UAC";
    $("#hv-games-count").textContent = String(status?.games_count ?? state.games.length);
    if (!game) {
      state.processMonitorToken += 1;
      state.gameRunning = false;
      $("#hv-selected-title").textContent = "Игра не выбрана";
      $("#hv-selected-path").textContent = "Добавьте исполняемый файл игры.";
      $("#hv-selected-profile").textContent = "Профиль: —";
      $("#hv-selected-state").textContent = "Нет игры";
      $("#hv-selected-state").className = "";
      $("#hv-ready-status").textContent = "Нет игры";
      $("#hv-selected-verify").disabled = true;
      $("#hv-selected-profiles").innerHTML = "";
      updateLaunchButton();
      return;
    }
    if (!state.running && typeof status?.game_running === "boolean") {
      state.gameRunning = status.game_running;
    }
    $("#hv-selected-title").textContent = game.name;
    $("#hv-selected-path").textContent = game.exe_path;
    $("#hv-selected-profile").textContent = `Профиль: ${game.launcher_name || status?.selected_launcher?.name || "—"}`;
    $("#hv-selected-state").textContent = game.ready ? "Готова" : "Нужна проверка";
    $("#hv-selected-state").className = game.ready ? "ready" : "";
    $("#hv-ready-status").textContent = game.ready ? "Готово" : "Проверить";
    $("#hv-selected-verify").disabled = state.running || !state.selectedId;
    updateLaunchButton();
    const profiles = state.launchers.length ? state.launchers : [
      { id: "hv_launcher", badge: "KVC", name: "HV-Launcher" },
      { id: "plug_n_play", badge: "DRV", name: "HV-PlugNPlay" },
    ];
    $("#hv-selected-profiles").innerHTML = profiles.map((profile) => `
      <button class="hv-selected-profile ${game.launcher_id === profile.id ? "active" : ""}"
        data-hv-top-profile="${escapeHtml(profile.id)}" title="${escapeHtml(profile.name)}"
        ${state.running ? "disabled" : ""}>
        <span>${escapeHtml(profile.badge)}</span>
      </button>`).join("");
  }

  async function refreshAll(silent = false) {
    try {
      const [library, status] = await Promise.all([call("list_games"), call("get_app_status")]);
      applyLibrary(library);
      renderStatus(status);
      if (status?.game_running && !state.running) monitorGameProcess(false);
      if (!silent) appendLog("success", "Библиотека и состояние профилей обновлены.");
    } catch (error) {
      appendLog("error", `Не удалось обновить модуль: ${error}`);
    }
  }

  async function addGame() {
    if (state.running) return;
    try {
      const exePath = await call("select_game_exe");
      if (!exePath) return;
      setBusy(true, "Подготовка");
      appendLog("info", `Выбран EXE: ${exePath}`);
      const library = await call("add_game", { exePath, launcherId: "hv_launcher" });
      applyLibrary(library);
      await refreshAll(true);
      appendLog("success", "Игра добавлена и подготовлена с профилем KVC.");
    } catch (error) {
      appendLog("error", String(error));
    } finally {
      setBusy(false, "Ожидание");
    }
  }

  async function selectGame(id) {
    try {
      state.processMonitorToken += 1;
      setGameRunning(false);
      const library = await call("set_selected_game", { id });
      applyLibrary(library);
      await refreshAll(true);
    } catch (error) { appendLog("error", String(error)); }
  }

  async function verifyGame(id) {
    try {
      setBusy(true, "Проверка");
      const library = await call("verify_game", { id });
      applyLibrary(library);
      await refreshAll(true);
      appendLog("success", "Файлы игры проверены и восстановлены при необходимости.");
    } catch (error) { appendLog("error", String(error)); }
    finally { setBusy(false, "Ожидание"); }
  }

  async function setProfile(id, launcherId) {
    const game = state.games.find((item) => item.id === id);
    const profile = state.launchers.find((item) => item.id === launcherId);
    if (!game || game.launcher_id === launcherId) return;

    // The selector responds instantly. File swaps are serialized in the
    // background so a second click is never blocked and the last choice wins.
    game.launcher_id = launcherId;
    game.launcher_name = profile?.name || (launcherId === "plug_n_play" ? "HV-PlugNPlay" : "HV-Launcher");
    game.ready = false;
    state.pendingProfile = { id, launcherId };
    renderGames();
    renderStatus({ selected_game: selectedGame(), games_count: state.games.length });

    if (state.profileSwitching) return;
    state.profileSwitching = true;

    while (state.pendingProfile) {
      const request = state.pendingProfile;
      state.pendingProfile = null;
      try {
        const library = await call("set_game_launcher", request);
        if (!state.pendingProfile) {
          applyLibrary(library);
          renderStatus({ selected_game: selectedGame(), games_count: state.games.length });
          appendLog("success", `Профиль ${request.launcherId === "plug_n_play" ? "DRV" : "KVC"} установлен, файлы заменены.`);
        }
      } catch (error) {
        appendLog("error", String(error));
        if (!state.pendingProfile) await refreshAll(true);
      }
    }

    state.profileSwitching = false;
    $("#hv-run-state").textContent = "Ожидание";
  }

  async function removeGame(id) {
    if (state.confirmingRemove !== id) {
      state.confirmingRemove = id;
      renderGames();
      return;
    }
    try {
      const library = await call("remove_game", { id });
      state.confirmingRemove = "";
      applyLibrary(library);
      await refreshAll(true);
      appendLog("warning", "Игра удалена из библиотеки модуля. Файлы в папке игры не удалялись.");
    } catch (error) { appendLog("error", String(error)); }
  }

  async function launchSelected() {
    if (state.running || state.gameRunning || !state.selectedId) return;
    resetSteps();
    activateStep(1);
    state.launching = true;
    setBusy(true, "Запуск");
    appendLog("info", "Запущен сценарий выбранного профиля.");
    let launchSucceeded = false;
    try {
      const report = await call("launch_selected_game");
      launchSucceeded = Boolean(report?.success);
      appendLog(report?.success ? "success" : "error", report?.message || "Сценарий завершён.");
      if (report?.success) {
        completeSteps();
        $("#hv-run-state").textContent = "Готово";
      } else {
        markStepError();
        $("#hv-run-state").textContent = "Ошибка";
      }
    } catch (error) {
      appendLog("error", String(error));
      markStepError();
      $("#hv-run-state").textContent = "Ошибка";
    } finally {
      state.launching = false;
      setBusy(false);
      if (launchSucceeded) {
        setGameRunning(true);
        monitorGameProcess(true);
      } else {
        setGameRunning(false);
      }
    }
  }

  function handleStationEvent(payload) {
    if (!payload) return;
    appendLog(payload.kind || "info", payload.message || "Событие модуля");
    if (payload.step) activateStep(payload.step);
    if (payload.kind === "error") markStepError();
  }

  async function copyLog() {
    const text = [...document.querySelectorAll("#hv-log .hv-log-line")]
      .map((line) => `${line.querySelector("time")?.textContent || ""} ${line.querySelector("span")?.textContent || ""}`.trim())
      .join("\n");
    if (!text) return appendLog("warning", "Журнал пока пуст.");
    try {
      await navigator.clipboard.writeText(text);
      appendLog("success", "Журнал скопирован.");
    } catch (error) { appendLog("error", `Не удалось скопировать журнал: ${error}`); }
  }

  function bindEvents() {
    $("#hv-add-game")?.addEventListener("click", addGame);
    $("#hv-help-button")?.addEventListener("click", () => {
      const message = [
        "1. Нажмите «Добавить игру» и выберите основной EXE игры.",
        "2. Выберите KVC или DRV — нужные файлы заменятся автоматически.",
        "3. Если карточка показывает «Нужна проверка», нажмите «Проверить».",
        "4. Нажмите светлую кнопку «Запустить» и подтвердите UAC.",
        "5. Пока игра работает, повторный запуск заблокирован. После закрытия игры кнопка включится сама.",
      ].join("\n\n");
      if (typeof window.cgsShowModal === "function") {
        const modal = document.querySelector("#modal-backdrop .modal");
        modal?.classList.add("hv-help-modal");
        window.cgsShowModal("Как пользоваться Гипервизором", message, { symbol: "💡" })
          .finally(() => modal?.classList.remove("hv-help-modal"));
      } else {
        appendLog("info", message.replaceAll("\n\n", " "));
      }
    });
    $("#hv-launch-button")?.addEventListener("click", launchSelected);
    $("#hv-selected-verify")?.addEventListener("click", () => {
      if (state.selectedId) verifyGame(state.selectedId);
    });
    $("#hv-selected-profiles")?.addEventListener("click", (event) => {
      const button = event.target.closest("[data-hv-top-profile]");
      if (button && state.selectedId && !button.disabled) setProfile(state.selectedId, button.dataset.hvTopProfile);
    });
    $("#hv-copy-log")?.addEventListener("click", copyLog);
    $("#hv-clear-log")?.addEventListener("click", () => { $("#hv-log").innerHTML = ""; });
    $("#hv-game-list")?.addEventListener("click", (event) => {
      const button = event.target.closest("[data-hv-action]");
      if (!button || button.disabled) return;
      const id = button.dataset.gameId;
      const action = button.dataset.hvAction;
      if (action === "select") selectGame(id);
      if (action === "verify") verifyGame(id);
      if (action === "remove") removeGame(id);
      if (action === "profile") setProfile(id, button.dataset.profileId);
    });
    window.addEventListener("cgs:view-changed", (event) => {
      if (event.detail === "hypervisor") refreshAll(true);
    });
  }

  async function boot() {
    if (state.initialized) return;
    state.initialized = true;
    renderSteps();
    bindEvents();
    appendLog("info", "Модуль «Гипервизор» готов.");
    if (listen) {
      try { await listen("station-event", (event) => handleStationEvent(event.payload ?? event)); }
      catch (error) { appendLog("warning", `События запуска недоступны: ${error}`); }
    }
    await refreshAll(true);
    call("check_asset_updates").catch((error) => appendLog("warning", `Обновление профилей пропущено: ${error}`));
  }

  boot();
})();
