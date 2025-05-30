// src/processing/event_processor.rs

use crate::app_config::Settings;
use crate::core_monitors::clipboard_capture::RawClipboardData;
use crate::core_monitors::keyboard_capture::RawKeyboardData;
use crate::errors::AppError; // Assuming this is in crate::errors
use crate::event_types::{ClipboardActivity, EventData, LogEvent}; // Assuming these are in crate::event_types
use crate::storage::log_store::LogStoreHandle; // Assuming this is in crate::storage::log_store
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Interval, MissedTickBehavior, interval}; // For hashing clipboard content

struct CurrentSession {
    application_name: String,
    initial_window_title: String,
    latest_window_title: String, // Track the most recent title within the session
    start_time: DateTime<Utc>,
    typed_text: String,
    clipboard_actions: Vec<ClipboardActivity>,
}

impl CurrentSession {
    fn new(app_name: String, window_title: String, start_time: DateTime<Utc>) -> Self {
        CurrentSession {
            application_name: app_name,
            initial_window_title: window_title.clone(),
            latest_window_title: window_title,
            start_time,
            typed_text: String::new(),
            clipboard_actions: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.typed_text.is_empty() && self.clipboard_actions.is_empty()
    }
}

pub async fn run_event_processor(
    settings: Arc<Settings>,
    mut raw_keyboard_rx: mpsc::Receiver<RawKeyboardData>,
    mut raw_clipboard_rx: mpsc::Receiver<RawClipboardData>,
    log_store: LogStoreHandle,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), AppError> {
    tracing::info!(
        "Event processor started. Grouping by application. Periodic flush: {}s",
        settings.processor_periodic_flush_interval_secs
    );

    let mut current_session: Option<CurrentSession> = None;

    let mut periodic_flush_interval_opt: Option<Interval> =
        if settings.processor_periodic_flush_interval_secs > 0 {
            let mut intv = interval(Duration::from_secs(
                settings.processor_periodic_flush_interval_secs,
            ));
            intv.set_missed_tick_behavior(MissedTickBehavior::Delay);
            Some(intv)
        } else {
            None
        };

    loop {
        let tick_future = async {
            if let Some(ref mut interval) = periodic_flush_interval_opt.as_mut() {
                if current_session.is_some() {
                    interval.tick().await;
                    return Some(());
                }
            }
            std::future::pending().await // Pend if no interval or no session
        };

        tokio::select! {
            biased;

            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow_and_update() {
                    tracing::info!("Event processor: Shutdown signal received.");
                    if let Some(session) = current_session.take() {
                        if !session.is_empty() {
                            finalize_and_store_session(session, Utc::now(), &settings, &log_store).await;
                        }
                    }
                    break;
                }
            }

            maybe_tick_completed = tick_future => {
                if maybe_tick_completed.is_some() {
                    if let Some(session) = current_session.take() {
                         tracing::debug!("Event processor: Periodic flush for app: {}", session.application_name);
                         finalize_and_store_session(session, Utc::now(), &settings, &log_store).await;
                    }
                }
            }

            Some(kbd_data) = raw_keyboard_rx.recv() => {
                tracing::trace!("Event processor: RawKbd: '{}' in App:'{}'", kbd_data.key_value, kbd_data.foreground_app_name);
                match current_session.as_mut() {
                    Some(session) if session.application_name == kbd_data.foreground_app_name => {
                        if kbd_data.is_char && !kbd_data.key_value.starts_with('[') {
                            session.typed_text.push_str(&kbd_data.key_value);
                        } else if !kbd_data.is_char {
                            session.typed_text.push_str(&format!("{} ", kbd_data.key_value.trim()));
                        }
                        session.latest_window_title = kbd_data.foreground_window_title;
                    }
                    _ => {
                        if let Some(old_session) = current_session.take() {
                            if !old_session.is_empty() {
                                finalize_and_store_session(old_session, kbd_data.timestamp, &settings, &log_store).await;
                            }
                        }
                        let mut new_session = CurrentSession::new(
                            kbd_data.foreground_app_name.clone(),
                            kbd_data.foreground_window_title.clone(),
                            kbd_data.timestamp
                        );
                        if kbd_data.is_char && !kbd_data.key_value.starts_with('[') {
                            new_session.typed_text.push_str(&kbd_data.key_value);
                        } else if !kbd_data.is_char {
                            new_session.typed_text.push_str(&format!("{} ", kbd_data.key_value.trim()));
                        }
                        current_session = Some(new_session);
                    }
                }
            }

            Some(clip_data) = raw_clipboard_rx.recv() => {
                tracing::trace!("Event processor: RawClip in App:'{}'", clip_data.foreground_app_name);
                let clipboard_activity = ClipboardActivity {
                    timestamp: clip_data.timestamp,
                    content_hash: {
                        let mut hasher = Sha256::new();
                        hasher.update(clip_data.text_content.as_bytes());
                        format!("{:x}", hasher.finalize())
                    },
                    content_preview: clip_data.text_content.chars().take(100).collect(),
                    char_count: clip_data.text_content.chars().count(),
                };

                match current_session.as_mut() {
                    Some(session) if session.application_name == clip_data.foreground_app_name => {
                        session.clipboard_actions.push(clipboard_activity);
                        session.latest_window_title = clip_data.foreground_window_title;
                    }
                    _ => {
                        if let Some(old_session) = current_session.take() {
                             if !old_session.is_empty() {
                                finalize_and_store_session(old_session, clip_data.timestamp, &settings, &log_store).await;
                            }
                        }
                        let mut new_session = CurrentSession::new(
                            clip_data.foreground_app_name.clone(),
                            clip_data.foreground_window_title.clone(),
                            clip_data.timestamp
                        );
                        new_session.clipboard_actions.push(clipboard_activity);
                        current_session = Some(new_session);
                    }
                }
            }

            else => {
                tracing::info!("Event processor: Input channels closed. Finalizing any pending session.");
                if let Some(session) = current_session.take() {
                     if !session.is_empty() {
                        finalize_and_store_session(session, Utc::now(), &settings, &log_store).await;
                    }
                }
                break;
            }
        }
    }
    tracing::info!("Event processor shut down.");
    Ok(())
}

async fn finalize_and_store_session(
    session: CurrentSession,
    end_time: DateTime<Utc>,
    settings: &Arc<Settings>,
    log_store: &LogStoreHandle,
) {
    if session.is_empty() {
        tracing::trace!(
            "Event processor: Skipping storage of empty session for app: {}",
            session.application_name
        );
        return;
    }

    tracing::debug!(
        "Event processor: Finalizing session for app: '{}', initial_title: '{}', typed_len: {}, clips: {}, start: {}, end: {}",
        session.application_name,
        session.initial_window_title,
        session.typed_text.len(), // Using len() for byte length, chars().count() for char count
        session.clipboard_actions.len(),
        session.start_time,
        end_time
    );

    let log_event = LogEvent::new_application_activity(
        settings.client_id,
        session.application_name,
        session.initial_window_title, // Could use session.latest_window_title if preferred
        session.start_time,
        end_time,
        session.typed_text.trim_end().to_string(),
        session.clipboard_actions,
    );

    if let Err(e) = log_store.add_event(log_event).await {
        tracing::error!(
            "Event processor: Failed to store finalized session event: {}",
            e
        );
    }
}
