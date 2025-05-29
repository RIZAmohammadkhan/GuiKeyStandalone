// src/application/log_service.rs

use crate::app_config::ServerSettings;
use crate::domain::event_types::LogEvent;
use crate::errors::ServerError;
use crate::infrastructure::{
    database::DbConnection,
    encryption::decrypt_payload, // Assuming decrypt_payload is pub in infrastructure::encryption
};
use std::sync::Arc;
use tokio::time::{interval, Duration, MissedTickBehavior};
use uuid::Uuid; // For client_id if needed in future filtering

#[derive(Clone)] // LogService needs to be Clone to be used as Actix web::Data
pub struct LogService {
    db_conn: DbConnection,
    encryption_key: [u8; 32], // Store the key for decryption
    settings: Arc<ServerSettings>, // For retention policy, etc.
}

impl LogService {
    pub fn new(db_conn: DbConnection, settings: Arc<ServerSettings>) -> Self {
        let key = settings.encryption_key; // Copy the key array
        LogService {
            db_conn,
            encryption_key: key,
            settings,
        }
    }

    /// Ingests a batch of encrypted log data.
    /// It decrypts, deserializes, and stores the log events.
    pub async fn ingest_log_batch(
        &self,
        client_id_str: &str, // Client ID from header, for logging/potential future use
        encrypted_data: Vec<u8>,
    ) -> Result<usize, ServerError> {
        tracing::debug!(
            "LogService: Received encrypted log batch of {} bytes from client_id: {}",
            encrypted_data.len(),
            client_id_str
        );

        // 1. Decrypt payload
        let decrypted_json_bytes = decrypt_payload(&encrypted_data, &self.encryption_key)?;
        tracing::trace!("LogService: Successfully decrypted payload.");

        // 2. Deserialize JSON into Vec<LogEvent>
        // The client sends a JSON array of LogEvent objects.
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

        // 3. Store in database
        // This can be a blocking operation, so consider spawn_blocking if it becomes a bottleneck.
        // For SQLite with a Mutex-guarded connection, direct call might be okay for now.
        self.db_conn.insert_log_events(log_events)?;
        tracing::info!("LogService: Successfully stored {} log events from client_id: {}.", num_events, client_id_str);

        Ok(num_events)
    }

    /// Retrieves a paginated list of log events.
    pub async fn get_log_events_paginated(
        &self,
        page: u32,
        page_size: u32,
    ) -> Result<Vec<LogEvent>, ServerError> {
        tracing::debug!("LogService: Querying log events - page: {}, page_size: {}", page, page_size);
        // Again, potential spawn_blocking if DB queries are slow, though SQLite reads are often fast.
        self.db_conn.query_log_events(page, page_size)
    }

    /// Gets the total count of log events.
    pub async fn get_total_log_count(&self) -> Result<i64, ServerError> {
        tracing::debug!("LogService: Querying total log event count.");
        self.db_conn.count_total_log_events()
    }
    
    /// Deletes old logs based on the retention policy.
    /// This method is intended to be called periodically.
    async fn delete_old_logs_task(&self) -> Result<usize, ServerError> {
        tracing::info!("LogService: Starting scheduled task to delete old logs.");
        let deleted_count = self.db_conn.delete_old_logs(&self.settings)?;
        if deleted_count > 0 {
            tracing::info!("LogService: Scheduled deletion removed {} old log entries.", deleted_count);
        } else {
            tracing::debug!("LogService: Scheduled deletion found no old logs to remove.");
        }
        Ok(deleted_count)
    }
}

/// Spawns a Tokio task that periodically runs the log deletion process.
pub fn spawn_periodic_log_deletion_task(log_service: LogService) {
    if log_service.settings.log_retention_days == 0 {
        tracing::info!("LogService: Periodic log deletion is disabled (retention_days = 0).");
        return;
    }

    // Run deletion check, e.g., every 6 or 24 hours. Make this configurable if needed.
    let deletion_check_interval_hours = 24u64; 
    let mut interval = interval(Duration::from_secs(deletion_check_interval_hours * 60 * 60));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay); // If server was off, run when it starts

    tokio::spawn(async move {
        tracing::info!(
            "LogService: Periodic log deletion task started. Check interval: {} hours.",
            deletion_check_interval_hours
        );
        loop {
            interval.tick().await;
            tracing::info!("LogService: Triggering periodic deletion of old logs...");
            if let Err(e) = log_service.delete_old_logs_task().await {
                tracing::error!("LogService: Error during periodic log deletion: {}", e);
            }
        }
    });
}