use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use std::collections::HashMap;
use uuid::Uuid;
use chrono;

use steam_cloud_sync_cloud::{
    CloudSaveService, ProgressUpdate, SyncResult
};
use steam_cloud_sync_persistence::{
    PersistenceManager, CloudOperation, CloudOperationType, CloudOperationStatus,
    GameConfig
};
use crate::{AppSettings, GameWithSave};

/// Service manager that coordinates cloud operations with persistence
pub struct ServiceManager {
    pub cloud_service: CloudSaveService,
    pub persistence: Option<Arc<PersistenceManager>>, // Made optional for graceful degradation
    progress_rx: Arc<Mutex<mpsc::UnboundedReceiver<ProgressUpdate>>>,
    active_operations: Arc<Mutex<HashMap<Uuid, CloudOperation>>>,
    /// Whether the service manager is running in degraded mode (without database)
    pub degraded_mode: bool,
}

impl ServiceManager {
    /// Create a new service manager with graceful degradation
    pub async fn new(settings: &AppSettings) -> Result<Self> {
        println!("🏗️ [DEBUG] ServiceManager::new() called with graceful degradation support");
        
        // Try to initialize persistence - but don't fail if it doesn't work
        let persistence = match Self::try_initialize_persistence().await {
            Ok(persistence_manager) => {
                println!("✅ [DEBUG] Persistence initialized successfully");
                Some(Arc::new(persistence_manager))
            }
            Err(e) => {
                println!("⚠️ [DEBUG] Persistence initialization failed: {}", e);
                println!("📱 [DEBUG] ServiceManager will run in DEGRADED MODE");
                println!("   - Cloud upload/download will work");  
                println!("   - History and statistics will be disabled");
                None
            }
        };
        
        // Create cloud backend - this is the core functionality
        println!("☁️ [DEBUG] Creating cloud backend...");
        let backend = steam_cloud_sync_cloud::backend_with_settings(
            settings.selected_backend,
            Some((
                settings.tencent_secret_id.clone(),
                settings.tencent_secret_key.clone(),
                settings.tencent_bucket.clone(),
                settings.tencent_region.clone(),
            )),
            Some((settings.s3_bucket.clone(), "saves/".to_string())),
        );
        println!("✅ [DEBUG] Cloud backend created");
        
        // Create progress channel
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        println!("📡 [DEBUG] Progress channel created");
        
        // Create cloud service with progress tracking and user ID
        let cloud_service = CloudSaveService::new(backend)
            .with_user_id(settings.user_id.clone())
            .with_progress_channel(progress_tx);
        println!("🔄 [DEBUG] Cloud service created with user_id: {}", settings.user_id);
        
        let degraded_mode = persistence.is_none();
        if degraded_mode {
            println!("🔶 [DEBUG] ServiceManager running in DEGRADED MODE");
        } else {
            println!("🟢 [DEBUG] ServiceManager running in FULL MODE");
        }
        
        println!("🎉 [DEBUG] ServiceManager created successfully");
        
        Ok(Self {
            cloud_service,
            persistence,
            progress_rx: Arc::new(Mutex::new(progress_rx)),
            active_operations: Arc::new(Mutex::new(HashMap::new())),
            degraded_mode,
        })
    }
    
    /// Try to initialize persistence layer - returns error if it fails
    async fn try_initialize_persistence() -> Result<PersistenceManager> {
        println!("💾 [DEBUG] Attempting to initialize persistence layer...");
        
        match steam_cloud_sync_persistence::initialize_persistence().await {
            Ok(persistence) => {
                println!("✅ [DEBUG] Persistence layer created");
                
                // Try to initialize default configurations
                if let Err(e) = persistence.config_store.init_default_configs().await {
                    println!("⚠️ [DEBUG] Failed to initialize default configs: {}", e);
                    println!("   - This is not critical, continuing...");
                }
                
                // Test basic database functionality
                match persistence.database.health_check().await {
                    Ok(true) => {
                        println!("✅ [DEBUG] Database health check passed");
                        Ok(persistence)
                    }
                    Ok(false) => {
                        Err(anyhow::anyhow!("Database health check returned false"))
                    }
                    Err(e) => {
                        println!("❌ [DEBUG] Database health check failed: {}", e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                println!("❌ [DEBUG] Failed to initialize persistence: {}", e);
                Err(e)
            }
        }
    }
    
    /// Upload a save file with optional tracking (works in degraded mode)
    pub async fn upload_save(&self, game_id: &str, local_path: &std::path::Path) -> Result<steam_cloud_sync_cloud::SaveMetadata> {
        // Always perform the cloud upload - this is the core functionality
        let metadata = self.cloud_service.upload_save(game_id, local_path).await?;
        
        // Create operation record for tracking (always do this for history)
        let operation = CloudOperation::new(
            game_id.to_string(),
            CloudOperationType::Upload,
        );
        
        let operation_id = operation.id;
        
        // Try to store in database if persistence is available
        if let Some(persistence) = &self.persistence {
            println!("💾 [DEBUG] Storing upload operation in database");
            match persistence.cloud_history.create_operation(operation.clone()).await {
                Ok(stored_operation) => {
                    println!("✅ [DEBUG] Operation stored in database successfully");
                    
                    // Store in active operations
                    {
                        let mut active = self.active_operations.lock().await;
                        active.insert(operation_id, stored_operation.clone());
                    }
                    
                    // Update operation as completed
                    if let Err(e) = persistence.cloud_history
                        .update_operation_progress(operation_id, CloudOperationStatus::Completed, Some(1.0))
                        .await 
                    {
                        println!("⚠️ [DEBUG] Failed to update operation progress: {}", e);
                    } else {
                        println!("✅ [DEBUG] Operation marked as completed in database");
                    }
                    
                    // Update game config last sync time
                    if let Err(e) = persistence.config_store
                        .update_game_last_sync(game_id)
                        .await 
                    {
                        println!("⚠️ [DEBUG] Failed to update game last sync: {}", e);
                    }
                    
                    // Remove from active operations (since it's completed)
                    {
                        let mut active = self.active_operations.lock().await;
                        active.remove(&operation_id);
                    }
                }
                Err(e) => {
                    println!("❌ [DEBUG] Failed to create operation record in database: {}", e);
                    println!("📱 [DEBUG] Storing operation in memory only (degraded mode)");
                    
                    // Store in memory as fallback - mark as completed
                    let mut completed_operation = operation.clone();
                    completed_operation.status = CloudOperationStatus::Completed;
                    completed_operation.completed_at = Some(chrono::Utc::now());
                    completed_operation.file_size = Some(metadata.size_bytes as i64);
                    completed_operation.file_path = Some(local_path.to_string_lossy().to_string());
                    
                    {
                        let mut active = self.active_operations.lock().await;
                        active.insert(operation_id, completed_operation);
                    }
                }
            }
        } else {
            println!("📱 [DEBUG] No persistence available - storing upload operation in memory only");
            
            // Store in memory only (degraded mode) - mark as completed
            let mut completed_operation = operation.clone();
            completed_operation.status = CloudOperationStatus::Completed;
            completed_operation.completed_at = Some(chrono::Utc::now());
            completed_operation.file_size = Some(metadata.size_bytes as i64);
            completed_operation.file_path = Some(local_path.to_string_lossy().to_string());
            
            {
                let mut active = self.active_operations.lock().await;
                active.insert(operation_id, completed_operation);
            }
        }
        
        println!("🎉 [DEBUG] Upload completed successfully for game {} - history recorded", game_id);
        Ok(metadata)
    }
    
    /// Download a save file with optional tracking (works in degraded mode)
    pub async fn download_save(
        &self, 
        save_metadata: &steam_cloud_sync_cloud::SaveMetadata, 
        local_path: &std::path::Path
    ) -> Result<()> {
        // Always perform the cloud download - this is the core functionality
        self.cloud_service.download_save(save_metadata, local_path).await?;
        
        // Only track in database if persistence is available
        if let Some(persistence) = &self.persistence {
            // Create operation record for history tracking
            let operation = CloudOperation::new(
                save_metadata.game_id.clone(),
                CloudOperationType::Download,
            );
            
            let operation_id = operation.id;
            
            // Try to store in persistence (but don't fail if it doesn't work)
            match persistence.cloud_history.create_operation(operation).await {
                Ok(stored_operation) => {
                    // Store in active operations
                    {
                        let mut active = self.active_operations.lock().await;
                        active.insert(operation_id, stored_operation);
                    }
                    
                    // Update operation as completed
                    if let Err(e) = persistence.cloud_history
                        .update_operation_progress(operation_id, CloudOperationStatus::Completed, Some(1.0))
                        .await 
                    {
                        println!("⚠️ [DEBUG] Failed to update operation progress: {}", e);
                    }
                    
                    // Remove from active operations
                    {
                        let mut active = self.active_operations.lock().await;
                        active.remove(&operation_id);
                    }
                }
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to create operation record (degraded mode): {}", e);
                    // Don't fail - the download succeeded
                }
            }
        } else {
            println!("📱 [DEBUG] Download completed in degraded mode - no history tracking");
        }
        
        Ok(())
    }
    
    /// Restore a save file with backup (works in degraded mode)
    pub async fn restore_save(
        &self,
        save_metadata: &steam_cloud_sync_cloud::SaveMetadata,
        local_path: &std::path::Path
    ) -> Result<()> {
        // Always perform the cloud restore - this is the core functionality
        self.cloud_service.restore_save(save_metadata, local_path).await?;
        
        // Only track in database if persistence is available
        if let Some(persistence) = &self.persistence {
            let operation = CloudOperation::new(
                save_metadata.game_id.clone(),
                CloudOperationType::Restore,
            );
            let operation_id = operation.id;
            
            match persistence.cloud_history.create_operation(operation).await {
                Ok(_) => {
                    if let Err(e) = persistence.cloud_history
                        .update_operation_progress(operation_id, CloudOperationStatus::Completed, Some(1.0))
                        .await 
                    {
                        println!("⚠️ [DEBUG] Failed to update restore operation progress: {}", e);
                    }
                }
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to create restore operation record: {}", e);
                }
            }
        } else {
            println!("📱 [DEBUG] Restore completed in degraded mode - no history tracking");
        }
        
        Ok(())
    }
    
    /// Delete a save file (works in degraded mode)
    pub async fn delete_save(&self, save_metadata: &steam_cloud_sync_cloud::SaveMetadata) -> Result<()> {
        // Always perform the cloud delete - this is the core functionality
        self.cloud_service.delete_save(save_metadata).await?;
        
        // Only track in database if persistence is available
        if let Some(persistence) = &self.persistence {
            let operation = CloudOperation::new(
                save_metadata.game_id.clone(),
                CloudOperationType::Delete,
            );
            let operation_id = operation.id;
            
            match persistence.cloud_history.create_operation(operation).await {
                Ok(_) => {
                    if let Err(e) = persistence.cloud_history
                        .update_operation_progress(operation_id, CloudOperationStatus::Completed, Some(1.0))
                        .await 
                    {
                        println!("⚠️ [DEBUG] Failed to update delete operation progress: {}", e);
                    }
                }
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to create delete operation record: {}", e);
                }
            }
        } else {
            println!("📱 [DEBUG] Delete completed in degraded mode - no history tracking");
        }
        
        Ok(())
    }
    
    /// List saves for a game (always works)
    pub async fn list_saves(&self, game_id: Option<&str>) -> Result<Vec<steam_cloud_sync_cloud::SaveMetadata>> {
        // This only depends on cloud service, always works
        self.cloud_service.list_saves(game_id).await
    }
    
    /// Get storage information (always works)
    pub async fn get_storage_info(&self) -> Result<steam_cloud_sync_cloud::StorageInfo> {
        // This only depends on cloud service, always works
        self.cloud_service.get_storage_info().await
    }
    
    /// Sync a game's saves
    pub async fn sync_game(&self, game: &GameWithSave) -> Result<SyncResult> {
        println!("🎯 [DEBUG] sync_game() called for: {} ({})", game.game.name, game.game.id);
        println!("   - Has save info: {}", game.save_info.is_some());
        println!("   - Sync enabled: {}", game.sync_enabled);
        println!("   - Sync state: {:?}", game.sync_state);
        
        if let Some(save_info) = &game.save_info {
            println!("   - Save path: {}", save_info.save_path.display());
            println!("   - Save path exists: {}", save_info.save_path.exists());
            
            // Get cloud saves to determine sync direction
            let cloud_saves = self.cloud_service.list_saves(Some(&game.game.id)).await?;
            
            if !save_info.save_path.exists() {
                // No local save, download latest cloud save if available
                if let Some(latest_save) = cloud_saves.first() {
                    println!("🔄 [DEBUG] No local save, downloading from cloud...");
                    self.download_save(latest_save, &save_info.save_path).await?;
                    return Ok(SyncResult::Downloaded(latest_save.clone()));
                } else {
                    println!("❌ [DEBUG] No local save and no cloud save available");
                    return Ok(SyncResult::NoAction);
                }
            }
            
            // Get local file info
            let local_metadata = tokio::fs::metadata(&save_info.save_path).await?;
            let local_modified = local_metadata.modified()?;
            
            // Find the latest cloud save
            if let Some(latest_cloud_save) = cloud_saves.first() {
                // Parse cloud timestamp
                let cloud_timestamp = chrono::DateTime::parse_from_rfc3339(&latest_cloud_save.timestamp)?;
                let cloud_modified: std::time::SystemTime = cloud_timestamp.into();
                
                if local_modified > cloud_modified {
                    // Local is newer, upload
                    println!("🔄 [DEBUG] Local save is newer, uploading...");
                    let uploaded = self.upload_save(&game.game.id, &save_info.save_path).await?;
                    Ok(SyncResult::Uploaded(uploaded))
                } else if cloud_modified > local_modified {
                    // Cloud is newer, download
                    println!("🔄 [DEBUG] Cloud save is newer, downloading...");
                    self.download_save(latest_cloud_save, &save_info.save_path).await?;
                    Ok(SyncResult::Downloaded(latest_cloud_save.clone()))
                } else {
                    // Same timestamp, no action needed
                    println!("✅ [DEBUG] Local and cloud saves are in sync");
                    Ok(SyncResult::NoAction)
                }
            } else {
                // No cloud save, upload local
                println!("🔄 [DEBUG] No cloud save found, uploading local save...");
                let uploaded = self.upload_save(&game.game.id, &save_info.save_path).await?;
                Ok(SyncResult::Uploaded(uploaded))
            }
        } else {
            println!("❌ [DEBUG] Game has no save path configured");
            Err(anyhow::anyhow!("Game has no save path configured"))
        }
    }
    
    /// Get recent operations for UI display (graceful degradation)
    pub async fn get_recent_operations(&self, limit: Option<i32>) -> Result<Vec<CloudOperation>> {
        if let Some(persistence) = &self.persistence {
            // Try to get from database
            match persistence.cloud_history.get_recent_operations(limit).await {
                Ok(operations) => Ok(operations),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to get recent operations from database: {}", e);
                    println!("📱 [DEBUG] Falling back to in-memory operations");
                    
                    // Fallback to in-memory active operations
                    let active = self.active_operations.lock().await;
                    let mut operations: Vec<_> = active.values().cloned().collect();
                    operations.sort_by(|a, b| b.started_at.cmp(&a.started_at));
                    
                    if let Some(limit) = limit {
                        operations.truncate(limit as usize);
                    }
                    
                    Ok(operations)
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: returning in-memory operations only");
            
            // In degraded mode, return in-memory operations only
            let active = self.active_operations.lock().await;
            let mut operations: Vec<_> = active.values().cloned().collect();
            operations.sort_by(|a, b| b.started_at.cmp(&a.started_at));
            
            if let Some(limit) = limit {
                operations.truncate(limit as usize);
            }
            
            Ok(operations)
        }
    }
    
    /// Get operations for a specific game (graceful degradation)
    pub async fn get_game_operations(&self, game_id: &str) -> Result<Vec<CloudOperation>> {
        if let Some(persistence) = &self.persistence {
            match persistence.cloud_history.get_game_operations(game_id, Some(10)).await {
                Ok(operations) => Ok(operations),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to get game operations from database: {}", e);
                    println!("📱 [DEBUG] Falling back to in-memory operations for game {}", game_id);
                    
                    // Fallback to in-memory operations for this game
                    let active = self.active_operations.lock().await;
                    let operations: Vec<_> = active.values()
                        .filter(|op| op.game_id == game_id)
                        .cloned()
                        .collect();
                    
                    Ok(operations)
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: returning in-memory operations for game {}", game_id);
            
            // In degraded mode, return in-memory operations for this game
            let active = self.active_operations.lock().await;
            let operations: Vec<_> = active.values()
                .filter(|op| op.game_id == game_id)
                .cloned()
                .collect();
            
            Ok(operations)
        }
    }
    
    /// Get game configuration (graceful degradation)
    pub async fn get_game_config(&self, game_id: &str) -> Result<Option<GameConfig>> {
        if let Some(persistence) = &self.persistence {
            match persistence.config_store.get_game_config(game_id).await {
                Ok(config) => Ok(config),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to get game config from database: {}", e);
                    println!("📱 [DEBUG] Returning default game config for {}", game_id);
                    
                    // Return default config
                    Ok(Some(GameConfig::new(game_id.to_string())))
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: returning default game config for {}", game_id);
            // In degraded mode, return default config
            Ok(Some(GameConfig::new(game_id.to_string())))
        }
    }
    
    /// Update game configuration (graceful degradation)
    pub async fn set_game_config(&self, config: GameConfig) -> Result<()> {
        if let Some(persistence) = &self.persistence {
            match persistence.config_store.set_game_config(config).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to save game config to database: {}", e);
                    println!("📱 [DEBUG] Game config changes will not persist");
                    // Don't fail - just log the warning
                    Ok(())
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: game config changes will not persist");
            // In degraded mode, silently succeed (changes won't persist)
            Ok(())
        }
    }
    
    /// Enable/disable game sync (graceful degradation)
    pub async fn set_game_enabled(&self, game_id: &str, enabled: bool) -> Result<()> {
        if let Some(persistence) = &self.persistence {
            match persistence.config_store.set_game_enabled(game_id, enabled).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to save game enabled state: {}", e);
                    println!("📱 [DEBUG] Game enabled state changes will not persist");
                    // Don't fail - just log the warning
                    Ok(())
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: game enabled state changes will not persist");
            // In degraded mode, silently succeed (changes won't persist)
            Ok(())
        }
    }
    
    /// Get application configuration (graceful degradation)
    pub async fn get_app_config(&self, key: &str) -> Result<Option<String>> {
        if let Some(persistence) = &self.persistence {
            match persistence.config_store.get_string_config(key).await {
                Ok(value) => Ok(value),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to get app config '{}': {}", key, e);
                    println!("📱 [DEBUG] Returning None for app config key '{}'", key);
                    Ok(None)
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: returning None for app config key '{}'", key);
            Ok(None)
        }
    }
    
    /// Set application configuration (graceful degradation)
    pub async fn set_app_config(&self, key: &str, value: &str) -> Result<()> {
        if let Some(persistence) = &self.persistence {
            match persistence.config_store.set_string_config(key, value).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to save app config '{}': {}", key, e);
                    println!("📱 [DEBUG] App config changes will not persist");
                    // Don't fail - just log the warning
                    Ok(())
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: app config '{}' changes will not persist", key);
            // In degraded mode, silently succeed (changes won't persist)
            Ok(())
        }
    }
    
    /// Get sync statistics (graceful degradation)
    pub async fn get_sync_stats(&self) -> Result<steam_cloud_sync_persistence::SyncStats> {
        if let Some(persistence) = &self.persistence {
            match persistence.cloud_history.get_sync_stats().await {
                Ok(stats) => Ok(stats),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to get sync stats from database: {}", e);
                    println!("📱 [DEBUG] Returning default sync stats");
                    
                    // Return default stats
                    Ok(steam_cloud_sync_persistence::SyncStats {
                        total_sessions: 0,
                        successful_sessions: 0,
                        total_operations: 0,
                        successful_operations: 0,
                        total_bytes_synced: 0,
                    })
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: returning default sync stats");
            
            // In degraded mode, return default stats
            Ok(steam_cloud_sync_persistence::SyncStats {
                total_sessions: 0,
                successful_sessions: 0,
                total_operations: 0,
                successful_operations: 0,
                total_bytes_synced: 0,
            })
        }
    }
    
    /// Get database statistics (graceful degradation)
    pub async fn get_database_stats(&self) -> Result<steam_cloud_sync_persistence::DatabaseStats> {
        if let Some(persistence) = &self.persistence {
            match persistence.database.get_stats().await {
                Ok(stats) => Ok(stats),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to get database stats: {}", e);
                    println!("📱 [DEBUG] Returning default database stats");
                    
                    // Return default stats
                    Ok(steam_cloud_sync_persistence::DatabaseStats {
                        operations_count: 0,
                        sessions_count: 0,
                        game_configs_count: 0,
                        app_configs_count: 0,
                        database_size: 0,
                    })
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: returning default database stats");
            
            // In degraded mode, return default stats indicating no database
            Ok(steam_cloud_sync_persistence::DatabaseStats {
                operations_count: 0,
                sessions_count: 0,
                game_configs_count: 0,
                app_configs_count: 0,
                database_size: 0,
            })
        }
    }
    
    /// Cleanup old operations (graceful degradation)
    pub async fn cleanup_old_operations(&self, older_than_days: i64) -> Result<u64> {
        if let Some(persistence) = &self.persistence {
            match persistence.cloud_history.cleanup_old_operations(older_than_days).await {
                Ok(count) => Ok(count),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to cleanup old operations: {}", e);
                    println!("📱 [DEBUG] Returning 0 for cleanup count");
                    Ok(0)
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: no database operations to cleanup");
            Ok(0)
        }
    }
    
    /// Vacuum database (graceful degradation)
    pub async fn vacuum_database(&self) -> Result<()> {
        if let Some(persistence) = &self.persistence {
            match persistence.database.vacuum().await {
                Ok(_) => Ok(()),
                Err(e) => {
                    println!("⚠️ [DEBUG] Failed to vacuum database: {}", e);
                    println!("📱 [DEBUG] Vacuum operation failed but continuing");
                    // Don't fail - just log the warning
                    Ok(())
                }
            }
        } else {
            println!("📱 [DEBUG] Degraded mode: no database to vacuum");
            Ok(())
        }
    }
    
    /// Process progress updates (should be called periodically) - always works
    pub async fn process_progress_updates(&self) -> Vec<ProgressUpdate> {
        let mut progress_rx = self.progress_rx.lock().await;
        let mut updates = Vec::new();
        
        // Collect all available progress updates
        while let Ok(update) = progress_rx.try_recv() {
            updates.push(update);
        }
        
        updates
    }
    
    /// Get active operations - always works
    pub async fn get_active_operations(&self) -> HashMap<Uuid, CloudOperation> {
        let active = self.active_operations.lock().await;
        active.clone()
    }
}