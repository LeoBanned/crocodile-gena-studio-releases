//! Shared pinned DLL selection. Schema 2 records exact relative destinations, including bin/dwmapi.dll.
use crate::{
    loader_github::{self, Bundle},
    luatools,
    models::OperationResult,
};
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
        _ => Err("Неизвестная версия загрузчика".into()),
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn validate_layout(
    variant: &str,
    schema: u32,
    paths: &BTreeMap<String, String>,
) -> Result<(), String> {
    resources(variant)?;
    if paths.len() != 3
        || !paths.contains_key("OpenSteamTool.dll")
        || !paths.contains_key("xinput1_4.dll")
    {
        return Err("Неполный комплект DLL".into());
    }
    let valid = match (schema, variant) {
        (1, _) => paths.contains_key("dwmapi.dll"),
        (2, "compatibility") => paths.contains_key("bin/dwmapi.dll"),
        (2, "standard") => paths.contains_key("dwmapi.dll") || paths.contains_key("version.dll"),
        _ => false,
    };
    if !valid {
        return Err("Неподдерживаемые пути DLL".into());
    }
    Ok(())
}
fn bundle_selection(bundle: &Bundle) -> Result<Selection, String> {
    let selection = Selection {
        schema: 2,
        variant: bundle.variant.clone(),
        hashes: bundle
            .files
            .iter()
            .map(|(p, b)| (p.clone(), hash(b)))
            .collect(),
    };
    validate_layout(&selection.variant, 2, &selection.hashes)?;
    Ok(selection)
}
fn embedded_bundle(variant: &str, schema: u32) -> Result<Bundle, String> {
    Ok(Bundle {
        variant: variant.into(),
        files: resources(variant)?
            .iter()
            .map(|(name, b)| {
                let target = if schema == 2 && variant == "compatibility" && *name == "dwmapi.dll" {
                    "bin/dwmapi.dll"
                } else {
                    *name
                };
                (target.into(), b.to_vec())
            })
            .collect(),
    })
}
fn bundled_selection(variant: &str) -> Result<Selection, String> {
    bundle_selection(&embedded_bundle(variant, 2)?)
}
pub(crate) fn reject_link(path: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        let mut link = metadata.file_type().is_symlink();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            link |= metadata.file_attributes() & 0x400 != 0;
        }
        if link {
            return Err(format!(
                "Проверка DLL отменена: {} является ссылкой.",
                path.display()
            ));
        }
    }
    Ok(())
}
pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    reject_link(path)?;
    if let Some(parent) = path.parent() {
        reject_link(parent)?;
    }
    if fs::metadata(path).is_ok_and(|m| m.len() > loader_github::MAX_DLL as u64) {
        return Err(format!("Слишком большой файл: {}", path.display()));
    }
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(
            "Не удалось прочитать {}: {e}. Закройте Steam и повторите.",
            path.display()
        )),
    }
}
fn parse_selection(bytes: &[u8]) -> Result<Selection, String> {
    if bytes.len() > 16384 {
        return Err("Слишком большой файл выбора DLL".into());
    }
    let value: Selection = serde_json::from_slice(bytes)
        .map_err(|_| "Файл выбора DLL повреждён. Выберите комплект заново через «Обновить DLL».")?;
    validate_layout(&value.variant, value.schema, &value.hashes)?;
    if value.hashes.values().any(|h| {
        h.len() != 64
            || !h
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    }) {
        return Err("Некорректный отпечаток закреплённой DLL".into());
    }
    Ok(value)
}
fn read_selection(steam: &Path) -> Result<Option<Selection>, String> {
    reject_link(&steam.join("config"))?;
    read_optional(&steam.join(SELECTION))?
        .map(|b| parse_selection(&b))
        .transpose()
}
fn detect_variant(steam: &Path) -> Result<&'static str, String> {
    if let Some(bytes) = read_optional(&steam.join("OpenSteamTool.dll"))? {
        return Ok(if bytes == COMPATIBILITY[0].1 {
            "compatibility"
        } else {
            "standard"
        });
    }
    if read_optional(&steam.join("bin/dwmapi.dll"))?.is_some_and(|b| b == COMPATIBILITY[1].1) {
        return Ok("compatibility");
    }
    Ok("standard")
}
pub(crate) fn selected_variant(steam: &Path) -> Result<String, String> {
    Ok(read_selection(steam)?
        .map(|s| s.variant)
        .unwrap_or(detect_variant(steam)?.into()))
}
fn matches_selection(steam: &Path, selection: &Selection) -> Result<bool, String> {
    let mut current = true;
    for (path, expected) in &selection.hashes {
        current &= read_optional(&steam.join(path))?.is_some_and(|b| hash(&b) == *expected);
    }
    Ok(current)
}
fn acquire_file_lock(steam: &Path) -> Result<fs::File, String> {
    reject_link(&steam.join("config"))?;
    fs::create_dir_all(steam.join("config")).map_err(|e| e.to_string())?;
    let path = steam.join("config/cgs-luatools-loader.lock");
    reject_link(&path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options
        .open(path)
        .map_err(|_| "Загрузчик занят другой операцией или плагином. Повторите позже.".into())
}
fn cache_dir(steam: &Path, selection: &Selection) -> Result<PathBuf, String> {
    Ok(steam
        .join("config/cgs-luatools-loader-cache")
        .join(&selection.variant)
        .join(hash(
            &serde_json::to_vec(selection).map_err(|e| e.to_string())?,
        )))
}
fn store_bundle(steam: &Path, selection: &Selection, bundle: &Bundle) -> Result<(), String> {
    let path = cache_dir(steam, selection)?;
    let mut parent = steam.to_path_buf();
    for component in path.strip_prefix(steam).unwrap().components() {
        parent.push(component);
        reject_link(&parent)?;
        fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    }
    for (target, bytes) in &bundle.files {
        let file = path.join(Path::new(target).file_name().unwrap());
        reject_link(&file)?;
        if read_optional(&file)?.as_deref() != Some(bytes) {
            fs::write(file, bytes).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn pinned_bundle(steam: &Path, selection: &Selection) -> Result<Bundle, String> {
    let embedded = embedded_bundle(&selection.variant, selection.schema)?;
    let expected = Selection {
        schema: selection.schema,
        variant: selection.variant.clone(),
        hashes: embedded
            .files
            .iter()
            .map(|(p, b)| (p.clone(), hash(b)))
            .collect(),
    };
    if expected == *selection {
        return Ok(embedded);
    }
    let path = cache_dir(steam, selection)?;
    reject_link(&path)?;
    let mut files = BTreeMap::new();
    for (target, expected) in &selection.hashes {
        let file = path.join(Path::new(target).file_name().unwrap());
        let bytes=read_optional(&file)?.ok_or("Закреплённый комплект повреждён и отсутствует в кэше. Автозамена запрещена. Выберите комплект через «Обновить DLL».")?;
        if hash(&bytes) != *expected {
            return Err("Кэш закреплённых DLL повреждён. Автозамена запрещена.".into());
        }
        files.insert(target.clone(), bytes);
    }
    Ok(Bundle {
        variant: selection.variant.clone(),
        files,
    })
}
fn apply_selection(
    steam: &Path,
    selection: Selection,
    bundle: Option<Bundle>,
    manual: bool,
) -> Result<bool, String> {
    validate_layout(&selection.variant, selection.schema, &selection.hashes)?;
    let current = matches_selection(steam, &selection)?;
    let previous = read_selection(steam).ok().flatten();
    let config = if manual && current {
        None
    } else {
        let bytes = read_optional(&steam.join("opensteamtool.toml"))?.unwrap_or_default();
        let content =
            String::from_utf8(bytes).map_err(|_| "opensteamtool.toml должен быть UTF-8")?;
        Some(luatools::configure_lua_folder(
            content.trim_start_matches('\u{feff}'),
        )?)
    };
    let bundle = if let Some(b) = bundle {
        Some(b)
    } else if !current {
        Some(pinned_bundle(steam, &selection)?)
    } else {
        None
    };
    if let Some(bundle) = &bundle {
        let hashes: BTreeMap<_, _> = bundle
            .files
            .iter()
            .map(|(p, b)| (p.clone(), hash(b)))
            .collect();
        if hashes != selection.hashes {
            return Err("Комплект не соответствует закреплённой версии".into());
        }
        store_bundle(steam, &selection, bundle)?;
    }
    let mut owned = BTreeMap::new();
    if let Some(old) = previous {
        owned = old.hashes;
    } else {
        // Adopt only known files from earlier app builds, never arbitrary third-party proxies.
        for name in ["dwmapi.dll", "version.dll", "bin/dwmapi.dll"] {
            if let Some(bytes) = read_optional(&steam.join(name))? {
                if [STANDARD, COMPATIBILITY]
                    .iter()
                    .any(|set| set.iter().any(|(_, b)| **b == bytes))
                {
                    owned.insert(name.into(), hash(&bytes));
                }
            }
        }
    }
    let mut changes: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
    if !current {
        for (path, bytes) in &bundle.as_ref().unwrap().files {
            changes.insert(path.clone(), Some(bytes.clone()));
        }
    }
    for (path, expected) in &owned {
        if !selection.hashes.contains_key(path) {
            if let Some(bytes) = read_optional(&steam.join(path))? {
                if hash(&bytes) != *expected {
                    return Err(format!(
                        "Прежняя DLL {path} изменена извне. Удаление отменено; файлы не заменены."
                    ));
                }
                changes.insert(path.clone(), None);
            }
        }
    }
    if let Some(config) = &config {
        fs::create_dir_all(luatools::stplugin_dir(steam)).map_err(|e| e.to_string())?;
        changes.insert(
            "opensteamtool.toml".into(),
            Some(config.as_bytes().to_vec()),
        );
    }
    // Marker is committed last, so failed file swaps cannot change the selected variant.
    let marker = read_optional(&steam.join(SELECTION))?;
    let marker_changed = marker
        .as_deref()
        .and_then(|b| parse_selection(b).ok())
        .as_ref()
        != Some(&selection);
    let marker_bytes = serde_json::to_vec_pretty(&selection).map_err(|e| e.to_string())?;
    let mut plan: Vec<_> = changes
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_deref()))
        .collect();
    if marker_changed {
        plan.push((SELECTION, Some(marker_bytes.as_slice())));
    }
    luatools::install_loader_changes(steam, &plan)?;
    if config.is_some() {
        luatools::migrate_legacy_lua(steam, &luatools::stplugin_dir(steam));
    }
    Ok(!current)
}
pub(crate) fn ensure(steam: &Path) -> Result<(), String> {
    let _guard = INSTALL_LOCK
        .lock()
        .map_err(|_| "Перезапустите программу после ошибки DLL")?;
    let _file = acquire_file_lock(steam)?;
    let saved = read_selection(steam)?;
    if let Some(selection) = saved {
        apply_selection(steam, selection, None, false).map(|_| ())
    } else {
        let variant = detect_variant(steam)?;
        let bundle = embedded_bundle(variant, 2)?;
        apply_selection(steam, bundle_selection(&bundle)?, Some(bundle), false).map(|_| ())
    }
}
fn install(steam: &Path, bundle: Bundle) -> OperationResult {
    if !steam.join("steam.exe").is_file() {
        return OperationResult::error(
            "steam.exe не найден в выбранной папке. DLL не изменены",
            vec![],
        );
    }
    let result = (|| {
        let _guard = INSTALL_LOCK
            .lock()
            .map_err(|_| "Перезапустите программу после ошибки DLL")?;
        let _file = acquire_file_lock(steam)?;
        apply_selection(steam, bundle_selection(&bundle)?, Some(bundle), true)
    })();
    #[cfg(windows)]
    if result.is_ok() {
        if let Err(error) = crate::millennium::update_installed_for_loader(steam) {
            return OperationResult::error(format!("DLL закреплены, но не удалось обновить плагин: {error}. Повторите действие до использования старого плагина."),vec![]);
        }
    }
    match result {
        Ok(false) => OperationResult::ok(
            "DLL уже актуальны. Выбранный комплект закреплён, DLL не переписаны",
            vec![],
        ),
        Ok(true) => OperationResult::ok(
            "DLL обновлены. Комплект закреплён. Полностью перезапустите Steam",
            vec!["Предыдущие файлы сохранены в config/cgs-luatools-backups".into()],
        ),
        Err(error) => OperationResult::error(error, vec![]),
    }
}
pub(crate) fn install_downloaded(steam: &Path, bundle: Bundle) -> OperationResult {
    install(steam, bundle)
}
#[cfg(test)]
pub(crate) fn update(steam: &Path, variant: Option<&str>) -> OperationResult {
    let variant = match variant {
        Some(value) => value.to_string(),
        None => match selected_variant(steam) {
            Ok(v) => v,
            Err(e) => return OperationResult::error(e, vec![]),
        },
    };
    match embedded_bundle(&variant, 2) {
        Ok(bundle) => install(steam, bundle),
        Err(e) => OperationResult::error(e, vec![]),
    }
}
pub(crate) fn resolve(steam_path: Option<String>) -> Result<PathBuf, String> {
    let path = match steam_path.filter(|s| !s.trim().is_empty()) {
        Some(s) => PathBuf::from(s.trim()),
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
        Ok(s) => (s, None),
        Err(e) => (None, Some(e)),
    };
    let variant = saved
        .as_ref()
        .map(|s| s.variant.clone())
        .unwrap_or(detect_variant(&steam)?.into());
    let expected = saved.clone().unwrap_or(bundled_selection(&variant)?);
    let current = match matches_selection(&steam, &expected) {
        Ok(v) => v,
        Err(e) => {
            issue = Some(e);
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
            let name = if variant == "compatibility" && name == "dwmapi.dll" {
                "bin/dwmapi.dll"
            } else {
                name
            };
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
            .map(|(name, _)| {
                fs::metadata(steam.join(if *name == "dwmapi.dll" {
                    "bin/dwmapi.dll"
                } else {
                    *name
                }))
                .unwrap()
                .modified()
                .unwrap()
            })
            .collect();
        ensure(steam).unwrap();
        assert_bundle(steam, "compatibility");
        for (index, &(name, _)) in COMPATIBILITY.iter().enumerate() {
            let name = if name == "dwmapi.dll" {
                "bin/dwmapi.dll"
            } else {
                name
            };
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
        fs::remove_file(steam.join("bin/dwmapi.dll")).unwrap();
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
        selected.hashes.clear();
        for &(name, _) in STANDARD {
            let name = if name == "dwmapi.dll" {
                "bin/dwmapi.dll"
            } else {
                name
            };
            fs::create_dir_all(steam.join(name).parent().unwrap()).unwrap();
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
        fs::remove_file(steam.join("bin/dwmapi.dll")).unwrap();
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
        assert_eq!(metadata["version"], "0.5.2");
        assert!(
            fs::read_to_string(plugin.join("backend/install-loader.ps1"))
                .unwrap()
                .contains("cgs-luatools-loader.json")
        );
        assert_bundle(steam, "compatibility");
    }
    #[cfg(windows)]
    #[test]
    fn proxy_removal_and_selection_are_rolled_back_if_a_later_dll_is_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = steam();
        let steam = dir.path();
        assert!(update(steam, Some("standard")).success);
        let saved = fs::read(steam.join(SELECTION)).unwrap();
        let mut bundle = embedded_bundle("standard", 2).unwrap();
        let proxy = bundle.files.remove("dwmapi.dll").unwrap();
        bundle.files.insert("version.dll".into(), proxy);
        let input = bundle.files.get_mut("xinput1_4.dll").unwrap();
        let last = input.len() - 1;
        input[last] ^= 1;
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(steam.join("xinput1_4.dll"))
            .unwrap();
        assert!(!install_downloaded(steam, bundle).success);
        assert_bundle(steam, "standard");
        assert!(!steam.join("version.dll").exists());
        assert_eq!(fs::read(steam.join(SELECTION)).unwrap(), saved);
    }
}
