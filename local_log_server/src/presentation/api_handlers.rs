use crate::application::log_service::LogService;
// use crate::errors::ServerError; // ServerError is used via Result's Err variant
use actix_web::{HttpRequest, HttpResponse, post, web};
use bytes::Bytes;

const MAX_PAYLOAD_SIZE: usize = 10 * 1024 * 1024;

#[post("/api/log")]
pub async fn ingest_logs_route(
    req: HttpRequest,
    log_service: web::Data<LogService>,
    payload: Bytes,
) -> Result<HttpResponse, crate::errors::ServerError> {
    // Explicit Result with ServerError
    let client_id = match req.headers().get("X-Client-ID") {
        Some(val) => match val.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                tracing::warn!("API: Received X-Client-ID header with non-UTF8 characters.");
                // Directly return HttpResponse for early exits before main logic
                return Ok(HttpResponse::BadRequest()
                    .body("X-Client-ID header contains invalid characters."));
            }
        },
        None => {
            tracing::warn!("API: Received log submission without X-Client-ID header.");
            "UnknownClient".to_string()
        }
    };

    if payload.is_empty() {
        tracing::warn!("API: Received empty payload from client_id: {}", client_id);
        return Ok(HttpResponse::BadRequest().body("Empty payload received."));
    }

    if payload.len() > MAX_PAYLOAD_SIZE {
        tracing::warn!(
            "API: Payload from client_id: {} exceeds max size of {} bytes. Received: {} bytes.",
            client_id,
            MAX_PAYLOAD_SIZE,
            payload.len()
        );
        return Ok(HttpResponse::PayloadTooLarge().body(format!(
            "Payload exceeds maximum size of {} bytes.",
            MAX_PAYLOAD_SIZE
        )));
    }

    tracing::info!(
        "API: Received log data from client_id: {}, payload size: {} bytes.",
        client_id,
        payload.len()
    );

    match log_service
        .ingest_log_batch(&client_id, payload.to_vec())
        .await
    {
        Ok(num_events_ingested) => {
            tracing::info!(
                "API: Successfully ingested {} events for client_id: {}",
                num_events_ingested,
                client_id
            );
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "status": "success",
                "message": format!("Successfully processed {} log events.", num_events_ingested)
            })))
        }
        Err(e) => {
            tracing::error!(
                "API: Error processing log batch for client_id: {}: {}",
                client_id,
                e
            );
            Err(e) // Propagate ServerError, Actix will use its ResponseError impl
        }
    }
}
