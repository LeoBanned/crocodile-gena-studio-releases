use crate::lua_loader::{read_optional, reject_link, STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::{Cursor, Read}, path::Path, time::Duration};

const FEED: &str = "https://raw.githubusercontent.com/LeoBanned/crocodile-gena-studio-releases/refs/heads/croclua-dll-feed/";
pub(crate) const CACHE: &str = "config/cgs-luatools-online";
const MAX_DLL: u64 = 16 * 1024 * 1024;
const MAX_ZIP: u64 = 3 * MAX_DLL + 65536;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub revision: u32,
    pub sha256: String,
    pub files: BTreeMap<String, String>,
}

pub(crate) struct Package {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
}

pub(crate) fn embedded_revision() -> u32 {
    include_str!("assets/loader-revision.txt").trim().parse().expect("embedded DLL revision")
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn parse(bytes: &[u8]) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let valid_hash = |value: &str| value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if manifest.revision == 0 || !valid_hash(&manifest.sha256) || manifest.files.len() != STANDARD.len()
        || !STANDARD.iter().all(|(name, _)| manifest.files.get(*name).is_some_and(|value| valid_hash(value))) {
        return Err("Некорректный список файлов обновления DLL".into());
    }
    Ok(manifest)
}

fn verify(manifest: Manifest, files: BTreeMap<String, Vec<u8>>) -> Result<Package, String> {
    for (name, bytes) in &files {
        let pe = bytes.get(60..64).map(|offset| u32::from_le_bytes(offset.try_into().unwrap()) as usize);
        let header = pe.and_then(|offset| offset.checked_add(24).and_then(|end| bytes.get(offset..end)));
        if bytes.len() as u64 > MAX_DLL || !bytes.starts_with(b"MZ")
            || !header.is_some_and(|header| header.starts_with(b"PE\0\0") && header[23] & 0x20 != 0)
            || manifest.files.get(name) != Some(&hash(bytes)) {
            return Err(format!("Проверка DLL не пройдена: {name}"));
        }
    }
    Ok(Package { manifest, files })
}

pub(crate) fn cached(steam: &Path) -> Result<Option<Package>, String> {
    reject_link(&steam.join("config"))?;
    let root = steam.join(CACHE);
    let Some(bytes) = read_optional(&root.join("latest.json"))? else { return Ok(None); };
    let manifest = parse(&bytes)?;
    if manifest.revision <= embedded_revision() {
        return Ok(None);
    }
    let mut files = BTreeMap::new();
    for name in manifest.files.keys() {
        let bytes = read_optional(&root.join(name))?.ok_or("В кэше отсутствует DLL. Повторите обновление.")?;
        files.insert(name.clone(), bytes);
    }
    verify(manifest, files).map(Some)
}

fn fetch(client: &reqwest::blocking::Client, name: &str, max: u64) -> Result<Vec<u8>, String> {
    let response = client.get(format!("{FEED}{name}")).send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response.take(max + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("Размер файла обновления превышает допустимый".into());
    }
    Ok(bytes)
}

fn unpack(manifest: Manifest, bytes: Vec<u8>) -> Result<Package, String> {
    if hash(&bytes) != manifest.sha256 {
        return Err("SHA-256 архива DLL не совпадает".into());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if zip.len() != manifest.files.len() {
        return Err("Неожиданное содержимое архива DLL".into());
    }
    let mut files = BTreeMap::new();
    for name in manifest.files.keys() {
        let file = zip.by_name(name).map_err(|e| e.to_string())?;
        let mut dll = Vec::new();
        file.take(MAX_DLL + 1).read_to_end(&mut dll).map_err(|e| e.to_string())?;
        files.insert(name.clone(), dll);
    }
    verify(manifest, files)
}

pub(crate) fn download(current: u32) -> Result<Option<Package>, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e| e.to_string())?;
    let manifest = parse(&fetch(&client, "latest.json", 16384)?)?;
    if manifest.revision <= current {
        return Ok(None);
    }
    let archive = fetch(&client, &format!("dll-{}.zip", manifest.revision), MAX_ZIP)?;
    unpack(manifest, archive).map(Some)
}
