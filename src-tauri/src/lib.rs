#[cfg(target_os = "windows")]
mod anticheat;
#[cfg(not(target_os = "windows"))]
#[path = "linux/anticheat.rs"]
mod anticheat;
mod dlc;
mod dlc_catalog;
mod engine;
mod files;
mod archive_install;
mod hubcap;
#[cfg(target_os = "windows")]
mod hypervisor;
#[cfg(not(target_os = "windows"))]
#[path = "linux/hypervisor.rs"]
mod hypervisor;
mod luatools;
mod lua_loader;
mod loader_github;
#[cfg(target_os = "windows")]
mod millennium;
#[cfg(not(target_os = "windows"))]
#[path = "linux/millennium.rs"]
mod millennium;
mod models;
mod native_dialog;
mod native_drop;
mod ryuu;
mod saves;
mod smaki;
mod steam;
mod steam_library;
mod game_artwork;
mod updates;
#[cfg(target_os = "windows")]
mod vbs;
#[cfg(not(target_os = "windows"))]
#[path = "linux/vbs.rs"]
mod vbs;

use serde::Serialize;
use std::process::Command;
use tauri::{AppHandle, Manager, WebviewWindow};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

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

#[tauri::command]
async fn pick_game_directory(window: WebviewWindow) -> Result<Option<String>, String> {
    #[cfg(windows)]
    let owner = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    #[cfg(not(windows))]
    let owner = {
        let _ = window;
        0
    };
    tauri::async_runtime::spawn_blocking(move || {
        native_dialog::pick_folder("Выберите папку", owner)
    })
    .await
    .map_err(|error| format!("Диалог выбора прерван: {error}"))?
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Разрешены только HTTP(S)-ссылки".into());
    }
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", &url]);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(&url);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(&url);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Не удалось открыть ссылку: {error}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformInfo {
    os: &'static str,
    windows: bool,
    windows_only_modules: Vec<&'static str>,
}

#[tauri::command]
fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        windows: cfg!(target_os = "windows"),
        windows_only_modules: if cfg!(target_os = "windows") {
            Vec::new()
        } else {
            vec!["hypervisor"]
        },
    }
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

#[tauri::command]
fn process_is_elevated() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("fltmc.exe")
            .creation_flags(0x08000000)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn relaunch_as_admin() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Не удалось определить путь программы: {error}"))?;
    let executable: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(executable.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(if result.0 as isize == 5 {
            "Запрос прав администратора отменён".into()
        } else {
            format!(
                "Не удалось запустить программу с правами администратора (код {})",
                result.0 as isize
            )
        });
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn relaunch_as_admin() -> Result<(), String> {
    Err("Повышение прав через UAC доступно только в Windows".into())
}

pub fn run() {
    tauri::Builder::default()
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                archive_install::clear_sessions();
            }
        })
        .invoke_handler(tauri::generate_handler![
            pick_game_directory,
            native_drop::enable_file_drop,
            open_external,
            platform_info,
            minimize_window,
            toggle_maximize_window,
            close_window,
            start_dragging_window,
            process_is_elevated,
            relaunch_as_admin,
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
            game_artwork::game_artwork,
            game_artwork::game_artwork_store_url,
            smaki::install_smaki,
            smaki::uninstall_smaki,
            dlc_catalog::scan_dlc_catalog,
            dlc::install_dlc_unlocker,
            dlc::uninstall_dlc_unlocker,
            luatools::luatools_status,
            luatools::luatools_update_loader,
            lua_loader::luatools_loader_info,
            loader_github::luatools_check_loader_updates,
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
            files::files_copy_password,
            archive_install::files_archive_inspect,
            archive_install::files_archive_contents,
            archive_install::files_archive_prepare,
            archive_install::files_archive_install,
            archive_install::files_archive_cancel,
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
            resource_dir.join("_up_").join("CreamAPI"),
        ]);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join("CreamAPI"));
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
