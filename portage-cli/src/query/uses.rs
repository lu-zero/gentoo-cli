use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use portage_metadata::IUseDefault;
use portage_repo::Repository;
use portage_vdb::Vdb;

use super::which::dep_matches_cpv;
use crate::error::{Error, Result};
use crate::vdb::find_packages;
use portage_atom::Dep;

pub fn run(repo_path: &Path, vdb: Option<&Vdb>, atoms: &[String]) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(|e| Error::Other(e.to_string()))?;
    let use_db = repo.use_db().unwrap_or_else(|_| portage_repo::UseDb::default());

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

        let best = matches.last().unwrap();
        let cpv = best.cpv();
        let entry = repo
            .cache_entry(cpv)
            .map_err(|e| Error::Other(e.to_string()))?
            .ok_or_else(|| Error::Other(format!("no cache entry for {cpv} — run `em regen`")))?;

        // Load per-package metadata.xml use_flags as fallback for packages
        // not yet represented in use.local.desc (e.g. overlay packages).
        let xml_flags: BTreeMap<String, String> = best
            .path()
            .parent()
            .and_then(|dir| std::fs::read_to_string(dir.join("metadata.xml")).ok())
            .and_then(|xml| portage_repo::PkgMetadata::parse(&xml).ok())
            .map(|meta| meta.into_use_flags())
            .unwrap_or_default();

        let installed_flags: Option<(String, HashSet<String>)> = vdb.and_then(|v| {
            find_packages(v, &cpv.cpn.to_string())
                .into_iter()
                .max_by(|a, b| a.cpv().version.cmp(&b.cpv().version))
                .and_then(|pkg| {
                    let version = pkg.cpv().to_string();
                    pkg.use_flags()
                        .ok()
                        .map(|flags| (version, flags.into_iter().collect()))
                })
        });

        match &installed_flags {
            Some((installed_ver, _)) if installed_ver != &cpv.to_string() => {
                println!("[{cpv}]  (installed: {installed_ver})");
            }
            Some(_) => println!("[{cpv}]  (installed)"),
            None => println!("[{cpv}]"),
        }

        let mut flags: Vec<_> = entry.metadata.iuse.iter().collect();
        flags.sort_by_key(|f| f.name());

        let cpn = cpv.cpn.to_string();
        for flag in flags {
            let iuse_prefix = match flag.default {
                Some(IUseDefault::Enabled) => "+",
                Some(IUseDefault::Disabled) => "-",
                None => " ",
            };
            let name = flag.name();

            let installed_marker = match &installed_flags {
                Some((_, active)) if active.contains(name) => "[+]",
                Some(_) => "[-]",
                None => "   ",
            };

            let desc = use_db
                .describe(&cpn, name)
                .or_else(|| xml_flags.get(name).map(String::as_str))
                .unwrap_or("");
            println!("  {iuse_prefix}{name:<30} {installed_marker}  {desc}");
        }
    }
    Ok(())
}
