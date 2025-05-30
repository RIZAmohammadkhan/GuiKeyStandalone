// src/p2p/data_sender.rs

use crate::app_config::Settings;
use crate::errors::AppError;
use crate::p2p::{
    protocol::{LogBatchRequest, LogBatchResponse},
    swarm_manager::SwarmCommand,
};
// use libp2p::PeerId; // Not directly needed here if settings has it
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct P2pDataSender {
    settings: Arc<Settings>,
    command_tx: mpsc::Sender<SwarmCommand>,
}

impl P2pDataSender {
    pub fn new(settings: Arc<Settings>, command_tx: mpsc::Sender<SwarmCommand>) -> Self {
        Self {
            settings,
            command_tx,
        }
    }

    pub async fn send_log_batch(
        &self,
        app_client_id_str: String,
        encrypted_log_payload: Vec<u8>,
    ) -> Result<LogBatchResponse, AppError> {
        tracing::info!(
            "P2pDataSender: Preparing to send log batch of {} bytes to server PeerId: {}",
            encrypted_log_payload.len(),
            self.settings.server_peer_id
        );

        let request = LogBatchRequest {
            app_client_id: app_client_id_str,
            encrypted_log_payload,
        };

        let (response_tx, response_rx) = oneshot::channel();

        let command = SwarmCommand::SendLogBatch {
            target_peer_id: self.settings.server_peer_id,
            request,
            responder: response_tx,
        };

        if self.command_tx.send(command).await.is_err() {
            tracing::error!(
                "P2pDataSender: Failed to send command to SwarmManager. Channel closed."
            );
            return Err(AppError::Internal(
                // Changed to Internal
                "P2P command channel closed".to_string(),
            ));
        }

        match tokio::time::timeout(Duration::from_secs(60), response_rx).await {
            Ok(Ok(Ok(response))) => {
                tracing::info!(
                    "P2pDataSender: Successfully sent batch. Server response: status='{}', msg='{}', processed={}",
                    response.status,
                    response.message,
                    response.events_processed
                );
                Ok(response)
            }
            Ok(Ok(Err(app_error))) => {
                tracing::error!("P2pDataSender: P2P request failed: {}", app_error);
                Err(app_error)
            }
            Ok(Err(_oneshot_cancelled_err)) => {
                tracing::error!("P2pDataSender: P2P response channel cancelled by SwarmManager.");
                Err(AppError::Internal(
                    // Changed to Internal
                    "P2P response channel cancelled".to_string(),
                ))
            }
            Err(_timeout_err) => {
                tracing::error!("P2pDataSender: P2P request timed out while waiting for response.");
                Err(AppError::P2pOperation("Request timed out".to_string())) // Use new P2pOperation error
            }
        }
    }
}
