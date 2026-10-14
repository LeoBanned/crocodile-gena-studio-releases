use serde_json::Value;

fn unsupported<T>() -> Result<T, String> {
    Err("Модуль античитов управляет службами и реестром Windows и недоступен в Linux".into())
}

#[tauri::command]
pub(crate) async fn anticheat_scan() -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn anticheat_remove(_id: String, _backup_dir: String) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn anticheat_remove_all(_backup_dir: String) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn anticheat_pick_backup_directory(
    _window: tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    unsupported()
}
