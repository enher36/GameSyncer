use eframe::egui;
use steam_cloud_sync_ui::SteamCloudSyncApp;

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    // Initialize logging
    env_logger::init();
    
    // Set up native options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "SteamCloudSync",
        native_options,
        Box::new(|cc| {
            // Set up fonts for Chinese support
            let mut fonts = egui::FontDefinitions::default();
            
            // Try to load system fonts for Chinese characters
            if let Some(font_data) = load_system_font() {
                fonts.font_data.insert(
                    "system_chinese".to_owned(),
                    font_data,
                );
                fonts.families.entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "system_chinese".to_owned());
                    
                fonts.families.entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("system_chinese".to_owned());
                    
                cc.egui_ctx.set_fonts(fonts);
            }
            
            Box::new(SteamCloudSyncApp::default())
        }),
    )
}

fn load_system_font() -> Option<egui::FontData> {
    // Try to load system Chinese fonts on Windows
    #[cfg(target_os = "windows")]
    {
        use std::path::Path;
        let font_paths = [
            "C:/Windows/Fonts/msyh.ttc",      // Microsoft YaHei
            "C:/Windows/Fonts/simsun.ttc",   // SimSun
            "C:/Windows/Fonts/simhei.ttf",   // SimHei
        ];
        
        for path in &font_paths {
            if Path::new(path).exists() {
                if let Ok(data) = std::fs::read(path) {
                    return Some(egui::FontData::from_owned(data));
                }
            }
        }
    }
    None
}