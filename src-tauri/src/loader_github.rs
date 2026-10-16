//! Read-only discovery; DLL installation is a separate, explicit user action.
use crate::{lua_loader, models::OperationResult};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const API: &str = "https://api.github.com/repos/LeoBanned/pirate-station-hv";
const RAW: &str = "https://raw.githubusercontent.com/LeoBanned/pirate-station-hv";
pub(crate) const MAX_DLL: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct Commit {
    sha: String,
    commit: CommitBody,
}
#[derive(Clone, Debug, Deserialize)]
struct CommitBody {
    tree: TreeId,
}
#[derive(Clone, Debug, Deserialize)]
struct TreeId {
    sha: String,
}
#[derive(Clone, Debug, Deserialize)]
struct Tree {
    tree: Vec<Entry>,
    truncated: bool,
}
#[derive(Clone, Debug, Deserialize)]
struct Entry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    mode: String,
    sha: String,
    size: Option<usize>,
}
#[derive(Clone, Debug)]
struct Snapshot {
    commit: String,
    variants: BTreeMap<String, Vec<RemoteFile>>,
}
#[derive(Clone, Debug)]
struct RemoteFile {
    path: String,
    target: String,
    sha: String,
    size: usize,
}

pub(crate) struct Bundle {
    pub variant: String,
    pub files: BTreeMap<String, Vec<u8>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    path: String,
    bytes: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantInfo {
    variant: String,
    available: bool,
    update_available: bool,
    files: Vec<FileInfo>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    commit: String,
    selected_variant: String,
    update_available: bool,
    variants: Vec<VariantInfo>,
}

fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent("Crocodile-Gena-Studio-CrocLua")
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}
fn download(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = client.get(url).send().map_err(|_| {
        "Не удалось подключиться к GitHub. Установленные DLL не изменены.".to_string()
    })?;
    if matches!(response.status().as_u16(), 403 | 429) {
        return Err(
            "GitHub ограничил частоту запросов. Повторите проверку позже; DLL не изменены.".into(),
        );
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub вернул HTTP {}. DLL не изменены.",
            response.status().as_u16()
        ));
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("Ответ GitHub превышает допустимый размер.".into());
    }
    let mut bytes = Vec::new();
    response
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Ответ GitHub превышает допустимый размер.".into());
    }
    Ok(bytes)
}
fn valid_id(id: &str) -> bool {
    id.len() == 40 && id.bytes().all(|c| c.is_ascii_hexdigit())
}
pub(crate) fn blob_hash(bytes: &[u8]) -> String {
    let mut digest = Sha1::new();
    digest.update(format!("blob {}\0", bytes.len()).as_bytes());
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}
fn snapshot_from_tree(commit: &str, tree: Tree) -> Result<Snapshot, String> {
    if !valid_id(commit) || tree.truncated {
        return Err("GitHub вернул неполный или некорректный список файлов.".into());
    }
    let mut variants = BTreeMap::new();
    for variant in ["standard", "compatibility"] {
        let folder = if variant == "standard"
            && !tree.tree.iter().any(|e| e.path.starts_with("standard/"))
        {
            "standart"
        } else {
            variant
        };
        let mut files = Vec::new();
        for entry in tree.tree.iter().filter(|e| {
            e.path.starts_with(&format!("{folder}/"))
                && e.path.to_ascii_lowercase().ends_with(".dll")
        }) {
            let name = entry.path.strip_prefix(&format!("{folder}/")).unwrap();
            let canonical = [
                "OpenSteamTool.dll",
                "xinput1_4.dll",
                "dwmapi.dll",
                "version.dll",
            ]
            .into_iter()
            .find(|n| n.eq_ignore_ascii_case(name))
            .ok_or("В комплекте GitHub найдена неподдерживаемая DLL или подпапка")?;
            let target = if variant == "compatibility" && canonical == "dwmapi.dll" {
                "bin/dwmapi.dll".to_string()
            } else {
                canonical.into()
            };
            if entry.kind != "blob"
                || !matches!(entry.mode.as_str(), "100644" | "100755")
                || !valid_id(&entry.sha)
                || !entry.size.is_some_and(|n| (1024..=MAX_DLL).contains(&n))
                || files.iter().any(|f: &RemoteFile| f.target == target)
            {
                return Err(format!("Некорректный файл GitHub: {}", entry.path));
            }
            files.push(RemoteFile {
                path: entry.path.clone(),
                target,
                sha: entry.sha.clone(),
                size: entry.size.unwrap(),
            });
        }
        let paths = files
            .iter()
            .map(|f| (f.target.clone(), String::new()))
            .collect::<BTreeMap<_, _>>();
        lua_loader::validate_layout(variant,2,&paths).map_err(|_|format!("Комплект {folder} на GitHub неполон. Нужны OpenSteamTool.dll, xinput1_4.dll и соответствующая прокси-DLL."))?;
        files.sort_by(|a, b| a.target.cmp(&b.target));
        variants.insert(variant.into(), files);
    }
    Ok(Snapshot {
        commit: commit.into(),
        variants,
    })
}
fn fetch_snapshot() -> Result<Snapshot, String> {
    let client = client()?;
    let commit: Commit = serde_json::from_slice(&download(
        &client,
        &format!("{API}/commits/main"),
        512 * 1024,
    )?)
    .map_err(|_| "Некорректный ответ GitHub о версии")?;
    if !valid_id(&commit.commit.tree.sha) {
        return Err("Некорректный идентификатор дерева GitHub.".into());
    }
    let tree: Tree = serde_json::from_slice(&download(
        &client,
        &format!("{API}/git/trees/{}?recursive=1", commit.commit.tree.sha),
        2 * 1024 * 1024,
    )?)
    .map_err(|_| "Некорректный список файлов GitHub")?;
    snapshot_from_tree(&commit.sha, tree)
}
type Cached = Option<(Instant, Result<Snapshot, String>)>;
fn snapshot(force: bool) -> Result<Snapshot, String> {
    static CACHE: OnceLock<Mutex<Cached>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "Перезапустите проверку GitHub")?;
    if let Some((when, result)) = cache.as_ref() {
        let ttl = if result.is_ok() { 300 } else { 30 };
        if when.elapsed() < Duration::from_secs(if force { 3 } else { ttl }) {
            return result.clone();
        }
    }
    let result = fetch_snapshot();
    *cache = Some((Instant::now(), result.clone()));
    result
}
fn file_current(steam: &Path, file: &RemoteFile) -> Result<bool, String> {
    Ok(lua_loader::read_optional(&steam.join(&file.target))?
        .is_some_and(|b| b.len() == file.size && blob_hash(&b) == file.sha))
}
pub(crate) fn fetch_bundle(variant: &str, steam: &Path) -> Result<Bundle, String> {
    if !matches!(variant, "standard" | "compatibility") {
        return Err("Неизвестный комплект DLL".into());
    }
    let snapshot = snapshot(true)?;
    let remote = snapshot
        .variants
        .get(variant)
        .ok_or("Комплект не найден на GitHub")?;
    let client = client()?;
    let mut files = BTreeMap::new();
    for file in remote {
        let bytes = if file_current(steam, file)? {
            lua_loader::read_optional(&steam.join(&file.target))?
                .ok_or("Файл изменился во время проверки")?
        } else {
            download(
                &client,
                &format!("{RAW}/{}/{}", snapshot.commit, file.path),
                MAX_DLL,
            )?
        };
        if bytes.len() != file.size || blob_hash(&bytes) != file.sha {
            return Err(format!(
                "Проверка целостности {} не пройдена. DLL не изменены.",
                file.target
            ));
        }
        validate_dll(&bytes)?;
        files.insert(file.target.clone(), bytes);
    }
    Ok(Bundle {
        variant: variant.into(),
        files,
    })
}
pub(crate) fn validate_dll(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 1024 || bytes.len() > MAX_DLL || !bytes.starts_with(b"MZ") {
        return Err("Вместо DLL получен некорректный файл. Установка отменена.".into());
    }
    let offset = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
    if offset.checked_add(24).is_none_or(|end| end > bytes.len())
        || &bytes[offset..offset + 4] != b"PE\0\0"
        || u16::from_le_bytes(bytes[offset + 22..offset + 24].try_into().unwrap()) & 0x2000 == 0
    {
        return Err("Файл не является корректной PE DLL. Установка отменена.".into());
    }
    Ok(())
}
fn check(steam_path: Option<String>, force: bool) -> Result<UpdateInfo, String> {
    let steam = lua_loader::resolve(steam_path)?;
    let selected = lua_loader::selected_variant(&steam).unwrap_or_else(|_| "standard".into());
    let snapshot = snapshot(force)?;
    let mut variants = Vec::new();
    for (variant, files) in &snapshot.variants {
        let mut current = true;
        for file in files {
            current &= file_current(&steam, file)?;
        }
        variants.push(VariantInfo {
            variant: variant.clone(),
            available: true,
            update_available: !current,
            files: files
                .iter()
                .map(|f| FileInfo {
                    path: f.target.clone(),
                    bytes: f.size,
                })
                .collect(),
        });
    }
    let update_available = variants
        .iter()
        .any(|v| v.variant == selected && v.update_available);
    Ok(UpdateInfo {
        commit: snapshot.commit,
        selected_variant: selected,
        update_available,
        variants,
    })
}
#[tauri::command]
pub async fn luatools_check_loader_updates(
    steam_path: Option<String>,
    force: Option<bool>,
) -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || check(steam_path, force.unwrap_or(false)))
        .await
        .map_err(|e| e.to_string())?
}
pub(crate) fn update(steam: &Path, variant: &str) -> OperationResult {
    match fetch_bundle(variant, steam) {
        Ok(bundle) => lua_loader::install_downloaded(steam, bundle),
        Err(error) => OperationResult::error(error, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path: &str) -> Entry {
        Entry {
            path: path.into(),
            kind: "blob".into(),
            mode: "100644".into(),
            sha: "a".repeat(40),
            size: Some(2000),
        }
    }
    fn tree() -> Tree {
        Tree {
            truncated: false,
            tree: vec![
                entry("standard/OpenSteamTool.dll"),
                entry("standard/xinput1_4.dll"),
                entry("standard/version.dll"),
                entry("compatibility/OpenSteamTool.dll"),
                entry("compatibility/xinput1_4.dll"),
                entry("compatibility/dwmapi.dll"),
            ],
        }
    }
    #[test]
    fn derives_only_allowlisted_destinations() {
        let s = snapshot_from_tree(&"a".repeat(40), tree()).unwrap();
        assert!(s.variants["standard"]
            .iter()
            .any(|f| f.target == "version.dll"));
        assert!(s.variants["compatibility"]
            .iter()
            .any(|f| f.target == "bin/dwmapi.dll"));
    }
    #[test]
    fn rejects_incomplete_links_and_unexpected_dlls() {
        let mut t = tree();
        t.tree.pop();
        assert!(snapshot_from_tree(&"a".repeat(40), t).is_err());
        let mut t = tree();
        t.tree[0].mode = "120000".into();
        assert!(snapshot_from_tree(&"a".repeat(40), t).is_err());
        let mut t = tree();
        t.tree.push(entry("standard/sub/other.dll"));
        assert!(snapshot_from_tree(&"a".repeat(40), t).is_err());
    }
    #[test]
    fn rejects_html_and_corrupt_pe() {
        assert!(validate_dll(b"<html>failure</html>").is_err());
        assert!(validate_dll(&vec![77; 2000]).is_err());
    }
    #[test]
    fn git_blob_hash_matches_known_empty_blob() {
        assert_eq!(blob_hash(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }
    #[test]
    fn detects_unchanged_bytes_without_a_version_file() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"same contents";
        std::fs::write(dir.path().join("version.dll"), bytes).unwrap();
        let file = RemoteFile {
            path: "standard/version.dll".into(),
            target: "version.dll".into(),
            sha: blob_hash(bytes),
            size: bytes.len(),
        };
        assert!(file_current(dir.path(), &file).unwrap());
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "Reads the user-provided public GitHub repository; installs only in a temporary sandbox"]
    fn live_github_both_bundles_and_plugin_cache() {
        let dir = tempfile::tempdir().unwrap();
        let steam = dir.path();
        std::fs::write(steam.join("steam.exe"), []).unwrap();
        let info = check(Some(steam.display().to_string()), true).unwrap();
        assert!(info.update_available);
        assert_eq!(
            std::fs::read_dir(steam).unwrap().count(),
            1,
            "Checking must not create files"
        );
        let first = fetch_bundle("standard", steam).unwrap();
        assert!(first.files.contains_key("version.dll"));
        let expected = first.files["version.dll"].clone();
        let result = lua_loader::install_downloaded(steam, first);
        assert!(result.success, "{}", result.message);
        assert!(!steam.join("dwmapi.dll").exists());
        lua_loader::ensure(steam).unwrap();
        std::fs::remove_file(steam.join("version.dll")).unwrap();
        let backend =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/crocodile-manifest/backend");
        let mut command = std::process::Command::new("powershell");
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        let response = command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(backend.join("install-loader.ps1"))
            .arg("-SteamPath")
            .arg(steam)
            .arg("-ResourcePath")
            .arg(backend.join("loader"))
            .output()
            .unwrap();
        assert!(
            response.status.success(),
            "{}",
            String::from_utf8_lossy(&response.stderr)
        );
        assert_eq!(std::fs::read(steam.join("version.dll")).unwrap(), expected);
        let second = fetch_bundle("compatibility", steam).unwrap();
        let proxy = second.files["bin/dwmapi.dll"].clone();
        let result = lua_loader::install_downloaded(steam, second);
        assert!(result.success, "{}", result.message);
        assert!(!steam.join("version.dll").exists());
        assert!(!steam.join("dwmapi.dll").exists());
        assert_eq!(std::fs::read(steam.join("bin/dwmapi.dll")).unwrap(), proxy);
        std::fs::remove_file(steam.join("bin/dwmapi.dll")).unwrap();
        lua_loader::ensure(steam).unwrap();
        assert_eq!(std::fs::read(steam.join("bin/dwmapi.dll")).unwrap(), proxy);
        let info = check(Some(steam.display().to_string()), false).unwrap();
        assert!(!info.update_available);
    }
}
