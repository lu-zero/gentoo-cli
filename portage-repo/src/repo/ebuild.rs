use std::sync::LazyLock;

use camino::{Utf8Path, Utf8PathBuf};

use portage_atom::Cpv;
use portage_metadata::Eapi;
use regex::Regex;

use super::util;
use crate::error::Result;

/// PMS 7.3.1 regex for detecting EAPI before sourcing.
///
/// Matches lines of the form `EAPI=value`, `EAPI='value'`, or `EAPI="value"`,
/// with optional leading whitespace and optional trailing comment.
/// Uses alternation instead of a backreference (unsupported by the `regex` crate).
static EAPI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^[ \t]*EAPI=(?:'([A-Za-z0-9+_.-]*)'|"([A-Za-z0-9+_.-]*)"|([A-Za-z0-9+_.-]*))[ \t]*([ \t]#.*)?$"#,
    )
    .unwrap()
});

/// A single ebuild file within a package directory.
///
/// This is intentionally thin — it represents the `.ebuild` file on disk.
/// Metadata extraction goes through [`EbuildShell`](crate::EbuildShell).
///
/// See [PMS 4](https://projects.gentoo.org/pms/9/pms.html#tree-layout).
#[derive(Debug, Clone)]
pub struct Ebuild {
    cpv: Cpv,
    path: Utf8PathBuf,
}

impl Ebuild {
    pub(crate) fn new(cpv: Cpv, path: Utf8PathBuf) -> Self {
        Self { cpv, path }
    }

    /// The full category/package-version atom.
    pub fn cpv(&self) -> &Cpv {
        &self.cpv
    }

    /// The category name.
    pub fn category(&self) -> &str {
        &self.cpv.cpn.category
    }

    /// The package name (without version).
    pub fn name(&self) -> &str {
        &self.cpv.cpn.package
    }

    /// The version.
    pub fn version(&self) -> &portage_atom::Version {
        &self.cpv.version
    }

    /// Absolute path to the `.ebuild` file.
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// Read the raw ebuild file content.
    pub fn read_raw(&self) -> Result<String> {
        util::read_to_string(&self.path)
    }

    /// Detect the EAPI by regex-matching the ebuild text before sourcing.
    ///
    /// Skips blank and comment lines, then checks whether the first
    /// non-blank non-comment line is an `EAPI=` assignment. If matched,
    /// returns the parsed [`Eapi`]; otherwise defaults to [`Eapi::Zero`].
    ///
    /// See [PMS 7.3.1](https://projects.gentoo.org/pms/9/pms.html#x1-690007.3.1).
    pub fn detect_eapi(&self) -> Result<Eapi> {
        let content = self.read_raw()?;
        Ok(detect_eapi_from_str(&content))
    }
}

/// Parse EAPI from ebuild file content per PMS 7.3.1.
///
/// Extracted as a free function for testability without filesystem access.
fn detect_eapi_from_str(content: &str) -> Eapi {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // First non-blank non-comment line: check for EAPI assignment.
        // Groups: 1 = single-quoted, 2 = double-quoted, 3 = unquoted.
        if let Some(caps) = EAPI_RE.captures(line) {
            let eapi_str = caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("");
            return eapi_str.parse().unwrap_or(Eapi::Zero);
        }
        // Not an EAPI assignment — default to 0
        return Eapi::Zero;
    }
    // Empty file or only blanks/comments — default to 0
    Eapi::Zero
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_eapi_simple() {
        assert_eq!(detect_eapi_from_str("EAPI=8\n"), Eapi::Eight);
    }

    #[test]
    fn detect_eapi_single_quoted() {
        assert_eq!(detect_eapi_from_str("EAPI='7'\n"), Eapi::Seven);
    }

    #[test]
    fn detect_eapi_double_quoted() {
        assert_eq!(detect_eapi_from_str("EAPI=\"6\"\n"), Eapi::Six);
    }

    #[test]
    fn detect_eapi_with_comment_and_blanks() {
        let content = "\n# Copyright\n# License\n\nEAPI=8\n";
        assert_eq!(detect_eapi_from_str(content), Eapi::Eight);
    }

    #[test]
    fn detect_eapi_trailing_comment() {
        assert_eq!(detect_eapi_from_str("EAPI=7 # foo\n"), Eapi::Seven);
    }

    #[test]
    fn detect_eapi_leading_whitespace() {
        assert_eq!(detect_eapi_from_str("  EAPI=5\n"), Eapi::Five);
        assert_eq!(detect_eapi_from_str("\tEAPI=5\n"), Eapi::Five);
    }

    #[test]
    fn detect_eapi_missing_defaults_to_zero() {
        let content = "inherit eutils\nSRC_URI=foo\n";
        assert_eq!(detect_eapi_from_str(content), Eapi::Zero);
    }

    #[test]
    fn detect_eapi_empty_value_defaults_to_zero() {
        assert_eq!(detect_eapi_from_str("EAPI=\n"), Eapi::Zero);
    }

    #[test]
    fn detect_eapi_empty_file() {
        assert_eq!(detect_eapi_from_str(""), Eapi::Zero);
    }

    #[test]
    fn detect_eapi_only_comments() {
        assert_eq!(detect_eapi_from_str("# comment\n# another\n"), Eapi::Zero);
    }

    #[test]
    fn detect_eapi_mismatched_quotes_defaults_to_zero() {
        // Mismatched quotes: single open, double close — regex won't match
        assert_eq!(detect_eapi_from_str("EAPI='7\"\n"), Eapi::Zero);
    }
}
