use std::collections::BTreeMap;

use portage_vdb::Vdb;

use crate::error::Result;

/// List packages that have more than one revision installed, showing which
/// revisions are superseded.
pub fn run(vdb: &Vdb) -> Result<()> {
    // Group installed packages by CPN (category/name, no version).
    let mut by_cpn: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for pkg in vdb.packages() {
        by_cpn
            .entry(pkg.cpn().to_string())
            .or_default()
            .push(pkg);
    }

    let mut found = false;
    for (cpn, mut pkgs) in by_cpn {
        if pkgs.len() < 2 {
            continue;
        }
        found = true;
        // Sort ascending by version; the last entry is the keeper.
        pkgs.sort_by(|a, b| a.cpv().version.cmp(&b.cpv().version));
        let newest = pkgs.last().unwrap().cpv().to_string();
        println!("{cpn}:");
        for pkg in &pkgs {
            let cpv = pkg.cpv().to_string();
            if cpv == newest {
                println!("  {cpv}  (keep)");
            } else {
                println!("  {cpv}  (superseded)");
            }
        }
    }

    if !found {
        println!("No packages with multiple revisions installed.");
    }

    Ok(())
}
