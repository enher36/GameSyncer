use crate::models::*;
use anyhow::Result;
use sqlx::{sqlite::SqlitePool, Pool, Sqlite};
use std::path::PathBuf;

/// Database wrapper
#[derive(Debug, Clone)]
pub struct Database {
    pub pool: Pool<Sqlite>,
}

impl Database {
    /// Create a new database instance
    pub async fn new(db_path: PathBuf) -> Result<Self> {
        println!("🗃️ [DEBUG] Database::new() called with path: {}", db_path.display());
        
        let database_url = format!("sqlite:{}", db_path.display());
        println!("🔗 [DEBUG] Database URL: {}", database_url);
        
        // Try to create the parent directory if it doesn't exist
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                println!("📂 [DEBUG] Creating parent directory: {}", parent.display());
                std::fs::create_dir_all(parent)?;
            }
        }
        
        // Try to create an empty file if it doesn't exist (test write permissions)
        if !db_path.exists() {
            println!("📄 [DEBUG] Database file doesn't exist, testing write permissions...");
            match std::fs::File::create(&db_path) {
                Ok(_) => {
                    println!("✅ [DEBUG] Write permission test successful");
                    // Remove the test file
                    std::fs::remove_file(&db_path)?;
                }
                Err(e) => {
                    println!("❌ [DEBUG] Write permission test failed: {}", e);
                    return Err(anyhow::anyhow!("Cannot create database file: {}", e));
                }
            }
        }
        
        println!("🔌 [DEBUG] Attempting SQLite connection...");
        let pool = match SqlitePool::connect(&database_url).await {
            Ok(pool) => {
                println!("✅ [DEBUG] SQLite connection successful");
                pool
            }
            Err(e) => {
                println!("❌ [DEBUG] SQLite connection failed: {}", e);
                
                // Provide more specific error information
                let error_string = e.to_string();
                if error_string.contains("unable to open database file") {
                    println!("🔍 [DEBUG] SQLite cannot open the database file");
                    println!("💡 [DEBUG] Possible causes:");
                    println!("   - File is locked by another process");
                    println!("   - Insufficient permissions");
                    println!("   - Disk full or filesystem error");
                    println!("   - Path contains invalid characters");
                }
                
                return Err(anyhow::anyhow!("Failed to connect to database: {}", e));
            }
        };
        
        let db = Self { pool };
        
        println!("🏗️ [DEBUG] Running database migrations...");
        match db.run_migrations().await {
            Ok(_) => {
                println!("✅ [DEBUG] Database migrations completed successfully");
            }
            Err(e) => {
                println!("❌ [DEBUG] Database migrations failed: {}", e);
                return Err(e);
            }
        }
        
        println!("🎉 [DEBUG] Database initialization completed successfully");
        Ok(db)
    }
    
    /// Run database migrations
    async fn run_migrations(&self) -> Result<()> {
        // Create cloud_operations table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cloud_operations (
                id TEXT PRIMARY KEY NOT NULL,
                game_id TEXT NOT NULL,
                operation_type TEXT NOT NULL CHECK (operation_type IN ('upload', 'download', 'delete', 'list', 'restore')),
                status TEXT NOT NULL CHECK (status IN ('pending', 'inprogress', 'completed', 'failed', 'cancelled')),
                file_path TEXT,
                file_size INTEGER,
                checksum TEXT,
                error_message TEXT,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                progress REAL,
                metadata TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create sync_sessions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sync_sessions (
                id TEXT PRIMARY KEY NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                games_synced INTEGER NOT NULL DEFAULT 0,
                operations_count INTEGER NOT NULL DEFAULT 0,
                total_bytes INTEGER,
                success BOOLEAN,
                error_message TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create game_configs table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS game_configs (
                game_id TEXT PRIMARY KEY NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT true,
                auto_sync BOOLEAN NOT NULL DEFAULT false,
                last_sync_at TEXT,
                local_path TEXT,
                cloud_path TEXT,
                exclusion_patterns TEXT,
                compression_enabled BOOLEAN NOT NULL DEFAULT true,
                max_versions INTEGER DEFAULT 5,
                sync_direction TEXT DEFAULT 'bidirectional',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create app_configs table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS app_configs (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL,
                config_type TEXT NOT NULL DEFAULT 'string',
                description TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create cloud_stats table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cloud_stats (
                id TEXT PRIMARY KEY NOT NULL,
                recorded_at TEXT NOT NULL,
                total_files INTEGER NOT NULL DEFAULT 0,
                total_size_bytes INTEGER NOT NULL DEFAULT 0,
                games_count INTEGER NOT NULL DEFAULT 0,
                backend_type TEXT NOT NULL,
                metadata TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_cloud_operations_game_id ON cloud_operations(game_id)")
            .execute(&self.pool)
            .await?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_cloud_operations_status ON cloud_operations(status)")
            .execute(&self.pool)
            .await?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_cloud_operations_started_at ON cloud_operations(started_at)")
            .execute(&self.pool)
            .await?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sync_sessions_started_at ON sync_sessions(started_at)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_cloud_stats_recorded_at ON cloud_stats(recorded_at)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }
    
    /// Get database pool reference
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
    
    /// Close database connection
    pub async fn close(self) {
        self.pool.close().await;
    }
    
    /// Execute a health check on the database
    pub async fn health_check(&self) -> Result<bool> {
        let row: (i64,) = sqlx::query_as("SELECT 1")
            .fetch_one(&self.pool)
            .await?;
        
        Ok(row.0 == 1)
    }
    
    /// Get database size in bytes
    pub async fn get_database_size(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT page_count * page_size as size FROM pragma_page_count(), pragma_page_size()")
            .fetch_one(&self.pool)
            .await?;
        
        Ok(row.0)
    }
    
    /// Vacuum the database to reclaim space
    pub async fn vacuum(&self) -> Result<()> {
        sqlx::query("VACUUM").execute(&self.pool).await?;
        Ok(())
    }
    
    /// Get database statistics
    pub async fn get_stats(&self) -> Result<DatabaseStats> {
        let operations_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cloud_operations")
            .fetch_one(&self.pool)
            .await?;
        
        let sessions_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sync_sessions")
            .fetch_one(&self.pool)
            .await?;
        
        let game_configs_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM game_configs")
            .fetch_one(&self.pool)
            .await?;
        
        let app_configs_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM app_configs")
            .fetch_one(&self.pool)
            .await?;
        
        Ok(DatabaseStats {
            operations_count: operations_count.0,
            sessions_count: sessions_count.0,
            game_configs_count: game_configs_count.0,
            app_configs_count: app_configs_count.0,
            database_size: self.get_database_size().await.unwrap_or(0),
        })
    }
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub operations_count: i64,
    pub sessions_count: i64,
    pub game_configs_count: i64,
    pub app_configs_count: i64,
    pub database_size: i64,
}