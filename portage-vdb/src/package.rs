//! Installed package representation.

use std::path::{Path, PathBuf};

use portage_atom::{Cpv, DepEntry};
use portage_metadata::Eapi;

use crate::contents::ContentsEntry;
use crate::error::Error;
use crate::Result;

/// A package installed in the VDB.
///
/// Each instance corresponds to a directory under `/var/db/pkg/$CATEGORY/$PF/`.
/// Fields are read lazily from the filesystem on first access.
#[derive(Debug)]
pub struct InstalledPackage {
    path: PathBuf,
    category: String,
    cpv: Cpv,
}

impl InstalledPackage {
    pub(crate) fn from_dir(path: &Path, category: &str, cpv: Cpv) -> Self {
        Self {
            path: path.to_path_buf(),
            category: category.to_string(),
            cpv,
        }
    }

    /// The directory path in the VDB (`/var/db/pkg/$CATEGORY/$PF`).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The category name (e.g. `app-shells`).
    pub fn category(&self) -> &str {
        &self.category
    }

    /// The package name-version without category (e.g. `bash-5.3_p9-r2`).
    ///
    /// This is the PF format used in VDB directory names.
    pub fn pf(&self) -> String {
        format!("{}-{}", self.cpv.cpn.package, self.cpv.version)
    }

    /// The parsed Cpn (category + package name).
    pub fn cpn(&self) -> &portage_atom::Cpn {
        &self.cpv.cpn
    }

    /// The parsed Cpv (category + package name + version).
    pub fn cpv(&self) -> &Cpv {
        &self.cpv
    }

    // -- Metadata fields (read from individual files) --

    /// Read a single metadata file as a trimmed String.
    fn read_field(&self, name: &str) -> Result<String> {
        let p = self.path.join(name);
        std::fs::read_to_string(&p)
            .map(|s| s.trim().to_string())
            .map_err(|source| Error::Io {
                path: p,
                source,
            })
    }

    /// Read a single metadata file, returning `None` if it doesn't exist.
    fn read_field_opt(&self, name: &str) -> Result<Option<String>> {
        let p = self.path.join(name);
        match std::fs::read_to_string(&p) {
            Ok(s) => Ok(Some(s.trim().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::Io {
                path: self.path.join(name),
                source,
            }),
        }
    }

    /// The package description.
    pub fn description(&self) -> Result<String> {
        self.read_field("DESCRIPTION")
    }

    /// The EAPI this package was built with.
    pub fn eapi(&self) -> Result<Eapi> {
        let raw = self.read_field("EAPI")?;
        raw.parse()
            .map_err(|_| Error::MalformedPackage {
                path: self.path.clone(),
                reason: format!("invalid EAPI: {raw}"),
            })
    }

    /// The slot (may include subslot, e.g. `0/5.1`).
    pub fn slot(&self) -> Result<String> {
        self.read_field("SLOT")
    }

    /// The repository name this package was installed from.
    pub fn repository(&self) -> Result<Option<String>> {
        self.read_field_opt("repository")
    }

    /// USE flags active at build time (space-separated).
    pub fn use_flags(&self) -> Result<Vec<String>> {
        let raw = self.read_field("USE")?;
        Ok(raw
            .split_whitespace()
            .map(|s| s.to_string())
            .collect())
    }

    /// IUSE flags defined by the package (space-separated).
    pub fn iuse(&self) -> Result<Vec<String>> {
        let raw = self.read_field("IUSE")?;
        Ok(raw
            .split_whitespace()
            .map(|s| s.to_string())
            .collect())
    }

    /// Build timestamp (Unix epoch).
    pub fn build_time(&self) -> Result<Option<u64>> {
        self.read_field_opt("BUILD_TIME")?
            .map(|s| s.parse().map_err(|_| Error::MalformedPackage {
                path: self.path.clone(),
                reason: format!("invalid BUILD_TIME: {s}"),
            }))
            .transpose()
    }

    /// Installed size in bytes.
    pub fn size(&self) -> Result<Option<u64>> {
        self.read_field_opt("SIZE")?
            .map(|s| s.parse().map_err(|_| Error::MalformedPackage {
                path: self.path.clone(),
                reason: format!("invalid SIZE: {s}"),
            }))
            .transpose()
    }

    /// Installation counter (monotonically increasing).
    pub fn counter(&self) -> Result<Option<u64>> {
        self.read_field_opt("COUNTER")?
            .map(|s| s.parse().map_err(|_| Error::MalformedPackage {
                path: self.path.clone(),
                reason: format!("invalid COUNTER: {s}"),
            }))
            .transpose()
    }

    /// Keywords (space-separated).
    pub fn keywords(&self) -> Result<Vec<String>> {
        let raw = self.read_field("KEYWORDS")?;
        Ok(raw
            .split_whitespace()
            .map(|s| s.to_string())
            .collect())
    }

    /// License string.
    pub fn license(&self) -> Result<Option<String>> {
        self.read_field_opt("LICENSE")
    }

    /// Homepage URL(s).
    pub fn homepage(&self) -> Result<Option<String>> {
        self.read_field_opt("HOMEPAGE")
    }

    // -- Dependency fields --

    /// DEPEND (build dependencies) parsed as a dep tree.
    pub fn depend(&self) -> Result<Option<Vec<DepEntry>>> {
        self.read_dep_field("DEPEND")
    }

    /// RDEPEND (runtime dependencies) parsed as a dep tree.
    pub fn rdepend(&self) -> Result<Option<Vec<DepEntry>>> {
        self.read_dep_field("RDEPEND")
    }

    /// BDEPEND (build-tool dependencies) parsed as a dep tree.
    pub fn bdepend(&self) -> Result<Option<Vec<DepEntry>>> {
        self.read_dep_field("BDEPEND")
    }

    /// PDEPEND (post-merge dependencies) parsed as a dep tree.
    pub fn pdepend(&self) -> Result<Option<Vec<DepEntry>>> {
        self.read_dep_field("PDEPEND")
    }

    /// IDEPEND (install-time dependencies) parsed as a dep tree.
    pub fn idepend(&self) -> Result<Option<Vec<DepEntry>>> {
        self.read_dep_field("IDEPEND")
    }

    fn read_dep_field(&self, name: &str) -> Result<Option<Vec<DepEntry>>> {
        let raw = match self.read_field_opt(name)? {
            Some(s) if !s.is_empty() => s,
            _ => return Ok(None),
        };
        DepEntry::parse(&raw)
            .map(Some)
            .map_err(|source| Error::MalformedPackage {
                path: self.path.clone(),
                reason: format!("failed to parse {name}: {source}"),
            })
    }

    // -- CONTENTS --

    /// Parse the CONTENTS file — the list of files installed by this package.
    pub fn contents(&self) -> Result<Vec<ContentsEntry>> {
        let raw = self.read_field("CONTENTS")?;
        Ok(ContentsEntry::parse(&raw))
    }

    /// Find which installed package owns a given file path.
    ///
    /// Returns `true` if this package owns the given path.
    pub fn owns(&self, file_path: &Path) -> Result<bool> {
        let entries = self.contents()?;
        Ok(entries.iter().any(|e| {
            matches!(e.kind, crate::ContentsKind::Obj | crate::ContentsKind::Sym)
                && e.path == file_path
        }))
    }
}

impl std::fmt::Display for InstalledPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.category, self.pf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_fake_pkg(dir: &std::path::Path, category: &str, pf: &str, fields: &[(&str, &str)]) -> PathBuf {
        let pkg_dir = dir.join(category).join(pf);
        fs::create_dir_all(&pkg_dir).unwrap();
        for (name, content) in fields {
            fs::write(pkg_dir.join(name), content).unwrap();
        }
        pkg_dir
    }

    #[test]
    fn read_basic_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let cpv = Cpv::parse("app-shells/bash-5.3_p9-r2").unwrap();
        let fields = [
            ("DESCRIPTION", "The standard GNU Bourne again shell"),
            ("EAPI", "8"),
            ("SLOT", "0"),
            ("USE", "net nls readline"),
            ("IUSE", "+net +nls +readline"),
            ("BUILD_TIME", "1778566176"),
            ("SIZE", "10401340"),
            ("COUNTER", "992555"),
            ("CATEGORY", "app-shells"),
            ("repository", "gentoo"),
        ];
        let pkg_dir = make_fake_pkg(tmp.path(), "app-shells", "bash-5.3_p9-r2", &fields);
        let pkg = InstalledPackage::from_dir(&pkg_dir, "app-shells", cpv);

        assert_eq!(pkg.category(), "app-shells");
        assert_eq!(pkg.pf(), "bash-5.3_p9-r2");
        assert_eq!(pkg.description().unwrap(), "The standard GNU Bourne again shell");
        assert_eq!(pkg.slot().unwrap(), "0");
        assert_eq!(pkg.use_flags().unwrap(), vec!["net", "nls", "readline"]);
        assert_eq!(pkg.iuse().unwrap(), vec!["+net", "+nls", "+readline"]);
        assert_eq!(pkg.build_time().unwrap(), Some(1778566176));
        assert_eq!(pkg.size().unwrap(), Some(10401340));
        assert_eq!(pkg.counter().unwrap(), Some(992555));
        assert_eq!(pkg.repository().unwrap().as_deref(), Some("gentoo"));
    }

    #[test]
    fn read_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let cpv = Cpv::parse("app-shells/bash-5.3").unwrap();
        let contents = "dir /etc\nobj /etc/foo abc123 100\nsym /etc/bar -> baz 200\n";
        let fields = [("CONTENTS", contents)];
        let pkg_dir = make_fake_pkg(tmp.path(), "app-shells", "bash-5.3", &fields);
        let pkg = InstalledPackage::from_dir(&pkg_dir, "app-shells", cpv);

        let entries = pkg.contents().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].path, PathBuf::from("/etc/foo"));
    }

    #[test]
    fn owns_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cpv = Cpv::parse("app-shells/bash-5.3").unwrap();
        let contents = "dir /etc\nobj /etc/foo abc123 100\n";
        let fields = [("CONTENTS", contents)];
        let pkg_dir = make_fake_pkg(tmp.path(), "app-shells", "bash-5.3", &fields);
        let pkg = InstalledPackage::from_dir(&pkg_dir, "app-shells", cpv);

        assert!(pkg.owns(Path::new("/etc/foo")).unwrap());
        assert!(!pkg.owns(Path::new("/etc/bar")).unwrap());
        // dir entries don't count as "owned"
        assert!(!pkg.owns(Path::new("/etc")).unwrap());
    }
}
