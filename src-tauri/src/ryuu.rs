use crate::luatools::{self, LuaInstallResult};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Manager};

const HOST: &str = "generator.ryuu.lol";
const ORIGIN: &str = "https://generator.ryuu.lol";
/// Реверс-прокси на своей инфраструктуре (обход DPI/SNI-блокировки провайдера). Пусто = выключен.
/// Формат без слэша в конце, напр. "https://gen.zelees.digital".
const PROXY_BASE: &str = "";

/// Встроенный ключ доступа к API генератора. Компилируется в бинарник и не попадает во фронтенд.
/// ВНИМАНИЕ: в exe его технически можно извлечь реверс-инжинирингом — полностью скрыть можно
/// только через промежуточный прокси-сервер.
// Public builds never include the developer's credential.
// Optional: set CGS_RYUU_API_KEY before invoking Cargo/Tauri.
const AUTH_KEY: &str = match option_env!("CGS_RYUU_API_KEY") {
    Some(key) => key,
    None => "",
};

const CATALOG_URL: &str = "https://generator.ryuu.lol/api/games";
const DOWNLOAD_BASE: &str = "https://generator.ryuu.lol/api/download/";
const CATALOG_TTL: Duration = Duration::from_secs(24 * 3600);
/// Реальный Chrome-User-Agent: чтобы Cloudflare не отсекал запрос как бота (без него у части
/// пользователей генератор доступен только через VPN, хотя сайт открывается напрямую).
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[derive(Deserialize)]
struct RawGame {
    appid: String,
    name: String,
    #[serde(default)]
    header_image: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RyuuGame {
    pub appid: String,
    pub name: String,
    pub header_image: Option<String>,
    pub kind: Option<String>,
}

fn catalog_cache() -> &'static Mutex<Option<Vec<RyuuGame>>> {
    static CACHE: OnceLock<Mutex<Option<Vec<RyuuGame>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Сериализует загрузку каталога: параллельные запросы поиска не качают его по нескольку раз.
fn download_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn catalog_loaded() -> bool {
    catalog_cache()
        .lock()
        .map(|cache| cache.is_some())
        .unwrap_or(false)
}

fn base_headers() -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        reqwest::header::REFERER,
        HeaderValue::from_static("https://generator.ryuu.lol/"),
    );
    headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    headers
}

/// Клиент к генератору. `resolve_to` пинует домен на конкретный IP (получен через DoH в обход
/// провайдерского DNS); при `None` используется системный DNS.
fn build_client(resolve_to: Option<IpAddr>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .user_agent(USER_AGENT)
        .default_headers(base_headers())
        .timeout(Duration::from_secs(90));
    if let Some(ip) = resolve_to {
        builder = builder.resolve(HOST, SocketAddr::new(ip, 443));
    }
    builder
        .build()
        .map_err(|error| format!("Не удалось подготовить сетевой клиент: {error}"))
}

fn client() -> Result<Client, String> {
    build_client(None)
}

fn doh_cache() -> &'static Mutex<Option<IpAddr>> {
    static CACHE: OnceLock<Mutex<Option<IpAddr>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Один DoH-запрос к провайдеру (по IP, без системного DNS). Возвращает первый A-адрес домена.
fn query_doh(url: &str) -> Option<IpAddr> {
    let response = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?
        .get(url)
        .header(reqwest::header::ACCEPT, "application/dns-json")
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: Value = serde_json::from_str(&response.text().ok()?).ok()?;
    json.get("Answer")?
        .as_array()?
        .iter()
        .filter(|answer| answer.get("type").and_then(Value::as_u64) == Some(1))
        .find_map(|answer| answer.get("data").and_then(Value::as_str))
        .and_then(|address| address.parse::<IpAddr>().ok())
}

/// Резолвит домен генератора через шифрованный DNS (обход блокировки провайдера). Кэшируется.
fn doh_resolve() -> Option<IpAddr> {
    if let Some(ip) = doh_cache().lock().ok().and_then(|cache| *cache) {
        return Some(ip);
    }
    let providers = [
        "https://1.1.1.1/dns-query?name=generator.ryuu.lol&type=A",
        "https://1.0.0.1/dns-query?name=generator.ryuu.lol&type=A",
        "https://8.8.8.8/resolve?name=generator.ryuu.lol&type=A",
    ];
    for url in providers {
        if let Some(ip) = query_doh(url) {
            if let Ok(mut cache) = doh_cache().lock() {
                *cache = Some(ip);
            }
            return Some(ip);
        }
    }
    None
}

enum GetError {
    /// Сетевой сбой (DNS/соединение) — имеет смысл повторить через DoH.
    Network(String),
    /// Ответ сервера с ошибкой (401/404/…) — повтор не поможет.
    Http(String),
}

fn map_status(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "Доступ к генератору запрещён (проверьте ключ)".to_string(),
        404 => "Для этой игры нет манифестов в генераторе".to_string(),
        429 => "Слишком много запросов, попробуйте позже".to_string(),
        other => format!("Сервер вернул ошибку {other}"),
    }
}

fn http_get_once(
    client: &Client,
    url: &str,
    auth: Option<&str>,
    identity: bool,
) -> Result<Vec<u8>, GetError> {
    let mut request = client.get(url);
    if let Some(key) = auth {
        request = request.header("X-Auth-Key", key);
    }
    if identity {
        // Готовый zip — без авто-декомпрессии, иначе reqwest падает на декодировании бинарника.
        request = request.header(reqwest::header::ACCEPT_ENCODING, "identity");
    }
    let response = request
        .send()
        .map_err(|error| GetError::Network(format!("Сеть недоступна: {error}")))?;
    if !response.status().is_success() {
        return Err(GetError::Http(map_status(response.status())));
    }
    response
        .bytes()
        .map(|data| data.to_vec())
        .map_err(|error| GetError::Network(format!("Не удалось получить файл: {error}")))
}

/// Переписывает адрес генератора на реверс-прокси (если он задан).
fn proxy_url(url: &str) -> Option<String> {
    if PROXY_BASE.is_empty() {
        return None;
    }
    url.strip_prefix(ORIGIN)
        .map(|path| format!("{PROXY_BASE}{path}"))
}

/// GET с обходом блокировки: прямой запрос → DoH (обход DNS-блока) → реверс-прокси (обход DPI/SNI).
/// Прокси задействуется последним, только когда прямой путь и DoH не прошли.
fn http_get(url: &str, auth: Option<&str>, identity: bool) -> Result<Vec<u8>, String> {
    // 1. Прямой запрос (системный DNS).
    let network_error = match http_get_once(&client()?, url, auth, identity) {
        Ok(bytes) => return Ok(bytes),
        Err(GetError::Http(message)) => return Err(message),
        Err(GetError::Network(message)) => message,
    };

    // 2. DoH — обход блокировки по DNS.
    if let Some(ip) = doh_resolve() {
        match http_get_once(&build_client(Some(ip))?, url, auth, identity) {
            Ok(bytes) => return Ok(bytes),
            Err(GetError::Http(message)) => return Err(message),
            Err(GetError::Network(_)) => {}
        }
    }

    // 3. Реверс-прокси — обход блокировки по SNI/DPI.
    if let Some(proxied) = proxy_url(url) {
        match http_get_once(&client()?, &proxied, auth, identity) {
            Ok(bytes) => return Ok(bytes),
            Err(GetError::Http(message)) => return Err(message),
            Err(GetError::Network(second)) => {
                return Err(format!("{network_error}. Прокси тоже недоступен: {second}"))
            }
        }
    }

    let _ = network_error;
    Err("Генератор недоступен — вероятно, провайдер блокирует домен. Включите VPN и попробуйте снова.".to_string())
}

fn catalog_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_local_data_dir().ok()?;
    let _ = fs::create_dir_all(&dir);
    Some(dir.join("ryuu-games.json"))
}

fn parse_catalog(bytes: &[u8]) -> Vec<RyuuGame> {
    serde_json::from_slice::<Vec<RawGame>>(bytes)
        .map(|list| {
            list.into_iter()
                .map(|game| RyuuGame {
                    appid: game.appid,
                    name: game.name,
                    header_image: game.header_image,
                    kind: game.kind,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Свежая копия каталога с диска (если моложе TTL).
fn fresh_disk_catalog(app: &AppHandle) -> Option<Vec<u8>> {
    let path = catalog_path(app)?;
    let age = fs::metadata(&path).ok()?.modified().ok()?.elapsed().ok()?;
    (age < CATALOG_TTL).then(|| fs::read(&path).ok()).flatten()
}

/// Гарантирует, что каталог загружен в память: диск (если свежий) → иначе скачивание с сохранением.
fn ensure_catalog(app: &AppHandle) -> Result<(), String> {
    if catalog_loaded() {
        return Ok(());
    }
    // Только один поток скачивает каталог; остальные ждут и берут уже загруженный.
    let _guard = download_lock()
        .lock()
        .map_err(|_| "Загрузка каталога занята".to_string())?;
    if catalog_loaded() {
        return Ok(());
    }
    let bytes = match fresh_disk_catalog(app) {
        Some(bytes) => bytes,
        None => {
            let bytes = http_get(CATALOG_URL, None, false)
                .map_err(|error| format!("Каталог игр недоступен: {error}"))?;
            if let Some(path) = catalog_path(app) {
                let _ = fs::write(path, &bytes);
            }
            bytes
        }
    };
    let games = parse_catalog(&bytes);
    if games.is_empty() {
        return Err("Каталог игр пуст или в неизвестном формате".into());
    }
    if let Ok(mut cache) = catalog_cache().lock() {
        *cache = Some(games);
    }
    Ok(())
}

/// Async-обёртка: тяжёлая сетевая/парсинг-работа уходит в блокирующий пул, чтобы не морозить UI.
#[tauri::command]
pub async fn ryuu_search(app: AppHandle, query: String) -> Result<Vec<RyuuGame>, String> {
    tauri::async_runtime::spawn_blocking(move || ryuu_search_blocking(app, query))
        .await
        .map_err(|error| format!("Поиск прервался: {error}"))?
}

fn ryuu_search_blocking(app: AppHandle, query: String) -> Result<Vec<RyuuGame>, String> {
    let query = query.trim().to_lowercase();
    if query.len() < 2 {
        return Ok(Vec::new());
    }
    let numeric = query.chars().all(|c| c.is_ascii_digit());
    // Если каталог не удалось загрузить (нет сети/Cloudflare/антивирус), числовой AppID всё равно
    // остаётся рабочим — можно сразу получить игру по нему.
    if let Err(error) = ensure_catalog(&app) {
        if numeric {
            return Ok(vec![RyuuGame {
                appid: query.clone(),
                name: format!("AppID {query}"),
                header_image: None,
                kind: None,
            }]);
        }
        return Err(error);
    }
    let guard = catalog_cache()
        .lock()
        .map_err(|_| "Каталог занят".to_string())?;
    let games = guard.as_ref().ok_or("Каталог не загружен")?;

    // Ранг: точное совпадение → начинается с → содержит.
    let mut scored: Vec<(u8, &RyuuGame)> = Vec::new();
    for game in games {
        let rank = if numeric {
            if game.appid == query {
                Some(0)
            } else if game.appid.starts_with(&query) {
                Some(1)
            } else {
                None
            }
        } else {
            let name = game.name.to_lowercase();
            if name == query {
                Some(0)
            } else if name.starts_with(&query) {
                Some(1)
            } else if name.contains(&query) {
                Some(2)
            } else if game.appid == query {
                Some(1)
            } else {
                None
            }
        };
        if let Some(rank) = rank {
            scored.push((rank, game));
        }
    }
    scored.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.name.len().cmp(&right.1.name.len()))
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    Ok(scored
        .into_iter()
        .take(24)
        .map(|(_, game)| game.clone())
        .collect())
}

/// Скачивает zip-пакет игры по её appid и раскладывает по папкам Steam (как ручной drop архива).
#[tauri::command]
pub async fn ryuu_get(
    app: AppHandle,
    appid: String,
    steam_path: Option<String>,
    auto_update: bool,
) -> LuaInstallResult {
    match tauri::async_runtime::spawn_blocking(move || {
        ryuu_get_blocking(app, appid, steam_path, auto_update)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => LuaInstallResult {
            success: false,
            message: format!("Загрузка прервалась: {error}"),
            logs: vec![],
            added_appids: vec![],
        },
    }
}

fn ryuu_get_blocking(
    app: AppHandle,
    appid: String,
    steam_path: Option<String>,
    auto_update: bool,
) -> LuaInstallResult {
    let clean: String = appid.chars().filter(char::is_ascii_digit).collect();
    if clean.is_empty() {
        return LuaInstallResult {
            success: false,
            message: "Некорректный AppID".into(),
            logs: vec![],
            added_appids: vec![],
        };
    }

    if AUTH_KEY.trim().is_empty() {
        return LuaInstallResult {
            success: false,
            message: "В публичной сборке ключ Ryuu не задан. Задайте CGS_RYUU_API_KEY перед сборкой или выберите HubCap в настройках.".into(),
            logs: vec![],
            added_appids: vec![],
        };
    }

    // http_get: системный DNS → при сетевом сбое DoH-обход. Загрузка идёт сырым zip (identity).
    let bytes = http_get(&format!("{DOWNLOAD_BASE}{clean}"), Some(AUTH_KEY), true);

    match bytes {
        Ok(bytes) => luatools::install_zip_bytes(&app, &bytes, steam_path, auto_update),
        Err(message) => LuaInstallResult {
            success: false,
            message,
            logs: vec![],
            added_appids: vec![],
        },
    }
}
