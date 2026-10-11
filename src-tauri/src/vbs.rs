use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
use tauri::{AppHandle, Manager};
use winreg::{
    enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE},
    RegKey,
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Copy)]
struct RegistryTarget {
    path: &'static str,
    name: &'static str,
    disabled_value: u32,
}

const REGISTRY_TARGETS: &[RegistryTarget] = &[
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\DeviceGuard",
        name: "EnableVirtualizationBasedSecurity",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SOFTWARE\Policies\Microsoft\Windows\DeviceGuard",
        name: "EnableVirtualizationBasedSecurity",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity",
        name: "Enabled",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\CredentialGuard",
        name: "Enabled",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\Lsa",
        name: "LsaCfgFlags",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SOFTWARE\Policies\Microsoft\Windows\DeviceGuard",
        name: "LsaCfgFlags",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\SystemGuard",
        name: "Enabled",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HyperGuard",
        name: "Enabled",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\Host-Guardian",
        name: "Enabled",
        disabled_value: 0,
    },
    RegistryTarget {
        path: r"SYSTEM\CurrentControlSet\Control\DeviceGuard",
        name: "DisableHypervisorEnforcedPagingTranslation",
        disabled_value: 1,
    },
];

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SystemProbe {
    vbs_status: i32,
    configured_services: String,
    running_services: String,
    hypervisor_present: bool,
    virtualization_firmware_enabled: bool,
    secure_boot: String,
    dse_mode: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VbsDseStatus {
    state: String,
    label: String,
    detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VbsComponent {
    id: String,
    name: String,
    description: String,
    state: String,
    enabled: bool,
    locked: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VbsReport {
    admin: bool,
    secure_boot: String,
    virtualization_enabled: bool,
    hypervisor_present: bool,
    restart_required: bool,
    can_restore: bool,
    test_signing_configured: Option<bool>,
    test_signing_active: bool,
    dse: VbsDseStatus,
    components: Vec<VbsComponent>,
    enabled_count: usize,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VbsChangeResult {
    success: bool,
    message: String,
    changed: usize,
    failed: Vec<String>,
    restart_required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistrySnapshot {
    path: String,
    name: String,
    value: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VbsSnapshot {
    registry: Vec<RegistrySnapshot>,
    hypervisor_launch_type: Option<String>,
    vsm_launch_type: Option<String>,
    #[serde(default)]
    test_signing: Option<String>,
}

#[derive(Clone, Copy)]
enum BootMode {
    AdvancedOptions,
    TestSigning,
}

fn boot_changes(mode: BootMode) -> Vec<(&'static str, &'static str)> {
    let mut changes = vec![("hypervisorlaunchtype", "off"), ("vsmlaunchtype", "off")];
    match mode {
        BootMode::AdvancedOptions => changes.push(("onetimeadvancedoptions", "on")),
        BootMode::TestSigning => changes.push(("testsigning", "on")),
    }
    changes
}

#[tauri::command]
pub(crate) async fn vbs_scan(app: AppHandle) -> Result<VbsReport, String> {
    tauri::async_runtime::spawn_blocking(move || scan_report(&app))
        .await
        .map_err(|error| format!("Проверка VBS была прервана: {error}"))?
}

#[tauri::command]
pub(crate) async fn vbs_disable(app: AppHandle) -> Result<VbsChangeResult, String> {
    tauri::async_runtime::spawn_blocking(move || disable_vbs(&app, BootMode::AdvancedOptions))
        .await
        .map_err(|error| format!("Отключение VBS было прервано: {error}"))?
}

#[tauri::command]
pub(crate) async fn vbs_restore(app: AppHandle) -> Result<VbsChangeResult, String> {
    tauri::async_runtime::spawn_blocking(move || restore_vbs(&app, false))
        .await
        .map_err(|error| format!("Восстановление VBS было прервано: {error}"))?
}

#[tauri::command]
pub(crate) async fn vbs_set_test_mode(
    app: AppHandle,
    enabled: bool,
) -> Result<VbsChangeResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            disable_vbs(&app, BootMode::TestSigning)
        } else {
            restore_vbs(&app, true)
        }
    })
    .await
    .map_err(|error| format!("Изменение тестового режима было прервано: {error}"))?
}

#[tauri::command]
pub(crate) fn vbs_restart() -> Result<(), String> {
    run_command("shutdown.exe", &["/r", "/t", "0"])
        .map(|_| ())
        .map_err(|error| format!("Не удалось перезапустить Windows: {error}"))
}

fn scan_report(app: &AppHandle) -> Result<VbsReport, String> {
    let probe = system_probe()?;
    let configured = parse_service_ids(&probe.configured_services);
    let running = parse_service_ids(&probe.running_services);
    let vbs_registry = registry_dword(
        r"SYSTEM\CurrentControlSet\Control\DeviceGuard",
        "EnableVirtualizationBasedSecurity",
    );
    let hvci_registry = registry_dword(
        r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity",
        "Enabled",
    );
    let credential_registry = registry_dword(
        r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\CredentialGuard",
        "Enabled",
    );
    let lsa_registry = registry_dword(r"SYSTEM\CurrentControlSet\Control\Lsa", "LsaCfgFlags");

    let vbs_enabled = probe.vbs_status > 0 || vbs_registry.is_some_and(|value| value > 0);
    let hvci_enabled = running.contains(&2) || hvci_registry.is_some_and(|value| value > 0);
    let credential_enabled = running.contains(&1)
        || credential_registry.is_some_and(|value| value > 0)
        || lsa_registry.is_some_and(|value| value > 0);
    let windows_hello_enabled = registry_enabled(
        r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\WindowsHello",
        "Enabled",
    );
    let vbs_locked = registry_enabled(r"SYSTEM\CurrentControlSet\Control\DeviceGuard", "Locked");
    let hvci_locked = registry_enabled(
        r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity",
        "Locked",
    );
    let credential_locked = lsa_registry == Some(1);

    let mut components = vec![
        component(
            "vbs",
            "Virtualization-based Security (VBS)",
            "Основа всей VBS-защиты Windows 10/11",
            vbs_enabled,
            vbs_locked,
        ),
        component(
            "hvci",
            "Memory Integrity (HVCI)",
            "Hyper-V Protected Code Integrity — изоляция памяти ядра",
            hvci_enabled,
            hvci_locked,
        ),
        component(
            "credential-guard",
            "Credential Guard",
            "Защита хешей NTLM и Kerberos-учётных данных",
            credential_enabled,
            credential_locked,
        ),
    ];

    if configured.contains(&2) && !components[1].enabled {
        components[1].state = "configured".into();
    }
    if configured.contains(&1) && !components[2].enabled {
        components[2].state = "configured".into();
    }

    let enabled_count = components.iter().filter(|item| item.enabled).count();
    let mut warnings = Vec::new();
    if !probe.virtualization_firmware_enabled && !probe.hypervisor_present {
        warnings.push("Аппаратная виртуализация VT-x/SVM отключена в BIOS/UEFI.".into());
    }
    if vbs_locked || hvci_locked || credential_locked {
        warnings.push("Обнаружена UEFI-защита одного или нескольких компонентов. Автоматическое отключение заблокированных параметров не выполняется.".into());
    }
    if windows_hello_enabled {
        warnings.push("Windows Hello защищён VBS. Перед отключением проверьте, что у учётной записи настроен обычный пароль.".into());
    }

    Ok(VbsReport {
        admin: is_admin(),
        secure_boot: probe.secure_boot,
        virtualization_enabled: probe.virtualization_firmware_enabled || probe.hypervisor_present,
        hypervisor_present: probe.hypervisor_present,
        restart_required: snapshot_path(app)?.exists(),
        can_restore: snapshot_path(app)?.exists(),
        test_signing_configured: configured_test_signing(),
        test_signing_active: probe.dse_mode == 1,
        dse: dse_status(probe.dse_mode),
        components,
        enabled_count,
        warnings,
    })
}

fn component(id: &str, name: &str, description: &str, enabled: bool, locked: bool) -> VbsComponent {
    VbsComponent {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        state: if locked {
            "locked"
        } else if enabled {
            "enabled"
        } else {
            "disabled"
        }
        .into(),
        enabled,
        locked,
    }
}

fn dse_status(mode: i32) -> VbsDseStatus {
    match mode {
        0 => VbsDseStatus {
            state: "disabled".into(),
            label: "Отключена".into(),
            detail: "Windows разрешает загрузку неподписанных драйверов".into(),
        },
        1 => VbsDseStatus {
            state: "testsigning".into(),
            label: "Test Signing".into(),
            detail: "Включён тестовый режим подписи драйверов".into(),
        },
        2 => VbsDseStatus {
            state: "enabled".into(),
            label: "Включена".into(),
            detail: "Проверка цифровой подписи драйверов работает".into(),
        },
        _ => VbsDseStatus {
            state: "unknown".into(),
            label: "Не определено".into(),
            detail: "Windows не вернула состояние Code Integrity".into(),
        },
    }
}

fn disable_vbs(app: &AppHandle, mode: BootMode) -> Result<VbsChangeResult, String> {
    if !is_admin() {
        return Err("Для изменения VBS запустите программу от имени администратора".into());
    }

    let test_mode = matches!(mode, BootMode::TestSigning);
    if test_mode && system_probe()?.secure_boot == "true" {
        return Err("Для включения тестового режима и запуска игры отключите Secure Boot в BIOS/UEFI, затем повторите действие.".into());
    }

    let snapshot_file = snapshot_path(app)?;
    let new_snapshot = !snapshot_file.exists();
    if new_snapshot {
        let snapshot = capture_snapshot();
        if let Some(parent) = snapshot_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Не удалось создать папку восстановления: {error}"))?;
        }
        fs::write(
            &snapshot_file,
            serde_json::to_vec_pretty(&snapshot)
                .map_err(|error| format!("Не удалось подготовить снимок VBS: {error}"))?,
        )
        .map_err(|error| format!("Не удалось сохранить снимок VBS: {error}"))?;
    }

    let mut changed = 0;
    let mut failed = Vec::new();
    if test_mode {
        // Secure Boot may be unavailable to the probe. Ask BCDEdit first so a
        // rejected test-signing change leaves the protection settings untouched.
        if let Err(error) = run_command("bcdedit.exe", &["/set", "{current}", "testsigning", "on"])
        {
            if new_snapshot {
                let _ = fs::remove_file(&snapshot_file);
            }
            return Err(format!("Не удалось включить тестовый режим: {error}"));
        }
        changed += 1;
        if bcd_value("onetimeadvancedoptions").is_some() {
            match run_command(
                "bcdedit.exe",
                &["/deletevalue", "{current}", "onetimeadvancedoptions"],
            ) {
                Ok(_) => changed += 1,
                Err(error) => {
                    failed.push(format!("Меню дополнительных параметров загрузки: {error}"))
                }
            }
        }
    }
    for target in REGISTRY_TARGETS {
        if registry_dword(target.path, target.name) == Some(target.disabled_value) {
            continue;
        }
        match set_registry_dword(target.path, target.name, target.disabled_value) {
            Ok(()) => changed += 1,
            Err(error) => failed.push(error),
        }
    }

    for (name, value) in boot_changes(mode) {
        if test_mode && name == "testsigning" {
            continue;
        }
        match run_command("bcdedit.exe", &["/set", "{current}", name, value]) {
            Ok(_) => changed += 1,
            Err(error) => failed.push(format!("BCD {name}: {error}")),
        }
    }

    Ok(VbsChangeResult {
        success: failed.is_empty(),
        message: if test_mode && failed.is_empty() {
            "Тестовый режим подготовлен к включению. Перезапустите Windows".into()
        } else if failed.is_empty() {
            "VBS и системный гипервизор подготовлены к отключению".into()
        } else {
            "Часть параметров VBS изменить не удалось".into()
        },
        changed,
        failed,
        restart_required: changed > 0,
    })
}

fn restore_vbs(app: &AppHandle, disable_test_mode: bool) -> Result<VbsChangeResult, String> {
    if !is_admin() {
        return Err("Для восстановления VBS запустите программу от имени администратора".into());
    }
    let snapshot_file = snapshot_path(app)?;
    if !snapshot_file.exists() {
        if disable_test_mode {
            run_command("bcdedit.exe", &["/set", "{current}", "testsigning", "off"])?;
            return Ok(VbsChangeResult {
                success: true,
                message: "Тестовый режим подготовлен к выключению. Перезапустите Windows".into(),
                changed: 1,
                failed: Vec::new(),
                restart_required: true,
            });
        }
        return Err("Сохранённое состояние VBS не найдено".into());
    }
    let snapshot: VbsSnapshot = serde_json::from_slice(
        &fs::read(&snapshot_file)
            .map_err(|error| format!("Не удалось прочитать снимок VBS: {error}"))?,
    )
    .map_err(|error| format!("Снимок VBS повреждён: {error}"))?;

    let mut changed = 0;
    let mut failed = Vec::new();
    if disable_test_mode {
        // Do not discard the recovery snapshot if Windows rejects this change.
        run_command("bcdedit.exe", &["/set", "{current}", "testsigning", "off"])?;
        changed += 1;
    }
    for item in &snapshot.registry {
        let result = match item.value {
            Some(value) => set_registry_dword(&item.path, &item.name, value),
            None => delete_registry_value(&item.path, &item.name),
        };
        match result {
            Ok(()) => changed += 1,
            Err(error) => failed.push(error),
        }
    }

    for (name, value) in [
        (
            "hypervisorlaunchtype",
            snapshot.hypervisor_launch_type.as_deref(),
        ),
        ("vsmlaunchtype", snapshot.vsm_launch_type.as_deref()),
    ] {
        let result = if let Some(value) = value {
            run_command("bcdedit.exe", &["/set", "{current}", name, value])
        } else {
            run_command("bcdedit.exe", &["/deletevalue", "{current}", name])
        };
        match result {
            Ok(_) => changed += 1,
            Err(error) => failed.push(format!("BCD {name}: {error}")),
        }
    }

    if !disable_test_mode {
        let result = if let Some(value) = snapshot.test_signing.as_deref() {
            Some(run_command(
                "bcdedit.exe",
                &["/set", "{current}", "testsigning", value],
            ))
        } else if bcd_value("testsigning").is_some() {
            Some(run_command(
                "bcdedit.exe",
                &["/deletevalue", "{current}", "testsigning"],
            ))
        } else {
            None
        };
        match result {
            Some(Ok(_)) => changed += 1,
            Some(Err(error)) => failed.push(format!("BCD testsigning: {error}")),
            None => {}
        }
    }

    // Windows consumes this one-time option during boot. If it is already gone,
    // BCDEdit returns an error even though there is nothing left to restore.
    // When the user cancels before rebooting, this best-effort cleanup removes it.
    let _ = run_command(
        "bcdedit.exe",
        &["/deletevalue", "{current}", "onetimeadvancedoptions"],
    );

    if failed.is_empty() {
        fs::remove_file(&snapshot_file)
            .map_err(|error| format!("Параметры восстановлены, но снимок не удалён: {error}"))?;
    }

    Ok(VbsChangeResult {
        success: failed.is_empty(),
        message: if disable_test_mode && failed.is_empty() {
            "Тестовый режим выключен в параметрах загрузки. Исходные параметры VBS восстановлены"
                .into()
        } else if failed.is_empty() {
            "Исходные параметры VBS восстановлены".into()
        } else {
            "Часть параметров VBS восстановить не удалось".into()
        },
        changed,
        failed,
        restart_required: changed > 0,
    })
}

fn capture_snapshot() -> VbsSnapshot {
    VbsSnapshot {
        registry: REGISTRY_TARGETS
            .iter()
            .map(|target| RegistrySnapshot {
                path: target.path.into(),
                name: target.name.into(),
                value: registry_dword(target.path, target.name),
            })
            .collect(),
        hypervisor_launch_type: bcd_value("hypervisorlaunchtype"),
        vsm_launch_type: bcd_value("vsmlaunchtype"),
        test_signing: bcd_value("testsigning"),
    }
}

fn system_probe() -> Result<SystemProbe, String> {
    let script = r#"
$ErrorActionPreference='SilentlyContinue'
$dg=Get-CimInstance -Namespace 'root\Microsoft\Windows\DeviceGuard' -ClassName 'Win32_DeviceGuard'
$cs=Get-CimInstance -ClassName Win32_ComputerSystem
$cpu=Get-CimInstance -ClassName Win32_Processor | Select-Object -First 1
$sb='unknown'; try { $sb=([bool](Confirm-SecureBootUEFI)).ToString().ToLowerInvariant() } catch [System.PlatformNotSupportedException] { $sb='false' } catch {}
$dse=-1
try {
  $t=Add-Type -PassThru -MemberDefinition '[DllImport("ntdll.dll")] public static extern uint NtQuerySystemInformation(int c,IntPtr b,uint s,out uint r);' -Name CGSCI -Namespace Crocodile
  $p=[Runtime.InteropServices.Marshal]::AllocHGlobal(8)
  [Runtime.InteropServices.Marshal]::WriteInt32($p,8)
  $r=[uint32]0
  $status=$t::NtQuerySystemInformation(103,$p,8,[ref]$r)
  if($status -eq 0){
    $o=[uint32][Runtime.InteropServices.Marshal]::ReadInt32($p,4)
    if($o -band 2){$dse=1}elseif(-not($o -band 1)){$dse=0}else{$dse=2}
  }
  [Runtime.InteropServices.Marshal]::FreeHGlobal($p)
} catch {}
[pscustomobject]@{
  vbsStatus=[int]$dg.VirtualizationBasedSecurityStatus
  configuredServices=(@($dg.SecurityServicesConfigured | ForEach-Object {[int]$_}) -join ',')
  runningServices=(@($dg.SecurityServicesRunning | ForEach-Object {[int]$_}) -join ',')
  hypervisorPresent=[bool]$cs.HypervisorPresent
  virtualizationFirmwareEnabled=[bool]$cpu.VirtualizationFirmwareEnabled
  secureBoot=$sb
  dseMode=[int]$dse
} | ConvertTo-Json -Compress
"#;
    let output = powershell_output(script)?;
    serde_json::from_str(output.trim())
        .map_err(|error| format!("Windows вернула некорректный отчёт VBS: {error}"))
}

fn parse_service_ids(value: &str) -> Vec<i32> {
    value
        .split(',')
        .filter_map(|item| item.trim().parse::<i32>().ok())
        .collect()
}

fn registry_dword(path: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY)
        .ok()?
        .get_value(name)
        .ok()
}

fn registry_enabled(path: &str, name: &str) -> bool {
    registry_dword(path, name).is_some_and(|value| value > 0)
}

fn set_registry_dword(path: &str, name: &str, value: u32) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .create_subkey_with_flags(path, KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| format!("HKLM\\{path}\\{name}: {error}"))?;
    key.set_value(name, &value)
        .map_err(|error| format!("HKLM\\{path}\\{name}: {error}"))
}

fn delete_registry_value(path: &str, name: &str) -> Result<(), String> {
    let key = match RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(path, KEY_WRITE | KEY_WOW64_64KEY)
    {
        Ok(key) => key,
        Err(_) => return Ok(()),
    };
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("HKLM\\{path}\\{name}: {error}")),
    }
}

fn bcd_value(name: &str) -> Option<String> {
    let output = run_command("bcdedit.exe", &["/enum", "{current}"]).ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let key = parts.next()?;
            key.eq_ignore_ascii_case(name)
                .then(|| parts.next().unwrap_or_default().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn bcd_boolean(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "yes" | "on" | "true" | "1" | "да" => Some(true),
        "no" | "off" | "false" | "0" | "нет" => Some(false),
        _ => None,
    }
}

fn test_signing_from_bcd(text: &str) -> Option<bool> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts
            .next()
            .is_some_and(|key| key.eq_ignore_ascii_case("testsigning"))
        {
            return parts.next().and_then(bcd_boolean);
        }
    }
    // An absent TESTSIGNING element is the default OFF state, including after
    // rollback while the running OS still reports test mode until the reboot.
    Some(false)
}

fn configured_test_signing() -> Option<bool> {
    let output = run_command("bcdedit.exe", &["/enum", "{current}"]).ok()?;
    test_signing_from_bcd(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_uses_normal_boot_and_same_protection_settings() {
        let changes = boot_changes(BootMode::TestSigning);
        assert!(changes.contains(&("testsigning", "on")));
        assert!(!changes
            .iter()
            .any(|(name, _)| *name == "onetimeadvancedoptions"));
        for (name, value) in boot_changes(BootMode::AdvancedOptions) {
            if name != "onetimeadvancedoptions" {
                assert!(changes.contains(&(name, value)));
            }
        }
    }

    #[test]
    fn detects_configured_test_mode_and_pending_rollback() {
        assert_eq!(test_signing_from_bcd("testsigning Yes"), Some(true));
        assert_eq!(test_signing_from_bcd("testsigning Нет"), Some(false));
        assert_eq!(test_signing_from_bcd("testsigning Да"), Some(true));
        assert_eq!(
            test_signing_from_bcd("hypervisorlaunchtype Auto"),
            Some(false)
        );
        assert_eq!(test_signing_from_bcd("testsigning unexpected"), None);
    }

    #[test]
    fn old_visor_snapshot_remains_usable() {
        let snapshot: VbsSnapshot = serde_json::from_str(
            r#"{"registry":[],"hypervisorLaunchType":"Auto","vsmLaunchType":null}"#,
        )
        .unwrap();
        assert!(snapshot.test_signing.is_none());
        assert_eq!(snapshot.hypervisor_launch_type.as_deref(), Some("Auto"));
    }
}

fn snapshot_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Не удалось определить папку приложения: {error}"))?
        .join("vbs")
        .join("restore-state.json"))
}

fn powershell_output(script: &str) -> Result<String, String> {
    let output = run_command(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_command(program: &str, args: &[&str]) -> Result<Output, String> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| format!("Не удалось запустить {program}: {error}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if !error.is_empty() { error } else { stdout })
    }
}

fn is_admin() -> bool {
    run_command("fltmc.exe", &[]).is_ok()
}
