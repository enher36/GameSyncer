use crate::{models::*, Database};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;

/// Configuration store
#[derive(Debug, Clone)]
pub struct ConfigStore {
    db: Database,
}

impl ConfigStore {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    
    /// Set application configuration
    pub async fn set_app_config(&self, key: &str, value: &str, config_type: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO app_configs (key, value, config_type, created_at, updated_at)
            VALUES (?1, ?2, ?3, COALESCE((SELECT created_at FROM app_configs WHERE key = ?1), ?4), ?4)
            "#,
        )
        .bind(key)
        .bind(value)
        .bind(config_type)
        .bind(&now)
        .execute(&self.db.pool)
        .await?;
        
        Ok(())
    }
    
    /// Get application configuration
    pub async fn get_app_config(&self, key: &str) -> Result<Option<AppConfig>> {
        let config = sqlx::query_as::<_, AppConfig>(
            "SELECT * FROM app_configs WHERE key = ?1"
        )
        .bind(key)
        .fetch_optional(&self.db.pool)
        .await?;
        
        Ok(config)
    }
    
    /// Get all application configurations
    pub async fn get_all_app_configs(&self) -> Result<Vec<AppConfig>> {
        let configs = sqlx::query_as::<_, AppConfig>(
            "SELECT * FROM app_configs ORDER BY key"
        )
        .fetch_all(&self.db.pool)
        .await?;
        
        Ok(configs)
    }
    
    /// Delete application configuration
    pub async fn delete_app_config(&self, key: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM app_configs WHERE key = ?1")
            .bind(key)
            .execute(&self.db.pool)
            .await?;
        
        Ok(result.rows_affected() > 0)
    }
    
    /// Set string configuration
    pub async fn set_string_config(&self, key: &str, value: &str) -> Result<()> {
        self.set_app_config(key, value, "string").await
    }
    
    /// Get string configuration
    pub async fn get_string_config(&self, key: &str) -> Result<Option<String>> {
        match self.get_app_config(key).await? {
            Some(config) => Ok(Some(config.value)),
            None => Ok(None),
        }
    }
    
    /// Set boolean configuration
    pub async fn set_bool_config(&self, key: &str, value: bool) -> Result<()> {
        self.set_app_config(key, &value.to_string(), "boolean").await
    }
    
    /// Get boolean configuration
    pub async fn get_bool_config(&self, key: &str) -> Result<Option<bool>> {
        match self.get_app_config(key).await? {
            Some(config) => Ok(Some(config.value.parse::<bool>()?)),
            None => Ok(None),
        }
    }
    
    /// Set number configuration
    pub async fn set_number_config(&self, key: &str, value: i64) -> Result<()> {
        self.set_app_config(key, &value.to_string(), "number").await
    }
    
    /// Get number configuration
    pub async fn get_number_config(&self, key: &str) -> Result<Option<i64>> {
        match self.get_app_config(key).await? {
            Some(config) => Ok(Some(config.value.parse::<i64>()?)),
            None => Ok(None),
        }
    }
    
    /// Set JSON configuration
    pub async fn set_json_config(&self, key: &str, value: &Value) -> Result<()> {
        let json_str = serde_json::to_string(value)?;
        self.set_app_config(key, &json_str, "json").await
    }
    
    /// Get JSON configuration
    pub async fn get_json_config(&self, key: &str) -> Result<Option<Value>> {
        match self.get_app_config(key).await? {
            Some(config) => {
                let value: Value = serde_json::from_str(&config.value)?;
                Ok(Some(value))
            },
            None => Ok(None),
        }
    }
    
    /// Create or update game configuration
    pub async fn set_game_config(&self, config: GameConfig) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO game_configs (
                game_id, enabled, auto_sync, last_sync_at, local_path, cloud_path,
                exclusion_patterns, compression_enabled, max_versions, sync_direction,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     COALESCE((SELECT created_at FROM game_configs WHERE game_id = ?1), ?11), ?11)
            "#,
        )
        .bind(&config.game_id)
        .bind(config.enabled)
        .bind(config.auto_sync)
        .bind(config.last_sync_at.map(|dt| dt.to_rfc3339()))
        .bind(&config.local_path)
        .bind(&config.cloud_path)
        .bind(&config.exclusion_patterns)
        .bind(config.compression_enabled)
        .bind(config.max_versions)
        .bind(&config.sync_direction)
        .bind(&now)
        .execute(&self.db.pool)
        .await?;
        
        Ok(())
    }
    
    /// Get game configuration
    pub async fn get_game_config(&self, game_id: &str) -> Result<Option<GameConfig>> {
        let config = sqlx::query_as::<_, GameConfig>(
            "SELECT * FROM game_configs WHERE game_id = ?1"
        )
        .bind(game_id)
        .fetch_optional(&self.db.pool)
        .await?;
        
        Ok(config)
    }
    
    /// Get all game configurations
    pub async fn get_all_game_configs(&self) -> Result<Vec<GameConfig>> {
        let configs = sqlx::query_as::<_, GameConfig>(
            "SELECT * FROM game_configs ORDER BY game_id"
        )
        .fetch_all(&self.db.pool)
        .await?;
        
        Ok(configs)
    }
    
    /// Get enabled game configurations
    pub async fn get_enabled_game_configs(&self) -> Result<Vec<GameConfig>> {
        let configs = sqlx::query_as::<_, GameConfig>(
            "SELECT * FROM game_configs WHERE enabled = true ORDER BY game_id"
        )
        .fetch_all(&self.db.pool)
        .await?;
        
        Ok(configs)
    }
    
    /// Update game last sync time
    pub async fn update_game_last_sync(&self, game_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE game_configs SET last_sync_at = ?1, updated_at = ?1 WHERE game_id = ?2"
        )
        .bind(Utc::now().to_rfc3339())
        .bind(game_id)
        .execute(&self.db.pool)
        .await?;
        
        Ok(())
    }
    
    /// Enable/disable game sync
    pub async fn set_game_enabled(&self, game_id: &str, enabled: bool) -> Result<()> {
        sqlx::query(
            "UPDATE game_configs SET enabled = ?1, updated_at = ?2 WHERE game_id = ?3"
        )
        .bind(enabled)
        .bind(Utc::now().to_rfc3339())
        .bind(game_id)
        .execute(&self.db.pool)
        .await?;
        
        Ok(())
    }
    
    /// Delete game configuration
    pub async fn delete_game_config(&self, game_id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM game_configs WHERE game_id = ?1")
            .bind(game_id)
            .execute(&self.db.pool)
            .await?;
        
        Ok(result.rows_affected() > 0)
    }
    
    /// Initialize default application configurations
    pub async fn init_default_configs(&self) -> Result<()> {
        // Cloud backend settings
        self.set_string_config("cloud.backend_type", "tencent_cos").await?;
        self.set_string_config("cloud.tencent_secret_id", "").await?;
        self.set_string_config("cloud.tencent_secret_key", "").await?;
        self.set_string_config("cloud.tencent_bucket", "steam-cloud-sync").await?;
        self.set_string_config("cloud.tencent_region", "ap-beijing").await?;
        self.set_string_config("cloud.s3_access_key", "").await?;
        self.set_string_config("cloud.s3_secret_key", "").await?;
        self.set_string_config("cloud.s3_bucket", "steam-cloud-sync").await?;
        self.set_string_config("cloud.s3_region", "us-east-1").await?;
        
        // Application settings
        self.set_bool_config("app.auto_start", false).await?;
        self.set_bool_config("app.minimize_to_tray", true).await?;
        self.set_bool_config("app.auto_sync", false).await?;
        self.set_number_config("app.sync_interval_minutes", 60).await?;
        self.set_string_config("app.language", "en").await?;
        self.set_string_config("app.user_id", &uuid::Uuid::new_v4().to_string()).await?;
        
        // Sync settings
        self.set_bool_config("sync.compression_enabled", true).await?;
        self.set_number_config("sync.max_versions_per_game", 5).await?;
        self.set_bool_config("sync.rate_limiting_enabled", false).await?;
        self.set_number_config("sync.rate_limit_mbps", 10).await?;
        self.set_number_config("sync.parallel_operations", 3).await?;
        
        // Storage settings
        self.set_number_config("storage.max_total_size_gb", 100).await?;
        self.set_number_config("storage.cleanup_older_than_days", 90).await?;
        
        Ok(())
    }
    
    /// Export configurations as JSON
    pub async fn export_configs(&self) -> Result<Value> {
        let app_configs = self.get_all_app_configs().await?;
        let game_configs = self.get_all_game_configs().await?;
        
        let mut export = serde_json::Map::new();
        export.insert("app_configs".to_string(), serde_json::to_value(app_configs)?);
        export.insert("game_configs".to_string(), serde_json::to_value(game_configs)?);
        export.insert("export_timestamp".to_string(), serde_json::to_value(Utc::now())?);
        
        Ok(Value::Object(export))
    }
    
    /// Import configurations from JSON
    pub async fn import_configs(&self, data: &Value) -> Result<()> {
        if let Some(app_configs) = data.get("app_configs") {
            if let Ok(configs) = serde_json::from_value::<Vec<AppConfig>>(app_configs.clone()) {
                for config in configs {
                    self.set_app_config(&config.key, &config.value, &config.config_type).await?;
                }
            }
        }
        
        if let Some(game_configs) = data.get("game_configs") {
            if let Ok(configs) = serde_json::from_value::<Vec<GameConfig>>(game_configs.clone()) {
                for config in configs {
                    self.set_game_config(config).await?;
                }
            }
        }
        
        Ok(())
    }
}