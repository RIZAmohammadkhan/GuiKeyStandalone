use crate::app_config::Settings;
use crate::errors::AppError;
use std::sync::Arc;
use std::str::FromStr;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, EnvFilter, Layer, prelude::*};

pub fn init_logging(settings: &Arc<Settings>) -> Result<(), AppError> {
    // Create separate EnvFilter instances for each layer if they might differ or to avoid clone issues.
    let file_log_level_filter = EnvFilter::from_str(&settings.internal_log_level)
        .map_err(|e| AppError::Config(format!("Invalid internal_log_level for file: '{}': {}", settings.internal_log_level, e)))?;

    let log_dir = &settings.internal_log_file_dir;
    
    if !log_dir.exists() {
        std::fs::create_dir_all(log_dir)
            .map_err(|e| AppError::Initialization(format!("Failed to create log directory {:?}: {}", log_dir, e)))?;
    }
    
    let file_appender = rolling::daily(log_dir, &settings.internal_log_file_name);
    let (non_blocking_writer, guard) = tracing_appender::non_blocking(file_appender);
    
    let file_layer = fmt::layer()
        .with_writer(non_blocking_writer)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_filter(file_log_level_filter); // Apply the filter for the file layer

    // Start with the registry and add the file layer.
    // The type of subscriber_builder will change as layers are added.
    let subscriber = tracing_subscriber::registry().with(file_layer);

    #[cfg(debug_assertions)]
    let subscriber = { // This shadows the previous `subscriber`, creating a new one with an added layer
        let console_log_level_filter = EnvFilter::from_str(&settings.internal_log_level)
            .map_err(|e| AppError::Config(format!("Invalid internal_log_level for console: '{}': {}", settings.internal_log_level, e)))?;

        let console_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_thread_ids(true)
            .with_filter(console_log_level_filter);
        
        subscriber.with(console_layer) // Add the console layer to the existing subscriber
    };
    // #[cfg(not(debug_assertions))]
    // let subscriber = subscriber; // If not debug, `subscriber` remains the one with just the file layer.

    subscriber.try_init()
        .map_err(|e| AppError::Initialization(format!("Failed to set global tracing subscriber: {}", e)))?;

    std::mem::forget(guard);

    tracing::info!(
        "Internal diagnostics logger initialized. Level: {}, Output Directory: {:?}, File Name: {}",
        settings.internal_log_level,
        settings.internal_log_file_dir,
        settings.internal_log_file_name
    );

    Ok(())
}