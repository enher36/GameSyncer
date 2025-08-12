use anyhow::Result;
use steam_cloud_sync_core::{Game, GameSave};
use std::path::PathBuf;

pub mod pages;
pub mod models;
pub mod services;
pub mod service_manager;
pub mod view_model;
pub mod ui;
pub mod settings;

pub use pages::*;
pub use models::*;
pub use services::*;
pub use service_manager::*;
pub use view_model::*; // 重新添加新的AppViewModel导出
pub use ui::*;
pub use settings::*;

#[derive(Clone, Debug, PartialEq)]
pub enum SyncState {
    Pending,   // Has changes to sync
    Synced,    // Up to date
    Unknown,   // No save detected or sync status unknown
}

#[derive(Clone, Debug)]
pub struct GameWithSave {
    pub game: Game,
    pub save_info: Option<GameSave>,
    pub save_detection_status: SaveDetectionStatus,
    pub sync_enabled: bool, // New field to track if game is selected for sync
    pub cloud_saves: Vec<steam_cloud_sync_cloud::SaveMetadata>, // Available cloud saves
    pub downloading: bool, // Track download state
    pub sync_state: SyncState, // Current sync state
    pub sync_progress: Option<f32>, // Progress 0.0-1.0 when syncing
}

#[derive(Clone, Debug)]
pub enum SaveDetectionStatus {
    NotScanned,
    Scanning,
    Found,
    NotFound,
    ManualMappingRequired,
}

#[derive(Clone, Debug)]
pub struct UndoableSync {
    pub game_id: String,
    pub game_name: String,
    pub backup_path: PathBuf,
    pub original_path: PathBuf,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub struct LocalizationManager {
    current_language: String,
}

impl LocalizationManager {
    pub fn new() -> Self {
        Self {
            current_language: "en-US".to_string(),
        }
    }
    
    pub fn get_string(&self, key: &str) -> String {
        match (self.current_language.as_str(), key) {
            ("zh-CN", "AppTitle") => "Steam云同步".to_string(),
            ("zh-CN", "SyncNow") => "立即同步".to_string(),
            ("zh-CN", "Home") => "主页".to_string(),
            ("zh-CN", "History") => "历史记录".to_string(),
            ("zh-CN", "Settings") => "设置".to_string(),
            ("zh-CN", "Scanning") => "正在扫描游戏...".to_string(),
            ("zh-CN", "LastSync") => "上次同步".to_string(),
            ("zh-CN", "Refresh") => "刷新".to_string(),
            ("zh-CN", "Language") => "语言".to_string(),
            ("zh-CN", "InterfaceLanguage") => "界面语言：".to_string(),
            ("zh-CN", "ApplyLanguage") => "应用语言".to_string(),
            ("zh-CN", "CloudBackend") => "云存储后端".to_string(),
            ("zh-CN", "SaveBackendSettings") => "保存后端设置".to_string(),
            ("zh-CN", "Application") => "应用程序".to_string(),
            ("zh-CN", "StartWithWindows") => "随Windows启动".to_string(),
            ("zh-CN", "EnableRateLimiting") => "启用速率限制".to_string(),
            ("zh-CN", "GameName") => "游戏名称".to_string(),
            ("zh-CN", "Direction") => "方向".to_string(),
            ("zh-CN", "Timestamp") => "时间戳".to_string(),
            ("zh-CN", "Duration") => "持续时间".to_string(),
            ("zh-CN", "Result") => "结果".to_string(),
            ("zh-CN", "Ready") => "就绪".to_string(),
            ("zh-CN", "UserID") => "用户ID".to_string(),
            ("zh-CN", "UserIDDescription") => "用于区分不同用户存档的唯一标识符：".to_string(),
            ("zh-CN", "UserIDNote") => "注意：更改此ID将创建单独的存档文件夹。之前的存档不会自动迁移。".to_string(),
            ("zh-CN", "Download") => "下载".to_string(),
            ("zh-CN", "CloudSaves") => "云端存档".to_string(),
            ("zh-CN", "NoCloudSaves") => "未找到云端存档".to_string(),
            ("zh-CN", "Downloading") => "下载中...".to_string(),
            ("zh-CN", "RefreshCloudSaves") => "刷新云端存档".to_string(),
            ("zh-CN", "DefaultDownloadLocation") => "默认下载位置".to_string(),
            (_, "AppTitle") => "SteamCloudSync".to_string(),
            (_, "SyncNow") => "Sync Now".to_string(),
            (_, "Home") => "Home".to_string(),
            (_, "History") => "History".to_string(),
            (_, "Settings") => "Settings".to_string(),
            (_, "Scanning") => "Scanning games...".to_string(),
            (_, "LastSync") => "Last sync".to_string(),
            (_, "Refresh") => "Refresh".to_string(),
            (_, "Language") => "Language".to_string(),
            (_, "InterfaceLanguage") => "Interface Language:".to_string(),
            (_, "ApplyLanguage") => "Apply Language".to_string(),
            (_, "CloudBackend") => "Cloud Backend".to_string(),
            (_, "SaveBackendSettings") => "Save Backend Settings".to_string(),
            (_, "Application") => "Application".to_string(),
            (_, "StartWithWindows") => "Start with Windows".to_string(),
            (_, "EnableRateLimiting") => "Enable rate limiting".to_string(),
            (_, "GameName") => "Game Name".to_string(),
            (_, "Direction") => "Direction".to_string(),
            (_, "Timestamp") => "Timestamp".to_string(),
            (_, "Duration") => "Duration".to_string(),
            (_, "Result") => "Result".to_string(),
            (_, "Ready") => "Ready".to_string(),
            (_, "UserID") => "User ID".to_string(),
            (_, "UserIDDescription") => "Unique identifier to separate your saves from other users:".to_string(),
            (_, "UserIDNote") => "Note: Changing this ID will create a separate save folder. Previous saves won't be automatically migrated.".to_string(),
            (_, "Download") => "Download".to_string(),
            (_, "CloudSaves") => "Cloud Saves".to_string(),
            (_, "NoCloudSaves") => "No cloud saves found".to_string(),
            (_, "Downloading") => "Downloading...".to_string(),
            (_, "RefreshCloudSaves") => "Refresh Cloud Saves".to_string(),
            (_, "DefaultDownloadLocation") => "Default Download Location".to_string(),
            _ => key.to_string(),
        }
    }
    
    pub fn set_language(&mut self, language: String) {
        self.current_language = language;
    }
}