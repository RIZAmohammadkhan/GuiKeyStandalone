use crate::application::log_service::LogService;
use crate::domain::event_types::{EventData, LogEvent, ClipboardActivity}; // Import ClipboardActivity
use crate::errors::ServerError;
use actix_web::{web, get, HttpResponse, Responder}; // Responder for HttpResponse
use askama::Template; // <<<<<<<<<<<< ENSURE THIS TRAIT IS IMPORTED
use serde::Deserialize;
use std::marker::PhantomData;

// New struct to pass pre-formatted clipboard data to the template
#[derive(Debug)] // No Serialize/Deserialize needed if only for display
struct DisplayClipboardActivity<'a> {
    timestamp_str: String,
    content_preview: &'a str,
    char_count: usize,
    content_hash_short: String,
}

// New struct to pass pre-formatted event data to the template
struct DisplayLogEvent<'a> {
    id_str: String,
    client_id_str: String,
    application_name: &'a str,
    initial_window_title: &'a str,
    schema_version: u32,
    session_start_str: String,
    session_end_str: String,
    typed_text: &'a str,
    clipboard_actions: Vec<DisplayClipboardActivity<'a>>,
    log_timestamp_str: String, // The LogEvent's main timestamp
}

#[derive(Template)]
#[template(path = "logs_view.html")]
struct LogsViewTemplate<'a> {
    // Pass the display-ready structs
    display_events: Vec<DisplayLogEvent<'a>>,
    current_page: u32,
    total_pages: u32,
    // Marker to help Askama resolve EventData if needed for more complex cases
    // but with pre-formatting, we might not need it.
    _marker: PhantomData<&'a EventData>,
}

#[derive(Template)]
#[template(path = "error_page.html")]
struct ErrorPageTemplate<'a> {
    error_title: &'a str,
    error_message: &'a str,
}

#[derive(Deserialize, Debug)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
}
fn default_page() -> u32 { 1 }
fn default_page_size() -> u32 { 20 }

#[get("/")]
pub async fn index_route() -> impl Responder {
    HttpResponse::Found()
        .append_header((actix_web::http::header::LOCATION, "/logs"))
        .finish()
}

#[get("/logs")]
pub async fn view_logs_route(
    log_service: web::Data<LogService>,
    query_params: web::Query<PaginationParams>,
) -> Result<HttpResponse, ServerError> {
    tracing::info!(
        "WebUI: Request to view logs - page: {}, page_size: {}",
        query_params.page,
        query_params.page_size
    );

    let current_page = query_params.page.max(1);
    let page_size = query_params.page_size.max(1).min(100);

    let events = log_service.get_log_events_paginated(current_page, page_size).await?;
    let total_count = log_service.get_total_log_count().await?;
    
    let total_pages = (total_count as f64 / page_size as f64).ceil() as u32;

    // Prepare data for display to avoid complex logic in template
    let display_events: Vec<DisplayLogEvent> = events.iter().map(|event| {
        let (session_start_str, session_end_str, typed_text_ref, display_clips) = 
            if let EventData::ApplicationActivity { start_time, end_time, typed_text, clipboard_actions } = &event.event_data {
                (
                    start_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                    end_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                    typed_text.as_str(),
                    clipboard_actions.iter().map(|clip| DisplayClipboardActivity {
                        timestamp_str: clip.timestamp.format("%H:%M:%S").to_string(),
                        content_preview: &clip.content_preview,
                        char_count: clip.char_count,
                        content_hash_short: clip.content_hash.chars().take(8).collect(),
                    }).collect()
                )
        } else {
            // Handle case where event_data might not be ApplicationActivity (if your enum grows)
            (String::new(), String::new(), "", Vec::new())
        };

        DisplayLogEvent {
            id_str: event.id.to_string(),
            client_id_str: event.client_id.to_string(),
            application_name: &event.application_name,
            initial_window_title: &event.initial_window_title,
            schema_version: event.schema_version,
            session_start_str,
            session_end_str,
            typed_text: typed_text_ref,
            clipboard_actions: display_clips,
            log_timestamp_str: event.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }).collect();
    
    let template = LogsViewTemplate {
        display_events, // Pass the pre-formatted data
        current_page,
        total_pages: total_pages.max(1),
        _marker: PhantomData,
    };

    match template.render() { // This should now find the render() method
        Ok(html_body) => Ok(HttpResponse::Ok().content_type("text/html; charset=utf-8").body(html_body)),
        Err(askama_err) => {
            tracing::error!("WebUI: Error rendering logs_view template: {}", askama_err);
            Err(ServerError::from(askama_err))
        }
    }
}