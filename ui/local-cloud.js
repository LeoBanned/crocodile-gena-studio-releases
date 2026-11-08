(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const invoke = window.__TAURI__?.core?.invoke;
  let busy = false, loaded = false, result = null, picked = '', sequence = 0;
  const english = () => window.cgsI18n?.language === 'en';
  const t = (ru, en) => english() ? en : ru;
  const steamPath = () => $('lua-steam-path')?.value.trim() || localStorage.getItem('luaSteamPath') || null;
  const updateDialog = $('lc-update-dialog');
  const notified = new Set();
  let noticeQueued = false, updateBusy = false, updateDone = false, updatePath = null, previousFocus = null;
  function renderUpdateDialog() {
    text('lc-update-title', 'Нужно обновить DLL локального облака', 'Local Cloud DLL update required');
    text('lc-update-description', 'В программу встроен исправленный CloudRedirect. Обновите установленную DLL, чтобы синхронизация сохранений игр с Lua и манифестами работала корректно, в том числе после выхода и повторного запуска игры.', 'This app includes a repaired CloudRedirect component. Update the installed DLL for correct save synchronization in Lua and manifest games, including after exiting and reopening a game.');
    text('lc-update-note', 'Если Steam открыт, он перезапустится автоматически. Перед обновлением закройте игры. Прежняя DLL сохраняется в резервной копии; папка сохранений и выбранный режим останутся прежними. Скачивать файлы не требуется.', 'Steam will restart automatically if it is running. Close your games first. The previous DLL is backed up; your save folder and enabled/disabled setting are kept. No download is required.');
    text('lc-update-later', updateDone ? 'Закрыть' : 'Позже', updateDone ? 'Close' : 'Later');
    text('lc-update-install', updateBusy ? 'Обновляем…' : 'Обновить DLL', updateBusy ? 'Updating…' : 'Update DLL');
    $('lc-update-later').disabled = updateBusy;
    $('lc-update-install').disabled = updateBusy;
    $('lc-update-install').hidden = updateDone;
    $('lc-update-progress').hidden = !updateBusy;
    updateDialog.setAttribute('aria-busy', String(updateBusy));
  }
  function notifyUpdate(force = false) {
    const key = (result?.steamPath || result?.folder || 'configured-cloud').toLowerCase();
    if (!result?.componentUpdateAvailable || !result?.loaderSupported || busy || updateDialog.open || noticeQueued || (!force && notified.has(key))) return;
    noticeQueued = true;
    const openWhenReady = () => {
      if (!result?.componentUpdateAvailable || busy) { noticeQueued = false; return; }
      if (document.querySelector('.modal-backdrop:not(.hidden), dialog[open]')) {
        setTimeout(openWhenReady, 400); return;
      }
      noticeQueued = false; notified.add(key); updateDone = false; updatePath = result.steamPath || steamPath();
      previousFocus = document.activeElement;
      $('lc-update-status').textContent = '';
      renderUpdateDialog(); updateDialog.showModal(); $('lc-update-install').focus();
    };
    openWhenReady();
  }
  updateDialog.addEventListener('cancel', event => { if (updateBusy) event.preventDefault(); });
  updateDialog.addEventListener('keydown', event => {
    if (event.key !== 'Tab') return;
    const controls = [...updateDialog.querySelectorAll('button:not(:disabled):not([hidden])')];
    const index = controls.indexOf(document.activeElement);
    event.preventDefault();
    if (controls.length) controls[(index + (event.shiftKey ? -1 : 1) + controls.length) % controls.length].focus();
  });
  updateDialog.addEventListener('close', () => { if (previousFocus?.isConnected) previousFocus.focus(); });
  $('lc-update-later').addEventListener('click', () => { if (!updateBusy) updateDialog.close(); });
  $('lc-update-install').addEventListener('click', async () => {
    if (busy || updateBusy) return;
    busy = true; updateBusy = true; ++sequence; render(); renderUpdateDialog();
    const status = $('lc-update-status');
    status.dataset.kind = 'info';
    status.textContent = t('Закрываем Steam, сохраняем резервную копию и обновляем DLL…', 'Closing Steam, backing up the previous DLL and installing the update…');
    try {
      result = await invoke('local_cloud_update_component', {steamPath:updatePath});
      if (result.componentUpdateAvailable) throw new Error(t('DLL не обновлена. Повторите операцию.', 'The DLL was not updated. Please retry.'));
      loaded = true; updateDone = true;
      status.textContent = t('DLL обновлена. Если Steam был открыт, он запущен снова. Локальное облако готово к работе.', 'DLL updated. Steam has been restarted if it was running. The Local Cloud component is ready.');
      statusMessage();
    } catch(error) { status.textContent = String(error); status.dataset.kind = 'error'; }
    finally { busy = false; updateBusy = false; render(); renderUpdateDialog(); if (updateDone) $('lc-update-later').focus(); }
  });
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
  async function refresh(notify = false) {
    if (!invoke || busy) return;
    const ticket = ++sequence;
    try {
      const data = await invoke('local_cloud_status', {steamPath:steamPath()});
      if (ticket !== sequence || busy) return;
      result = data; loaded = true; picked = ''; render(); statusMessage(); notifyUpdate(notify === true);
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
  $('lc-refresh').addEventListener('click', () => refresh(true));
  $('lc-info').addEventListener('click', () => {
    const expanded = $('lc-info').getAttribute('aria-expanded') !== 'true';
    $('lc-info').setAttribute('aria-expanded', String(expanded));
    $('lc-help').hidden = !expanded;
  });
  $('lua-steam-path').addEventListener('change', refresh);
  window.addEventListener('cgs:language-changed', () => { render(); renderUpdateDialog(); if(!busy) statusMessage(); });
  const observer = new MutationObserver(() => { if($('view-lua').classList.contains('active')) refresh(); });
  observer.observe($('view-lua'), {attributes:true,attributeFilter:['class']});
  window.cgsLocalCloud = {refresh};
  render();
  refresh(); // Check the installed component at startup, even on the Home screen.
})();
