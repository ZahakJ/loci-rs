//! `LociError`: the one error type the engine returns (recall `src/core/api.ts`).
//!
//! Every public operation fails with a code the UI can switch on, a human message and an
//! optional detail. Serialised as `{ code, message, detail? }`, which is what the TypeScript
//! `LociError` carries across the worker boundary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    NotFound,
    Invalid,
    Conflict,
    UnsupportedPackage,
    Corrupt,
    Io,
    Busy,
    Template,
    Media,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::NotFound => "not-found",
            ErrorCode::Invalid => "invalid",
            ErrorCode::Conflict => "conflict",
            ErrorCode::UnsupportedPackage => "unsupported-package",
            ErrorCode::Corrupt => "corrupt",
            ErrorCode::Io => "io",
            ErrorCode::Busy => "busy",
            ErrorCode::Template => "template",
            ErrorCode::Media => "media",
            ErrorCode::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("[{}] {message}", code.as_str())]
pub struct LociError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl LociError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        LociError {
            code,
            message: message.into(),
            detail: None,
        }
    }
    pub fn with_detail(mut self, detail: impl ToString) -> Self {
        self.detail = Some(detail.to_string());
        self
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, m)
    }
    pub fn invalid(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Invalid, m)
    }
    pub fn conflict(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, m)
    }
    pub fn unsupported_package(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::UnsupportedPackage, m)
    }
    pub fn corrupt(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Corrupt, m)
    }
    pub fn io(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Io, m)
    }
    pub fn busy(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Busy, m)
    }
    pub fn template(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Template, m)
    }
    pub fn media(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Media, m)
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, m)
    }
}

impl From<rusqlite::Error> for LociError {
    fn from(e: rusqlite::Error) -> Self {
        match &e {
            rusqlite::Error::SqliteFailure(ffi, _)
                if matches!(
                    ffi.code,
                    rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
                ) =>
            {
                LociError::corrupt(format!("The collection database is damaged: {e}"))
            }
            rusqlite::Error::SqliteFailure(ffi, _)
                if matches!(
                    ffi.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                LociError::busy(format!("The collection is in use: {e}"))
            }
            _ => LociError::internal(format!("Database error: {e}")),
        }
    }
}

impl From<std::io::Error> for LociError {
    fn from(e: std::io::Error) -> Self {
        LociError::io(e.to_string())
    }
}

impl From<serde_json::Error> for LociError {
    fn from(e: serde_json::Error) -> Self {
        LociError::invalid(format!("Invalid JSON: {e}"))
    }
}

pub type Result<T, E = LociError> = std::result::Result<T, E>;
