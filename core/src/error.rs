use thiserror::Error;

/// The central error type for the Taurine engine.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("RPC error: {0}")]
    Rpc(Box<tonic::Status>),

    #[error("Transport error: {0}")]
    Transport(Box<tonic::transport::Error>),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Engine error: {0}")]
    Engine(String),

    #[error("Service error: {0}")]
    Service(String),
}

impl From<tonic::Status> for Error {
    fn from(s: tonic::Status) -> Self {
        Self::Rpc(Box::new(s))
    }
}

impl From<tonic::transport::Error> for Error {
    fn from(e: tonic::transport::Error) -> Self {
        Self::Transport(Box::new(e))
    }
}

/// A specialized Result type for Taurine core operations.
pub type Result<T> = std::result::Result<T, Error>;
