//! Module 06: inspect -> stage -> explicit confirmation -> copy. Never execute archive contents.
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};
use tauri::Emitter;

const MAX_ENTRIES: usize = 20_000;
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(1800);
static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();

#[derive(Clone)]
struct Entry {
    name: String,
    size: u64,
    index: usize,
}
struct Session {
    archive: PathBuf,
    password: String,
    entries: Vec<Entry>,
    folders: Vec<String>,
    stamp: Stamp,
    created: Instant,
    prepared: Option<Prepared>,
}
struct Prepared {
    stage: tempfile::TempDir,
    root: PathBuf,
    files: Vec<Planned>,
}
struct Planned {
    relative: PathBuf,
    staged: PathBuf,
    before: Option<Stamp>,
    size: u64,
}
#[derive(Clone, PartialEq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Inspection {
    id: String,
    folders: Vec<Folder>,
    recognized: bool,
}
#[derive(Serialize)]
struct Folder {
    path: String,
}
#[derive(Serialize)]
pub(crate) struct FolderFile {
    path: String,
}

#[tauri::command]
pub(crate) fn files_archive_contents(
    id: String,
    source: String,
) -> Result<Vec<FolderFile>, String> {
    let registry = sessions().lock().map_err(err)?;
    let session = registry
        .get(&id)
        .ok_or("Распаковка уже завершена или отменена")?;
    if session.created.elapsed() >= TTL {
        return Err("Время ожидания истекло. Выберите архив заново.".into());
    }
    if !session.folders.contains(&source) {
        return Err("Выберите папку из списка архива".into());
    }
    let prefix = format!("{source}/");
    let mut files: Vec<_> = session
        .entries
        .iter()
        .filter_map(|entry| {
            entry
                .name
                .strip_prefix(&prefix)
                .map(|path| FolderFile { path: path.into() })
        })
        .collect();
    files.sort_by_cached_key(|file| file.path.to_lowercase());
    Ok(files)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preview {
    files: Vec<PreviewFile>,
    destination: String,
    total_bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewFile {
    path: String,
    size: u64,
    replaces: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Installed {
    count: usize,
    backup_path: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    id: String,
    phase: &'static str,
    completed: usize,
    total: usize,
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn clear_sessions() {
    if let Ok(mut registry) = sessions().lock() {
        registry.clear();
    }
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn archive_error(error: impl std::fmt::Display) -> String {
    format!("Не удалось прочитать архив. Проверьте пароль и целостность файла: {error}")
}
fn stamp(path: &Path) -> Result<Stamp, String> {
    let m = fs::metadata(path).map_err(err)?;
    if !m.is_file() {
        return Err("Ожидался обычный файл".into());
    }
    Ok(Stamp {
        size: m.len(),
        modified: m.modified().ok(),
    })
}

// Validate independently of the host OS: backslashes, ADS, device names and aliases are forbidden.
fn safe_name(name: &str) -> Result<String, String> {
    let normalized = name.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.len() > 4096
        || normalized.split('/').count() > 64
    {
        return Err("Небезопасный путь в архиве".into());
    }
    for component in normalized.split('/') {
        let base = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || component
                .chars()
                .any(|c| c.is_control() || ":<>\"|?*".contains(c))
            || ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base.as_str())
            || ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"].contains(&base.as_str())
            || (base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && base.as_bytes()[3].is_ascii_digit())
            || component.eq_ignore_ascii_case(".cgs-backups")
        {
            return Err(format!("Небезопасный путь в архиве: {name}"));
        }
    }
    Ok(normalized.into())
}
fn validate_entries(entries: &[Entry]) -> Result<(), String> {
    if entries.len() > MAX_ENTRIES {
        return Err("В архиве слишком много файлов".into());
    }
    let mut names = HashSet::new();
    for entry in entries {
        let name = safe_name(&entry.name)?.to_lowercase();
        if !names.insert(name.clone()) {
            return Err(format!("Повторяющийся путь в архиве: {name}"));
        }
    }
    for name in &names {
        let mut parent = name.as_str();
        while let Some((p, _)) = parent.rsplit_once('/') {
            if names.contains(p) {
                return Err(format!("Конфликт файла и папки в архиве: {p}"));
            }
            parent = p;
        }
    }
    Ok(())
}
fn rar(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rar"))
}
fn list_entries(path: &Path, password: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    if rar(path) {
        let archive = unrar::Archive::with_password(path, password)
            .open_for_listing()
            .map_err(archive_error)?;
        for (index, header) in archive.enumerate() {
            let h = header.map_err(archive_error)?;
            if h.is_split() {
                return Err("Многотомные RAR пока не поддерживаются".into());
            }
            if h.is_file() {
                if h.file_attr & 0xf000 == 0xa000 || h.file_attr & 0x400 != 0 {
                    return Err("Ссылки в архиве не поддерживаются".into());
                }
                entries.push(Entry {
                    name: safe_name(&h.filename.to_string_lossy())?,
                    size: h.unpacked_size,
                    index,
                });
            }
            if index >= MAX_ENTRIES {
                return Err("В архиве слишком много записей".into());
            }
        }
    } else {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            return Err("Поддерживаются ZIP и RAR".into());
        }
        let mut archive =
            zip::ZipArchive::new(fs::File::open(path).map_err(err)?).map_err(archive_error)?;
        if archive.len() > MAX_ENTRIES {
            return Err("В архиве слишком много записей".into());
        }
        for index in 0..archive.len() {
            let h = archive.by_index_raw(index).map_err(archive_error)?;
            if h.is_symlink() {
                return Err("Ссылки в архиве не поддерживаются".into());
            }
            if !h.is_dir() {
                entries.push(Entry {
                    name: safe_name(h.name())?,
                    size: h.size(),
                    index,
                });
            }
        }
    }
    validate_entries(&entries)?;
    Ok(entries)
}
fn source_folders(entries: &[Entry]) -> (Vec<String>, bool) {
    let mut known = BTreeSet::new();
    let mut all = BTreeSet::new();
    for entry in entries {
        let parts: Vec<_> = entry.name.split('/').collect();
        for n in 1..parts.len() {
            let folder = parts[..n].join("/");
            all.insert(folder.clone());
            if parts[n - 1].eq_ignore_ascii_case("DenuvOwO")
                || parts[n - 1].eq_ignore_ascii_case("ARTIFACT")
            {
                known.insert(folder);
            }
        }
    }
    let recognized = !known.is_empty();
    (
        if recognized { known } else { all }.into_iter().collect(),
        recognized,
    )
}
fn inspect(path: PathBuf, password: String) -> Result<(Session, Vec<String>, bool), String> {
    if password.contains('\0') || password.len() > 1024 {
        return Err("Недопустимый пароль архива".into());
    }
    let archive = path.canonicalize().map_err(err)?;
    let before = stamp(&archive)?;
    let entries = list_entries(&archive, &password)?;
    let (folders, recognized) = source_folders(&entries);
    if folders.is_empty() {
        return Err("В архиве нет папки DenuvOwO или ARTIFACT и нет другой папки для выбора. Автоматическая распаковка всего архива отключена.".into());
    }
    let session = Session {
        archive,
        password,
        entries,
        folders: folders.clone(),
        stamp: before,
        created: Instant::now(),
        prepared: None,
    };
    Ok((session, folders, recognized))
}
#[tauri::command]
pub(crate) async fn files_archive_inspect(
    path: String,
    password: String,
) -> Result<Inspection, String> {
    let (session, folders, recognized) =
        tauri::async_runtime::spawn_blocking(move || inspect(path.into(), password))
            .await
            .map_err(err)??;
    let id = uuid::Uuid::new_v4().to_string();
    let mut registry = sessions().lock().map_err(err)?;
    registry.retain(|_, s| s.created.elapsed() < TTL);
    if registry.len() >= 8 {
        return Err("Сначала завершите текущие распаковки".into());
    }
    registry.insert(id.clone(), session);
    Ok(Inspection {
        id,
        folders: folders.into_iter().map(|path| Folder { path }).collect(),
        recognized,
    })
}
fn take(id: &str) -> Result<Session, String> {
    let session = sessions()
        .lock()
        .map_err(err)?
        .remove(id)
        .ok_or("Распаковка уже завершена или отменена")?;
    if session.created.elapsed() >= TTL {
        return Err("Время ожидания истекло. Выберите архив заново.".into());
    }
    Ok(session)
}
#[tauri::command]
pub(crate) fn files_archive_cancel(id: String) -> Result<(), String> {
    sessions().lock().map_err(err)?.remove(&id);
    Ok(())
}
fn no_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            #[cfg(windows)]
            let link = {
                use std::os::windows::fs::MetadataExt;
                m.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let link = m.file_type().is_symlink();
            if link {
                return Err(format!(
                    "Путь содержит ссылку или junction: {}",
                    path.display()
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(err(e)),
    }
    Ok(())
}
fn check_chain(path: &Path) -> Result<(), String> {
    for part in path.ancestors() {
        no_link(part)?;
    }
    Ok(())
}
fn target(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    safe_name(&relative.to_string_lossy())?;
    let path = root.join(relative);
    check_chain(&path)?;
    if let Some(parent) = path.parent() {
        for p in parent.ancestors().take_while(|p| *p != root) {
            if p.exists() && !p.is_dir() {
                return Err(format!("Папка занята файлом: {}", p.display()));
            }
        }
    }
    if path.exists() && !path.is_file() {
        return Err(format!("Файл занят папкой: {}", path.display()));
    }
    Ok(path)
}
fn game_root(path: &Path) -> Result<PathBuf, String> {
    check_chain(path)?;
    let root = path.canonicalize().map_err(err)?;
    if !root.is_dir() || root.parent().is_none() {
        return Err("Выберите отдельную папку игры, не корень диска".into());
    }
    for name in [
        "WINDIR",
        "SystemRoot",
        "USERPROFILE",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "TEMP",
        "LOCALAPPDATA",
        "APPDATA",
    ] {
        if let Some(value) = std::env::var_os(name) {
            if let Ok(protected) = PathBuf::from(value).canonicalize() {
                if root == protected
                    || ([
                        "WINDIR",
                        "SystemRoot",
                        "ProgramData",
                        "LOCALAPPDATA",
                        "APPDATA",
                        "TEMP",
                    ]
                    .contains(&name)
                        && root.starts_with(protected))
                {
                    return Err("Выберите папку игры, не системную папку".into());
                }
            }
        }
    }
    Ok(root)
}
fn notify(
    app: Option<&tauri::AppHandle>,
    id: &str,
    phase: &'static str,
    completed: usize,
    total: usize,
) {
    if let Some(app) = app {
        let _ = app.emit(
            "files-archive-progress",
            Progress {
                id: id.into(),
                phase,
                completed,
                total,
            },
        );
    }
}
fn prepare(
    session: &mut Session,
    source: &str,
    destination: &Path,
    app: Option<&tauri::AppHandle>,
    id: &str,
) -> Result<Preview, String> {
    session.prepared = None;
    if !session.folders.iter().any(|p| p == source) {
        return Err("Выберите папку из списка архива".into());
    }
    if stamp(&session.archive)? != session.stamp {
        return Err("Архив изменился. Выберите его заново.".into());
    }
    let root = game_root(destination)?;
    let prefix = format!("{source}/");
    let selected: Vec<_> = session
        .entries
        .iter()
        .filter(|e| e.name.starts_with(&prefix))
        .cloned()
        .collect();
    let total_bytes = selected.iter().try_fold(0u64, |sum, e| {
        if e.size > MAX_FILE {
            return Err("Файл в архиве превышает лимит 256 МБ".to_string());
        }
        sum.checked_add(e.size)
            .filter(|n| *n <= MAX_TOTAL)
            .ok_or("Содержимое папки превышает лимит 2 ГБ".into())
    })?;
    if selected.is_empty() {
        return Err("Выбранная папка пуста".into());
    }
    let stage = tempfile::Builder::new()
        .prefix("cgs-extract-")
        .tempdir()
        .map_err(err)?;
    let mut files = Vec::new();
    for (n, e) in selected.iter().enumerate() {
        let relative = PathBuf::from(e.name.strip_prefix(&prefix).unwrap());
        let to = target(&root, &relative)?;
        files.push(Planned {
            relative,
            staged: stage.path().join(n.to_string()),
            before: if to.exists() { Some(stamp(&to)?) } else { None },
            size: e.size,
        });
    }
    if rar(&session.archive) {
        // Read into memory, NEVER let UnRAR create paths, links or execute contents.
        let mut archive = unrar::Archive::with_password(&session.archive, &session.password)
            .open_for_processing()
            .map_err(archive_error)?;
        let mut index = 0;
        let mut done = 0;
        while let Some(header) = archive.read_header().map_err(archive_error)? {
            if let Some(n) = selected.iter().position(|e| e.index == index) {
                let e = &selected[n];
                if safe_name(&header.entry().filename.to_string_lossy())? != e.name
                    || header.entry().unpacked_size != e.size
                {
                    return Err("Содержимое архива изменилось".into());
                }
                let (bytes, next) = header.read().map_err(archive_error)?;
                if bytes.len() as u64 != e.size {
                    return Err("Размер распакованного файла не совпадает".into());
                }
                fs::write(&files[n].staged, bytes).map_err(err)?;
                archive = next;
                done += 1;
                notify(app, id, "extract", done, files.len());
            } else {
                archive = header.skip().map_err(archive_error)?;
            }
            index += 1;
        }
        if done != files.len() {
            return Err("Архив неполный".into());
        }
    } else {
        let mut archive = zip::ZipArchive::new(fs::File::open(&session.archive).map_err(err)?)
            .map_err(archive_error)?;
        for (n, e) in selected.iter().enumerate() {
            let file = archive
                .by_index_decrypt(e.index, session.password.as_bytes())
                .map_err(archive_error)?;
            let mut limited = file.take(e.size + 1);
            let mut output = fs::File::create(&files[n].staged).map_err(err)?;
            let copied = std::io::copy(&mut limited, &mut output).map_err(archive_error)?;
            if copied != e.size {
                return Err("Размер распакованного файла не совпадает".into());
            }
            output.flush().map_err(err)?;
            notify(app, id, "extract", n + 1, files.len());
        }
    }
    if stamp(&session.archive)? != session.stamp {
        return Err("Архив изменился во время распаковки".into());
    }
    let preview = Preview {
        files: files
            .iter()
            .map(|f| PreviewFile {
                path: f.relative.to_string_lossy().replace('\\', "/"),
                size: f.size,
                replaces: f.before.is_some(),
            })
            .collect(),
        destination: root.display().to_string(),
        total_bytes,
    };
    session.prepared = Some(Prepared { stage, root, files });
    Ok(preview)
}
#[tauri::command]
pub(crate) async fn files_archive_prepare(
    app: tauri::AppHandle,
    id: String,
    source: String,
    destination: String,
) -> Result<Preview, String> {
    let mut session = take(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = prepare(
            &mut session,
            &source,
            Path::new(&destination),
            Some(&app),
            &id,
        );
        sessions().lock().map_err(err)?.insert(id, session);
        result
    })
    .await
    .map_err(err)?
}
fn install(
    session: Session,
    app: Option<&tauri::AppHandle>,
    id: &str,
) -> Result<Installed, String> {
    install_with_copy(session, app, id, |from, to| fs::copy(from, to))
}
fn install_with_copy(
    session: Session,
    app: Option<&tauri::AppHandle>,
    id: &str,
    mut copy: impl FnMut(&Path, &Path) -> std::io::Result<u64>,
) -> Result<Installed, String> {
    let p = session
        .prepared
        .ok_or("Сначала подготовьте и подтвердите список файлов")?;
    // Recheck before ANY game writes: paths, snapshots and extracted file sizes.
    check_chain(&p.root)?;
    for f in &p.files {
        let to = target(&p.root, &f.relative)?;
        let now = if to.exists() { Some(stamp(&to)?) } else { None };
        if now != f.before {
            return Err(format!(
                "Файл изменился после предпросмотра: {}. Подготовьте распаковку заново.",
                f.relative.display()
            ));
        }
        if stamp(&f.staged)?.size != f.size {
            return Err("Временный файл изменился".into());
        }
    }
    // Keep the private TempDir alive until every copy/rollback has completed.
    let _stage = p.stage;
    let backup_base = p.root.join(".cgs-backups");
    check_chain(&backup_base)?;
    if backup_base.exists() && !backup_base.is_dir() {
        return Err("Папка резервных копий занята файлом".into());
    }
    let backup = backup_base.join(uuid::Uuid::new_v4().to_string());
    let replaced = p.files.iter().any(|f| f.before.is_some());
    if replaced {
        fs::create_dir_all(&backup_base).map_err(err)?;
        fs::create_dir(&backup).map_err(err)?;
        // A failed backup leaves ALL game files untouched.
        for f in p.files.iter().filter(|f| f.before.is_some()) {
            let to = target(&p.root, &f.relative)?;
            let original = backup.join(&f.relative);
            fs::create_dir_all(original.parent().unwrap()).map_err(err)?;
            fs::copy(to, original).map_err(err)?;
        }
    }
    let mut attempted = Vec::new();
    let mut directories = Vec::new();
    let result = (|| -> Result<(), String> {
        for (n, f) in p.files.iter().enumerate() {
            let to = target(&p.root, &f.relative)?;
            let now = if to.exists() { Some(stamp(&to)?) } else { None };
            if now != f.before {
                return Err(format!("Файл изменился: {}", f.relative.display()));
            }
            let mut missing = Vec::new();
            for parent in to.parent().unwrap().ancestors().take_while(|p| !p.exists()) {
                missing.push(parent.to_path_buf());
            }
            for parent in missing.into_iter().rev() {
                fs::create_dir(&parent).map_err(err)?;
                directories.push(parent);
            }
            attempted.push(n);
            // No DLLs, scripts or drivers are started by this operation.
            copy(&f.staged, &to).map_err(err)?;
            notify(app, id, "copy", n + 1, p.files.len());
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_errors = Vec::new();
        for n in attempted.into_iter().rev() {
            let f = &p.files[n];
            let restore = target(&p.root, &f.relative).and_then(|to| {
                if f.before.is_some() {
                    fs::copy(backup.join(&f.relative), to)
                        .map(|_| ())
                        .map_err(err)
                } else if to.exists() {
                    fs::remove_file(to).map_err(err)
                } else {
                    Ok(())
                }
            });
            if let Err(e) = restore {
                rollback_errors.push(e);
            }
        }
        for dir in directories.into_iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        return Err(if rollback_errors.is_empty() {
            format!("Копирование отменено, изменения откачены: {error}")
        } else {
            format!(
                "{error}. Часть файлов не удалось восстановить: {}. Резервная копия: {}",
                rollback_errors.join("; "),
                backup.display()
            )
        });
    }
    Ok(Installed {
        count: p.files.len(),
        backup_path: replaced.then(|| backup.display().to_string()),
    })
}
#[tauri::command]
pub(crate) async fn files_archive_install(
    app: tauri::AppHandle,
    id: String,
) -> Result<Installed, String> {
    let session = take(&id)?;
    tauri::async_runtime::spawn_blocking(move || install(session, Some(&app), &id))
        .await
        .map_err(err)?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixtures() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(".archive-test-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
    }
    fn zip_file(path: &Path, members: &[(&str, &str)], password: Option<&str>) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, text) in members {
            let mut options = zip::write::SimpleFileOptions::default();
            if let Some(password) = password {
                options = options.with_aes_encryption(zip::AesMode::Aes256, password);
            }
            zip.start_file(*name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    #[test]
    fn paths_reject_escape_aliases_and_ads() {
        for name in [
            "../x",
            "ARTIFACT/../x",
            "C:\\x",
            "/x",
            "x:stream",
            "NUL.txt",
            "x /file",
            "a//b",
            "a\\..\\x",
            ".cgs-backups/x",
        ] {
            assert!(safe_name(name).is_err(), "{name}");
        }
        assert_eq!(
            safe_name("ARTIFACT\\driver_amd\\file.dll").unwrap(),
            "ARTIFACT/driver_amd/file.dll"
        );
    }
    #[test]
    fn known_folders_backup_and_no_root_extras() {
        for folder in ["DenuvOwO", "ARTIFACT"] {
            let temp = fixtures();
            let archive = temp.path().join("test.zip");
            let game = temp.path().join("game");
            fs::create_dir(&game).unwrap();
            fs::write(game.join("file.dll"), "original").unwrap();
            zip_file(
                &archive,
                &[
                    (&format!("wrapper/{folder}/file.dll"), "new"),
                    (&format!("wrapper/{folder}/nested/config.ini"), "config"),
                    ("VBS.cmd", "outside"),
                    ("source.7z", "outside"),
                ],
                None,
            );
            let (mut session, folders, known) = inspect(archive.clone(), String::new()).unwrap();
            assert!(known);
            assert_eq!(folders.len(), 1);
            let preview = prepare(&mut session, &folders[0], &game, None, "test").unwrap();
            assert_eq!(preview.files.len(), 2);
            assert!(preview.files[0].replaces);
            assert_eq!(
                fs::read_to_string(game.join("file.dll")).unwrap(),
                "original"
            );
            let result = install(session, None, "test").unwrap();
            assert_eq!(result.count, 2);
            assert_eq!(
                fs::read_to_string(Path::new(&result.backup_path.unwrap()).join("file.dll"))
                    .unwrap(),
                "original"
            );
            assert_eq!(fs::read_to_string(game.join("file.dll")).unwrap(), "new");
            assert!(game.join("nested/config.ini").is_file());
            assert!(!game.join(folder).exists());
            assert!(!game.join("VBS.cmd").exists());
            assert!(archive.is_file());
        }
    }
    #[test]
    fn jpg_and_jpeg_are_still_prepared_copied_and_backed_up() {
        for folder in ["ARTIFACT", "DenuvOwO"] {
            let temp = fixtures();
            let archive = temp.path().join("test.zip");
            let game = temp.path().join("game");
            fs::create_dir(&game).unwrap();
            fs::write(game.join("cover.JPG"), "original jpg").unwrap();
            zip_file(
                &archive,
                &[
                    (&format!("{folder}/cover.JPG"), "new jpg"),
                    (&format!("{folder}/nested/photo.JpEg"), "jpeg"),
                    (&format!("{folder}/file.dll"), "dll"),
                    (&format!("{folder}/readme.txt"), "readme"),
                    (&format!("{folder}/preview.png"), "png"),
                ],
                None,
            );
            let (mut session, _, _) = inspect(archive, String::new()).unwrap();
            let token = uuid::Uuid::new_v4().to_string();
            sessions().lock().unwrap().insert(token.clone(), session);
            let contents = files_archive_contents(token.clone(), folder.into()).unwrap();
            assert_eq!(contents.len(), 5);
            assert!(contents.iter().any(|f| f.path == "cover.JPG"));
            assert!(files_archive_contents(token.clone(), "Other".into()).is_err());
            session = take(&token).unwrap();
            let preview = prepare(&mut session, folder, &game, None, "test").unwrap();
            assert_eq!(preview.files.len(), 5);
            assert!(preview
                .files
                .iter()
                .any(|f| f.path == "cover.JPG" && f.replaces));
            assert!(preview.files.iter().any(|f| f.path == "nested/photo.JpEg"));
            let installed = install(session, None, "test").unwrap();
            assert_eq!(installed.count, 5);
            assert_eq!(
                fs::read_to_string(game.join("cover.JPG")).unwrap(),
                "new jpg"
            );
            assert_eq!(
                fs::read_to_string(game.join("nested/photo.JpEg")).unwrap(),
                "jpeg"
            );
            assert_eq!(
                fs::read_to_string(Path::new(&installed.backup_path.unwrap()).join("cover.JPG"))
                    .unwrap(),
                "original jpg"
            );
            assert!(game.join("readme.txt").is_file());
            assert!(game.join("preview.png").is_file());
        }
    }
    #[test]
    fn encrypted_zip_uses_password_and_failure_leaves_game_untouched() {
        let temp = fixtures();
        let archive = temp.path().join("test.zip");
        let game = temp.path().join("game");
        fs::create_dir(&game).unwrap();
        zip_file(&archive, &[("ARTIFACT/file.dll", "secret")], Some("pass"));
        let (mut s, _, _) = inspect(archive.clone(), "wrong".into()).unwrap();
        assert!(prepare(&mut s, "ARTIFACT", &game, None, "test").is_err());
        assert_eq!(fs::read_dir(&game).unwrap().count(), 0);
        let (mut s, _, _) = inspect(archive, "pass".into()).unwrap();
        prepare(&mut s, "ARTIFACT", &game, None, "test").unwrap();
        install(s, None, "test").unwrap();
        assert_eq!(fs::read_to_string(game.join("file.dll")).unwrap(), "secret");
    }
    #[test]
    fn changed_destination_aborts_before_writes() {
        let temp = fixtures();
        let archive = temp.path().join("test.zip");
        let game = temp.path().join("game");
        fs::create_dir(&game).unwrap();
        zip_file(
            &archive,
            &[("ARTIFACT/file.dll", "new"), ("ARTIFACT/next.dll", "next")],
            None,
        );
        let (mut s, _, _) = inspect(archive, String::new()).unwrap();
        prepare(&mut s, "ARTIFACT", &game, None, "test").unwrap();
        fs::write(game.join("file.dll"), "user changed it").unwrap();
        assert!(install(s, None, "test").is_err());
        assert!(!game.join("next.dll").exists());
        assert!(!game.join(".cgs-backups").exists());
    }
    #[test]
    fn multiple_folders_and_unknown_folder_require_selection() {
        let temp = fixtures();
        let archive = temp.path().join("test.zip");
        zip_file(
            &archive,
            &[("DenuvOwO/a.dll", "a"), ("ARTIFACT/a.dll", "b")],
            None,
        );
        let (_, folders, known) = inspect(archive.clone(), String::new()).unwrap();
        assert!(known);
        assert_eq!(folders, vec!["ARTIFACT", "DenuvOwO"]);
        zip_file(
            &archive,
            &[("Other/a.dll", "a"), ("readme.txt", "outside")],
            None,
        );
        let (_, folders, known) = inspect(archive, String::new()).unwrap();
        assert!(!known);
        assert_eq!(folders, vec!["Other"]);
    }
    #[test]
    fn malicious_zip_fails_before_preparing() {
        let temp = fixtures();
        let archive = temp.path().join("test.zip");
        for members in [
            vec![("ARTIFACT/../outside", "bad")],
            vec![("ARTIFACT/A.dll", "a"), ("ARTIFACT/a.dll", "b")],
            vec![("ARTIFACT/dir", "a"), ("ARTIFACT/dir/a", "b")],
        ] {
            zip_file(&archive, &members, None);
            assert!(inspect(archive.clone(), String::new()).is_err());
        }
    }
    #[test]
    fn cancelled_prepare_leaves_no_game_files() {
        let temp = fixtures();
        let archive = temp.path().join("test.zip");
        let game = temp.path().join("game");
        fs::create_dir(&game).unwrap();
        zip_file(&archive, &[("ARTIFACT/a.dll", "a")], None);
        let (mut s, _, _) = inspect(archive, String::new()).unwrap();
        prepare(&mut s, "ARTIFACT", &game, None, "test").unwrap();
        let staged = s.prepared.as_ref().unwrap().stage.path().to_path_buf();
        drop(s);
        assert!(!staged.exists());
        assert_eq!(fs::read_dir(game).unwrap().count(), 0);
    }
    #[test]
    fn selected_data_limit_is_checked_before_staging() {
        let temp = fixtures();
        let archive = temp.path().join("test.zip");
        let game = temp.path().join("game");
        fs::create_dir(&game).unwrap();
        zip_file(&archive, &[("ARTIFACT/a.dll", "a")], None);
        let (mut s, _, _) = inspect(archive, String::new()).unwrap();
        s.entries[0].size = MAX_FILE + 1;
        assert!(prepare(&mut s, "ARTIFACT", &game, None, "test").is_err());
        assert!(s.prepared.is_none());
    }
    #[test]
    fn failed_copy_rolls_back_replaced_and_new_files() {
        let temp = fixtures();
        let archive = temp.path().join("test.zip");
        let game = temp.path().join("game");
        fs::create_dir(&game).unwrap();
        fs::write(game.join("a.dll"), "original").unwrap();
        zip_file(
            &archive,
            &[
                ("ARTIFACT/a.dll", "new"),
                ("ARTIFACT/nested/new.dll", "new file"),
                ("ARTIFACT/last.dll", "last"),
            ],
            None,
        );
        let (mut s, _, _) = inspect(archive, String::new()).unwrap();
        prepare(&mut s, "ARTIFACT", &game, None, "test").unwrap();
        let mut calls = 0;
        let result = install_with_copy(s, None, "test", |from, to| {
            calls += 1;
            if calls == 3 {
                return Err(std::io::Error::other("simulated disk write failure"));
            }
            fs::copy(from, to)
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(game.join("a.dll")).unwrap(), "original");
        assert!(!game.join("nested").exists());
        assert!(!game.join("last.dll").exists());
        assert_eq!(fs::read_dir(game.join(".cgs-backups")).unwrap().count(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn junction_destination_is_rejected() {
        let temp = fixtures();
        let real = temp.path().join("real");
        let link = temp.path().join("link");
        fs::create_dir(&real).unwrap();
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .unwrap()
            .status
            .success());
        assert!(game_root(&link).is_err());
        fs::remove_dir(link).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn rar_embedded_reader_handles_artifact_and_password() {
        // Optional fixture generator; application itself has NO WinRAR dependency.
        let rar_tool = Path::new("C:/Program Files/WinRAR/Rar.exe");
        if !rar_tool.is_file() {
            return;
        }
        let temp = fixtures();
        let game = temp.path().join("game");
        fs::create_dir(&game).unwrap();
        fs::create_dir(temp.path().join("ARTIFACT")).unwrap();
        fs::write(temp.path().join("ARTIFACT/test.dll"), "test bytes").unwrap();
        fs::write(temp.path().join("VBS.cmd"), "outside").unwrap();
        let archive = temp.path().join("test.rar");
        assert!(std::process::Command::new(rar_tool)
            .current_dir(temp.path())
            .args(["a", "-idq", "-hpsecret"])
            .arg(&archive)
            .args(["ARTIFACT", "VBS.cmd"])
            .status()
            .unwrap()
            .success());
        assert!(inspect(archive.clone(), "wrong".into()).is_err());
        let (mut s, folders, known) = inspect(archive, "secret".into()).unwrap();
        assert!(known);
        assert_eq!(folders, vec!["ARTIFACT"]);
        prepare(&mut s, "ARTIFACT", &game, None, "test").unwrap();
        install(s, None, "test").unwrap();
        assert_eq!(
            fs::read_to_string(game.join("test.dll")).unwrap(),
            "test bytes"
        );
        assert!(!game.join("VBS.cmd").exists());
    }
}
