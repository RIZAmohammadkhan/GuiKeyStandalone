// src/app_state.rs (for activity_generator_gui)
use crate::config_models::{ClientSettingsOutput, LocalServerConfigOutput};
use std::path::PathBuf; // Path is not directly used here anymore

#[derive(Clone, Debug)]
pub struct GeneratorAppState {
    pub output_dir_path_str: String,
    pub public_server_url_str: String, // For client's server_url (e.g., https://my.tunnel.com/api/log)
    pub client_config: ClientSettingsOutput, // Client settings modified by user
    pub server_config: LocalServerConfigOutput, // Server settings (listen_address, db_path, retention) modified by user
    pub status_message: String,
    pub operation_in_progress: bool,
    pub generated_key_hex_display_snippet: String,
    pub generated_client_id_display: String,
}

impl Default for GeneratorAppState {
    fn default() -> Self {
        Self {
            output_dir_path_str: String::new(),
            // Default to common local setup, user MUST change this for remote deployment.
            // It's critical this ends with /api/log if that's what the server expects.
            public_server_url_str: "http://127.0.0.1:8090/api/log".to_string(),
            client_config: ClientSettingsOutput::new_with_defaults(),
            server_config: LocalServerConfigOutput::new_with_defaults(),
            status_message: "Welcome! Configure Public Server URL, Output Dir, and other settings.".to_string(),
            operation_in_progress: false,
            generated_key_hex_display_snippet: "N/A".to_string(),
            generated_client_id_display: "N/A".to_string(),
        }
    }
}

impl GeneratorAppState {
    // Removed get_client_template_exe_path
    // Removed get_server_template_exe_path
    // Removed get_server_static_assets_source_path
    // Removed get_server_templates_source_path

    pub fn get_output_dir_path(&self) -> Option<PathBuf> {
        if self.output_dir_path_str.is_empty() { None } else { Some(PathBuf::from(&self.output_dir_path_str)) }
    }

    // Removed synchronize_dependent_configs
}