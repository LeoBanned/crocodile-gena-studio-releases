//! Read-only artwork lookup. Does not alter the hypervisor library, game files or launch profiles.
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameArtwork {
    appid: u64,
    local: Option<crate::steam_library::SteamLibraryCover>,
    urls: Vec<String>,
}

fn read_id(path: &Path) -> Option<u64> {
    if !fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(128)
        .read_to_string(&mut text)
        .ok()?;
    let id = text
        .trim()
        .trim_start_matches('\u{feff}')
        .parse::<u64>()
        .ok()?;
    // Spacewar in a replacement Steam API is not reliable evidence of the actual game.
    (id > 0 && id <= u32::MAX as u64 && id != 480 && id != 228980).then_some(id)
}
fn game_root(exe: &Path) -> Option<PathBuf> {
    for dir in exe.parent()?.ancestors() {
        if dir
            .parent()?
            .file_name()?
            .to_string_lossy()
            .eq_ignore_ascii_case("common")
            && dir
                .parent()?
                .parent()?
                .file_name()?
                .to_string_lossy()
                .eq_ignore_ascii_case("steamapps")
        {
            return Some(dir.to_path_buf());
        }
    }
    None
}
fn identify(exe: &Path) -> Option<u64> {
    if !exe.is_file() || !exe.extension()?.eq_ignore_ascii_case("exe") {
        return None;
    }
    let exe = exe.canonicalize().ok()?;
    let root = game_root(&exe);
    if let Some(game) = root
        .as_deref()
        .and_then(crate::steam_library::installed_game_for_path)
    {
        return Some(game.appid);
    }
    let parent = exe.parent()?;
    for dir in parent.ancestors().take(6) {
        if dir.parent().is_none()
            || dir
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("common"))
        {
            break;
        }
        for name in ["steam_appid.txt", "steam_settings/steam_appid.txt"] {
            if let Some(id) = read_id(&dir.join(name)) {
                return Some(id);
            }
        }
        if root.as_deref() == Some(dir) {
            break;
        }
    }
    // Includes Unity *_Data/Plugins/x86_64/steam_settings, without traversing whole disks.
    let search = root.as_deref().unwrap_or(parent);
    let mut ids = BTreeSet::new();
    for entry in walkdir::WalkDir::new(search)
        .follow_links(false)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            ![".cgs-backups", ".backup_orig", ".git"]
                .iter()
                .any(|s| e.file_name().to_string_lossy().eq_ignore_ascii_case(s))
        })
        .take(5000)
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("steam_appid.txt")
        {
            if let Some(id) = read_id(entry.path()) {
                ids.insert(id);
            }
        }
    }
    if ids.len() == 1 {
        ids.into_iter().next()
    } else {
        None
    }
}

#[tauri::command]
pub(crate) async fn game_artwork(exe_path: String) -> Result<Option<GameArtwork>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(appid) = identify(Path::new(&exe_path)) else { return Ok(None); };
        let local = crate::luatools::resolve_steam_path(None)
            .and_then(|steam| crate::steam_library::read_local_cover(&steam, appid).ok().flatten());
        Ok(Some(GameArtwork { appid, local, urls: vec![
            format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/header.jpg"),
            format!("https://cdn.akamai.steamstatic.com/steam/apps/{appid}/header.jpg"),
            format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg"),
        ] }))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn game_artwork_store_url(appid: u64) -> Result<Option<String>, String> {
    if appid == 0 || appid > u32::MAX as u64 {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || {
        crate::steam::app_overview(appid).and_then(|app| app.header_image)
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_identifies_nested_exe_before_a_spoofed_appid() {
        let temp = tempfile::tempdir().unwrap();
        let apps = temp.path().join("steamapps");
        let root = apps.join("common/Far Changing Tides");
        let exe = root.join("Binaries/Win64/Game.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"test").unwrap();
        fs::write(exe.parent().unwrap().join("steam_appid.txt"), "480").unwrap();
        fs::write(apps.join("appmanifest_876650.acf"), "\"AppState\" { \"appid\" \"876650\" \"name\" \"FAR: Changing Tides\" \"installdir\" \"Far Changing Tides\" \"StateFlags\" \"4\" }").unwrap();
        assert_eq!(identify(&exe), Some(876650));
    }
    #[test]
    fn standalone_appid_works_but_unknown_and_ambiguous_games_have_no_guessed_cover() {
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("Game.exe");
        fs::write(&exe, b"test").unwrap();
        assert_eq!(identify(&exe), None);
        let settings = temp.path().join("Game_Data/Plugins/x86_64/steam_settings");
        fs::create_dir_all(&settings).unwrap();
        fs::write(settings.join("steam_appid.txt"), "\u{feff}876650\r\n").unwrap();
        assert_eq!(identify(&exe), Some(876650));
        fs::create_dir(temp.path().join("Other")).unwrap();
        fs::write(temp.path().join("Other/steam_appid.txt"), "49520").unwrap();
        assert_eq!(identify(&exe), None);
        fs::write(temp.path().join("steam_appid.txt"), "49520").unwrap();
        assert_eq!(identify(&exe), Some(49520));
    }
    #[test]
    fn backup_metadata_and_spacewar_are_not_mistaken_for_the_game() {
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("Game.exe");
        fs::write(&exe, b"test").unwrap();
        fs::create_dir_all(temp.path().join(".cgs-backups/old")).unwrap();
        fs::write(
            temp.path().join(".cgs-backups/old/steam_appid.txt"),
            "49520",
        )
        .unwrap();
        fs::write(temp.path().join("steam_appid.txt"), "480").unwrap();
        assert_eq!(identify(&exe), None);
    }
}
