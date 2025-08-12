use crate::AppViewModel;
use egui::{ScrollArea, RichText, Color32};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use steam_cloud_sync_persistence::{CloudOperation, CloudOperationType};
use anyhow::Result;

pub struct HistoryPage {
    view_model: Option<Arc<AppViewModel>>,
    operations: Vec<CloudOperation>,
    loading: bool,
    last_refresh: Option<std::time::Instant>,
    refresh_started: bool,
    // Shared state for async-UI communication
    shared_operations: Arc<Mutex<Option<(Vec<CloudOperation>, HashMap<String, String>)>>>,
    // Game ID to name mapping for display
    game_names: HashMap<String, String>,
}

impl HistoryPage {
    pub fn new() -> Self {
        Self {
            view_model: None,
            operations: Vec::new(),
            loading: false,
            last_refresh: None,
            refresh_started: false,
            shared_operations: Arc::new(Mutex::new(None)),
            game_names: HashMap::new(),
        }
    }
    
    pub fn with_context(view_model: Arc<AppViewModel>) -> Self {
        Self {
            view_model: Some(view_model),
            operations: Vec::new(),
            loading: false,
            last_refresh: None,
            refresh_started: false,
            shared_operations: Arc::new(Mutex::new(None)),
            game_names: HashMap::new(),
        }
    }
    
    pub fn start_refresh(&mut self) {
        if let Some(view_model) = &self.view_model {
            if !self.refresh_started {
                println!("🔄 [DEBUG] Starting history refresh with shared state");
                self.loading = true;
                self.refresh_started = true;
                let vm_clone = view_model.clone();
                let shared_ops = self.shared_operations.clone();
                
                // Start the async task that updates shared state
                tokio::spawn(async move {
                    println!("🔍 [DEBUG] Async task: fetching operations and game names...");
                    match vm_clone.get_recent_operations().await {
                        Ok(operations) => {
                            println!("✅ [DEBUG] Async task: loaded {} operations, fetching game names...", operations.len());
                            
                            // Fetch game names for the operations
                            let games = vm_clone.get_games().await;
                            let mut game_names = HashMap::new();
                            for op in &operations {
                                if let Some(game) = games.iter().find(|g| g.game.id == op.game_id) {
                                    game_names.insert(op.game_id.clone(), game.game.name.clone());
                                }
                            }
                            println!("📋 [DEBUG] Async task: mapped {} game names", game_names.len());
                            
                            // Update shared state with both operations and game names
                            if let Ok(mut shared) = shared_ops.lock() {
                                *shared = Some((operations, game_names));
                                println!("🔄 [DEBUG] Async task: shared state updated successfully");
                            } else {
                                println!("❌ [DEBUG] Async task: failed to lock shared state");
                            }
                        }
                        Err(e) => {
                            println!("❌ [DEBUG] Async task: failed to load operations: {}", e);
                            
                            // Clear shared state on error
                            if let Ok(mut shared) = shared_ops.lock() {
                                *shared = Some((Vec::new(), HashMap::new()));
                            }
                        }
                    }
                });
            }
        }
    }
    
    pub fn check_and_update_from_shared_state(&mut self) {
        // Check if async task has completed and update local state
        if let Ok(mut shared) = self.shared_operations.lock() {
            if let Some((operations, game_names)) = shared.take() {
                println!("🔄 [DEBUG] UI: updating from shared state with {} operations and {} game names", operations.len(), game_names.len());
                self.operations = operations;
                self.game_names = game_names;
                self.loading = false;
                self.refresh_started = false;
                self.last_refresh = Some(std::time::Instant::now());
                println!("✅ [DEBUG] UI: local state updated successfully");
            }
        }
    }
    
    pub fn set_operations(&mut self, operations: Vec<CloudOperation>) {
        self.operations = operations;
        self.loading = false;
        self.refresh_started = false;
    }
    
    pub fn should_auto_refresh(&self) -> bool {
        // Auto refresh if no data and not currently loading
        (self.operations.is_empty() && !self.loading && !self.refresh_started) ||
        // Or if it's been more than 30 seconds since last refresh
        self.last_refresh.map_or(true, |last| last.elapsed().as_secs() > 30)
    }
}

pub fn show_history_page(ui: &mut egui::Ui, page: &mut HistoryPage, view_model: Arc<AppViewModel>) {
    // Update context if not set
    if page.view_model.is_none() {
        page.view_model = Some(view_model.clone());
    }
    
    // IMPORTANT: Check for async updates on every UI frame
    page.check_and_update_from_shared_state();
    
    // Header
    ui.horizontal(|ui| {
        ui.heading("📋 Sync History");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🔄 Refresh").clicked() {
                page.start_refresh();
            }
            if page.loading {
                ui.spinner();
                ui.label("Loading...");
            }
        });
    });
    
    ui.separator();
    
    // Auto-start refresh if needed
    if page.should_auto_refresh() {
        page.start_refresh();
    }
    
    ScrollArea::vertical().show(ui, |ui| {
        if page.loading && page.operations.is_empty() {
            // Show loading only if we have no data to display
            ui.centered_and_justified(|ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading history...");
                });
            });
        } else if page.operations.is_empty() {
            // Show empty state
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.0);
                    ui.label(RichText::new("📭 No sync history yet").size(16.0));
                    ui.add_space(10.0);
                    ui.label("Upload some games to see sync history here");
                    ui.add_space(10.0);
                    if ui.button("🔄 Refresh History").clicked() {
                        page.start_refresh();
                    }
                });
            });
        } else {
            // Show operations
            ui.label(format!("📋 Showing {} operations:", page.operations.len()));
            ui.add_space(5.0);
            
            for op in &page.operations {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        // Status indicator
                        let (color, icon) = match op.operation_type {
                            CloudOperationType::Upload => (Color32::from_rgb(0, 200, 0), "⬆"),
                            CloudOperationType::Download => (Color32::from_rgb(0, 150, 255), "⬇"),
                            CloudOperationType::Delete => (Color32::from_rgb(200, 0, 0), "🗑"),
                            CloudOperationType::List => (Color32::from_rgb(100, 100, 100), "📋"),
                            CloudOperationType::Restore => (Color32::from_rgb(255, 165, 0), "🔄"),
                        };
                        
                        ui.colored_label(color, icon);
                        // Display game name instead of ID
                        let game_display = if let Some(game_name) = page.game_names.get(&op.game_id) {
                            format!("Game: {}", game_name)
                        } else {
                            format!("Game: {} (ID: {})", op.game_id, op.game_id)
                        };
                        ui.label(game_display);
                        ui.separator();
                        
                        // Display operation status
                        let status_text = match op.status {
                            steam_cloud_sync_persistence::CloudOperationStatus::Pending => "⏳ Pending",
                            steam_cloud_sync_persistence::CloudOperationStatus::InProgress => "🔄 In Progress",
                            steam_cloud_sync_persistence::CloudOperationStatus::Completed => "✅ Completed",
                            steam_cloud_sync_persistence::CloudOperationStatus::Failed => "❌ Failed",
                            steam_cloud_sync_persistence::CloudOperationStatus::Cancelled => "🚫 Cancelled",
                        };
                        ui.label(status_text);
                        ui.separator();
                        
                        // Display file size if available
                        if let Some(size) = op.file_size {
                            let size_formatted = if size > 1024 * 1024 {
                                format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
                            } else if size > 1024 {
                                format!("{:.1} KB", size as f64 / 1024.0)
                            } else {
                                format!("{} bytes", size)
                            };
                            ui.label(size_formatted);
                        } else {
                            ui.label("- bytes");
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(op.started_at.format("%Y-%m-%d %H:%M:%S").to_string());
                        });
                    });
                    
                    // Show error message if any
                    if let Some(error) = &op.error_message {
                        ui.horizontal(|ui| {
                            ui.add_space(20.0);
                            ui.colored_label(Color32::RED, format!("❌ Error: {}", error));
                        });
                    }
                    
                    // Show file path if available
                    if let Some(file_path) = &op.file_path {
                        ui.horizontal(|ui| {
                            ui.add_space(20.0);
                            ui.colored_label(Color32::GRAY, format!("📁 Path: {}", file_path));
                        });
                    }
                });
                ui.add_space(2.0);
            }
        }
    });
}