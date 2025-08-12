use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MappingData {
    mappings: HashMap<u32, PathBuf>,
}

impl Default for MappingData {
    fn default() -> Self {
        Self {
            mappings: HashMap::new(),
        }
    }
}

/// Register a manual mapping for a game's save location
pub fn register_manual_mapping(app_id: u32, path: PathBuf) -> Result<()> {
    let config_path = get_mappings_file_path()?;
    
    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    // Load existing mappings
    let mut data = load_mappings()?;
    
    // Add new mapping
    data.mappings.insert(app_id, path);
    
    // Save mappings
    save_mappings(&data)?;
    
    Ok(())
}

/// Get manual mapping for a specific app_id
pub fn get_manual_mapping(app_id: u32) -> Result<Option<PathBuf>> {
    let data = load_mappings()?;
    Ok(data.mappings.get(&app_id).cloned())
}

/// Remove manual mapping for a specific app_id
pub fn remove_manual_mapping(app_id: u32) -> Result<bool> {
    let mut data = load_mappings()?;
    let removed = data.mappings.remove(&app_id).is_some();
    
    if removed {
        save_mappings(&data)?;
    }
    
    Ok(removed)
}

/// Get all manual mappings
pub fn get_all_mappings() -> Result<HashMap<u32, PathBuf>> {
    let data = load_mappings()?;
    Ok(data.mappings)
}

/// Load mappings from JSON file
fn load_mappings() -> Result<MappingData> {
    let config_path = get_mappings_file_path()?;
    
    if !config_path.exists() {
        return Ok(MappingData::default());
    }
    
    let content = fs::read_to_string(&config_path)?;
    let data: MappingData = serde_json::from_str(&content)
        .unwrap_or_else(|_| MappingData::default());
    
    Ok(data)
}

/// Save mappings to JSON file
fn save_mappings(data: &MappingData) -> Result<()> {
    let config_path = get_mappings_file_path()?;
    let content = serde_json::to_string_pretty(data)?;
    fs::write(&config_path, content)?;
    Ok(())
}

/// Get the path to the mappings.json file
fn get_mappings_file_path() -> Result<PathBuf> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    
    Ok(home_dir.join(".steam-cloud-sync").join("mappings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    
    #[test]
    fn test_mapping_operations() -> Result<()> {
        // Use a temporary directory for testing
        let temp_dir = env::temp_dir().join("steam-cloud-sync-test");
        fs::create_dir_all(&temp_dir)?;
        
        let test_path = PathBuf::from("/test/save/path");
        let app_id = 12345;
        
        // Test registration
        register_manual_mapping(app_id, test_path.clone())?;
        
        // Test retrieval
        let retrieved = get_manual_mapping(app_id)?;
        assert_eq!(retrieved, Some(test_path));
        
        // Test removal
        let removed = remove_manual_mapping(app_id)?;
        assert!(removed);
        
        // Test that it's gone
        let retrieved_after_removal = get_manual_mapping(app_id)?;
        assert_eq!(retrieved_after_removal, None);
        
        Ok(())
    }
}