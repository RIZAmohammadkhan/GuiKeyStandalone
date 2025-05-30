// src/errors.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("Configuration Error: {0}")]
    Config(String),

    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database Error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON Serialization/Deserialization Error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML Deserialization Error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("Hex Decoding Error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("Encryption/Decryption Error: {0}")]
    Crypto(String),

    #[error("HTTP Server Initialization Error: {0}")]
    HttpServerInit(String), // For errors during server setup

    #[error("API Request Error: {0}")]
    ApiRequest(String), // For issues with incoming API requests (e.g., bad payload)

    #[error("Template Rendering Error: {0}")]
    Template(#[from] askama::Error),

    #[error("Internal Server Error: {0}")]
    Internal(String), // Catch-all for unexpected issues
}

// Implement conversion from actix_web error types to ServerError if needed
// This helps in propagating errors cleanly within actix handlers
impl From<actix_web::Error> for ServerError {
    fn from(err: actix_web::Error) -> Self {
        ServerError::HttpServerInit(err.to_string()) // Or a more specific variant
    }
}
impl From<actix_web::error::PayloadError> for ServerError {
    fn from(err: actix_web::error::PayloadError) -> Self {
        ServerError::ApiRequest(format!("Payload error: {}", err))
    }
}

// We can also implement actix_web::ResponseError for ServerError
// to allow our handlers to return Result<_, ServerError> directly.
impl actix_web::ResponseError for ServerError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match *self {
            ServerError::Config(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Io(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Database(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Json(_) => actix_web::http::StatusCode::BAD_REQUEST, // Or internal if it's our serialization
            ServerError::TomlDe(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Hex(_) => actix_web::http::StatusCode::BAD_REQUEST,
            ServerError::Crypto(_) => actix_web::http::StatusCode::BAD_REQUEST, // Or internal if server-side crypto fails
            ServerError::HttpServerInit(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::ApiRequest(_) => actix_web::http::StatusCode::BAD_REQUEST,
            ServerError::Template(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Internal(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse {
        tracing::error!("Responding with error: {}", self);
        actix_web::HttpResponse::build(self.status_code())
            .insert_header(actix_web::http::header::ContentType::plaintext())
            .body(self.to_string())
    }
}
