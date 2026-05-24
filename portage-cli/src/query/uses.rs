use std::collections::HashMap;
use std::path::Path;

use portage_metadata::IUseDefault;
use portage_repo::Repository;

use super::which::dep_matches_cpv;
use crate::error::{Error, Result};
use portage_atom::Dep;

pub fn run(repo_path: &Path, atoms: &[String]) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(|e| Error::Other(e.to_string()))?;

    // Build USE flag description map lazily
    let use_descs: HashMap<String, String> =
        repo.use_desc().unwrap_or_default().into_iter().collect();

    let ebuilds: Vec<_> = repo
        .ebuilds()
        .map_err(|e| Error::Other(e.to_string()))?
        .into_iter()
        .collect();

    for raw in atoms {
        let dep = Dep::parse(raw).map_err(|e| Error::Other(format!("bad atom '{raw}': {e}")))?;

        let mut matches: Vec<_> = ebuilds
            .iter()
            .filter(|e| dep_matches_cpv(&dep, e.cpv()))
            .collect();

        if matches.is_empty() {
            eprintln!("em: no ebuilds found for '{raw}'");
            continue;
        }

        matches.sort_by(|a, b| a.cpv().version.cmp(&b.cpv().version));

        // Use the best (latest) match
        let best = matches.last().unwrap();
        let cpv = best.cpv();
        let entry = repo
            .cache_entry(cpv)
            .map_err(|e| Error::Other(e.to_string()))?
            .ok_or_else(|| Error::Other(format!("no cache entry for {cpv} — run `em regen`")))?;

        println!("[{cpv}]");
        let mut flags: Vec<_> = entry.metadata.iuse.iter().collect();
        flags.sort_by_key(|f| f.name());

        for flag in flags {
            let prefix = match flag.default {
                Some(IUseDefault::Enabled) => "+",
                Some(IUseDefault::Disabled) => "-",
                None => " ",
            };
            let name = flag.name();
            let desc = use_descs.get(name).map(|s| s.as_str()).unwrap_or("");
            println!("  {prefix}{name:<30} {desc}");
        }
    }
    Ok(())
}
