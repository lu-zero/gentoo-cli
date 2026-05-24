use std::collections::{HashMap, HashSet};
use std::path::Path;

use gentoo_core::Arch;
use portage_atom::interner::{DefaultInterner, Interned};
use portage_atom::{Cpn, Cpv, Dep, Operator};
use portage_atom_pubgrub::{
    DepClass, IUseDefault, PackageDeps, PackageRepository, PackageVersions,
    PortageDependencyProvider, PortagePackage, PortageVersionSet, UseConfig,
};
use portage_metadata::{Keyword, Stability};
use portage_repo::Repository;

fn keyword_accepts(keywords: &[Keyword], arch: &str) -> bool {
    keywords.iter().any(|kw| {
        kw.arch.as_str() == arch && matches!(kw.stability, Stability::Stable | Stability::Testing)
    })
}

struct RepoData {
    cpns: Vec<Cpn>,
    versions: HashMap<Cpn, Vec<(Cpv, portage_metadata::CacheEntry)>>,
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
                        let repo = Some(Interned::<DefaultInterner>::intern(&self.data.repo_name));
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
    let mut cpns_set: HashSet<Cpn> = HashSet::new();
    let mut versions: HashMap<Cpn, Vec<(Cpv, portage_metadata::CacheEntry)>> = HashMap::new();

    // Iterate the md5-cache tree directly — depgraph requires cache entries,
    // so anything without one would have been skipped anyway.
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
                    if s.as_str().is_empty() {
                        None
                    } else {
                        Some(*s)
                    }
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
                            if s.as_str().is_empty() {
                                None
                            } else {
                                Some((cpv.version.clone(), *s))
                            }
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

pub fn depgraph(
    repo_path: &Path,
    atoms: &[String],
    arch: &Arch,
    use_flags: Option<&HashSet<String>>,
) -> crate::error::Result<()> {
    let repo =
        Repository::open(repo_path).map_err(|e| crate::error::Error::Other(e.to_string()))?;
    let data = load_repo(&repo);

    let adapter = Adapter { data: &data, arch };

    let mut use_config = UseConfig::new();
    if let Some(flags) = use_flags {
        for flag in flags {
            use_config.enable(Interned::intern(flag));
        }
    }

    let mut provider = PortageDependencyProvider::new(adapter, use_config, &[]);

    let mut root_deps = Vec::new();
    for target in atoms {
        let dep = Dep::parse(target)
            .map_err(|e| crate::error::Error::Other(format!("bad atom '{}': {}", target, e)))?;
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
            .map(|(pkg, _)| format!("{}", pkg.cpn()))
            .collect();
        cpns.sort();
        cpns.dedup();
        eprintln!(
            "warning: {} dropped deps ({} unique CPNs)",
            dropped.len(),
            cpns.len()
        );
    }

    let solution = provider
        .resolve_targets(root_deps)
        .map_err(|e| crate::error::Error::Other(format!("resolution failed: {:?}", e)))?;

    let edges = provider.dependency_graph(&solution);
    let order = provider.install_order(&solution);
    println!("Packages: {}", order.len());

    let class_label = |c: DepClass| match c {
        DepClass::Depend => "DEPEND",
        DepClass::Rdepend => "RDEPEND",
        DepClass::Bdepend => "BDEPEND",
        DepClass::Pdepend => "PDEPEND",
        DepClass::Idepend => "IDEPEND",
    };

    if !edges.is_empty() {
        println!("\nDependency graph:");
        for edge in &edges {
            println!(
                "  {}-{} --[{}]--> {}-{}",
                edge.from.0,
                edge.from.1,
                class_label(edge.class),
                edge.to.0,
                edge.to.1,
            );
        }
    }

    println!("\nInstall order:");
    for (i, (pkg, ver)) in order.iter().enumerate() {
        println!("  {:>3}. {}-{}", i + 1, pkg.cpn(), ver);
    }

    Ok(())
}
