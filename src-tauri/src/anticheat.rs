use serde::Serialize;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use winreg::{
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
    RegKey,
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Copy)]
struct PathProbe {
    variable: &'static str,
    suffix: &'static str,
}

#[derive(Clone, Copy)]
struct RegistryProbe {
    hive: &'static str,
    path: &'static str,
}

#[derive(Clone, Copy)]
struct Definition {
    id: &'static str,
    name: &'static str,
    vendor: &'static str,
    keywords: &'static [&'static str],
    services: &'static [&'static str],
    paths: &'static [PathProbe],
    registry: &'static [RegistryProbe],
}

const DEFINITIONS: &[Definition] = &[
    Definition {
        id: "easy-anticheat",
        name: "Easy Anti-Cheat (EAC)",
        vendor: "Epic Games",
        keywords: &["easy anti-cheat", "easyanticheat"],
        services: &["EasyAntiCheat", "EasyAntiCheat_EOS", "EasyAntiCheat*"],
        paths: &[
            PathProbe {
                variable: "ProgramFiles(x86)",
                suffix: "EasyAntiCheat",
            },
            PathProbe {
                variable: "ProgramFiles(x86)",
                suffix: "EasyAntiCheat_EOS",
            },
            PathProbe {
                variable: "ProgramFiles",
                suffix: "EasyAntiCheat",
            },
            PathProbe {
                variable: "ProgramData",
                suffix: "EasyAntiCheat",
            },
        ],
        registry: &[
            RegistryProbe {
                hive: "HKLM",
                path: r"SOFTWARE\EasyAntiCheat",
            },
            RegistryProbe {
                hive: "HKLM",
                path: r"SOFTWARE\WOW6432Node\EasyAntiCheat",
            },
        ],
    },
    Definition {
        id: "battleye",
        name: "BattlEye",
        vendor: "BattlEye Innovations",
        keywords: &["battleye"],
        services: &["BEService", "BEService_*"],
        paths: &[
            PathProbe {
                variable: "ProgramFiles(x86)",
                suffix: r"Common Files\BattlEye",
            },
            PathProbe {
                variable: "ProgramFiles",
                suffix: r"Common Files\BattlEye",
            },
            PathProbe {
                variable: "ProgramData",
                suffix: "BattlEye",
            },
        ],
        registry: &[],
    },
    Definition {
        id: "riot-vanguard",
        name: "Riot Vanguard",
        vendor: "Riot Games",
        keywords: &["riot vanguard", "vanguard"],
        services: &["vgc", "vgk"],
        paths: &[PathProbe {
            variable: "ProgramFiles",
            suffix: "Riot Vanguard",
        }],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\Riot Games\Riot Vanguard",
        }],
    },
    Definition {
        id: "faceit",
        name: "FACEIT Anti-Cheat",
        vendor: "FACEIT",
        keywords: &["faceit anti-cheat", "faceit anticheat"],
        services: &["FACEIT*"],
        paths: &[
            PathProbe {
                variable: "ProgramFiles",
                suffix: "FACEIT AC",
            },
            PathProbe {
                variable: "ProgramFiles(x86)",
                suffix: "FACEIT AC",
            },
            PathProbe {
                variable: "ProgramData",
                suffix: "FACEIT",
            },
        ],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\FACEIT",
        }],
    },
    Definition {
        id: "ea-anticheat",
        name: "EA AntiCheat",
        vendor: "Electronic Arts",
        keywords: &["ea anticheat", "ea anti-cheat"],
        services: &["EAAntiCheat*"],
        paths: &[
            PathProbe {
                variable: "ProgramFiles",
                suffix: r"EA\AC",
            },
            PathProbe {
                variable: "ProgramData",
                suffix: "EAanticheat",
            },
        ],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\EA AntiCheat",
        }],
    },
    Definition {
        id: "punkbuster",
        name: "PunkBuster",
        vendor: "Even Balance",
        keywords: &["punkbuster", "punk buster"],
        services: &["PnkBstrA", "PnkBstrB"],
        paths: &[
            PathProbe {
                variable: "WINDIR",
                suffix: r"System32\PnkBstrA.exe",
            },
            PathProbe {
                variable: "WINDIR",
                suffix: r"System32\PnkBstrB.exe",
            },
            PathProbe {
                variable: "WINDIR",
                suffix: r"SysWOW64\PnkBstrA.exe",
            },
            PathProbe {
                variable: "WINDIR",
                suffix: r"SysWOW64\PnkBstrB.exe",
            },
        ],
        registry: &[],
    },
    Definition {
        id: "xigncode3",
        name: "XIGNCODE3",
        vendor: "Wellbia",
        keywords: &["xigncode", "wellbia"],
        services: &["xhunter1", "xhunter*"],
        paths: &[PathProbe {
            variable: "WINDIR",
            suffix: r"System32\drivers\xhunter1.sys",
        }],
        registry: &[],
    },
    Definition {
        id: "gameguard",
        name: "nProtect GameGuard",
        vendor: "INCA Internet",
        keywords: &["nprotect gameguard", "gameguard"],
        services: &["npggsvc", "GameGuard*"],
        paths: &[PathProbe {
            variable: "ProgramData",
            suffix: "nProtect",
        }],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\INCAInternet",
        }],
    },
    Definition {
        id: "ace",
        name: "Anti-Cheat Expert (ACE / Tencent)",
        vendor: "Tencent",
        keywords: &["anti-cheat expert", "anticheatexpert", "tencent ace"],
        services: &["ace-base", "ace-game", "AntiCheatExpert*"],
        paths: &[
            PathProbe {
                variable: "WINDIR",
                suffix: r"System32\drivers\ace-base.sys",
            },
            PathProbe {
                variable: "WINDIR",
                suffix: r"System32\drivers\ace-game.sys",
            },
            PathProbe {
                variable: "ProgramData",
                suffix: "AntiCheatExpert",
            },
        ],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\AntiCheatExpert",
        }],
    },
    Definition {
        id: "trueprotect",
        name: "TrueProtect / MoMo",
        vendor: "NetEase / Emulator components",
        keywords: &["trueprotect", "momo anti-cheat"],
        services: &["TrueProtect*"],
        paths: &[PathProbe {
            variable: "ProgramData",
            suffix: "TrueProtect",
        }],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\TrueProtect",
        }],
    },
    Definition {
        id: "denuvo-anticheat",
        name: "Denuvo Anti-Cheat",
        vendor: "Irdeto",
        keywords: &["denuvo anti-cheat", "denuvo anticheat"],
        services: &["DenuvoAntiCheat*"],
        paths: &[
            PathProbe {
                variable: "ProgramFiles",
                suffix: "Denuvo Anti-Cheat",
            },
            PathProbe {
                variable: "ProgramData",
                suffix: "Denuvo Anti-Cheat",
            },
        ],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\Denuvo Anti-Cheat",
        }],
    },
    Definition {
        id: "equ8",
        name: "EQU8 Anti-Cheat",
        vendor: "EQU8",
        keywords: &["equ8 anti-cheat", "equ8"],
        services: &["EQU8*"],
        paths: &[
            PathProbe {
                variable: "ProgramFiles",
                suffix: "EQU8",
            },
            PathProbe {
                variable: "ProgramData",
                suffix: "EQU8",
            },
        ],
        registry: &[RegistryProbe {
            hive: "HKLM",
            path: r"SOFTWARE\EQU8",
        }],
    },
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AntiCheatEvidence {
    kind: String,
    path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AntiCheatItem {
    id: String,
    name: String,
    vendor: String,
    detected: bool,
    version: Option<String>,
    evidence: Vec<AntiCheatEvidence>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AntiCheatReport {
    items: Vec<AntiCheatItem>,
    detected: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AntiCheatRemovalResult {
    success: bool,
    message: String,
    removed: usize,
    failed: Vec<String>,
    backup_dir: Option<String>,
    restart_required: bool,
}

#[derive(Clone)]
enum Target {
    Service(String),
    Registry(String),
    Path(PathBuf),
}

#[derive(Clone)]
struct Detection {
    item: AntiCheatItem,
    targets: Vec<Target>,
}

#[tauri::command]
pub(crate) async fn anticheat_scan() -> Result<AntiCheatReport, String> {
    tauri::async_runtime::spawn_blocking(scan_report)
        .await
        .map_err(|error| format!("Сканирование античитов прервано: {error}"))?
}

#[tauri::command]
pub(crate) async fn anticheat_remove(
    id: String,
    backup_dir: String,
) -> Result<AntiCheatRemovalResult, String> {
    tauri::async_runtime::spawn_blocking(move || remove_selected(Some(&id), Path::new(&backup_dir)))
        .await
        .map_err(|error| format!("Удаление античита прервано: {error}"))?
}

#[tauri::command]
pub(crate) async fn anticheat_remove_all(
    backup_dir: String,
) -> Result<AntiCheatRemovalResult, String> {
    tauri::async_runtime::spawn_blocking(move || remove_selected(None, Path::new(&backup_dir)))
        .await
        .map_err(|error| format!("Удаление античитов прервано: {error}"))?
}

#[tauri::command]
pub(crate) async fn anticheat_pick_backup_directory(
    window: tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    #[cfg(windows)]
    let owner = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    #[cfg(not(windows))]
    let owner = {
        let _ = window;
        0
    };
    tauri::async_runtime::spawn_blocking(move || {
        crate::native_dialog::pick_folder("Выберите папку для резервной копии античитов", owner)
    })
    .await
    .map_err(|error| format!("Выбор папки прерван: {error}"))?
}

fn scan_report() -> Result<AntiCheatReport, String> {
    let detections = scan_internal()?;
    let detected = detections
        .iter()
        .filter(|entry| entry.item.detected)
        .count();
    Ok(AntiCheatReport {
        items: detections.into_iter().map(|entry| entry.item).collect(),
        detected,
    })
}

fn scan_internal() -> Result<Vec<Detection>, String> {
    let uninstall_entries = read_uninstall_entries();
    let services = read_services();
    let mut result = Vec::with_capacity(DEFINITIONS.len());

    for definition in DEFINITIONS {
        let mut evidence = Vec::new();
        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        let mut version = None;

        for entry in &uninstall_entries {
            let searchable = format!("{} {}", entry.display_name, entry.publisher).to_lowercase();
            if definition
                .keywords
                .iter()
                .any(|word| searchable.contains(word))
            {
                add_evidence(&mut evidence, &mut seen, "ПРОГРАММА", &entry.display_name);
                targets.push(Target::Registry(entry.registry_path.clone()));
                if version.is_none() && !entry.version.is_empty() {
                    version = Some(entry.version.clone());
                }
            }
        }

        for service in &services {
            if definition
                .services
                .iter()
                .any(|pattern| service_matches(&service.name, pattern))
            {
                let display = if service.display_name.is_empty() {
                    service.name.clone()
                } else {
                    format!("{} ({})", service.name, service.display_name)
                };
                add_evidence(
                    &mut evidence,
                    &mut seen,
                    "РЕЕСТР",
                    &format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{display}"),
                );
                targets.push(Target::Service(service.name.clone()));
                if let Some(path) = &service.image_path {
                    if path.exists() {
                        add_evidence(&mut evidence, &mut seen, "ФАЙЛ", &path.to_string_lossy());
                        targets.push(Target::Path(path.clone()));
                    }
                }
            }
        }

        for probe in definition.paths {
            if let Some(path) = expand_probe(*probe) {
                if path.exists() {
                    let kind = if path.is_dir() {
                        "ПАПКА"
                    } else {
                        "ФАЙЛ"
                    };
                    add_evidence(&mut evidence, &mut seen, kind, &path.to_string_lossy());
                    targets.push(Target::Path(path));
                }
            }
        }

        for probe in definition.registry {
            let full = format!(r"{}\{}", probe.hive, probe.path);
            if registry_key_exists(&full) {
                add_evidence(&mut evidence, &mut seen, "РЕЕСТР", &full);
                targets.push(Target::Registry(full));
            }
        }

        deduplicate_targets(&mut targets);
        result.push(Detection {
            item: AntiCheatItem {
                id: definition.id.to_string(),
                name: definition.name.to_string(),
                vendor: definition.vendor.to_string(),
                detected: !evidence.is_empty(),
                version,
                evidence,
            },
            targets,
        });
    }
    Ok(result)
}

#[derive(Default)]
struct UninstallEntry {
    display_name: String,
    publisher: String,
    version: String,
    registry_path: String,
}

fn read_uninstall_entries() -> Vec<UninstallEntry> {
    let locations = [
        (HKEY_LOCAL_MACHINE, "HKLM", KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, "HKLM", KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, "HKCU", KEY_WOW64_64KEY),
        (HKEY_CURRENT_USER, "HKCU", KEY_WOW64_32KEY),
    ];
    let base = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for (hive, label, view) in locations {
        let root = RegKey::predef(hive);
        let Ok(uninstall) = root.open_subkey_with_flags(base, KEY_READ | view) else {
            continue;
        };
        for key_name in uninstall.enum_keys().filter_map(Result::ok) {
            let Ok(key) = uninstall.open_subkey_with_flags(&key_name, KEY_READ | view) else {
                continue;
            };
            let display_name: String = key.get_value("DisplayName").unwrap_or_default();
            if display_name.trim().is_empty() {
                continue;
            }
            let registry_path = format!(r"{label}\{base}\{key_name}");
            if !seen.insert(registry_path.to_lowercase()) {
                continue;
            }
            entries.push(UninstallEntry {
                display_name,
                publisher: key.get_value("Publisher").unwrap_or_default(),
                version: key.get_value("DisplayVersion").unwrap_or_default(),
                registry_path,
            });
        }
    }
    entries
}

struct ServiceEntry {
    name: String,
    display_name: String,
    image_path: Option<PathBuf>,
}

fn read_services() -> Vec<ServiceEntry> {
    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(services) = root.open_subkey_with_flags(
        r"SYSTEM\CurrentControlSet\Services",
        KEY_READ | KEY_WOW64_64KEY,
    ) else {
        return Vec::new();
    };
    services
        .enum_keys()
        .filter_map(Result::ok)
        .filter_map(|name| {
            let key = services.open_subkey_with_flags(&name, KEY_READ).ok()?;
            let display_name = key.get_value("DisplayName").unwrap_or_default();
            let raw_image: String = key.get_value("ImagePath").unwrap_or_default();
            Some(ServiceEntry {
                name,
                display_name,
                image_path: service_image_path(&raw_image),
            })
        })
        .collect()
}

fn service_matches(name: &str, pattern: &str) -> bool {
    let name = name.to_lowercase();
    let pattern = pattern.to_lowercase();
    if let Some(prefix) = pattern.strip_suffix('*') {
        name.starts_with(prefix)
    } else {
        name == pattern
    }
}

fn service_image_path(raw: &str) -> Option<PathBuf> {
    let expanded = expand_environment(raw.trim().trim_start_matches(r"\??\"));
    let value = expanded.trim();
    let executable = if let Some(rest) = value.strip_prefix('"') {
        rest.split('"').next().unwrap_or_default()
    } else {
        let lower = value.to_lowercase();
        let end = lower
            .find(".sys")
            .map(|index| index + 4)
            .or_else(|| lower.find(".exe").map(|index| index + 4))?;
        &value[..end]
    };
    let path = PathBuf::from(executable);
    path.is_absolute().then_some(path)
}

fn expand_probe(probe: PathProbe) -> Option<PathBuf> {
    env::var_os(probe.variable)
        .map(PathBuf::from)
        .map(|base| base.join(probe.suffix))
}

fn expand_environment(value: &str) -> String {
    let mut result = value.to_string();
    for (key, value) in env::vars() {
        result = result.replace(&format!("%{key}%"), &value);
        result = result.replace(&format!("%{}%", key.to_uppercase()), &value);
    }
    result
}

fn registry_key_exists(full_path: &str) -> bool {
    let Some((hive, path)) = split_registry_path(full_path) else {
        return false;
    };
    let root = match hive {
        "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
        "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
        _ => return false,
    };
    root.open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY)
        .is_ok()
        || root
            .open_subkey_with_flags(path, KEY_READ | KEY_WOW64_32KEY)
            .is_ok()
}

fn split_registry_path(value: &str) -> Option<(&str, &str)> {
    value.split_once(char::from(92))
}

fn add_evidence(
    evidence: &mut Vec<AntiCheatEvidence>,
    seen: &mut HashSet<String>,
    kind: &str,
    path: &str,
) {
    let key = format!("{kind}:{}", path.to_lowercase());
    if seen.insert(key) {
        evidence.push(AntiCheatEvidence {
            kind: kind.to_string(),
            path: path.to_string(),
        });
    }
}

fn deduplicate_targets(targets: &mut Vec<Target>) {
    let mut seen = HashSet::new();
    targets.retain(|target| {
        let key = match target {
            Target::Service(value) => format!("service:{}", value.to_lowercase()),
            Target::Registry(value) => format!("registry:{}", value.to_lowercase()),
            Target::Path(value) => format!("path:{}", value.to_string_lossy().to_lowercase()),
        };
        seen.insert(key)
    });
}

fn remove_selected(
    selected_id: Option<&str>,
    backup_root: &Path,
) -> Result<AntiCheatRemovalResult, String> {
    let detections = scan_internal()?;
    let selected = detections
        .into_iter()
        .filter(|entry| entry.item.detected)
        .filter(|entry| selected_id.is_none_or(|id| entry.item.id == id))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Ok(AntiCheatRemovalResult {
            success: true,
            message: "Следы выбранных античитов не найдены".into(),
            removed: 0,
            failed: Vec::new(),
            backup_dir: None,
            restart_required: false,
        });
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    if backup_root.as_os_str().is_empty() || !backup_root.is_absolute() {
        return Err("Выберите существующую папку для резервной копии".into());
    }
    fs::create_dir_all(backup_root)
        .map_err(|error| format!("Не удалось открыть выбранную папку: {error}"))?;
    let backup = backup_root.join(format!("Crocodile-Gena-AntiCheat-Backup-{timestamp}"));
    fs::create_dir_all(&backup)
        .map_err(|error| format!("Не удалось создать резервную папку: {error}"))?;

    let mut removed = 0;
    let mut failed = Vec::new();
    let mut restart_required = false;
    for detection in selected {
        let item_backup = backup.join(&detection.item.id);
        let _ = fs::create_dir_all(&item_backup);
        for (index, target) in detection.targets.iter().enumerate() {
            let result = match target {
                Target::Service(name) => remove_service(name, &item_backup),
                Target::Registry(path) => remove_registry(path, &item_backup, index),
                Target::Path(path) => quarantine_path(path, &item_backup, index),
            };
            match result {
                Ok(()) => removed += 1,
                Err(error) => {
                    if error.contains("используется") || error.contains("занят") {
                        restart_required = true;
                    }
                    failed.push(format!("{}: {error}", detection.item.name));
                }
            }
        }
    }

    Ok(AntiCheatRemovalResult {
        success: failed.is_empty(),
        message: if failed.is_empty() {
            "Найденные компоненты удалены".into()
        } else {
            "Очистка завершена, но некоторые компоненты удалить не удалось".into()
        },
        removed,
        failed,
        backup_dir: Some(backup.to_string_lossy().to_string()),
        restart_required,
    })
}

fn remove_service(name: &str, backup: &Path) -> Result<(), String> {
    let registry_path = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}");
    let _ = export_registry(
        &registry_path,
        &backup.join(format!("service-{name}.reg")),
        "64",
    );
    let mut stop = Command::new("sc.exe");
    stop.args(["stop", name]);
    #[cfg(target_os = "windows")]
    stop.creation_flags(CREATE_NO_WINDOW);
    let _ = stop.output();
    let mut delete = Command::new("sc.exe");
    delete.args(["delete", name]);
    #[cfg(target_os = "windows")]
    delete.creation_flags(CREATE_NO_WINDOW);
    let output = delete
        .output()
        .map_err(|error| format!("не удалось удалить службу {name}: {error}"))?;
    if output.status.success() || !registry_key_exists(&registry_path) {
        Ok(())
    } else {
        Err(format!("служба {name} не удалена"))
    }
}

fn remove_registry(path: &str, backup: &Path, index: usize) -> Result<(), String> {
    for view in ["64", "32"] {
        let export = backup.join(format!("registry-{index}-{view}.reg"));
        let _ = export_registry(path, &export, view);
        let mut command = Command::new("reg.exe");
        command.args(["delete", path, "/f", &format!("/reg:{view}")]);
        #[cfg(target_os = "windows")]
        command.creation_flags(CREATE_NO_WINDOW);
        command
            .output()
            .map_err(|error| format!("не удалось удалить запись {path}: {error}"))?;
    }
    if !registry_key_exists(path) {
        Ok(())
    } else {
        Err(format!("запись реестра не удалена: {path}"))
    }
}

fn export_registry(path: &str, destination: &Path, view: &str) -> Result<(), String> {
    let mut command = Command::new("reg.exe");
    command.args(["export", path]);
    command.arg(destination);
    command.args(["/y", &format!("/reg:{view}")]);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| format!("не удалось сохранить {path}: {error}"))?;
    output
        .status
        .success()
        .then_some(())
        .ok_or_else(|| format!("не удалось сохранить резервную копию {path}"))
}

fn quarantine_path(path: &Path, backup: &Path, index: usize) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.parent().is_none() || path.components().count() < 3 {
        return Err(format!("небезопасный путь пропущен: {}", path.display()));
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("component");
    let destination = backup.join(format!("file-{index}-{name}"));
    fs::rename(path, &destination).map_err(|error| {
        format!(
            "файл или папка используется и не перемещена: {} ({error})",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{scan_report, service_image_path, service_matches, DEFINITIONS};

    #[test]
    fn scanner_always_reports_every_supported_product() {
        let report = scan_report().expect("scan");
        assert_eq!(report.items.len(), DEFINITIONS.len());
    }

    #[test]
    fn service_patterns_support_exact_and_prefix_matches() {
        assert!(service_matches("EasyAntiCheat_EOS", "EasyAntiCheat*"));
        assert!(service_matches("vgk", "vgk"));
        assert!(!service_matches("unrelated", "vgk"));
    }

    #[test]
    fn extracts_driver_path_from_service_command() {
        let path =
            service_image_path(r#"\??\C:\Windows\System32\drivers\ace-base.sys"#).expect("path");
        assert!(path.ends_with(r"System32\drivers\ace-base.sys"));
    }
}
