import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import { callable, definePlugin } from '@steambrew/client';

type CardMode = 'full' | 'button' | 'crocodile';
type ManifestService = 'ryuu' | 'hubcap';

type HubcapUsage = {
  used: number;
  limit: number;
  remaining: number;
};

type InstalledGame = {
  appid: string;
  name: string;
  installedAt: number;
  luaCount?: number;
  manifestCount?: number;
};

type PluginStateReply = {
  success: boolean;
  message: string;
  cardMode?: CardMode;
  service?: ManifestService;
  hasRyuuKey?: boolean;
  hasHubcapKey?: boolean;
  hubcapUsage?: HubcapUsage;
  validationError?: string;
  installedGames?: InstalledGame[];
  removedCount?: number;
};

const getPluginState = callable<[], string>('get_plugin_state');
const setCardModeRequest = callable<[{ mode: CardMode }], string>('set_card_mode');
const setServiceSettings = callable<[{ payload: string }], string>('set_service_settings');
const removeManifest = callable<[{ appid: string }], string>('remove_manifest');
const KEY_PAGES: Record<ManifestService, string> = {
  ryuu: 'https://generator.ryuu.lol/api',
  hubcap: 'https://hubcapmanifest.com/api-keys/',
};
const DISCORD_PAGES: Record<ManifestService, string> = {
  ryuu: 'https://discord.gg/manifests',
  hubcap: 'https://discord.gg/hubcapsmanifest',
};

const pageStyle: CSSProperties = {
  display: 'grid',
  gap: 14,
  padding: '8px 12px 22px',
};

const sectionStyle: CSSProperties = {
  overflow: 'hidden',
  border: '1px solid rgba(255,255,255,.08)',
  borderRadius: 14,
  background: 'linear-gradient(145deg, rgba(17,20,29,.96), rgba(9,12,18,.98))',
  boxShadow: 'inset 0 1px 0 rgba(255,255,255,.035), 0 12px 28px rgba(0,0,0,.14)',
};

const sectionHeaderStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 14,
  padding: '16px 18px',
  borderBottom: '1px solid rgba(255,255,255,.065)',
};

const modeGridStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
  gap: 9,
  padding: 14,
};

const historyListStyle: CSSProperties = {
  display: 'grid',
  gap: 9,
  padding: 14,
};

function modeButtonStyle(active: boolean): CSSProperties {
  return {
    display: 'grid',
    placeItems: 'center',
    gap: 7,
    minHeight: 82,
    padding: '12px 9px',
    border: active ? '1px solid rgba(255,110,155,.28)' : '1px solid rgba(255,255,255,.075)',
    borderRadius: 10,
    color: active ? '#f3e7eb' : '#a9b0bb',
    background: active
      ? 'rgba(255,110,155,.055)'
      : 'rgba(0,0,0,.15)',
    boxShadow: active ? 'inset 3px 0 rgba(255,110,155,.55), inset 0 1px 0 rgba(255,255,255,.04)' : 'none',
    cursor: 'pointer',
    font: 'inherit',
  };
}

function smallButtonStyle(danger = false): CSSProperties {
  return {
    height: 32,
    padding: '0 12px',
    border: danger ? '1px solid rgba(255,112,137,.25)' : '1px solid rgba(255,110,155,.18)',
    borderRadius: 8,
    color: danger ? '#ff91a5' : '#d6a5b6',
    background: danger ? 'rgba(145,42,65,.14)' : 'rgba(255,110,155,.045)',
    cursor: 'pointer',
    fontSize: 12,
    fontWeight: 600,
  };
}

function serviceButtonStyle(active: boolean): CSSProperties {
  return {
    display: 'grid',
    width: '100%',
    minWidth: 0,
    gap: 5,
    padding: '13px 14px',
    border: active ? '1px solid rgba(255,110,155,.28)' : '1px solid rgba(255,255,255,.075)',
    borderRadius: 10,
    color: active ? '#f3e7eb' : '#a9b0bb',
    background: active
      ? 'rgba(255,110,155,.055)'
      : 'rgba(0,0,0,.15)',
    boxShadow: active ? 'inset 3px 0 rgba(255,110,155,.55), inset 0 1px 0 rgba(255,255,255,.04)' : 'none',
    cursor: 'pointer',
    font: 'inherit',
    textAlign: 'left',
  };
}

function keyLinkButtonStyle(active: boolean): CSSProperties {
  return {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
    width: '100%',
    minWidth: 0,
    height: 32,
    padding: '0 6px',
    border: active ? '1px solid rgba(255,110,155,.23)' : '1px solid rgba(255,255,255,.07)',
    borderRadius: 7,
    color: active ? '#df9fb3' : '#939ca9',
    background: active ? 'rgba(255,110,155,.055)' : 'rgba(0,0,0,.11)',
    cursor: 'pointer',
    font: '600 10px/1 inherit',
    whiteSpace: 'nowrap',
  };
}

function discordButtonStyle(): CSSProperties {
  return {
    ...keyLinkButtonStyle(false),
    border: '1px solid rgba(169,135,255,.14)',
    color: '#b9afd0',
    background: 'rgba(169,135,255,.045)',
  };
}

const keyInputStyle: CSSProperties = {
  width: '100%',
  boxSizing: 'border-box',
  height: 40,
  padding: '0 12px',
  border: '1px solid rgba(255,255,255,.09)',
  borderRadius: 8,
  outline: 'none',
  color: '#e0e4e9',
  background: 'rgba(0,0,0,.2)',
  fontFamily: 'Consolas, monospace',
  fontSize: 12,
};

function openExternalPage(url: string): void {
  try {
    SteamClient.System.OpenInSystemBrowser(url);
  } catch {
    window.open(url, '_blank', 'noopener,noreferrer');
  }
}

function openKeyPage(service: ManifestService): void {
  openExternalPage(KEY_PAGES[service]);
}

function openDiscordPage(service: ManifestService): void {
  openExternalPage(DISCORD_PAGES[service]);
}

function PluginContent() {
  const [cardMode, setCardModeState] = useState<CardMode>('full');
  const [service, setService] = useState<ManifestService>('ryuu');
  const [ryuuKey, setRyuuKey] = useState('');
  const [hubcapKey, setHubcapKey] = useState('');
  const [hasRyuuKey, setHasRyuuKey] = useState(false);
  const [hasHubcapKey, setHasHubcapKey] = useState(false);
  const [savingService, setSavingService] = useState(false);
  const [installedGames, setInstalledGames] = useState<InstalledGame[]>([]);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState('');
  const [confirmingAppId, setConfirmingAppId] = useState('');
  const [busyAppId, setBusyAppId] = useState('');

  const loadState = useCallback(async () => {
    setLoading(true);
    try {
      const reply = JSON.parse(await getPluginState()) as PluginStateReply;
      if (!reply.success) throw new Error(reply.message);
      setCardModeState(reply.cardMode || 'full');
      setService(reply.service || 'ryuu');
      setHasRyuuKey(reply.hasRyuuKey === true);
      setHasHubcapKey(reply.hasHubcapKey === true);
      setInstalledGames(Array.isArray(reply.installedGames) ? reply.installedGames : []);
      setMessage('');
    } catch (error) {
      setMessage(`Не удалось загрузить настройки: ${String(error)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadState();
  }, [loadState]);

  const changeCardMode = async (mode: CardMode) => {
    const previous = cardMode;
    setCardModeState(mode);
    try {
      const reply = JSON.parse(await setCardModeRequest({ mode })) as PluginStateReply;
      if (!reply.success) throw new Error(reply.message);
      setMessage('Режим карточки сохранён');
    } catch (error) {
      setCardModeState(previous);
      setMessage(`Не удалось сохранить режим: ${String(error)}`);
    }
  };

  const saveManifestService = async () => {
    setSavingService(true);
    try {
      const payload = JSON.stringify({
        service,
        ryuuKey: ryuuKey.trim(),
        hubcapKey: hubcapKey.trim(),
        clearRyuu: false,
        clearHubcap: false,
      });
      const reply = JSON.parse(await setServiceSettings({ payload })) as PluginStateReply;
      if (!reply.success) throw new Error(reply.message);
      setHasRyuuKey(reply.hasRyuuKey === true);
      setHasHubcapKey(reply.hasHubcapKey === true);
      setRyuuKey('');
      setHubcapKey('');
      setMessage(reply.validationError
        ? `Настройки сохранены, но ключ не прошёл проверку: ${reply.validationError}`
        : 'Сервис и API-ключ сохранены');
    } catch (error) {
      setMessage(`Не удалось сохранить сервис: ${String(error)}`);
    } finally {
      setSavingService(false);
    }
  };

  const removeGame = async (appid: string) => {
    if (confirmingAppId !== appid) {
      setConfirmingAppId(appid);
      return;
    }
    setBusyAppId(appid);
    try {
      const reply = JSON.parse(await removeManifest({ appid })) as PluginStateReply;
      if (!reply.success) throw new Error(reply.message);
      setConfirmingAppId('');
      await loadState();
      setMessage(reply.message);
    } catch (error) {
      setMessage(`Не удалось удалить файлы: ${String(error)}`);
    } finally {
      setBusyAppId('');
    }
  };

  const modes: Array<{ id: CardMode; icon: string; title: string; description: string }> = [
    { id: 'full', icon: '▤', title: 'Full', description: 'Полная карточка' },
    { id: 'button', icon: '▬', title: 'Button', description: 'Только кнопка' },
    { id: 'crocodile', icon: '🐊', title: 'Crocodile', description: 'Только эмодзи' },
  ];

  return (
    <div style={pageStyle}>
      <section
        style={{
          ...sectionStyle,
          padding: 18,
          background:
            'radial-gradient(circle at 0 0, rgba(255,110,155,.06), transparent 42%), linear-gradient(145deg, rgba(17,20,29,.98), rgba(9,12,18,.99))',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 13 }}>
          <div
            style={{
              display: 'grid',
              width: 44,
              height: 44,
              placeItems: 'center',
              border: '1px solid rgba(255,110,155,.24)',
              borderRadius: 11,
              background: 'rgba(255,110,155,.075)',
              fontSize: 24,
            }}
          >
            🐊
          </div>
          <div style={{ minWidth: 0, flex: 1 }}>
            <h2 style={{ margin: 0, color: '#f5f8fb', fontSize: 18 }}>Crocodile Manifest</h2>
            <p style={{ margin: '4px 0 0', color: '#8fa0ae', fontSize: 12 }}>
              Управление видом карточки и установленными манифестами
            </p>
          </div>
          <span
            style={{
              padding: '5px 9px',
              border: '1px solid rgba(255,110,155,.14)',
              borderRadius: 99,
              color: '#d99aaf',
              background: 'rgba(255,110,155,.04)',
              fontSize: 11,
              fontWeight: 700,
            }}
          >
            v0.4.9
          </span>
        </div>
      </section>

      {message && (
        <div
          style={{
            padding: '10px 12px',
            border: '1px solid rgba(255,110,155,.14)',
            borderRadius: 9,
            color: '#c9a5b1',
            background: 'rgba(255,110,155,.04)',
            fontSize: 12,
          }}
        >
          {message}
        </div>
      )}

      <section style={sectionStyle}>
        <div style={sectionHeaderStyle}>
          <div>
            <strong style={{ color: '#edf3f7', fontSize: 14 }}>Сервис манифестов</strong>
            <div style={{ marginTop: 4, color: '#8293a1', fontSize: 12 }}>
              Выберите источник и добавьте свой API-ключ
            </div>
          </div>
          <span
            style={{
              padding: '5px 9px',
              border: '1px solid rgba(255,110,155,.14)',
              borderRadius: 99,
              color: '#d99aaf',
              background: 'rgba(255,110,155,.04)',
              fontSize: 10,
              fontWeight: 700,
            }}
          >
            {service === 'hubcap' ? 'HubCap' : 'Ryuu'}
          </span>
        </div>

        <div style={{ display: 'grid', gap: 12, padding: 14 }}>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 9 }}>
            <button type="button" style={serviceButtonStyle(service === 'ryuu')} onClick={() => setService('ryuu')}>
              <strong style={{ fontSize: 13 }}>Ryuu</strong>
              <span style={{ color: '#80919e', fontSize: 10 }}>Личный X-Auth-Key</span>
            </button>
            <button
              type="button"
              style={serviceButtonStyle(service === 'hubcap')}
              onClick={() => setService('hubcap')}
            >
              <strong style={{ fontSize: 13 }}>HubCap</strong>
              <span style={{ color: '#80919e', fontSize: 10 }}>Личный Bearer API-ключ</span>
            </button>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 9, marginTop: 2 }}>
            <div style={{ display: 'grid', minWidth: 0, gap: 6 }}>
              <button
                type="button"
                style={keyLinkButtonStyle(service === 'ryuu')}
                title="Открыть страницу получения ключа Ryuu"
                onClick={() => openKeyPage('ryuu')}
              >
                Получить ключ
              </button>
              <button
                type="button"
                style={discordButtonStyle()}
                title="Вступить в Discord Ryuu"
                onClick={() => openDiscordPage('ryuu')}
              >
                Discord Ryuu
              </button>
            </div>
            <div style={{ display: 'grid', minWidth: 0, gap: 6 }}>
              <button
                type="button"
                style={keyLinkButtonStyle(service === 'hubcap')}
                title="Открыть страницу получения ключа HubCap"
                onClick={() => openKeyPage('hubcap')}
              >
                Получить ключ
              </button>
              <button
                type="button"
                style={discordButtonStyle()}
                title="Вступить в Discord HubCap"
                onClick={() => openDiscordPage('hubcap')}
              >
                Discord HubCap
              </button>
            </div>
          </div>

          <label style={{ display: 'grid', gap: 7 }}>
            <span style={{ color: '#9aabb7', fontSize: 11, fontWeight: 650 }}>
              API-ключ {service === 'hubcap' ? 'HubCap' : 'Ryuu'}
            </span>
            <input
              type="password"
              value={service === 'hubcap' ? hubcapKey : ryuuKey}
              autoComplete="off"
              spellCheck={false}
              placeholder={(service === 'hubcap' ? hasHubcapKey : hasRyuuKey)
                ? 'Ключ сохранён — введите новый только для замены'
                : 'Вставьте личный API-ключ'}
              style={keyInputStyle}
              onChange={(event) => {
                if (service === 'hubcap') setHubcapKey(event.currentTarget.value);
                else setRyuuKey(event.currentTarget.value);
              }}
            />
          </label>

          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
            <span style={{ color: '#71828f', fontSize: 10.5, lineHeight: 1.4 }}>
              Ключ сохраняется локально и не отображается обратно в интерфейсе.
            </span>
            <button
              type="button"
              style={smallButtonStyle(false)}
              disabled={savingService}
              onClick={() => void saveManifestService()}
            >
              {savingService ? 'Сохранение…' : 'Сохранить'}
            </button>
          </div>
        </div>
      </section>

      <section style={sectionStyle}>
        <div style={sectionHeaderStyle}>
          <div>
            <strong style={{ color: '#edf3f7', fontSize: 14 }}>Установленные манифесты</strong>
            <div style={{ marginTop: 4, color: '#8293a1', fontSize: 12 }}>
              {installedGames.length
                ? `${installedGames.length} игр можно удалить отдельно`
                : 'Установленные через плагин игры появятся здесь'}
            </div>
          </div>
        </div>

        <div style={historyListStyle}>
          {!loading && installedGames.length === 0 && (
            <div style={{ padding: 20, color: '#758793', textAlign: 'center' }}>
              Установленных манифестов пока нет
            </div>
          )}

          {installedGames.map((game) => {
            const confirming = confirmingAppId === String(game.appid);
            const busy = busyAppId === String(game.appid);
            return (
              <article
                key={game.appid}
                style={{
                  display: 'flex',
                  width: '100%',
                  minWidth: 0,
                  boxSizing: 'border-box',
                  alignItems: 'center',
                  gap: 10,
                  padding: '12px',
                  border: '1px solid rgba(255,255,255,.07)',
                  borderRadius: 9,
                  background: 'rgba(0,0,0,.15)',
                }}
              >
                <div
                  style={{
                    display: 'grid',
                    flex: '0 0 34px',
                    width: 34,
                    height: 34,
                    placeItems: 'center',
                    borderRadius: 8,
                    color: '#79d9b1',
                    background: 'rgba(52,144,109,.16)',
                    fontSize: 17,
                    fontWeight: 800,
                  }}
                >
                  ✓
                </div>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <strong
                    style={{
                      display: 'block',
                      overflow: 'hidden',
                      color: '#f0f4f7',
                      fontSize: 13,
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {game.name || `AppID ${game.appid}`}
                  </strong>
                  <span style={{ display: 'block', marginTop: 5, color: '#7f909d', fontSize: 11 }}>
                    AppID {game.appid} · Lua: {game.luaCount || 0} · manifest: {game.manifestCount || 0}
                  </span>
                </div>
                <button
                  type="button"
                  title={confirming ? 'Нажмите ещё раз для подтверждения' : `Удалить манифест ${game.name || game.appid}`}
                  style={{
                    ...smallButtonStyle(true),
                    flex: '0 0 82px',
                    width: 82,
                    minWidth: 82,
                    padding: '0 6px',
                    fontSize: 11,
                  }}
                  disabled={busy}
                  onClick={() => void removeGame(String(game.appid))}
                >
                  {busy ? 'Удаление…' : confirming ? 'Подтвердить' : 'Удалить'}
                </button>
              </article>
            );
          })}
        </div>
      </section>

      <section style={sectionStyle}>
        <div style={sectionHeaderStyle}>
          <div>
            <strong style={{ color: '#edf3f7', fontSize: 14 }}>Вид карточки</strong>
            <div style={{ marginTop: 4, color: '#8293a1', fontSize: 12 }}>
              Выберите, сколько места Crocodile Manifest занимает на странице игры
            </div>
          </div>
        </div>
        <div style={modeGridStyle}>
          {modes.map((mode) => (
            <button
              key={mode.id}
              type="button"
              style={modeButtonStyle(cardMode === mode.id)}
              disabled={loading}
              onClick={() => void changeCardMode(mode.id)}
            >
              <span style={{ fontSize: mode.id === 'crocodile' ? 24 : 21 }}>{mode.icon}</span>
              <strong style={{ fontSize: 13 }}>{mode.title}</strong>
              <span style={{ color: cardMode === mode.id ? '#cda7b3' : '#75808e', fontSize: 10 }}>
                {mode.description}
              </span>
            </button>
          ))}
        </div>
      </section>

    </div>
  );
}

export default definePlugin(() => ({
  title: 'Crocodile Manifest',
  icon: <span>🐊</span>,
  content: <PluginContent />,
}));
