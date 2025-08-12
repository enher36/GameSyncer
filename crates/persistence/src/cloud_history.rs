use crate::{models::*, Database};
use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Cloud history operations store
#[derive(Debug, Clone)]
pub struct CloudHistoryStore {
    db: Database,
}

impl CloudHistoryStore {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    
    /// Create a new cloud operation record
    pub async fn create_operation(&self, mut operation: CloudOperation) -> Result<CloudOperation> {
        operation.id = Uuid::new_v4();
        operation.started_at = Utc::now();
        
        sqlx::query(
            r#"
            INSERT INTO cloud_operations (
                id, game_id, operation_type, status, file_path, file_size, 
                checksum, error_message, started_at, completed_at, progress, metadata
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
        )
        .bind(operation.id.to_string())
        .bind(&operation.game_id)
        .bind(&operation.operation_type)
        .bind(&operation.status)
        .bind(&operation.file_path)
        .bind(operation.file_size)
        .bind(&operation.checksum)
        .bind(&operation.error_message)
        .bind(operation.started_at.to_rfc3339())
        .bind(operation.completed_at.map(|dt| dt.to_rfc3339()))
        .bind(operation.progress)
        .bind(&operation.metadata)
        .execute(&self.db.pool)
        .await?;
        
        Ok(operation)
    }
    
    /// Update operation status and progress
    pub async fn update_operation_progress(&self, id: Uuid, status: CloudOperationStatus, progress: Option<f32>) -> Result<()> {
        let completed_at = if matches!(status, CloudOperationStatus::Completed | CloudOperationStatus::Failed | CloudOperationStatus::Cancelled) {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };
        
        sqlx::query(
            r#"
            UPDATE cloud_operations 
            SET status = ?1, progress = ?2, completed_at = ?3
            WHERE id = ?4
            "#,
        )
        .bind(&status)
        .bind(progress)
        .bind(completed_at)
        .bind(id.to_string())
        .execute(&self.db.pool)
        .await?;
        
        Ok(())
    }
    
    /// Update operation with error
    pub async fn update_operation_error(&self, id: Uuid, error_message: String) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE cloud_operations 
            SET status = 'failed', error_message = ?1, completed_at = ?2
            WHERE id = ?3
            "#,
        )
        .bind(error_message)
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.db.pool)
        .await?;
        
        Ok(())
    }
    
    /// Get operations for a specific game
    pub async fn get_game_operations(&self, game_id: &str, limit: Option<i32>) -> Result<Vec<CloudOperation>> {
        let limit = limit.unwrap_or(50);
        
        let operations = sqlx::query_as::<_, CloudOperation>(
            r#"
            SELECT * FROM cloud_operations 
            WHERE game_id = ?1 
            ORDER BY started_at DESC 
            LIMIT ?2
            "#,
        )
        .bind(game_id)
        .bind(limit)
        .fetch_all(&self.db.pool)
        .await?;
        
        Ok(operations)
    }
    
    /// Get recent operations across all games
    pub async fn get_recent_operations(&self, limit: Option<i32>) -> Result<Vec<CloudOperation>> {
        let limit = limit.unwrap_or(100);
        
        let operations = sqlx::query_as::<_, CloudOperation>(
            r#"
            SELECT * FROM cloud_operations 
            ORDER BY started_at DESC 
            LIMIT ?1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.db.pool)
        .await?;
        
        Ok(operations)
    }
    
    /// Get operations by status
    pub async fn get_operations_by_status(&self, status: CloudOperationStatus) -> Result<Vec<CloudOperation>> {
        let operations = sqlx::query_as::<_, CloudOperation>(
            "SELECT * FROM cloud_operations WHERE status = ?1 ORDER BY started_at DESC"
        )
        .bind(&status)
        .fetch_all(&self.db.pool)
        .await?;
        
        Ok(operations)
    }
    
    /// Get operation by ID
    pub async fn get_operation(&self, id: Uuid) -> Result<Option<CloudOperation>> {
        let operation = sqlx::query_as::<_, CloudOperation>(
            "SELECT * FROM cloud_operations WHERE id = ?1"
        )
        .bind(id.to_string())
        .fetch_optional(&self.db.pool)
        .await?;
        
        Ok(operation)
    }
    
    /// Delete old operations (cleanup)
    pub async fn cleanup_old_operations(&self, older_than_days: i64) -> Result<u64> {
        let cutoff_date = Utc::now() - chrono::Duration::days(older_than_days);
        
        let result = sqlx::query(
            "DELETE FROM cloud_operations WHERE started_at < ?1"
        )
        .bind(cutoff_date.to_rfc3339())
        .execute(&self.db.pool)
        .await?;
        
        Ok(result.rows_affected())
    }
    
    /// Create a new sync session
    pub async fn create_sync_session(&self) -> Result<SyncSession> {
        let session = SyncSession::new();
        
        sqlx::query(
            r#"
            INSERT INTO sync_sessions (
                id, started_at, completed_at, games_synced, operations_count, 
                total_bytes, success, error_message
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(session.id.to_string())
        .bind(session.started_at.to_rfc3339())
        .bind(session.completed_at.map(|dt| dt.to_rfc3339()))
        .bind(session.games_synced)
        .bind(session.operations_count)
        .bind(session.total_bytes)
        .bind(session.success)
        .bind(&session.error_message)
        .execute(&self.db.pool)
        .await?;
        
        Ok(session)
    }
    
    /// Update sync session
    pub async fn update_sync_session(&self, session: &SyncSession) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sync_sessions 
            SET completed_at = ?1, games_synced = ?2, operations_count = ?3,
                total_bytes = ?4, success = ?5, error_message = ?6
            WHERE id = ?7
            "#,
        )
        .bind(session.completed_at.map(|dt| dt.to_rfc3339()))
        .bind(session.games_synced)
        .bind(session.operations_count)
        .bind(session.total_bytes)
        .bind(session.success)
        .bind(&session.error_message)
        .bind(session.id.to_string())
        .execute(&self.db.pool)
        .await?;
        
        Ok(())
    }
    
    /// Get recent sync sessions
    pub async fn get_recent_sessions(&self, limit: Option<i32>) -> Result<Vec<SyncSession>> {
        let limit = limit.unwrap_or(20);
        
        let sessions = sqlx::query_as::<_, SyncSession>(
            "SELECT * FROM sync_sessions ORDER BY started_at DESC LIMIT ?1"
        )
        .bind(limit)
        .fetch_all(&self.db.pool)
        .await?;
        
        Ok(sessions)
    }
    
    /// Get sync statistics
    pub async fn get_sync_stats(&self) -> Result<SyncStats> {
        let total_sessions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sync_sessions")
            .fetch_one(&self.db.pool)
            .await?;
        
        let successful_sessions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sync_sessions WHERE success = true")
            .fetch_one(&self.db.pool)
            .await?;
        
        let total_operations: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cloud_operations")
            .fetch_one(&self.db.pool)
            .await?;
        
        let successful_operations: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cloud_operations WHERE status = 'completed'")
            .fetch_one(&self.db.pool)
            .await?;
        
        let total_bytes: (Option<i64>,) = sqlx::query_as("SELECT SUM(total_bytes) FROM sync_sessions WHERE total_bytes IS NOT NULL")
            .fetch_one(&self.db.pool)
            .await?;
        
        Ok(SyncStats {
            total_sessions: total_sessions.0,
            successful_sessions: successful_sessions.0,
            total_operations: total_operations.0,
            successful_operations: successful_operations.0,
            total_bytes_synced: total_bytes.0.unwrap_or(0),
        })
    }
}

/// Sync statistics
#[derive(Debug, Clone)]
pub struct SyncStats {
    pub total_sessions: i64,
    pub successful_sessions: i64,
    pub total_operations: i64,
    pub successful_operations: i64,
    pub total_bytes_synced: i64,
}