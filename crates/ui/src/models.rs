use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHistoryItem {
    pub game_name: String,
    pub direction: SyncDirection,
    pub timestamp: DateTime<Utc>,
    pub duration: Duration,
    pub result: SyncResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncDirection {
    Upload,
    Download,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncResult {
    Success,
    Error(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct GameListItem {
    pub app_id: u32,
    pub name: String,
    pub install_path: String,
    pub last_sync: Option<DateTime<Utc>>,
    pub sync_status: SyncStatus,
}

#[derive(Debug, Clone)]
pub enum SyncStatus {
    Synced,
    Syncing,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct CloudVersion {
    pub version_id: String,
    pub upload_time: DateTime<Utc>,
    pub size_bytes: u64,
    pub checksum: String,
}