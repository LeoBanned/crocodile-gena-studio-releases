use crate::models::OperationResult;
use serde_json::{json, Value};

#[tauri::command]
pub fn millennium_plugin_status(_steam_path: Option<String>) -> Value {
    json!({
        "status": "unsupported",
        "message": "Millennium из комплекта предназначен для Steam под Windows",
        "millenniumInstalled": false,
        "pluginInstalled": false,
        "pluginCurrent": false,
        "pluginVersion": "",
        "bundledVersion": "",
        "pluginPath": ""
    })
}

fn unsupported() -> OperationResult {
    OperationResult::error(
        "Millennium из комплекта предназначен для Steam под Windows",
        vec![],
    )
}

#[tauri::command]
pub fn millennium_install_plugin(_steam_path: Option<String>) -> OperationResult {
    unsupported()
}

#[tauri::command]
pub fn millennium_remove_plugin(_steam_path: Option<String>) -> OperationResult {
    unsupported()
}

#[tauri::command]
pub fn millennium_install_bundled(_steam_path: Option<String>) -> OperationResult {
    unsupported()
}
