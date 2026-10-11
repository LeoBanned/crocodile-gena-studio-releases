use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

const GIST_ID: &str = "b314de5f9a3e2293b7e3c22136d49889";
const GIST_OWNER: &str = "LeoBanned";
const DOWNLOAD_BUFFER_SIZE: usize = 1024 * 1024;
const PARALLEL_DOWNLOAD_PARTS: u64 = 6;
static DOWNLOAD_CANCELLATIONS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

#[derive(Deserialize)]
struct RemoteCatalog {
    #[serde(default)]
    cracks: Vec<RemoteFile>,
}

#[derive(Deserialize)]
struct RemoteFile {
    #[serde(default)]
    name: String,
    #[serde(default)]
    build: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    pinned: bool,
}

#[derive(Clone, Serialize)]
pub(crate) struct FileItem {
    name: String,
    build: String,
    url: String,
    password: String,
    pinned: bool,
}

#[derive(Serialize)]
pub(crate) struct FilesCatalog {
    files: Vec<FileItem>,
    total: usize,
    cached: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    url: String,
    downloaded: u64,
    total: Option<u64>,
    percent: Option<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadResult {
    path: String,
    file_name: String,
}

#[tauri::command]
pub(crate) async fn files_catalog(app: tauri::AppHandle) -> Result<FilesCatalog, String> {
    tauri::async_runtime::spawn_blocking(move || load_catalog(&app))
        .await
        .map_err(|error| format!("Загрузка списка файлов прервана: {error}"))?
}

#[tauri::command]
pub(crate) async fn files_download(
    app: tauri::AppHandle,
    url: String,
    suggested_name: String,
) -> Result<DownloadResult, String> {
    let cancellation = Arc::new(AtomicBool::new(false));
    cancellation_registry()
        .lock()
        .map_err(|_| "Не удалось запустить загрузку".to_string())?
        .insert(url.clone(), Arc::clone(&cancellation));
    let registry_url = url.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_file(&app, &url, &suggested_name, &cancellation)
    })
    .await
    .map_err(|error| format!("Загрузка файла прервана: {error}"))?;
    if let Ok(mut registry) = cancellation_registry().lock() {
        registry.remove(&registry_url);
    }
    result
}

#[tauri::command]
pub(crate) fn files_cancel_download(url: String) -> Result<bool, String> {
    let registry = cancellation_registry()
        .lock()
        .map_err(|_| "Не удалось отменить загрузку".to_string())?;
    if let Some(cancellation) = registry.get(&url) {
        cancellation.store(true, Ordering::Relaxed);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub(crate) fn files_reveal(path: String) -> Result<(), String> {
    let file = PathBuf::from(path);
    if !file.is_file() {
        return Err("Загруженный файл больше не найден".into());
    }

    #[cfg(target_os = "windows")]
    {
        let folder = file
            .parent()
            .ok_or_else(|| "Не удалось определить папку загрузки".to_string())?;
        std::process::Command::new("explorer.exe")
            .arg(folder)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Не удалось открыть папку: {error}"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = file;
        Err("Открытие папки поддерживается только в Windows".into())
    }
}

fn download_file(
    app: &tauri::AppHandle,
    url: &str,
    suggested_name: &str,
    cancellation: &Arc<AtomicBool>,
) -> Result<DownloadResult, String> {
    if !url.starts_with("https://") {
        return Err("Разрешены только безопасные HTTPS-ссылки".into());
    }

    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| format!("Не удалось определить папку «Загрузки»: {error}"))?;
    fs::create_dir_all(&downloads)
        .map_err(|error| format!("Не удалось подготовить папку «Загрузки»: {error}"))?;

    let safe_name = safe_file_name(suggested_name);
    let destination = available_path(&downloads, &safe_name);
    let partial = destination.with_extension(format!(
        "{}part",
        destination
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!("{value}."))
            .unwrap_or_default()
    ));

    let result = (|| {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(900))
            .pool_max_idle_per_host(PARALLEL_DOWNLOAD_PARTS as usize)
            .user_agent("CrocodileGenaStudio/5.1.0")
            .build()
            .map_err(|error| format!("HTTP-клиент не создан: {error}"))?;
        let head = client
            .head(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .ok();
        let total = head
            .as_ref()
            .and_then(reqwest::blocking::Response::content_length);
        let supports_ranges = head
            .as_ref()
            .and_then(|response| response.headers().get(reqwest::header::ACCEPT_RANGES))
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("bytes"));
        let resolved_url = head
            .as_ref()
            .map(|response| response.url().as_str())
            .unwrap_or(url)
            .to_string();

        let downloaded = if supports_ranges && total.is_some_and(|size| size >= 4 * 1024 * 1024) {
            download_parallel(
                app,
                &client,
                url,
                &resolved_url,
                &partial,
                total.unwrap_or_default(),
                cancellation,
            )?
        } else {
            download_sequential(app, &client, url, &partial, total, cancellation)?
        };
        ensure_not_cancelled(cancellation)?;
        fs::rename(&partial, &destination)
            .map_err(|error| format!("Не удалось сохранить загруженный файл: {error}"))?;
        emit_progress(app, url, downloaded, total.or(Some(downloaded)));
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }

    Ok(DownloadResult {
        file_name: destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(&safe_name)
            .to_string(),
        path: destination.to_string_lossy().to_string(),
    })
}

fn cancellation_registry() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    DOWNLOAD_CANCELLATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ensure_not_cancelled(cancellation: &AtomicBool) -> Result<(), String> {
    if cancellation.load(Ordering::Relaxed) {
        Err("Загрузка отменена".into())
    } else {
        Ok(())
    }
}

fn download_sequential(
    app: &tauri::AppHandle,
    client: &reqwest::blocking::Client,
    event_url: &str,
    destination: &Path,
    known_total: Option<u64>,
    cancellation: &AtomicBool,
) -> Result<u64, String> {
    let mut response = client
        .get(event_url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("Сервер вернул ошибку: {error}"))?;
    let total = known_total.or_else(|| response.content_length());
    let mut output = fs::File::create(destination)
        .map_err(|error| format!("Не удалось создать файл: {error}"))?;
    let mut buffer = vec![0_u8; DOWNLOAD_BUFFER_SIZE];
    let mut downloaded = 0_u64;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    emit_progress(app, event_url, 0, total);
    loop {
        ensure_not_cancelled(cancellation)?;
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("Ошибка получения данных: {error}"))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("Ошибка записи файла: {error}"))?;
        downloaded += read as u64;
        if last_emit.elapsed() >= Duration::from_millis(180) {
            emit_progress(app, event_url, downloaded, total);
            last_emit = Instant::now();
        }
    }
    output
        .flush()
        .map_err(|error| format!("Не удалось завершить запись файла: {error}"))?;
    Ok(downloaded)
}

fn download_parallel(
    app: &tauri::AppHandle,
    client: &reqwest::blocking::Client,
    event_url: &str,
    resolved_url: &str,
    destination: &Path,
    total: u64,
    cancellation: &Arc<AtomicBool>,
) -> Result<u64, String> {
    let part_count = PARALLEL_DOWNLOAD_PARTS.min((total / (2 * 1024 * 1024)).max(2));
    let downloaded = Arc::new(AtomicU64::new(0));
    let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let segment_paths = (0..part_count)
        .map(|index| PathBuf::from(format!("{}.segment-{index}", destination.display())))
        .collect::<Vec<_>>();

    let results = thread::scope(|scope| {
        let mut handles = Vec::new();
        for index in 0..part_count {
            let start = total * index / part_count;
            let end = (total * (index + 1) / part_count).saturating_sub(1);
            let client = client.clone();
            let source = resolved_url.to_string();
            let segment = segment_paths[index as usize].clone();
            let cancellation = Arc::clone(cancellation);
            let downloaded = Arc::clone(&downloaded);
            let last_emit = Arc::clone(&last_emit);
            let app = app.clone();
            let event_url = event_url.to_string();
            handles.push(scope.spawn(move || -> Result<(), String> {
                let mut response = client
                    .get(source)
                    .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                    .send()
                    .map_err(|error| format!("Не удалось получить часть файла: {error}"))?;
                if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    return Err("Сервер не подтвердил ускоренную загрузку".into());
                }
                let mut output = fs::File::create(&segment)
                    .map_err(|error| format!("Не удалось создать часть файла: {error}"))?;
                let mut buffer = vec![0_u8; DOWNLOAD_BUFFER_SIZE];
                loop {
                    ensure_not_cancelled(&cancellation)?;
                    let read = response
                        .read(&mut buffer)
                        .map_err(|error| format!("Ошибка получения части файла: {error}"))?;
                    if read == 0 {
                        break;
                    }
                    output
                        .write_all(&buffer[..read])
                        .map_err(|error| format!("Ошибка записи части файла: {error}"))?;
                    let current =
                        downloaded.fetch_add(read as u64, Ordering::Relaxed) + read as u64;
                    if let Ok(mut emitted_at) = last_emit.lock() {
                        if emitted_at.elapsed() >= Duration::from_millis(180) {
                            emit_progress(&app, &event_url, current, Some(total));
                            *emitted_at = Instant::now();
                        }
                    }
                }
                Ok(())
            }));
        }
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "Поток ускоренной загрузки завершился с ошибкой".to_string())?
            })
            .collect::<Result<Vec<_>, String>>()
    });

    if let Err(error) = results {
        for segment in &segment_paths {
            let _ = fs::remove_file(segment);
        }
        return Err(error);
    }
    let assemble_result = (|| {
        ensure_not_cancelled(cancellation)?;
        let mut output = fs::File::create(destination)
            .map_err(|error| format!("Не удалось собрать файл: {error}"))?;
        let mut buffer = vec![0_u8; DOWNLOAD_BUFFER_SIZE];
        for segment in &segment_paths {
            ensure_not_cancelled(cancellation)?;
            let mut input = fs::File::open(segment)
                .map_err(|error| format!("Не удалось открыть часть файла: {error}"))?;
            input
                .seek(SeekFrom::Start(0))
                .map_err(|error| format!("Не удалось прочитать часть файла: {error}"))?;
            loop {
                let read = input
                    .read(&mut buffer)
                    .map_err(|error| format!("Не удалось собрать часть файла: {error}"))?;
                if read == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| format!("Не удалось записать собранный файл: {error}"))?;
            }
        }
        output
            .flush()
            .map_err(|error| format!("Не удалось завершить файл: {error}"))
    })();
    for segment in &segment_paths {
        let _ = fs::remove_file(segment);
    }
    assemble_result?;
    Ok(downloaded.load(Ordering::Relaxed))
}

fn emit_progress(app: &tauri::AppHandle, url: &str, downloaded: u64, total: Option<u64>) {
    let percent = total
        .filter(|value| *value > 0)
        .map(|value| ((downloaded.saturating_mul(100) / value).min(100)) as u8);
    let _ = app.emit(
        "files-download-progress",
        DownloadProgress {
            url: url.to_string(),
            downloaded,
            total,
            percent,
        },
    );
}

fn safe_file_name(value: &str) -> String {
    let base = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download.bin");
    let cleaned = base
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_string();
    if cleaned.is_empty() {
        "download.bin".to_string()
    } else {
        cleaned
    }
}

fn available_path(directory: &Path, file_name: &str) -> PathBuf {
    let original = directory.join(file_name);
    if !original.exists() {
        return original;
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 1..10_000 {
        let candidate = match extension {
            Some(extension) => directory.join(format!("{stem} ({index}).{extension}")),
            None => directory.join(format!("{stem} ({index})")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{stem}-new"))
}

fn load_catalog(app: &tauri::AppHandle) -> Result<FilesCatalog, String> {
    let cache = cache_path(app)?;
    let online = download_catalog();
    let (content, cached) = match online {
        Ok(content) => {
            if let Some(parent) = cache.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&cache, &content);
            (content, false)
        }
        Err(network_error) => {
            let content = fs::read_to_string(&cache).map_err(|_| {
                format!(
                    "Не удалось загрузить список из Gist и локальный кэш отсутствует: {network_error}"
                )
            })?;
            (content, true)
        }
    };

    parse_catalog(&content, cached)
}

fn download_catalog() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("CrocodileGenaStudio/5.1.0")
        .build()
        .map_err(|error| format!("HTTP-клиент не создан: {error}"))?;
    let refs_url =
        format!("https://gist.github.com/{GIST_ID}.git/info/refs?service=git-upload-pack");
    let refs = client
        .get(refs_url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
        .header(reqwest::header::PRAGMA, "no-cache")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("Не удалось проверить ревизию Gist: {error}"))?
        .text()
        .map_err(|error| format!("Не удалось прочитать ревизию Gist: {error}"))?;
    let revision = parse_head_revision(&refs)
        .ok_or_else(|| "GitHub не вернул текущую ревизию Gist".to_string())?;
    let catalog_url = format!(
        "https://gist.githubusercontent.com/{GIST_OWNER}/{GIST_ID}/raw/{revision}/cracks.json"
    );
    let response = client
        .get(catalog_url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
        .header(reqwest::header::PRAGMA, "no-cache")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("GitHub вернул ошибку: {error}"))?;
    response
        .text()
        .map_err(|error| format!("Ответ Gist не прочитан: {error}"))
}

fn parse_head_revision(advertisement: &str) -> Option<&str> {
    let head = advertisement.find(" HEAD")?;
    if head < 40 {
        return None;
    }
    let revision = &advertisement[head - 40..head];
    (revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(revision)
}

fn parse_catalog(content: &str, cached: bool) -> Result<FilesCatalog, String> {
    let remote: RemoteCatalog = serde_json::from_str(content)
        .map_err(|error| format!("Некорректный cracks.json: {error}"))?;

    let mut files = remote
        .cracks
        .into_iter()
        .filter_map(|item| {
            let name = item.name.trim().to_string();
            let url = item.url.trim().to_string();
            if name.is_empty() || !url.starts_with("https://") {
                return None;
            }
            Some(FileItem {
                name,
                build: item.build.trim().to_string(),
                url,
                password: item.password.trim().to_string(),
                pinned: item.pinned,
            })
        })
        .collect::<Vec<_>>();

    // The Gist is maintained from oldest to newest, so reverse insertion
    // order for the interface: the last added file is shown first.
    files.reverse();
    let total = files.len();
    Ok(FilesCatalog {
        files,
        total,
        cached,
    })
}

fn cache_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("files").join("cracks.json"))
        .map_err(|error| format!("Не удалось определить папку кэша: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{parse_catalog, parse_head_revision};

    #[test]
    fn reads_current_revision_from_git_advertisement() {
        let response = "001e# service=git-upload-pack\n000001591d770c9e1b5c87b59b667c4378310a16bc4191f2 HEAD\0multi_ack";
        assert_eq!(
            parse_head_revision(response),
            Some("1d770c9e1b5c87b59b667c4378310a16bc4191f2")
        );
    }

    #[test]
    fn catalog_is_reversed_to_newest_first() {
        let json = r#"{"cracks":[
          {"name":"Старый","build":"Build 1","url":"https://example.com/1","password":""},
          {"name":"Новый","build":"Build 2","url":"https://example.com/2","password":"pass"}
        ]}"#;
        let result = parse_catalog(json, false).expect("catalog");
        assert_eq!(result.total, 2);
        assert_eq!(result.files[0].name, "Новый");
        assert_eq!(result.files[0].password, "pass");
    }

    #[test]
    fn catalog_skips_entries_without_https_download() {
        let json = r#"{"cracks":[{"name":"Bad","url":"http://example.com/file"}]}"#;
        let result = parse_catalog(json, true).expect("catalog");
        assert_eq!(result.total, 0);
        assert!(result.cached);
    }

    #[test]
    fn catalog_reads_pinned_flag_without_relying_on_name() {
        let json = r#"{"cracks":[
          {"name":"Обычный файл","url":"https://example.com/regular"},
          {"name":"Любое название","url":"https://example.com/pinned","pinned":true}
        ]}"#;
        let result = parse_catalog(json, false).expect("catalog");
        assert!(result.files[0].pinned);
        assert!(!result.files[1].pinned);
    }
}
