(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const invoke = window.__TAURI__?.core?.invoke;
  let busy = false, loaded = false, result = null, picked = '', sequence = 0;
  const english = () => window.cgsI18n?.language === 'en';
  const t = (ru, en) => english() ? en : ru;
  const steamPath = () => $('lua-steam-path')?.value.trim() || localStorage.getItem('luaSteamPath') || null;
  function text(id, ru, en) { $(id).textContent = t(ru, en); }
  function render() {
    text('lc-title', 'Локальное облако', 'Local Cloud');
    text('lc-info-label', 'Информация', 'Information');
    text('lc-toggle-label', result?.enabled ? 'Включено' : 'Включить', result?.enabled ? 'Enabled' : 'Enable');
    text('lc-description', 'Сохранения игр с Lua и манифестами обрабатываются как облачные, но хранятся на вашем диске. CloudRedirect перенаправляет запросы Steam Cloud в локальную папку — без Google Drive, OneDrive и регистрации. Обычные игры Steam продолжают использовать своё облако.', 'Saves for Lua games with manifests use the cloud interface while staying on your disk. CloudRedirect routes Steam Cloud requests to a local folder, without Google Drive, OneDrive or registration. Regular Steam games keep their usual cloud storage.');
    text('lc-note', 'После изменения режима Steam перезапустится автоматически. Сохранения сохраняются после отключения. Настроенный режим работает вместе со Steam, даже когда эта программа закрыта. Совместимость зависит от игры и версии Steam.', 'Steam restarts automatically when the mode changes. Disabling keeps your saved files. Once configured, the mode runs with Steam even when this app is closed. Compatibility depends on the game and Steam version.');
    text('lc-browse', 'Выбрать папку', 'Choose folder'); text('lc-refresh', 'Проверить', 'Check');
    $('lc-folder').placeholder = t('Выберите папку хранения', 'Choose a storage folder');
    $('lc-folder').setAttribute('aria-label', t('Папка локального облака', 'Local cloud folder'));
    $('lc-folder').value = picked || result?.folder || '';
    $('lc-enabled').checked = !!result?.enabled;
    $('lc-enabled').disabled = busy || !loaded || (!result?.enabled && !result?.loaderSupported);
    $('lc-browse').disabled = busy || !loaded;
    $('lc-refresh').disabled = busy;
    $('local-cloud-panel').dataset.busy = String(busy);
  }
  function message(value, error = false) { $('lc-status').textContent = value; $('lc-status').dataset.kind = error ? 'error' : 'info'; }
  function statusMessage() {
    if (result?.issue) return message(result.issue, true);
    const count = result?.eligibleApps?.length || 0;
    message(result?.enabled
      ? t(`Локальный режим включён · игр с манифестами: ${count}.`, `Local mode enabled · manifest games: ${count}.`)
      : t('Локальный режим выключен. Выберите папку и включите переключатель.', 'Local mode is off. Choose a folder and enable the switch.'));
  }
  async function refresh() {
    if (!invoke || busy) return;
    const ticket = ++sequence;
    try {
      const data = await invoke('local_cloud_status', {steamPath:steamPath()});
      if (ticket !== sequence || busy) return;
      result = data; loaded = true; picked = ''; render(); statusMessage();
    } catch (error) { if(ticket === sequence) { loaded = false; render(); message(String(error), true); } }
  }
  async function apply(enabled, folder) {
    if (busy) return;
    busy = true; ++sequence; render();
    message(t('Применяем локальное облако: закрываем Steam, сохраняем настройки и запускаем клиент…', 'Applying local cloud: closing Steam, saving settings and restarting the client…'));
    try {
      result = await invoke('local_cloud_set', {steamPath:steamPath(),enabled,folder});
      picked = ''; loaded = true; statusMessage();
    } catch(error) { message(String(error), true); }
    finally { busy = false; render(); }
  }
  $('lc-enabled').addEventListener('change', async (event) => {
    const enabled = event.target.checked;
    $('lc-enabled').checked = !!result?.enabled;
    let folder = picked || result?.folder;
    if(enabled && !folder) {
      busy = true; render();
      try { folder = await invoke('pick_game_directory'); }
      catch(error) { message(String(error), true); }
      finally { busy = false; render(); }
      if(!folder) return;
    }
    await apply(enabled, folder || null);
  });
  $('lc-browse').addEventListener('click', async () => {
    if (busy) return;
    busy = true; render();
    let folder;
    try { folder = await invoke('pick_game_directory'); }
    catch(error) { message(String(error), true); }
    finally { busy = false; render(); }
    if (!folder) return;
    if (result?.enabled) await apply(true, folder);
    else { picked = folder; render(); message(t('Папка выбрана. Включите локальное облако.', 'Folder selected. Enable local cloud to apply it.')); }
  });
  $('lc-refresh').addEventListener('click', refresh);
  $('lc-info').addEventListener('click', () => {
    const expanded = $('lc-info').getAttribute('aria-expanded') !== 'true';
    $('lc-info').setAttribute('aria-expanded', String(expanded));
    $('lc-help').hidden = !expanded;
  });
  $('lua-steam-path').addEventListener('change', refresh);
  window.addEventListener('cgs:language-changed', () => { render(); if(!busy) statusMessage(); });
  const observer = new MutationObserver(() => { if($('view-lua').classList.contains('active')) refresh(); });
  observer.observe($('view-lua'), {attributes:true,attributeFilter:['class']});
  window.cgsLocalCloud = {refresh};
  render();
  if ($('view-lua').classList.contains('active')) refresh();
})();
