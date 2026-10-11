use crate::{dlc_unlocker_path, engine, models::OperationResult, steam};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

const CONFIG_NAME: &str = "cream_api.ini";
const NAMED_LIMIT: usize = 200;
const PARALLEL_REQUESTS: usize = 8;

/// Пары «библиотека Steam → её резервная копия CreamAPI».
const API_NAMES: &[(&str, &str, &str)] = &[
    ("steam_api64.dll", "steam_api64_o.dll", "x64"),
    ("steam_api.dll", "steam_api_o.dll", "x86"),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcTarget {
    pub dir: String,
    pub api: String,
    pub bitness: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcScan {
    pub game_dir: String,
    pub detected: bool,
    pub appid: Option<u64>,
    pub name: Option<String>,
    pub header_image: Option<String>,
    pub source: Option<String>,
    pub targets: Vec<DlcTarget>,
    pub installed: bool,
    pub dlcs: Vec<DlcEntry>,
    pub configured_dlcs: Vec<DlcEntry>,
    pub unlock_all: bool,
    pub exes: Vec<crate::smaki::GameExecutable>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcInstallRequest {
    pub game_dir: String,
    #[serde(default)]
    pub appid: u64,
    #[serde(default)]
    pub dlcs: BTreeMap<String, String>,
    #[serde(default)]
    pub unlock_all: bool,
    #[serde(default)]
    pub extra_protection: bool,
}

fn backup_for(api: &str) -> &'static str {
    API_NAMES
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(api))
        .map(|(_, backup, _)| *backup)
        .unwrap_or("steam_api64_o.dll")
}

fn bitness_for(api: &str) -> &'static str {
    API_NAMES
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(api))
        .map(|(_, _, bitness)| *bitness)
        .unwrap_or("x64")
}

/// Директории с оригинальной библиотекой Steam, пригодные для установки анлокера.
/// Пропускаются служебные каталоги дистрибутивов и уже сохранённые резервные копии `_o.dll`.
fn collect_targets(game_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut seen = BTreeMap::new();
    for entry in engine::walk(game_dir) {
        if !entry.file_type().is_file() || engine::ignored_distribution(entry.path()) {
            continue;
        }
        let lower = entry.file_name().to_string_lossy().to_lowercase();
        let matched = API_NAMES
            .iter()
            .find(|(name, _, _)| lower == *name)
            .map(|(name, _, _)| *name);
        let Some(canonical) = matched else { continue };
        if let Some(parent) = entry.path().parent() {
            seen.entry(parent.to_path_buf())
                .or_insert_with(|| canonical.to_string());
        }
    }
    seen.into_iter().collect()
}

fn is_unlocker_installed(dir: &Path, api: &str) -> bool {
    engine::find_case_insensitive(dir, CONFIG_NAME).is_some()
        && engine::find_case_insensitive(dir, backup_for(api)).is_some()
}

fn section_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    (trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2)
        .then(|| trimmed[1..trimmed.len() - 1].trim().to_lowercase())
}

fn key_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.starts_with(';') || trimmed.starts_with('#') {
        return None;
    }
    trimmed
        .split_once('=')
        .map(|(key, value)| (key.trim(), value.trim()))
}

/// Читает appid, флаг unlockall и список DLC из уже установленного `cream_api.ini`.
fn read_config(dir: &Path) -> Option<(Option<u64>, bool, BTreeMap<String, String>)> {
    let path = engine::find_case_insensitive(dir, CONFIG_NAME)?;
    let content = fs::read_to_string(path).ok()?;
    let mut section = String::new();
    let mut appid = None;
    let mut unlock_all = false;
    let mut dlcs = BTreeMap::new();
    for line in content.lines() {
        if let Some(name) = section_name(line) {
            section = name;
            continue;
        }
        let Some((key, value)) = key_value(line) else {
            continue;
        };
        match section.as_str() {
            "steam" if key.eq_ignore_ascii_case("appid") => {
                appid = value.parse::<u64>().ok().filter(|id| *id > 0);
            }
            "steam" if key.eq_ignore_ascii_case("unlockall") => {
                unlock_all = value.eq_ignore_ascii_case("true") || value == "1";
            }
            "dlc" => {
                if key.parse::<u64>().is_ok() {
                    dlcs.insert(key.to_string(), value.to_string());
                }
            }
            _ => {}
        }
    }
    Some((appid, unlock_all, dlcs))
}

/// Формирует `cream_api.ini`. По логике CreamAPI, если секция `[dlc]` не пуста, флаг `unlockall`
/// игнорируется и разблокируются только перечисленные DLC. Поэтому при наличии списка мы пишем его
/// целиком и выключаем `unlockall`; blanket-режим (`unlockall = true`, пустая секция) остаётся
/// запасным вариантом только когда список DLC неизвестен.
fn build_config(request: &DlcInstallRequest) -> String {
    let has_list = !request.dlcs.is_empty();
    let mut config = String::new();
    config.push_str("[steam]\r\n");
    config.push_str("; Generated by Crocodile Gena Studio — DLC Unlocker\r\n");
    config.push_str(&format!("appid = {}\r\n", request.appid));
    config.push_str(&format!(
        "unlockall = {}\r\n",
        if has_list { "false" } else { "true" }
    ));
    config.push_str("orgapi = steam_api_o.dll\r\n");
    config.push_str("orgapi64 = steam_api64_o.dll\r\n");
    config.push_str(&format!(
        "extraprotection = {}\r\n",
        if request.extra_protection {
            "true"
        } else {
            "false"
        }
    ));
    config.push_str("forceoffline = false\r\n");
    config.push_str("\r\n[steam_misc]\r\n");
    config.push_str("disableuserinterface = false\r\n");
    config.push_str("\r\n[dlc]\r\n");
    for (id, name) in &request.dlcs {
        let name = name.replace(['\r', '\n'], " ");
        config.push_str(&format!("{id} = {name}\r\n"));
    }
    config
}

fn named_dlcs(dlcs: BTreeMap<String, String>) -> Vec<DlcEntry> {
    let mut entries = dlcs
        .into_iter()
        .map(|(id, name)| DlcEntry { id, name })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.id.parse::<u64>().unwrap_or(u64::MAX));
    entries
}

/// Steam возвращает название только по одному AppID за запрос — имена DLC запрашиваются
/// небольшими параллельными группами.
fn resolve_names(ids: &[u64]) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    for chunk in ids.chunks(PARALLEL_REQUESTS) {
        let resolved = std::thread::scope(|scope| {
            let handles = chunk
                .iter()
                .map(|id| (*id, scope.spawn(move || steam::app_name(*id))))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|(id, handle)| (id, handle.join().ok().flatten()))
                .collect::<Vec<_>>()
        });
        for (id, name) in resolved {
            names.insert(id.to_string(), name.unwrap_or_else(|| format!("DLC {id}")));
        }
    }
    names
}

#[tauri::command]
pub async fn scan_dlc_target(game_dir: String) -> Result<DlcScan, String> {
    tauri::async_runtime::spawn_blocking(move || scan_dlc_target_blocking(game_dir))
        .await
        .map_err(|error| format!("Сканирование прервано: {error}"))?
}

fn scan_dlc_target_blocking(game_dir: String) -> Result<DlcScan, String> {
    let directory = PathBuf::from(&game_dir);
    if !directory.is_dir() {
        return Err("Папка игры не найдена".into());
    }
    let raw_targets = collect_targets(&directory);
    let mut targets = Vec::new();
    let mut installed_any = false;
    let mut configured = BTreeMap::new();
    let mut config_appid = None;
    let mut config_unlock_all = false;

    for (dir, api) in &raw_targets {
        let installed = is_unlocker_installed(dir, api);
        if installed {
            installed_any = true;
            if let Some((appid, unlock_all, dlcs)) = read_config(dir) {
                config_appid = config_appid.or(appid);
                config_unlock_all = config_unlock_all || unlock_all;
                configured.extend(dlcs);
            }
        }
        targets.push(DlcTarget {
            dir: engine::display_relative(dir, &directory),
            api: api.clone(),
            bitness: bitness_for(api).to_string(),
            installed,
        });
    }

    let exes = crate::smaki::executables(&directory);
    // Installed Steam games have an authoritative AppID in appmanifest_*.acf.
    // Do not guess a similarly named store result or perform slow name searches.
    if let Some(game) = crate::steam_library::installed_game_for_path(&directory) {
        return Ok(DlcScan {
            game_dir: game.game_dir,
            detected: true,
            appid: Some(game.appid),
            name: Some(game.name),
            header_image: Some(game.header_image),
            source: Some("Steam appmanifest".into()),
            targets,
            installed: installed_any,
            dlcs: Vec::new(),
            configured_dlcs: named_dlcs(configured),
            unlock_all: config_unlock_all,
            exes,
        });
    }
    let detected = steam::auto_detect_from_path(&directory);
    Ok(DlcScan {
        game_dir: directory.display().to_string(),
        detected: detected.detected,
        appid: detected.appid.or(config_appid),
        name: detected.name,
        header_image: detected.header_image,
        source: detected.source,
        targets,
        installed: installed_any,
        dlcs: named_dlcs(detected.dlcs),
        configured_dlcs: named_dlcs(configured),
        unlock_all: config_unlock_all,
        exes,
    })
}

#[tauri::command]
pub async fn fetch_dlc_catalog(appid: u64) -> Result<Vec<DlcEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || fetch_dlc_catalog_blocking(appid))
        .await
        .map_err(|error| format!("Запрос прерван: {error}"))?
}

fn fetch_dlc_catalog_blocking(appid: u64) -> Result<Vec<DlcEntry>, String> {
    if appid == 0 {
        return Err("Не указан AppID игры".into());
    }
    let ids = steam::dlc_ids(appid)?;
    let named = ids.len().min(NAMED_LIMIT);
    let mut catalog = resolve_names(&ids[..named]);
    for id in ids.iter().skip(named) {
        catalog.insert(id.to_string(), format!("DLC {id}"));
    }
    Ok(named_dlcs(catalog))
}

/// Устанавливает CreamAPI в один каталог: сохраняет оригинал как `_o.dll`, подменяет библиотеку
/// и записывает `cream_api.ini`. Оригинал сохраняется только если резервной копии ещё нет.
fn install_one(
    dir: &Path,
    api: &str,
    unlocker: &Path,
    config: &str,
    game_dir: &Path,
    logs: &mut Vec<String>,
) -> Result<(), String> {
    let target_dll = dir.join(api);
    let source = unlocker.join(api);
    if !source.is_file() {
        return Err(format!(
            "Компонент анлокера не найден: {}",
            source.display()
        ));
    }
    let backup = dir.join(backup_for(api));
    let current_is_cream = engine::files_equal(&target_dll, &source);

    if !backup.exists() {
        if current_is_cream {
            logs.push(format!(
                "⚠ Оригинал {} не найден (библиотека уже подменена), резервная копия не создана",
                engine::display_relative(&target_dll, game_dir)
            ));
        } else if target_dll.is_file() {
            fs::copy(&target_dll, &backup)
                .map_err(|error| format!("Не удалось сохранить оригинал: {error}"))?;
            if !engine::files_equal(&target_dll, &backup) {
                return Err(format!(
                    "Проверка резервной копии не пройдена: {}",
                    backup.display()
                ));
            }
            logs.push(format!(
                "Сохранён оригинал Steam API: {}",
                engine::display_relative(&backup, game_dir)
            ));
        }
    }

    if target_dll.exists() {
        engine::make_writable(&target_dll);
    }
    fs::copy(&source, &target_dll)
        .map_err(|error| format!("Не удалось установить {}: {error}", target_dll.display()))?;
    if !engine::files_equal(&source, &target_dll) {
        return Err(format!(
            "Проверка библиотеки не пройдена: {}",
            target_dll.display()
        ));
    }

    let config_path = dir.join(CONFIG_NAME);
    if config_path.exists() {
        engine::make_writable(&config_path);
    }
    fs::write(&config_path, config)
        .map_err(|error| format!("Не удалось записать cream_api.ini: {error}"))?;
    logs.push(format!(
        "Анлокер установлен: {} · {}",
        engine::display_relative(dir, game_dir),
        api
    ));
    Ok(())
}

#[tauri::command]
pub async fn install_dlc_unlocker(app: AppHandle, request: DlcInstallRequest) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || install_dlc_unlocker_blocking(app, request))
        .await
    {
        Ok(result) => result,
        Err(error) => OperationResult::error(format!("Установка прервана: {error}"), vec![]),
    }
}

fn install_dlc_unlocker_blocking(app: AppHandle, request: DlcInstallRequest) -> OperationResult {
    let game_dir = PathBuf::from(&request.game_dir);
    if !game_dir.is_dir() {
        return OperationResult::error("Папка игры не найдена", vec![]);
    }
    if request.appid == 0 {
        return OperationResult::error("Не определён Steam AppID игры", vec![]);
    }
    if request
        .dlcs
        .keys()
        .any(|id| id.parse::<u64>().ok().filter(|id| *id > 0).is_none())
    {
        return OperationResult::error("Некорректный AppID DLC", vec![]);
    }
    if !request.unlock_all && request.dlcs.is_empty() {
        return OperationResult::error("Выберите DLC или включите «Разблокировать все»", vec![]);
    }
    if crate::smaki::has_install(&game_dir) {
        return OperationResult::error(
            "Сначала удалите SmokeAPI кнопкой «Удалить анлокер», чтобы не смешивать два способа.",
            vec![],
        );
    }
    let targets = collect_targets(&game_dir);
    if targets.is_empty() {
        return OperationResult::error(
            "Не найдена библиотека steam_api.dll / steam_api64.dll — установка невозможна",
            vec![],
        );
    }

    let unlocker = dlc_unlocker_path(&app);
    let config = build_config(&request);
    let mut logs = Vec::new();
    let mut installed = 0usize;
    let mut errors = Vec::new();
    for (dir, api) in &targets {
        match install_one(dir, api, &unlocker, &config, &game_dir, &mut logs) {
            Ok(()) => installed += 1,
            Err(error) => {
                logs.push(format!("⚠ {error}"));
                errors.push(error);
            }
        }
    }

    if installed == 0 {
        return OperationResult::error("Не удалось установить DLC-анлокер", logs);
    }
    let mode = if request.unlock_all || request.dlcs.is_empty() {
        "все DLC".to_string()
    } else {
        format!("выбранных DLC: {}", request.dlcs.len())
    };
    if errors.is_empty() {
        OperationResult::ok(
            format!(
                "DLC-анлокер установлен в {installed} директори{} · {mode}",
                if installed == 1 { "ю" } else { "и" }
            ),
            logs,
        )
    } else {
        OperationResult::error(
            format!(
                "Анлокер установлен частично: {installed} готово, ошибок: {}",
                errors.len()
            ),
            logs,
        )
    }
}

/// Восстанавливает оригинальную библиотеку из `_o.dll` и удаляет `cream_api.ini`.
fn uninstall_one(
    dir: &Path,
    api: &str,
    game_dir: &Path,
    logs: &mut Vec<String>,
) -> Result<(), String> {
    let backup = dir.join(backup_for(api));
    let target = dir.join(api);
    if backup.is_file() {
        if fs::metadata(&backup)
            .map(|item| item.len() < 30_000)
            .unwrap_or(true)
        {
            return Err(format!(
                "Резервная копия подозрительно мала, восстановление отменено: {}",
                backup.display()
            ));
        }
        if target.exists() {
            engine::make_writable(&target);
        }
        fs::copy(&backup, &target)
            .map_err(|error| format!("Не удалось восстановить {}: {error}", target.display()))?;
        if !engine::files_equal(&backup, &target) {
            return Err(format!(
                "Проверка восстановления не пройдена: {}",
                target.display()
            ));
        }
        engine::make_writable(&backup);
        fs::remove_file(&backup).map_err(|error| {
            format!("Оригинал восстановлен, но резервную копию удалить не удалось: {error}")
        })?;
        logs.push(format!(
            "Восстановлена оригинальная библиотека: {}",
            engine::display_relative(&target, game_dir)
        ));
    }
    if let Some(config) = engine::find_case_insensitive(dir, CONFIG_NAME) {
        engine::make_writable(&config);
        fs::remove_file(&config)
            .map_err(|error| format!("Не удалось удалить cream_api.ini: {error}"))?;
        logs.push(format!(
            "Удалён cream_api.ini: {}",
            engine::display_relative(dir, game_dir)
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn uninstall_dlc_unlocker(game_dir: String) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || uninstall_dlc_unlocker_blocking(game_dir))
        .await
    {
        Ok(result) => result,
        Err(error) => OperationResult::error(format!("Удаление прервано: {error}"), vec![]),
    }
}

fn uninstall_dlc_unlocker_blocking(game_dir: String) -> OperationResult {
    let game_dir = PathBuf::from(&game_dir);
    if !game_dir.is_dir() {
        return OperationResult::error("Папка игры не найдена", vec![]);
    }
    let targets = collect_targets(&game_dir);
    let mut logs = Vec::new();
    let mut removed = 0usize;
    let mut errors = Vec::new();
    for (dir, api) in &targets {
        if !is_unlocker_installed(dir, api) {
            continue;
        }
        match uninstall_one(dir, api, &game_dir, &mut logs) {
            Ok(()) => removed += 1,
            Err(error) => {
                logs.push(format!("⚠ {error}"));
                errors.push(error);
            }
        }
    }
    if !errors.is_empty() {
        OperationResult::error(
            format!("Удаление выполнено не полностью. Ошибок: {}", errors.len()),
            logs,
        )
    } else if removed == 0 {
        OperationResult::ok("DLC-анлокер не обнаружен — удалять нечего", logs)
    } else {
        OperationResult::ok(
            format!("DLC-анлокер удалён, оригиналы восстановлены ({removed})"),
            logs,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(label: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("cgs-dlc-{label}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn provided_list_is_written_even_in_unlock_all_mode() {
        let request = DlcInstallRequest {
            game_dir: String::new(),
            appid: 42,
            dlcs: BTreeMap::from([("7".into(), "Pack".into())]),
            unlock_all: true,
            extra_protection: false,
        };
        let config = build_config(&request);
        assert!(config.contains("appid = 42"));
        // Список известен → он вписывается, а unlockall выключается (иначе CreamAPI его игнорирует).
        assert!(config.contains("unlockall = false"));
        assert!(config.contains("7 = Pack"));
    }

    #[test]
    fn selective_mode_lists_dlcs_and_disables_unlockall() {
        let request = DlcInstallRequest {
            game_dir: String::new(),
            appid: 42,
            dlcs: BTreeMap::from([("7".into(), "Pack".into()), ("9".into(), "Skin".into())]),
            unlock_all: false,
            extra_protection: true,
        };
        let config = build_config(&request);
        assert!(config.contains("unlockall = false"));
        assert!(config.contains("extraprotection = true"));
        assert!(config.contains("7 = Pack"));
        assert!(config.contains("9 = Skin"));
    }

    #[test]
    fn empty_selection_falls_back_to_unlock_all() {
        let request = DlcInstallRequest {
            game_dir: String::new(),
            appid: 1,
            dlcs: BTreeMap::new(),
            unlock_all: false,
            extra_protection: false,
        };
        assert!(build_config(&request).contains("unlockall = true"));
    }

    #[test]
    fn install_backs_up_original_and_writes_config() {
        let dir = test_dir("install");
        let unlocker = dir.join("unlocker");
        let game = dir.join("game");
        fs::create_dir_all(&unlocker).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(unlocker.join("steam_api64.dll"), b"CREAMAPI-LIBRARY-BYTES").unwrap();
        fs::write(game.join("steam_api64.dll"), b"ORIGINAL-VALVE-LIBRARY").unwrap();

        let request = DlcInstallRequest {
            game_dir: game.display().to_string(),
            appid: 5,
            dlcs: BTreeMap::new(),
            unlock_all: true,
            extra_protection: false,
        };
        let config = build_config(&request);
        let mut logs = Vec::new();
        install_one(
            &game,
            "steam_api64.dll",
            &unlocker,
            &config,
            &game,
            &mut logs,
        )
        .unwrap();

        assert_eq!(
            fs::read(game.join("steam_api64_o.dll")).unwrap(),
            b"ORIGINAL-VALVE-LIBRARY"
        );
        assert_eq!(
            fs::read(game.join("steam_api64.dll")).unwrap(),
            b"CREAMAPI-LIBRARY-BYTES"
        );
        assert!(game.join("cream_api.ini").is_file());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reinstall_keeps_first_original_backup() {
        let dir = test_dir("reinstall");
        let unlocker = dir.join("unlocker");
        let game = dir.join("game");
        fs::create_dir_all(&unlocker).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(unlocker.join("steam_api64.dll"), b"CREAMAPI-LIBRARY-BYTES").unwrap();
        fs::write(game.join("steam_api64.dll"), b"ORIGINAL-VALVE-LIBRARY").unwrap();
        let request = DlcInstallRequest {
            game_dir: game.display().to_string(),
            appid: 5,
            dlcs: BTreeMap::new(),
            unlock_all: true,
            extra_protection: false,
        };
        let config = build_config(&request);
        let mut logs = Vec::new();
        install_one(
            &game,
            "steam_api64.dll",
            &unlocker,
            &config,
            &game,
            &mut logs,
        )
        .unwrap();
        install_one(
            &game,
            "steam_api64.dll",
            &unlocker,
            &config,
            &game,
            &mut logs,
        )
        .unwrap();
        // Резервная копия по-прежнему содержит настоящий оригинал, а не CreamAPI.
        assert_eq!(
            fs::read(game.join("steam_api64_o.dll")).unwrap(),
            b"ORIGINAL-VALVE-LIBRARY"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn uninstall_restores_original_and_removes_config() {
        let dir = test_dir("uninstall");
        let game = dir.join("game");
        fs::create_dir_all(&game).unwrap();
        let original = vec![7u8; 40_000];
        fs::write(game.join("steam_api64.dll"), b"CREAMAPI-LIBRARY-BYTES").unwrap();
        fs::write(game.join("steam_api64_o.dll"), &original).unwrap();
        fs::write(game.join("cream_api.ini"), b"[steam]\nappid = 5\n").unwrap();

        let mut logs = Vec::new();
        uninstall_one(&game, "steam_api64.dll", &game, &mut logs).unwrap();
        assert_eq!(fs::read(game.join("steam_api64.dll")).unwrap(), original);
        assert!(!game.join("steam_api64_o.dll").exists());
        assert!(!game.join("cream_api.ini").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_back_written_config() {
        let dir = test_dir("roundtrip");
        let request = DlcInstallRequest {
            game_dir: String::new(),
            appid: 77,
            dlcs: BTreeMap::from([("3".into(), "Extra".into())]),
            unlock_all: false,
            extra_protection: false,
        };
        fs::write(dir.join("cream_api.ini"), build_config(&request)).unwrap();
        let (appid, unlock_all, dlcs) = read_config(&dir).unwrap();
        assert_eq!(appid, Some(77));
        assert!(!unlock_all);
        assert_eq!(dlcs.get("3").map(String::as_str), Some("Extra"));
        fs::remove_dir_all(dir).unwrap();
    }
}
