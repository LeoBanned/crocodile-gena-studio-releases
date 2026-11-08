//! Native local Steam Cloud for manifest games. No scheduled backup process:
//! the embedded, scoped CloudRedirect engine runs in Steam via OpenSteamTool.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::{Path, PathBuf}, sync::Mutex};
use toml_edit::DocumentMut;

const LIBRARY: &str = "cgs_cloud_redirect.dll";
const DLL: &[u8] = include_bytes!("../resources/local-cloud/cgs_cloud_redirect.dll");
const STATE: &str = "config/cgs-local-cloud-state.json";
const CONFIG: &str = "config/cgs-local-cloud.json";
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Saved {
    original_toml: String,
    folder: String,
    enabled: bool,
    #[serde(default)]
    library_hash: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    steam_path: String,
    enabled: bool,
    managed: bool,
    folder: Option<String>,
    eligible_apps: Vec<String>,
    loader_supported: bool,
    component_update_available: bool,
    issue: Option<String>,
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    for parent in path.ancestors() { crate::lua_loader::reject_link(parent)?; }
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
fn atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Нет родительской папки")?;
    for p in path.ancestors() { crate::lua_loader::reject_link(p)?; }
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(data).and_then(|_| temp.as_file().sync_all()).map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn saved(steam: &Path) -> Result<Option<Saved>, String> {
    read(&steam.join(STATE))?.map(|data| serde_json::from_slice(&data)
        .map_err(|_| "Повреждены настройки локального облака".into())).transpose()
}
fn toml(steam: &Path) -> Result<(String, DocumentMut), String> {
    let data = read(&steam.join("opensteamtool.toml"))?.unwrap_or_default();
    let text = String::from_utf8(data).map_err(|_| "Конфигурация загрузчика не в UTF-8")?;
    let parsed = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    if parsed.get("cloud").is_some_and(|item| !item.is_table_like()) {
        return Err("Секция cloud в opensteamtool.toml имеет неверный формат.".into());
    }
    Ok((text, parsed))
}
fn eligible(steam: &Path) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for dir in ["config/lua", "config/stplug-in"] {
        if let Ok(files) = fs::read_dir(steam.join(dir)) {
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("lua") { continue; }
                let Some(id) = path.file_stem().and_then(|s| s.to_str()).filter(|s| s.parse::<u32>().is_ok_and(|v| v > 0)) else { continue; };
                let Ok(Some(bytes)) = read(&path) else { continue; };
                let text = String::from_utf8_lossy(&bytes);
                let self_unlock = text.lines().any(|line| {
                    let line: String = line.trim_start_matches('\u{feff}').chars().filter(|c| !c.is_whitespace()).collect();
                    line.starts_with(&format!("addappid({id})")) || line.starts_with(&format!("addappid({id},"))
                });
                if self_unlock && !crate::luatools::parse_manifest_refs(&text).is_empty() {
                    ids.insert(id.to_owned());
                }
            }
        }
    }
    ids.into_iter().collect()
}
fn supported(steam: &Path) -> bool {
    read(&steam.join("OpenSteamTool.dll")).ok().flatten().is_some_and(|bytes|
        bytes.windows(b"CR_InitCloudSave".len()).any(|s| s == b"CR_InitCloudSave") &&
        bytes.windows(b"CR_SetApps".len()).any(|s| s == b"CR_SetApps"))
}
fn status(steam: &Path) -> Result<Status, String> {
    let state = saved(steam)?;
    let (_, doc) = toml(steam)?;
    let enabled = doc.get("cloud").and_then(|s| s.get("enabled")).and_then(|s| s.as_bool()) == Some(true);
    let managed = state.is_some() && doc.get("cloud").and_then(|s| s.get("library")).and_then(|s| s.as_str()) == Some(LIBRARY);
    let loader_supported = supported(steam);
    let installed = read(&steam.join(LIBRARY))?;
    let component_update_available = state.is_some() && loader_supported &&
        (!enabled || managed) && component_update_needed(state.as_ref(), installed.as_deref());
    let issue = if component_update_available {
        Some("Обновите DLL локального облака для корректной синхронизации сохранений.".into())
    } else if enabled && managed && installed.as_deref() != Some(DLL) {
        Some("Файл локального облака изменён извне. Автоматическая замена заблокирована.".into())
    } else if !loader_supported {
        Some("Этот загрузчик не поддерживает CloudRedirect. Выберите совместимый комплект DLL в CrocLua.".into())
    } else { None };
    Ok(Status { steam_path: steam.to_string_lossy().into_owned(), enabled: enabled && managed, managed, folder: state.map(|s| s.folder), eligible_apps: eligible(steam), loader_supported, component_update_available, issue })
}
fn component_update_needed(state: Option<&Saved>, installed: Option<&[u8]>) -> bool {
    let Some(state) = state else { return false; };
    installed != Some(DLL) && installed.is_none_or(|bytes|
        state.library_hash == format!("{:x}", Sha256::digest(bytes)))
}
// Update only the managed component. Do not enable a disabled cloud, migrate
// saves, or regenerate the provider configuration (the storage drive may be offline).
fn update_component(steam: &Path) -> Result<Status, String> {
    let _loader_lock = crate::lua_loader::acquire_file_lock(steam)?;
    let mut state = saved(steam)?.ok_or("Локальное облако ещё не настроено.")?;
    if !status(steam)?.component_update_available {
        if read(&steam.join(LIBRARY))?.as_deref() == Some(DLL) { return status(steam); }
        return Err("Замена DLL заблокирована: установленный файл или настройки изменены извне.".into());
    }
    let before = read(&steam.join(LIBRARY))?;
    if !component_update_needed(Some(&state), before.as_deref()) {
        return Err("DLL изменилась во время проверки; повторите операцию.".into());
    }
    let state_before = read(&steam.join(STATE))?.ok_or("Настройки локального облака отсутствуют.")?;
    let backup = steam.join("config/cgs-local-cloud-backups").join(uuid::Uuid::new_v4().to_string());
    atomic(&backup.join("state.json"), &state_before)?;
    if let Some(bytes) = &before { atomic(&backup.join(LIBRARY), bytes)?; }
    state.library_hash = format!("{:x}", Sha256::digest(DLL));
    let change = (|| {
        atomic(&steam.join(LIBRARY), DLL)?;
        atomic(&steam.join(STATE), &serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?)
    })();
    if let Err(error) = change {
        let rollback = (|| {
            match before {
                Some(bytes) => atomic(&steam.join(LIBRARY), &bytes)?,
                None => { if steam.join(LIBRARY).exists() { fs::remove_file(steam.join(LIBRARY)).map_err(|e| e.to_string())?; } }
            }
            atomic(&steam.join(STATE), &state_before)
        })();
        return Err(match rollback { Ok(()) => error, Err(e) => format!("{error}. Откат: {e}. Копия: {}", backup.display()) });
    }
    status(steam)
}
#[tauri::command]
pub async fn local_cloud_update_component(steam_path: Option<String>) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOCK.lock().map_err(|_| "Обновление локального облака прервано")?;
        let steam = crate::luatools::resolve_steam_path(steam_path).ok_or("Папка Steam не найдена")?;
        if !status(&steam)?.component_update_available {
            return Err("Обновление DLL не требуется или её замена заблокирована. Проверьте локальное облако в CrocLua.".into());
        }
        with_steam_stopped(&steam, false, || update_component(&steam))
    }).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn local_cloud_status(steam_path: Option<String>) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let steam = crate::luatools::resolve_steam_path(steam_path).ok_or("Папка Steam не найдена")?;
        status(&steam)
    }).await.map_err(|e| e.to_string())?
}
fn folder(path: &str, steam: &Path) -> Result<PathBuf, String> {
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() || path.to_string_lossy().starts_with(['\\', '/']) {
        return Err("Выберите папку на локальном диске, например D:\\Game Saves.".into());
    }
    for p in path.ancestors() { crate::lua_loader::reject_link(p)?; }
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let steam = steam.canonicalize().map_err(|e| e.to_string())?;
    if path.starts_with(&steam) || steam.starts_with(&path) {
        return Err("Папка облака должна находиться отдельно от папки Steam.".into());
    }
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        let visible = path.to_string_lossy().trim_start_matches("\\\\?\\").to_owned();
        let root = format!("{}\\", visible.get(..2).ok_or("Неверный путь диска")?);
        let wide: Vec<u16> = std::ffi::OsStr::new(&root).encode_wide().chain(Some(0)).collect();
        let kind = unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(wide.as_ptr())) };
        if kind != 2 && kind != 3 { return Err("Выберите локальный или съёмный диск.".into()); }
    }
    tempfile::NamedTempFile::new_in(&path).map_err(|e| format!("Папка недоступна для записи: {e}"))?;
    Ok(path)
}
// Copies remain in the old folder. Existing different bytes are never overwritten.
fn migrate(old: &Path, new: &Path) -> Result<(), String> {
    let old_canonical = old.canonicalize().map_err(|e| e.to_string())?;
    let new_canonical = new.canonicalize().map_err(|e| e.to_string())?;
    let (old, new) = (old_canonical.as_path(), new_canonical.as_path());
    if old == new { return Ok(()); }
    if old.starts_with(new) || new.starts_with(old) { return Err("Папки хранения не должны быть вложены друг в друга.".into()); }
    let mut files = Vec::new();
    for item in walkdir::WalkDir::new(old).follow_links(false) {
        let item = item.map_err(|e| e.to_string())?;
        crate::lua_loader::reject_link(item.path())?;
        if !item.file_type().is_file() { continue; }
        let destination = new.join(item.path().strip_prefix(old).map_err(|e| e.to_string())?);
        if let Some(bytes) = read(&destination)? {
            if bytes != fs::read(item.path()).map_err(|e| e.to_string())? {
                return Err(format!("В новой папке уже есть другое сохранение: {}. Выберите пустую папку.", destination.display()));
            }
        } else { files.push((item.path().to_owned(), destination)); }
    }
    for (source, target) in files {
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut temp = tempfile::NamedTempFile::new_in(target.parent().unwrap()).map_err(|e| e.to_string())?;
        std::io::copy(&mut fs::File::open(source).map_err(|e| e.to_string())?, &mut temp).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist_noclobber(target).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn apply(steam: &Path, enabled: bool, chosen: Option<String>) -> Result<Status, String> {
    let _loader_lock = crate::lua_loader::acquire_file_lock(steam)?;
    let previous = saved(steam)?;
    let (before, mut doc) = toml(steam)?;
    let own = doc.get("cloud").and_then(|s| s.get("library")).and_then(|s| s.as_str()) == Some(LIBRARY);
    if !own && doc.get("cloud").and_then(|s| s.get("enabled")).and_then(|s| s.as_bool()) == Some(true) {
        return Err("Уже настроен другой CloudRedirect. Сначала отключите его через прежнюю программу.".into());
    }
    let Some(chosen) = chosen.or_else(|| previous.as_ref().map(|s| s.folder.clone())) else { return Err("Сначала выберите папку локального облака.".into()); };
    let target = if enabled { folder(&chosen, steam)? } else { PathBuf::from(&chosen) };
    if enabled && !supported(steam) { return Err("Установленный OpenSteamTool не поддерживает CloudRedirect.".into()); }
    let original = if let Some(s) = previous.as_ref().filter(|s| s.enabled || own) { s.original_toml.clone() } else { before.clone() };
    let cloud_before = read(&steam.join(CONFIG))?;
    let dll_before = read(&steam.join(LIBRARY))?;
    let state_before = read(&steam.join(STATE))?;
    if let Some(old) = previous.as_ref().filter(|_| enabled) {
        let old_path = PathBuf::from(&old.folder);
        if old_path.exists() { migrate(&old_path, &target)?; }
        else if old.folder != target.to_string_lossy() { return Err("Прежняя папка облака недоступна. Подключите диск перед переносом.".into()); }
    }
    if enabled {
        doc["cloud"]["enabled"] = toml_edit::value(true);
        doc["cloud"]["library"] = toml_edit::value(LIBRARY);
    } else if own {
        let original_doc = original.parse::<DocumentMut>().map_err(|e| e.to_string())?;
        if let Some(cloud) = original_doc.get("cloud") { doc["cloud"] = cloud.clone(); }
        else { doc.remove("cloud"); }
    }
    // Back up every changed file before installing. Save data is never deleted.
    let backup = steam.join("config/cgs-local-cloud-backups").join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    for (name, bytes) in [("opensteamtool.toml", Some(before.as_bytes())), ("provider.json", cloud_before.as_deref()), (LIBRARY, dll_before.as_deref()), ("state.json", state_before.as_deref())] {
        if let Some(bytes) = bytes { atomic(&backup.join(name), bytes)?; }
    }
    let change = (|| {
        if enabled {
            if dll_before.as_deref().is_some_and(|b| b != DLL && previous.as_ref().is_none_or(|s| s.library_hash != format!("{:x}", Sha256::digest(b)))) { return Err("Файл cgs_cloud_redirect.dll изменён извне; автоматическая замена отменена.".into()); }
            let config = serde_json::json!({"provider":"folder","sync_path":target.to_string_lossy().trim_start_matches("\\\\?\\"),"sync_achievements":false,"sync_playtime":false,"sync_luas":false,"auto_update_dll":false,"show_non_steam_game":false,"parental_bypass_playtime":false,"parental_ignore_playtime":false});
            atomic(&steam.join(CONFIG), &serde_json::to_vec_pretty(&config).unwrap())?;
            atomic(&steam.join(LIBRARY), DLL)?;
        }
        let library_hash = if enabled || dll_before.as_deref() == Some(DLL) { format!("{:x}", Sha256::digest(DLL)) }
            else { previous.as_ref().map(|s| s.library_hash.clone()).unwrap_or_default() };
        atomic(&steam.join(STATE), &serde_json::to_vec_pretty(&Saved { original_toml: original, folder: target.to_string_lossy().trim_start_matches("\\\\?\\").into(), enabled, library_hash }).unwrap())?;
        let current = fs::read_to_string(steam.join("opensteamtool.toml")).unwrap_or_default();
        if current != before { return Err("Конфигурация Steam изменилась во время настройки. Повторите операцию.".into()); }
        atomic(&steam.join("opensteamtool.toml"), doc.to_string().as_bytes())?;
        Ok::<_,String>(())
    })();
    if let Err(error) = change {
        for (path, bytes) in [(steam.join(CONFIG), cloud_before), (steam.join(LIBRARY), dll_before), (steam.join(STATE), state_before)] {
            let result = match bytes { Some(b) => atomic(&path, &b), None if path.exists() => fs::remove_file(path).map_err(|e| e.to_string()), None => Ok(()) };
            if let Err(rollback) = result { return Err(format!("{error}. Откат: {rollback}. Копия: {}", backup.display())); }
        }
        return Err(error);
    }
    status(steam)
}

#[cfg(windows)]
fn with_steam_stopped<T>(steam: &Path, start_if_closed: bool, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use std::os::windows::process::CommandExt;
    let client = steam.join("steam.exe");
    if !client.is_file() { return Err("В выбранной папке не найден steam.exe.".into()); }
    let running = crate::hypervisor::process_name_running("steam.exe")?;
    for id in eligible(steam) {
        if winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(format!("Software\\Valve\\Steam\\Apps\\{id}")).ok().and_then(|key| key.get_value::<u32,_>("Running").ok()) == Some(1) {
            return Err("Закройте игры перед изменением локального облака.".into());
        }
    }
    crate::steam_hv::restart_around(running, || {
        std::process::Command::new(&client).arg("-shutdown").creation_flags(0x08000000).spawn().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while crate::hypervisor::process_name_running("steam.exe")? {
            if std::time::Instant::now() >= deadline { return Err("Steam не завершился за 30 секунд; настройки не изменены.".into()); }
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        Ok(())
    }, || { crate::hypervisor::require_steam_closed()?; work() }, || {
        if !running && !start_if_closed { return Ok(()); }
        std::process::Command::new(&client).current_dir(steam).spawn().map(|_|()).map_err(|e| e.to_string())
    })
}
#[cfg(not(windows))]
fn with_steam_stopped<T>(_steam: &Path, _start_if_closed: bool, _work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    Err("Локальное облако пока поддерживается только в Windows.".into())
}
#[tauri::command]
pub async fn local_cloud_set(steam_path: Option<String>, enabled: bool, folder: Option<String>) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOCK.lock().map_err(|_| "Настройка локального облака прервана")?;
        let steam = crate::luatools::resolve_steam_path(steam_path).ok_or("Папка Steam не найдена")?;
        #[cfg(windows)] {
            let client = steam.join("steam.exe");
            if !client.is_file() { return Err("В выбранной папке не найден steam.exe.".into()); }

            // Validate path/loader before asking the client to exit.
            if enabled {
                let chosen = folder.clone().or(saved(&steam)?.map(|s| s.folder)).ok_or("Сначала выберите папку локального облака")?;
                self::folder(&chosen, &steam)?;
                if !supported(&steam) { return Err("Этот OpenSteamTool не поддерживает CloudRedirect.".into()); }
            }
            with_steam_stopped(&steam, true, || apply(&steam, enabled, folder))
        }
        #[cfg(not(windows))] { let _ = (steam, enabled, folder); Err("Локальное облако пока поддерживается только в Windows.".into()) }
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_update_preserves_disabled_cloud_and_offline_storage_and_backs_up() {
        let temp = tempfile::tempdir().unwrap(); let steam = temp.path().join("Steam");
        fs::create_dir_all(steam.join("config")).unwrap();
        let old = b"previous managed CloudRedirect";
        fs::write(steam.join(LIBRARY), old).unwrap();
        fs::write(steam.join("OpenSteamTool.dll"), b"CR_InitCloudSave\0CR_SetApps").unwrap();
        let config = b"[lua]\npaths=['config/lua']\n";
        fs::write(steam.join("opensteamtool.toml"), config).unwrap();
        let provider = b"{\"provider\":\"folder\",\"sync_path\":\"Z:/offline-saves\"}";
        fs::write(steam.join(CONFIG), provider).unwrap();
        let state = Saved { original_toml: String::from_utf8(config.to_vec()).unwrap(), folder: "Z:/offline-saves".into(), enabled: false,
            library_hash: format!("{:x}", Sha256::digest(old)) };
        atomic(&steam.join(STATE), &serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(status(&steam).unwrap().component_update_available);
        let result = update_component(&steam).unwrap();
        assert!(!result.enabled && !result.component_update_available);
        assert_eq!(result.folder.as_deref(), Some("Z:/offline-saves"));
        assert_eq!(fs::read(steam.join(LIBRARY)).unwrap(), DLL);
        assert_eq!(fs::read(steam.join(CONFIG)).unwrap(), provider);
        assert_eq!(fs::read(steam.join("opensteamtool.toml")).unwrap(), config);
        let backup = fs::read_dir(steam.join("config/cgs-local-cloud-backups")).unwrap().next().unwrap().unwrap().path();
        assert_eq!(fs::read(backup.join(LIBRARY)).unwrap(), old);
        assert!(!update_component(&steam).unwrap().component_update_available);
        fs::write(steam.join(LIBRARY), b"external modification").unwrap();
        assert!(!status(&steam).unwrap().component_update_available);
        assert!(update_component(&steam).is_err());
        assert_eq!(fs::read(steam.join(LIBRARY)).unwrap(), b"external modification");
        fs::remove_file(steam.join(LIBRARY)).unwrap();
        assert!(status(&steam).unwrap().component_update_available);
        assert!(!update_component(&steam).unwrap().component_update_available);
    }
    #[test]
    fn disabling_an_old_component_retains_its_hash_for_the_next_upgrade() {
        let temp = tempfile::tempdir().unwrap(); let steam = temp.path().join("Steam");
        let storage = temp.path().join("Saves"); fs::create_dir(&steam).unwrap();
        fs::write(steam.join("OpenSteamTool.dll"), b"CR_InitCloudSave\0CR_SetApps").unwrap();
        apply(&steam, true, Some(storage.to_string_lossy().into())).unwrap();
        fs::write(storage.join("slot"), b"progress").unwrap();
        let old = b"old managed component"; fs::write(steam.join(LIBRARY), old).unwrap();
        let mut state = saved(&steam).unwrap().unwrap(); state.library_hash = format!("{:x}", Sha256::digest(old));
        atomic(&steam.join(STATE), &serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(!apply(&steam, false, None).unwrap().enabled);
        assert_eq!(saved(&steam).unwrap().unwrap().library_hash, format!("{:x}", Sha256::digest(old)));
        assert!(status(&steam).unwrap().component_update_available);
        update_component(&steam).unwrap();
        assert!(apply(&steam, true, None).unwrap().enabled);
        assert_eq!(fs::read(storage.join("slot")).unwrap(), b"progress");
        // Disabling must not turn an externally modified DLL into a trusted one.
        fs::write(steam.join(LIBRARY), b"external component").unwrap();
        apply(&steam, false, None).unwrap();
        assert!(!status(&steam).unwrap().component_update_available);
        assert!(apply(&steam, true, None).is_err());
        assert_eq!(fs::read(steam.join(LIBRARY)).unwrap(), b"external component");
        assert!(!status(&temp.path().join("unconfigured")).unwrap().component_update_available);
    }
    #[test]
    fn only_root_lua_with_manifest_reference_is_eligible_even_after_cache_eviction() {
        let temp = tempfile::tempdir().unwrap(); let root = temp.path();
        fs::create_dir_all(root.join("config/lua")).unwrap(); fs::create_dir(root.join("depotcache")).unwrap();
        fs::write(root.join("config/lua/10.lua"), "addappid(10)\n--setManifestid(11,\"123\")").unwrap();
        fs::write(root.join("config/lua/20.lua"), "addappid(21)\nsetManifestid(11,\"123\")").unwrap();
        fs::write(root.join("config/lua/30.lua.disabled"), "addappid(30)\nsetManifestid(11,\"123\")").unwrap();
        fs::write(root.join("config/lua/40.lua"), "addappid(40)").unwrap();
        assert_eq!(eligible(root), vec!["10"]);
        fs::write(root.join("depotcache/11_123.manifest"), b"manifest").unwrap();
        assert_eq!(eligible(root), vec!["10"]);
    }
    #[test]
    fn migration_preserves_source_and_refuses_conflicts() {
        let temp = tempfile::tempdir().unwrap(); let a=temp.path().join("a"); let b=temp.path().join("b");
        fs::create_dir(&a).unwrap(); fs::create_dir(&b).unwrap(); fs::write(a.join("slot"),b"new").unwrap(); fs::write(b.join("slot"),b"old").unwrap();
        assert!(migrate(&a,&b).is_err()); assert_eq!(fs::read(b.join("slot")).unwrap(),b"old");
        fs::remove_file(b.join("slot")).unwrap(); migrate(&a,&b).unwrap();
        assert_eq!(fs::read(a.join("slot")).unwrap(),b"new"); assert_eq!(fs::read(b.join("slot")).unwrap(),b"new");
        assert!(migrate(&a,&a.join("nested")).is_err());
    }
    #[test]
    fn toggle_restores_original_cloud_preserves_lua_and_keeps_saves() {
        let temp=tempfile::tempdir().unwrap(); let steam=temp.path().join("Steam"); let storage=temp.path().join("Saves");
        fs::create_dir(&steam).unwrap();
        let original="[lua]\npaths = ['config/lua']\n[other]\nflag = true\n[cloud]\nenabled = false\nlibrary = 'custom.dll'\n";
        fs::write(steam.join("opensteamtool.toml"),original).unwrap();
        fs::write(steam.join("OpenSteamTool.dll"),b"CR_InitCloudSave\0CR_SetApps").unwrap();
        let enabled=apply(&steam,true,Some(storage.to_string_lossy().into())).unwrap();
        assert!(enabled.enabled); assert!(enabled.managed);
        let config:serde_json::Value=serde_json::from_slice(&fs::read(steam.join(CONFIG)).unwrap()).unwrap();
        assert_eq!(config["provider"],"folder"); assert_eq!(config["auto_update_dll"],false);
        fs::write(storage.join("slot"),b"save").unwrap();
        let disabled=apply(&steam,false,None).unwrap(); assert!(!disabled.enabled);
        let (_,doc)=toml(&steam).unwrap(); assert_eq!(doc["cloud"]["library"].as_str(),Some("custom.dll"));
        assert_eq!(doc["cloud"]["enabled"].as_bool(),Some(false)); assert_eq!(doc["other"]["flag"].as_bool(),Some(true));
        assert_eq!(doc["lua"]["paths"][0].as_str(),Some("config/lua"));
        assert_eq!(fs::read(storage.join("slot")).unwrap(),b"save");
        assert!(apply(&steam,true,None).unwrap().enabled);
        // Disabling still works if the user disconnected the storage drive.
        let mut state=saved(&steam).unwrap().unwrap(); state.folder="Z:\\unavailable-cgs-cloud".into();
        atomic(&steam.join(STATE),&serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(!apply(&steam,false,None).unwrap().enabled);
    }
    #[test]
    fn external_cloud_and_malformed_config_are_not_overwritten() {
        let temp=tempfile::tempdir().unwrap(); let steam=temp.path().join("Steam"); fs::create_dir(&steam).unwrap();
        let original="[cloud]\nenabled = true\nlibrary = 'external.dll'\n";
        fs::write(steam.join("opensteamtool.toml"),original).unwrap();
        assert!(apply(&steam,true,Some(temp.path().join("Saves").to_string_lossy().into())).is_err());
        assert_eq!(fs::read_to_string(steam.join("opensteamtool.toml")).unwrap(),original);
        assert!(!steam.join(STATE).exists());
        fs::write(steam.join("opensteamtool.toml"),"cloud = true").unwrap();
        assert!(toml(&steam).is_err());
    }
}
