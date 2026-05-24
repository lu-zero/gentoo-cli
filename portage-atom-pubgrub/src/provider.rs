use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};

use portage_atom::interner::{DefaultInterner, Interned};
use portage_atom::{Cpn, Dep, Version};
use pubgrub::{
    Dependencies, DependencyConstraints, DependencyProvider, PackageResolutionStatistics,
    SelectedDependencies, VersionSet,
};

use crate::convert;
use crate::error::Error;
use crate::package::PortagePackage;
use crate::repository::PackageRepository;
use crate::use_config::UseConfig;
use crate::version_set::PortageVersionSet;

/// Whether an installed package should be favored or locked during resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledPolicy {
    /// Prefer the installed version when multiple candidates exist,
    /// but allow upgrades if required by dependencies.
    Favor,
    /// The installed version must not change — only that exact version
    /// is acceptable.
    Lock,
}

pub(crate) struct VersionDeps {
    /// Merged deps for PubGrub's DependencyProvider trait.
    pub(crate) merged: Dependencies<PortagePackage, PortageVersionSet, String>,
    /// Per-class converted deps.
    /// Index: 0=DEPEND, 1=RDEPEND, 2=BDEPEND, 3=PDEPEND, 4=IDEPEND
    pub(crate) by_class: Vec<Vec<(PortagePackage, PortageVersionSet)>>,
}

impl VersionDeps {
    /// Build from pre-extracted per-class requirements.  `by_class` is moved
    /// in directly; `merged` is collected from a flattened view of it.
    fn from_by_class(by_class: Vec<Vec<(PortagePackage, PortageVersionSet)>>) -> Self {
        let merged = Dependencies::Available(by_class.iter().flatten().cloned().collect());
        Self { merged, by_class }
    }
}

pub(crate) struct PackageData {
    pub(crate) versions: BTreeMap<Version, VersionDeps>,
    pub(crate) blockers: BTreeMap<Version, Vec<Dep>>,
    pub(crate) use_deps: BTreeMap<Version, Vec<convert::UseDepConstraint>>,
    pub(crate) iuse: BTreeMap<Version, Vec<Interned<DefaultInterner>>>,
    pub(crate) repo: BTreeMap<Version, Interned<DefaultInterner>>,
    pub(crate) repo_constraints: BTreeMap<Version, Vec<convert::RepoConstraint>>,
    pub(crate) slot_operator_deps: BTreeMap<Version, Vec<convert::SlotOperatorDep>>,
}

/// A package that is already installed, with its version and policy.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    /// The installed package identity.
    pub package: PortagePackage,
    /// The installed version.
    pub version: Version,
    /// How to treat this package during resolution.
    pub policy: InstalledPolicy,
}

/// Build a per-CPV `UseConfig` by starting from the global config and applying
/// any matching `package_use` entries in order.
///
/// Returns `Borrowed(base)` when `package_use` is empty to avoid a clone per CPV.
fn apply_package_use<'a>(
    base: &'a UseConfig,
    cpv: &portage_atom::Cpv,
    package_use: &[(Dep, Vec<String>)],
) -> Cow<'a, UseConfig> {
    if package_use.is_empty() {
        return Cow::Borrowed(base);
    }
    let mut cfg = base.clone();
    for (dep, flags) in package_use {
        if crate::validate::dep_matches_cpv(dep, cpv) {
            for flag in flags {
                let name = flag.strip_prefix('+').unwrap_or(flag);
                if let Some(stripped) = name.strip_prefix('-') {
                    cfg.disable(portage_atom::interner::Interned::intern(stripped));
                } else {
                    cfg.enable(portage_atom::interner::Interned::intern(name));
                }
            }
        }
    }
    Cow::Owned(cfg)
}

/// A PubGrub `DependencyProvider` backed by a portage package repository.
///
/// Pre-computes all dependency information at construction time, then serves
/// it to the PubGrub solver.
pub struct PortageDependencyProvider {
    pub(crate) packages: HashMap<PortagePackage, PackageData>,
    pub(crate) installed: HashMap<PortagePackage, (Version, InstalledPolicy)>,
    pub(crate) installed_cpns: HashSet<Cpn>,
    pub(crate) dropped_deps: Vec<(PortagePackage, PortageVersionSet)>,
}

impl PortageDependencyProvider {
    /// Build the provider from a repository, a global USE flag configuration,
    /// and per-package USE overrides (from `package.use` / `package.use.force`).
    ///
    /// `package_use` is a list of `(atom, flags)` pairs applied in order; a
    /// flag prefixed with `-` disables it, a bare or `+`-prefixed flag enables
    /// it.  Entries are matched against each CPV using the atom's version
    /// constraint, so `>=dev-libs/foo-2.0 flag` only affects matching versions.
    pub fn new<R: PackageRepository>(
        repo: R,
        use_config: UseConfig,
        package_use: &[(Dep, Vec<String>)],
    ) -> Self {
        let mut packages = HashMap::new();
        let mut cpn_slots: HashMap<portage_atom::Cpn, Vec<Interned<DefaultInterner>>> =
            HashMap::new();

        // First pass: collect slots per CPN directly from version metadata.
        // This ensures slots are derived from the same filtered data that
        // versions_for provides, avoiding phantom slots for live/9999 ebuilds.
        for cpn in repo.all_packages() {
            let versions = repo.versions_for(&cpn);
            let mut slots: Vec<Interned<DefaultInterner>> =
                versions.iter().filter_map(|(_, meta)| meta.slot).collect();
            slots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            slots.dedup();
            if !slots.is_empty() {
                cpn_slots.insert(cpn, slots);
            }
        }

        // Build the slot map for convert_deps.
        let slot_map: convert::SlotMap = cpn_slots
            .iter()
            .map(|(&cpn, slots)| {
                let slot_packages = slots
                    .iter()
                    .map(|&s| (s, PortagePackage::slotted(cpn, s)))
                    .collect();
                (cpn, slot_packages)
            })
            .collect();

        // Second pass: register versions and convert deps.
        for cpn in repo.all_packages() {
            let versions_data = repo.versions_for(&cpn);

            for (cpv, meta) in versions_data {
                let pkg = match &meta.slot {
                    Some(slot) => PortagePackage::slotted(cpn, *slot),
                    None => {
                        if let Some([(_, sole_pkg)]) = slot_map.get(&cpn).map(|v| v.as_slice()) {
                            sole_pkg.clone()
                        } else {
                            PortagePackage::unslotted(cpn)
                        }
                    }
                };

                let cpn_str = format!("{}/{}", cpn.category, cpn.package);

                let dep_classes: [&[portage_atom::DepEntry]; 5] = [
                    &meta.deps.depend,
                    &meta.deps.rdepend,
                    &meta.deps.bdepend,
                    &meta.deps.pdepend,
                    &meta.deps.idepend,
                ];

                let cpv_use_cfg = apply_package_use(&use_config, &cpv, package_use);

                let class_results: [convert::ConversionResult; 5] = dep_classes.map(|entries| {
                    convert::convert_deps(
                        entries,
                        &cpn_str,
                        &cpv_use_cfg,
                        &slot_map,
                        &meta.iuse_defaults,
                    )
                });

                let mut all_blockers = Vec::new();
                let mut all_use_deps = Vec::new();
                let mut all_repo_constraints = Vec::new();
                let mut all_virtual_choices = Vec::new();
                let mut all_slot_operator_deps = Vec::new();
                let mut by_class: Vec<Vec<(PortagePackage, PortageVersionSet)>> =
                    Vec::with_capacity(5);

                for result in class_results {
                    all_blockers.extend(result.blockers);
                    all_use_deps.extend(result.use_deps);
                    all_repo_constraints.extend(result.repo_constraints);
                    all_virtual_choices.extend(result.virtual_choices);
                    all_slot_operator_deps.extend(result.slot_operator_deps);
                    by_class.push(result.requirements);
                }

                let ver_deps = VersionDeps::from_by_class(by_class);
                let entry = packages.entry(pkg).or_insert_with(|| PackageData {
                    versions: BTreeMap::new(),
                    blockers: BTreeMap::new(),
                    use_deps: BTreeMap::new(),
                    iuse: BTreeMap::new(),
                    repo: BTreeMap::new(),
                    repo_constraints: BTreeMap::new(),
                    slot_operator_deps: BTreeMap::new(),
                });
                let ver = cpv.version.clone();
                entry.versions.insert(ver.clone(), ver_deps);
                if !all_blockers.is_empty() {
                    entry.blockers.insert(ver.clone(), all_blockers);
                }
                if !all_use_deps.is_empty() {
                    entry.use_deps.insert(ver.clone(), all_use_deps);
                }
                if !meta.iuse.is_empty() {
                    entry.iuse.insert(ver.clone(), meta.iuse);
                }
                if let Some(r) = meta.repo {
                    entry.repo.insert(ver.clone(), r);
                }
                if !all_repo_constraints.is_empty() {
                    entry
                        .repo_constraints
                        .insert(ver.clone(), all_repo_constraints);
                }
                if !all_slot_operator_deps.is_empty() {
                    entry.slot_operator_deps.insert(ver, all_slot_operator_deps);
                }

                register_virtual_choices(&mut packages, all_virtual_choices);
            }
        }

        // Post-process: remove dependencies on packages not present in the
        // repository.  Without this filtering, PubGrub will encounter
        // `NoVersions` for any missing package and immediately declare the
        // problem unsolvable.
        let known: HashSet<PortagePackage> = packages.keys().cloned().collect();
        let mut dropped_deps = Vec::new();
        for data in packages.values_mut() {
            for vd in data.versions.values_mut() {
                if let Dependencies::Available(constraints) = &mut vd.merged {
                    let taken = std::mem::take(constraints);
                    let (kept, dropped): (Vec<_>, Vec<_>) =
                        taken.into_iter().partition(|(pkg, _)| known.contains(pkg));
                    dropped_deps.extend(dropped);
                    *constraints = kept.into_iter().collect();
                }
                for class in &mut vd.by_class {
                    class.retain(|(pkg, _)| known.contains(pkg));
                }
            }
        }

        Self {
            packages,
            installed: HashMap::new(),
            installed_cpns: HashSet::new(),
            dropped_deps,
        }
    }

    /// Register an installed package.
    ///
    /// **Favored** packages are preferred during version selection but may be
    /// upgraded if a dependency requires it. **Locked** packages are pinned to
    /// their exact installed version.
    pub fn add_installed(&mut self, installed: InstalledPackage) {
        self.installed_cpns.insert(*installed.package.cpn());
        self.installed
            .insert(installed.package, (installed.version, installed.policy));
    }

    /// Returns the list of dependencies that were dropped during construction
    /// because their target package was not present in the repository.
    ///
    /// Each entry is the `(package, version_set)` that could not be resolved.
    /// Callers should inspect this list to detect typos or genuinely missing
    /// packages rather than silently accepting an incomplete solution.
    pub fn dropped_deps(&self) -> &[(PortagePackage, PortageVersionSet)] {
        &self.dropped_deps
    }

    /// Return all real (non-virtual, non-synthetic) packages in the provider
    /// whose CPN matches `cpn`.
    ///
    /// For packages with a single slot this returns one entry; for multi-slot
    /// packages (e.g. `dev-lang/python:3.11`, `dev-lang/python:3.12`) it
    /// returns one entry per slot.  Returns an empty vec if the CPN is not
    /// present in the repository.
    pub fn packages_for_cpn(&self, cpn: &portage_atom::Cpn) -> Vec<PortagePackage> {
        self.packages
            .keys()
            .filter(|p| !p.is_virtual() && p.cpn() == cpn)
            .cloned()
            .collect()
    }

    /// Return all versions registered for a given package, sorted ascending.
    pub fn versions_for_pkg(&self, pkg: &PortagePackage) -> Vec<Version> {
        self.packages
            .get(pkg)
            .map(|d| d.versions.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Return the merged dependency requirements for a specific package version,
    /// or `None` if the package/version is not registered.
    pub fn deps_for(
        &self,
        pkg: &PortagePackage,
        ver: &Version,
    ) -> Option<Vec<(PortagePackage, PortageVersionSet)>> {
        let data = self.packages.get(pkg)?;
        let vd = data.versions.get(ver)?;
        if let Dependencies::Available(reqs) = &vd.merged {
            Some(reqs.iter().cloned().collect::<Vec<_>>())
        } else {
            None
        }
    }

    /// Resolve a set of target packages using PubGrub.
    ///
    /// Creates an `__internal__/root` node whose dependencies are the given
    /// `targets`, runs the solver, then strips all `__internal__/` bookkeeping
    /// nodes (root + any USE-flag decision nodes) before returning.
    /// Callers only receive real Gentoo packages.  See `package.rs` for the
    /// full description of the `__internal__/` convention.
    ///
    /// Each target is a `(PortagePackage, PortageVersionSet)` pair, e.g. the
    /// package `dev-libs/openssl` with the version set `>=3.0`.
    #[allow(clippy::result_large_err)]
    pub fn resolve_targets(
        &mut self,
        targets: Vec<(PortagePackage, PortageVersionSet)>,
    ) -> std::result::Result<
        SelectedDependencies<PortagePackage, Version>,
        pubgrub::PubGrubError<Self>,
    > {
        let root = PortagePackage::synthetic_root();
        let root_ver = Version::parse("0").unwrap();

        let constraints: DependencyConstraints<PortagePackage, PortageVersionSet> =
            targets.iter().cloned().collect();
        let vd = VersionDeps {
            merged: Dependencies::Available(constraints),
            by_class: vec![targets, vec![], vec![], vec![], vec![]],
        };
        let entry = self
            .packages
            .entry(root.clone())
            .or_insert_with(|| PackageData {
                versions: BTreeMap::new(),
                blockers: BTreeMap::new(),
                use_deps: BTreeMap::new(),
                iuse: BTreeMap::new(),
                repo: BTreeMap::new(),
                repo_constraints: BTreeMap::new(),
                slot_operator_deps: BTreeMap::new(),
            });
        entry.versions.insert(root_ver.clone(), vd);

        let solution = pubgrub::resolve(self, root.clone(), root_ver)?;
        self.packages.remove(&root);
        Ok(solution
            .into_iter()
            .filter(|(p, _)| !p.is_virtual())
            .collect())
    }

    /// Returns true if the deps of `vd` transitively reach any installed CPN,
    /// descending up to `depth` levels through `__internal__/*` virtual packages.
    fn deps_reach_installed(&self, vd: &VersionDeps, depth: u8) -> bool {
        let Dependencies::Available(ref constraints) = vd.merged else {
            return false;
        };
        for (dep_pkg, _) in constraints.iter() {
            if dep_pkg.is_virtual() {
                if depth > 0
                    && let Some(dep_data) = self.packages.get(dep_pkg)
                {
                    for dep_vd in dep_data.versions.values() {
                        if self.deps_reach_installed(dep_vd, depth - 1) {
                            return true;
                        }
                    }
                }
            } else if self.installed_cpns.contains(dep_pkg.cpn()) {
                return true;
            }
        }
        false
    }
}

impl DependencyProvider for PortageDependencyProvider {
    type P = PortagePackage;
    type V = Version;
    type VS = PortageVersionSet;
    type M = String;
    type Err = Error;
    type Priority = (u32, Reverse<usize>);

    fn prioritize(
        &self,
        package: &Self::P,
        range: &Self::VS,
        stats: &PackageResolutionStatistics,
    ) -> Self::Priority {
        let count = self
            .packages
            .get(package)
            .map(|d| d.versions.keys().filter(|v| range.contains(v)).count())
            .unwrap_or(0);
        (stats.conflict_count(), Reverse(count))
    }

    fn choose_version(
        &self,
        package: &Self::P,
        range: &Self::VS,
    ) -> std::result::Result<Option<Self::V>, Self::Err> {
        let Some(data) = self.packages.get(package) else {
            return Ok(None);
        };

        let candidates: Vec<&Version> =
            data.versions.keys().filter(|v| range.contains(v)).collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        if let Some((installed_ver, policy)) = self.installed.get(package) {
            match policy {
                InstalledPolicy::Lock => {
                    if range.contains(installed_ver) {
                        return Ok(Some(installed_ver.clone()));
                    }
                    return Ok(None);
                }
                InstalledPolicy::Favor => {
                    if range.contains(installed_ver) {
                        return Ok(Some(installed_ver.clone()));
                    }
                }
            }
        }

        // For OR-group choice packages, prefer branches that lead to installed packages.
        // Only applied when installed packages exist and some-but-not-all branches match,
        // so ties (all/none installed) fall through to the default highest-version pick.
        if package.is_virtual() && !self.installed_cpns.is_empty() {
            let has_installed: Vec<bool> = candidates
                .iter()
                .map(|&ver| {
                    data.versions
                        .get(ver)
                        .is_some_and(|vd| self.deps_reach_installed(vd, 2))
                })
                .collect();
            let installed_count = has_installed.iter().filter(|&&x| x).count();
            if installed_count > 0 && installed_count < candidates.len() {
                let best = candidates
                    .into_iter()
                    .zip(has_installed)
                    .filter(|(_, has)| *has)
                    .map(|(v, _)| v)
                    .max()
                    .cloned();
                return Ok(best);
            }
        }

        let version = candidates.into_iter().max().cloned();
        Ok(version)
    }

    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> std::result::Result<Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
        let Some(data) = self.packages.get(package) else {
            return Ok(Dependencies::Unavailable(format!(
                "package not found: {}",
                package
            )));
        };
        match data.versions.get(version) {
            Some(vd) => Ok(vd.merged.clone()),
            None => Ok(Dependencies::Unavailable(format!(
                "version not found: {}@{}",
                package, version
            ))),
        }
    }
}

fn register_virtual_choices(
    packages: &mut HashMap<PortagePackage, PackageData>,
    choices: Vec<convert::VirtualChoice>,
) {
    for vc in choices {
        let entry = packages.entry(vc.package).or_insert_with(|| PackageData {
            versions: BTreeMap::new(),
            blockers: BTreeMap::new(),
            use_deps: BTreeMap::new(),
            iuse: BTreeMap::new(),
            repo: BTreeMap::new(),
            repo_constraints: BTreeMap::new(),
            slot_operator_deps: BTreeMap::new(),
        });
        for (ver, deps) in vc.versions {
            let vd = VersionDeps {
                merged: Dependencies::Available(deps.into_iter().collect()),
                by_class: vec![vec![], vec![], vec![], vec![], vec![]],
            };
            entry.versions.insert(ver, vd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{InMemoryRepository, PackageDeps};
    use portage_atom::interner::Interned;
    use portage_atom::{Cpn, Dep, DepEntry};

    fn empty_deps() -> PackageDeps {
        PackageDeps {
            depend: vec![],
            rdepend: vec![],
            bdepend: vec![],
            pdepend: vec![],
            idepend: vec![],
        }
    }

    fn make_simple_repo() -> InMemoryRepository {
        let mut repo = InMemoryRepository::new();

        let openssl_cpv = portage_atom::Cpv::parse("dev-libs/openssl-3.0.0").unwrap();
        repo.add_version(openssl_cpv, Some(Interned::intern("0")), None, empty_deps());

        let openssl_cpv2 = portage_atom::Cpv::parse("dev-libs/openssl-3.1.0").unwrap();
        repo.add_version(
            openssl_cpv2,
            Some(Interned::intern("0")),
            None,
            empty_deps(),
        );

        let rust_cpv = portage_atom::Cpv::parse("dev-lang/rust-1.75.0").unwrap();
        repo.add_version(
            rust_cpv,
            Some(Interned::intern("0")),
            None,
            PackageDeps {
                depend: DepEntry::parse(">=dev-libs/openssl-3.0.0").unwrap(),
                rdepend: DepEntry::parse(">=dev-libs/openssl-3.0.0").unwrap(),
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        repo
    }

    #[test]
    fn provider_constructs() {
        let repo = make_simple_repo();
        let config = UseConfig::new();
        let _provider = PortageDependencyProvider::new(repo, config, &[]);
    }

    #[test]
    fn choose_highest_version() {
        let repo = make_simple_repo();
        let config = UseConfig::new();
        let provider = PortageDependencyProvider::new(repo, config, &[]);
        let openssl = PortagePackage::slotted(
            portage_atom::Cpn::parse("dev-libs/openssl").unwrap(),
            Interned::intern("0"),
        );
        let version = provider
            .choose_version(&openssl, &PortageVersionSet::any())
            .unwrap();
        assert_eq!(version, Some(Version::parse("3.1.0").unwrap()));
    }

    #[test]
    fn resolve_simple() {
        let repo = make_simple_repo();
        let config = UseConfig::new();
        let provider = PortageDependencyProvider::new(repo, config, &[]);
        let root = PortagePackage::slotted(
            portage_atom::Cpn::parse("dev-lang/rust").unwrap(),
            Interned::intern("0"),
        );
        let result = pubgrub::resolve(&provider, root, Version::parse("1.75.0").unwrap());
        assert!(result.is_ok());
        let solution = result.unwrap();
        assert!(
            solution
                .get(&PortagePackage::slotted(
                    portage_atom::Cpn::parse("dev-libs/openssl").unwrap(),
                    Interned::intern("0"),
                ))
                .is_some()
        );
    }

    #[test]
    fn multi_slot_installs_both_when_required() {
        let mut repo = InMemoryRepository::new();

        repo.add_version(
            portage_atom::Cpv::parse("dev-lang/python-3.11.9").unwrap(),
            Some(Interned::intern("3.11")),
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-lang/python-3.12.4").unwrap(),
            Some(Interned::intern("3.12")),
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("app-misc/myapp-1.0").unwrap(),
            Some(Interned::intern("0")),
            None,
            PackageDeps {
                depend: vec![
                    DepEntry::Atom(Dep::parse("dev-lang/python:3.11").unwrap()),
                    DepEntry::Atom(Dep::parse("dev-lang/python:3.12").unwrap()),
                ],
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let root =
            PortagePackage::slotted(Cpn::parse("app-misc/myapp").unwrap(), Interned::intern("0"));
        let result = pubgrub::resolve(&provider, root, Version::parse("1.0").unwrap());
        assert!(result.is_ok());
        let solution = result.unwrap();
        assert!(
            solution
                .get(&PortagePackage::slotted(
                    Cpn::parse("dev-lang/python").unwrap(),
                    Interned::intern("3.11"),
                ))
                .is_some(),
            "python:3.11 should be in solution"
        );
        assert!(
            solution
                .get(&PortagePackage::slotted(
                    Cpn::parse("dev-lang/python").unwrap(),
                    Interned::intern("3.12"),
                ))
                .is_some(),
            "python:3.12 should be in solution"
        );
    }

    #[test]
    fn resolve_slot_operator_equal() {
        let mut repo = InMemoryRepository::new();

        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.0.0").unwrap(),
            Some(Interned::intern("0")),
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("app-misc/myapp-1.0").unwrap(),
            Some(Interned::intern("0")),
            None,
            PackageDeps {
                depend: DepEntry::parse("dev-libs/openssl:=").unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let root =
            PortagePackage::slotted(Cpn::parse("app-misc/myapp").unwrap(), Interned::intern("0"));
        let result = pubgrub::resolve(&provider, root, Version::parse("1.0").unwrap());
        assert!(result.is_ok());
        let solution = result.unwrap();
        assert!(
            solution
                .get(&PortagePackage::slotted(
                    Cpn::parse("dev-libs/openssl").unwrap(),
                    Interned::intern("0"),
                ))
                .is_some()
        );
    }

    #[test]
    fn resolve_slot_operator_star() {
        let mut repo = InMemoryRepository::new();

        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.0.0").unwrap(),
            Some(Interned::intern("0")),
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("app-misc/myapp-1.0").unwrap(),
            Some(Interned::intern("0")),
            None,
            PackageDeps {
                depend: DepEntry::parse("dev-libs/openssl:*").unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let root =
            PortagePackage::slotted(Cpn::parse("app-misc/myapp").unwrap(), Interned::intern("0"));
        let result = pubgrub::resolve(&provider, root, Version::parse("1.0").unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn installed_favored_picks_installed_version() {
        let mut repo = InMemoryRepository::new();

        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.0.0").unwrap(),
            None,
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.1.0").unwrap(),
            None,
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("app-misc/myapp-1.0").unwrap(),
            None,
            None,
            PackageDeps {
                depend: DepEntry::parse(">=dev-libs/openssl-3.0.0").unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let mut provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let openssl = PortagePackage::unslotted(Cpn::parse("dev-libs/openssl").unwrap());
        provider.add_installed(InstalledPackage {
            package: openssl,
            version: Version::parse("3.0.0").unwrap(),
            policy: InstalledPolicy::Favor,
        });

        let myapp = PortagePackage::unslotted(Cpn::parse("app-misc/myapp").unwrap());
        let solution = provider
            .resolve_targets(vec![(myapp, PortageVersionSet::any())])
            .unwrap();
        assert_eq!(
            solution.get(&PortagePackage::unslotted(
                Cpn::parse("dev-libs/openssl").unwrap()
            )),
            Some(&Version::parse("3.0.0").unwrap()),
            "should pick favored installed version 3.0.0 over 3.1.0"
        );
    }

    #[test]
    fn installed_favored_upgrades_when_required() {
        let mut repo = InMemoryRepository::new();

        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.0.0").unwrap(),
            None,
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.1.0").unwrap(),
            None,
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("app-misc/myapp-1.0").unwrap(),
            None,
            None,
            PackageDeps {
                depend: DepEntry::parse(">=dev-libs/openssl-3.1.0").unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let mut provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let openssl = PortagePackage::unslotted(Cpn::parse("dev-libs/openssl").unwrap());
        provider.add_installed(InstalledPackage {
            package: openssl,
            version: Version::parse("3.0.0").unwrap(),
            policy: InstalledPolicy::Favor,
        });

        let myapp = PortagePackage::unslotted(Cpn::parse("app-misc/myapp").unwrap());
        let solution = provider
            .resolve_targets(vec![(myapp, PortageVersionSet::any())])
            .unwrap();
        assert_eq!(
            solution.get(&PortagePackage::unslotted(
                Cpn::parse("dev-libs/openssl").unwrap()
            )),
            Some(&Version::parse("3.1.0").unwrap()),
            "should upgrade from favored 3.0.0 to 3.1.0 when required"
        );
    }

    #[test]
    fn installed_locked_pins_version() {
        let mut repo = InMemoryRepository::new();

        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.0.0").unwrap(),
            None,
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/openssl-3.1.0").unwrap(),
            None,
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("app-misc/myapp-1.0").unwrap(),
            None,
            None,
            PackageDeps {
                depend: DepEntry::parse(">=dev-libs/openssl-3.0.0").unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let mut provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let openssl = PortagePackage::unslotted(Cpn::parse("dev-libs/openssl").unwrap());
        provider.add_installed(InstalledPackage {
            package: openssl,
            version: Version::parse("3.0.0").unwrap(),
            policy: InstalledPolicy::Lock,
        });

        let myapp = PortagePackage::unslotted(Cpn::parse("app-misc/myapp").unwrap());
        let solution = provider
            .resolve_targets(vec![(myapp, PortageVersionSet::any())])
            .unwrap();
        assert_eq!(
            solution.get(&PortagePackage::unslotted(
                Cpn::parse("dev-libs/openssl").unwrap()
            )),
            Some(&Version::parse("3.0.0").unwrap()),
            "locked should pin to 3.0.0 even though 3.1.0 exists"
        );
    }

    #[test]
    fn or_group_prefers_installed_alternative() {
        // || ( dev-libs/not-installed dev-libs/installed ) — installed is listed second.
        // Without installed preference the solver picks "not-installed" (higher choice version).
        // With installed preference it should pick "installed".
        let mut repo = InMemoryRepository::new();

        let not_inst = portage_atom::Cpv::parse("dev-libs/not-installed-1.0").unwrap();
        repo.add_version(not_inst, Some(Interned::intern("0")), None, empty_deps());

        let inst = portage_atom::Cpv::parse("dev-libs/installed-1.0").unwrap();
        repo.add_version(
            inst.clone(),
            Some(Interned::intern("0")),
            None,
            empty_deps(),
        );

        let consumer = portage_atom::Cpv::parse("app-misc/consumer-1.0").unwrap();
        repo.add_version(
            consumer,
            Some(Interned::intern("0")),
            None,
            PackageDeps {
                depend: DepEntry::parse("|| ( dev-libs/not-installed dev-libs/installed )")
                    .unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let config = UseConfig::new();
        let mut provider = PortageDependencyProvider::new(repo, config, &[]);

        provider.add_installed(InstalledPackage {
            package: PortagePackage::slotted(
                Cpn::parse("dev-libs/installed").unwrap(),
                Interned::intern("0"),
            ),
            version: Version::parse("1.0").unwrap(),
            policy: InstalledPolicy::Favor,
        });

        let consumer_pkg = PortagePackage::slotted(
            Cpn::parse("app-misc/consumer").unwrap(),
            Interned::intern("0"),
        );
        let solution = provider
            .resolve_targets(vec![(consumer_pkg, PortageVersionSet::any())])
            .unwrap();

        let in_solution = |cpn: &str| {
            let pkg = PortagePackage::slotted(Cpn::parse(cpn).unwrap(), Interned::intern("0"));
            solution.get(&pkg).is_some()
        };

        assert!(
            in_solution("dev-libs/installed"),
            "installed package should be chosen"
        );
        assert!(
            !in_solution("dev-libs/not-installed"),
            "non-installed alternative should not be chosen"
        );
    }

    #[test]
    fn or_group_no_preference_when_both_installed() {
        // || ( A B ) where both A and B are installed — solver falls through to
        // highest choice version (A, listed first), same as without installed preference.
        let mut repo = InMemoryRepository::new();

        for cpv in ["dev-libs/a-1.0", "dev-libs/b-1.0"] {
            repo.add_version(
                portage_atom::Cpv::parse(cpv).unwrap(),
                Some(Interned::intern("0")),
                None,
                empty_deps(),
            );
        }

        let consumer = portage_atom::Cpv::parse("app-misc/consumer-1.0").unwrap();
        repo.add_version(
            consumer,
            Some(Interned::intern("0")),
            None,
            PackageDeps {
                depend: DepEntry::parse("|| ( dev-libs/a dev-libs/b )").unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let config = UseConfig::new();
        let mut provider = PortageDependencyProvider::new(repo, config, &[]);

        for cpn in ["dev-libs/a", "dev-libs/b"] {
            provider.add_installed(InstalledPackage {
                package: PortagePackage::slotted(Cpn::parse(cpn).unwrap(), Interned::intern("0")),
                version: Version::parse("1.0").unwrap(),
                policy: InstalledPolicy::Favor,
            });
        }

        let consumer_pkg = PortagePackage::slotted(
            Cpn::parse("app-misc/consumer").unwrap(),
            Interned::intern("0"),
        );
        let solution = provider
            .resolve_targets(vec![(consumer_pkg, PortageVersionSet::any())])
            .unwrap();

        // With both installed, falls through to highest choice version = a (listed first).
        let in_sol = |cpn: &str| {
            solution
                .get(&PortagePackage::slotted(
                    Cpn::parse(cpn).unwrap(),
                    Interned::intern("0"),
                ))
                .is_some()
        };
        assert!(
            in_sol("dev-libs/a"),
            "first alternative chosen when both installed"
        );
        assert!(!in_sol("dev-libs/b"));
    }

    #[test]
    fn or_group_prefers_installed_with_slot_nesting() {
        // Mirrors the real-world case: || ( >=A-1.0:* >=B-1.0:* ) where A has
        // multiple slots (triggering the choice→slot→pkg two-level nesting) and
        // only B is installed.  The solver should pick B.
        let mut repo = InMemoryRepository::new();

        // A has two slots (1.0 and 2.0) — not installed
        for (cpv, slot) in [("dev-libs/a-1.0", "1"), ("dev-libs/a-2.0", "2")] {
            repo.add_version(
                portage_atom::Cpv::parse(cpv).unwrap(),
                Some(Interned::intern(slot)),
                None,
                empty_deps(),
            );
        }

        // B has a single slot — installed
        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/b-1.0").unwrap(),
            Some(Interned::intern("0")),
            None,
            empty_deps(),
        );

        let consumer = portage_atom::Cpv::parse("app-misc/consumer-1.0").unwrap();
        repo.add_version(
            consumer,
            Some(Interned::intern("0")),
            None,
            PackageDeps {
                // slot-star deps trigger push_unslotted_or_choice → slot_* nesting
                depend: DepEntry::parse("|| ( dev-libs/a:* dev-libs/b:* )").unwrap(),
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );

        let config = UseConfig::new();
        let mut provider = PortageDependencyProvider::new(repo, config, &[]);

        provider.add_installed(InstalledPackage {
            package: PortagePackage::slotted(
                Cpn::parse("dev-libs/b").unwrap(),
                Interned::intern("0"),
            ),
            version: Version::parse("1.0").unwrap(),
            policy: InstalledPolicy::Favor,
        });

        let consumer_pkg = PortagePackage::slotted(
            Cpn::parse("app-misc/consumer").unwrap(),
            Interned::intern("0"),
        );
        let solution = provider
            .resolve_targets(vec![(consumer_pkg, PortageVersionSet::any())])
            .unwrap();

        let b_in_sol = solution
            .get(&PortagePackage::slotted(
                Cpn::parse("dev-libs/b").unwrap(),
                Interned::intern("0"),
            ))
            .is_some();
        let a_in_sol = solution
            .iter()
            .any(|(p, _)| p.cpn().package.as_str() == "a");

        assert!(
            b_in_sol,
            "installed B should be chosen over non-installed A"
        );
        assert!(!a_in_sol, "non-installed A should not appear in solution");
    }

    #[test]
    fn packages_for_cpn_excludes_virtual_choice_nodes() {
        // Multi-slot packages cause register_virtual_choices to insert Choice
        // nodes into self.packages. packages_for_cpn must skip those nodes
        // rather than calling cpn() on them (which panics).
        let mut repo = InMemoryRepository::new();
        repo.add_version(
            portage_atom::Cpv::parse("dev-lang/python-3.11.9").unwrap(),
            Some(Interned::intern("3.11")),
            None,
            empty_deps(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-lang/python-3.12.4").unwrap(),
            Some(Interned::intern("3.12")),
            None,
            empty_deps(),
        );

        let provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let cpn = Cpn::parse("dev-lang/python").unwrap();
        let pkgs = provider.packages_for_cpn(&cpn);

        assert_eq!(pkgs.len(), 2, "expected one entry per slot");
        assert!(pkgs.iter().all(|p| !p.is_virtual()), "no virtual nodes");
        assert!(
            pkgs.iter()
                .any(|p| p.slot() == Some(Interned::intern("3.11"))),
            "slot 3.11 present"
        );
        assert!(
            pkgs.iter()
                .any(|p| p.slot() == Some(Interned::intern("3.12"))),
            "slot 3.12 present"
        );
    }
}
