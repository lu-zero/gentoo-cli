use std::time::{Duration, UNIX_EPOCH};

use camino::{Utf8Path, Utf8PathBuf};
use humansize::{BINARY, format_size};

use portage_vdb::Vdb;

use crate::error::{Error, Result};

/// `em query belongs <file>...` — find which package owns a file.
pub fn query_belongs(vdb: &Vdb, files: &[String]) -> Result<()> {
    for file_str in files {
        let path = Utf8Path::new(file_str);
        if let Some(pkg) = vdb.owner(path) {
            println!("{}", pkg);
            continue;
        }
        let resolved = resolve_path(file_str);
        if resolved.as_path() != path
            && let Some(pkg) = vdb.owner(&resolved)
        {
            println!("{}", pkg);
            continue;
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
                            println!("{}\t{}", pkg, entry.path);
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
    let size_str = match pkg.size() {
        Ok(Some(bytes)) => format_size(bytes, BINARY),
        Ok(None) => "size unknown".to_string(),
        Err(e) => return Err(Error::Other(format!("{}: {}", pkg, e))),
    };

    let built_str = match pkg.build_time() {
        Ok(Some(ts)) => {
            let t = UNIX_EPOCH + Duration::from_secs(ts);
            format!("  built {}", humantime::format_rfc3339_seconds(t))
        }
        _ => String::new(),
    };

    println!("{}: {}{}", pkg, size_str, built_str);
    Ok(())
}

pub(crate) fn find_packages(vdb: &Vdb, pattern: &str) -> Vec<portage_vdb::InstalledPackage> {
    if let Some(slash) = pattern.find('/') {
        let cat_name = &pattern[..slash];
        let rest = &pattern[slash + 1..];
        let Some(cat) = vdb.category(cat_name) else {
            return vec![];
        };
        if let Some(pkg) = cat.package(rest) {
            return vec![pkg];
        }
        let rest = rest.to_string();
        cat.packages()
            .filter(move |p| p.cpn().package.as_ref() == rest)
            .collect_vec()
    } else {
        vdb.packages()
            .into_iter()
            .filter(|p| p.cpn().package.as_ref() == pattern || p.pf() == pattern)
            .collect()
    }
}

fn resolve_path(path_str: &str) -> Utf8PathBuf {
    match std::fs::canonicalize(path_str) {
        Ok(resolved) => Utf8PathBuf::from_path_buf(resolved)
            .unwrap_or_else(|_| Utf8PathBuf::from(path_str)),
        Err(_) => Utf8PathBuf::from(path_str),
    }
}
