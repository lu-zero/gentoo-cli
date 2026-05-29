use std::collections::HashMap;

use camino::Utf8Path;
use gentoo_core::Arch;
use portage_atom::interner::{DefaultInterner, Interned};
use portage_atom::{Cpn, Cpv, Dep, Operator, Version};
use portage_atom_pubgrub::{
    DepClass, IUseDefault, PackageDeps, PackageRepository, PackageVersions,
    PortageDependencyProvider, PortagePackage, PortageVersionSet, UseConfig,
};
use portage_metadata::{CacheEntry, Keyword, Stability};
use portage_repo::Repository;

use crate::cli::DepgraphFormat;

// ---------------------------------------------------------------------------
// Repository adapter
// ---------------------------------------------------------------------------

fn keyword_accepts(keywords: &[Keyword], arch: &str) -> bool {
    keywords.iter().any(|kw| {
        kw.arch.as_str() == arch && matches!(kw.stability, Stability::Stable | Stability::Testing)
    })
}

struct RepoData {
    cpns: Vec<Cpn>,
    versions: HashMap<Cpn, Vec<(Cpv, CacheEntry)>>,
    repo_name: String,
}

struct Adapter<'a> {
    data: &'a RepoData,
    arch: &'a Arch,
}

impl PackageRepository for Adapter<'_> {
    fn all_packages(&self) -> Vec<Cpn> {
        self.data.cpns.clone()
    }

    fn versions_for(&self, cpn: &Cpn) -> Vec<(Cpv, PackageVersions)> {
        self.data
            .versions
            .get(cpn)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|(_, cache)| {
                        keyword_accepts(&cache.metadata.keywords, self.arch.as_str())
                    })
                    .map(|(cpv, cache)| {
                        let meta = &cache.metadata;
                        let slot = if meta.slot.slot.as_str().is_empty() {
                            None
                        } else {
                            Some(meta.slot.slot)
                        };
                        let subslot = meta.slot.subslot;
                        let repo =
                            Some(Interned::<DefaultInterner>::intern(&self.data.repo_name));
                        let iuse: Vec<Interned<DefaultInterner>> = meta
                            .iuse
                            .iter()
                            .map(|iu| Interned::intern(iu.name()))
                            .collect();
                        let iuse_defaults: HashMap<Interned<DefaultInterner>, IUseDefault> = meta
                            .iuse
                            .iter()
                            .filter_map(|iu| {
                                iu.default.map(|d| {
                                    let val = match d {
                                        portage_metadata::IUseDefault::Enabled => {
                                            IUseDefault::Enabled
                                        }
                                        portage_metadata::IUseDefault::Disabled => {
                                            IUseDefault::Disabled
                                        }
                                    };
                                    (Interned::intern(iu.name()), val)
                                })
                            })
                            .collect();
                        let deps = PackageDeps {
                            depend: meta.depend.clone(),
                            rdepend: meta.rdepend.clone(),
                            bdepend: meta.bdepend.clone(),
                            pdepend: meta.pdepend.clone(),
                            idepend: meta.idepend.clone(),
                        };
                        (
                            cpv.clone(),
                            PackageVersions {
                                slot,
                                subslot,
                                repo,
                                iuse,
                                iuse_defaults,
                                deps,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn load_repo(repo: &Repository) -> RepoData {
    use std::collections::HashSet;
    let mut cpns_set: HashSet<Cpn> = HashSet::new();
    let mut versions: HashMap<Cpn, Vec<(Cpv, CacheEntry)>> = HashMap::new();

    for (cpv, entry) in repo.cache_entries() {
        let Ok(entry) = entry else { continue };
        let cpn = cpv.cpn;
        cpns_set.insert(cpn);
        versions.entry(cpn).or_default().push((cpv, entry));
    }

    let mut cpns: Vec<Cpn> = cpns_set.into_iter().collect();
    cpns.sort_by_key(|c| format!("{}/{}", c.category, c.package));

    RepoData {
        cpns,
        versions,
        repo_name: repo.name().to_string(),
    }
}

fn target_package(data: &RepoData, dep: &Dep) -> PortagePackage {
    match data.versions.get(&dep.cpn) {
        Some(entries) => {
            let mut slots: Vec<_> = entries
                .iter()
                .filter_map(|(_, cache)| {
                    let s = &cache.metadata.slot.slot;
                    if s.as_str().is_empty() { None } else { Some(*s) }
                })
                .collect();
            slots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            slots.dedup();
            match slots.as_slice() {
                [] => PortagePackage::unslotted(dep.cpn),
                [sole] => PortagePackage::slotted(dep.cpn, *sole),
                _ => {
                    let latest_slot = entries
                        .iter()
                        .filter_map(|(cpv, cache)| {
                            let s = &cache.metadata.slot.slot;
                            if s.as_str().is_empty() { None } else { Some((cpv.version.clone(), *s)) }
                        })
                        .max_by(|a, b| a.0.cmp(&b.0))
                        .map(|(_, s)| s)
                        .unwrap();
                    PortagePackage::slotted(dep.cpn, latest_slot)
                }
            }
        }
        None => PortagePackage::unslotted(dep.cpn),
    }
}

/// Look up the cache entry for a resolved `(PortagePackage, Version)` pair.
fn find_cache<'a>(
    data: &'a RepoData,
    pkg: &PortagePackage,
    ver: &Version,
) -> Option<&'a CacheEntry> {
    data.versions
        .get(pkg.cpn())?
        .iter()
        .find(|(cpv, _)| &cpv.version == ver)
        .map(|(_, e)| e)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn depgraph(
    repo_path: &Utf8Path,
    atoms: &[String],
    arch: &Arch,
    format: DepgraphFormat,
) -> crate::error::Result<()> {
    let repo = Repository::open(repo_path)
        .map_err(|e| crate::error::Error::Other(e.to_string()))?;
    let data = load_repo(&repo);

    let adapter = Adapter { data: &data, arch };
    let use_config = UseConfig::new();
    let mut provider = PortageDependencyProvider::new(adapter, use_config, &[]);

    let mut root_deps = Vec::new();
    for target in atoms {
        let dep = Dep::parse(target).map_err(|e| {
            crate::error::Error::Other(format!("bad atom '{}': {}", target, e))
        })?;
        let pkg = target_package(&data, &dep);
        let vs = match &dep.version {
            Some(v) => {
                let op = dep.op.unwrap_or(Operator::GreaterOrEqual);
                PortageVersionSet::from_operator(op, dep.glob, v.clone())
            }
            None => PortageVersionSet::any(),
        };
        root_deps.push((pkg, vs));
    }

    let dropped = provider.dropped_deps();
    if !dropped.is_empty() {
        let mut cpns: Vec<String> = dropped
            .iter()
            .filter(|(pkg, _)| !pkg.is_virtual())
            .map(|(pkg, _)| pkg.cpn().to_string())
            .collect();
        cpns.sort();
        cpns.dedup();
        if !cpns.is_empty() {
            eprintln!(
                "warning: {} dropped deps ({} unique CPNs)",
                dropped.len(),
                cpns.len()
            );
        }
    }

    let solution = provider
        .resolve_targets(root_deps)
        .map_err(|e| crate::error::Error::Other(format!("resolution failed: {:?}", e)))?;

    // Virtual packages (USE-decision nodes, synthetic root) are internal solver
    // bookkeeping — strip them before output.
    let order: Vec<_> = provider
        .install_order(&solution)
        .into_iter()
        .filter(|(pkg, _)| !pkg.is_virtual())
        .collect();
    let edges: Vec<_> = provider
        .dependency_graph(&solution)
        .into_iter()
        .filter(|e| !e.from.0.is_virtual() && !e.to.0.is_virtual())
        .collect();

    match format {
        DepgraphFormat::Pretty => print_pretty(&data, &order, &edges),
        DepgraphFormat::Json => print_json(&data, &order, &edges),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Pretty output — emerge -p style
// ---------------------------------------------------------------------------

fn iuse_tokens(cache: &CacheEntry) -> Vec<String> {
    let mut flags: Vec<_> = cache.metadata.iuse.iter().collect();
    flags.sort_by_key(|f| f.name());
    flags
        .iter()
        .map(|f| match f.default {
            Some(portage_metadata::IUseDefault::Enabled) => f.name().to_string(),
            _ => format!("-{}", f.name()),
        })
        .collect()
}

fn print_pretty(
    data: &RepoData,
    order: &[(PortagePackage, Version)],
    _edges: &[portage_atom_pubgrub::DepEdge],
) {
    println!("These are the packages that would be merged, in order:\n");
    println!("Calculating dependencies... done!");

    for (pkg, ver) in order {
        let cpn = pkg.cpn();
        // Tag: always N (new) since we don't check VDB here
        let tag = "N";
        let repo = &data.repo_name;

        let use_str = find_cache(data, pkg, ver)
            .map(|c| {
                let tokens = iuse_tokens(c);
                if tokens.is_empty() {
                    String::new()
                } else {
                    format!("  USE=\"{}\"", tokens.join(" "))
                }
            })
            .unwrap_or_default();

        println!("[ebuild  {tag:<6}] {cpn}-{ver}::{repo}{use_str}");
    }

    println!("\nTotal: {} package(s)", order.len());
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

fn class_str(c: DepClass) -> &'static str {
    match c {
        DepClass::Depend => "DEPEND",
        DepClass::Rdepend => "RDEPEND",
        DepClass::Bdepend => "BDEPEND",
        DepClass::Pdepend => "PDEPEND",
        DepClass::Idepend => "IDEPEND",
    }
}

fn print_json(
    data: &RepoData,
    order: &[(PortagePackage, Version)],
    edges: &[portage_atom_pubgrub::DepEdge],
) {
    let packages: Vec<serde_json::Value> = order
        .iter()
        .map(|(pkg, ver)| {
            let cpn = pkg.cpn();
            let mut obj = serde_json::json!({
                "atom": format!("{cpn}-{ver}"),
                "cpn": cpn.to_string(),
                "version": ver.to_string(),
                "repo": data.repo_name,
                "status": "new",
            });
            if let Some(cache) = find_cache(data, pkg, ver) {
                let slot = &cache.metadata.slot;
                obj["slot"] = serde_json::Value::String(slot.slot.as_str().to_owned());
                if let Some(sub) = slot.subslot {
                    obj["subslot"] = serde_json::Value::String(sub.as_str().to_owned());
                }
                let iuse: Vec<String> = {
                    let mut flags: Vec<_> = cache.metadata.iuse.iter().collect();
                    flags.sort_by_key(|f| f.name());
                    flags.iter().map(|f| match f.default {
                        Some(portage_metadata::IUseDefault::Enabled) => {
                            format!("+{}", f.name())
                        }
                        _ => format!("-{}", f.name()),
                    }).collect()
                };
                obj["iuse"] = serde_json::json!(iuse);
            }
            obj
        })
        .collect();

    let dep_edges: Vec<serde_json::Value> = edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "from": format!("{}-{}", e.from.0.cpn(), e.from.1),
                "to": format!("{}-{}", e.to.0.cpn(), e.to.1),
                "class": class_str(e.class),
            })
        })
        .collect();

    let out = serde_json::json!({
        "packages": packages,
        "edges": dep_edges,
    });

    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
