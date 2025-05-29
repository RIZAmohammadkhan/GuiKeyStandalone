// src/app_state.rs (for activity_generator_gui)
use crate::config_models::{ClientSettingsOutput, LocalServerConfigOutput};
use std::path::{Path, PathBuf}; // Ensure Path is imported for Path::new("")

#[derive(Clone, Debug)]
pub struct GeneratorAppState {
    pub client_template_exe_path_str: String,
    pub server_template_exe_path_str: String,
    pub output_dir_path_str: String,
    pub client_config: ClientSettingsOutput,
    pub server_config: LocalServerConfigOutput,
    pub status_message: String,
    pub operation_in_progress: bool,
    pub generated_key_hex_display_snippet: String,
    pub generated_client_id_display: String,
}

impl Default for GeneratorAppState {
    fn default() -> Self {
        let current_exe_path = std::env::current_exe().unwrap_or_default();
        let current_exe_dir = current_exe_path.parent().unwrap_or_else(|| Path::new("")); // Path::new needs std::path::Path

        let default_client_template_path = current_exe_dir
            .join("bundled_binaries")
            .join("activity_monitor_client_template.exe");
        let default_server_template_path = current_exe_dir
            .join("bundled_binaries")
            .join("local_log_server_template.exe");

        Self {
            client_template_exe_path_str: default_client_template_path.to_string_lossy().into_owned(),
            server_template_exe_path_str: default_server_template_path.to_string_lossy().into_owned(),
            output_dir_path_str: String::new(),
            client_config: ClientSettingsOutput::new_with_defaults(),
            server_config: LocalServerConfigOutput::new_with_defaults(),
            status_message: "Welcome! Please select paths and adjust settings.".to_string(),
            operation_in_progress: false,
            generated_key_hex_display_snippet: "N/A".to_string(),
            generated_client_id_display: "N/A".to_string(),
        }
    }
}

impl GeneratorAppState {
    pub fn get_client_template_exe_path(&self) -> Option<PathBuf> {
        if self.client_template_exe_path_str.is_empty() { None } else { Some(PathBuf::from(&self.client_template_exe_path_str)) }
    }
    pub fn get_server_template_exe_path(&self) -> Option<PathBuf> {
        if self.server_template_exe_path_str.is_empty() { None } else { Some(PathBuf::from(&self.server_template_exe_path_str)) }
    }
    pub fn get_output_dir_path(&self) -> Option<PathBuf> {
        if self.output_dir_path_str.is_empty() { None } else { Some(PathBuf::from(&self.output_dir_path_str)) }
    }

    pub fn synchronize_dependent_configs(&mut self) {
        let parts: Vec<&str> = self.server_config.listen_address.split(':').collect();
        let port = if parts.len() == 2 { parts[1] } else { "8090" }; // Default if parsing fails
        self.client_config.server_url = format!("http://127.0.0.1:{}/api/log", port);
    }
}