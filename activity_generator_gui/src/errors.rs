// src/errors.rs (for activity_generator_gui)
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GeneratorError {
    #[error("I/O Error: {source}")]
    Io { #[from] source: std::io::Error },

    #[error("TOML Serialization Error: {source}")]
    TomlSer { #[from] source: toml::ser::Error },

    // If you were to use JSON for server config and deserialize it:
    // #[error("JSON Serialization Error: {source}")]
    // JsonSer { #[from] source: serde_json::Error },
    // #[error("JSON Deserialization Error: {0}")]
    // JsonDe(String),

    #[error("Input not provided or invalid: {field}: {message}")]
    InputValidation { field: String, message: String },

    #[error("File or path operation error: {0}")]
    PathError(String),

    #[error("A required bundled binary template was not found: {binary_name} at path {path_searched}")]
    BinaryTemplateNotFound { binary_name: String, path_searched: String },

    #[error("fs_extra Error: {source}")]
    FsExtra { #[from] source: fs_extra::error::Error },

    #[error("Hex Encoding Error (should not happen with internal key gen): {source}")]
    HexEncoding { #[from] source: hex::FromHexError },

    #[error("An unexpected internal error occurred: {0}")]
    Other(String),
}