//! Gentoo ebuild repository layout reader based on the
//! [Package Manager Specification (PMS)](https://projects.gentoo.org/pms/9/pms.html).
//!
//! This crate provides types for reading and navigating a Gentoo ebuild
//! repository: `metadata/layout.conf`, category and package directory
//! enumeration, profiles, metadata cache access, and ebuild/eclass sourcing
//! via an embedded bash shell ([brush](https://crates.io/crates/brush-core)).
//!
//! # Quick start
//!
//! ```no_run
//! use portage_repo::Repository;
//!
//! let repo = Repository::open("/var/db/repos/gentoo").unwrap();
//! println!("repo: {} (masters: {:?})", repo.name(), repo.layout().masters);
//!
//! for cat in repo.categories() {
//!     for pkg in cat.packages() {
//!         for ebuild in pkg.ebuilds().unwrap() {
//!             println!("{}", ebuild.cpv());
//!         }
//!     }
//! }
//! ```
//!
//! # Crate family
//!
//! - [`portage-atom`](https://crates.io/crates/portage-atom) — PMS atom parser
//! - [`portage-metadata`](https://crates.io/crates/portage-metadata) — metadata cache types
//! - `portage-repo` (this crate) — repository layout reader
//!
//! > **Warning**: This codebase was largely AI-generated and has not yet been
//! > thoroughly audited. It may contain bugs, incomplete PMS coverage, or
//! > surprising edge-case behaviour. Use at your own risk.

pub(crate) mod build;
pub mod cache;
mod error;
pub mod make_conf;
pub(crate) mod repo;
pub mod source;

pub use build::inherit;

pub use error::{Error, Result};

// Re-export the most-used types at crate root for backwards compat
pub use build::EbuildShell;
pub use cache::{
    CacheReadOpts, RegenOpts, RegenStats, cache_cpvs, cache_entries_parallel, regen_cache,
};
pub use gentoo_core::arch::ExoticKey;
pub use gentoo_core::{Arch, KnownArch, arch};
pub use portage_metadata::EbuildMetadata;
pub use portage_metadata::interner::{DefaultInterner, GlobalInterner, Interner, NoInterner};
pub use repo::{Categories, CategoriesIter, Category, Packages, PackagesIter};
pub use repo::Ebuild;
pub use repo::LayoutConf;
pub use repo::Package;
pub use make_conf::{MakeConf, QuoteStyle, DEFAULT_MAKE_CONF, LEGACY_MAKE_CONF};
pub use repo::{Maintainer, MaintainerKind, PkgMetadata};
pub use repo::UseExpand;
pub use repo::{CacheEntries, CacheEntriesIter, Ebuilds, EbuildsIter, ProfileUpdate, Repository};
pub use repo::{Manifest, ManifestEntry};
pub use repo::{Profile, ProfileDesc, ProfileStack, ProfileStatus};
pub use repo::{RepoEntry, ReposConf};
pub use source::{SourceContext, SourceOpts, source_parallel, source_single};
