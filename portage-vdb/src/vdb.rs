//! Top-level VDB reader.

use std::path::{Path, PathBuf};

use portage_atom::Cpv;

use crate::Result;
use crate::error::Error;
use crate::package::InstalledPackage;

/// The default VDB path.
pub const DEFAULT_VDB_PATH: &str = "/var/db/pkg";

/// Reader for the Portage installed package database (VDB).
///
/// The VDB lives at `/var/db/pkg` and contains one subdirectory per
/// category, each containing one subdirectory per installed package.
///
/// # Example
///
/// ```no_run
/// use portage_vdb::Vdb;
/// use std::path::Path;
///
/// let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
/// for pkg in vdb.packages() {
///     println!("{}", pkg);
/// }
/// ```
#[derive(Debug)]
pub struct Vdb {
    root: PathBuf,
}

impl Vdb {
    /// Open the VDB at the given root path (typically `/var/db/pkg`).
    pub fn open(path: &Path) -> Result<Self> {
        if path.is_dir() {
            Ok(Self {
                root: path.to_path_buf(),
            })
        } else {
            Err(Error::RootNotFound(path.to_path_buf()))
        }
    }

    /// Open the VDB at the default path (`/var/db/pkg`).
    pub fn open_default() -> Result<Self> {
        Self::open(Path::new(DEFAULT_VDB_PATH))
    }

    /// The root path of this VDB.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Iterate over all installed packages.
    ///
    /// Each category directory is scanned for package directories.
    /// Malformed entries (bad directory names, missing metadata) are skipped.
    pub fn packages(&self) -> impl Iterator<Item = InstalledPackage> + '_ {
        let cat_dirs: Vec<PathBuf> = self.category_dirs();
        cat_dirs.into_iter().flat_map(|cat_dir| {
            let pkgs: Vec<_> = self.packages_in_category_dir(&cat_dir).collect();
            pkgs
        })
    }

    /// List category directory names (e.g. `app-shells`, `sys-libs`).
    pub fn category_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .category_dirs()
            .iter()
            .filter_map(|d| {
                d.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            })
            .collect();
        names.sort();
        names
    }

    /// Iterate over all installed packages in a specific category.
    pub fn packages_in_category(&self, name: &str) -> Vec<InstalledPackage> {
        let cat_dir = self.root.join(name);
        self.packages_in_category_dir(&cat_dir).collect()
    }

    /// Find an installed package by category and PF (e.g. `app-shells/bash-5.3_p9-r2`).
    ///
    /// Returns the first match, or `None` if not found.
    pub fn find(&self, category: &str, pf: &str) -> Option<InstalledPackage> {
        let pkg_dir = self.root.join(category).join(pf);
        if pkg_dir.is_dir() {
            let cpv = parse_cpv_from_parts(category, pf)?;
            Some(InstalledPackage::from_dir(&pkg_dir, category, cpv))
        } else {
            None
        }
    }

    /// Find all installed versions of a package by category and package name.
    ///
    /// E.g. `find_by_cpn("app-shells", "bash")` returns all installed
    /// bash versions.
    pub fn find_by_cpn(&self, category: &str, package: &str) -> Vec<InstalledPackage> {
        self.packages_in_category(category)
            .into_iter()
            .filter(|pkg| pkg.cpn().package.as_ref() == package)
            .collect()
    }

    /// Find which installed package owns a given file path.
    ///
    /// Scans all packages' CONTENTS files. This is O(n) over installed packages.
    pub fn owner(&self, file_path: &Path) -> Option<InstalledPackage> {
        self.packages()
            .find(|pkg| pkg.owns(file_path).unwrap_or(false))
    }

    /// Total number of installed packages.
    pub fn len(&self) -> usize {
        self.packages().count()
    }

    /// Whether the VDB is empty.
    pub fn is_empty(&self) -> bool {
        self.packages().next().is_none()
    }

    // -- internal helpers --

    fn category_dirs(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    }

    fn packages_in_category_dir(
        &self,
        cat_dir: &Path,
    ) -> impl Iterator<Item = InstalledPackage> + '_ {
        let cat_name = cat_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let cat_dir = cat_dir.to_path_buf();

        std::fs::read_dir(&cat_dir)
            .into_iter()
            .flatten()
            .filter_map(move |e| {
                let entry = e.ok()?;
                let path = entry.path();
                if !path.is_dir() {
                    return None;
                }

                let pf = path.file_name()?.to_str()?;
                let cpv = parse_cpv_from_parts(&cat_name, pf)?;

                Some(InstalledPackage::from_dir(&path, &cat_name, cpv))
            })
    }
}

/// Parse a Cpv from the VDB category + PF directory name.
///
/// The VDB stores `category/PF` where PF is `package-version` (no category prefix).
/// `Cpv::parse` expects `category/package-version`, so we concatenate.
fn parse_cpv_from_parts(category: &str, pf: &str) -> Option<Cpv> {
    let full = format!("{category}/{pf}");
    Cpv::parse(&full).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_fake_vdb(dir: &std::path::Path) {
        let pkgs = [
            ("app-shells", "bash-5.3_p9-r2"),
            ("sys-libs", "glibc-2.43-r1"),
        ];

        for (cat, pf) in &pkgs {
            let pkg_dir = dir.join(cat).join(pf);
            fs::create_dir_all(&pkg_dir).unwrap();
            fs::write(pkg_dir.join("DESCRIPTION"), "test package").unwrap();
            fs::write(pkg_dir.join("EAPI"), "8").unwrap();
            fs::write(pkg_dir.join("SLOT"), "0").unwrap();
            fs::write(pkg_dir.join("CONTENTS"), "dir /usr\n").unwrap();
        }
    }

    #[test]
    fn open_and_iterate() {
        let tmp = tempfile::tempdir().unwrap();
        make_fake_vdb(tmp.path());

        let vdb = Vdb::open(tmp.path()).unwrap();
        let mut pkgs: Vec<_> = vdb.packages().map(|p| p.to_string()).collect();
        pkgs.sort();
        assert_eq!(
            pkgs,
            vec!["app-shells/bash-5.3_p9-r2", "sys-libs/glibc-2.43-r1"]
        );
    }

    #[test]
    fn category_names() {
        let tmp = tempfile::tempdir().unwrap();
        make_fake_vdb(tmp.path());

        let vdb = Vdb::open(tmp.path()).unwrap();
        assert_eq!(vdb.category_names(), vec!["app-shells", "sys-libs"]);
    }

    #[test]
    fn find_package() {
        let tmp = tempfile::tempdir().unwrap();
        make_fake_vdb(tmp.path());

        let vdb = Vdb::open(tmp.path()).unwrap();
        let pkg = vdb.find("app-shells", "bash-5.3_p9-r2").unwrap();
        assert_eq!(pkg.pf(), "bash-5.3_p9-r2");
    }

    #[test]
    fn find_missing() {
        let tmp = tempfile::tempdir().unwrap();
        make_fake_vdb(tmp.path());

        let vdb = Vdb::open(tmp.path()).unwrap();
        assert!(vdb.find("app-misc", "nonexistent-1.0").is_none());
    }

    #[test]
    fn find_by_cpn() {
        let tmp = tempfile::tempdir().unwrap();
        make_fake_vdb(tmp.path());

        let vdb = Vdb::open(tmp.path()).unwrap();
        let pkgs = vdb.find_by_cpn("sys-libs", "glibc");
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].pf(), "glibc-2.43-r1");
    }

    #[test]
    fn len_and_empty() {
        let tmp = tempfile::tempdir().unwrap();
        make_fake_vdb(tmp.path());

        let vdb = Vdb::open(tmp.path()).unwrap();
        assert_eq!(vdb.len(), 2);
        assert!(!vdb.is_empty());
    }

    #[test]
    fn empty_vdb() {
        let tmp = tempfile::tempdir().unwrap();
        let vdb = Vdb::open(tmp.path()).unwrap();
        assert!(vdb.is_empty());
        assert_eq!(vdb.len(), 0);
    }

    #[test]
    fn open_missing_root() {
        let result = Vdb::open(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }
}
