(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const invoke = (name, args = {}) => window.__TAURI__?.core?.invoke(name, args) ?? Promise.reject(new Error('Native module unavailable'));
  const t = (ru, en) => window.cgsI18n?.language === 'ru' ? ru : en;
  const nativeText = value => window.cgsI18n?.t(String(value ?? '')) || String(value ?? '');
  const languageDialog = $('first-language-dialog');
  let finishLanguage;
  window.cgsLanguageReady = new Promise(resolve => { finishLanguage = resolve; });
  let savedLanguage = '';
  try { savedLanguage = localStorage.getItem('cgs-language'); } catch (_) {}
  if (savedLanguage === 'ru' || savedLanguage === 'en') finishLanguage();
  else {
    languageDialog.addEventListener('cancel', event => event.preventDefault());
    languageDialog.querySelectorAll('[data-first-language]').forEach(button => {
      button.addEventListener('click', () => {
        const language = button.dataset.firstLanguage;
        try {
          localStorage.setItem('cgs-language', language);
          window.cgsI18n.setLanguage(language);
          languageDialog.close(); finishLanguage();
        } catch (_) {
          $('first-language-error').textContent = 'Could not save your choice. / Не удалось сохранить выбор.';
        }
      });
    });
    languageDialog.showModal();
  }

  const dialog = $('hv-protection-dialog');
  const accept = $('hv-protection-accept');
  const later = $('hv-protection-later');
  const consent = $('hv-protection-consent');
  const resumeKey = 'cgs-hv-protection-reopen';
  let report = null, checking = false, busy = false, applied = false, retry = false, details = '', resolveDialog = null;

  function activeProtections(value) {
    const active = (value.components || []).filter(item => item.enabled === true || item.locked === true || ['enabled', 'locked', 'configured'].includes(item.state));
    const labels = active.map(item => item.name || item.id);
    if (value.hypervisorPresent === true && !active.some(item => item.id === 'vbs')) labels.push('Windows Hypervisor');
    return [...new Set(labels)];
  }
  function verified(value) {
    return Array.isArray(value?.components) && value.components.length >= 3 &&
      value.components.every(item => item.state !== 'unknown');
  }
  function status(value, error = false) {
    details = value; $('hv-protection-status').textContent = value;
    $('hv-protection-status').dataset.kind = error ? 'error' : 'info';
  }
  function render() {
    $('hv-protection-title').textContent = retry ? t('Проверка защит недоступна', 'Protection check unavailable') : applied ? t('Настройки применены', 'Settings applied') : t('Защиты Windows включены', 'Windows protections are enabled');
    $('hv-protection-description').textContent = t('Игра уже добавлена, ярлык сохранён. Включены защиты Windows, которые могут мешать работе Crocodile-HV. Отключите их перед запуском прямо здесь, без перехода в другие вкладки.', 'Your game is added and its shortcut is saved. Windows protections that may prevent Crocodile-HV from working are enabled. Disable them here before launching, without opening another tab.');
    $('hv-protection-once').textContent = t('Это общая настройка компьютера: обычно она выполняется один раз, а не для каждой игры. Повтор потребуется, только если защиты снова включатся.', 'This is a computer-wide setup, usually done once rather than for every game. You only need to repeat it if the protections are enabled again.');
    $('hv-protection-restart-note').textContent = t('Сохраните открытые файлы и закройте игры. Кнопка отключит защиты и перезагрузит Windows. Исходные настройки сохраняются для восстановления. Может потребоваться подтверждение UAC.', 'Save your open files and close your games. The button disables protections and restarts Windows. Original settings are backed up for recovery. Windows may request UAC approval.');
    $('hv-protection-consent-wrap').hidden = !report?.needsUefiConsent || applied || retry;
    $('hv-protection-consent-text').textContent = t('Разрешаю подготовку снятия UEFI-блокировок. При загрузке потребуется моё физическое подтверждение.', 'Allow preparation to remove UEFI locks. I understand that physical confirmation is required at boot.');
    $('hv-protection-list').replaceChildren();
    if (report) for (const label of activeProtections(report)) {
      const li = document.createElement('li'); li.textContent = nativeText(label); $('hv-protection-list').append(li);
    }
    $('hv-protection-warnings').textContent = (report?.warnings || []).map(nativeText).join('\n');
    later.textContent = t('Позже', 'Later');
    accept.textContent = busy ? t('Подготавливаем…', 'Preparing…') : retry ? t('Проверить снова', 'Check again') : applied ? t('Перезагрузить Windows', 'Restart Windows') : t('Отключить защиты и перезагрузить', 'Disable protections & restart');
    later.disabled = busy; consent.disabled = busy;
    accept.disabled = busy || (!retry && !applied && report?.needsUefiConsent === true && !consent.checked);
    dialog.setAttribute('aria-busy', String(busy));
  }
  function close() {
    if (busy) return;
    dialog.close(); const done = resolveDialog; resolveDialog = null; done?.();
  }
  later.addEventListener('click', close);
  dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
  consent.addEventListener('change', render);

  async function scan() {
    report = await invoke('vbs_scan');
    if (!verified(report)) throw new Error(t('Windows не вернула полное состояние защит. Изменения не выполнялись. Повторите проверку или отложите настройку.', 'Windows did not return a complete protection status. No changes were made. Retry the check or set this up later.'));
    return activeProtections(report).length > 0;
  }
  async function restart() {
    status(t('Защиты подготовлены. Перезагружаем Windows…', 'Protections are prepared. Restarting Windows…'));
    await invoke('vbs_restart');
  }
  accept.addEventListener('click', async () => {
    if (busy || accept.disabled) return;
    busy = true; render(); window.cgsReportBusy?.('hv-protection', true);
    try {
      if (applied) { await restart(); return; }
      status(t('Проверяем текущее состояние защит…', 'Checking the current protection status…'));
      if (!(await scan())) { busy = false; close(); return; }
      if (retry) { retry = false; status(''); return; }
      if (report.needsUefiConsent && !consent.checked) return;
      if (!(await invoke('process_is_elevated'))) {
        // Only reopen this window after UAC. Stored UI state never authorizes system changes.
        localStorage.setItem(resumeKey, String(Date.now()));
        localStorage.setItem('cgs-admin-reopen-view', 'hypervisor');
        status(t('Запрашиваем права Windows. После подтверждения откроется это же окно.', 'Requesting Windows privileges. This same window will reopen after approval.'));
        try { await invoke('relaunch_as_admin'); }
        catch (error) { localStorage.removeItem(resumeKey); localStorage.removeItem('cgs-admin-reopen-view'); throw error; }
        await invoke('close_window');
        return;
      }
      $('hv-protection-progress').textContent = ''; $('hv-protection-progress').hidden = false;
      status(t('Отключаем защиты и сохраняем исходные настройки…', 'Disabling protections and backing up original settings…'));
      const result = await invoke('hv_disable_protections', { allowUefi: report.needsUefiConsent === true && consent.checked });
      if (!result?.success || result.failed?.length) {
        throw new Error([result?.message || t('Подготовка не завершена.', 'Preparation was not completed.'), ...(result?.failed || [])].map(nativeText).join('\n'));
      }
      applied = true;
      await restart();
    } catch (error) {
      status(nativeText(error?.message || error), true);
    } finally {
      busy = false; window.cgsReportBusy?.('hv-protection', false); render();
    }
  });
  window.__TAURI__?.event?.listen('vbs-progress', event => {
    if (!busy || !dialog.open) return;
    const log = $('hv-protection-progress');
    log.textContent = (log.textContent + '\n' + nativeText(event.payload ?? event)).split('\n').slice(-8).join('\n').trim();
    log.scrollTop = log.scrollHeight;
  }).catch(() => {});
  window.addEventListener('cgs:language-changed', () => { render(); if (details) $('hv-protection-status').textContent = nativeText(details); });

  window.cgsHvProtectionCheck = async () => {
    if (checking || dialog.open) return;
    checking = true;
    try {
      await window.cgsLanguageReady;
      // Avoid replacing a Local Cloud, update or administrator notification.
      while (document.querySelector('dialog[open], .modal-backdrop:not(.hidden)')) await new Promise(resolve => setTimeout(resolve, 150));
      report = null; applied = retry = false; consent.checked = false; status('');
      $('hv-protection-progress').hidden = true; $('hv-protection-progress').textContent = '';
      try { if (!(await scan())) return; }
      catch (error) { retry = true; status(nativeText(error?.message || error), true); }
      render();
      const closed = new Promise(resolve => { resolveDialog = resolve; });
      dialog.showModal(); later.focus();
      await closed;
    } finally { checking = false; }
  };
  window.cgsHvProtectionResume = async () => {
    let timestamp = 0;
    try { timestamp = Number(localStorage.getItem(resumeKey)); localStorage.removeItem(resumeKey); } catch (_) {}
    if (timestamp > 0 && Date.now() - timestamp >= 0 && Date.now() - timestamp < 120000) await window.cgsHvProtectionCheck();
  };
  render();
})();
