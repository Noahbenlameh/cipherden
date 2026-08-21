use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("vault already exists at {0}")]
    AlreadyExists(PathBuf),

    #[error("vault not found at {0}")]
    NotFound(PathBuf),

    #[error("incorrect master password, or vault file is corrupted")]
    InvalidPassword,

    #[error("vault metadata file is missing or malformed: {0}")]
    InvalidMeta(String),

    #[error("entry {0} not found")]
    EntryNotFound(i64),

    #[error("folder {0} not found")]
    FolderNotFound(i64),

    #[error("folder {0} is not empty")]
    FolderNotEmpty(i64),

    #[error("cannot move a folder into itself or one of its own subfolders")]
    CyclicFolderMove,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("metadata (de)serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("key derivation error: {0}")]
    Kdf(String),
}

pub type Result<T> = std::result::Result<T, VaultError>;
