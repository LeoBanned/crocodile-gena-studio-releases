(() => {
  "use strict";

  const invoke = window.__TAURI__?.core?.invoke;
  const $ = (selector, root = document) => root.querySelector(selector);
  const state = { report: null, scanning: false, changing: false };

  function escapeHtml(value) {
    return String(value ?? "")
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#039;");
  }

  function call(command, args = {}) {
    if (!invoke) return Promise.reject(new Error("Системный модуль VBS недоступен"));
    if (["vbs_disable", "vbs_restore", "vbs_set_test_mode", "vbs_restart", "vbs_repair"].includes(command) && window.cgsInvokePrivileged) {
      return window.cgsInvokePrivileged(command, args, "hypervisor");
    }
    return invoke(command, args);
  }

  function isActive() {
    return $('[data-hv-tab-panel="vbs"]')?.classList.contains("active") === true;
  }

  function setBusy(busy, label = "") {
    window.cgsReportBusy?.("vbs", busy || state.changing);
    state.scanning = busy;
    ["#vbs-scan-button", "#vbs-enable-button", "#vbs-disable-button", "#vbs-test-button", "#vbs-fix-wmi", "#vbs-fix-dism", "#vbs-fix-sfc"].forEach((selector) => {
      const button = $(selector);
      if (button) button.disabled = busy || state.changing || (selector === "#vbs-test-button" && !state.report);
    });
    if (label) $("#vbs-scan-time").textContent = label;
  }

  function testModeEnabled(report = state.report) {
    if (typeof report?.testSigningConfigured === "boolean") return report.testSigningConfigured;
    return report?.testSigningActive === true || report?.dse?.state === "testsigning";
  }

  function render() {
    const report = state.report;
    if (!report) return;

    const components = Array.isArray(report.components) ? report.components : [];
    const visible = [
      ["vbs", "Virtualization-based Security (VBS)"],
      ["hvci", "Memory Integrity (HVCI)"],
      ["credential-guard", "Credential Guard"],
    ];
    $("#vbs-components").innerHTML = visible.map(([id, name]) => {
      const item = components.find(component => component.id === id);
      const unknown = !item || item.state === "unknown";
      const label = unknown ? "Не определено" : item.locked ? "Заблокирован" : item.state === "configured" ? "Настроен" : item.enabled ? "Включён" : "Выключен";
      const style = unknown ? "unknown" : item.locked ? "locked" : item.enabled ? "enabled" : "disabled";
      return `<article class="vbs-component ${style}"><div><h4>${escapeHtml(name)}</h4><p>${escapeHtml(item?.description || "Windows не вернула состояние компонента.")}</p></div><span>${escapeHtml(label)}</span></article>`;
    }).join("");

    const dse = report.dse || {};
    const dseCard = $("#vbs-dse-card");
    if (dseCard) dseCard.dataset.state = dse.state || "unknown";
    $("#vbs-dse-label").textContent = dse.label || "Не определено";
    $("#vbs-dse-detail").textContent = dse.detail || "Windows не вернула состояние Code Integrity";
    $("#vbs-dse-badge").textContent = dse.state === "testsigning" ? "TEST MODE" : dse.state === "disabled" ? "DSE OFF" : dse.state === "enabled" ? "DSE ON" : "DSE TEST";

    const warnings = Array.isArray(report.warnings) ? report.warnings : [];
    $("#vbs-warning-panel").classList.toggle("hidden", warnings.length === 0);
    $("#vbs-warnings").innerHTML = warnings.map((warning) => `<p>${escapeHtml(warning)}</p>`).join("");
    const dseIsOff = dse.state === "disabled";
    const rollbackMode = report.canRestore && dse.state === "enabled";
    const testEnabled = testModeEnabled(report);
    $("#vbs-test-label").textContent = testEnabled ? "Выключить тестовый" : "Включить тестовый";
    $("#vbs-test-button").dataset.enabled = String(testEnabled);
    const testActive = report.testSigningActive === true || dse.state === "testsigning";
    $("#vbs-test-status").textContent = testEnabled !== testActive
      ? testEnabled ? "Тестовый режим включится после перезагрузки." : "Тестовый режим выключится после перезагрузки."
      : testActive ? "Тестовый режим активен." : "Тестовый режим выключен.";
    $("#vbs-enable-button").classList.toggle("hidden", testEnabled || (report.canRestore && dseIsOff));
    $("#vbs-disable-button").classList.toggle("hidden", testEnabled || !report.canRestore);
    $("#vbs-disable-label").textContent = rollbackMode ? "Откатить визор" : "Выключить визор";
  }

  async function scan() {
    if (state.scanning || state.changing) return;
    setBusy(true, "Проверка");
    try {
      state.report = await call("vbs_scan");
      $("#vbs-scan-time").textContent = `Последняя проверка: ${new Date().toLocaleTimeString(window.cgsI18n?.locale?.() || "ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`;
    } catch (error) {
      $("#vbs-scan-time").textContent = "Ошибка проверки";
      if (typeof window.cgsShowModal === "function") {
        await window.cgsShowModal("Не удалось проверить VBS", String(error), { symbol: "!" });
      }
    } finally {
      setBusy(false);
      render();
    }
  }

  async function confirmChange(mode) {
    if (mode === "test-enable" || mode === "test-disable") {
      const enabling = mode === "test-enable";
      const title = enabling ? "Включить тестовый режим?" : "Выключить тестовый режим?";
      const message = enabling
        ? "Программа отключит VBS, HVCI, Credential Guard и системный гипервизор, затем включит Test Signing. Для запуска игры отключите Secure Boot в BIOS/UEFI. После обычной перезагрузки Windows тестовый режим станет активным."
        : "Программа выключит Test Signing и вернёт сохранённые системные защиты. Для применения потребуется обычная перезагрузка Windows.";
      return typeof window.cgsShowModal === "function"
        ? window.cgsShowModal(title, message, { confirm: true, confirmText: enabling ? "Включить" : "Выключить", cancelText: "Отмена", symbol: "🧪" })
        : window.confirm(`${title}\n\n${message}`);
    }
    const enabling = mode === "enable";
    const rollbackMode = !enabling && state.report?.canRestore && state.report?.dse?.state === "enabled";
    const title = enabling ? "Включить визор?" : rollbackMode ? "Откатить визор?" : "Выключить визор?";
    const message = enabling
      ? "Нативный VBS 2.0 проверит Windows Hello, AVX и виртуализацию, сохранит исходные настройки и отключит активные компоненты. При выключенном Secure Boot будет подготовлен Test Signing; иначе — одноразовое меню загрузки с выбором 7/F7. Если BitLocker включён, его защита будет приостановлена на одну перезагрузку."
      : "Программа вернёт системные защиты и параметры загрузчика, сохранённые до включения визора. Для применения потребуется перезапуск Windows.";
    if (typeof window.cgsShowModal === "function") {
      return window.cgsShowModal(title, message, { confirm: true, confirmText: enabling ? "Включить" : rollbackMode ? "Откатить" : "Выключить", cancelText: "Отмена", symbol: "!" });
    }
    return window.confirm(`${title}\n\n${message}`);
  }

  async function askRestart(mode) {
    const message = mode === "test-enable"
      ? "Сохраните открытые файлы. После обычной перезагрузки Windows включится тестовый режим. Secure Boot должен быть отключён в BIOS/UEFI."
      : mode === "test-disable"
      ? "Сохраните открытые файлы. После обычной перезагрузки Windows тестовый режим будет выключен, а сохранённые системные защиты восстановлены."
      : mode === "enable"
      ? state.report?.secureBoot === "false"
        ? "Сохраните открытые файлы. После обычной перезагрузки Windows включится Test Signing. Если был подготовлен SecConfig.efi, подтвердите изменение UEFI на экране компьютера."
        : "Сохраните открытые файлы. После перезапуска выберите 7/F7 — «Отключить обязательную проверку подписи драйверов». Если был подготовлен SecConfig.efi, подтвердите изменение UEFI на экране компьютера."
      : "Сохранённые системные защиты будут полностью включены после обычного перезапуска Windows.";
    const accepted = typeof window.cgsShowModal === "function"
      ? await window.cgsShowModal("Требуется перезапуск", message, { confirm: true, confirmText: "Перезапустить", cancelText: "Позже", symbol: "↻" })
      : window.confirm(`Требуется перезапуск\n\n${message}`);
    if (accepted) await call("vbs_restart");
  }

  async function change(mode) {
    if (state.scanning || state.changing) return;
    state.changing = true;
    liveLines = [];
    const enabling = mode === "enable";
    const testMode = mode === "test-enable" || mode === "test-disable";
    const rollbackMode = !enabling && state.report?.canRestore && state.report?.dse?.state === "enabled";
    setBusy(false);
    const logPanel = $("#vbs-log-panel");
    const log = $("#vbs-log");
    try {
      if (!(await confirmChange(mode))) return;
      let allowUefi = false;
      if ((enabling || mode === "test-enable") && state.report?.needsUefiConsent) {
        allowUefi = await window.cgsShowModal("Снять UEFI-блокировки?", "VBS 2.0 подготовит SecConfig.efi и одноразовую запись загрузки. Продолжайте только на личном компьютере. При следующей загрузке потребуется ваше физическое подтверждение. Исходные параметры сохранятся для отката.", { confirm: true, confirmText: "Подготовить", cancelText: "Отмена", symbol: "!" });
        if (!allowUefi) return;
      }
      setBusy(false, testMode ? mode === "test-enable" ? "Включение тестового режима…" : "Выключение тестового режима…" : enabling ? "Включение визора…" : rollbackMode ? "Откат визора…" : "Выключение визора…");
      const result = testMode
        ? await call("vbs_set_test_mode", { enabled: mode === "test-enable", allowUefi })
        : await call(enabling ? "vbs_disable" : "vbs_restore", enabling ? { allowUefi } : {});
      const lines = [result.message, `Изменено параметров: ${result.changed || 0}`];
      if (result.restartRequired) lines.push("Для применения изменений требуется перезагрузка Windows.");
      if (Array.isArray(result.failed) && result.failed.length) lines.push("", "Ошибки:", ...result.failed.map((value) => `• ${value}`));
      logPanel.classList.remove("hidden");
      log.textContent = lines.join("\n");
      if (typeof window.cgsShowModal === "function") {
        await window.cgsShowModal(result.success ? "Операция завершена" : "Операция выполнена частично", lines.join("\n"), { symbol: result.success ? "✓" : "!" });
      }
      if (result.success && result.restartRequired) await askRestart(mode);
    } catch (error) {
      logPanel.classList.remove("hidden");
      log.textContent = String(error);
      if (typeof window.cgsShowModal === "function") await window.cgsShowModal("Ошибка VBS", String(error), { symbol: "!" });
    } finally {
      state.changing = false;
      await scan();
    }
  }

  const repairNames = { wmi: "Fix WMI", dism: "DISM RestoreHealth", sfc: "SFC Scannow" };
  let liveLines = [];
  function appendProgress(line) {
    if (!state.changing) return;
    liveLines.push(String(line));
    if (liveLines.length > 1500) liveLines.shift();
    $("#vbs-log-panel").classList.remove("hidden");
    const log = $("#vbs-log");
    log.textContent = liveLines.join("\n");
    log.scrollTop = log.scrollHeight;
  }
  window.__TAURI__?.event?.listen("vbs-progress", event => appendProgress(event.payload ?? event)).catch(() => {});

  async function repair(kind) {
    if (state.scanning || state.changing || !repairNames[kind]) return;
    state.changing = true;
    liveLines = [];
    setBusy(false);
    try {
      const detail = kind === "wmi"
        ? "Программа проверит WMI и при необходимости восстановит репозиторий и регистрацию компонентов. Служба WMI и зависимые службы могут временно останавливаться. Прежний репозиторий будет сохранён. На Windows Server перестройка не выполняется."
        : kind === "dism"
          ? "Восстановление хранилища компонентов Windows через DISM. Может потребоваться интернет и работающий Центр обновления Windows. Обычно занимает 5–15 минут или больше."
          : "Проверка и восстановление системных файлов Windows через SFC. Рекомендуется сначала выполнить DISM RestoreHealth. Может занять 10–15 минут или больше.";
      const accepted = typeof window.cgsShowModal === "function"
        ? await window.cgsShowModal(`Запустить ${repairNames[kind]}?`, detail, { confirm: true, confirmText: "Запустить", cancelText: "Отмена", symbol: "🛠" })
        : window.confirm(detail);
      if (!accepted) return;
      appendProgress(`${repairNames[kind]}: запуск…`);
      const result = await call("vbs_repair", { kind });
      appendProgress(result.message);
      if (result.logPath) appendProgress(`Журналы: ${result.logPath}`);
      if (result.restartRequired) appendProgress("Windows сообщает о необходимости перезагрузки. Перезагрузите компьютер после сохранения работы.");
      if (typeof window.cgsShowModal === "function") await window.cgsShowModal(result.success ? "Проверка завершена" : "Восстановление не завершено", result.message, { symbol: result.success ? "✓" : "!" });
    } catch (error) {
      appendProgress(String(error));
      if (typeof window.cgsShowModal === "function") await window.cgsShowModal("Ошибка восстановления Windows", String(error), { symbol: "!" });
    } finally {
      state.changing = false;
      setBusy(false);
      // A broken WMI scan must never prevent the repair buttons from being used.
    }
  }

  $("#vbs-fix-wmi")?.addEventListener("click", () => repair("wmi"));
  $("#vbs-fix-dism")?.addEventListener("click", () => repair("dism"));
  $("#vbs-fix-sfc")?.addEventListener("click", () => repair("sfc"));

  $("#vbs-scan-button")?.addEventListener("click", scan);
  $("#vbs-enable-button")?.addEventListener("click", () => change("enable"));
  $("#vbs-disable-button")?.addEventListener("click", () => change("disable"));
  $("#vbs-test-button")?.addEventListener("click", () => change(testModeEnabled() ? "test-disable" : "test-enable"));
  document.addEventListener("cgs:hv-tab-changed", (event) => {
    if (event.detail === "vbs" && !state.report && !state.scanning) scan();
  });

  if (isActive()) scan();
})();
