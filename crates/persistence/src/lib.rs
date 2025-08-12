pub mod database;
pub mod models;
pub mod cloud_history;
pub mod config_store;

pub use database::*;
pub use models::*;
pub use cloud_history::*;
pub use config_store::*;

use anyhow::Result;
use std::path::PathBuf;

/// Initialize the persistence layer
pub async fn initialize_persistence() -> Result<PersistenceManager> {
    println!("🗂️ [DEBUG] Initializing persistence layer...");
    
    let data_dir = get_data_directory()?;
    println!("📁 [DEBUG] Data directory: {}", data_dir.display());
    
    // Check if directory exists
    if !data_dir.exists() {
        println!("📂 [DEBUG] Data directory doesn't exist, creating...");
        std::fs::create_dir_all(&data_dir)?;
        println!("✅ [DEBUG] Data directory created successfully");
    } else {
        println!("✅ [DEBUG] Data directory already exists");
    }
    
    // Check directory permissions
    let dir_metadata = std::fs::metadata(&data_dir)?;
    println!("🔐 [DEBUG] Directory permissions - readonly: {}", dir_metadata.permissions().readonly());
    
    let db_path = data_dir.join("steam-cloud-sync.db");
    println!("💾 [DEBUG] Database path: {}", db_path.display());
    
    // Check if database file exists
    if db_path.exists() {
        println!("📄 [DEBUG] Database file exists");
        let file_metadata = std::fs::metadata(&db_path)?;
        println!("📊 [DEBUG] Database file size: {} bytes", file_metadata.len());
        println!("🔐 [DEBUG] Database file readonly: {}", file_metadata.permissions().readonly());
    } else {
        println!("📄 [DEBUG] Database file doesn't exist, will be created");
    }
    
    // Check available disk space
    match std::fs::metadata(&data_dir) {
        Ok(_) => {
            println!("💽 [DEBUG] Data directory is accessible");
        }
        Err(e) => {
            println!("❌ [DEBUG] Cannot access data directory: {}", e);
            return Err(anyhow::anyhow!("Cannot access data directory: {}", e));
        }
    }
    
    println!("🔗 [DEBUG] Attempting database connection...");
    match Database::new(db_path).await {
        Ok(db) => {
            println!("✅ [DEBUG] Database connection successful");
            Ok(PersistenceManager::new(db))
        }
        Err(e) => {
            println!("❌ [DEBUG] Database connection failed: {}", e);
            println!("🔍 [DEBUG] Error type: {}", e);
            
            // Try to provide more specific error information
            if e.to_string().contains("unable to open database file") {
                println!("💡 [DEBUG] This is typically caused by:");
                println!("   1. Insufficient permissions to write to the directory");
                println!("   2. Disk space issues");
                println!("   3. Antivirus software blocking file creation");
                println!("   4. Path too long or contains invalid characters");
            }
            
            Err(e)
        }
    }
}

/// Get the application data directory
pub fn get_data_directory() -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?
        .join("steam-cloud-sync");
    Ok(data_dir)
}

/// Main persistence manager
pub struct PersistenceManager {
    pub database: Database,
    pub cloud_history: CloudHistoryStore,
    pub config_store: ConfigStore,
}

impl PersistenceManager {
    pub fn new(database: Database) -> Self {
        let cloud_history = CloudHistoryStore::new(database.clone());
        let config_store = ConfigStore::new(database.clone());
        
        Self {
            database,
            cloud_history,
            config_store,
        }
    }
}