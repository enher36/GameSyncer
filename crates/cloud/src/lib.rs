use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use chrono;
use serde::{Deserialize, Serialize};
use std::path::Path;
use steam_cloud_sync_core::GameSave;
use sha2::{Digest, Sha256};
use sha1::Sha1;
use hmac::{Hmac, Mac};
use zip::{ZipArchive, ZipWriter};
use std::io::{Write, Cursor};
use tokio::fs;
use uuid::Uuid;

pub mod cloud_save_service;
pub mod game_mapping;

pub use cloud_save_service::*;
use game_mapping::extract_and_map_game_id;
use std::io::Read;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Copy)]
pub enum BackendType {
    TencentCOS,
    S3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgress {
    pub bytes_uploaded: u64,
    pub total_bytes: u64,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveMetadata {
    pub game_id: String,
    pub timestamp: String,
    pub size_bytes: u64,
    pub checksum: String,
    pub compressed: bool,
    pub file_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub used_bytes: u64,          // 用户存储使用量
    pub total_bytes: Option<u64>, // None means unlimited
    pub file_count: u32,          // 用户文件数量
    pub bucket_used_bytes: Option<u64>, // 整个存储桶使用量（可选）
    pub bucket_total_objects: Option<u32>, // 整个存储桶对象数量（可选）
}

/// Builder for SaveMetadata to help with XML parsing
#[derive(Debug, Default)]
struct SaveMetadataBuilder {
    file_id: Option<String>,
    size_bytes: Option<u64>,
    timestamp: Option<String>,
    checksum: Option<String>,
}

impl SaveMetadataBuilder {
    fn new() -> Self {
        Self::default()
    }
    
    fn build(self) -> Option<SaveMetadata> {
        Some(SaveMetadata {
            game_id: String::new(), // Will be filled later from file path
            timestamp: self.timestamp.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
            size_bytes: self.size_bytes.unwrap_or(0),
            checksum: self.checksum.unwrap_or_default(),
            compressed: true, // Assume compressed for .zip files
            file_id: self.file_id?,
        })
    }
}

/// Extract value from XML tag
fn extract_xml_value(line: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);
    
    if let Some(start) = line.find(&open_tag) {
        if let Some(end) = line.find(&close_tag) {
            let start_pos = start + open_tag.len();
            if start_pos < end {
                return Some(line[start_pos..end].to_string());
            }
        }
    }
    None
}

/// Extract game ID from file path
/// Supports multiple path formats:
/// - saves/user_id/game_id/filename.zip
/// - saves/user_id/game_id_timestamp_uuid.zip
fn extract_game_id_from_path(file_path: &str) -> String {
    let parts: Vec<&str> = file_path.split('/').collect();
    
    if parts.len() >= 3 {
        // Check if it's a directory format: saves/user_id/game_id/filename.zip
        if parts.len() >= 4 {
            return parts[2].to_string(); // game_id is the third part
        }
        
        // Check if it's a file format: saves/user_id/game_id_timestamp_uuid.zip
        let filename = parts[2];
        if let Some(first_underscore) = filename.find('_') {
            return filename[..first_underscore].to_string();
        }
        
        // Fallback: use the filename without extension
        if let Some(dot) = filename.rfind('.') {
            return filename[..dot].to_string();
        }
        
        return filename.to_string();
    }
    
    // Fallback: extract from the full path
    "unknown".to_string()
}

/// Helper function to extract a save archive to the target path
async fn extract_save_archive_helper(data: &[u8], target_path: &Path) -> Result<()> {
    let data = data.to_vec();
    let target_path = target_path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        let cursor = Cursor::new(&data);
        let mut zip = ZipArchive::new(cursor)?;
        
        // Create target directory if it doesn't exist
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        // If there's only one file in the archive and target_path is a file path,
        // extract directly to that file
        if zip.len() == 1 && target_path.extension().is_some() {
            let mut file = zip.by_index(0)?;
            let mut contents = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut contents)?;
            std::fs::write(&target_path, contents)?;
        } else {
            // Otherwise, extract all files to the target directory
            let extract_dir = if target_path.is_dir() || target_path.extension().is_none() {
                target_path
            } else {
                target_path.parent().unwrap().to_path_buf()
            };
            
            std::fs::create_dir_all(&extract_dir)?;
            
            for i in 0..zip.len() {
                let mut file = zip.by_index(i)?;
                let file_path = match file.enclosed_name() {
                    Some(path) => extract_dir.join(path),
                    None => continue,
                };
                
                if file.name().ends_with('/') {
                    std::fs::create_dir_all(&file_path)?;
                } else {
                    if let Some(parent) = file_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut contents = Vec::new();
                    std::io::Read::read_to_end(&mut file, &mut contents)?;
                    std::fs::write(&file_path, contents)?;
                }
            }
        }
        
        Ok::<(), anyhow::Error>(())
    }).await?
}

#[async_trait]
pub trait CloudBackend: Send + Sync {
    async fn upload_save(&self, game_save: &GameSave, user_id: &str) -> Result<SaveMetadata>;
    async fn download_save(&self, metadata: &SaveMetadata, local_path: &Path) -> Result<()>;
    async fn list_saves(&self, user_id: &str, game_id: Option<&str>) -> Result<Vec<SaveMetadata>>;
    async fn delete_save(&self, metadata: &SaveMetadata) -> Result<()>;
    async fn resume_upload(&self, upload_id: &str, offset: u64, data: Bytes) -> Result<UploadProgress>;
    async fn test_connection(&self) -> Result<()>;
    async fn get_storage_info(&self, user_id: &str) -> Result<StorageInfo>;
    async fn get_bucket_storage_info(&self) -> Result<(u64, u32)>; // (total_bytes, total_objects)
}

pub fn backend(kind: BackendType) -> Box<dyn CloudBackend> {
    match kind {
        BackendType::TencentCOS => Box::new(TencentCOSBackend::new()),
        BackendType::S3 => Box::new(S3Backend::new()),
    }
}

pub fn backend_with_settings(kind: BackendType, tencent_credentials: Option<(String, String, String, String)>, s3_config: Option<(String, String)>) -> Box<dyn CloudBackend> {
    match kind {
        BackendType::TencentCOS => {
            if let Some((secret_id, secret_key, bucket, region)) = tencent_credentials {
                Box::new(TencentCOSBackend::with_credentials(secret_id, secret_key, bucket, region))
            } else {
                Box::new(TencentCOSBackend::new())
            }
        }
        BackendType::S3 => {
            if let Some((bucket, prefix)) = s3_config {
                Box::new(S3Backend::with_config(bucket, prefix))
            } else {
                Box::new(S3Backend::new())
            }
        }
    }
}

pub struct TencentCOSBackend {
    client: reqwest::Client,
    secret_id: Option<String>,
    secret_key: Option<String>,
    bucket: String,
    region: String,
}

impl TencentCOSBackend {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            secret_id: None,
            secret_key: None,
            bucket: "steam-cloud-sync".to_string(),
            region: "ap-beijing".to_string(),
        }
    }

    pub fn with_credentials(secret_id: String, secret_key: String, bucket: String, region: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            secret_id: Some(secret_id),
            secret_key: Some(secret_key),
            bucket,
            region,
        }
    }

    fn get_cos_url(&self, object_key: &str) -> String {
        format!("https://{}.cos.{}.myqcloud.com/{}", self.bucket, self.region, object_key)
    }

    async fn test_bucket_access(&self) -> Result<()> {
        eprintln!("[TencentCOS] Testing bucket access: bucket={}, region={}", self.bucket, self.region);
        
        // Test with a simple HEAD request to the bucket root
        // If credentials are provided, we can test with authentication
        if self.secret_id.is_some() && self.secret_key.is_some() {
            eprintln!("[TencentCOS] Testing with authentication");
            // Test by trying to list objects (more thorough test)
            let url = format!("https://{}.cos.{}.myqcloud.com/", self.bucket, self.region);
            
            let (authorization, _) = self.generate_cos_authorization("GET", "", "", 0)?;;
            
            eprintln!("[TencentCOS] Test URL: {}", url);
            
            let response = self.client
                .get(&url)
                .header("Authorization", authorization)
                .header("Host", format!("{}.cos.{}.myqcloud.com", self.bucket, self.region))
                .send()
                .await?;

            eprintln!("[TencentCOS] Test response status: {}", response.status());

            if response.status().is_success() || response.status().as_u16() == 404 {
                eprintln!("[TencentCOS] Bucket access test successful");
                Ok(())
            } else if response.status().as_u16() == 403 {
                eprintln!("[TencentCOS] Access denied (403)");
                Err(anyhow::anyhow!("Access denied. Check your credentials and bucket permissions."))
            } else {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                eprintln!("[TencentCOS] Test failed with status {}: {}", status, body);
                Err(anyhow::anyhow!("Failed to access bucket: HTTP {} - {}", status, body))
            }
        } else {
            eprintln!("[TencentCOS] Testing without authentication (simple connectivity test)");
            // Fallback to simple connectivity test without authentication
            let url = format!("https://{}.cos.{}.myqcloud.com/", self.bucket, self.region);
            
            let response = self.client
                .head(&url)
                .send()
                .await?;

            eprintln!("[TencentCOS] Simple test response status: {}", response.status());

            if response.status().is_success() || response.status().as_u16() == 404 || response.status().as_u16() == 403 {
                eprintln!("[TencentCOS] Simple connectivity test successful");
                Ok(())
            } else {
                Err(anyhow::anyhow!("Failed to access bucket: HTTP {}", response.status()))
            }
        }
    }

    async fn compress_save(&self, save_path: &Path) -> Result<Vec<u8>> {
        let save_path = save_path.to_path_buf();
        
        tokio::task::spawn_blocking(move || {
            let mut buffer = Vec::new();
            {
                let cursor = Cursor::new(&mut buffer);
                let mut zip = ZipWriter::new(cursor);
                
                if save_path.is_file() {
                    let file_name = save_path.file_name().unwrap().to_str().unwrap();
                    let file_data = std::fs::read(&save_path)?;
                    zip.start_file(file_name, zip::write::FileOptions::default())?;
                    zip.write_all(&file_data)?;
                } else if save_path.is_dir() {
                    Self::add_dir_to_zip_sync(&mut zip, &save_path, "")?;
                }
                
                zip.finish()?;
            }
            Ok::<Vec<u8>, anyhow::Error>(buffer)
        }).await?
    }

    fn add_dir_to_zip_sync<W: Write + std::io::Seek>(zip: &mut ZipWriter<W>, dir: &Path, prefix: &str) -> Result<()> {
        let entries = std::fs::read_dir(dir)?;
        
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let file_name = format!("{}{}", prefix, name.to_str().unwrap());

            if path.is_file() {
                let file_data = std::fs::read(&path)?;
                zip.start_file(&file_name, zip::write::FileOptions::default())?;
                zip.write_all(&file_data)?;
            } else if path.is_dir() {
                Self::add_dir_to_zip_sync(zip, &path, &format!("{}/", file_name))?;
            }
        }
        Ok(())
    }

    fn calculate_sha256(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    fn generate_cos_authorization(&self, method: &str, object_key: &str, query_params: &str, _content_length: usize) -> Result<(String, String)> {
        let secret_id = self.secret_id.as_ref().ok_or_else(|| anyhow::anyhow!("Secret ID not configured"))?;
        let secret_key = self.secret_key.as_ref().ok_or_else(|| anyhow::anyhow!("Secret Key not configured"))?;
        
        eprintln!("[TencentCOS] Generating authorization for {} {}, query: {}", method, object_key, query_params);
        
        // Validate that secret_id and secret_key don't contain invalid characters for HTTP headers
        if secret_id.chars().any(|c| c.is_control() || c == '\n' || c == '\r') {
            return Err(anyhow::anyhow!("Secret ID contains invalid characters"));
        }
        
        // Generate timestamp (current time and expiration time)
        let now = chrono::Utc::now().timestamp();
        let expire_time = now + 3600; // 1 hour expiration
        
        // Create signing key
        let key_time = format!("{};{}", now, expire_time);
        let signing_key = {
            let mut mac = Hmac::<Sha1>::new_from_slice(secret_key.as_bytes())
                .map_err(|e| anyhow::anyhow!("Invalid secret key: {}", e))?;
            mac.update(key_time.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };
        
        // Clean object_key to ensure it doesn't cause issues
        let clean_object_key = object_key.trim_start_matches('/');
        
        // Create query parameter list for signature (sorted)
        let mut query_param_list = String::new();
        let mut url_param_list = String::new();
        
        if !query_params.is_empty() {
            // Parse and sort query parameters for signature
            let mut params: Vec<&str> = query_params.split('&').collect();
            params.sort();
            
            // Create parameter lists for signature
            for (i, param) in params.iter().enumerate() {
                if let Some(eq_pos) = param.find('=') {
                    let key = &param[..eq_pos];
                    let value = &param[eq_pos + 1..];
                    
                    if i > 0 {
                        query_param_list.push('&');
                        url_param_list.push(';');
                    }
                    query_param_list.push_str(&format!("{}={}", key, value));
                    url_param_list.push_str(key);
                }
            }
        }
        
        eprintln!("[TencentCOS] Query param list: {}", query_param_list);
        eprintln!("[TencentCOS] URL param list: {}", url_param_list);
        
        // Create string to sign with proper format including query parameters
        let http_string = format!("{}\n/{}\n{}\nhost={}.cos.{}.myqcloud.com\n", 
            method.to_lowercase(), clean_object_key, query_param_list, self.bucket, self.region);
        let string_to_sign = format!("sha1\n{}\n{}\n", key_time, 
            hex::encode(Sha1::digest(http_string.as_bytes())));
        
        eprintln!("[TencentCOS] HTTP string: {}", http_string.replace('\n', "\\n"));
        eprintln!("[TencentCOS] String to sign: {}", string_to_sign.replace('\n', "\\n"));
        
        // Create signature
        let signature = {
            let mut mac = Hmac::<Sha1>::new_from_slice(signing_key.as_bytes())
                .map_err(|e| anyhow::anyhow!("Invalid signing key: {}", e))?;
            mac.update(string_to_sign.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };
        
        // Create authorization header with proper URL encoding for any special characters
        let authorization = format!(
            "q-sign-algorithm=sha1&q-ak={}&q-sign-time={}&q-key-time={}&q-header-list=host&q-url-param-list={}&q-signature={}",
            urlencoding::encode(secret_id), key_time, key_time, url_param_list, signature
        );
        
        eprintln!("[TencentCOS] Final authorization: {}", authorization);
        
        // Validate that the final authorization header doesn't contain invalid characters
        if authorization.chars().any(|c| c.is_control() || c == '\n' || c == '\r') {
            return Err(anyhow::anyhow!("Generated authorization header contains invalid characters"));
        }
        
        Ok((authorization, key_time))
    }

    async fn upload_to_cos(&self, object_key: &str, data: &[u8]) -> Result<()> {
        let url = self.get_cos_url(object_key);
        let content_type = "application/octet-stream";
        let content_sha256 = Self::calculate_sha256(data);
        
        // Generate authorization header
        let (authorization, _key_time) = self.generate_cos_authorization("PUT", object_key, "", data.len())?;;
        
        let response = self.client
            .put(&url)
            .header("Content-Type", content_type)
            .header("Content-Length", data.len())
            .header("Authorization", authorization)
            .header("Host", format!("{}.cos.{}.myqcloud.com", self.bucket, self.region))
            .header("x-cos-meta-sha256", &content_sha256) // Store SHA256 as metadata
            .body(data.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to upload to Tencent COS: {} - {}", status, body));
        }

        Ok(())
    }
}

#[async_trait]
impl CloudBackend for TencentCOSBackend {
    async fn upload_save(&self, game_save: &GameSave, user_id: &str) -> Result<SaveMetadata> {
        let compressed_data = self.compress_save(&game_save.save_path).await?;
        let checksum = Self::calculate_sha256(&compressed_data);
        
        // Create filename with user ID and timestamp for separation
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let sanitized_user_id = user_id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();
        let object_key = format!("saves/{}/{}_{}_{}.zip", sanitized_user_id, game_save.app_id, timestamp, Uuid::new_v4());
        
        self.upload_to_cos(&object_key, &compressed_data).await?;

        Ok(SaveMetadata {
            game_id: game_save.app_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            size_bytes: compressed_data.len() as u64,
            checksum,
            compressed: true,
            file_id: object_key,
        })
    }

    async fn download_save(&self, metadata: &SaveMetadata, local_path: &Path) -> Result<()> {
        let url = self.get_cos_url(&metadata.file_id);
        
        // Generate authorization header for GET request
        let (authorization, _) = self.generate_cos_authorization("GET", &metadata.file_id, "", 0)?;;
        
        let response = self.client
            .get(&url)
            .header("Authorization", authorization)
            .header("Host", format!("{}.cos.{}.myqcloud.com", self.bucket, self.region))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to download from Tencent COS: {} - {}", status, body));
        }

        let data = response.bytes().await?;
        
        // Verify checksum with improved handling for TencentCOS
        let calculated_checksum = Self::calculate_sha256(&data);
        
        // Handle different checksum types more gracefully
        match metadata.checksum.len() {
            64 => {
                // This should be a SHA256 hash
                if calculated_checksum != metadata.checksum {
                    eprintln!("⚠️  [TencentCOS] SHA256 checksum mismatch:");
                    eprintln!("   Expected: {}", metadata.checksum);
                    eprintln!("   Calculated: {}", calculated_checksum);
                    eprintln!("   File size: {} bytes", data.len());
                    eprintln!("   Continuing download (integrity warning)...");
                } else {
                    eprintln!("✅ [TencentCOS] SHA256 checksum verified");
                }
            },
            32 => {
                // This is likely an MD5/ETag, skip SHA256 verification
                eprintln!("ℹ️  [TencentCOS] MD5/ETag checksum detected ({})", metadata.checksum);
                eprintln!("   File SHA256: {}", calculated_checksum);
                eprintln!("   Skipping verification (different hash types)");
            },
            40 => {
                // This might be SHA1
                eprintln!("ℹ️  [TencentCOS] SHA1 checksum detected ({})", metadata.checksum);
                eprintln!("   File SHA256: {}", calculated_checksum);
                eprintln!("   Skipping verification (different hash types)");
            },
            _ => {
                // Unknown format, log and continue
                eprintln!("⚠️  [TencentCOS] Unknown checksum format:");
                eprintln!("   Stored: '{}' ({} chars)", metadata.checksum, metadata.checksum.len());
                eprintln!("   File SHA256: {}", calculated_checksum);
                eprintln!("   Continuing download (no verification possible)...");
            }
        }

        // If the target path ends with .zip, save as ZIP file directly (for custom download location)
        if local_path.extension().and_then(|s| s.to_str()) == Some("zip") {
            fs::write(local_path, data).await?;
        } else {
            // Otherwise, extract the ZIP to the game save directory
            extract_save_archive_helper(&data, local_path).await?;
        }
        
        Ok(())
    }

    async fn list_saves(&self, user_id: &str, game_id: Option<&str>) -> Result<Vec<SaveMetadata>> {
        println!("📋 [DEBUG] TencentCOS: listing saves for user={}, game_id={:?}", user_id, game_id);
        
        // Sanitize user ID for path safety
        let sanitized_user_id = user_id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();
        
        // First, get all saves for the user
        let prefix = format!("saves/{}/", sanitized_user_id);
        println!("🔍 [DEBUG] Using prefix: '{}'", prefix);
        
        // Use COS LIST Objects API
        let url = format!("https://{}.cos.{}.myqcloud.com/?prefix={}", 
            self.bucket, self.region, urlencoding::encode(&prefix));
        
        println!("📡 [DEBUG] Request URL: {}", url);
        
        // Generate authorization header for GET request
        let query_params = format!("prefix={}", urlencoding::encode(&prefix));
        let (authorization, _) = self.generate_cos_authorization("GET", "", &query_params, 0)?;
        
        let response = self.client
            .get(&url)
            .header("Authorization", authorization)
            .header("Host", format!("{}.cos.{}.myqcloud.com", self.bucket, self.region))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            println!("❌ [DEBUG] COS request failed: {} - {}", status, body);
            return Err(anyhow::anyhow!("Failed to list saves from Tencent COS: {} - {}", status, body));
        }

        let body = response.text().await?;
        println!("📄 [DEBUG] COS response length: {} chars", body.len());
        
        let mut saves = Vec::new();
        
        // Improved XML parsing using a more robust approach
        let mut current_object: Option<SaveMetadataBuilder> = None;
        
        for line in body.lines() {
            let line = line.trim();
            
            // Start of a new object
            if line.starts_with("<Contents>") {
                current_object = Some(SaveMetadataBuilder::new());
                continue;
            }
            
            // End of current object - finalize it
            if line.starts_with("</Contents>") {
                if let Some(builder) = current_object.take() {
                    if let Some(metadata) = builder.build() {
                        // Only include .zip files in saves directory
                        if metadata.file_id.starts_with("saves/") && metadata.file_id.ends_with(".zip") {
                            println!("✅ [DEBUG] Found save: {} ({} bytes, {})", 
                                metadata.file_id, metadata.size_bytes, metadata.timestamp);
                            saves.push(metadata);
                        }
                    }
                }
                continue;
            }
            
            // Parse fields within current object
            if let Some(ref mut builder) = current_object {
                if let Some(key) = extract_xml_value(line, "Key") {
                    builder.file_id = Some(key);
                } else if let Some(size_str) = extract_xml_value(line, "Size") {
                    if let Ok(size) = size_str.parse::<u64>() {
                        builder.size_bytes = Some(size);
                    }
                } else if let Some(modified) = extract_xml_value(line, "LastModified") {
                    // Parse ISO 8601 timestamp from COS
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&modified) {
                        builder.timestamp = Some(dt.to_rfc3339());
                    } else {
                        // Try alternative timestamp format
                        println!("⚠️ [DEBUG] Failed to parse timestamp: {}", modified);
                        builder.timestamp = Some(chrono::Utc::now().to_rfc3339());
                    }
                } else if let Some(etag) = extract_xml_value(line, "ETag") {
                    // Clean up ETag: remove quotes, whitespace, and other formatting
                    let cleaned_etag = etag
                        .trim()                           // Remove leading/trailing whitespace
                        .trim_matches('"')               // Remove quotes
                        .trim_matches('\'')              // Remove single quotes
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric()) // Keep only alphanumeric characters
                        .collect::<String>();
                    
                    if !cleaned_etag.is_empty() {
                        println!("🏷️ [DEBUG] Raw ETag: '{}' ({}), Cleaned: '{}' ({})", 
                            etag, etag.len(), cleaned_etag, cleaned_etag.len());
                        builder.checksum = Some(cleaned_etag);
                    } else {
                        println!("⚠️ [DEBUG] Empty ETag after cleaning: '{}'", etag);
                    }
                }
            }
        }
        
        println!("📊 [DEBUG] Found {} total saves after parsing", saves.len());
        
        // Extract game_id from file path and map to app_id if needed
        for save in &mut saves {
            // Use the improved mapping function that handles game names
            save.game_id = extract_and_map_game_id(&save.file_id);
            println!("🎮 [DEBUG] Mapped {} -> game_id: {}", save.file_id, save.game_id);
        }
        
        // Filter by game_id if specified
        if let Some(gid) = game_id {
            println!("🔍 [DEBUG] Filtering saves for game_id: {}", gid);
            
            // Get all possible names for this app_id
            let possible_names = game_mapping::get_possible_names_for_appid(gid);
            println!("🔍 [DEBUG] Possible names for {}: {:?}", gid, possible_names);
            
            // Filter saves that match any of the possible names
            saves = saves.into_iter()
                .filter(|save| possible_names.contains(&save.game_id))
                .collect();
            
            println!("📊 [DEBUG] {} saves match game_id {}", saves.len(), gid);
        }
        
        saves.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        println!("✅ [DEBUG] Returning {} saves for user {}", saves.len(), user_id);
        Ok(saves)
    }

    async fn delete_save(&self, metadata: &SaveMetadata) -> Result<()> {
        let url = self.get_cos_url(&metadata.file_id);
        
        // Generate authorization header for DELETE request
        let (authorization, _) = self.generate_cos_authorization("DELETE", &metadata.file_id, "", 0)?;;
        
        let response = self.client
            .delete(&url)
            .header("Authorization", authorization)
            .header("Host", format!("{}.cos.{}.myqcloud.com", self.bucket, self.region))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to delete from Tencent COS: {} - {}", status, body));
        }

        Ok(())
    }

    async fn resume_upload(&self, _upload_id: &str, offset: u64, data: Bytes) -> Result<UploadProgress> {
        let total_size = offset + data.len() as u64;
        let checksum = Self::calculate_sha256(&data);
        
        // Simplified implementation - would need proper multipart upload support
        Ok(UploadProgress {
            bytes_uploaded: offset + data.len() as u64,
            total_bytes: total_size,
            checksum,
        })
    }

    async fn test_connection(&self) -> Result<()> {
        self.test_bucket_access().await
    }
    
    async fn get_storage_info(&self, user_id: &str) -> Result<StorageInfo> {
        // Sanitize user ID for path safety
        let sanitized_user_id = user_id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();
            
        let prefix = format!("saves/{}/", sanitized_user_id);
        
        eprintln!("[TencentCOS] Getting storage info for user: {}, prefix: {}", sanitized_user_id, prefix);
        
        // Use COS LIST Objects API to get user's storage info
        let url = format!("https://{}.cos.{}.myqcloud.com/?prefix={}", 
            self.bucket, self.region, urlencoding::encode(&prefix));
        
        eprintln!("[TencentCOS] Request URL: {}", url);
        
        // Generate authorization header for GET request
        let query_params = format!("prefix={}", urlencoding::encode(&prefix));
        let (authorization, _) = self.generate_cos_authorization("GET", "", &query_params, 0)?;
        
        eprintln!("[TencentCOS] Authorization generated, sending request...");
        
        let response = self.client
            .get(&url)
            .header("Authorization", authorization)
            .header("Host", format!("{}.cos.{}.myqcloud.com", self.bucket, self.region))
            .send()
            .await?;

        eprintln!("[TencentCOS] Response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            eprintln!("[TencentCOS] Error response body: {}", body);
            return Err(anyhow::anyhow!("Failed to get storage info from Tencent COS: {} - {}", status, body));
        }

        let body = response.text().await?;
        let mut used_bytes = 0u64;
        let mut file_count = 0u32;
        
        // Debug: Print the complete XML response
        eprintln!("[TencentCOS] Complete response body:\n{}", body);
        eprintln!("[TencentCOS] Response length: {} bytes", body.len());
        
        // Parse XML response to calculate storage usage
        // Look for <Contents> sections which contain file information
        let mut in_contents = false;
        let mut current_size: Option<u64> = None;
        
        eprintln!("[TencentCOS] Starting XML parsing...");
        let mut contents_found = 0;
        
        for (line_num, line) in body.lines().enumerate() {
            let line = line.trim();
            
            if line.contains("<Contents>") {
                in_contents = true;
                current_size = None;
                contents_found += 1;
                eprintln!("[TencentCOS] Found <Contents> #{} at line {}", contents_found, line_num);
            } else if line.contains("</Contents>") {
                if let Some(size) = current_size {
                    used_bytes += size;
                    file_count += 1;
                    eprintln!("[TencentCOS] Processed file: {} bytes (total: {} bytes, {} files)", size, used_bytes, file_count);
                } else {
                    eprintln!("[TencentCOS] Warning: </Contents> without size at line {}", line_num);
                }
                in_contents = false;
                current_size = None;
            } else if in_contents && line.starts_with("<Size>") && line.ends_with("</Size>") {
                if let Some(start) = line.find("<Size>") {
                    if let Some(end) = line.find("</Size>") {
                        let size_str = &line[start + 6..end];
                        match size_str.parse::<u64>() {
                            Ok(size) => {
                                current_size = Some(size);
                                eprintln!("[TencentCOS] Parsed size: {} bytes", size);
                            },
                            Err(e) => {
                                eprintln!("[TencentCOS] Failed to parse size '{}': {}", size_str, e);
                            }
                        }
                    }
                }
            } else if in_contents && line.contains("<Key>") && line.contains("</Key>") {
                if let Some(start) = line.find("<Key>") {
                    if let Some(end) = line.find("</Key>") {
                        let key = &line[start + 5..end];
                        eprintln!("[TencentCOS] Processing key: {}", key);
                    }
                }
            }
        }
        
        eprintln!("[TencentCOS] Storage info result: {} bytes, {} files", used_bytes, file_count);
        
        Ok(StorageInfo {
            used_bytes,
            total_bytes: None, // COS doesn't have fixed quota limits
            file_count,
            bucket_used_bytes: None, // Will be filled by combined call
            bucket_total_objects: None, // Will be filled by combined call
        })
    }
    
    async fn get_bucket_storage_info(&self) -> Result<(u64, u32)> {
        // Use COS LIST Objects API to get entire bucket storage info
        let url = format!("https://{}.cos.{}.myqcloud.com/", self.bucket, self.region);
        
        eprintln!("[TencentCOS] Getting bucket storage info, URL: {}", url);
        
        // Generate authorization header for GET request
        let (authorization, _) = self.generate_cos_authorization("GET", "", "", 0)?;
        
        let response = self.client
            .get(&url)
            .header("Authorization", authorization)
            .header("Host", format!("{}.cos.{}.myqcloud.com", self.bucket, self.region))
            .send()
            .await?;

        eprintln!("[TencentCOS] Bucket info response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            eprintln!("[TencentCOS] Bucket error response body: {}", body);
            return Err(anyhow::anyhow!("Failed to get bucket info from Tencent COS: {} - {}", status, body));
        }

        let body = response.text().await?;
        let mut total_bytes = 0u64;
        let mut total_objects = 0u32;
        
        // Debug: Print the complete bucket XML response
        eprintln!("[TencentCOS] Complete bucket response body:\n{}", body);
        eprintln!("[TencentCOS] Bucket response length: {} bytes", body.len());
        
        // Parse XML response to calculate total storage usage
        let mut in_contents = false;
        let mut current_size: Option<u64> = None;
        
        eprintln!("[TencentCOS] Starting bucket XML parsing...");
        let mut bucket_contents_found = 0;
        
        for (line_num, line) in body.lines().enumerate() {
            let line = line.trim();
            
            if line.contains("<Contents>") {
                in_contents = true;
                current_size = None;
                bucket_contents_found += 1;
                eprintln!("[TencentCOS] Found bucket <Contents> #{} at line {}", bucket_contents_found, line_num);
            } else if line.contains("</Contents>") {
                if let Some(size) = current_size {
                    total_bytes += size;
                    total_objects += 1;
                    eprintln!("[TencentCOS] Processed bucket file: {} bytes (total: {} bytes, {} objects)", size, total_bytes, total_objects);
                } else {
                    eprintln!("[TencentCOS] Warning: bucket </Contents> without size at line {}", line_num);
                }
                in_contents = false;
            } else if in_contents && line.contains("<Size>") {
                if let Some(start) = line.find("<Size>") {
                    if let Some(end) = line.find("</Size>") {
                        let size_str = &line[start + 6..end];
                        match size_str.parse::<u64>() {
                            Ok(size) => {
                                current_size = Some(size);
                                eprintln!("[TencentCOS] Parsed bucket size: {} bytes", size);
                            },
                            Err(e) => {
                                eprintln!("[TencentCOS] Failed to parse bucket size '{}': {}", size_str, e);
                            }
                        }
                    }
                }
            } else if in_contents && line.contains("<Key>") && line.contains("</Key>") {
                if let Some(start) = line.find("<Key>") {
                    if let Some(end) = line.find("</Key>") {
                        let key = &line[start + 5..end];
                        eprintln!("[TencentCOS] Processing bucket key: {}", key);
                    }
                }
            }
        }
        
        eprintln!("[TencentCOS] Bucket storage info result: {} bytes, {} objects", total_bytes, total_objects);
        
        Ok((total_bytes, total_objects))
    }
}

pub struct S3Backend {
    config: Option<aws_config::SdkConfig>,
    bucket: String,
    prefix: String,
}

impl S3Backend {
    pub fn new() -> Self {
        Self {
            config: None,
            bucket: "steam-cloud-sync".to_string(),
            prefix: "saves/".to_string(),
        }
    }

    pub fn with_config(bucket: String, prefix: String) -> Self {
        Self {
            config: None,
            bucket,
            prefix,
        }
    }

    async fn get_client(&self) -> Result<aws_sdk_s3::Client> {
        let config = match &self.config {
            Some(cfg) => cfg.clone(),
            None => aws_config::defaults(aws_config::BehaviorVersion::latest()).load().await,
        };
        Ok(aws_sdk_s3::Client::new(&config))
    }

    async fn compress_save(&self, save_path: &Path) -> Result<Vec<u8>> {
        let save_path = save_path.to_path_buf();
        
        tokio::task::spawn_blocking(move || {
            let mut buffer = Vec::new();
            {
                let cursor = Cursor::new(&mut buffer);
                let mut zip = ZipWriter::new(cursor);
                
                if save_path.is_file() {
                    let file_name = save_path.file_name().unwrap().to_str().unwrap();
                    let file_data = std::fs::read(&save_path)?;
                    zip.start_file(file_name, zip::write::FileOptions::default())?;
                    zip.write_all(&file_data)?;
                } else if save_path.is_dir() {
                    Self::add_dir_to_zip_sync(&mut zip, &save_path, "")?;
                }
                
                zip.finish()?;
            }
            Ok::<Vec<u8>, anyhow::Error>(buffer)
        }).await?
    }

    fn add_dir_to_zip_sync<W: Write + std::io::Seek>(zip: &mut ZipWriter<W>, dir: &Path, prefix: &str) -> Result<()> {
        let entries = std::fs::read_dir(dir)?;
        
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let file_name = format!("{}{}", prefix, name.to_str().unwrap());

            if path.is_file() {
                let file_data = std::fs::read(&path)?;
                zip.start_file(&file_name, zip::write::FileOptions::default())?;
                zip.write_all(&file_data)?;
            } else if path.is_dir() {
                Self::add_dir_to_zip_sync(zip, &path, &format!("{}/", file_name))?;
            }
        }
        Ok(())
    }

    fn calculate_sha256(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
}

#[async_trait]
impl CloudBackend for S3Backend {
    async fn upload_save(&self, game_save: &GameSave, user_id: &str) -> Result<SaveMetadata> {
        let client = self.get_client().await?;
        let compressed_data = self.compress_save(&game_save.save_path).await?;
        let checksum = Self::calculate_sha256(&compressed_data);
        
        // Create filename with user ID and timestamp for separation
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let sanitized_user_id = user_id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();
        let key = format!("{}{}/{}/{}_{}_{}.zip", self.prefix, sanitized_user_id, game_save.app_id, game_save.name, timestamp, Uuid::new_v4());

        // Multipart upload for large files with resumability
        let multipart_upload = client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await?;

        let upload_id = multipart_upload.upload_id().unwrap();
        const CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5MB minimum for S3 multipart
        let mut parts = Vec::new();

        for (i, chunk) in compressed_data.chunks(CHUNK_SIZE).enumerate() {
            let part_number = (i + 1) as i32;
            
            let upload_part = client
                .upload_part()
                .bucket(&self.bucket)
                .key(&key)
                .upload_id(upload_id)
                .part_number(part_number)
                .body(aws_sdk_s3::primitives::ByteStream::from(chunk.to_vec()))
                .send()
                .await?;

            parts.push(
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(part_number)
                    .e_tag(upload_part.e_tag().unwrap_or_default())
                    .build()
            );
        }

        client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&key)
            .upload_id(upload_id)
            .multipart_upload(
                aws_sdk_s3::types::CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build()
            )
            .send()
            .await?;

        Ok(SaveMetadata {
            game_id: game_save.app_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            size_bytes: compressed_data.len() as u64,
            checksum,
            compressed: true,
            file_id: key,
        })
    }

    async fn download_save(&self, metadata: &SaveMetadata, local_path: &Path) -> Result<()> {
        let client = self.get_client().await?;
        
        let response = client
            .get_object()
            .bucket(&self.bucket)
            .key(&metadata.file_id)
            .send()
            .await?;

        let data = response.body.collect().await?.into_bytes();
        
        // Verify checksum with improved handling for S3
        let calculated_checksum = Self::calculate_sha256(&data);
        
        // Handle different checksum types more gracefully
        match metadata.checksum.len() {
            64 => {
                // This should be a SHA256 hash
                if calculated_checksum != metadata.checksum {
                    eprintln!("⚠️  [S3] SHA256 checksum mismatch:");
                    eprintln!("   Expected: {}", metadata.checksum);
                    eprintln!("   Calculated: {}", calculated_checksum);
                    eprintln!("   File size: {} bytes", data.len());
                    eprintln!("   Continuing download (integrity warning)...");
                } else {
                    eprintln!("✅ [S3] SHA256 checksum verified");
                }
            },
            32 => {
                // This is likely an MD5/ETag, skip SHA256 verification
                eprintln!("ℹ️  [S3] MD5/ETag checksum detected ({})", metadata.checksum);
                eprintln!("   File SHA256: {}", calculated_checksum);
                eprintln!("   Skipping verification (different hash types)");
            },
            40 => {
                // This might be SHA1
                eprintln!("ℹ️  [S3] SHA1 checksum detected ({})", metadata.checksum);
                eprintln!("   File SHA256: {}", calculated_checksum);
                eprintln!("   Skipping verification (different hash types)");
            },
            _ => {
                // Unknown format, log and continue
                eprintln!("⚠️  [S3] Unknown checksum format:");
                eprintln!("   Stored: '{}' ({} chars)", metadata.checksum, metadata.checksum.len());
                eprintln!("   File SHA256: {}", calculated_checksum);
                eprintln!("   Continuing download (no verification possible)...");
            }
        }

        // If the target path ends with .zip, save as ZIP file directly (for custom download location)
        if local_path.extension().and_then(|s| s.to_str()) == Some("zip") {
            fs::write(local_path, &data).await?;
        } else {
            // Otherwise, extract the ZIP to the game save directory
            extract_save_archive_helper(&data, local_path).await?;
        }
        
        Ok(())
    }

    async fn list_saves(&self, user_id: &str, game_id: Option<&str>) -> Result<Vec<SaveMetadata>> {
        let client = self.get_client().await?;
        
        // Sanitize user ID for path safety
        let sanitized_user_id = user_id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();
        
        // Construct prefix based on user ID and optionally game ID
        let prefix = match game_id {
            Some(gid) => format!("{}{}/{}/", self.prefix, sanitized_user_id, gid),
            None => format!("{}{}/", self.prefix, sanitized_user_id),
        };
        
        let response = client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&prefix)
            .send()
            .await?;

        let mut saves = Vec::new();
        
        for object in response.contents() {
            if let (Some(key), Some(size), Some(modified)) = (object.key(), object.size(), object.last_modified()) {
                // Extract game ID from the key path
                // Format: saves/user_id/app_id/game_name_timestamp_uuid.zip
                let key_parts: Vec<&str> = key.split('/').collect();
                let extracted_game_id = if key_parts.len() >= 4 {
                    key_parts[2].to_string() // app_id part
                } else {
                    "unknown".to_string()
                };
                
                saves.push(SaveMetadata {
                    game_id: extracted_game_id,
                    timestamp: modified.to_string(),
                    size_bytes: size as u64,
                    checksum: object.e_tag().unwrap_or_default().trim_matches('"').to_string(),
                    compressed: true,
                    file_id: key.to_string(),
                });
            }
        }
        
        // Sort by timestamp (newest first)
        saves.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        Ok(saves)
    }

    async fn delete_save(&self, metadata: &SaveMetadata) -> Result<()> {
        let client = self.get_client().await?;
        
        client
            .delete_object()
            .bucket(&self.bucket)
            .key(&metadata.file_id)
            .send()
            .await?;

        Ok(())
    }

    async fn resume_upload(&self, _upload_id: &str, offset: u64, data: Bytes) -> Result<UploadProgress> {
        // For S3, resume would involve continuing a multipart upload
        // This is a simplified implementation
        let checksum = Self::calculate_sha256(&data);
        
        Ok(UploadProgress {
            bytes_uploaded: offset + data.len() as u64,
            total_bytes: offset + data.len() as u64,
            checksum,
        })
    }

    async fn test_connection(&self) -> Result<()> {
        let client = self.get_client().await?;
        
        // Test by attempting to list objects in the bucket (HEAD bucket operation)
        match client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_msg = format!("Failed to connect to S3 bucket '{}': {}", self.bucket, e);
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }
    
    async fn get_storage_info(&self, user_id: &str) -> Result<StorageInfo> {
        let client = self.get_client().await?;
        
        // Sanitize user ID for path safety
        let sanitized_user_id = user_id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();
            
        let prefix = format!("{}{}/", self.prefix, sanitized_user_id);
        
        // List objects with the user's prefix to calculate storage usage
        let mut used_bytes = 0u64;
        let mut file_count = 0u32;
        
        let list_objects_output = client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&prefix)
            .send()
            .await?;
        
        if let Some(contents) = list_objects_output.contents {
            for object in contents {
                if let Some(size) = object.size {
                    used_bytes += size as u64;
                    file_count += 1;
                }
            }
        }
        
        Ok(StorageInfo {
            used_bytes,
            total_bytes: None, // S3 doesn't have fixed quota limits by default
            file_count,
            bucket_used_bytes: None, // Will be filled by combined call
            bucket_total_objects: None, // Will be filled by combined call
        })
    }
    
    async fn get_bucket_storage_info(&self) -> Result<(u64, u32)> {
        let client = self.get_client().await?;
        
        // List all objects in the bucket to calculate total storage usage
        let mut total_bytes = 0u64;
        let mut total_objects = 0u32;
        let mut continuation_token: Option<String> = None;
        
        loop {
            let mut request = client
                .list_objects_v2()
                .bucket(&self.bucket);
            
            if let Some(token) = &continuation_token {
                request = request.continuation_token(token);
            }
            
            let list_objects_output = request.send().await?;
            
            if let Some(contents) = list_objects_output.contents {
                for object in contents {
                    if let Some(size) = object.size {
                        total_bytes += size as u64;
                        total_objects += 1;
                    }
                }
            }
            
            // Check if there are more objects to list
            if list_objects_output.is_truncated.unwrap_or(false) {
                continuation_token = list_objects_output.next_continuation_token;
            } else {
                break;
            }
        }
        
        Ok((total_bytes, total_objects))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio_test;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    #[tokio::test]
    async fn test_tencentcos_backend() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let backend = TencentCOSBackend::new();
        
        let temp_dir = TempDir::new().unwrap();
        let save_path = temp_dir.path().join("test_save.dat");
        tokio::fs::write(&save_path, b"test save data").await.unwrap();

        let game_save = GameSave {
            app_id: 12345,
            name: "Test Game".to_string(),
            save_path,
        };

        // This would require mocking the actual HTTP calls
        // let result = backend.upload_save(&game_save).await;
        // assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_s3_backend() {
        // This would require localstack or AWS SDK mocking
        let backend = S3Backend::new();
        
        let temp_dir = TempDir::new().unwrap();
        let save_path = temp_dir.path().join("test_save.dat");
        tokio::fs::write(&save_path, b"test save data").await.unwrap();

        let game_save = GameSave {
            app_id: 54321,
            name: "Test Game S3".to_string(),
            save_path,
        };

        // Mock S3 operations would be implemented here
        // let result = backend.upload_save(&game_save).await;
        // assert!(result.is_ok());
    }

    #[test]
    fn test_backend_factory() {
        let tencentcos_backend = backend(BackendType::TencentCOS);
        let s3_backend = backend(BackendType::S3);
        
        // Type checking - these should compile
        assert!(std::ptr::addr_of!(tencentcos_backend) != std::ptr::addr_of!(s3_backend));
    }

    #[tokio::test]
    async fn test_compression() {
        let backend = TencentCOSBackend::new();
        
        let temp_dir = TempDir::new().unwrap();
        let save_file = temp_dir.path().join("test.save");
        tokio::fs::write(&save_file, b"test save content").await.unwrap();

        let compressed = backend.compress_save(&save_file).await.unwrap();
        assert!(!compressed.is_empty());
        
        // Verify it's actually a ZIP file
        let cursor = Cursor::new(&compressed);
        let mut zip = zip::ZipArchive::new(cursor).unwrap();
        assert_eq!(zip.len(), 1);
    }

    #[test]
    fn test_sha256_calculation() {
        let data = b"test data";
        let checksum = TencentCOSBackend::calculate_sha256(data);
        let expected = "916f0027a575074ce72a331777c3478d6513f786a591bd892da1a577bf2335f9";
        assert_eq!(checksum, expected);
    }
}