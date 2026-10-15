use crate::{luatools::resolve_steam_path, models::OperationResult};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

const PLUGIN_FOLDER_NAME: &str = "crocodile-manifest";
const BUNDLED_PLUGIN_VERSION: &str = "0.5.1";

const EMBEDDED_MILLENNIUM_FILES: &[(&str, &[u8])] = &[
    ("wsock32.dll", include_bytes!("../../millenium/wsock32.dll")),
    (
        "millennium/bin/millennium.crashhandler64.exe",
        include_bytes!("../../millenium/millennium/bin/millennium.crashhandler64.exe"),
    ),
    (
        "millennium/bin/millennium.luavm64.exe",
        include_bytes!("../../millenium/millennium/bin/millennium.luavm64.exe"),
    ),
    (
        "millennium/lib/millennium.dll",
        include_bytes!("../../millenium/millennium/lib/millennium.dll"),
    ),
    (
        "millennium/lib/millennium.hhx64.dll",
        include_bytes!("../../millenium/millennium/lib/millennium.hhx64.dll"),
    ),
];

const EMBEDDED_PLUGIN_FILES: &[(&str, &[u8])] = &[
    (
        "backend/install-loader.ps1",
        include_bytes!("assets/crocodile-manifest/backend/install-loader.ps1"),
    ),
    ("backend/loader/OpenSteamTool.dll", include_bytes!("assets/OpenSteamTool.dll")),
    ("backend/loader/dwmapi.dll", include_bytes!("assets/dwmapi.dll")),
    ("backend/loader/xinput1_4.dll", include_bytes!("assets/xinput1_4.dll")),
    ("backend/loader/compatibility/OpenSteamTool.dll", include_bytes!("assets/loader-v2/OpenSteamTool.dll")),
    ("backend/loader/compatibility/dwmapi.dll", include_bytes!("assets/loader-v2/dwmapi.dll")),
    ("backend/loader/compatibility/xinput1_4.dll", include_bytes!("assets/loader-v2/xinput1_4.dll")),
    ("backend/loader/opensteamtool.toml", include_bytes!("assets/opensteamtool.toml")),
    (
        "plugin.json",
        include_bytes!("assets/crocodile-manifest/plugin.json"),
    ),
    (
        "backend/main.lua",
        include_bytes!("assets/crocodile-manifest/backend/main.lua"),
    ),
    (
        ".millennium/Dist/index.js",
        include_bytes!("assets/crocodile-manifest/.millennium/Dist/index.js"),
    ),
    (
        ".millennium/Dist/webkit.js",
        include_bytes!("assets/crocodile-manifest/.millennium/Dist/webkit.js"),
    ),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MillenniumPluginStatus {
    pub steam_path: Option<String>,
    pub millennium_installed: bool,
    pub millennium_path: Option<String>,
    pub plugin_path: Option<String>,
    pub plugin_installed: bool,
    pub plugin_current: bool,
    pub plugin_version: Option<String>,
    pub bundled_version: String,
    pub status: String,
    pub message: String,
}

fn millennium_dir(steam: &Path) -> PathBuf {
    steam.join("millennium")
}

fn plugin_dir(steam: &Path) -> PathBuf {
    millennium_dir(steam)
        .join("plugins")
        .join(PLUGIN_FOLDER_NAME)
}

fn millennium_is_installed(steam: &Path) -> bool {
    (steam.join("wsock32.dll").is_file()
        && millennium_dir(steam)
            .join("lib")
            .join("millennium.dll")
            .is_file()
        && millennium_dir(steam)
            .join("bin")
            .join("millennium.luavm64.exe")
            .is_file())
        || steam.join("millennium.dll").is_file()
        || steam.join("user32.dll").is_file()
}

fn installed_plugin_version(plugin: &Path) -> Option<String> {
    let data = fs::read(plugin.join("plugin.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&data).ok()?;
    value
        .get("version")
        .and_then(|version| version.as_str())
        .map(str::to_owned)
}

fn installed_plugin_name(plugin: &Path) -> Option<String> {
    let data = fs::read(plugin.join("plugin.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&data).ok()?;
    value
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_owned)
}

fn status_for(steam_path: Option<String>) -> MillenniumPluginStatus {
    let Some(steam) = resolve_steam_path(steam_path) else {
        return MillenniumPluginStatus {
            steam_path: None,
            millennium_installed: false,
            millennium_path: None,
            plugin_path: None,
            plugin_installed: false,
            plugin_current: false,
            plugin_version: None,
            bundled_version: BUNDLED_PLUGIN_VERSION.into(),
            status: "steam_missing".into(),
            message: "Сначала укажите папку Steam".into(),
        };
    };

    let millennium = millennium_dir(&steam);
    let plugin = plugin_dir(&steam);
    let millennium_installed = millennium_is_installed(&steam);
    let plugin_version = installed_plugin_version(&plugin);
    let plugin_installed = plugin.join("plugin.json").is_file();
    let plugin_current = plugin_version.as_deref() == Some(BUNDLED_PLUGIN_VERSION);

    let (status, message) = if !steam.join("steam.exe").is_file() {
        ("steam_invalid", "В выбранной папке не найден steam.exe")
    } else if !millennium_installed {
        (
            "millennium_missing",
            "Millennium не установлен. Программа может установить встроенные файлы напрямую в Steam",
        )
    } else if !plugin_installed {
        (
            "plugin_missing",
            "Millennium найден — плагин готов к установке",
        )
    } else if !plugin_current {
        (
            "update_available",
            "Доступна новая встроенная версия плагина",
        )
    } else {
        ("ready", "Crocodile Manifest установлен и готов к работе")
    };

    MillenniumPluginStatus {
        steam_path: Some(steam.display().to_string()),
        millennium_installed,
        millennium_path: Some(millennium.display().to_string()),
        plugin_path: Some(plugin.display().to_string()),
        plugin_installed,
        plugin_current,
        plugin_version,
        bundled_version: BUNDLED_PLUGIN_VERSION.into(),
        status: status.into(),
        message: message.into(),
    }
}

#[tauri::command]
pub fn millennium_plugin_status(steam_path: Option<String>) -> MillenniumPluginStatus {
    status_for(steam_path)
}

#[tauri::command]
pub fn millennium_install_plugin(steam_path: Option<String>) -> OperationResult {
    let Some(steam) = resolve_steam_path(steam_path) else {
        return OperationResult::error("Папка Steam не найдена", vec![]);
    };
    if !steam.join("steam.exe").is_file() {
        return OperationResult::error("steam.exe не найден в выбранной папке", vec![]);
    }
    if !millennium_is_installed(&steam) {
        return OperationResult::error(
            "Millennium не установлен. Сначала установите встроенный комплект Millennium",
            vec![],
        );
    }

    if let Err(message) = crate::luatools::ensure_opensteamtool_installed(&steam) {
        return OperationResult::error(message, vec![]);
    }

    let target_root = plugin_dir(&steam);
    let previous_version = installed_plugin_version(&target_root);
    let mut logs = Vec::new();
    let mut changed = 0_u32;

    for (relative, contents) in EMBEDDED_PLUGIN_FILES {
        let target = relative
            .split('/')
            .fold(target_root.clone(), |path, part| path.join(part));
        if let Some(parent) = target.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return OperationResult::error(
                    format!("Не удалось создать папку плагина: {error}"),
                    logs,
                );
            }
        }

        if fs::read(&target).ok().as_deref() == Some(*contents) {
            logs.push(format!("Без изменений: {relative}"));
            continue;
        }
        if let Err(error) = fs::write(&target, contents) {
            return OperationResult::error(
                format!("Не удалось записать {}: {error}", target.display()),
                logs,
            );
        }
        changed += 1;
        logs.push(format!("Установлен: {relative}"));
    }

    let action = match previous_version.as_deref() {
        None => "Плагин Crocodile Manifest установлен",
        Some(BUNDLED_PLUGIN_VERSION) if changed == 0 => "Плагин Crocodile Manifest уже установлен",
        Some(_) => "Плагин Crocodile Manifest обновлён",
    };
    logs.push(format!("Папка: {}", target_root.display()));
    OperationResult::ok(
        format!("{action}. Перезапустите Steam, чтобы применить изменения"),
        logs,
    )
}

#[tauri::command]
pub fn millennium_remove_plugin(steam_path: Option<String>) -> OperationResult {
    let Some(steam) = resolve_steam_path(steam_path) else {
        return OperationResult::error("Папка Steam не найдена", vec![]);
    };
    let target = plugin_dir(&steam);
    if !target.is_dir() {
        return OperationResult::error("Плагин Crocodile Manifest не установлен", vec![]);
    }
    if installed_plugin_name(&target).as_deref() != Some(PLUGIN_FOLDER_NAME) {
        return OperationResult::error(
            "Удаление отменено: папка не принадлежит Crocodile Manifest",
            vec![],
        );
    }

    match fs::remove_dir_all(&target) {
        Ok(_) => OperationResult::ok(
            "Плагин Crocodile Manifest удалён. Перезапустите Steam, чтобы применить изменения",
            vec![format!("Удалена папка: {}", target.display())],
        ),
        Err(error) => OperationResult::error(
            format!("Не удалось удалить плагин: {error}"),
            vec![format!("Папка: {}", target.display())],
        ),
    }
}

/// Only after an explicit DLL selection: old installers do not understand pinning.
pub(crate) fn update_installed_for_loader(steam: &Path) -> Result<(), String> {
    let plugin = plugin_dir(steam);
    if !plugin.join("plugin.json").is_file() { return Ok(()); }
    if installed_plugin_name(&plugin).as_deref() != Some(PLUGIN_FOLDER_NAME) {
        return Err("В папке Crocodile Manifest найден другой плагин. Обновите его вручную до версии с поддержкой закрепления DLL.".into());
    }
    let installed_version = installed_plugin_version(&plugin);
    if let Some(version) = &installed_version {
        let parts: Vec<u32> = version.split('.').map(str::parse).collect::<Result<_, _>>()
            .map_err(|_| "Неизвестная версия установленного плагина. Проверьте поддержку закрепления DLL вручную.".to_string())?;
        if parts.len() != 3 { return Err("Неизвестная версия установленного плагина. Проверьте его вручную.".into()); }
        if parts.as_slice() > [0, 5, 1].as_slice() { return Ok(()); } // Never downgrade a later plugin.
    }
    if installed_version.as_deref() == Some(BUNDLED_PLUGIN_VERSION)
        && fs::read(plugin.join("backend/install-loader.ps1")).ok().as_deref() == Some(EMBEDDED_PLUGIN_FILES[0].1) { return Ok(()); }
    let result = millennium_install_plugin(Some(steam.display().to_string()));
    if result.success { Ok(()) } else { Err(result.message) }
}

fn install_bundled_runtime(steam: &Path) -> OperationResult {
    let mut logs = Vec::new();
    let mut changed = 0_u32;
    for (relative, contents) in EMBEDDED_MILLENNIUM_FILES {
        if contents.len() < 1024 || !contents.starts_with(b"MZ") {
            return OperationResult::error(
                format!("Встроенный файл Millennium повреждён: {relative}"),
                logs,
            );
        }
        let target = relative
            .split('/')
            .fold(steam.to_path_buf(), |path, part| path.join(part));
        if let Some(parent) = target.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return OperationResult::error(
                    format!("Не удалось создать {}: {error}", parent.display()),
                    logs,
                );
            }
        }
        if fs::read(&target).ok().as_deref() == Some(*contents) {
            logs.push(format!("Без изменений: {relative}"));
            continue;
        }
        if let Err(error) = fs::write(&target, contents) {
            return OperationResult::error(
                format!("Не удалось записать {}: {error}", target.display()),
                logs,
            );
        }
        changed += 1;
        logs.push(format!("Установлен: {relative}"));
    }
    logs.push(format!("Папка Steam: {}", steam.display()));
    OperationResult::ok(
        if changed == 0 {
            "Millennium уже установлен. Перезапустите Steam, если плагин ещё не появился"
        } else {
            "Millennium установлен напрямую в папку Steam. Полностью перезапустите Steam"
        },
        logs,
    )
}

#[tauri::command]
pub fn millennium_install_bundled(steam_path: Option<String>) -> OperationResult {
    let Some(steam) = resolve_steam_path(steam_path) else {
        return OperationResult::error("Папка Steam не найдена", vec![]);
    };
    if !steam.join("steam.exe").is_file() {
        return OperationResult::error("steam.exe не найден в выбранной папке", vec![]);
    }
    install_bundled_runtime(&steam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_steam_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "cgs-millennium-test-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn installs_plugin_into_millennium_plugins_folder() {
        let steam = test_steam_dir();
        fs::create_dir_all(&steam).expect("create fake Steam");
        fs::write(steam.join("steam.exe"), []).expect("create fake steam.exe");
        let runtime = install_bundled_runtime(&steam);
        assert!(runtime.success, "{}", runtime.message);

        let before = status_for(Some(steam.display().to_string()));
        assert_eq!(before.status, "plugin_missing");

        let result = millennium_install_plugin(Some(steam.display().to_string()));
        assert!(result.success, "{}", result.message);

        let expected = steam
            .join("millennium")
            .join("plugins")
            .join(PLUGIN_FOLDER_NAME);
        assert!(expected.join("plugin.json").is_file());
        assert!(expected.join("backend").join("main.lua").is_file());
        assert_eq!(fs::read(expected.join("backend/loader/OpenSteamTool.dll")).unwrap(),
            include_bytes!("assets/OpenSteamTool.dll"));
        assert_eq!(fs::read(steam.join("OpenSteamTool.dll")).unwrap(),
            include_bytes!("assets/OpenSteamTool.dll"));
        assert!(fs::read_to_string(steam.join("opensteamtool.toml")).unwrap().contains("config/lua"));
        assert!(expected.join("backend/install-loader.ps1").is_file());
        let dll_times: Vec<_> = ["OpenSteamTool.dll", "dwmapi.dll", "xinput1_4.dll"].iter()
            .map(|name| fs::metadata(steam.join(name)).unwrap().modified().unwrap()).collect();
        let repeated = millennium_install_plugin(Some(steam.display().to_string()));
        assert!(repeated.success, "{}", repeated.message);
        for (index, name) in ["OpenSteamTool.dll", "dwmapi.dll", "xinput1_4.dll"].iter().enumerate() {
            assert_eq!(fs::metadata(steam.join(name)).unwrap().modified().unwrap(), dll_times[index]);
        }
        assert!(expected
            .join(".millennium")
            .join("Dist")
            .join("index.js")
            .is_file());

        let after = status_for(Some(steam.display().to_string()));
        assert_eq!(after.status, "ready");
        assert!(after.plugin_current);
        assert_eq!(
            after.plugin_version.as_deref(),
            Some(BUNDLED_PLUGIN_VERSION)
        );

        let removed = millennium_remove_plugin(Some(steam.display().to_string()));
        assert!(removed.success, "{}", removed.message);
        assert!(!expected.exists());
        assert_eq!(
            status_for(Some(steam.display().to_string())).status,
            "plugin_missing"
        );

        fs::remove_dir_all(steam).expect("clean test directory");
    }

    #[test]
    fn installs_bundled_millennium_directly_into_steam() {
        let steam = test_steam_dir();
        fs::create_dir_all(&steam).expect("create fake Steam");
        fs::write(steam.join("steam.exe"), []).expect("create fake steam.exe");

        let result = millennium_install_bundled(Some(steam.display().to_string()));
        assert!(result.success, "{}", result.message);
        assert!(steam.join("wsock32.dll").is_file());
        assert!(steam.join("millennium/lib/millennium.dll").is_file());
        assert!(steam
            .join("millennium/bin/millennium.luavm64.exe")
            .is_file());
        assert!(millennium_is_installed(&steam));

        fs::remove_dir_all(steam).expect("clean test directory");
    }

    #[test]
    fn reports_missing_after_millennium_directory_is_removed() {
        let steam = test_steam_dir();
        fs::create_dir_all(steam.join("ext")).expect("create leftover ext directory");
        fs::write(steam.join("steam.exe"), []).expect("create fake steam.exe");

        let status = status_for(Some(steam.display().to_string()));
        assert_eq!(status.status, "millennium_missing");
        assert!(!status.millennium_installed);
        assert!(!status.plugin_installed);

        fs::remove_dir_all(steam).expect("clean test directory");
    }
}
