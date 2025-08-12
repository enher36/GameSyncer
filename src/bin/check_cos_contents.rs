# 检查COS bucket完整内容的脚本
# 运行: cargo run --bin check_cos_contents

use anyhow::Result;
use reqwest;

#[tokio::main] 
async fn main() -> Result<()> {
    println!("🔍 [DEBUG] 检查COS bucket完整内容...");
    
    // 从环境变量读取配置
    let bucket = "game-s-1316634448";
    let region = "ap-guangzhou";
    
    // 构造请求URL - 不使用prefix，列出所有文件
    let url = format!("https://{}.cos.{}.myqcloud.com/", bucket, region);
    
    println!("📡 [DEBUG] 请求URL: {}", url);
    
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Host", format!("{}.cos.{}.myqcloud.com", bucket, region))
        .send()
        .await?;
    
    let status = response.status();
    println!("📊 [DEBUG] 响应状态: {}", status);
    
    if status.is_success() {
        let body = response.text().await?;
        println!("📄 [DEBUG] 响应长度: {} 字符", body.len());
        println!("📄 [DEBUG] 完整响应内容:");
        println!("{}", body);
        
        // 简单分析
        let object_count = body.matches("<Key>").count();
        println!("\n📈 [DEBUG] 总结:");
        println!("   - 找到 {} 个对象", object_count);
        
        if object_count == 0 {
            println!("   ❌ Bucket为空，没有任何文件");
            println!("   💡 建议：");
            println!("      1. 检查是否使用了正确的bucket");
            println!("      2. 确认之前是否真的上传过存档");
            println!("      3. 检查用户ID是否发生过变化");
        } else {
            println!("   ✅ Bucket中有文件，分析路径格式...");
            
            // 提取所有Key
            let mut start = 0;
            while let Some(key_start) = body[start..].find("<Key>") {
                let abs_start = start + key_start + 5;
                if let Some(key_end) = body[abs_start..].find("</Key>") {
                    let key = &body[abs_start..abs_start + key_end];
                    println!("      - {}", key);
                    start = abs_start + key_end;
                } else {
                    break;
                }
            }
        }
    } else {
        let error_body = response.text().await.unwrap_or_default();
        println!("❌ [DEBUG] 请求失败: {} - {}", status, error_body);
        println!("💡 这可能需要COS认证信息");
    }
    
    Ok(())
}