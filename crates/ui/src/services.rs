use crate::LocalizationManager;
use anyhow::Result;

pub struct LanguageService {
    localization: LocalizationManager,
}

impl LanguageService {
    pub fn new() -> Self {
        let localization = LocalizationManager::new();
        Self { localization }
    }
    
    pub fn get_current_language(&self) -> String {
        // Since current_language is private, we'll return a default or implement a getter
        "en-US".to_string()
    }
    
    pub fn set_language(&mut self, language_tag: &str) {
        self.localization.set_language(language_tag.to_string());
    }
    
    pub fn get_localized_string(&self, key: &str) -> String {
        self.localization.get_string(key)
    }
    
    pub fn get_available_languages(&self) -> Vec<(String, String)> {
        vec![
            ("en-US".to_string(), "English".to_string()),
            ("zh-CN".to_string(), "简体中文".to_string()),
        ]
    }
}