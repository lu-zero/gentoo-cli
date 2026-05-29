use camino::Utf8Path;
use portage_vdb::Vdb;

use crate::error::{Error, Result};

/// A parsed entry from a `profiles/updates/` file.
enum UpdateEntry {
    /// `move old-cat/old-pkg new-cat/new-pkg`
    Move { from: String, to: String },
    /// `slotmove cat/pkg old-slot new-slot`
    SlotMove {
        cpn: String,
        from_slot: String,
        to_slot: String,
    },
}

/// Detect installed packages that are affected by package moves recorded in
/// `profiles/updates/`.  Reports what would need to be renamed; does not
/// modify the VDB.
pub fn run(repo_path: &Utf8Path, vdb: &Vdb) -> Result<()> {
    let updates_dir = repo_path.join("profiles/updates");

    if !updates_dir.exists() {
        println!("No profiles/updates directory found.");
        return Ok(());
    }

    let moves = load_moves(&updates_dir)?;

    if moves.is_empty() {
        println!("No package moves found.");
        return Ok(());
    }

    // Index installed packages by CPN for fast lookup.
    let installed: Vec<_> = vdb.packages().into_iter().collect();

    let mut any = false;
    for entry in &moves {
        match entry {
            UpdateEntry::Move { from, to } => {
                let affected: Vec<_> = installed
                    .iter()
                    .filter(|pkg| pkg.cpn().to_string() == *from)
                    .collect();
                for pkg in affected {
                    any = true;
                    let old_cpv = pkg.cpv().to_string();
                    let new_cpv = old_cpv.replacen(from, to, 1);
                    println!("move:     {old_cpv}  →  {new_cpv}");
                }
            }
            UpdateEntry::SlotMove {
                cpn,
                from_slot,
                to_slot,
            } => {
                let affected: Vec<_> = installed
                    .iter()
                    .filter(|pkg| {
                        pkg.cpn().to_string() == *cpn
                            && pkg.slot().ok().as_deref() == Some(from_slot)
                    })
                    .collect();
                for pkg in affected {
                    any = true;
                    println!(
                        "slotmove: {}  slot {} → {}",
                        pkg.cpv(),
                        from_slot,
                        to_slot
                    );
                }
            }
        }
    }

    if !any {
        println!("All installed packages are up to date with package moves.");
    }

    Ok(())
}

/// Parse all `profiles/updates/*` files and collect move entries.
/// Files are processed in alphabetical order (quarter-named: 1Q-2020, etc.).
fn load_moves(updates_dir: &Utf8Path) -> Result<Vec<UpdateEntry>> {
    let mut entries = Vec::new();

    let mut files: Vec<_> = std::fs::read_dir(updates_dir)
        .map_err(|e| Error::Other(format!("reading {}: {}", updates_dir, e)))?
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.is_file() {
                camino::Utf8PathBuf::try_from(path).ok()
            } else {
                None
            }
        })
        .collect();

    files.sort();

    for file in &files {
        let content = std::fs::read_to_string(file)
            .map_err(|e| Error::Other(format!("reading {}: {}", file, e)))?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(4, ' ').collect();
            match parts.as_slice() {
                ["move", from, to] => entries.push(UpdateEntry::Move {
                    from: from.to_string(),
                    to: to.to_string(),
                }),
                ["slotmove", cpn, old_slot, new_slot] => entries.push(UpdateEntry::SlotMove {
                    cpn: cpn.to_string(),
                    from_slot: old_slot.to_string(),
                    to_slot: new_slot.to_string(),
                }),
                _ => {}
            }
        }
    }

    Ok(entries)
}
