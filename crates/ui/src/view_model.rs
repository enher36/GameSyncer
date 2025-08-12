use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::{GameWithSave, AppSettings, ServiceManager, SyncState, SaveDetectionStatus, SyncHistoryItem, UndoableSync};
use steam_cloud_sync_core::{scan_installed_games, locate_save};
use steam_cloud_sync_cloud::{SaveMetadata, StorageInfo};

/// Main application view model that manages state and operations
#[derive(Clone)]
pub struct AppViewModel {
    service_manager: Arc<Mutex<Option<Arc<ServiceManager>>>>,
    cache: Arc<Mutex<ViewModelCache>>,
}

#[derive(Default)]
struct ViewModelCache {
    games: Vec<GameWithSave>,
    last_scan: Option<std::time::Instant>,
    scanning: bool,
    storage_info: Option<StorageInfo>,
    last_storage_update: Option<std::time::Instant>,
    sync_history: Vec<SyncHistoryItem>,
    undoable_syncs: Vec<UndoableSync>,
}

impl AppViewModel {
    pub fn new() -> Self {
        Self {
            service_manager: Arc::new(Mutex::new(None)),
            cache: Arc::new(Mutex::new(ViewModelCache::default())),
        }
    }
    
    /// Initialize the view model with settings
    pub async fn initialize(&self, _settings: &AppSettings) -> Result<()> {
        // Check if already initialized
        if let Some(_) = self.get_service_manager().await {
            return Ok(());
        }
        
        // This is a temporary workaround - in production, consider using interior mutability
        // or initializing the ServiceManager separately
        eprintln!("Warning: AppViewModel not fully initialized - ServiceManager features may be limited");
        Ok(())
    }
    
    /// Set service manager (for initialization)
    pub async fn set_service_manager(&self, settings: &AppSettings) -> Result<()> {
        println!("🔧 [DEBUG] Creating service manager with settings:");
        println!("   - Selected backend: {:?}", settings.selected_backend);
        println!("   - User ID: {}", settings.user_id);
        
        // Check credentials based on selected backend
        match settings.selected_backend {
            steam_cloud_sync_cloud::BackendType::TencentCOS => {
                println!("   - Tencent COS bucket: {}", settings.tencent_bucket);
                println!("   - Tencent COS region: {}", settings.tencent_region);
                println!("   - Tencent Secret ID configured: {}", !settings.tencent_secret_id.is_empty());
                println!("   - Tencent Secret Key configured: {}", !settings.tencent_secret_key.is_empty());
                
                if settings.tencent_secret_id.is_empty() || settings.tencent_secret_key.is_empty() {
                    return Err(anyhow::anyhow!("Tencent COS credentials not configured. Please set Secret ID and Secret Key in settings."));
                }
            },
            steam_cloud_sync_cloud::BackendType::S3 => {
                println!("   - S3 bucket: {}", settings.s3_bucket);
                println!("   - S3 region: {}", settings.s3_region);
                println!("   - S3 Access Key configured: {}", !settings.s3_access_key.is_empty());
                println!("   - S3 Secret Key configured: {}", !settings.s3_secret_key.is_empty());
                
                if settings.s3_access_key.is_empty() || settings.s3_secret_key.is_empty() {
                    return Err(anyhow::anyhow!("S3 credentials not configured. Please set Access Key and Secret Key in settings."));
                }
            }
        }
        
        let service_manager = Arc::new(ServiceManager::new(settings).await?);
        
        let mut sm = self.service_manager.lock().await;
        *sm = Some(service_manager);
        
        println!("✅ [DEBUG] Service manager created and stored successfully");
        Ok(())
    }
    
    /// Get service manager (helper method)
    async fn get_service_manager(&self) -> Option<Arc<ServiceManager>> {
        let sm = self.service_manager.lock().await;
        sm.clone()
    }
    
    /// Scan for installed games
    pub async fn scan_games(&self) -> Result<Vec<GameWithSave>> {
        self.scan_games_internal(false).await
    }
    
    /// Force scan for installed games (bypass cache)
    pub async fn force_scan_games(&self) -> Result<Vec<GameWithSave>> {
        self.scan_games_internal(true).await
    }
    
    /// Internal scan implementation
    async fn scan_games_internal(&self, force: bool) -> Result<Vec<GameWithSave>> {
        let mut cache = self.cache.lock().await;
        
        // Don't scan too frequently unless forced
        if !force {
            if let Some(last_scan) = cache.last_scan {
                if last_scan.elapsed() < std::time::Duration::from_secs(30) {
                    return Ok(cache.games.clone());
                }
            }
        }
        
        cache.scanning = true;
        drop(cache);
        
        // Scan for games
        let installed_games = scan_installed_games()?;
        let mut games_with_saves = Vec::new();
        
        for game in installed_games {
            // Try to locate save for this game
            let save_info = match locate_save(&game) {
                Ok(save) => {
                    if save.is_none() {
                        eprintln!("No save location found for game: {} ({})", game.name, game.id);
                    }
                    save
                }
                Err(e) => {
                    eprintln!("Error locating save for game {} ({}): {}", game.name, game.id, e);
                    None
                }
            };
            
            // Get cloud saves if we have a service manager
            let cloud_saves = if let Some(service_manager) = self.get_service_manager().await {
                println!("☁️ [DEBUG] Fetching cloud saves for game: {} ({})", game.name, game.id);
                match service_manager.list_saves(Some(&game.id)).await {
                    Ok(saves) => {
                        println!("✅ [DEBUG] Found {} cloud saves for game: {}", saves.len(), game.name);
                        for save in &saves {
                            println!("   - Save: {} ({:.2} MB) at {}", 
                                save.file_id, save.size_bytes as f64 / (1024.0 * 1024.0), save.timestamp);
                        }
                        saves
                    }
                    Err(e) => {
                        println!("❌ [DEBUG] Failed to fetch cloud saves for {}: {}", game.name, e);
                        Vec::new()
                    }
                }
            } else {
                println!("⚠️ [DEBUG] No service manager available for cloud saves lookup");
                Vec::new()
            };
            
            // Determine sync state
            let sync_state = if save_info.is_some() && !cloud_saves.is_empty() {
                // Compare timestamps to determine sync state
                // For now, just mark as pending if we have both local and cloud saves
                SyncState::Pending
            } else if save_info.is_some() && cloud_saves.is_empty() {
                SyncState::Pending // Local save, no cloud save
            } else if save_info.is_none() && !cloud_saves.is_empty() {
                SyncState::Pending // Cloud save, no local save
            } else {
                SyncState::Unknown // No saves found
            };
            
            let game_with_save = GameWithSave {
                game,
                save_detection_status: if save_info.is_some() { 
                    SaveDetectionStatus::Found 
                } else { 
                    SaveDetectionStatus::NotFound 
                },
                save_info,
                sync_enabled: true, // Default enabled
                cloud_saves,
                downloading: false,
                sync_state,
                sync_progress: None,
            };
            
            games_with_saves.push(game_with_save);
        }
        
        // Print scan summary with more details
        eprintln!("\n========== Scan Results ==========");
        let games_with_local_saves: Vec<_> = games_with_saves.iter()
            .filter(|g| g.save_info.is_some())
            .collect();
        let games_with_cloud_saves: Vec<_> = games_with_saves.iter()
            .filter(|g| !g.cloud_saves.is_empty())
            .collect();
        let games_sync_enabled: Vec<_> = games_with_saves.iter()
            .filter(|g| g.sync_enabled)
            .collect();
        let games_pending_sync: Vec<_> = games_with_saves.iter()
            .filter(|g| g.sync_enabled && matches!(g.sync_state, SyncState::Pending))
            .collect();
        
        eprintln!("Total games scanned: {}", games_with_saves.len());
        eprintln!("Games with local saves found: {}", games_with_local_saves.len());
        eprintln!("Games with cloud saves found: {}", games_with_cloud_saves.len());
        eprintln!("Games with sync enabled: {}", games_sync_enabled.len());
        eprintln!("Games ready for sync (pending): {}", games_pending_sync.len());
        
        if !games_with_local_saves.is_empty() {
            eprintln!("\nLocal saves found for:");
            for game in &games_with_local_saves {
                if let Some(save_info) = &game.save_info {
                    eprintln!("  - {} ({}): {} [sync: {}, state: {:?}]", 
                        game.game.name, 
                        game.game.id,
                        save_info.save_path.display(),
                        game.sync_enabled,
                        game.sync_state);
                }
            }
        }
        
        if !games_with_cloud_saves.is_empty() {
            eprintln!("\nCloud saves found for:");
            for game in &games_with_cloud_saves {
                eprintln!("  - {} ({}) - {} cloud save(s) [sync: {}, state: {:?}]", 
                    game.game.name, 
                    game.game.id,
                    game.cloud_saves.len(),
                    game.sync_enabled,
                    game.sync_state);
            }
        }
        
        if games_pending_sync.is_empty() {
            eprintln!("\n⚠️ WARNING: No games are ready for sync!");
            eprintln!("   This means no games have sync_enabled=true AND sync_state=Pending");
            eprintln!("   Users won't be able to sync until games are properly configured");
        } else {
            eprintln!("\n✅ Ready for sync:");
            for game in &games_pending_sync {
                eprintln!("  - {} ({}) [has local: {}, has cloud: {}]", 
                    game.game.name,
                    game.game.id,
                    game.save_info.is_some(),
                    !game.cloud_saves.is_empty());
            }
        }
        eprintln!("==================================\n");
        
        // Update cache
        let mut cache = self.cache.lock().await;
        cache.games = games_with_saves.clone();
        cache.last_scan = Some(std::time::Instant::now());
        cache.scanning = false;
        
        Ok(games_with_saves)
    }
    
    /// Get cached games
    pub async fn get_games(&self) -> Vec<GameWithSave> {
        let cache = self.cache.lock().await;
        cache.games.clone()
    }
    
    /// Sync all enabled games
    pub async fn sync_now(&self, _settings: &AppSettings) -> Result<()> {
        println!("🔍 [DEBUG] sync_now() called in view_model");
        
        let service_manager = self.get_service_manager().await;
        
        if service_manager.is_none() {
            println!("❌ [DEBUG] Service manager not initialized - sync cannot proceed");
            return Err(anyhow::anyhow!("Service manager not initialized"));
        }
        
        let service_manager = service_manager.unwrap();
        println!("✅ [DEBUG] Service manager is available");
        
        let games = self.get_games().await;
        println!("🎮 [DEBUG] Total games loaded: {}", games.len());
        
        let enabled_games: Vec<_> = games.into_iter()
            .filter(|g| g.sync_enabled && matches!(g.sync_state, SyncState::Pending))
            .collect();
        
        println!("⚡ [DEBUG] Games enabled for sync: {}", enabled_games.len());
        
        if enabled_games.is_empty() {
            println!("⚠️ [DEBUG] No games are enabled and ready for sync");
            return Ok(());
        }
        
        for (i, game) in enabled_games.iter().enumerate() {
            println!("🎯 [DEBUG] Syncing game {}/{}: {} ({})", 
                i + 1, enabled_games.len(), game.game.name, game.game.id);
            
            match service_manager.sync_game(game).await {
                Ok(_) => {
                    println!("✅ [DEBUG] Successfully synced: {}", game.game.name);
                }
                Err(e) => {
                    println!("❌ [DEBUG] Failed to sync {}: {}", game.game.name, e);
                    eprintln!("Failed to sync game {}: {}", game.game.name, e);
                }
            }
        }
        
        println!("🔄 [DEBUG] Starting game rescan after sync");
        // Refresh games after sync
        self.scan_games().await?;
        println!("✅ [DEBUG] sync_now() completed successfully");
        
        Ok(())
    }
    
    /// Upload a save file
    pub async fn upload_save(&self, game_id: &str, local_path: &std::path::Path) -> Result<SaveMetadata> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Err(anyhow::anyhow!("Service manager not initialized"));
        };
        
        service_manager.upload_save(game_id, local_path).await
    }
    
    /// Download a save file
    pub async fn download_save(&self, _settings: &AppSettings, save_metadata: &SaveMetadata, game: &GameWithSave) -> Result<()> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Err(anyhow::anyhow!("Service manager not initialized"));
        };
        
        if let Some(save_info) = &game.save_info {
            service_manager.download_save(save_metadata, &save_info.save_path).await
        } else {
            Err(anyhow::anyhow!("Game has no save path"))
        }
    }
    
    /// Download a save file to custom location
    pub async fn download_save_to_path(&self, save_metadata: &SaveMetadata, target_path: &std::path::Path) -> Result<()> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Err(anyhow::anyhow!("Service manager not initialized"));
        };
        
        // Create directory if it doesn't exist
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        service_manager.download_save(save_metadata, target_path).await
    }
    
    /// Restore a save file (download and replace local)
    pub async fn restore_save(&self, save_metadata: &SaveMetadata, local_path: &std::path::Path) -> Result<()> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Err(anyhow::anyhow!("Service manager not initialized"));
        };
        
        service_manager.restore_save(save_metadata, local_path).await
    }
    
    /// Delete a save file from cloud
    pub async fn delete_save(&self, save_metadata: &SaveMetadata) -> Result<()> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Err(anyhow::anyhow!("Service manager not initialized"));
        };
        
        service_manager.delete_save(save_metadata).await
    }
    
    /// List cloud saves for a game
    pub async fn list_cloud_saves(&self, settings: &AppSettings, game_id: Option<&str>) -> Result<Vec<SaveMetadata>> {
        // Try to get existing service manager first
        if let Some(service_manager) = self.get_service_manager().await {
            return service_manager.list_saves(game_id).await;
        }
        
        // If no service manager, try lazy initialization
        eprintln!("Service manager not initialized for list_cloud_saves - attempting lazy initialization");
        match ServiceManager::new(settings).await {
            Ok(sm) => {
                let service_manager = Arc::new(sm);
                // Store the newly created service manager
                {
                    let mut sm_lock = self.service_manager.lock().await;
                    *sm_lock = Some(service_manager.clone());
                }
                service_manager.list_saves(game_id).await
            },
            Err(e) => {
                eprintln!("Failed to create service manager for list_cloud_saves: {}", e);
                Ok(Vec::new())
            }
        }
    }
    
    /// Refresh cloud saves for a specific game
    pub async fn refresh_cloud_saves(&self, settings: &AppSettings, game_id: &str) -> Result<()> {
        let cloud_saves = self.list_cloud_saves(settings, Some(game_id)).await?;
        
        // Update the game in cache
        let mut cache = self.cache.lock().await;
        if let Some(game) = cache.games.iter_mut().find(|g| g.game.id == game_id) {
            game.cloud_saves = cloud_saves;
        }
        
        Ok(())
    }
    
    /// Get storage information
    pub async fn get_storage_info(&self, settings: &AppSettings) -> Result<StorageInfo> {
        let cache = self.cache.lock().await;
        
        // Return cached info if recent
        if let Some(last_update) = cache.last_storage_update {
            if let Some(storage_info) = &cache.storage_info {
                if last_update.elapsed() < std::time::Duration::from_secs(60) {
                    return Ok(storage_info.clone());
                }
            }
        }
        
        drop(cache);
        
        // Try to get existing service manager or create one
        let service_manager = if let Some(sm) = self.get_service_manager().await {
            sm
        } else {
            // Only log once per minute to avoid spam
            static mut LAST_LOG_TIME: Option<std::time::Instant> = None;
            let should_log = unsafe {
                LAST_LOG_TIME.map_or(true, |last| last.elapsed().as_secs() >= 60)
            };
            
            if should_log {
                eprintln!("Service manager not initialized - attempting lazy initialization");
                unsafe { LAST_LOG_TIME = Some(std::time::Instant::now()); }
            }
            
            // Try to create a temporary service manager for this operation
            match ServiceManager::new(settings).await {
                Ok(sm) => {
                    if should_log {
                        eprintln!("Temporary service manager created successfully");
                    }
                    Arc::new(sm)
                }
                Err(e) => {
                    if should_log {
                        eprintln!("Failed to create temporary service manager: {}", e);
                    }
                    return Ok(StorageInfo {
                        used_bytes: 0,
                        total_bytes: Some(100 * 1024 * 1024 * 1024), // 100GB default
                        file_count: 0,
                        bucket_used_bytes: None,
                        bucket_total_objects: None,
                    });
                }
            }
        };
        
        // Fetch fresh storage info
        let storage_info = match service_manager.get_storage_info().await {
            Ok(info) => {
                // Only log success once per minute
                static mut LAST_SUCCESS_LOG: Option<std::time::Instant> = None;
                let should_log_success = unsafe {
                    LAST_SUCCESS_LOG.map_or(true, |last| last.elapsed().as_secs() >= 60)
                };
                
                if should_log_success && (info.used_bytes > 0 || info.file_count > 0) {
                    eprintln!("Storage info fetched successfully: {} bytes used, {} files", 
                        info.used_bytes, info.file_count);
                    unsafe { LAST_SUCCESS_LOG = Some(std::time::Instant::now()); }
                }
                info
            }
            Err(e) => {
                // Only log errors once per minute
                static mut LAST_ERROR_LOG: Option<std::time::Instant> = None;
                let should_log_error = unsafe {
                    LAST_ERROR_LOG.map_or(true, |last| last.elapsed().as_secs() >= 60)
                };
                
                if should_log_error {
                    eprintln!("Failed to fetch storage info: {}", e);
                    unsafe { LAST_ERROR_LOG = Some(std::time::Instant::now()); }
                }
                StorageInfo {
                    used_bytes: 0,
                    total_bytes: Some(100 * 1024 * 1024 * 1024), // 100GB default
                    file_count: 0,
                    bucket_used_bytes: None,
                    bucket_total_objects: None,
                }
            }
        };
        
        // Update cache
        let mut cache = self.cache.lock().await;
        cache.storage_info = Some(storage_info.clone());
        cache.last_storage_update = Some(std::time::Instant::now());
        
        Ok(storage_info)
    }
    
    /// Set game downloading status
    pub async fn set_game_downloading(&self, game_id: &str, downloading: bool) -> Result<()> {
        let mut cache = self.cache.lock().await;
        if let Some(game) = cache.games.iter_mut().find(|g| g.game.id == game_id) {
            game.downloading = downloading;
        }
        Ok(())
    }
    
    /// Get recent operations
    pub async fn get_recent_operations(&self) -> Result<Vec<steam_cloud_sync_persistence::CloudOperation>> {
        let Some(service_manager) = self.get_service_manager().await else {
            println!("❌ [DEBUG] No service manager available for get_recent_operations");
            return Ok(Vec::new());
        };
        
        println!("🔍 [DEBUG] Fetching recent operations from database...");
        match service_manager.get_recent_operations(Some(20)).await {
            Ok(operations) => {
                println!("✅ [DEBUG] Loaded {} operations from database", operations.len());
                for op in &operations {
                    println!("   - Operation: {:?} for game {} at {}", 
                        op.operation_type, op.game_id, op.started_at);
                }
                Ok(operations)
            }
            Err(e) => {
                println!("❌ [DEBUG] Failed to load operations: {}", e);
                Err(e)
            }
        }
    }
    
    /// Get operations for a specific game
    pub async fn get_game_operations(&self, game_id: &str) -> Result<Vec<steam_cloud_sync_persistence::CloudOperation>> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Ok(Vec::new());
        };
        
        service_manager.get_game_operations(game_id).await
    }
    
    /// Get sync statistics
    pub async fn get_sync_stats(&self) -> Result<steam_cloud_sync_persistence::SyncStats> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Ok(steam_cloud_sync_persistence::SyncStats {
                total_sessions: 0,
                successful_sessions: 0,
                total_operations: 0,
                successful_operations: 0,
                total_bytes_synced: 0,
            });
        };
        
        service_manager.get_sync_stats().await
    }
    
    /// Get database statistics
    pub async fn get_database_stats(&self) -> Result<steam_cloud_sync_persistence::DatabaseStats> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Ok(steam_cloud_sync_persistence::DatabaseStats {
                operations_count: 0,
                sessions_count: 0,
                game_configs_count: 0,
                app_configs_count: 0,
                database_size: 0,
            });
        };
        
        service_manager.get_database_stats().await
    }
    
    /// Process progress updates
    pub async fn process_progress_updates(&self) -> Vec<steam_cloud_sync_cloud::ProgressUpdate> {
        if let Some(service_manager) = self.get_service_manager().await {
            service_manager.process_progress_updates().await
        } else {
            Vec::new()
        }
    }
    
    /// Enable/disable game sync
    pub async fn set_game_enabled(&self, game_id: &str, enabled: bool) -> Result<()> {
        let Some(service_manager) = self.get_service_manager().await else {
            return Ok(());
        };
        
        service_manager.set_game_enabled(game_id, enabled).await?;
        
        // Update local cache
        let mut cache = self.cache.lock().await;
        if let Some(game) = cache.games.iter_mut().find(|g| g.game.id == game_id) {
            game.sync_enabled = enabled;
        }
        
        Ok(())
    }
    
    /// Toggle game sync enabled/disabled (alias for compatibility)
    pub async fn toggle_game_sync_enabled(&self, game_id: &str, enabled: bool) -> Result<()> {
        self.set_game_enabled(game_id, enabled).await
    }
    
    /// Get sync history
    pub async fn get_sync_history(&self) -> Vec<SyncHistoryItem> {
        let cache = self.cache.lock().await;
        cache.sync_history.clone()
    }
    
    /// Add sync history item
    pub async fn add_sync_history(&self, item: SyncHistoryItem) {
        let mut cache = self.cache.lock().await;
        cache.sync_history.push(item);
        // Keep only last 100 items
        if cache.sync_history.len() > 100 {
            cache.sync_history.remove(0);
        }
    }
    
    /// Clear sync history
    pub async fn clear_sync_history(&self) {
        let mut cache = self.cache.lock().await;
        cache.sync_history.clear();
    }
    
    /// Undo sync operation
    pub async fn undo_sync(&self, game_id: &str) -> Result<()> {
        let mut cache = self.cache.lock().await;
        
        // Find and remove the undoable sync for this game
        if let Some(index) = cache.undoable_syncs.iter().position(|u| u.game_id == game_id) {
            let undoable = cache.undoable_syncs.remove(index);
            
            // Restore the backup
            if undoable.backup_path.exists() {
                std::fs::copy(&undoable.backup_path, &undoable.original_path)?;
                
                // Clean up backup file
                let _ = std::fs::remove_file(&undoable.backup_path);
                
                // Add to history
                let history_item = SyncHistoryItem {
                    game_name: undoable.game_name,
                    direction: crate::SyncDirection::Download, // Using download as "restored"
                    timestamp: chrono::Utc::now(),
                    duration: std::time::Duration::from_secs(0),
                    result: crate::SyncResult::Success,
                };
                
                cache.sync_history.push(history_item);
                
                Ok(())
            } else {
                Err(anyhow::anyhow!("Backup file not found"))
            }
        } else {
            Err(anyhow::anyhow!("No undoable sync found for this game"))
        }
    }
    
    /// Add undoable sync
    pub async fn add_undoable_sync(&self, undoable: UndoableSync) {
        let mut cache = self.cache.lock().await;
        cache.undoable_syncs.push(undoable);
        // Keep only last 10 undoable syncs
        if cache.undoable_syncs.len() > 10 {
            cache.undoable_syncs.remove(0);
        }
    }
    
    /// Get undoable sync for a game
    pub async fn get_undoable_sync(&self, game_id: &str) -> Option<UndoableSync> {
        let cache = self.cache.lock().await;
        cache.undoable_syncs.iter().find(|u| u.game_id == game_id).cloned()
    }
    
    /// Set manual mapping for a game's save location
    pub async fn set_manual_mapping(&self, app_id: u32, save_path: std::path::PathBuf) -> Result<()> {
        // Register the manual mapping
        steam_cloud_sync_core::register_manual_mapping(app_id, save_path.clone())?;
        
        // Update the game in our cache
        let mut cache = self.cache.lock().await;
        for game in cache.games.iter_mut() {
            if game.game.id == app_id.to_string() {
                let save_info = steam_cloud_sync_core::GameSave {
                    app_id,
                    name: game.game.name.clone(),
                    save_path,
                };
                game.save_info = Some(save_info);
                game.save_detection_status = SaveDetectionStatus::Found;
                break;
            }
        }
        
        Ok(())
    }
    
    /// Test cloud backend connection
    pub async fn test_cloud_backend(&self, backend_type: steam_cloud_sync_cloud::BackendType, settings: &AppSettings) -> Result<()> {
        // Create a temporary backend for testing
        let backend = match backend_type {
            steam_cloud_sync_cloud::BackendType::TencentCOS => {
                steam_cloud_sync_cloud::backend_with_settings(
                    backend_type,
                    Some((
                        settings.tencent_secret_id.clone(),
                        settings.tencent_secret_key.clone(),
                        settings.tencent_bucket.clone(),
                        settings.tencent_region.clone(),
                    )),
                    None,
                )
            }
            steam_cloud_sync_cloud::BackendType::S3 => {
                steam_cloud_sync_cloud::backend_with_settings(
                    backend_type,
                    None,
                    Some((settings.s3_bucket.clone(), "saves/".to_string())),
                )
            }
        };

        backend.test_connection().await
    }
    
    /// Check if scanning
    pub async fn is_scanning(&self) -> bool {
        let cache = self.cache.lock().await;
        cache.scanning
    }
}