// --- local_log_server/src/app_config.rs ---
use crate::errors::ServerError;
use config::{Config, File as ConfigFile, Environment};
use serde::Deserialize;
use std::path::{PathBuf};
use std::sync::Arc;
use libp2p::Multiaddr; // For P2P listen address, though not directly parsed here yet
use std::str::FromStr;


// Default interval for checking old logs to delete, in hours.
const DEFAULT_LOG_DELETION_CHECK_INTERVAL_HOURS: u64 = 24;

#[derive(Debug, Clone)]
pub struct ServerSettings {
    pub p2p_listen_address: Multiaddr, // Changed from String to Multiaddr
    pub web_ui_listen_address: String,
    pub server_identity_key_seed: [u8; 32], // Decoded binary seed
    pub encryption_key: [u8; 32], // For application-level data
    pub database_path: PathBuf,
    pub log_retention_days: u32,
    pub log_deletion_check_interval_hours: u64,
}

#[derive(Debug, Deserialize)]
struct RawServerSettings {
    listen_address: String, // Libp2p Multiaddress as string from TOML
    web_ui_listen_address: String,
    server_identity_key_seed_hex: String,
    encryption_key_hex: String,
    database_path: String,
    log_retention_days: u32,
    log_deletion_check_interval_hours: Option<u64>,
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

        tracing::info!("Server: Loading configuration from: {:?}", config_file_path);

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

        // Parse P2P Listen Address
        let p2p_listen_address = Multiaddr::from_str(&raw_settings.listen_address)
            .map_err(|e| ServerError::Config(format!("Invalid P2P listen_address in config: '{}'. Error: {}", raw_settings.listen_address, e)))?;

        // Decode server identity seed
        let seed_bytes = hex::decode(&raw_settings.server_identity_key_seed_hex)
            .map_err(|e| ServerError::Config(format!("Invalid server_identity_key_seed_hex: {}. Must be 64 hex chars.", e)))?;
        if seed_bytes.len() != 32 {
            return Err(ServerError::Config(
                "Decoded server identity seed must be 32 bytes long.".to_string(),
            ));
        }
        let mut server_identity_key_seed = [0u8; 32];
        server_identity_key_seed.copy_from_slice(&seed_bytes);

        // Decode app-level encryption key
        let app_key_bytes = hex::decode(&raw_settings.encryption_key_hex)
            .map_err(|e| ServerError::Config(format!("Invalid encryption_key_hex: {}. Must be 64 hex chars.", e)))?;
        if app_key_bytes.len() != 32 {
            return Err(ServerError::Config(
                "Decoded app-level encryption key must be 32 bytes long.".to_string(),
            ));
        }
        let mut encryption_key = [0u8; 32];
        encryption_key.copy_from_slice(&app_key_bytes);

        let settings = ServerSettings {
            p2p_listen_address,
            web_ui_listen_address: raw_settings.web_ui_listen_address,
            server_identity_key_seed,
            encryption_key,
            database_path: exe_dir.join(raw_settings.database_path),
            log_retention_days: raw_settings.log_retention_days,
            log_deletion_check_interval_hours: raw_settings
                .log_deletion_check_interval_hours
                .unwrap_or(DEFAULT_LOG_DELETION_CHECK_INTERVAL_HOURS),
        };

        Ok(Arc::new(settings))
    }
}