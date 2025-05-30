use crate::app_config::Settings;
use crate::errors::AppError;
use crate::event_types::LogEvent;
use chrono::Utc;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, ErrorKind, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, MissedTickBehavior, interval};
use uuid::Uuid;

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
        self.tx
            .send(LogStoreCommand::AddEvent(event, resp_tx))
            .await
            .map_err(|e| {
                AppError::TokioMpscSend(format!("LogStore add_event send failed: {}", e))
            })?;
        resp_rx.await.map_err(AppError::TokioOneshotRecv)?
    }

    pub async fn get_batch_for_sync(&self, limit: usize) -> Result<Vec<LogEvent>, AppError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(LogStoreCommand::GetBatch(limit, resp_tx))
            .await
            .map_err(|e| {
                AppError::TokioMpscSend(format!("LogStore get_batch send failed: {}", e))
            })?;
        resp_rx.await.map_err(AppError::TokioOneshotRecv)?
    }

    pub async fn confirm_events_synced(&self, ids: Vec<Uuid>) -> Result<(), AppError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(LogStoreCommand::ConfirmSync(ids, resp_tx))
            .await
            .map_err(|e| {
                AppError::TokioMpscSend(format!("LogStore confirm_sync send failed: {}", e))
            })?;
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
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    AppError::Initialization(format!(
                        "Failed to create log directory {:?}: {}",
                        parent, e
                    ))
                })?;
            }
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        tracing::info!("LogStoreActor initialized. Storage file: {:?}", file_path);
        Ok(Self {
            settings,
            file_path,
        })
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

    fn deserialize_line(line: &str, line_num: usize) -> Option<LogEvent> {
        if line.trim().is_empty() {
            return None;
        }
        match serde_json::from_str::<LogEvent>(line) {
            Ok(event) => Some(event),
            Err(e) => {
                tracing::warn!(
                    "LogStore: Failed to deserialize event from log store file at line {}: {}. Line snippet: '{}'",
                    line_num,
                    e,
                    line.chars().take(100).collect::<String>()
                );
                None
            }
        }
    }

    fn write_event_to_file(&mut self, event: &LogEvent) -> Result<(), AppError> {
        if let Some(max_size_mb) = self.settings.max_log_file_size_mb {
            match std::fs::metadata(&self.file_path) {
                Ok(metadata) => {
                    let max_size_bytes = max_size_mb * 1024 * 1024;
                    if metadata.len() > max_size_bytes {
                        let is_stuck = match self.read_batch_from_file(1) {
                            Ok(batch) => batch.is_empty(),
                            Err(_) => true,
                        };

                        if is_stuck && metadata.len() > (max_size_bytes as f64 * 1.1) as u64 {
                            tracing::error!(
                                "LogStore: Log file {:?} (size {}B) exceeds max size ({}MB) and appears stuck. \
                                Halting writes to prevent disk exhaustion. Event ID {:?} will NOT be written.",
                                self.file_path,
                                metadata.len(),
                                max_size_mb,
                                event.id
                            );
                            return Err(AppError::Storage(format!(
                                "Log file full ({}MB limit) and not shrinking. Halting writes.",
                                max_size_mb
                            )));
                        } else if metadata.len() > max_size_bytes {
                            tracing::warn!(
                                "LogStore: Log file {:?} (size {}B) exceeds max size ({}MB). Will attempt to write event ID {:?}. \
                               Sync process should clear space soon.",
                                self.file_path,
                                metadata.len(),
                                max_size_mb,
                                event.id
                            );
                        }
                    }
                }
                Err(e) if e.kind() == ErrorKind::NotFound => { /* File doesn't exist yet, will be created */
                }
                Err(e) => {
                    tracing::warn!(
                        "LogStore: Could not get metadata for log file {:?}: {}. Proceeding with write.",
                        self.file_path,
                        e
                    );
                }
            }
        }

        if let Some(parent_dir) = self.file_path.parent() {
            if !parent_dir.exists() {
                fs::create_dir_all(parent_dir).map_err(|e| {
                    AppError::Storage(format!(
                        "Failed to create log directory {:?}: {}",
                        parent_dir, e
                    ))
                })?;
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        let json_event = serde_json::to_string(event)?;
        writeln!(file, "{}", json_event)?;
        tracing::trace!(
            "LogStore: Event {:?} written to log store file {:?}",
            event.id,
            self.file_path
        );
        Ok(())
    }

    fn read_batch_from_file(&self, limit: usize) -> Result<Vec<LogEvent>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let file = match File::open(&self.file_path) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(AppError::Io(e)),
        };
        let reader = BufReader::new(file);
        let mut batch = Vec::with_capacity(std::cmp::min(limit, 1000));

        for (idx, line_res) in reader.lines().enumerate() {
            if batch.len() >= limit {
                break;
            }
            let line = line_res?;
            if let Some(event) = Self::deserialize_line(&line, idx + 1) {
                batch.push(event);
            }
        }
        tracing::debug!(
            "LogStore: Read {} events for batch (limit {}) from log store file {:?}.",
            batch.len(),
            limit,
            self.file_path
        );
        Ok(batch)
    }

    fn remove_events_from_file(&mut self, ids_to_remove: &[Uuid]) -> Result<(), AppError> {
        if ids_to_remove.is_empty() {
            tracing::debug!("LogStore: remove_events_from_file called with no IDs to remove.");
            return Ok(());
        }

        let parent_dir = self
            .file_path
            .parent()
            .ok_or_else(|| AppError::Storage("Log file path has no parent.".to_string()))?;
        let temp_file = NamedTempFile::new_in(parent_dir)?;

        let mut removed_count = 0;
        let mut lines_kept = 0;

        let original_file = match File::open(&self.file_path) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tracing::warn!(
                    "LogStore: Original log file {:?} not found during remove_events_from_file. Nothing to remove.",
                    self.file_path
                );
                return Ok(());
            }
            Err(e) => return Err(AppError::Io(e)),
        };
        let reader = BufReader::new(original_file);
        let mut writer = BufWriter::new(File::create(temp_file.path())?);

        for (idx, line_res) in reader.lines().enumerate() {
            let line_num = idx + 1;
            let line = line_res?;

            if let Some(event) = Self::deserialize_line(&line, line_num) {
                if ids_to_remove.contains(&event.id) {
                    removed_count += 1;
                } else {
                    writeln!(writer, "{}", line)?;
                    lines_kept += 1;
                }
            } else {
                if !line.trim().is_empty() {
                    writeln!(writer, "{}", line)?;
                    lines_kept += 1;
                    tracing::warn!(
                        "LogStore: Kept an unparseable line (line {}) during rewrite as its ID could not be checked.",
                        line_num
                    );
                }
            }
        }
        writer.flush()?;
        drop(writer);

        temp_file.persist(&self.file_path).map_err(|e| {
            AppError::Storage(format!("Failed to persist temp log file over original: {}. Original path: {:?}, Temp path: {:?}", e.error, self.file_path, e.file.path()))
        })?;

        tracing::info!(
            "LogStore: Events removal complete. IDs to remove: {}. Actual removed: {}. Lines kept: {}. File: {:?}",
            ids_to_remove.len(),
            removed_count,
            lines_kept,
            self.file_path
        );
        Ok(())
    }

    async fn periodic_cleanup(&mut self) {
        if self.settings.local_log_cache_retention_days == 0 {
            tracing::debug!("LogStore: Periodic cleanup disabled (retention_days = 0).");
            return;
        }
        let retention_duration =
            chrono::Duration::days(self.settings.local_log_cache_retention_days as i64);
        let cutoff_time = Utc::now() - retention_duration;
        tracing::info!(
            "LogStore: Running periodic cleanup for logs older than {} days (cutoff: {}). File: {:?}",
            self.settings.local_log_cache_retention_days,
            cutoff_time,
            self.file_path
        );

        let parent_dir = match self.file_path.parent() {
            Some(p) => p,
            None => {
                tracing::error!(
                    "LogStore: Cleanup failed - Log file path has no parent: {:?}",
                    self.file_path
                );
                return;
            }
        };
        let temp_file = match NamedTempFile::new_in(parent_dir) {
            Ok(tf) => tf,
            Err(e) => {
                tracing::error!(
                    "LogStore: Cleanup failed - Could not create temp file: {}",
                    e
                );
                return;
            }
        };
        let temp_file_path_for_log = temp_file.path().to_path_buf();

        let mut removed_count = 0;
        let mut lines_kept = 0;

        let original_file = match File::open(&self.file_path) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tracing::debug!(
                    "LogStore: Cleanup - Original log file {:?} not found. Nothing to clean.",
                    self.file_path
                );
                return;
            }
            Err(e) => {
                tracing::error!(
                    "LogStore: Cleanup failed - Could not open original log file {:?}: {}",
                    self.file_path,
                    e
                );
                return;
            }
        };
        let reader = BufReader::new(original_file);

        // CORRECTED PART: Handle Result from File::create
        let mut writer = match File::create(temp_file.path()) {
            Ok(f) => BufWriter::new(f),
            Err(e) => {
                tracing::error!(
                    "LogStore: Cleanup failed - Could not create writer for temp file {:?}: {}",
                    temp_file.path(),
                    e
                );
                return;
            }
        };

        for (idx, line_res) in reader.lines().enumerate() {
            let line_num = idx + 1;
            match line_res {
                Ok(line) => {
                    if let Some(event) = Self::deserialize_line(&line, line_num) {
                        if event.timestamp >= cutoff_time {
                            if let Err(e) = writeln!(writer, "{}", line) {
                                tracing::error!(
                                    "LogStore: Cleanup failed - Error writing kept event to temp file: {}",
                                    e
                                );
                                return;
                            }
                            lines_kept += 1;
                        } else {
                            tracing::trace!(
                                "LogStore: Cleaning up old event ID {:?}, timestamp {}",
                                event.id,
                                event.timestamp
                            );
                            removed_count += 1;
                        }
                    } else {
                        if !line.trim().is_empty() {
                            if let Err(e) = writeln!(writer, "{}", line) {
                                tracing::error!(
                                    "LogStore: Cleanup failed - Error writing unparseable (but kept) line to temp file: {}",
                                    e
                                );
                                return;
                            }
                            lines_kept += 1;
                            tracing::warn!(
                                "LogStore: Cleanup - Kept an unparseable line (line {}) during rewrite.",
                                line_num
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "LogStore: Cleanup failed - Error reading line {} from original log file: {}",
                        line_num,
                        e
                    );
                    return;
                }
            }
        }

        if let Err(e) = writer.flush() {
            tracing::error!("LogStore: Cleanup failed - Error flushing temp file: {}", e);
            return;
        }
        drop(writer);

        if removed_count > 0
            || (lines_kept > 0 && removed_count == 0)
            || (lines_kept == 0
                && removed_count == 0
                && fs::metadata(&self.file_path)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false))
        {
            match temp_file.persist(&self.file_path) {
                Ok(_) => {
                    tracing::info!(
                        "LogStore: Periodic cleanup successful. Removed: {}. Kept: {}. File: {:?}",
                        removed_count,
                        lines_kept,
                        self.file_path
                    );
                }
                Err(e) => {
                    tracing::error!(
                        "LogStore: Cleanup failed - Could not persist temp file over original: {}. Original path: {:?}, Temp path: {:?}. Data may be in temp file.",
                        e.error,
                        self.file_path,
                        temp_file_path_for_log
                    );
                }
            }
        } else {
            tracing::debug!(
                "LogStore: Periodic cleanup resulted in no changes to file content (original was empty or no events expired/were removed). Temp file {:?} will be removed.",
                temp_file_path_for_log
            );
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

    let mut cleanup_interval_opt = if actor.settings.local_log_cache_retention_days > 0 {
        let initial_delay = Duration::from_secs(60);
        let periodic_interval_duration = Duration::from_secs(60 * 60 * 6); // Every 6 hours

        let stream = async_stream::stream! {
            tokio::time::sleep(initial_delay).await;
            yield ();

            let mut periodic_timer = interval(periodic_interval_duration);
            periodic_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                periodic_timer.tick().await;
                yield ();
            }
        };
        Some(Box::pin(stream))
    } else {
        None
    };

    loop {
        let cleanup_fut = async {
            if let Some(ref mut interval_pinned_stream) = cleanup_interval_opt {
                use futures::StreamExt;
                interval_pinned_stream.next().await;
                return Some(());
            }
            std::future::pending::<Option<()>>().await
        };

        tokio::select! {
            biased;

            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow_and_update() {
                    tracing::info!("LogStore: Shutdown signal received.");
                    break;
                }
            }

            tick_result = cleanup_fut => {
                if tick_result.is_some() {
                    actor.periodic_cleanup().await;
                }
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
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> (
    LogStoreHandle,
    tokio::task::JoinHandle<Result<(), AppError>>,
) {
    let (tx, rx_for_actor) = mpsc::channel(buffer_size);
    let handle = LogStoreHandle { tx };
    let task_settings = Arc::clone(&settings);
    let task = tokio::spawn(run_log_store_actor(
        task_settings,
        rx_for_actor,
        shutdown_rx,
    ));
    (handle, task)
}
