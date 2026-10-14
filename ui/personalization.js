(() => {
  "use strict";

  const STORAGE = "cgs-personalization-v1";
  const MODULES = ["fix", "saves", "dlc", "lua", "hypervisor", "files", "utilities"];
  const LABELS = { home: "Главная", fix: "Online Fix", saves: "Сохранения", dlc: "DLC Unlocker", lua: "CrocLua", hypervisor: "Гипервизор", files: "Кряки", utilities: "Утилиты", settings: "Настройки" };
  const THEMES = ["graphite", "glass", "minimal", "neon", "midnight", "obsidian", "forest", "plum", "warm", "arctic", "white"];
  const THEME_LABELS = { midnight: "Полночь", obsidian: "Обсидиан", forest: "Лес", plum: "Слива", warm: "Тёплый", arctic: "Север", white: "Белый" };
  const FONTS = {
    system: '"Segoe UI Variable", "Segoe UI", system-ui, sans-serif',
    bahnschrift: 'Bahnschrift, "Segoe UI", system-ui, sans-serif',
    verdana: 'Verdana, "Segoe UI", sans-serif',
    tahoma: 'Tahoma, "Segoe UI", sans-serif',
    consolas: 'Consolas, "Cascadia Mono", monospace',
  };
  const DEFAULTS = { theme: "graphite", sidebar: "full", scale: 100, shape: "soft", font: "system", weight: "regular", contrast: "normal", motion: true, glow: true, mascot: true, mascotPlace: "sidebar", reactions: true, order: MODULES };
  const root = document.documentElement;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const $ = (selector) => document.querySelector(selector);
  const $$ = (selector) => [...document.querySelectorAll(selector)];
  const choice = (value, allowed, fallback) => allowed.includes(value) ? value : fallback;

  function normalize(value) {
    const input = value && typeof value === "object" ? value : {};
    const order = [...new Set((Array.isArray(input.order) ? input.order : []).filter((id) => MODULES.includes(id)))];
    return {
      theme: choice(input.theme, THEMES, "graphite"),
      sidebar: choice(input.sidebar, ["full", "compact", "icons"], "full"),
      scale: Number.isFinite(Number(input.scale)) ? Math.max(90, Math.min(125, Math.round(Number(input.scale) / 5) * 5)) : 100,
      shape: choice(input.shape, ["sharp", "soft", "round"], "soft"),
      font: choice(input.font, Object.keys(FONTS), "system"),
      weight: choice(input.weight, ["light", "regular", "bold"], "regular"),
      contrast: choice(input.contrast, ["normal", "high"], "normal"),
      motion: typeof input.motion === "boolean" ? input.motion : true,
      glow: typeof input.glow === "boolean" ? input.glow : true,
      mascot: typeof input.mascot === "boolean" ? input.mascot : true,
      mascotPlace: choice(input.mascotPlace, ["sidebar", "floating"], "sidebar"),
      reactions: typeof input.reactions === "boolean" ? input.reactions : true,
      order: [...order, ...MODULES.filter((id) => !order.includes(id))],
    };
  }

  let settings;
  try { settings = normalize(JSON.parse(localStorage.getItem(STORAGE) || "null")); }
  catch (_) { settings = normalize(DEFAULTS); }
  let mounted = false;
  let mascotState = "idle";
  let mascotMessage = "Рад тебя видеть!";
  let reactionTimer;
  let eyeFrame;
  const tasks = new Set();

  const CROCODILE = `<svg viewBox="0 0 150 140" class="croc-svg" aria-hidden="true">
    <ellipse cx="76" cy="123" rx="43" ry="6" class="croc-shadow"/>
    <g class="croc-character">
      <path class="croc-tail" d="M95 94c19 3 27-6 34-17 3 14-2 30-19 32l-16-2z"/>
      <path class="croc-body" d="M46 77c-7 11-7 30 1 40 7 6 38 8 49 0 8-12 5-30-3-39z"/>
      <path class="croc-belly" d="M56 88c-5 6-6 23-1 28h27c6-6 6-23 0-29z"/>
      <path class="croc-foot" d="M46 114c-9 2-14 8-11 10h29l-1-9m20 0c10-2 17 2 20 9H77"/>
      <g class="croc-head">
        <path class="croc-crest" d="m41 55-5-13 14 3 3-17 13 12 12-12 5 17"/>
        <path class="croc-face" d="M41 54c-2-23 27-33 43-15 5 6 6 12 6 18 9-1 24-1 32 2 10 4 9 20-1 25-12 7-45 9-67 0-14-5-17-18-13-30z"/>
        <path class="croc-jaw" d="M47 72c14 10 48 12 77 0-1 8-3 12-13 15-18 5-47 3-57-3z"/>
        <g class="croc-eyes"><ellipse cx="57" cy="50" rx="9" ry="11"/><ellipse cx="78" cy="50" rx="8" ry="10"/><g class="croc-pupils"><ellipse cx="60" cy="51" rx="3" ry="5"/><ellipse cx="80" cy="51" rx="3" ry="5"/></g></g>
        <path class="croc-mouth" d="M56 72c17 8 43 8 59-1"/>
        <path class="croc-teeth" d="m75 78 4 6 4-6m15-1 3 5 4-6"/>
        <ellipse class="croc-nostril" cx="116" cy="63" rx="2.5" ry="2"/>
        <ellipse class="croc-cheek" cx="49" cy="68" rx="5" ry="3"/>
      </g>
      <path class="croc-arm-left" d="M47 89c-10 0-12 11-8 15l8-4"/>
      <path class="croc-arm-right" d="M91 90c10-9 12-11 14-6 4 7-1 15-12 17"/>
      <path class="croc-scales" d="m47 106 5-2m35 4 6-2m12-9 6-1"/>
    </g>
    <g class="croc-spark"><path d="m113 29 2 6 6 2-6 2-2 6-2-6-6-2 6-2z"/><circle cx="35" cy="29" r="2"/></g>
  </svg>`;

  function motionEnabled() { return settings.motion && !reducedMotion.matches; }

  function applyRoot() {
    root.dataset.uiTheme = settings.theme;
    root.dataset.uiSidebar = settings.sidebar;
    root.dataset.uiShape = settings.shape;
    root.dataset.uiFont = settings.font;
    root.dataset.uiWeight = settings.weight;
    root.dataset.uiContrast = settings.contrast;
    root.style.setProperty("--font", FONTS[settings.font]);
    root.dataset.uiMotion = motionEnabled() ? "on" : "off";
    root.dataset.uiGlow = settings.glow ? "on" : "off";
    root.dataset.uiMascotPlace = settings.mascotPlace;
    root.style.setProperty("--ui-scale", String(settings.scale / 100));
  }
  applyRoot();

  function applyOrder() {
    const navigation = $(".navigation");
    const last = $(".nav-item[data-view='settings']");
    const list = $(".module-visibility-list");
    const navOrder = $$(".navigation .nav-item[data-view]").map((button) => button.dataset.view).filter((id) => MODULES.includes(id));
    const rowOrder = $$('[data-module-row]').map((row) => row.dataset.moduleRow);
    if (navOrder.join() !== settings.order.join()) {
      for (const id of settings.order) {
        const button = $(`.nav-item[data-view='${id}']`);
        if (button && navigation && last) navigation.insertBefore(button, last);
      }
    }
    if (rowOrder.join() !== settings.order.join()) {
      for (const id of settings.order) {
        const row = $(`[data-module-row='${id}']`);
        if (row && list) list.appendChild(row);
      }
    }
    $$('[data-module-row]').forEach((row, index) => {
      row.querySelector('[data-order-step="up"]').disabled = index === 0;
      row.querySelector('[data-order-step="down"]').disabled = index === MODULES.length - 1;
    });
    $$(".nav-item[data-view]").forEach((button) => {
      const label = LABELS[button.dataset.view] || "";
      if (button.getAttribute("title") !== label) button.setAttribute("title", label);
      if (button.getAttribute("aria-label") !== label) button.setAttribute("aria-label", label);
    });
  }

  function syncControls() {
    for (const [selector, key, attribute] of [
      ["[data-theme-choice]", "theme", "themeChoice"],
      ["[data-shape-choice]", "shape", "shapeChoice"],
      ["[data-sidebar-choice]", "sidebar", "sidebarChoice"],
      ["[data-mascot-place]", "mascotPlace", "mascotPlace"],
      ["[data-weight-choice]", "weight", "weightChoice"],
    ]) {
      $$(selector).forEach((button) => {
        const active = button.dataset[attribute] === settings[key];
        button.classList.toggle("active", active);
        button.setAttribute("aria-pressed", String(active));
      });
    }
    $("#ui-scale").value = String(settings.scale);
    $("#ui-scale-value").textContent = `${settings.scale}%`;
    $("#ui-font").value = settings.font;
    $("#ui-contrast").checked = settings.contrast === "high";
    $("#extra-theme-selected").textContent = THEME_LABELS[settings.theme] || "7";
    $("#extra-theme-selected").classList.toggle("selected", Boolean(THEME_LABELS[settings.theme]));
    for (const [id, key] of [["ui-motion", "motion"], ["ui-glow", "glow"], ["ui-mascot", "mascot"], ["ui-mascot-reactions", "reactions"]]) $("#" + id).checked = settings[key];
    const widget = $("#mascot-widget");
    widget.hidden = !settings.mascot;
    widget.classList.toggle("mascot-floating", settings.mascotPlace === "floating");
    const target = settings.mascotPlace === "floating" ? document.body : $(".sidebar-footer");
    if (widget.parentElement !== target) target.prepend(widget);
    widget.dataset.state = mascotState;
    $("#mascot-message").textContent = mascotMessage;
  }

  function update(patch) {
    settings = normalize({ ...settings, ...patch });
    try { localStorage.setItem(STORAGE, JSON.stringify(settings)); } catch (_) {}
    applyRoot();
    if (mounted) { applyOrder(); syncControls(); }
    window.dispatchEvent(new CustomEvent("cgs:personalization-changed", { detail: { ...settings, order: [...settings.order] } }));
  }

  function changeState(next, message) {
    mascotState = next;
    mascotMessage = message;
    const widget = $("#mascot-widget");
    if (widget) { widget.dataset.state = next; $("#mascot-message").textContent = message; }
  }

  function settle() {
    clearTimeout(reactionTimer);
    if (tasks.size && settings.reactions) changeState("busy", "Я слежу за процессом…");
    else changeState("idle", "Всё готово к игре.");
  }

  function react(type) {
    if (!settings.mascot || !settings.reactions || document.hidden) return;
    if (!["error", "pet", "success"].includes(type)) return;
    clearTimeout(reactionTimer);
    if (type === "error") changeState("error", "Есть нюанс. Проверь сообщение.");
    else if (type === "pet") changeState("happy", "Спасибо! Продолжим?");
    else if (type === "success") changeState("happy", "Готово! Отличная работа.");
    else return;
    reactionTimer = setTimeout(settle, 4200);
  }

  function enhanceOrderEditor() {
    $$("[data-module-toggle]").forEach((input) => {
      const row = input.closest("label");
      row.dataset.moduleRow = input.dataset.moduleToggle;
      const handle = document.createElement("span");
      handle.className = "module-sort-handle";
      handle.draggable = true;
      handle.setAttribute("title", "Перетащить модуль");
      handle.innerHTML = '<svg viewBox="0 0 16 20"><path d="M5 4h.01M11 4h.01M5 10h.01M11 10h.01M5 16h.01M11 16h.01"/></svg>';
      row.prepend(handle);
      const controls = document.createElement("span");
      controls.className = "module-order-controls";
      controls.innerHTML = '<button type="button" data-order-step="up" title="Переместить выше" aria-label="Переместить выше"><svg viewBox="0 0 16 16"><path d="m4 10 4-4 4 4"/></svg></button><button type="button" data-order-step="down" title="Переместить ниже" aria-label="Переместить ниже"><svg viewBox="0 0 16 16"><path d="m4 6 4 4 4-4"/></svg></button>';
      row.insertBefore(controls, input.closest(".switch"));
    });
    const list = $(".module-visibility-list");
    let dragging = null;
    list.addEventListener("dragstart", (event) => {
      const handle = event.target.closest(".module-sort-handle");
      if (!handle) return;
      dragging = handle.closest("label").dataset.moduleRow;
      event.dataTransfer.setData("application/x-cgs-module", dragging);
      event.dataTransfer.effectAllowed = "move";
      handle.closest("label").classList.add("module-dragging");
    });
    list.addEventListener("dragover", (event) => {
      if (!dragging) return;
      const row = event.target.closest("[data-module-row]");
      if (!row) return;
      event.preventDefault();
      $$('[data-module-row]').forEach((item) => item.classList.toggle("module-drop-target", item === row));
    });
    const clear = () => {
      dragging = null;
      $$('[data-module-row]').forEach((row) => row.classList.remove("module-dragging", "module-drop-target"));
    };
    list.addEventListener("drop", (event) => {
      const targetRow = event.target.closest("[data-module-row]");
      const target = targetRow?.dataset.moduleRow;
      if (!dragging || !target || dragging === target) { clear(); return; }
      event.preventDefault();
      const order = settings.order.filter((id) => id !== dragging);
      const box = targetRow.getBoundingClientRect();
      const after = event.clientY > box.top + box.height / 2;
      order.splice(order.indexOf(target) + (after ? 1 : 0), 0, dragging);
      update({ order });
      clear();
    });
    list.addEventListener("dragend", clear);
    list.addEventListener("click", (event) => {
      const button = event.target.closest("[data-order-step]");
      if (!button) return;
      event.preventDefault();
      event.stopPropagation();
      const id = button.closest("label").dataset.moduleRow;
      const index = settings.order.indexOf(id);
      const target = index + (button.dataset.orderStep === "up" ? -1 : 1);
      if (target < 0 || target >= settings.order.length) return;
      const order = [...settings.order];
      [order[index], order[target]] = [order[target], order[index]];
      update({ order });
      button.focus({ preventScroll: true });
    });
  }

  function mount() {
    if (mounted) return;
    mounted = true;
    $("#mascot-pet").innerHTML = CROCODILE;
    $("#mascot-preview").innerHTML = CROCODILE;
    enhanceOrderEditor();
    applyOrder();
    syncControls();
    document.addEventListener("click", (event) => {
      for (const [selector, key, attribute] of [["[data-theme-choice]", "theme", "themeChoice"], ["[data-shape-choice]", "shape", "shapeChoice"], ["[data-sidebar-choice]", "sidebar", "sidebarChoice"], ["[data-mascot-place]", "mascotPlace", "mascotPlace"], ["[data-weight-choice]", "weight", "weightChoice"]]) {
        const button = event.target.closest(selector);
        if (button) update({ [key]: button.dataset[attribute] });
      }
    });
    $("#ui-scale").addEventListener("input", (event) => update({ scale: Number(event.target.value) }));
    $("#ui-font").addEventListener("change", (event) => update({ font: event.target.value }));
    $("#ui-contrast").addEventListener("change", (event) => update({ contrast: event.target.checked ? "high" : "normal" }));
    for (const [id, key] of [["ui-motion", "motion"], ["ui-glow", "glow"], ["ui-mascot", "mascot"], ["ui-mascot-reactions", "reactions"]]) $("#" + id).addEventListener("change", (event) => {
      update({ [key]: event.target.checked });
      if (key === "reactions" || key === "mascot") settle();
    });
    $("#reset-appearance").addEventListener("click", () => {
      update({ theme: DEFAULTS.theme, scale: DEFAULTS.scale, shape: DEFAULTS.shape, font: DEFAULTS.font, weight: DEFAULTS.weight, contrast: DEFAULTS.contrast, motion: DEFAULTS.motion, glow: DEFAULTS.glow });
      $("[data-accent='mint']").click();
    });
    $("#reset-module-order").addEventListener("click", () => update({ order: MODULES }));
    $("#mascot-pet").addEventListener("click", () => react("pet"));
    $("#mascot-pet").addEventListener("pointermove", (event) => {
      if (!motionEnabled() || eyeFrame) return;
      const box = event.currentTarget.getBoundingClientRect();
      const x = Math.max(-2, Math.min(2, ((event.clientX - box.left) / box.width - .5) * 4));
      const y = Math.max(-1.5, Math.min(1.5, ((event.clientY - box.top) / box.height - .5) * 3));
      eyeFrame = requestAnimationFrame(() => {
        $("#mascot-pet .croc-pupils").style.transform = `translate(${x}px, ${y}px)`;
        eyeFrame = null;
      });
    });
    $("#mascot-pet").addEventListener("pointerleave", () => { $("#mascot-pet .croc-pupils").style.transform = ""; });
    window.addEventListener("cgs:activity", (event) => react(event.detail?.type));
    window.addEventListener("cgs:busy-changed", (event) => {
      const { id, busy } = event.detail || {};
      if (!id) return;
      if (busy) tasks.add(id); else tasks.delete(id);
      if (settings.reactions && settings.mascot && (busy || tasks.size || mascotState === "busy")) settle();
    });
    const activityVisibility = () => root.classList.toggle("ui-background", document.hidden || !document.hasFocus());
    document.addEventListener("visibilitychange", activityVisibility);
    window.addEventListener("focus", activityVisibility);
    window.addEventListener("blur", activityVisibility);
    activityVisibility();
    window.addEventListener("cgs:language-changed", () => { applyOrder(); syncControls(); });
    reducedMotion.addEventListener("change", applyRoot);
  }

  window.cgsPersonalization = { get: () => ({ ...settings, order: [...settings.order] }), set: update, motionEnabled, react };
  document.addEventListener("DOMContentLoaded", mount);
})();
