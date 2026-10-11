use crate::{engine, models::OperationResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const SOURCE64: &[u8] = include_bytes!("../../SmokeAPI/SmokeAPI64.dll");
const SOURCE32: &[u8] = include_bytes!("../../SmokeAPI/SmokeAPI86.dll");
const CONFIG: &str = "SmokeAPI.config.json";
const JOURNAL: &str = ".cgs-smaki.json";
const PROXIES: &[&str] = &["version.dll", "winhttp.dll", "winmm.dll"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameExecutable {
    pub path: String,
    pub relative_path: String,
    pub bitness: String,
    pub recommended: bool,
    pub proxy: Option<String>,
    pub configured_dlcs: BTreeMap<String, String>,
    pub unlock_all: bool,
}

#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    proxy: String,
    exe: String,
    config: String,
    original_config: Option<Vec<u8>>,
    #[serde(default)]
    bitness: String,
}

fn source_for(bitness: &str) -> Result<&'static [u8], String> {
    match bitness {
        "x64" => Ok(SOURCE64),
        "x86" => Ok(SOURCE32),
        _ => Err("SmokeAPI поддерживает только x64 и x86 EXE".into()),
    }
}

fn journal_source(dir: &Path, journal: &Journal) -> Result<&'static [u8], String> {
    if !journal.bitness.is_empty() {
        return source_for(&journal.bitness);
    }
    let bitness = pe_bitness(&dir.join(&journal.proxy))
        .ok_or("Не удалось определить архитектуру установленной прокси-DLL")?;
    source_for(bitness)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SmakiRequest {
    pub game_dir: String,
    pub exe_path: String,
    pub proxy: String,
    pub appid: u64,
    pub dlcs: BTreeMap<String, String>,
    pub unlock_all: bool,
}

pub(crate) fn pe_bitness(path: &Path) -> Option<&'static str> {
    let mut file = fs::File::open(path).ok()?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).ok()?;
    if &dos[..2] != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().ok()?) as u64;
    if offset < 64 || offset > file.metadata().ok()?.len().saturating_sub(6) {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut pe = [0u8; 6];
    file.read_exact(&mut pe).ok()?;
    if &pe[..4] != b"PE\0\0" {
        return None;
    }
    match u16::from_le_bytes([pe[4], pe[5]]) {
        0x8664 => Some("x64"),
        0x14c => Some("x86"),
        0xaa64 => Some("ARM64"),
        _ => None,
    }
}

fn read_journal(dir: &Path) -> Result<Option<Journal>, String> {
    safe_file(dir, JOURNAL)?;
    let path = dir.join(JOURNAL);
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(&path).map_err(|e| e.to_string())?;
    let journal: Journal = serde_json::from_slice(&data)
        .map_err(|_| "Повреждена запись установки SmokeAPI. Файлы не изменены.")?;
    if journal.version != 1 || !PROXIES.contains(&journal.proxy.as_str()) {
        return Err("Неизвестная запись установки SmokeAPI".into());
    }
    Ok(Some(journal))
}

fn safe_file(dir: &Path, name: &str) -> Result<(), String> {
    match fs::symlink_metadata(dir.join(name)) {
        Ok(meta) if !meta.file_type().is_file() => Err(format!(
            "{name}: путь занят папкой или ссылкой. Изменение отменено."
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn executables(game: &Path) -> Vec<GameExecutable> {
    let mut candidates = Vec::new();
    for item in engine::walk(game) {
        if !item.file_type().is_file() || engine::ignored_distribution(item.path()) {
            continue;
        }
        if !item
            .path()
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            continue;
        }
        let name = item.file_name().to_string_lossy().to_lowercase();
        if [
            "unins",
            "crash",
            "setup",
            "installer",
            "vcredist",
            "dxsetup",
            "helper",
            "reporter",
            "prereq",
            "cefprocess",
            "server",
        ]
        .iter()
        .any(|part| name.contains(part))
        {
            continue;
        }
        let Some(bitness) = pe_bitness(item.path()) else {
            continue;
        };
        let relative = engine::display_relative(item.path(), game);
        if relative.to_lowercase().split(['\\', '/']).any(|p| {
            matches!(
                p,
                "redist" | "thirdparty" | "extras" | "engine" | "plugins" | ".cgs-backup"
            )
        }) {
            continue;
        }
        let dir = item.path().parent().unwrap();
        let journal = read_journal(dir).ok().flatten();
        let config = journal
            .as_ref()
            .and_then(|j| serde_json::from_str::<serde_json::Value>(&j.config).ok());
        let dlcs = config
            .as_ref()
            .and_then(|c| c.get("extra_dlcs"))
            .and_then(|c| c.as_object())
            .and_then(|apps| apps.values().next())
            .and_then(|c| c.get("dlcs"))
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();
        let score = if name.contains("shipping") {
            100
        } else if engine::find_case_insensitive(dir, "steam_api64.dll").is_some()
            || engine::find_case_insensitive(dir, "steam_api.dll").is_some()
        {
            80
        } else if name.contains("launcher") {
            10
        } else if dir == game {
            60
        } else {
            40
        };
        candidates.push((
            score,
            GameExecutable {
                path: item.path().display().to_string(),
                relative_path: relative,
                bitness: bitness.into(),
                recommended: false,
                proxy: journal
                    .filter(|j| dir.join(&j.proxy).is_file())
                    .map(|j| j.proxy),
                configured_dlcs: dlcs,
                unlock_all: config
                    .as_ref()
                    .and_then(|v| v.get("default_app_status"))
                    .and_then(|v| v.as_str())
                    == Some("unlocked"),
            },
        ));
    }
    candidates.sort_by(|(a, ae), (b, be)| b.cmp(a).then(ae.relative_path.cmp(&be.relative_path)));
    let mut exes: Vec<_> = candidates.into_iter().map(|(_, exe)| exe).collect();
    if let Some(first) = exes.first_mut() {
        first.recommended = true;
    }
    exes
}

pub(crate) fn has_install(game: &Path) -> bool {
    engine::walk(game).any(|e| e.file_type().is_file() && e.file_name() == JOURNAL)
}

fn validated_dir(game_dir: &str, exe_path: &str) -> Result<PathBuf, String> {
    let game = fs::canonicalize(game_dir).map_err(|_| "Папка игры не найдена")?;
    let exe = fs::canonicalize(exe_path).map_err(|_| "EXE игры не найден")?;
    if !exe.starts_with(&game)
        || !exe.is_file()
        || !exe
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
    {
        return Err("Выберите EXE внутри папки выбранной игры".into());
    }
    Ok(exe.parent().ok_or("Папка EXE не найдена")?.to_path_buf())
}

fn build_config(request: &SmakiRequest) -> Result<String, String> {
    if request.appid == 0
        || request
            .dlcs
            .keys()
            .any(|id| id.parse::<u64>().ok().filter(|n| *n > 0).is_none())
    {
        return Err("Некорректный AppID игры или DLC".into());
    }
    if !request.unlock_all && request.dlcs.is_empty() {
        return Err("Выберите DLC или включите «Разблокировать все»".into());
    }
    let overrides: BTreeMap<_, _> = request.dlcs.keys().map(|id| (id, "unlocked")).collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "$schema":"https://raw.githubusercontent.com/acidicoala/SmokeAPI/refs/tags/v4.0.0/res/SmokeAPI.schema.json", "$version":4,
        "logging":false, "log_steam_http":false,
        "default_app_status": if request.unlock_all { "unlocked" } else { "locked" },
        "override_app_status":{}, "override_dlc_status":overrides,
        "auto_inject_inventory":true, "extra_inventory_items":[],
        "extra_dlcs":{request.appid.to_string():{"dlcs":request.dlcs}}
    })).map_err(|e| e.to_string())
}

// Only owned files may be changed or removed. Never replace an existing game's
// version/winhttp/winmm DLL, including an unrelated mod using the same proxy.
fn verify_owned(dir: &Path, journal: &Journal) -> Result<(), String> {
    safe_file(dir, &journal.proxy)?;
    safe_file(dir, CONFIG)?;
    let proxy = dir.join(&journal.proxy);
    if proxy.exists()
        && fs::read(&proxy).map_err(|e| e.to_string())? != journal_source(dir, journal)?
    {
        return Err("Прокси-DLL изменён другой программой. Автоматическая замена отменена.".into());
    }
    let config = dir.join(CONFIG);
    if config.exists() && fs::read(&config).map_err(|e| e.to_string())? != journal.config.as_bytes()
    {
        return Err("Конфигурация SmokeAPI изменена вручную. Сохраните её и удалите вручную перед изменением установки.".into());
    }
    Ok(())
}

fn install(request: &SmakiRequest) -> Result<String, String> {
    if !PROXIES.contains(&request.proxy.as_str()) {
        return Err("Допустимы только version.dll, winhttp.dll или winmm.dll".into());
    }
    let dir = validated_dir(&request.game_dir, &request.exe_path)?;
    let bitness = pe_bitness(Path::new(&request.exe_path))
        .ok_or("Не удалось определить архитектуру выбранного EXE")?;
    let source = source_for(bitness)?;
    if engine::walk(Path::new(&request.game_dir)).any(|e| {
        e.file_type().is_file()
            && e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("cream_api.ini")
    }) {
        return Err(
            "Сначала удалите CreamAPI кнопкой «Удалить анлокер», чтобы не смешивать два способа."
                .into(),
        );
    }
    let old = read_journal(&dir)?;
    if let Some(journal) = &old {
        verify_owned(&dir, journal)?;
    }
    let proxy = dir.join(&request.proxy);
    safe_file(&dir, &request.proxy)?;
    safe_file(&dir, CONFIG)?;
    if proxy.exists() && old.as_ref().is_none_or(|j| j.proxy != request.proxy) {
        return Err(format!(
            "{} уже существует рядом с EXE. Выберите другой прокси — чужой файл не будет заменён.",
            request.proxy
        ));
    }
    let config = build_config(request)?;
    let config_path = dir.join(CONFIG);
    let journal_path = dir.join(JOURNAL);
    let original_config = if let Some(old) = &old {
        old.original_config.clone()
    } else if config_path.exists() {
        Some(fs::read(&config_path).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let journal = Journal {
        version: 1,
        proxy: request.proxy.clone(),
        exe: Path::new(&request.exe_path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into(),
        config: config.clone(),
        original_config,
        bitness: bitness.into(),
    };
    let journal_data = serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?;
    let mut paths = vec![proxy.clone(), config_path.clone(), journal_path.clone()];
    if let Some(old) = &old {
        paths.push(dir.join(&old.proxy));
    }
    paths.sort();
    paths.dedup();
    let before: Vec<_> = paths
        .into_iter()
        .map(|p| {
            let bytes = if p.exists() {
                Some(fs::read(&p).map_err(|e| e.to_string())?)
            } else {
                None
            };
            Ok((p, bytes))
        })
        .collect::<Result<_, String>>()?;
    let result = (|| -> Result<(), String> {
        fs::write(&proxy, source).map_err(|e| format!("Не удалось записать DLL: {e}"))?;
        if fs::read(&proxy).map_err(|e| e.to_string())? != source {
            return Err("Проверка DLL не пройдена".into());
        }
        fs::write(&config_path, &config)
            .map_err(|e| format!("Не удалось записать конфигурацию: {e}"))?;
        fs::write(&journal_path, journal_data)
            .map_err(|e| format!("Не удалось сохранить запись установки: {e}"))?;
        if let Some(old) = &old {
            if old.proxy != request.proxy && dir.join(&old.proxy).exists() {
                fs::remove_file(dir.join(&old.proxy)).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (path, bytes) in before {
            let restored = if let Some(bytes) = bytes {
                fs::write(&path, bytes)
            } else if path.exists() {
                fs::remove_file(&path)
            } else {
                Ok(())
            };
            if let Err(e) = restored {
                failures.push(format!("{}: {e}", path.display()));
            }
        }
        return Err(if failures.is_empty() {
            error
        } else {
            format!(
                "{error}. Не удалось завершить откат: {}",
                failures.join("; ")
            )
        });
    }
    Ok(format!(
        "SmokeAPI установлен: {}. Steam API не изменён.",
        proxy.display()
    ))
}

fn uninstall(game_dir: &str, exe_path: &str) -> Result<String, String> {
    let dir = validated_dir(game_dir, exe_path)?;
    let Some(journal) = read_journal(&dir)? else {
        return Err("Установка SmokeAPI рядом с выбранным EXE не найдена".into());
    };
    verify_owned(&dir, &journal)?;
    let proxy = dir.join(&journal.proxy);
    let config = dir.join(CONFIG);
    let record = dir.join(JOURNAL);
    let before = [proxy.clone(), config.clone(), record.clone()]
        .into_iter()
        .map(|path| {
            let bytes = if path.exists() {
                Some(fs::read(&path).map_err(|e| e.to_string())?)
            } else {
                None
            };
            Ok((path, bytes))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let result = (|| -> Result<(), String> {
        if proxy.exists() {
            fs::remove_file(&proxy).map_err(|e| format!("Не удалось удалить DLL: {e}"))?;
        }
        if let Some(bytes) = journal.original_config {
            fs::write(&config, bytes).map_err(|e| e.to_string())?;
        } else if config.exists() {
            fs::remove_file(&config).map_err(|e| e.to_string())?;
        }
        fs::remove_file(&record).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (path, bytes) in before {
            let restored = if let Some(bytes) = bytes {
                fs::write(&path, bytes)
            } else if path.exists() {
                fs::remove_file(&path)
            } else {
                Ok(())
            };
            if let Err(e) = restored {
                failures.push(format!("{}: {e}", path.display()));
            }
        }
        return Err(if failures.is_empty() {
            error
        } else {
            format!(
                "{error}. Не удалось завершить откат: {}",
                failures.join("; ")
            )
        });
    }
    Ok("SmokeAPI удалён. Библиотеки Steam не изменялись.".into())
}

#[tauri::command]
pub(crate) async fn install_smaki(request: SmakiRequest) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || install(&request)).await {
        Ok(Ok(message)) => OperationResult::ok(message, vec![]),
        Ok(Err(error)) => OperationResult::error(error, vec![]),
        Err(error) => OperationResult::error(error.to_string(), vec![]),
    }
}

#[tauri::command]
pub(crate) async fn uninstall_smaki(game_dir: String, exe_path: String) -> OperationResult {
    match tauri::async_runtime::spawn_blocking(move || uninstall(&game_dir, &exe_path)).await {
        Ok(Ok(message)) => OperationResult::ok(message, vec![]),
        Ok(Err(error)) => OperationResult::error(error, vec![]),
        Err(error) => OperationResult::error(error.to_string(), vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, SmakiRequest) {
        let game = std::env::temp_dir().join(format!(
            "cgs-smaki-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir = game.join("Game/Binaries/Win64");
        fs::create_dir_all(&dir).unwrap();
        let mut exe = vec![0u8; 128];
        exe[..2].copy_from_slice(b"MZ");
        exe[60..64].copy_from_slice(&64u32.to_le_bytes());
        exe[64..70].copy_from_slice(b"PE\0\0\x64\x86");
        fs::write(dir.join("Game-Win64-Shipping.exe"), exe).unwrap();
        fs::write(dir.join("steam_api64.dll"), b"VALVE-ORIGINAL").unwrap();
        let request = SmakiRequest {
            game_dir: game.display().to_string(),
            exe_path: dir.join("Game-Win64-Shipping.exe").display().to_string(),
            proxy: "version.dll".into(),
            appid: 10,
            dlcs: BTreeMap::from([("11".into(), "Pack".into())]),
            unlock_all: false,
        };
        (game, request)
    }
    #[test]
    fn install_and_switch_proxy_next_to_nested_exe_without_touching_steam_api() {
        let (game, mut request) = fixture();
        let dir = Path::new(&request.exe_path).parent().unwrap().to_path_buf();
        fs::write(dir.join(CONFIG), b"original config").unwrap();
        install(&request).unwrap();
        assert!(!game.join("version.dll").exists());
        assert_eq!(fs::read(dir.join("version.dll")).unwrap(), SOURCE64);
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(CONFIG)).unwrap()).unwrap();
        assert_eq!(config["default_app_status"], "locked");
        assert_eq!(config["override_dlc_status"]["11"], "unlocked");
        assert_eq!(config["extra_dlcs"]["10"]["dlcs"]["11"], "Pack");
        request.proxy = "winmm.dll".into();
        install(&request).unwrap();
        assert!(!dir.join("version.dll").exists());
        assert!(dir.join("winmm.dll").is_file());
        uninstall(&request.game_dir, &request.exe_path).unwrap();
        assert!(!dir.join("winmm.dll").exists());
        assert_eq!(fs::read(dir.join(CONFIG)).unwrap(), b"original config");
        assert_eq!(
            fs::read(dir.join("steam_api64.dll")).unwrap(),
            b"VALVE-ORIGINAL"
        );
        fs::remove_dir_all(game).unwrap();
    }
    #[test]
    fn rejects_collisions_invalid_proxy_and_outside_exe_but_supports_x86() {
        let (game, mut request) = fixture();
        let dir = Path::new(&request.exe_path).parent().unwrap().to_path_buf();
        fs::write(dir.join("version.dll"), b"another mod").unwrap();
        assert!(install(&request).unwrap_err().contains("уже существует"));
        assert_eq!(fs::read(dir.join("version.dll")).unwrap(), b"another mod");
        request.proxy = "../wrong.dll".into();
        assert!(install(&request).is_err());
        request.proxy = "winhttp.dll".into();
        let old = request.game_dir.clone();
        request.game_dir = dir.join("other").display().to_string();
        fs::create_dir_all(&request.game_dir).unwrap();
        assert!(install(&request).is_err());
        request.game_dir = old;
        let mut exe = fs::read(&request.exe_path).unwrap();
        exe[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
        fs::write(&request.exe_path, exe).unwrap();
        install(&request).unwrap();
        assert_eq!(fs::read(dir.join("winhttp.dll")).unwrap(), SOURCE32);
        uninstall(&request.game_dir, &request.exe_path).unwrap();
        fs::remove_dir_all(game).unwrap();
    }
    #[test]
    fn changed_owned_files_are_not_deleted() {
        let (game, request) = fixture();
        install(&request).unwrap();
        let dir = Path::new(&request.exe_path).parent().unwrap();
        fs::write(dir.join("version.dll"), b"replaced by other mod").unwrap();
        assert!(uninstall(&request.game_dir, &request.exe_path).is_err());
        assert!(dir.join(JOURNAL).exists());
        fs::remove_dir_all(game).unwrap();
    }
}
