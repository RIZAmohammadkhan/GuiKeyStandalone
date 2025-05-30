// src/config_models.rs (for activity_generator_gui)
use serde::Serialize;

// --- For client_settings.toml (to be used by activity_monitor_client_core.exe) ---
#[derive(Serialize, Debug, Clone)]
pub struct ClientSettingsOutput {
    pub server_peer_id: String, // Libp2p PeerId of the server
    pub encryption_key_hex: String,
    pub bootstrap_addresses: Vec<String>, // List of multiaddrs for bootstrapping
    pub client_id: String, // Application-level UUID for the client
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
            server_peer_id: "".to_string(), // Must be configured by the generator
            encryption_key_hex: String::new(), // Will be generated
            // Example public bootstrap nodes for libp2p
            bootstrap_addresses: vec![
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN".to_string(),
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb".to_string(),
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt".to_string()
            ],
            client_id: String::new(), // Will be generated (UUID)
            sync_interval: 60, // seconds
            processor_periodic_flush_interval_secs: 120, // seconds
            internal_log_level: "info".to_string(),
            log_file_path: "activity_data.jsonl".to_string(),
            app_name_for_autorun: "SystemActivityAgent".to_string(),
            local_log_cache_retention_days: 7,
            retry_interval_on_fail: 60, // seconds
            max_retries_per_batch: 3,
            max_log_file_size_mb: Some(20),
            max_events_per_sync_batch: 200,
            internal_log_file_dir: "client_logs".to_string(),
            internal_log_file_name: "monitor_client_diag.log".to_string(),
            client_id_file: None, // Typically not used if client_id is directly in config
        }
    }
}

// --- For local_server_config.toml (to be used by local_log_server.exe) ---
#[derive(Serialize, Debug, Clone)]
pub struct LocalServerConfigOutput {
    pub listen_address: String, // This will be for the libp2p listener (e.g. /ip4/0.0.0.0/tcp/0 or /ip4/0.0.0.0/udp/0/quic-v1)
    pub web_ui_listen_address: String, // For Actix-Web UI and API e.g. 0.0.0.0:8090
    pub encryption_key_hex: String, // For application-level data encryption
    pub server_identity_key_seed_hex: String, // 32-byte seed as hex for libp2p Ed25519 keypair
    pub database_path: String,
    pub log_retention_days: u32,
}

impl LocalServerConfigOutput {
    pub fn new_with_defaults() -> Self {
        Self {
            listen_address: "/ip4/0.0.0.0/tcp/0".to_string(), // Default libp2p listen multiaddr (TCP, any port)
            web_ui_listen_address: "0.0.0.0:8090".to_string(), // Default for the Web UI
            encryption_key_hex: String::new(), // Will be generated
            server_identity_key_seed_hex: String::new(), // Will be generated
            database_path: "activity_database.sqlite".to_string(),
            log_retention_days: 30,
        }
    }
}