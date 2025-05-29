use crate::errors::AppError;
use config::{Config, Environment, File as ConfigFile}; // Renamed File to avoid conflict
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

// The main Settings struct used throughout the application
#[derive(Debug, Clone)] // Removed Deserialize as we build this from RawSettings
pub struct Settings {
    pub server_url: String,
    pub encryption_key: [u8; 32], // Decoded binary key
    pub client_id: Uuid,           // Loaded or generated

    pub sync_interval: u64,          // seconds
    pub retry_interval_on_fail: u64, // seconds
    pub max_retries_per_batch: u32,

    pub processor_periodic_flush_interval_secs: u64, // seconds

    pub log_file_path: PathBuf, // Path for activity_data.log.bin
    pub max_log_file_size_mb: Option<u64>,
    pub max_events_per_sync_batch: usize, // Number of ApplicationActivity blocks

    pub app_name_for_autorun: String,
    pub internal_log_level: String,
    pub internal_log_file_dir: PathBuf, // Path for diagnostic logs
    pub internal_log_file_name: String,

    // Path to the file where client_id might be persisted (optional)
    pub client_id_file_path: Option<PathBuf>,
    pub local_log_cache_retention_days: u32,
}

// Struct to directly deserialize from client_settings.toml
#[derive(Debug, Deserialize)]
struct RawSettings {
    server_url: String,
    encryption_key_hex: String, // Key as hex string from TOML
    client_id: Option<String>,  // Client ID might be directly in TOML

    sync_interval: u64,
    retry_interval_on_fail: u64,
    max_retries_per_batch: u32,

    processor_periodic_flush_interval_secs: u64,

    log_file_path: String,
    max_log_file_size_mb: Option<u64>,
    max_events_per_sync_batch: usize,

    app_name_for_autorun: String,
    internal_log_level: String,
    internal_log_file_dir: String,
    internal_log_file_name: String,

    client_id_file: Option<String>, // Path to file for persisting client_id
    local_log_cache_retention_days: Option<u32>, // Optional in TOML, with default
}

impl Settings {
    pub fn new() -> Result<Arc<Self>, AppError> {
        // Determine config path:
        // 1. Try executable_dir/config/client_settings.toml
        // 2. Try executable_dir/client_settings.toml
        // 3. Try current_dir/config/client_settings.toml (for dev)
        // 4. Try current_dir/client_settings.toml (for dev)

        let exe_path = std::env::current_exe()
            .map_err(|e| AppError::Config(format!("Failed to get current exe path: {}", e)))?;
        let exe_dir = exe_path.parent()
            .ok_or_else(|| AppError::Config("Failed to get parent directory of executable.".to_string()))?;

        let config_paths_to_try = [
            exe_dir.join("config").join("client_settings.toml"),
            exe_dir.join("client_settings.toml"),
            PathBuf::from("config").join("client_settings.toml"), // Relative to CWD
            PathBuf::from("client_settings.toml"),                // Relative to CWD
        ];

        let mut config_builder = Config::builder();
        let mut loaded_from_file = false;

        for path_to_try in &config_paths_to_try {
            if path_to_try.exists() {
                config_builder = config_builder.add_source(ConfigFile::from(path_to_try.clone()).required(true));
                loaded_from_file = true;
                println!("[INFO] Loading configuration from: {:?}", path_to_try); // Temporary println
                break;
            }
        }

        if !loaded_from_file {
            return Err(AppError::Config(
                "client_settings.toml not found in standard locations.".to_string(),
            ));
        }

        // Add environment variable overrides
        config_builder = config_builder.add_source(
            Environment::with_prefix("AMS_CLIENT") // Activity Monitor Suite Client
                .separator("__")
                .try_parsing(true), // Attempt to parse strings to target types
        );

        let raw_settings: RawSettings = config_builder
            .build()
            .map_err(|e| AppError::Config(format!("Failed to build configuration: {}", e)))?
            .try_deserialize()
            .map_err(|e| AppError::Config(format!("Failed to deserialize configuration: {}", e)))?;

        // Process encryption key
        let key_bytes = hex::decode(&raw_settings.encryption_key_hex)
            .map_err(AppError::HexDecode)?;
        if key_bytes.len() != 32 {
            return Err(AppError::Config(
                "Encryption key must be 32 bytes (64 hex characters).".to_string(),
            ));
        }
        let mut encryption_key = [0u8; 32];
        encryption_key.copy_from_slice(&key_bytes);

        // Determine client_id_file_path
        let client_id_file_path = raw_settings.client_id_file.as_ref().map(PathBuf::from);

        // Load or generate client_id
        // Priority: 1. Direct from TOML, 2. From client_id_file, 3. Generate new
        let client_id = if let Some(id_str) = raw_settings.client_id {
            Uuid::parse_str(&id_str)
                .map_err(|e| AppError::Config(format!("Invalid client_id in TOML: {}", e)))?
        } else {
            load_or_generate_client_id(client_id_file_path.as_deref())?
        };

        // Construct the final Settings struct
        Ok(Arc::new(Settings {
            server_url: raw_settings.server_url,
            encryption_key,
            client_id,
            sync_interval: raw_settings.sync_interval,
            retry_interval_on_fail: raw_settings.retry_interval_on_fail,
            max_retries_per_batch: raw_settings.max_retries_per_batch,
            processor_periodic_flush_interval_secs: raw_settings.processor_periodic_flush_interval_secs,
            log_file_path: exe_dir.join(raw_settings.log_file_path), // Make relative to exe dir
            max_log_file_size_mb: raw_settings.max_log_file_size_mb,
            max_events_per_sync_batch: raw_settings.max_events_per_sync_batch,
            app_name_for_autorun: raw_settings.app_name_for_autorun,
            internal_log_level: raw_settings.internal_log_level,
            internal_log_file_dir: exe_dir.join(raw_settings.internal_log_file_dir), // Make relative to exe dir
            internal_log_file_name: raw_settings.internal_log_file_name,
            client_id_file_path,
            local_log_cache_retention_days: raw_settings.local_log_cache_retention_days.unwrap_or(7), // Default if not in TOML
        }))
    }
}

// Helper function to load client_id from a file or generate a new one
fn load_or_generate_client_id(path_opt: Option<&Path>) -> Result<Uuid, AppError> {
    if let Some(p) = path_opt {
        // Ensure path is absolute or resolve relative to exe_dir if needed
        // For simplicity, assuming if client_id_file is specified, it's an intended path
        let file_path = if p.is_absolute() {
            p.to_path_buf()
        } else {
            // If relative, make it relative to the executable's directory
            let exe_dir = std::env::current_exe()
                .ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .ok_or_else(|| AppError::Config("Cannot determine executable directory for relative client_id_file path.".to_string()))?;
            exe_dir.join(p)
        };

        if file_path.exists() {
            let id_str = std::fs::read_to_string(&file_path)
                .map_err(|e| AppError::Io(e))?; // Map IO error to AppError
            return Uuid::parse_str(id_str.trim())
                .map_err(|e| AppError::Config(format!("Invalid client_id in file {:?}: {}", file_path, e)));
        }
    }

    // Generate new ID
    let new_id = Uuid::new_v4();
    tracing::info!("Generated new client_id: {}", new_id); // Use tracing, but logger might not be init yet

    // Attempt to save if path was provided
    if let Some(p) = path_opt {
        let file_path = if p.is_absolute() {
            p.to_path_buf()
        } else {
            let exe_dir = std::env::current_exe()
                .ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .ok_or_else(|| AppError::Config("Cannot determine executable directory for relative client_id_file path.".to_string()))?;
            exe_dir.join(p)
        };

        if let Some(parent_dir) = file_path.parent() {
            std::fs::create_dir_all(parent_dir).map_err(|e| AppError::Io(e))?;
        }
        std::fs::write(&file_path, new_id.to_string()).map_err(|e| AppError::Io(e))?;
        // Use println here as tracing might not be initialized when config is first loaded
        println!("[INFO] Generated and saved new client_id: {} to {:?}", new_id, file_path);
    } else {
         println!("[INFO] Generated new client_id for this session (no client_id_file configured): {}", new_id);
    }
    Ok(new_id)
}