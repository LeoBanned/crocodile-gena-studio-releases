//! Shared native updater transaction. Also compiled into the small updater EXE.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,
    pub nonce: String,
    pub target: PathBuf,
    pub parent_pid: u32,
    pub old_hash: String,
    pub new_hash: String,
    pub version: String,
}
impl Plan {
    pub fn candidate(&self) -> PathBuf {
        self.target
            .parent()
            .unwrap()
            .join(format!(".cgs-update-{}.new.exe", self.nonce))
    }
    pub fn backup(&self) -> PathBuf {
        self.target
            .parent()
            .unwrap()
            .join(format!(".cgs-update-{}.old.exe", self.nonce))
    }
    pub fn validate(&self, stage: &Path) -> Result<(), String> {
        if self.schema != 1
            || self.nonce.len() != 32
            || !self.nonce.bytes().all(|c| c.is_ascii_hexdigit())
            || stage.file_name().and_then(|s| s.to_str())
                != Some(&format!("cgs-update-{}", self.nonce))
            || !self.target.is_absolute()
            || !self.target.is_file()
            || self.target.starts_with(stage)
            || [self.old_hash.as_str(), self.new_hash.as_str()]
                .iter()
                .any(|s| s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err("Некорректный план обновления".into());
        }
        for path in [
            stage.to_path_buf(),
            self.target.clone(),
            self.candidate(),
            self.backup(),
        ] {
            reject_links(&path)?;
        }
        if fs::read_to_string(stage.join("owner")).ok().as_deref() != Some(&self.nonce) {
            return Err("Не подтверждена папка обновления".into());
        }
        Ok(())
    }
}
pub fn reject_links(path: &Path) -> Result<(), String> {
    for path in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(path) {
            let mut linked = meta.file_type().is_symlink();
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                linked |= meta.file_attributes() & 0x400 != 0;
            }
            if linked {
                return Err(format!(
                    "Обновление через ссылку запрещено: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}
pub fn hash(path: &Path) -> Result<String, String> {
    reject_links(path)?;
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut sha = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
    }
    Ok(format!("{:x}", sha.finalize()))
}
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    reject_links(path)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub nonce: String,
    pub success: bool,
    pub message: String,
}
pub fn outcome(stage: &Path, p: &Plan, success: bool, message: &str) {
    let value = Outcome {
        nonce: p.nonce.clone(),
        success,
        message: message.into(),
    };
    let _ = write(
        &stage.join("result.json"),
        &serde_json::to_vec(&value).unwrap(),
    );
}
pub fn read_plan(stage: &Path) -> Result<Plan, String> {
    read_bound_plan(stage, None)
}
pub fn read_bound_plan(stage: &Path, expected: Option<&str>) -> Result<Plan, String> {
    reject_links(stage)?;
    let bytes = fs::read(stage.join("plan.json")).map_err(|e| e.to_string())?;
    if bytes.len() > 16384 {
        return Err("Слишком большой план обновления".into());
    }
    if expected.is_some_and(|hash| hash != format!("{:x}", Sha256::digest(&bytes))) {
        return Err("План обновления был изменён после подготовки".into());
    }
    let p: Plan = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    p.validate(stage)?;
    Ok(p)
}
pub fn cleanup(stage: &Path, p: &Plan) -> Result<(), String> {
    p.validate(stage)?;
    for name in [
        "payload.exe",
        "updater.exe",
        "ready",
        "healthy",
        "go",
        "abort",
        "result.json",
        "plan.json",
    ] {
        let path = stage.join(name);
        reject_links(&path)?;
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    fs::remove_file(stage.join("owner")).map_err(|e| e.to_string())?;
    fs::remove_dir(stage).map_err(|e| e.to_string())
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::{
        ffi::c_void,
        os::windows::{ffi::OsStrExt, process::CommandExt},
    };
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            target: *const u16,
            new: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            handle: *mut c_void,
            flags: u32,
            path: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    #[link(name = "version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(path: *const u16, handle: u32, size: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(
            data: *const c_void,
            key: *const u16,
            value: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    pub fn replace(target: &Path, new: &Path, backup: Option<&Path>) -> Result<(), String> {
        let a = wide(target);
        let b = wide(new);
        let c = backup.map(wide);
        if unsafe {
            ReplaceFileW(
                a.as_ptr(),
                b.as_ptr(),
                c.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    pub fn file_version(path: &Path) -> Result<[u16; 4], String> {
        let name = wide(path);
        let size = unsafe { GetFileVersionInfoSizeW(name.as_ptr(), std::ptr::null_mut()) };
        if size == 0 || size > 1024 * 1024 {
            return Err("В EXE отсутствует корректная версия программы".into());
        }
        let mut data = vec![0u8; size as usize];
        if unsafe { GetFileVersionInfoW(name.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
            return Err("Не удалось прочитать версию EXE".into());
        }
        let mut value = std::ptr::null_mut();
        let mut len = 0;
        if unsafe {
            VerQueryValueW(
                data.as_ptr().cast(),
                [92u16, 0].as_ptr(),
                &mut value,
                &mut len,
            )
        } == 0
            || len < 52
        {
            return Err("Некорректный ресурс версии EXE".into());
        }
        let info = unsafe { std::slice::from_raw_parts(value.cast::<u32>(), 13) };
        if info[0] != 0xfeef04bd {
            return Err("Неверная подпись ресурса версии".into());
        }
        Ok([
            (info[2] >> 16) as u16,
            info[2] as u16,
            (info[3] >> 16) as u16,
            info[3] as u16,
        ])
    }
    pub struct Parent(*mut c_void);
    pub fn parent_alive(pid: u32) -> bool {
        let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
        if handle.is_null() {
            return std::io::Error::last_os_error().raw_os_error() == Some(5);
        }
        let running = unsafe { WaitForSingleObject(handle, 0) } == 258;
        unsafe {
            CloseHandle(handle);
        }
        running
    }
    impl Drop for Parent {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    impl Parent {
        pub fn open(p: &Plan) -> Result<Self, String> {
            let h = unsafe { OpenProcess(0x00100000 | 0x1000, 0, p.parent_pid) };
            if h.is_null() {
                return Err("Не найден исходный процесс программы".into());
            }
            let parent = Self(h);
            let mut buf = vec![0u16; 32768];
            let mut size = buf.len() as u32;
            if unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size) } == 0 {
                return Err("Не удалось проверить исходный процесс".into());
            }
            let image = PathBuf::from(String::from_utf16_lossy(&buf[..size as usize]));
            if image.canonicalize().map_err(|e| e.to_string())?
                != p.target.canonicalize().map_err(|e| e.to_string())?
            {
                return Err("Путь исходного процесса изменился".into());
            }
            Ok(parent)
        }
        pub fn wait(&self, stage: &Path, p: &Plan) -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                if stage.join("abort").exists() {
                    return Err("Обновление отменено до замены EXE".into());
                }
                match unsafe { WaitForSingleObject(self.0, 200) } {
                    0 => {
                        return if fs::read_to_string(stage.join("go")).ok().as_deref()
                            == Some(&p.nonce)
                        {
                            Ok(())
                        } else {
                            Err("Замена EXE не была подтверждена программой".into())
                        }
                    }
                    258 if Instant::now() < deadline => (),
                    _ => return Err("Программа не закрылась за 120 секунд; EXE не изменён".into()),
                }
            }
        }
    }
    pub fn run(stage: &Path, expected: &str) -> Result<(), String> {
        let p = read_bound_plan(stage, Some(expected))?;
        let parent = Parent::open(&p)?;
        if hash(&p.target)? != p.old_hash || hash(&stage.join("payload.exe"))? != p.new_hash {
            return Err("Контрольная сумма перед установкой не совпала".into());
        }
        validate_executable(&stage.join("payload.exe"), &p.version)?;
        let candidate = p.candidate();
        let backup = p.backup();
        if backup.exists() {
            return Err("Резервная копия уже существует; установка отменена".into());
        }
        let mut input = fs::File::open(stage.join("payload.exe")).map_err(|e| e.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .map_err(|e| e.to_string())?;
        struct Candidate(PathBuf);
        impl Drop for Candidate {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let _candidate_guard = Candidate(candidate.clone());
        std::io::copy(&mut input, &mut output)
            .and_then(|_| output.sync_all())
            .map_err(|e| e.to_string())?;
        drop(output);
        let result = (|| {
            if hash(&candidate)? != p.new_hash {
                return Err("Не совпала копия нового EXE".into());
            }
            write(&stage.join("ready"), p.nonce.as_bytes())?;
            parent.wait(stage, &p)?;
            transact(&p, || {
                let mut child = std::process::Command::new(&p.target)
                    .args(["--cgs-update-probe", &stage.to_string_lossy(), &p.nonce])
                    .current_dir(p.target.parent().unwrap())
                    .creation_flags(0x08000000)
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let deadline = Instant::now() + Duration::from_secs(90);
                loop {
                    if fs::read_to_string(stage.join("healthy")).ok().as_deref() == Some(&p.nonce) {
                        return Ok(());
                    }
                    if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                        return Err("Новая версия завершилась до готовности интерфейса".into());
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err("Новая версия не подтвердила запуск за 90 секунд".into());
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            })
        })();
        if candidate.exists() {
            let _ = fs::remove_file(&candidate);
        }
        match result {
            Ok(()) => {
                outcome(stage, &p, true, "Программа обновлена");
                Ok(())
            }
            Err(error) => {
                outcome(stage, &p, false, &error);
                // Only relaunch after the old process really exited and rollback
                // restored its exact original image.
                if unsafe { WaitForSingleObject(parent.0, 0) } == 0
                    && hash(&p.target).ok().as_deref() == Some(&p.old_hash)
                {
                    let _ = std::process::Command::new(&p.target)
                        .args(["--cgs-update-failed", &stage.to_string_lossy(), &p.nonce])
                        .current_dir(p.target.parent().unwrap())
                        .creation_flags(0x08000000)
                        .spawn();
                }
                Err(error)
            }
        }
    }
    pub fn transact(p: &Plan, launch: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        if hash(&p.target)? != p.old_hash || hash(&p.candidate())? != p.new_hash {
            return Err("Файлы изменились во время подготовки обновления".into());
        }
        replace(&p.target, &p.candidate(), Some(&p.backup()))?;
        let launched = if hash(&p.target)? != p.new_hash {
            Err("Повреждён установленный EXE".into())
        } else {
            launch()
        };
        if let Err(error) = launched {
            if hash(&p.target)? != p.new_hash || hash(&p.backup())? != p.old_hash {
                return Err(format!(
                    "{error}. Автоматический откат отменён: файлы изменены. Резервная копия: {}",
                    p.backup().display()
                ));
            }
            replace(&p.target, &p.backup(), None).map_err(|e| {
                format!(
                    "{error}. Откат не выполнен: {e}. Копия: {}",
                    p.backup().display()
                )
            })?;
            return Err(format!("{error}. Предыдущая версия восстановлена."));
        }
        // Failure to delete a locked backup does not undo a healthy update.
        if hash(&p.target)? != p.new_hash {
            return Err(format!(
                "EXE изменён после запуска. Предыдущая версия сохранена: {}",
                p.backup().display()
            ));
        }
        let _ = fs::remove_file(p.backup());
        Ok(())
    }
}
#[cfg(windows)]
pub use win::{file_version, parent_alive, run, transact};

pub fn validate_executable(path: &Path, version: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos).map_err(|e| e.to_string())?;
    if &dos[..2] != b"MZ" {
        return Err("Скачанный файл не является EXE".into());
    }
    use std::io::{Seek, SeekFrom};
    let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
    if offset > 1024 * 1024 {
        return Err("Некорректный заголовок EXE".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut pe = [0; 26];
    file.read_exact(&mut pe).map_err(|e| e.to_string())?;
    if &pe[..4] != b"PE\0\0"
        || u16::from_le_bytes([pe[4], pe[5]]) != 0x8664
        || u16::from_le_bytes([pe[22], pe[23]]) & 0x2000 != 0
        || u16::from_le_bytes([pe[24], pe[25]]) != 0x20b
    {
        return Err(
            "Ожидалась программа Windows x64, а не DLL или установщик другой архитектуры".into(),
        );
    }
    #[cfg(windows)]
    {
        let wanted = version
            .split('.')
            .map(str::parse::<u16>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Некорректная версия релиза")?;
        let actual = file_version(path)?;
        if wanted.len() != 3 || actual[..3] != wanted[..] || actual[3] != 0 {
            return Err(format!(
                "Версия EXE {}.{}.{}.{} не соответствует релизу {version}",
                actual[0], actual[1], actual[2], actual[3]
            ));
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Plan) {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Программа с пробелами.exe");
        fs::write(&target, b"old executable").unwrap();
        let p = Plan {
            schema: 1,
            nonce: "a".repeat(32),
            target: target.clone(),
            parent_pid: 1,
            old_hash: hash(&target).unwrap(),
            new_hash: format!("{:x}", Sha256::digest(b"new executable")),
            version: "5.6.2".into(),
        };
        fs::write(p.candidate(), b"new executable").unwrap();
        (dir, p)
    }
    #[test]
    fn commits_only_after_successful_launch() {
        let (_dir, p) = fixture();
        transact(&p, || Ok(())).unwrap();
        assert_eq!(hash(&p.target).unwrap(), p.new_hash);
        assert!(!p.backup().exists());
    }
    #[test]
    fn startup_failure_restores_original() {
        let (_dir, p) = fixture();
        assert!(transact(&p, || Err("failed".into())).is_err());
        assert_eq!(hash(&p.target).unwrap(), p.old_hash);
    }
    #[test]
    fn changed_plan_is_rejected_against_parent_hash() {
        let (dir, p) = fixture();
        let stage = dir.path().join(format!("cgs-update-{}", p.nonce));
        fs::create_dir(&stage).unwrap();
        write(&stage.join("owner"), p.nonce.as_bytes()).unwrap();
        let bytes = serde_json::to_vec(&p).unwrap();
        let expected = format!("{:x}", Sha256::digest(&bytes));
        write(&stage.join("plan.json"), &bytes).unwrap();
        assert!(read_bound_plan(&stage, Some(&expected)).is_ok());
        let mut changed = p.clone();
        changed.version = "9.9.9".into();
        fs::write(
            stage.join("plan.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(read_bound_plan(&stage, Some(&expected)).is_err());
    }
    #[test]
    fn concurrent_file_change_cancels_replacement() {
        let (_dir, p) = fixture();
        fs::write(&p.target, b"external change").unwrap();
        assert!(transact(&p, || panic!("must not launch")).is_err());
        assert_eq!(fs::read(&p.target).unwrap(), b"external change");
    }
    #[test]
    fn locked_target_remains_unchanged() {
        use std::os::windows::fs::OpenOptionsExt;
        let (_dir, p) = fixture();
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&p.target)
            .unwrap();
        assert!(transact(&p, || panic!("must not launch")).is_err());
        assert_eq!(hash(&p.target).unwrap(), p.old_hash);
    }
    #[test]
    fn corrupted_candidate_never_replaces_original() {
        let (_dir, p) = fixture();
        fs::write(p.candidate(), b"broken").unwrap();
        assert!(transact(&p, || panic!("must not launch")).is_err());
        assert_eq!(hash(&p.target).unwrap(), p.old_hash);
    }
}
