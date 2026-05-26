use camino::Utf8PathBuf;
use thiserror::Error;

/// Errors that can occur when reading the VDB.
#[derive(Error, Debug)]
pub enum Error {
    /// The VDB root directory does not exist or is not a directory.
    #[error("VDB root not found: {0}")]
    RootNotFound(Utf8PathBuf),

    /// A package directory is malformed (e.g. missing required files).
    #[error("malformed package directory {path}: {reason}")]
    MalformedPackage { path: Utf8PathBuf, reason: String },

    /// An I/O error occurred reading a VDB file.
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        source: std::io::Error,
    },
}
