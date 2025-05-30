use crate::app_state::GeneratorAppState;
use crate::errors::GeneratorError;
use rand::RngCore;
use std::fs;
use std::io::Write; // For writing bytes
use uuid::Uuid;
// Correct imports for libp2p-identity 0.2.x
use libp2p_identity::{Keypair, PeerId};

use include_dir::{Dir, include_dir};

// Embed the binaries and server assets directly into the generator executable
static CLIENT_TEMPLATE_PAYLOAD: &[u8] =
    include_bytes!("embedded_assets/client_template_payload.bin");
static SERVER_TEMPLATE_PAYLOAD: &[u8] =
    include_bytes!("embedded_assets/server_template_payload.bin");
static SERVER_PACKAGE_CONTENT_DIR: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/src/embedded_assets/server_package_content");

const CLIENT_TEMPLATE_ORIGINAL_NAME: &str = "activity_monitor_client_template.exe";
const SERVER_TEMPLATE_ORIGINAL_NAME: &str = "local_log_server_template.exe";

pub fn perform_generation(app_state: &mut GeneratorAppState) -> Result<(), GeneratorError> {
    app_state.operation_in_progress = true;
    app_state.status_message = "Starting generation...".to_string();
    app_state.generated_client_id_display = "Generating...".to_string(); // App-level UUID
    app_state.generated_server_peer_id_display = "Generating...".to_string(); // libp2p PeerId
    app_state.generated_key_hex_display_snippet = "Generating...".to_string(); // App-level AES key

    // --- 1. Validate Inputs ---
    let output_dir =
        app_state
            .get_output_dir_path()
            .ok_or_else(|| GeneratorError::InputValidation {
                field: "Output Directory".to_string(),
                message: "Output directory is not set.".to_string(),
            })?;

    // Validate bootstrap addresses
    if app_state.bootstrap_addresses_str.is_empty() {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::InputValidation {
            field: "Bootstrap Multiaddresses".to_string(),
            message: "At least one bootstrap multiaddress is required (e.g., for a public relay or the server itself).".to_string(),
        });
    }
    let bootstrap_addrs_for_client_config: Vec<String> = app_state
        .bootstrap_addresses_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.starts_with("/")) // Basic multiaddr check
        .collect();

    if bootstrap_addrs_for_client_config.is_empty() {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::InputValidation {
            field: "Bootstrap Multiaddresses".to_string(),
            message: "No valid bootstrap multiaddresses found after parsing (must start with '/')."
                .to_string(),
        });
    }

    // Validate server P2P listen address format (libp2p multiaddr format)
    if !app_state.server_config.listen_address.starts_with("/") {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::InputValidation {
            field: "Server P2P Listen Multiaddress".to_string(),
            message: "Format must be a libp2p Multiaddress (e.g., /ip4/0.0.0.0/tcp/0).".to_string(),
        });
    }
    // Validate server Web UI listen address format (basic check)
    if app_state
        .server_config
        .web_ui_listen_address
        .split(':')
        .count()
        != 2
    {
        app_state.operation_in_progress = false;
        return Err(GeneratorError::InputValidation {
            field: "Server Web UI Listen Address".to_string(),
            message: "Format must be IP:PORT (e.g., 0.0.0.0:8090 or 127.0.0.1:8090).".to_string(),
        });
    }

    app_state.status_message = "Inputs validated. Generating keys and IDs...".to_string();

    // --- 2. Generate Unique Keys and IDs ---
    // App-level Client ID (UUID)
    let client_uuid = Uuid::new_v4().to_string();
    app_state.generated_client_id_display = client_uuid.clone();

    // App-level AES Encryption Key
    let mut encryption_key_bytes = [0u8; 32]; // AES-256
    rand::thread_rng().fill_bytes(&mut encryption_key_bytes);
    let encryption_key_hex = hex::encode(encryption_key_bytes);
    app_state.generated_key_hex_display_snippet =
        encryption_key_hex.chars().take(8).collect::<String>() + "...";

    // Server Libp2p Identity (Ed25519 keypair from seed)
    let mut server_identity_seed_bytes = [0u8; 32]; // 32-byte seed for Ed25519
    rand::thread_rng().fill_bytes(&mut server_identity_seed_bytes);
    let server_identity_key_seed_hex = hex::encode(server_identity_seed_bytes);

    // Create libp2p Keypair directly from seed bytes using the new API
    let server_libp2p_keypair =
        Keypair::ed25519_from_bytes(server_identity_seed_bytes).map_err(|e| {
            GeneratorError::Other(format!(
                "Failed to create libp2p keypair from seed bytes: {:?}",
                e
            ))
        })?;

    // Get the PeerId from the keypair's public key
    let server_peer_id = PeerId::from_public_key(&server_libp2p_keypair.public());
    app_state.generated_server_peer_id_display = server_peer_id.to_string();

    // --- 3. Prepare Configuration Data ---
    // Client Configuration
    app_state.client_config.server_peer_id = server_peer_id.to_string();
    app_state.client_config.encryption_key_hex = encryption_key_hex.clone();
    app_state.client_config.client_id = client_uuid.clone(); // App-level UUID
    app_state.client_config.bootstrap_addresses = bootstrap_addrs_for_client_config;

    // Server Configuration
    app_state.server_config.encryption_key_hex = encryption_key_hex.clone();
    app_state.server_config.server_identity_key_seed_hex = server_identity_key_seed_hex.clone();

    app_state.status_message = format!(
        "Configuration data prepared. Server PeerID: {}",
        server_peer_id
    );

    // --- 4. Create Output Directory and Package Files ---
    fs::create_dir_all(&output_dir).map_err(|e| GeneratorError::Io { source: e })?;

    // --- Client Package ---
    let client_output_dir = output_dir.join("ActivityMonitorClient_Package");
    fs::create_dir_all(&client_output_dir)?;

    let final_client_exe_name = CLIENT_TEMPLATE_ORIGINAL_NAME.replace("_template", "");
    let final_client_exe_path = client_output_dir.join(&final_client_exe_name);
    let mut client_exe_file = fs::File::create(&final_client_exe_path)?;
    client_exe_file.write_all(CLIENT_TEMPLATE_PAYLOAD)?;
    drop(client_exe_file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&final_client_exe_path, fs::Permissions::from_mode(0o755))?;
    }

    let client_toml_content = toml::to_string_pretty(&app_state.client_config)?;
    fs::write(
        client_output_dir.join("client_settings.toml"),
        client_toml_content,
    )?;

    // --- Server Package ---
    let server_output_dir = output_dir.join("LocalLogServer_Package");
    fs::create_dir_all(&server_output_dir)?;

    let final_server_exe_name = SERVER_TEMPLATE_ORIGINAL_NAME.replace("_template", "");
    let final_server_exe_path = server_output_dir.join(&final_server_exe_name);
    let mut server_exe_file = fs::File::create(&final_server_exe_path)?;
    server_exe_file.write_all(SERVER_TEMPLATE_PAYLOAD)?;
    drop(server_exe_file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&final_server_exe_path, fs::Permissions::from_mode(0o755))?;
    }

    let server_toml_content = toml::to_string_pretty(&app_state.server_config)?;
    fs::write(
        server_output_dir.join("local_server_config.toml"),
        server_toml_content,
    )?;

    SERVER_PACKAGE_CONTENT_DIR
        .extract(&server_output_dir)
        .map_err(|e| {
            GeneratorError::PathError(format!("Failed to extract embedded server assets: {}", e))
        })?;

    // --- Create README ---
    let local_server_ui_access_address = app_state
        .server_config
        .web_ui_listen_address
        .replace("0.0.0.0", "127.0.0.1");

    let readme_content = format!(
        "Activity Monitoring Suite - Generated Packages (P2P Mode)\n\
        ========================================================\n\n\
        This package was generated by the GuiKeyStandalone Generator.\n\n\
        Generated App-Level Client ID (for logs): {app_client_id}\n\
        Generated App-Level Encryption Key (Hex Snippet): {app_key_snippet}\n\
        Generated Server Libp2p Peer ID: {server_actual_peer_id}\n\
        Server Libp2p Identity Seed (Hex Snippet): {server_seed_snippet}...\n\n\
        Instructions:\n\
        ------------\n\n\
        1. Local Log Server (For Your Machine - The Operator):\n\
           - The 'LocalLogServer_Package' directory contains the server application and its configuration.\n\
           - It's configured with the unique libp2p identity seed (see `server_identity_key_seed_hex` in `local_server_config.toml`).\n\
           - Run the '{server_exe_name}' executable from within this directory.\n\
           - The server's P2P component is configured to listen on multiaddress(es) like: {server_p2p_listen_config}\n\
           - On startup, the server will log its *actual* listening multiaddresses and its PeerID ({server_actual_peer_id}). Note these down if you need to update client configurations later or provide them directly to clients.\n\
           - For clients to connect, the server needs to be reachable via the libp2p network. This may involve NAT traversal (hole punching, relays). Ensure your network/firewall allows UDP/TCP traffic for libp2p on the ports it chooses or is configured for.\n\
           - The server's Web UI for viewing logs is configured to listen on {server_web_ui_listen_config} and can be accessed locally at: http://{web_ui_access}/logs\n\
        \n\
        2. Activity Monitor Client (For Distribution to Target Machines):\n\
           - The 'ActivityMonitorClient_Package' directory contains the client application and its configuration.\n\
           - Distribute the *contents* of this directory (the client executable and its 'client_settings.toml') to the machine(s) you want to monitor.\n\
           - Run the '{client_exe_name}' on the target machine(s).\n\
           - It is configured to connect to Server Peer ID: {server_actual_peer_id}\n\
           - It will use these bootstrap multiaddresses: {client_bootstrap_list}\n\
        \n\
        Security Considerations:\n\
        - The app-level encryption key is vital for data confidentiality. Keep it secure.\n\
        - The server's libp2p identity seed is critical. If compromised, an attacker could impersonate your server on the P2P network.\n\
        - You are responsible for securing the machine running the Local Log Server.\n\
        - Ensure you have proper consent and adhere to all relevant privacy laws and ethical guidelines when deploying the client monitor.\n",
        app_client_id = app_state.generated_client_id_display,
        app_key_snippet = app_state.generated_key_hex_display_snippet,
        server_actual_peer_id = app_state.generated_server_peer_id_display,
        server_seed_snippet = app_state
            .server_config
            .server_identity_key_seed_hex
            .chars()
            .take(16)
            .collect::<String>(),
        server_exe_name = final_server_exe_name,
        server_p2p_listen_config = app_state.server_config.listen_address,
        server_web_ui_listen_config = app_state.server_config.web_ui_listen_address,
        web_ui_access = local_server_ui_access_address,
        client_exe_name = final_client_exe_name,
        client_bootstrap_list = app_state.client_config.bootstrap_addresses.join(", ")
    );
    fs::write(
        output_dir.join("README_IMPORTANT_INSTRUCTIONS.txt"),
        readme_content,
    )?;

    app_state.status_message = format!(
        "Success! Packages generated in {}. Server PeerID: {}. README_IMPORTANT_INSTRUCTIONS.txt created.",
        output_dir.display(),
        app_state.generated_server_peer_id_display
    );
    app_state.operation_in_progress = false;
    Ok(())
}
