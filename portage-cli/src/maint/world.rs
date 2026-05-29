use camino::{Utf8Path, Utf8PathBuf};
use portage_atom::Dep;
use portage_vdb::Vdb;

use crate::error::{Error, Result};
use crate::query::which::dep_matches_cpv;
use super::sets::KnownSets;

const DEFAULT_WORLD: &str = "/var/lib/portage/world";

pub fn run(vdb: &Vdb, fix: bool, root: Option<&Utf8Path>) -> Result<()> {
    let known_sets = KnownSets::load(root);
    let installed: Vec<_> = vdb.packages().into_iter().collect();

    let mut total_orphaned = 0usize;
    let mut total_invalid = 0usize;

    check_world_file(
        &world_path(root),
        &installed,
        &known_sets,
        fix,
        &mut total_orphaned,
        &mut total_invalid,
    )?;
    check_world_sets_file(
        &world_sets_path(root),
        &known_sets,
        fix,
        &mut total_orphaned,
    )?;

    if total_orphaned == 0 && total_invalid == 0 {
        println!("World files are consistent.");
    } else if !fix {
        eprintln!(
            "\n{} issue{} found. Run with --fix to remove them.",
            total_orphaned + total_invalid,
            if total_orphaned + total_invalid == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

fn check_world_file(
    path: &Utf8Path,
    installed: &[portage_vdb::InstalledPackage],
    known_sets: &KnownSets,
    fix: bool,
    orphaned_count: &mut usize,
    invalid_count: &mut usize,
) -> Result<()> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::Other(format!("reading {}: {}", path, e))
    })?;

    let mut orphaned: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    let mut kept: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            kept.push(line);
            continue;
        }
        if let Some(set_name) = trimmed.strip_prefix('@') {
            if known_sets.contains(set_name) {
                kept.push(line);
            } else {
                orphaned.push(trimmed.to_owned());
            }
            continue;
        }
        let dep = match Dep::parse(trimmed) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("warning: invalid world entry '{trimmed}': {e}");
                invalid.push(trimmed.to_owned());
                continue;
            }
        };
        if installed.iter().any(|pkg| dep_matches_cpv(&dep, pkg.cpv())) {
            kept.push(line);
        } else {
            orphaned.push(trimmed.to_owned());
        }
    }

    for atom in &invalid {
        println!("!!! {path}: '{atom}': invalid atom");
    }
    for atom in &orphaned {
        println!("!!! {path}: '{atom}': not installed / unknown set");
    }

    *orphaned_count += orphaned.len();
    *invalid_count += invalid.len();

    if fix && (!orphaned.is_empty() || !invalid.is_empty()) {
        let new_content = kept.join("\n") + "\n";
        std::fs::write(path, new_content)
            .map_err(|e| Error::Other(format!("writing {path}: {e}")))?;
        println!("Fixed {path}.");
    }

    Ok(())
}

fn check_world_sets_file(
    path: &Utf8Path,
    known_sets: &KnownSets,
    fix: bool,
    orphaned_count: &mut usize,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::Other(format!("reading {}: {}", path, e))
    })?;

    let mut orphaned: Vec<String> = Vec::new();
    let mut kept: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            kept.push(line);
            continue;
        }
        let set_name = trimmed.strip_prefix('@').unwrap_or(trimmed);
        if known_sets.contains(set_name) {
            kept.push(line);
        } else {
            orphaned.push(trimmed.to_owned());
        }
    }

    for atom in &orphaned {
        println!("!!! {path}: '{atom}': unknown set");
    }
    *orphaned_count += orphaned.len();

    if fix && !orphaned.is_empty() {
        let new_content = kept.join("\n") + "\n";
        std::fs::write(path, new_content)
            .map_err(|e| Error::Other(format!("writing {path}: {e}")))?;
        println!("Fixed {path}.");
    }

    Ok(())
}

fn world_path(root: Option<&Utf8Path>) -> Utf8PathBuf {
    match root {
        Some(r) => r.join("var/lib/portage/world"),
        None => Utf8PathBuf::from(DEFAULT_WORLD),
    }
}

fn world_sets_path(root: Option<&Utf8Path>) -> Utf8PathBuf {
    match root {
        Some(r) => r.join("var/lib/portage/world_sets"),
        None => Utf8PathBuf::from("/var/lib/portage/world_sets"),
    }
}
