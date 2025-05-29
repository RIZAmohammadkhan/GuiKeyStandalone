use crate::app_state::GeneratorAppState;
use crate::errors::GeneratorError;
use std::fs;
use std::io::Write; // For writing bytes
use std::path::Path; // For joining paths
use rand::RngCore;
use uuid::Uuid;
use fs_extra::dir as fs_extra_dir; // Not strictly needed if include_dir handles extraction well
use include_dir::{include_dir, Dir};

// Embed the binaries and server assets directly into the generator executable
static CLIENT_TEMPLATE_PAYLOAD: &[u8] = include_bytes!("embedded_assets/client_template_payload.bin");
static SERVER_TEMPLATE_PAYLOAD: &[u8] = include_bytes!("embedded_assets/server_template_payload.bin");
static SERVER_PACKAGE_CONTENT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/embedded_assets/server_package_content");

const CLIENT_TEMPLATE_ORIGINAL_NAME: &str = "activity_monitor_client_template.exe";
const SERVER_TEMPLATE_ORIGINAL_NAME: &str = "local_log_server_template.exe";

pub fn perform_generation(app_state: &mut GeneratorAppState) -> Result<(), GeneratorError> {
    app_state.operation_in_progress = true;
    app_state.status_message = "Starting generation...".to_string();
    app_state.generated_client_id_display = "Generating...".to_string();
    app_state.generated_key_hex_display_snippet = "Generating...".to_string();

    // --- 1. Validate Inputs ---
    let output_dir = app_state.get_output_dir_path().ok_or_else(|| {
        GeneratorError::InputValidation {
            field: "Output Directory".to_string(),
            message: "Output directory is not set.".to_string(),
        }
    })?;

    if app_state.public_server_url_str.is_empty() || 
       (!app_state.public_server_url_str.starts_with("http://") && !app_state.public_server_url_str.starts_with("https://")) {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::InputValidation {
            field: "Public Server URL".to_string(),
            message: "Public Server URL must be a valid HTTP/HTTPS URL (e.g., https://your.domain.com/api/log).".to_string(),
        });
    }
    if !app_state.public_server_url_str.ends_with("/api/log") {
        // Update status but proceed, user might have specific needs
        app_state.status_message = format!(
            "Warning: Public Server URL '{}' does not end with '/api/log'. This is the typical client endpoint.",
            app_state.public_server_url_str
        );
    }

    // Validate server listen address format (basic check)
    if app_state.server_config.listen_address.split(':').count() != 2 {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::InputValidation {
            field: "Server Listen Address".to_string(),
            message: "Format must be IP:PORT (e.g., 0.0.0.0:8090 or 127.0.0.1:8090).".to_string(),
        });
    }


    app_state.status_message = "Inputs validated. Generating key and client ID...".to_string();

    // --- 2. Generate Unique Key and Client ID ---
    let client_id = Uuid::new_v4().to_string();
    let mut encryption_key_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut encryption_key_bytes);
    let encryption_key_hex = hex::encode(encryption_key_bytes);
    
    app_state.generated_client_id_display = client_id.clone();
    app_state.generated_key_hex_display_snippet = encryption_key_hex.chars().take(8).collect();

    // --- 3. Prepare Configuration Data ---
    app_state.client_config.server_url = app_state.public_server_url_str.clone();
    app_state.client_config.encryption_key_hex = encryption_key_hex.clone();
    app_state.client_config.client_id = client_id.clone();
    
    app_state.server_config.encryption_key_hex = encryption_key_hex.clone();

    if app_state.server_config.listen_address.starts_with("127.0.0.1") && 
       (app_state.public_server_url_str.starts_with("https://") || // If public URL is HTTPS (implies remote)
        app_state.public_server_url_str.starts_with("http://") && !app_state.public_server_url_str.contains("127.0.0.1") && !app_state.public_server_url_str.contains("localhost")) { // or HTTP and not local
        let current_status = app_state.status_message.clone();
        app_state.status_message = format!(
            "{}\nWarning: Server listen_address is '{}' but public URL seems remote. Consider '0.0.0.0:port' for the server to be accessible by a tunnel/proxy.",
            current_status, app_state.server_config.listen_address
        );
    }

    app_state.status_message = format!("Configuration data prepared. {}", app_state.status_message.split('\n').last().unwrap_or(""));


    // --- 4. Create Output Directory and Package Files ---
    fs::create_dir_all(&output_dir)
        .map_err(|e| GeneratorError::Io{ source: e})?;


    // --- Client Package ---
    let client_output_dir = output_dir.join("ActivityMonitorClient_Package");
    fs::create_dir_all(&client_output_dir)?;

    let final_client_exe_name = CLIENT_TEMPLATE_ORIGINAL_NAME.replace("_template", "");
    let final_client_exe_path = client_output_dir.join(&final_client_exe_name);
    let mut client_exe_file = fs::File::create(&final_client_exe_path)?;
    client_exe_file.write_all(CLIENT_TEMPLATE_PAYLOAD)?;
    drop(client_exe_file); // Ensure file is closed
    #[cfg(unix)] // Make executable on Unix if generated there (though templates are .exe)
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&final_client_exe_path, fs::Permissions::from_mode(0o755))?;
    }
    
    let client_toml_content = toml::to_string_pretty(&app_state.client_config)?;
    fs::write(client_output_dir.join("client_settings.toml"), client_toml_content)?;

    // --- Server Package ---
    let server_output_dir = output_dir.join("LocalLogServer_Package");
    fs::create_dir_all(&server_output_dir)?;
    
    let final_server_exe_name = SERVER_TEMPLATE_ORIGINAL_NAME.replace("_template", "");
    let final_server_exe_path = server_output_dir.join(&final_server_exe_name);
    let mut server_exe_file = fs::File::create(&final_server_exe_path)?;
    server_exe_file.write_all(SERVER_TEMPLATE_PAYLOAD)?;
    drop(server_exe_file); // Ensure file is closed
     #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&final_server_exe_path, fs::Permissions::from_mode(0o755))?;
    }

    let server_toml_content = toml::to_string_pretty(&app_state.server_config)?;
    fs::write(server_output_dir.join("local_server_config.toml"), server_toml_content)?;
    
    // Extract embedded server static assets and templates
    // The SERVER_PACKAGE_CONTENT_DIR contains 'static/' and 'templates/'
    SERVER_PACKAGE_CONTENT_DIR.extract(&server_output_dir)
        .map_err(|e| GeneratorError::PathError(format!("Failed to extract embedded server assets: {}", e)))?;


    // --- Create README ---
    let local_server_ui_host = app_state.server_config.listen_address.split(':').next().unwrap_or("127.0.0.1");
    let local_server_ui_port = app_state.server_config.listen_address.split(':').nth(1).unwrap_or("8090");
    let local_server_ui_access_address = format!("{}:{}", local_server_ui_host.replace("0.0.0.0", "127.0.0.1"), local_server_ui_port);
    
    let public_server_base_url = if let Some(idx) = app_state.public_server_url_str.rfind("/api/log") {
        app_state.public_server_url_str[..idx].to_string()
    } else {
        // If it doesn't end with /api/log, guess the base by removing the last path segment
        Path::new(&app_state.public_server_url_str).parent().unwrap_or_else(|| Path::new(&app_state.public_server_url_str)).to_string_lossy().to_string()
    };


    let readme_content = format!(
        "Activity Monitoring Suite - Generated Packages\n\
        =============================================\n\n\
        This package was generated by the GuiKeyStandalone Generator.\n\n\
        Client ID: {}\n\
        Encryption Key (Hex Snippet): {}...\n\n\
        Instructions:\n\
        ------------\n\n\
        1. Local Log Server (For Your Machine - The Operator):\n\
           - The 'LocalLogServer_Package' directory contains the server application.\n\
           - Run the '{final_server_exe_name}' executable from within this directory.\n\
           - The server is configured to listen on: {server_listen_address}\n\
           - IMPORTANT: For remote clients to connect, this server must be made accessible via the Public Server URL you configured:\n\
             {public_url}\n\
           - This usually requires a tunneling service (like cloudflared, ngrok), a reverse proxy (Nginx, Caddy with HTTPS), or manual port forwarding.\n\
             Your chosen method should forward traffic from the public URL to the server's listening address (e.g., http://{local_server_ui_access_address}).\n\
           - Once the server is running and publicly accessible, the Web UI can be found at: {public_server_base_url}/logs\n\
             (Or locally for testing: http://{local_server_ui_access_address}/logs)\n\
        \n\
        2. Activity Monitor Client (For Distribution to Target Machines):\n\
           - The 'ActivityMonitorClient_Package' directory contains the client application.\n\
           - Distribute the *contents* of this directory (the client executable and its 'client_settings.toml') to the machine(s) you want to monitor.\n\
           - Run the '{final_client_exe_name_str}' on the target machine(s).\n\
           - It will automatically attempt to connect and send data to your server at: {public_url}\n\
        \n\
        Security Considerations:\n\
        - The generated encryption key is vital. Keep it secure.\n\
        - You are responsible for securing the machine running the Local Log Server, especially since it will be exposed (directly or indirectly) to the internet.\n\
        - Ensure you have proper consent and adhere to all relevant privacy laws and ethical guidelines when deploying the client monitor.\n",
        app_state.generated_client_id_display,
        app_state.generated_key_hex_display_snippet,
        final_server_exe_name = final_server_exe_name,
        server_listen_address = app_state.server_config.listen_address,
        public_url = app_state.public_server_url_str,
        local_server_ui_access_address = local_server_ui_access_address,
        public_server_base_url = public_server_base_url,
        final_client_exe_name_str = final_client_exe_name
    );
    fs::write(output_dir.join("README_IMPORTANT_INSTRUCTIONS.txt"), readme_content)?;
    
    app_state.status_message = format!(
        "Success! Packages generated in {}. Client ID: {}. README_IMPORTANT_INSTRUCTIONS.txt created.",
        output_dir.display(),
        app_state.generated_client_id_display
    );
    app_state.operation_in_progress = false;
    Ok(())
}