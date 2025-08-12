use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use sqlx::{FromRow, Row, Type, Decode, Sqlite, encode::IsNull, Encode};

/// Cloud operation types
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "operation_type", rename_all = "lowercase")]
pub enum CloudOperationType {
    Upload,
    Download,
    Delete,
    List,
    Restore,
}

/// Cloud operation status
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "operation_status", rename_all = "lowercase")]
pub enum CloudOperationStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

/// Cloud operation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudOperation {
    pub id: Uuid,
    pub game_id: String,
    pub operation_type: CloudOperationType,
    pub status: CloudOperationStatus,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub checksum: Option<String>,
    pub error_message: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: Option<f32>, // 0.0 to 1.0
    pub metadata: Option<String>, // JSON metadata
}

// Custom FromRow implementation to handle UUID stored as TEXT
impl FromRow<'_, sqlx::sqlite::SqliteRow> for CloudOperation {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        // Parse UUID from TEXT field
        let id_str: String = row.try_get("id")?;
        let id = Uuid::parse_str(&id_str)
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "id".to_string(),
                source: Box::new(e),
            })?;

        // Parse timestamps from TEXT fields
        let started_at_str: String = row.try_get("started_at")?;
        let started_at = DateTime::parse_from_rfc3339(&started_at_str)
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "started_at".to_string(),
                source: Box::new(e),
            })?
            .with_timezone(&Utc);

        let completed_at = if let Ok(completed_at_str) = row.try_get::<Option<String>, _>("completed_at") {
            if let Some(s) = completed_at_str {
                Some(DateTime::parse_from_rfc3339(&s)
                    .map_err(|e| sqlx::Error::ColumnDecode {
                        index: "completed_at".to_string(),
                        source: Box::new(e),
                    })?
                    .with_timezone(&Utc))
            } else {
                None
            }
        } else {
            None
        };

        // Parse operation type from TEXT
        let operation_type_str: String = row.try_get("operation_type")?;
        let operation_type = match operation_type_str.as_str() {
            "upload" => CloudOperationType::Upload,
            "download" => CloudOperationType::Download,
            "delete" => CloudOperationType::Delete,
            "list" => CloudOperationType::List,
            "restore" => CloudOperationType::Restore,
            _ => return Err(sqlx::Error::ColumnDecode {
                index: "operation_type".to_string(),
                source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation type")),
            }),
        };

        // Parse status from TEXT
        let status_str: String = row.try_get("status")?;
        let status = match status_str.as_str() {
            "pending" => CloudOperationStatus::Pending,
            "inprogress" => CloudOperationStatus::InProgress,
            "completed" => CloudOperationStatus::Completed,
            "failed" => CloudOperationStatus::Failed,
            "cancelled" => CloudOperationStatus::Cancelled,
            _ => return Err(sqlx::Error::ColumnDecode {
                index: "status".to_string(),
                source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation status")),
            }),
        };

        Ok(CloudOperation {
            id,
            game_id: row.try_get("game_id")?,
            operation_type,
            status,
            file_path: row.try_get("file_path")?,
            file_size: row.try_get("file_size")?,
            checksum: row.try_get("checksum")?,
            error_message: row.try_get("error_message")?,
            started_at,
            completed_at,
            progress: row.try_get("progress")?,
            metadata: row.try_get("metadata")?,
        })
    }
}

impl CloudOperation {
    pub fn new(game_id: String, operation_type: CloudOperationType) -> Self {
        Self {
            id: Uuid::new_v4(),
            game_id,
            operation_type,
            status: CloudOperationStatus::Pending,
            file_path: None,
            file_size: None,
            checksum: None,
            error_message: None,
            started_at: Utc::now(),
            completed_at: None,
            progress: None,
            metadata: None,
        }
    }
}

/// Sync session record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub games_synced: i32,
    pub operations_count: i32,
    pub total_bytes: Option<i64>,
    pub success: Option<bool>,
    pub error_message: Option<String>,
}

// Custom FromRow implementation to handle UUID stored as TEXT
impl FromRow<'_, sqlx::sqlite::SqliteRow> for SyncSession {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        // Parse UUID from TEXT field
        let id_str: String = row.try_get("id")?;
        let id = Uuid::parse_str(&id_str)
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "id".to_string(),
                source: Box::new(e),
            })?;

        // Parse timestamps from TEXT fields
        let started_at_str: String = row.try_get("started_at")?;
        let started_at = DateTime::parse_from_rfc3339(&started_at_str)
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "started_at".to_string(),
                source: Box::new(e),
            })?
            .with_timezone(&Utc);

        let completed_at = if let Ok(completed_at_str) = row.try_get::<Option<String>, _>("completed_at") {
            if let Some(s) = completed_at_str {
                Some(DateTime::parse_from_rfc3339(&s)
                    .map_err(|e| sqlx::Error::ColumnDecode {
                        index: "completed_at".to_string(),
                        source: Box::new(e),
                    })?
                    .with_timezone(&Utc))
            } else {
                None
            }
        } else {
            None
        };

        Ok(SyncSession {
            id,
            started_at,
            completed_at,
            games_synced: row.try_get("games_synced")?,
            operations_count: row.try_get("operations_count")?,
            total_bytes: row.try_get("total_bytes")?,
            success: row.try_get("success")?,
            error_message: row.try_get("error_message")?,
        })
    }
}

impl SyncSession {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            started_at: Utc::now(),
            completed_at: None,
            games_synced: 0,
            operations_count: 0,
            total_bytes: None,
            success: None,
            error_message: None,
        }
    }
}

/// Game-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GameConfig {
    pub game_id: String,
    pub enabled: bool,
    pub auto_sync: bool,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub local_path: Option<String>,
    pub cloud_path: Option<String>,
    pub exclusion_patterns: Option<String>, // JSON array of patterns
    pub compression_enabled: bool,
    pub max_versions: Option<i32>,
    pub sync_direction: Option<String>, // "bidirectional", "upload_only", "download_only"
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl GameConfig {
    pub fn new(game_id: String) -> Self {
        let now = Utc::now();
        Self {
            game_id,
            enabled: true,
            auto_sync: false,
            last_sync_at: None,
            local_path: None,
            cloud_path: None,
            exclusion_patterns: None,
            compression_enabled: true,
            max_versions: Some(5),
            sync_direction: Some("bidirectional".to_string()),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Application-wide configuration
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppConfig {
    pub key: String,
    pub value: String,
    pub config_type: String, // "string", "json", "boolean", "number"
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AppConfig {
    pub fn new(key: String, value: String, config_type: String) -> Self {
        let now = Utc::now();
        Self {
            key,
            value,
            config_type,
            description: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Cloud backend statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudStats {
    pub id: Uuid,
    pub recorded_at: DateTime<Utc>,
    pub total_files: i32,
    pub total_size_bytes: i64,
    pub games_count: i32,
    pub backend_type: String,
    pub metadata: Option<String>, // JSON metadata about the backend
}

// Custom FromRow implementation to handle UUID stored as TEXT
impl FromRow<'_, sqlx::sqlite::SqliteRow> for CloudStats {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        // Parse UUID from TEXT field
        let id_str: String = row.try_get("id")?;
        let id = Uuid::parse_str(&id_str)
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "id".to_string(),
                source: Box::new(e),
            })?;

        // Parse timestamp from TEXT field
        let recorded_at_str: String = row.try_get("recorded_at")?;
        let recorded_at = DateTime::parse_from_rfc3339(&recorded_at_str)
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "recorded_at".to_string(),
                source: Box::new(e),
            })?
            .with_timezone(&Utc);

        Ok(CloudStats {
            id,
            recorded_at,
            total_files: row.try_get("total_files")?,
            total_size_bytes: row.try_get("total_size_bytes")?,
            games_count: row.try_get("games_count")?,
            backend_type: row.try_get("backend_type")?,
            metadata: row.try_get("metadata")?,
        })
    }
}

impl CloudStats {
    pub fn new(backend_type: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            recorded_at: Utc::now(),
            total_files: 0,
            total_size_bytes: 0,
            games_count: 0,
            backend_type,
            metadata: None,
        }
    }
}