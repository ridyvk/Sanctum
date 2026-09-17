use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SanctumError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("vault already exists: {0}")]
    VaultExists(PathBuf),
    #[error("vault is incomplete or missing: {0}")]
    VaultMissing(PathBuf),
    #[error("vault is already open by another process")]
    VaultLocked,
    #[error("block not found: {0}")]
    BlockNotFound(String),
    #[error(
        "concurrent edit detected for {block_id}; expected row {expected}, current row {actual}"
    )]
    Conflict {
        block_id: String,
        expected: i64,
        actual: i64,
    },
    #[error("integrity verification failed: {0}")]
    Integrity(String),
    #[error("backup authentication failed or the password is incorrect")]
    Authentication,
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("operation refused because it would overwrite an existing path: {0}")]
    RefuseOverwrite(PathBuf),
}

pub type Result<T> = std::result::Result<T, SanctumError>;
