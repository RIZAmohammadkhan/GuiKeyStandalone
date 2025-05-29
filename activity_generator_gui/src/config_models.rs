// src/config_models.rs (for activity_generator_gui)
use serde::Serialize;

// --- For client_settings.toml (to be used by activity_monitor_client_core.exe) ---
#[derive(Serialize, Debug, Clone)]
pub struct ClientSettingsOutput {
    pub server_url: String,
    pub encryption_key_hex: String,
    pub client_id: String,
    pub sync_interval: u64,
    pub processor_periodic_flush_interval_secs: u64,
    pub internal_log_level: String,
    pub log_file_path: String,
    pub app_name_for_autorun: String,
    pub local_log_cache_retention_days: u32,
    pub retry_interval_on_fail: u64,
    pub max_retries_per_batch: u32,
    pub max_log_file_size_mb: Option<u64>,
    pub max_events_per_sync_batch: usize,
    pub internal_log_file_dir: String,
    pub internal_log_file_name: String,
    pub client_id_file: Option<String>,
}

impl ClientSettingsOutput {
    pub fn new_with_defaults() -> Self {
        Self {
            server_url: "http://127.0.0.1:8090/api/log".to_string(),
            encryption_key_hex: String::new(),
            client_id: String::new(),
            sync_interval: 60,
            processor_periodic_flush_interval_secs: 120,
            internal_log_level: "info".to_string(),
            log_file_path: "activity_data.jsonl".to_string(), // MODIFIED HERE from .log.bin
            app_name_for_autorun: "SystemActivityAgent".to_string(),
            local_log_cache_retention_days: 7,
            retry_interval_on_fail: 60,
            max_retries_per_batch: 3,
            max_log_file_size_mb: Some(20),
            max_events_per_sync_batch: 200,
            internal_log_file_dir: "client_logs".to_string(),
            internal_log_file_name: "monitor_client_diag.log".to_string(),
            client_id_file: None,
        }
    }
}

// --- For local_server_config.toml (to be used by local_log_server.exe) ---
#[derive(Serialize, Debug, Clone)]
pub struct LocalServerConfigOutput {
    pub listen_address: String,
    pub encryption_key_hex: String,
    pub database_path: String,
    pub log_retention_days: u32,
}

impl LocalServerConfigOutput {
    pub fn new_with_defaults() -> Self {
        Self {
            listen_address: "0.0.0.0:8090".to_string(), // MODIFIED HERE
            encryption_key_hex: String::new(),
            database_path: "activity_database.sqlite".to_string(),
            log_retention_days: 30,
        }
    }
}