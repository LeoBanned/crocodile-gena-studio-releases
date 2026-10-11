import { callable } from '@steambrew/webkit';

type HubcapUsage = {
  used: number;
  limit: number;
  remaining: number;
};

type InstallReply = {
  success: boolean;
  message: string;
  luaCount?: number;
  manifestCount?: number;
  service?: 'ryuu' | 'hubcap';
  configured?: boolean;
  hubcapUsage?: HubcapUsage;
  hubcapUsageError?: string;
  repaired?: boolean;
};

type GameStatusReply = InstallReply & {
  state?: 'available' | 'installed' | 'update_available' | 'repair_required' | 'pending_restart' | 'error' | 'configuration_required';
  cardMode?: 'full' | 'button' | 'crocodile';
  installed?: boolean;
  updateAvailable?: boolean;
  lastMessage?: string;
  lastTimestamp?: number;
  missingManifestCount?: number;
  restoredManifestCount?: number;
};

type ButtonState = 'idle' | 'loading' | 'success' | 'error' | 'installed' | 'pending';

const installManifest = callable<[{ appid: string; name: string }], string>('install_manifest');
const restartSteam = callable<[], string>('restart_steam');
const getGameStatus = callable<[{ appid: string }], string>('get_game_status');
const restoreManifestCache = callable<[], string>('restore_manifest_cache');

const PANEL_ID = 'crocodile-manifest-panel';
const STYLE_ID = 'crocodile-manifest-style';
const MODAL_ID = 'crocodile-manifest-modal';
let cacheRestoreRunning = false;

async function keepManifestsAvailable(): Promise<void> {
  if (cacheRestoreRunning) return;
  cacheRestoreRunning = true;
  try {
    await restoreManifestCache();
  } catch {
    // The next background pass retries silently.
  } finally {
    cacheRestoreRunning = false;
  }
}

function currentAppId(): string | null {
  return window.location.pathname.match(/\/app\/(\d+)/)?.[1] ?? null;
}

function currentGameName(): string {
  const heading = document.querySelector<HTMLElement>('#appHubAppName')?.textContent?.trim();
  if (heading) return heading;
  const title = document.querySelector<HTMLMetaElement>('meta[property="og:title"]')?.content?.trim();
  return title?.replace(/\s+on Steam$/i, '') || '';
}

function addStyles(): void {
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement('style');
  style.id = STYLE_ID;
  style.textContent = `
    #${PANEL_ID} {
      position: relative;
      isolation: isolate;
      overflow: hidden;
      margin: 0 0 16px;
      padding: 17px;
      border: 1px solid rgba(255, 110, 155, .18);
      border-radius: 11px;
      background:
        radial-gradient(circle at 0 0, rgba(255, 110, 155, .055), transparent 42%),
        linear-gradient(145deg, rgba(17, 20, 29, .98), rgba(9, 12, 18, .99));
      box-shadow:
        0 18px 42px rgba(0, 0, 0, .3),
        inset 0 1px 0 rgba(255, 255, 255, .035);
      font-family: "Motiva Sans", Arial, sans-serif;
    }
    #${PANEL_ID}::before {
      content: "";
      position: absolute;
      z-index: -1;
      inset: 0 auto auto 0;
      width: 100%;
      height: 1px;
      background: linear-gradient(90deg, transparent, rgba(255, 110, 155, .68), transparent);
      opacity: .7;
    }
    #${PANEL_ID}[data-mode="button"],
    #${PANEL_ID}[data-mode="crocodile"] {
      width: fit-content;
      padding: 0;
      border: 0;
      background: transparent;
      box-shadow: none;
    }
    #${PANEL_ID}[data-mode="button"] {
      min-width: 220px;
    }
    #${PANEL_ID}[data-mode="crocodile"] {
      display: flex;
      align-items: center;
      gap: 7px;
    }
    #${PANEL_ID}[data-mode="button"]::before,
    #${PANEL_ID}[data-mode="button"] .ryuu-header,
    #${PANEL_ID}[data-mode="crocodile"]::before,
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-header {
      display: none;
    }
    #${PANEL_ID}[data-mode="button"] .ryuu-button {
      width: auto;
      min-width: 220px;
      height: 38px;
      padding: 0 40px 0 14px;
    }
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-button {
      position: relative;
      display: grid;
      place-items: center;
      width: 42px;
      min-width: 42px;
      height: 42px;
      padding: 0;
      border-radius: 10px;
    }
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-button-label,
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-button-arrow,
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-button-icon svg,
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-button-icon::before {
      display: none;
    }
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-button-icon {
      position: absolute;
      inset: 0;
      display: grid;
      width: 100%;
      height: 100%;
      place-items: center;
    }
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-button-icon::after {
      content: "🐊";
      display: grid;
      place-items: center;
      font-family: "Segoe UI Emoji", "Apple Color Emoji", sans-serif;
      font-size: 21px;
      line-height: 1;
    }
    #${PANEL_ID} .ryuu-header {
      display: flex;
      align-items: center;
      gap: 11px;
      margin-bottom: 15px;
    }
    #${PANEL_ID} .ryuu-logo {
      display: grid;
      flex: 0 0 38px;
      width: 38px;
      height: 38px;
      place-items: center;
      border: 1px solid rgba(255, 110, 155, .24);
      border-radius: 10px;
      color: #fff;
      background: rgba(255, 110, 155, .075);
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, .055);
      font-family: "Segoe UI Emoji", "Apple Color Emoji", sans-serif;
      font-size: 21px;
      line-height: 1;
    }
    #${PANEL_ID} .ryuu-copy {
      min-width: 0;
      flex: 1;
    }
    #${PANEL_ID} .ryuu-title {
      margin: 0 0 3px;
      color: #f3f6f8;
      font-size: 15px;
      font-weight: 650;
      line-height: 1.2;
      letter-spacing: .1px;
    }
    #${PANEL_ID} .ryuu-subtitle {
      overflow: hidden;
      color: #8c96a7;
      font-size: 11.5px;
      line-height: 1.35;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    #${PANEL_ID} .ryuu-service-meta {
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: fit-content;
      max-width: 100%;
      gap: 5px;
      margin: -4px 0 9px auto;
      padding: 4px 7px;
      border: 1px solid rgba(255, 110, 155, .14);
      border-radius: 999px;
      color: #8c96a7;
      background: rgba(255, 110, 155, .04);
      font-size: 10px;
      line-height: 1;
      white-space: nowrap;
    }
    #${PANEL_ID} .ryuu-service-meta[hidden] {
      display: none;
    }
    #${PANEL_ID} .ryuu-service-name {
      color: #db9caf;
      font-weight: 700;
    }
    #${PANEL_ID} .ryuu-service-usage {
      color: #b4bac4;
      font-weight: 650;
    }
    #${PANEL_ID} .ryuu-service-meta[data-error="true"] {
      border-color: rgba(255, 139, 160, .2);
      color: #e7a2af;
      background: rgba(114, 39, 59, .16);
    }
    #${PANEL_ID}[data-mode="button"] .ryuu-service-meta {
      margin: 0 0 6px auto;
    }
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-service-meta {
      order: 2;
      margin: 0;
      padding: 4px 7px;
      white-space: nowrap;
    }
    #${PANEL_ID}[data-mode="crocodile"] .ryuu-service-name {
      display: none;
    }
    #${PANEL_ID} .ryuu-button {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 9px;
      overflow: hidden;
      width: 100%;
      height: 44px;
      padding: 0 16px;
      border: 1px solid rgba(255, 110, 155, .28);
      border-radius: 9px;
      color: #e9a8bc;
      background: rgba(255, 110, 155, .065);
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, .035);
      font: 650 14px/1 "Motiva Sans", Arial, sans-serif;
      letter-spacing: .1px;
      cursor: pointer;
      transition: color .16s ease, background .16s ease, border-color .16s ease, transform .16s ease;
    }
    #${PANEL_ID} .ryuu-button::before {
      display: none;
    }
    #${PANEL_ID} .ryuu-button:hover {
      color: #ffc0d1;
      border-color: rgba(255, 110, 155, .43);
      background: rgba(255, 110, 155, .105);
      transform: translateY(-1px);
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, .05);
    }
    #${PANEL_ID} .ryuu-button:active {
      transform: translateY(0);
      background: rgba(255, 110, 155, .08);
    }
    #${PANEL_ID} .ryuu-button:disabled {
      cursor: wait;
      transform: none;
      filter: none;
      opacity: .82;
    }
    #${PANEL_ID} .ryuu-button:disabled[data-state="installed"] {
      cursor: default;
      opacity: 1;
    }
    #${PANEL_ID} .ryuu-button[data-state="success"] {
      border-color: rgba(99, 245, 194, .25);
      color: #82d9bb;
      background: rgba(49, 151, 114, .09);
      box-shadow: none;
    }
    #${PANEL_ID} .ryuu-button[data-state="installed"] {
      border-color: rgba(99, 245, 194, .2);
      color: #75c9aa;
      background: rgba(49, 151, 114, .075);
      box-shadow: none;
    }
    #${PANEL_ID} .ryuu-button[data-state="pending"] {
      border-color: rgba(255, 207, 104, .24);
      color: #d9b87d;
      background: rgba(148, 101, 39, .08);
      box-shadow: none;
    }
    #${PANEL_ID} .ryuu-button[data-state="error"] {
      border-color: rgba(255, 110, 138, .28);
      color: #ee9aad;
      background: rgba(145, 42, 65, .1);
      box-shadow: none;
    }
    #${PANEL_ID} .ryuu-button-icon {
      position: relative;
      z-index: 1;
      display: grid;
      width: 18px;
      height: 18px;
      place-items: center;
    }
    #${PANEL_ID} .ryuu-button-icon svg {
      width: 18px;
      height: 18px;
      fill: none;
      stroke: currentColor;
      stroke-linecap: round;
      stroke-linejoin: round;
      stroke-width: 1.8;
    }
    #${PANEL_ID} .ryuu-button[data-state="loading"] .ryuu-button-icon svg,
    #${PANEL_ID} .ryuu-button[data-state="success"] .ryuu-button-icon svg,
    #${PANEL_ID} .ryuu-button[data-state="installed"] .ryuu-button-icon svg,
    #${PANEL_ID} .ryuu-button[data-state="pending"] .ryuu-button-icon svg {
      display: none;
    }
    #${PANEL_ID} .ryuu-button[data-state="loading"] .ryuu-button-icon::before {
      content: "";
      width: 14px;
      height: 14px;
      border: 2px solid rgba(255, 255, 255, .35);
      border-top-color: #fff;
      border-radius: 50%;
      animation: ryuu-spin .75s linear infinite;
    }
    #${PANEL_ID} .ryuu-button[data-state="success"] .ryuu-button-icon::before {
      content: "✓";
      color: #fff;
      font-size: 17px;
      font-weight: 800;
    }
    #${PANEL_ID} .ryuu-button[data-state="installed"] .ryuu-button-icon::before {
      content: "✓";
      color: #fff;
      font-size: 17px;
      font-weight: 800;
    }
    #${PANEL_ID} .ryuu-button[data-state="pending"] .ryuu-button-icon::before {
      content: "↻";
      color: #fff;
      font-size: 18px;
      font-weight: 700;
    }
    #${PANEL_ID} .ryuu-button-label {
      position: relative;
      z-index: 1;
    }
    #${PANEL_ID} .ryuu-button-arrow {
      position: absolute;
      z-index: 1;
      right: 15px;
      color: rgba(255, 255, 255, .72);
      font-size: 20px;
      font-weight: 300;
      transition: transform .16s ease;
    }
    #${PANEL_ID} .ryuu-button:hover .ryuu-button-arrow {
      transform: translateX(2px);
    }
    #${PANEL_ID} .ryuu-button:disabled .ryuu-button-arrow {
      opacity: 0;
    }
    @keyframes ryuu-spin {
      to { transform: rotate(360deg); }
    }
    #${MODAL_ID} {
      position: fixed;
      inset: 0;
      z-index: 2147483647;
      display: grid;
      place-items: center;
      padding: 24px;
      background: rgba(3, 5, 9, .82);
      backdrop-filter: blur(7px);
      animation: ryuu-fade-in .16s ease-out;
    }
    #${MODAL_ID} .ryuu-modal-card {
      position: relative;
      overflow: hidden;
      width: min(440px, calc(100vw - 48px));
      padding: 25px;
      border: 1px solid rgba(255, 110, 155, .2);
      border-radius: 16px;
      background:
        radial-gradient(circle at 0 0, rgba(255, 110, 155, .055), transparent 42%),
        linear-gradient(145deg, #11151e, #090c12);
      box-shadow: 0 28px 90px rgba(0, 0, 0, .62), inset 0 1px 0 rgba(255, 255, 255, .04);
      color: #d6d7d8;
      font-family: "Motiva Sans", Arial, sans-serif;
      animation: ryuu-card-in .2s ease-out;
    }
    #${MODAL_ID} .ryuu-modal-card::before {
      content: "";
      position: absolute;
      inset: 0 0 auto;
      height: 2px;
      background: linear-gradient(90deg, transparent, rgba(255, 110, 155, .72), transparent);
    }
    #${MODAL_ID} .ryuu-modal-card[data-kind="error"]::before {
      background: linear-gradient(90deg, transparent, rgba(255, 91, 125, .78), transparent);
    }
    #${MODAL_ID} h2 {
      margin: 0 0 10px;
      color: #fff;
      font-size: 20px;
      font-weight: 650;
    }
    #${MODAL_ID} p {
      margin: 0;
      color: #acb2b8;
      font-size: 14px;
      line-height: 1.55;
      white-space: pre-line;
    }
    #${MODAL_ID} .ryuu-modal-actions {
      display: flex;
      justify-content: flex-end;
      gap: 10px;
      margin-top: 22px;
    }
    #${MODAL_ID} button {
      min-width: 110px;
      height: 38px;
      padding: 0 16px;
      border: 1px solid rgba(255, 255, 255, .09);
      border-radius: 9px;
      color: #c8ced6;
      background: rgba(255, 255, 255, .035);
      cursor: pointer;
      font-size: 14px;
      font-weight: 600;
      transition: transform .15s ease, filter .15s ease, background .15s ease;
    }
    #${MODAL_ID} button:hover { color: #fff; border-color: rgba(255, 110, 155, .22); background: rgba(255, 110, 155, .06); transform: translateY(-1px); }
    #${MODAL_ID} button.primary {
      border-color: rgba(255, 110, 155, .28);
      color: #f0abc0;
      background: rgba(255, 110, 155, .085);
      box-shadow: none;
    }
    #${MODAL_ID} button.primary:hover {
      filter: brightness(1.1);
    }
    #${MODAL_ID} button:disabled { cursor: wait; opacity: .6; }
    @keyframes ryuu-fade-in {
      from { opacity: 0; }
    }
    @keyframes ryuu-card-in {
      from { opacity: 0; transform: translateY(8px) scale(.985); }
    }
  `;
  document.head.appendChild(style);
}

function createDownloadIcon(): SVGSVGElement {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  const arrow = document.createElementNS(ns, 'path');
  arrow.setAttribute('d', 'M12 3v12m0 0 4-4m-4 4-4-4');
  const tray = document.createElementNS(ns, 'path');
  tray.setAttribute('d', 'M5 18v2h14v-2');
  svg.append(arrow, tray);
  return svg;
}

function setButtonState(button: HTMLButtonElement, state: ButtonState, label: string): void {
  button.dataset.state = state;
  const text = button.querySelector<HTMLElement>('.ryuu-button-label');
  if (text) text.textContent = label;
}

function closeModal(): void {
  document.getElementById(MODAL_ID)?.remove();
}

function showMessage(title: string, message: string): void {
  closeModal();
  const overlay = document.createElement('div');
  overlay.id = MODAL_ID;
  const card = document.createElement('div');
  card.className = 'ryuu-modal-card';
  card.dataset.kind = 'error';
  const heading = document.createElement('h2');
  heading.textContent = title;
  const copy = document.createElement('p');
  copy.textContent = message;
  const actions = document.createElement('div');
  actions.className = 'ryuu-modal-actions';
  const close = document.createElement('button');
  close.textContent = 'Понятно';
  close.addEventListener('click', closeModal);
  actions.appendChild(close);
  card.append(heading, copy, actions);
  overlay.appendChild(card);
  overlay.addEventListener('click', (event) => {
    if (event.target === overlay) closeModal();
  });
  document.body.appendChild(overlay);
}

function showRestartPrompt(reply: InstallReply): void {
  closeModal();
  const overlay = document.createElement('div');
  overlay.id = MODAL_ID;
  const card = document.createElement('div');
  card.className = 'ryuu-modal-card';
  card.dataset.kind = 'success';
  const heading = document.createElement('h2');
  heading.textContent = 'Манифест получен';
  const copy = document.createElement('p');
  const details = reply.luaCount != null && reply.manifestCount != null
    ? `\nLua-файлов: ${reply.luaCount} · manifest-файлов: ${reply.manifestCount}`
    : '';
  copy.textContent = `${reply.message}${details}\n\nПерезапустить Steam сейчас?`;
  const actions = document.createElement('div');
  actions.className = 'ryuu-modal-actions';
  const later = document.createElement('button');
  later.textContent = 'Нет';
  later.addEventListener('click', closeModal);
  const restart = document.createElement('button');
  restart.className = 'primary';
  restart.textContent = 'Да, перезапустить';
  restart.addEventListener('click', async () => {
    restart.disabled = true;
    later.disabled = true;
    restart.textContent = 'Перезапуск…';
    try {
      const result = JSON.parse(await restartSteam()) as InstallReply;
      if (!result.success) {
        showMessage('Не удалось перезапустить Steam', result.message);
      }
    } catch (error) {
      showMessage('Не удалось перезапустить Steam', String(error));
    }
  });
  actions.append(later, restart);
  card.append(heading, copy, actions);
  overlay.appendChild(card);
  document.body.appendChild(overlay);
}

async function handleDownload(button: HTMLButtonElement, appid: string): Promise<void> {
  const panel = button.closest<HTMLElement>(`#${PANEL_ID}`);
  const repairing = button.dataset.gameState === 'repair_required';
  button.disabled = true;
  setButtonState(button, 'loading', 'Получение манифеста…');
  try {
    const reply = JSON.parse(await installManifest({ appid, name: currentGameName() })) as InstallReply;
    if (panel) applyServiceStatus(panel, reply);
    if (!reply.success) {
      showMessage('Не удалось получить манифест', reply.message);
      button.dataset.gameState = 'error';
      setButtonState(button, 'error', 'Повторить загрузку');
      return;
    }
    if (repairing || reply.repaired) {
      button.dataset.gameState = 'installed';
      setButtonState(button, 'installed', 'Уже добавлено');
      showMessage('Манифест восстановлен', 'Файлы возвращены в Steam. Игру можно сразу снова поставить на установку.');
    } else {
      button.dataset.gameState = 'pending_restart';
      setButtonState(button, 'pending', 'Требуется перезапуск Steam');
      showRestartPrompt(reply);
    }
  } catch (error) {
    showMessage('Ошибка плагина', String(error));
    button.dataset.gameState = 'error';
    setButtonState(button, 'error', 'Повторить загрузку');
  } finally {
    button.disabled = false;
    if (panel) void refreshGameStatus(panel, appid, true);
  }
}

function applyServiceStatus(panel: HTMLElement, status: InstallReply): void {
  const meta = panel.querySelector<HTMLElement>('.ryuu-service-meta');
  const usageText = panel.querySelector<HTMLElement>('.ryuu-service-usage');
  if (!meta || !usageText) return;
  const service = status.service || 'ryuu';
  meta.hidden = service !== 'hubcap';
  if (service !== 'hubcap') return;

  if (status.configured === false) {
    meta.dataset.error = 'true';
    usageText.textContent = 'укажите API-ключ';
    meta.title = 'Добавьте личный API-ключ HubCap в настройках Crocodile Manifest';
    return;
  }
  if (status.hubcapUsage) {
    const { used, limit, remaining } = status.hubcapUsage;
    meta.dataset.error = remaining <= 0 ? 'true' : 'false';
    usageText.textContent = `${used}/${limit} · осталось ${remaining}`;
    meta.title = `HubCap: использовано ${used} из ${limit}, осталось ${remaining}`;
    return;
  }
  meta.dataset.error = status.hubcapUsageError ? 'true' : 'false';
  usageText.textContent = status.hubcapUsageError ? 'лимит недоступен' : 'проверка лимита…';
  meta.title = status.hubcapUsageError || 'Получаем дневной лимит HubCap';
}

function applyGameStatus(
  panel: HTMLElement,
  button: HTMLButtonElement,
  subtitle: HTMLElement,
  status: GameStatusReply,
): void {
  panel.dataset.mode = status.cardMode || 'full';
  applyServiceStatus(panel, status);
  const gameState = status.state || 'available';
  button.dataset.gameState = gameState;

  if (gameState === 'configuration_required') {
    const serviceName = status.service === 'hubcap' ? 'HubCap' : 'Ryuu';
    subtitle.textContent = `Укажите личный API-ключ ${serviceName} в настройках плагина`;
    button.title = `Требуется API-ключ ${serviceName}`;
    setButtonState(button, 'error', 'Нужен API-ключ');
  } else if (gameState === 'pending_restart') {
    subtitle.textContent = 'Файлы установлены — Steam нужно перезапустить';
    button.title = 'Требуется перезапуск Steam';
    setButtonState(button, 'pending', 'Требуется перезапуск Steam');
  } else if (gameState === 'update_available') {
    subtitle.textContent = 'Для установленного манифеста найдено обновление';
    button.title = 'Обновить манифест';
    setButtonState(button, 'idle', 'Обновить манифест');
  } else if (gameState === 'repair_required') {
    const missing = status.missingManifestCount || 1;
    subtitle.textContent = `Steam удалил manifest-файлы: ${missing}. Их можно вернуть без повторного добавления игры`;
    button.title = 'Восстановить удалённые manifest-файлы';
    setButtonState(button, 'idle', 'Восстановить манифест');
  } else if (gameState === 'installed') {
    subtitle.textContent = status.restoredManifestCount
      ? `Манифест автоматически восстановлен: ${status.restoredManifestCount}`
      : 'Установлена актуальная версия манифеста';
    button.title = 'Уже добавлено';
    setButtonState(button, 'installed', 'Уже добавлено');
  } else if (gameState === 'error') {
    subtitle.textContent = status.lastMessage || 'Предыдущая загрузка завершилась ошибкой';
    button.title = 'Повторить загрузку';
    setButtonState(button, 'error', 'Повторить загрузку');
  } else {
    subtitle.textContent = 'Lua и манифесты прямо из магазина Steam';
    button.title = 'Получить манифест';
    setButtonState(button, 'idle', 'Получить манифест');
  }
  button.disabled = gameState === 'installed' || gameState === 'configuration_required';
}

async function refreshGameStatus(panel: HTMLElement, appid: string, force = false): Promise<void> {
  const lastCheck = Number(panel.dataset.statusChecked || 0);
  if (!force && Date.now() - lastCheck < 4000) return;
  if (panel.dataset.statusLoading === 'true') return;
  const button = panel.querySelector<HTMLButtonElement>('.ryuu-button');
  const subtitle = panel.querySelector<HTMLElement>('.ryuu-subtitle');
  if (!button || !subtitle) return;

  panel.dataset.statusLoading = 'true';
  panel.dataset.statusChecked = String(Date.now());
  try {
    const status = JSON.parse(await getGameStatus({ appid })) as GameStatusReply;
    if (status.success && button.dataset.state !== 'loading') {
      applyGameStatus(panel, button, subtitle, status);
    }
  } catch {
    // Фоновая проверка не должна перекрывать карточку сетевой или IPC-ошибкой.
  } finally {
    panel.dataset.statusLoading = 'false';
  }
}

function findPurchaseArea(): Element | null {
  return document.querySelector('#game_area_purchase')
    ?? document.querySelector('.game_area_purchase_game_wrapper')?.parentElement
    ?? document.querySelector('.queue_actions_ctn')?.parentElement
    ?? null;
}

function injectButton(): void {
  const appid = currentAppId();
  const existing = document.getElementById(PANEL_ID);
  if (!appid) {
    existing?.remove();
    return;
  }
  if (existing?.dataset.appid === appid) {
    void refreshGameStatus(existing, appid);
    return;
  }
  existing?.remove();

  const target = findPurchaseArea();
  if (!target) return;

  const panel = document.createElement('section');
  panel.id = PANEL_ID;
  panel.dataset.appid = appid;
  panel.dataset.mode = 'full';
  const header = document.createElement('div');
  header.className = 'ryuu-header';
  const logo = document.createElement('div');
  logo.className = 'ryuu-logo';
  logo.textContent = '🐊';
  const copy = document.createElement('div');
  copy.className = 'ryuu-copy';
  const title = document.createElement('div');
  title.className = 'ryuu-title';
  title.textContent = 'Crocodile Manifest';
  const subtitle = document.createElement('div');
  subtitle.className = 'ryuu-subtitle';
  subtitle.textContent = 'Lua и манифесты прямо из магазина Steam';
  copy.append(title, subtitle);
  header.append(logo, copy);
  const serviceMeta = document.createElement('div');
  serviceMeta.className = 'ryuu-service-meta';
  serviceMeta.hidden = true;
  const serviceName = document.createElement('span');
  serviceName.className = 'ryuu-service-name';
  serviceName.textContent = 'HubCap';
  const serviceUsage = document.createElement('span');
  serviceUsage.className = 'ryuu-service-usage';
  serviceUsage.textContent = 'проверка лимита…';
  serviceMeta.append(serviceName, serviceUsage);
  const button = document.createElement('button');
  button.className = 'ryuu-button';
  button.type = 'button';
  button.dataset.state = 'idle';
  button.dataset.gameState = 'available';
  const icon = document.createElement('span');
  icon.className = 'ryuu-button-icon';
  icon.appendChild(createDownloadIcon());
  const label = document.createElement('span');
  label.className = 'ryuu-button-label';
  label.textContent = 'Получить манифест';
  const arrow = document.createElement('span');
  arrow.className = 'ryuu-button-arrow';
  arrow.textContent = '›';
  button.append(icon, label, arrow);
  button.addEventListener('click', () => {
    if (button.dataset.gameState === 'pending_restart') {
      showRestartPrompt({
        success: true,
        message: 'Файлы уже установлены. Для применения изменений требуется перезапуск Steam.',
      });
      return;
    }
    void handleDownload(button, appid);
  });
  panel.append(header, serviceMeta, button);
  target.prepend(panel);
  void refreshGameStatus(panel, appid, true);
}

export default async function WebkitMain(): Promise<void> {
  void keepManifestsAvailable();
  window.setInterval((): void => {
    void keepManifestsAvailable();
  }, 2000);
  if (!/^(store\.)?steampowered\.com$/i.test(window.location.hostname)) return;
  addStyles();
  injectButton();

  let scheduled = false;
  const observer = new MutationObserver(() => {
    if (scheduled) return;
    scheduled = true;
    window.setTimeout(() => {
      scheduled = false;
      injectButton();
    }, 180);
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  window.setInterval(injectButton, 1500);
}
