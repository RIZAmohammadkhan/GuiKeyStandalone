// src/services/sync_manager.rs

use crate::app_config::Settings;
use crate::errors::AppError;
use crate::storage::log_store::LogStoreHandle;
use crate::network::data_sender::DataSender;
use crate::network::encryption::encrypt_payload; // Assuming this is in crate::network::encryption
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, MissedTickBehavior}; // Added MissedTickBehavior
use uuid::Uuid; // For LogEvent IDs

pub async fn run_sync_manager(
    settings: Arc<Settings>,
    log_store: LogStoreHandle,
    data_sender: DataSender,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>, // For graceful shutdown
) -> Result<(), AppError> {
    tracing::info!(
        "SyncManager: Started. Sync interval: {}s, Retry interval: {}s",
        settings.sync_interval,
        settings.retry_interval_on_fail
    );

    let mut interval_timer = tokio::time::interval(Duration::from_secs(settings.sync_interval));
    interval_timer.set_missed_tick_behavior(MissedTickBehavior::Delay); // Or Skip
    // interval_timer.tick().await; // Consume initial immediate tick if not desired on startup

    loop {
        let mut perform_sync_now = false;
        let mut shutdown_requested = *shutdown_rx.borrow();

        if shutdown_requested {
            tracing::info!("SyncManager: Shutdown signal received, attempting one final sync.");
            perform_sync_now = true;
        } else {
            tokio::select! {
                biased; // Prioritize shutdown signal

                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow_and_update() { // Consume the change
                        tracing::info!("SyncManager: Shutdown signal updated, attempting final sync.");
                        shutdown_requested = true;
                        perform_sync_now = true;
                    } else {
                        // Spurious wakeup or signal toggled back, continue normal interval.
                        continue; // Re-evaluate select
                    }
                }
                _ = interval_timer.tick() => {
                    tracing::debug!("SyncManager: Interval tick for sync.");
                    perform_sync_now = true;
                }
            };
        }
        
        if perform_sync_now {
            tracing::info!("SyncManager: Checking for logs to sync...");
            match log_store.get_batch_for_sync(settings.max_events_per_sync_batch).await {
                Ok(events_batch) if !events_batch.is_empty() => {
                    let batch_size = events_batch.len();
                    let batch_event_ids: Vec<Uuid> = events_batch.iter().map(|e| e.id).collect();
                    tracing::info!("SyncManager: Found {} events in batch for sync. First ID: {:?}", batch_size, batch_event_ids.first());

                    match serde_json::to_vec(&events_batch) {
                        Ok(serialized_data) => {
                            match encrypt_payload(&serialized_data, &settings.encryption_key) {
                                Ok(encrypted_payload) => {
                                    let mut attempts = 0;
                                    loop { // Retry loop for sending this specific batch
                                        attempts += 1;
                                        match data_sender.send_log_batch(encrypted_payload.clone()).await {
                                            Ok(_) => {
                                                tracing::info!("SyncManager: Batch of {} events synced successfully (attempt {}).", batch_size, attempts);
                                                if let Err(e) = log_store.confirm_events_synced(batch_event_ids.clone()).await {
                                                    tracing::error!(
                                                        "SyncManager: CRITICAL - Failed to confirm sync for batch {:?}: {}. Data may be resent.",
                                                        batch_event_ids.first(), e
                                                    );
                                                }
                                                break; // Break from retry loop
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "SyncManager: Failed to send batch (attempt {}/{}): {}",
                                                    attempts, settings.max_retries_per_batch, e
                                                );
                                                if attempts >= settings.max_retries_per_batch || shutdown_requested {
                                                    tracing::error!(
                                                        "SyncManager: Max retries ({}) reached or shutdown requested for batch {:?}. Batch remains in store.",
                                                        settings.max_retries_per_batch, batch_event_ids.first()
                                                    );
                                                    break; // Break from retry loop, batch remains in store
                                                }
                                                // Simple fixed retry interval, could be exponential backoff
                                                sleep(Duration::from_secs(settings.retry_interval_on_fail)).await;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("SyncManager: Failed to encrypt batch: {}. Batch will be retried later.", e);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("SyncManager: Failed to serialize batch for encryption: {}. Batch will be retried later.", e);
                        }
                    }
                }
                Ok(_) => { // Batch was empty
                    tracing::info!("SyncManager: No new events to sync.");
                }
                Err(e) => {
                    tracing::error!("SyncManager: Failed to get batch from log store: {}. Retrying after interval.", e);
                }
            }
        } // end if perform_sync_now

        if shutdown_requested {
            tracing::info!("SyncManager: Finished final sync attempt due to shutdown signal.");
            break; // Exit the main sync loop
        }
    }
    tracing::info!("SyncManager shut down.");
    Ok(())
}