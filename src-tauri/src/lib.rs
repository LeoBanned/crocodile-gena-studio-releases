mod anticheat;
mod dlc;
mod dlc_catalog;
mod engine;
mod files;
mod hubcap;
mod hypervisor;
mod luatools;
mod millennium;
mod models;
mod ryuu;
mod saves;
mod smaki;
mod steam;
mod steam_library;
mod updates;
mod vbs;

use std::process::Command;
use tauri::{AppHandle, Manager, WebviewWindow};

const EMBEDDED_CORE_COMPONENTS: &[(&str, &[u8])] = &[
    (
        "SteamFix64.dll",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../backend/core_engine/SteamFix64.dll"
        )),
    ),
    (
        "winmm_unity.dll",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../backend/core_engine/winmm_unity.dll"
        )),
    ),
    (
        "EpicFix64.dll",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../backend/core_engine/EpicFix64.dll"
        )),
    ),
    (
        "EpicFix.ini",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../backend/core_engine/EpicFix.ini"
        )),
    ),
    (
        "EOSSDK-Win64-Shipping.dll",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../backend/core_engine/EOSSDK-Win64-Shipping.dll"
        )),
    ),
];

const EMBEDDED_DLC_COMPONENTS: &[(&str, &[u8])] = &[
    (
        "steam_api.dll",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../CreamAPI/steam_api.dll"
        )),
    ),
    (
        "steam_api64.dll",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../CreamAPI/steam_api64.dll"
        )),
    ),
];

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[tauri::command]
async fn pick_game_directory() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(pick_game_directory_blocking)
        .await
        .map_err(|error| format!("Диалог выбора прерван: {error}"))?
}

fn pick_game_directory_blocking() -> Result<Option<String>, String> {
    let script = r#"Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = 'Выберите папку с игрой'; $d.ShowNewFolderButton = $false; if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [Text.Encoding]::UTF8; Write-Output $d.SelectedPath }"#;
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-STA", "-Command", script]);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000);
    let output = command
        .output()
        .map_err(|error| format!("Не удалось открыть выбор папки: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then_some(path))
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Разрешены только HTTP(S)-ссылки".into());
    }
    Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Не удалось открыть ссылку: {error}"))
}

#[tauri::command]
fn minimize_window(window: WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
fn toggle_maximize_window(window: WebviewWindow) -> Result<(), String> {
    if window.is_maximized().map_err(|error| error.to_string())? {
        window.unmaximize().map_err(|error| error.to_string())
    } else {
        window.maximize().map_err(|error| error.to_string())
    }
}

#[tauri::command]
fn close_window(window: WebviewWindow) -> Result<(), String> {
    window.close().map_err(|error| error.to_string())
}

#[tauri::command]
fn start_dragging_window(window: WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            pick_game_directory,
            open_external,
            minimize_window,
            toggle_maximize_window,
            close_window,
            start_dragging_window,
            saves::scan_save_locations,
            saves::pick_save_archive,
            saves::create_save_archive,
            saves::pick_save_restore_archive,
            saves::restore_save_archive,
            engine::scan_game_directory,
            engine::install_fix,
            engine::uninstall_fix,
            engine::install_epicfix_only,
            engine::restore_eos_only,
            dlc::scan_dlc_target,
            dlc::fetch_dlc_catalog,
            steam_library::scan_installed_steam_games,
            steam_library::steam_library_cover,
            smaki::install_smaki,
            smaki::uninstall_smaki,
            dlc_catalog::scan_dlc_catalog,
            dlc::install_dlc_unlocker,
            dlc::uninstall_dlc_unlocker,
            luatools::luatools_status,
            luatools::luatools_restore_manifests,
            luatools::luatools_detect_steam,
            luatools::luatools_pick_files,
            luatools::luatools_install,
            luatools::luatools_toggle,
            luatools::luatools_remove,
            luatools::luatools_restart_steam,
            luatools::luatools_game_detail,
            luatools::luatools_names,
            millennium::millennium_plugin_status,
            millennium::millennium_install_plugin,
            millennium::millennium_remove_plugin,
            millennium::millennium_install_bundled,
            ryuu::ryuu_search,
            ryuu::ryuu_get,
            hubcap::hubcap_search,
            hubcap::hubcap_get,
            hubcap::hubcap_usage,
            hypervisor::get_app_status,
            hypervisor::check_asset_updates,
            hypervisor::list_games,
            hypervisor::select_game_exe,
            hypervisor::add_game,
            hypervisor::set_game_launcher,
            hypervisor::set_selected_game,
            hypervisor::remove_game,
            hypervisor::verify_game,
            hypervisor::launch_selected_game,
            hypervisor::is_selected_game_running,
            anticheat::anticheat_scan,
            anticheat::anticheat_pick_backup_directory,
            anticheat::anticheat_remove,
            anticheat::anticheat_remove_all,
            vbs::vbs_scan,
            vbs::vbs_disable,
            vbs::vbs_restore,
            vbs::vbs_set_test_mode,
            vbs::vbs_restart,
            files::files_catalog,
            files::files_download,
            files::files_cancel_download,
            files::files_reveal,
            steam::search_game,
            steam::get_app_details,
            steam::steam_drm_info,
            updates::check_for_update,
            updates::updates_page_url,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running CrocodileGena Studio");
}

fn extract_embedded(
    app: &AppHandle,
    folder: &str,
    components: &[(&str, &[u8])],
) -> Option<std::path::PathBuf> {
    let directory = app.path().app_local_data_dir().ok()?.join(folder);
    std::fs::create_dir_all(&directory).ok()?;

    for (name, contents) in components {
        let target = directory.join(name);
        let is_current = std::fs::metadata(&target)
            .map(|metadata| metadata.is_file() && metadata.len() == contents.len() as u64)
            .unwrap_or(false);
        if !is_current {
            std::fs::write(&target, contents).ok()?;
        }
    }

    Some(directory)
}

fn extract_embedded_core(app: &AppHandle) -> Option<std::path::PathBuf> {
    extract_embedded(app, "core_engine", EMBEDDED_CORE_COMPONENTS)
}

pub(crate) fn core_engine_path(app: &AppHandle) -> std::path::PathBuf {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.extend([
            resource_dir.join("backend").join("core_engine"),
            resource_dir
                .join("_up_")
                .join("backend")
                .join("core_engine"),
        ]);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join("backend").join("core_engine"));
        }
    }
    for candidate in candidates {
        if candidate.join("SteamFix64.dll").is_file() {
            return candidate;
        }
    }

    if let Some(directory) = extract_embedded_core(app) {
        return directory;
    }

    #[cfg(debug_assertions)]
    {
        return std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("backend")
            .join("core_engine");
    }

    #[cfg(not(debug_assertions))]
    app.path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("core_engine")
}

pub(crate) fn dlc_unlocker_path(app: &AppHandle) -> std::path::PathBuf {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.extend([
            resource_dir.join("CreamAPI"),
            resource_dir.join("DLCUnlocker"),
            resource_dir.join("_up_").join("CreamAPI"),
            resource_dir.join("_up_").join("DLCUnlocker"),
        ]);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join("CreamAPI"));
            candidates.push(directory.join("DLCUnlocker"));
        }
    }
    for candidate in candidates {
        if candidate.join("steam_api64.dll").is_file() {
            return candidate;
        }
    }

    if let Some(directory) = extract_embedded(app, "CreamAPI", EMBEDDED_DLC_COMPONENTS) {
        return directory;
    }

    #[cfg(debug_assertions)]
    {
        return std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("CreamAPI");
    }

    #[cfg(not(debug_assertions))]
    app.path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("CreamAPI")
}
