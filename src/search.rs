use std::collections::BTreeMap;

use portage_atom::{Cpn, Cpv};
use portage_metadata::RawCacheEntry;
use portage_repo::{CacheReadOpts, Repository, cache_entries_parallel};

use crate::error::{Error, Result};

pub async fn run(
    repo_paths: &[std::path::PathBuf],
    pattern: Option<&str>,
    all: bool,
    search_desc: bool,
    name_only: bool,
    homepage: bool,
) -> Result<()> {
    if repo_paths.is_empty() {
        return Err(Error::Other("no repositories configured".into()));
    }
    let mut repos: Vec<Repository> = Vec::with_capacity(repo_paths.len());
    for p in repo_paths {
        match Repository::open(p) {
            Ok(r) => repos.push(r),
            Err(e) => eprintln!("em: skipping {}: {e}", p.display()),
        }
    }
    if repos.is_empty() {
        return Err(Error::Other("no usable repositories".into()));
    }
    let pat = pattern.unwrap_or("");

    if search_desc {
        run_desc(&repos, pat, all, name_only, homepage).await
    } else {
        run_name(&repos, pat, all, name_only, homepage)
    }
}

/// Name-mode: enumerate category/package directories across every repo —
/// dedupe by `cat/pkg` (first repo wins). Reads the metadata cache only when
/// we actually have to print something other than the cpn.
fn run_name(
    repos: &[Repository],
    pat: &str,
    all: bool,
    name_only: bool,
    homepage: bool,
) -> Result<()> {
    let pat_has_slash = pat.contains('/');
    // value: (cpn, repo_index_of_first_sighting)
    let mut matched: BTreeMap<String, (Cpn, usize)> = BTreeMap::new();
    for (idx, repo) in repos.iter().enumerate() {
        let cats = match repo.categories() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("em: skipping {} categories: {e}", repo.path());
                continue;
            }
        };
        for cat in cats {
            let pkgs = match cat.packages() {
                Ok(v) => v,
                Err(_) => continue,
            };
            for pkg in pkgs {
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
                    matched.entry(key).or_insert_with(|| (pkg.cpn().clone(), idx));
                }
            }
        }
    }

    if name_only {
        for key in matched.keys() {
            println!("{key}");
        }
        return Ok(());
    }

    for (key, (cpn, idx)) in &matched {
        let info = latest_entry_info(&repos[*idx], cpn, homepage);
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

/// Description mode: walks every cache entry across every repo via the
/// parallel reader, keeps the highest cpv per cpn, then filters on
/// description content. Uses `RawCacheEntry` to skip atom-tree parsing —
/// we only need DESCRIPTION (and optionally HOMEPAGE) per file.
async fn run_desc(
    repos: &[Repository],
    pat: &str,
    all: bool,
    name_only: bool,
    homepage: bool,
) -> Result<()> {
    // latest_per_cpn drops older versions and overlay-duplicates at
    // discovery time, so we never pay to read-and-parse them.
    let opts = CacheReadOpts {
        latest_per_cpn: true,
        ..Default::default()
    };

    // The closure runs on a worker that owns the file text only for the
    // duration of the call, so values we want to keep must be cloned.
    // Filter and pick the single field we'll actually print inside the
    // closure: non-matches return None (zero allocs); matches allocate
    // exactly one String (the field needed for output).
    let pat_owned = pat.to_string();
    let mut entries: Vec<(Cpv, Option<String>)> =
        cache_entries_parallel(repos, &opts, move |text| {
            let raw = RawCacheEntry::new(text);
            let desc = raw.field("DESCRIPTION").unwrap_or("");
            if !all && !desc.contains(&pat_owned) {
                return Ok(None);
            }
            let info = if name_only {
                None
            } else if homepage {
                Some(raw.field("HOMEPAGE").unwrap_or("").to_string())
            } else {
                Some(desc.to_string())
            };
            Ok::<_, portage_repo::Error>(Some(info))
        })
        .await
        .into_iter()
        .filter_map(|(cpv, r)| r.ok().flatten().map(|info| (cpv, info)))
        .collect();

    // Sort for deterministic output order (HashMap from latest_per_cpn
    // hands back arbitrary insertion order).
    entries.sort_by(|(a, _), (b, _)| {
        a.cpn
            .category
            .cmp(&b.cpn.category)
            .then_with(|| a.cpn.package.cmp(&b.cpn.package))
    });

    for (cpv, info) in &entries {
        let key = format!("{}/{}", cpv.cpn.category, cpv.cpn.package);
        match info {
            None => println!("{key}"),
            Some(s) => println!("{key}: {s}"),
        }
    }
    Ok(())
}


