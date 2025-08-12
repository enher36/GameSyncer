use std::collections::HashMap;

/// 游戏名称到app_id的映射
/// 这个映射用于处理历史存档文件名不一致的问题
pub fn get_game_name_to_appid_map() -> HashMap<String, String> {
    let mut map = HashMap::new();
    
    // 常见游戏的映射
    map.insert("Wallpaper Engine".to_string(), "431960".to_string());
    map.insert("wallpaper_engine".to_string(), "431960".to_string());
    map.insert("WallpaperEngine".to_string(), "431960".to_string());
    
    // 添加用户的其他游戏
    map.insert("Terraria".to_string(), "105600".to_string());
    map.insert("Baldur's Gate 3".to_string(), "1086940".to_string());
    map.insert("BaldursGate3".to_string(), "1086940".to_string());
    map.insert("Senren＊Banka".to_string(), "1144400".to_string());
    map.insert("SenrenBanka".to_string(), "1144400".to_string());
    map.insert("ASTLIBRA ～生きた証～ Revision".to_string(), "1718570".to_string());
    map.insert("ASTLIBRA".to_string(), "1718570".to_string());
    map.insert("Lost Castle 2".to_string(), "2445690".to_string());
    map.insert("LostCastle2".to_string(), "2445690".to_string());
    map.insert("The Binding of Isaac: Rebirth".to_string(), "250900".to_string());
    map.insert("Isaac".to_string(), "250900".to_string());
    map.insert("Hacknet".to_string(), "365450".to_string());
    map.insert("Cultist Simulator".to_string(), "718670".to_string());
    map.insert("CultistSimulator".to_string(), "718670".to_string());
    map.insert("Counter-Strike 2".to_string(), "730".to_string());
    map.insert("CS2".to_string(), "730".to_string());
    map.insert("Lossless Scaling".to_string(), "993090".to_string());
    map.insert("LosslessScaling".to_string(), "993090".to_string());
    
    map
}

/// 改进的game_id提取和映射逻辑
pub fn extract_and_map_game_id(file_path: &str) -> String {
    println!("🔍 [MAPPING DEBUG] Processing file: {}", file_path);
    let raw_id = extract_game_id_from_path(file_path);
    println!("🔍 [MAPPING DEBUG] Extracted raw_id: '{}'", raw_id);
    
    // 如果提取的ID已经是数字（app_id），直接返回
    if raw_id.chars().all(|c| c.is_numeric()) {
        println!("🔍 [MAPPING DEBUG] Raw ID is numeric, returning as-is: {}", raw_id);
        return raw_id;
    }
    
    // 特殊处理：对于"save_"开头的文件，无法确定具体游戏
    // 这些文件是由于历史bug产生的，使用了错误的命名格式
    if raw_id == "save" || file_path.contains("/save_") {
        println!("🔍 [MAPPING DEBUG] Detected save_ file, returning 'unknown'");
        // 无法确定具体是哪个游戏，返回unknown
        // 这样这些存档不会被错误地分配给特定游戏
        return "unknown".to_string();
    }
    
    // 尝试从映射表查找
    let map = get_game_name_to_appid_map();
    if let Some(app_id) = map.get(&raw_id) {
        return app_id.clone();
    }
    
    // 尝试忽略大小写和下划线的变体
    let normalized = raw_id.replace(" ", "_").to_lowercase();
    if let Some(app_id) = map.get(&normalized) {
        return app_id.clone();
    }
    
    // 如果都找不到，返回原始值
    raw_id
}

fn extract_game_id_from_path(file_path: &str) -> String {
    let parts: Vec<&str> = file_path.split('/').collect();
    
    if parts.len() >= 3 {
        // Check if it's a directory format: saves/user_id/game_id/filename.zip
        if parts.len() >= 4 {
            return parts[2].to_string();
        }
        
        // Check the filename format
        let filename = parts[2];
        
        // Handle save_timestamp_date_uuid.zip format
        if filename.starts_with("save_") {
            // This is a problematic format, we can't determine the real game
            return "save".to_string();
        }
        
        // Handle game_name_timestamp_uuid.zip format  
        if let Some(first_underscore) = filename.find('_') {
            return filename[..first_underscore].to_string();
        }
        
        // Fallback: use the filename without extension
        if let Some(dot) = filename.rfind('.') {
            return filename[..dot].to_string();
        }
        
        return filename.to_string();
    }
    
    "unknown".to_string()
}

/// 反向映射：从app_id获取可能的游戏名称
/// 用于在搜索时匹配旧的文件名格式
pub fn get_possible_names_for_appid(app_id: &str) -> Vec<String> {
    let mut names = Vec::new();
    
    // Always include the app_id itself
    names.push(app_id.to_string());
    
    // 根据app_id添加可能的游戏名称
    match app_id {
        "431960" => {
            names.push("Wallpaper Engine".to_string());
            names.push("wallpaper_engine".to_string());
            names.push("WallpaperEngine".to_string());
        },
        "105600" => {
            names.push("Terraria".to_string());
        },
        "1086940" => {
            names.push("Baldur's Gate 3".to_string());
            names.push("BaldursGate3".to_string());
        },
        "1144400" => {
            names.push("Senren＊Banka".to_string());
            names.push("SenrenBanka".to_string());
        },
        "1718570" => {
            names.push("ASTLIBRA ～生きた証～ Revision".to_string());
            names.push("ASTLIBRA".to_string());
        },
        "2445690" => {
            names.push("Lost Castle 2".to_string());
            names.push("LostCastle2".to_string());
        },
        "250900" => {
            names.push("The Binding of Isaac: Rebirth".to_string());
            names.push("Isaac".to_string());
        },
        "365450" => {
            names.push("Hacknet".to_string());
        },
        "718670" => {
            names.push("Cultist Simulator".to_string());
            names.push("CultistSimulator".to_string());
        },
        "730" => {
            names.push("Counter-Strike 2".to_string());
            names.push("CS2".to_string());
        },
        "993090" => {
            names.push("Lossless Scaling".to_string());
            names.push("LosslessScaling".to_string());
        },
        _ => {}
    }
    
    names
}