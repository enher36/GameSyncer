#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppViewModel, GameWithSave, Game, SyncState, AppSettings};
    use steam_cloud_sync_cloud::{BackendType, SaveMetadata};
    
    #[test]
    fn test_cloud_saves_page_creation() {
        let page = CloudSavesPage::new();
        
        assert!(page.game_entries.is_empty());
        assert!(page.download_progress.is_empty());
        assert!(page.delete_confirm.is_none());
        assert!(page.version_detail.is_none());
        assert_eq!(page.total_cloud_size, 0);
        assert_eq!(page.total_versions, 0);
        assert!(page.show_only_with_saves);
        assert!(page.sort_by_latest);
    }
    
    #[test]
    fn test_cloud_saves_page_with_context() {
        let view_model = AppViewModel::new();
        let settings = AppSettings::default();
        
        let page = CloudSavesPage::with_context(view_model, settings);
        
        assert!(page.view_model.is_some());
        assert!(page.settings.is_some());
    }
    
    #[test]
    fn test_refresh_cloud_saves() {
        let mut page = CloudSavesPage::new();
        
        // Create test game with cloud saves
        let game = GameWithSave {
            game: Game {
                id: "test_game_1".to_string(),
                name: "Test Game 1".to_string(),
                platform: "Steam".to_string(),
            },
            save_info: None,
            save_detection_status: crate::SaveDetectionStatus::NotScanned,
            sync_enabled: true,
            cloud_saves: vec![
                SaveMetadata {
                    game_id: "test_game_1".to_string(),
                    timestamp: "2024-01-15T10:00:00Z".to_string(),
                    size_bytes: 1024 * 1024, // 1MB
                    checksum: "abc123".to_string(),
                    compressed: true,
                    file_id: "save_v1".to_string(),
                },
                SaveMetadata {
                    game_id: "test_game_1".to_string(),
                    timestamp: "2024-01-14T10:00:00Z".to_string(),
                    size_bytes: 512 * 1024, // 512KB
                    checksum: "def456".to_string(),
                    compressed: true,
                    file_id: "save_v2".to_string(),
                },
            ],
            downloading: false,
            sync_state: SyncState::Synced,
            sync_progress: None,
        };
        
        let games = vec![game];
        page.refresh_cloud_saves(&games);
        
        // Verify game entry was created
        assert_eq!(page.game_entries.len(), 1);
        assert!(page.game_entries.contains_key("test_game_1"));
        
        let entry = page.game_entries.get("test_game_1").unwrap();
        assert_eq!(entry.versions.len(), 2);
        assert!(entry.versions[0].is_latest);
        assert!(!entry.versions[1].is_latest);
        
        // Verify statistics were calculated
        assert_eq!(page.total_versions, 2);
        assert_eq!(page.total_cloud_size, 1024 * 1024 + 512 * 1024);
    }
    
    #[test]
    fn test_cloud_save_version_creation() {
        let metadata = SaveMetadata {
            game_id: "test_game".to_string(),
            timestamp: "2024-01-15T10:00:00Z".to_string(),
            size_bytes: 1024 * 1024,
            checksum: "abc123".to_string(),
            compressed: true,
            file_id: "test_save".to_string(),
        };
        
        let version = CloudSaveVersion {
            metadata: metadata.clone(),
            is_latest: true,
            is_current_local: false,
        };
        
        assert_eq!(version.metadata.game_id, "test_game");
        assert!(version.is_latest);
        assert!(!version.is_current_local);
    }
    
    #[tokio::test]
    async fn test_load_game_saves() {
        let mut page = CloudSavesPage::new();
        let view_model = AppViewModel::new();
        let settings = AppSettings::default();
        
        page.view_model = Some(view_model);
        page.settings = Some(settings);
        
        // Create initial game entry
        let game = GameWithSave {
            game: Game {
                id: "test_game".to_string(),
                name: "Test Game".to_string(),
                platform: "Steam".to_string(),
            },
            save_info: None,
            save_detection_status: crate::SaveDetectionStatus::NotScanned,
            sync_enabled: true,
            cloud_saves: vec![],
            downloading: false,
            sync_state: SyncState::Unknown,
            sync_progress: None,
        };
        
        page.game_entries.insert("test_game".to_string(), CloudSaveGameEntry {
            game,
            versions: Vec::new(),
            expanded: false,
            loading: false,
        });
        
        // This will spawn an async task
        page.load_game_saves("test_game".to_string());
        
        // Verify loading state was set
        assert!(page.game_entries.get("test_game").unwrap().loading);
    }
}