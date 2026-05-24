//! Parser for the VDB `CONTENTS` file.
//!
//! Format (one entry per line):
//! ```text
//! obj /path/to/file md5hash mtime
//! dir /path/to/dir
//! sym /path/to/link -> target mtime
//! fif /path/to/pipe
//! dev /path/to/device
//! ```

use std::path::PathBuf;

/// Kind of filesystem entry in a CONTENTS file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentsKind {
    /// Regular file (`obj`). Carries an MD5 hash and mtime.
    Obj,
    /// Directory (`dir`).
    Dir,
    /// Symbolic link (`sym`). Carries a target path and mtime.
    Sym,
    /// FIFO/pipe (`fif`).
    Fifo,
    /// Device node (`dev`).
    Dev,
}

/// A single entry from a `CONTENTS` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentsEntry {
    /// Kind of entry (obj, dir, sym, fif, dev).
    pub kind: ContentsKind,
    /// Absolute path of the installed file/directory/symlink.
    pub path: PathBuf,
    /// MD5 digest (only for `Obj` entries).
    pub md5: Option<String>,
    /// File size or symlink target mtime as a Unix timestamp.
    pub mtime: Option<u64>,
    /// Symlink target (only for `Sym` entries).
    pub target: Option<PathBuf>,
}

impl ContentsEntry {
    /// Parse a single line from a CONTENTS file.
    ///
    /// Returns `None` for blank lines.
    pub fn parse_line(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        let mut parts = line.splitn(5, ' ');
        let kind_str = parts.next()?;

        let kind = match kind_str {
            "obj" => ContentsKind::Obj,
            "dir" => ContentsKind::Dir,
            "sym" => ContentsKind::Sym,
            "fif" => ContentsKind::Fifo,
            "dev" => ContentsKind::Dev,
            _ => return None,
        };

        let path_str = parts.next()?;
        let path = PathBuf::from(path_str);

        match kind {
            ContentsKind::Obj => {
                let md5 = parts.next().map(|s| s.to_string());
                let mtime = parts.next().and_then(|s| s.parse().ok());
                Some(ContentsEntry {
                    kind,
                    path,
                    md5,
                    mtime,
                    target: None,
                })
            }
            ContentsKind::Sym => {
                // sym /path -> target mtime
                // The path field already consumed the first token.
                // But sym format is: sym /link/path -> /target mtime
                // We need to re-parse because the target contains spaces
                // after the '->' separator.
                let rest = line.strip_prefix("sym ")?;
                let (path_and_target, mtime_str) = rest.rsplit_once(' ')?;
                let (path_str, target_str) = path_and_target.split_once(" -> ")?;
                Some(ContentsEntry {
                    kind,
                    path: PathBuf::from(path_str),
                    md5: None,
                    mtime: mtime_str.parse().ok(),
                    target: Some(PathBuf::from(target_str)),
                })
            }
            ContentsKind::Dir | ContentsKind::Fifo | ContentsKind::Dev => Some(ContentsEntry {
                kind,
                path,
                md5: None,
                mtime: None,
                target: None,
            }),
        }
    }

    /// Parse a full CONTENTS file into entries.
    pub fn parse(contents: &str) -> Vec<Self> {
        contents.lines().filter_map(Self::parse_line).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_obj() {
        let entry = ContentsEntry::parse_line(
            "obj /etc/skel/.bashrc d210b9cd7fc07420736480f2062d7d7f 1778566175",
        )
        .unwrap();
        assert_eq!(entry.kind, ContentsKind::Obj);
        assert_eq!(entry.path, PathBuf::from("/etc/skel/.bashrc"));
        assert_eq!(
            entry.md5.as_deref(),
            Some("d210b9cd7fc07420736480f2062d7d7f")
        );
        assert_eq!(entry.mtime, Some(1778566175));
        assert!(entry.target.is_none());
    }

    #[test]
    fn parse_dir() {
        let entry = ContentsEntry::parse_line("dir /etc").unwrap();
        assert_eq!(entry.kind, ContentsKind::Dir);
        assert_eq!(entry.path, PathBuf::from("/etc"));
    }

    #[test]
    fn parse_sym() {
        let entry = ContentsEntry::parse_line("sym /bin/rbash -> bash 1778566174").unwrap();
        assert_eq!(entry.kind, ContentsKind::Sym);
        assert_eq!(entry.path, PathBuf::from("/bin/rbash"));
        assert_eq!(entry.target.as_deref(), Some(std::path::Path::new("bash")));
        assert_eq!(entry.mtime, Some(1778566174));
    }

    #[test]
    fn parse_sym_absolute_target() {
        let entry =
            ContentsEntry::parse_line("sym /usr/lib/libfoo.so -> libfoo.so.1 1234567890").unwrap();
        assert_eq!(entry.kind, ContentsKind::Sym);
        assert_eq!(
            entry.target.as_deref(),
            Some(std::path::Path::new("libfoo.so.1"))
        );
    }

    #[test]
    fn parse_empty_line() {
        assert!(ContentsEntry::parse_line("").is_none());
        assert!(ContentsEntry::parse_line("  ").is_none());
    }

    #[test]
    fn parse_full_contents() {
        let raw = "dir /etc\nobj /etc/foo abc123 100\nsym /etc/bar -> baz 200\n";
        let entries = ContentsEntry::parse(raw);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, ContentsKind::Dir);
        assert_eq!(entries[1].kind, ContentsKind::Obj);
        assert_eq!(entries[2].kind, ContentsKind::Sym);
    }
}
