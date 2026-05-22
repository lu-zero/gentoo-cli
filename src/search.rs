use std::collections::BTreeMap;
use std::path::Path;

use portage_atom::{Cpn, Cpv};
use portage_metadata::CacheEntry;
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
    let pat = pattern.unwrap_or("");

    if search_desc {
        run_desc(&repo, pat, all, name_only, homepage)
    } else {
        run_name(&repo, pat, all, name_only, homepage)
    }
}

/// Name-mode: enumerate category/package directories — cheap. Read the
/// metadata cache only for packages whose name passes the filter (and only
/// when we actually have to print something other than the cpn).
fn run_name(
    repo: &Repository,
    pat: &str,
    all: bool,
    name_only: bool,
    homepage: bool,
) -> Result<()> {
    // A pattern containing `/` matches against full `category/package`; bare
    // patterns match the package basename only (qsearch parity).
    let pat_has_slash = pat.contains('/');
    let mut matched: BTreeMap<String, Cpn> = BTreeMap::new();
    for cat in repo.categories().map_err(|e| Error::Other(e.to_string()))? {
        for pkg in cat.packages().map_err(|e| Error::Other(e.to_string()))? {
            let hit = if all {
                true
            } else if pat_has_slash {
                let full = format!("{}/{}", cat.name(), pkg.name());
                full.contains(pat)
            } else {
                pkg.name().contains(pat)
            };
            if hit {
                let key = format!("{}/{}", cat.name(), pkg.name());
                matched.insert(key, pkg.cpn().clone());
            }
        }
    }

    if name_only {
        for key in matched.keys() {
            println!("{key}");
        }
        return Ok(());
    }

    for (key, cpn) in &matched {
        let info = latest_entry_info(repo, cpn, homepage);
        println!("{key}: {info}");
    }
    Ok(())
}

fn latest_entry_info(repo: &Repository, cpn: &Cpn, homepage: bool) -> String {
    let Some(cat) = repo.category(cpn.category.as_str()) else {
        return String::new();
    };
    let Some(pkg) = cat.package(cpn.package.as_str()) else {
        return String::new();
    };
    let Ok(ebuilds) = pkg.ebuilds() else {
        return String::new();
    };
    let Some(latest) = ebuilds.last() else {
        return String::new();
    };
    match repo.cache_entry(latest.cpv()).ok().flatten() {
        Some(entry) if homepage => entry.metadata.homepage.join(" "),
        Some(entry) => entry.metadata.description,
        None => String::new(),
    }
}

/// Description mode: walks every cache entry via the parallel iterator,
/// keeps the highest cpv per cpn, then filters on description content.
fn run_desc(
    repo: &Repository,
    pat: &str,
    all: bool,
    name_only: bool,
    homepage: bool,
) -> Result<()> {
    let mut entries: Vec<(Cpv, CacheEntry)> = repo
        .cache_entries()
        .into_iter()
        .filter_map(|(cpv, r)| r.ok().map(|e| (cpv, e)))
        .collect();

    // Group by cpn (asc), then highest version first within each group.
    entries.sort_by(|(a, _), (b, _)| {
        a.cpn
            .category
            .cmp(&b.cpn.category)
            .then_with(|| a.cpn.package.cmp(&b.cpn.package))
            .then_with(|| b.version.cmp(&a.version))
    });
    entries.dedup_by(|(a, _), (b, _)| a.cpn == b.cpn);

    for (cpv, entry) in &entries {
        let hit = all || entry.metadata.description.contains(pat);
        if !hit {
            continue;
        }
        let key = format!("{}/{}", cpv.cpn.category, cpv.cpn.package);
        if name_only {
            println!("{key}");
        } else {
            let info = if homepage {
                entry.metadata.homepage.join(" ")
            } else {
                entry.metadata.description.clone()
            };
            println!("{key}: {info}");
        }
    }
    Ok(())
}
