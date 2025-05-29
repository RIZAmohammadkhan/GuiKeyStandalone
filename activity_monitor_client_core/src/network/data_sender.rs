// src/network/data_sender.rs

use crate::app_config::Settings;
use crate::errors::AppError;
use reqwest::{Client, Body};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)] // Cloneable if DataSender needs to be shared (e.g. Arc<DataSender>)
pub struct DataSender {
    client: Client,
    settings: Arc<Settings>, // Store Arc<Settings> for access to server_url, client_id etc.
}

impl DataSender {
    pub fn new(settings: Arc<Settings>) -> Result<Self, AppError> {
        // Consider more robust timeout configurations
        let client = Client::builder()
            .timeout(Duration::from_secs(60)) // Overall request timeout
            .connect_timeout(Duration::from_secs(20)) // Connection phase timeout
            .user_agent(format!(
                "{}/{} (RustMonitorClient)", // Use a more descriptive user agent
                settings.app_name_for_autorun, // Assuming this is a good app identifier
                env!("CARGO_PKG_VERSION")
            ))
            .use_rustls_tls() // Prefer Rustls for TLS backend
            .build()?; // Converts reqwest::Error into AppError::Network via From trait
        Ok(Self { client, settings })
    }

    pub async fn send_log_batch(&self, encrypted_payload: Vec<u8>) -> Result<(), AppError> {
        tracing::info!(
            "DataSender: Sending log batch of {} bytes to {}",
            encrypted_payload.len(),
            self.settings.server_url // Access server_url from stored settings
        );
        
        let response_result = self.client
            .post(&self.settings.server_url)
            .header("Content-Type", "application/octet-stream")
            .header("X-Client-ID", self.settings.client_id.to_string()) // Send client_id
            .body(Body::from(encrypted_payload)) // reqwest::Body can take Vec<u8>
            .send()
            .await;

        match response_result {
            Ok(response) => {
                // error_for_status will turn 4xx and 5xx responses into an Err
                match response.error_for_status() {
                    Ok(successful_response) => {
                        tracing::info!(
                            "DataSender: Log batch sent successfully. Server status: {}",
                            successful_response.status()
                        );
                        // Optionally log response body if needed for debugging (usually small for success)
                        // let response_text = successful_response.text().await.unwrap_or_default();
                        // tracing::debug!("DataSender: Server success response body: {}", response_text);
                        Ok(())
                    }
                    Err(reqwest_error_with_status) => {
                        // This error already contains status and potentially the response body if read
                        tracing::error!("DataSender: Server responded with HTTP error: {}", reqwest_error_with_status);
                        Err(AppError::Network(reqwest_error_with_status))
                    }
                }
            }
            Err(e) => { // Network-level error (DNS, connection refused, etc.)
                tracing::error!("DataSender: HTTP request to send logs failed: {}", e);
                Err(AppError::Network(e))
            }
        }
    }
}