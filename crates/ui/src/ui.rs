use eframe::egui;
use crate::{AppViewModel, LocalizationManager, SyncHistoryItem, GameWithSave, AppSettings};
use steam_cloud_sync_cloud::BackendType;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurrentPage {
    Home,
    CloudSaves,
    History,
    Settings,
}

#[derive(Clone, Debug)]
pub enum ConnectionTestStatus {
    None,
    Testing,
    Success,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct UndoNotification {
    pub game_id: String,
    pub game_name: String,
    pub show_time: std::time::Instant,
}

#[derive(Clone, Debug)]
pub enum UIMessage {
    UpdateDefaultDownloadPath(Option<String>),
}

pub struct SteamCloudSyncApp {
    pub view_model: Arc<AppViewModel>,
    pub localization: LocalizationManager,
    pub current_page: CurrentPage,
    pub games_cache: Arc<std::sync::Mutex<Vec<GameWithSave>>>,
    pub history_cache: Vec<SyncHistoryItem>,
    pub status_text: String,
    pub is_scanning: Arc<std::sync::Mutex<bool>>,
    pub settings: AppSettings,
    pub connection_test_result: Option<Result<(), String>>,
    pub connection_test_status: Arc<Mutex<ConnectionTestStatus>>,
    // Cloud saves page state
    pub cloud_saves_page: Option<crate::pages::cloud_saves::CloudSavesPage>,
    // History page state  
    pub history_page: Option<crate::pages::history::HistoryPage>,
    // Grouping and UI state
    pub pending_expanded: bool,
    pub synced_expanded: bool,
    pub unknown_expanded: bool,
    pub cloud_storage_used: u64,
    pub cloud_storage_total: u64,
    pub file_count: u32,
    // Bucket-wide storage information
    pub bucket_storage_used: u64,
    pub bucket_total_objects: u32,
    pub last_storage_update: Option<std::time::Instant>,
    pub storage_info_loading: bool,
    // Add shared storage info state
    pub storage_info_state: Arc<std::sync::Mutex<Option<steam_cloud_sync_cloud::StorageInfo>>>,
    // Undo notifications
    pub undo_notifications: Vec<UndoNotification>,
    // Flag to track first run
    pub first_run: bool,
    // UI message receiver
    pub ui_message_rx: Option<mpsc::UnboundedReceiver<UIMessage>>,
    pub ui_message_tx: mpsc::UnboundedSender<UIMessage>,
}

impl Default for SteamCloudSyncApp {
    fn default() -> Self {
        // Load settings from disk first
        let settings = AppSettings::load().unwrap_or_default();
        
        // Create view model - will be initialized later
        let view_model = AppViewModel::new();
        
        // Initialize localization with saved language
        let mut localization = LocalizationManager::new();
        let lang = match settings.language_index {
            1 => "zh-CN",
            _ => "en-US",
        };
        localization.set_language(lang.to_string());
        
        // Create UI message channel
        let (ui_tx, ui_rx) = mpsc::unbounded_channel();
        
        Self {
            view_model: Arc::new(view_model),
            localization,
            current_page: CurrentPage::Home,
            games_cache: Arc::new(std::sync::Mutex::new(Vec::new())),
            history_cache: Vec::new(),
            status_text: "Initializing...".to_string(),
            is_scanning: Arc::new(std::sync::Mutex::new(false)),
            settings,
            connection_test_result: None,
            connection_test_status: Arc::new(Mutex::new(ConnectionTestStatus::None)),
            cloud_saves_page: None,
            history_page: None,
            // Initialize grouping state
            pending_expanded: true,  // Default expand pending items
            synced_expanded: false,
            unknown_expanded: false,
            cloud_storage_used: 0,
            cloud_storage_total: 100 * 1024 * 1024 * 1024, // 100GB default - will be updated with real data
            file_count: 0,
            // Initialize bucket-wide storage fields
            bucket_storage_used: 0,
            bucket_total_objects: 0,
            last_storage_update: None,
            storage_info_loading: false,
            // Initialize shared storage info state
            storage_info_state: Arc::new(std::sync::Mutex::new(None)),
            undo_notifications: Vec::new(),
            first_run: true,
            ui_message_rx: Some(ui_rx),
            ui_message_tx: ui_tx,
        }
    }
}

impl eframe::App for SteamCloudSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process UI messages
        if let Some(rx) = &mut self.ui_message_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    UIMessage::UpdateDefaultDownloadPath(path) => {
                        self.settings.default_download_path = path;
                        if self.settings.auto_save_on_change {
                            if let Err(e) = self.settings.save() {
                                eprintln!("Failed to save settings: {}", e);
                            }
                        }
                    }
                }
            }
        }
        
        // Trigger initial scan on first run
        if self.first_run {
            self.first_run = false;
            
            // Initialize service manager for cloud operations
            self.initialize_service_manager();
            
            self.refresh_games();
        }
        
        // Update status based on scanning state
        let is_scanning = *self.is_scanning.lock().unwrap();
        if is_scanning && !self.status_text.contains("Scanning") {
            self.status_text = "Scanning for games...".to_string();
        } else if !is_scanning && self.status_text.contains("Scanning") {
            let games_count = self.games_cache.lock().unwrap().len();
            if games_count > 0 {
                self.status_text = format!("Ready - {} games found", games_count);
            } else {
                self.status_text = "Ready - No games found".to_string();
            }
        }
        
        // Update caches from async view model
        self.update_caches();
        
        // Clean up expired undo notifications
        let now = std::time::Instant::now();
        self.undo_notifications.retain(|notif| now.duration_since(notif.show_time).as_secs() < 10);
        
        // Show undo notifications
        if !self.undo_notifications.is_empty() {
            egui::Window::new("undo_notification")
                .title_bar(false)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-5.0, -40.0])
                .show(ctx, |ui| {
                    // Clone notifications to avoid borrow issues
                    let notifications = self.undo_notifications.clone();
                    for notif in notifications {
                        ui.horizontal(|ui| {
                            ui.label(format!("✓ {} synced successfully", notif.game_name));
                            if ui.button("Undo").clicked() {
                                self.undo_sync(notif.game_id);
                            }
                        });
                    }
                });
        }
        
        egui::TopBottomPanel::top("title_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(self.localization.get_string("AppTitle"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🌐").clicked() {
                        // Toggle theme
                        let visuals = if ctx.style().visuals.dark_mode {
                            egui::Visuals::light()
                        } else {
                            egui::Visuals::dark()
                        };
                        ctx.set_visuals(visuals);
                    }
                });
            });
        });

        egui::SidePanel::left("navigation").show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.selectable_value(&mut self.current_page, CurrentPage::Home, 
                    format!("🏠 {}", self.localization.get_string("Home")));
                ui.selectable_value(&mut self.current_page, CurrentPage::CloudSaves, 
                    format!("☁ {}", self.localization.get_string("CloudSaves")));
                ui.selectable_value(&mut self.current_page, CurrentPage::History, 
                    format!("📋 {}", self.localization.get_string("History")));
                ui.selectable_value(&mut self.current_page, CurrentPage::Settings, 
                    format!("⚙ {}", self.localization.get_string("Settings")));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.current_page {
                CurrentPage::Home => self.show_home_page(ui, ctx),
                CurrentPage::CloudSaves => self.show_cloud_saves_page(ui),
                CurrentPage::History => self.show_history_page(ui),
                CurrentPage::Settings => self.show_settings_page(ui),
            }
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_text);
                if *self.is_scanning.lock().unwrap() {
                    ui.spinner();
                }
            });
        });
    }
}

impl SteamCloudSyncApp {
    /// Initialize service manager for cloud operations
    fn initialize_service_manager(&mut self) {
        println!("🚀 [DEBUG] initialize_service_manager() called");
        
        let view_model = self.view_model.clone();
        let settings = self.settings.clone();
        
        // Check if cloud settings are configured
        let has_tencent_config = !settings.tencent_secret_id.is_empty() && !settings.tencent_secret_key.is_empty();
        let has_s3_config = !settings.s3_access_key.is_empty() && !settings.s3_secret_key.is_empty();
        
        println!("🔧 [DEBUG] Cloud configuration check:");
        println!("   - Tencent COS configured: {}", has_tencent_config);
        println!("   - S3 configured: {}", has_s3_config);
        println!("   - Selected backend: {:?}", settings.selected_backend);
        
        if !has_tencent_config && !has_s3_config {
            println!("⚠️ [DEBUG] No cloud storage credentials configured - service manager may not work properly");
        }
        
        tokio::spawn(async move {
            println!("🔄 [DEBUG] Starting service manager initialization in async task");
            match view_model.set_service_manager(&settings).await {
                Ok(_) => {
                    println!("✅ [DEBUG] Service manager initialized successfully");
                }
                Err(e) => {
                    println!("❌ [DEBUG] Failed to initialize service manager: {}", e);
                    eprintln!("✗ Failed to initialize service manager: {}", e);
                    println!("Note: Cloud features will be limited. Check your cloud storage settings.");
                }
            }
        });
    }

    fn update_caches(&mut self) {
        // Only update games cache if it's empty (no need to constantly refresh)
        // Manual refresh is handled by refresh_games() function
        let games_cache = self.games_cache.clone();
        let is_empty = games_cache.lock().unwrap().is_empty();
        let is_scanning = *self.is_scanning.lock().unwrap();
        
        if is_empty && !is_scanning {
            // Only load from cache if not currently scanning
            let view_model = self.view_model.clone();
            let cache_clone = games_cache.clone();
            tokio::spawn(async move {
                let games = view_model.get_games().await;
                if !games.is_empty() {
                    let mut cache = cache_clone.lock().unwrap();
                    *cache = games;
                    eprintln!("Games cache initialized: {} games loaded", cache.len());
                }
            });
        }
        
        // Check if there's updated storage info in the shared state
        if let Ok(state) = self.storage_info_state.lock() {
            if let Some(storage_info) = &*state {
                // Update UI fields from shared state
                let old_user_bytes = self.cloud_storage_used;
                let old_bucket_bytes = self.bucket_storage_used;
                let old_file_count = self.file_count;
                
                self.cloud_storage_used = storage_info.used_bytes;
                self.file_count = storage_info.file_count;
                
                if let Some(total_bytes) = storage_info.total_bytes {
                    self.cloud_storage_total = total_bytes;
                }
                
                if let Some(bucket_bytes) = storage_info.bucket_used_bytes {
                    self.bucket_storage_used = bucket_bytes;
                }
                
                if let Some(bucket_objects) = storage_info.bucket_total_objects {
                    self.bucket_total_objects = bucket_objects;
                }
                
                // Log if values changed (only once per change to avoid spam)
                if old_user_bytes != self.cloud_storage_used || old_bucket_bytes != self.bucket_storage_used || old_file_count != self.file_count {
                    println!("UI updated with storage info: user={} bytes, bucket={} bytes, files={}", 
                        self.cloud_storage_used, self.bucket_storage_used, self.file_count);
                }
                
                // Mark loading as complete
                self.storage_info_loading = false;
            }
        }
        
        // Update storage info periodically (every 30 seconds)
        let now = std::time::Instant::now();
        let should_update_storage = self.last_storage_update
            .map_or(true, |last| now.duration_since(last).as_secs() > 30);
            
        if should_update_storage && !self.storage_info_loading {
            self.update_storage_info();
        }
    }

    fn update_storage_info(&mut self) {
        // Set loading state
        self.storage_info_loading = true;
        self.last_storage_update = Some(std::time::Instant::now());
        
        let view_model = self.view_model.clone();
        let settings = self.settings.clone();
        let storage_state = self.storage_info_state.clone();
        
        tokio::spawn(async move {
            match view_model.get_storage_info(&settings).await {
                Ok(storage_info) => {
                    // Store the storage info in shared state
                    if let Ok(mut state) = storage_state.lock() {
                        *state = Some(storage_info.clone());
                    }
                    
                    println!("✓ Storage info loaded successfully:");
                    println!("  User storage: {} bytes ({} files)", 
                        storage_info.used_bytes, storage_info.file_count);
                    if let Some(bucket_bytes) = storage_info.bucket_used_bytes {
                        println!("  Bucket total: {} bytes", bucket_bytes);
                    }
                    if let Some(bucket_objects) = storage_info.bucket_total_objects {
                        println!("  Bucket objects: {}", bucket_objects);
                    }
                }
                Err(e) => {
                    // Clear the state on error
                    if let Ok(mut state) = storage_state.lock() {
                        *state = None;
                    }
                    
                    eprintln!("✗ Failed to get storage info: {}", e);
                    // This is expected if cloud credentials aren't configured
                    if e.to_string().contains("not configured") || e.to_string().contains("not initialized") {
                        println!("Note: Configure cloud settings to display storage information");
                    }
                }
            }
        });
    }

    // Removed sync_storage_info function - no longer needed with the new shared state approach

    fn get_connection_test_status(&self) -> ConnectionTestStatus {
        if let Ok(status) = self.connection_test_status.try_lock() {
            status.clone()
        } else {
            ConnectionTestStatus::None
        }
    }

    fn show_home_page(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.vertical(|ui| {
            // Batch operation toolbar
            ui.horizontal(|ui| {
                if ui.button(format!("↻ {}", self.localization.get_string("Refresh"))).clicked() {
                    self.refresh_games();
                }
                
                ui.separator();
                
                // Count selected games that need sync
                let pending_count = self.games_cache.lock().unwrap().iter()
                    .filter(|g| g.sync_enabled && matches!(g.sync_state, crate::SyncState::Pending))
                    .count();
                
                let sync_button_text = if pending_count > 0 {
                    format!("☁ {} ({})", self.localization.get_string("SyncNow"), pending_count)
                } else {
                    format!("☁ {}", self.localization.get_string("SyncNow"))
                };
                
                if ui.button(sync_button_text).clicked() {
                    self.sync_games();
                }
                
                ui.separator();
                
                // Select/Deselect All buttons
                if ui.button("☑ Select All").clicked() {
                    self.toggle_all_games_sync(true);
                }
                
                if ui.button("☐ Deselect All").clicked() {
                    self.toggle_all_games_sync(false);
                }
                
                // Cloud storage indicator
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Add refresh button for debugging
                    if ui.small_button("↻").on_hover_text("Refresh storage info").clicked() {
                        self.last_storage_update = None; // Force refresh
                        self.storage_info_loading = false;
                        
                        // Force immediate refresh of storage info
                        self.storage_info_loading = false; // Reset loading state
                        self.last_storage_update = None; // Force refresh
                        self.update_storage_info(); // Trigger immediate update
                    }
                    
                    if self.storage_info_loading {
                        ui.spinner();
                        ui.label("Loading storage info...");
                    } else {
                        // Check if cloud storage is configured
                        let has_cloud_config = match self.settings.selected_backend {
                            steam_cloud_sync_cloud::BackendType::TencentCOS => {
                                !self.settings.tencent_secret_id.is_empty() && !self.settings.tencent_secret_key.is_empty()
                            },
                            steam_cloud_sync_cloud::BackendType::S3 => {
                                !self.settings.s3_access_key.is_empty() && !self.settings.s3_secret_key.is_empty()
                            },
                        };
                        
                        if !has_cloud_config {
                            ui.label("⚙ Configure cloud storage in Settings");
                        } else {
                            // Display bucket storage size (total) with user file count
                            let display_storage = if self.bucket_storage_used > 0 {
                                self.bucket_storage_used
                            } else {
                                self.cloud_storage_used
                            };
                            let used_gb = display_storage as f64 / (1024.0 * 1024.0 * 1024.0);
                            
                            if self.file_count > 0 {
                                ui.label(format!("Bucket: {:.2} GB ({} user files)", used_gb, self.file_count));
                            } else if display_storage > 0 {
                                ui.label(format!("Bucket: {:.2} GB", used_gb));
                            } else {
                                ui.label("No storage used");
                            }
                        }
                    }
                });
            });
            
            ui.separator();
            
            // Group games by sync state
            let mut pending_games = Vec::new();
            let mut synced_games = Vec::new();
            let mut unknown_games = Vec::new();
            
            let games_copy = self.games_cache.lock().unwrap().clone();
            for game in &games_copy {
                match game.sync_state {
                    crate::SyncState::Pending => pending_games.push(game.clone()),
                    crate::SyncState::Synced => synced_games.push(game.clone()),
                    crate::SyncState::Unknown => unknown_games.push(game.clone()),
                }
            }
            
            // Games list with grouped sections
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Pending section
                if !pending_games.is_empty() {
                    let _header_response = ui.horizontal(|ui| {
                        let icon = if self.pending_expanded { "▼" } else { "▶" };
                        if ui.button(format!("{} Pending ({})", icon, pending_games.len())).clicked() {
                            self.pending_expanded = !self.pending_expanded;
                        }
                    });
                    
                    if self.pending_expanded {
                        for game in pending_games {
                            self.show_game_item(ui, game);
                        }
                    }
                    
                    ui.add_space(10.0);
                }
                
                // Synced section
                if !synced_games.is_empty() {
                    ui.horizontal(|ui| {
                        let icon = if self.synced_expanded { "▼" } else { "▶" };
                        if ui.button(format!("{} Synced ({})", icon, synced_games.len())).clicked() {
                            self.synced_expanded = !self.synced_expanded;
                        }
                    });
                    
                    if self.synced_expanded {
                        for game in synced_games {
                            self.show_game_item(ui, game);
                        }
                    }
                    
                    ui.add_space(10.0);
                }
                
                // Unknown section
                if !unknown_games.is_empty() {
                    ui.horizontal(|ui| {
                        let icon = if self.unknown_expanded { "▼" } else { "▶" };
                        if ui.button(format!("{} Unknown ({})", icon, unknown_games.len())).clicked() {
                            self.unknown_expanded = !self.unknown_expanded;
                        }
                    });
                    
                    if self.unknown_expanded {
                        for game in unknown_games {
                            self.show_game_item(ui, game);
                        }
                    }
                }
                
                if self.games_cache.lock().unwrap().is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label("No games found. Click Refresh to scan for games.");
                    });
                }
            });
        });
    }

    fn show_cloud_saves_page(&mut self, ui: &mut egui::Ui) {
        // Initialize cloud saves page if needed
        if self.cloud_saves_page.is_none() {
            self.cloud_saves_page = Some(crate::pages::cloud_saves::CloudSavesPage::with_context(
                self.view_model.clone(),
                self.settings.clone(),
            ));
        }
        
        if let Some(page) = &mut self.cloud_saves_page {
            crate::pages::cloud_saves::show_cloud_saves_page(
                ui,
                &self.games_cache,
                page,
                self.view_model.clone(),
                self.settings.clone(),
            );
        }
    }
    
    fn show_history_page(&mut self, ui: &mut egui::Ui) {
        // Initialize history page if needed
        if self.history_page.is_none() {
            self.history_page = Some(crate::pages::history::HistoryPage::with_context(
                self.view_model.clone(),
            ));
        }
        
        if let Some(page) = &mut self.history_page {
            crate::pages::history::show_history_page(
                ui,
                page,
                self.view_model.clone(),
            );
        }
    }

    fn show_settings_page(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.heading(self.localization.get_string("Settings"));
            ui.separator();
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Language settings
                ui.group(|ui| {
                    ui.strong(&self.localization.get_string("Language"));
                    ui.horizontal(|ui| {
                        ui.label(&self.localization.get_string("InterfaceLanguage"));
                        egui::ComboBox::from_label("")
                            .selected_text(match self.settings.language_index {
                                0 => "English",
                                1 => "简体中文",
                                _ => "English",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.settings.language_index, 0, "English");
                                ui.selectable_value(&mut self.settings.language_index, 1, "简体中文");
                            });
                    });
                    
                    if ui.button(&self.localization.get_string("ApplyLanguage")).clicked() {
                        let lang = match self.settings.language_index {
                            1 => "zh-CN",
                            _ => "en-US",
                        };
                        self.localization.set_language(lang.to_string());
                        
                        // Save settings when language changes
                        if let Err(e) = self.settings.save() {
                            eprintln!("Failed to save settings: {}", e);
                        }
                        
                        // Force UI refresh by requesting repaint
                        ui.ctx().request_repaint();
                    }
                });
                
                ui.separator();
                
                // User ID settings
                ui.group(|ui| {
                    ui.strong(&self.localization.get_string("UserID"));
                    ui.label(&self.localization.get_string("UserIDDescription"));
                    ui.text_edit_singleline(&mut self.settings.user_id)
                        .on_hover_text("Enter a unique user ID to distinguish your saves from other users sharing the same cloud storage");
                    ui.small(&self.localization.get_string("UserIDNote"));
                });
                
                ui.separator();
                
                // Cloud backend settings
                let previous_backend = self.settings.selected_backend;
                let mut settings_changed = false;
                
                ui.group(|ui| {
                    ui.strong(&self.localization.get_string("CloudBackend"));
                    
                    ui.radio_value(&mut self.settings.selected_backend, BackendType::TencentCOS, "Tencent Cloud COS");
                    ui.radio_value(&mut self.settings.selected_backend, BackendType::S3, "Amazon S3");
                    
                    match self.settings.selected_backend {
                        BackendType::TencentCOS => {
                            ui.label("Tencent Cloud COS Credentials:");
                            ui.text_edit_singleline(&mut self.settings.tencent_secret_id)
                                .on_hover_text("Tencent Cloud Secret ID");
                            ui.text_edit_singleline(&mut self.settings.tencent_secret_key)
                                .on_hover_text("Tencent Cloud Secret Key");
                            ui.text_edit_singleline(&mut self.settings.tencent_bucket)
                                .on_hover_text("COS Bucket Name");
                            ui.text_edit_singleline(&mut self.settings.tencent_region)
                                .on_hover_text("COS Region (e.g., ap-beijing)");
                        }
                        BackendType::S3 => {
                            ui.label("AWS Credentials:");
                            ui.text_edit_singleline(&mut self.settings.s3_access_key)
                                .on_hover_text("AWS Access Key ID");
                            ui.text_edit_singleline(&mut self.settings.s3_secret_key)
                                .on_hover_text("AWS Secret Access Key");
                            ui.text_edit_singleline(&mut self.settings.s3_bucket)
                                .on_hover_text("S3 Bucket Name");
                            ui.text_edit_singleline(&mut self.settings.s3_region)
                                .on_hover_text("AWS Region");
                        }
                    }
                    
                    ui.horizontal(|ui| {
                        if ui.button(&self.localization.get_string("SaveBackendSettings")).clicked() {
                            settings_changed = true;
                        }
                        
                        ui.label("|");
                        
                        ui.checkbox(&mut self.settings.auto_save_on_change, "Auto-save settings")
                            .on_hover_text("Automatically save settings when changed");
                    });
                    
                    ui.separator();
                    
                    // Connection test section
                    let test_status = self.get_connection_test_status();
                    let is_testing = matches!(test_status, ConnectionTestStatus::Testing);
                    
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!is_testing, egui::Button::new("Test Connection")).clicked() {
                            let view_model = self.view_model.clone();
                            let backend_type = self.settings.selected_backend;
                            let settings = self.settings.clone();
                            let test_status = self.connection_test_status.clone();
                            
                            // Set testing state
                            if let Ok(mut status) = test_status.try_lock() {
                                *status = ConnectionTestStatus::Testing;
                            }
                            
                            tokio::spawn(async move {
                                let result = view_model.test_cloud_backend(backend_type, &settings).await;
                                
                                // Update status based on result
                                let mut status = test_status.lock().await;
                                match result {
                                    Ok(_) => {
                                        *status = ConnectionTestStatus::Success;
                                    }
                                    Err(e) => {
                                        *status = ConnectionTestStatus::Failed(e.to_string());
                                    }
                                }
                            });
                        }
                        
                        // Show current test status
                        match test_status {
                            ConnectionTestStatus::None => {},
                            ConnectionTestStatus::Testing => {
                                ui.spinner();
                                ui.label("Testing connection...");
                            },
                            ConnectionTestStatus::Success => {
                                ui.colored_label(egui::Color32::GREEN, "✓ Connection successful!");
                            },
                            ConnectionTestStatus::Failed(ref error) => {
                                ui.colored_label(egui::Color32::RED, "✗ Connection failed")
                                    .on_hover_text(error);
                            },
                        }
                    });
                });
                
                // Auto-save logic
                if self.settings.auto_save_on_change || settings_changed || previous_backend != self.settings.selected_backend {
                    if let Err(e) = self.settings.save() {
                        eprintln!("Failed to save settings: {}", e);
                    }
                }
                
                // Clear test status if backend changed
                if previous_backend != self.settings.selected_backend {
                    if let Ok(mut status) = self.connection_test_status.try_lock() {
                        *status = ConnectionTestStatus::None;
                    }
                }
                
                ui.separator();
                
                // Application settings
                ui.group(|ui| {
                    ui.strong(&self.localization.get_string("Application"));
                    ui.checkbox(&mut self.settings.auto_start, &self.localization.get_string("StartWithWindows"));
                    ui.checkbox(&mut self.settings.rate_limit_enabled, &self.localization.get_string("EnableRateLimiting"));
                    
                    if self.settings.rate_limit_enabled {
                        ui.horizontal(|ui| {
                            ui.label("Upload speed (MB/s):");
                            ui.add(egui::Slider::new(&mut self.settings.rate_limit_value, 1.0..=100.0));
                        });
                    }
                    
                    ui.separator();
                    
                    // Download settings
                    ui.label(self.localization.get_string("DefaultDownloadLocation"));
                    ui.horizontal(|ui| {
                        let download_path_text = self.settings.default_download_path.as_ref()
                            .map(|p| p.as_str())
                            .unwrap_or("Not set (will prompt each time)");
                        ui.label(download_path_text);
                        
                        if ui.button("📁 Browse...").clicked() {
                            let tx = self.ui_message_tx.clone();
                            tokio::spawn(async move {
                                if let Some(folder) = rfd::AsyncFileDialog::new()
                                    .set_title("选择默认下载位置 / Select Default Download Location")
                                    .pick_folder()
                                    .await {
                                    let path = folder.path().to_string_lossy().to_string();
                                    println!("Selected default download path: {}", path);
                                    // Send message to update settings
                                    let _ = tx.send(UIMessage::UpdateDefaultDownloadPath(Some(path)));
                                }
                            });
                        }
                        
                        if self.settings.default_download_path.is_some() {
                            if ui.button("❌ Clear").clicked() {
                                self.settings.default_download_path = None;
                                if self.settings.auto_save_on_change {
                                    if let Err(e) = self.settings.save() {
                                        eprintln!("Failed to save settings: {}", e);
                                    }
                                }
                            }
                        }
                    });
                    
                    ui.separator();
                    
                    if ui.button("Save Application Settings").clicked() {
                        if let Err(e) = self.settings.save() {
                            eprintln!("Failed to save settings: {}", e);
                        }
                    }
                });
            });
        });
    }
    
    fn show_game_item(&mut self, ui: &mut egui::Ui, game_with_save: GameWithSave) {
        let game_id = game_with_save.game.id.clone();
        
        ui.group(|ui| {
            ui.horizontal(|ui| {
                // Left status color bar
                let color = match game_with_save.sync_state {
                    crate::SyncState::Synced => egui::Color32::from_rgb(0, 200, 0),    // Green
                    crate::SyncState::Pending => egui::Color32::from_rgb(255, 193, 7), // Amber
                    crate::SyncState::Unknown => egui::Color32::from_rgb(128, 128, 128), // Gray
                };
                
                // Draw 4px wide colored rectangle
                let rect = ui.available_rect_before_wrap();
                let painter = ui.painter();
                painter.rect_filled(
                    egui::Rect::from_min_size(rect.min, egui::vec2(4.0, 42.0)),
                    0.0,
                    color,
                );
                ui.add_space(8.0);
                
                // Main content area
                ui.vertical(|ui| {
                    ui.set_height(42.0); // Fixed height for performance
                    
                    ui.horizontal(|ui| {
                        // Sync enabled checkbox
                        let mut sync_enabled = game_with_save.sync_enabled;
                        if ui.checkbox(&mut sync_enabled, "").changed() {
                            let view_model = self.view_model.clone();
                            let game_id_clone = game_id.clone();
                            tokio::spawn(async move {
                                let _ = view_model.toggle_game_sync_enabled(&game_id_clone, sync_enabled).await;
                            });
                            
                            // Update local cache
                            let mut cache = self.games_cache.lock().unwrap();
                            for cached_game in cache.iter_mut() {
                                if cached_game.game.id == game_id {
                                    cached_game.sync_enabled = sync_enabled;
                                    break;
                                }
                            }
                        }
                        
                        // Game icon or progress bar
                        if let Some(progress) = game_with_save.sync_progress {
                            // Show progress bar during sync - smaller size for better visual balance
                            ui.add(egui::ProgressBar::new(progress).desired_width(24.0).desired_height(16.0));
                        } else {
                            ui.label("🎮");
                        }
                        
                        ui.vertical(|ui| {
                            ui.strong(&game_with_save.game.name);
                            
                            // Optimized path display
                            if let Some(save_info) = &game_with_save.save_info {
                                let path_str = save_info.save_path.to_string_lossy();
                                let display_path = if path_str.len() > 30 {
                                    format!("...{}", &path_str[path_str.len()-25..])
                                } else {
                                    path_str.to_string()
                                };
                                
                                ui.small(display_path)
                                    .on_hover_text(path_str.to_string());
                            }
                        });
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Sync/download status
                            if game_with_save.downloading {
                                ui.spinner();
                                ui.label(&self.localization.get_string("Downloading"));
                            } else if game_with_save.sync_progress.is_some() {
                                ui.label("Syncing...");
                            } else {
                                // Action buttons
                                match &game_with_save.save_info {
                                    Some(_) => {
                                        if !game_with_save.cloud_saves.is_empty() {
                                            if ui.small_button(&self.localization.get_string("Download")).clicked() {
                                                if let Some(latest_save) = game_with_save.cloud_saves.first() {
                                                    self.download_save_for_game(game_with_save.clone(), latest_save.clone());
                                                }
                                            }
                                        }
                                        
                                        if ui.small_button(&self.localization.get_string("RefreshCloudSaves")).clicked() {
                                            self.refresh_cloud_saves_for_game(game_id.clone());
                                        }
                                    }
                                    None => {
                                        if matches!(game_with_save.save_detection_status, 
                                            crate::SaveDetectionStatus::NotFound | 
                                            crate::SaveDetectionStatus::ManualMappingRequired) {
                                            if ui.small_button("Map Manually").clicked() {
                                                self.open_file_dialog_for_game(game_id.parse().unwrap_or(0));
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    });
                });
            });
        });
    }
    
    fn refresh_games(&mut self) {
        // Set scanning state
        *self.is_scanning.lock().unwrap() = true;
        
        let view_model = self.view_model.clone();
        let games_cache = self.games_cache.clone();
        let is_scanning = self.is_scanning.clone();
        
        tokio::spawn(async move {
            // Use force_scan_games for manual refresh to bypass cooldown
            match view_model.force_scan_games().await {
                Ok(games) => {
                    let mut cache = games_cache.lock().unwrap();
                    *cache = games;
                    eprintln!("Manual refresh complete: {} games found", cache.len());
                }
                Err(e) => {
                    eprintln!("Failed to scan games: {}", e);
                }
            }
            // Clear scanning state when done
            *is_scanning.lock().unwrap() = false;
        });
    }
    
    fn sync_games(&mut self) {
        println!("🚀 [DEBUG] sync_games() called - User clicked sync button");
        
        let view_model = self.view_model.clone();
        let settings = self.settings.clone();
        
        // Check if we have any games
        let games_count = self.games_cache.lock().unwrap().len();
        println!("🎮 [DEBUG] Games in cache: {}", games_count);
        
        // Check enabled games
        let enabled_count = self.games_cache.lock().unwrap().iter()
            .filter(|g| g.sync_enabled && matches!(g.sync_state, crate::SyncState::Pending))
            .count();
        println!("⚡ [DEBUG] Enabled games ready for sync: {}", enabled_count);
        
        if enabled_count == 0 {
            println!("⚠️ [DEBUG] No games enabled for sync - sync operation will do nothing");
        }
        
        tokio::spawn(async move {
            println!("🔄 [DEBUG] Starting async sync operation");
            match view_model.sync_now(&settings).await {
                Ok(_) => {
                    println!("✅ [DEBUG] sync_now() completed successfully");
                }
                Err(e) => {
                    println!("❌ [DEBUG] sync_now() failed: {}", e);
                    eprintln!("Sync error: {}", e);
                }
            }
        });
    }
    
    fn toggle_all_games_sync(&mut self, enabled: bool) {
        let view_model = self.view_model.clone();
        let game_ids: Vec<String> = self.games_cache.lock().unwrap().iter()
            .map(|game| game.game.id.clone())
            .collect();
            
        // Update local cache immediately for better UX
        let mut cache = self.games_cache.lock().unwrap();
        for game_with_save in cache.iter_mut() {
            game_with_save.sync_enabled = enabled;
        }
        
        // Update the view model asynchronously
        tokio::spawn(async move {
            for game_id in game_ids {
                let _ = view_model.toggle_game_sync_enabled(&game_id, enabled).await;
            }
        });
    }
    
    fn open_file_dialog_for_game(&mut self, app_id: u32) {
        let view_model = self.view_model.clone();
        
        // Run file dialog in a separate thread to avoid blocking UI
        tokio::spawn(async move {
            if let Some(folder) = rfd::AsyncFileDialog::new()
                .set_title("Select Save Game Folder")
                .set_directory(dirs::document_dir().unwrap_or_default())
                .pick_folder()
                .await
            {
                let path = folder.path().to_path_buf();
                if let Err(e) = view_model.set_manual_mapping(app_id, path).await {
                    eprintln!("Error setting manual mapping: {}", e);
                }
            }
        });
    }
    
    fn refresh_cloud_saves_for_game(&mut self, game_id: String) {
        let view_model = self.view_model.clone();
        let settings = self.settings.clone();
        
        tokio::spawn(async move {
            let _ = view_model.refresh_cloud_saves(&settings, &game_id).await;
        });
    }
    
    fn download_save_for_game(&mut self, game_with_save: GameWithSave, cloud_save: steam_cloud_sync_cloud::SaveMetadata) {
        let view_model = self.view_model.clone();
        let settings = self.settings.clone();
        let game_id = game_with_save.game.id.clone();
        
        // Set downloading state
        let download_view_model = view_model.clone();
        let game_id_for_download = game_id.clone();
        tokio::spawn(async move {
            let _ = download_view_model.set_game_downloading(&game_id_for_download, true).await;
        });
        
        tokio::spawn(async move {
            match view_model.download_save(&settings, &cloud_save, &game_with_save).await {
                Ok(_) => {
                    // Download successful
                    let _ = view_model.set_game_downloading(&game_id, false).await;
                }
                Err(e) => {
                    eprintln!("Download failed: {}", e);
                    let _ = view_model.set_game_downloading(&game_id, false).await;
                }
            }
        });
    }
    
    fn undo_sync(&mut self, game_id: String) {
        let view_model = self.view_model.clone();
        
        // Remove the notification immediately
        self.undo_notifications.retain(|n| n.game_id != game_id);
        
        tokio::spawn(async move {
            match view_model.undo_sync(&game_id).await {
                Ok(_) => {
                    println!("Successfully undid sync for game: {}", game_id);
                }
                Err(e) => {
                    eprintln!("Failed to undo sync: {}", e);
                }
            }
        });
    }
}