// Database fix utility to resolve Pending operations and missing file sizes
// Run this with: cargo run --bin fix_database

use anyhow::Result;
use sqlx::SqlitePool;
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔧 [DEBUG] Starting database fix utility...");
    
    // Get database path (same logic as in persistence crate)
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?
        .join("steam-cloud-sync");
    
    let db_path = data_dir.join("steam-cloud-sync.db");
    
    if !db_path.exists() {
        println!("❌ Database file doesn't exist at: {}", db_path.display());
        return Ok(());
    }
    
    println!("📄 [DEBUG] Found database at: {}", db_path.display());
    
    // Connect to database
    let database_url = format!("sqlite:{}", db_path.display());
    let pool = SqlitePool::connect(&database_url).await?;
    
    println!("✅ [DEBUG] Connected to database successfully");
    
    // Check current status counts
    println!("\n📊 Current operation status counts:");
    let status_counts = sqlx::query!(
        "SELECT status, COUNT(*) as count FROM cloud_operations GROUP BY status"
    )
    .fetch_all(&pool)
    .await?;
    
    for row in status_counts {
        println!("   {}: {} operations", row.status, row.count);
    }
    
    // Fix operations that are Pending but have file_size (indicating successful upload)
    println!("\n🔧 [DEBUG] Fixing Pending operations with file_size...");
    let fixed_count = sqlx::query!(
        r#"
        UPDATE cloud_operations 
        SET status = 'completed',
            completed_at = ?,
            progress = 1.0
        WHERE status = 'pending' 
          AND file_size IS NOT NULL 
          AND file_size > 0
        "#,
        Utc::now().to_rfc3339()
    )
    .execute(&pool)
    .await?;
    
    println!("✅ [DEBUG] Fixed {} operations from Pending to Completed", fixed_count.rows_affected());
    
    // Update missing file paths for completed operations if possible
    println!("\n🔧 [DEBUG] Checking for operations missing file_path...");
    let missing_paths = sqlx::query!(
        "SELECT COUNT(*) as count FROM cloud_operations WHERE file_path IS NULL"
    )
    .fetch_one(&pool)
    .await?;
    
    println!("📝 [DEBUG] Found {} operations without file_path", missing_paths.count);
    
    // Show final status counts
    println!("\n📊 Final operation status counts:");
    let final_status_counts = sqlx::query!(
        "SELECT status, COUNT(*) as count FROM cloud_operations GROUP BY status"
    )
    .fetch_all(&pool)
    .await?;
    
    for row in final_status_counts {
        println!("   {}: {} operations", row.status, row.count);
    }
    
    // Show sample of recent operations for verification
    println!("\n🔍 [DEBUG] Recent operations sample:");
    let recent_ops = sqlx::query!(
        r#"
        SELECT game_id, operation_type, status, file_size, started_at, completed_at
        FROM cloud_operations 
        ORDER BY started_at DESC 
        LIMIT 10
        "#
    )
    .fetch_all(&pool)
    .await?;
    
    for op in recent_ops {
        let size_str = match op.file_size {
            Some(size) if size > 1024 * 1024 => format!("{:.1} MB", size as f64 / (1024.0 * 1024.0)),
            Some(size) if size > 1024 => format!("{:.1} KB", size as f64 / 1024.0),
            Some(size) => format!("{} bytes", size),
            None => "no size".to_string()
        };
        
        println!("   Game: {}, Type: {}, Status: {}, Size: {}", 
            op.game_id, op.operation_type, op.status, size_str);
    }
    
    println!("\n✅ [DEBUG] Database fix completed successfully!");
    
    Ok(())
}