(() => {
  "use strict";
  const invoke = window.__TAURI__?.core?.invoke;
  let opened = false;
  const checks = new Map();
  let generation = 0;
  function invalidateChecks() { ++generation; checks.clear(); }
  async function check({steamPath = null, force = false} = {}) {
    if (!invoke || document.body.dataset.platform === 'linux') return null;
    const token = ++generation;
    const key = String(steamPath || '') + ':' + force;
    const banner = document.querySelector('#loader-update-banner');
    let request = checks.get(key);
    if (!request) {
      request = invoke('luatools_check_loader_updates', {steamPath, force}).finally(() => { if (checks.get(key) === request) checks.delete(key); });
      checks.set(key, request);
    }
    try {
      const info = await request;
      if (!Array.isArray(info?.variants) || info.variants.length !== 2) throw new Error('Не удалось проверить версии DLL на GitHub');
      if (token === generation && banner) {
        banner.classList.remove('error');
        banner.classList.toggle('hidden', !info.updateAvailable);
        document.querySelector('#loader-update-title').textContent = 'Доступно обновление DLL';
        document.querySelector('#loader-update-message').textContent = 'Обновите загрузчик для корректной работы CrocLua в Steam. Автоматической замены файлов не будет.';
        const action = document.querySelector('#loader-update-action');
        action.textContent = 'Обновить DLL';
        action.onclick = () => document.querySelector('#lua-update-dll-button').click();
      }
      return info;
    } catch (error) {
      if (token === generation && banner) {
        banner.classList.remove('hidden'); banner.classList.add('error');
        document.querySelector('#loader-update-title').textContent = 'Проверка DLL недоступна';
        document.querySelector('#loader-update-message').textContent = String(error?.message || error) + ' Установленные DLL не изменены.';
        const action = document.querySelector('#loader-update-action');
        action.textContent = 'Повторить'; action.onclick = () => check({steamPath, force:true}).catch(() => {});
      }
      throw error;
    }
  }
  const displayPath = value => String(value || '').replace(/^\\\\\?\\UNC\\/i, '\\\\').replace(/^\\\\\?\\/, '');
  const fileIcon = '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5 2h7l4 4v12H5zM12 2v5h4"/></svg>';

  async function open({steamPath}) {
    if (opened || !invoke) return null;
    opened = true;
    const previousFocus = document.activeElement;
    const overlay = document.createElement('div');
    overlay.className = 'loader-backdrop';
    overlay.innerHTML = `<section class="premium-panel loader-dialog" role="dialog" aria-modal="true" aria-labelledby="loader-title">
      <span class="hero-kicker">MODULE 04 · CROCLUA</span><h3 id="loader-title">Загрузчик OpenSteamTool</h3>
      <p class="loader-description">Выберите комплект DLL для своего Steam. Актуальные файлы будут получены из GitHub.</p>
      <div class="loader-path"><strong>Папка Steam</strong><span id="loader-steam-path"></span></div>
      <fieldset class="loader-choices" disabled><legend>Версия файлов</legend>
        <label class="loader-choice"><input type="radio" name="loader-variant" value="standard" checked/><span><strong>Версия 1 · Текущая</strong><span class="loader-bundle-badge" hidden></span><small>Исходный комплект CrocLua. Если всё работает — оставьте его.</small></span></label>
        <label class="loader-choice"><input type="radio" name="loader-variant" value="compatibility"/><span><strong>Версия 2 · Альтернативная</strong><span class="loader-bundle-badge" hidden></span><small>Другой комплект OpenSteamTool для тех, у кого не работает первый.</small></span></label>
      </fieldset>
      <div class="loader-files"><header><span>Файлы выбранного комплекта</span><small>GitHub</small></header><div class="loader-file-list"></div></div>
      <p class="loader-notification-note">version.dll устанавливается, но не влияет на уведомления об обновлении.</p>
      <div class="loader-protection"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m12 3 8 3v6c0 4-4 7-8 9-4-2-8-5-8-9V6zM8 12l3 3 5-6"/></svg><span>Выбор закрепляется в папке Steam. Если Crocodile Manifest установлен, он тоже будет обновлён до 0.5.2. Добавление манифестов не переключит версию DLL.</span></div>
      <details class="personalization-details loader-instructions"><summary>Как установить и что важно знать</summary><ol><li>Полностью закройте Steam через значок в трее → «Выход».</li><li>Выберите версию и нажмите «Установить и закрепить».</li><li>У standard все DLL идут в корень Steam. У compatibility файл dwmapi.dll идёт в Steam/bin, остальные DLL — в корень.</li><li>Запустите Steam заново. Lua-файлы останутся в Steam/config/lua.</li><li>Старые DLL сохраняются в Steam/config/cgs-luatools-backups. Совпадающие DLL не скачиваются повторно и не переписываются.</li><li>Плагин 0.5.2+ учитывает новые пути и кэш скачанных DLL. Защита не управляет старыми плагинами, сторонними программами и ручной заменой файлов.</li></ol></details>
      <div class="loader-status" role="status" aria-live="polite">Проверяем установленный комплект…</div>
      <div class="loader-footer"><button type="button" class="secondary-button" id="loader-cancel">Отмена</button><button type="button" class="primary-button" id="loader-install" disabled>Установить и закрепить</button></div>
    </section>`;
    const $ = selector => overlay.querySelector(selector);
    let busy = false, done = false, closed = false, result = null, remote = null;
    function showBadges() {
      for (const choice of overlay.querySelectorAll('.loader-choice')) {
        const variant = choice.querySelector('input').value;
        const item = remote?.variants?.find(item => item.variant === variant);
        const badge = choice.querySelector('.loader-bundle-badge');
        badge.hidden = !item;
        const state = item?.status || (variant === remote?.selectedVariant ? (item?.updateAvailable ? 'update' : 'current') : 'not_installed');
        badge.dataset.status = state;
        badge.textContent = ({update:'Обновление',current:'Актуально',not_installed:'Другой комплект'})[state] || 'Другой комплект';
      }
    }
    function showFiles() {
      const variant = $('input[name="loader-variant"]:checked')?.value;
      const files = remote?.variants?.find(item => item.variant === variant)?.files || [];
      const list = $('.loader-file-list'); list.replaceChildren();
      for (const file of files) {
        const row = document.createElement('div'); row.innerHTML = fileIcon;
        const name = document.createElement('span'); name.className = 'loader-file-name'; name.textContent = String(file.path).replaceAll('/', '\\'); row.appendChild(name);
        const label = ({changed:'Обновление',missing:'Нет файла',current:'Совпадает',ignored:'Без уведомлений'})[file.status];
        if (label) { const badge = document.createElement('small'); badge.className = 'loader-file-state'; badge.dataset.status = file.status; badge.textContent = label; row.appendChild(badge); }
        list.appendChild(row);
      }
    }
    $('.loader-choices').addEventListener('change', showFiles);
    let finish;
    const completed = new Promise(resolve => { finish = resolve; });
    const blocked = [...document.querySelectorAll('.main-area, .sidebar')].map(element => ({element, inert:element.inert}));
    blocked.forEach(({element}) => { element.inert = true; });
    function status(text, kind = '') { $('.loader-status').textContent = text; $('.loader-status').className = `loader-status ${kind}`; }
    function close() {
      if (busy || closed) return;
      closed = true;
      document.removeEventListener('keydown', keydown, true);
      blocked.forEach(({element, inert}) => { element.inert = inert; });
      overlay.remove();
      previousFocus?.focus({preventScroll:true});
      opened = false;
      finish(result);
    }
    function keydown(event) {
      if (event.key === 'Escape') { event.preventDefault(); event.stopImmediatePropagation(); close(); }
      if (event.key === 'Tab') {
        const targets = [...overlay.querySelectorAll('button:not(:disabled), input:not(:disabled), summary')].filter(el => !el.closest('fieldset:disabled') && el.getClientRects().length);
        event.preventDefault();
        const index = targets.indexOf(document.activeElement);
        const next = event.shiftKey ? (index <= 0 ? targets.length - 1 : index - 1) : (index + 1) % targets.length;
        targets[next]?.focus();
      }
    }
    $('#loader-cancel').addEventListener('click', close);
    $('#loader-install').addEventListener('click', async () => {
      if (busy) return;
      if (done) { close(); return; }
      const variant = $('input[name="loader-variant"]:checked')?.value;
      if (!['standard','compatibility'].includes(variant)) return;
      busy = true;
      $('.loader-dialog').setAttribute('aria-busy', 'true');
      $('.loader-choices').disabled = true;
      $('#loader-install').disabled = true;
      $('#loader-cancel').disabled = true;
      status('Скачиваем и проверяем комплект GitHub…');
      window.dispatchEvent(new CustomEvent('cgs:busy-changed', {detail:{id:'loader', busy:true}}));
      try {
        const response = await invoke('luatools_update_loader', {steamPath, variant});
        if (!response?.success) throw new Error(response?.message || 'Не удалось обновить DLL');
        result = response;
        done = true;
        invalidateChecks();
        const refreshed = await check({steamPath}).catch(() => null);
        if (refreshed) { remote = refreshed; showBadges(); showFiles(); }
        status(response.message, 'success');
        $('#loader-install').textContent = 'Готово';
        $('#loader-cancel').hidden = true;
      } catch (error) {
        status(String(error?.message || error), 'error');
        $('.loader-choices').disabled = false;
      } finally {
        busy = false;
        $('.loader-dialog').setAttribute('aria-busy', 'false');
        $('#loader-install').disabled = false;
        $('#loader-cancel').disabled = false;
        $('#loader-install').focus();
        window.dispatchEvent(new CustomEvent('cgs:busy-changed', {detail:{id:'loader', busy:false}}));
      }
    });
    document.body.appendChild(overlay);
    $('#loader-steam-path').textContent = displayPath(steamPath);
    document.addEventListener('keydown', keydown, true);
    $('#loader-cancel').focus();
    try {
      const info = await invoke('luatools_loader_info', {steamPath});
      if (!closed) {
        steamPath = info.steamPath || steamPath;
        $('#loader-steam-path').textContent = displayPath(steamPath);
        $(`input[value="${info.selectedVariant === 'compatibility' ? 'compatibility' : 'standard'}"]`).checked = true;
        status('Проверяем версии DLL на GitHub…');
        remote = await check({steamPath});
        if (closed) return completed;
        showFiles();
        showBadges();
        $('.loader-choices').disabled = false;
        $('#loader-install').disabled = false;
        status(info.issue || (info.pinned
          ? (remote.variants.find(item => item.variant === remote.selectedVariant)?.status === 'current' ? 'Версия закреплена. Сменить её можно только вручную в этом окне.' : 'Версия закреплена, но DLL отсутствуют или отличаются. Установите выбранный комплект заново.')
          : 'После установки выбранная версия будет закреплена.'), info.issue ? 'error' : '');
      }
    } catch (error) { if (!closed) status(String(error?.message || error), 'error'); }
    return completed;
  }
  window.cgsLoaderDialog = {open, check: options => check(options).catch(() => null)};
})();
