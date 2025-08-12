use crate::{AppViewModel, GameWithSave, SyncState, AppSettings};
use egui::{Color32, ProgressBar, ScrollArea};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct CloudSaveEntry {
    pub version_id: String,
    pub timestamp: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub is_current: bool,
}

#[derive(Clone, Debug)]
pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
}

pub struct HomePage {
    // 分组展开状态
    pub pending_expanded: bool,
    pub synced_expanded: bool,
    pub unknown_expanded: bool,
    
    // 选中的游戏
    pub selected_games: HashMap<String, bool>,
    
    // 云存档列表 (app_id -> saves) - using actual cloud saves from backend
    pub cloud_saves_expanded: HashMap<String, bool>,
    
    // 下载/恢复进度 (app_id -> progress)
    pub in_flight: HashMap<String, DownloadProgress>,
    
    // 删除确认对话框
    pub delete_confirm: Option<(String, String)>, // (app_id, version_id)
    
    // 云端空间使用情况
    pub cloud_storage_used: u64,
    pub cloud_storage_total: u64,
    
    // 加载状态
    pub loading_saves: HashMap<String, bool>,
    
    // Reference to view model and settings
    pub view_model: Option<AppViewModel>,
    pub settings: Option<AppSettings>,
}

impl HomePage {
    pub fn new() -> Self {
        Self {
            pending_expanded: true,
            synced_expanded: false,
            unknown_expanded: false,
            selected_games: HashMap::new(),
            cloud_saves_expanded: HashMap::new(),
            in_flight: HashMap::new(),
            delete_confirm: None,
            cloud_storage_used: 0,
            cloud_storage_total: 100 * 1024 * 1024 * 1024, // 100GB默认
            loading_saves: HashMap::new(),
            view_model: None,
            settings: None,
        }
    }
    
    pub fn with_context(view_model: AppViewModel, settings: AppSettings) -> Self {
        let mut home = Self::new();
        home.view_model = Some(view_model);
        home.settings = Some(settings);
        home
    }
    
    // 加载云存档 - 使用实际的后端调用
    pub fn load_cloud_saves(&mut self, app_id: String) {
        if let (Some(view_model), Some(settings)) = (self.view_model.as_ref(), self.settings.as_ref()) {
            self.loading_saves.insert(app_id.clone(), true);
            
            let vm = view_model.clone();
            let settings = settings.clone();
            let app_id_clone = app_id.clone();
            
            tokio::spawn(async move {
                // Refresh cloud saves for this game through the view model
                let _ = vm.refresh_cloud_saves(&settings, &app_id_clone).await;
            });
        }
    }
    
    // 模拟下载进度更新
    pub fn update_download_progress(&mut self, app_id: &str, bytes_downloaded: u64, total_bytes: u64) {
        self.in_flight.insert(
            app_id.to_string(),
            DownloadProgress {
                bytes_downloaded,
                total_bytes,
            },
        );
    }
    
    // 完成下载
    pub fn finish_download(&mut self, app_id: &str) {
        self.in_flight.remove(app_id);
    }
}

pub fn show_home_page(ui: &mut egui::Ui, games: &[GameWithSave], home: &mut HomePage, view_model: AppViewModel, settings: AppSettings) {
    // Update home context if not set
    if home.view_model.is_none() {
        home.view_model = Some(view_model.clone());
    }
    if home.settings.is_none() {
        home.settings = Some(settings.clone());
    }
    // 工具栏
    ui.horizontal(|ui| {
        if ui.button("↻ 刷新").clicked() {
            // 触发刷新逻辑
            let vm = view_model.clone();
            tokio::spawn(async move {
                let _ = vm.scan_games().await;
            });
        }
        
        ui.separator();
        
        // 统计选中的待同步游戏数量
        let selected_count = games.iter()
            .filter(|g| {
                home.selected_games.get(&g.game.id).copied().unwrap_or(false)
                    && matches!(g.sync_state, SyncState::Pending)
            })
            .count();
        
        let sync_text = if selected_count > 0 {
            format!("☁ 立即同步 ({})", selected_count)
        } else {
            "☁ 立即同步".to_string()
        };
        
        if ui.button(sync_text).clicked() && selected_count > 0 {
            // 触发同步逻辑
            let vm = view_model.clone();
            let settings_clone = settings.clone();
            tokio::spawn(async move {
                let _ = vm.sync_now(&settings_clone).await;
            });
        }
        
        ui.separator();
        
        // 全选/取消全选
        if ui.button("☑ 全选").clicked() {
            for game in games {
                home.selected_games.insert(game.game.id.clone(), true);
            }
        }
        
        if ui.button("☐ 取消全选").clicked() {
            home.selected_games.clear();
        }
        
        // 云端空间显示
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let used_gb = home.cloud_storage_used as f64 / (1024.0 * 1024.0 * 1024.0);
            let total_gb = home.cloud_storage_total as f64 / (1024.0 * 1024.0 * 1024.0);
            ui.label(format!("已用: {:.2} GB / {:.2} GB", used_gb, total_gb));
        });
    });
    
    ui.separator();
    
    // 删除确认对话框
    if let Some((app_id, version_id)) = &home.delete_confirm.clone() {
        egui::Window::new("确认删除")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("确定要删除这个云存档版本吗？");
                ui.horizontal(|ui| {
                    if ui.button("确认").clicked() {
                        // 执行删除逻辑
                        if let (Some(vm), Some(settings)) = (&home.view_model, &home.settings) {
                            // Find the save metadata to delete
                            let vm_clone = vm.clone();
                            let settings_clone = settings.clone();
                            let app_id_clone = app_id.clone();
                            let version_id_clone = version_id.clone();
                            
                            tokio::spawn(async move {
                                // Get saves for this game
                                if let Ok(saves) = vm_clone.list_cloud_saves(&settings_clone, Some(&app_id_clone)).await {
                                    // Find the specific save to delete
                                    if let Some(save_to_delete) = saves.iter().find(|s| s.file_id == version_id_clone) {
                                        // Delete through cloud backend
                                        let backend = match settings_clone.selected_backend {
                                            steam_cloud_sync_cloud::BackendType::TencentCOS => {
                                                steam_cloud_sync_cloud::backend_with_settings(
                                                    settings_clone.selected_backend,
                                                    Some((
                                                        settings_clone.tencent_secret_id.clone(),
                                                        settings_clone.tencent_secret_key.clone(),
                                                        settings_clone.tencent_bucket.clone(),
                                                        settings_clone.tencent_region.clone(),
                                                    )),
                                                    None,
                                                )
                                            }
                                            steam_cloud_sync_cloud::BackendType::S3 => {
                                                steam_cloud_sync_cloud::backend_with_settings(
                                                    settings_clone.selected_backend,
                                                    None,
                                                    Some((settings_clone.s3_bucket.clone(), "saves/".to_string())),
                                                )
                                            }
                                        };
                                        
                                        if let Err(e) = backend.delete_save(save_to_delete).await {
                                            eprintln!("Failed to delete save: {}", e);
                                        } else {
                                            // Refresh saves after deletion
                                            let _ = vm_clone.refresh_cloud_saves(&settings_clone, &app_id_clone).await;
                                        }
                                    }
                                }
                            });
                        }
                        home.delete_confirm = None;
                    }
                    if ui.button("取消").clicked() {
                        home.delete_confirm = None;
                    }
                });
            });
    }
    
    // 分组游戏列表
    let mut pending_games = Vec::new();
    let mut synced_games = Vec::new();
    let mut unknown_games = Vec::new();
    
    for game in games {
        match game.sync_state {
            SyncState::Pending => pending_games.push(game.clone()),
            SyncState::Synced => synced_games.push(game.clone()),
            SyncState::Unknown => unknown_games.push(game.clone()),
        }
    }
    
    // 使用虚拟化滚动区域
    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // Pending组
            if !pending_games.is_empty() {
                egui::CollapsingHeader::new(format!("▼ 待同步 ({})", pending_games.len()))
                    .default_open(home.pending_expanded)
                    .show(ui, |ui| {
                        for game in pending_games {
                            show_game_row(ui, home, &game);
                        }
                    });
            }
            
            // Synced组
            if !synced_games.is_empty() {
                egui::CollapsingHeader::new(format!("▼ 已同步 ({})", synced_games.len()))
                    .default_open(home.synced_expanded)
                    .show(ui, |ui| {
                        for game in synced_games {
                            show_game_row(ui, home, &game);
                        }
                    });
            }
            
            // Unknown组
            if !unknown_games.is_empty() {
                egui::CollapsingHeader::new(format!("▼ 未知 ({})", unknown_games.len()))
                    .default_open(home.unknown_expanded)
                    .show(ui, |ui| {
                        for game in unknown_games {
                            show_game_row(ui, home, &game);
                        }
                    });
            }
        });
}

fn show_game_row(ui: &mut egui::Ui, home: &mut HomePage, game: &GameWithSave) {
    let view_model = home.view_model.clone();
    let settings = home.settings.clone();
    ui.group(|ui| {
        ui.set_max_height(42.0); // 固定行高
        
        ui.horizontal(|ui| {
            // 左侧状态色条 (4px)
            let color = match game.sync_state {
                SyncState::Synced => Color32::from_rgb(0, 200, 0),    // 绿色
                SyncState::Pending => Color32::from_rgb(255, 193, 7), // 琥珀色
                SyncState::Unknown => Color32::from_rgb(128, 128, 128), // 灰色
            };
            
            let rect = ui.available_rect_before_wrap();
            let painter = ui.painter();
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(4.0, 42.0)),
                0.0,
                color,
            );
            ui.add_space(8.0);
            
            // 多选框
            let mut selected = home.selected_games.get(&game.game.id).copied().unwrap_or(false);
            if ui.checkbox(&mut selected, "").changed() {
                home.selected_games.insert(game.game.id.clone(), selected);
            }
            
            // 游戏信息
            ui.vertical(|ui| {
                ui.strong(&game.game.name);
                
                // 显示路径（优化长路径显示）
                if let Some(save_info) = &game.save_info {
                    let path_str = save_info.save_path.to_string_lossy();
                    let display_path = if path_str.len() > 30 {
                        format!("...{}", &path_str[path_str.len()-25..])
                    } else {
                        path_str.to_string()
                    };
                    ui.small(display_path).on_hover_text(path_str.to_string());
                }
            });
            
            // 进度条（如果正在下载/同步）
            if let Some(progress_info) = home.in_flight.get(&game.game.id) {
                let progress = progress_info.bytes_downloaded as f32 / progress_info.total_bytes as f32;
                ui.add(ProgressBar::new(progress).desired_width(100.0));
            } else if let Some(sync_progress) = game.sync_progress {
                ui.add(ProgressBar::new(sync_progress).desired_width(100.0));
            }
            
            // 云存档操作区
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let is_loading = home.loading_saves.get(&game.game.id).copied().unwrap_or(false);
                
                if is_loading {
                    ui.spinner();
                } else if game.cloud_saves.is_empty() {
                    if ui.small_button("加载云存档").clicked() {
                        home.load_cloud_saves(game.game.id.clone());
                    }
                } else {
                    let saves = &game.cloud_saves;
                    ui.menu_button(format!("云存档 ({})", saves.len()), |ui| {
                        ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for (i, save) in saves.iter().enumerate() {
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            // 版本信息
                                            ui.label(&save.timestamp);
                                            ui.label(format!("{:.2} MB", save.size_bytes as f64 / (1024.0 * 1024.0)));
                                            
                                            // 当前版本标记 - first one is the latest
                                            if i == 0 {
                                                ui.colored_label(Color32::GREEN, "✔ Latest");
                                            }
                                        });
                                        
                                        ui.horizontal(|ui| {
                                            // 操作按钮
                                            if ui.small_button("恢复").clicked() {
                                                // 触发恢复逻辑 - 下载到本地并覆盖
                                                if let (Some(vm), Some(settings)) = (view_model.as_ref(), settings.as_ref()) {
                                                    let vm_clone = vm.clone();
                                                    let settings_clone = settings.clone();
                                                    let save_metadata = save.clone();
                                                    let game_clone = game.clone();
                                                    
                                                    tokio::spawn(async move {
                                                        let _ = vm_clone.download_save(&settings_clone, &save_metadata, &game_clone).await;
                                                    });
                                                }
                                            }
                                            
                                            if ui.small_button("下载").clicked() {
                                                // 触发下载逻辑
                                                home.update_download_progress(&game.game.id, 0, save.size_bytes);
                                                
                                                if let (Some(vm), Some(settings)) = (view_model.as_ref(), settings.as_ref()) {
                                                    let vm_clone = vm.clone();
                                                    let settings_clone = settings.clone();
                                                    let save_metadata = save.clone();
                                                    let game_clone = game.clone();
                                                    let game_id = game.game.id.clone();
                                                    
                                                    tokio::spawn(async move {
                                                        let _ = vm_clone.set_game_downloading(&game_id, true).await;
                                                        match vm_clone.download_save(&settings_clone, &save_metadata, &game_clone).await {
                                                            Ok(_) => {
                                                                let _ = vm_clone.set_game_downloading(&game_id, false).await;
                                                            }
                                                            Err(e) => {
                                                                eprintln!("Download failed: {}", e);
                                                                let _ = vm_clone.set_game_downloading(&game_id, false).await;
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                            
                                            if ui.small_button("删除").clicked() {
                                                home.delete_confirm = Some((game.game.id.clone(), save.file_id.clone()));
                                            }
                                        });
                                    });
                                    ui.separator();
                                }
                            });
                    });
                }
            });
        });
    });
}