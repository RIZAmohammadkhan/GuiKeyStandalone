use crate::app_config::Settings;
use crate::event_types::LogEvent;
use crate::errors::AppError;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration, MissedTickBehavior};
use uuid::Uuid;
use chrono::Utc;

#[derive(Clone)]
pub struct LogStoreHandle {
    tx: mpsc::Sender<LogStoreCommand>,
}

enum LogStoreCommand {
    AddEvent(LogEvent, oneshot::Sender<Result<(), AppError>>),
    GetBatch(usize, oneshot::Sender<Result<Vec<LogEvent>, AppError>>),
    ConfirmSync(Vec<Uuid>, oneshot::Sender<Result<(), AppError>>),
}

impl LogStoreHandle {
    pub async fn add_event(&self, event: LogEvent) -> Result<(), AppError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx.send(LogStoreCommand::AddEvent(event, resp_tx)).await
            .map_err(|e| AppError::TokioMpscSend(format!("LogStore add_event send failed: {}", e)))?;
        resp_rx.await.map_err(AppError::TokioOneshotRecv)?
    }

    pub async fn get_batch_for_sync(&self, limit: usize) -> Result<Vec<LogEvent>, AppError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx.send(LogStoreCommand::GetBatch(limit, resp_tx)).await
            .map_err(|e| AppError::TokioMpscSend(format!("LogStore get_batch send failed: {}", e)))?;
        resp_rx.await.map_err(AppError::TokioOneshotRecv)?
    }

    pub async fn confirm_events_synced(&self, ids: Vec<Uuid>) -> Result<(), AppError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx.send(LogStoreCommand::ConfirmSync(ids, resp_tx)).await
            .map_err(|e| AppError::TokioMpscSend(format!("LogStore confirm_sync send failed: {}", e)))?;
        resp_rx.await.map_err(AppError::TokioOneshotRecv)?
    }
}

struct LogStoreActor {
    settings: Arc<Settings>,
    file_path: PathBuf,
}

impl LogStoreActor {
    fn new(settings: Arc<Settings>) -> Result<Self, AppError> {
        let file_path = settings.log_file_path.clone();
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        OpenOptions::new().create(true).append(true).open(&file_path)?;
        tracing::info!("LogStoreActor initialized. Storage file: {:?}", file_path);
        Ok(Self { settings, file_path })
    }

    async fn handle_command(&mut self, command: LogStoreCommand) {
        match command {
            LogStoreCommand::AddEvent(event, responder) => {
                let res = self.write_event_to_file(&event);
                let _ = responder.send(res);
            }
            LogStoreCommand::GetBatch(limit, responder) => {
                let res = self.read_batch_from_file(limit);
                let _ = responder.send(res);
            }
            LogStoreCommand::ConfirmSync(ids, responder) => {
                let res = self.remove_events_from_file(&ids);
                let _ = responder.send(res);
            }
        }
    }

    fn write_event_to_file(&mut self, event: &LogEvent) -> Result<(), AppError> {
        if let Some(max_size_mb) = self.settings.max_log_file_size_mb {
            if let Ok(metadata) = std::fs::metadata(&self.file_path) {
                if metadata.len() > max_size_mb * 1024 * 1024 {
                    tracing::warn!(
                        "LogStore: Log file {:?} exceeds max size ({}MB). Consider implementing rotation.",
                        self.file_path, max_size_mb
                    );
                }
            }
        }

        let mut file = OpenOptions::new().create(true).append(true).open(&self.file_path)?;
        let json_event = serde_json::to_string(event)?;
        writeln!(file, "{}", json_event)?;
        tracing::trace!("LogStore: Event {:?} written to log store", event.id);
        Ok(())
    }

    fn read_events_from_file(&self) -> Result<Vec<LogEvent>, AppError> {
        let file = File::open(&self.file_path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line_res in reader.lines() {
            let line = line_res?;
            if line.trim().is_empty() { continue; }
            match serde_json::from_str::<LogEvent>(&line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    tracing::warn!("LogStore: Failed to deserialize event from log store file: {}. Line: '{}'", e, line);
                }
            }
        }
        Ok(events)
    }
    
    fn read_batch_from_file(&self, limit: usize) -> Result<Vec<LogEvent>, AppError> {
        let all_events = self.read_events_from_file()?;
        // Add explicit type annotation for batch
        let batch: Vec<LogEvent> = all_events.into_iter().take(limit).collect();
        tracing::debug!("LogStore: Read {} events for batch from log store.", batch.len());
        Ok(batch)
    }

    fn remove_events_from_file(&mut self, ids_to_remove: &[Uuid]) -> Result<(), AppError> {
        if ids_to_remove.is_empty() {
            return Ok(());
        }
        let existing_events = self.read_events_from_file()?;
        
        let mut remaining_events_data = Vec::new();
        let mut removed_count = 0;
        for event in existing_events {
            if !ids_to_remove.contains(&event.id) {
                remaining_events_data.push(serde_json::to_string(&event)? + "\n");
            } else {
                removed_count +=1;
            }
        }

        let mut file = OpenOptions::new().write(true).truncate(true).create(true).open(&self.file_path)?;
        for event_json_line in remaining_events_data {
            file.write_all(event_json_line.as_bytes())?;
        }
        tracing::info!("LogStore: Attempted to remove {} event IDs; {} events actually removed from log file.", ids_to_remove.len(), removed_count);
        Ok(())
    }

    async fn periodic_cleanup(&mut self) {
        if self.settings.local_log_cache_retention_days == 0 {
            return;
        }
        let retention_duration = chrono::Duration::days(self.settings.local_log_cache_retention_days as i64);
        let cutoff_time = Utc::now() - retention_duration;
        tracing::debug!("LogStore: Running periodic cleanup for logs older than {} days. Cutoff: {}", 
            self.settings.local_log_cache_retention_days, cutoff_time);

        match self.read_events_from_file() {
            Ok(all_events) => {
                let mut events_to_keep_data = Vec::new();
                let mut removed_count = 0;
                for event in all_events {
                    if event.timestamp >= cutoff_time {
                        match serde_json::to_string(&event) {
                            Ok(json_line) => events_to_keep_data.push(json_line + "\n"),
                            Err(e) => tracing::error!("LogStore: Failed to re-serialize event for cleanup: {}", e),
                        }
                    } else {
                        tracing::trace!("LogStore: Cleaning up old event ID {:?}, timestamp {}", event.id, event.timestamp);
                        removed_count += 1;
                    }
                }

                if removed_count > 0 {
                    match OpenOptions::new().write(true).truncate(true).create(true).open(&self.file_path) {
                        Ok(mut file) => {
                            for event_json_line in events_to_keep_data {
                                if let Err(e) = file.write_all(event_json_line.as_bytes()) {
                                    tracing::error!("LogStore: Error writing kept event during cleanup: {}", e);
                                    break; 
                                }
                            }
                            tracing::info!("LogStore: Periodic cleanup removed {} old unsynced events.", removed_count);
                        }
                        Err(e) => {
                            tracing::error!("LogStore: Failed to open log file for writing during cleanup: {}", e);
                        }
                    }
                } else {
                    tracing::debug!("LogStore: No old unsynced events found for periodic cleanup.");
                }
            }
            Err(e) => {
                tracing::error!("LogStore: Failed to read events for periodic cleanup: {}", e);
            }
        }
    }
}

pub async fn run_log_store_actor(
    settings: Arc<Settings>,
    mut rx: mpsc::Receiver<LogStoreCommand>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), AppError> {
    let mut actor = LogStoreActor::new(settings.clone())?;
    tracing::info!("LogStore actor task started.");

    let mut cleanup_interval = if actor.settings.local_log_cache_retention_days > 0 {
        let mut intv = interval(Duration::from_secs(60 * 60 * 6)); // e.g., every 6 hours
        intv.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Some(intv)
    } else {
        None
    };

    loop {
        let cleanup_tick_future = async {
            if let Some(ref mut interval) = cleanup_interval.as_mut() {
                interval.tick().await;
                return Some(());
            }
            std::future::pending().await
        };

        tokio::select! {
            biased;

            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow_and_update() {
                    tracing::info!("LogStore: Shutdown signal received.");
                    break;
                }
            }
            
            Some(_) = cleanup_tick_future => {
                actor.periodic_cleanup().await;
            }

            Some(command) = rx.recv() => {
                actor.handle_command(command).await;
            }
            else => {
                tracing::info!("LogStore: Command channel closed. Actor shutting down.");
                break;
            }
        }
    }
    tracing::info!("LogStore actor task shut down.");
    Ok(())
}

pub fn create_log_store_handle_and_task(
    settings: Arc<Settings>,
    buffer_size: usize,
    shutdown_rx: tokio::sync::watch::Receiver<bool>, // Added shutdown_rx argument
) -> (LogStoreHandle, tokio::task::JoinHandle<Result<(), AppError>>) {
    let (tx, rx_for_actor) = mpsc::channel(buffer_size);
    let handle = LogStoreHandle { tx };
    let task_settings = Arc::clone(&settings);
    let task = tokio::spawn(run_log_store_actor(task_settings, rx_for_actor, shutdown_rx));
    (handle, task)
}