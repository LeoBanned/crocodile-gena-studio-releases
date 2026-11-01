//! Native VBS 2.0 workflow. The reference CMD is documentation, never executed.
//! All changes are journalled before writing; system tools run without a console.
use super::*;
use std::{
    io::Read,
    process::Stdio,
    sync::{Mutex, MutexGuard},
    thread,
};
use tauri::Emitter;

static OPERATION: Mutex<()> = Mutex::new(());
const DG: &str = r"SYSTEM\CurrentControlSet\Control\DeviceGuard";
const POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\DeviceGuard";
const MEMORY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management";
const OPT_OUT: &str = "{0cb3b571-2f2e-4343-a879-d86a476d7215}";

pub(super) const SCENARIOS: &[(&str, &str)] = &[
    ("WindowsHello", "Windows Hello Protection"),
    ("SecureBiometrics", "Secure Biometrics"),
    (
        "WindowsHelloSecureBiometrics",
        "Windows Hello Secure Biometrics",
    ),
    ("SecureFingerprint", "Secure Fingerprint"),
    (
        "WindowsHelloSecureFingerprint",
        "Windows Hello Secure Fingerprint",
    ),
    ("HyperGuard", "HyperGuard"),
    ("Host-Guardian", "Guarded Host"),
    ("SystemGuard", "System Guard"),
];

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    registry: Vec<RegistrySnapshot>,
    bcd: Vec<BootValue>,
    services: Vec<Service>,
    #[serde(default)]
    uefi: bool,
    #[serde(default)]
    efi_original: Option<Vec<u8>>,
    #[serde(default)]
    efi_existed: bool,
    #[serde(default)]
    bitlocker_suspended: bool,
}
#[derive(Serialize, Deserialize)]
struct BootValue {
    object: String,
    name: String,
    value: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Service {
    name: String,
    start: u32,
    running: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    build: u32,
    architecture: String,
    manufacturer: String,
    wmi: bool,
    hello_credential: bool,
    bitlocker: i32,
}

pub(super) fn lock() -> Result<MutexGuard<'static, ()>, String> {
    OPERATION
        .try_lock()
        .map_err(|_| "Другая операция VBS или восстановления Windows уже выполняется".into())
}
fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("vbs/native-v2-state.json"))
}
pub(super) fn has_snapshot(app: &AppHandle) -> bool {
    path(app).is_ok_and(|p| p.is_file())
}
fn save(app: &AppHandle, journal: &Journal) -> Result<(), String> {
    let file = path(app)?;
    fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    let data = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
    let temporary = file.with_extension("tmp");
    fs::write(&temporary, data).map_err(|e| e.to_string())?;
    // Never remove the last recovery copy before the replacement has been written.
    if file.exists() {
        fs::copy(&file, file.with_extension("previous.json")).map_err(|e| e.to_string())?;
    }
    fs::rename(&temporary, &file).map_err(|e| e.to_string())
}
fn emit(app: &AppHandle, line: impl AsRef<str>) {
    let _ = app.emit("vbs-progress", line.as_ref());
    if let Ok(directory) = app.path().app_local_data_dir() {
        let log = directory.join("vbs/operation.log");
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(log) {
            use std::io::Write;
            let _ = writeln!(file, "{}", line.as_ref());
        }
    }
}
fn reset_log(app: &AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("vbs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("operation.log"), []).map_err(|e| e.to_string())
}
fn bcd_get(object: &str, name: &str) -> Result<Option<String>, String> {
    let out = run_command("bcdedit.exe", &["/enum", object])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut values = Vec::new();
    let mut collecting = false;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(first) = parts.next() else {
            continue;
        };
        if first.eq_ignore_ascii_case(name) {
            values.extend(parts.map(str::to_string));
            collecting = true;
        } else if collecting && first.starts_with('{') {
            values.push(first.into());
        } else if collecting {
            break;
        }
    }
    Ok((!values.is_empty()).then(|| values.join(" ")))
}
fn bcd_write(object: &str, name: &str, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        let mut args = vec!["/set", object, name];
        args.extend(value.split_whitespace());
        run_command("bcdedit.exe", &args).map(|_| ())
    } else if bcd_get(object, name)?.is_some() {
        run_command("bcdedit.exe", &["/deletevalue", object, name]).map(|_| ())
    } else {
        Ok(())
    }
}
fn reg_change(
    app: &AppHandle,
    j: &mut Journal,
    key: &str,
    name: &str,
    value: Option<u32>,
) -> Result<bool, String> {
    let old = registry_dword(key, name);
    if old == value {
        return Ok(false);
    }
    if !j.registry.iter().any(|x| x.path == key && x.name == name) {
        j.registry.push(RegistrySnapshot {
            path: key.into(),
            name: name.into(),
            value: old,
        });
        save(app, j)?;
    }
    match value {
        Some(v) => set_registry_dword(key, name, v)?,
        None => delete_registry_value(key, name)?,
    }
    emit(app, format!("HKLM\\{key}\\{name}: параметр изменён"));
    Ok(true)
}
fn boot_change(
    app: &AppHandle,
    j: &mut Journal,
    object: &str,
    name: &str,
    value: Option<&str>,
) -> Result<bool, String> {
    let old = bcd_get(object, name)?;
    if old.as_deref().map(str::to_lowercase) == value.map(str::to_lowercase) {
        return Ok(false);
    }
    if !j.bcd.iter().any(|x| x.object == object && x.name == name) {
        j.bcd.push(BootValue {
            object: object.into(),
            name: name.into(),
            value: old,
        });
        save(app, j)?;
    }
    bcd_write(object, name, value)?;
    emit(app, format!("BCD {object} {name}: параметр изменён"));
    Ok(true)
}
fn environment() -> Result<Environment, String> {
    let text = powershell_output(
        r#"
$ErrorActionPreference='Stop'
$os=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$wmi=$false; $manufacturer=''; $hello=$false
try { $cs=Get-CimInstance Win32_ComputerSystem; $wmi=$cs.CreationClassName -eq 'Win32_ComputerSystem'; $manufacturer=$cs.Manufacturer } catch {}
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$key='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{D6886603-9D2F-4EB2-B667-1971041FA96B}\'+$sid
$hello=(Get-ItemProperty $key -ErrorAction SilentlyContinue).LogonCredsAvailable -eq 1
$bl=-1
try { $volume=Get-CimInstance -Namespace root\CIMV2\Security\MicrosoftVolumeEncryption -Class Win32_EncryptableVolume | Where-Object DriveLetter -eq $env:SystemDrive; if($null -ne $volume){$bl=[int]$volume.ProtectionStatus} } catch {}
[pscustomobject]@{build=[int]$os.CurrentBuildNumber;architecture=$env:PROCESSOR_ARCHITECTURE;manufacturer=[string]$manufacturer;wmi=[bool]$wmi;helloCredential=[bool]$hello;bitlocker=$bl} | ConvertTo-Json -Compress
"#,
    )?;
    serde_json::from_str(&text).map_err(|e| format!("Ошибка проверки Windows: {e}"))
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        info: *mut std::ffi::c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
}
fn kva_required() -> Result<bool, String> {
    let mut flags = 0u32;
    let mut returned = 0u32;
    let status =
        unsafe { NtQuerySystemInformation(196, (&mut flags as *mut u32).cast(), 4, &mut returned) };
    if status < 0 {
        return Err(format!(
            "Не удалось проверить KVA Shadow: NTSTATUS {status:#x}"
        ));
    }
    Ok(flags & 1 != 0 || flags & 0x30 == 0x30)
}
pub(super) fn compatibility_warnings() -> Vec<String> {
    let mut warnings = Vec::new();
    if registry_dword(
        r"SYSTEM\CurrentControlSet\Control\CI\Policy",
        "VerifiedAndReputablePolicyState",
    )
    .is_some_and(|v| v == 1 || v == 2)
    {
        warnings.push("Smart App Control включён или находится в режиме оценки и может блокировать игровые компоненты.".into());
    }
    if RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Bitdefender")
        .ok()
        .and_then(|k| k.get_value::<String, _>("atc_data").ok())
        .is_some_and(|v| v.contains("monitor_state") && v.contains("true"))
    {
        warnings.push("Обнаружен Bitdefender Advanced Threat Defense; при отказе запуска проверьте его журнал.".into());
    }
    if service_running("MacType") {
        warnings.push("MacType активен и может мешать запуску игры.".into());
    }
    if let Err(e) = xcr0() {
        warnings.push(e);
    }
    warnings
}
fn xcr0() -> Result<u64, String> {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let cpu = std::arch::x86_64::__cpuid(1);
        if cpu.ecx & (1 << 27) == 0 || cpu.ecx & (1 << 28) == 0 {
            return Err("AVX/OSXSAVE не поддерживаются или отключены".into());
        }
        let value = std::arch::x86_64::_xgetbv(0);
        if value & 7 != 7 {
            return Err("AVX не включён: XCR0 не содержит SSE/AVX".into());
        }
        return Ok(value);
    }
    #[cfg(not(target_arch = "x86_64"))]
    Err("VBS 2.0 поддерживает только Windows x64".into())
}
fn service_running(name: &str) -> bool {
    service_state(name) == Some(4)
}
#[repr(C)]
#[derive(Default)]
struct ServiceStatus {
    kind: u32,
    state: u32,
    controls: u32,
    win32_exit: u32,
    service_exit: u32,
    checkpoint: u32,
    wait_hint: u32,
}
#[link(name = "advapi32")]
extern "system" {
    fn OpenSCManagerW(
        machine: *const u16,
        database: *const u16,
        access: u32,
    ) -> *mut std::ffi::c_void;
    fn OpenServiceW(
        manager: *mut std::ffi::c_void,
        name: *const u16,
        access: u32,
    ) -> *mut std::ffi::c_void;
    fn QueryServiceStatus(service: *mut std::ffi::c_void, status: *mut ServiceStatus) -> i32;
    fn CloseServiceHandle(handle: *mut std::ffi::c_void) -> i32;
}
fn service_state(name: &str) -> Option<u32> {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), 1);
        if manager.is_null() {
            return None;
        }
        let service = OpenServiceW(manager, name.as_ptr(), 4);
        if service.is_null() {
            CloseServiceHandle(manager);
            return None;
        }
        let mut status = ServiceStatus::default();
        let ok = QueryServiceStatus(service, &mut status) != 0;
        CloseServiceHandle(service);
        CloseServiceHandle(manager);
        ok.then_some(status.state)
    }
}
fn stop_service(name: &str) -> Result<(), String> {
    if !service_running(name) {
        return Ok(());
    }
    run_command("sc.exe", &["stop", name])?;
    for _ in 0..60 {
        if service_state(name) == Some(1) {
            return Ok(());
        }
        thread::sleep(std::time::Duration::from_millis(500));
    }
    Err(format!("Служба {name} не остановилась за 30 секунд"))
}
fn disable_service(app: &AppHandle, j: &mut Journal, name: &str) -> Result<(), String> {
    let key = format!(r"SYSTEM\CurrentControlSet\Services\{name}");
    let Some(start) = registry_dword(&key, "Start") else {
        return Ok(());
    };
    if !j.services.iter().any(|s| s.name == name) {
        j.services.push(Service {
            name: name.into(),
            start,
            running: service_running(name),
        });
        save(app, j)?;
    }
    stop_service(name)?;
    run_command("sc.exe", &["config", name, "start=", "disabled"])?;
    emit(app, format!("Служба {name} остановлена и отключена"));
    Ok(())
}
fn efi_mount() -> Result<String, String> {
    for letter in 'S'..='Z' {
        let drive = format!("{letter}:");
        if !std::path::Path::new(&format!("{drive}\\")).exists() {
            run_command("mountvol.exe", &[&drive, "/s"])?;
            return Ok(drive);
        }
    }
    Err("Нет свободной буквы S–Z для раздела EFI".into())
}
fn setup_uefi(app: &AppHandle, j: &mut Journal) -> Result<(), String> {
    // Refuse to overwrite an unrelated boot entry, preserve the boot sequence and EFI file.
    if !j.uefi && run_command("bcdedit.exe", &["/enum", OPT_OUT]).is_ok() {
        return Err(
            "Запись DGOptOut уже существует. Завершите предыдущую операцию перед изменением UEFI"
                .into(),
        );
    }
    let drive = efi_mount()?;
    let result = (|| {
        let target = PathBuf::from(format!(r"{drive}\EFI\Microsoft\Boot\SecConfig.efi"));
        if !j.uefi {
            j.efi_existed = target.is_file();
            j.efi_original = if j.efi_existed {
                Some(fs::read(&target).map_err(|e| e.to_string())?)
            } else {
                None
            };
            j.bcd.push(BootValue {
                object: "{bootmgr}".into(),
                name: "bootsequence".into(),
                value: bcd_get("{bootmgr}", "bootsequence")?,
            });
            for key in [
                DG.to_string(),
                format!(r"{DG}\Scenarios\HypervisorEnforcedCodeIntegrity"),
            ] {
                if registry_enabled(&key, "Locked")
                    && !j
                        .registry
                        .iter()
                        .any(|v| v.path == key && v.name == "Locked")
                {
                    j.registry.push(RegistrySnapshot {
                        path: key.clone(),
                        name: "Locked".into(),
                        value: registry_dword(&key, "Locked"),
                    });
                }
            }
            j.uefi = true;
            save(app, j)?;
        }
        let source = system_tool("SecConfig.efi");
        fs::copy(&source, &target).map_err(|e| format!("SecConfig.efi: {e}"))?;
        if run_command("bcdedit.exe", &["/enum", OPT_OUT]).is_err() {
            run_command(
                "bcdedit.exe",
                &[
                    "/create",
                    OPT_OUT,
                    "/d",
                    "DGOptOut",
                    "/application",
                    "osloader",
                ],
            )?;
        }
        for (name, value) in [
            ("path", r"\EFI\Microsoft\Boot\SecConfig.efi"),
            ("loadoptions", "DISABLE-LSA-ISO,DISABLE-VBS"),
        ] {
            bcd_write(OPT_OUT, name, Some(value))?;
        }
        bcd_write(OPT_OUT, "device", Some(&format!("partition={drive}")))?;
        bcd_write("{bootmgr}", "bootsequence", Some(OPT_OUT))?;
        emit(app, "SecConfig.efi подготовлен. При загрузке подтвердите изменение UEFI на экране компьютера.");
        Ok(())
    })();
    let unmount = run_command("mountvol.exe", &[&drive, "/d"]).map(|_| ());
    result.and(unmount)
}

pub(super) fn disable(
    app: &AppHandle,
    mode: BootMode,
    allow_uefi: bool,
) -> Result<VbsChangeResult, String> {
    let _guard = lock()?;
    reset_log(app)?;
    if !is_admin() {
        return Err("Для изменения VBS нужны права администратора".into());
    }
    let env = environment()?;
    if env.architecture != "AMD64"
        || env.build <= 19041
        || env.manufacturer.to_lowercase().contains("parallels")
    {
        return Err("VBS 2.0 требует Windows x64 новее сборки 19041, без Parallels".into());
    }
    if !env.wmi {
        return Err("WMI не отвечает. Сначала выполните Fix WMI".into());
    }
    let probe = system_probe()?;
    if !probe.hypervisor_present && !probe.virtualization_firmware_enabled {
        return Err("Включите VT-x/SVM в BIOS/UEFI".into());
    }
    let xcr = xcr0()?;
    if registry_enabled(&format!(r"{DG}\Scenarios\WindowsHello"), "Enabled") && env.hello_credential
    {
        return Err("Windows Hello защищён VBS. Сначала удалите PIN/биометрию в Параметры → Учётные записи → Варианты входа".into());
    }
    let locked = registry_enabled(DG, "Locked")
        || registry_enabled(
            &format!(r"{DG}\Scenarios\HypervisorEnforcedCodeIntegrity"),
            "Locked",
        )
        || registry_dword(r"SYSTEM\CurrentControlSet\Control\Lsa", "LsaCfgFlags") == Some(1);
    if locked && !allow_uefi {
        return Err("Требуется подтверждение снятия UEFI-блокировок".into());
    }
    if locked && !system_tool("SecConfig.efi").is_file() {
        return Err("SecConfig.efi отсутствует; параметры VBS не изменены".into());
    }
    if matches!(mode, BootMode::TestSigning) && probe.secure_boot != "false" {
        return Err("Отключите Secure Boot в BIOS/UEFI перед включением тестового режима".into());
    }
    let test = probe.secure_boot == "false";
    if !test && env.bitlocker < 0 {
        return Err(
            "Не удалось определить BitLocker. Проверьте его состояние перед подготовкой загрузки"
                .into(),
        );
    }
    // A pre-existing legacy snapshot must retain its own recovery path.
    if snapshot_path(app)?.exists() {
        return Err(
            "Сначала восстановите прежние настройки VBS кнопкой «Выключить визор», затем повторите"
                .into(),
        );
    }
    let mut j: Journal = if has_snapshot(app) {
        serde_json::from_slice(&fs::read(path(app)?).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Снимок VBS повреждён: {e}"))?
    } else {
        Journal::default()
    };
    validate_journal(&j)?;
    let mut changed = 0usize;
    let mut failed = Vec::new();
    let result = (|| -> Result<(), String> {
        emit(
            app,
            "VBS 2.0: проверены AVX, WMI, виртуализация, Windows Hello и Secure Boot",
        );
        if test && configured_test_signing() != Some(true) {
            changed += boot_change(app, &mut j, "{current}", "testsigning", Some("on"))? as usize;
        }
        if xcr > 7 && bcd_get("{current}", "xsaveremovefeature")?.is_none() {
            changed += boot_change(
                app,
                &mut j,
                "{current}",
                "xsaveremovefeature",
                Some("0xFFFFFFFFFFFFFFF8"),
            )? as usize;
        }
        if locked {
            setup_uefi(app, &mut j)?;
            for (key, name) in [
                (DG, "EnableVirtualizationBasedSecurity"),
                (DG, "RequirePlatformSecurityFeatures"),
                (POLICY, "EnableVirtualizationBasedSecurity"),
                (POLICY, "RequirePlatformSecurityFeatures"),
            ] {
                changed += reg_change(app, &mut j, key, name, None)? as usize;
            }
        }
        let flt = run_command("fltmc.exe", &[])?;
        if String::from_utf8_lossy(&flt.stdout)
            .to_uppercase()
            .contains("FACEIT")
        {
            for name in ["FACEIT", "FACEITService"] {
                disable_service(app, &mut j, name)?;
                changed += 1;
            }
        }
        if service_running("MacType") {
            disable_service(app, &mut j, "MacType")?;
            changed += 1;
        }
        for scenario in ["HypervisorEnforcedCodeIntegrity", "CredentialGuard"]
            .into_iter()
            .chain(SCENARIOS.iter().map(|(id, _)| *id))
        {
            let key = format!(r"{DG}\Scenarios\{scenario}");
            let running = (scenario == "HypervisorEnforcedCodeIntegrity"
                && parse_service_ids(&probe.running_services).contains(&2))
                || (scenario == "CredentialGuard"
                    && parse_service_ids(&probe.running_services).contains(&1));
            if registry_enabled(&key, "Enabled") || running {
                changed += reg_change(app, &mut j, &key, "Enabled", Some(0))? as usize;
            }
        }
        for (key, name) in [
            (DG, "Mandatory"),
            (DG, "EnableVirtualizationBasedSecurity"),
            (POLICY, "EnableVirtualizationBasedSecurity"),
            (r"SYSTEM\CurrentControlSet\Control\Lsa", "LsaCfgFlags"),
            (POLICY, "LsaCfgFlags"),
        ] {
            if registry_enabled(key, name) {
                changed += reg_change(app, &mut j, key, name, Some(0))? as usize;
            }
        }
        if (probe.vbs_status > 0
            || j.registry.iter().any(|r| {
                r.path == DG
                    && r.name == "EnableVirtualizationBasedSecurity"
                    && r.value.is_some_and(|v| v > 0)
            }))
            && registry_dword(DG, "RequirePlatformSecurityFeatures").is_some()
        {
            changed +=
                reg_change(app, &mut j, DG, "RequirePlatformSecurityFeatures", None)? as usize;
        }
        for name in ["SecureBiometrics", "SecureFingerprint"] {
            let key = format!(r"{DG}\Scenarios");
            if registry_enabled(&key, name) {
                changed += reg_change(app, &mut j, &key, name, Some(0))? as usize;
            }
        }
        if parse_service_ids(&probe.running_services).contains(&7) {
            changed += reg_change(
                app,
                &mut j,
                DG,
                "DisableHypervisorEnforcedPagingTranslation",
                Some(1),
            )? as usize;
        }
        if kva_required()? {
            changed +=
                reg_change(app, &mut j, MEMORY, "FeatureSettingsOverride", Some(2))? as usize;
            changed +=
                reg_change(app, &mut j, MEMORY, "FeatureSettingsOverrideMask", Some(3))? as usize;
        }
        for object in ["{current}", "{hypervisorsettings}", "{bootloadersettings}"] {
            let old = bcd_get(object, "hypervisorlaunchtype")?;
            if old
                .as_deref()
                .is_some_and(|v| matches!(v.to_lowercase().as_str(), "auto" | "on"))
                || (object == "{current}" && probe.hypervisor_present)
            {
                changed +=
                    boot_change(app, &mut j, object, "hypervisorlaunchtype", Some("off"))? as usize;
            }
        }
        if bcd_get("{current}", "vsmlaunchtype")?
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case("auto"))
        {
            changed +=
                boot_change(app, &mut j, "{current}", "vsmlaunchtype", Some("off"))? as usize;
        }
        if !test && probe.dse_mode != 0 {
            if env.bitlocker == 1 {
                // Journal first. Only one reboot is suspended; never decrypt the volume.
                j.bitlocker_suspended = true;
                save(app, &j)?;
                powershell_output(
                    r#"$ErrorActionPreference='Stop'; $v=Get-CimInstance -Namespace root\CIMV2\Security\MicrosoftVolumeEncryption -Class Win32_EncryptableVolume | Where-Object DriveLetter -eq $env:SystemDrive; $r=Invoke-CimMethod -InputObject $v -MethodName DisableKeyProtectors -Arguments @{DisableCount=[uint32]1}; if($r.ReturnValue -ne 0){throw ('BitLocker: '+$r.ReturnValue)}"#,
                )?;
                emit(
                    app,
                    "BitLocker: защита приостановлена на одну перезагрузку; шифрование сохранено",
                );
            }
            for object in [
                "{current}",
                "{globalsettings}",
                "{bootloadersettings}",
                "{hypervisorsettings}",
                "{resumeloadersettings}",
                "{bootmgr}",
                "{dbgsettings}",
                "{fwbootmgr}",
                "{memdiag}",
                "{badmemory}",
                "{emssettings}",
            ] {
                if let Ok(Some(value)) = bcd_get(object, "advancedoptions") {
                    if bcd_boolean(&value) == Some(false) {
                        changed +=
                            boot_change(app, &mut j, object, "advancedoptions", None)? as usize;
                    }
                }
            }
            changed += boot_change(
                app,
                &mut j,
                "{current}",
                "onetimeadvancedoptions",
                Some("on"),
            )? as usize;
            if !bcd_get("{current}", "bootmenupolicy")?
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case("legacy"))
            {
                changed +=
                    boot_change(app, &mut j, "{current}", "bootmenupolicy", Some("standard"))?
                        as usize;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        emit(app, &error);
        failed.push(error);
    }
    Ok(VbsChangeResult {
        success: failed.is_empty(),
        message: if failed.is_empty() {
            if test {
                "VBS 2.0: параметры подготовлены; Test Signing применяется после перезагрузки"
                    .into()
            } else {
                "VBS 2.0: после перезагрузки выберите 7/F7 — отключение проверки подписи драйверов"
                    .into()
            }
        } else {
            "Операция остановлена. Сохранён снимок для восстановления".into()
        },
        changed,
        restart_required: changed > 0 || j.uefi || j.bitlocker_suspended,
        failed,
    })
}

fn valid_registry(key: &str, name: &str) -> bool {
    (key == DG
        && [
            "Locked",
            "Mandatory",
            "EnableVirtualizationBasedSecurity",
            "RequirePlatformSecurityFeatures",
            "DisableHypervisorEnforcedPagingTranslation",
        ]
        .contains(&name))
        || (key == POLICY
            && [
                "EnableVirtualizationBasedSecurity",
                "RequirePlatformSecurityFeatures",
                "LsaCfgFlags",
            ]
            .contains(&name))
        || (key == r"SYSTEM\CurrentControlSet\Control\Lsa" && name == "LsaCfgFlags")
        || (key == MEMORY
            && ["FeatureSettingsOverride", "FeatureSettingsOverrideMask"].contains(&name))
        || (key == format!(r"{DG}\Scenarios")
            && ["SecureBiometrics", "SecureFingerprint"].contains(&name))
        || (name == "Locked" && key == format!(r"{DG}\Scenarios\HypervisorEnforcedCodeIntegrity"))
        || (name == "Enabled"
            && ["HypervisorEnforcedCodeIntegrity", "CredentialGuard"]
                .into_iter()
                .chain(SCENARIOS.iter().map(|(id, _)| *id))
                .any(|id| key == format!(r"{DG}\Scenarios\{id}")))
}
fn validate_journal(j: &Journal) -> Result<(), String> {
    let objects = [
        "{current}",
        "{globalsettings}",
        "{bootloadersettings}",
        "{hypervisorsettings}",
        "{resumeloadersettings}",
        "{bootmgr}",
        "{dbgsettings}",
        "{fwbootmgr}",
        "{memdiag}",
        "{badmemory}",
        "{emssettings}",
    ];
    if j.registry.iter().any(|v| !valid_registry(&v.path, &v.name))
        || j.services.iter().any(|s| {
            !["FACEIT", "FACEITService", "MacType"].contains(&s.name.as_str()) || s.start > 4
        })
        || j.bcd.iter().any(|v| {
            !objects.contains(&v.object.as_str())
                || ![
                    "hypervisorlaunchtype",
                    "vsmlaunchtype",
                    "testsigning",
                    "xsaveremovefeature",
                    "bootmenupolicy",
                    "onetimeadvancedoptions",
                    "advancedoptions",
                    "bootsequence",
                ]
                .contains(&v.name.as_str())
                || v.value
                    .as_deref()
                    .is_some_and(|s| s.len() > 4096 || s.contains(['\r', '\n']))
        })
    {
        return Err("Снимок VBS содержит неподдерживаемые настройки".into());
    }
    Ok(())
}
pub(super) fn restore(app: &AppHandle) -> Result<VbsChangeResult, String> {
    let _guard = lock()?;
    reset_log(app)?;
    if !is_admin() {
        return Err("Для восстановления VBS нужны права администратора".into());
    }
    let j: Journal = serde_json::from_slice(&fs::read(path(app)?).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Снимок VBS повреждён: {e}"))?;
    validate_journal(&j)?;
    let mut failed = Vec::new();
    let mut changed = 0;
    for item in &j.registry {
        match item.value.map_or_else(
            || delete_registry_value(&item.path, &item.name),
            |v| set_registry_dword(&item.path, &item.name, v),
        ) {
            Ok(()) => {
                changed += 1;
                emit(app, format!("Восстановлен {}\\{}", item.path, item.name));
            }
            Err(e) => failed.push(e),
        }
    }
    for item in j.bcd.iter().rev() {
        match bcd_write(&item.object, &item.name, item.value.as_deref()) {
            Ok(()) => changed += 1,
            Err(e) => failed.push(e),
        }
    }
    for s in &j.services {
        let start = match s.start {
            0 => "boot",
            1 => "system",
            2 => "auto",
            3 => "demand",
            _ => "disabled",
        };
        match run_command("sc.exe", &["config", &s.name, "start=", start]) {
            Ok(_) => {
                changed += 1;
                if s.running && !service_running(&s.name) {
                    if let Err(e) = run_command("sc.exe", &["start", &s.name]) {
                        failed.push(e);
                    }
                }
            }
            Err(e) => failed.push(e),
        }
    }
    if j.bitlocker_suspended {
        if let Err(e) = powershell_output(
            r#"$ErrorActionPreference='Stop'; $v=Get-CimInstance -Namespace root\CIMV2\Security\MicrosoftVolumeEncryption -Class Win32_EncryptableVolume | Where-Object DriveLetter -eq $env:SystemDrive; if($v.ProtectionStatus -eq 0){$r=Invoke-CimMethod -InputObject $v -MethodName EnableKeyProtectors; if($r.ReturnValue -ne 0){throw ('BitLocker: '+$r.ReturnValue)}}"#,
        ) {
            failed.push(e);
        }
    }
    if j.uefi {
        let restore_efi = (|| -> Result<(), String> {
            if run_command("bcdedit.exe", &["/enum", OPT_OUT]).is_ok() {
                run_command("bcdedit.exe", &["/delete", OPT_OUT])?;
            }
            let drive = efi_mount()?;
            let target = PathBuf::from(format!(r"{drive}\EFI\Microsoft\Boot\SecConfig.efi"));
            let res = if let Some(bytes) = &j.efi_original {
                fs::write(&target, bytes)
            } else if !j.efi_existed && target.exists() {
                fs::remove_file(&target)
            } else {
                Ok(())
            };
            let unmount = run_command("mountvol.exe", &[&drive, "/d"]).map(|_| ());
            res.map_err(|e| e.to_string()).and(unmount)
        })();
        if let Err(e) = restore_efi {
            failed.push(e);
        }
    }
    if failed.is_empty() {
        fs::remove_file(path(app)?).map_err(|e| e.to_string())?;
    }
    Ok(VbsChangeResult {
        success: failed.is_empty(),
        message: if failed.is_empty() {
            "Исходные параметры VBS 2.0 восстановлены".into()
        } else {
            "Не все параметры восстановлены; снимок сохранён для повторной попытки".into()
        },
        changed,
        restart_required: changed > 0,
        failed,
    })
}

fn system_tool(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32")
        .join(name)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepairResult {
    success: bool,
    message: String,
    restart_required: bool,
    log_path: String,
}
// OEM output from DISM/SFC must not become mojibake in the webview.
#[link(name = "kernel32")]
extern "system" {
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        src: *const u8,
        count: i32,
        dst: *mut u16,
        size: i32,
    ) -> i32;
}
fn decode(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    unsafe {
        let n = MultiByteToWideChar(
            1,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut out = vec![0; n as usize];
        MultiByteToWideChar(
            1,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            out.as_mut_ptr(),
            n,
        );
        String::from_utf16_lossy(&out)
    }
}
#[derive(Default)]
struct OutputLines {
    buffer: Vec<u8>,
    utf16: Option<bool>,
}
impl OutputLines {
    fn push(&mut self, bytes: &[u8], end: bool) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        if self.utf16.is_none() && self.buffer.len() >= 2 {
            self.utf16 = Some(self.buffer.starts_with(&[255, 254]) || self.buffer[1] == 0);
            if self.buffer.starts_with(&[255, 254]) {
                self.buffer.drain(..2);
            }
        }
        let wide = self.utf16 == Some(true);
        let step = if wide { 2 } else { 1 };
        let mut result = Vec::new();
        let mut start = 0;
        for pos in (0..self.buffer.len().saturating_sub(step - 1)).step_by(step) {
            let separator =
                matches!(self.buffer[pos], b'\r' | b'\n') && (!wide || self.buffer[pos + 1] == 0);
            if separator {
                if pos > start {
                    result.push(Self::text(&self.buffer[start..pos], wide));
                }
                start = pos + step;
            }
        }
        if start > 0 {
            self.buffer.drain(..start);
        }
        if end || self.buffer.len() > 16384 {
            let size = if end {
                self.buffer.len()
            } else {
                self.buffer.len() / step * step
            };
            if size > 0 {
                result.push(Self::text(&self.buffer[..size], wide));
                self.buffer.drain(..size);
            }
        }
        result
    }
    fn text(bytes: &[u8], wide: bool) -> String {
        if wide {
            String::from_utf16_lossy(
                &bytes
                    .chunks_exact(2)
                    .map(|p| u16::from_le_bytes([p[0], p[1]]))
                    .collect::<Vec<_>>(),
            )
        } else {
            decode(bytes)
        }
    }
}
fn stream(app: &AppHandle, tool: &str, args: &[&str]) -> Result<i32, String> {
    emit(app, format!("{tool} {}", args.join(" ")));
    let mut child = Command::new(system_tool(tool))
        .args(args)
        .current_dir(if tool.contains("mofcomp") {
            system_tool("wbem")
        } else {
            system_tool("")
        })
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    fn pump(app: AppHandle, mut input: impl Read + Send + 'static) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut lines = OutputLines::default();
            let mut chunk = [0u8; 4096];
            while let Ok(n) = input.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                for line in lines.push(&chunk[..n], false) {
                    emit(&app, line);
                }
            }
            for line in lines.push(&[], true) {
                emit(&app, line);
            }
        })
    }
    let stdout = pump(app.clone(), child.stdout.take().unwrap());
    let stderr = pump(app.clone(), child.stderr.take().unwrap());
    let status = child.wait().map_err(|e| e.to_string())?;
    let _ = stdout.join();
    let _ = stderr.join();
    Ok(status.code().unwrap_or(-1))
}
fn wmi_ok() -> bool {
    powershell_output(r#"$ErrorActionPreference='Stop'; $c=Get-WmiObject Win32_ComputerSystem; if($c.CreationClassName -ne 'Win32_ComputerSystem'){throw 'Win32_ComputerSystem unavailable'}; $null=([WMISEARCHER]'SELECT * FROM SoftwareLicensingService').Get().Version; 'OK'"#).is_ok() && run_command("winmgmt.exe", &["/verifyrepository"]).is_ok()
}
fn fix_wmi(app: &AppHandle, backup: &std::path::Path) -> Result<(), String> {
    if RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()
        .and_then(|k| k.get_value::<String, _>("InstallationType").ok())
        .is_some_and(|v| v.to_lowercase().contains("server"))
    {
        return Err("Fix WMI не выполняет перестройку WMI на Windows Server".into());
    }
    emit(app, "Проверяем WMI и репозиторий");
    if wmi_ok() {
        emit(app, "WMI работает. Восстановление не требуется");
        return Ok(());
    }
    // Save service configuration/dependents BEFORE stopping anything.
    powershell_output(&format!(
        r#"$ErrorActionPreference='Stop'; Get-Service Winmgmt -DependentServices | Select-Object Name,Status | ConvertTo-Json | Set-Content -LiteralPath '{}' -Encoding UTF8"#,
        backup
            .join("wmi-dependents.json")
            .display()
            .to_string()
            .replace(char::from(39), "''")
    ))?;
    let service_key = r"SYSTEM\CurrentControlSet\Services\Winmgmt";
    let original_start = registry_dword(service_key, "Start")
        .ok_or("Служба Winmgmt повреждена; восстановление остановлено")?;
    // Exact fixed system subtree, not a user-supplied deletion path. Keep the old repository.
    let wbem = system_tool("wbem")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let repository = wbem.join("Repository");
    if fs::symlink_metadata(&repository).is_ok_and(|m| {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }) {
        return Err("Репозиторий WMI является ссылкой; перестройка отменена".into());
    }
    fs::write(
        backup.join("wmi-service-start.json"),
        original_start.to_string(),
    )
    .map_err(|e| e.to_string())?;
    let saved = wbem.join(format!("Repository.cgs-backup-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        powershell_output("$ErrorActionPreference='Stop'; Stop-Service Winmgmt -Force")?;
        let _ = run_command("winmgmt.exe", &["/salvagerepository"]);
        let _ = run_command("sc.exe", &["start", "Winmgmt"]);
        if wmi_ok() {
            emit(app, "Репозиторий WMI восстановлен без перестройки");
            return Ok(());
        }
        run_command("sc.exe", &["config", "Winmgmt", "start=", "disabled"])?;
        powershell_output("$ErrorActionPreference='Stop'; Stop-Service Winmgmt -Force")?;
        if service_running("Winmgmt") {
            return Err("Winmgmt не остановлена. Репозиторий не изменён".into());
        }
        if repository.exists() {
            fs::rename(&repository, &saved)
                .map_err(|e| format!("Не удалось сохранить репозиторий: {e}"))?;
            emit(
                app,
                format!("Прежний репозиторий сохранён: {}", saved.display()),
            );
        }
        run_command("sc.exe", &["config", "Winmgmt", "start=", "auto"])?;
        run_command("sc.exe", &["start", "Winmgmt"])?;
        if wmi_ok() {
            return Ok(());
        }
        powershell_output("$ErrorActionPreference='Stop'; Stop-Service Winmgmt -Force")?;
        for name in ["scecli.dll", "userenv.dll"] {
            let file = system_tool(name);
            let p = file.to_string_lossy();
            if stream(app, "regsvr32.exe", &["/s", &p])? != 0 {
                return Err(format!("Не удалось зарегистрировать {name}"));
            }
        }
        for item in walkdir::WalkDir::new(&wbem)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !e.file_name().to_string_lossy().starts_with("Repository"))
            .filter_map(Result::ok)
            .filter(|i| i.file_type().is_file())
        {
            let p = item.path();
            let ext = p.extension().and_then(|v| v.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("dll") {
                let _ = stream(app, "regsvr32.exe", &["/s", &p.to_string_lossy()]);
            } else if p.parent() == Some(wbem.as_path())
                && (ext.eq_ignore_ascii_case("mof") || ext.eq_ignore_ascii_case("mfl"))
            {
                let _ = stream(app, "wbem/mofcomp.exe", &[&p.to_string_lossy()]);
            }
        }
        run_command("winmgmt.exe", &["/salvagerepository"])?;
        run_command("winmgmt.exe", &["/resetrepository"])?;
        let _ = run_command("sc.exe", &["start", "Winmgmt"]);
        if wmi_ok() {
            Ok(())
        } else {
            Err("WMI всё ещё не отвечает. Выполните DISM RestoreHealth, затем SFC Scannow и повторите Fix WMI".into())
        }
    })();
    // Always restore service configuration even after a failure midway.
    let start = match original_start {
        0 => "boot",
        1 => "system",
        2 => "auto",
        3 => "demand",
        _ => "disabled",
    };
    let service_restore =
        run_command("sc.exe", &["config", "Winmgmt", "start=", start]).map(|_| ());
    let _ = run_command("sc.exe", &["start", "Winmgmt"]);
    let dependents_restore = restart_dependents(backup);
    let errors: Vec<_> = [result, service_restore, dependents_restore]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
fn restart_dependents(backup: &std::path::Path) -> Result<(), String> {
    powershell_output(&format!(r#"$ErrorActionPreference='Stop'; $p='{}'; if(Test-Path -LiteralPath $p) {{ $s=Get-Content -Raw -LiteralPath $p | ConvertFrom-Json; foreach($v in @($s)) {{ if($v.Status -eq 4 -or $v.Status -eq 'Running') {{ Start-Service -Name $v.Name }} }} }}"#, backup.join("wmi-dependents.json").display().to_string().replace(char::from(39), "''"))).map(|_| ())
}
pub(super) fn repair(app: &AppHandle, kind: &str) -> Result<RepairResult, String> {
    let _guard = lock()?;
    if !["wmi", "dism", "sfc"].contains(&kind) {
        return Err("Неизвестный тип восстановления".into());
    }
    if !is_admin() {
        return Err("Для восстановления Windows нужны права администратора".into());
    }
    reset_log(app)?;
    let folder = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("vbs/repair-logs")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut restart = false;
    let result = if kind == "wmi" {
        fix_wmi(app, &folder)
    } else {
        // Preserve system logs instead of deleting them as the reference script does.
        for (src, name) in [
            ("Logs/CBS/CBS.log", "CBS-before.log"),
            ("Logs/DISM/dism.log", "DISM-before.log"),
        ] {
            let source = system_tool("..").join(src);
            if source.is_file() {
                let _ = fs::copy(source, folder.join(name));
            }
        }
        let code = if kind == "dism" {
            stream(
                app,
                "Dism.exe",
                &["/Online", "/English", "/Cleanup-Image", "/RestoreHealth"],
            )
        } else {
            stream(app, "sfc.exe", &["/scannow"])
        };
        match code {
            Ok(0) => Ok(()),
            Ok(3010) => {
                restart = true;
                Ok(())
            }
            Ok(n) => Err(format!(
                "{} завершился с кодом {n}. Подробности — в журнале Windows",
                kind.to_uppercase()
            )),
            Err(e) => Err(e),
        }
    };
    for (src, name) in [
        ("Logs/CBS/CBS.log", "CBS.log"),
        ("Logs/DISM/dism.log", "DISM.log"),
    ] {
        let source = system_tool("..").join(src);
        if source.is_file() {
            let _ = fs::copy(source, folder.join(name));
        }
    }
    let message = match &result {
        Ok(()) => "Проверка завершена. Результат и подробности показаны в журнале".to_string(),
        Err(e) => e.clone(),
    };
    fs::write(folder.join("result.txt"), &message).map_err(|e| e.to_string())?;
    emit(app, &message);
    if let Ok(dir) = app.path().app_local_data_dir() {
        let _ = fs::copy(dir.join("vbs/operation.log"), folder.join("operation.log"));
    }
    Ok(RepairResult {
        success: result.is_ok(),
        message,
        restart_required: restart,
        log_path: folder.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_preserves_fragmented_utf16_and_carriage_return_progress() {
        let bytes: Vec<u8> = "Проверка 50%\rПроверка 100%\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut data = vec![255, 254];
        data.extend(bytes);
        let mut lines = OutputLines::default();
        let mut actual = Vec::new();
        for chunk in data.chunks(3) {
            actual.extend(lines.push(chunk, false));
        }
        actual.extend(lines.push(&[], true));
        assert_eq!(actual, vec!["Проверка 50%", "Проверка 100%"]);
        let mut utf8 = OutputLines::default();
        assert_eq!(utf8.push(b"50%\r100%\n", true), vec!["50%", "100%"]);
    }
    #[test]
    fn journal_rejects_unrelated_system_settings() {
        let mut j = Journal::default();
        j.registry.push(RegistrySnapshot {
            path: r"SOFTWARE\Unrelated".into(),
            name: "Enabled".into(),
            value: Some(1),
        });
        assert!(validate_journal(&j).is_err());
    }
    #[test]
    fn biometrics_and_old_snapshot_fields_are_supported() {
        for (id, _) in SCENARIOS {
            assert!(valid_registry(&format!(r"{DG}\Scenarios\{id}"), "Enabled"));
        }
        let j: Journal = serde_json::from_str(r#"{"registry":[],"bcd":[],"services":[]}"#).unwrap();
        assert!(!j.uefi);
        assert!(validate_journal(&j).is_ok());
    }
}
