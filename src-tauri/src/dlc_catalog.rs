use crate::{dlc::DlcEntry, luatools, steam_library};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DlcCatalog {
    pub entries: Vec<DlcEntry>,
    pub sources: Vec<String>,
    pub warnings: Vec<String>,
    pub online: bool,
}

fn numeric_id(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .filter(|id| *id > 0)
}

fn merge(target: &mut BTreeMap<String, String>, id: u64, name: Option<&str>, appid: u64) {
    if id == appid || id == 0 {
        return;
    }
    let key = id.to_string();
    let old = target.entry(key).or_insert_with(|| format!("DLC {id}"));
    if let Some(name) = name.filter(|s| !s.trim().is_empty()) {
        *old = name.to_string();
    }
}

fn store_ids(
    data: &Value,
    appid: u64,
    catalog: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let game = data
        .get(appid.to_string())
        .ok_or("Steam не вернул данные игры")?;
    if game.get("success").and_then(Value::as_bool) != Some(true) {
        return Err("Страница игры недоступна в Steam Store".into());
    }
    if let Some(ids) = game.pointer("/data/dlc").and_then(Value::as_array) {
        for id in ids.iter().filter_map(numeric_id) {
            merge(catalog, id, None, appid);
        }
    }
    Ok(())
}

fn store_list(
    data: &Value,
    appid: u64,
    catalog: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    if data.get("success").and_then(numeric_id) != Some(1)
        && data.get("success").and_then(Value::as_bool) != Some(true)
    {
        return Err("Каталог DLC Steam недоступен".into());
    }
    let list = data
        .get("dlcs")
        .and_then(Value::as_array)
        .ok_or("Steam вернул некорректный список DLC")?;
    for item in list {
        if let Some(id) = item.get("appid").and_then(numeric_id) {
            merge(catalog, id, item.get("name").and_then(Value::as_str), appid);
        }
    }
    Ok(())
}

fn scan(appid: u64, steam_path: Option<String>) -> Result<DlcCatalog, String> {
    if appid == 0 {
        return Err("Не указан AppID игры".into());
    }
    let mut catalog = BTreeMap::new();
    let mut sources = Vec::new();
    let mut warnings = Vec::new();
    if let Some(steam) = luatools::resolve_steam_path(steam_path) {
        match steam_library::read_appinfo(&steam) {
            Ok(apps) => {
                if apps.contains_key(&appid) {
                    sources.push("Steam appinfo".into());
                }
                catalog = steam_library::cached_dlc_names(&apps, appid);
            }
            Err(_) => warnings.push("Локальный кэш Steam недоступен".into()),
        }
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(4))
        .user_agent("CrocodileGenaStudio/5.1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let urls = [
        format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=english&cc=US"),
        format!("https://store.steampowered.com/dlc/{appid}/ajaxgetdlclist/?l=english&cc=US"),
    ];
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = urls
            .iter()
            .map(|url| {
                let client = &client;
                scope.spawn(move || {
                    client
                        .get(url)
                        .send()
                        .and_then(|r| r.error_for_status())
                        .and_then(|r| r.json::<Value>())
                        .map_err(|e| e.to_string())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("Запрос Steam прерван".into()))
            })
            .collect::<Vec<_>>()
    });
    let mut online = false;
    for (index, data) in results.into_iter().enumerate() {
        let result = data.and_then(|data| {
            if index == 0 {
                store_ids(&data, appid, &mut catalog)
            } else {
                store_list(&data, appid, &mut catalog)
            }
        });
        let name = if index == 0 {
            "Steam Store"
        } else {
            "Steam DLC list"
        };
        match result {
            Ok(()) => {
                online = true;
                sources.push(name.into());
            }
            Err(error) => warnings.push(format!("{name}: {error}")),
        }
    }
    if sources.is_empty() {
        return Err("Не удалось получить каталог DLC. Проверьте соединение со Steam и повторите сканирование.".into());
    }
    // Cached names and Steam's bulk listing are preferred. Bound per-DLC requests
    // so games with hundreds of DLC cannot hang the scan for several minutes.
    let unresolved: Vec<_> = catalog
        .iter()
        .filter(|(id, name)| **name == format!("DLC {id}"))
        .filter_map(|(id, _)| id.parse::<u64>().ok())
        .collect();
    let names_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .connect_timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    if online {
        for chunk in unresolved
            .iter()
            .take(48)
            .copied()
            .collect::<Vec<_>>()
            .chunks(8)
        {
            let names = std::thread::scope(|scope| {
                let handles: Vec<_> = chunk.iter().map(|id| { let client = &names_client; (*id, scope.spawn(move || client.get(format!("https://store.steampowered.com/api/appdetails?appids={id}&l=english&cc=US")).send().ok()?.json::<Value>().ok()?.get(id.to_string())?.pointer("/data/name")?.as_str().map(str::to_string))) }).collect();
                handles
                    .into_iter()
                    .map(|(id, h)| (id, h.join().ok().flatten()))
                    .collect::<Vec<_>>()
            });
            for (id, name) in names {
                merge(&mut catalog, id, name.as_deref(), appid);
            }
        }
    }
    if !online {
        warnings.push("Показан локальный кэш; новые DLC могут отсутствовать. Повторите сканирование при подключении к Steam.".into());
    }
    if catalog
        .iter()
        .any(|(id, name)| name == &format!("DLC {id}"))
    {
        warnings.push("Для части DLC доступен только AppID, название получить не удалось.".into());
    }
    let mut entries: Vec<_> = catalog
        .into_iter()
        .map(|(id, name)| DlcEntry { id, name })
        .collect();
    entries.sort_by_key(|e| e.id.parse::<u64>().unwrap_or(u64::MAX));
    Ok(DlcCatalog {
        entries,
        sources,
        warnings,
        online,
    })
}

#[tauri::command]
pub(crate) async fn scan_dlc_catalog(
    appid: u64,
    steam_path: Option<String>,
) -> Result<DlcCatalog, String> {
    tauri::async_runtime::spawn_blocking(move || scan(appid, steam_path))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merges_sources_without_duplicates_and_rejects_failed_store_response() {
        let mut catalog = BTreeMap::from([("11".into(), "Cached Pack".into())]);
        store_ids(
            &serde_json::json!({"10":{"success":true,"data":{"dlc":[11,12,12,10,0]}}}),
            10,
            &mut catalog,
        )
        .unwrap();
        store_list(&serde_json::json!({"success":1,"dlcs":[{"appid":"12","name":"New Pack"},{"appid":"13","name":"Hidden Pack"}],"soundtracks":[{"appid":"99"}]}), 10, &mut catalog).unwrap();
        assert_eq!(catalog.len(), 3);
        assert_eq!(catalog["11"], "Cached Pack");
        assert_eq!(catalog["12"], "New Pack");
        assert!(!catalog.contains_key("99"));
        assert!(store_ids(
            &serde_json::json!({"10":{"success":false}}),
            10,
            &mut catalog
        )
        .is_err());
    }
    #[test]
    fn reads_online_catalog_when_requested() {
        let Ok(id) = std::env::var("CGS_TEST_DLC_APPID") else {
            return;
        };
        let catalog = scan(id.parse().unwrap(), None).unwrap();
        assert!(!catalog.entries.is_empty());
        assert!(catalog.online);
        println!(
            "DLC count: {}; sources: {:?}; warnings: {:?}",
            catalog.entries.len(),
            catalog.sources,
            catalog.warnings
        );
    }
}
