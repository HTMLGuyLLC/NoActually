//! The store's error type.

use std::path::PathBuf;

/// What can go wrong in the store. File errors carry the path.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// SQLite refused (a locked or corrupt database, a full disk).
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    /// The database folder couldn't be created.
    #[error("{}: {source}", path.display())]
    Io {
        /// The folder or file involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A setting couldn't be encoded.
    #[error("bad setting: {0}")]
    Json(#[from] serde_json::Error),
    /// Reading a source failed (an unknown id, say).
    #[error(transparent)]
    Source(#[from] sources::Error),
    /// A value the user picked isn't allowed. The message is written for them.
    #[error("{0}")]
    Invalid(&'static str),
    /// The database was written by a newer version of the app.
    #[error("this database was made by a newer version of No, Actually... (format {found}, this version reads up to {known}). Update the app to open it.")]
    NewerDatabase {
        /// The format the file is in.
        found: usize,
        /// The newest format this build reads.
        known: usize,
    },
    /// The OS has no per-user data folder, and no database path was given.
    #[error("can't find your app data folder; set {} to choose a database file", crate::DB_ENV)]
    NoDataDir,
}

/// Shorthand for results from this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
