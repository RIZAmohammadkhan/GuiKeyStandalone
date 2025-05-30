// src/app_state.rs (for activity_generator_gui)
use crate::config_models::{ClientSettingsOutput, LocalServerConfigOutput};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct GeneratorAppState {
    pub output_dir_path_str: String,
    pub bootstrap_addresses_str: String, // Comma-separated multiaddresses for client config
    pub client_config: ClientSettingsOutput, // Client settings modified by user
    pub server_config: LocalServerConfigOutput, // Server settings modified by user
    pub status_message: String,
    pub operation_in_progress: bool,
    pub generated_key_hex_display_snippet: String, // For app-level AES key
    pub generated_client_id_display: String,       // App-level UUID for client
    pub generated_server_peer_id_display: String,  // Libp2p PeerId for the server
}

impl Default for GeneratorAppState {
    fn default() -> Self {
        let default_client_config = ClientSettingsOutput::new_with_defaults();
        Self {
            output_dir_path_str: String::new(),
            bootstrap_addresses_str: default_client_config.bootstrap_addresses.join(", "), // Initialize from defaults
            client_config: default_client_config,
            server_config: LocalServerConfigOutput::new_with_defaults(),
            status_message:
                "Welcome! Configure Output Dir, Bootstrap Addresses, and other settings."
                    .to_string(),
            operation_in_progress: false,
            generated_key_hex_display_snippet: "N/A".to_string(),
            generated_client_id_display: "N/A".to_string(),
            generated_server_peer_id_display: "N/A (will be generated)".to_string(),
        }
    }
}

impl GeneratorAppState {
    pub fn get_output_dir_path(&self) -> Option<PathBuf> {
        if self.output_dir_path_str.is_empty() {
            None
        } else {
            Some(PathBuf::from(&self.output_dir_path_str))
        }
    }
}
