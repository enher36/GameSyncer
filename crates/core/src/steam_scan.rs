use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use steamlocate::SteamDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSave {
    pub app_id: u32,
    pub name: String,
    pub save_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledGame {
    pub app_id: u32,
    pub name: String,
    pub install_path: PathBuf,
}

pub fn scan_steam_games() -> Result<Vec<InstalledGame>> {
    let steamdir = SteamDir::locate()?;
    let mut games = Vec::new();
    
    // First try using steamlocate's built-in app detection
    for lib in steamdir.libraries()? {
        let library = lib?;  // Handle the Result
        for app in library.apps() {
            if let Ok(app_info) = app {
                // Check if the app has a valid install directory
                if !app_info.install_dir.is_empty() {
                    games.push(InstalledGame {
                        app_id: app_info.app_id,
                        name: app_info.name.clone().unwrap_or("Unknown Game".to_string()),
                        install_path: library.path().join("steamapps").join("common").join(&app_info.install_dir),
                    });
                }
            }
        }
    }
    
    // If steamlocate didn't find games, fall back to manual parsing
    if games.is_empty() {
        games = fallback_scan_steam_games(&steamdir)?;
    }
    
    Ok(games)
}

// Fallback method: manual parsing of libraryfolders.vdf → appmanifest_*.acf → installdir
fn fallback_scan_steam_games(steamdir: &SteamDir) -> Result<Vec<InstalledGame>> {
    let mut games = Vec::new();
    
    // Parse libraryfolders.vdf to get all library paths
    let config_path = steamdir.path().join("config").join("libraryfolders.vdf");
    if !config_path.exists() {
        return Ok(games);
    }
    
    let library_folders = parse_library_folders(&config_path)?;
    
    for library_path in library_folders {
        let steamapps_path = library_path.join("steamapps");
        if !steamapps_path.exists() {
            continue;
        }
        
        // Scan for appmanifest_*.acf files
        if let Ok(entries) = std::fs::read_dir(&steamapps_path) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                
                if file_name_str.starts_with("appmanifest_") && file_name_str.ends_with(".acf") {
                    if let Ok(game) = parse_app_manifest(&entry.path(), &steamapps_path) {
                        games.push(game);
                    }
                }
            }
        }
    }
    
    Ok(games)
}

pub fn detect_game_saves(games: &[InstalledGame]) -> Result<Vec<GameSave>> {
    let mut game_saves = Vec::new();
    
    for game in games {
        if let Some(save_path) = detect_save_location(game)? {
            game_saves.push(GameSave {
                app_id: game.app_id,
                name: game.name.clone(),
                save_path,
            });
        }
    }
    
    Ok(game_saves)
}

fn detect_save_location(game: &InstalledGame) -> Result<Option<PathBuf>> {
    let save_locations = get_common_save_locations()?;
    
    // Heuristic patterns for save detection
    let patterns = vec![
        game.name.clone(),
        game.app_id.to_string(),
        format!("{}_{}", game.name, game.app_id),
        sanitize_game_name(&game.name),
    ];
    
    for base_path in &save_locations {
        if !base_path.exists() {
            continue;
        }
        
        for pattern in &patterns {
            // Direct match
            let direct_path = base_path.join(pattern);
            if direct_path.exists() && direct_path.is_dir() {
                return Ok(Some(direct_path));
            }
            
            // Case-insensitive search
            if let Ok(entries) = std::fs::read_dir(base_path) {
                for entry in entries.flatten() {
                    if let Some(dir_name) = entry.file_name().to_str() {
                        if dir_name.to_lowercase().contains(&pattern.to_lowercase()) {
                            let path = entry.path();
                            if path.is_dir() {
                                return Ok(Some(path));
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Check Steam userdata for cloud saves
    if let Some(steam_save) = check_steam_userdata(&game)? {
        return Ok(Some(steam_save));
    }
    
    // TODO: Hook for manual mapping - return None for now
    Ok(None)
}

fn get_common_save_locations() -> Result<Vec<PathBuf>> {
    let mut locations = Vec::new();
    
    // %UserProfile%\Saved Games
    if let Some(profile_dir) = dirs::home_dir() {
        locations.push(profile_dir.join("Saved Games"));
    }
    
    // %AppData%\Roaming
    if let Some(roaming_dir) = dirs::config_dir() {
        locations.push(roaming_dir);
    }
    
    // %LocalAppData%
    if let Some(local_dir) = dirs::data_local_dir() {
        locations.push(local_dir);
    }
    
    // Documents\My Games
    if let Some(docs_dir) = dirs::document_dir() {
        locations.push(docs_dir.join("My Games"));
    }
    
    Ok(locations)
}

fn check_steam_userdata(game: &InstalledGame) -> Result<Option<PathBuf>> {
    let steamdir = SteamDir::locate()?;
    
    let userdata_path = steamdir.path().join("userdata");
    if !userdata_path.exists() {
        return Ok(None);
    }
    
    // Look for app-specific folders in userdata
    if let Ok(entries) = std::fs::read_dir(&userdata_path) {
        for user_entry in entries.flatten() {
            let user_path = user_entry.path();
            if user_path.is_dir() {
                let app_save_path = user_path.join(game.app_id.to_string()).join("remote");
                if app_save_path.exists() && app_save_path.is_dir() {
                    return Ok(Some(app_save_path));
                }
            }
        }
    }
    
    Ok(None)
}

fn sanitize_game_name(name: &str) -> String {
    // Remove common suffixes and special characters
    let cleaned = name
        .replace("™", "")
        .replace("®", "")
        .replace("©", "")
        .replace(":", "")
        .replace("/", "")
        .replace("\\", "")
        .replace("*", "")
        .replace("?", "")
        .replace("\"", "")
        .replace("<", "")
        .replace(">", "")
        .replace("|", "")
        .trim()
        .to_string();
    
    // Handle common patterns
    if let Some(pos) = cleaned.find(" - ") {
        cleaned[..pos].to_string()
    } else {
        cleaned
    }
}

fn parse_library_folders(config_path: &PathBuf) -> Result<Vec<PathBuf>> {
    let content = std::fs::read_to_string(config_path)?;
    let mut library_paths = Vec::new();
    
    // Simple VDF parsing - look for "path" entries
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("\"path\"") {
            // Find the value part after "path"
            if let Some(colon_pos) = line.find('\t') {
                let value_part = &line[colon_pos..].trim();
                if let Some(start) = value_part.find('"') {
                    if let Some(end) = value_part[start + 1..].find('"') {
                        let path = &value_part[start + 1..start + 1 + end];
                        library_paths.push(PathBuf::from(path));
                    }
                }
            }
        }
    }
    
    Ok(library_paths)
}

fn parse_app_manifest(manifest_path: &PathBuf, steamapps_path: &PathBuf) -> Result<InstalledGame> {
    let content = std::fs::read_to_string(manifest_path)?;
    
    let mut app_id = 0;
    let mut name = String::new();
    let mut installdir = String::new();
    
    // Simple ACF parsing
    for line in content.lines() {
        let line = line.trim();
        
        if line.starts_with("\"appid\"") {
            // Parse: "appid"		"12345"
            if let Some(tab_pos) = line.find('\t') {
                let value_part = line[tab_pos..].trim();
                if let Some(start) = value_part.find('"') {
                    if let Some(end) = value_part[start + 1..].find('"') {
                        if let Ok(id) = value_part[start + 1..start + 1 + end].parse::<u32>() {
                            app_id = id;
                        }
                    }
                }
            }
        } else if line.starts_with("\"name\"") {
            if let Some(tab_pos) = line.find('\t') {
                let value_part = line[tab_pos..].trim();
                if let Some(start) = value_part.find('"') {
                    if let Some(end) = value_part[start + 1..].find('"') {
                        name = value_part[start + 1..start + 1 + end].to_string();
                    }
                }
            }
        } else if line.starts_with("\"installdir\"") {
            if let Some(tab_pos) = line.find('\t') {
                let value_part = line[tab_pos..].trim();
                if let Some(start) = value_part.find('"') {
                    if let Some(end) = value_part[start + 1..].find('"') {
                        installdir = value_part[start + 1..start + 1 + end].to_string();
                    }
                }
            }
        }
    }
    
    if app_id == 0 || name.is_empty() || installdir.is_empty() {
        return Err(anyhow::anyhow!("Invalid app manifest"));
    }
    
    let install_path = steamapps_path.join("common").join(&installdir);
    
    Ok(InstalledGame {
        app_id,
        name,
        install_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_game_name() {
        assert_eq!(sanitize_game_name("Game: Title™"), "Game Title");
        assert_eq!(sanitize_game_name("Game - Subtitle"), "Game");
        assert_eq!(sanitize_game_name("Normal Game"), "Normal Game");
    }
}