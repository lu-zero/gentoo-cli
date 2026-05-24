use std::collections::BTreeSet;
use std::path::Path;

use portage_repo::Repository;

use crate::error::{Error, Result};

pub fn run(repo_path: &Path, flags: &[String]) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(|e| Error::Other(e.to_string()))?;

    for flag in flags {
        let mut seen: BTreeSet<String> = BTreeSet::new();

        for ebuild in repo.ebuilds().map_err(|e| Error::Other(e.to_string()))? {
            let cpv = ebuild.cpv();
            let cpn = cpv.cpn.to_string();
            if seen.contains(&cpn) {
                continue;
            }
            if let Ok(Some(entry)) = repo.cache_entry(cpv)
                && entry
                    .metadata
                    .iuse
                    .iter()
                    .any(|u| u.name() == flag.as_str())
            {
                seen.insert(cpn);
            }
        }

        if flags.len() > 1 {
            println!("[{flag}]");
        }
        for cpn in &seen {
            println!("{cpn}");
        }
    }
    Ok(())
}
