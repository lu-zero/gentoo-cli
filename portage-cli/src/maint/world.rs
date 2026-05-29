use camino::{Utf8Path, Utf8PathBuf};
use portage_atom::Dep;
use portage_vdb::Vdb;

use crate::error::{Error, Result};
use crate::query::which::dep_matches_cpv;

const DEFAULT_WORLD: &str = "/var/lib/portage/world";

pub fn run(vdb: &Vdb, fix: bool, root: Option<&Utf8Path>) -> Result<()> {
    let path = world_path(root);

    let content = std::fs::read_to_string(&path).map_err(|e| {
        Error::Other(format!("reading {}: {}", path, e))
    })?;

    // Collect all installed CPVs once for O(n) scanning.
    let installed: Vec<_> = vdb.packages().into_iter().collect();

    let mut orphaned: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    let mut kept: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            kept.push(line);
            continue;
        }
        // @set references: keep them, we don't validate sets yet.
        if trimmed.starts_with('@') {
            kept.push(line);
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

    if orphaned.is_empty() && invalid.is_empty() {
        let ok = kept.iter().filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#')).count();
        println!("World file is consistent ({ok} entries).");
        return Ok(());
    }

    for atom in &invalid {
        println!("!!! '{atom}': invalid atom");
    }
    for atom in &orphaned {
        println!("!!! '{atom}': not installed");
    }

    if fix {
        let new_content = kept.join("\n") + "\n";
        std::fs::write(&path, new_content)
            .map_err(|e| Error::Other(format!("writing {path}: {e}")))?;
        println!(
            "Removed {} orphaned/invalid entr{} from {path}.",
            orphaned.len() + invalid.len(),
            if orphaned.len() + invalid.len() == 1 { "y" } else { "ies" }
        );
    } else {
        eprintln!(
            "\n{} issue{} found. Run with --fix to remove them.",
            orphaned.len() + invalid.len(),
            if orphaned.len() + invalid.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

fn world_path(root: Option<&Utf8Path>) -> Utf8PathBuf {
    match root {
        Some(r) => r.join("var/lib/portage/world"),
        None => Utf8PathBuf::from(DEFAULT_WORLD),
    }
}
