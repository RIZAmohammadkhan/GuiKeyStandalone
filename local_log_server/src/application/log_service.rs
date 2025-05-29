use crate::app_config::ServerSettings;
use crate::domain::event_types::LogEvent;
use crate::errors::ServerError;
use crate::infrastructure::{
    database::DbConnection,
    encryption::decrypt_payload,
};
use actix_web::web; // For web::block
use std::sync::Arc;
use tokio::time::{interval, Duration, MissedTickBehavior};

#[derive(Clone)]
pub struct LogService {
    db_conn: DbConnection,
    encryption_key: [u8; 32],
    settings: Arc<ServerSettings>,
}

// Helper to map BlockingError to ServerError
fn map_blocking_error(e: actix_web::error::BlockingError) -> ServerError {
    ServerError::Internal(format!("Blocking task panicked or was cancelled: {}", e))
}

impl LogService {
    pub fn new(db_conn: DbConnection, settings: Arc<ServerSettings>) -> Self {
        let key = settings.encryption_key;
        LogService {
            db_conn,
            encryption_key: key,
            settings,
        }
    }

    pub async fn ingest_log_batch(
        &self,
        client_id_str: &str,
        encrypted_data: Vec<u8>,
    ) -> Result<usize, ServerError> {
        tracing::debug!(
            "LogService: Received encrypted log batch of {} bytes from client_id: {}",
            encrypted_data.len(),
            client_id_str
        );

        let key_clone = self.encryption_key;
        // Closure for decrypt_payload returns Result<Vec<u8>, ServerError>
        // web::block(...).await -> Result<Result<Vec<u8>, ServerError>, BlockingError>
        // .map_err(map_blocking_error) -> Result<Result<Vec<u8>, ServerError>, ServerError>
        // outer ? -> Result<Vec<u8>, ServerError>
        // inner ? -> Vec<u8>
        let decrypted_json_bytes = web::block(move || decrypt_payload(&encrypted_data, &key_clone))
            .await
            .map_err(map_blocking_error)??; // This is correct if we want Vec<u8> here.
        
        tracing::trace!("LogService: Successfully decrypted payload.");

        let log_events: Vec<LogEvent> = serde_json::from_slice(&decrypted_json_bytes)
            .map_err(|e| {
                tracing::error!("LogService: Failed to deserialize log events JSON: {}. Data (first 200B): {:?}",
                    e,
                    String::from_utf8_lossy(
                        &decrypted_json_bytes[..std::cmp::min(200, decrypted_json_bytes.len())]
                    )
                );
                ServerError::Json(e)
            })?;

        let num_events = log_events.len();
        tracing::debug!("LogService: Deserialized {} log events from client_id: {}.", num_events, client_id_str);

        if num_events == 0 {
            tracing::debug!("LogService: Received empty batch of events (after deserialization). Nothing to store.");
            return Ok(0);
        }

        let db_conn_clone = self.db_conn.clone();
        // Closure for insert_log_events returns Result<(), ServerError>
        // web::block(...).await.map_err(...)?? -> unwraps fully to () on success, or propagates ServerError. Correct.
        web::block(move || db_conn_clone.insert_log_events(log_events))
            .await
            .map_err(map_blocking_error)??;

        tracing::info!("LogService: Successfully stored {} log events from client_id: {}.", num_events, client_id_str);
        Ok(num_events)
    }

    pub async fn get_log_events_paginated(
        &self,
        page: u32,
        page_size: u32,
    ) -> Result<Vec<LogEvent>, ServerError> {
        tracing::debug!("LogService: Querying log events - page: {}, page_size: {}", page, page_size);
        let db_conn_clone = self.db_conn.clone();
        // Closure returns Result<Vec<LogEvent>, ServerError>
        // web::block(...).await.map_err(...) -> Result<Result<Vec<LogEvent>, ServerError>, ServerError>
        // ? on this -> Result<Vec<LogEvent>, ServerError>. This matches function signature.
        web::block(move || db_conn_clone.query_log_events(page, page_size))
            .await
            .map_err(map_blocking_error)? // Single ? here
    }

    pub async fn get_total_log_count(&self) -> Result<i64, ServerError> {
        tracing::debug!("LogService: Querying total log event count.");
        let db_conn_clone = self.db_conn.clone();
        // Closure returns Result<i64, ServerError>
        // web::block(...).await.map_err(...) -> Result<Result<i64, ServerError>, ServerError>
        // ? on this -> Result<i64, ServerError>. This matches function signature.
        web::block(move || db_conn_clone.count_total_log_events())
            .await
            .map_err(map_blocking_error)? // Single ? here
    }

    // This is an internal helper, but let's make it consistent.
    // It's called by the spawned task which handles the Result.
    async fn delete_old_logs_from_db(&self) -> Result<usize, ServerError> {
        let db_conn_clone = self.db_conn.clone();
        let settings_clone = Arc::clone(&self.settings);
        // Closure returns Result<usize, ServerError>
        // web::block(...).await.map_err(...) -> Result<Result<usize, ServerError>, ServerError>
        // ? on this -> Result<usize, ServerError>.
        web::block(move || db_conn_clone.delete_old_logs(&settings_clone))
            .await
            .map_err(map_blocking_error)? // Single ? here
    }

    // This public method is for the spawned task, which will handle the Result.
    pub async fn run_scheduled_log_deletion(&self) -> Result<usize, ServerError> {
        tracing::info!("LogService: Starting scheduled task to delete old logs.");
        // Call the internal helper that returns Result<usize, ServerError>
        let deleted_count = self.delete_old_logs_from_db().await?;

        if deleted_count > 0 {
            tracing::info!("LogService: Scheduled deletion removed {} old log entries.", deleted_count);
        } else {
            tracing::debug!("LogService: Scheduled deletion found no old logs to remove.");
        }
        Ok(deleted_count)
    }
}

pub fn spawn_periodic_log_deletion_task(log_service: LogService) {
    if log_service.settings.log_retention_days == 0 {
        tracing::info!("LogService: Periodic log deletion is disabled (retention_days = 0).");
        return;
    }

    let deletion_check_interval_hours = log_service.settings.log_deletion_check_interval_hours;
    let mut interval = interval(Duration::from_secs(deletion_check_interval_hours * 60 * 60));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    tokio::spawn(async move {
        tracing::info!(
            "LogService: Periodic log deletion task started. Check interval: {} hours.",
            deletion_check_interval_hours
        );
        loop {
            interval.tick().await;
            tracing::info!("LogService: Triggering periodic deletion of old logs...");
            // run_scheduled_log_deletion now returns Result<usize, ServerError>
            match log_service.run_scheduled_log_deletion().await {
                Ok(count) => {
                    // This trace is fine, count is known.
                    tracing::debug!("LogService: Periodic deletion task completed, {} entries affected.", count);
                }
                Err(e) => {
                    tracing::error!("LogService: Error during periodic log deletion: {}", e);
                }
            }
        }
    });
}