use serde_json::Value;

fn unsupported<T>() -> Result<T, String> {
    Err("Гипервизор KVC/DRV работает только с Windows-драйверами".into())
}

#[tauri::command]
pub(crate) fn get_app_status() -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn check_asset_updates(_app: tauri::AppHandle) -> Result<(), String> {
    Ok(())
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
pub(crate) fn set_game_launcher(
    _app: tauri::AppHandle,
    _id: String,
    _launcher_id: String,
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
