use crate::{GameWithSave, AppSettings};
use crate::AppViewModel;
use std::sync::Arc;
use egui::{Color32, ProgressBar, ScrollArea, RichText};
use std::collections::HashMap;
use steam_cloud_sync_cloud::SaveMetadata;
use rfd::AsyncFileDialog;
use chrono;

#[derive(Clone, Debug)]
pub struct CloudSaveVersion {
    pub metadata: SaveMetadata,
    pub is_latest: bool,
    pub is_current_local: bool,
}

#[derive(Clone, Debug)]
pub struct CloudSaveGameEntry {
    pub game: GameWithSave,
    pub versions: Vec<CloudSaveVersion>,
    pub expanded: bool,
    pub loading: bool,
}

pub struct CloudSavesPage {
    // 游戏云存档列表
    pub game_entries: HashMap<String, CloudSaveGameEntry>,
    
    // 下载/恢复进度
    pub download_progress: HashMap<String, (u64, u64)>, // game_id -> (downloaded, total)
    
    // 删除确认对话框
    pub delete_confirm: Option<(String, SaveMetadata)>, // (game_id, save_metadata)
    
    // 版本详情弹窗
    pub version_detail: Option<CloudSaveVersion>,
    
    // 过滤和排序
    pub filter_text: String,
    pub show_only_with_saves: bool,
    pub sort_by_latest: bool,
    
    // 批量操作
    pub selected_versions: HashMap<String, bool>, // version_id -> selected
    
    // 统计信息
    pub total_cloud_size: u64,
    pub total_versions: usize,
    
    // Context
    pub view_model: Option<Arc<AppViewModel>>,
    pub settings: Option<AppSettings>,
}

impl CloudSavesPage {
    pub fn new() -> Self {
        Self {
            game_entries: HashMap::new(),
            download_progress: HashMap::new(),
            delete_confirm: None,
            version_detail: None,
            filter_text: String::new(),
            show_only_with_saves: true,
            sort_by_latest: true,
            selected_versions: HashMap::new(),
            total_cloud_size: 0,
            total_versions: 0,
            view_model: None,
            settings: None,
        }
    }
    
    pub fn with_context(view_model: Arc<AppViewModel>, settings: AppSettings) -> Self {
        let mut page = Self::new();
        page.view_model = Some(view_model);
        page.settings = Some(settings);
        page
    }
    
    pub fn refresh_cloud_saves(&mut self, games: &[GameWithSave]) {
        // Update game entries from the games list
        for game in games {
            let entry = self.game_entries.entry(game.game.id.clone()).or_insert_with(|| {
                CloudSaveGameEntry {
                    game: game.clone(),
                    versions: Vec::new(),
                    expanded: false,
                    loading: false,
                }
            });
            
            // Update game info
            entry.game = game.clone();
            
            // Convert cloud saves to versions
            if !game.cloud_saves.is_empty() {
                entry.versions = game.cloud_saves.iter().enumerate().map(|(i, save)| {
                    CloudSaveVersion {
                        metadata: save.clone(),
                        is_latest: i == 0, // First one is latest
                        is_current_local: false, // TODO: Compare with local save
                    }
                }).collect();
            }
        }
        
        // Calculate statistics
        self.total_cloud_size = 0;
        self.total_versions = 0;
        for entry in self.game_entries.values() {
            for version in &entry.versions {
                self.total_cloud_size += version.metadata.size_bytes;
                self.total_versions += 1;
            }
        }
    }
    
    pub fn load_game_saves(&mut self, game_id: String) {
        if let Some(entry) = self.game_entries.get_mut(&game_id) {
            entry.loading = true;
        }
        
        if let (Some(vm), Some(settings)) = (self.view_model.as_ref(), self.settings.as_ref()) {
            let vm_clone = vm.clone();
            let settings_clone = settings.clone();
            let game_id_clone = game_id.clone();
            
            tokio::spawn(async move {
                let _ = vm_clone.refresh_cloud_saves(&settings_clone, &game_id_clone).await;
            });
        }
    }
    
    fn start_download(&mut self, game_id: String, metadata: SaveMetadata) {
        self.download_progress.insert(game_id.clone(), (0, metadata.size_bytes));
        
        if let (Some(vm), Some(settings)) = (self.view_model.as_ref(), self.settings.as_ref()) {
            if let Some(entry) = self.game_entries.get(&game_id) {
                let vm_clone = vm.clone();
                let settings_clone = settings.clone();
                let game_clone = entry.game.clone();
                let metadata_clone = metadata.clone();
                let game_id_clone = game_id.clone();
                let game_name = entry.game.game.name.clone();
                let default_download_path = settings.default_download_path.clone();
                
                tokio::spawn(async move {
                    let _ = vm_clone.set_game_downloading(&game_id_clone, true).await;
                    
                    // Check if user has set a default download path
                    if let Some(default_path) = &default_download_path {
                        // Download to user's default location with generated filename
                        let download_dir = std::path::PathBuf::from(default_path);
                        
                        // Create directory if it doesn't exist
                        if let Err(e) = tokio::fs::create_dir_all(&download_dir).await {
                            eprintln!("Failed to create download directory {}: {}", download_dir.display(), e);
                            let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                            return;
                        }
                        
                        // Generate filename
                        let filename = format!("{}_backup_{}.zip", 
                            game_name.replace(' ', "_"), 
                            chrono::Local::now().format("%Y%m%d_%H%M%S"));
                        let target_path = download_dir.join(filename);
                        
                        println!("📥 Downloading to default location: {}", target_path.display());
                        
                        match vm_clone.download_save_to_path(&metadata_clone, &target_path).await {
                            Ok(_) => {
                                println!("✅ Download to default location completed: {}", target_path.display());
                                let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                            }
                            Err(e) => {
                                eprintln!("❌ Download to default location failed: {}", e);
                                let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                            }
                        }
                    } else {
                        // No default path set, use original behavior (download to game location)
                        match vm_clone.download_save(&settings_clone, &metadata_clone, &game_clone).await {
                            Ok(_) => {
                                println!("✅ Download to game location completed");
                                let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                            }
                            Err(e) => {
                                eprintln!("❌ Download to game location failed: {}", e);
                                let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                            }
                        }
                    }
                });
            }
        }
    }
    
    fn start_restore(&mut self, game_id: String, metadata: SaveMetadata) {
        self.download_progress.insert(game_id.clone(), (0, metadata.size_bytes));
        
        if let (Some(vm), Some(settings)) = (self.view_model.as_ref(), self.settings.as_ref()) {
            if let Some(entry) = self.game_entries.get(&game_id) {
                let vm_clone = vm.clone();
                let settings_clone = settings.clone();
                let game_clone = entry.game.clone();
                let metadata_clone = metadata.clone();
                let game_id_clone = game_id.clone();
                
                tokio::spawn(async move {
                    let _ = vm_clone.set_game_downloading(&game_id_clone, true).await;
                    // Always restore to original game location, ignore default_download_path
                    match vm_clone.download_save(&settings_clone, &metadata_clone, &game_clone).await {
                        Ok(_) => {
                            println!("✅ Restore to game location completed");
                            let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                        }
                        Err(e) => {
                            eprintln!("❌ Restore to game location failed: {}", e);
                            let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                        }
                    }
                });
            }
        }
    }
    
    fn start_download_to_custom_location(&mut self, game_id: String, metadata: SaveMetadata) {
        if let (Some(vm), Some(settings)) = (self.view_model.as_ref(), self.settings.as_ref()) {
            let vm_clone = vm.clone();
            let metadata_clone = metadata.clone();
            let game_id_clone = game_id.clone();
            let game_name = self.game_entries.get(&game_id)
                .map(|e| e.game.game.name.clone())
                .unwrap_or_else(|| game_id.clone());
            let default_download_path = settings.default_download_path.clone();
            
            tokio::spawn(async move {
                // Open file dialog to select save location
                let file_dialog = AsyncFileDialog::new()
                    .add_filter("ZIP Archive", &["zip"])
                    .set_file_name(&format!("{}_backup_{}.zip", 
                        game_name.replace(' ', "_"), 
                        chrono::Local::now().format("%Y%m%d_%H%M%S")))
                    .set_title("选择下载位置 / Select Download Location");
                
                // Set default directory if available
                let file_dialog = if let Some(default_path) = &default_download_path {
                    let path_buf = std::path::PathBuf::from(default_path);
                    
                    // Try to use the path directly first
                    if path_buf.exists() {
                        // Path exists, use it directly
                        if let Ok(canonical_path) = path_buf.canonicalize() {
                            file_dialog.set_directory(canonical_path)
                        } else {
                            file_dialog
                        }
                    } else {
                        // Path doesn't exist, try to create it or use parent
                        if let Some(parent) = path_buf.parent() {
                            if parent.exists() {
                                // Parent exists, try to create the directory
                                match std::fs::create_dir_all(&path_buf) {
                                    Ok(_) => {
                                        println!("✅ Created download directory: {}", path_buf.display());
                                        if let Ok(canonical_path) = path_buf.canonicalize() {
                                            file_dialog.set_directory(canonical_path)
                                        } else {
                                            file_dialog
                                        }
                                    }
                                    Err(e) => {
                                        println!("⚠️ Failed to create download directory {}: {}", path_buf.display(), e);
                                        println!("   Using parent directory instead");
                                        // Fall back to parent directory
                                        if let Ok(canonical_parent) = parent.canonicalize() {
                                            file_dialog.set_directory(canonical_parent)
                                        } else {
                                            file_dialog
                                        }
                                    }
                                }
                            } else {
                                println!("❌ Parent directory doesn't exist: {}", parent.display());
                                file_dialog
                            }
                        } else {
                            println!("❌ Invalid default download path: {}", default_path);
                            file_dialog
                        }
                    }
                } else {
                    file_dialog
                };
                
                if let Some(file_handle) = file_dialog.save_file().await {
                    let target_path = file_handle.path();
                    
                    println!("📥 Downloading to custom location: {}", target_path.display());
                    let _ = vm_clone.set_game_downloading(&game_id_clone, true).await;
                    
                    match vm_clone.download_save_to_path(&metadata_clone, target_path).await {
                        Ok(_) => {
                            println!("✅ Download to custom location completed: {}", target_path.display());
                            let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                        }
                        Err(e) => {
                            eprintln!("❌ Download to custom location failed: {}", e);
                            let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                        }
                    }
                } else {
                    println!("Download cancelled by user");
                    let _ = vm_clone.set_game_downloading(&game_id_clone, false).await;
                }
            });
        }
    }
    
    fn delete_save(&mut self, game_id: String, metadata: SaveMetadata) {
        if let (Some(vm), Some(settings)) = (self.view_model.as_ref(), self.settings.as_ref()) {
            let vm_clone = vm.clone();
            let settings_clone = settings.clone();
            let metadata_clone = metadata.clone();
            
            tokio::spawn(async move {
                match vm_clone.delete_save(&metadata_clone).await {
                    Ok(_) => {
                        println!("✅ Delete completed: {}", metadata_clone.file_id);
                        
                        // 强制刷新所有云存档，而不是只刷新特定游戏的存档
                        // 这是为了解决游戏ID映射不一致导致的删除后UI不更新问题
                        println!("🔄 Forcing full cloud saves refresh after deletion...");
                        match vm_clone.force_scan_games().await {
                            Ok(_) => {
                                println!("✅ Full games rescan completed after deletion");
                            }
                            Err(e) => {
                                println!("❌ Failed to rescan games after deletion: {}", e);
                                // 如果全量刷新失败，至少尝试刷新特定游戏
                                let _ = vm_clone.refresh_cloud_saves(&settings_clone, &game_id).await;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to delete save: {}", e);
                    }
                }
            });
        }
    }
}

pub fn show_cloud_saves_page(
    ui: &mut egui::Ui,
    games: &Arc<std::sync::Mutex<Vec<GameWithSave>>>,
    page: &mut CloudSavesPage,
    view_model: Arc<AppViewModel>,
    settings: AppSettings,
) {
    // Update context if not set
    if page.view_model.is_none() {
        page.view_model = Some(view_model.clone());
    }
    if page.settings.is_none() {
        page.settings = Some(settings.clone());
    }
    
    // Get games from mutex
    let games_list = games.lock().unwrap();
    
    // 避免重复刷新：只在页面首次显示或手动刷新时更新
    static mut LAST_REFRESH_TIME: Option<std::time::Instant> = None;
    static mut REFRESH_REQUESTED: bool = false;
    
    unsafe {
        let should_refresh = if let Some(last_time) = LAST_REFRESH_TIME {
            // 如果超过30秒或者手动请求刷新
            last_time.elapsed().as_secs() > 30 || REFRESH_REQUESTED
        } else {
            // 首次显示
            true
        };
        
        if should_refresh {
            println!("🎮 [DEBUG] Cloud saves page: processing {} games", games_list.len());
            
            // Count games with cloud saves
            let games_with_cloud_saves: Vec<_> = games_list.iter()
                .filter(|g| !g.cloud_saves.is_empty())
                .collect();
            
            println!("☁️ [DEBUG] Games with cloud saves: {}/{}", 
                games_with_cloud_saves.len(), games_list.len());
            
            for game in &games_with_cloud_saves {
                println!("   - {}: {} cloud saves", game.game.name, game.cloud_saves.len());
            }
            
            // Refresh cloud saves from games
            page.refresh_cloud_saves(&games_list);
            
            // 更新时间戳和标志
            LAST_REFRESH_TIME = Some(std::time::Instant::now());
            REFRESH_REQUESTED = false;
        }
    }
    
    // Header with statistics
    ui.horizontal(|ui| {
        ui.heading("☁ Cloud Saves Management");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!(
                "Total: {} versions | {:.2} GB",
                page.total_versions,
                page.total_cloud_size as f64 / (1024.0 * 1024.0 * 1024.0)
            ));
        });
    });
    
    ui.separator();
    
    // Toolbar
    ui.horizontal(|ui| {
        // Filter
        ui.label("🔍");
        ui.text_edit_singleline(&mut page.filter_text)
            .on_hover_text("Filter games by name");
        
        ui.separator();
        
        ui.checkbox(&mut page.show_only_with_saves, "Only with saves");
        ui.checkbox(&mut page.sort_by_latest, "Sort by latest");
        
        ui.separator();
        
        if ui.button("↻ Refresh All").clicked() {
            // 标记需要刷新
            unsafe {
                REFRESH_REQUESTED = true;
            }
            
            let vm = view_model.clone();
            let settings_clone = settings.clone();
            let games_clone = games.clone();
            tokio::spawn(async move {
                println!("🔄 [DEBUG] Manual refresh triggered - rescanning games and cloud saves");
                
                // First, scan for games to refresh the cache
                match vm.force_scan_games().await {
                    Ok(updated_games) => {
                        println!("✅ [DEBUG] Games rescanned: {} found", updated_games.len());
                        // Update games cache with rescanned results
                        {
                            let mut games_cache = games_clone.lock().unwrap();
                            *games_cache = updated_games;
                        }
                    }
                    Err(e) => {
                        println!("❌ [DEBUG] Failed to rescan games: {}", e);
                    }
                }
                
                // Then refresh cloud saves
                match vm.list_cloud_saves(&settings_clone, None).await {
                    Ok(_) => {
                        println!("✅ [DEBUG] Cloud saves refreshed successfully");
                    }
                    Err(e) => {
                        println!("❌ [DEBUG] Failed to refresh cloud saves: {}", e);
                    }
                }
            });
        }
        
        // Batch operations
        let selected_count = page.selected_versions.values().filter(|&&v| v).count();
        if selected_count > 0 {
            ui.separator();
            if ui.button(format!("🗑 Delete Selected ({})", selected_count)).clicked() {
                // Handle batch delete
            }
        }
    });
    
    ui.separator();
    
    // Delete confirmation dialog
    let mut delete_action = None;
    if let Some((game_id, metadata)) = &page.delete_confirm.clone() {
        egui::Window::new("Confirm Delete")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "Delete cloud save?\n\nFile: {}\nGame ID: {}\nTimestamp: {}\nSize: {:.2} MB\n\nThis action cannot be undone.",
                    metadata.file_id.split('/').last().unwrap_or(&metadata.file_id),
                    metadata.game_id,
                    metadata.timestamp,
                    metadata.size_bytes as f64 / (1024.0 * 1024.0)
                ));
                ui.horizontal(|ui| {
                    if ui.button("🗑 Delete").clicked() {
                        delete_action = Some((game_id.clone(), metadata.clone()));
                    }
                    if ui.button("Cancel").clicked() {
                        page.delete_confirm = None;
                    }
                });
            });
    }
    
    // Execute delete action if confirmed
    if let Some((game_id, metadata)) = delete_action {
        page.delete_save(game_id, metadata);
        page.delete_confirm = None;
    }
    
    // Version detail dialog
    let mut clear_detail = false;
    if let Some(version) = page.version_detail.clone() {
        egui::Window::new("Version Details")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.strong("Version Information");
                ui.separator();
                egui::Grid::new("version_details")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("File ID:");
                        ui.label(&version.metadata.file_id);
                        ui.end_row();
                        
                        ui.label("Timestamp:");
                        ui.label(&version.metadata.timestamp);
                        ui.end_row();
                        
                        ui.label("Size:");
                        ui.label(format!("{:.2} MB", version.metadata.size_bytes as f64 / (1024.0 * 1024.0)));
                        ui.end_row();
                        
                        ui.label("SHA256:");
                        ui.label(&version.metadata.checksum);
                        ui.end_row();
                        
                        ui.label("Compressed:");
                        ui.label(if version.metadata.compressed { "Yes" } else { "No" });
                        ui.end_row();
                        
                        ui.label("Status:");
                        if version.is_latest {
                            ui.colored_label(Color32::GREEN, "✔ Latest");
                        } else if version.is_current_local {
                            ui.colored_label(Color32::LIGHT_BLUE, "✔ Current Local");
                        } else {
                            ui.label("Previous Version");
                        }
                        ui.end_row();
                    });
                
                ui.separator();
                
                // Action buttons
                ui.horizontal(|ui| {
                    if ui.button("⬇ Download").clicked() {
                        if let Some(game_id) = page.game_entries.iter()
                            .find(|(_, entry)| entry.versions.iter().any(|v| v.metadata.file_id == version.metadata.file_id))
                            .map(|(id, _)| id.clone()) {
                            page.start_download(game_id, version.metadata.clone());
                            clear_detail = true;
                        }
                    }
                    
                    if ui.button("📁 Download to...").clicked() {
                        if let Some(game_id) = page.game_entries.iter()
                            .find(|(_, entry)| entry.versions.iter().any(|v| v.metadata.file_id == version.metadata.file_id))
                            .map(|(id, _)| id.clone()) {
                            page.start_download_to_custom_location(game_id, version.metadata.clone());
                            clear_detail = true;
                        }
                    }
                    
                    if ui.button("♻ Restore").clicked() {
                        if let Some(game_id) = page.game_entries.iter()
                            .find(|(_, entry)| entry.versions.iter().any(|v| v.metadata.file_id == version.metadata.file_id))
                            .map(|(id, _)| id.clone()) {
                            page.start_restore(game_id, version.metadata.clone());
                            clear_detail = true;
                        }
                    }
                    
                    if ui.button("Close").clicked() {
                        clear_detail = true;
                    }
                });
            });
    }
    
    if clear_detail {
        page.version_detail = None;
    }
    
    // Game list with cloud saves
    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // Filter and sort entries
            let mut entries: Vec<CloudSaveGameEntry> = page.game_entries.values()
                .filter(|e| {
                    if page.show_only_with_saves && e.versions.is_empty() {
                        return false;
                    }
                    if !page.filter_text.is_empty() {
                        let filter = page.filter_text.to_lowercase();
                        return e.game.game.name.to_lowercase().contains(&filter);
                    }
                    true
                })
                .cloned()
                .collect();
            
            if page.sort_by_latest {
                entries.sort_by(|a, b| {
                    let a_latest = a.versions.first().map(|v| &v.metadata.timestamp);
                    let b_latest = b.versions.first().map(|v| &v.metadata.timestamp);
                    b_latest.cmp(&a_latest)
                });
            }
            
            if entries.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.0);
                        ui.label(RichText::new("☁️ No cloud saves found").size(18.0));
                        ui.add_space(10.0);
                        if page.show_only_with_saves {
                            ui.label("Try unchecking 'Only with saves' to see all games");
                        } else {
                            ui.label("Upload some games to see cloud saves here");
                        }
                        ui.add_space(10.0);
                        if ui.button("↻ Refresh Cloud Saves").clicked() {
                            // 标记需要刷新
                            unsafe {
                                REFRESH_REQUESTED = true;
                            }
                            
                            // Trigger the same refresh logic as the toolbar button
                            let vm = page.view_model.as_ref().unwrap().clone();
                            let settings_clone = page.settings.as_ref().unwrap().clone();
                            tokio::spawn(async move {
                                println!("🔄 [DEBUG] Manual refresh triggered from empty state");
                                match vm.force_scan_games().await {
                                    Ok(games) => {
                                        println!("✅ [DEBUG] Manual refresh: {} games found", games.len());
                                    }
                                    Err(e) => {
                                        println!("❌ [DEBUG] Manual refresh failed: {}", e);
                                    }
                                }
                            });
                        }
                    });
                });
            } else {
                for entry in entries {
                    let game_id = entry.game.game.id.clone();
                    show_game_entry(ui, page, entry, game_id);
                }
            }
        });
}

fn show_game_entry(ui: &mut egui::Ui, page: &mut CloudSavesPage, entry: CloudSaveGameEntry, game_id: String) {
    ui.group(|ui| {
        // Game header
        ui.horizontal(|ui| {
            // Expand/collapse button
            let icon = if entry.expanded { "▼" } else { "▶" };
            if ui.button(icon).clicked() {
                if let Some(e) = page.game_entries.get_mut(&game_id) {
                    e.expanded = !e.expanded;
                    
                    // Load saves if expanding and empty
                    if e.expanded && e.versions.is_empty() && !e.loading {
                        page.load_game_saves(game_id.clone());
                    }
                }
            }
            
            // Game icon and name
            ui.label("🎮");
            ui.strong(&entry.game.game.name);
            
            // Save count and size
            let total_size: u64 = entry.versions.iter().map(|v| v.metadata.size_bytes).sum();
            ui.label(format!(
                "({} versions, {:.2} MB)",
                entry.versions.len(),
                total_size as f64 / (1024.0 * 1024.0)
            ));
            
            // Loading indicator
            if entry.loading {
                ui.spinner();
            }
            
            // Download progress for this game
            if let Some((downloaded, total)) = page.download_progress.get(&game_id) {
                let progress = *downloaded as f32 / *total as f32;
                ui.add(ProgressBar::new(progress).desired_width(100.0));
            }
        });
        
        // Version list (if expanded)
        if entry.expanded && !entry.versions.is_empty() {
            ui.separator();
            
            // Version table header
            ui.horizontal(|ui| {
                ui.label(RichText::new("Version").strong());
                ui.separator();
                ui.label(RichText::new("Timestamp").strong());
                ui.separator();
                ui.label(RichText::new("Size").strong());
                ui.separator();
                ui.label(RichText::new("Status").strong());
                ui.separator();
                ui.label(RichText::new("Actions").strong());
            });
            
            ui.separator();
            
            // Version rows
            for version in &entry.versions {
                ui.horizontal(|ui| {
                    // Checkbox for batch operations
                    let version_id = version.metadata.file_id.clone();
                    let mut selected = page.selected_versions.get(&version_id).copied().unwrap_or(false);
                    if ui.checkbox(&mut selected, "").changed() {
                        page.selected_versions.insert(version_id, selected);
                    }
                    
                    // Version info
                    ui.label(&version.metadata.file_id[..20.min(version.metadata.file_id.len())]);
                    ui.separator();
                    ui.label(&version.metadata.timestamp);
                    ui.separator();
                    ui.label(format!("{:.2} MB", version.metadata.size_bytes as f64 / (1024.0 * 1024.0)));
                    ui.separator();
                    
                    // Status
                    if version.is_latest {
                        ui.colored_label(Color32::GREEN, "✔ Latest");
                    } else if version.is_current_local {
                        ui.colored_label(Color32::LIGHT_BLUE, "✔ Local");
                    } else {
                        ui.label("-");
                    }
                    ui.separator();
                    
                    // Actions
                    if ui.small_button("👁").on_hover_text("View Details").clicked() {
                        page.version_detail = Some(version.clone());
                    }
                    
                    if ui.small_button("⬇").on_hover_text("Download (uses default location if set, otherwise to game folder)").clicked() {
                        page.start_download(game_id.clone(), version.metadata.clone());
                    }
                    
                    if ui.small_button("📁").on_hover_text("Choose download location...").clicked() {
                        page.start_download_to_custom_location(game_id.clone(), version.metadata.clone());
                    }
                    
                    if ui.small_button("♻").on_hover_text("Restore to game location").clicked() {
                        // Always restore to game location, ignore default_download_path
                        page.start_restore(game_id.clone(), version.metadata.clone());
                    }
                    
                    if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                        page.delete_confirm = Some((game_id.clone(), version.metadata.clone()));
                    }
                });
            }
        } else if entry.expanded && entry.versions.is_empty() && !entry.loading {
            ui.label("No cloud saves found for this game");
        }
    });
    
    ui.add_space(5.0);
}