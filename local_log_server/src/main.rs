// --- local_log_server/src/main.rs ---
use actix_files::Files;
use actix_web::{dev::ServerHandle, middleware::Logger as ActixLogger, web, App, HttpServer};
use std::sync::Arc;
use tokio::sync::watch; // For shutdown signaling
use tracing_subscriber::EnvFilter;

mod app_config;
mod errors;
mod domain;
mod infrastructure;
mod application;
mod presentation;
mod p2p; // Our new P2P module

use crate::app_config::ServerSettings;
use crate::infrastructure::database::DbConnection;
use crate::application::log_service::{LogService, spawn_periodic_log_deletion_task};
use crate::presentation::web_ui_handlers::{index_route, view_logs_route};
// Removed: use crate::presentation::api_handlers::ingest_logs_route; // Ingestion via P2P
use crate::p2p::swarm_manager::run_server_swarm_manager;

fn init_server_diagnostics(log_level_str: &str) {
    let effective_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level_str));
    let filter_description_for_log = effective_filter.to_string();
    tracing_subscriber::fmt()
        .with_env_filter(effective_filter)
        .with_thread_ids(true)
        .with_target(true)
        .with_line_number(true)
        .init();
    tracing::info!("Server: Diagnostic logging initialized. Effective filter: '{}'", filter_description_for_log);
}

#[tokio::main] // Changed from actix_web::main
async fn main() -> std::io::Result<()> {
    // Load settings first
    let settings = match ServerSettings::new() {
        Ok(s) => s,
        Err(e) => {
            // Use eprintln before logger is initialized
            eprintln!("FATAL: Server configuration error: {}. Ensure 'local_server_config.toml' exists and is valid.", e);
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    };

    // Initialize server diagnostics (tracing)
    init_server_diagnostics("info"); // Default to "info", or use a setting
    
    tracing::info!("Server: Starting Local Log Server (P2P Mode)...");
    tracing::info!("Server: Configured P2P Listen Multiaddr: '{}'", settings.p2p_listen_address);
    tracing::info!("Server: Configured Web UI Listen Address: '{}'", settings.web_ui_listen_address);
    tracing::debug!("Server: DB Path='{:?}', Log Retention: {} days", settings.database_path, settings.log_retention_days);

    // Initialize Database Connection
    let db_connection = match DbConnection::new(&settings.database_path) {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!("CRITICAL: Server: Failed to initialize database at {:?}: {}", settings.database_path, e);
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    };
    tracing::info!("Server: Database connection established and tables initialized.");

    // Initialize LogService (shared across P2P and Web UI)
    let log_service = LogService::new(db_connection.clone(), Arc::clone(&settings));
    tracing::info!("Server: LogService initialized.");

    // Spawn periodic task for deleting old logs
    spawn_periodic_log_deletion_task(log_service.clone());
    tracing::info!("Server: Periodic log deletion task manager spawned.");
    
    // --- Shutdown Signaling ---
    // This channel signals long-running tasks like the P2P manager to shut down.
    let (shutdown_tx, shutdown_rx_p2p) = watch::channel(false);

    // --- P2P Swarm Task ---
    let p2p_log_service_clone = log_service.clone();
    let p2p_settings_clone = Arc::clone(&settings);
    let p2p_manager_task = tokio::spawn(async move {
        tracing::info!("Server: P2P Swarm Manager task starting...");
        if let Err(e) = run_server_swarm_manager(p2p_settings_clone, p2p_log_service_clone, shutdown_rx_p2p).await {
            tracing::error!("Server: P2P Swarm Manager exited with error: {}", e);
        } else {
            tracing::info!("Server: P2P Swarm Manager exited gracefully.");
        }
    });
    tracing::info!("Server: P2P Swarm Manager task spawned.");

    // --- Actix Web UI Server ---
    let web_ui_log_service_shared = web::Data::new(log_service.clone()); // Share LogService
    let web_ui_listen_address = settings.web_ui_listen_address.clone();
    tracing::info!("Server: Attempting to bind Web UI HTTP server to: {}", web_ui_listen_address);

    let actix_server = HttpServer::new(move || {
        App::new()
            .wrap(ActixLogger::default()) // Actix's own request logger
            .app_data(web_ui_log_service_shared.clone())
            // Note: No direct /api/log for HTTP POST anymore. Ingestion is via P2P.
            .service(index_route)  // Redirects to /logs
            .service(view_logs_route) // Serves the log viewing page
            .service(Files::new("/static", "./static")) // Serves CSS, JS, etc.
    })
    .bind(&web_ui_listen_address)?
    .workers(2) // Adjust as needed
    .disable_signals() // Important: We handle Ctrl+C with tokio::signal
    .run();

    let actix_server_handle: ServerHandle = actix_server.handle(); // Get handle for graceful shutdown
    tokio::spawn(actix_server); // Spawn the server to run
    
    tracing::info!("Server: Web UI server started successfully on http://{}", web_ui_listen_address.replace("0.0.0.0", "127.0.0.1"));
    tracing::info!("Server: P2P Log Ingestion service is also running. Monitor P2P manager logs for PeerID and listening addresses.");
    tracing::info!("Server: Press Ctrl+C to stop.");

    // --- Graceful Shutdown Handling ---
    // Wait for Ctrl+C or for one of the main tasks to exit.
    tokio::select! {
        biased; // Prioritize Ctrl+C for shutdown initiation

        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Server: Ctrl+C received. Initiating shutdown sequence...");
        }
        
        // This branch handles if the p2p_manager_task exits prematurely (e.g., due to an unrecoverable error)
        p2p_join_result = p2p_manager_task => { // Re-assign to avoid move error if used later
            match p2p_join_result {
                Ok(_) => tracing::info!("Server: P2P Swarm Manager task completed (possibly due to internal error or signal)."),
                Err(e) => tracing::error!("Server: P2P Swarm Manager task panicked or failed: {}", e),
            }
            tracing::info!("Server: P2P Swarm Manager has exited. Initiating shutdown of other components...");
        }
    }

    // 1. Signal P2P Swarm Manager to shut down
    tracing::info!("Server: Sending shutdown signal to P2P Swarm Manager...");
    if shutdown_tx.send(true).is_err() {
        tracing::warn!("Server: Failed to send shutdown signal to P2P manager (receiver likely already dropped).");
    }
    // Note: We don't explicitly await the p2p_manager_task again here if Ctrl+C was the trigger,
    // as it's expected to shut down based on the watch channel signal. If it exited on its own,
    // the select block above already handled its completion.

    // 2. Request Actix Web UI server to stop gracefully
    tracing::info!("Server: Requesting Actix Web UI server to stop gracefully (timeout 10s)...");
    actix_server_handle.stop(true).await; // `true` for graceful shutdown
    tracing::info!("Server: Actix Web UI server stop request completed.");

    tracing::info!("Server: Shutdown sequence complete. Exiting.");
    Ok(())
}