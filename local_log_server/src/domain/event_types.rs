// src/domain/event_types.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a single, distinct block of user activity or a system event.
/// The `timestamp` field typically denotes the start of this activity block.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogEvent {
    /// Unique identifier for this log event.
    pub id: Uuid,
    /// Identifier for the client that generated this event.
    pub client_id: Uuid,
    /// Primary timestamp for the event, often the start of an activity session.
    pub timestamp: DateTime<Utc>,
    /// Name of the application associated with this activity block.
    pub application_name: String,
    /// The title of the application window when this activity block began.
    pub initial_window_title: String,
    /// The specific type and data of the event.
    pub event_data: EventData,
    /// Version of this log event schema, for future compatibility.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

/// Current schema version for LogEvent.
fn default_schema_version() -> u32 {
    2 // Matches the client's schema version for ApplicationActivity
}

/// Enum representing the different kinds of data that can be logged.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data")] // For clear JSON structure: { "type": "ApplicationActivity", "data": { ... } }
pub enum EventData {
    /// Represents a consolidated block of user activity within a single application.
    ApplicationActivity {
        /// Timestamp when this specific application session/activity block started.
        start_time: DateTime<Utc>,
        /// Timestamp when this specific application session/activity block ended
        /// (e.g., due to application switch, periodic flush, or client shutdown).
        end_time: DateTime<Utc>,
        /// All characters typed by the user during this session in this application,
        /// potentially including representations of special keys like "[ENTER]".
        typed_text: String,
        /// A list of clipboard copy actions that occurred during this application session.
        clipboard_actions: Vec<ClipboardActivity>,
        // /// Optional: A list of all distinct window titles encountered during this session,
        // /// if the title changed while the user was still in the same application.
        // distinct_window_titles_during_session: Option<Vec<String>>,
    },
    /*
    // Example of how you might add other top-level event types later:
    ClientStatus {
        status_time: DateTime<Utc>,
        status_type: ClientStatusType, // e.g., Started, Stopped, Heartbeat
        message: Option<String>,
    },
    */
}

/// Represents a single clipboard copy action.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipboardActivity {
    /// Precise timestamp of when this clipboard copy occurred.
    pub timestamp: DateTime<Utc>,
    /// SHA256 hash of the full clipboard content to detect duplicates or for brevity.
    pub content_hash: String,
    /// A short preview of the copied text content.
    pub content_preview: String,
    /// The total number of characters in the copied content.
    pub char_count: usize,
}

/*
// Example for future extensibility
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientStatusType {
    Started,
    Stopped,
    Heartbeat,
    ErrorCondition,
}
*/
