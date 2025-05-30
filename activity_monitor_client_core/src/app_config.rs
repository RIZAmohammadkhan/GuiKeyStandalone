use crate::errors::AppError;
use config::{Config, Environment, File as ConfigFile};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

// libp2p specific imports
use libp2p::{Multiaddr, PeerId};

#[derive(Debug, Clone)]
pub struct Settings {
    // Libp2p specific
    pub server_peer_id: PeerId,
    pub bootstrap_addresses: Vec<Multiaddr>,

    // Application specific
    pub encryption_key: [u8; 32], // For app-level payload encryption
    pub client_id: Uuid,          // App-level client identifier

    // Syncing and retry logic (may apply to P2P sends too)
    pub sync_interval: u64,          // seconds
    pub retry_interval_on_fail: u64, // seconds
    pub max_retries_per_batch: u32,

    // Event processing
    pub processor_periodic_flush_interval_secs: u64, // seconds

    // Local storage for logs
    pub log_file_path: PathBuf,
    pub max_log_file_size_mb: Option<u64>,
    pub max_events_per_sync_batch: usize,
    pub local_log_cache_retention_days: u32,

    // Application behavior
    pub app_name_for_autorun: String,
    pub internal_log_level: String,
    pub internal_log_file_dir: PathBuf,
    pub internal_log_file_name: String,
    pub client_id_file_path: Option<PathBuf>, // For persisting app-level client_id
}

#[derive(Debug, Deserialize)]
struct RawSettings {
    // Libp2p specific from config file
    server_peer_id: String,
    bootstrap_addresses: Vec<String>, // Read as strings first

    // Application specific from config file
    encryption_key_hex: String,
    client_id: Option<String>, // App-level client_id

    sync_interval: u64,
    retry_interval_on_fail: u64,
    max_retries_per_batch: u32,

    processor_periodic_flush_interval_secs: u64,

    log_file_path: String,
    max_log_file_size_mb: Option<u64>,
    max_events_per_sync_batch: usize,
    local_log_cache_retention_days: Option<u32>,

    app_name_for_autorun: String,
    internal_log_level: String,
    internal_log_file_dir: String,
    internal_log_file_name: String,
    client_id_file: Option<String>,
}

impl Settings {
    pub fn new() -> Result<Arc<Self>, AppError> {
        let exe_path = std::env::current_exe()
            .map_err(|e| AppError::Config(format!("Failed to get current exe path: {}", e)))?;
        let exe_dir = exe_path.parent().ok_or_else(|| {
            AppError::Config("Failed to get parent directory of executable.".to_string())
        })?;

        let config_paths_to_try = [
            exe_dir.join("config").join("client_settings.toml"),
            exe_dir.join("client_settings.toml"),
            PathBuf::from("config").join("client_settings.toml"), // Relative to CWD for dev
            PathBuf::from("client_settings.toml"),                // Relative to CWD for dev
        ];

        let mut config_builder = Config::builder();
        let mut loaded_from_file = false;

        for path_to_try in &config_paths_to_try {
            if path_to_try.exists() {
                config_builder =
                    config_builder.add_source(ConfigFile::from(path_to_try.clone()).required(true));
                loaded_from_file = true;
                // Use tracing here once it's initialized, or println for early config phase
                println!(
                    "[INFO] Client: Loading configuration from: {:?}",
                    path_to_try
                );
                break;
            }
        }

        if !loaded_from_file {
            return Err(AppError::Config(
                "client_settings.toml not found in standard locations.".to_string(),
            ));
        }

        config_builder = config_builder.add_source(
            Environment::with_prefix("AMS_CLIENT")
                .separator("__")
                .try_parsing(true),
        );

        let raw_settings: RawSettings = config_builder
            .build()
            .map_err(|e| AppError::Config(format!("Failed to build configuration: {}", e)))?
            .try_deserialize()
            .map_err(|e| AppError::Config(format!("Failed to deserialize configuration: {}", e)))?;

        // Process app-level encryption key
        let key_bytes =
            hex::decode(&raw_settings.encryption_key_hex).map_err(AppError::HexDecode)?;
        if key_bytes.len() != 32 {
            return Err(AppError::Config(
                "App-level encryption key must be 32 bytes (64 hex characters).".to_string(),
            ));
        }
        let mut encryption_key = [0u8; 32];
        encryption_key.copy_from_slice(&key_bytes);

        // Process libp2p server_peer_id
        let server_peer_id = PeerId::from_str(&raw_settings.server_peer_id).map_err(|e| {
            AppError::Config(format!(
                "Invalid server_peer_id in config: '{}'. Error: {}",
                raw_settings.server_peer_id, e
            ))
        })?;

        // Process libp2p bootstrap_addresses
        let bootstrap_addresses: Vec<Multiaddr> = raw_settings
            .bootstrap_addresses
            .iter()
            .map(|addr_str| {
                Multiaddr::from_str(addr_str).map_err(|e| {
                    AppError::Config(format!(
                        "Invalid bootstrap multiaddress in config: '{}'. Error: {}",
                        addr_str, e
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        if bootstrap_addresses.is_empty() {
            // This might be an error condition depending on your discovery strategy
            // For now, just a warning. Kademlia might still work if it can find other peers.
            println!(
                "[WARN] Client: No valid bootstrap addresses configured. P2P discovery might be impaired."
            );
        }

        // Determine client_id_file_path (for app-level client_id)
        let client_id_file_path = raw_settings
            .client_id_file
            .as_ref()
            .map(|s| exe_dir.join(s));

        // Load or generate app-level client_id
        let client_id_uuid = if let Some(id_str) = raw_settings.client_id {
            Uuid::parse_str(&id_str)
                .map_err(|e| AppError::Config(format!("Invalid client_id (UUID) in TOML: {}", e)))?
        } else {
            load_or_generate_client_id(client_id_file_path.as_deref())?
        };

        Ok(Arc::new(Settings {
            server_peer_id,
            bootstrap_addresses,
            encryption_key,
            client_id: client_id_uuid,
            sync_interval: raw_settings.sync_interval,
            retry_interval_on_fail: raw_settings.retry_interval_on_fail,
            max_retries_per_batch: raw_settings.max_retries_per_batch,
            processor_periodic_flush_interval_secs: raw_settings
                .processor_periodic_flush_interval_secs,
            log_file_path: exe_dir.join(raw_settings.log_file_path),
            max_log_file_size_mb: raw_settings.max_log_file_size_mb,
            max_events_per_sync_batch: raw_settings.max_events_per_sync_batch,
            local_log_cache_retention_days: raw_settings
                .local_log_cache_retention_days
                .unwrap_or(7),
            app_name_for_autorun: raw_settings.app_name_for_autorun,
            internal_log_level: raw_settings.internal_log_level,
            internal_log_file_dir: exe_dir.join(raw_settings.internal_log_file_dir),
            internal_log_file_name: raw_settings.internal_log_file_name,
            client_id_file_path,
        }))
    }
}

// Helper function to load app-level client_id from a file or generate a new one
fn load_or_generate_client_id(path_opt: Option<&Path>) -> Result<Uuid, AppError> {
    if let Some(p) = path_opt {
        // Path is already resolved relative to exe_dir if it came from raw_settings
        if p.exists() {
            let id_str = std::fs::read_to_string(p).map_err(AppError::Io)?;
            return Uuid::parse_str(id_str.trim()).map_err(|e| {
                AppError::Config(format!("Invalid client_id (UUID) in file {:?}: {}", p, e))
            });
        }
    }

    let new_id = Uuid::new_v4();
    println!(
        "[INFO] Client: Generated new app-level client_id: {}",
        new_id
    ); // Use tracing once logger is up

    if let Some(p) = path_opt {
        if let Some(parent_dir) = p.parent() {
            std::fs::create_dir_all(parent_dir).map_err(AppError::Io)?;
        }
        std::fs::write(p, new_id.to_string()).map_err(AppError::Io)?;
        println!("[INFO] Client: Saved new app-level client_id to {:?}", p);
    } else {
        println!(
            "[INFO] Client: Using new app-level client_id for this session (no client_id_file configured): {}",
            new_id
        );
    }
    Ok(new_id)
}
