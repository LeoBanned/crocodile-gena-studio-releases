use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::{DirEntry, WalkDir};

const SAVE_MANIFEST_NAME: &str = "cgs-save-manifest.json";
const MAX_RESTORE_FILES: usize = 250_000;
const MAX_RESTORE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveManifest {
    schema_version: u32,
    created_unix: u64,
    items: Vec<SaveManifestItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveManifestItem {
    folder: String,
    target: String,
}

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[derive(Clone)]
struct SaveCandidate {
    label: String,
    category: String,
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocation {
    pub id: String,
    pub label: String,
    pub category: String,
    pub path: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub modified_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveArchiveResult {
    pub success: bool,
    pub message: String,
    pub logs: Vec<String>,
    pub archive_path: Option<String>,
}

impl SaveArchiveResult {
    fn error(message: impl Into<String>, logs: Vec<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            logs,
            archive_path: None,
        }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn add_candidate(
    candidates: &mut BTreeMap<String, SaveCandidate>,
    path: PathBuf,
    label: impl Into<String>,
    category: impl Into<String>,
) {
    if !path.is_dir() {
        return;
    }
    let key = path.to_string_lossy().replace('/', "\\").to_lowercase();
    candidates.entry(key).or_insert_with(|| SaveCandidate {
        label: label.into(),
        category: category.into(),
        path,
    });
}

fn add_named_roots(candidates: &mut BTreeMap<String, SaveCandidate>, base: &Path, category: &str) {
    let names = [
        ("Goldberg SteamEmu Saves", "Goldberg SteamEmu"),
        ("GSE Saves", "Goldberg / GSE"),
        ("SmartSteamEmu", "SmartSteamEmu"),
        ("SteamEmu", "SteamEmu"),
        ("CODEX", "CODEX"),
        ("EMPRESS", "EMPRESS"),
        ("RUNE", "RUNE"),
        ("FLT", "FLT"),
        ("TENOKE", "TENOKE"),
        ("SKIDROW", "SKIDROW"),
        ("OnlineFix", "OnlineFix"),
        ("0xdeadc0de", "0xdeadc0de"),
    ];
    for (directory, label) in names {
        add_candidate(candidates, base.join(directory), label, category);
    }
}

fn ignored_save_directory_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_lowercase();
    matches!(
        name.as_str(),
        "cache"
            | "caches"
            | "cache_data"
            | "code cache"
            | "component_crx_cache"
            | "gpucache"
            | "dawncache"
            | "shadercache"
            | "webcache"
            | "platformprocess"
            | "crashpad"
            | "temp"
            | "tmp"
            | "logs"
            | "log"
            | "telemetry"
    ) || name.ends_with("_cache")
}

fn ignored_save_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    if entry.file_type().is_dir() {
        return ignored_save_directory_name(entry.file_name());
    }
    let name = entry.file_name().to_string_lossy().to_lowercase();
    name == "lockfile"
        || name.starts_with("singleton")
        || name.ends_with(".lock")
        || name.ends_with(".tmp")
        || name.ends_with(".dmp")
        || name.ends_with(".log")
}

fn add_local_low_games(candidates: &mut BTreeMap<String, SaveCandidate>, local_low: &Path) {
    let Ok(companies) = fs::read_dir(local_low) else {
        return;
    };
    for company in companies.flatten().filter(|item| item.path().is_dir()) {
        if ignored_save_directory_name(&company.file_name()) {
            continue;
        }
        let company_path = company.path();
        let company_name = company.file_name().to_string_lossy().into_owned();
        let has_direct_files = fs::read_dir(&company_path)
            .map(|items| items.flatten().any(|item| item.path().is_file()))
            .unwrap_or(false);
        if has_direct_files {
            add_candidate(
                candidates,
                company_path,
                format!("Unity · {company_name}"),
                "Unity / LocalLow",
            );
            continue;
        }

        let mut games_added = 0usize;
        if let Ok(games) = fs::read_dir(&company_path) {
            for game in games.flatten().filter(|item| item.path().is_dir()) {
                if ignored_save_directory_name(&game.file_name()) {
                    continue;
                }
                let game_name = game.file_name().to_string_lossy().into_owned();
                add_candidate(
                    candidates,
                    game.path(),
                    format!("Unity · {company_name} / {game_name}"),
                    "Unity / LocalLow",
                );
                games_added += 1;
            }
        }
        if games_added == 0 {
            add_candidate(
                candidates,
                company_path,
                format!("Unity · {company_name}"),
                "Unity / LocalLow",
            );
        }
    }
}

fn collect_candidates() -> BTreeMap<String, SaveCandidate> {
    let mut candidates = BTreeMap::new();

    if let Some(user) = env_path("USERPROFILE") {
        add_candidate(
            &mut candidates,
            user.join("Saved Games"),
            "Сохранённые игры Windows",
            "Стандартные папки",
        );
        add_candidate(
            &mut candidates,
            user.join("Documents").join("My Games"),
            "Documents / My Games",
            "Стандартные папки",
        );
        add_candidate(
            &mut candidates,
            user.join("Documents").join("SavedGames"),
            "Documents / SavedGames",
            "Стандартные папки",
        );
    }
    if let Some(one_drive) = env_path("OneDrive") {
        add_candidate(
            &mut candidates,
            one_drive.join("Documents").join("My Games"),
            "OneDrive / My Games",
            "Стандартные папки",
        );
    }

    if let Some(roaming) = env_path("APPDATA") {
        add_named_roots(&mut candidates, &roaming, "Эмуляторы и репаки");
        add_candidate(
            &mut candidates,
            roaming.join("Godot").join("app_userdata"),
            "Godot / app_userdata",
            "Игровые движки",
        );
    }

    if let Some(local) = env_path("LOCALAPPDATA") {
        add_named_roots(&mut candidates, &local, "Эмуляторы и репаки");
        if let Some(app_data) = local.parent() {
            add_local_low_games(&mut candidates, &app_data.join("LocalLow"));
        }
        if let Ok(items) = fs::read_dir(&local) {
            for item in items.flatten().filter(|item| item.path().is_dir()) {
                let root = item.path();
                for suffix in [
                    ["Saved", "SaveGames"],
                    ["Saved", "SaveGame"],
                    ["Saved", "Saves"],
                ] {
                    let path = root.join(suffix[0]).join(suffix[1]);
                    if path.is_dir() {
                        let game = item.file_name().to_string_lossy().into_owned();
                        add_candidate(
                            &mut candidates,
                            path,
                            format!("{game} / {}", suffix[1]),
                            "Unreal Engine и локальные игры",
                        );
                    }
                }
            }
        }
    }

    if let Some(public) = env_path("PUBLIC") {
        let documents = public.join("Documents");
        for (path, label) in [
            (documents.join("Steam").join("CODEX"), "Steam / CODEX"),
            (documents.join("Steam").join("RUNE"), "Steam / RUNE"),
            (documents.join("Steam").join("FLT"), "Steam / FLT"),
            (documents.join("Steam").join("TENOKE"), "Steam / TENOKE"),
            (documents.join("EMPRESS"), "EMPRESS"),
            (documents.join("OnlineFix"), "OnlineFix"),
        ] {
            add_candidate(&mut candidates, path, label, "Общие сохранения");
        }
    }

    if let Some(program_data) = env_path("PROGRAMDATA") {
        for (directory, label) in [
            ("RLD!", "Steam / RLD!"),
            ("Player", "Steam / Player"),
            ("CODEX", "Steam / CODEX"),
        ] {
            add_candidate(
                &mut candidates,
                program_data.join("Steam").join(directory),
                label,
                "Общие сохранения",
            );
        }
    }

    if let Some(program_files) = env_path("ProgramFiles(x86)") {
        add_candidate(
            &mut candidates,
            program_files.join("Steam").join("userdata"),
            "Steam / userdata",
            "Официальные клиенты",
        );
    }

    #[cfg(target_os = "linux")]
    if let Some(home) = env_path("HOME") {
        add_candidate(
            &mut candidates,
            home.join("Documents").join("My Games"),
            "Documents / My Games",
            "Linux",
        );
        add_candidate(
            &mut candidates,
            home.join(".local/share/godot/app_userdata"),
            "Godot / app_userdata",
            "Linux",
        );
        for steam in linux_steam_roots(&home) {
            add_candidate(
                &mut candidates,
                steam.join("userdata"),
                "Steam / userdata",
                "Steam Linux",
            );
            let compatdata = steam.join("steamapps/compatdata");
            let Ok(prefixes) = fs::read_dir(compatdata) else {
                continue;
            };
            for prefix in prefixes.flatten().filter(|entry| entry.path().is_dir()) {
                let appid = prefix.file_name().to_string_lossy().into_owned();
                let user = prefix.path().join("pfx/drive_c/users/steamuser");
                add_candidate(
                    &mut candidates,
                    user.join("Saved Games"),
                    format!("Proton {appid} / Saved Games"),
                    "Steam Proton",
                );
                add_candidate(
                    &mut candidates,
                    user.join("Documents/My Games"),
                    format!("Proton {appid} / My Games"),
                    "Steam Proton",
                );
                let roaming = user.join("AppData/Roaming");
                add_named_roots(&mut candidates, &roaming, "Steam Proton");
                let local = user.join("AppData/Local");
                if let Ok(items) = fs::read_dir(&local) {
                    for item in items.flatten().filter(|entry| entry.path().is_dir()) {
                        for suffix in [["Saved", "SaveGames"], ["Saved", "Saves"]] {
                            let path = item.path().join(suffix[0]).join(suffix[1]);
                            if path.is_dir() {
                                add_candidate(
                                    &mut candidates,
                                    path,
                                    format!(
                                        "Proton {appid} / {}",
                                        item.file_name().to_string_lossy()
                                    ),
                                    "Steam Proton",
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    candidates
}

#[cfg(target_os = "linux")]
fn linux_steam_roots(home: &Path) -> Vec<PathBuf> {
    [
        home.join(".steam/steam"),
        home.join(".local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join("snap/steam/common/.local/share/Steam"),
    ]
    .into_iter()
    .filter(|path| path.join("steamapps").is_dir())
    .collect()
}

fn directory_stats(path: &Path) -> (u64, u64, Option<u64>) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut modified = None;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !ignored_save_entry(entry))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        if let Ok(metadata) = entry.metadata() {
            files += 1;
            bytes = bytes.saturating_add(metadata.len());
            if let Ok(time) = metadata.modified() {
                if let Ok(value) = time.duration_since(UNIX_EPOCH) {
                    modified = Some(modified.unwrap_or(0).max(value.as_secs()));
                }
            }
        }
    }
    (files, bytes, modified)
}

#[tauri::command]
pub async fn scan_save_locations() -> Result<Vec<SaveLocation>, String> {
    tauri::async_runtime::spawn_blocking(scan_save_locations_blocking)
        .await
        .map_err(|error| format!("Сканирование прервано: {error}"))?
}

fn scan_save_locations_blocking() -> Result<Vec<SaveLocation>, String> {
    let mut locations = collect_candidates()
        .into_values()
        .filter_map(|candidate| {
            let (file_count, total_bytes, modified_unix) = directory_stats(&candidate.path);
            (file_count > 0).then(|| SaveLocation {
                id: format!("save-{:016x}", stable_hash(&candidate.path)),
                label: candidate.label,
                category: candidate.category,
                path: candidate.path.display().to_string(),
                file_count,
                total_bytes,
                modified_unix,
            })
        })
        .collect::<Vec<_>>();
    locations.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.label.cmp(&right.label))
    });
    Ok(locations)
}

fn stable_hash(path: &Path) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.to_string_lossy().to_lowercase().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[tauri::command]
pub async fn pick_save_archive(format: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || pick_save_archive_blocking(format))
        .await
        .map_err(|error| format!("Диалог выбора прерван: {error}"))?
}

fn pick_save_archive_blocking(format: String) -> Result<Option<String>, String> {
    let format = if format.eq_ignore_ascii_case("rar") {
        "rar"
    } else {
        "zip"
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let default_name = format!("CrocodileGena-Saves-{stamp}.{format}");
    #[cfg(target_os = "linux")]
    {
        return Ok(crate::native_dialog::save_file(
            "Сохранить архив игровых сохранений",
            &default_name,
            format,
        ));
    }
    #[cfg(target_os = "windows")]
    {
        let script = r#"Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.SaveFileDialog; $d.Title = 'Сохранить архив игровых сохранений'; $d.FileName = $env:CGS_ARCHIVE_NAME; if ($env:CGS_ARCHIVE_FORMAT -eq 'rar') { $d.Filter = 'RAR archive (*.rar)|*.rar' } else { $d.Filter = 'ZIP archive (*.zip)|*.zip' }; $d.DefaultExt = $env:CGS_ARCHIVE_FORMAT; $d.AddExtension = $true; $d.OverwritePrompt = $true; if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [Text.Encoding]::UTF8; Write-Output $d.FileName }"#;
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-STA", "-Command", script])
            .env("CGS_ARCHIVE_NAME", default_name)
            .env("CGS_ARCHIVE_FORMAT", format);
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        let output = command
            .output()
            .map_err(|error| format!("Не удалось открыть выбор файла: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!path.is_empty()).then_some(path))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Ok(None)
    }
}

#[tauri::command]
pub async fn pick_save_restore_archive() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(pick_save_restore_archive_blocking)
        .await
        .map_err(|error| format!("Диалог выбора прерван: {error}"))?
}

fn pick_save_restore_archive_blocking() -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    {
        return Ok(crate::native_dialog::pick_file(
            "Выберите архив сохранений Crocodile Gena Studio",
            &["zip", "rar"],
        ));
    }
    #[cfg(target_os = "windows")]
    {
        let script = r#"Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Title = 'Выберите архив сохранений Crocodile Gena Studio'; $d.Filter = 'Архивы сохранений (*.zip;*.rar)|*.zip;*.rar|ZIP archive (*.zip)|*.zip|RAR archive (*.rar)|*.rar'; $d.Multiselect = $false; $d.CheckFileExists = $true; if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [Text.Encoding]::UTF8; Write-Output $d.FileName }"#;
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-STA", "-Command", script]);
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        let output = command
            .output()
            .map_err(|error| format!("Не удалось открыть выбор файла: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!path.is_empty()).then_some(path))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Ok(None)
    }
}

fn portable_path(path: &Path) -> String {
    let mut roots = [
        "HOME",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        "OneDrive",
        "PUBLIC",
        "PROGRAMDATA",
        "ProgramFiles(x86)",
    ]
    .into_iter()
    .filter_map(|name| env_path(name).map(|root| (name, root)))
    .collect::<Vec<_>>();
    roots.sort_by_key(|(_, root)| std::cmp::Reverse(root.as_os_str().len()));
    for (name, root) in roots {
        if let Ok(relative) = path.strip_prefix(&root) {
            let suffix = relative.to_string_lossy().replace('/', "\\");
            return if suffix.is_empty() {
                format!("{{{name}}}")
            } else {
                format!("{{{name}}}\\{suffix}")
            };
        }
    }
    path.display().to_string()
}

fn path_has_only_normal_components(path: &Path) -> bool {
    use std::path::Component;
    path.components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

fn expand_portable_path(value: &str) -> Option<PathBuf> {
    if let Some(rest) = value.strip_prefix('{') {
        let end = rest.find('}')?;
        let name = &rest[..end];
        let allowed = [
            "LOCALAPPDATA",
            "APPDATA",
            "USERPROFILE",
            "HOME",
            "OneDrive",
            "PUBLIC",
            "PROGRAMDATA",
            "ProgramFiles(x86)",
        ];
        if !allowed.iter().any(|item| item.eq_ignore_ascii_case(name)) {
            return None;
        }
        let relative = rest[end + 1..].trim_start_matches(['\\', '/']);
        let relative = Path::new(relative);
        if !path_has_only_normal_components(relative) {
            return None;
        }
        return env_path(name).map(|root| root.join(relative));
    }
    let absolute = PathBuf::from(value);
    absolute.is_absolute().then_some(absolute)
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_lowercase()
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = normalized_path(path);
    let root = normalized_path(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|tail| tail.starts_with('\\'))
}

fn known_emulator_name(name: &str) -> bool {
    [
        "Goldberg SteamEmu Saves",
        "GSE Saves",
        "SmartSteamEmu",
        "SteamEmu",
        "CODEX",
        "EMPRESS",
        "RUNE",
        "FLT",
        "TENOKE",
        "SKIDROW",
        "OnlineFix",
        "0xdeadc0de",
    ]
    .iter()
    .any(|known| known.eq_ignore_ascii_case(name))
}

fn allowed_save_destination(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    if let Some(user) = env_path("USERPROFILE") {
        for root in [
            user.join("Saved Games"),
            user.join("Documents").join("My Games"),
            user.join("Documents").join("SavedGames"),
        ] {
            if path_is_within(path, &root) {
                return true;
            }
        }
    }
    if let Some(one_drive) = env_path("OneDrive") {
        if path_is_within(path, &one_drive.join("Documents").join("My Games")) {
            return true;
        }
    }
    if let Some(roaming) = env_path("APPDATA") {
        if let Ok(relative) = path.strip_prefix(&roaming) {
            let first = relative
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str());
            if first.is_some_and(known_emulator_name)
                || path_is_within(path, &roaming.join("Godot").join("app_userdata"))
            {
                return true;
            }
        }
    }
    if let Some(local) = env_path("LOCALAPPDATA") {
        if let Ok(relative) = path.strip_prefix(&local) {
            let first = relative
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str());
            if first.is_some_and(known_emulator_name) {
                return true;
            }
            let parts = relative
                .components()
                .filter_map(|part| part.as_os_str().to_str())
                .collect::<Vec<_>>();
            if parts.windows(2).any(|pair| {
                pair[0].eq_ignore_ascii_case("Saved")
                    && ["SaveGames", "SaveGame", "Saves"]
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(pair[1]))
            }) {
                return true;
            }
        }
        if let Some(app_data) = local.parent() {
            if path_is_within(path, &app_data.join("LocalLow")) {
                return true;
            }
        }
    }
    if let Some(public) = env_path("PUBLIC") {
        if path_is_within(path, &public.join("Documents")) {
            return true;
        }
    }
    if let Some(program_data) = env_path("PROGRAMDATA") {
        if path_is_within(path, &program_data.join("Steam")) {
            return true;
        }
    }
    if let Some(program_files) = env_path("ProgramFiles(x86)") {
        if path_is_within(path, &program_files.join("Steam").join("userdata")) {
            return true;
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(home) = env_path("HOME") {
        if path_is_within(path, &home.join("Documents"))
            || path_is_within(path, &home.join(".local/share"))
            || path_is_within(path, &home.join(".config"))
            || linux_steam_roots(&home).iter().any(|root| {
                path_is_within(path, &root.join("userdata"))
                    || path_is_within(path, &root.join("steamapps/compatdata"))
            })
        {
            return true;
        }
    }
    false
}

fn safe_folder_name(value: &OsStr, index: usize) -> String {
    let cleaned = value
        .to_string_lossy()
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            other => other,
        })
        .collect::<String>();
    let cleaned = cleaned.trim().trim_matches('.');
    format!(
        "{:02}_{}",
        index + 1,
        if cleaned.is_empty() { "Saves" } else { cleaned }
    )
}

fn copy_save_directory(source: &Path, target: &Path, logs: &mut Vec<String>) -> (u64, u64) {
    let mut copied = 0u64;
    let mut skipped = 0u64;
    let mut detailed_errors = 0usize;
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !ignored_save_entry(entry))
        .filter_map(Result::ok)
    {
        let Ok(relative) = entry.path().strip_prefix(source) else {
            continue;
        };
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            if fs::create_dir_all(&destination).is_err() {
                skipped += 1;
            }
        } else if entry.file_type().is_file() {
            if let Some(parent) = destination.parent() {
                let _ = fs::create_dir_all(parent);
            }
            match fs::copy(entry.path(), &destination) {
                Ok(_) => copied += 1,
                Err(error) => {
                    skipped += 1;
                    if error.kind() != std::io::ErrorKind::PermissionDenied && detailed_errors < 3 {
                        logs.push(format!("Файл пропущен {}: {error}", entry.path().display()));
                        detailed_errors += 1;
                    }
                }
            }
        }
    }
    (copied, skipped)
}

fn find_winrar() -> Option<PathBuf> {
    [env_path("ProgramFiles"), env_path("ProgramFiles(x86)")]
        .into_iter()
        .flatten()
        .map(|root| root.join("WinRAR").join("WinRAR.exe"))
        .find(|path| path.is_file())
}

fn create_zip(stage: &Path, destination: &Path) -> Result<(), String> {
    let file = File::create(destination)
        .map_err(|error| format!("Не удалось создать ZIP-архив: {error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    let file_options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let directory_options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    for entry in WalkDir::new(stage)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let Ok(relative) = entry.path().strip_prefix(stage) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let name = relative.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            archive
                .add_directory(
                    format!("{}/", name.trim_end_matches('/')),
                    directory_options,
                )
                .map_err(|error| format!("Не удалось добавить папку в ZIP: {error}"))?;
        } else if entry.file_type().is_file() {
            archive
                .start_file(name, file_options)
                .map_err(|error| format!("Не удалось добавить файл в ZIP: {error}"))?;
            let mut source = File::open(entry.path()).map_err(|error| {
                format!("Не удалось прочитать {}: {error}", entry.path().display())
            })?;
            std::io::copy(&mut source, &mut archive)
                .map_err(|error| format!("Не удалось записать файл в ZIP: {error}"))?;
        }
    }
    archive
        .finish()
        .map(|_| ())
        .map_err(|error| format!("Не удалось завершить ZIP-архив: {error}"))
}

fn create_rar(stage: &Path, destination: &Path) -> Result<(), String> {
    let executable = find_winrar().ok_or_else(|| {
        "WinRAR не найден. Установите WinRAR или выберите формат ZIP.".to_string()
    })?;
    let source = stage.join("*");
    let mut command = Command::new(executable);
    command.args([
        OsStr::new("a"),
        OsStr::new("-r"),
        OsStr::new("-ep1"),
        OsStr::new("-idq"),
        destination.as_os_str(),
        source.as_os_str(),
    ]);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000);
    let output = command
        .output()
        .map_err(|error| format!("Не удалось запустить WinRAR: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[tauri::command]
pub async fn create_save_archive(
    paths: Vec<String>,
    destination: String,
    format: String,
) -> SaveArchiveResult {
    match tauri::async_runtime::spawn_blocking(move || {
        create_save_archive_blocking(paths, destination, format)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => SaveArchiveResult::error(format!("Архивация прервана: {error}"), vec![]),
    }
}

fn create_save_archive_blocking(
    paths: Vec<String>,
    destination: String,
    format: String,
) -> SaveArchiveResult {
    if paths.is_empty() {
        return SaveArchiveResult::error("Не выбраны папки с сохранениями", vec![]);
    }
    if paths.len() > 256 {
        return SaveArchiveResult::error("Выбрано слишком много папок", vec![]);
    }
    let format = if format.eq_ignore_ascii_case("rar") {
        "rar"
    } else {
        "zip"
    };
    let mut destination = PathBuf::from(destination);
    if destination.extension().and_then(OsStr::to_str) != Some(format) {
        destination.set_extension(format);
    }
    if destination.as_os_str().is_empty() {
        return SaveArchiveResult::error("Не выбран путь для архива", vec![]);
    }
    if let Some(parent) = destination.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return SaveArchiveResult::error(
                format!("Не удалось подготовить папку архива: {error}"),
                vec![],
            );
        }
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let stage =
        std::env::temp_dir().join(format!("cgs-save-backup-{}-{stamp}", std::process::id()));
    if let Err(error) = fs::create_dir_all(&stage) {
        return SaveArchiveResult::error(
            format!("Не удалось создать временную папку: {error}"),
            vec![],
        );
    }

    let mut logs = Vec::new();
    let mut manifest = String::from("Crocodile Gena Studio — архив сохранений\r\n\r\n");
    let mut portable_manifest = SaveManifest {
        schema_version: 1,
        created_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0),
        items: Vec::new(),
    };
    let mut copied = 0u64;
    let mut skipped = 0u64;
    let mut accepted = 0u64;
    for (index, raw_path) in paths.into_iter().enumerate() {
        let source = PathBuf::from(raw_path);
        if !source.is_dir() {
            logs.push(format!("Папка пропущена: {}", source.display()));
            continue;
        }
        let folder = safe_folder_name(
            source.file_name().unwrap_or_else(|| OsStr::new("Saves")),
            index,
        );
        let target = stage.join(&folder);
        let (folder_copied, folder_skipped) = copy_save_directory(&source, &target, &mut logs);
        if folder_copied > 0 {
            accepted += 1;
            copied += folder_copied;
            skipped += folder_skipped;
            manifest.push_str(&format!("{folder} = {}\r\n", source.display()));
            portable_manifest.items.push(SaveManifestItem {
                folder,
                target: portable_path(&source),
            });
        }
    }
    let _ = fs::write(stage.join("backup-info.txt"), manifest.as_bytes());
    if let Ok(json) = serde_json::to_vec_pretty(&portable_manifest) {
        let _ = fs::write(stage.join(SAVE_MANIFEST_NAME), json);
    }
    if copied == 0 {
        let _ = fs::remove_dir_all(&stage);
        return SaveArchiveResult::error("Файлы сохранений для архивации не найдены", logs);
    }

    if destination.exists() {
        if let Err(error) = fs::remove_file(&destination) {
            let _ = fs::remove_dir_all(&stage);
            return SaveArchiveResult::error(
                format!("Не удалось заменить существующий архив: {error}"),
                logs,
            );
        }
    }
    let archive_result = if format == "rar" {
        create_rar(&stage, &destination)
    } else {
        create_zip(&stage, &destination)
    };
    let _ = fs::remove_dir_all(&stage);

    match archive_result {
        Ok(()) if destination.is_file() => {
            logs.push(format!("Добавлено папок: {accepted}"));
            logs.push(format!("Скопировано файлов: {copied}"));
            if skipped > 0 {
                logs.push(format!(
                    "Пропущено недоступных файлов: {skipped}. Закройте запущенные игры перед следующим архивированием."
                ));
            }
            SaveArchiveResult {
                success: true,
                message: format!("Архив сохранений создан: {}", destination.display()),
                logs,
                archive_path: Some(destination.display().to_string()),
            }
        }
        Ok(()) => SaveArchiveResult::error("Архиватор не создал выходной файл", logs),
        Err(error) => SaveArchiveResult::error(error, logs),
    }
}

fn extract_zip_secure(archive: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|error| format!("Не удалось открыть ZIP: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("Архив ZIP повреждён или имеет неизвестный формат: {error}"))?;
    if archive.len() > MAX_RESTORE_FILES {
        return Err("В архиве слишком много файлов".into());
    }
    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("Не удалось проверить ZIP: {error}"))?;
        if entry.enclosed_name().is_none() {
            return Err("В архиве найден небезопасный путь".into());
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Символические ссылки в архиве не поддерживаются".into());
        }
        total = total.saturating_add(entry.size());
        if total > MAX_RESTORE_BYTES {
            return Err("Распакованный архив превышает допустимый размер 10 ГБ".into());
        }
    }
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Не удалось прочитать ZIP: {error}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "В архиве найден небезопасный путь".to_string())?
            .to_path_buf();
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("Не удалось создать временную папку: {error}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Не удалось создать временную папку: {error}"))?;
        }
        let mut output = File::create(&target)
            .map_err(|error| format!("Не удалось распаковать {}: {error}", target.display()))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|error| format!("Не удалось распаковать {}: {error}", target.display()))?;
        output
            .flush()
            .map_err(|error| format!("Не удалось сохранить {}: {error}", target.display()))?;
    }
    Ok(())
}

fn extract_rar(archive: &Path, destination: &Path) -> Result<(), String> {
    let executable = find_winrar().ok_or_else(|| {
        "Для восстановления RAR требуется WinRAR. Используйте ZIP или установите WinRAR."
            .to_string()
    })?;
    let mut command = Command::new(executable);
    command.args([
        OsStr::new("x"),
        OsStr::new("-o+"),
        OsStr::new("-idq"),
        archive.as_os_str(),
        destination.as_os_str(),
    ]);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000);
    let output = command
        .output()
        .map_err(|error| format!("Не удалось запустить WinRAR: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn remap_legacy_path(value: &str) -> Option<PathBuf> {
    let original = PathBuf::from(value);
    if allowed_save_destination(&original) {
        return Some(original);
    }
    let normalized = value.replace('/', "\\");
    let mappings = [
        ("\\AppData\\Roaming\\", "APPDATA"),
        ("\\AppData\\Local\\", "LOCALAPPDATA"),
        ("\\Documents\\My Games\\", "USERPROFILE"),
        ("\\Documents\\SavedGames\\", "USERPROFILE"),
        ("\\Saved Games\\", "USERPROFILE"),
    ];
    for (marker, env) in mappings {
        let lower = normalized.to_lowercase();
        let marker_lower = marker.to_lowercase();
        let Some(position) = lower.find(&marker_lower) else {
            continue;
        };
        let suffix = &normalized[position + marker.len()..];
        let base = env_path(env)?;
        let target = match marker {
            "\\Documents\\My Games\\" => base.join("Documents").join("My Games").join(suffix),
            "\\Documents\\SavedGames\\" => base.join("Documents").join("SavedGames").join(suffix),
            "\\Saved Games\\" => base.join("Saved Games").join(suffix),
            _ => base.join(suffix),
        };
        if allowed_save_destination(&target) {
            return Some(target);
        }
    }
    let local_low_marker = "\\AppData\\LocalLow\\";
    let lower = normalized.to_lowercase();
    if let Some(position) = lower.find(&local_low_marker.to_lowercase()) {
        let suffix = &normalized[position + local_low_marker.len()..];
        if let Some(local) = env_path("LOCALAPPDATA") {
            if let Some(app_data) = local.parent() {
                let target = app_data.join("LocalLow").join(suffix);
                if allowed_save_destination(&target) {
                    return Some(target);
                }
            }
        }
    }
    None
}

fn load_restore_manifest(stage: &Path) -> Result<Vec<SaveManifestItem>, String> {
    let json_path = stage.join(SAVE_MANIFEST_NAME);
    if json_path.is_file() {
        let data = fs::read(&json_path)
            .map_err(|error| format!("Не удалось прочитать карту сохранений: {error}"))?;
        let manifest: SaveManifest = serde_json::from_slice(&data)
            .map_err(|error| format!("Карта сохранений повреждена: {error}"))?;
        if manifest.schema_version != 1 || manifest.items.is_empty() || manifest.items.len() > 256 {
            return Err("Версия или содержимое карты сохранений не поддерживается".into());
        }
        return Ok(manifest.items);
    }

    let legacy = fs::read_to_string(stage.join("backup-info.txt")).map_err(|_| {
        "Это не архив Crocodile Gena Studio: карта исходных папок не найдена".to_string()
    })?;
    let items = legacy
        .lines()
        .filter_map(|line| line.split_once(" = "))
        .map(|(folder, target)| SaveManifestItem {
            folder: folder.trim().to_string(),
            target: target.trim().to_string(),
        })
        .collect::<Vec<_>>();
    if items.is_empty() || items.len() > 256 {
        Err("В старом архиве не найдены сведения об исходных папках".into())
    } else {
        Ok(items)
    }
}

fn valid_manifest_folder(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && Path::new(value).components().count() == 1
        && !value.contains(['/', '\\'])
        && !value.eq_ignore_ascii_case(SAVE_MANIFEST_NAME)
        && !value.eq_ignore_ascii_case("backup-info.txt")
}

fn copy_restored_directory(
    source: &Path,
    target: &Path,
    backup: &Path,
    logs: &mut Vec<String>,
) -> (u64, u64, u64) {
    let mut copied = 0u64;
    let mut backed_up = 0u64;
    let mut skipped = 0u64;
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let Ok(relative) = entry.path().strip_prefix(source) else {
            continue;
        };
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            if fs::create_dir_all(&destination).is_err() {
                skipped += 1;
            }
            continue;
        }
        if !entry.file_type().is_file() {
            skipped += 1;
            continue;
        }
        if let Some(parent) = destination.parent() {
            if fs::create_dir_all(parent).is_err() {
                skipped += 1;
                continue;
            }
        }
        if destination.is_file() {
            let backup_file = backup.join(relative);
            if let Some(parent) = backup_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            match fs::copy(&destination, &backup_file) {
                Ok(_) => backed_up += 1,
                Err(error) => {
                    skipped += 1;
                    logs.push(format!(
                        "Не удалось защитить существующий файл {}: {error}",
                        destination.display()
                    ));
                    continue;
                }
            }
        }
        match fs::copy(entry.path(), &destination) {
            Ok(_) => copied += 1,
            Err(error) => {
                skipped += 1;
                if logs.len() < 20 {
                    logs.push(format!(
                        "Не удалось восстановить {}: {error}",
                        destination.display()
                    ));
                }
            }
        }
    }
    (copied, backed_up, skipped)
}

#[tauri::command]
pub async fn restore_save_archive(archive: String) -> SaveArchiveResult {
    match tauri::async_runtime::spawn_blocking(move || restore_save_archive_blocking(archive)).await
    {
        Ok(result) => result,
        Err(error) => SaveArchiveResult::error(format!("Восстановление прервано: {error}"), vec![]),
    }
}

fn restore_save_archive_blocking(archive: String) -> SaveArchiveResult {
    let archive = PathBuf::from(archive);
    if !archive.is_file() {
        return SaveArchiveResult::error("Выбранный архив не найден", vec![]);
    }
    let format = archive
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_lowercase();
    if format != "zip" && format != "rar" {
        return SaveArchiveResult::error("Поддерживаются только архивы ZIP и RAR", vec![]);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let stage =
        std::env::temp_dir().join(format!("cgs-save-restore-{}-{stamp}", std::process::id()));
    if let Err(error) = fs::create_dir_all(&stage) {
        return SaveArchiveResult::error(
            format!("Не удалось создать временную папку: {error}"),
            vec![],
        );
    }
    let extraction = if format == "rar" {
        extract_rar(&archive, &stage)
    } else {
        extract_zip_secure(&archive, &stage)
    };
    if let Err(error) = extraction {
        let _ = fs::remove_dir_all(&stage);
        return SaveArchiveResult::error(error, vec![]);
    }
    let items = match load_restore_manifest(&stage) {
        Ok(items) => items,
        Err(error) => {
            let _ = fs::remove_dir_all(&stage);
            return SaveArchiveResult::error(error, vec![]);
        }
    };
    let backup_root = env_path("LOCALAPPDATA")
        .unwrap_or_else(std::env::temp_dir)
        .join("CrocodileGenaStudio")
        .join("save-restore-backups")
        .join(stamp.to_string());
    let mut logs = Vec::new();
    let mut restored_folders = 0u64;
    let mut copied = 0u64;
    let mut backed_up = 0u64;
    let mut skipped = 0u64;

    for item in items {
        if !valid_manifest_folder(&item.folder) {
            logs.push(format!(
                "Пропущена папка с небезопасным именем: {}",
                item.folder
            ));
            skipped += 1;
            continue;
        }
        let source = stage.join(&item.folder);
        if !source.is_dir() {
            logs.push(format!("Папка отсутствует в архиве: {}", item.folder));
            skipped += 1;
            continue;
        }
        let target = expand_portable_path(&item.target)
            .filter(|path| allowed_save_destination(path))
            .or_else(|| remap_legacy_path(&item.target));
        let Some(target) = target else {
            logs.push(format!(
                "Небезопасный или неизвестный путь пропущен: {}",
                item.target
            ));
            skipped += 1;
            continue;
        };
        let (folder_copied, folder_backed_up, folder_skipped) =
            copy_restored_directory(&source, &target, &backup_root.join(&item.folder), &mut logs);
        if folder_copied > 0 {
            restored_folders += 1;
            logs.push(format!("{} → {}", item.folder, target.display()));
        }
        copied += folder_copied;
        backed_up += folder_backed_up;
        skipped += folder_skipped;
    }
    let _ = fs::remove_dir_all(&stage);
    if backed_up == 0 {
        let _ = fs::remove_dir_all(&backup_root);
    }
    logs.push(format!("Восстановлено папок: {restored_folders}"));
    logs.push(format!("Разложено файлов: {copied}"));
    if backed_up > 0 {
        logs.push(format!(
            "Предыдущие версии файлов сохранены: {}",
            backup_root.display()
        ));
    }
    if skipped > 0 {
        logs.push(format!("Пропущено элементов: {skipped}"));
    }
    if copied == 0 {
        SaveArchiveResult::error("Не удалось восстановить ни одного файла", logs)
    } else {
        SaveArchiveResult {
            success: true,
            message: format!("Сохранения восстановлены: папок {restored_folders}, файлов {copied}"),
            logs,
            archive_path: Some(archive.display().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cgs-save-test-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn calculates_save_directory_stats() {
        let directory = test_directory("stats");
        fs::create_dir_all(directory.join("slot1")).unwrap();
        fs::write(directory.join("slot1").join("save.dat"), b"12345").unwrap();
        fs::write(directory.join("profile.json"), b"1234567").unwrap();

        let (files, bytes, modified) = directory_stats(&directory);
        assert_eq!(files, 2);
        assert_eq!(bytes, 12);
        assert!(modified.is_some());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn excludes_service_caches_from_stats_and_backup() {
        let directory = test_directory("cache-filter");
        let source = directory.join("Game");
        let target = directory.join("Backup");
        let cache = source.join("PlatformProcess").join("component_crx_cache");
        fs::create_dir_all(source.join("Saves")).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(source.join("Saves").join("slot.sav"), b"progress").unwrap();
        fs::write(cache.join("service-cache.bin"), b"not a save").unwrap();

        let (files, bytes, _) = directory_stats(&source);
        assert_eq!(files, 1);
        assert_eq!(bytes, 8);
        let mut logs = Vec::new();
        let (copied, skipped) = copy_save_directory(&source, &target, &mut logs);
        assert_eq!(copied, 1);
        assert_eq!(skipped, 0);
        assert!(target.join("Saves").join("slot.sav").is_file());
        assert!(!target.join("PlatformProcess").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn creates_zip_archive_from_staging_directory() {
        let directory = test_directory("zip");
        let stage = directory.join("stage");
        let archive = directory.join("saves.zip");
        fs::create_dir_all(stage.join("Game")).unwrap();
        fs::write(stage.join("Game").join("save.dat"), b"game progress").unwrap();

        create_zip(&stage, &archive).unwrap();
        assert!(archive.is_file());
        assert!(fs::metadata(&archive).unwrap().len() > 0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn portable_paths_follow_the_current_windows_profile() {
        let local = env_path("LOCALAPPDATA").unwrap();
        let original = local.join("ExampleGame").join("Saved").join("SaveGames");
        let encoded = portable_path(&original);
        assert!(encoded.starts_with("{LOCALAPPDATA}"));
        assert_eq!(expand_portable_path(&encoded).unwrap(), original);
    }

    #[test]
    fn restore_copy_preserves_replaced_files() {
        let directory = test_directory("restore-copy");
        let source = directory.join("Source");
        let target = directory.join("Target");
        let backup = directory.join("Backup");
        fs::create_dir_all(source.join("slot")).unwrap();
        fs::create_dir_all(target.join("slot")).unwrap();
        fs::write(source.join("slot").join("save.dat"), b"new progress").unwrap();
        fs::write(target.join("slot").join("save.dat"), b"old progress").unwrap();

        let mut logs = Vec::new();
        let (copied, backed_up, skipped) =
            copy_restored_directory(&source, &target, &backup, &mut logs);
        assert_eq!((copied, backed_up, skipped), (1, 1, 0));
        assert_eq!(
            fs::read(target.join("slot").join("save.dat")).unwrap(),
            b"new progress"
        );
        assert_eq!(
            fs::read(backup.join("slot").join("save.dat")).unwrap(),
            b"old progress"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
