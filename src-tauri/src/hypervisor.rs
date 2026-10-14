use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env, fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Emitter;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const VERSION: &str = "3.3.5";
const KVC_URL: &str = "https://github.com/wesmar/kvc/releases/download/latest/kvc.7z";
const SEVEN_ZIP_URL: &str = "https://www.7-zip.org/a/7zr.exe";
const UPDATE_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/LeoBanned/pirate-station-hv/main/manifest.json";
const UPDATE_TIMEOUT_SECONDS: u32 = 20;
const MIN_REMOTE_MANIFEST_VERSION: u32 = 2;
const DEFAULT_LAUNCHER_ID: &str = "hv_launcher";
const PLUG_N_PLAY_LAUNCHER_ID: &str = "plug_n_play";
include!(concat!(env!("OUT_DIR"), "/hypervisor_resources.rs"));

#[derive(Clone, Copy)]
struct LauncherProfile {
    id: &'static str,
    name: &'static str,
    badge: &'static str,
    batch_name: &'static str,
    helper_label: &'static str,
    description: &'static str,
}

const LAUNCHER_PROFILES: [LauncherProfile; 2] = [
    LauncherProfile {
        id: DEFAULT_LAUNCHER_ID,
        name: "HV-Launcher",
        badge: "KVC",
        batch_name: "HV-Launcher.bat",
        helper_label: "kvc.exe",
        description:
            "Для Windows 10/11\nТребует отключенную изоляцию ядра\nТребует запуск от администратора",
    },
    LauncherProfile {
        id: PLUG_N_PLAY_LAUNCHER_ID,
        name: "HV-PlugNPlay",
        badge: "DRV",
        batch_name: "HV-PlugNPlay.bat",
        helper_label: "drvloader.exe",
        description:
            "Для Windows 11\nТребует отключенную изоляцию ядра\nТребует запуск от администратора",
    },
];

#[derive(Clone, Serialize, Deserialize)]
struct GameEntry {
    id: String,
    name: String,
    exe_path: String,
    dir_path: String,
    loader_name: String,
    #[serde(default = "default_launcher_id")]
    launcher_id: String,
    #[serde(default)]
    launcher_name: String,
    #[serde(default)]
    batch_name: String,
    #[serde(default)]
    helper_label: String,
    batch_found: bool,
    kvc_found: bool,
    ready: bool,
    last_message: String,
}

#[derive(Serialize, Deserialize)]
struct Library {
    selected_id: Option<String>,
    #[serde(default = "default_launcher_id")]
    selected_launcher_id: String,
    games: Vec<GameEntry>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            selected_id: None,
            selected_launcher_id: default_launcher_id(),
            games: Vec::new(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct LibraryView {
    selected_id: Option<String>,
    selected_launcher_id: String,
    launchers: Vec<LauncherProfileView>,
    games: Vec<GameEntry>,
}

#[derive(Serialize)]
pub(crate) struct AppStatus {
    version: String,
    admin: bool,
    games_count: usize,
    game_running: bool,
    selected_game: Option<GameEntry>,
    selected_launcher: LauncherProfileView,
    root_batch_found: bool,
    root_kvc_found: bool,
}

#[derive(Clone, Serialize)]
struct LauncherProfileView {
    id: String,
    name: String,
    badge: String,
    batch_name: String,
    helper_label: String,
    description: String,
}

#[derive(Clone, Serialize)]
struct UiEvent {
    kind: String,
    message: String,
    step: Option<u8>,
    game_id: Option<String>,
}

#[derive(Deserialize)]
struct RemoteUpdateManifest {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    profiles: HashMap<String, RemoteProfileUpdate>,
}

#[derive(Clone, Deserialize)]
struct RemoteProfileUpdate {
    #[serde(default, alias = "batchUrl", alias = "bat_url")]
    batch_url: Option<String>,
    #[serde(default, alias = "batchSha256", alias = "bat_sha256")]
    batch_sha256: Option<String>,
}

struct PreparedAssetUpdates {
    cache_dir: PathBuf,
    manifest_download: PathBuf,
    manifest: RemoteUpdateManifest,
}

#[derive(Serialize)]
pub(crate) struct RunReport {
    success: bool,
    code: Option<i32>,
    message: String,
}

#[tauri::command]
pub(crate) fn get_app_status() -> Result<AppStatus, String> {
    let library = refresh_library(load_library()?);
    let selected_game = selected_game(&library).cloned();
    let game_running = selected_game
        .as_ref()
        .is_some_and(|game| process_name_running(&game.loader_name).unwrap_or(false));
    let selected_launcher = launcher_profile_or_default(&library.selected_launcher_id);
    let root = find_launcher_root().ok();

    Ok(AppStatus {
        version: VERSION.to_string(),
        admin: is_admin(),
        games_count: library.games.len(),
        game_running,
        selected_game,
        selected_launcher: launcher_view(selected_launcher),
        root_batch_found: root
            .as_ref()
            .is_some_and(|dir| dir.join(selected_launcher.batch_name).exists()),
        root_kvc_found: root
            .as_ref()
            .is_some_and(|dir| helper_path(selected_launcher, dir).exists()),
    })
}

#[tauri::command]
pub(crate) fn is_selected_game_running() -> Result<bool, String> {
    let library = refresh_library(load_library()?);
    let Some(game) = selected_game(&library) else {
        return Ok(false);
    };
    process_name_running(&game.loader_name)
}

#[tauri::command]
pub(crate) fn list_games() -> Result<LibraryView, String> {
    let library = refresh_library(load_library()?);
    save_library(&library)?;
    Ok(library_view(library))
}

#[tauri::command]
pub(crate) async fn check_asset_updates(app: tauri::AppHandle) -> Result<(), String> {
    if let Err(error) = check_asset_updates_inner(app.clone()).await {
        emit(
            &app,
            "warning",
            format!("Автообновление пропущено: {error}"),
            None,
            None,
        );
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn select_game_exe() -> Result<Option<String>, String> {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title = 'Выберите EXE игры'
$dialog.Filter = 'Executable (*.exe)|*.exe'
$dialog.Multiselect = $false
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
  Write-Output $dialog.FileName
}
"#;

    let output = powershell_output(script)?;
    let path = output.trim();

    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(path.to_string()))
    }
}

#[tauri::command]
pub(crate) fn add_game(
    app: tauri::AppHandle,
    exe_path: String,
    launcher_id: Option<String>,
) -> Result<LibraryView, String> {
    let exe = validate_exe_path(&exe_path)?;
    let dir = exe
        .parent()
        .ok_or_else(|| "Не удалось определить папку игры.".to_string())?
        .to_path_buf();
    let loader_name = exe
        .file_name()
        .ok_or_else(|| "Не удалось определить имя exe.".to_string())?
        .to_string_lossy()
        .to_string();
    let game_id = make_game_id(&exe);
    let mut library = load_library()?;
    let profile = match launcher_id {
        Some(id) => launcher_profile(&id)?,
        None => launcher_profile_or_default(&library.selected_launcher_id),
    };

    emit(
        &app,
        "info",
        format!("Добавляю игру: {}", exe.display()),
        None,
        Some(game_id.clone()),
    );

    prepare_game_files(&app, &game_id, &dir, &loader_name, profile, false)?;

    library
        .games
        .retain(|game| game.exe_path != exe.display().to_string());
    let name = exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "Game".to_string());

    library.games.push(GameEntry {
        id: game_id.clone(),
        name,
        exe_path: exe.display().to_string(),
        dir_path: dir.display().to_string(),
        loader_name,
        launcher_id: profile.id.to_string(),
        launcher_name: profile.name.to_string(),
        batch_name: profile.batch_name.to_string(),
        helper_label: profile.helper_label.to_string(),
        batch_found: true,
        kvc_found: helper_path(profile, &dir).exists(),
        ready: true,
        last_message: "Игра добавлена и подготовлена.".to_string(),
    });
    library.selected_id = Some(game_id);
    library.selected_launcher_id = profile.id.to_string();

    let library = refresh_library(library);
    save_library(&library)?;
    Ok(library_view(library))
}

#[tauri::command]
pub(crate) fn set_game_launcher(
    app: tauri::AppHandle,
    id: String,
    launcher_id: String,
) -> Result<LibraryView, String> {
    let profile = launcher_profile(&launcher_id)?;
    let mut library = load_library()?;
    let game = library
        .games
        .iter_mut()
        .find(|game| game.id == id)
        .ok_or_else(|| "Игра не найдена в библиотеке.".to_string())?;
    let game_id = game.id.clone();
    let dir = PathBuf::from(&game.dir_path);
    let loader_name = game.loader_name.clone();

    // Profile switching is a local, immediate file swap. Both helpers and both
    // BAT templates are embedded in the application, so this path never waits
    // for the network and does not require a separate verification step.
    prepare_game_files(&app, &game_id, &dir, &loader_name, profile, false)?;

    game.launcher_id = profile.id.to_string();
    game.launcher_name = profile.name.to_string();
    game.batch_name = profile.batch_name.to_string();
    game.helper_label = profile.helper_label.to_string();
    game.last_message = format!("Профиль {} установлен и готов к запуску.", profile.badge);
    library.selected_launcher_id = profile.id.to_string();

    let library = refresh_library(library);
    save_library(&library)?;
    emit(
        &app,
        "info",
        format!("Профиль {} установлен, файлы заменены.", profile.badge),
        None,
        Some(game_id),
    );
    Ok(library_view(library))
}

#[tauri::command]
pub(crate) fn set_selected_game(id: String) -> Result<LibraryView, String> {
    let mut library = load_library()?;
    if library.games.iter().any(|game| game.id == id) {
        library.selected_id = Some(id);
    }
    let library = refresh_library(library);
    save_library(&library)?;
    Ok(library_view(library))
}

#[tauri::command]
pub(crate) fn remove_game(id: String) -> Result<LibraryView, String> {
    let mut library = load_library()?;
    library.games.retain(|game| game.id != id);
    if library.selected_id.as_deref() == Some(id.as_str()) {
        library.selected_id = library.games.first().map(|game| game.id.clone());
    }
    let library = refresh_library(library);
    save_library(&library)?;
    Ok(library_view(library))
}

#[tauri::command]
pub(crate) fn verify_game(app: tauri::AppHandle, id: String) -> Result<LibraryView, String> {
    let mut library = load_library()?;
    let game = library
        .games
        .iter()
        .find(|game| game.id == id)
        .cloned()
        .ok_or_else(|| "Игра не найдена в библиотеке.".to_string())?;
    let dir = PathBuf::from(&game.dir_path);
    let profile = launcher_profile_or_default(&game.launcher_id);

    prepare_game_files(&app, &game.id, &dir, &game.loader_name, profile, false)?;
    library = refresh_library(library);
    save_library(&library)?;
    Ok(library_view(library))
}

#[tauri::command]
pub(crate) async fn launch_selected_game(app: tauri::AppHandle) -> Result<RunReport, String> {
    tauri::async_runtime::spawn_blocking(move || launch_selected_game_inner(app))
        .await
        .map_err(|error| format!("Задача запуска завершилась ошибкой: {error}"))?
}

fn launch_selected_game_inner(app: tauri::AppHandle) -> Result<RunReport, String> {
    let library = refresh_library(load_library()?);
    let game = selected_game(&library)
        .cloned()
        .ok_or_else(|| "Сначала добавьте и выберите игру во вкладке Игры.".to_string())?;
    let dir = PathBuf::from(&game.dir_path);
    let profile = launcher_profile_or_default(&game.launcher_id);

    prepare_game_files(&app, &game.id, &dir, &game.loader_name, profile, true)?;

    let batch = dir.join(profile.batch_name);
    if !batch.exists() {
        return Err(format!(
            "{} не найден рядом с выбранной игрой.",
            profile.batch_name
        ));
    }

    emit(
        &app,
        "info",
        format!("Запускаю {} через {}.", game.name, profile.name),
        None,
        Some(game.id.clone()),
    );
    emit_launch_diagnostics(&app, &game, profile);

    run_batch(&app, &game, &batch, &dir, profile)
}

fn run_batch(
    app: &tauri::AppHandle,
    game: &GameEntry,
    batch: &Path,
    dir: &Path,
    profile: LauncherProfile,
) -> Result<RunReport, String> {
    clear_diagnostic_logs(profile, dir);

    let mut command = Command::new("cmd.exe");
    command
        .arg("/D")
        .arg("/C")
        .arg(batch)
        .arg("__MINIMIZED__")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Не удалось запустить батник: {error}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let game_id = game.id.clone();
    let output_lines = Arc::new(Mutex::new(Vec::<String>::new()));

    let out_app = app.clone();
    let out_game_id = game_id.clone();
    let out_lines = Arc::clone(&output_lines);
    let stdout_thread = stdout.map(|stream| {
        thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines().map_while(Result::ok) {
                remember_batch_line(&out_lines, &line);
                forward_batch_line(&out_app, &line, false, Some(out_game_id.clone()));
            }
        })
    });

    let err_app = app.clone();
    let err_game_id = game_id.clone();
    let err_lines = Arc::clone(&output_lines);
    let stderr_thread = stderr.map(|stream| {
        thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines().map_while(Result::ok) {
                remember_batch_line(&err_lines, &line);
                forward_batch_line(&err_app, &line, true, Some(err_game_id.clone()));
            }
        })
    });

    let status = child
        .wait()
        .map_err(|error| format!("Процесс батника завершился ошибкой: {error}"))?;

    if let Some(handle) = stdout_thread {
        let _ = handle.join();
    }
    if let Some(handle) = stderr_thread {
        let _ = handle.join();
    }

    let success = status.success();
    let code = status.code();
    let message = if success {
        format!("{}: запуск завершен.", game.name)
    } else {
        let code_text = code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "неизвестный".to_string());
        let output = output_lines
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default();
        let diagnosis = diagnose_launch_failure(profile, dir, &output);
        format!(
            "{}: запуск не удался (код {}). {}",
            game.name, code_text, diagnosis
        )
    };

    emit(
        app,
        if success { "success" } else { "error" },
        message.clone(),
        None,
        Some(game.id.clone()),
    );

    Ok(RunReport {
        success,
        code,
        message,
    })
}

fn prepare_game_files(
    app: &tauri::AppHandle,
    game_id: &str,
    dir: &Path,
    loader_name: &str,
    profile: LauncherProfile,
    allow_download: bool,
) -> Result<(), String> {
    if !dir.exists() {
        return Err(format!("Папка игры не найдена: {}", dir.display()));
    }

    ensure_batch(app, game_id, dir, loader_name, profile)?;
    ensure_helper(app, game_id, dir, profile, allow_download)?;
    cleanup_other_profile(app, game_id, dir, profile)?;

    Ok(())
}

fn ensure_batch(
    app: &tauri::AppHandle,
    game_id: &str,
    dir: &Path,
    loader_name: &str,
    profile: LauncherProfile,
) -> Result<(), String> {
    let target = dir.join(profile.batch_name);
    let mut content = if target.exists() {
        emit(
            app,
            "success",
            format!("{} найден, обновляю выбранный exe.", profile.batch_name),
            Some(2),
            Some(game_id.to_string()),
        );
        read_batch_source(profile)
    } else {
        emit(
            app,
            "info",
            format!(
                "{} отсутствует, устанавливаю копию профиля.",
                profile.batch_name
            ),
            Some(2),
            Some(game_id.to_string()),
        );
        read_batch_source(profile)
    };

    if content.trim().is_empty() && target.is_file() {
        content = read_text_lossy(&target)?;
    }
    require_batch_source(&content, profile.batch_name)?;
    content = patch_custom_exe(&content, loader_name);
    fs::write(&target, content)
        .map_err(|error| format!("Не удалось записать {}: {error}", target.display()))?;

    Ok(())
}

fn ensure_helper(
    app: &tauri::AppHandle,
    game_id: &str,
    dir: &Path,
    profile: LauncherProfile,
    allow_download: bool,
) -> Result<(), String> {
    match profile.id {
        DEFAULT_LAUNCHER_ID => ensure_kvc(app, game_id, dir, allow_download),
        PLUG_N_PLAY_LAUNCHER_ID => ensure_drvloader(app, game_id, dir),
        _ => Err(format!("Неизвестный профиль запуска: {}", profile.id)),
    }
}

fn cleanup_other_profile(
    app: &tauri::AppHandle,
    game_id: &str,
    dir: &Path,
    profile: LauncherProfile,
) -> Result<(), String> {
    if profile.id == PLUG_N_PLAY_LAUNCHER_ID {
        remove_kvc_for_drv(app, game_id, dir)?;
    } else if profile.id == DEFAULT_LAUNCHER_ID {
        remove_drv_for_kvc(app, game_id, dir)?;
    }

    Ok(())
}

fn remove_kvc_for_drv(app: &tauri::AppHandle, game_id: &str, dir: &Path) -> Result<(), String> {
    remove_obsolete_file(
        app,
        game_id,
        &dir.join("HV-Launcher.bat"),
        "DRV выбран: старый HV-Launcher.bat удален.",
        "DRV выбран, но старый HV-Launcher.bat не удалился",
    );

    let kvc = dir.join("kvc.exe");
    if kvc.exists() {
        match remove_file_best_effort(&kvc) {
            Ok(()) => emit(
                app,
                "success",
                "DRV выбран: kvc.exe удален из папки игры.".to_string(),
                Some(3),
                Some(game_id.to_string()),
            ),
            Err(error) => emit(
                app,
                "warning",
                format!("DRV выбран, но kvc.exe не удалился: {error}"),
                Some(3),
                Some(game_id.to_string()),
            ),
        }
    }

    let install_dir = dir.join("KVC Installing");
    if install_dir.exists() {
        if let Err(error) = fs::remove_dir_all(&install_dir) {
            emit(
                app,
                "warning",
                format!("Не удалось удалить {}: {error}", install_dir.display()),
                Some(3),
                Some(game_id.to_string()),
            );
        }
    }

    Ok(())
}

fn remove_drv_for_kvc(app: &tauri::AppHandle, game_id: &str, dir: &Path) -> Result<(), String> {
    remove_obsolete_file(
        app,
        game_id,
        &dir.join("HV-PlugNPlay.bat"),
        "KVC выбран: старый HV-PlugNPlay удален.",
        "KVC выбран, но старый HV-PlugNPlay не удалился",
    );

    let drv_dir = dir.join("DenuvOwO");
    if drv_dir.exists() {
        match fs::remove_dir_all(&drv_dir) {
            Ok(()) => emit(
                app,
                "success",
                "KVC выбран: папка DenuvOwO с DRV удалена из папки игры.".to_string(),
                Some(3),
                Some(game_id.to_string()),
            ),
            Err(error) => emit(
                app,
                "warning",
                format!("KVC выбран, но DenuvOwO не удалился: {error}"),
                Some(3),
                Some(game_id.to_string()),
            ),
        }
    }

    Ok(())
}

fn remove_obsolete_file(
    app: &tauri::AppHandle,
    game_id: &str,
    path: &Path,
    success_message: &str,
    error_prefix: &str,
) {
    if !path.exists() {
        return;
    }

    match remove_file_best_effort(path) {
        Ok(()) => emit(
            app,
            "success",
            success_message.to_string(),
            Some(3),
            Some(game_id.to_string()),
        ),
        Err(error) => emit(
            app,
            "warning",
            format!("{error_prefix}: {error}"),
            Some(3),
            Some(game_id.to_string()),
        ),
    }
}

fn remove_file_best_effort(path: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::metadata(path) {
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            let _ = fs::set_permissions(path, permissions);
        }
    }

    fs::remove_file(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn ensure_kvc(
    app: &tauri::AppHandle,
    game_id: &str,
    dir: &Path,
    allow_download: bool,
) -> Result<(), String> {
    let target = dir.join("kvc.exe");
    if sync_cached_helper(
        app,
        game_id,
        launcher_profile_or_default(DEFAULT_LAUNCHER_ID),
        &target,
    )? {
        return Ok(());
    }

    if let Ok(root) = find_launcher_root() {
        let local_kvc = root.join("kvc.exe");
        if local_kvc.exists() {
            if copy_file_if_changed(&local_kvc, &target)? {
                emit(
                    app,
                    "success",
                    "kvc.exe восстановлен из текущей папки лаунчера.".to_string(),
                    Some(3),
                    Some(game_id.to_string()),
                );
            } else {
                emit(
                    app,
                    "success",
                    "kvc.exe проверен: совпадает с локальной копией лаунчера.".to_string(),
                    Some(3),
                    Some(game_id.to_string()),
                );
            }
            return Ok(());
        }
    }

    match write_embedded_binary_if_changed(&target, EMBEDDED_KVC) {
        Ok(true) => {
            emit(
                app,
                "success",
                "kvc.exe установлен из встроенных ресурсов.".to_string(),
                Some(3),
                Some(game_id.to_string()),
            );
            return Ok(());
        }
        Ok(false) => {
            emit(
                app,
                "success",
                "kvc.exe проверен: совпадает со встроенной копией.".to_string(),
                Some(3),
                Some(game_id.to_string()),
            );
            return Ok(());
        }
        Err(error) if allow_download => {
            emit(
                app,
                "warning",
                format!("Не удалось записать встроенный kvc.exe: {error}"),
                Some(3),
                Some(game_id.to_string()),
            );
        }
        Err(error) => return Err(error),
    }

    if target.exists() {
        emit(
            app,
            "warning",
            "kvc.exe найден рядом с игрой, но встроенная копия не записалась для сверки."
                .to_string(),
            Some(3),
            Some(game_id.to_string()),
        );
        return Ok(());
    }

    emit(
        app,
        "info",
        "kvc.exe отсутствует, скачиваю релиз с GitHub.".to_string(),
        Some(3),
        Some(game_id.to_string()),
    );
    download_and_extract_kvc(dir)?;

    if target.exists() {
        emit(
            app,
            "success",
            "kvc.exe установлен.".to_string(),
            Some(3),
            Some(game_id.to_string()),
        );
        Ok(())
    } else {
        Err("kvc.exe не появился после установки.".to_string())
    }
}

fn ensure_drvloader(app: &tauri::AppHandle, game_id: &str, dir: &Path) -> Result<(), String> {
    let target = helper_path(launcher_profile_or_default(PLUG_N_PLAY_LAUNCHER_ID), dir);
    if sync_cached_helper(
        app,
        game_id,
        launcher_profile_or_default(PLUG_N_PLAY_LAUNCHER_ID),
        &target,
    )? {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Не удалось создать {}: {error}", parent.display()))?;
    }

    if let Ok(root) = find_launcher_root() {
        let local_candidates = [
            root.join("drvloader.exe"),
            root.join("DenuvOwO").join("drvloader.exe"),
        ];

        for local_drvloader in local_candidates {
            if local_drvloader.exists() {
                if copy_file_if_changed(&local_drvloader, &target)? {
                    emit(
                        app,
                        "success",
                        "drvloader.exe восстановлен из текущей папки лаунчера.".to_string(),
                        Some(3),
                        Some(game_id.to_string()),
                    );
                } else {
                    emit(
                        app,
                        "success",
                        "drvloader.exe проверен: совпадает с локальной копией лаунчера."
                            .to_string(),
                        Some(3),
                        Some(game_id.to_string()),
                    );
                }
                return Ok(());
            }
        }
    }

    if write_embedded_binary_if_changed(&target, EMBEDDED_DRVLOADER)? {
        emit(
            app,
            "success",
            "drvloader.exe установлен из встроенных ресурсов.".to_string(),
            Some(3),
            Some(game_id.to_string()),
        );
    } else {
        emit(
            app,
            "success",
            "drvloader.exe проверен: совпадает со встроенной копией.".to_string(),
            Some(3),
            Some(game_id.to_string()),
        );
    }
    Ok(())
}

fn download_and_extract_kvc(dir: &Path) -> Result<(), String> {
    let install_dir = dir.join("KVC Installing");
    let archive = install_dir.join("kvc.7z");
    let seven_zip = install_dir.join("7zr.exe");

    fs::create_dir_all(&install_dir)
        .map_err(|error| format!("Не удалось создать {}: {error}", install_dir.display()))?;

    download_file(KVC_URL, &archive)?;
    download_file(SEVEN_ZIP_URL, &seven_zip)?;

    let mut command = Command::new(&seven_zip);
    command
        .arg("x")
        .arg(&archive)
        .arg(format!("-o{}", install_dir.display()))
        .arg("-pgithub.com")
        .arg("-y");

    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let status = command
        .status()
        .map_err(|error| format!("Не удалось запустить 7zr.exe: {error}"))?;

    if !status.success() {
        let _ = fs::remove_dir_all(&install_dir);
        return Err("Не удалось распаковать архив kvc.7z.".to_string());
    }

    let extracted = install_dir.join("kvc.exe");
    let target = dir.join("kvc.exe");
    if !extracted.exists() {
        let _ = fs::remove_dir_all(&install_dir);
        return Err("В архиве не найден kvc.exe.".to_string());
    }

    fs::copy(&extracted, &target)
        .map_err(|error| format!("Не удалось перенести kvc.exe: {error}"))?;
    let _ = fs::remove_dir_all(&install_dir);

    Ok(())
}

fn download_file(url: &str, target: &Path) -> Result<(), String> {
    download_file_with_timeout(url, target, 60)
}

fn download_file_with_timeout(
    url: &str,
    target: &Path,
    timeout_seconds: u32,
) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Не удалось создать {}: {error}", parent.display()))?;
    }

    let _ = fs::remove_file(target);
    let curl_error = match download_file_with_curl(url, target, timeout_seconds) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };

    let powershell_error = match download_file_with_powershell_http(url, target, timeout_seconds) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };

    Err(format!(
        "curl: {curl_error}; PowerShell: {powershell_error}"
    ))
}

fn download_file_with_curl(url: &str, target: &Path, timeout_seconds: u32) -> Result<(), String> {
    let mut command = Command::new("curl.exe");
    command
        .arg("--fail")
        .arg("--location")
        .arg("--silent")
        .arg("--show-error")
        .arg("--connect-timeout")
        .arg(timeout_seconds.to_string())
        .arg("--max-time")
        .arg((timeout_seconds + 10).to_string())
        .arg("--output")
        .arg(target)
        .arg(url);

    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|error| format!("curl.exe недоступен: {error}"))?;

    if output.status.success() && target.exists() {
        return Ok(());
    }

    let _ = fs::remove_file(target);
    Err(compact_command_output(&output))
}

fn download_file_with_powershell_http(
    url: &str,
    target: &Path,
    timeout_seconds: u32,
) -> Result<(), String> {
    let timeout_ms = timeout_seconds.saturating_mul(1000);
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
         $uri = '{}'; \
         $out = '{}'; \
         $request = [System.Net.WebRequest]::Create($uri); \
         $request.Method = 'GET'; \
         $request.Timeout = {}; \
         $request.ReadWriteTimeout = {}; \
         $request.UserAgent = 'PirateStation/{}'; \
         $response = $request.GetResponse(); \
         try {{ \
           $stream = $response.GetResponseStream(); \
           $file = [System.IO.File]::Open($out, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None); \
           try {{ $stream.CopyTo($file) }} finally {{ $file.Dispose(); $stream.Dispose() }} \
         }} finally {{ $response.Close() }}",
        powershell_escape(url),
        powershell_escape(&target.display().to_string()),
        timeout_ms,
        timeout_ms,
        VERSION
    );

    if let Err(error) = run_powershell(&script) {
        let _ = fs::remove_file(target);
        return Err(error);
    }

    if target.exists() {
        Ok(())
    } else {
        Err(format!("Файл не скачался: {}", target.display()))
    }
}

async fn check_asset_updates_inner(app: tauri::AppHandle) -> Result<(), String> {
    let prepare_app = app.clone();
    let Some(prepared) =
        tauri::async_runtime::spawn_blocking(move || prepare_asset_updates(&prepare_app))
            .await
            .map_err(|error| format!("Подготовка автообновления завершилась ошибкой: {error}"))??
    else {
        return Ok(());
    };

    let changed = update_profiles_with_await(app.clone(), prepared.manifest).await;

    let finish_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        finish_asset_updates(
            &finish_app,
            prepared.cache_dir,
            prepared.manifest_download,
            changed,
        )
    })
    .await
    .map_err(|error| format!("Завершение автообновления завершилось ошибкой: {error}"))?
}

fn prepare_asset_updates(app: &tauri::AppHandle) -> Result<Option<PreparedAssetUpdates>, String> {
    if UPDATE_MANIFEST_URL.trim().is_empty() {
        return Ok(None);
    }

    emit(
        app,
        "info",
        "Проверяю обновления профилей с GitHub через async/await и несколько потоков.".to_string(),
        None,
        None,
    );

    let cache_dir = update_cache_dir()?;
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Не удалось создать {}: {error}", cache_dir.display()))?;

    let manifest_download = cache_dir.join("manifest.json.download");
    if let Err(error) = download_file_with_timeout(
        UPDATE_MANIFEST_URL,
        &manifest_download,
        UPDATE_TIMEOUT_SECONDS,
    ) {
        emit(
            app,
            "warning",
            format!("manifest.json недоступен, использую локальные файлы: {error}"),
            None,
            None,
        );
        let _ = fs::remove_file(&manifest_download);
        return Ok(None);
    }

    let content = read_text_lossy(&manifest_download)?;
    let manifest = match serde_json::from_str::<RemoteUpdateManifest>(&content) {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = fs::remove_file(&manifest_download);
            emit(
                app,
                "warning",
                format!("manifest.json не прочитан: {error}"),
                None,
                None,
            );
            return Ok(None);
        }
    };

    if manifest.version < MIN_REMOTE_MANIFEST_VERSION {
        let _ = fs::remove_file(&manifest_download);
        clear_update_cache_assets();
        emit(
            app,
            "warning",
            format!(
                "manifest.json устарел (v{}). Использую встроенные файлы программы.",
                manifest.version
            ),
            None,
            None,
        );
        return Ok(None);
    }

    if manifest.profiles.is_empty() {
        emit(
            app,
            "warning",
            "manifest.json не содержит профили обновления.".to_string(),
            None,
            None,
        );
        let manifest_cache = cache_dir.join("manifest.json");
        let _ = replace_file(&manifest_download, &manifest_cache);
        return Ok(None);
    }

    clear_cached_helper_assets();

    Ok(Some(PreparedAssetUpdates {
        cache_dir,
        manifest_download,
        manifest,
    }))
}

async fn update_profiles_with_await(
    app: tauri::AppHandle,
    manifest: RemoteUpdateManifest,
) -> usize {
    let update_tasks = LAUNCHER_PROFILES
        .iter()
        .filter_map(|profile| {
            manifest
                .profiles
                .get(profile.id)
                .cloned()
                .map(|remote| (*profile, remote))
        })
        .map(|(profile, remote)| {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                update_profile_assets(&app, profile, &remote)
            })
        })
        .collect::<Vec<_>>();

    let mut changed = 0usize;
    for update_task in update_tasks {
        match update_task.await {
            Ok(count) => changed += count,
            Err(_) => emit(
                &app,
                "warning",
                "Один async-поток автообновления завершился ошибкой.".to_string(),
                None,
                None,
            ),
        }
    }

    changed
}

fn finish_asset_updates(
    app: &tauri::AppHandle,
    cache_dir: PathBuf,
    manifest_download: PathBuf,
    changed: usize,
) -> Result<(), String> {
    let manifest_cache = cache_dir.join("manifest.json");
    let _ = replace_file(&manifest_download, &manifest_cache);

    if changed > 0 {
        if let Err(error) = sync_cached_assets_to_games(app) {
            emit(
                app,
                "warning",
                format!("Кэш обновлен, но добавленные игры не все синхронизированы: {error}"),
                None,
                None,
            );
        }
        emit(
            app,
            "success",
            format!("Обновления применены: {changed} файл(ов)."),
            None,
            None,
        );
    } else {
        emit(
            app,
            "success",
            "Профили запуска уже актуальны.".to_string(),
            None,
            None,
        );
    }

    Ok(())
}

fn update_profile_assets(
    app: &tauri::AppHandle,
    profile: LauncherProfile,
    remote: &RemoteProfileUpdate,
) -> usize {
    let mut changed = 0usize;

    if let Some(url) = remote_value(remote.batch_url.as_deref()) {
        match update_cached_asset(
            app,
            profile,
            "батник",
            url,
            remote.batch_sha256.as_deref(),
            &cached_batch_path(profile),
        ) {
            Ok(true) => changed += 1,
            Ok(false) => {}
            Err(error) => emit(
                app,
                "warning",
                format!(
                    "{}: батник не обновлен. {}",
                    profile.name,
                    friendly_download_error(&error)
                ),
                None,
                None,
            ),
        }
    }

    changed
}

fn update_cached_asset(
    app: &tauri::AppHandle,
    profile: LauncherProfile,
    label: &str,
    url: &str,
    expected_sha256: Option<&str>,
    target: &Result<PathBuf, String>,
) -> Result<bool, String> {
    let target = target.as_ref().map_err(|error| error.to_string())?;
    let expected_hash = normalize_sha256(expected_sha256);
    if let Some(hash) = expected_hash.as_deref() {
        if target.exists() {
            if let Ok(actual_hash) = file_sha256(target) {
                if actual_hash == hash {
                    return Ok(false);
                }
            }
        }
    }

    emit(
        app,
        "info",
        format!("Скачиваю {} для {}.", label, profile.name),
        None,
        None,
    );

    if expected_hash.is_none() {
        emit(
            app,
            "warning",
            format!(
                "Для {} ({}) не указан SHA256, сравнение будет пропущено.",
                profile.name, label
            ),
            None,
            None,
        );
    }

    let download = target.with_extension("download");
    let _ = fs::remove_file(&download);
    download_file_with_timeout(url, &download, UPDATE_TIMEOUT_SECONDS)?;

    if let Some(hash) = expected_hash.as_deref() {
        let actual_hash = file_sha256(&download)?;
        if actual_hash != hash {
            let _ = fs::remove_file(&download);
            return Err(format!(
                "SHA256 не совпал для {} (ожидал {}, получил {}).",
                label, hash, actual_hash
            ));
        }
    }

    replace_file(&download, target)?;
    emit(
        app,
        "success",
        format!("{} для {} обновлен.", label, profile.name),
        None,
        None,
    );
    Ok(true)
}

fn sync_cached_assets_to_games(app: &tauri::AppHandle) -> Result<(), String> {
    let mut library = refresh_library(load_library()?);
    let mut synced = 0usize;

    for game in &library.games {
        let profile = launcher_profile_or_default(&game.launcher_id);
        let dir = PathBuf::from(&game.dir_path);
        if !dir.exists() {
            continue;
        }

        if let Ok(batch) = cached_batch_path(profile) {
            if let Ok(content) = read_text_lossy(&batch) {
                let patched = patch_custom_exe(&content, &game.loader_name);
                let target = dir.join(profile.batch_name);
                match write_text_if_changed(&target, &patched) {
                    Ok(true) => synced += 1,
                    Ok(false) => {}
                    Err(error) => emit(app, "warning", error, None, Some(game.id.clone())),
                }
            }
        }
    }

    if synced > 0 {
        library = refresh_library(library);
        save_library(&library)?;
        emit(
            app,
            "success",
            "Кэш обновлений применен к добавленным играм.".to_string(),
            None,
            None,
        );
    }

    Ok(())
}

fn sync_cached_helper(
    app: &tauri::AppHandle,
    game_id: &str,
    profile: LauncherProfile,
    target: &Path,
) -> Result<bool, String> {
    let _ = (app, game_id, profile, target);
    Ok(false)
}

fn update_cache_dir() -> Result<PathBuf, String> {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .ok_or_else(|| "Не удалось определить папку для кэша обновлений.".to_string())?;
    Ok(base.join("Pirate Station").join("updates"))
}

fn cached_profile_dir(profile: LauncherProfile) -> Result<PathBuf, String> {
    Ok(update_cache_dir()?.join(profile.id))
}

fn cached_batch_path(profile: LauncherProfile) -> Result<PathBuf, String> {
    Ok(cached_profile_dir(profile)?.join(profile.batch_name))
}

fn cached_helper_path(profile: LauncherProfile) -> Result<PathBuf, String> {
    let file_name = match profile.id {
        PLUG_N_PLAY_LAUNCHER_ID => "drvloader.exe",
        _ => "kvc.exe",
    };
    Ok(cached_profile_dir(profile)?.join(file_name))
}

fn cached_updates_allowed() -> bool {
    let Ok(path) = update_cache_dir().map(|dir| dir.join("manifest.json")) else {
        return false;
    };
    let Ok(content) = read_text_lossy(&path) else {
        return false;
    };
    serde_json::from_str::<RemoteUpdateManifest>(&content).is_ok_and(|manifest| {
        manifest.version >= MIN_REMOTE_MANIFEST_VERSION && !manifest.profiles.is_empty()
    })
}

fn clear_update_cache_assets() {
    if let Ok(cache_dir) = update_cache_dir() {
        for profile in LAUNCHER_PROFILES {
            let _ = fs::remove_dir_all(cache_dir.join(profile.id));
        }
        let _ = fs::remove_file(cache_dir.join("manifest.json"));
    }
}

fn clear_cached_helper_assets() {
    for profile in LAUNCHER_PROFILES {
        if let Ok(path) = cached_helper_path(profile) {
            let _ = fs::remove_file(path);
        }
    }
}

fn remote_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn friendly_download_error(error: &str) -> String {
    let lower = error.to_lowercase();

    if lower.contains("sha256 не совпал") {
        return "SHA256 не совпал. Проверьте hash в manifest.json после обновления файла."
            .to_string();
    }

    if contains_any(&lower, &["404", "not found"]) {
        return "Файл не найден на GitHub. Проверьте имя файла, raw-ссылку и что репозиторий публичный."
            .to_string();
    }

    if contains_any(&lower, &["403", "401", "forbidden", "unauthorized"]) {
        return "GitHub не дал доступ к файлу. Raw-ссылка должна быть публичной и без token."
            .to_string();
    }

    if contains_any(
        &lower,
        &[
            "could not resolve host",
            "name resolution",
            "the remote name could not be resolved",
            "dns",
        ],
    ) {
        return "Не удалось открыть GitHub: проблема с DNS или интернетом. Будет использована локальная копия."
            .to_string();
    }

    if contains_any(
        &lower,
        &[
            "timed out",
            "timeout",
            "operation has timed out",
            "connection timed",
        ],
    ) {
        return "GitHub не ответил вовремя. Будет использована локальная копия.".to_string();
    }

    if contains_any(
        &lower,
        &["ssl", "tls", "schannel", "secure channel", "certificate"],
    ) {
        return "Ошибка TLS/сертификата при скачивании с GitHub. Часто помогает обновление Windows или смена сети."
            .to_string();
    }

    if contains_any(
        &lower,
        &[
            "access is denied",
            "permission denied",
            "operation did not complete",
            "virus",
            "antivirus",
            "defender",
        ],
    ) {
        return "Windows Defender или антивирус мог заблокировать скачивание exe. Будет использована локальная копия."
            .to_string();
    }

    "GitHub/raw недоступен на этом ПК или скачивание заблокировано. Программа продолжит работать с локальными файлами."
        .to_string()
}

fn normalize_sha256(value: Option<&str>) -> Option<String> {
    remote_value(value).map(|hash| hash.to_ascii_lowercase())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let script = format!(
        "(Get-FileHash -Algorithm SHA256 -LiteralPath '{}').Hash",
        powershell_escape(&path.display().to_string())
    );
    Ok(powershell_output(&script)?.trim().to_ascii_lowercase())
}

fn write_text_if_changed(target: &Path, content: &str) -> Result<bool, String> {
    if target.exists().then(|| {
        read_text_lossy(target)
            .ok()
            .is_some_and(|current| current == content)
    }) == Some(true)
    {
        return Ok(false);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Не удалось создать {}: {error}", parent.display()))?;
    }
    fs::write(target, content)
        .map_err(|error| format!("Не удалось записать {}: {error}", target.display()))?;
    Ok(true)
}

fn copy_file_if_changed(source: &Path, target: &Path) -> Result<bool, String> {
    if target.exists() && files_match(source, target)? {
        return Ok(false);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Не удалось создать {}: {error}", parent.display()))?;
    }
    fs::copy(source, target).map_err(|error| {
        format!(
            "Не удалось скопировать {} в {}: {error}",
            source.display(),
            target.display()
        )
    })?;
    Ok(true)
}

fn write_embedded_binary_if_changed(target: &Path, content: &[u8]) -> Result<bool, String> {
    if content.is_empty() {
        return Err(format!("{} не включён в эту сборку. Добавьте файлы профиля из разрешённого источника.", target.file_name().unwrap_or_default().to_string_lossy()));
    }
    if target.exists() && fs::read(target).is_ok_and(|current| current == content) {
        return Ok(false);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Не удалось создать {}: {error}", parent.display()))?;
    }
    fs::write(target, content)
        .map_err(|error| format!("Не удалось записать {}: {error}", target.display()))?;
    Ok(true)
}

fn files_match(left: &Path, right: &Path) -> Result<bool, String> {
    let left_meta = fs::metadata(left)
        .map_err(|error| format!("Не удалось прочитать {}: {error}", left.display()))?;
    let right_meta = fs::metadata(right)
        .map_err(|error| format!("Не удалось прочитать {}: {error}", right.display()))?;
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }

    let left_bytes = fs::read(left)
        .map_err(|error| format!("Не удалось прочитать {}: {error}", left.display()))?;
    let right_bytes = fs::read(right)
        .map_err(|error| format!("Не удалось прочитать {}: {error}", right.display()))?;
    Ok(left_bytes == right_bytes)
}

fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Не удалось создать {}: {error}", parent.display()))?;
    }
    if target.exists() {
        fs::remove_file(target)
            .map_err(|error| format!("Не удалось заменить {}: {error}", target.display()))?;
    }
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(source, target)
                .map_err(|error| format!("Не удалось сохранить {}: {error}", target.display()))?;
            let _ = fs::remove_file(source);
            Ok(())
        }
    }
}

fn read_batch_source(profile: LauncherProfile) -> String {
    if let Ok(root) = find_launcher_root() {
        let batch = root.join(profile.batch_name);
        if let Ok(content) = read_text_lossy(&batch) {
            return content;
        }
    }

    let embedded = match profile.id {
        PLUG_N_PLAY_LAUNCHER_ID => EMBEDDED_PLUG_N_PLAY_BATCH.to_string(),
        _ => EMBEDDED_HV_BATCH.to_string(),
    };

    if !embedded.trim().is_empty() {
        return embedded;
    }

    if cached_updates_allowed() {
        if let Ok(batch) = cached_batch_path(profile) {
            if let Ok(content) = read_text_lossy(&batch) {
                return content;
            }
        }
    }

    embedded
}

fn require_batch_source(content: &str, filename: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        Err(format!("{filename} не включён в эту сборку. Для запуска профиля нужны его файлы из разрешённого источника; остальные модули доступны."))
    } else {
        Ok(())
    }
}

fn patch_custom_exe(content: &str, loader_name: &str) -> String {
    let custom_line = format!(r#"set "CUSTOM_EXE_SUPPORT={}""#, loader_name);
    let mut replaced = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if line
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("set \"custom_exe_support=")
        {
            lines.push(custom_line.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    let patched = if replaced {
        lines.join("\r\n") + "\r\n"
    } else {
        let mut output = String::new();
        output.push_str("@echo off\r\n");
        output.push_str(&custom_line);
        output.push_str("\r\n");
        output.push_str(content);
        output
    };

    patch_plug_n_play_custom_exe(&patched)
}

fn patch_plug_n_play_custom_exe(content: &str) -> String {
    const MARKER: &str = "CUSTOM_EXE_SUPPORT selected by Pirate Station";
    if !content.contains(":find_launcher_exe") || content.contains(MARKER) {
        return content.to_string();
    }

    let needle = "set \"GAME_DIR=\"";
    let replacement = format!(
        "{needle}\r\n\r\n\
if defined CUSTOM_EXE_SUPPORT if exist \"%SCRIPT_DIR%%CUSTOM_EXE_SUPPORT%\" (\r\n\
    rem {MARKER}\r\n\
    set \"launcher_exe=%SCRIPT_DIR%%CUSTOM_EXE_SUPPORT%\"\r\n\
    for %%I in (\"!launcher_exe!\") do set \"GAME_DIR=%%~dpI\"\r\n\
    >>\"%LOG_FILE%\" echo [%date% %time%] custom launcher selected: \"!launcher_exe!\"\r\n\
)\r\n"
    );

    content.replacen(needle, &replacement, 1)
}

fn default_launcher_id() -> String {
    DEFAULT_LAUNCHER_ID.to_string()
}

fn launcher_profile(id: &str) -> Result<LauncherProfile, String> {
    LAUNCHER_PROFILES
        .iter()
        .copied()
        .find(|profile| profile.id == id)
        .ok_or_else(|| format!("Неизвестный профиль запуска: {id}"))
}

fn launcher_profile_or_default(id: &str) -> LauncherProfile {
    launcher_profile(id).unwrap_or(LAUNCHER_PROFILES[0])
}

fn launcher_view(profile: LauncherProfile) -> LauncherProfileView {
    LauncherProfileView {
        id: profile.id.to_string(),
        name: profile.name.to_string(),
        badge: profile.badge.to_string(),
        batch_name: profile.batch_name.to_string(),
        helper_label: profile.helper_label.to_string(),
        description: profile.description.to_string(),
    }
}

fn launcher_views() -> Vec<LauncherProfileView> {
    LAUNCHER_PROFILES
        .iter()
        .copied()
        .map(launcher_view)
        .collect()
}

fn library_view(library: Library) -> LibraryView {
    LibraryView {
        selected_id: library.selected_id,
        selected_launcher_id: library.selected_launcher_id,
        launchers: launcher_views(),
        games: library.games,
    }
}

fn helper_path(profile: LauncherProfile, dir: &Path) -> PathBuf {
    match profile.id {
        PLUG_N_PLAY_LAUNCHER_ID => dir.join("DenuvOwO").join("drvloader.exe"),
        _ => dir.join("kvc.exe"),
    }
}

fn load_library() -> Result<Library, String> {
    let path = library_path()?;
    if !path.exists() {
        return Ok(Library::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|error| format!("Не удалось прочитать {}: {error}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(Library::default());
    }

    match serde_json::from_str(&content) {
        Ok(library) => Ok(library),
        Err(_) => {
            backup_broken_library(&path);
            Ok(Library::default())
        }
    }
}

fn save_library(library: &Library) -> Result<(), String> {
    let path = library_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Не удалось создать {}: {error}", parent.display()))?;
    }

    let content = serde_json::to_string_pretty(library)
        .map_err(|error| format!("Не удалось сохранить библиотеку: {error}"))?;
    fs::write(&path, content)
        .map_err(|error| format!("Не удалось записать {}: {error}", path.display()))
}

fn library_path() -> Result<PathBuf, String> {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .ok_or_else(|| "Не удалось определить папку для настроек.".to_string())?;
    Ok(base.join("Pirate Station").join("games.json"))
}

fn backup_broken_library(path: &Path) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup = path.with_file_name(format!("games.broken-{stamp}.json"));
    let _ = fs::copy(path, backup);
}

fn refresh_library(mut library: Library) -> Library {
    if launcher_profile(&library.selected_launcher_id).is_err() {
        library.selected_launcher_id = default_launcher_id();
    }

    for game in &mut library.games {
        refresh_game(game);
    }

    if library.selected_id.is_none() {
        library.selected_id = library.games.first().map(|game| game.id.clone());
    }

    library
}

fn refresh_game(game: &mut GameEntry) {
    let dir = PathBuf::from(&game.dir_path);
    let exe = PathBuf::from(&game.exe_path);
    if game.launcher_id.trim().is_empty() {
        game.launcher_id = default_launcher_id();
    }

    let profile = launcher_profile_or_default(&game.launcher_id);
    game.launcher_name = profile.name.to_string();
    game.batch_name = profile.batch_name.to_string();
    game.helper_label = profile.helper_label.to_string();
    game.batch_found = dir.join(profile.batch_name).exists();
    game.kvc_found = helper_path(profile, &dir).exists();
    game.ready = exe.exists() && game.batch_found && game.kvc_found;
    game.last_message = if game.ready {
        "Готова к запуску.".to_string()
    } else if !exe.exists() {
        "EXE больше не найден.".to_string()
    } else if !game.batch_found && !game.kvc_found {
        "Нужна установка bat и kvc.".to_string()
    } else if !game.batch_found {
        "Нужен HV-Launcher.bat.".to_string()
    } else {
        "Нужен kvc.exe.".to_string()
    };
    game.last_message = if game.ready {
        format!("Готова к запуску через {}.", profile.name)
    } else if !exe.exists() {
        "EXE больше не найден.".to_string()
    } else if !game.batch_found && !game.kvc_found {
        format!(
            "Нужна установка {} и {}.",
            profile.batch_name, profile.helper_label
        )
    } else if !game.batch_found {
        format!("Нужен {}.", profile.batch_name)
    } else {
        format!("Нужен {}.", profile.helper_label)
    };
}

fn selected_game(library: &Library) -> Option<&GameEntry> {
    let selected_id = library.selected_id.as_deref()?;
    library.games.iter().find(|game| game.id == selected_id)
}

fn validate_exe_path(exe_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(exe_path.trim_matches('"'));
    if !path.exists() {
        return Err("Выбранный exe не найден.".to_string());
    }
    if !path.is_file() {
        return Err("Нужно выбрать именно exe-файл.".to_string());
    }
    if path
        .extension()
        .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("exe"))
        != Some(true)
    {
        return Err("Можно добавить только .exe файл.".to_string());
    }
    Ok(path)
}

fn find_launcher_root() -> Result<PathBuf, String> {
    let mut bases = Vec::new();

    if let Ok(current_dir) = env::current_dir() {
        bases.push(current_dir);
    }

    if let Ok(current_exe) = env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            bases.push(exe_dir.to_path_buf());
        }
    }

    for base in bases {
        for ancestor in base.ancestors().take(12) {
            if is_launcher_root(ancestor) {
                return Ok(ancestor.to_path_buf());
            }

            if ancestor.file_name().is_some_and(|name| name == "UI") {
                if let Some(parent) = ancestor.parent() {
                    if is_launcher_root(parent) {
                        return Ok(parent.to_path_buf());
                    }
                }
            }
        }
    }

    Err("HV-Launcher.bat не найден рядом с UI.".to_string())
}

fn is_launcher_root(path: &Path) -> bool {
    LAUNCHER_PROFILES
        .iter()
        .any(|profile| path.join(profile.batch_name).exists())
        || path.join("kvc.exe").exists()
        || path.join("drvloader.exe").exists()
        || path.join("DenuvOwO").join("drvloader.exe").exists()
}

fn is_admin() -> bool {
    let mut command = Command::new("fltmc");
    command_success(&mut command)
}

fn command_success(command: &mut Command) -> bool {
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    command.status().is_ok_and(|status| status.success())
}

fn process_name_running(process_name: &str) -> Result<bool, String> {
    let process_name = process_name.trim();
    if process_name.is_empty() {
        return Ok(false);
    }

    let mut command = Command::new("tasklist.exe");
    command
        .arg("/FI")
        .arg(format!("IMAGENAME eq {process_name}"))
        .arg("/FO")
        .arg("CSV")
        .arg("/NH");

    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|error| format!("Не удалось проверить процесс игры: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Проверка процесса игры завершилась с ошибкой: {}",
            compact_command_output(&output)
        ));
    }

    Ok(tasklist_contains_process(
        &String::from_utf8_lossy(&output.stdout),
        process_name,
    ))
}

fn tasklist_contains_process(output: &str, process_name: &str) -> bool {
    let expected = format!("\"{}\"", process_name.trim()).to_ascii_lowercase();
    output
        .lines()
        .map(str::trim)
        .any(|line| line.to_ascii_lowercase().starts_with(&expected))
}

fn powershell_output(script: &str) -> Result<String, String> {
    let mut command = Command::new("powershell.exe");
    let utf8_script = format!(
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); \
         $OutputEncoding = [System.Text.UTF8Encoding]::new(); \
         {script}"
    );
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-STA")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(utf8_script);

    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|error| format!("PowerShell не запустился: {error}"))?;

    if !output.status.success() {
        return Err(compact_command_output(&output));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_powershell(script: &str) -> Result<(), String> {
    powershell_output(script).map(|_| ())
}

fn powershell_escape(value: &str) -> String {
    value.replace('\'', "''")
}

fn compact_command_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let mut line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("команда завершилась с ошибкой")
        .to_string();

    if line.len() > 220 {
        line.truncate(220);
        line.push_str("...");
    }

    line
}

fn read_text_lossy(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("Не удалось прочитать {}: {error}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn make_game_id(path: &Path) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "game".to_string())
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();

    format!("{}-{}", name.to_ascii_lowercase(), stamp)
}

fn emit_launch_diagnostics(app: &tauri::AppHandle, game: &GameEntry, profile: LauncherProfile) {
    emit(
        app,
        "info",
        format!(
            "Диагностика запуска: профиль {} ({}) для {}.",
            profile.name, profile.badge, game.name
        ),
        None,
        Some(game.id.clone()),
    );

    match system_security_summary() {
        Ok(summary) => emit(app, "info", summary, None, Some(game.id.clone())),
        Err(error) => emit(
            app,
            "warning",
            format!("Диагностику системы не удалось прочитать: {error}"),
            None,
            Some(game.id.clone()),
        ),
    }
}

fn system_security_summary() -> Result<String, String> {
    let script = r#"
$ErrorActionPreference='SilentlyContinue'
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$name = if ([int]$cv.CurrentBuild -ge 22000) { 'Windows 11' } else { 'Windows 10' }
$display = if ($cv.DisplayVersion) { $cv.DisplayVersion } else { $cv.ReleaseId }
$build = $cv.CurrentBuild
$ciPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity'
$ci = (Get-ItemProperty $ciPath -Name Enabled -ErrorAction SilentlyContinue).Enabled
if ($null -eq $ci) { $ciText = 'unknown' } elseif ($ci -eq 1) { $ciText = 'ON' } else { $ciText = 'OFF' }
$sbText = 'unknown'
try {
  $sb = Confirm-SecureBootUEFI -ErrorAction Stop
  if ($sb) { $sbText = 'ON' } else { $sbText = 'OFF' }
} catch {}
"{0} {1} build {2}; Core Isolation/HVCI: {3}; Secure Boot: {4}" -f $name,$display,$build,$ciText,$sbText
"#;

    let output = powershell_output(script)?;
    let summary = output.lines().next().unwrap_or("").trim();
    if summary.is_empty() {
        Err("PowerShell вернул пустой ответ".to_string())
    } else {
        Ok(format!("Система: {summary}"))
    }
}

fn remember_batch_line(lines: &Arc<Mutex<Vec<String>>>, raw_line: &str) {
    let line = strip_ansi(raw_line).trim().to_string();
    if line.is_empty() {
        return;
    }

    if let Ok(mut lines) = lines.lock() {
        lines.push(line);
        if lines.len() > 240 {
            lines.remove(0);
        }
    }
}

fn diagnose_launch_failure(profile: LauncherProfile, dir: &Path, output: &[String]) -> String {
    let context = diagnostic_context(profile, dir, output);
    let lower = context.to_lowercase();
    let log_hint = diagnostic_log_hint(profile, dir);

    let reason = if contains_any(
        &lower,
        &[
            "core isolation is enabled",
            "core isolation (memory integrity) is on",
            "memory integrity) is on",
            "error: core isolation",
            "check_core_isolation failed",
            "failure: core isolation",
        ],
    ) {
        "Похоже, включена «Изоляция ядра / Memory Integrity». Отключите её и перезагрузите ПК."
    } else if contains_any(
        &lower,
        &[
            "administrator privileges",
            "requires administrator",
            "ensure_admin",
            "run as administrator",
            "uac",
        ],
    ) {
        "Похоже, не хватило прав администратора или UAC не подтвердили. Запустите программу от имени администратора."
    } else if contains_any(
        &lower,
        &[
            "drvloader preparation failed",
            "drvloader bypass failed",
            "drvloader restore failed",
            "drvloader.exe could not be prepared",
            "bypass command failed",
            "restore command failed",
        ],
    ) {
        "Проблема с DRV: drvloader.exe не подготовился или не смог выполнить команду. Часто это блокировка антивирусом, несовместимая Windows или включенная защита ядра."
    } else if contains_any(
        &lower,
        &[
            "0x80070241",
            "startservicew failed",
            "digital signature",
            "windows cannot verify the digital signature",
            "failed to initialize watchdog",
        ],
    ) {
        "Служба игры не стартовала из-за проверки цифровой подписи. Для таких случаев увеличено ожидание перед восстановлением после запуска; также проверьте изоляцию ядра и выбранный профиль."
    } else if contains_any(&lower, &["failed to disable dse", "failed to enable dse"]) {
        "Не удалось переключить DSE. Обычно причина в защите ядра, правах администратора или блокировке служебного файла."
    } else if contains_any(
        &lower,
        &[
            "kvc.exe не появился",
            "failed to download kvc",
            "failed to move kvc.exe",
            "kvc.7z",
            "kvc.exe отсутствует",
        ],
    ) {
        "Проблема с KVC: kvc.exe отсутствует, не скачался, не распаковался или был заблокирован Защитником Windows."
    } else if contains_any(
        &lower,
        &[
            "download failed",
            "failed to download",
            "extraction failed",
            "failed to download 7zr",
            "invoke-webrequest",
            "404",
        ],
    ) {
        "Не удалось скачать или распаковать нужный файл. Проверьте интернет, GitHub/raw-ссылки и блокировку антивирусом."
    } else if contains_any(
        &lower,
        &[
            "game executable not found",
            "find_launcher_exe failed",
            "no launcher candidates",
            "выбранный exe не найден",
            "exe больше не найден",
        ],
    ) {
        "Не найден exe игры. Проверьте, что игра не перенесена и в карточке выбран правильный exe."
    } else if contains_any(
        &lower,
        &[
            "access is denied",
            "permission denied",
            "unauthorized",
            "operation did not complete",
            "contains a virus",
            "заблок",
            "отказано",
            "антивирус",
            "defender",
        ],
    ) {
        "Похоже, файл заблокирован Windows Defender/антивирусом или нет доступа к папке игры."
    } else {
        "Точная причина не определена. Откройте подробный лог батника или последние строки Live Log."
    };

    match log_hint {
        Some(path) => format!("{reason} Подробный лог: {path}"),
        None => reason.to_string(),
    }
}

fn diagnostic_context(profile: LauncherProfile, dir: &Path, output: &[String]) -> String {
    let mut context = output.join("\n");

    for path in diagnostic_log_paths(profile, dir) {
        if let Ok(content) = read_text_lossy(&path) {
            context.push('\n');
            context.push_str(&last_lines(&content, 120));
        }
    }

    context
}

fn diagnostic_log_hint(profile: LauncherProfile, dir: &Path) -> Option<String> {
    diagnostic_log_paths(profile, dir)
        .into_iter()
        .find(|path| path.exists())
        .map(|path| path.display().to_string())
}

fn clear_diagnostic_logs(profile: LauncherProfile, dir: &Path) {
    for path in diagnostic_log_paths(profile, dir) {
        let _ = fs::remove_file(path);
    }
}

fn diagnostic_log_paths(profile: LauncherProfile, dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if profile.id == PLUG_N_PLAY_LAUNCHER_ID {
        if let Some(system_root) = env::var_os("SystemRoot") {
            paths.push(
                PathBuf::from(system_root)
                    .join("Temp")
                    .join("HV-PlugNPlay-debug.log"),
            );
        }
        paths.push(PathBuf::from(r"C:\Windows\Temp\HV-PlugNPlay-debug.log"));
    } else {
        paths.push(dir.join("HV-Launcher-Error.log"));
    }

    paths
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn last_lines(content: &str, max_lines: usize) -> String {
    let mut lines = content.lines().rev().take(max_lines).collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

fn forward_batch_line(
    app: &tauri::AppHandle,
    raw_line: &str,
    stderr: bool,
    game_id: Option<String>,
) {
    let line = strip_ansi(raw_line).trim().to_string();
    if line.is_empty() {
        return;
    }

    let step = detect_step(&line);
    let lower = line.to_ascii_lowercase();
    let kind = if stderr || lower.contains("error") {
        "error"
    } else if lower.contains("warning") {
        "warning"
    } else if lower.contains("success")
        || lower.contains("ready")
        || lower.contains("detected:")
        || lower.contains("found:")
    {
        "success"
    } else {
        "info"
    };

    emit(app, kind, line, step, game_id);
}

fn detect_step(line: &str) -> Option<u8> {
    let marker = "[STEP ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let number = rest.split('/').next()?;
    number.trim().parse::<u8>().ok()
}

fn emit(
    app: &tauri::AppHandle,
    kind: &str,
    message: String,
    step: Option<u8>,
    game_id: Option<String>,
) {
    let _ = app.emit(
        "station-event",
        UiEvent {
            kind: kind.to_string(),
            message,
            step,
            game_id,
        },
    );
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek().is_some_and(|next| *next == '[') {
                let _ = chars.next();
                for sequence_char in chars.by_ref() {
                    if sequence_char.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }

        output.push(ch);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{tasklist_contains_process, require_batch_source, write_embedded_binary_if_changed};

    #[test]
    fn missing_profile_batch_is_rejected_instead_of_generating_an_empty_launcher() {
        assert!(require_batch_source("", "HV-Launcher.bat").is_err());
        assert!(require_batch_source(" \r\n", "HV-PlugNPlay.bat").is_err());
        assert!(require_batch_source("@echo off\r\n", "HV-Launcher.bat").is_ok());
    }

    #[test]
    fn missing_embedded_helper_never_creates_or_truncates_a_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("drvloader.exe");
        assert!(write_embedded_binary_if_changed(&target, &[]).is_err());
        assert!(!target.exists());
        std::fs::write(&target, b"existing helper").unwrap();
        assert!(write_embedded_binary_if_changed(&target, &[]).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"existing helper");
    }

    #[test]
    fn tasklist_detects_selected_executable_case_insensitively() {
        let output = "\"BBQ-Win64-Shipping.exe\",\"14840\",\"Console\",\"1\",\"512,000 K\"\r\n";
        assert!(tasklist_contains_process(output, "bbq-win64-shipping.exe"));
    }

    #[test]
    fn tasklist_ignores_localized_no_process_message() {
        assert!(!tasklist_contains_process(
            "INFO: No tasks are running which match the specified criteria.\r\n",
            "Game.exe"
        ));
    }
}
