use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use portage_atom::interner::{DefaultInterner, Interned};
use portage_atom::{Cpn, Dep, DepEntry, Operator, SlotDep, SlotOperator, UseDep, Version};

use crate::package::PortagePackage;
use crate::repository::IUseDefault;
use crate::use_config::{UseConfig, UseFlagState};
use crate::version_set::PortageVersionSet;

static CHOICE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_choice_id() -> u64 {
    CHOICE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// A virtual choice package encoding an OR group (`||`, `^^`, `??`).
///
/// The solver picks exactly one version of this package, and each version's
/// dependencies are one alternative from the group.
#[derive(Clone)]
pub(crate) struct VirtualChoice {
    /// The virtual package to register in the provider.
    pub package: PortagePackage,
    /// (version, dependencies for that version).
    pub versions: Vec<(Version, Vec<(PortagePackage, PortageVersionSet)>)>,
}

/// Result of converting a dependency tree.
#[derive(Clone)]
pub(crate) struct ConversionResult {
    /// Direct dependency constraints.
    pub requirements: Vec<(PortagePackage, PortageVersionSet)>,
    /// Blocker atoms for post-solve validation.
    pub blockers: Vec<Dep>,
    /// Virtual choice packages to register in the provider.
    pub virtual_choices: Vec<VirtualChoice>,
    /// USE-dep constraints for post-solve validation.
    pub use_deps: Vec<UseDepConstraint>,
    /// Repo-constrained deps for post-solve validation.
    pub repo_constraints: Vec<RepoConstraint>,
    /// Slot-operator deps (`:=` / `:0=`) for post-solve slot binding.
    pub slot_operator_deps: Vec<SlotOperatorDep>,
}

/// A slot-operator dependency extracted during conversion.
///
/// Records `:=` and `:slot=` dependencies so that, after resolution,
/// the solver's slot assignment can be bound for rebuild tracking.
///
/// `:*` dependencies are **not** collected — they express "any slot"
/// with no rebuild implications.
///
/// See [PMS 8.3.3](https://projects.gentoo.org/pms/9/pms.html#slot_deps).
#[derive(Debug, Clone)]
pub(crate) struct SlotOperatorDep {
    /// The target package and version range.
    pub target: (PortagePackage, PortageVersionSet),
    /// The slot operator (`Equal` for `:=` / `:0=`).
    pub operator: SlotOperator,
    /// The declared slot name, if the atom specified one (`:0=` → `Some("0")`,
    /// `:=` → `None`).
    pub slot: Option<Interned<DefaultInterner>>,
}

/// A repository constraint extracted from a dependency atom.
///
/// See [PMS 8.3.5](https://projects.gentoo.org/pms/9/pms.html#repository).
#[derive(Debug, Clone)]
pub(crate) struct RepoConstraint {
    /// The target package that must come from a specific repo.
    pub target: (PortagePackage, PortageVersionSet),
    /// The required repository name.
    pub repo: Interned<DefaultInterner>,
}

/// A USE-dep constraint extracted from a dependency atom.
///
/// See [PMS 8.3.4](https://projects.gentoo.org/pms/9/pms.html#style-and-style-use-dependencies).
#[derive(Debug, Clone)]
pub(crate) struct UseDepConstraint {
    /// The package that must satisfy the USE constraints.
    pub target: (PortagePackage, PortageVersionSet),
    /// The USE flag constraints on that package.
    pub use_deps: Vec<UseDep>,
    /// The parent package's CPN string (e.g. "dev-libs/openssl"),
    /// used to look up solver-decided USE virtuals for `[flag?]`/`[flag=]`.
    pub parent_cpn_str: String,
}

/// Slot info for a CPN: pre-computed `(slot_interned, slotted_package)` pairs.
pub type SlotMap = HashMap<Cpn, Vec<(Interned<DefaultInterner>, PortagePackage)>>;

/// Convert a `DepEntry` tree into PubGrub dependency constraints.
///
/// USE conditionals are handled according to the `UseConfig`:
/// - `Enabled`/`Disabled` flags are eagerly evaluated
/// - `SolverDecided` flags produce virtual package references
///
/// OR groups (`||`, `^^`, `??`) are encoded as virtual choice packages.
///
/// `slot_map` maps each CPN to its known slots, so unslotted deps on
/// multi-slot packages can be resolved correctly.
pub fn convert_deps(
    entries: &[DepEntry],
    cpn_str: &str,
    use_config: &UseConfig,
    slot_map: &SlotMap,
    iuse_defaults: &HashMap<Interned<DefaultInterner>, IUseDefault>,
) -> ConversionResult {
    let mut ctx = ConvertCtx {
        cpn_str,
        use_config,
        slot_map,
        iuse_defaults,
        requirements: Vec::new(),
        blockers: Vec::new(),
        virtual_choices: Vec::new(),
        use_deps: Vec::new(),
        repo_constraints: Vec::new(),
        slot_operator_deps: Vec::new(),
    };

    for entry in entries {
        ctx.convert_entry(entry);
    }

    ConversionResult {
        requirements: ctx.requirements,
        blockers: ctx.blockers,
        virtual_choices: ctx.virtual_choices,
        use_deps: ctx.use_deps,
        repo_constraints: ctx.repo_constraints,
        slot_operator_deps: ctx.slot_operator_deps,
    }
}

struct ConvertCtx<'a> {
    cpn_str: &'a str,
    use_config: &'a UseConfig,
    slot_map: &'a SlotMap,
    iuse_defaults: &'a HashMap<Interned<DefaultInterner>, IUseDefault>,
    requirements: Vec<(PortagePackage, PortageVersionSet)>,
    blockers: Vec<Dep>,
    virtual_choices: Vec<VirtualChoice>,
    use_deps: Vec<UseDepConstraint>,
    repo_constraints: Vec<RepoConstraint>,
    slot_operator_deps: Vec<SlotOperatorDep>,
}

impl ConvertCtx<'_> {
    fn convert_entry(&mut self, entry: &DepEntry) {
        match entry {
            DepEntry::Atom(dep) => {
                if dep.blocker.is_some() {
                    self.blockers.push(dep.clone());
                } else {
                    self.convert_atom(dep);
                }
            }
            DepEntry::UseConditional {
                flag,
                negate,
                children,
            } => {
                let state = self
                    .use_config
                    .get_with_iuse_default(flag, self.iuse_defaults.get(flag).copied());
                match state {
                    UseFlagState::Enabled => {
                        if !negate {
                            for child in children {
                                self.convert_entry(child);
                            }
                        }
                    }
                    UseFlagState::Disabled => {
                        if *negate {
                            for child in children {
                                self.convert_entry(child);
                            }
                        }
                    }
                    UseFlagState::SolverDecided => {
                        let flag_name = flag.as_str();
                        let virtual_pkg = PortagePackage::use_decision(Interned::intern(&format!(
                            "USE_{}_{}",
                            self.cpn_str.replace('/', "_"),
                            flag_name
                        )));

                        let on_deps = if *negate {
                            vec![]
                        } else {
                            let saved_reqs = std::mem::take(&mut self.requirements);
                            let saved_blockers = std::mem::take(&mut self.blockers);
                            for child in children {
                                self.convert_entry(child);
                            }
                            let reqs = std::mem::take(&mut self.requirements);
                            let on_blockers = std::mem::take(&mut self.blockers);
                            self.requirements = saved_reqs;
                            self.blockers = saved_blockers;
                            self.blockers.extend(on_blockers);
                            reqs
                        };

                        let off_deps = if *negate {
                            let saved_reqs = std::mem::take(&mut self.requirements);
                            let saved_blockers = std::mem::take(&mut self.blockers);
                            for child in children {
                                self.convert_entry(child);
                            }
                            let reqs = std::mem::take(&mut self.requirements);
                            let off_blockers = std::mem::take(&mut self.blockers);
                            self.requirements = saved_reqs;
                            self.blockers = saved_blockers;
                            self.blockers.extend(off_blockers);
                            reqs
                        } else {
                            vec![]
                        };

                        self.virtual_choices.push(VirtualChoice {
                            package: virtual_pkg.clone(),
                            versions: vec![
                                (Version::parse("0").unwrap(), off_deps),
                                (Version::parse("1").unwrap(), on_deps),
                            ],
                        });
                        self.requirements
                            .push((virtual_pkg, PortageVersionSet::any()));
                    }
                }
            }
            DepEntry::AnyOf(children) | DepEntry::ExactlyOneOf(children) => {
                self.convert_choice_group(children, false);
            }
            DepEntry::AtMostOneOf(children) => {
                self.convert_choice_group(children, true);
            }
            DepEntry::AllOf(children) => {
                for child in children {
                    self.convert_entry(child);
                }
            }
        }
    }

    fn convert_atom(&mut self, dep: &Dep) {
        let cpn = &dep.cpn;
        let version_set = match &dep.version {
            Some(v) => {
                let op = dep.op.unwrap_or(Operator::Equal);
                PortageVersionSet::from_operator(op, dep.glob, v.clone())
            }
            None => PortageVersionSet::any(),
        };

        if let Some(slot_dep) = &dep.slot_dep {
            match slot_dep {
                SlotDep::Slot {
                    slot: Some(slot),
                    op,
                } => {
                    let pkg = PortagePackage::slotted(*cpn, slot.slot);
                    self.collect_post_solve(&pkg, &version_set, dep);
                    self.collect_slot_op(&pkg, &version_set, *op, Some(slot.slot));
                    self.requirements.push((pkg, version_set));
                }
                SlotDep::Slot { slot: None, op } => {
                    self.collect_slot_op_unslotted(cpn, &version_set, *op);
                    self.collect_post_solve_unslotted(cpn, &version_set, dep);
                    self.push_unslotted_or_choice(cpn, version_set);
                }
                SlotDep::Operator(SlotOperator::Equal) => {
                    self.collect_slot_op_unslotted(cpn, &version_set, Some(SlotOperator::Equal));
                    self.collect_post_solve_unslotted(cpn, &version_set, dep);
                    self.push_unslotted_or_choice(cpn, version_set);
                }
                SlotDep::Operator(SlotOperator::Star) => {
                    self.collect_post_solve_unslotted(cpn, &version_set, dep);
                    self.push_unslotted_or_choice(cpn, version_set);
                }
            }
            return;
        }

        self.collect_post_solve_unslotted(cpn, &version_set, dep);
        self.push_unslotted_or_choice(cpn, version_set);
    }

    fn collect_post_solve(&mut self, pkg: &PortagePackage, vs: &PortageVersionSet, dep: &Dep) {
        if let Some(use_deps) = &dep.use_deps
            && !use_deps.is_empty()
        {
            self.use_deps.push(UseDepConstraint {
                target: (pkg.clone(), vs.clone()),
                use_deps: use_deps.clone(),
                parent_cpn_str: self.cpn_str.to_string(),
            });
        }
        if let Some(repo) = dep.repo {
            self.repo_constraints.push(RepoConstraint {
                target: (pkg.clone(), vs.clone()),
                repo,
            });
        }
    }

    fn collect_post_solve_unslotted(&mut self, cpn: &Cpn, vs: &PortageVersionSet, dep: &Dep) {
        if let Some([(_, sole_pkg)]) = self.slot_map.get(cpn).map(|v| v.as_slice()) {
            self.collect_post_solve(sole_pkg, vs, dep);
        } else {
            let pkg = PortagePackage::unslotted(*cpn);
            self.collect_post_solve(&pkg, vs, dep);
        }
    }

    fn collect_slot_op(
        &mut self,
        pkg: &PortagePackage,
        vs: &PortageVersionSet,
        op: Option<SlotOperator>,
        declared_slot: Option<Interned<DefaultInterner>>,
    ) {
        if op == Some(SlotOperator::Equal) {
            self.slot_operator_deps.push(SlotOperatorDep {
                target: (pkg.clone(), vs.clone()),
                operator: SlotOperator::Equal,
                slot: declared_slot,
            });
        }
    }

    fn collect_slot_op_unslotted(
        &mut self,
        cpn: &Cpn,
        vs: &PortageVersionSet,
        op: Option<SlotOperator>,
    ) {
        if op != Some(SlotOperator::Equal) {
            return;
        }
        if let Some([(_, sole_pkg)]) = self.slot_map.get(cpn).map(|v| v.as_slice()) {
            self.slot_operator_deps.push(SlotOperatorDep {
                target: (sole_pkg.clone(), vs.clone()),
                operator: SlotOperator::Equal,
                slot: None,
            });
        } else {
            let pkg = PortagePackage::unslotted(*cpn);
            self.slot_operator_deps.push(SlotOperatorDep {
                target: (pkg, vs.clone()),
                operator: SlotOperator::Equal,
                slot: None,
            });
        }
    }

    fn push_unslotted_or_choice(&mut self, cpn: &Cpn, version_set: PortageVersionSet) {
        let target_slots = self.slot_map.get(cpn).map(|v| v.as_slice());

        match target_slots {
            None | Some([]) => {
                self.requirements
                    .push((PortagePackage::unslotted(*cpn), version_set));
            }
            Some([(_, sole_pkg)]) => {
                self.requirements.push((sole_pkg.clone(), version_set));
            }
            Some(slots) => {
                let id = next_choice_id();
                let choice_pkg =
                    PortagePackage::slot_choice(Interned::intern(&format!("slot_{id}")));
                let n = slots.len();
                let versions: Vec<(Version, Vec<(PortagePackage, PortageVersionSet)>)> = slots
                    .iter()
                    .enumerate()
                    .map(|(i, (_, slot_pkg))| {
                        let ver = Version::new(&[(n - i) as u64]);
                        (ver, vec![(slot_pkg.clone(), version_set.clone())])
                    })
                    .collect();
                self.virtual_choices.push(VirtualChoice {
                    package: choice_pkg.clone(),
                    versions,
                });
                self.requirements
                    .push((choice_pkg, PortageVersionSet::any()));
            }
        }
    }

    fn convert_choice_group(&mut self, children: &[DepEntry], allow_none: bool) {
        let id = next_choice_id();
        let pkg = PortagePackage::choice(Interned::intern(&format!("choice_{id}")));

        let n = children.len();
        let mut versions = Vec::new();

        if allow_none {
            versions.push((Version::new(&[0]), vec![]));
        }

        for (i, child) in children.iter().enumerate() {
            let ver_num = n - i;
            let saved_reqs = std::mem::take(&mut self.requirements);
            let saved_blockers = std::mem::take(&mut self.blockers);

            self.convert_entry(child);

            let deps = std::mem::take(&mut self.requirements);
            let choice_blockers = std::mem::take(&mut self.blockers);

            self.requirements = saved_reqs;
            self.blockers = saved_blockers;
            self.blockers.extend(choice_blockers);

            versions.push((Version::new(&[ver_num as u64]), deps));
        }

        self.virtual_choices.push(VirtualChoice {
            package: pkg.clone(),
            versions,
        });
        self.requirements.push((pkg, PortageVersionSet::any()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubgrub::VersionSet as _;

    fn empty_slots() -> SlotMap {
        HashMap::new()
    }

    fn empty_iuse_defaults() -> HashMap<Interned<DefaultInterner>, IUseDefault> {
        HashMap::new()
    }

    #[test]
    fn convert_simple_atom() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert_eq!(result.requirements[0].0.to_string(), "dev-libs/openssl");
        assert!(result.requirements[0].1.is_full());
    }

    #[test]
    fn convert_versioned_atom() {
        let config = UseConfig::new();
        let entries = DepEntry::parse(">=dev-libs/openssl-3.0.0").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert!(
            result.requirements[0]
                .1
                .contains(&portage_atom::Version::parse("3.0.0").unwrap())
        );
        assert!(
            result.requirements[0]
                .1
                .contains(&portage_atom::Version::parse("3.1.0").unwrap())
        );
        assert!(
            !result.requirements[0]
                .1
                .contains(&portage_atom::Version::parse("2.9.9").unwrap())
        );
    }

    #[test]
    fn convert_eager_use_enabled() {
        let mut config = UseConfig::new();
        config.enable(Interned::intern("ssl"));
        let entries = DepEntry::parse("ssl? ( dev-libs/openssl )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
    }

    #[test]
    fn convert_eager_use_disabled() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("ssl? ( dev-libs/openssl )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert!(result.requirements.is_empty());
    }

    #[test]
    fn convert_blocker_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("!dev-libs/openssl-compat").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.blockers.len(), 1);
    }

    #[test]
    fn convert_any_of_creates_virtual_choice() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("|| ( dev-libs/openssl dev-libs/libressl )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );

        // Should have 1 requirement: the virtual choice package
        assert_eq!(result.requirements.len(), 1);
        assert!(
            result.requirements[0]
                .0
                .to_string()
                .starts_with("__internal__/choice_")
        );

        // Should have 1 virtual choice with 2 versions (one per alternative)
        assert_eq!(result.virtual_choices.len(), 1);
        let vc = &result.virtual_choices[0];
        assert_eq!(vc.versions.len(), 2);

        // Version 0 deps = openssl, version 1 deps = libressl
        let v0_deps = &vc.versions[0].1;
        let v1_deps = &vc.versions[1].1;
        assert_eq!(v0_deps.len(), 1);
        assert_eq!(v1_deps.len(), 1);
        assert!(v0_deps[0].0.to_string().contains("openssl"));
        assert!(v1_deps[0].0.to_string().contains("libressl"));
    }

    #[test]
    fn convert_at_most_one_adds_empty_version() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("?? ( dev-libs/openssl dev-libs/libressl )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );

        assert_eq!(result.virtual_choices.len(), 1);
        let vc = &result.virtual_choices[0];
        // 3 versions: 0 (empty), 1 (openssl), 2 (libressl)
        assert_eq!(vc.versions.len(), 3);
        assert!(vc.versions[0].1.is_empty());
    }

    #[test]
    fn convert_solver_decided_use_creates_virtual() {
        let mut config = UseConfig::new();
        config.solver_decide(Interned::intern("ssl"));
        let entries = DepEntry::parse("ssl? ( dev-libs/openssl )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );

        // Should have 1 requirement: the virtual USE package
        assert_eq!(result.requirements.len(), 1);
        assert!(
            result.requirements[0]
                .0
                .to_string()
                .starts_with("__internal__/USE_")
        );

        // Virtual has 2 versions: 0 (off, empty), 1 (on, openssl)
        assert_eq!(result.virtual_choices.len(), 1);
        let vc = &result.virtual_choices[0];
        assert_eq!(vc.versions.len(), 2);
        assert!(vc.versions[0].1.is_empty()); // off
        assert_eq!(vc.versions[1].1.len(), 1); // on → openssl
    }

    #[test]
    fn convert_single_slot_uses_direct_dep() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl").unwrap();
        let openssl_cpn = portage_atom::Cpn::parse("dev-libs/openssl").unwrap();
        let mut slots = HashMap::new();
        slots.insert(
            openssl_cpn,
            vec![(
                Interned::intern("0"),
                PortagePackage::slotted(openssl_cpn, Interned::intern("0")),
            )],
        );
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &slots,
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert_eq!(result.requirements[0].0.to_string(), "dev-libs/openssl:0");
        assert!(result.virtual_choices.is_empty());
    }

    #[test]
    fn convert_multi_slot_creates_choice() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-lang/python").unwrap();
        let py_cpn = portage_atom::Cpn::parse("dev-lang/python").unwrap();
        let mut slots = HashMap::new();
        slots.insert(
            py_cpn,
            vec![
                (
                    Interned::intern("3.11"),
                    PortagePackage::slotted(py_cpn, Interned::intern("3.11")),
                ),
                (
                    Interned::intern("3.12"),
                    PortagePackage::slotted(py_cpn, Interned::intern("3.12")),
                ),
            ],
        );
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &slots,
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert!(
            result.requirements[0]
                .0
                .to_string()
                .starts_with("__internal__/slot_")
        );
        assert_eq!(result.virtual_choices.len(), 1);
        assert_eq!(result.virtual_choices[0].versions.len(), 2);
    }

    #[test]
    fn convert_slot_operator_any_slot() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl:*").unwrap();
        let openssl_cpn = portage_atom::Cpn::parse("dev-libs/openssl").unwrap();
        let mut slots = HashMap::new();
        slots.insert(
            openssl_cpn,
            vec![(
                Interned::intern("0"),
                PortagePackage::slotted(openssl_cpn, Interned::intern("0")),
            )],
        );
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &slots,
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert_eq!(result.requirements[0].0.to_string(), "dev-libs/openssl:0");
    }

    #[test]
    fn convert_slot_operator_equal() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl:=").unwrap();
        let openssl_cpn = portage_atom::Cpn::parse("dev-libs/openssl").unwrap();
        let mut slots = HashMap::new();
        slots.insert(
            openssl_cpn,
            vec![(
                Interned::intern("0"),
                PortagePackage::slotted(openssl_cpn, Interned::intern("0")),
            )],
        );
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &slots,
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert_eq!(result.requirements[0].0.to_string(), "dev-libs/openssl:0");
    }

    #[test]
    fn convert_named_slot_with_operator() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl:0=").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert_eq!(result.requirements[0].0.to_string(), "dev-libs/openssl:0");
    }

    #[test]
    fn convert_use_dep_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl[ssl]").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert_eq!(result.use_deps.len(), 1);
        assert_eq!(result.use_deps[0].use_deps.len(), 1);
        assert_eq!(result.use_deps[0].use_deps[0].flag.as_str(), "ssl");
    }

    #[test]
    fn convert_use_dep_disabled_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl[-debug]").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.use_deps.len(), 1);
        assert_eq!(
            result.use_deps[0].use_deps[0].kind,
            portage_atom::UseDepKind::Disabled
        );
    }

    #[test]
    fn convert_use_dep_multiple_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl[ssl,-debug]").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.use_deps.len(), 1);
        assert_eq!(result.use_deps[0].use_deps.len(), 2);
    }

    #[test]
    fn convert_no_use_deps() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert!(result.use_deps.is_empty());
    }

    #[test]
    fn convert_blocker_in_or_group_preserved() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("|| ( dev-libs/a !dev-libs/b )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(
            result.blockers.len(),
            1,
            "blocker inside || () should be preserved"
        );
        assert_eq!(result.blockers[0].cpn.package.as_str(), "b");
    }

    #[test]
    fn convert_blocker_in_solver_decided_use_preserved() {
        let mut config = UseConfig::new();
        config.solver_decide(Interned::intern("ssl"));
        let entries = DepEntry::parse("ssl? ( !dev-libs/openssl-compat )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(
            result.blockers.len(),
            1,
            "blocker inside solver-decided USE conditional should be preserved"
        );
        assert_eq!(result.blockers[0].cpn.package.as_str(), "openssl-compat");
    }

    #[test]
    fn convert_iuse_default_enabled() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("ssl? ( dev-libs/openssl )").unwrap();
        let mut defaults = HashMap::new();
        defaults.insert(Interned::intern("ssl"), IUseDefault::Enabled);
        let result = convert_deps(&entries, "test/pkg", &config, &empty_slots(), &defaults);
        assert_eq!(
            result.requirements.len(),
            1,
            "ssl? should include deps when IUSE default is +ssl and config is unset"
        );
    }

    #[test]
    fn convert_iuse_default_disabled() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("ssl? ( dev-libs/openssl )").unwrap();
        let mut defaults = HashMap::new();
        defaults.insert(Interned::intern("ssl"), IUseDefault::Disabled);
        let result = convert_deps(&entries, "test/pkg", &config, &empty_slots(), &defaults);
        assert!(
            result.requirements.is_empty(),
            "ssl? should skip deps when IUSE default is -ssl and config is unset"
        );
    }

    #[test]
    fn convert_iuse_default_overridden_by_config() {
        let mut config = UseConfig::new();
        config.disable(Interned::intern("ssl"));
        let entries = DepEntry::parse("ssl? ( dev-libs/openssl )").unwrap();
        let mut defaults = HashMap::new();
        defaults.insert(Interned::intern("ssl"), IUseDefault::Enabled);
        let result = convert_deps(&entries, "test/pkg", &config, &empty_slots(), &defaults);
        assert!(
            result.requirements.is_empty(),
            "explicit config should override IUSE default"
        );
    }

    #[test]
    fn convert_negated_use_conditional() {
        let mut config = UseConfig::new();
        config.disable(Interned::intern("ssl"));
        let entries = DepEntry::parse("!ssl? ( dev-libs/openssl )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(
            result.requirements.len(),
            1,
            "!ssl? with ssl disabled should include deps"
        );

        let mut config2 = UseConfig::new();
        config2.enable(Interned::intern("ssl"));
        let result2 = convert_deps(
            &entries,
            "test/pkg",
            &config2,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert!(
            result2.requirements.is_empty(),
            "!ssl? with ssl enabled should skip deps"
        );
    }

    #[test]
    fn convert_exactly_one_of_creates_choice() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("^^ ( dev-libs/a dev-libs/b )").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.virtual_choices.len(), 1);
        assert_eq!(result.virtual_choices[0].versions.len(), 2);
    }

    #[test]
    fn convert_empty_input() {
        let config = UseConfig::new();
        let result = convert_deps(
            &[],
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert!(result.requirements.is_empty());
        assert!(result.blockers.is_empty());
        assert!(result.virtual_choices.is_empty());
    }

    #[test]
    fn convert_repo_constraint_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl::gentoo").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.repo_constraints.len(), 1);
        assert_eq!(result.repo_constraints[0].repo.as_str(), "gentoo");
    }

    #[test]
    fn convert_strictly_greater_atom() {
        let config = UseConfig::new();
        let entries = DepEntry::parse(">dev-libs/openssl-3.0.0").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert!(
            !result.requirements[0]
                .1
                .contains(&Version::parse("3.0.0").unwrap())
        );
        assert!(
            result.requirements[0]
                .1
                .contains(&Version::parse("3.0.1").unwrap())
        );
    }

    #[test]
    fn convert_less_or_equal_atom() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("<=dev-libs/openssl-3.0.0").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.requirements.len(), 1);
        assert!(
            result.requirements[0]
                .1
                .contains(&Version::parse("3.0.0").unwrap())
        );
        assert!(
            result.requirements[0]
                .1
                .contains(&Version::parse("2.9.9").unwrap())
        );
        assert!(
            !result.requirements[0]
                .1
                .contains(&Version::parse("3.0.1").unwrap())
        );
    }

    #[test]
    fn convert_bare_slot_operator_equals_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl:=").unwrap();
        let openssl_cpn = portage_atom::Cpn::parse("dev-libs/openssl").unwrap();
        let mut slots = HashMap::new();
        slots.insert(
            openssl_cpn,
            vec![(
                Interned::intern("0"),
                PortagePackage::slotted(openssl_cpn, Interned::intern("0")),
            )],
        );
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &slots,
            &empty_iuse_defaults(),
        );
        assert_eq!(result.slot_operator_deps.len(), 1);
        assert_eq!(result.slot_operator_deps[0].operator, SlotOperator::Equal);
        assert!(result.slot_operator_deps[0].slot.is_none());
    }

    #[test]
    fn convert_named_slot_operator_equals_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl:0=").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert_eq!(result.slot_operator_deps.len(), 1);
        assert_eq!(result.slot_operator_deps[0].operator, SlotOperator::Equal);
        assert_eq!(
            result.slot_operator_deps[0].slot,
            Some(Interned::intern("0"))
        );
    }

    #[test]
    fn convert_slot_star_not_collected() {
        let config = UseConfig::new();
        let entries = DepEntry::parse("dev-libs/openssl:*").unwrap();
        let result = convert_deps(
            &entries,
            "test/pkg",
            &config,
            &empty_slots(),
            &empty_iuse_defaults(),
        );
        assert!(
            result.slot_operator_deps.is_empty(),
            ":* should not produce slot-operator dep"
        );
    }
}
