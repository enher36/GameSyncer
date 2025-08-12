use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Result;

pub mod steam_scan;
pub mod save_detection;
pub mod manual_mapping;

// Re-export from steam_scan module
pub use steam_scan::{GameSave, InstalledGame, scan_steam_games, detect_game_saves};
pub use save_detection::locate_save;
pub use manual_mapping::register_manual_mapping;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub install_path: PathBuf,
    pub save_locations: Vec<PathBuf>,
}

impl From<InstalledGame> for Game {
    fn from(installed: InstalledGame) -> Self {
        Self {
            id: installed.app_id.to_string(),
            name: installed.name,
            install_path: installed.install_path,
            save_locations: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("Registry access error: {0}")]
    Registry(String),
    #[error("Path not found: {0}")]
    PathNotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// Public API as required - scan_installed_games returns Vec<Game>
pub fn scan_installed_games() -> Result<Vec<Game>, ScanError> {
    let installed_games = steam_scan::scan_steam_games()
        .map_err(|e| ScanError::PathNotFound(e.to_string()))?;
    
    Ok(installed_games.into_iter().map(Game::from).collect())
}

// Legacy API for backward compatibility
pub fn scan_steam_games_legacy() -> Result<Vec<InstalledGame>, ScanError> {
    steam_scan::scan_steam_games()
        .map_err(|e| ScanError::PathNotFound(e.to_string()))
}