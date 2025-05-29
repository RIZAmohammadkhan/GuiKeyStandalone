use crate::errors::ServerError;
use config::{Config, File as ConfigFile, Environment};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// Default interval for checking old logs to delete, in hours.
const DEFAULT_LOG_DELETION_CHECK_INTERVAL_HOURS: u64 = 24;

#[derive(Debug, Clone)]
pub struct ServerSettings {
    pub listen_address: String,
    pub encryption_key: [u8; 32],
    pub database_path: PathBuf,
    pub log_retention_days: u32,
    pub log_deletion_check_interval_hours: u64, // Added
}

#[derive(Debug, Deserialize)]
struct RawServerSettings {
    listen_address: String,
    encryption_key_hex: String,
    database_path: String,
    log_retention_days: u32,
    log_deletion_check_interval_hours: Option<u64>, // Added as Option
}

impl ServerSettings {
    pub fn new() -> Result<Arc<Self>, ServerError> {
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

        tracing::info!("Loading server configuration from: {:?}", config_file_path);

        let builder = Config::builder()
            .add_source(ConfigFile::from(config_file_path).required(true))
            .add_source(
                Environment::with_prefix("LOCAL_LOG_SERVER")
                    .separator("__")
                    .try_parsing(true),
            );

        let raw_settings: RawServerSettings = builder
            .build()
            .map_err(|e| ServerError::Config(format!("Failed to build server configuration: {}", e)))?
            .try_deserialize()
            .map_err(|e| ServerError::Config(format!("Failed to deserialize server configuration: {}", e)))?;

        let key_bytes = hex::decode(&raw_settings.encryption_key_hex)
            .map_err(|e| ServerError::Config(format!("Invalid encryption_key_hex in config: {}. Ensure it's a 64-character hex string.", e)))?;
        if key_bytes.len() != 32 {
            return Err(ServerError::Config(
                "Decoded encryption key must be 32 bytes long.".to_string(),
            ));
        }
        let mut encryption_key = [0u8; 32];
        encryption_key.copy_from_slice(&key_bytes);

        let settings = ServerSettings {
            listen_address: raw_settings.listen_address,
            encryption_key,
            database_path: exe_dir.join(raw_settings.database_path),
            log_retention_days: raw_settings.log_retention_days,
            log_deletion_check_interval_hours: raw_settings
                .log_deletion_check_interval_hours
                .unwrap_or(DEFAULT_LOG_DELETION_CHECK_INTERVAL_HOURS), // Use default if not present
        };

        Ok(Arc::new(settings))
    }
}