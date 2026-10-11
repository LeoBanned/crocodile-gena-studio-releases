use crate::{
    core_engine_path,
    models::{InstallRequest, OperationResult, ScanResult},
    steam,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};
use tauri::AppHandle;
use walkdir::{DirEntry, WalkDir};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const IGNORED_DIRS: &[&str] = &[
    "engine",
    "plugins",
    "thirdparty",
    "extras",
    "redist",
    "redistributable_bin",
    "sdk",
    "d3d12",
    "dml",
    "crashreportclient",
    "prerequisites",
];

const FIX_MARKERS: &[&str] = &[
    "onlinefix64.dll",
    "onlinefix.ini",
    "steamfix64.dll",
    "steamfix.ini",
    "winmm.txt",
    "epicfix64.dll",
    "epicfix.ini",
    "crocodilegena64.dll",
];

const FIX_FILES: &[&str] = &[
    "winmm.dll",
    "winmm.txt",
    "SteamFix64.dll",
    "SteamFix.ini",
    "EpicFix64.dll",
    "EpicFix.ini",
    "SteamOverlay64.dll",
    "CrocodileGena64.dll",
    "OnlineFix64.dll",
    "Custom.dll",
    "Custom.net",
    "CrocodileGena.ini",
    "CrocodileGena64.ini",
    "OnlineFix.ini",
    "dlllist.txt",
    "OnlineFix.url",
    "CrocodileGena.url",
    "steam_proxy.dll",
    "steam_appid.txt",
    "steam_interfaces.txt",
];

const BACKUP_DIR_NAME: &str = ".backup_orig";
const BACKUP_MANIFEST_NAME: &str = "cgs-backup.json";

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    version: u8,
    #[serde(default)]
    originals: BTreeMap<String, String>,
    #[serde(default)]
    created: BTreeMap<String, String>,
    #[serde(default)]
    legacy_import: bool,
}

impl Default for BackupManifest {
    fn default() -> Self {
        Self {
            version: 2,
            originals: BTreeMap::new(),
            created: BTreeMap::new(),
            legacy_import: false,
        }
    }
}

#[derive(Default)]
struct RestoreReport {
    restored: BTreeSet<String>,
    errors: Vec<String>,
}

fn is_hidden_backup(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(BACKUP_DIR_NAME)
}

pub(crate) fn walk(path: &Path) -> impl Iterator<Item = DirEntry> {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_hidden_backup(entry))
        .filter_map(Result::ok)
}

fn lower_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect()
}

fn ignored(path: &Path) -> bool {
    let components = lower_components(path);
    IGNORED_DIRS
        .iter()
        .any(|name| components.iter().any(|item| item == name))
}

pub(crate) fn ignored_distribution(path: &Path) -> bool {
    let components = lower_components(path);
    ["extras", "redist", "d3d12", "dml"]
        .iter()
        .any(|name| components.iter().any(|item| item == name))
}

fn valid_exe_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".exe")
        && ![
            "unins", "crash", "setup", "launcher", "server", "vcredist", "dxsetup", "boost",
            "helper", "process", "handler", "prereq",
        ]
        .iter()
        .any(|skip| lower.contains(skip))
}

pub(crate) fn display_relative(path: &Path, game_dir: &Path) -> String {
    path.strip_prefix(game_dir)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|| ".".into())
}

pub(crate) fn make_writable(path: &Path) {
    if let Ok(metadata) = fs::metadata(path) {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

fn backup_manifest_path(backup_dir: &Path) -> PathBuf {
    backup_dir.join(BACKUP_MANIFEST_NAME)
}

fn load_backup_manifest(backup_dir: &Path) -> BackupManifest {
    fs::read(backup_manifest_path(backup_dir))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn save_backup_manifest(backup_dir: &Path, manifest: &BackupManifest) -> Result<(), String> {
    fs::create_dir_all(backup_dir)
        .map_err(|error| format!("Не удалось создать папку бэкапа: {error}"))?;
    let target = backup_manifest_path(backup_dir);
    let temporary = backup_dir.join("cgs-backup.tmp");
    let data = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("Не удалось сформировать манифест бэкапа: {error}"))?;
    fs::write(&temporary, data)
        .map_err(|error| format!("Не удалось записать манифест бэкапа: {error}"))?;
    if target.exists() {
        make_writable(&target);
        fs::remove_file(&target)
            .map_err(|error| format!("Не удалось обновить манифест бэкапа: {error}"))?;
    }
    fs::rename(&temporary, &target)
        .map_err(|error| format!("Не удалось зафиксировать манифест бэкапа: {error}"))
}

pub(crate) fn files_equal(first: &Path, second: &Path) -> bool {
    let Ok(first_meta) = fs::metadata(first) else {
        return false;
    };
    let Ok(second_meta) = fs::metadata(second) else {
        return false;
    };
    if first_meta.len() != second_meta.len() {
        return false;
    }
    const BUFFER: usize = 64 * 1024;
    use std::io::{BufReader, Read};
    let Ok(first_file) = fs::File::open(first) else {
        return false;
    };
    let Ok(second_file) = fs::File::open(second) else {
        return false;
    };
    let mut first_reader = BufReader::new(first_file);
    let mut second_reader = BufReader::new(second_file);
    let mut first_buffer = [0_u8; BUFFER];
    let mut second_buffer = [0_u8; BUFFER];
    loop {
        let Ok(first_read) = first_reader.read(&mut first_buffer) else {
            return false;
        };
        let Ok(second_read) = second_reader.read(&mut second_buffer) else {
            return false;
        };
        if first_read != second_read || first_buffer[..first_read] != second_buffer[..second_read] {
            return false;
        }
        if first_read == 0 {
            return true;
        }
    }
}

fn backup_file(path: &Path, logs: &mut Vec<String>) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let Some(name) = path.file_name() else {
        return Ok(());
    };
    let backup_dir = parent.join(BACKUP_DIR_NAME);
    let backup = backup_dir.join(name);
    let manifest_existed = backup_manifest_path(&backup_dir).is_file();
    let backup_dir_existed = backup_dir.is_dir();
    let mut manifest = load_backup_manifest(&backup_dir);
    if backup_dir_existed && !manifest_existed {
        manifest.legacy_import = true;
    }
    let key = name.to_string_lossy().to_lowercase();
    if manifest.originals.contains_key(&key) || manifest.created.contains_key(&key) {
        return Ok(());
    }

    if backup.is_file() {
        manifest
            .originals
            .insert(key, name.to_string_lossy().into_owned());
        return save_backup_manifest(&backup_dir, &manifest);
    }

    if path.is_file() {
        fs::create_dir_all(&backup_dir)
            .map_err(|error| format!("Не удалось создать бэкап: {error}"))?;
        fs::copy(path, &backup).map_err(|error| {
            let message = format!("Ошибка бэкапа {}: {error}", path.display());
            logs.push(format!("⚠ {message}"));
            message
        })?;
        if !files_equal(path, &backup) {
            let message = format!("Проверка бэкапа не пройдена: {}", path.display());
            logs.push(format!("⚠ {message}"));
            return Err(message);
        }
        if let Ok(metadata) = fs::metadata(path) {
            let _ = fs::set_permissions(&backup, metadata.permissions());
        }
        manifest
            .originals
            .insert(key, name.to_string_lossy().into_owned());
    } else {
        manifest
            .created
            .insert(key, name.to_string_lossy().into_owned());
    }
    save_backup_manifest(&backup_dir, &manifest)
}

fn safe_copy(source: &Path, target: &Path, logs: &mut Vec<String>) -> Result<(), String> {
    if !source.is_file() {
        let message = format!("Компонент не найден: {}", source.display());
        logs.push(format!("⚠ {message}"));
        return Err(message);
    }
    backup_file(target, logs)?;
    if target.exists() {
        make_writable(target);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::copy(source, target).map_err(|error| {
        let message = format!("Не удалось скопировать {}: {error}", target.display());
        logs.push(format!("⚠ {message}"));
        message
    })?;
    if !files_equal(source, target) {
        let message = format!(
            "Проверка скопированного файла не пройдена: {}",
            target.display()
        );
        logs.push(format!("⚠ {message}"));
        return Err(message);
    }
    Ok(())
}

fn safe_remove(path: &Path, logs: &mut Vec<String>) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    backup_file(path, logs)?;
    make_writable(path);
    fs::remove_file(path).map_err(|error| {
        let message = format!("Не удалось удалить {}: {error}", path.display());
        logs.push(format!("⚠ {message}"));
        message
    })
}

pub(crate) fn write_with_backup(
    path: &Path,
    content: &str,
    logs: &mut Vec<String>,
) -> Result<(), String> {
    backup_file(path, logs)?;
    fs::write(path, content).map_err(|error| {
        let message = format!("Не удалось записать {}: {error}", path.display());
        logs.push(format!("⚠ {message}"));
        message
    })
}

fn kill_game_processes(game_dir: &Path, logs: &mut Vec<String>) {
    let mut names = BTreeSet::from([
        "crashreportclient.exe".to_string(),
        "werfault.exe".to_string(),
    ]);
    for entry in walk(game_dir) {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".exe") {
                names.insert(name);
            }
        }
    }
    for name in names {
        let mut command = Command::new("taskkill.exe");
        command.args(["/F", "/IM", &name]);
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        if command
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            logs.push(format!("Завершён процесс, блокировавший файлы: {name}"));
        }
    }
}

fn loader_directories(game_dir: &Path) -> BTreeSet<PathBuf> {
    let mut result = BTreeSet::new();
    for entry in walk(game_dir) {
        if entry.file_type().is_file() && valid_exe_name(&entry.file_name().to_string_lossy()) {
            if let Some(parent) = entry.path().parent() {
                if !ignored(parent) {
                    result.insert(parent.to_path_buf());
                }
            }
        }
    }
    if result.iter().any(|path| {
        path != game_dir && {
            let lower = path.to_string_lossy().to_lowercase();
            lower.contains("binaries") || lower.contains("\\bin") || lower.contains("/bin")
        }
    }) {
        result.remove(game_dir);
    }
    result
}

fn detect_game_engine(game_dir: &Path) -> String {
    let mut unity = 0u16;
    let mut unreal = 0u16;
    let mut godot = 0u16;
    let mut cryengine = 0u16;
    let mut source = 0u16;
    let mut gamemaker = 0u16;
    let mut renpy = 0u16;
    let mut rpg_maker = 0u16;

    for entry in walk(game_dir) {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let path = entry
            .path()
            .to_string_lossy()
            .replace('\\', "/")
            .to_lowercase();

        if entry.file_type().is_dir() {
            if name.ends_with("_data") {
                unity = unity.max(35);
            }
            if name == "monobleedingedge" {
                unity = unity.max(55);
            }
            if path.contains("/engine/binaries") {
                unreal = unreal.max(85);
            }
            if path.ends_with("/content/paks") {
                unreal = unreal.max(65);
            }
            if name == ".godot" {
                godot = godot.max(80);
            }
            if name == "renpy" {
                renpy = renpy.max(85);
            }
            if name == "www" {
                rpg_maker = rpg_maker.max(35);
            }
            continue;
        }

        match name.as_str() {
            "unityplayer.dll" => unity = 120,
            "globalgamemanagers" => unity = unity.max(100),
            "gameassembly.dll" => unity = unity.max(70),
            "unitycrashhandler64.exe" | "unitycrashhandler32.exe" => unity = unity.max(55),
            "ue4commandline.txt" | "ue5commandline.txt" => unreal = unreal.max(90),
            "crysystem.dll" => cryengine = 120,
            "gameinfo.txt" => source = source.max(75),
            "engine.dll" | "tier0.dll" | "vstdlib.dll" => source = source.max(65),
            "data.win" => gamemaker = 120,
            "nw.dll" => rpg_maker = rpg_maker.max(75),
            "archive.rpa" => renpy = renpy.max(70),
            _ => {}
        }

        if (name.ends_with("-win64-shipping.exe")
            || name.ends_with("-win32-shipping.exe")
            || name.ends_with("-wingdk-shipping.exe"))
            && path.contains("/binaries/")
        {
            unreal = 120;
        }
        if name.ends_with(".pak") && path.contains("/content/paks/") {
            unreal = unreal.max(100);
        }
        if path.contains("/unrealengine/") || path.contains("/engine/binaries/") {
            unreal = unreal.max(90);
        }
        if path.contains("/managed/unityengine") {
            unity = unity.max(95);
        }
        if name.ends_with(".pck") {
            godot = godot.max(65);
        }
        if name.starts_with("godot") && (name.ends_with(".exe") || name.ends_with(".dll")) {
            godot = godot.max(100);
        }
        if path.contains("/cryengine/") || name.starts_with("cry") && name.ends_with(".dll") {
            cryengine = cryengine.max(85);
        }
        if name.starts_with("python") && name.ends_with(".dll") && path.contains("/lib/") {
            renpy = renpy.max(55);
        }
        if name == "package.json" && path.contains("/www/") {
            rpg_maker = rpg_maker.max(65);
        }
    }

    let scores = [
        ("Unity", unity),
        ("Unreal Engine", unreal),
        ("Godot", godot),
        ("CryEngine", cryengine),
        ("Source", source),
        ("GameMaker", gamemaker),
        ("Ren'Py", renpy),
        ("RPG Maker", rpg_maker),
    ];
    let mut detected = ("Другой / Собственный", 0u16);
    for candidate in scores {
        if candidate.1 > detected.1 {
            detected = candidate;
        }
    }
    if detected.1 >= 35 {
        detected.0.to_string()
    } else {
        "Другой / Собственный".to_string()
    }
}

#[tauri::command]
pub async fn scan_game_directory(path: String) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || scan_game_directory_blocking(path))
        .await
        .map_err(|error| format!("Сканирование прервано: {error}"))?
}

fn scan_game_directory_blocking(path: String) -> Result<ScanResult, String> {
    let game_dir = PathBuf::from(&path);
    if !game_dir.is_dir() {
        return Err("Указанная папка не существует".into());
    }
    let mut exes = Vec::new();
    let mut steam_api_paths = Vec::new();
    let mut installed = false;
    let has_backup = WalkDir::new(&game_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .any(|entry| {
            entry.file_type().is_dir()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(BACKUP_DIR_NAME)
        });
    let mut has_eos = false;
    let mut has_eos_backup = false;
    let mut has_epicfix = false;

    for entry in walk(&game_dir) {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let lower = name.to_lowercase();
        let parent = entry.path().parent().unwrap_or(&game_dir);
        if valid_exe_name(&name) && !ignored(parent) {
            exes.push(display_relative(entry.path(), &game_dir));
        }
        if [
            "steam_api64.dll",
            "steam_api.dll",
            "steam_api64_o.dll",
            "steam_api_o.dll",
        ]
        .contains(&lower.as_str())
            && !ignored_distribution(parent)
        {
            steam_api_paths.push(display_relative(entry.path(), &game_dir));
        }
        if FIX_MARKERS.contains(&lower.as_str()) {
            installed = true;
        }
        if lower.starts_with("eossdk")
            && lower.ends_with(".dll")
            && !lower.contains("_o.dll")
            && !lower.ends_with(".of")
        {
            has_eos = true;
        }
        if lower.starts_with("eossdk") && lower.ends_with("_o.dll") {
            has_eos_backup = true;
        }
        if lower == "epicfix64.dll" {
            has_epicfix = true;
        }
    }
    exes.sort();
    steam_api_paths.sort();
    let engine = detect_game_engine(&game_dir);
    let detected_game = steam::auto_detect_from_path(&game_dir);
    Ok(ScanResult {
        game_dir: game_dir.display().to_string(),
        exes,
        steam_api_paths,
        detected_game,
        status: if installed {
            "installed"
        } else if has_backup {
            "backup_exists"
        } else {
            "clean"
        }
        .into(),
        engine,
        has_eos,
        has_eos_backup,
        has_epicfix,
    })
}

fn steamfix_config(request: &InstallRequest, dlcs: &BTreeMap<String, String>) -> String {
    let mut config = format!(
        "[Main]\nRealAppId={}\nFakeAppId={}\n#Language=english\nBuildId=0\n\n[Misc]\nOverlay=true\nShowLANServers=false\nFilterLobby=false\nUnlockAllDLC={}\n\n[Interfaces]\nApps=true\nFriends=true\nUser=true\nInventory=true\nStats=true\nStorage=true\nUtils=true\nWorkshop=false\n",
        request.real_appid,
        request.fake_appid,
        if request.unlock_all_dlcs { "true" } else { "false" }
    );
    if request.unlock_all_dlcs && !dlcs.is_empty() {
        config.push_str("\n[DLC]\n");
        for (id, name) in dlcs {
            config.push_str(&format!("{id}={name}\n"));
        }
    }
    config
}

#[tauri::command]
pub async fn install_fix(app: AppHandle, request: InstallRequest) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || install_fix_blocking(app, request)).await {
        Ok(result) => result,
        Err(error) => OperationResult::error(format!("Установка прервана: {error}"), vec![]),
    }
}

pub(crate) fn install_fix_blocking(app: AppHandle, request: InstallRequest) -> OperationResult {
    let game_dir = PathBuf::from(&request.game_dir);
    if !game_dir.is_dir() {
        return OperationResult::error("Папка игры не найдена", vec![]);
    }
    let mut logs = Vec::new();
    kill_game_processes(&game_dir, &mut logs);
    let core = core_engine_path(&app);
    let loaders = loader_directories(&game_dir);
    if loaders.is_empty() {
        return OperationResult::error("Не найден подходящий исполняемый файл игры", logs);
    }

    let dlcs = if request.dlcs.is_empty() {
        steam::get_app_details_inner(request.real_appid)
            .ok()
            .flatten()
            .map(|details| details.dlcs)
            .unwrap_or_default()
    } else {
        request.dlcs.clone()
    };

    let mut eos_targets = Vec::new();
    for entry in walk(&game_dir) {
        if !entry.file_type().is_file() || ignored_distribution(entry.path()) {
            continue;
        }
        let lower = entry.file_name().to_string_lossy().to_lowercase();
        if lower.starts_with("eossdk")
            && lower.ends_with(".dll")
            && !lower.contains("_o.dll")
            && !lower.ends_with(".of")
        {
            eos_targets.push(entry.path().to_path_buf());
        }
    }

    let core_eos = core.join("EOSSDK-Win64-Shipping.dll");
    for target in eos_targets {
        let backup_name = format!(
            "{}_o.dll",
            target.file_stem().unwrap_or_default().to_string_lossy()
        );
        let backup = target.with_file_name(backup_name);
        if !backup.exists()
            || fs::metadata(&backup)
                .map(|item| item.len() < 100_000)
                .unwrap_or(true)
        {
            if safe_copy(&target, &backup, &mut logs).is_ok() {
                logs.push(format!(
                    "Сохранён оригинальный EOS: {}",
                    display_relative(&backup, &game_dir)
                ));
            }
        }
        if safe_copy(&core_eos, &target, &mut logs).is_ok() {
            logs.push(format!(
                "Установлен EOS-модуль: {}",
                display_relative(&target, &game_dir)
            ));
        }
    }

    let config = steamfix_config(&request, &dlcs);
    let mut installed_count = 0usize;
    for directory in loaders {
        for obsolete in [
            "OnlineFix64.dll",
            "SteamOverlay64.dll",
            "Custom.dll",
            "dlllist.txt",
            "OnlineFix.url",
            "OnlineFix.ini",
            "CrocodileGena.ini",
            "CrocodileGena64.ini",
        ] {
            let path = directory.join(obsolete);
            if path.exists() {
                let _ = safe_remove(&path, &mut logs);
            }
        }
        let mut success = true;
        success &= write_with_backup(
            &directory.join("steam_appid.txt"),
            &request.fake_appid.to_string(),
            &mut logs,
        )
        .is_ok();
        success &= safe_copy(
            &core.join("SteamFix64.dll"),
            &directory.join("SteamFix64.dll"),
            &mut logs,
        )
        .is_ok();
        success &= safe_copy(
            &core.join("winmm_unity.dll"),
            &directory.join("winmm.dll"),
            &mut logs,
        )
        .is_ok();
        success &= write_with_backup(&directory.join("SteamFix.ini"), &config, &mut logs).is_ok();
        let mut modules = vec!["SteamFix64.dll"];
        if request.install_epicfix {
            success &= safe_copy(
                &core.join("EpicFix64.dll"),
                &directory.join("EpicFix64.dll"),
                &mut logs,
            )
            .is_ok();
            success &= safe_copy(
                &core.join("EpicFix.ini"),
                &directory.join("EpicFix.ini"),
                &mut logs,
            )
            .is_ok();
            modules.push("EpicFix64.dll");
        }
        success &= write_with_backup(
            &directory.join("winmm.txt"),
            &(modules.join("\n") + "\n"),
            &mut logs,
        )
        .is_ok();
        if success {
            installed_count += 1;
            logs.push(format!(
                "Готово: загрузчик и конфигурация установлены в {}",
                display_relative(&directory, &game_dir)
            ));
        }
    }
    if installed_count == 0 {
        OperationResult::error(
            "Не удалось установить компоненты ни в одну директорию",
            logs,
        )
    } else {
        OperationResult::ok(
            format!(
                "Онлайн-фикс установлен в {installed_count} директори{}",
                if installed_count == 1 { "ю" } else { "и" }
            ),
            logs,
        )
    }
}

fn looks_like_generated_backup(backup: &Path, name: &str, core: &Path) -> bool {
    match name.to_lowercase().as_str() {
        "steamfix64.dll" => files_equal(backup, &core.join("SteamFix64.dll")),
        "epicfix64.dll" => files_equal(backup, &core.join("EpicFix64.dll")),
        "epicfix.ini" => files_equal(backup, &core.join("EpicFix.ini")),
        "winmm.dll" => files_equal(backup, &core.join("winmm_unity.dll")),
        "steamfix.ini" => fs::read_to_string(backup)
            .map(|content| {
                content.contains("[Main]")
                    && content.contains("RealAppId=")
                    && content.contains("FakeAppId=")
                    && content.contains("[Interfaces]")
            })
            .unwrap_or(false),
        "winmm.txt" => fs::read_to_string(backup)
            .map(|content| {
                let modules = content
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>();
                !modules.is_empty()
                    && modules.iter().all(|module| {
                        module.eq_ignore_ascii_case("SteamFix64.dll")
                            || module.eq_ignore_ascii_case("EpicFix64.dll")
                    })
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn restore_one_file(backup: &Path, target: &Path) -> Result<(), String> {
    let Some(parent) = target.parent() else {
        return Err(format!(
            "Некорректный путь восстановления: {}",
            target.display()
        ));
    };
    let temporary = parent.join(format!(
        ".cgs-restore-{}",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    if temporary.exists() {
        make_writable(&temporary);
        let _ = fs::remove_file(&temporary);
    }
    fs::copy(backup, &temporary)
        .map_err(|error| format!("Не удалось подготовить {}: {error}", target.display()))?;
    if !files_equal(backup, &temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Проверка копии не пройдена: {}", target.display()));
    }
    if target.exists() {
        make_writable(target);
        fs::remove_file(target)
            .map_err(|error| format!("Не удалось заменить {}: {error}", target.display()))?;
    }
    fs::rename(&temporary, target)
        .map_err(|error| format!("Не удалось вернуть {} на место: {error}", target.display()))?;
    if !files_equal(backup, target) {
        return Err(format!(
            "Файл после восстановления повреждён: {}",
            target.display()
        ));
    }
    if let Ok(metadata) = fs::metadata(backup) {
        let _ = fs::set_permissions(target, metadata.permissions());
    }
    Ok(())
}

fn restore_backups(directory: &Path, core: &Path, logs: &mut Vec<String>) -> RestoreReport {
    let mut report = RestoreReport::default();
    let backup_dir = directory.join(BACKUP_DIR_NAME);
    if !backup_dir.is_dir() {
        return report;
    }

    let manifest_path = backup_manifest_path(&backup_dir);
    let manifest_exists = manifest_path.is_file();
    let mut manifest = load_backup_manifest(&backup_dir);

    if let Ok(items) = fs::read_dir(&backup_dir) {
        for item in items.flatten() {
            if !item.path().is_file() {
                continue;
            }
            let name = item.file_name().to_string_lossy().into_owned();
            if name.eq_ignore_ascii_case(BACKUP_MANIFEST_NAME)
                || name.eq_ignore_ascii_case("cgs-backup.tmp")
            {
                continue;
            }
            let key = name.to_lowercase();
            if !manifest.originals.contains_key(&key) && !manifest.created.contains_key(&key) {
                manifest.originals.insert(key, name);
                manifest.legacy_import = true;
            }
        }
    }

    let legacy_mode = !manifest_exists || manifest.legacy_import || manifest.version < 2;
    let originals = manifest.originals.clone();
    let mut created = manifest.created.clone();

    for (key, name) in originals {
        let backup = backup_dir.join(&name);
        if legacy_mode && backup.is_file() && looks_like_generated_backup(&backup, &name, core) {
            created.insert(key, name);
            continue;
        }
        if !backup.is_file() {
            report
                .errors
                .push(format!("В бэкапе отсутствует оригинал: {name}"));
            continue;
        }
        let target = directory.join(&name);
        match restore_one_file(&backup, &target) {
            Ok(()) => {
                report.restored.insert(key);
                logs.push(format!(
                    "Восстановлен и проверен оригинал: {}",
                    target.display()
                ));
            }
            Err(error) => report.errors.push(error),
        }
    }

    if report.errors.is_empty() {
        for (key, name) in created {
            let target = directory.join(&name);
            if target.exists() {
                make_writable(&target);
                if let Err(error) = fs::remove_file(&target) {
                    report.errors.push(format!(
                        "Не удалось удалить созданный фиксом файл {}: {error}",
                        target.display()
                    ));
                    continue;
                }
                logs.push(format!(
                    "Удалён созданный фиксом файл: {}",
                    target.display()
                ));
            }
            report.restored.insert(key);
        }
    }

    if report.errors.is_empty() {
        if let Err(error) = fs::remove_dir_all(&backup_dir) {
            report.errors.push(format!(
                "Файлы восстановлены, но папку бэкапа удалить не удалось: {error}"
            ));
        } else {
            logs.push(format!(
                "Бэкап полностью применён и проверен: {}",
                backup_dir.display()
            ));
        }
    } else {
        logs.push(format!(
            "⚠ Бэкап сохранён из-за ошибок восстановления: {}",
            backup_dir.display()
        ));
    }

    report
}

pub(crate) fn find_case_insensitive(directory: &Path, target: &str) -> Option<PathBuf> {
    fs::read_dir(directory).ok()?.flatten().find_map(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(target)
            .then_some(entry.path())
    })
}

fn restore_sidecar(original: &Path, target: &Path, logs: &mut Vec<String>) -> Result<(), String> {
    restore_one_file(original, target)?;
    make_writable(original);
    fs::remove_file(original).map_err(|error| {
        format!("Оригинал восстановлен, но резервную копию удалить не удалось: {error}")
    })?;
    logs.push(format!(
        "Восстановлен и проверен оригинал: {}",
        target.display()
    ));
    Ok(())
}

#[tauri::command]
pub async fn uninstall_fix(app: AppHandle, game_dir: String) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || uninstall_fix_blocking(app, game_dir)).await
    {
        Ok(result) => result,
        Err(error) => OperationResult::error(format!("Откат прерван: {error}"), vec![]),
    }
}

fn uninstall_fix_blocking(app: AppHandle, game_dir: String) -> OperationResult {
    let game_dir = PathBuf::from(game_dir);
    if !game_dir.is_dir() {
        return OperationResult::error("Папка игры не найдена", vec![]);
    }
    let mut logs = Vec::new();
    let mut errors = Vec::new();
    let core = core_engine_path(&app);
    kill_game_processes(&game_dir, &mut logs);
    let mut targets = BTreeSet::from([game_dir.clone()]);
    for entry in WalkDir::new(&game_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(BACKUP_DIR_NAME)
        {
            if let Some(parent) = entry.path().parent() {
                targets.insert(parent.to_path_buf());
            }
        }
    }
    for entry in walk(&game_dir) {
        if !entry.file_type().is_file() {
            continue;
        }
        let lower = entry.file_name().to_string_lossy().to_lowercase();
        if valid_exe_name(&lower)
            || FIX_MARKERS.contains(&lower.as_str())
            || lower.ends_with("_o.dll")
        {
            if let Some(parent) = entry.path().parent() {
                targets.insert(parent.to_path_buf());
            }
        }
    }
    logs.push(format!(
        "Найдено директорий для восстановления: {}",
        targets.len()
    ));
    for directory in targets {
        if !directory.is_dir() {
            continue;
        }
        let restore_report = restore_backups(&directory, &core, &mut logs);
        let restore_failed = !restore_report.errors.is_empty();
        let restored_files = restore_report.restored;
        errors.extend(restore_report.errors);
        if restore_failed {
            continue;
        }

        for original_name in ["steam_api64_o.dll", "steam_api_o.dll"] {
            if let Some(original) = find_case_insensitive(&directory, original_name) {
                let target_name = original_name.replace("_o.dll", ".dll");
                let target = directory.join(target_name);
                if fs::metadata(&original)
                    .map(|item| item.len() > 50_000)
                    .unwrap_or(false)
                {
                    if let Err(error) = restore_sidecar(&original, &target, &mut logs) {
                        errors.push(error);
                    }
                }
            }
        }

        if let Ok(items) = fs::read_dir(&directory) {
            for item in items.flatten() {
                let lower = item.file_name().to_string_lossy().to_lowercase();
                if lower.starts_with("eossdk") && lower.ends_with("_o.dll") {
                    let file_name = item.file_name();
                    let base = file_name.to_string_lossy();
                    let target =
                        directory.join(format!("{}.dll", &base[..base.len().saturating_sub(6)]));
                    if let Err(error) = restore_sidecar(&item.path(), &target, &mut logs) {
                        errors.push(error);
                    }
                }
            }
        }
        for name in FIX_FILES {
            if restored_files.contains(&name.to_lowercase()) {
                continue;
            }
            if let Some(path) = find_case_insensitive(&directory, name) {
                make_writable(&path);
                if let Err(error) = fs::remove_file(&path) {
                    errors.push(format!("Не удалось удалить {}: {error}", path.display()));
                }
            }
        }
    }
    if errors.is_empty() {
        OperationResult::ok(
            "Фикс удалён, оригинальные файлы восстановлены и проверены",
            logs,
        )
    } else {
        logs.extend(errors.iter().map(|error| format!("⚠ {error}")));
        OperationResult::error(
            format!(
                "Откат выполнен не полностью. Ошибок: {}. Бэкапы с проблемными файлами сохранены.",
                errors.len()
            ),
            logs,
        )
    }
}

#[tauri::command]
pub async fn install_epicfix_only(app: AppHandle, game_dir: String) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || install_epicfix_only_blocking(app, game_dir))
        .await
    {
        Ok(result) => result,
        Err(error) => OperationResult::error(format!("Установка прервана: {error}"), vec![]),
    }
}

fn install_epicfix_only_blocking(app: AppHandle, game_dir: String) -> OperationResult {
    let game_dir = PathBuf::from(game_dir);
    if !game_dir.is_dir() {
        return OperationResult::error("Папка игры не найдена", vec![]);
    }
    let mut logs = Vec::new();
    let mut targets = loader_directories(&game_dir);
    if targets.is_empty() {
        targets.insert(game_dir.clone());
    }
    let core = core_engine_path(&app);
    let mut count = 0;
    for directory in targets {
        let mut success = true;
        for name in ["EpicFix64.dll", "EpicFix.ini", "SteamFix64.dll"] {
            success &= safe_copy(&core.join(name), &directory.join(name), &mut logs).is_ok();
        }
        success &= safe_copy(
            &core.join("winmm_unity.dll"),
            &directory.join("winmm.dll"),
            &mut logs,
        )
        .is_ok();
        let list_path = directory.join("winmm.txt");
        let existing = fs::read_to_string(&list_path).unwrap_or_default();
        let mut modules = existing
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !modules
            .iter()
            .any(|item| item.eq_ignore_ascii_case("SteamFix64.dll"))
        {
            modules.insert(0, "SteamFix64.dll".into());
        }
        if !modules
            .iter()
            .any(|item| item.eq_ignore_ascii_case("EpicFix64.dll"))
        {
            modules.push("EpicFix64.dll".into());
        }
        success &= write_with_backup(&list_path, &(modules.join("\n") + "\n"), &mut logs).is_ok();
        if success {
            count += 1;
            logs.push(format!(
                "EpicFix установлен в {}",
                display_relative(&directory, &game_dir)
            ));
        }
    }
    if count > 0 {
        OperationResult::ok("Компоненты EpicFix успешно установлены", logs)
    } else {
        OperationResult::error("Не удалось установить EpicFix", logs)
    }
}

#[tauri::command]
pub async fn restore_eos_only(game_dir: String) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || restore_eos_only_blocking(game_dir)).await {
        Ok(result) => result,
        Err(error) => OperationResult::error(format!("Восстановление прервано: {error}"), vec![]),
    }
}

fn restore_eos_only_blocking(game_dir: String) -> OperationResult {
    let game_dir = PathBuf::from(game_dir);
    if !game_dir.is_dir() {
        return OperationResult::error("Папка игры не найдена", vec![]);
    }
    let mut logs = Vec::new();
    let backups = walk(&game_dir)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            let lower = entry.file_name().to_string_lossy().to_lowercase();
            lower.starts_with("eossdk") && lower.ends_with("_o.dll")
        })
        .map(|entry| entry.path().to_path_buf())
        .collect::<Vec<_>>();
    let mut count = 0;
    let mut errors = Vec::new();
    for backup in backups {
        let name = backup.file_name().unwrap_or_default().to_string_lossy();
        let target =
            backup.with_file_name(format!("{}.dll", &name[..name.len().saturating_sub(6)]));
        match restore_sidecar(&backup, &target, &mut logs) {
            Ok(()) => {
                count += 1;
            }
            Err(error) => errors.push(error),
        }
    }
    if !errors.is_empty() {
        logs.extend(errors.iter().map(|error| format!("⚠ {error}")));
        OperationResult::error(
            format!("EOS восстановлен не полностью. Ошибок: {}", errors.len()),
            logs,
        )
    } else if count == 0 {
        OperationResult::ok(
            "Резервные копии EOS не найдены — возможно, оригинал уже восстановлен",
            logs,
        )
    } else {
        OperationResult::ok(format!("Восстановлено файлов EOS: {count}"), logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("cgs-{label}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn restores_originals_and_removes_only_created_files() {
        let directory = test_directory("restore");
        let core = directory.join("core");
        fs::create_dir_all(&core).unwrap();
        let original = directory.join("winmm.dll");
        let created = directory.join("SteamFix64.dll");
        fs::write(&original, b"original winmm content").unwrap();
        let mut logs = Vec::new();

        backup_file(&original, &mut logs).unwrap();
        fs::write(&original, b"installed replacement").unwrap();
        backup_file(&created, &mut logs).unwrap();
        fs::write(&created, b"created by application").unwrap();

        let report = restore_backups(&directory, &core, &mut logs);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(fs::read(&original).unwrap(), b"original winmm content");
        assert!(!created.exists());
        assert!(!directory.join(BACKUP_DIR_NAME).exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn keeps_backup_directory_when_an_original_is_missing() {
        let directory = test_directory("failed-restore");
        let core = directory.join("core");
        fs::create_dir_all(&core).unwrap();
        let original = directory.join("original.dll");
        fs::write(&original, b"original").unwrap();
        let mut logs = Vec::new();
        backup_file(&original, &mut logs).unwrap();
        fs::remove_file(directory.join(BACKUP_DIR_NAME).join("original.dll")).unwrap();
        fs::write(&original, b"replacement").unwrap();

        let report = restore_backups(&directory, &core, &mut logs);
        assert!(!report.errors.is_empty());
        assert!(directory.join(BACKUP_DIR_NAME).exists());
        assert_eq!(fs::read(&original).unwrap(), b"replacement");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cleans_generated_files_from_legacy_backup() {
        let directory = test_directory("legacy");
        let core = directory.join("core");
        let backup_dir = directory.join(BACKUP_DIR_NAME);
        fs::create_dir_all(&core).unwrap();
        fs::create_dir_all(&backup_dir).unwrap();
        let generated_config = b"[Main]\nRealAppId=1\nFakeAppId=480\n[Interfaces]\nApps=true\n";
        fs::write(directory.join("SteamFix.ini"), generated_config).unwrap();
        fs::write(backup_dir.join("SteamFix.ini"), generated_config).unwrap();
        let mut logs = Vec::new();

        let report = restore_backups(&directory, &core, &mut logs);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(!directory.join("SteamFix.ini").exists());
        assert!(!backup_dir.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn detects_unreal_engine_from_shipping_layout() {
        let directory = test_directory("unreal-engine");
        let binaries = directory.join("Hercules").join("Binaries").join("Win64");
        let paks = directory.join("Hercules").join("Content").join("Paks");
        fs::create_dir_all(&binaries).unwrap();
        fs::create_dir_all(&paks).unwrap();
        fs::write(binaries.join("Hercules-Win64-Shipping.exe"), b"").unwrap();
        fs::write(paks.join("Hercules.pak"), b"").unwrap();

        assert_eq!(detect_game_engine(&directory), "Unreal Engine");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn detects_unity_from_runtime_markers() {
        let directory = test_directory("unity-engine");
        fs::create_dir_all(directory.join("Example_Data")).unwrap();
        fs::write(directory.join("UnityPlayer.dll"), b"").unwrap();
        fs::write(directory.join("GameAssembly.dll"), b"").unwrap();

        assert_eq!(detect_game_engine(&directory), "Unity");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ignores_engine_markers_inside_backup_directory() {
        let directory = test_directory("engine-backup");
        let backup = directory.join(BACKUP_DIR_NAME);
        let paks = directory.join("Game").join("Content").join("Paks");
        fs::create_dir_all(&backup).unwrap();
        fs::create_dir_all(&paks).unwrap();
        fs::write(backup.join("UnityPlayer.dll"), b"").unwrap();
        fs::write(paks.join("Game.pak"), b"").unwrap();

        assert_eq!(detect_game_engine(&directory), "Unreal Engine");
        fs::remove_dir_all(directory).unwrap();
    }
}
