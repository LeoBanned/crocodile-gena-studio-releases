use crate::{
    luatools::{self, LuaInstallResult},
    ryuu::RyuuGame,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::AppHandle;

const BASE: &str = "https://hubcapmanifest.com/api/v1";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<HubGame>,
}

#[derive(Deserialize)]
struct HubGame {
    game_id: String,
    #[serde(default)]
    game_name: Option<String>,
    #[serde(default)]
    header_image: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubcapUsage {
    #[serde(alias = "daily_usage")]
    pub daily_usage: u64,
    #[serde(alias = "daily_limit")]
    pub daily_limit: u64,
    #[serde(default)]
    pub remaining: u64,
}

fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("Не удалось подготовить сетевой клиент: {error}"))
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "%20".to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn map_status(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "Неверный или просроченный API-ключ HubCap".to_string(),
        404 => "Для этой игры нет манифеста в HubCap".to_string(),
        429 => "Достигнут дневной лимит HubCap".to_string(),
        other => format!("HubCap вернул ошибку {other}"),
    }
}

#[tauri::command]
pub async fn hubcap_usage(api_key: String) -> Result<HubcapUsage, String> {
    tauri::async_runtime::spawn_blocking(move || hubcap_usage_blocking(api_key))
        .await
        .map_err(|error| format!("Проверка лимита прервана: {error}"))?
}

fn hubcap_usage_blocking(api_key: String) -> Result<HubcapUsage, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Укажите API-ключ HubCap в настройках".into());
    }
    let response = client()?
        .get(format!("{BASE}/user/stats"))
        .bearer_auth(key)
        .send()
        .map_err(|error| format!("HubCap недоступен: {error}"))?;
    if !response.status().is_success() {
        return Err(map_status(response.status()));
    }
    let mut usage: HubcapUsage = response
        .json()
        .map_err(|error| format!("HubCap вернул некорректный ответ лимита: {error}"))?;
    usage.remaining = usage.daily_limit.saturating_sub(usage.daily_usage);
    Ok(usage)
}

#[tauri::command]
pub async fn hubcap_search(query: String, api_key: String) -> Result<Vec<RyuuGame>, String> {
    tauri::async_runtime::spawn_blocking(move || hubcap_search_blocking(query, api_key))
        .await
        .map_err(|error| format!("Поиск прерван: {error}"))?
}

fn hubcap_search_blocking(query: String, api_key: String) -> Result<Vec<RyuuGame>, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Укажите API-ключ HubCap в настройках".into());
    }
    let query = query.trim();
    // HubCap требует минимум 3 символа в запросе.
    if query.chars().count() < 3 {
        return Ok(Vec::new());
    }
    let numeric = query.chars().all(|c| c.is_ascii_digit());
    let url = format!(
        "{BASE}/search?q={}&limit=24{}",
        encode(query),
        if numeric { "&appid=true" } else { "" }
    );

    let response = client()?
        .get(url)
        .bearer_auth(key)
        .send()
        .map_err(|error| format!("HubCap недоступен: {error}"))?;
    if !response.status().is_success() {
        return Err(map_status(response.status()));
    }
    let parsed: SearchResponse = response
        .json()
        .map_err(|error| format!("HubCap вернул некорректный ответ: {error}"))?;

    Ok(parsed
        .results
        .into_iter()
        .map(|game| RyuuGame {
            name: game
                .game_name
                .unwrap_or_else(|| format!("AppID {}", game.game_id)),
            header_image: game.header_image,
            kind: None,
            appid: game.game_id,
        })
        .collect())
}

#[tauri::command]
pub async fn hubcap_get(
    app: AppHandle,
    appid: String,
    api_key: String,
    steam_path: Option<String>,
    auto_update: bool,
) -> LuaInstallResult {
    match tauri::async_runtime::spawn_blocking(move || {
        hubcap_get_blocking(app, appid, api_key, steam_path, auto_update)
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

fn hubcap_get_blocking(
    app: AppHandle,
    appid: String,
    api_key: String,
    steam_path: Option<String>,
    auto_update: bool,
) -> LuaInstallResult {
    let key = api_key.trim();
    if key.is_empty() {
        return LuaInstallResult {
            success: false,
            message: "Укажите API-ключ HubCap в настройках".into(),
            logs: vec![],
            added_appids: vec![],
        };
    }
    let clean: String = appid.chars().filter(char::is_ascii_digit).collect();
    if clean.is_empty() {
        return LuaInstallResult {
            success: false,
            message: "Некорректный AppID".into(),
            logs: vec![],
            added_appids: vec![],
        };
    }

    let bytes = (|| -> Result<Vec<u8>, String> {
        let client = client()?;
        let response = client
            .get(format!("{BASE}/manifest/{clean}"))
            .bearer_auth(key)
            // Готовый zip — без авто-декомпрессии.
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .map_err(|error| format!("HubCap недоступен: {error}"))?;
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        response
            .bytes()
            .map(|data| data.to_vec())
            .map_err(|error| format!("Не удалось получить файл: {error}"))
    })();

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
