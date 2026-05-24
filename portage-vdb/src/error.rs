use thiserror::Error;

/// Errors that can occur when reading the VDB.
#[derive(Error, Debug)]
pub enum Error {
    /// The VDB root directory does not exist or is not a directory.
    #[error("VDB root not found: {0}")]
    RootNotFound(std::path::PathBuf),

    /// A package directory is malformed (e.g. missing required files).
    #[error("malformed package directory {path}: {reason}")]
    MalformedPackage {
        path: std::path::PathBuf,
        reason: String,
    },

    /// An I/O error occurred reading a VDB file.
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    /// Failed to parse an atom (Cpn/Cpv) from a package directory name.
    #[error("failed to parse atom from {name}: {source}")]
    AtomParse {
        name: String,
        source: portage_atom::Error,
    },
}
