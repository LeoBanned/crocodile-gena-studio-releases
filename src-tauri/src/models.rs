use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DetectedGame {
    pub detected: bool,
    pub appid: Option<u64>,
    pub name: Option<String>,
    pub header_image: Option<String>,
    pub dlcs: BTreeMap<String, String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub game_dir: String,
    pub exes: Vec<String>,
    pub steam_api_paths: Vec<String>,
    pub detected_game: DetectedGame,
    pub status: String,
    pub engine: String,
    pub has_eos: bool,
    pub has_eos_backup: bool,
    pub has_epicfix: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    pub appid: u64,
    pub name: String,
    pub header_image: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDetails {
    pub appid: u64,
    pub name: String,
    pub header_image: Option<String>,
    pub short_description: Option<String>,
    pub release_date: Option<String>,
    pub dlcs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
    pub logs: Vec<String>,
}

impl OperationResult {
    pub fn ok(message: impl Into<String>, logs: Vec<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            logs,
        }
    }

    pub fn error(message: impl Into<String>, logs: Vec<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            logs,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub game_dir: String,
    pub real_appid: u64,
    #[serde(default = "default_fake_appid")]
    pub fake_appid: u64,
    #[serde(default)]
    pub dlcs: BTreeMap<String, String>,
    #[serde(default)]
    pub install_epicfix: bool,
    #[serde(default = "default_unlock_dlcs")]
    pub unlock_all_dlcs: bool,
}

fn default_fake_appid() -> u64 {
    480
}
fn default_unlock_dlcs() -> bool {
    true
}
