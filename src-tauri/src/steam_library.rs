use crate::luatools::resolve_steam_path;
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstalledSteamGame {
    pub appid: u64,
    pub name: String,
    pub game_dir: String,
    pub library_path: String,
    pub header_image: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SteamLibraryScan {
    pub games: Vec<InstalledSteamGame>,
    pub libraries: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SteamLibraryCover {
    pub bytes: Vec<u8>,
    pub mime: String,
}

fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        text.strip_prefix("\\\\?\\").unwrap_or(&text).to_string()
    }
}

// Steam's text KeyValues: do not flatten the file with a regex, since app lists,
// install depots, and library paths live in different nested sections.
pub(crate) fn parse_vdf(content: &str) -> Result<Value, String> {
    let chars: Vec<char> = content.trim_start_matches('\u{feff}').chars().collect();
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < chars.len() {
        let ch = chars[pos];
        if ch.is_whitespace() {
            pos += 1;
            continue;
        }
        if ch == '/' && chars.get(pos + 1) == Some(&'/') {
            while pos < chars.len() && chars[pos] != '\n' {
                pos += 1;
            }
            continue;
        }
        if ch == '{' || ch == '}' {
            tokens.push(ch.to_string());
            pos += 1;
            continue;
        }
        if ch == '"' {
            pos += 1;
            let mut value = String::new();
            let mut closed = false;
            while pos < chars.len() {
                let ch = chars[pos];
                pos += 1;
                if ch == '"' {
                    closed = true;
                    break;
                }
                if ch == '\\' && matches!(chars.get(pos), Some('\\' | '"')) {
                    value.push(chars[pos]);
                    pos += 1;
                } else {
                    value.push(ch);
                }
            }
            if !closed {
                return Err("Незавершённая строка Steam VDF".into());
            }
            tokens.push(value);
        } else {
            let start = pos;
            while pos < chars.len()
                && !chars[pos].is_whitespace()
                && chars[pos] != '{'
                && chars[pos] != '}'
            {
                pos += 1;
            }
            tokens.push(chars[start..pos].iter().collect());
        }
    }
    fn object(
        tokens: &[String],
        pos: &mut usize,
        nested: bool,
        depth: usize,
    ) -> Result<Value, String> {
        if depth > 64 {
            return Err("Слишком глубокий Steam VDF".into());
        }
        let mut map = Map::new();
        while let Some(key) = tokens.get(*pos) {
            if key == "}" {
                if !nested {
                    return Err("Лишняя скобка Steam VDF".into());
                }
                *pos += 1;
                return Ok(Value::Object(map));
            }
            if key == "{" {
                return Err("Не указано имя секции Steam VDF".into());
            }
            let key = key.to_lowercase();
            *pos += 1;
            let token = tokens.get(*pos).ok_or("Отсутствует значение Steam VDF")?;
            let value = if token == "{" {
                *pos += 1;
                object(tokens, pos, true, depth + 1)?
            } else {
                if token == "}" {
                    return Err("Отсутствует значение Steam VDF".into());
                }
                *pos += 1;
                Value::String(token.clone())
            };
            map.insert(key, value);
        }
        if nested {
            Err("Незавершённая секция Steam VDF".into())
        } else {
            Ok(Value::Object(map))
        }
    }
    object(&tokens, &mut 0, false, 0)
}

fn normalized_library(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy();
    let root = if name.eq_ignore_ascii_case("common")
        && path
            .parent()?
            .file_name()?
            .to_string_lossy()
            .eq_ignore_ascii_case("steamapps")
    {
        path.parent()?.parent()?.to_path_buf()
    } else if name.eq_ignore_ascii_case("steamapps") {
        path.parent()?.to_path_buf()
    } else {
        path.to_path_buf()
    };
    root.join("steamapps").is_dir().then_some(root)
}

fn library_roots(
    steam: Option<&Path>,
    extra: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut roots = BTreeMap::new();
    let mut add = |path: PathBuf| {
        if let Some(root) = normalized_library(&path) {
            let root = fs::canonicalize(&root).unwrap_or(root);
            roots.insert(root.to_string_lossy().to_lowercase(), root);
        }
    };
    if let Some(steam) = steam {
        add(steam.to_path_buf());
        for path in [
            steam.join("steamapps/libraryfolders.vdf"),
            steam.join("config/libraryfolders.vdf"),
        ] {
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            match parse_vdf(&content) {
                Ok(data) => {
                    if let Some(folders) = data.get("libraryfolders").and_then(Value::as_object) {
                        for (id, value) in folders {
                            if id.parse::<u32>().is_err() {
                                continue;
                            }
                            if let Some(path) = value
                                .as_str()
                                .or_else(|| value.get("path").and_then(Value::as_str))
                            {
                                add(PathBuf::from(path));
                            }
                        }
                    }
                }
                Err(error) => warnings.push(format!("{}: {error}", path.display())),
            }
        }
    }
    if let Some(extra) = extra {
        add(extra.to_path_buf());
    }
    roots.into_values().collect()
}

fn games_in_library(root: &Path, warnings: &mut Vec<String>) -> Vec<InstalledSteamGame> {
    let steamapps = root.join("steamapps");
    let common = steamapps.join("common");
    let Ok(entries) = fs::read_dir(&steamapps) else {
        warnings.push(format!(
            "Не удалось прочитать библиотеку: {}",
            root.display()
        ));
        return Vec::new();
    };
    let mut games = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file = entry.file_name().to_string_lossy().to_lowercase();
        let Some(id) = file
            .strip_prefix("appmanifest_")
            .and_then(|s| s.strip_suffix(".acf"))
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        if id == 0 || id == 228980 {
            continue;
        }
        let data = match fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| parse_vdf(&s))
        {
            Ok(data) => data,
            Err(error) => {
                warnings.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let Some(app) = data.get("appstate") else {
            continue;
        };
        if app
            .get("appid")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            != Some(id)
        {
            continue;
        }
        let flags = app
            .get("stateflags")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        if flags & 4 == 0 {
            continue;
        } // FullyInstalled, also with pending updates.
        let Some(install) = app
            .get("installdir")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let relative = Path::new(install);
        if relative.is_absolute()
            || relative
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
        {
            continue;
        }
        let dir = common.join(relative);
        if !dir.is_dir() {
            continue;
        }
        let name = app
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && !s.starts_with("appid_"))
            .unwrap_or(install)
            .to_string();
        games.push(InstalledSteamGame {
            appid: id, name, game_dir: display_path(&dir), library_path: display_path(root),
            header_image: format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{id}/header.jpg"),
        });
    }
    games
}

pub(crate) fn scan_libraries(
    steam: Option<&Path>,
    extra: Option<&Path>,
) -> Result<SteamLibraryScan, String> {
    let mut warnings = Vec::new();
    let roots = library_roots(steam, extra, &mut warnings);
    if roots.is_empty() {
        return Err(
            "Библиотеки Steam не найдены. Выберите папку SteamLibrary, steamapps или common."
                .into(),
        );
    }
    let cache = steam
        .and_then(|path| read_appinfo(path).ok())
        .unwrap_or_default();
    let mut found = BTreeMap::new();
    for root in &roots {
        for mut game in games_in_library(root, &mut warnings) {
            if let Some(data) = cache.get(&game.appid) {
                if matches!(
                    data.pointer("/common/type")
                        .and_then(Value::as_str)
                        .map(str::to_lowercase)
                        .as_deref(),
                    Some("dlc" | "tool" | "music" | "application" | "video")
                ) {
                    continue;
                }
                if let Some(name) = data
                    .pointer("/common/name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    game.name = name.into();
                }
            }
            found.entry(game.appid).or_insert(game);
        }
    }
    let mut games: Vec<_> = found.into_values().collect();
    games.sort_by_key(|game| game.name.to_lowercase());
    Ok(SteamLibraryScan {
        games,
        libraries: roots.iter().map(|p| display_path(p)).collect(),
        warnings,
    })
}

#[tauri::command]
pub(crate) async fn scan_installed_steam_games(
    steam_path: Option<String>,
    library_path: Option<String>,
) -> Result<SteamLibraryScan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let steam = resolve_steam_path(steam_path);
        let extra = library_path.filter(|s| !s.trim().is_empty()).map(PathBuf::from);
        if extra.as_ref().is_some_and(|p| normalized_library(p).is_none()) {
            return Err("В выбранной папке не найдена библиотека Steam. Выберите SteamLibrary, steamapps или common.".into());
        }
        scan_libraries(steam.as_deref(), extra.as_deref())
    }).await.map_err(|error| format!("Сканирование библиотек прервано: {error}"))?
}

#[tauri::command]
pub(crate) async fn steam_library_cover(
    appid: u64,
    steam_path: Option<String>,
) -> Result<Option<SteamLibraryCover>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if appid == 0 {
            return Err("Некорректный AppID".into());
        }
        let steam = resolve_steam_path(steam_path)
            .filter(|path| path.join("appcache/librarycache").is_dir())
            .or_else(|| resolve_steam_path(None));
        let Some(steam) = steam else {
            return Ok(None);
        };
        read_local_cover(&steam, appid)
    })
    .await
    .map_err(|error| format!("Чтение обложки прервано: {error}"))?
}

pub(crate) fn read_local_cover(steam: &Path, appid: u64) -> Result<Option<SteamLibraryCover>, String> {
    let root = steam.join("appcache/librarycache").join(appid.to_string());
    if !root.is_dir() {
        return Ok(None);
    }
    let preferred = [
        "library_header.jpg",
        "header.jpg",
        "library_capsule.jpg",
        "library_600x900.jpg",
    ];
    let files = walkdir::WalkDir::new(&root)
        .follow_links(false)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .collect::<Vec<_>>();
    for name in preferred {
        let Some(file) = files.iter().find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        }) else {
            continue;
        };
        let size = file.metadata().map_err(|error| error.to_string())?.len();
        if size == 0 || size > 3 * 1024 * 1024 {
            continue;
        }
        let bytes = fs::read(file.path()).map_err(|error| error.to_string())?;
        return Ok(Some(SteamLibraryCover {
            bytes,
            mime: "image/jpeg".into(),
        }));
    }
    Ok(None)
}

pub(crate) fn installed_game_for_path(dir: &Path) -> Option<InstalledSteamGame> {
    let dir = fs::canonicalize(dir).ok()?;
    for ancestor in dir.ancestors() {
        if !ancestor
            .file_name()
            .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("common"))
        {
            continue;
        }
        let root = ancestor.parent()?.parent()?;
        return games_in_library(root, &mut Vec::new())
            .into_iter()
            .find(|game| fs::canonicalize(&game.game_dir).ok().as_deref() == Some(dir.as_path()));
    }
    None
}

struct BinaryReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> BinaryReader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or("Некорректная длина appinfo")?;
        let result = self
            .bytes
            .get(self.pos..end)
            .ok_or("Обрезанный appinfo.vdf")?;
        self.pos = end;
        Ok(result)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn text(&mut self) -> Result<String, String> {
        let len = self
            .bytes
            .get(self.pos..)
            .and_then(|b| b.iter().position(|b| *b == 0))
            .ok_or("Незавершённая строка appinfo")?;
        let text = String::from_utf8_lossy(self.take(len)?).into_owned();
        self.take(1)?;
        Ok(text)
    }
    fn object(&mut self, keys: Option<&[String]>, depth: usize) -> Result<Value, String> {
        if depth > 64 {
            return Err("Слишком глубокий appinfo".into());
        }
        let mut result = Map::new();
        loop {
            let kind = self.take(1)?[0];
            if kind == 8 || kind == 11 {
                return Ok(Value::Object(result));
            }
            let key = if let Some(keys) = keys {
                keys.get(self.u32()? as usize)
                    .ok_or("Некорректный индекс appinfo")?
                    .to_lowercase()
            } else {
                self.text()?.to_lowercase()
            };
            let value = match kind {
                0 => self.object(keys, depth + 1)?,
                1 => Value::String(self.text()?),
                2 => Value::from(i32::from_le_bytes(self.take(4)?.try_into().unwrap())),
                3 => Value::from(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
                4 | 6 => Value::from(self.u32()?),
                5 => {
                    let mut text = Vec::new();
                    loop {
                        let ch = u16::from_le_bytes(self.take(2)?.try_into().unwrap());
                        if ch == 0 {
                            break;
                        }
                        text.push(ch);
                    }
                    Value::String(String::from_utf16_lossy(&text))
                }
                7 => Value::from(u64::from_le_bytes(self.take(8)?.try_into().unwrap())),
                10 => Value::from(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
                _ => return Err(format!("Неизвестный тип appinfo: {kind}")),
            };
            result.insert(key, value);
        }
    }
}

fn parse_appinfo(bytes: &[u8]) -> Result<BTreeMap<u64, Value>, String> {
    let mut reader = BinaryReader { bytes, pos: 0 };
    let version = reader.u32()?;
    if ![0x07564427, 0x07564428, 0x07564429].contains(&version) {
        return Err("Неизвестная версия appinfo.vdf".into());
    }
    reader.u32()?;
    let keys = if version == 0x07564429 {
        let offset = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
        let offset = usize::try_from(offset).map_err(|_| "Некорректный offset appinfo")?;
        let mut table = BinaryReader { bytes, pos: offset };
        let count = table.u32()? as usize;
        if count > 1_000_000 {
            return Err("Слишком большая таблица appinfo".into());
        }
        let mut keys = Vec::with_capacity(count);
        for _ in 0..count {
            keys.push(table.text()?);
        }
        Some(keys)
    } else {
        None
    };
    let mut apps = BTreeMap::new();
    loop {
        let appid = reader.u32()? as u64;
        if appid == 0 {
            break;
        }
        let size = reader.u32()? as usize;
        let fixed = if version >= 0x07564428 { 60 } else { 40 };
        let payload = reader.take(size)?;
        if payload.len() < fixed {
            return Err("Некорректная запись appinfo".into());
        }
        let mut data = BinaryReader {
            bytes: &payload[fixed..],
            pos: 0,
        };
        if let Ok(value) = data.object(keys.as_deref(), 0) {
            let app = value.get("appinfo").cloned().unwrap_or(value);
            apps.insert(appid, app);
        }
    }
    Ok(apps)
}

pub(crate) fn read_appinfo(steam: &Path) -> Result<BTreeMap<u64, Value>, String> {
    let path = steam.join("appcache/appinfo.vdf");
    if fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .len()
        > 256 * 1024 * 1024
    {
        return Err("appinfo.vdf слишком большой".into());
    }
    parse_appinfo(&fs::read(path).map_err(|error| error.to_string())?)
}

pub(crate) fn cached_dlc_names(
    apps: &BTreeMap<u64, Value>,
    appid: u64,
) -> BTreeMap<String, String> {
    let mut dlcs = BTreeMap::new();
    let ids = apps
        .get(&appid)
        .and_then(|v| v.pointer("/extended/listofdlc"))
        .and_then(Value::as_str)
        .unwrap_or("");
    for id in ids
        .split(',')
        .filter_map(|s| s.trim().parse::<u64>().ok())
        .filter(|id| *id > 0 && *id != appid)
    {
        let name = apps
            .get(&id)
            .and_then(|v| v.pointer("/common/name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("DLC {id}"));
        dlcs.insert(id.to_string(), name);
    }
    for (id, data) in apps {
        let parent = data.pointer("/extended/dlcforappid");
        if parent.and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        }) != Some(appid)
        {
            continue;
        }
        let name = data
            .pointer("/common/name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("DLC {id}"));
        dlcs.insert(id.to_string(), name);
    }
    dlcs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_library_paths_and_escaped_backslashes() {
        let value = parse_vdf(r#""libraryfolders" { "0" { "path" "C:\\Steam" "apps" { "480" "1" } } "1" "E:\\SteamLibrary" }"#).unwrap();
        assert_eq!(
            value
                .pointer("/libraryfolders/0/path")
                .and_then(Value::as_str),
            Some("C:\\Steam")
        );
        assert_eq!(
            value.pointer("/libraryfolders/1").and_then(Value::as_str),
            Some("E:\\SteamLibrary")
        );
        assert!(parse_vdf("\"broken\" {").is_err());
    }

    #[test]
    fn scans_only_present_installed_games_not_downloads_or_escaped_paths() {
        let dir = std::env::temp_dir().join(format!(
            "cgs-library-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let apps = dir.join("steamapps");
        fs::create_dir_all(apps.join("common/Installed")).unwrap();
        fs::create_dir_all(apps.join("common/Downloading")).unwrap();
        for (id, folder, state) in [
            (10, "Installed", 4),
            (11, "Downloading", 1026),
            (12, "Gone", 4),
            (13, "../escape", 4),
        ] {
            fs::write(apps.join(format!("appmanifest_{id}.acf")), format!(r#""AppState" {{ "appid" "{id}" "name" "Test {id}" "StateFlags" "{state}" "installdir" "{folder}" }}"#)).unwrap();
        }
        let result = scan_libraries(None, Some(&apps.join("common"))).unwrap();
        assert_eq!(result.games.len(), 1);
        assert_eq!(result.games[0].appid, 10);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn local_dlc_sources_merge_and_do_not_include_depot_ids() {
        let apps = BTreeMap::from([
            (
                10,
                serde_json::json!({"extended":{"listofdlc":"11,12,11"},"depots":{"999":{}}}),
            ),
            (
                11,
                serde_json::json!({"common":{"name":"Pack One"},"extended":{"dlcforappid":"10"}}),
            ),
            (
                13,
                serde_json::json!({"common":{"name":"Hidden Pack"},"extended":{"dlcforappid":10}}),
            ),
        ]);
        let dlcs = cached_dlc_names(&apps, 10);
        assert_eq!(dlcs.len(), 3);
        assert_eq!(dlcs.get("13").map(String::as_str), Some("Hidden Pack"));
        assert!(!dlcs.contains_key("999"));
    }

    #[test]
    fn reads_real_local_steam_library_when_requested() {
        let Ok(root) = std::env::var("CGS_TEST_STEAM_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let apps = read_appinfo(&root).unwrap();
        assert!(!apps.is_empty());
        let result = scan_libraries(Some(&root), None).unwrap();
        assert!(!result.games.is_empty());
        assert!(result.games.iter().all(|g| Path::new(&g.game_dir).is_dir()));
        assert!(result.games.iter().any(|game| {
            read_local_cover(&root, game.appid)
                .ok()
                .flatten()
                .is_some_and(|cover| !cover.bytes.is_empty())
        }));
    }
}
