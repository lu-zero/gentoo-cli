use camino::{Utf8Path, Utf8PathBuf};

use super::package::Package;
use super::util;
use crate::error::Result;

/// A category directory within an ebuild repository.
///
/// Represents a directory such as `dev-lang/` containing package directories.
///
/// See [PMS 4](https://projects.gentoo.org/pms/9/pms.html#tree-layout).
#[derive(Debug, Clone)]
pub struct Category {
    name: String,
    path: Utf8PathBuf,
}

impl Category {
    pub(crate) fn new(name: String, path: Utf8PathBuf) -> Self {
        Self { name, path }
    }

    /// The category name (e.g. `dev-lang`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Absolute path to the category directory.
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// Whether the category directory exists on disk.
    pub fn exists(&self) -> bool {
        self.path.is_dir()
    }

    /// List all packages in this category.
    ///
    /// Returns package directories sorted by name. Non-directory entries and
    /// dotfiles are skipped.
    pub fn packages(&self) -> Result<Vec<Package>> {
        let entries = match std::fs::read_dir(&self.path) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(util::io_err(self.path.as_std_path(), e)),
        };

        let mut packages = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| util::io_err(self.path.as_std_path(), e))?;
            // file_type() reads `d_type` from getdents() on Linux filesystems
            // that fill it in (ext4/btrfs/xfs/tmpfs), avoiding a per-entry
            // stat(). For symlinks we have to follow with metadata() — some
            // overlays (notably crossdev) symlink their package dirs at the
            // gentoo originals, so dropping symlinks-to-dirs here would lose
            // those packages entirely.
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let is_dir = if ft.is_dir() {
                true
            } else if ft.is_symlink() {
                // DirEntry::metadata is lstat — won't follow. Use the free
                // fs::metadata (stat) so we see whether the target is a dir.
                std::fs::metadata(entry.path())
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
            } else {
                false
            };
            if !is_dir {
                continue;
            }
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name.starts_with('.') || name == "CVS" {
                continue;
            }
            let path: Utf8PathBuf = match entry.path().try_into() {
                Ok(p) => p,
                Err(_) => continue,
            };
            packages.push(Package::new(&self.name, name.into_owned(), path));
        }
        packages.sort_by(|a, b| a.name().cmp(b.name()));
        Ok(packages)
    }

    /// Look up a specific package by name.
    pub fn package(&self, name: &str) -> Option<Package> {
        let path = self.path.join(name);
        if path.is_dir() {
            Some(Package::new(&self.name, name.to_string(), path))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("metadata")).unwrap();
        std::fs::write(root.join("metadata/layout.conf"), "masters =\n").unwrap();
        std::fs::create_dir_all(root.join("profiles")).unwrap();
        std::fs::write(root.join("profiles/repo_name"), "test\n").unwrap();
        tmp
    }

    #[test]
    fn packages_lists_subdirectories() {
        let tmp = setup_repo();
        let cat_dir = tmp.path().join("dev-util");
        std::fs::create_dir_all(cat_dir.join("foo")).unwrap();
        std::fs::create_dir_all(cat_dir.join("bar")).unwrap();
        std::fs::write(cat_dir.join("README"), "not a package").unwrap();

        let cat = Category::new("dev-util".into(), cat_dir.try_into().unwrap());
        let pkgs = cat.packages().unwrap();
        let names: Vec<&str> = pkgs.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["bar", "foo"]);
    }

    #[test]
    fn packages_skips_dotfiles() {
        let tmp = setup_repo();
        let cat_dir = tmp.path().join("dev-util");
        std::fs::create_dir_all(cat_dir.join("foo")).unwrap();
        std::fs::create_dir_all(cat_dir.join(".hidden")).unwrap();

        let cat = Category::new("dev-util".into(), cat_dir.try_into().unwrap());
        let pkgs = cat.packages().unwrap();
        let names: Vec<&str> = pkgs.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn package_lookup_existing() {
        let tmp = setup_repo();
        let cat_dir = tmp.path().join("dev-util");
        std::fs::create_dir_all(cat_dir.join("foo")).unwrap();

        let cat = Category::new("dev-util".into(), cat_dir.try_into().unwrap());
        assert!(cat.package("foo").is_some());
        assert!(cat.package("nonexistent").is_none());
    }

    #[test]
    fn exists_checks_directory() {
        let tmp = setup_repo();
        let cat_dir = tmp.path().join("dev-util");
        std::fs::create_dir_all(&cat_dir).unwrap();

        let cat = Category::new("dev-util".into(), cat_dir.try_into().unwrap());
        assert!(cat.exists());

        let cat2 = Category::new(
            "missing".into(),
            tmp.path().join("no-such-dir").try_into().unwrap(),
        );
        assert!(!cat2.exists());
    }
}
