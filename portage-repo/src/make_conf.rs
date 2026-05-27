//! Parser and editor for `/etc/portage/make.conf`.
//!
//! Reads the file into a structured list of entries (assignments, comments,
//! blank lines, unparsed lines) so that modifying a single variable and
//! re-serialising produces byte-identical output for all unmodified content.
//!
//! # Examples
//!
//! ```no_run
//! use portage_repo::MakeConf;
//!
//! let mut conf = MakeConf::load_default().unwrap();
//!
//! // Read a value
//! println!("{:?}", conf.get("CFLAGS"));
//!
//! // Modify a space-separated flag list
//! conf.add_token("USE", "lto");
//! conf.remove_token("USE", "bindist");
//!
//! // Overwrite with a new value (quote style from the original is preserved)
//! conf.set("MAKEOPTS", "-j4");
//!
//! conf.save().unwrap();
//! ```

use std::fmt;
use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{Error, Result};

// ── Public API ────────────────────────────────────────────────────────────────

/// Conventional path for `/etc/portage/make.conf`.
pub const DEFAULT_MAKE_CONF: &str = "/etc/portage/make.conf";
/// Legacy path (pre-`/etc/portage` split).
pub const LEGACY_MAKE_CONF: &str = "/etc/make.conf";

/// Quote style surrounding a value in `make.conf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    /// `KEY="value"`
    Double,
    /// `KEY='value'`
    Single,
    /// `KEY=value` (no surrounding quotes)
    Unquoted,
}

/// A parsed, editable `make.conf` file.
///
/// Comments, blank lines, and surrounding whitespace are preserved verbatim so
/// that round-tripping a file that has not been modified produces byte-identical
/// output.
#[derive(Debug, Clone, Default)]
pub struct MakeConf {
    /// Source path, set by [`load`](Self::load) / [`load_default`](Self::load_default).
    path: Option<Utf8PathBuf>,
    entries: Vec<Entry>,
}

impl MakeConf {
    // ── Construction ──────────────────────────────────────────────────────────

    /// Parse a `make.conf` from a string slice.
    pub fn parse(src: &str) -> Self {
        // split_inclusive keeps the '\n' attached to each line, which is what
        // we need for round-trip fidelity.
        let entries = src.split_inclusive('\n').map(parse_line).collect();
        Self { path: None, entries }
    }

    /// Load and parse a `make.conf` from `path`.
    pub fn load(path: &Utf8Path) -> Result<Self> {
        let src = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf().into(),
            source,
        })?;
        let mut conf = Self::parse(&src);
        conf.path = Some(path.to_path_buf());
        Ok(conf)
    }

    /// Load the system `make.conf`, trying `/etc/portage/make.conf` first,
    /// then falling back to `/etc/make.conf`.
    pub fn load_default() -> Result<Self> {
        for p in [DEFAULT_MAKE_CONF, LEGACY_MAKE_CONF] {
            let path = Utf8Path::new(p);
            if path.exists() {
                return Self::load(path);
            }
        }
        Err(Error::Io {
            path: PathBuf::from(DEFAULT_MAKE_CONF),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "make.conf not found at /etc/portage/make.conf or /etc/make.conf",
            ),
        })
    }

    // ── Read ──────────────────────────────────────────────────────────────────

    /// Return the raw value string for `key`, or `None` if not set.
    ///
    /// "Raw" means the content between the surrounding quotes (if any) is
    /// returned **as stored on disk** — variable references like `${CFLAGS}`
    /// and command substitutions are **not** expanded.
    ///
    /// When `key` appears multiple times the last assignment wins (same
    /// behaviour as `bash`).
    pub fn get(&self, key: &str) -> Option<&str> {
        // iterate in reverse — last assignment wins
        self.entries.iter().rev().find_map(|e| {
            if let Entry::Var(v) = e {
                if v.key == key { Some(v.value.as_str()) } else { None }
            } else {
                None
            }
        })
    }

    /// Return the space-separated tokens in the value of `key`.
    ///
    /// Useful for list variables such as `USE`, `FEATURES`, `MAKEOPTS`.
    /// Variable references (e.g. `${FEATURES}`) are returned as opaque tokens.
    pub fn get_tokens(&self, key: &str) -> Vec<&str> {
        self.get(key)
            .map(|v| v.split_whitespace().collect())
            .unwrap_or_default()
    }

    /// Return `true` if `key` is defined in this file.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Return `true` if the space-separated list for `key` contains `token`.
    pub fn has_token(&self, key: &str, token: &str) -> bool {
        self.get_tokens(key).contains(&token)
    }

    /// Return all variable names defined in this file, in first-occurrence order.
    pub fn keys(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        let mut keys = Vec::new();
        for entry in &self.entries {
            if let Entry::Var(v) = entry {
                if seen.insert(v.key.as_str()) {
                    keys.push(v.key.as_str());
                }
            }
        }
        keys
    }

    // ── Write ─────────────────────────────────────────────────────────────────

    /// Set `key` to `value`, preserving the original quote style.
    ///
    /// - If `key` already exists, the **first** occurrence is updated in place
    ///   and any duplicates are removed.
    /// - If `key` does not exist, a new double-quoted assignment is appended.
    ///
    /// The original quoting style, inline comments, and surrounding blank
    /// lines are all preserved.
    pub fn set(&mut self, key: &str, value: &str) {
        let mut first_idx: Option<usize> = None;
        let mut duplicates: Vec<usize> = Vec::new();

        for (i, entry) in self.entries.iter_mut().enumerate() {
            if let Entry::Var(v) = entry {
                if v.key == key {
                    if first_idx.is_none() {
                        v.value = value.to_string();
                        first_idx = Some(i);
                    } else {
                        duplicates.push(i);
                    }
                }
            }
        }

        // Remove duplicates in reverse order to keep indices stable.
        for i in duplicates.into_iter().rev() {
            self.entries.remove(i);
        }

        if first_idx.is_none() {
            self.append_var(key, value, QuoteStyle::Double);
        }
    }

    /// Remove all assignments for `key`.
    ///
    /// Returns `true` if at least one assignment was removed.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|e| !matches!(e, Entry::Var(v) if v.key == key));
        self.entries.len() < before
    }

    /// Add `token` to the space-separated list in `key` if it is not already present.
    ///
    /// If `key` does not exist it is created.
    pub fn add_token(&mut self, key: &str, token: &str) {
        if let Some(v) = self.find_var_mut(key) {
            if !v.value.split_whitespace().any(|t| t == token) {
                if v.value.is_empty() {
                    v.value = token.to_string();
                } else {
                    v.value.push(' ');
                    v.value.push_str(token);
                }
            }
        } else {
            self.append_var(key, token, QuoteStyle::Double);
        }
    }

    /// Remove `token` from the space-separated list in `key`.
    ///
    /// Returns `true` if `token` was present.
    pub fn remove_token(&mut self, key: &str, token: &str) -> bool {
        if let Some(v) = self.find_var_mut(key) {
            let before: Vec<&str> = v.value.split_whitespace().collect();
            let after: Vec<&str> = before.iter().copied().filter(|t| *t != token).collect();
            if after.len() < before.len() {
                v.value = after.join(" ");
                return true;
            }
        }
        false
    }

    // ── Serialisation ─────────────────────────────────────────────────────────

    /// Serialise the file back to a `String`.
    ///
    /// If no modifications have been made this is byte-identical to the
    /// original source.
    pub fn to_string_repr(&self) -> String {
        self.entries.iter().map(|e| e.raw()).collect()
    }

    /// Write the file back to the path it was loaded from.
    ///
    /// Fails with an error if the [`MakeConf`] was created with
    /// [`parse`](Self::parse) (no path set) — use [`save_to`](Self::save_to)
    /// in that case.
    pub fn save(&self) -> Result<()> {
        let path = self.path.as_deref().ok_or_else(|| Error::Io {
            path: PathBuf::from("<unknown>"),
            source: std::io::Error::new(
                std::io::ErrorKind::Other,
                "no path associated — use save_to()",
            ),
        })?;
        self.save_to(path)
    }

    /// Write the file to `path`.
    pub fn save_to(&self, path: &Utf8Path) -> Result<()> {
        std::fs::write(path, self.to_string_repr()).map_err(|source| Error::Io {
            path: path.to_path_buf().into(),
            source,
        })
    }

    /// The path this file was loaded from, if any.
    pub fn path(&self) -> Option<&Utf8Path> {
        self.path.as_deref()
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn find_var_mut(&mut self, key: &str) -> Option<&mut VarEntry> {
        self.entries.iter_mut().find_map(|e| {
            if let Entry::Var(v) = e {
                if v.key == key { Some(v) } else { None }
            } else {
                None
            }
        })
    }

    fn append_var(&mut self, key: &str, value: &str, quote: QuoteStyle) {
        // Ensure there's a trailing newline on the last entry so the new line
        // doesn't get glued to it.
        if let Some(last) = self.entries.last() {
            if !last.raw().ends_with('\n') {
                self.entries.push(Entry::Blank("\n".to_string()));
            }
        }
        self.entries.push(Entry::Var(VarEntry {
            prefix: key.to_string(),
            key: key.to_string(),
            value: value.to_string(),
            quote,
            suffix: "\n".to_string(),
        }));
    }
}

impl fmt::Display for MakeConf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_repr())
    }
}

// ── Internal representation ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Entry {
    /// A `#`-prefixed comment line (newline included).
    Comment(String),
    /// A blank (all-whitespace) line.
    Blank(String),
    /// A parsed `KEY=VALUE` assignment.
    Var(VarEntry),
    /// A line we couldn't parse — preserved verbatim.
    Raw(String),
}

impl Entry {
    fn raw(&self) -> String {
        match self {
            Entry::Comment(s) | Entry::Blank(s) | Entry::Raw(s) => s.clone(),
            Entry::Var(v) => v.to_raw(),
        }
    }
}

#[derive(Debug, Clone)]
struct VarEntry {
    /// Original text before `=` (key name, possibly with leading whitespace).
    prefix: String,
    /// Trimmed variable name.
    key: String,
    /// Unquoted logical value (content between surrounding quotes, verbatim).
    value: String,
    /// Quote style to use when re-serialising.
    quote: QuoteStyle,
    /// Text after the closing quote/value: trailing comment, whitespace, newline.
    suffix: String,
}

impl VarEntry {
    fn to_raw(&self) -> String {
        match self.quote {
            QuoteStyle::Double => format!("{}=\"{}\"{}", self.prefix, self.value, self.suffix),
            QuoteStyle::Single => format!("{}='{}'{}", self.prefix, self.value, self.suffix),
            QuoteStyle::Unquoted => format!("{}={}{}", self.prefix, self.value, self.suffix),
        }
    }
}

// ── Parser ────────────────────────────────────────────────────────────────────

fn parse_line(line: &str) -> Entry {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Entry::Blank(line.to_string());
    }
    if trimmed.starts_with('#') {
        return Entry::Comment(line.to_string());
    }
    if let Some(v) = try_parse_assignment(line) {
        return Entry::Var(v);
    }
    Entry::Raw(line.to_string())
}

fn try_parse_assignment(line: &str) -> Option<VarEntry> {
    let eq = line.find('=')?;
    let prefix = &line[..eq];
    let key = prefix.trim();

    if !is_valid_key(key) {
        return None;
    }

    let after_eq = &line[eq + 1..];
    let (value, quote, suffix) = parse_value(after_eq)?;

    Some(VarEntry {
        prefix: prefix.to_string(),
        key: key.to_string(),
        value,
        quote,
        suffix,
    })
}

/// Shell identifiers: must start with `[A-Za-z_]`, then `[A-Za-z0-9_]`.
fn is_valid_key(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {
            chars.all(|c| c.is_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

/// Parse the RHS of a `KEY=…` assignment.
///
/// Returns `(unquoted_value, quote_style, trailing_text)` or `None` if the
/// string is unterminated (treated as `Raw`).
fn parse_value(input: &str) -> Option<(String, QuoteStyle, String)> {
    match input.as_bytes().first() {
        Some(b'"') => parse_double_quoted(input),
        Some(b'\'') => {
            // Single-quoted — no escapes, scan for matching `'`.
            let inner = &input[1..];
            let end = inner.find('\'')?;
            let value = inner[..end].to_string();
            let suffix = inner[end + 1..].to_string();
            Some((value, QuoteStyle::Single, suffix))
        }
        _ => {
            // Unquoted — ends at `#` (comment start) or newline.
            let end = input
                .find(|c: char| c == '#' || c == '\n')
                .unwrap_or(input.len());
            let value = input[..end].trim_end().to_string();
            let suffix = input[end..].to_string();
            Some((value, QuoteStyle::Unquoted, suffix))
        }
    }
}

/// Dedicated, correct double-quote parser (avoids the tangled logic above).
fn parse_double_quoted(input: &str) -> Option<(String, QuoteStyle, String)> {
    debug_assert!(input.starts_with('"'));
    let inner = &input[1..]; // skip opening `"`
    let mut value = String::new();
    let mut chars = inner.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => {
                // Closing quote; the suffix is everything after this `"`.
                let suffix = inner[i + 1..].to_string();
                return Some((value, QuoteStyle::Double, suffix));
            }
            '\\' => {
                // Preserve escape sequences verbatim (don't expand them).
                value.push('\\');
                if let Some((_, next)) = chars.next() {
                    value.push(next);
                }
            }
            c => value.push(c),
        }
    }
    None // unterminated
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Compiler flags
COMMON_FLAGS="-mcpu=native -O2 -pipe"
CFLAGS="${COMMON_FLAGS}"
CXXFLAGS="${COMMON_FLAGS}"

# Portage settings
MAKEOPTS="-j4"
USE="X alsa -systemd"
FEATURES="${FEATURES} splitdebug"

LC_MESSAGES=C.UTF-8
"#;

    #[test]
    fn get_double_quoted() {
        let conf = MakeConf::parse(SAMPLE);
        assert_eq!(conf.get("COMMON_FLAGS"), Some("-mcpu=native -O2 -pipe"));
    }

    #[test]
    fn get_variable_reference() {
        let conf = MakeConf::parse(SAMPLE);
        // Variable references are kept verbatim — not expanded.
        assert_eq!(conf.get("CFLAGS"), Some("${COMMON_FLAGS}"));
    }

    #[test]
    fn get_unquoted() {
        let conf = MakeConf::parse(SAMPLE);
        assert_eq!(conf.get("LC_MESSAGES"), Some("C.UTF-8"));
    }

    #[test]
    fn get_missing() {
        let conf = MakeConf::parse(SAMPLE);
        assert!(conf.get("NONEXISTENT").is_none());
    }

    #[test]
    fn get_tokens() {
        let conf = MakeConf::parse(SAMPLE);
        let use_flags = conf.get_tokens("USE");
        assert!(use_flags.contains(&"X"));
        assert!(use_flags.contains(&"alsa"));
        assert!(use_flags.contains(&"-systemd"));
    }

    #[test]
    fn has_token() {
        let conf = MakeConf::parse(SAMPLE);
        assert!(conf.has_token("USE", "alsa"));
        assert!(!conf.has_token("USE", "pulseaudio"));
    }

    #[test]
    fn set_existing_preserves_quote_style() {
        let mut conf = MakeConf::parse(SAMPLE);
        conf.set("MAKEOPTS", "-j8");
        let out = conf.to_string_repr();
        assert!(out.contains("MAKEOPTS=\"-j8\""));
        // Other lines are unchanged.
        assert!(out.contains("# Compiler flags"));
        assert!(out.contains("LC_MESSAGES=C.UTF-8"));
    }

    #[test]
    fn set_preserves_single_quote_style() {
        let src = "FOO='bar baz'\n";
        let mut conf = MakeConf::parse(src);
        conf.set("FOO", "qux");
        assert_eq!(conf.to_string_repr(), "FOO='qux'\n");
    }

    #[test]
    fn set_new_key_appended() {
        let mut conf = MakeConf::parse(SAMPLE);
        conf.set("VIDEO_CARDS", "amdgpu radeonsi");
        let out = conf.to_string_repr();
        assert!(out.contains("VIDEO_CARDS=\"amdgpu radeonsi\""));
    }

    #[test]
    fn set_removes_duplicates() {
        let src = "USE=\"X\"\nUSE=\"alsa\"\n";
        let mut conf = MakeConf::parse(src);
        conf.set("USE", "wayland");
        let out = conf.to_string_repr();
        assert_eq!(out.lines().filter(|l| l.starts_with("USE=")).count(), 1);
        assert!(out.contains("USE=\"wayland\""));
    }

    #[test]
    fn remove_key() {
        let mut conf = MakeConf::parse(SAMPLE);
        assert!(conf.remove("MAKEOPTS"));
        assert!(conf.get("MAKEOPTS").is_none());
        assert!(!conf.remove("NONEXISTENT"));
    }

    #[test]
    fn add_token() {
        let mut conf = MakeConf::parse(SAMPLE);
        conf.add_token("USE", "lto");
        assert!(conf.has_token("USE", "lto"));
        assert!(conf.has_token("USE", "alsa")); // old tokens still present
    }

    #[test]
    fn add_token_idempotent() {
        let mut conf = MakeConf::parse(SAMPLE);
        conf.add_token("USE", "alsa");
        let tokens = conf.get_tokens("USE");
        assert_eq!(tokens.iter().filter(|&&t| t == "alsa").count(), 1);
    }

    #[test]
    fn add_token_creates_key() {
        let mut conf = MakeConf::parse(SAMPLE);
        conf.add_token("LLVM_TARGETS", "AArch64");
        assert!(conf.has_token("LLVM_TARGETS", "AArch64"));
    }

    #[test]
    fn remove_token() {
        let mut conf = MakeConf::parse(SAMPLE);
        assert!(conf.remove_token("USE", "alsa"));
        assert!(!conf.has_token("USE", "alsa"));
        assert!(conf.has_token("USE", "X")); // other tokens preserved
    }

    #[test]
    fn remove_token_missing() {
        let mut conf = MakeConf::parse(SAMPLE);
        assert!(!conf.remove_token("USE", "pulseaudio"));
    }

    #[test]
    fn roundtrip_unmodified() {
        let conf = MakeConf::parse(SAMPLE);
        assert_eq!(conf.to_string_repr(), SAMPLE);
    }

    #[test]
    fn keys_in_order() {
        let conf = MakeConf::parse(SAMPLE);
        let keys = conf.keys();
        assert_eq!(
            keys,
            &[
                "COMMON_FLAGS",
                "CFLAGS",
                "CXXFLAGS",
                "MAKEOPTS",
                "USE",
                "FEATURES",
                "LC_MESSAGES",
            ]
        );
    }

    #[test]
    fn inline_comment_preserved_on_set() {
        let src = "MAKEOPTS=\"-j4\" # number of jobs\n";
        let mut conf = MakeConf::parse(src);
        conf.set("MAKEOPTS", "-j8");
        assert_eq!(conf.to_string_repr(), "MAKEOPTS=\"-j8\" # number of jobs\n");
    }
}
