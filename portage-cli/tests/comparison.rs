//! Comparison tests: `em query` vs `qfile`/`qlist`/`qsize`/`equery`.
//!
//! These tests only run on a live Gentoo system with `portage-utils` installed.
//! They are ignored by default; run with `cargo test -- --ignored`.

use std::path::Path;
use std::process::Command;

fn em(args: &str) -> String {
    let output = Command::new("cargo")
        .args(["run", "-q", "-p", "portage-cli", "--"])
        .args(args.split_whitespace())
        .output()
        .expect("failed to run em");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn q(args: &str) -> String {
    let output = Command::new(args.split_whitespace().next().unwrap())
        .args(args.split_whitespace().skip(1))
        .output()
        .expect("failed to run comparison tool");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Extract the category/package (without version) from a full atom string.
fn strip_version(atom: &str) -> &str {
    // e.g. "app-shells/bash-5.3_p9-r2" -> "app-shells/bash"
    let Some(slash) = atom.find('/') else {
        return atom;
    };
    let _cat = &atom[..slash];
    let pf = &atom[slash + 1..];
    // Find last hyphen before a digit (PMS version boundary)
    let ver_start = pf
        .rmatch_indices('-')
        .find_map(|(i, _)| {
            pf.get(i + 1..)
                .and_then(|s| s.chars().next())
                .map(|c| (i, c))
        })
        .filter(|(_, c)| c.is_ascii_digit())
        .map(|(i, _)| i);
    match ver_start {
        Some(pos) => &atom[..slash + 1 + pos],
        None => atom,
    }
}

/// Sort and deduplicate lines for comparison.
fn normalize(s: &str) -> Vec<String> {
    let mut lines: Vec<String> = s.lines().map(|l| l.to_string()).collect();
    lines.sort();
    lines.dedup();
    lines
}

#[test]
#[ignore]
fn query_belongs_matches_qfile() {
    let files = ["/bin/bash", "/bin/ls", "/usr/bin/qfile", "/usr/bin/make"];

    for file in files {
        let em_out = em(&format!("query belongs {}", file));
        let q_out = q(&format!("qfile {}", file));

        // qfile outputs "category/package: /path", em outputs "category/package-version"
        // Compare the category/package part
        let em_pkg = strip_version(&em_out);
        let q_line = q_out.lines().next().unwrap_or("");
        let q_pkg = q_line.split(':').next().unwrap_or("").trim();

        assert_eq!(em_pkg, q_pkg, "mismatch for {file}: em={em_out} q={q_out}");
    }
}

#[test]
#[ignore]
fn query_files_matches_qlist() {
    // Use category-qualified atoms for exact match (qlist does substring match on PN)
    let atoms = ["app-shells/bash"];

    for atom in atoms {
        let em_out = em(&format!("query files {}", atom));
        let q_out = q(&format!("qlist {}", atom));

        // em prepends "category/pkg-version\t", strip it
        let em_clean: Vec<String> = em_out
            .lines()
            .filter_map(|l| l.split('\t').nth(1).map(|s| s.to_string()))
            .collect();

        let q_lines: Vec<String> = q_out.lines().map(|s| s.to_string()).collect();

        // qlist may include files from bash-completion if it matches "bash"
        // when using unqualified name. With category-qualified name it should be exact.
        assert_eq!(
            em_clean.len(),
            q_lines.len(),
            "file count mismatch for {atom}: em={} q={}",
            em_clean.len(),
            q_lines.len()
        );
    }
}

#[test]
#[ignore]
fn installed_count_matches_qlist() {
    let q_count = q("qlist -I").lines().count();
    let vdb = portage_vdb::Vdb::open(Path::new("/var/db/pkg")).unwrap();
    let em_count = vdb.packages().count();

    assert_eq!(
        em_count, q_count,
        "installed package count mismatch: em={em_count} qlist={q_count}"
    );
}

#[test]
#[ignore]
fn vdb_pkg_size_matches() {
    let vdb = portage_vdb::Vdb::open(Path::new("/var/db/pkg")).unwrap();

    // Pick a known package
    if let Some(pkg) = vdb.find("app-shells", "bash-5.3_p9-r2") {
        let size = pkg.size().unwrap();
        // bash is typically ~8-12 MiB
        assert!(size.is_some());
        let bytes = size.unwrap();
        assert!(bytes > 5_000_000, "bash size too small: {bytes}");
        assert!(bytes < 50_000_000, "bash size too large: {bytes}");
    }
}

#[test]
#[ignore]
fn vdb_contents_roundtrip() {
    let vdb = portage_vdb::Vdb::open(Path::new("/var/db/pkg")).unwrap();

    if let Some(pkg) = vdb.find("app-shells", "bash-5.3_p9-r2") {
        let entries = pkg.contents().unwrap();
        assert!(!entries.is_empty());

        let files: Vec<_> = entries
            .iter()
            .filter(|e| matches!(e.kind, portage_vdb::ContentsKind::Obj))
            .collect();
        let dirs: Vec<_> = entries
            .iter()
            .filter(|e| matches!(e.kind, portage_vdb::ContentsKind::Dir))
            .collect();
        let syms: Vec<_> = entries
            .iter()
            .filter(|e| matches!(e.kind, portage_vdb::ContentsKind::Sym))
            .collect();

        assert!(!files.is_empty(), "no obj entries in bash");
        assert!(!dirs.is_empty(), "no dir entries in bash");
        assert!(!syms.is_empty(), "no sym entries in bash");

        // Every obj should have md5 and mtime
        for f in &files {
            assert!(f.md5.is_some(), "obj missing md5: {:?}", f.path);
            assert!(f.mtime.is_some(), "obj missing mtime: {:?}", f.path);
        }

        // Every sym should have target and mtime
        for s in &syms {
            assert!(s.target.is_some(), "sym missing target: {:?}", s.path);
            assert!(s.mtime.is_some(), "sym missing mtime: {:?}", s.path);
        }
    }
}
