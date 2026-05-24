//! Gentoo Portage VDB (installed package database) reader.
//!
//! Reads `/var/db/pkg` to provide access to installed package metadata,
//! USE flags, dependencies, and file ownership information.
//!
//! # Example
//!
//! ```no_run
//! use portage_vdb::Vdb;
//! use std::path::Path;
//!
//! let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
//! for pkg in vdb.packages() {
//!     println!("{}", pkg);
//! }
//! ```

mod contents;
mod error;
mod package;
mod vdb;

pub use contents::{ContentsEntry, ContentsKind};
pub use error::Error;
pub use package::InstalledPackage;
pub use vdb::Vdb;

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, Error>;
