use serde_json::Value;

#[tauri::command]
pub(crate) fn steam_launch_status(_id: String, _account: Option<String>) -> Result<Value, String> { unsupported() }
#[tauri::command]
pub(crate) fn set_steam_launch(_id: String, _enabled: bool, _account: Option<String>) -> Result<Value, String> { unsupported() }
#[tauri::command]
pub(crate) fn steam_launch_accounts() -> Result<Value, String> { unsupported() }

fn unsupported<T>() -> Result<T, String> {
    Err("Crocodile-HV доступен только в Windows".into())
}

#[tauri::command]
pub(crate) fn get_app_status() -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn list_games() -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn select_game_exe() -> Result<Option<String>, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn add_game(
    _app: tauri::AppHandle,
    _exe_path: String,
    _launcher_id: Option<String>,
) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn set_selected_game(_id: String) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn remove_game(_id: String) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn verify_game(_app: tauri::AppHandle, _id: String) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn launch_selected_game(_app: tauri::AppHandle) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn is_selected_game_running() -> Result<bool, String> {
    unsupported()
}
