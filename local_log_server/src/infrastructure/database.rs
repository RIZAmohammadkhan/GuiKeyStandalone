// src/infrastructure/database.rs

use crate::app_config::ServerSettings;
use crate::domain::event_types::{EventData as DomainEventData, LogEvent}; // Alias EventData
use crate::errors::ServerError;
use chrono::Utc;
use rusqlite::{Connection, params}; // Removed OptionalExtension, RusqliteResult, ToSql as not directly used
use std::path::Path;
use std::sync::{Arc, Mutex}; // Removed DateTime as Utc::now() is used
// use uuid::Uuid; // Not directly used here, Uuid comes from LogEvent

#[derive(Clone)]
pub struct DbConnection(Arc<Mutex<Connection>>);

impl DbConnection {
    pub fn new(db_path: &Path) -> Result<Self, ServerError> {
        tracing::info!("Opening database at: {:?}", db_path);
        let conn = Connection::open(db_path)?;
        let db_conn = DbConnection(Arc::new(Mutex::new(conn)));
        db_conn.init_tables()?;
        Ok(db_conn)
    }

    fn init_tables(&self) -> Result<(), ServerError> {
        let conn = self
            .0
            .lock()
            .map_err(|_e| ServerError::Internal("DB Mutex poisoned".to_string()))?;
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE IF NOT EXISTS logs (
                id TEXT PRIMARY KEY,
                client_id TEXT NOT NULL,
                event_timestamp INTEGER NOT NULL,
                application_name TEXT NOT NULL,
                initial_window_title TEXT,
                schema_version INTEGER NOT NULL,
                session_start_time INTEGER NOT NULL,
                session_end_time INTEGER NOT NULL,
                typed_text TEXT,
                clipboard_actions_json TEXT,
                raw_event_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_logs_event_timestamp ON logs (event_timestamp);
            CREATE INDEX IF NOT EXISTS idx_logs_client_id ON logs (client_id);
            CREATE INDEX IF NOT EXISTS idx_logs_application_name ON logs (application_name);
            COMMIT;",
        )?;
        tracing::info!("Database tables initialized successfully.");
        Ok(())
    }

    pub fn insert_log_events(&self, events_vec: Vec<LogEvent>) -> Result<(), ServerError> {
        if events_vec.is_empty() {
            return Ok(());
        }
        let num_events_to_insert = events_vec.len();
        let mut conn = self
            .0
            .lock()
            .map_err(|_e| ServerError::Internal("DB Mutex poisoned".to_string()))?;

        let tx = conn.transaction()?;

        for event in events_vec {
            // events_vec is moved here
            let (session_start_time_ts, session_end_time_ts, typed_text_opt, clipboard_json_opt) =
                match &event.event_data {
                    DomainEventData::ApplicationActivity {
                        // Use aliased DomainEventData
                        start_time,
                        end_time,
                        typed_text,
                        clipboard_actions,
                    } => (
                        start_time.timestamp(),
                        end_time.timestamp(),
                        Some(typed_text.clone()),
                        Some(serde_json::to_string(clipboard_actions)?),
                    ),
                    // If other variants existed, they would be handled here
                    // _ => return Err(ServerError::Internal(format!("Unknown EventData variant for event id: {}", event.id))),
                };

            let raw_event_json = serde_json::to_string(&event)?;

            tx.execute(
                "INSERT OR IGNORE INTO logs (
                    id, client_id, event_timestamp, application_name, initial_window_title, schema_version,
                    session_start_time, session_end_time, typed_text, clipboard_actions_json, raw_event_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    event.id.to_string(),
                    event.client_id.to_string(),
                    event.timestamp.timestamp(),
                    event.application_name,
                    event.initial_window_title,
                    event.schema_version,
                    session_start_time_ts,
                    session_end_time_ts,
                    typed_text_opt,
                    clipboard_json_opt,
                    raw_event_json,
                ],
            )?;
        }
        tx.commit()?;
        tracing::debug!(
            "Successfully inserted {} log events into the database.",
            num_events_to_insert
        );
        Ok(())
    }

    pub fn query_log_events(
        &self,
        page: u32,
        page_size: u32,
    ) -> Result<Vec<LogEvent>, ServerError> {
        let conn = self
            .0
            .lock()
            .map_err(|_e| ServerError::Internal("DB Mutex poisoned".to_string()))?;
        let offset = (page.saturating_sub(1)) * page_size;

        let mut stmt = conn.prepare(
            "SELECT raw_event_json FROM logs ORDER BY event_timestamp DESC LIMIT ?1 OFFSET ?2",
        )?;

        let event_iter = stmt.query_map(params![page_size, offset], |row| {
            let raw_json: String = row.get(0)?;
            serde_json::from_str::<LogEvent>(&raw_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })?;

        let mut events = Vec::new();
        for event_result in event_iter {
            events.push(event_result?);
        }
        tracing::debug!(
            "Queried {} log events (page {}, page_size {}).",
            events.len(),
            page,
            page_size
        );
        Ok(events)
    }

    pub fn count_total_log_events(&self) -> Result<i64, ServerError> {
        let conn = self
            .0
            .lock()
            .map_err(|_e| ServerError::Internal("DB Mutex poisoned".to_string()))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn delete_old_logs(&self, settings: &Arc<ServerSettings>) -> Result<usize, ServerError> {
        if settings.log_retention_days == 0 {
            tracing::debug!("Log retention is indefinite (0 days), skipping deletion of old logs.");
            return Ok(0);
        }
        let conn = self
            .0
            .lock()
            .map_err(|_e| ServerError::Internal("DB Mutex poisoned".to_string()))?;
        let retention_period_duration = chrono::Duration::days(settings.log_retention_days as i64);
        let cutoff_timestamp = (Utc::now() - retention_period_duration).timestamp();

        tracing::info!(
            "Deleting logs older than {} days (before timestamp {}).",
            settings.log_retention_days,
            cutoff_timestamp
        );

        let rows_deleted = conn.execute(
            "DELETE FROM logs WHERE event_timestamp < ?1",
            params![cutoff_timestamp],
        )?;

        tracing::info!("Deleted {} old log entries.", rows_deleted);
        Ok(rows_deleted)
    }
}
