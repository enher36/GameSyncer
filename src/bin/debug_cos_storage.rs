// 调试工具：检查腾讯云COS实际存储内容
// cargo run --bin debug_cos_storage

use anyhow::Result;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔍 [DEBUG] COS存储内容调试工具");
    
    // 从环境变量或配置读取COS信息
    let bucket = std::env::var("TENCENT_BUCKET").or_else(|_| std::env::var("COS_BUCKET"))
        .unwrap_or_else(|_| {
            println!("请设置TENCENT_BUCKET环境变量");
            std::process::exit(1);
        });
    
    let region = std::env::var("TENCENT_REGION").or_else(|_| std::env::var("COS_REGION"))
        .unwrap_or_else(|_| "ap-beijing".to_string());
    
    let secret_id = std::env::var("TENCENT_SECRET_ID").or_else(|_| std::env::var("COS_SECRET_ID"))
        .unwrap_or_else(|_| {
            println!("请设置TENCENT_SECRET_ID环境变量");
            std::process::exit(1);
        });
    
    let secret_key = std::env::var("TENCENT_SECRET_KEY").or_else(|_| std::env::var("COS_SECRET_KEY"))
        .unwrap_or_else(|_| {
            println!("请设置TENCENT_SECRET_KEY环境变量");
            std::process::exit(1);
        });
    
    println!("🔧 [DEBUG] COS配置:");
    println!("   Bucket: {}", bucket);
    println!("   Region: {}", region);
    println!("   Secret ID: {}***", &secret_id[..std::cmp::min(4, secret_id.len())]);
    
    // 创建HTTP客户端
    let client = reqwest::Client::new();
    
    // 生成请求URL - 列出所有对象
    let url = format!("https://{}.cos.{}.myqcloud.com/?prefix=saves/", bucket, region);
    
    println!("📡 [DEBUG] 请求URL: {}", url);
    
    // 简化的授权头生成（这里应该使用完整的COS签名算法）
    // 为了调试，我们先尝试一个基本请求
    
    // 使用临时的简单方法测试连接
    let test_response = client
        .get(&url)
        .header("Host", format!("{}.cos.{}.myqcloud.com", bucket, region))
        .send()
        .await;
    
    match test_response {
        Ok(response) => {
            let status = response.status();
            println!("📊 [DEBUG] 响应状态: {}", status);
            
            if status.is_success() {
                let body = response.text().await?;
                println!("✅ [DEBUG] 响应内容 (前1000字符):");
                println!("{}", &body[..std::cmp::min(1000, body.len())]);
                
                // 保存完整响应到文件
                let mut file = std::fs::File::create("cos_list_response.xml")?;
                file.write_all(body.as_bytes())?;
                println!("💾 [DEBUG] 完整响应已保存到 cos_list_response.xml");
                
                // 分析XML内容
                analyze_xml_content(&body);
                
            } else {
                let error_body = response.text().await.unwrap_or_default();
                println!("❌ [DEBUG] 请求失败: {} - {}", status, error_body);
                println!("💡 [DEBUG] 这可能是因为：");
                println!("   1. 需要正确的COS签名认证");
                println!("   2. Bucket名称或区域不正确");
                println!("   3. Secret ID/Key不正确");
                println!("   4. 网络连接问题");
            }
        }
        Err(e) => {
            println!("❌ [DEBUG] 网络请求失败: {}", e);
        }
    }
    
    Ok(())
}

fn analyze_xml_content(xml: &str) {
    println!("\n🔍 [DEBUG] XML内容分析:");
    
    // 统计对象数量
    let key_count = xml.matches("<Key>").count();
    println!("   对象数量: {}", key_count);
    
    // 提取所有Key
    println!("   所有文件路径:");
    let mut current_pos = 0;
    while let Some(start) = xml[current_pos..].find("<Key>") {
        let abs_start = current_pos + start + 5;
        if let Some(end) = xml[abs_start..].find("</Key>") {
            let key = &xml[abs_start..abs_start + end];
            println!("     - {}", key);
            current_pos = abs_start + end;
        } else {
            break;
        }
    }
    
    // 检查是否有saves目录的文件
    let saves_files: Vec<_> = xml.lines()
        .filter(|line| line.contains("<Key>") && line.contains("saves/"))
        .collect();
    
    println!("   saves目录下的文件: {} 个", saves_files.len());
    
    // 检查文件大小信息
    let size_count = xml.matches("<Size>").count();
    println!("   包含大小信息的对象: {}", size_count);
    
    // 检查时间戳信息
    let modified_count = xml.matches("<LastModified>").count();
    println!("   包含修改时间的对象: {}", modified_count);
}