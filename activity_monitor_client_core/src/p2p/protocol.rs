// src/p2p/protocol.rs

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite, prelude::*};
use libp2p::request_response::{self, InboundRequestId, OutboundRequestId};
use serde::{Deserialize, Serialize};
use std::io;

// --- Protocol Name String ---
// This is the actual string that will be used.
pub const LOG_SYNC_PROTOCOL_NAME_STR: &str = "/guikey_standalone/log_sync/1.0.0";

// --- Protocol Marker Type (needs AsRef<str>) ---
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LogSyncProtocol();

impl AsRef<str> for LogSyncProtocol {
    // Changed back to AsRef<str>
    fn as_ref(&self) -> &str {
        LOG_SYNC_PROTOCOL_NAME_STR
    }
}

// ... (rest of the file: LogBatchRequest, LogBatchResponse, LogSyncCodec remains the same) ...
// --- Request and Response Structures ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBatchRequest {
    pub app_client_id: String,
    pub encrypted_log_payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBatchResponse {
    pub status: String,
    pub message: String,
    pub events_processed: usize,
}

// --- Codec Implementation ---
#[derive(Clone, Default)]
pub struct LogSyncCodec;

#[async_trait]
impl request_response::Codec for LogSyncCodec {
    type Protocol = LogSyncProtocol; // This now implements AsRef<str>
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
        // tracing::trace!("Reading request with protocol: {}", protocol.as_ref());
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;

        if len > 10 * 1024 * 1024 {
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
        // tracing::trace!("Reading response with protocol: {}", protocol.as_ref());
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;

        if len > 1 * 1024 * 1024 {
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
        // tracing::trace!("Writing request with protocol: {}", protocol.as_ref());
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
        // tracing::trace!("Writing response with protocol: {}", protocol.as_ref());
        let buffer =
            serde_json::to_vec(&res).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let len = buffer.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        io.write_all(&buffer).await?;
        io.flush().await?;
        Ok(())
    }
}
