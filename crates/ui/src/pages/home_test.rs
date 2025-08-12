#[cfg(test)]
mod tests {
    use super::super::home::*;
    use crate::{AppViewModel, AppSettings};
    use steam_cloud_sync_cloud::BackendType;
    
    #[test]
    fn test_home_page_creation() {
        let home = HomePage::new();
        
        assert!(home.pending_expanded);
        assert!(!home.synced_expanded);
        assert!(!home.unknown_expanded);
        assert!(home.selected_games.is_empty());
        assert!(home.cloud_saves_expanded.is_empty());
        assert_eq!(home.cloud_storage_used, 0);
        assert_eq!(home.cloud_storage_total, 100 * 1024 * 1024 * 1024);
    }
    
    #[test]
    fn test_home_page_with_context() {
        let view_model = AppViewModel::new();
        let settings = AppSettings {
            selected_backend: BackendType::TencentCOS,
            tencent_secret_id: "test_id".to_string(),
            tencent_secret_key: "test_key".to_string(),
            tencent_bucket: "test_bucket".to_string(),
            tencent_region: "ap-beijing".to_string(),
            s3_access_key: String::new(),
            s3_secret_key: String::new(),
            s3_bucket: String::new(),
            s3_region: String::new(),
            language_index: 0,
            user_id: "test_user".to_string(),
            auto_start: false,
            rate_limit_enabled: false,
            rate_limit_value: 10.0,
            auto_save_on_change: false,
        };
        
        let home = HomePage::with_context(view_model, settings);
        
        assert!(home.view_model.is_some());
        assert!(home.settings.is_some());
    }
    
    #[test]
    fn test_download_progress_update() {
        let mut home = HomePage::new();
        
        home.update_download_progress("game_123", 500, 1000);
        
        assert!(home.in_flight.contains_key("game_123"));
        let progress = home.in_flight.get("game_123").unwrap();
        assert_eq!(progress.bytes_downloaded, 500);
        assert_eq!(progress.total_bytes, 1000);
        
        home.finish_download("game_123");
        assert!(!home.in_flight.contains_key("game_123"));
    }
    
    #[tokio::test]
    async fn test_cloud_saves_loading() {
        let mut home = HomePage::new();
        let view_model = AppViewModel::new();
        let settings = AppSettings::default();
        
        home.view_model = Some(view_model);
        home.settings = Some(settings);
        
        // This will spawn an async task, but we can at least verify it doesn't panic
        home.load_cloud_saves("test_game".to_string());
        
        // Check loading state was set
        assert!(home.loading_saves.contains_key("test_game"));
        assert_eq!(*home.loading_saves.get("test_game").unwrap(), true);
    }
}