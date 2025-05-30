// src/services/sync_manager.rs

use crate::app_config::Settings;
use crate::errors::AppError;
use crate::network::encryption::encrypt_payload; // Still used for app-level encryption
use crate::p2p::data_sender::P2pDataSender; // The new P2P data sender
use crate::p2p::protocol::LogBatchResponse; // The response type from P2P
use crate::storage::log_store::LogStoreHandle;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, sleep};
use uuid::Uuid;

pub async fn run_sync_manager(
    settings: Arc<Settings>,
    log_store: LogStoreHandle,
    p2p_data_sender: P2pDataSender, // Changed from DataSender to P2pDataSender
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), AppError> {
    tracing::info!(
        "SyncManager: Started. Sync interval: {}s, Retry interval for P2P send: {}s",
        settings.sync_interval,
        settings.retry_interval_on_fail // This retry is now for the P2P send attempt itself
    );

    let mut interval_timer = tokio::time::interval(Duration::from_secs(settings.sync_interval));
    interval_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        let mut perform_sync_now = false;
        let mut shutdown_requested = *shutdown_rx.borrow();

        if shutdown_requested {
            tracing::info!("SyncManager: Shutdown signal received, attempting one final sync.");
            perform_sync_now = true;
        } else {
            tokio::select! {
                biased;

                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow_and_update() {
                        tracing::info!("SyncManager: Shutdown signal updated, attempting final sync.");
                        shutdown_requested = true;
                        perform_sync_now = true;
                    } else {
                        continue;
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
            match log_store
                .get_batch_for_sync(settings.max_events_per_sync_batch)
                .await
            {
                Ok(events_batch) if !events_batch.is_empty() => {
                    let batch_size = events_batch.len();
                    let batch_event_ids: Vec<Uuid> = events_batch.iter().map(|e| e.id).collect();
                    tracing::info!(
                        "SyncManager: Found {} events in batch for sync. First ID: {:?}",
                        batch_size,
                        batch_event_ids.first()
                    );

                    // 1. Serialize the batch of LogEvent objects to JSON
                    match serde_json::to_vec(&events_batch) {
                        Ok(serialized_data) => {
                            // 2. Encrypt the JSON payload using the app-level AES key
                            match encrypt_payload(&serialized_data, &settings.encryption_key) {
                                Ok(encrypted_app_payload) => {
                                    let mut attempts = 0;
                                    loop {
                                        // Retry loop for sending this specific batch via P2P
                                        attempts += 1;
                                        tracing::debug!(
                                            "SyncManager: Attempting to send batch (attempt {}/{}) via P2P.",
                                            attempts,
                                            settings.max_retries_per_batch
                                        );

                                        // 3. Send via P2pDataSender
                                        // The app_client_id (UUID) is taken from settings.
                                        match p2p_data_sender
                                            .send_log_batch(
                                                settings.client_id.to_string(), // Pass app-level client ID
                                                encrypted_app_payload.clone(),  // Clone if retrying
                                            )
                                            .await
                                        {
                                            Ok(log_batch_response) => {
                                                if log_batch_response.status == "success" {
                                                    tracing::info!(
                                                        "SyncManager: Batch of {} events synced successfully via P2P (attempt {}). Server processed {} events. Msg: {}",
                                                        batch_size,
                                                        attempts,
                                                        log_batch_response.events_processed,
                                                        log_batch_response.message
                                                    );
                                                    // 4. Confirm sync with LogStore
                                                    if let Err(e) = log_store
                                                        .confirm_events_synced(
                                                            batch_event_ids.clone(),
                                                        )
                                                        .await
                                                    {
                                                        tracing::error!(
                                                            "SyncManager: CRITICAL - Failed to confirm P2P sync for batch {:?}: {}. Data may be resent.",
                                                            batch_event_ids.first(),
                                                            e
                                                        );
                                                    }
                                                    // TODO: Potentially check if log_batch_response.events_processed matches batch_size.
                                                    // If not, it might indicate partial processing on server, though our current protocol implies all or nothing.
                                                } else {
                                                    // Server responded but indicated an issue.
                                                    tracing::error!(
                                                        "SyncManager: Server responded to P2P log submission with non-success: status='{}', message='{}' (attempt {}). Batch remains in store.",
                                                        log_batch_response.status,
                                                        log_batch_response.message,
                                                        attempts
                                                    );
                                                    // Treat as a failure for retry purposes, but don't infinitely retry if server keeps saying "error".
                                                    // This might need more nuanced handling based on server error types.
                                                    if attempts >= settings.max_retries_per_batch
                                                        || shutdown_requested
                                                    {
                                                        break; // Break from retry loop
                                                    }
                                                    sleep(Duration::from_secs(
                                                        settings.retry_interval_on_fail,
                                                    ))
                                                    .await;
                                                    continue; // Continue to next attempt
                                                }
                                                break; // Break from retry loop on successful processing or server-side logical error
                                            }
                                            Err(e) => {
                                                // Network-level or P2P internal error from P2pDataSender
                                                tracing::warn!(
                                                    "SyncManager: P2P send_log_batch failed (attempt {}/{}): {}",
                                                    attempts,
                                                    settings.max_retries_per_batch,
                                                    e
                                                );
                                                if attempts >= settings.max_retries_per_batch
                                                    || shutdown_requested
                                                {
                                                    tracing::error!(
                                                        "SyncManager: Max P2P send retries ({}) reached or shutdown requested for batch {:?}. Batch remains in store.",
                                                        settings.max_retries_per_batch,
                                                        batch_event_ids.first()
                                                    );
                                                    break; // Break from retry loop
                                                }
                                                sleep(Duration::from_secs(
                                                    settings.retry_interval_on_fail,
                                                ))
                                                .await;
                                                // Continue to next attempt in the loop
                                            }
                                        }
                                    } // End of retry loop
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "SyncManager: Failed to encrypt batch for P2P sending: {}. Batch will be retried later.",
                                        e
                                    );
                                    // No P2P send attempt, batch remains.
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "SyncManager: Failed to serialize batch for encryption: {}. Batch will be retried later.",
                                e
                            );
                            // No encryption or P2P send attempt, batch remains.
                        }
                    }
                }
                Ok(_) => {
                    // Batch was empty
                    tracing::info!("SyncManager: No new events to sync.");
                }
                Err(e) => {
                    tracing::error!(
                        "SyncManager: Failed to get batch from log store: {}. Retrying after interval.",
                        e
                    );
                }
            }
        }

        if shutdown_requested {
            tracing::info!("SyncManager: Finished final sync attempt due to shutdown signal.");
            break;
        }
    }
    tracing::info!("SyncManager shut down.");
    Ok(())
}
