use std::collections::BTreeSet;
use std::path::Path;

use portage_repo::Repository;

use crate::error::{Error, Result};

pub fn run(
    repo_path: &Path,
    pattern: Option<&str>,
    all: bool,
    search_desc: bool,
    name_only: bool,
    homepage: bool,
) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(|e| Error::Other(e.to_string()))?;

    let mut seen: BTreeSet<String> = BTreeSet::new();

    for ebuild in repo.ebuilds().map_err(|e| Error::Other(e.to_string()))? {
        let cpv = ebuild.cpv();
        let cpn = cpv.cpn.to_string();

        if seen.contains(&cpn) {
            continue;
        }

        let matched = if all {
            true
        } else {
            let pat = pattern.unwrap_or("");
            if search_desc {
                // -S: search description text
                repo.cache_entry(cpv)
                    .ok()
                    .flatten()
                    .map(|e| e.metadata.description.contains(pat))
                    .unwrap_or(false)
            } else {
                // default: search package basename
                cpn.contains(pat)
            }
        };

        if matched {
            seen.insert(cpn.clone());
            if name_only {
                println!("{cpn}");
            } else {
                let entry = repo.cache_entry(cpv).ok().flatten();
                let info = if homepage {
                    entry
                        .map(|e| e.metadata.homepage.join(" "))
                        .unwrap_or_default()
                } else {
                    entry
                        .map(|e| e.metadata.description.clone())
                        .unwrap_or_default()
                };
                println!("{cpn}: {info}");
            }
        }
    }
    Ok(())
}
