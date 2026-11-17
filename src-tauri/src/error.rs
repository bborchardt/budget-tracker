use serde::{Serialize, Serializer};

/// Errors returned to the frontend.
///
/// Messages must describe the problem without including transaction data (descriptions,
/// amounts, balances), per AGENTS.md.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("data format error: {0}")]
    Json(#[from] serde_json::Error),
}

impl AppError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        AppError::Invalid(msg.into())
    }
    pub fn not_found(what: impl Into<String>) -> Self {
        AppError::NotFound(what.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
