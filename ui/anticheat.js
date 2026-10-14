(() => {
  "use strict";

  const invoke = window.__TAURI__?.core?.invoke;
  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const state = { report: null, scanning: false, removing: false };

  function escapeHtml(value) {
    return String(value ?? "")
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#039;");
  }

  function call(command, args = {}) {
    if (!invoke) return Promise.reject(new Error("Системный модуль недоступен"));
    if (["anticheat_remove", "anticheat_remove_all"].includes(command) && window.cgsInvokePrivileged) {
      return window.cgsInvokePrivileged(command, args, "hypervisor");
    }
    return invoke(command, args);
  }

  function setTab(name) {
    $$('[data-hv-tab]').forEach((button) => {
      const active = button.dataset.hvTab === name;
      button.classList.toggle("active", active);
      button.setAttribute("aria-selected", String(active));
    });
    $$('[data-hv-tab-panel]').forEach((panel) => panel.classList.toggle("active", panel.dataset.hvTabPanel === name));
    try { localStorage.setItem("cgs-hv-tab", name); } catch (_) {}
    if (name === "anticheat" && !state.report && !state.scanning) scan();
    document.dispatchEvent(new CustomEvent("cgs:hv-tab-changed", { detail: name }));
  }

  function setBusy(scanning, label = "") {
    state.scanning = scanning;
    $("#ac-loading").classList.toggle("hidden", !scanning);
    $("#ac-list").classList.toggle("hidden", scanning);
    $("#ac-scan-button").disabled = scanning || state.removing;
    $("#ac-remove-all").disabled = scanning || state.removing;
    if (label) $("#ac-status").textContent = label;
  }

  function render() {
    const items = Array.isArray(state.report?.items) ? state.report.items : [];
    const detected = Number(state.report?.detected || 0);
    $("#ac-supported-count").textContent = String(items.length || 12);
    $("#ac-detected-count").textContent = state.report ? String(detected) : "—";
    $("#ac-remove-count").textContent = String(detected);
    $("#ac-remove-all").classList.toggle("hidden", !detected);
    $("#ac-status").textContent = detected ? "Найдены следы" : state.report ? "Система чиста" : "Готово";
    $("#ac-status-detail").textContent = detected
      ? `${detected} ${detected === 1 ? "система требует внимания" : "системы требуют внимания"}`
      : state.report ? "известные компоненты не обнаружены" : "нажмите «Сканировать»";

    $("#ac-list").innerHTML = items.map((item) => {
      const evidence = Array.isArray(item.evidence) ? item.evidence : [];
      return `
        <article class="ac-item ${item.detected ? "detected" : ""}" data-ac-id="${escapeHtml(item.id)}">
          <div class="ac-item-main">
            <div class="ac-item-heading"><h4>${escapeHtml(item.name)}</h4><small>${escapeHtml(item.version || item.vendor || "")}</small></div>
            <div class="ac-item-evidence">
              ${evidence.map((entry) => `<div class="ac-evidence"><b>${escapeHtml(entry.kind)}</b><span title="${escapeHtml(entry.path)}">${escapeHtml(entry.path)}</span></div>`).join("")}
            </div>
          </div>
          <div class="ac-item-state">
            <span class="ac-state">${item.detected ? "Обнаружен" : "Чисто"}</span>
            ${item.detected ? `<button class="ac-remove-button" data-ac-remove="${escapeHtml(item.id)}" ${state.removing ? "disabled" : ""}>Удалить</button>` : ""}
          </div>
        </article>`;
    }).join("");
  }

  async function scan() {
    if (state.scanning || state.removing) return;
    setBusy(true, "Сканирование");
    $("#ac-status-detail").textContent = "проверяем службы, реестр и файлы";
    try {
      state.report = await call("anticheat_scan");
      $("#ac-scan-time").textContent = `Последняя проверка: ${new Date().toLocaleTimeString(window.cgsI18n?.locale?.() || "ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`;
    } catch (error) {
      $("#ac-status").textContent = "Ошибка";
      $("#ac-status-detail").textContent = String(error);
    } finally {
      setBusy(false);
      render();
    }
  }

  function removalWarning(item) {
    const target = item ? `«${item.name}»` : "все найденные античиты";
    return `Удалить ${target}? Будут остановлены известные службы и удалены записи реестра. Перед очисткой вы сами выберете папку, куда программа сохранит резервные файлы. Игры, которым нужна эта защита, могут потребовать восстановление файлов.`;
  }

  async function remove(id = null) {
    if (state.removing || state.scanning) return;
    const item = id ? state.report?.items?.find((entry) => entry.id === id) : null;
    const confirmed = typeof window.cgsShowModal === "function"
      ? await window.cgsShowModal("Подтверждение удаления", removalWarning(item), { confirm: true, confirmText: "Удалить", cancelText: "Отмена", symbol: "!" })
      : window.confirm(removalWarning(item));
    if (!confirmed) return;

    let backupDir;
    try {
      backupDir = await call("anticheat_pick_backup_directory");
    } catch (error) {
      if (typeof window.cgsShowModal === "function") await window.cgsShowModal("Не удалось выбрать папку", String(error), { symbol: "!" });
      return;
    }
    if (!backupDir) return;

    state.removing = true;
    $("#ac-scan-button").disabled = true;
    $("#ac-remove-all").disabled = true;
    render();
    $("#ac-status").textContent = "Очистка";
    $("#ac-status-detail").textContent = "останавливаем службы и сохраняем резервную копию";
    try {
      const result = id
        ? await call("anticheat_remove", { id, backupDir })
        : await call("anticheat_remove_all", { backupDir });
      const lines = [result.message, `Удалено компонентов: ${result.removed}`];
      if (result.backupDir) lines.push(`Резервная копия: ${result.backupDir}`);
      if (result.restartRequired) lines.push("Для завершения очистки рекомендуется перезагрузить Windows.");
      if (Array.isArray(result.failed) && result.failed.length) lines.push("", "Не удалено:", ...result.failed.map((value) => `• ${value}`));
      $("#ac-log-panel").classList.remove("hidden");
      $("#ac-log").textContent = lines.join("\n");
      if (typeof window.cgsShowModal === "function") {
        await window.cgsShowModal(result.success ? "Очистка завершена" : "Очистка выполнена частично", lines.join("\n"), { symbol: result.success ? "✓" : "!" });
      }
    } catch (error) {
      if (typeof window.cgsShowModal === "function") await window.cgsShowModal("Не удалось удалить", String(error), { symbol: "!" });
    } finally {
      state.removing = false;
      await scan();
    }
  }

  function bindEvents() {
    $(".hv-module-tabs")?.addEventListener("click", (event) => {
      const button = event.target.closest("[data-hv-tab]");
      if (button) setTab(button.dataset.hvTab);
    });
    $("#ac-scan-button")?.addEventListener("click", scan);
    $("#ac-remove-all")?.addEventListener("click", () => remove());
    $("#ac-list")?.addEventListener("click", (event) => {
      const button = event.target.closest("[data-ac-remove]");
      if (button) remove(button.dataset.acRemove);
    });
  }

  bindEvents();
  let initialTab = "hypervisor";
  try { initialTab = localStorage.getItem("cgs-hv-tab") || initialTab; } catch (_) {}
  setTab(["hypervisor", "anticheat", "vbs"].includes(initialTab) ? initialTab : "hypervisor");
})();
