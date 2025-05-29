use crate::errors::ServerError; // Using the ServerError enum defined for this server project
use config::{Config, File as ConfigFile, Environment}; // ConfigFile alias for clarity
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// The primary Settings struct that the rest of the server application will use.
// It holds processed and validated configuration values.
#[derive(Debug, Clone)]
pub struct ServerSettings {
    pub listen_address: String,     // e.g., "127.0.0.1:8090"
    pub encryption_key: [u8; 32],   // The actual 32-byte AES key
    pub database_path: PathBuf,     // Absolute or relative path to the SQLite DB file
    pub log_retention_days: u32,    // How long to keep logs in the database
    // Add any other server-specific settings here, e.g.:
    // pub web_ui_title: String,
    // pub diagnostic_log_level: String,
}

// This struct maps directly to the fields expected in `local_server_config.toml`.
// It's used for deserializing the TOML file content.
#[derive(Debug, Deserialize)]
struct RawServerSettings {
    listen_address: String,
    encryption_key_hex: String, // Key is stored as a hex string in the config file
    database_path: String,      // Path as a string, will be converted to PathBuf
    log_retention_days: u32,
    // web_ui_title: Option<String>, // Example of an optional setting
}

impl ServerSettings {
    /// Loads configuration from `local_server_config.toml` (expected to be next to the executable)
    /// and environment variables, then processes them into the `ServerSettings` struct.
    pub fn new() -> Result<Arc<Self>, ServerError> {
        // Determine the directory where the executable is running.
        // Configuration file and database path will be relative to this directory.
        let exe_path = std::env::current_exe()
            .map_err(|e| ServerError::Config(format!("Failed to determine executable path: {}", e)))?;
        let exe_dir = exe_path.parent().ok_or_else(|| {
            ServerError::Config("Failed to determine executable directory.".to_string())
        })?;

        let config_file_name = "local_server_config.toml";
        let config_file_path = exe_dir.join(config_file_name);

        if !config_file_path.exists() {
            return Err(ServerError::Config(format!(
                "Configuration file '{}' not found in executable directory: {:?}",
                config_file_name, exe_dir
            )));
        }

        tracing::info!("Loading server configuration from: {:?}", config_file_path); // Requires tracing to be set up

        let builder = Config::builder()
            // Add configuration file source
            .add_source(ConfigFile::from(config_file_path).required(true))
            // Add environment variable source with a prefix
            .add_source(
                Environment::with_prefix("LOCAL_LOG_SERVER")
                    .separator("__") // e.g., LOCAL_LOG_SERVER__LISTEN_ADDRESS
                    .try_parsing(true), // Attempt to parse env vars to target types
            );

        // Build the configuration and deserialize into RawServerSettings
        let raw_settings: RawServerSettings = builder
            .build()
            .map_err(|e| ServerError::Config(format!("Failed to build server configuration: {}", e)))?
            .try_deserialize()
            .map_err(|e| ServerError::Config(format!("Failed to deserialize server configuration: {}", e)))?;

        // Process the encryption key from hex to bytes
        let key_bytes = hex::decode(&raw_settings.encryption_key_hex)
            .map_err(|e| ServerError::Config(format!("Invalid encryption_key_hex in config: {}. Ensure it's a 64-character hex string.", e)))?;
        if key_bytes.len() != 32 {
            return Err(ServerError::Config(
                "Decoded encryption key must be 32 bytes long.".to_string(),
            ));
        }
        let mut encryption_key = [0u8; 32];
        encryption_key.copy_from_slice(&key_bytes);

        // Construct the final, processed ServerSettings
        let settings = ServerSettings {
            listen_address: raw_settings.listen_address,
            encryption_key,
            // Resolve database_path relative to the executable directory
            database_path: exe_dir.join(raw_settings.database_path),
            log_retention_days: raw_settings.log_retention_days,
        };

        Ok(Arc::new(settings))
    }
}