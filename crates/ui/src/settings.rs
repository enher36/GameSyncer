use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Result;
use steam_cloud_sync_cloud::BackendType;
use chrono;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub selected_backend: BackendType,
    pub language_index: usize,
    pub auto_save_on_change: bool,
    
    // User identification
    pub user_id: String,
    
    // Tencent COS settings
    pub tencent_secret_id: String,
    pub tencent_secret_key: String,
    pub tencent_bucket: String,
    pub tencent_region: String,
    
    // S3 settings
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_bucket: String,
    pub s3_region: String,
    
    // Application settings
    pub auto_start: bool,
    pub rate_limit_enabled: bool,
    pub rate_limit_value: f32,
    
    // Download settings
    pub default_download_path: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        // Generate a default user ID based on current timestamp
        let user_id = format!("user_{}", chrono::Utc::now().timestamp());
        
        Self {
            selected_backend: BackendType::TencentCOS,
            language_index: 0,
            auto_save_on_change: true, // Enable auto-save by default for better UX
            user_id,
            tencent_secret_id: String::new(),
            tencent_secret_key: String::new(),
            tencent_bucket: "steam-cloud-sync".to_string(),
            tencent_region: "ap-beijing".to_string(),
            s3_access_key: String::new(),
            s3_secret_key: String::new(),
            s3_bucket: "steam-cloud-sync".to_string(),
            s3_region: "us-east-1".to_string(),
            auto_start: false,
            rate_limit_enabled: false,
            rate_limit_value: 10.0,
            default_download_path: None,
        }
    }
}

impl AppSettings {
    pub fn get_config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
            .join("steam-cloud-sync");
        
        std::fs::create_dir_all(&config_dir)?;
        Ok(config_dir.join("settings.json"))
    }
    
    pub fn load() -> Result<AppSettings> {
        let config_path = Self::get_config_path()?;
        
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let settings: AppSettings = serde_json::from_str(&content)?;
            Ok(settings)
        } else {
            Ok(AppSettings::default())
        }
    }
    
    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path()?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }
}