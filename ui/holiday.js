(() => {
  "use strict";
  const root = document.documentElement;
  const badge = document.createElement("div");
  badge.className = "holiday-season";
  badge.setAttribute("data-i18n-skip", "");
  badge.innerHTML = '<svg viewBox="0 0 132 48" aria-hidden="true"><path class="holiday-wire" d="M0 9Q66 39 132 9"/>' +
    [12, 39, 66, 93, 120].map((x, i) => {
      const y = 13 + (2 - Math.abs(2 - i)) * 5;
      return `<path class="holiday-wire" d="M${x} ${y}v6"/><ellipse class="holiday-bulb" cx="${x}" cy="${y + 10}" rx="3" ry="4"/>`;
    }).join("") + '</svg><span></span>';
  document.querySelector(".page-topbar")?.append(badge);
  function translate() {
    const now = new Date();
    const year = now.getMonth() === 0 ? now.getFullYear() : now.getFullYear() + 1;
    badge.querySelector("span").textContent = `${window.cgsI18n?.language === "ru" ? "Новогодний сезон" : "Winter edition"} · ${year}`;
  }
  translate();
  window.addEventListener("cgs:language-changed", translate);

  const scene = document.createElement("div");
  scene.className = "holiday-hero-scene";
  scene.setAttribute("aria-hidden", "true");
  scene.innerHTML = `<svg class="holiday-pine" viewBox="0 0 350 170"><defs><g id="holiday-sprig"><path d="M0 0Q65 28 140 14"/><path d="M20 8 9 25M30 12 19 33M42 16 31 40M54 19 46 45M67 20 62 47M80 20 80 43M94 19 98 39M108 17 116 34M122 16 132 29M20 8 23-7M32 12 39-6M45 16 55-4M58 19 72-1M73 20 88 1M88 20 104 3M104 18 120 4M120 16 136 5"/></g></defs><g class="holiday-needles"><use href="#holiday-sprig" transform="translate(0 16) rotate(10) scale(1.7)"/><use href="#holiday-sprig" transform="translate(8 0) rotate(43) scale(1.2)"/><use href="#holiday-sprig" transform="translate(70 3) rotate(-5) scale(1.6)"/></g><g class="holiday-berries"><circle cx="65" cy="48" r="4"/><circle cx="73" cy="54" r="4"/><circle cx="64" cy="58" r="3"/><circle cx="168" cy="38" r="3"/><circle cx="175" cy="43" r="4"/></g></svg>
    <svg class="holiday-ornaments" viewBox="0 0 180 230"><defs><radialGradient id="holiday-gold" cx="30%" cy="25%"><stop stop-color="#f4dfad"/><stop offset=".55" stop-color="#b39154"/><stop offset="1" stop-color="#635033"/></radialGradient><radialGradient id="holiday-accent" cx="30%" cy="25%"><stop stop-color="#d9f9ef"/><stop offset=".45" stop-color="var(--accent)"/><stop offset="1" stop-color="#19483e"/></radialGradient></defs><g class="holiday-ornament holiday-ornament-one"><path d="M50 0V87"/><rect x="45" y="84" width="10" height="7" rx="2"/><circle cx="50" cy="113" r="24" fill="url(#holiday-gold)"/><path class="holiday-ball-detail" d="M29 104Q50 119 71 104M28 115Q50 130 72 115"/></g><g class="holiday-ornament holiday-ornament-two"><path d="M121 0V136"/><rect x="116" y="133" width="10" height="7" rx="2"/><circle cx="121" cy="166" r="30" fill="url(#holiday-accent)"/><path class="holiday-ball-detail" d="m121 149 4 12 12 5-12 4-4 13-4-13-12-4 12-5Z"/></g><g class="holiday-star"><path d="M160 33v26m-13-13h26m-22-9 18 18m0-18-18 18"/></g></svg>`;
  document.querySelector(".home-hero")?.append(scene);

  const canvas = document.createElement("canvas");
  canvas.className = "holiday-snow";
  canvas.setAttribute("aria-hidden", "true");
  document.body.append(canvas);
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const reduced = matchMedia("(prefers-reduced-motion: reduce)");
  let frame = 0, previous = 0, width = 1, height = 1, ink = "#c8dbe9";
  const flakes = Array.from({length: 58}, () => ({x: Math.random(), y: Math.random(), r: .7 + Math.random() * 1.6, speed: 6 + Math.random() * 14, phase: Math.random() * Math.PI * 2}));
  function resize() {
    width = canvas.clientWidth || 1;
    height = canvas.clientHeight || 1;
    const ratio = Math.min(devicePixelRatio || 1, 1.5);
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ink = root.dataset.uiTheme === "white" ? "#517382" : "#c8dbe9";
  }
  function enabled() {
    return root.dataset.uiHoliday === "on" && root.dataset.uiMotion === "on" && root.dataset.uiContrast !== "high" && !reduced.matches && !document.hidden;
  }
  function draw(time) {
    frame = 0;
    if (!enabled()) return;
    frame = requestAnimationFrame(draw);
    if (time - previous < 40) return;
    const delta = previous ? Math.min((time - previous) / 1000, .1) : 0;
    previous = time;
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = ink;
    for (const f of flakes) {
      f.y = (f.y + f.speed * delta / height) % 1;
      ctx.globalAlpha = .2 + f.r * .12;
      ctx.beginPath();
      ctx.arc(f.x * width + Math.sin(time / 6000 + f.phase) * 12, f.y * height, f.r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  function sync() {
    cancelAnimationFrame(frame);
    frame = 0;
    previous = 0;
    resize();
    ctx.clearRect(0, 0, width, height);
    if (enabled()) frame = requestAnimationFrame(draw);
  }
  new MutationObserver(sync).observe(root, {attributes: true, attributeFilter: ["data-ui-holiday", "data-ui-motion", "data-ui-contrast", "data-ui-theme", "style"]});
  window.addEventListener("resize", sync);
  document.addEventListener("visibilitychange", sync);
  reduced.addEventListener("change", sync);
  sync();
})();
