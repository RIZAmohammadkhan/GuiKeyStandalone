// src/main.rs (for local_log_server)

use actix_files::Files;
use actix_web::{middleware::Logger as ActixLogger, web, App, HttpServer};
use std::sync::Arc;
use tracing_subscriber::EnvFilter; // Ensure EnvFilter is imported

mod app_config;
mod errors;
mod domain;
mod infrastructure;
mod application;
mod presentation;

use crate::app_config::ServerSettings;
use crate::infrastructure::database::DbConnection;
use crate::application::log_service::{LogService, spawn_periodic_log_deletion_task};
use crate::presentation::{
    api_handlers::ingest_logs_route,
    web_ui_handlers::{index_route, view_logs_route},
};

fn init_server_diagnostics(log_level_str: &str) {
    // Try to get filter from RUST_LOG, otherwise use the passed string
    let effective_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level_str));
    
    // Store the string representation for logging *before* the filter might be consumed
    let filter_description_for_log = effective_filter.to_string();

    tracing_subscriber::fmt()
        .with_env_filter(effective_filter) // Pass the filter; with_env_filter takes it by value
        .with_thread_ids(true)
        .with_target(true)
        .with_line_number(true)
        .init();
    
    tracing::info!("Local Log Server diagnostic logging initialized. Effective filter set to: '{}'", filter_description_for_log);
}


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let settings = match ServerSettings::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FATAL: Server configuration error: {}. Ensure 'local_server_config.toml' exists and is valid next to the executable.", e);
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    };

    // Assuming you might add a diagnostic_log_level to ServerSettings later
    // For now, using "info" or what RUST_LOG dictates.
    let diagnostic_log_level = "info"; // Or: &settings.diagnostic_log_level;
    init_server_diagnostics(diagnostic_log_level);
    
    tracing::info!("Local Log Server starting up...");
    tracing::debug!(
        "Server Settings: Listen Address='{}', DB Path='{:?}', Log Retention: {} days",
        settings.listen_address,
        settings.database_path,
        settings.log_retention_days
    );

    let db_connection = match DbConnection::new(&settings.database_path) {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!("CRITICAL: Failed to initialize database at {:?}: {}", settings.database_path, e);
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    };
    tracing::info!("Database connection established and tables initialized.");

    let log_service_shared = web::Data::new(LogService::new(db_connection.clone(), Arc::clone(&settings)));
    tracing::info!("LogService initialized and wrapped for Actix.");

    spawn_periodic_log_deletion_task(LogService::new(db_connection, Arc::clone(&settings)));
    tracing::info!("Periodic log deletion task spawned.");
    
    let listen_address = settings.listen_address.clone();
    tracing::info!("Attempting to bind HTTP server to: {}", listen_address);

    let server_future = HttpServer::new(move || {
        App::new()
            .wrap(ActixLogger::default())
            .app_data(log_service_shared.clone())
            .service(ingest_logs_route)
            .service(index_route)
            .service(view_logs_route)
            .service(Files::new("/static", "./static"))
    })
    .bind(&listen_address)?
    .workers(2)
    .run();

    tracing::info!("Local Log Server started successfully on http://{}", listen_address.replace("0.0.0.0", "127.0.0.1"));
    tracing::info!("API endpoint for client: http://{}/api/log", listen_address.replace("0.0.0.0", "127.0.0.1"));
    tracing::info!("Press Ctrl+C to stop the server.");

    server_future.await?;
    tracing::info!("Local Log Server has shut down.");
    Ok(())
}