//! Адаптировано из Eclipse Studio, коммит 13a295e0725ba450adea3322a8de1aca0ed4cff6.
//! Патч appcache: фикс ошибки «0 байт» / «Нет соединения».
//!
//! Повторяет режим Patch из SteamTools Fixer: Steam закрывается,
//! `appcache/appinfo.vdf` и `appcache/packageinfo.vdf` пересоздаются как пустые
//! read-only файлы. Оригиналы предварительно копируются в данные приложения,
//! поэтому патч можно выключить и вернуть стоковое состояние без ожидания Steam.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

/// Файлы appcache, которые пересоздаёт патч.
const TARGET_FILES: [&str; 2] = ["appinfo.vdf", "packageinfo.vdf"];
/// Каталог резервных копий внутри данных приложения.
const BACKUP_DIR_NAME: &str = "steam-patch-backup";
const BACKUP_MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamPatchFileState {
    pub name: String,
    pub exists: bool,
    pub size: u64,
    pub readonly: bool,
    /// Файл пересоздан патчем: пустой и read-only.
    pub patched: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamPatchStatus {
    pub steam_path: Option<String>,
    pub appcache_found: bool,
    /// stock | partial | patched | unknown
    pub state: String,
    pub files: Vec<SteamPatchFileState>,
    /// Есть проверенная резервная копия для текущей папки Steam.
    pub backup_available: bool,
    pub backup_at: Option<u64>,
    pub can_apply: bool,
    pub can_restore: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamPatchResult {
    pub status: SteamPatchStatus,
    /// Возврат выполнен из резервной копии, а не удалением пустых файлов.
    pub used_backup: bool,
    /// Steam был запущен до операции и успешно запущен снова.
    pub steam_restarted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupFile {
    name: String,
    existed: bool,
    size: u64,
    readonly: bool,
    sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchBackup {
    created_unix: u64,
    steam_path: String,
    files: Vec<BackupFile>,
}

impl SteamPatchStatus {
    fn unknown() -> Self {
        Self {
            steam_path: None,
            appcache_found: false,
            state: "unknown".to_string(),
            files: TARGET_FILES
                .iter()
                .map(|name| SteamPatchFileState {
                    name: (*name).to_string(),
                    exists: false,
                    size: 0,
                    readonly: false,
                    patched: false,
                })
                .collect(),
            backup_available: false,
            backup_at: None,
            can_apply: false,
            can_restore: false,
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn appcache_dir(steam: &Path) -> PathBuf {
    steam.join("appcache")
}

fn backup_dir(app: &AppHandle, steam: &Path) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|dir| dir.join(BACKUP_DIR_NAME).join(format!("{:x}", Sha256::digest(normalized_path(steam).as_bytes()))))
        .map_err(|error| format!("Не удалось определить папку данных приложения: {error}"))
}

fn normalized_path(path: &Path) -> String {
    let value = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut text = value.to_string_lossy().replace('/', "\\");
    while text.ends_with('\\') {
        text.pop();
    }
    text.to_lowercase()
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalized_path(left) == normalized_path(right)
}

fn file_state(path: &Path, name: &str) -> SteamPatchFileState {
    match fs::metadata(path) {
        Ok(metadata) => {
            let size = metadata.len();
            let readonly = metadata.permissions().readonly();
            SteamPatchFileState {
                name: name.to_string(),
                exists: true,
                size,
                readonly,
                patched: size == 0 && readonly,
            }
        }
        Err(_) => SteamPatchFileState {
            name: name.to_string(),
            exists: false,
            size: 0,
            readonly: false,
            patched: false,
        },
    }
}

fn inspect_appcache(appcache: &Path) -> Vec<SteamPatchFileState> {
    TARGET_FILES
        .iter()
        .map(|name| file_state(&appcache.join(name), name))
        .collect()
}

/// Оба файла — пустые read-only: патч активен. Часть файлов — partial.
fn patch_state(files: &[SteamPatchFileState]) -> &'static str {
    let patched = files.iter().filter(|file| file.patched).count();
    if patched == 0 {
        "stock"
    } else if patched == files.len() {
        "patched"
    } else {
        "partial"
    }
}

fn set_readonly(path: &Path, readonly: bool) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Не удалось прочитать атрибуты {}: {error}", path.display()))?;
    let mut permissions = metadata.permissions();
    permissions.set_readonly(readonly);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("Не удалось изменить атрибуты {}: {error}", path.display()))
}

// На Windows снимается только атрибут read-only; Unix-ветка здесь не используется.
#[allow(clippy::permissions_set_readonly_false)]
fn remove_readonly(path: &Path) {
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.permissions().readonly() {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            let _ = fs::set_permissions(path, permissions);
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("Не удалось открыть {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Не удалось прочитать {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn temporary_path(target: &Path, suffix: &str) -> Result<PathBuf, String> {
    let parent = target
        .parent()
        .ok_or_else(|| format!("Некорректный путь {}", target.display()))?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("Некорректное имя файла {}", target.display()))?;
    Ok(parent.join(format!(".{name}.{}.{suffix}", uuid::Uuid::new_v4())))
}

/// Заменяет файл пустым read-only; временный файл готовится до подмены.
fn write_placeholder(path: &Path) -> Result<(), String> {
    let temp = temporary_path(path, "cgs-patch.tmp")?;
    let _ = fs::remove_file(&temp);
    {
        let file = File::create(&temp)
            .map_err(|error| format!("Не удалось создать {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("Не удалось сохранить {}: {error}", temp.display()))?;
    }
    set_readonly(&temp, true)?;
    if path.exists() {
        remove_readonly(path);
        fs::remove_file(path)
            .map_err(|error| format!("Не удалось удалить {}: {error}", path.display()))?;
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("Не удалось заменить {}: {error}", path.display())
    })
}

/// Пересоздаёт оба целевых файла как пустые read-only. Уже пропатченные пропускает.
fn apply_placeholders(appcache: &Path) -> Result<(), String> {
    if !appcache.is_dir() {
        return Err("В папке Steam не найден appcache.".to_string());
    }
    for name in TARGET_FILES {
        let path = appcache.join(name);
        let state = file_state(&path, name);
        if state.patched {
            continue;
        }
        write_placeholder(&path)?;
    }
    Ok(())
}

/// Удаляет только пустые файлы: Steam сам создаст стандартные при запуске.
fn remove_empty_files(appcache: &Path) -> Result<u32, String> {
    let mut removed = 0_u32;
    for name in TARGET_FILES {
        let path = appcache.join(name);
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() != 0 || !metadata.permissions().readonly() {
            continue;
        }
        remove_readonly(&path);
        fs::remove_file(&path)
            .map_err(|error| format!("Не удалось удалить {}: {error}", path.display()))?;
        removed += 1;
    }
    Ok(removed)
}

/// Сохраняет оригиналы в отдельный каталог и атомарно заменяет прежнюю копию.
fn backup_originals(
    appcache: &Path,
    backup: &Path,
    steam: &Path,
    skip_placeholders: bool,
) -> Result<PatchBackup, String> {
    let parent = backup
        .parent()
        .ok_or_else(|| "Не удалось определить папку резервной копии.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Не удалось создать папку резервной копии: {error}"))?;
    let staging = parent.join(format!(".{BACKUP_DIR_NAME}-{}", uuid::Uuid::new_v4()));
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Не удалось подготовить резервную копию: {error}"))?;

    let result = (|| -> Result<PatchBackup, String> {
        let mut files = Vec::new();
        for name in TARGET_FILES {
            let path = appcache.join(name);
            let metadata = fs::metadata(&path).ok();
            let placeholder = metadata
                .as_ref()
                .is_some_and(|value| value.len() == 0 && value.permissions().readonly());
            // Уже пропатченный пустой файл не содержит оригинала: при возврате его
            // нужно удалить, чтобы Steam скачал стандартную версию.
            let existed = metadata.is_some() && !(skip_placeholders && placeholder);
            let (size, readonly, sha256) = if existed {
                let metadata = metadata.as_ref().expect("metadata проверена выше");
                let copy = staging.join(name);
                fs::copy(&path, &copy)
                    .map_err(|error| format!("Не удалось сохранить {name}: {error}"))?;
                (
                    metadata.len(),
                    metadata.permissions().readonly(),
                    Some(sha256_file(&path)?),
                )
            } else {
                (0, false, None)
            };
            files.push(BackupFile {
                name: (*name).to_string(),
                existed,
                size,
                readonly,
                sha256,
            });
        }

        let manifest = PatchBackup {
            created_unix: now_unix(),
            steam_path: steam.display().to_string(),
            files,
        };
        let data = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("Не удалось записать резервную копию: {error}"))?;
        fs::write(staging.join(BACKUP_MANIFEST), data)
            .map_err(|error| format!("Не удалось записать резервную копию: {error}"))?;

        if backup.exists() {
            fs::remove_dir_all(backup)
                .map_err(|error| format!("Не удалось обновить резервную копию: {error}"))?;
        }
        fs::rename(&staging, backup)
            .map_err(|error| format!("Не удалось закрепить резервную копию: {error}"))?;
        Ok(manifest)
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Читает резервную копию только для той же папки Steam и проверяет размеры копий.
fn read_backup(backup: &Path, steam: &Path) -> Option<PatchBackup> {
    let data = fs::read(backup.join(BACKUP_MANIFEST)).ok()?;
    let manifest: PatchBackup = serde_json::from_slice(&data).ok()?;
    if !same_path(Path::new(&manifest.steam_path), steam) {
        return None;
    }
    if manifest.files.len() != TARGET_FILES.len() || !TARGET_FILES.iter().all(|name| manifest.files.iter().any(|file| &file.name == name)) {
        return None;
    }
    for file in &manifest.files {
        if !file.existed {
            continue;
        }
        let metadata = fs::metadata(backup.join(&file.name)).ok()?;
        if metadata.len() != file.size {
            return None;
        }
    }
    Some(manifest)
}

fn replace_from_backup(source: &Path, target: &Path, readonly: bool) -> Result<(), String> {
    let temp = temporary_path(target, "cgs-restore.tmp")?;
    let _ = fs::remove_file(&temp);
    fs::copy(source, &temp)
        .map_err(|error| format!("Не удалось восстановить {}: {error}", target.display()))?;
    if target.exists() {
        remove_readonly(target);
        fs::remove_file(target)
            .map_err(|error| format!("Не удалось заменить {}: {error}", target.display()))?;
    }
    fs::rename(&temp, target).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("Не удалось восстановить {}: {error}", target.display())
    })?;
    set_readonly(target, readonly)
}

/// Возвращает оригиналы из резервной копии и удаляет её после успеха.
fn restore_from_backup(appcache: &Path, backup: &Path, steam: &Path) -> Result<(), String> {
    let manifest = read_backup(backup, steam).ok_or_else(|| {
        "Резервная копия не найдена или относится к другой папке Steam.".to_string()
    })?;

    // Сначала проверяем целостность всей копии: при ошибке файлы Steam не трогаем.
    for file in &manifest.files {
        if !file.existed {
            continue;
        }
        let source = backup.join(&file.name);
        if !source.is_file() {
            return Err(format!("В резервной копии отсутствует {}.", file.name));
        }
        if let Some(expected) = &file.sha256 {
            let actual = sha256_file(&source)?;
            if &actual != expected {
                return Err(format!(
                    "Резервная копия {} повреждена. Файлы Steam не изменены.",
                    file.name
                ));
            }
        }
    }

    for file in &manifest.files {
        let target = appcache.join(&file.name);
        if file.existed {
            let source = backup.join(&file.name);
            replace_from_backup(&source, &target, file.readonly)?;
        } else if target.exists() {
            // Файла не было до патча: удаляем только созданный пустой плейсхолдер,
            // но не настоящие данные, которые Steam мог успеть записать.
            if fs::metadata(&target)
                .map(|value| value.len() == 0)
                .unwrap_or(false)
            {
                remove_readonly(&target);
                fs::remove_file(&target)
                    .map_err(|error| format!("Не удалось удалить {}: {error}", target.display()))?;
            }
        }
    }

    let _ = fs::remove_dir_all(backup);
    Ok(())
}

#[cfg(windows)]
fn close_steam(steam: &Path) -> bool {
    let Ok(mut shutdown) = crate::luatools::hidden_command(steam.join("steam.exe").to_string_lossy().as_ref())
        .arg("-shutdown").spawn() else { return false; };
    for _ in 0..40 {
        if !crate::luatools::process_is_running("steam.exe") {
            let _ = shutdown.try_wait();
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

#[cfg(windows)]
fn restart_steam(steam: &Path) -> bool {
    let exe = steam.join("steam.exe");
    if !exe.is_file() {
        return false;
    }
    crate::luatools::hidden_command(exe.to_string_lossy().as_ref())
        .spawn()
        .is_ok()
}

#[cfg(not(windows))]
fn close_steam(_steam: &Path) -> bool { false }

#[cfg(not(windows))]
fn restart_steam(_steam: &Path) -> bool { false }

fn steam_running() -> bool {
    #[cfg(windows)]
    { crate::luatools::process_is_running("steam.exe") }
    #[cfg(not(windows))]
    { false }
}

fn validate_appcache(appcache: &Path) -> Result<(), String> {
    crate::lua_loader::reject_link(appcache)?;
    for name in TARGET_FILES {
        crate::lua_loader::reject_link(&appcache.join(name))?;
    }
    Ok(())
}

/// Ошибка после закрытия Steam не должна оставлять клиент выключенным.
fn with_steam_restart_on_error<T>(
    steam: &Path,
    restart_needed: bool,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let result = operation();
    if result.is_err() && restart_needed {
        let _ = restart_steam(steam);
    }
    result
}

fn status_snapshot(app: &AppHandle, steam_path: Option<String>) -> SteamPatchStatus {
    let Some(steam) = crate::lua_loader::resolve(steam_path).ok() else {
        return SteamPatchStatus::unknown();
    };
    let appcache = appcache_dir(&steam);
    let appcache_found = appcache.is_dir() && validate_appcache(&appcache).is_ok();
    let files = if appcache_found {
        inspect_appcache(&appcache)
    } else {
        TARGET_FILES
            .iter()
            .map(|name| SteamPatchFileState {
                name: (*name).to_string(),
                exists: false,
                size: 0,
                readonly: false,
                patched: false,
            })
            .collect()
    };
    let state = if appcache_found {
        patch_state(&files)
    } else {
        "unknown"
    };
    let (backup_available, backup_at) = match backup_dir(app, &steam) {
        Ok(dir) => match read_backup(&dir, &steam) {
            Some(manifest) => (true, Some(manifest.created_unix)),
            None => (false, None),
        },
        Err(_) => (false, None),
    };

    SteamPatchStatus {
        steam_path: Some(steam.display().to_string()),
        appcache_found,
        state: state.to_string(),
        files,
        backup_available,
        backup_at,
        can_apply: appcache_found && state != "patched",
        can_restore: state == "patched" || state == "partial",
    }
}

fn apply_blocking(app: AppHandle, steam_path: Option<String>) -> Result<SteamPatchResult, String> {
    let Some(steam) = crate::lua_loader::resolve(steam_path).ok() else {
        return Err("Папка Steam не найдена. Укажите путь в разделе CrocLua.".to_string());
    };
    let appcache = appcache_dir(&steam);
    if !appcache.is_dir() {
        return Err("В папке Steam не найден appcache. Запустите Steam и повторите.".to_string());
    }
    let _guard = crate::lua_loader::acquire_file_lock(&steam)?;
    validate_appcache(&appcache)?;

    let before = status_snapshot(&app, Some(steam.display().to_string()));
    if before.state == "patched" {
        let used_backup = before.backup_available;
        return Ok(SteamPatchResult {
            status: before,
            used_backup,
            steam_restarted: false,
        });
    }

    let was_running = steam_running();
    if was_running && !close_steam(&steam) {
        return Err(
            "Steam не удалось завершить автоматически. Закройте Steam и повторите попытку."
                .to_string(),
        );
    }

    with_steam_restart_on_error(&steam, was_running, || {
        let backup = backup_dir(&app, &steam)?;
        let has_backup = read_backup(&backup, &steam).is_some();
        if !has_backup || before.state == "stock" {
            backup_originals(&appcache, &backup, &steam, before.state == "partial")?;
        }

        if let Err(error) = apply_placeholders(&appcache) {
            let rollback = restore_from_backup(&appcache, &backup, &steam);
            let suffix = if rollback.is_ok() {
                "Изменения отменены."
            } else {
                "Не удалось полностью отменить изменения: проверьте папку appcache."
            };
            return Err(format!("Не удалось применить патч: {error}. {suffix}"));
        }

        let steam_restarted = was_running && restart_steam(&steam);
        let status = status_snapshot(&app, Some(steam.display().to_string()));
        Ok(SteamPatchResult {
            status,
            used_backup: read_backup(&backup, &steam).is_some(),
            steam_restarted,
        })
    })
}

fn restore_blocking(
    app: AppHandle,
    steam_path: Option<String>,
) -> Result<SteamPatchResult, String> {
    let Some(steam) = crate::lua_loader::resolve(steam_path).ok() else {
        return Err("Папка Steam не найдена. Укажите путь в разделе CrocLua.".to_string());
    };
    let appcache = appcache_dir(&steam);
    if !appcache.is_dir() {
        return Err("В папке Steam не найден appcache. Запустите Steam и повторите.".to_string());
    }
    let _guard = crate::lua_loader::acquire_file_lock(&steam)?;
    validate_appcache(&appcache)?;

    let before = status_snapshot(&app, Some(steam.display().to_string()));
    if before.state == "stock" || before.state == "unknown" {
        return Ok(SteamPatchResult {
            status: before,
            used_backup: false,
            steam_restarted: false,
        });
    }

    let was_running = steam_running();
    if was_running && !close_steam(&steam) {
        return Err(
            "Steam не удалось завершить автоматически. Закройте Steam и повторите попытку."
                .to_string(),
        );
    }

    with_steam_restart_on_error(&steam, was_running, || {
        let backup = backup_dir(&app, &steam)?;
        let used_backup = if backup.exists() {
            restore_from_backup(&appcache, &backup, &steam)?;
            true
        } else {
            remove_empty_files(&appcache)?;
            false
        };

        let steam_restarted = was_running && restart_steam(&steam);
        let status = status_snapshot(&app, Some(steam.display().to_string()));
        Ok(SteamPatchResult {
            status,
            used_backup,
            steam_restarted,
        })
    })
}

#[tauri::command(async)]
pub fn steam_patch_status(app: AppHandle, steam_path: Option<String>) -> SteamPatchStatus {
    if cfg!(windows) { status_snapshot(&app, steam_path) } else { SteamPatchStatus::unknown() }
}

#[tauri::command]
pub async fn steam_patch_apply(
    app: AppHandle,
    steam_path: Option<String>,
) -> Result<SteamPatchResult, String> {
    if !cfg!(windows) { return Err("Фикс доступен только в Windows.".into()); }
    tauri::async_runtime::spawn_blocking(move || apply_blocking(app, steam_path))
        .await
        .map_err(|error| format!("Патч прерван: {error}"))?
}

#[tauri::command]
pub async fn steam_patch_restore(
    app: AppHandle,
    steam_path: Option<String>,
) -> Result<SteamPatchResult, String> {
    if !cfg!(windows) { return Err("Фикс доступен только в Windows.".into()); }
    tauri::async_runtime::spawn_blocking(move || restore_blocking(app, steam_path))
        .await
        .map_err(|error| format!("Возврат файлов прерван: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_appcache() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("appcache")).unwrap();
        dir
    }

    fn write_stock(appcache: &Path, name: &str, contents: &[u8]) {
        fs::write(appcache.join(name), contents).unwrap();
    }

    #[test]
    fn stock_patch_and_restore_round_trip() {
        let temp = make_appcache();
        let steam = temp.path();
        let appcache = appcache_dir(steam);
        let backup = temp.path().join("backup");
        write_stock(&appcache, "appinfo.vdf", b"stock-appinfo");
        write_stock(&appcache, "packageinfo.vdf", b"stock-packageinfo");

        let before = inspect_appcache(&appcache);
        assert_eq!(patch_state(&before), "stock");

        backup_originals(&appcache, &backup, steam, false).unwrap();
        apply_placeholders(&appcache).unwrap();

        let patched = inspect_appcache(&appcache);
        assert_eq!(patch_state(&patched), "patched");
        assert!(patched.iter().all(|file| file.patched));

        restore_from_backup(&appcache, &backup, steam).unwrap();
        assert_eq!(
            fs::read(appcache.join("appinfo.vdf")).unwrap(),
            b"stock-appinfo"
        );
        assert_eq!(
            fs::read(appcache.join("packageinfo.vdf")).unwrap(),
            b"stock-packageinfo"
        );
        assert!(!backup.exists());
        assert_eq!(patch_state(&inspect_appcache(&appcache)), "stock");
    }

    #[test]
    fn partial_patch_marks_existing_placeholder_as_missing_original() {
        let temp = make_appcache();
        let steam = temp.path();
        let appcache = appcache_dir(steam);
        let backup = temp.path().join("backup");
        write_stock(&appcache, "appinfo.vdf", b"stock-appinfo");
        write_placeholder(&appcache.join("packageinfo.vdf")).unwrap();

        assert_eq!(patch_state(&inspect_appcache(&appcache)), "partial");
        backup_originals(&appcache, &backup, steam, true).unwrap();
        apply_placeholders(&appcache).unwrap();
        restore_from_backup(&appcache, &backup, steam).unwrap();

        assert_eq!(
            fs::read(appcache.join("appinfo.vdf")).unwrap(),
            b"stock-appinfo"
        );
        assert!(!appcache.join("packageinfo.vdf").exists());
    }

    #[test]
    fn restore_without_backup_removes_only_empty_files() {
        let temp = make_appcache();
        let steam = temp.path();
        let appcache = appcache_dir(steam);
        write_placeholder(&appcache.join("appinfo.vdf")).unwrap();
        write_stock(&appcache, "packageinfo.vdf", b"keep-me");

        assert_eq!(remove_empty_files(&appcache).unwrap(), 1);
        assert!(!appcache.join("appinfo.vdf").exists());
        assert_eq!(
            fs::read(appcache.join("packageinfo.vdf")).unwrap(),
            b"keep-me"
        );
        let _ = steam;
    }

    #[test]
    fn backup_is_ignored_for_another_steam_path() {
        let temp = make_appcache();
        let steam = temp.path().join("Steam");
        let other = temp.path().join("Other");
        fs::create_dir_all(steam.join("appcache")).unwrap();
        fs::create_dir_all(other.join("appcache")).unwrap();
        write_stock(&steam.join("appcache"), "appinfo.vdf", b"stock");
        write_stock(&steam.join("appcache"), "packageinfo.vdf", b"stock");
        let backup = temp.path().join("backup");
        backup_originals(&steam.join("appcache"), &backup, &steam, false).unwrap();

        assert!(read_backup(&backup, &steam).is_some());
        assert!(read_backup(&backup, &other).is_none());
        assert!(restore_from_backup(&other.join("appcache"), &backup, &other).is_err());
    }

    #[test]
    fn missing_files_become_placeholders_and_restore_removes_them() {
        let temp = make_appcache();
        let steam = temp.path();
        let appcache = appcache_dir(steam);
        let backup = temp.path().join("backup");
        backup_originals(&appcache, &backup, steam, false).unwrap();
        apply_placeholders(&appcache).unwrap();
        assert_eq!(patch_state(&inspect_appcache(&appcache)), "patched");

        restore_from_backup(&appcache, &backup, steam).unwrap();
        assert!(!appcache.join("appinfo.vdf").exists());
        assert!(!appcache.join("packageinfo.vdf").exists());
    }
}
