use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error (JSON): {0}")]
    SerializationJson(#[from] serde_json::Error),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Encryption error: {0}")]
    Encryption(String),
    #[error("Decryption error: {0}")]
    Decryption(String), // Though client mostly encrypts
    #[error("Windows API error: {context} (Code: {code})")]
    WinApi { context: String, code: u32 },
    #[error("Data storage error: {0}")]
    Storage(String),
    #[error("Hooking error: {0}")]
    Hook(String),
    #[error("Task join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("Channel send error (std::mpsc): {0}")]
    StdMpscSend(String),
    #[error("Channel send error (tokio::mpsc): {0}")]
    TokioMpscSend(String),
    #[error("Channel receive error (tokio::oneshot): {0}")]
    TokioOneshotRecv(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("Hex decoding error: {0}")]
    HexDecode(#[from] hex::FromHexError),
    #[error("Initialization failed: {0}")]
    Initialization(String),
    #[error("An unexpected error occurred: {0}")]
    Unknown(String),
}

pub fn win_api_error(context: &str) -> AppError {
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    AppError::WinApi {
        context: context.to_string(),
        code,
    }
}