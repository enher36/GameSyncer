use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;
use steamlocate::SteamDir;
use chrono::{DateTime, Utc, Duration};

use crate::steam_scan::GameSave;
use crate::manual_mapping::get_manual_mapping;

// Import Game struct from parent module
use crate::Game;

/// Multi-layer heuristic save location detection
pub fn locate_save(game: &Game) -> Result<Option<GameSave>> {
    // Parse app_id with proper error handling
    let app_id: u32 = game.id.parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse game ID '{}' as u32: {}", game.id, e))?;
    
    eprintln!("Locating save for game: {} (app_id: {})", game.name, app_id);
    
    // Check manual mapping first
    if let Some(mapped_path) = get_manual_mapping(app_id)? {
        if mapped_path.exists() {
            eprintln!("  Found save via manual mapping: {}", mapped_path.display());
            return Ok(Some(GameSave {
                app_id,
                name: game.name.clone(),
                save_path: mapped_path,
            }));
        }
    }
    
    // Layer 1: Steam Cloud remote
    eprintln!("  Checking Steam Cloud remote for app_id {}", app_id);
    if let Some(save_path) = check_steam_cloud_remote(app_id)? {
        eprintln!("  Found save in Steam Cloud: {}", save_path.display());
        return Ok(Some(GameSave {
            app_id,
            name: game.name.clone(),
            save_path,
        }));
    }
    
    // Layer 2: Known Folders
    eprintln!("  Checking known folders for '{}'", game.name);
    if let Some(save_path) = check_known_folders(&game.name, &game.install_path)? {
        eprintln!("  Found save in known folders: {}", save_path.display());
        return Ok(Some(GameSave {
            app_id,
            name: game.name.clone(),
            save_path,
        }));
    }
    
    // Layer 3: Install directory recursive search
    eprintln!("  Checking install directory: {}", game.install_path.display());
    if let Some(save_path) = check_install_directory(&game.install_path)? {
        eprintln!("  Found save in install directory: {}", save_path.display());
        return Ok(Some(GameSave {
            app_id,
            name: game.name.clone(),
            save_path,
        }));
    }
    
    eprintln!("  No save location found for {}", game.name);
    Ok(None)
}

/// Layer 1: Check Steam Cloud remote directory
fn check_steam_cloud_remote(app_id: u32) -> Result<Option<PathBuf>> {
    let steamdir = SteamDir::locate()?;
    let userdata_path = steamdir.path().join("userdata");
    
    if !userdata_path.exists() {
        return Ok(None);
    }
    
    // Check all user directories
    for entry in std::fs::read_dir(&userdata_path)? {
        let user_dir = entry?.path();
        if user_dir.is_dir() {
            let remote_path = user_dir.join(app_id.to_string()).join("remote");
            if remote_path.exists() && is_non_empty_directory(&remote_path)? {
                return Ok(Some(remote_path));
            }
        }
    }
    
    Ok(None)
}

/// Layer 2: Check known folders with fuzzy matching
fn check_known_folders(game_name: &str, install_path: &Path) -> Result<Option<PathBuf>> {
    let search_names = generate_search_names(game_name, install_path);
    let known_folders = get_known_folders()?;
    
    for base_path in known_folders {
        if !base_path.exists() {
            continue;
        }
        
        for search_name in &search_names {
            if let Some(save_path) = find_fuzzy_match(&base_path, search_name)? {
                if is_valid_save_directory(&save_path)? {
                    return Ok(Some(save_path));
                }
            }
        }
    }
    
    Ok(None)
}

/// Layer 3: Recursive search in install directory
fn check_install_directory(install_path: &Path) -> Result<Option<PathBuf>> {
    if !install_path.exists() {
        return Ok(None);
    }
    
    let save_keywords = ["save", "saved", "profile", "userdata", "savegame", "saves"];
    
    for entry in WalkDir::new(install_path).max_depth(3) {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_dir() {
            let dir_name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
                
            for keyword in &save_keywords {
                if dir_name.contains(keyword) {
                    if is_valid_save_directory(path)? {
                        return Ok(Some(path.to_path_buf()));
                    }
                }
            }
        }
    }
    
    Ok(None)
}

/// Generate possible search names for the game
fn generate_search_names(game_name: &str, install_path: &Path) -> Vec<String> {
    let mut names = Vec::new();
    
    // Original game name
    names.push(game_name.to_string());
    
    // Sanitized game name
    names.push(sanitize_name(game_name));
    
    // Install directory name
    if let Some(dir_name) = install_path.file_name().and_then(|n| n.to_str()) {
        names.push(dir_name.to_string());
        names.push(sanitize_name(dir_name));
    }
    
    // Remove duplicates
    names.sort();
    names.dedup();
    names
}

/// Sanitize name for directory matching
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            ':' | '/' | '\\' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            '™' | '®' | '©' => ' ',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Get known folders for different platforms
fn get_known_folders() -> Result<Vec<PathBuf>> {
    let mut folders = Vec::new();
    
    #[cfg(target_os = "windows")]
    {
        // Windows Known Folders
        if let Some(profile) = dirs::home_dir() {
            folders.push(profile.join("Saved Games"));
        }
        if let Some(documents) = dirs::document_dir() {
            folders.push(documents.join("My Games"));
            folders.push(documents);
        }
        if let Some(appdata) = dirs::data_dir() {
            folders.push(appdata);
        }
        if let Some(local_appdata) = dirs::data_local_dir() {
            folders.push(local_appdata);
        }
    }
    
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // Linux/macOS directories
        if let Some(home) = dirs::home_dir() {
            folders.push(home.join(".local/share"));
            folders.push(home.join(".config"));
            folders.push(home.join("Documents"));
        }
    }
    
    Ok(folders)
}

/// Find fuzzy match in directory
fn find_fuzzy_match(base_path: &Path, search_name: &str) -> Result<Option<PathBuf>> {
    if !base_path.exists() {
        return Ok(None);
    }
    
    let search_lower = search_name.to_lowercase();
    
    for entry in std::fs::read_dir(base_path)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_dir() {
            if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                let dir_lower = dir_name.to_lowercase();
                
                // Exact match
                if dir_lower == search_lower {
                    return Ok(Some(path));
                }
                
                // Contains match
                if dir_lower.contains(&search_lower) || search_lower.contains(&dir_lower) {
                    return Ok(Some(path));
                }
            }
        }
    }
    
    Ok(None)
}

/// Check if directory is a valid save directory
fn is_valid_save_directory(path: &Path) -> Result<bool> {
    if !path.exists() || !path.is_dir() {
        return Ok(false);
    }
    
    // Check modification time (< 30 days)
    let metadata = std::fs::metadata(path)?;
    if let Ok(modified) = metadata.modified() {
        let modified_timestamp = modified.duration_since(UNIX_EPOCH)?.as_secs();
        let modified_date = DateTime::<Utc>::from_timestamp(modified_timestamp as i64, 0);
        
        if let Some(modified_date) = modified_date {
            let thirty_days_ago = Utc::now() - Duration::days(30);
            if modified_date < thirty_days_ago {
                return Ok(false);
            }
        }
    }
    
    // Check file count (≥ 3 files)
    let mut file_count = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.path().is_file() {
            file_count += 1;
            if file_count >= 3 {
                return Ok(true);
            }
        }
    }
    
    Ok(file_count >= 3)
}

/// Check if directory is non-empty
fn is_non_empty_directory(path: &Path) -> Result<bool> {
    if !path.exists() || !path.is_dir() {
        return Ok(false);
    }
    
    for _entry in std::fs::read_dir(path)? {
        return Ok(true);
    }
    
    Ok(false)
}