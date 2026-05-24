use std::path::Path;

use portage_vdb::Vdb;

use crate::error::{Error, Result};

/// `em query belongs <file>...` — find which package owns a file.
pub fn query_belongs(vdb: &Vdb, files: &[String]) -> Result<()> {
    for file_str in files {
        let path = Path::new(file_str);
        if let Some(pkg) = vdb.owner(path) {
            println!("{}", pkg);
            continue;
        }
        let resolved = resolve_path(file_str);
        if resolved != path {
            if let Some(pkg) = vdb.owner(&resolved) {
                println!("{}", pkg);
                continue;
            }
        }
        eprintln!("no package owns '{}'", file_str);
    }
    Ok(())
}

/// `em query files <atom>...` — list files owned by matching installed packages.
pub fn query_files(vdb: &Vdb, atoms: &[String]) -> Result<()> {
    for raw in atoms {
        let matched = find_packages(vdb, raw);
        if matched.is_empty() {
            eprintln!("no installed package matches '{}'", raw);
            continue;
        }
        for pkg in matched {
            match pkg.contents() {
                Ok(entries) => {
                    for entry in entries {
                        if !matches!(entry.kind, portage_vdb::ContentsKind::Dir) {
                            println!("{}\t{}", pkg, entry.path.display());
                        }
                    }
                }
                Err(e) => eprintln!("{}: {}", pkg, e),
            }
        }
    }
    Ok(())
}

/// `em query size <atom>...` — show disk usage of matching installed packages.
pub fn query_size(vdb: &Vdb, atoms: &[String]) -> Result<()> {
    for raw in atoms {
        let matched = find_packages(vdb, raw);
        if matched.is_empty() {
            eprintln!("no installed package matches '{}'", raw);
            continue;
        }
        for pkg in matched {
            print_pkg_size(&pkg)?;
        }
    }
    Ok(())
}

fn print_pkg_size(pkg: &portage_vdb::InstalledPackage) -> Result<()> {
    match pkg.size() {
        Ok(Some(bytes)) => {
            let size_str = humansize(bytes);
            println!("{}: {}", pkg, size_str);
        }
        Ok(None) => println!("{}: size unknown", pkg),
        Err(e) => return Err(Error::Other(format!("{}: {}", pkg, e))),
    }
    Ok(())
}

/// Find installed packages matching an atom pattern.
///
/// Supports:
/// - `category/package-version` (exact match)
/// - `category/package` (all versions)
/// - `package` (all versions across all categories)
fn find_packages(vdb: &Vdb, pattern: &str) -> Vec<portage_vdb::InstalledPackage> {
    if let Some(slash_pos) = pattern.find('/') {
        let cat = &pattern[..slash_pos];
        let rest = &pattern[slash_pos + 1..];

        // Try exact category/pf match first
        if let Some(pkg) = vdb.find(cat, rest) {
            return vec![pkg];
        }

        // category/package — find all versions
        vdb.find_by_cpn(cat, rest)
    } else {
        // No category — search all packages by name
        vdb.packages()
            .filter(|pkg| {
                pkg.cpn().package.as_ref() == pattern
                    || pkg.pf() == pattern
            })
            .collect()
    }
}

fn resolve_path(path_str: &str) -> std::path::PathBuf {
    let path = Path::new(path_str);
    match std::fs::canonicalize(path) {
        Ok(resolved) => resolved,
        Err(_) => path.to_path_buf(),
    }
}

fn humansize(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}
