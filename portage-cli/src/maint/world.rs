use camino::{Utf8Path, Utf8PathBuf};
use portage_atom::Dep;
use portage_vdb::Vdb;

use crate::error::{Error, Result};
use crate::query::which::dep_matches_cpv;

const DEFAULT_WORLD: &str = "/var/lib/portage/world";

pub fn run(vdb: &Vdb, root: Option<&Utf8Path>) -> Result<()> {
    let world_path = world_path(root);

    let content = std::fs::read_to_string(&world_path).map_err(|e| {
        Error::Other(format!("reading {}: {}", world_path, e))
    })?;

    // Collect all installed CPVs once for O(n) scanning.
    let installed: Vec<_> = vdb.packages().into_iter().collect();

    let mut orphaned: Vec<String> = Vec::new();
    let mut ok: usize = 0;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
            continue;
        }

        let dep = match Dep::parse(line) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("warning: skipping invalid world entry '{line}': {e}");
                continue;
            }
        };

        if installed.iter().any(|pkg| dep_matches_cpv(&dep, pkg.cpv())) {
            ok += 1;
        } else {
            orphaned.push(line.to_owned());
        }
    }

    if orphaned.is_empty() {
        println!("World file is consistent ({ok} packages installed).");
    } else {
        for atom in &orphaned {
            println!("!!! {atom}: not installed");
        }
        eprintln!(
            "\n{} orphaned entr{} in {world_path} (installed: {ok})",
            orphaned.len(),
            if orphaned.len() == 1 { "y" } else { "ies" },
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
