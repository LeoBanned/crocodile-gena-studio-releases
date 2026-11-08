//! Public, data-only partner catalog. No GitHub credentials are shipped.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::{Read, Write}, path::Path, time::Duration};
use tauri::Manager;

const GIST: &str = "a2d281b56c652fe6db58e4847eb27b85";
const OWNER: &str = "LeoBanned";
const LIMIT: usize = 256 * 1024;
const SEED: &str = include_str!("assets/partners.json");

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum Copy {
    Text(String),
    Localized { #[serde(default)] ru: String, #[serde(default)] en: String },
}
impl Default for Copy { fn default() -> Self { Self::Text(String::new()) } }
impl Copy {
    fn valid(&self, limit: usize) -> bool {
        let valid = |s: &str| s.chars().count() <= limit && !s.chars().any(|c| c.is_control() && c != '\n');
        match self { Self::Text(s) => valid(s), Self::Localized {ru,en} => valid(ru) && valid(en) }
    }
}
fn yes() -> bool { true }
#[derive(Clone, Deserialize, Serialize)]
struct Link { label: Copy, url: String }
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Partner {
    id: String,
    name: String,
    description: Copy,
    #[serde(default)] category: Copy,
    #[serde(default)] tagline: Copy,
    #[serde(default)] features: Vec<Copy>,
    #[serde(default)] tags: Vec<Copy>,
    links: Vec<Link>,
    #[serde(default)] logo_url: String,
    #[serde(default)] banner_url: String,
    #[serde(default)] featured: bool,
    #[serde(default = "yes")] enabled: bool,
    #[serde(default)] order: u32,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Catalog { schema_version: u32, partners: Vec<Partner> }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Response {
    partners: Vec<Partner>,
    source: &'static str,
    stale: bool,
}
fn public_https(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else { return false; };
    let Some(host) = url.host_str() else { return false; };
    let host = host.trim_end_matches('.');
    url.scheme() == "https" && url.username().is_empty() && url.password().is_none() &&
        url.port().is_none_or(|p| p == 443) && value.len() <= 2048 &&
        host.trim_matches(['[',']']).parse::<std::net::IpAddr>().is_err() && host.contains('.') &&
        ![".localhost", ".local", ".internal", ".test", ".invalid"].iter().any(|s| host.ends_with(s))
}
fn image_url(value: &str) -> bool {
    if value.is_empty() { return true; }
    if !public_https(value) { return false; }
    reqwest::Url::parse(value).ok().and_then(|u| u.host_str().map(str::to_owned)).is_some_and(|host|
        ["cdn.discordapp.com", "media.discordapp.net", "raw.githubusercontent.com",
         "gist.githubusercontent.com", "github.com", "user-images.githubusercontent.com"].contains(&host.as_str()))
}
fn parse(content: &str) -> Result<Catalog, String> {
    if content.len() > LIMIT { return Err("Partner catalog exceeds 256 KiB".into()); }
    let mut catalog: Catalog = serde_json::from_str(content).map_err(|e| format!("Invalid partners.json: {e}"))?;
    if catalog.schema_version != 1 || catalog.partners.len() > 60 { return Err("Unsupported partner catalog".into()); }
    let mut ids = HashSet::new();
    for p in &catalog.partners {
        if p.id.is_empty() || p.id.len() > 64 || !p.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') ||
            !ids.insert(&p.id) || p.name.trim().is_empty() || p.name.chars().count() > 80 || p.name.chars().any(char::is_control) ||
            !p.description.valid(2000) || !p.category.valid(100) || !p.tagline.valid(180) ||
            p.features.len() > 6 || !p.features.iter().all(|s| s.valid(180)) ||
            p.tags.len() > 6 || !p.tags.iter().all(|s| s.valid(40)) ||
            p.links.is_empty() || p.links.len() > 3 || !p.links.iter().all(|l| l.label.valid(80) && public_https(&l.url)) ||
            !image_url(&p.logo_url) || !image_url(&p.banner_url) {
            return Err(format!("Invalid partner entry: {}", p.id.chars().take(64).collect::<String>()));
        }
    }
    catalog.partners.sort_by_key(|p| p.order);
    Ok(catalog)
}
fn response(catalog: Catalog, source: &'static str, stale: bool) -> Response {
    Response {partners: catalog.partners.into_iter().filter(|p| p.enabled).collect(), source, stale}
}
fn fallback(cache: &Path, stale: bool) -> Result<Response, String> {
    if fs::metadata(cache).is_ok_and(|m| m.len() <= LIMIT as u64) {
        if let Some(catalog) = fs::read_to_string(cache).ok().and_then(|s| parse(&s).ok()) {
            return Ok(response(catalog, "cache", stale));
        }
    }
    Ok(response(parse(SEED)?, "bundled", stale))
}
fn head_revision(advertisement: &str) -> Option<&str> {
    let end = advertisement.find(" HEAD")?;
    let revision = advertisement.get(end.checked_sub(40)?..end)?;
    revision.bytes().all(|b| b.is_ascii_hexdigit()).then_some(revision)
}
fn bounded(response: reqwest::blocking::Response) -> Result<String, String> {
    let mut bytes = Vec::new();
    response.take(LIMIT as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT { return Err("Partner catalog is too large".into()); }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
fn download() -> Result<Catalog, String> {
    let client = reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10)).https_only(true)
        .user_agent(concat!("CrocodileGenaStudio/", env!("CARGO_PKG_VERSION")))
        .build().map_err(|e| e.to_string())?;
    let refs = client.get(format!("https://gist.github.com/{GIST}.git/info/refs?service=git-upload-pack"))
        .header("Cache-Control", "no-cache").send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let refs = bounded(refs)?;
    let revision = head_revision(&refs).ok_or("Gist revision not found")?;
    let data = client.get(format!("https://gist.githubusercontent.com/{OWNER}/{GIST}/raw/{revision}/partners.json"))
        .header("Cache-Control", "no-cache").send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    parse(&bounded(data)?)
}
fn store(cache: &Path, catalog: &Catalog) -> Result<(), String> {
    let parent = cache.parent().ok_or("Missing cache folder")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(catalog).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    file.persist(cache).map_err(|e| e.to_string())?;
    Ok(())
}
fn load(cache: &Path, refresh: bool) -> Result<Response, String> {
    if !refresh { return fallback(cache, false); }
    match download() {
        Ok(catalog) => { let _ = store(cache, &catalog); Ok(response(catalog, "gist", false)) }
        Err(_) => fallback(cache, true),
    }
}
#[tauri::command]
pub(crate) async fn partners_catalog(app: tauri::AppHandle, refresh: bool) -> Result<Response, String> {
    let cache = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("partners/catalog.json");
    tauri::async_runtime::spawn_blocking(move || load(&cache, refresh)).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_and_public_urls_are_valid() {
        assert_eq!(parse(SEED).unwrap().partners[0].id, "xero-nation");
        for url in ["javascript:alert(1)", "http://example.com", "https://127.0.0.1/a", "https://[::ffff:127.0.0.1]/a", "https://localhost./a", "https://host.local/a", "https://user:pass@example.com", "https://example.com:8443"] { assert!(!public_https(url)); }
        assert!(public_https("https://discord.gg/nationxero"));
        assert!(!image_url("https://example.com/pixel"));
    }
    #[test]
    fn malformed_or_unsafe_catalog_cannot_replace_cache() {
        assert!(parse("{}").is_err());
        assert!(parse(&SEED.replace("https://discord.gg/nationxero", "file:///C:/Windows")).is_err());
        let mut value: serde_json::Value = serde_json::from_str(SEED).unwrap();
        let p = value["partners"][0].clone(); value["partners"].as_array_mut().unwrap().push(p);
        assert!(parse(&value.to_string()).is_err());
        let temp = tempfile::tempdir().unwrap(); let cache = temp.path().join("catalog.json");
        store(&cache, &parse(SEED).unwrap()).unwrap();
        assert_eq!(fallback(&cache, true).unwrap().source, "cache");
        fs::write(&cache, "broken").unwrap();
        assert_eq!(fallback(&cache, true).unwrap().source, "bundled");
    }
    #[test]
    fn removal_and_disabled_partners_are_respected() {
        let empty = parse(r#"{"schemaVersion":1,"partners":[]}"#).unwrap();
        let temp = tempfile::tempdir().unwrap(); let cache = temp.path().join("catalog.json");
        store(&cache, &empty).unwrap(); assert!(fallback(&cache, false).unwrap().partners.is_empty());
        assert!(response(parse(&SEED.replace("\"enabled\": true", "\"enabled\": false")).unwrap(), "gist", false).partners.is_empty());
    }
    #[test]
    fn revision_parser_rejects_invalid_data() {
        assert!(head_revision("bad HEAD").is_none());
        assert!(head_revision(&format!("{} HEAD", "z".repeat(40))).is_none());
        assert_eq!(head_revision(&format!("{} HEAD\0caps", "a".repeat(40))).unwrap(), "a".repeat(40));
    }
    #[test]
    #[ignore = "explicit live Gist integration check"]
    fn live_gist_is_readable_without_credentials() {
        let catalog = download().unwrap();
        assert!(catalog.partners.iter().any(|p| p.id == "xero-nation"));
    }
}
