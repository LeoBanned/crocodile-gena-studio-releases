use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Emitter;
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        UI::Shell::{
            FOLDERID_Desktop, ILCreateFromPathW, ILFree, IShellLinkW, SHGetKnownFolderPath,
            ShellLink, KF_FLAG_DEFAULT,
        },
    },
};

use std::os::windows::process::CommandExt;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELPER_NAME: &str = "Crocodile-HV.exe";
const HELPER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/hypervisor/Crocodile-HV.exe"
));
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn require_steam_closed() -> Result<(), String> {
    if process_name_running("steam.exe")? {
        return Err("Полностью закройте Steam через «Steam → Выход», затем повторите. Окно Steam, свёрнутое в трей, всё ещё работает.".into());
    }
    Ok(())
}

fn steam_target(id: &str) -> Result<(PathBuf, u64, PathBuf), String> {
    let library = load_library()?;
    let game = library.games.iter().find(|g|g.id==id).ok_or("Игра не найдена")?;
    let exe = validate_exe_path(&game.exe_path)?;
    let installed = exe.parent().into_iter().flat_map(Path::ancestors)
        .find_map(crate::steam_library::installed_game_for_path)
        .ok_or("Игра не найдена среди установленных игр Steam. Установите её через Steam и выберите EXE из её папки.")?;
    let steam = crate::luatools::resolve_steam_path(None).ok_or("Папка Steam не найдена")?;
    Ok((steam, installed.appid, exe.parent().unwrap().join(HELPER_NAME)))
}

#[tauri::command]
pub(crate) fn steam_launch_accounts() -> Result<Vec<crate::steam_hv::Account>, String> {
    let steam=crate::luatools::resolve_steam_path(None).ok_or("Папка Steam не найдена")?;
    crate::steam_hv::accounts(&steam)
}
#[tauri::command]
pub(crate) fn steam_launch_status(id: String, account: Option<String>) -> Result<crate::steam_hv::Status, String> {
    let (steam, appid, helper) = steam_target(&id)?;
    crate::steam_hv::status_for(&steam, appid, &helper,account.as_deref())
}

#[tauri::command]
pub(crate) async fn set_steam_launch(id: String, enabled: bool, account: Option<String>) -> Result<crate::steam_hv::Status, String> {
    tauri::async_runtime::spawn_blocking(move||set_steam_launch_inner(id,enabled,account)).await.map_err(|error|format!("Настройка Steam прервана: {error}"))?
}
fn set_steam_launch_inner(id:String,enabled:bool,account:Option<String>)->Result<crate::steam_hv::Status,String>{
    static LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());
    let _guard=LOCK.lock().map_err(|_|"Настройка Steam прервана")?;
    let (steam, appid, helper) = steam_target(&id)?;
    let before=crate::steam_hv::status_for(&steam,appid,&helper,account.as_deref())?;
    if enabled { ensure_helper(helper.parent().unwrap())?; }
    let client=steam.join("steam.exe");
    if !client.is_file(){return Err("Не найден steam.exe".into())}
    let was_running=process_name_running("steam.exe")?;
    crate::steam_hv::restart_around(was_running,||{
        Command::new(&client).arg("-shutdown").current_dir(&steam).creation_flags(CREATE_NO_WINDOW).spawn().map_err(|error|format!("Не удалось завершить Steam: {error}"))?;
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        loop {
            if !process_name_running("steam.exe")? {return Ok(())}
            if std::time::Instant::now()>=deadline{return Err("Steam не завершился за 30 секунд. Параметры не изменены; проверьте, не ожидает ли Steam завершения игры.".into())}
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    },||crate::steam_hv::set(&steam,appid,&helper,enabled,Some(&before.account)),||{
        Command::new(&client).current_dir(&steam).spawn().map(|_|()).map_err(|error|error.to_string())
    })
}

#[derive(Clone, Serialize, Deserialize)]
struct GameEntry {
    id: String,
    name: String,
    exe_path: String,
    #[serde(default)]
    display_exe_path: String,
    dir_path: String,
    loader_name: String,
    #[serde(default)]
    shortcut_path: String,
    #[serde(default)]
    shortcut_brand_version: u8,
    #[serde(default)]
    ready: bool,
    #[serde(default)]
    last_message: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Library {
    selected_id: Option<String>,
    games: Vec<GameEntry>,
}

#[derive(Serialize)]
pub(crate) struct LibraryView {
    selected_id: Option<String>,
    games: Vec<GameEntry>,
}

#[derive(Serialize)]
pub(crate) struct AppStatus {
    version: String,
    admin: bool,
    games_count: usize,
    game_running: bool,
    selected_game: Option<GameEntry>,
}

#[derive(Serialize)]
pub(crate) struct RunReport {
    success: bool,
    code: Option<i32>,
    message: String,
}

#[derive(Clone, Serialize)]
struct UiEvent {
    kind: String,
    message: String,
    step: Option<u8>,
    game_id: Option<String>,
}

#[tauri::command]
pub(crate) fn get_app_status() -> Result<AppStatus, String> {
    let library = refresh_library(load_library()?);
    let selected_game = selected_game(&library).cloned();
    let game_running = selected_game
        .as_ref()
        .is_some_and(|game| process_name_running(&game.loader_name).unwrap_or(false));
    Ok(AppStatus {
        version: VERSION.to_string(),
        admin: is_admin(),
        games_count: library.games.len(),
        game_running,
        selected_game,
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
    let mut library = load_library()?;
    for game in &mut library.games {
        if game.shortcut_brand_version == 0 {
            let _ = update_game_shortcut(game);
        }
    }
    let library = refresh_library(library);
    save_library(&library)?;
    Ok(library_view(library))
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
    let powershell = system_tool("WindowsPowerShell/v1.0/powershell.exe");
    let output = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("Не удалось открыть выбор файла: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Выбор файла завершился с ошибкой: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then_some(path))
}

#[tauri::command]
pub(crate) fn add_game(
    app: tauri::AppHandle,
    exe_path: String,
    _launcher_id: Option<String>,
) -> Result<LibraryView, String> {
    let exe = validate_exe_path(&exe_path)?;
    let dir = exe.parent().ok_or("Не удалось определить папку игры.")?;
    let exe_text = exe.to_string_lossy().to_string();
    let mut library = load_library()?;
    let existing = library.games.iter().find(|game| {
        Path::new(&game.exe_path)
            .canonicalize()
            .is_ok_and(|path| shell_path(&path) == exe)
    });
    let id = existing
        .map(|game| game.id.clone())
        .unwrap_or_else(|| make_game_id(&exe));
    let existing_shortcut = existing.map(|game| game.shortcut_path.as_str());
    let display_exe = display_exe_for(&exe);
    let name = display_exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "Game".to_string());
    emit(
        &app,
        "info",
        format!("Подготавливаю {}.", exe.display()),
        Some(id.clone()),
    );
    ensure_helper(dir)?;
    let shortcut = ensure_shortcut(&exe, &display_exe, dir, &name, existing_shortcut)?;

    let game = GameEntry {
        id: id.clone(),
        name,
        exe_path: exe_text,
        display_exe_path: display_exe.to_string_lossy().to_string(),
        dir_path: dir.to_string_lossy().to_string(),
        loader_name: exe
            .file_name()
            .ok_or("Не удалось определить имя EXE.")?
            .to_string_lossy()
            .to_string(),
        shortcut_path: shortcut.to_string_lossy().to_string(),
        shortcut_brand_version: 1,
        ready: true,
        last_message: "Лаунчер и ярлык на рабочем столе готовы.".to_string(),
    };
    library.games.retain(|entry| entry.id != id);
    library.games.push(game);
    library.selected_id = Some(id.clone());
    save_library(&library)?;
    emit(
        &app,
        "success",
        format!("{} добавлена. Ярлык: {}", exe.display(), shortcut.display()),
        Some(id),
    );
    Ok(library_view(refresh_library(library)))
}

#[tauri::command]
pub(crate) fn set_selected_game(id: String) -> Result<LibraryView, String> {
    let mut library = load_library()?;
    if !library.games.iter().any(|game| game.id == id) {
        return Err("Игра не найдена в библиотеке.".to_string());
    }
    library.selected_id = Some(id);
    save_library(&library)?;
    Ok(library_view(refresh_library(library)))
}

#[tauri::command]
pub(crate) fn remove_game(id: String) -> Result<LibraryView, String> {
    if steam_launch_accounts().unwrap_or_default().into_iter().any(|account|steam_launch_status(id.clone(),Some(account.id)).is_ok_and(|status|status.managed)) {
        return Err("Сначала нажмите «Вернуть обычный запуск» для этой игры, затем удалите её из библиотеки.".into());
    }
    let mut library = load_library()?;
    library.games.retain(|game| game.id != id);
    if library.selected_id.as_deref() == Some(&id) {
        library.selected_id = library.games.first().map(|game| game.id.clone());
    }
    save_library(&library)?;
    Ok(library_view(refresh_library(library)))
}

#[tauri::command]
pub(crate) fn verify_game(app: tauri::AppHandle, id: String) -> Result<LibraryView, String> {
    let mut library = load_library()?;
    let game = library
        .games
        .iter_mut()
        .find(|game| game.id == id)
        .ok_or("Игра не найдена в библиотеке.")?;
    update_game_shortcut(game)?;
    refresh_game(game);
    save_library(&library)?;
    emit(
        &app,
        "success",
        "Crocodile-HV и ярлык игры проверены.".to_string(),
        Some(id),
    );
    Ok(library_view(library))
}

fn update_game_shortcut(game: &mut GameEntry) -> Result<(), String> {
    let exe = validate_exe_path(&game.exe_path)?;
    let dir = exe.parent().ok_or("Не удалось определить папку игры.")?;
    let display_exe = display_exe_for(&exe);
    game.name = display_exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "Game".to_string());
    game.display_exe_path = display_exe.to_string_lossy().to_string();
    ensure_helper(dir)?;
    let shortcut = ensure_shortcut(
        &exe,
        &display_exe,
        dir,
        &game.name,
        Some(&game.shortcut_path),
    )?;
    game.shortcut_path = shortcut.to_string_lossy().to_string();
    game.shortcut_brand_version = 1;
    Ok(())
}

#[tauri::command]
pub(crate) async fn launch_selected_game(app: tauri::AppHandle) -> Result<RunReport, String> {
    tauri::async_runtime::spawn_blocking(move || launch_selected_game_inner(app))
        .await
        .map_err(|error| format!("Задача запуска завершилась ошибкой: {error}"))?
}

fn launch_selected_game_inner(app: tauri::AppHandle) -> Result<RunReport, String> {
    let library = load_library()?;
    let game = selected_game(&library).ok_or("Сначала добавьте и выберите игру.")?;
    let exe = validate_exe_path(&game.exe_path)?;
    let dir = exe.parent().ok_or("Не удалось определить папку игры.")?;
    ensure_helper(dir)?;
    let helper = dir.join(HELPER_NAME);
    Command::new(&helper)
        .arg(&exe)
        .current_dir(dir)
        .spawn()
        .map_err(|error| format!("Не удалось запустить Crocodile-HV.exe: {error}"))?;
    let message = format!("Crocodile-HV запущен для {}.", game.name);
    emit(&app, "success", message.clone(), Some(game.id.clone()));
    Ok(RunReport {
        success: true,
        code: None,
        message,
    })
}

fn ensure_helper(dir: &Path) -> Result<(), String> {
    let target = dir.join(HELPER_NAME);
    match fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(format!("{} не является обычным файлом.", target.display()));
            }
            if helper_matches(&target) {
                return Ok(());
            }
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let backup = dir.join(format!("Crocodile-HV.previous-{stamp}.exe"));
            fs::rename(&target, &backup).map_err(|error| {
                format!("Не удалось сохранить прежний {}: {error}", target.display())
            })?;
            if let Err(error) = fs::write(&target, HELPER_BYTES) {
                let _ = fs::rename(&backup, &target);
                return Err(format!("Не удалось обновить {}: {error}", target.display()));
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Не удалось проверить {}: {error}",
                target.display()
            ));
        }
    }
    fs::write(&target, HELPER_BYTES)
        .map_err(|error| format!("Не удалось установить {}: {error}", target.display()))
}

fn helper_matches(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    Sha256::digest(&bytes) == Sha256::digest(HELPER_BYTES)
}

fn display_exe_for(launch_exe: &Path) -> PathBuf {
    let Some(file_name) = launch_exe.file_name().and_then(|name| name.to_str()) else {
        return launch_exe.to_path_buf();
    };
    if !file_name.eq_ignore_ascii_case("hypervisor-launcher.exe")
        && !file_name.eq_ignore_ascii_case("hypervisor-laucher.exe")
    {
        return launch_exe.to_path_buf();
    }
    let Some(dir) = launch_exe.parent() else {
        return launch_exe.to_path_buf();
    };
    let folder_name = dir
        .file_name()
        .map(|name| normalized_name(&name.to_string_lossy()))
        .unwrap_or_default();
    let Ok(entries) = fs::read_dir(dir) else {
        return launch_exe.to_path_buf();
    };
    let mut candidates: Vec<(PathBuf, String)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        })
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy();
            let lower = stem.to_ascii_lowercase();
            let excluded = lower.starts_with("unitycrashhandler")
                || lower.starts_with("crash")
                || lower.starts_with("unins")
                || lower.starts_with("uninstall")
                || lower.starts_with("setup")
                || lower.starts_with("install")
                || lower.starts_with("vc_redist")
                || lower.starts_with("dxsetup")
                || lower == "crocodile-hv"
                || lower == "eclipse-hv"
                || lower == "hypervisor-launcher"
                || lower == "hypervisor-laucher";
            let normalized = normalized_name(&stem);
            (!excluded).then_some((path, normalized))
        })
        .collect();
    if candidates.len() == 1 {
        return candidates.remove(0).0;
    }
    let matches: Vec<_> = candidates
        .into_iter()
        .filter(|(_, stem)| !folder_name.is_empty() && stem == &folder_name)
        .collect();
    if matches.len() == 1 {
        return matches.into_iter().next().unwrap().0;
    }
    launch_exe.to_path_buf()
}

fn normalized_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn ensure_shortcut(
    exe: &Path,
    display_exe: &Path,
    dir: &Path,
    name: &str,
    preferred: Option<&str>,
) -> Result<PathBuf, String> {
    let exe = exe.to_path_buf();
    let display_exe = display_exe.to_path_buf();
    let dir = dir.to_path_buf();
    let name = name.to_string();
    let preferred = preferred.map(PathBuf::from);
    std::thread::spawn(move || {
        create_shortcut_sta(&exe, &display_exe, &dir, &name, preferred.as_deref())
    })
    .join()
    .map_err(|_| "Не удалось создать ярлык игры: поток завершился аварийно.".to_string())?
}

fn create_shortcut_sta(
    exe: &Path,
    display_exe: &Path,
    dir: &Path,
    name: &str,
    preferred: Option<&Path>,
) -> Result<PathBuf, String> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|error| format!("Не удалось инициализировать Windows Shell: {error}"))?;
    let result = create_shortcut_initialized(exe, display_exe, dir, name, preferred);
    unsafe { CoUninitialize() };
    result
}

fn create_shortcut_initialized(
    exe: &Path,
    display_exe: &Path,
    dir: &Path,
    name: &str,
    preferred: Option<&Path>,
) -> Result<PathBuf, String> {
    let desktop_pointer = unsafe { SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None) }
        .map_err(|error| format!("Не удалось найти рабочий стол: {error}"))?;
    let desktop_text = unsafe { desktop_pointer.to_string() };
    unsafe { CoTaskMemFree(Some(desktop_pointer.0.cast())) };
    let desktop = PathBuf::from(
        desktop_text
            .map_err(|error| format!("Не удалось прочитать путь рабочего стола: {error}"))?,
    );
    if !desktop.is_dir() {
        return Err(format!("Рабочий стол не найден: {}", desktop.display()));
    }
    let old_shortcut = preferred.filter(|path| {
        path.parent() == Some(desktop.as_path())
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            && path.is_file()
    });
    let safe_name = safe_shortcut_name(name);
    let mut shortcut = desktop.join(format!("{safe_name} (Crocodile HV).lnk"));
    for number in 2..=100 {
        if !shortcut.exists() || old_shortcut == Some(shortcut.as_path()) {
            break;
        }
        shortcut = desktop.join(format!("{safe_name} (Crocodile HV {number}).lnk"));
    }
    if shortcut.exists() && old_shortcut != Some(shortcut.as_path()) {
        return Err("Не удалось подобрать свободное имя ярлыка.".to_string());
    }

    write_shortcut_initialized(exe, display_exe, dir, name, &shortcut)?;
    if let Some(old) = old_shortcut.filter(|old| *old != shortcut.as_path()) {
        let _ = fs::remove_file(old);
    }
    Ok(shortcut)
}

fn write_shortcut_initialized(
    exe: &Path,
    display_exe: &Path,
    dir: &Path,
    name: &str,
    shortcut: &Path,
) -> Result<(), String> {
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| format!("Не удалось создать Windows-ярлык: {error}"))?;
    let target = wide(&dir.join(HELPER_NAME));
    let argument = wide_quoted_path(exe);
    let working_dir = wide(dir);
    let icon = wide(display_exe);
    let description = wide_text(&format!("Crocodile-HV: запуск игры {name}"));
    let target_pidl = unsafe { ILCreateFromPathW(PCWSTR(target.as_ptr())) };
    let pidl_result = if target_pidl.is_null() {
        Err("Windows Shell не смог определить файл лаунчера.".to_string())
    } else {
        let result = unsafe { link.SetIDList(target_pidl) }
            .map_err(|error| format!("Не удалось задать цель через Windows Shell: {error}"));
        unsafe { ILFree(Some(target_pidl)) };
        result
    };
    if let Err(pidl_error) = pidl_result {
        unsafe { link.SetPath(PCWSTR(target.as_ptr())) }.map_err(|error| {
            format!(
                "Не удалось задать цель ярлыка {}: {pidl_error}; резервный способ: {error}",
                dir.join(HELPER_NAME).display()
            )
        })?;
    }
    unsafe {
        link.SetArguments(PCWSTR(argument.as_ptr()))
            .map_err(|error| format!("Не удалось задать игру для ярлыка: {error}"))?;
        link.SetWorkingDirectory(PCWSTR(working_dir.as_ptr()))
            .map_err(|error| format!("Не удалось задать папку игры: {error}"))?;
        link.SetIconLocation(PCWSTR(icon.as_ptr()), 0)
            .map_err(|error| format!("Не удалось задать значок игры: {error}"))?;
        link.SetDescription(PCWSTR(description.as_ptr()))
            .map_err(|error| format!("Не удалось задать описание ярлыка: {error}"))?;
    }
    let persist: IPersistFile = link
        .cast()
        .map_err(|error| format!("Не удалось сохранить ярлык: {error}"))?;
    let output = wide(&shortcut);
    unsafe { persist.Save(PCWSTR(output.as_ptr()), true) }
        .map_err(|error| format!("Не удалось записать {}: {error}", shortcut.display()))?;
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    shell_path(path)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect()
}

fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn wide_quoted_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut result = vec![b'"' as u16];
    result.extend(shell_path(path).as_os_str().encode_wide());
    result.extend([b'"' as u16, 0]);
    result
}

fn shell_path(path: &Path) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if !units.starts_with(&prefix) {
        return path.to_path_buf();
    }
    let unc = [b'U' as u16, b'N' as u16, b'C' as u16, b'\\' as u16];
    let normalized = if units.len() >= 8
        && units[4..8].iter().zip(unc).all(|(&actual, expected)| {
            actual == expected || (expected != b'\\' as u16 && actual == expected + 32)
        }) {
        let mut value = vec![b'\\' as u16, b'\\' as u16];
        value.extend_from_slice(&units[8..]);
        value
    } else if units.get(5) == Some(&(b':' as u16)) {
        units[4..].to_vec()
    } else {
        return path.to_path_buf();
    };
    PathBuf::from(OsString::from_wide(&normalized))
}

fn safe_shortcut_name(name: &str) -> String {
    let result = name
        .chars()
        .filter(|ch| !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .collect::<String>()
        .trim_end_matches([' ', '.'])
        .trim()
        .to_string();
    if result.is_empty() {
        "Игра".to_string()
    } else {
        result
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
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let _ = fs::copy(
                &path,
                path.with_file_name(format!("games.broken-{stamp}.json")),
            );
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
        .map_err(|error| format!("Не удалось сериализовать библиотеку: {error}"))?;
    fs::write(&path, content)
        .map_err(|error| format!("Не удалось сохранить {}: {error}", path.display()))
}

fn library_path() -> Result<PathBuf, String> {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("Не удалось найти папку Local AppData.")?;
    Ok(base.join("Pirate Station").join("games.json"))
}

fn library_view(library: Library) -> LibraryView {
    LibraryView {
        selected_id: library.selected_id,
        games: library.games,
    }
}

fn refresh_library(mut library: Library) -> Library {
    for game in &mut library.games {
        refresh_game(game);
    }
    if !library
        .selected_id
        .as_ref()
        .is_some_and(|id| library.games.iter().any(|game| &game.id == id))
    {
        library.selected_id = library.games.first().map(|game| game.id.clone());
    }
    library
}

fn refresh_game(game: &mut GameEntry) {
    let exe = Path::new(&game.exe_path);
    let helper = Path::new(&game.dir_path).join(HELPER_NAME);
    let shortcut = Path::new(&game.shortcut_path);
    game.ready = exe.is_file() && helper_matches(&helper) && shortcut.is_file();
    game.last_message = if !exe.is_file() {
        "EXE игры больше не найден.".to_string()
    } else if !helper_matches(&helper) {
        "Нужно установить Crocodile-HV.exe.".to_string()
    } else if !shortcut.is_file() {
        "Нужно создать ярлык игры на рабочем столе.".to_string()
    } else {
        "Готова к запуску через Crocodile-HV.".to_string()
    };
}

fn selected_game(library: &Library) -> Option<&GameEntry> {
    library
        .selected_id
        .as_ref()
        .and_then(|id| library.games.iter().find(|game| &game.id == id))
}

fn validate_exe_path(exe_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(exe_path.trim_matches('"'));
    if !path.is_file() {
        return Err("Выбранный EXE игры не найден.".to_string());
    }
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return Err("Можно добавить только EXE-файл.".to_string());
    }
    if path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(HELPER_NAME))
    {
        return Err("Выберите оригинальный EXE игры, а не Crocodile-HV.exe.".to_string());
    }
    path.canonicalize()
        .map(|canonical| shell_path(&canonical))
        .map_err(|error| format!("Не удалось определить путь игры: {error}"))
}

fn make_game_id(path: &Path) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    format!("{}-{stamp}", name.to_ascii_lowercase())
}

fn system_tool(relative: &str) -> PathBuf {
    env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join(relative)
}

fn is_admin() -> bool {
    Command::new(system_tool("fltmc.exe"))
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|status| status.success())
}

fn process_name_running(name: &str) -> Result<bool, String> {
    let output = Command::new(system_tool("tasklist.exe"))
        .args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("Не удалось проверить процесс игры: {error}"))?;
    if !output.status.success() {
        return Err("Проверка процесса игры завершилась ошибкой.".to_string());
    }
    let expected = format!("\"{}\"", name.to_ascii_lowercase());
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with(&expected)))
}

fn emit(app: &tauri::AppHandle, kind: &str, message: String, game_id: Option<String>) {
    let _ = app.emit(
        "station-event",
        UiEvent {
            kind: kind.to_string(),
            message,
            step: None,
            game_id,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::STGM_READ;

    #[test]
    fn legacy_library_migrates_without_old_profiles() {
        let json = r#"{"selected_id":"x","selected_launcher_id":"plug_n_play","games":[{"id":"x","name":"Game","exe_path":"C:\\Game\\Game.exe","dir_path":"C:\\Game","loader_name":"Game.exe","launcher_id":"plug_n_play","batch_name":"HV-PlugNPlay.bat","batch_found":true,"kvc_found":true}]}"#;
        let library: Library = serde_json::from_str(json).unwrap();
        assert_eq!(library.games.len(), 1);
        assert!(library.games[0].shortcut_path.is_empty());
    }

    #[test]
    fn shortcut_names_are_safe() {
        assert_eq!(safe_shortcut_name("A:B?*"), "AB");
        assert_eq!(safe_shortcut_name("   "), "Игра");
    }

    #[test]
    fn old_launcher_uses_original_game_exe_for_appearance() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path().join("FAR Changing Tides");
        fs::create_dir(&dir).unwrap();
        for name in [
            "hypervisor-launcher.exe",
            "FarChangingTides.exe",
            "UnityCrashHandler64.exe",
            "Crocodile-HV.exe",
        ] {
            fs::write(dir.join(name), b"MZ").unwrap();
        }
        assert_eq!(
            display_exe_for(&dir.join("hypervisor-launcher.exe")),
            dir.join("FarChangingTides.exe")
        );
    }

    #[test]
    fn canonical_windows_paths_are_normalized_for_shell() {
        let temporary = tempfile::tempdir().unwrap();
        let exe = temporary.path().join("hypervisor-launcher.exe");
        fs::write(&exe, b"MZ").unwrap();
        let canonical = exe.canonicalize().unwrap();
        assert!(canonical.to_string_lossy().starts_with(r"\\?\"));
        let normalized = validate_exe_path(&exe.to_string_lossy()).unwrap();
        assert_eq!(normalized, exe);
        assert_eq!(shell_path(&canonical), exe);
        let target = wide(&canonical);
        let argument = wide_quoted_path(&canonical);
        assert_eq!(
            String::from_utf16_lossy(&target[..target.len() - 1]),
            exe.to_string_lossy()
        );
        assert_eq!(
            String::from_utf16_lossy(&argument[..argument.len() - 1]),
            format!("\"{}\"", exe.display())
        );
    }

    #[test]
    fn native_shortcut_targets_helper_and_original_game() {
        let temporary = tempfile::tempdir().unwrap();
        let exe = temporary.path().join("Тестовая игра.exe");
        let shortcut = temporary.path().join("Тестовая игра.lnk");
        fs::write(&exe, b"MZ").unwrap();
        ensure_helper(temporary.path()).unwrap();
        assert!(helper_matches(&temporary.path().join(HELPER_NAME)));
        let canonical = exe.canonicalize().unwrap();
        let dir = canonical.parent().unwrap().to_path_buf();
        std::thread::spawn(move || {
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
                .ok()
                .unwrap();
            write_shortcut_initialized(&canonical, &canonical, &dir, "Тестовая игра", &shortcut)
                .unwrap();
            assert!(shortcut.is_file());

            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
            let persist: IPersistFile = link.cast().unwrap();
            let path = wide(&shortcut);
            unsafe { persist.Load(PCWSTR(path.as_ptr()), STGM_READ) }.unwrap();
            let mut target = vec![0u16; 32768];
            let mut arguments = vec![0u16; 32768];
            let mut working_dir = vec![0u16; 32768];
            let mut icon = vec![0u16; 32768];
            let mut description = vec![0u16; 32768];
            let mut icon_index = 0;
            unsafe {
                link.GetPath(&mut target, std::ptr::null_mut(), 0).unwrap();
                link.GetArguments(&mut arguments).unwrap();
                link.GetWorkingDirectory(&mut working_dir).unwrap();
                link.GetIconLocation(&mut icon, &mut icon_index).unwrap();
                link.GetDescription(&mut description).unwrap();
            }
            let decode = |value: &[u16]| {
                String::from_utf16_lossy(&value[..value.iter().position(|&ch| ch == 0).unwrap()])
            };
            assert_eq!(
                PathBuf::from(decode(&target)).canonicalize().unwrap(),
                dir.join(HELPER_NAME).canonicalize().unwrap()
            );
            assert_eq!(
                decode(&arguments),
                format!("\"{}\"", shell_path(&canonical).display())
            );
            assert_eq!(PathBuf::from(decode(&working_dir)), shell_path(&dir));
            assert_eq!(PathBuf::from(decode(&icon)), shell_path(&canonical));
            assert_eq!(icon_index, 0);
            assert_eq!(
                decode(&description),
                "Crocodile-HV: запуск игры Тестовая игра"
            );
            unsafe { CoUninitialize() };
        })
        .join()
        .unwrap();
    }

    #[test]
    fn shortcut_accepts_external_game_path_when_requested() {
        let Ok(exe) = std::env::var("CGS_HV_SHORTCUT_REPRO_EXE") else {
            return;
        };
        let exe = PathBuf::from(exe).canonicalize().unwrap();
        let dir = exe.parent().unwrap().to_path_buf();
        let temporary = tempfile::tempdir().unwrap();
        let shortcut = temporary.path().join("game.lnk");
        std::thread::spawn(move || {
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
                .ok()
                .unwrap();
            write_shortcut_initialized(&exe, &exe, &dir, "Game", &shortcut).unwrap();
            assert!(shortcut.is_file());
            unsafe { CoUninitialize() };
        })
        .join()
        .unwrap();
    }
}
