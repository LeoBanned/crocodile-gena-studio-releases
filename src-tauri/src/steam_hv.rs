//! Experimental Steam Play integration. Edit only one LaunchOptions value, while Steam is closed.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn restart_around<T>(
    was_running: bool,
    stop: impl FnOnce() -> Result<(), String>,
    change: impl FnOnce() -> Result<T, String>,
    start: impl FnOnce() -> Result<(), String>,
) -> Result<T, String> {
    if was_running {
        stop()?;
    }
    let result = change();
    if was_running || result.is_ok() {
        if let Err(error) = start() {
            return Err(match &result {
                Ok(_) => format!("Параметры сохранены, но Steam не удалось открыть: {error}"),
                Err(original) => format!("{original}. Также не удалось открыть Steam: {error}"),
            });
        }
    }
    result
}

#[derive(Serialize)]
pub(crate) struct Status {
    pub appid: u64,
    pub account: String,
    pub enabled: bool,
    pub managed: bool,
    pub command: String,
}
#[derive(Serialize)]
pub(crate) struct Account {
    pub id: String,
    pub name: String,
    pub preferred: bool,
}
#[derive(Serialize, Deserialize)]
struct Backup {
    original: Option<String>,
    applied: String,
}

// Keep every byte outside the edited string: Steam's other settings and comments survive.
struct Entry {
    key: String,
    key_start: usize,
    start: usize,
    end: usize,
    value: Option<String>,
    children: Vec<Entry>,
    close: usize,
}
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
fn entries(text: &str) -> Result<Vec<Entry>, String> {
    fn space(b: &[u8], p: &mut usize) {
        loop {
            while *p < b.len() && b[*p].is_ascii_whitespace() {
                *p += 1;
            }
            if b.get(*p..*p + 2) == Some(b"//") {
                while *p < b.len() && b[*p] != b'\n' {
                    *p += 1;
                }
            } else {
                break;
            }
        }
    }
    fn string(t: &str, p: &mut usize) -> Result<String, String> {
        let b = t.as_bytes();
        if b.get(*p) != Some(&b'"') {
            return Err("Неожиданный формат Steam VDF; файл не изменён.".into());
        }
        *p += 1;
        let mut out = Vec::new();
        while *p < b.len() {
            let c = b[*p];
            *p += 1;
            if c == b'"' {
                return String::from_utf8(out).map_err(|e| e.to_string());
            }
            if c == b'\\' && matches!(b.get(*p), Some(b'\\' | b'"')) {
                out.push(b[*p]);
                *p += 1;
            } else {
                out.push(c);
            }
        }
        Err("Незавершённая строка Steam VDF.".into())
    }
    fn object(
        t: &str,
        p: &mut usize,
        nested: bool,
        depth: usize,
    ) -> Result<(Vec<Entry>, usize), String> {
        if depth > 64 {
            return Err("Слишком глубокий VDF.".into());
        }
        let b = t.as_bytes();
        let mut result = Vec::new();
        loop {
            space(b, p);
            if *p == b.len() {
                return if nested {
                    Err("Незавершённый VDF.".into())
                } else {
                    Ok((result, *p))
                };
            }
            if b[*p] == b'}' {
                let close = *p;
                *p += 1;
                return if nested {
                    Ok((result, close))
                } else {
                    Err("Лишняя скобка VDF.".into())
                };
            }
            let key_start = *p;
            let key = string(t, p)?;
            space(b, p);
            let start = *p;
            let (value, children, close) = if b.get(*p) == Some(&b'{') {
                *p += 1;
                let (c, end) = object(t, p, true, depth + 1)?;
                (None, c, end)
            } else {
                (Some(string(t, p)?), vec![], 0)
            };
            result.push(Entry {
                key,
                key_start,
                start,
                end: *p,
                value,
                children,
                close,
            });
        }
    }
    let mut p = if text.starts_with('\u{feff}') { 3 } else { 0 };
    Ok(object(text, &mut p, false, 0)?.0)
}
fn find<'a>(items: &'a [Entry], key: &str) -> Result<Option<&'a Entry>, String> {
    let mut matches = items.iter().filter(|e| e.key.eq_ignore_ascii_case(key));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(format!("Повторяющийся ключ VDF: {key}"));
    }
    Ok(first)
}
fn path_for(appid: u64) -> Vec<String> {
    vec!["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]
        .into_iter()
        .map(String::from)
        .chain([appid.to_string(), "LaunchOptions".into()])
        .collect()
}
fn get(text: &str, path: &[String]) -> Result<Option<String>, String> {
    let parsed = entries(text)?;
    let mut items = parsed.as_slice();
    for (i, key) in path.iter().enumerate() {
        let Some(e) = find(items, key)? else {
            return Ok(None);
        };
        if i + 1 == path.len() {
            return e
                .value
                .clone()
                .map(Some)
                .ok_or("LaunchOptions оказался секцией".into());
        }
        if e.value.is_some() {
            return Err(format!("Ожидалась секция {key}"));
        }
        items = &e.children;
    }
    Ok(None)
}
fn edit(text: &str, path: &[String], value: Option<&str>) -> Result<String, String> {
    let parsed = entries(text)?;
    let mut items = parsed.as_slice();
    let mut close = None;
    for (i, key) in path.iter().enumerate() {
        if let Some(e) = find(items, key)? {
            if i + 1 == path.len() {
                if e.value.is_none() {
                    return Err("Некорректный LaunchOptions".into());
                }
                let replacement = value.map(quoted).unwrap_or_default();
                let mut out = text.to_string();
                if value.is_some() {
                    out.replace_range(e.start..e.end, &replacement)
                } else {
                    out.replace_range(e.key_start..e.end, "");
                }
                return Ok(out);
            }
            if e.value.is_some() {
                return Err(format!("Ожидалась секция {key}"));
            }
            items = &e.children;
            close = Some(e.close);
        } else {
            let Some(value) = value else {
                return Ok(text.into());
            };
            // The root must exist; never synthesize a new localconfig file.
            let at = close.ok_or("Не найдена корневая секция настроек Steam")?;
            let mut added = format!("{}\t\t{}\n", quoted(path.last().unwrap()), quoted(value));
            for parent in path[i..path.len() - 1].iter().rev() {
                added = format!("{}\n{{\n{added}}}\n", quoted(parent));
            }
            let mut out = text.to_string();
            out.insert_str(at, &format!("\n{added}"));
            return Ok(out);
        }
    }
    Err("Пустой путь VDF".into())
}
#[cfg(test)]
fn config_path(steam: &Path) -> Result<(PathBuf, String), String> {
    config_path_for(steam, None)
}
pub(crate) fn accounts(steam: &Path) -> Result<Vec<Account>, String> {
    let text = fs::read_to_string(steam.join("config/loginusers.vdf"))
        .map_err(|_| "Сначала войдите в аккаунт Steam на этом ПК.")?;
    let data = crate::steam_library::parse_vdf(&text)?;
    let users = data
        .get("users")
        .and_then(|v| v.as_object())
        .ok_or("Не найдены аккаунты Steam")?;
    let active = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam\\ActiveProcess")
        .ok()
        .and_then(|key| key.get_value::<u32, _>("ActiveUser").ok())
        .filter(|id| *id != 0);
    let mut result = Vec::new();
    for (steam_id, value) in users {
        let Some(id) = steam_id
            .parse::<u64>()
            .ok()
            .and_then(|v| v.checked_sub(76561197960265728))
            .filter(|v| *v <= u32::MAX as u64)
        else {
            continue;
        };
        let id_text = id.to_string();
        if !steam
            .join("userdata")
            .join(&id_text)
            .join("config/localconfig.vdf")
            .is_file()
        {
            continue;
        }
        result.push(Account {
            id: id_text,
            name: value
                .get("personaname")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Steam")
                .into(),
            preferred: active == Some(id as u32),
        });
    }
    if !result.iter().any(|a| a.preferred) {
        for item in &mut result {
            item.preferred = users
                .get(&(item.id.parse::<u64>().unwrap() + 76561197960265728).to_string())
                .and_then(|v| v.get("mostrecent"))
                .and_then(|v| v.as_str())
                == Some("1");
        }
    }
    if result.len() == 1 {
        result[0].preferred = true;
    }
    if result.is_empty() {
        return Err(
            "Не найдены локальные настройки аккаунтов Steam. Войдите в Steam на этом ПК.".into(),
        );
    }
    Ok(result)
}
fn config_path_for(steam: &Path, requested: Option<&str>) -> Result<(PathBuf, String), String> {
    let list = accounts(steam)?;
    let preferred: Vec<_> = list.iter().filter(|a| a.preferred).collect();
    let selected = if let Some(id) = requested {
        list.iter()
            .find(|a| a.id == id)
            .ok_or("Выбранный аккаунт не найден в Steam")?
    } else if preferred.len() == 1 {
        preferred[0]
    } else {
        return Err("Выберите аккаунт Steam в списке.".into());
    };
    let account = selected.id.clone();
    let path = steam
        .join("userdata")
        .join(&account)
        .join("config/localconfig.vdf");
    for ancestor in path.ancestors().take_while(|p| p.starts_with(steam)) {
        crate::lua_loader::reject_link(ancestor)?;
    }
    if !path.is_file() {
        return Err("Локальные настройки аккаунта Steam ещё не созданы.".into());
    }
    Ok((path, account))
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("Нет папки настроек")?)
        .map_err(|e| e.to_string())?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn backup_path(config: &Path, appid: u64) -> PathBuf {
    config
        .parent()
        .unwrap()
        .join(format!("cgs-hv-launch-{appid}.json"))
}
pub(crate) fn status_for(
    steam: &Path,
    appid: u64,
    helper: &Path,
    requested: Option<&str>,
) -> Result<Status, String> {
    let (config, account) = config_path_for(steam, requested)?;
    let text = fs::read_to_string(&config).map_err(|e| e.to_string())?;
    let current = get(&text, &path_for(appid))?;
    let backup = backup_path(&config, appid);
    let saved: Option<Backup> = if backup.exists() {
        Some(
            serde_json::from_slice(&fs::read(&backup).map_err(|e| e.to_string())?)
                .map_err(|_| "Повреждена резервная копия параметров запуска")?,
        )
    } else {
        None
    };
    let command = format!("\"{}\" %command%", helper.display());
    Ok(Status {
        appid,
        account,
        enabled: saved
            .as_ref()
            .is_some_and(|s| current.as_deref() == Some(&s.applied)),
        managed: saved.is_some(),
        command,
    })
}
pub(crate) fn set(
    steam: &Path,
    appid: u64,
    helper: &Path,
    enabled: bool,
    account: Option<&str>,
) -> Result<Status, String> {
    set_account_checked(
        steam,
        appid,
        helper,
        enabled,
        account,
        crate::hypervisor::require_steam_closed,
    )
}
#[cfg(test)]
fn set_checked(
    steam: &Path,
    appid: u64,
    helper: &Path,
    enabled: bool,
    closed: impl Fn() -> Result<(), String>,
) -> Result<Status, String> {
    set_account_checked(steam, appid, helper, enabled, None, closed)
}
fn set_account_checked(
    steam: &Path,
    appid: u64,
    helper: &Path,
    enabled: bool,
    requested: Option<&str>,
    closed: impl Fn() -> Result<(), String>,
) -> Result<Status, String> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().map_err(|_| "Задача настройки Steam прервана")?;
    closed()?;
    let (config, _) = config_path_for(steam, requested)?;
    let path = path_for(appid);
    let before = fs::read_to_string(&config).map_err(|e| e.to_string())?;
    let current = get(&before, &path)?;
    let backup = backup_path(&config, appid);
    crate::lua_loader::reject_link(&backup)?;
    let saved: Option<Backup> = if backup.exists() {
        Some(
            serde_json::from_slice(&fs::read(&backup).map_err(|e| e.to_string())?)
                .map_err(|_| "Повреждена резервная копия")?,
        )
    } else {
        None
    };
    let command = format!("\"{}\" %command%", helper.display());
    if command.contains(['\n', '\r']) || helper.to_string_lossy().contains(['"', '%']) {
        return Err("Путь лаунчера содержит неподдерживаемые символы".into());
    }
    if let Some(saved) = &saved {
        if current.as_deref() != Some(&saved.applied)
            && current != saved.original
            && !(current.as_deref() == Some("") && saved.original.is_none())
        {
            return Err(
                "Параметры запуска изменены вне программы. Автоматическая перезапись отменена."
                    .into(),
            );
        }
    } else if !enabled {
        return status_for(steam, appid, helper, requested);
    } else if current
        .as_ref()
        .is_some_and(|v| v.contains("%command%") || v.contains("Crocodile-HV"))
    {
        return Err(
            "У игры уже задан собственный лаунчер. Сначала проверьте параметры запуска в Steam."
                .into(),
        );
    }
    let original = saved
        .as_ref()
        .map(|s| s.original.clone())
        .unwrap_or(current.clone());
    let applied = if let Some(args) = original.as_ref().filter(|s| !s.trim().is_empty()) {
        format!("{command} {args}")
    } else {
        command
    };
    if enabled && saved.as_ref().is_some_and(|s| s.applied != applied) {
        return Err("Путь лаунчера изменился. Сначала верните обычный запуск, затем включите настройку заново.".into());
    }
    let target = if enabled {
        Some(applied.as_str())
    } else {
        original.as_deref()
    };
    let changed = edit(&before, &path, target)?;
    // Save recovery data before the atomic config replacement.
    if saved.is_none() {
        let full = config
            .parent()
            .unwrap()
            .join(format!("cgs-hv-launch-{appid}-before.vdf"));
        if !full.exists() {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&full)
                .map_err(|e| e.to_string())?;
            f.write_all(before.as_bytes()).map_err(|e| e.to_string())?;
        }
        atomic(
            &backup,
            &serde_json::to_vec(&Backup {
                original: original.clone(),
                applied: applied.clone(),
            })
            .unwrap(),
        )?;
    }
    closed()?;
    if fs::read_to_string(&config).map_err(|e| e.to_string())? != before {
        return Err(
            "Настройки Steam изменились во время операции. Повторите после закрытия Steam.".into(),
        );
    }
    atomic(&config, changed.as_bytes())?;
    if !enabled {
        fs::remove_file(&backup).map_err(|e| e.to_string())?;
    }
    status_for(steam, appid, helper, requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_restart_orders_shutdown_write_start_and_recovers_on_error() {
        let events = std::cell::RefCell::new(Vec::new());
        restart_around(
            true,
            || {
                events.borrow_mut().push("stop");
                Ok(())
            },
            || {
                events.borrow_mut().push("write");
                Ok(())
            },
            || {
                events.borrow_mut().push("start");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), vec!["stop", "write", "start"]);
        events.borrow_mut().clear();
        assert!(restart_around::<()>(
            true,
            || {
                events.borrow_mut().push("stop");
                Ok(())
            },
            || {
                events.borrow_mut().push("write-error");
                Err("failure".into())
            },
            || {
                events.borrow_mut().push("start");
                Ok(())
            }
        )
        .is_err());
        assert_eq!(*events.borrow(), vec!["stop", "write-error", "start"]);
    }
    #[test]
    fn shutdown_failure_never_writes_or_forces_a_restart() {
        assert!(restart_around::<()>(
            true,
            || Err("timeout".into()),
            || panic!("must not write"),
            || panic!("must not restart")
        )
        .is_err());
        assert!(restart_around(false, || panic!("not running"), || Ok(()), || Ok(())).is_ok());
    }
    #[test]
    fn actual_config_is_parseable_without_writing() {
        let Some(steam) = crate::luatools::resolve_steam_path(None) else {
            return;
        };
        let Ok((config, _)) = config_path(&steam) else {
            return;
        };
        let before = fs::read(&config).unwrap();
        let text = std::str::from_utf8(&before).unwrap();
        let edited = edit(text, &path_for(4294967294), Some("test-only-never-written")).unwrap();
        assert_eq!(
            get(&edited, &path_for(4294967294)).unwrap().as_deref(),
            Some("test-only-never-written")
        );
        assert_eq!(fs::read(&config).unwrap(), before);
    }
    #[test]
    fn edits_only_target_value_and_preserves_escaping() {
        let text="\u{feff}\"UserLocalConfigStore\"\n{\n// keep\n\"Software\"{\"Valve\"{\"Steam\"{\"apps\"{\"42\"{\"LaunchOptions\" \"-dx11\" \"Other\" \"A\\\\B\"}}}}}\n}";
        let command = "\"E:\\Игры\\A B\\Crocodile-HV.exe\" %command% -dx11";
        let changed = edit(text, &path_for(42), Some(command)).unwrap();
        assert_eq!(
            get(&changed, &path_for(42)).unwrap().as_deref(),
            Some(command)
        );
        assert_eq!(edit(&changed, &path_for(42), Some("-dx11")).unwrap(), text);
    }
    #[test]
    fn creates_missing_app_without_touching_siblings() {
        let text="\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" {\"1\"{\"Other\"\"keep\"}}}}}}";
        let changed = edit(text, &path_for(42), Some("test")).unwrap();
        assert!(changed.contains("\"1\"{\"Other\"\"keep\"}"));
        assert_eq!(
            get(&changed, &path_for(42)).unwrap().as_deref(),
            Some("test")
        );
        assert!(get(text, &path_for(42)).unwrap().is_none());
    }
    #[test]
    fn refuses_duplicate_keys() {
        assert!(get(
            "\"UserLocalConfigStore\"{}\"UserLocalConfigStore\"{}",
            &path_for(1)
        )
        .is_err());
    }
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::create_dir_all(dir.path().join("userdata/42/config")).unwrap();
        fs::write(
            dir.path().join("config/loginusers.vdf"),
            "\"users\"{\"76561197960265770\"{\"MostRecent\"\"1\"}}",
        )
        .unwrap();
        fs::write(dir.path().join("userdata/42/config/localconfig.vdf"),"\"UserLocalConfigStore\"{\"Software\"{\"Valve\"{\"Steam\"{\"apps\"{\"7\"{\"LaunchOptions\"\"-dx11\"}\"8\"{\"Unrelated\"\"keep\"}}}}}}").unwrap();
        dir
    }
    #[test]
    fn explicit_account_resolves_multiple_users_without_mostrecent() {
        let dir = fixture();
        let root = dir.path();
        let first = root.join("userdata/42/config/localconfig.vdf");
        let before = fs::read(&first).unwrap();
        fs::create_dir_all(root.join("userdata/98/config")).unwrap();
        fs::write(root.join("userdata/98/config/localconfig.vdf"), &before).unwrap();
        fs::write(root.join("config/loginusers.vdf"),"\"users\"{\"76561197960265770\"{\"PersonaName\"\"First\"}\"76561197960265826\"{\"PersonaName\"\"Second\"}}").unwrap();
        assert_eq!(accounts(root).unwrap().len(), 2);
        assert!(config_path_for(root, None).is_err());
        assert!(config_path_for(root, Some("../42")).is_err());
        let status = set_account_checked(
            root,
            7,
            &root.join("Crocodile-HV.exe"),
            true,
            Some("98"),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(status.account, "98");
        assert!(status.enabled);
        assert_eq!(fs::read(&first).unwrap(), before);
        set_account_checked(
            root,
            7,
            &root.join("Crocodile-HV.exe"),
            false,
            Some("98"),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(
            fs::read(root.join("userdata/98/config/localconfig.vdf")).unwrap(),
            before
        );
    }
    #[test]
    fn enables_idempotently_and_restores_previous_options() {
        let dir = fixture();
        let root = dir.path();
        let helper = root.join("Game Folder/Crocodile-HV.exe");
        let config = root.join("userdata/42/config/localconfig.vdf");
        let before = fs::read(&config).unwrap();
        assert!(
            set_checked(root, 7, &helper, true, || Ok(()))
                .unwrap()
                .enabled
        );
        let first = fs::read(&config).unwrap();
        assert!(String::from_utf8(first.clone()).unwrap().contains("-dx11"));
        assert!(
            set_checked(root, 7, &helper, true, || Ok(()))
                .unwrap()
                .enabled
        );
        assert_eq!(fs::read(&config).unwrap(), first);
        assert!(
            !set_checked(root, 7, &helper, false, || Ok(()))
                .unwrap()
                .managed
        );
        assert_eq!(fs::read(&config).unwrap(), before);
    }
    #[test]
    fn refuses_external_edits_and_running_steam() {
        let dir = fixture();
        let root = dir.path();
        let helper = root.join("Crocodile-HV.exe");
        let config = root.join("userdata/42/config/localconfig.vdf");
        let before = fs::read(&config).unwrap();
        assert!(set_checked(root, 7, &helper, true, || Err("Steam running".into())).is_err());
        assert_eq!(fs::read(&config).unwrap(), before);
        set_checked(root, 7, &helper, true, || Ok(())).unwrap();
        let external = edit(
            &fs::read_to_string(&config).unwrap(),
            &path_for(7),
            Some("-user-change"),
        )
        .unwrap();
        fs::write(&config, &external).unwrap();
        assert!(set_checked(root, 7, &helper, false, || Ok(())).is_err());
        assert_eq!(fs::read_to_string(&config).unwrap(), external);
    }
    #[test]
    fn restores_absent_launch_options_without_touching_other_games() {
        let dir = fixture();
        let root = dir.path();
        let helper = root.join("Crocodile-HV.exe");
        set_checked(root, 8, &helper, true, || Ok(())).unwrap();
        set_checked(root, 8, &helper, false, || Ok(())).unwrap();
        let text = fs::read_to_string(root.join("userdata/42/config/localconfig.vdf")).unwrap();
        assert_eq!(get(&text, &path_for(8)).unwrap(), None);
        assert_eq!(get(&text, &path_for(7)).unwrap().as_deref(), Some("-dx11"));
    }
}
