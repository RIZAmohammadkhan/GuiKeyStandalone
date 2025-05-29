use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogEvent {
    pub id: Uuid,
    pub client_id: Uuid,
    pub timestamp: DateTime<Utc>, // Represents the start_time of the ApplicationActivity block
    pub application_name: String,
    pub initial_window_title: String,
    pub event_data: EventData,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

fn default_schema_version() -> u32 { 2 } // Start with schema version 2 for this new format

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data")] // "type" will be "ApplicationActivity"
pub enum EventData {
    ApplicationActivity {
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        typed_text: String,
        clipboard_actions: Vec<ClipboardActivity>,
        // final_window_title: String, // Optional: title at the end of the session
    },
    // Could add other distinct event types here if needed, e.g., SystemStatus, ClientStart, ClientStop
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipboardActivity {
    pub timestamp: DateTime<Utc>, // Specific timestamp of this clipboard action
    pub content_hash: String,
    pub content_preview: String,
    pub char_count: usize,
}

impl LogEvent {
    pub fn new_application_activity(
        client_id: Uuid,
        application_name: String,
        initial_window_title: String,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        typed_text: String,
        clipboard_actions: Vec<ClipboardActivity>,
    ) -> Self {
        LogEvent {
            id: Uuid::new_v4(),
            client_id,
            timestamp: start_time, // Main LogEvent timestamp is the session start
            application_name,
            initial_window_title,
            event_data: EventData::ApplicationActivity {
                start_time,
                end_time,
                typed_text,
                clipboard_actions,
            },
            schema_version: default_schema_version(),
        }
    }
}