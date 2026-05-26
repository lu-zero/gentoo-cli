//! Gentoo Portage VDB (installed package database) reader.
//!
//! Reads `/var/db/pkg` to provide access to installed package metadata,
//! USE flags, dependencies, and file ownership information.
//!
//! # Example
//!
//! ```no_run
//! use portage_vdb::Vdb;
//!
//! let vdb = Vdb::open("/var/db/pkg").unwrap();
//! for cat in vdb.categories() {
//!     for pkg in cat.packages() {
//!         println!("{}", pkg);
//!     }
//! }
//! ```

pub mod category;
mod contents;
mod error;
mod package;
mod vdb;

pub use category::{Categories, CategoriesIter, Category, Packages, PackagesIter};
pub use contents::{ContentsEntry, ContentsKind};
pub use error::Error;
pub use package::InstalledPackage;
pub use vdb::{AllPackages, AllPackagesIter, Vdb};

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, Error>;
