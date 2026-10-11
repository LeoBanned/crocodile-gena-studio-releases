use crate::models::{DetectedGame, GameDetails, GameSummary};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};
use walkdir::WalkDir;

const DRM_CACHE_TTL_SECS: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamDrmInfo {
    pub appid: String,
    pub label: String,
    pub kind: String,
    pub notice: String,
    pub checked_at: u64,
}

fn drm_cache_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn drm_cache_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|directory| directory.join("steam-drm-cache.json"))
}

fn read_drm_cache(path: Option<&Path>) -> HashMap<String, SteamDrmInfo> {
    path.and_then(|path| fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn write_drm_cache(path: Option<&Path>, cache: &HashMap<String, SteamDrmInfo>) {
    let Some(path) = path else {
        return;
    };
    if let Some(directory) = path.parent() {
        let _ = fs::create_dir_all(directory);
    }
    if let Ok(content) = serde_json::to_vec(cache) {
        let _ = fs::write(path, content);
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn drm_label(drm_notice: &str, account_notice: &str) -> (String, String) {
    let combined = format!("{drm_notice} {account_notice}").to_lowercase();
    let mut labels = Vec::new();
    let mut kind = "unknown";

    if combined.contains("denuvo") {
        labels.push("Denuvo");
        kind = "denuvo";
    }
    if contains_any(&combined, &["ea app", "ea account", "origin"]) {
        labels.push(if drm_notice.to_lowercase().contains("ea app") {
            "EA App"
        } else {
            "EA Account"
        });
        if kind == "unknown" {
            kind = "launcher";
        }
    }
    if contains_any(&combined, &["ubisoft", "uplay"]) {
        labels.push("Ubisoft Connect");
        if kind == "unknown" {
            kind = "launcher";
        }
    }
    if combined.contains("rockstar") {
        labels.push("Rockstar");
        if kind == "unknown" {
            kind = "launcher";
        }
    }

    if labels.is_empty() && !drm_notice.is_empty() {
        if contains_any(&combined, &["steam drm", "steamworks drm"]) {
            labels.push("Steam DRM");
            kind = "steam";
        } else {
            labels.push("Сторонняя DRM");
            kind = "third-party";
        }
    } else if labels.is_empty() && !account_notice.is_empty() {
        labels.push("Сторонний аккаунт");
        kind = "third-party";
    }

    if labels.is_empty() {
        ("Не указано".into(), "unknown".into())
    } else {
        (labels.join(" + "), kind.into())
    }
}

fn drm_info_inner(appid: u64) -> Result<SteamDrmInfo, String> {
    let Some(data) = details_payload(appid)? else {
        return Err("Steam Store не вернул данные об игре".into());
    };
    let drm_notice = data
        .get("drm_notice")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let account_notice = data
        .get("ext_user_account_notice")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let (label, kind) = drm_label(drm_notice, account_notice);
    let notice = match (drm_notice.is_empty(), account_notice.is_empty()) {
        (false, false) => format!("{drm_notice}\n{account_notice}"),
        (false, true) => drm_notice.to_string(),
        (true, false) => account_notice.to_string(),
        (true, true) => "Steam не указывает сведения о защите для этой игры.".into(),
    };
    Ok(SteamDrmInfo {
        appid: appid.to_string(),
        label,
        kind,
        notice,
        checked_at: unix_time(),
    })
}

/// Кэш успешно полученных названий приложений (повторные открытия и общие DLC не перезапрашиваются).
/// Неудачи НЕ кэшируются, чтобы временные ошибки/rate-limit можно было повторить.
fn name_cache() -> &'static Mutex<HashMap<u64, String>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fetch_json(url: &str) -> Result<Value, String> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("Не удалось подготовить сетевой клиент: {error}"))?
        .get(url)
        .header("User-Agent", "Mozilla/5.0 CrocodileGenaStudio/3.0")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Steam Store API не ответил: {error}"))?
        .json::<Value>()
        .map_err(|error| format!("Steam Store вернул некорректные данные: {error}"))
}

fn encode_query(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (*byte as char).to_string()
            }
            b' ' => "%20".to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn clean_query(query: &str) -> String {
    query
        .chars()
        .filter(|character| {
            character.is_alphanumeric()
                || character.is_whitespace()
                || *character == '-'
                || *character == '_'
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn search_game_inner(query: &str) -> Result<Option<GameSummary>, String> {
    let clean = clean_query(query);
    if clean.chars().count() < 2 {
        return Ok(None);
    }
    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        encode_query(&clean)
    );
    let json = fetch_json(&url)?;
    let Some(item) = json
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return Ok(None);
    };
    let Some(appid) = item.get("id").and_then(Value::as_u64) else {
        return Ok(None);
    };
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Unknown game")
        .to_string();
    Ok(Some(GameSummary {
        appid,
        name,
        header_image: format!(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/header.jpg"
        ),
    }))
}

fn details_payload(appid: u64) -> Result<Option<Value>, String> {
    // cc=US обязателен: без региона Steam отдаёт success:false для многих регионально
    // ограниченных DLC (например, паков Street Fighter 6).
    let url =
        format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=english&cc=US");
    let json = fetch_json(&url)?;
    let Some(entry) = json.get(appid.to_string()) else {
        return Ok(None);
    };
    if !entry
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    Ok(entry.get("data").cloned())
}

fn get_dlc_names(ids: &[u64]) -> BTreeMap<String, String> {
    let mut dlcs = BTreeMap::new();
    for id in ids.iter().take(25) {
        let name = details_payload(*id)
            .ok()
            .flatten()
            .and_then(|data| data.get("name").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| format!("DLC {id}"));
        dlcs.insert(id.to_string(), name);
    }
    for id in ids.iter().skip(25) {
        dlcs.insert(id.to_string(), format!("DLC {id}"));
    }
    dlcs
}

pub(crate) fn dlc_ids(appid: u64) -> Result<Vec<u64>, String> {
    let Some(data) = details_payload(appid)? else {
        return Err("Steam Store не знает игру с таким AppID".into());
    };
    Ok(data
        .get("dlc")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default())
}

pub(crate) fn app_name(appid: u64) -> Option<String> {
    if let Some(cached) = name_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&appid).cloned())
    {
        return Some(cached);
    }
    let name = details_payload(appid)
        .ok()
        .flatten()
        .and_then(|data| data.get("name").and_then(Value::as_str).map(str::to_string));
    if let Some(ref name) = name {
        if let Ok(mut cache) = name_cache().lock() {
            cache.insert(appid, name.clone());
        }
    }
    name
}

/// Основные данные игры одним запросом, без последовательного резолва имён DLC.
pub(crate) struct AppOverview {
    pub name: String,
    pub header_image: Option<String>,
    pub release_date: Option<String>,
    pub dlc_ids: Vec<u64>,
}

pub(crate) fn app_overview(appid: u64) -> Option<AppOverview> {
    let data = details_payload(appid).ok().flatten()?;
    Some(AppOverview {
        name: data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown game")
            .to_string(),
        header_image: data
            .get("header_image")
            .and_then(Value::as_str)
            .map(str::to_string),
        release_date: data
            .get("release_date")
            .and_then(|value| value.get("date"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        dlc_ids: data
            .get("dlc")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default(),
    })
}

pub(crate) fn get_app_details_inner(appid: u64) -> Result<Option<GameDetails>, String> {
    let Some(data) = details_payload(appid)? else {
        return Ok(None);
    };
    let ids = data
        .get("dlc")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(Some(GameDetails {
        appid,
        name: data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown game")
            .to_string(),
        header_image: data
            .get("header_image")
            .and_then(Value::as_str)
            .map(str::to_string),
        short_description: data
            .get("short_description")
            .and_then(Value::as_str)
            .map(str::to_string),
        release_date: data
            .get("release_date")
            .and_then(|value| value.get("date"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        dlcs: get_dlc_names(&ids),
    }))
}

#[tauri::command]
pub async fn search_game(query: String) -> Result<Option<GameSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || search_game_inner(&query))
        .await
        .map_err(|error| format!("Поиск прерван: {error}"))?
}

#[tauri::command]
pub async fn get_app_details(appid: u64) -> Result<Option<GameDetails>, String> {
    tauri::async_runtime::spawn_blocking(move || get_app_details_inner(appid))
        .await
        .map_err(|error| format!("Запрос прерван: {error}"))?
}

#[tauri::command]
pub async fn steam_drm_info(
    app: AppHandle,
    appids: Vec<String>,
) -> Result<Vec<SteamDrmInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = drm_cache_lock()
            .lock()
            .map_err(|_| "Не удалось открыть кэш сведений о защите".to_string())?;
        let cache_path = drm_cache_path(&app);
        let mut cache = read_drm_cache(cache_path.as_deref());
        let now = unix_time();
        let mut results = Vec::new();
        let mut changed = false;
        let mut unique = Vec::new();

        for appid in appids.into_iter().take(12) {
            let Ok(parsed) = appid.parse::<u64>() else {
                continue;
            };
            let normalized = parsed.to_string();
            if !unique.contains(&normalized) {
                unique.push(normalized);
            }
        }

        for appid in unique {
            if let Some(cached) = cache.get(&appid) {
                if now.saturating_sub(cached.checked_at) < DRM_CACHE_TTL_SECS {
                    results.push(cached.clone());
                    continue;
                }
            }
            if let Ok(info) = drm_info_inner(appid.parse().unwrap_or_default()) {
                cache.insert(appid, info.clone());
                results.push(info);
                changed = true;
            }
        }

        if changed {
            write_drm_cache(cache_path.as_deref(), &cache);
        }
        Ok(results)
    })
    .await
    .map_err(|error| format!("Получение сведений о защите прервано: {error}"))?
}

pub fn auto_detect_from_path(game_path: &Path) -> DetectedGame {
    if !game_path.is_dir() {
        return DetectedGame::default();
    }

    for entry in WalkDir::new(game_path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("steam_appid.txt")
        {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Some(appid) = content
                    .split(|c: char| !c.is_ascii_digit())
                    .find_map(|part| part.parse::<u64>().ok())
                {
                    if let Ok(Some(details)) = get_app_details_inner(appid) {
                        return DetectedGame {
                            detected: true,
                            appid: Some(appid),
                            name: Some(details.name),
                            header_image: details.header_image,
                            dlcs: details.dlcs,
                            source: Some("steam_appid.txt".into()),
                        };
                    }
                }
            }
        }
    }

    let mut queries = Vec::new();
    if let Some(folder) = game_path.file_name().and_then(|name| name.to_str()) {
        queries.push(folder.to_string());
    }
    for entry in WalkDir::new(game_path)
        .max_depth(5)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let lower = name.to_lowercase();
        if lower.ends_with(".exe")
            && !["unins", "crash", "setup", "launcher", "dxsetup", "vcredist"]
                .iter()
                .any(|skip| lower.contains(skip))
        {
            if let Some(stem) = entry.path().file_stem().and_then(|value| value.to_str()) {
                queries.push(stem.to_string());
            }
        }
    }

    queries.sort();
    queries.dedup();
    for query in queries.into_iter().take(8) {
        if let Ok(Some(summary)) = search_game_inner(&query) {
            let details = get_app_details_inner(summary.appid).ok().flatten();
            return DetectedGame {
                detected: true,
                appid: Some(summary.appid),
                name: Some(summary.name),
                header_image: details
                    .as_ref()
                    .and_then(|item| item.header_image.clone())
                    .or(Some(summary.header_image)),
                dlcs: details.map(|item| item.dlcs).unwrap_or_default(),
                source: Some(format!("Steam Store · {query}")),
            };
        }
    }
    DetectedGame::default()
}

#[cfg(test)]
mod tests {
    use super::drm_label;

    #[test]
    fn classifies_known_protection_notices() {
        assert_eq!(drm_label("Denuvo", ""), ("Denuvo".into(), "denuvo".into()));
        assert_eq!(
            drm_label(
                "EA online activation and EA app software installation required.",
                "EA Account"
            ),
            ("EA App".into(), "launcher".into())
        );
        assert_eq!(
            drm_label("", "Rockstar Games (Supports Linking to Steam Account)"),
            ("Rockstar".into(), "launcher".into())
        );
        assert_eq!(drm_label("", ""), ("Не указано".into(), "unknown".into()));
    }
}
