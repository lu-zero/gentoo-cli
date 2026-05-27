//! `em query has` — list installed packages where a VDB field matches a value.
//!
//! Usage: `em query has <FIELD> [VALUE]`
//!
//! Searches the installed package database for packages whose metadata field
//! `FIELD` contains (or equals) `VALUE`.  When no `VALUE` is given, lists all
//! packages for which the field is non-empty.
//!
//! Examples:
//!   em query has SLOT 0          — packages installed in slot 0
//!   em query has repository      — packages with any repository set
//!   em query has USE lto         — packages built with the `lto` USE flag

use portage_vdb::Vdb;

use crate::error::{Error, Result};

pub fn run(vdb: &Vdb, args: &[String]) -> Result<()> {
    let (field, value) = match args {
        [] => {
            return Err(Error::Other(
                "em query has: expected <FIELD> [VALUE]".into(),
            ));
        }
        [field] => (field.as_str(), None),
        [field, value, ..] => (field.as_str(), Some(value.as_str())),
    };

    for pkg in vdb.packages() {
        let raw = match pkg.field(field) {
            Ok(Some(v)) if !v.is_empty() => v,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("warning: {pkg}: {e}");
                continue;
            }
        };

        let matched = match value {
            None => true,
            Some(want) => field_matches(&raw, want),
        };

        if matched {
            println!("{pkg}");
        }
    }
    Ok(())
}

/// Match `want` against a raw VDB field value.
///
/// Space-separated fields (USE, IUSE, DEPEND, …) are matched token-by-token so
/// `lto` doesn't accidentally match `no-lto`.  Single-value fields (SLOT,
/// repository, EAPI, …) are matched as a trimmed equality check.
fn field_matches(raw: &str, want: &str) -> bool {
    // If the raw value has spaces it's a list field — match whole tokens.
    if raw.contains(' ') {
        raw.split_whitespace().any(|token| {
            // Strip leading +/- (IUSE defaults) before comparing.
            token.trim_start_matches(['+', '-']) == want
        })
    } else {
        raw.trim() == want
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_matches_list() {
        assert!(field_matches("net nls readline", "nls"));
        assert!(!field_matches("net nls readline", "nl"));
        assert!(field_matches("+net +nls", "nls")); // strips IUSE default prefix
    }

    #[test]
    fn field_matches_single() {
        assert!(field_matches("0", "0"));
        assert!(!field_matches("0/5.1", "0"));
        assert!(field_matches("gentoo", "gentoo"));
    }
}
