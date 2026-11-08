use super::*;
use crate::update_core::{self as core, Outcome, Plan};
use sha2::Digest;
use std::os::windows::process::CommandExt;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use tauri::{Emitter, Manager};

const HELPER: &[u8] = include_bytes!("../../resources/updater/CgsUpdater.exe");
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::SeqCst);
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    phase: &'static str,
    downloaded: u64,
    total: u64,
    percent: u64,
}
fn progress(app: &tauri::AppHandle, phase: &'static str, n: u64, total: u64) {
    let _ = app.emit(
        "cgs-update-progress",
        Progress {
            phase,
            downloaded: n,
            total,
            percent: if total == 0 {
                0
            } else {
                n.saturating_mul(100) / total
            },
        },
    );
}
pub fn cancel() {
    if ACTIVE.load(Ordering::SeqCst) {
        CANCEL.store(true, Ordering::SeqCst);
    }
}
fn check_cancel() -> Result<(), String> {
    if CANCEL.load(Ordering::SeqCst) {
        Err("Обновление отменено; установленная версия не изменена.".into())
    } else {
        Ok(())
    }
}
fn allowed_url(asset: &GithubAsset) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(&asset.browser_download_url).map_err(|e| e.to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url
            .path()
            .starts_with("/LeoBanned/crocodile-gena-studio-releases/releases/download/")
        || url.path_segments().and_then(|mut s| s.next_back()) != Some(asset.name.as_str())
    {
        return Err("Адрес EXE не принадлежит официальному репозиторию обновлений".into());
    }
    Ok(url)
}
async fn interruptible<F: std::future::Future>(
    future: F,
    pulse: impl Fn(),
) -> Result<F::Output, String> {
    use std::task::Poll;
    let mut future = std::pin::pin!(future);
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    std::future::poll_fn(|cx| {
        if let Err(error) = check_cancel() {
            return Poll::Ready(Err(error));
        }
        if let Poll::Ready(value) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(value));
        }
        if tick.poll_tick(cx).is_ready() {
            pulse();
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    })
    .await
}
fn range_matches(
    header: Option<&reqwest::header::HeaderValue>,
    start: u64,
    end: u64,
    total: u64,
) -> bool {
    header
        .and_then(|s| s.to_str().ok())
        .is_some_and(|s| s.trim() == format!("bytes {start}-{end}/{total}"))
}
fn download(
    asset: &GithubAsset,
    path: &Path,
    notify: impl Fn(&'static str, u64, u64),
) -> Result<(), String> {
    tauri::async_runtime::block_on(download_async(asset, path, notify))
}
async fn download_async(
    asset: &GithubAsset,
    path: &Path,
    notify: impl Fn(&'static str, u64, u64),
) -> Result<(), String> {
    use std::{
        io::{Seek, SeekFrom},
        sync::{atomic::AtomicU64, Arc},
    };
    let url = allowed_url(asset)?;
    let total = asset.verification.size;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(600))
        .http1_only()
        .gzip(false)
        .brotli(false)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or_default();
            if attempt.previous().len() < 6
                && attempt.url().scheme() == "https"
                && [
                    "github.com",
                    "release-assets.githubusercontent.com",
                    "github-releases.githubusercontent.com",
                    "objects.githubusercontent.com",
                ]
                .contains(&host)
            {
                attempt.follow()
            } else {
                attempt.error("Недопустимое перенаправление загрузки")
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    notify("downloading", 0, total);
    let probe = interruptible(
        client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send(),
        || notify("downloading", 0, total),
    )
    .await?
    .map_err(|e| format!("Не удалось подключиться к загрузке: {}", e.without_url()))?;
    let supports_ranges = probe.status() == reqwest::StatusCode::PARTIAL_CONTENT
        && range_matches(
            probe.headers().get(reqwest::header::CONTENT_RANGE),
            0,
            0,
            total,
        );
    let resolved = probe.url().clone();
    let etag = probe
        .headers()
        .get(reqwest::header::ETAG)
        .filter(|v| v.to_str().is_ok_and(|s| !s.starts_with("W/")))
        .cloned();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let done = Arc::new(AtomicU64::new(0));
    let mut single_response = None;
    let mut parallel_complete = false;
    if supports_ranges && total >= 8 * 1024 * 1024 {
        // Resolve the CDN once, then use four bounded HTTP/1 connections.
        drop(probe);
        file.set_len(total).map_err(|e| e.to_string())?;
        let mut tasks = tokio::task::JoinSet::new();
        for index in 0..4u64 {
            let start = total * index / 4;
            let end = total * (index + 1) / 4 - 1;
            let client = client.clone();
            let address = resolved.clone();
            let destination = path.to_owned();
            let completed = done.clone();
            let validator = etag.clone();
            tasks.spawn(async move {
                let mut request = client
                    .get(address)
                    .header(reqwest::header::USER_AGENT, USER_AGENT)
                    .header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
                if let Some(etag) = validator {
                    request = request.header(reqwest::header::IF_RANGE, etag);
                }
                let mut response = request
                    .send()
                    .await
                    .map_err(|e| e.without_url().to_string())?;
                if response.status() != reqwest::StatusCode::PARTIAL_CONTENT
                    || !range_matches(
                        response.headers().get(reqwest::header::CONTENT_RANGE),
                        start,
                        end,
                        total,
                    )
                {
                    return Err("Сервер не подтвердил диапазон загрузки".to_string());
                }
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .open(destination)
                    .map_err(|e| e.to_string())?;
                output
                    .seek(SeekFrom::Start(start))
                    .map_err(|e| e.to_string())?;
                let expected = end - start + 1;
                let mut received = 0u64;
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|e| e.without_url().to_string())?
                {
                    check_cancel()?;
                    received += chunk.len() as u64;
                    if received > expected {
                        return Err("Сервер прислал лишние данные диапазона".into());
                    }
                    output.write_all(&chunk).map_err(|e| e.to_string())?;
                    completed.fetch_add(chunk.len() as u64, Ordering::SeqCst);
                }
                if received != expected {
                    return Err("Диапазон EXE скачан не полностью".into());
                }
                Ok::<_, String>(())
            });
        }
        let mut failed = None;
        while !tasks.is_empty() {
            match interruptible(tasks.join_next(), || {
                notify("downloading", done.load(Ordering::SeqCst), total)
            })
            .await
            {
                Ok(Some(Ok(Ok(())))) => (),
                Ok(Some(Ok(Err(error)))) => {
                    failed = Some(error);
                    break;
                }
                Ok(Some(Err(error))) => {
                    failed = Some(error.to_string());
                    break;
                }
                Err(error) => {
                    failed = Some(error);
                    break;
                }
                Ok(None) => break,
            }
        }
        if failed.is_some() {
            tasks.shutdown().await;
            check_cancel()?;
            // Range support is optional. Retry once as a normal verified GET.
            done.store(0, Ordering::SeqCst);
            file.set_len(0).map_err(|e| e.to_string())?;
            notify("retrying", 0, total);
        } else {
            parallel_complete = true;
        }
    } else if probe.status() == reqwest::StatusCode::OK {
        single_response = Some(probe);
    } else {
        if !probe.status().is_success() {
            return Err(format!("Сервер загрузки вернул {}", probe.status()));
        }
        drop(probe);
    }
    if !parallel_complete {
        let mut response = if let Some(response) = single_response {
            response
        } else {
            interruptible(
                client
                    .get(url)
                    .header(reqwest::header::USER_AGENT, USER_AGENT)
                    .send(),
                || notify("downloading", 0, total),
            )
            .await?
            .map_err(|e| format!("Загрузка прервана: {}", e.without_url()))?
        };
        if response.status() != reqwest::StatusCode::OK {
            return Err(format!("Сервер загрузки вернул {}", response.status()));
        }
        if response.content_length().is_some_and(|n| n != total) {
            return Err("Размер ответа не соответствует релизу".into());
        }
        file.set_len(0).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        done.store(0, Ordering::SeqCst);
        loop {
            let chunk = interruptible(response.chunk(), || {
                notify("downloading", done.load(Ordering::SeqCst), total)
            })
            .await?
            .map_err(|e| format!("Загрузка прервана: {}", e.without_url()))?;
            let Some(chunk) = chunk else { break };
            let count = done.fetch_add(chunk.len() as u64, Ordering::SeqCst) + chunk.len() as u64;
            if count > total {
                return Err("Размер загрузки превысил размер релиза".into());
            }
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }
    }
    check_cancel()?;
    file.sync_all().map_err(|e| e.to_string())?;
    if done.load(Ordering::SeqCst) != total {
        return Err("EXE скачан не полностью. Текущая программа не изменена.".into());
    }
    notify("downloading", total, total);
    notify("verifying", total, total);
    check_cancel()?;
    if core::hash(path)? != asset_hash(asset)? {
        return Err("SHA-256 скачанного EXE не совпал с GitHub. Установка отменена.".into());
    }
    Ok(())
}
fn cleanup_later(stage: PathBuf, plan: Plan) {
    std::thread::spawn(move || {
        for _ in 0..120 {
            if !stage.join("result.json").exists() && !stage.join("abort").exists() {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            if let Ok(bytes) = fs::read(stage.join("result.json")) {
                if serde_json::from_slice::<Outcome>(&bytes)
                    .is_ok_and(|r| r.nonce == plan.nonce && r.success)
                    && plan.backup().exists()
                {
                    if core::hash(&plan.backup()).ok().as_deref() != Some(&plan.old_hash) {
                        break; // Preserve diagnostics and an externally changed backup.
                    }
                    if fs::remove_file(plan.backup()).is_err() {
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                }
            }
            if core::cleanup(&stage, &plan).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}
fn launch_helper(stage: &Path, admin: bool, plan_hash: &str) -> Result<(), String> {
    let exe = stage.join("updater.exe");
    if !admin {
        std::process::Command::new(exe)
            .args([stage.as_os_str(), std::ffi::OsStr::new(plan_hash)])
            .current_dir(stage)
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    use windows::{
        core::PCWSTR,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_HIDE},
    };
    let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    let file: Vec<u16> = exe
        .to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let arguments: Vec<u16> = format!(
        "\"{}\" {}",
        stage.to_string_lossy().trim_start_matches("\\\\?\\"),
        plan_hash
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR(arguments.as_ptr()),
            PCWSTR::null(),
            SW_HIDE,
        )
    };
    if result.0 as isize <= 32 {
        return Err("Windows не разрешила запуск обновления с правами администратора. Текущая версия сохранена.".into());
    }
    Ok(())
}
fn fetch_release_interruptible(app: &tauri::AppHandle) -> Result<GithubRelease, String> {
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let pulse = || progress(app, "preparing", 0, 0);
        let response = interruptible(
            client
                .get(LATEST_RELEASE_API)
                .header(reqwest::header::USER_AGENT, USER_AGENT)
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .send(),
            pulse,
        )
        .await?
        .map_err(|e| format!("Не удалось проверить релиз: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!(
                "GitHub вернул {} при проверке релиза",
                response.status()
            ));
        }
        interruptible(response.json::<GithubRelease>(), pulse)
            .await?
            .map_err(|e| format!("Некорректные данные релиза: {}", e.without_url()))
    })
}
pub async fn install(app: tauri::AppHandle, version: String) -> Result<(), String> {
    if ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Обновление уже выполняется".into());
    }
    CANCEL.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move||{
        let _active=Active;
        progress(&app,"preparing",0,0);
        let release=fetch_release_interruptible(&app)?;let info=release_info(&release,env!("CARGO_PKG_VERSION").into())?;
        if !info.available || info.latest_version!=version{return Err("Версия релиза изменилась или обновление не требуется. Проверьте обновления ещё раз.".into())}
        let asset=release.assets.iter().find(|a|canonical_asset(&a.name)).ok_or("Для обновления нужен CrocodileGenaStudio.exe в GitHub Releases")?;
        let new_hash=asset_hash(asset)?;allowed_url(asset)?;check_cancel()?;
        let target=std::env::current_exe().and_then(|p|p.canonicalize()).map_err(|e|e.to_string())?;core::reject_links(&target)?;
        if target.to_string_lossy().starts_with("\\\\?\\UNC\\"){return Err("Перенесите программу с сетевого пути на локальный диск перед обновлением.".into())}
        let admin=match tempfile::NamedTempFile::new_in(target.parent().unwrap()) {
            Ok(_)=>false,Err(e) if e.kind()==std::io::ErrorKind::PermissionDenied=>true,Err(e)=>return Err(format!("Папка программы недоступна: {e}")),
        };
        let root=app.path().app_local_data_dir().map_err(|e|e.to_string())?.join("updates");core::reject_links(&root)?;fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let nonce=uuid::Uuid::new_v4().simple().to_string();let stage=root.join(format!("cgs-update-{nonce}"));fs::create_dir(&stage).map_err(|e|e.to_string())?;
        core::write(&stage.join("owner"),nonce.as_bytes())?;
        let plan=Plan{schema:1,nonce,target,parent_pid:std::process::id(),old_hash:core::hash(&std::env::current_exe().map_err(|e|e.to_string())?)?,new_hash,version};
        let plan_bytes=serde_json::to_vec(&plan).map_err(|e|e.to_string())?;
        let plan_hash=format!("{:x}",sha2::Sha256::digest(&plan_bytes));
        core::write(&stage.join("plan.json"),&plan_bytes)?;
        let prepared=(||{
            download(asset,&stage.join("payload.exe"),|phase,n,total|progress(&app,phase,n,total))?;core::validate_executable(&stage.join("payload.exe"),&plan.version)?;check_cancel()?;
            core::write(&stage.join("updater.exe"),HELPER)?;
            progress(&app,if admin{"permission"}else{"installing"},asset.verification.size,asset.verification.size);
            use std::os::windows::fs::OpenOptionsExt;
            let _helper_lock=fs::OpenOptions::new().read(true).share_mode(1).open(stage.join("updater.exe")).map_err(|e|e.to_string())?;
            if core::hash(&stage.join("updater.exe"))?!=format!("{:x}",sha2::Sha256::digest(HELPER)){return Err("Помощник обновления изменён; установка отменена.".into())}
            launch_helper(&stage,admin,&plan_hash)?;
            let deadline=Instant::now()+Duration::from_secs(120);
            loop {
                check_cancel()?;
                if fs::read_to_string(stage.join("ready")).ok().as_deref()==Some(&plan.nonce){break}
                if let Ok(bytes)=fs::read(stage.join("result.json")){if let Ok(result)=serde_json::from_slice::<Outcome>(&bytes){if result.nonce==plan.nonce{return Err(result.message)}}}
                if Instant::now()>=deadline{return Err("Помощник обновления не подтвердил готовность. Программа не изменена.".into())}
                std::thread::sleep(Duration::from_millis(100));
            }
            progress(&app,"restarting",asset.verification.size,asset.verification.size);
            check_cancel()?;
            core::write(&stage.join("go"),plan.nonce.as_bytes())?;
            app.exit(0);Ok(())
        })();
        if prepared.is_err(){let _=core::write(&stage.join("abort"),plan.nonce.as_bytes());cleanup_later(stage,plan);}
        prepared
    }).await.map_err(|e|{ACTIVE.store(false,Ordering::SeqCst);format!("Обновление прервано: {e}")})?
}

pub fn startup(app: tauri::AppHandle) -> Result<Option<Outcome>, String> {
    let args: Vec<_> = std::env::args_os().collect();
    let marker = args.get(1).and_then(|s| s.to_str());
    if !matches!(marker, Some("--cgs-update-probe" | "--cgs-update-failed")) {
        // Remove only completed owned staging folders; live downloads are left alone.
        if let Ok(root) = app.path().app_local_data_dir() {
            if let Ok(items) = fs::read_dir(root.join("updates")) {
                for item in items.flatten().take(30) {
                    let stage = item.path();
                    if let Ok(plan) = core::read_plan(&stage) {
                        let stale = stage
                            .join("plan.json")
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|age| age > Duration::from_secs(3600))
                            && !core::parent_alive(plan.parent_pid);
                        if plan.target.canonicalize().ok()
                            == std::env::current_exe().and_then(|p| p.canonicalize()).ok()
                            && (stage.join("result.json").is_file() || stale)
                        {
                            if stale {
                                let _ = core::write(&stage.join("abort"), plan.nonce.as_bytes());
                            }
                            cleanup_later(stage, plan);
                        }
                    }
                }
            }
        }
        return Ok(None);
    }
    let stage = PathBuf::from(args.get(2).ok_or("Нет папки обновления")?);
    let plan = core::read_plan(&stage)?;
    if args.get(3).and_then(|v| v.to_str()) != Some(&plan.nonce)
        || std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .map_err(|e| e.to_string())?
            != plan.target.canonicalize().map_err(|e| e.to_string())?
    {
        return Err("Не подтверждён запуск обновлённой программы".into());
    }
    let success = marker == Some("--cgs-update-probe");
    let result = if success {
        if plan.version != env!("CARGO_PKG_VERSION") || core::hash(&plan.target)? != plan.new_hash {
            return Err("Версия запущенной программы не соответствует обновлению".into());
        }
        if fs::read_to_string(stage.join("healthy")).ok().as_deref() != Some(&plan.nonce) {
            core::write(&stage.join("healthy"), plan.nonce.as_bytes())?;
        }
        Outcome {
            nonce: plan.nonce.clone(),
            success: true,
            message: format!("Программа обновлена до {}", plan.version),
        }
    } else {
        if core::hash(&plan.target)? != plan.old_hash {
            return Err("Не подтверждена предыдущая версия программы".into());
        }
        serde_json::from_slice::<Outcome>(
            &fs::read(stage.join("result.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?
    };
    if result.nonce != plan.nonce {
        return Err("Отчёт обновления относится к другой установке".into());
    }
    cleanup_later(stage, plan);
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_headers_must_match_the_requested_slice() {
        let valid = reqwest::header::HeaderValue::from_static("bytes 10-19/100");
        assert!(range_matches(Some(&valid), 10, 19, 100));
        assert!(!range_matches(Some(&valid), 0, 9, 100));
        assert!(!range_matches(Some(&valid), 10, 19, 101));
        assert!(!range_matches(None, 10, 19, 100));
    }
    #[test]
    fn cancellation_interrupts_a_stalled_network_future() {
        CANCEL.store(false, Ordering::SeqCst);
        let start = Instant::now();
        let result = tauri::async_runtime::block_on(async {
            let setter = tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(25)).await;
                CANCEL.store(true, Ordering::SeqCst);
            });
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                interruptible(std::future::pending::<()>(), || {}),
            )
            .await;
            setter.await.unwrap();
            result
        });
        CANCEL.store(false, Ordering::SeqCst);
        assert!(result.unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    #[ignore = "Downloads the published EXE from GitHub for read-only integrity verification; never executes it"]
    fn published_release_download_verifies_without_installing() {
        let release = fetch_release().unwrap();
        let info = release_info(&release, env!("CARGO_PKG_VERSION").into()).unwrap();
        let asset = release
            .assets
            .iter()
            .find(|a| canonical_asset(&a.name))
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        CANCEL.store(false, Ordering::SeqCst);
        let start = Instant::now();
        download(asset, &temp.path().join("published.exe"), |_, _, _| {}).unwrap();
        eprintln!(
            "Verified GitHub download: {} bytes in {:.2} s",
            asset.verification.size,
            start.elapsed().as_secs_f64()
        );
        core::validate_executable(&temp.path().join("published.exe"), &info.latest_version)
            .unwrap();
    }
    #[test]
    fn only_official_asset_urls_are_accepted() {
        let mut asset=GithubAsset{name:"CrocodileGenaStudio.exe".into(),browser_download_url:"https://github.com/LeoBanned/crocodile-gena-studio-releases/releases/download/v5.6.2/CrocodileGenaStudio.exe".into(),verification:AssetVerification{size:50_000_000,digest:Some(format!("sha256:{}","a".repeat(64)))}};
        assert!(allowed_url(&asset).is_ok());
        assert!(asset_hash(&asset).is_ok());
        for url in ["https://example.com/CrocodileGenaStudio.exe","http://github.com/LeoBanned/crocodile-gena-studio-releases/releases/download/v5.6.2/CrocodileGenaStudio.exe","https://github.com/Other/repo/releases/download/v5.6.2/CrocodileGenaStudio.exe"]{asset.browser_download_url=url.into();assert!(allowed_url(&asset).is_err());}
        asset.verification.digest = None;
        assert!(asset_hash(&asset).is_err());
    }
}
