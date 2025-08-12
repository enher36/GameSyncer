use crate::{CloudBackend, SaveMetadata, StorageInfo};
use anyhow::Result;
use std::path::Path;
use tokio::sync::mpsc;
use uuid::Uuid;
use steam_cloud_sync_core::GameSave;

/// Progress callback for upload/download operations
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send + Sync>;

/// Cloud save service for managing save files
pub struct CloudSaveService {
    backend: Box<dyn CloudBackend + Send + Sync>,
    progress_tx: Option<mpsc::UnboundedSender<ProgressUpdate>>,
    user_id: String,
}

/// Progress update message
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub operation_id: Uuid,
    pub game_id: String,
    pub operation_type: OperationType,
    pub bytes_processed: u64,
    pub total_bytes: u64,
    pub status: OperationStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum OperationType {
    Upload,
    Download,
    Delete,
    List,
}

#[derive(Debug, Clone)]
pub enum OperationStatus {
    Starting,
    InProgress,
    Completed,
    Failed,
}

impl CloudSaveService {
    pub fn new(backend: Box<dyn CloudBackend + Send + Sync>) -> Self {
        Self {
            backend,
            progress_tx: None,
            user_id: "default_user".to_string(), // TODO: Make this configurable
        }
    }
    
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = user_id;
        self
    }
    
    pub fn with_progress_channel(mut self, tx: mpsc::UnboundedSender<ProgressUpdate>) -> Self {
        self.progress_tx = Some(tx);
        self
    }
    
    /// Upload a save file to the cloud
    pub async fn upload_save(&self, game_id: &str, local_path: &Path) -> Result<SaveMetadata> {
        let operation_id = Uuid::new_v4();
        
        // Send starting progress
        self.send_progress(ProgressUpdate {
            operation_id,
            game_id: game_id.to_string(),
            operation_type: OperationType::Upload,
            bytes_processed: 0,
            total_bytes: 0,
            status: OperationStatus::Starting,
            error: None,
        }).await;
        
        // Create GameSave from local path
        let app_id: u32 = game_id.parse()
            .map_err(|_| anyhow::anyhow!("Invalid game_id format: {}, expected numeric app_id", game_id))?;
        
        let game_save = GameSave {
            app_id,
            name: format!("save_{}", chrono::Utc::now().timestamp()),
            save_path: local_path.to_path_buf(),
        };
        
        // Get file size for progress tracking
        let file_size = tokio::fs::metadata(local_path).await?.len();
        
        // Send progress update with file size
        self.send_progress(ProgressUpdate {
            operation_id,
            game_id: game_id.to_string(),
            operation_type: OperationType::Upload,
            bytes_processed: 0,
            total_bytes: file_size,
            status: OperationStatus::InProgress,
            error: None,
        }).await;
        
        // Upload to cloud using existing backend
        match self.backend.upload_save(&game_save, &self.user_id).await {
            Ok(metadata) => {
                // Send completion progress
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: game_id.to_string(),
                    operation_type: OperationType::Upload,
                    bytes_processed: file_size,
                    total_bytes: file_size,
                    status: OperationStatus::Completed,
                    error: None,
                }).await;
                
                Ok(metadata)
            },
            Err(e) => {
                // Send error progress
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: game_id.to_string(),
                    operation_type: OperationType::Upload,
                    bytes_processed: 0,
                    total_bytes: file_size,
                    status: OperationStatus::Failed,
                    error: Some(e.to_string()),
                }).await;
                
                Err(e)
            }
        }
    }
    
    /// Download a save file from the cloud
    pub async fn download_save(&self, save_metadata: &SaveMetadata, local_path: &Path) -> Result<()> {
        let operation_id = Uuid::new_v4();
        
        // Send starting progress
        self.send_progress(ProgressUpdate {
            operation_id,
            game_id: save_metadata.game_id.clone(),
            operation_type: OperationType::Download,
            bytes_processed: 0,
            total_bytes: save_metadata.size_bytes,
            status: OperationStatus::Starting,
            error: None,
        }).await;
        
        // Download from cloud using existing backend
        match self.backend.download_save(save_metadata, local_path).await {
            Ok(_) => {
                // Send completion progress
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: save_metadata.game_id.clone(),
                    operation_type: OperationType::Download,
                    bytes_processed: save_metadata.size_bytes,
                    total_bytes: save_metadata.size_bytes,
                    status: OperationStatus::Completed,
                    error: None,
                }).await;
                
                Ok(())
            },
            Err(e) => {
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: save_metadata.game_id.clone(),
                    operation_type: OperationType::Download,
                    bytes_processed: 0,
                    total_bytes: save_metadata.size_bytes,
                    status: OperationStatus::Failed,
                    error: Some(e.to_string()),
                }).await;
                
                Err(e)
            }
        }
    }
    
    /// Restore a save file (download and replace local save)
    pub async fn restore_save(&self, save_metadata: &SaveMetadata, local_path: &Path) -> Result<()> {
        // Create backup of existing file if it exists
        if local_path.exists() {
            let backup_path = local_path.with_extension(format!("bak.{}", chrono::Utc::now().timestamp()));
            tokio::fs::copy(local_path, &backup_path).await?;
            eprintln!("Created backup at: {}", backup_path.display());
        }
        
        // Download the save file
        self.download_save(save_metadata, local_path).await
    }
    
    /// Delete a save file from the cloud
    pub async fn delete_save(&self, save_metadata: &SaveMetadata) -> Result<()> {
        let operation_id = Uuid::new_v4();
        
        // Send starting progress
        self.send_progress(ProgressUpdate {
            operation_id,
            game_id: save_metadata.game_id.clone(),
            operation_type: OperationType::Delete,
            bytes_processed: 0,
            total_bytes: 0,
            status: OperationStatus::Starting,
            error: None,
        }).await;
        
        match self.backend.delete_save(save_metadata).await {
            Ok(_) => {
                // Send completion progress
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: save_metadata.game_id.clone(),
                    operation_type: OperationType::Delete,
                    bytes_processed: 0,
                    total_bytes: 0,
                    status: OperationStatus::Completed,
                    error: None,
                }).await;
                
                Ok(())
            },
            Err(e) => {
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: save_metadata.game_id.clone(),
                    operation_type: OperationType::Delete,
                    bytes_processed: 0,
                    total_bytes: 0,
                    status: OperationStatus::Failed,
                    error: Some(e.to_string()),
                }).await;
                
                Err(e)
            }
        }
    }
    
    /// List all saves for a game
    pub async fn list_saves(&self, game_id: Option<&str>) -> Result<Vec<SaveMetadata>> {
        let operation_id = Uuid::new_v4();
        
        // Send starting progress
        self.send_progress(ProgressUpdate {
            operation_id,
            game_id: game_id.unwrap_or("all").to_string(),
            operation_type: OperationType::List,
            bytes_processed: 0,
            total_bytes: 0,
            status: OperationStatus::Starting,
            error: None,
        }).await;
        
        match self.backend.list_saves(&self.user_id, game_id).await {
            Ok(saves) => {
                // Send completion progress
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: game_id.unwrap_or("all").to_string(),
                    operation_type: OperationType::List,
                    bytes_processed: 0,
                    total_bytes: 0,
                    status: OperationStatus::Completed,
                    error: None,
                }).await;
                
                Ok(saves)
            },
            Err(e) => {
                self.send_progress(ProgressUpdate {
                    operation_id,
                    game_id: game_id.unwrap_or("all").to_string(),
                    operation_type: OperationType::List,
                    bytes_processed: 0,
                    total_bytes: 0,
                    status: OperationStatus::Failed,
                    error: Some(e.to_string()),
                }).await;
                
                Err(e)
            }
        }
    }
    
    /// Get storage information (combines user and bucket info)
    pub async fn get_storage_info(&self) -> Result<StorageInfo> {
        // Get user-specific storage info
        let mut storage_info = self.backend.get_storage_info(&self.user_id).await?;
        
        // Get bucket-wide storage info and add it to the result
        match self.backend.get_bucket_storage_info().await {
            Ok((bucket_bytes, bucket_objects)) => {
                storage_info.bucket_used_bytes = Some(bucket_bytes);
                storage_info.bucket_total_objects = Some(bucket_objects);
            }
            Err(e) => {
                eprintln!("Warning: Failed to get bucket storage info: {}", e);
                // Continue with just user info if bucket info fails
            }
        }
        
        Ok(storage_info)
    }
    
    /// Send progress update through channel
    async fn send_progress(&self, update: ProgressUpdate) {
        if let Some(tx) = &self.progress_tx {
            let _ = tx.send(update);
        }
    }
    
    /// Batch upload multiple saves
    pub async fn batch_upload_saves(&self, saves: Vec<(&str, &Path)>) -> Result<Vec<Result<SaveMetadata>>> {
        let mut results = Vec::new();
        
        for (game_id, local_path) in saves {
            let result = self.upload_save(game_id, local_path).await;
            results.push(result);
        }
        
        Ok(results)
    }
    
    /// Batch download multiple saves
    pub async fn batch_download_saves(&self, saves: Vec<(&SaveMetadata, &Path)>) -> Result<Vec<Result<()>>> {
        let mut results = Vec::new();
        
        for (save_metadata, local_path) in saves {
            let result = self.download_save(save_metadata, local_path).await;
            results.push(result);
        }
        
        Ok(results)
    }
    
    /// Sync saves for a game (upload if local is newer, download if cloud is newer)
    pub async fn sync_game_saves(&self, game_id: &str, local_path: &Path) -> Result<SyncResult> {
        // List cloud saves
        let cloud_saves = self.list_saves(Some(game_id)).await?;
        
        if !local_path.exists() {
            // No local save, download latest cloud save if available
            if let Some(latest_save) = cloud_saves.first() {
                self.download_save(latest_save, local_path).await?;
                return Ok(SyncResult::Downloaded(latest_save.clone()));
            } else {
                return Ok(SyncResult::NoAction);
            }
        }
        
        // Get local file info
        let local_metadata = tokio::fs::metadata(local_path).await?;
        let local_modified = local_metadata.modified()?;
        
        // Find the latest cloud save
        if let Some(latest_cloud_save) = cloud_saves.first() {
            // Parse cloud timestamp
            let cloud_timestamp = chrono::DateTime::parse_from_rfc3339(&latest_cloud_save.timestamp)?;
            let cloud_modified: std::time::SystemTime = cloud_timestamp.into();
            
            if local_modified > cloud_modified {
                // Local is newer, upload
                let uploaded = self.upload_save(game_id, local_path).await?;
                Ok(SyncResult::Uploaded(uploaded))
            } else if cloud_modified > local_modified {
                // Cloud is newer, download
                self.download_save(latest_cloud_save, local_path).await?;
                Ok(SyncResult::Downloaded(latest_cloud_save.clone()))
            } else {
                // Same timestamp, no action needed
                Ok(SyncResult::NoAction)
            }
        } else {
            // No cloud save, upload local
            let uploaded = self.upload_save(game_id, local_path).await?;
            Ok(SyncResult::Uploaded(uploaded))
        }
    }
}

/// Result of a sync operation
#[derive(Debug, Clone)]
pub enum SyncResult {
    Uploaded(SaveMetadata),
    Downloaded(SaveMetadata),
    NoAction,
}