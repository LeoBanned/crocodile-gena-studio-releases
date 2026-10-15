(() => {
  "use strict";
  const invoke = window.__TAURI__?.core?.invoke;
  let opened = false;
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
      <p class="loader-description">Выберите комплект DLL для своего Steam. Если текущий не работает, попробуйте альтернативный.</p>
      <div class="loader-path"><strong>Папка Steam</strong><span id="loader-steam-path"></span></div>
      <fieldset class="loader-choices" disabled><legend>Версия файлов</legend>
        <label class="loader-choice"><input type="radio" name="loader-variant" value="standard" checked/><span><strong>Версия 1 · Текущая</strong><small>Исходный комплект CrocLua. Если всё работает — оставьте его.</small></span></label>
        <label class="loader-choice"><input type="radio" name="loader-variant" value="compatibility"/><span><strong>Версия 2 · Альтернативная</strong><small>Другой комплект OpenSteamTool для тех, у кого не работает первый.</small></span></label>
      </fieldset>
      <div class="loader-files"><header><span>Файлы в корне Steam</span><small>3 DLL</small></header><div>${fileIcon}OpenSteamTool.dll</div><div>${fileIcon}dwmapi.dll</div><div>${fileIcon}xinput1_4.dll</div></div>
      <div class="loader-protection"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m12 3 8 3v6c0 4-4 7-8 9-4-2-8-5-8-9V6zM8 12l3 3 5-6"/></svg><span>Выбор закрепляется в папке Steam. Если Crocodile Manifest установлен, он тоже будет обновлён до 0.5.1. Добавление манифестов не переключит версию DLL.</span></div>
      <details class="personalization-details loader-instructions"><summary>Как установить и что важно знать</summary><ol><li>Полностью закройте Steam через значок в трее → «Выход».</li><li>Выберите версию и нажмите «Установить и закрепить».</li><li>Запустите Steam заново. Lua-файлы останутся в Steam/config/lua.</li><li>Если игры не появились, закройте Steam и попробуйте другую версию в этом же окне.</li><li>Старые DLL сохраняются в Steam/config/cgs-luatools-backups. Уже совпадающие файлы не переписываются.</li><li>Обновите Crocodile Manifest до 0.5.1 или новее. Защита не управляет старыми плагинами, сторонними программами и ручной заменой файлов.</li></ol></details>
      <div class="loader-status" role="status" aria-live="polite">Проверяем установленный комплект…</div>
      <div class="loader-footer"><button type="button" class="secondary-button" id="loader-cancel">Отмена</button><button type="button" class="primary-button" id="loader-install" disabled>Установить и закрепить</button></div>
    </section>`;
    const $ = selector => overlay.querySelector(selector);
    let busy = false, done = false, closed = false, result = null;
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
      status('Устанавливаем выбранный комплект…');
      window.dispatchEvent(new CustomEvent('cgs:busy-changed', {detail:{id:'loader', busy:true}}));
      try {
        const response = await invoke('luatools_update_loader', {steamPath, variant});
        if (!response?.success) throw new Error(response?.message || 'Не удалось обновить DLL');
        result = response;
        done = true;
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
        $('.loader-choices').disabled = false;
        $('#loader-install').disabled = false;
        status(info.issue || (info.pinned
          ? (info.current ? 'Версия закреплена. Сменить её можно только вручную в этом окне.' : 'Версия закреплена, но DLL отсутствуют или отличаются. Установите выбранный комплект заново.')
          : 'После установки выбранная версия будет закреплена.'), info.issue ? 'error' : '');
      }
    } catch (error) { if (!closed) status(String(error?.message || error), 'error'); }
    return completed;
  }
  window.cgsLoaderDialog = {open};
})();
