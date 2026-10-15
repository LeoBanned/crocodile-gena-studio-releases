//! Shared, pinned loader selection. The Millennium installer uses the same JSON/lock contract.
use crate::{luatools, models::OperationResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub(crate) const STANDARD: &[(&str, &[u8])] = &[
    (
        "OpenSteamTool.dll",
        include_bytes!("assets/OpenSteamTool.dll"),
    ),
    ("dwmapi.dll", include_bytes!("assets/dwmapi.dll")),
    ("xinput1_4.dll", include_bytes!("assets/xinput1_4.dll")),
];
const COMPATIBILITY: &[(&str, &[u8])] = &[
    (
        "OpenSteamTool.dll",
        include_bytes!("assets/loader-v2/OpenSteamTool.dll"),
    ),
    ("dwmapi.dll", include_bytes!("assets/loader-v2/dwmapi.dll")),
    (
        "xinput1_4.dll",
        include_bytes!("assets/loader-v2/xinput1_4.dll"),
    ),
];
const SELECTION: &str = "config/cgs-luatools-loader.json";
static INSTALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Selection {
    schema: u32,
    variant: String,
    hashes: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderInfo {
    steam_path: String,
    selected_variant: String,
    pinned: bool,
    current: bool,
    issue: Option<String>,
}

fn resources(variant: &str) -> Result<&'static [(&'static str, &'static [u8])], String> {
    match variant {
        "standard" => Ok(STANDARD),
        "compatibility" => Ok(COMPATIBILITY),
        _ => Err("Неизвестная версия загрузчика. Выберите версию 1 или 2.".into()),
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bundled_selection(variant: &str) -> Result<Selection, String> {
    Ok(Selection {
        schema: 1,
        variant: variant.into(),
        hashes: resources(variant)?
            .iter()
            .map(|(name, bytes)| ((*name).into(), hash(bytes)))
            .collect(),
    })
}

fn reject_link(path: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        let mut link = metadata.file_type().is_symlink();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            link |= metadata.file_attributes() & 0x400 != 0;
        }
        if link {
            return Err(format!(
                "Проверка загрузчика отменена: {} является ссылкой.",
                path.display()
            ));
        }
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    reject_link(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Не удалось прочитать {}: {error}. Закройте Steam и повторите.",
            path.display()
        )),
    }
}

fn read_selection(steam: &Path) -> Result<Option<Selection>, String> {
    reject_link(&steam.join("config"))?;
    let path = steam.join(SELECTION);
    if fs::metadata(&path).is_ok_and(|m| m.len() > 16384) {
        return Err(
            "Файл выбора DLL повреждён. Выберите версию заново через «Обновить DLL».".into(),
        );
    }
    let Some(bytes) = read_optional(&path)? else {
        return Ok(None);
    };
    let parsed: Selection = serde_json::from_slice(&bytes).map_err(|_| {
        "Файл выбора DLL повреждён. Выберите версию заново через «Обновить DLL».".to_string()
    })?;
    if parsed.schema != 1
        || resources(&parsed.variant).is_err()
        || parsed.hashes.len() != 3
        || STANDARD.iter().any(|(name, _)| {
            !parsed.hashes.get(*name).is_some_and(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            })
        })
    {
        return Err("Файл выбора DLL имеет неподдерживаемый формат. Автозамена запрещена; выберите версию вручную.".into());
    }
    Ok(Some(parsed))
}

fn detect_variant(steam: &Path) -> Result<&'static str, String> {
    // Detect existing version 2 even before this app has created its preference file.
    if let Some(bytes) = read_optional(&steam.join("OpenSteamTool.dll"))? {
        if bytes == COMPATIBILITY[0].1 {
            return Ok("compatibility");
        }
        return Ok("standard");
    }
    for &(name, expected) in &COMPATIBILITY[1..] {
        if read_optional(&steam.join(name))?.is_some_and(|bytes| bytes == expected) {
            return Ok("compatibility");
        }
    }
    Ok("standard")
}

fn matches_selection(steam: &Path, selection: &Selection) -> Result<bool, String> {
    let mut matches = true;
    for &(name, _) in STANDARD {
        matches &= read_optional(&steam.join(name))?
            .is_some_and(|bytes| Some(&hash(&bytes)) == selection.hashes.get(name));
    }
    Ok(matches)
}

fn acquire_file_lock(steam: &Path) -> Result<fs::File, String> {
    reject_link(&steam.join("config"))?;
    fs::create_dir_all(steam.join("config"))
        .map_err(|e| format!("Не удалось создать папку конфигурации Steam: {e}"))?;
    let path = steam.join("config/cgs-luatools-loader.lock");
    reject_link(&path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(path).map_err(|_| {
        "Загрузчик занят другой операцией или плагином. Дождитесь её окончания и повторите.".into()
    })
}

fn apply_locked(steam: &Path, explicit: Option<&str>) -> Result<bool, String> {
    let saved = if explicit.is_none() {
        read_selection(steam)?
    } else {
        None
    };
    let variant = match explicit {
        Some(value) => {
            resources(value)?;
            value.to_string()
        }
        None => saved
            .as_ref()
            .map(|s| s.variant.clone())
            .unwrap_or(detect_variant(steam)?.into()),
    };
    let bundled = bundled_selection(&variant)?;
    let selection = saved.as_ref().unwrap_or(&bundled);
    let current = matches_selection(steam, selection)?;
    if !current && saved.is_some() && selection != &bundled {
        return Err("Закреплённые DLL отличаются от встроенного комплекта и требуют восстановления. Автозамена запрещена. Откройте «Обновить DLL» и явно выберите нужную версию.".into());
    }
    let mut files = if current {
        Vec::new()
    } else {
        resources(&variant)?.to_vec()
    };
    // A manual no-op must not parse or rewrite a custom config merely to pin a choice.
    let config = if explicit.is_some() && current {
        None
    } else {
        let previous = read_optional(&steam.join("opensteamtool.toml"))?.unwrap_or_default();
        let previous = String::from_utf8(previous).map_err(|_| {
            "opensteamtool.toml должен быть в UTF-8. Конфиг не изменён.".to_string()
        })?;
        Some(luatools::configure_lua_folder(
            previous.trim_start_matches('\u{feff}'),
        )?)
    };
    if let Some(config) = &config {
        files.push(("opensteamtool.toml", config.as_bytes()));
    }
    let selection_bytes = serde_json::to_vec_pretty(selection).map_err(|e| e.to_string())?;
    // Keep the exact original marker bytes when it is already valid: no timestamp churn.
    let old_selection = read_optional(&steam.join(SELECTION))?;
    if old_selection
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Selection>(bytes).ok())
        .as_ref()
        != Some(selection)
    {
        files.push((SELECTION, &selection_bytes));
    }
    if config.is_some() {
        let target = luatools::stplugin_dir(steam);
        fs::create_dir_all(&target).map_err(|e| format!("Не удалось создать config/lua: {e}"))?;
    }
    luatools::install_loader_resources(steam, &files)?;
    if config.is_some() {
        let target = luatools::stplugin_dir(steam);
        luatools::migrate_legacy_lua(steam, &target);
    }
    Ok(!current)
}

pub(crate) fn ensure(steam: &Path) -> Result<(), String> {
    let _guard = INSTALL_LOCK
        .lock()
        .map_err(|_| "Предыдущее обновление DLL прервано. Перезапустите программу.".to_string())?;
    let _file = acquire_file_lock(steam)?;
    apply_locked(steam, None).map(|_| ())
}

pub(crate) fn update(steam: &Path, variant: Option<&str>) -> OperationResult {
    if !steam.join("steam.exe").is_file() {
        return OperationResult::error(
            "steam.exe не найден в выбранной папке. DLL не изменены",
            vec![],
        );
    }
    // Validate the explicit choice before creating any directories or files.
    if let Some(variant) = variant {
        if let Err(error) = resources(variant) {
            return OperationResult::error(error, vec![]);
        }
    }
    let result = (|| {
        let _guard = INSTALL_LOCK.lock().map_err(|_| {
            "Предыдущее обновление DLL прервано. Перезапустите программу.".to_string()
        })?;
        let _file = acquire_file_lock(steam)?;
        let choice = match variant {
            Some(value) => value.to_string(),
            None => read_selection(steam)?
                .map(|s| s.variant)
                .unwrap_or(detect_variant(steam)?.into()),
        };
        apply_locked(steam, Some(&choice))
    })();
    #[cfg(windows)]
    if result.is_ok() {
        if let Err(error) = crate::millennium::update_installed_for_loader(steam) {
            return OperationResult::error(format!("Версия DLL закреплена, но не удалось обновить установленный плагин: {error}. Не добавляйте манифесты через старый плагин до его обновления. Повторите установку в этом окне."), vec![]);
        }
    }
    match result {
        Ok(false) => OperationResult::ok(
            "DLL уже актуальны. Выбранная версия закреплена, DLL не переписаны",
            vec![],
        ),
        Ok(true) => OperationResult::ok(
            "DLL обновлены. Версия закреплена. Полностью перезапустите Steam",
            vec![
                format!("Папка Steam: {}", steam.display()),
                "Комплект: OpenSteamTool.dll · dwmapi.dll · xinput1_4.dll".into(),
                "Предыдущие файлы сохранены в config/cgs-luatools-backups".into(),
            ],
        ),
        Err(error) => OperationResult::error(error, vec![]),
    }
}

fn resolve(steam_path: Option<String>) -> Result<PathBuf, String> {
    let path = match steam_path.filter(|s| !s.trim().is_empty()) {
        Some(value) => PathBuf::from(value.trim()),
        None => luatools::resolve_steam_path(None).ok_or("Папка Steam не найдена")?,
    };
    if !path.join("steam.exe").is_file() {
        return Err("В выбранной папке не найден steam.exe".into());
    }
    Ok(path)
}

#[tauri::command]
pub fn luatools_loader_info(steam_path: Option<String>) -> Result<LoaderInfo, String> {
    let steam = resolve(steam_path)?;
    let (saved, mut issue) = match read_selection(&steam) {
        Ok(value) => (value, None),
        Err(error) => (None, Some(error)),
    };
    let variant = saved
        .as_ref()
        .map(|s| s.variant.clone())
        .unwrap_or(detect_variant(&steam)?.into());
    let expected = saved.clone().unwrap_or(bundled_selection(&variant)?);
    let current = match matches_selection(&steam, &expected) {
        Ok(value) => value,
        Err(error) => {
            issue = Some(error);
            false
        }
    };
    Ok(LoaderInfo {
        steam_path: steam.display().to_string(),
        selected_variant: variant,
        pinned: saved.is_some(),
        current,
        issue,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn steam() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("steam.exe"), []).unwrap();
        dir
    }
    fn assert_bundle(steam: &Path, variant: &str) {
        for &(name, bytes) in resources(variant).unwrap() {
            assert_eq!(
                hash(&fs::read(steam.join(name)).unwrap()),
                hash(bytes),
                "{name}"
            );
        }
        assert_eq!(read_selection(steam).unwrap().unwrap().variant, variant);
    }
    #[test]
    fn second_version_is_pinned_and_automatic_install_does_not_rewrite_it() {
        let dir = steam();
        let steam = dir.path();
        assert!(update(steam, Some("compatibility")).success);
        let times: Vec<_> = COMPATIBILITY
            .iter()
            .map(|(name, _)| fs::metadata(steam.join(name)).unwrap().modified().unwrap())
            .collect();
        ensure(steam).unwrap();
        assert_bundle(steam, "compatibility");
        for (index, &(name, _)) in COMPATIBILITY.iter().enumerate() {
            assert_eq!(
                fs::metadata(steam.join(name)).unwrap().modified().unwrap(),
                times[index]
            );
        }
    }
    #[test]
    fn switching_both_directions_is_explicit_and_backed_up() {
        let dir = steam();
        let steam = dir.path();
        assert!(update(steam, Some("standard")).success);
        assert!(update(steam, Some("compatibility")).success);
        assert_bundle(steam, "compatibility");
        assert!(fs::read_dir(steam.join("config/cgs-luatools-backups"))
            .unwrap()
            .flatten()
            .any(|entry| fs::read(entry.path().join("OpenSteamTool.dll"))
                .is_ok_and(|bytes| bytes == STANDARD[0].1)));
        assert!(update(steam, Some("standard")).success);
        ensure(steam).unwrap();
        assert_bundle(steam, "standard");
    }
    #[test]
    fn detects_existing_second_version_without_a_preference_file() {
        let dir = steam();
        let steam = dir.path();
        for &(name, bytes) in COMPATIBILITY {
            fs::write(steam.join(name), bytes).unwrap();
        }
        ensure(steam).unwrap();
        assert_bundle(steam, "compatibility");
    }
    #[test]
    fn repairs_missing_files_only_from_the_pinned_variant() {
        let dir = steam();
        let steam = dir.path();
        assert!(update(steam, Some("compatibility")).success);
        fs::remove_file(steam.join("dwmapi.dll")).unwrap();
        fs::write(steam.join("xinput1_4.dll"), STANDARD[2].1).unwrap();
        ensure(steam).unwrap();
        assert_bundle(steam, "compatibility");
    }
    #[test]
    fn keeps_pinned_bytes_even_when_embedded_resources_change() {
        let dir = steam();
        let steam = dir.path();
        fs::create_dir_all(steam.join("config")).unwrap();
        let mut selected = bundled_selection("compatibility").unwrap();
        for &(name, _) in STANDARD {
            let bytes = format!("pinned older release: {name}").into_bytes();
            fs::write(steam.join(name), &bytes).unwrap();
            selected.hashes.insert(name.into(), hash(&bytes));
        }
        fs::write(
            steam.join(SELECTION),
            serde_json::to_vec(&selected).unwrap(),
        )
        .unwrap();
        ensure(steam).unwrap();
        assert!(matches_selection(steam, &selected).unwrap());
        fs::remove_file(steam.join("dwmapi.dll")).unwrap();
        let before = fs::read(steam.join("OpenSteamTool.dll")).unwrap();
        assert!(ensure(steam).unwrap_err().contains("Автозамена запрещена"));
        assert_eq!(fs::read(steam.join("OpenSteamTool.dll")).unwrap(), before);
        assert!(update(steam, Some("compatibility")).success);
        assert_bundle(steam, "compatibility");
    }
    #[test]
    fn damaged_preference_never_silently_falls_back_to_version_one() {
        let dir = steam();
        let steam = dir.path();
        assert!(update(steam, Some("compatibility")).success);
        fs::write(steam.join(SELECTION), b"damaged JSON").unwrap();
        assert!(ensure(steam).is_err());
        assert_eq!(
            fs::read(steam.join("OpenSteamTool.dll")).unwrap(),
            COMPATIBILITY[0].1
        );
        assert!(update(steam, Some("compatibility")).success);
        assert_bundle(steam, "compatibility");
    }
    #[test]
    fn invalid_variant_has_no_side_effects() {
        let dir = steam();
        let steam = dir.path();
        assert!(!update(steam, Some("../other")).success);
        assert!(!steam.join("config").exists());
    }
    #[test]
    fn status_and_cancel_are_read_only() {
        let dir = steam();
        let steam = dir.path();
        let info = luatools_loader_info(Some(steam.display().to_string())).unwrap();
        assert_eq!(info.selected_variant, "standard");
        assert!(!info.pinned);
        assert_eq!(fs::read_dir(steam).unwrap().count(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn locked_switch_restores_old_files_and_keeps_old_selection() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = steam();
        let steam = dir.path();
        assert!(update(steam, Some("standard")).success);
        let saved = fs::read(steam.join(SELECTION)).unwrap();
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(steam.join("dwmapi.dll"))
            .unwrap();
        let result = update(steam, Some("compatibility"));
        assert!(!result.success);
        assert_bundle(steam, "standard");
        assert_eq!(fs::read(steam.join(SELECTION)).unwrap(), saved);
    }
    #[cfg(windows)]
    #[test]
    fn shared_lock_prevents_concurrent_plugin_or_app_updates() {
        let dir = steam();
        let steam = dir.path();
        let _file = acquire_file_lock(steam).unwrap();
        assert!(ensure(steam).unwrap_err().contains("занят"));
        assert!(!steam.join("OpenSteamTool.dll").exists());
    }
    #[cfg(windows)]
    #[test]
    fn native_selection_is_respected_by_the_real_plugin_installer() {
        let dir = steam();
        let steam = dir.path();
        assert!(update(steam, Some("compatibility")).success);
        let backend =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/crocodile-manifest/backend");
        let mut command = std::process::Command::new("powershell");
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        let result = command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(backend.join("install-loader.ps1"))
            .arg("-SteamPath")
            .arg(steam)
            .arg("-ResourcePath")
            .arg(backend.join("loader"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_bundle(steam, "compatibility");
    }
    #[cfg(windows)]
    #[test]
    fn manual_selection_upgrades_an_existing_old_official_plugin() {
        let dir = steam();
        let steam = dir.path();
        fs::create_dir_all(steam.join("millennium/lib")).unwrap();
        fs::create_dir_all(steam.join("millennium/bin")).unwrap();
        for name in [
            "wsock32.dll",
            "millennium/lib/millennium.dll",
            "millennium/bin/millennium.luavm64.exe",
        ] {
            fs::write(steam.join(name), []).unwrap();
        }
        let plugin = steam.join("millennium/plugins/crocodile-manifest");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join("plugin.json"),
            br#"{"name":"crocodile-manifest","version":"0.5.0"}"#,
        )
        .unwrap();
        let result = update(steam, Some("compatibility"));
        assert!(result.success, "{}", result.message);
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(plugin.join("plugin.json")).unwrap()).unwrap();
        assert_eq!(metadata["version"], "0.5.1");
        assert!(
            fs::read_to_string(plugin.join("backend/install-loader.ps1"))
                .unwrap()
                .contains("cgs-luatools-loader.json")
        );
        assert_bundle(steam, "compatibility");
    }
}
