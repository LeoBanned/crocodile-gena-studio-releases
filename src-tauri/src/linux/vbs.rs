use serde_json::Value;

fn unsupported<T>() -> Result<T, String> {
    Err("VBS, DSE и тестовый режим относятся только к загрузчику Windows".into())
}

#[tauri::command]
pub(crate) async fn vbs_scan(_app: tauri::AppHandle) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn vbs_disable(_app: tauri::AppHandle) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn vbs_restore(_app: tauri::AppHandle) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) async fn vbs_set_test_mode(
    _app: tauri::AppHandle,
    _enabled: bool,
) -> Result<Value, String> {
    unsupported()
}

#[tauri::command]
pub(crate) fn vbs_restart() -> Result<(), String> {
    unsupported()
}
