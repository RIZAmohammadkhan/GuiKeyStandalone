// --- local_log_server/src/p2p/protocol.rs ---
// This file is identical to activity_monitor_client_core/src/p2p/protocol.rs
// Ensure you copy the exact content. For brevity, I'll paste it here.

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite, prelude::*};
use libp2p::request_response::{self}; // Removed OutboundRequestId, InboundRequestId as not directly used
use serde::{Deserialize, Serialize};
use std::io;

pub const LOG_SYNC_PROTOCOL_NAME_STR: &str = "/guikey_standalone/log_sync/1.0.0";

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LogSyncProtocol();

impl AsRef<str> for LogSyncProtocol {
    fn as_ref(&self) -> &str {
        LOG_SYNC_PROTOCOL_NAME_STR
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBatchRequest {
    pub app_client_id: String, // This is the application-level UUID of the client
    pub encrypted_log_payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBatchResponse {
    pub status: String,          // e.g., "success", "error"
    pub message: String,         // Detailed message, especially on error
    pub events_processed: usize, // Number of LogEvent items processed from the batch
}

#[derive(Clone, Default)]
pub struct LogSyncCodec;

#[async_trait]
impl request_response::Codec for LogSyncCodec {
    type Protocol = LogSyncProtocol;
    type Request = LogBatchRequest;
    type Response = LogBatchResponse;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;

        if len > 10 * 1024 * 1024 {
            // Max 10MB request
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Request too large",
            ));
        }

        let mut buffer = vec![0u8; len];
        io.read_exact(&mut buffer).await?;

        serde_json::from_slice(&buffer).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;

        if len > 1 * 1024 * 1024 {
            // Max 1MB response
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Response too large",
            ));
        }

        let mut buffer = vec![0u8; len];
        io.read_exact(&mut buffer).await?;

        serde_json::from_slice(&buffer).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let buffer =
            serde_json::to_vec(&req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let len = buffer.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        io.write_all(&buffer).await?;
        io.flush().await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let buffer =
            serde_json::to_vec(&res).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let len = buffer.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        io.write_all(&buffer).await?;
        io.flush().await?;
        Ok(())
    }
}
