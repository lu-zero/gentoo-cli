use std::collections::BTreeMap;
use std::path::Path;

use portage_metadata::Stability;
use portage_repo::Repository;

use crate::error::{Error, Result};
use super::which::dep_matches_cpv;
use portage_atom::Dep;

pub fn run(repo_path: &Path, atoms: &[String]) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(|e| Error::Other(e.to_string()))?;

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

        // Collect all arches seen across versions
        let mut all_arches: std::collections::BTreeSet<String> = Default::default();
        let mut version_keywords: Vec<(String, BTreeMap<String, Stability>)> = Vec::new();

        for ebuild in &matches {
            let cpv = ebuild.cpv();
            let mut kw_map: BTreeMap<String, Stability> = BTreeMap::new();
            if let Ok(entry) = repo.cache_entry(cpv) {
                for kw in &entry.metadata.keywords {
                    let arch = kw.arch.as_str().to_owned();
                    if arch != "*" {
                        all_arches.insert(arch.clone());
                        kw_map.insert(arch, kw.stability);
                    }
                }
            }
            version_keywords.push((cpv.to_string(), kw_map));
        }

        // Header
        let arches: Vec<String> = all_arches.into_iter().collect();
        let col_w = arches.iter().map(|a| a.len()).max().unwrap_or(4).max(4);
        let ver_w = version_keywords.iter().map(|(v, _)| v.len()).max().unwrap_or(7).max(7);

        print!("{:<ver_w$}", "version");
        for arch in &arches {
            print!("  {:>col_w$}", arch);
        }
        println!();

        print!("{}", "-".repeat(ver_w));
        for _ in &arches {
            print!("  {}", "-".repeat(col_w));
        }
        println!();

        for (version, kw_map) in &version_keywords {
            print!("{:<ver_w$}", version);
            for arch in &arches {
                let sym = match kw_map.get(arch) {
                    Some(Stability::Stable) => "+",
                    Some(Stability::Testing) => "~",
                    Some(Stability::Disabled) => "-",
                    Some(Stability::DisabledAll) => "*",
                    None => " ",
                };
                print!("  {:>col_w$}", sym);
            }
            println!();
        }
    }
    Ok(())
}
