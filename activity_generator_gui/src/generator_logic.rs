// src/generator_logic.rs

use crate::app_state::GeneratorAppState;
// use crate::config_models::{ClientSettingsOutput, LocalServerConfigOutput}; // Used via app_state
use crate::errors::GeneratorError;
use std::fs;
// use std::io::Write; // Not directly needed if using fs::write
// Removed unused import: std::path::Path
use rand::RngCore;
use uuid::Uuid;
use fs_extra::file as fs_extra_file;

pub fn perform_generation(app_state: &mut GeneratorAppState) -> Result<(), GeneratorError> {
    app_state.operation_in_progress = true; // Set at the start
    app_state.status_message = "Starting generation...".to_string();
    app_state.generated_client_id_display = "Generating...".to_string(); // Reset display
    app_state.generated_key_hex_display_snippet = "Generating...".to_string(); // Reset display

    // Defer setting operation_in_progress to false until the end of the function
    // using a guard pattern, or set it explicitly in both Ok and Err arms.
    // For simplicity, we'll set it at the end of this function.

    // --- 1. Validate Inputs from AppState ---
    let client_template_path = app_state.get_client_template_exe_path().ok_or_else(|| {
        GeneratorError::InputValidation {
            field: "Client Template Path".to_string(),
            message: "Path to client_monitor_client_template.exe is not set.".to_string(),
        }
    })?;
    if !client_template_path.exists() {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::BinaryTemplateNotFound {
            binary_name: "activity_monitor_client_template.exe".to_string(),
            path_searched: client_template_path.to_string_lossy().into_owned(),
        });
    }

    let server_template_path = app_state.get_server_template_exe_path().ok_or_else(|| {
        GeneratorError::InputValidation {
            field: "Server Template Path".to_string(),
            message: "Path to local_log_server_template.exe is not set.".to_string(),
        }
    })?;
    if !server_template_path.exists() {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::BinaryTemplateNotFound {
            binary_name: "local_log_server_template.exe".to_string(),
            path_searched: server_template_path.to_string_lossy().into_owned(),
        });
    }

    let output_dir = app_state.get_output_dir_path().ok_or_else(|| {
        GeneratorError::InputValidation {
            field: "Output Directory".to_string(),
            message: "Output directory is not set.".to_string(),
        }
    })?;

    app_state.status_message = "Inputs validated. Generating key and client ID...".to_string();

    // --- 2. Generate Unique Key and Client ID ---
    let client_id = Uuid::new_v4().to_string();
    let mut encryption_key_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut encryption_key_bytes);
    let encryption_key_hex = hex::encode(encryption_key_bytes);
    
    app_state.generated_client_id_display = client_id.clone();
    app_state.generated_key_hex_display_snippet = encryption_key_hex.chars().take(8).collect();

    // --- 3. Prepare Configuration Data (mutating app_state directly) ---
    app_state.synchronize_dependent_configs(); 

    app_state.client_config.encryption_key_hex = encryption_key_hex.clone();
    app_state.client_config.client_id = client_id.clone();
    
    app_state.server_config.encryption_key_hex = encryption_key_hex.clone();
    // Other server_config fields (listen_address, database_path, log_retention_days) are already set in app_state via UI

    app_state.status_message = "Configuration data prepared. Creating output package...".to_string();

    // --- 4. Create Output Directory and Package Files ---
    fs::create_dir_all(&output_dir)?;

    // Copy client executable
    let client_exe_name_template = client_template_path
        .file_name()
        .ok_or_else(|| GeneratorError::PathError("Invalid client template file name.".to_string()))?;
    let final_client_exe_name = client_exe_name_template.to_string_lossy().replace("_template", "");
    let final_client_exe_path = output_dir.join(final_client_exe_name);
    fs_extra_file::copy(&client_template_path, &final_client_exe_path, &fs_extra_file::CopyOptions::new().overwrite(true))?;
    
    // Write client_settings.toml
    let client_toml_content = toml::to_string_pretty(&app_state.client_config)?;
    fs::write(output_dir.join("client_settings.toml"), client_toml_content)?;
    
    // Copy server executable
    let server_exe_name_template = server_template_path
        .file_name()
        .ok_or_else(|| GeneratorError::PathError("Invalid server template file name.".to_string()))?;
    let final_server_exe_name = server_exe_name_template.to_string_lossy().replace("_template", "");
    let final_server_exe_path = output_dir.join(&final_server_exe_name);
    fs_extra_file::copy(&server_template_path, &final_server_exe_path, &fs_extra_file::CopyOptions::new().overwrite(true))?;

    // Write local_server_config.toml
    let server_toml_content = toml::to_string_pretty(&app_state.server_config)?;
    fs::write(output_dir.join("local_server_config.toml"), server_toml_content)?;
    
    // Create static and templates directories for the server
    let server_static_dir = output_dir.join("static");
    let server_templates_dir = output_dir.join("templates");
    fs::create_dir_all(server_static_dir.join("css"))?; // Create css subdir
    fs::create_dir_all(&server_templates_dir)?;
    
    // Here you would copy your actual static/css/style.css and templates/*.html files
    // For simplicity, let's assume they are empty or the server will look for them relative to its exe
    // In a real app, you'd copy them from the generator's assets or a source location.
    // Example: fs::write(server_static_dir.join("css").join("style.css"), "/* Basic styles */")?;

    // FIX: Move the positional argument to the beginning, before named arguments
    let batch_content = format!(
        "@echo off\n\
        echo Starting Local Log Server...\n\
        start \"Local Log Server\" /D \".\" \".\\{}\"\n\
        echo Waiting a few seconds for server to initialize...\n\
        timeout /t 5 /nobreak > nul\n\
        echo Starting Activity Monitor Client...\n\
        start \"Activity Monitor Client\" /D \".\" \".\\{}\"\n\
        echo.\n\
        echo Both applications should be running.\n\
        echo Open http://{} in your browser to view logs.\n\
        echo Press any key to close this window (applications will continue running in background).\n\
        pause > nul",
        final_server_exe_name,
        final_client_exe_path.file_name().unwrap_or_default().to_string_lossy(), // Get just filename
        app_state.server_config.listen_address.replace("0.0.0.0", "127.0.0.1")
    );
    fs::write(output_dir.join("start_monitoring_suite.bat"), batch_content)?;
    
    app_state.status_message = format!(
        "Success! Package generated in {}. Client ID: {}",
        output_dir.display(),
        app_state.generated_client_id_display
    );
    app_state.operation_in_progress = false; // Reset flag
    Ok(())
}