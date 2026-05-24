use std::collections::{BinaryHeap, HashMap};

use portage_atom::{Cpn, Version};
use pubgrub::VersionSet;

use crate::package::PortagePackage;
use crate::provider::PortageDependencyProvider;

/// Dependency class label for an edge in the dependency graph.
///
/// Corresponds to the five dependency variables defined by PMS 8.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepClass {
    /// DEPEND — build-time dependencies.
    Depend,
    /// RDEPEND — run-time dependencies.
    Rdepend,
    /// BDEPEND — build-host dependencies (EAPI 7+).
    Bdepend,
    /// PDEPEND — post-merge dependencies.
    Pdepend,
    /// IDEPEND — install-time dependencies (EAPI 8+).
    Idepend,
}

/// A labeled edge in the dependency graph: (from_pkg, from_version) depends on
/// (to_pkg, to_version) via the given class.
#[derive(Debug, Clone)]
pub struct DepEdge {
    /// The package that declares the dependency.
    pub from: (PortagePackage, Version),
    /// The package that is depended upon.
    pub to: (PortagePackage, Version),
    /// Which dependency class this edge belongs to.
    pub class: DepClass,
}

impl PortageDependencyProvider {
    /// Build the labeled dependency graph from a solution.
    ///
    /// Returns edges labeled with the dependency class (DEPEND, RDEPEND, etc.).
    /// Only edges where both endpoints are in the solution are included.
    pub fn dependency_graph(
        &self,
        solution: &pubgrub::SelectedDependencies<PortagePackage, Version>,
    ) -> Vec<DepEdge> {
        let mut edges = Vec::new();
        let classes = [
            DepClass::Depend,
            DepClass::Rdepend,
            DepClass::Bdepend,
            DepClass::Pdepend,
            DepClass::Idepend,
        ];

        // Index solution by CPN so dependency lookups are O(1) instead of O(n).
        let mut by_cpn: HashMap<&Cpn, Vec<(&PortagePackage, &Version)>> = HashMap::new();
        for (sol_pkg, sol_ver) in solution.iter() {
            by_cpn
                .entry(sol_pkg.cpn())
                .or_default()
                .push((sol_pkg, sol_ver));
        }

        for (pkg, version) in solution.iter() {
            let Some(data) = self.packages.get(pkg) else {
                continue;
            };
            let Some(vd) = data.versions.get(version) else {
                continue;
            };

            for (class_idx, &class) in classes.iter().enumerate() {
                for (dep_pkg, dep_vs) in &vd.by_class[class_idx] {
                    if let Some(candidates) = by_cpn.get(dep_pkg.cpn()) {
                        for &(sol_pkg, sol_ver) in candidates {
                            if dep_vs.contains(sol_ver) {
                                edges.push(DepEdge {
                                    from: (pkg.clone(), version.clone()),
                                    to: (sol_pkg.clone(), sol_ver.clone()),
                                    class,
                                });
                            }
                        }
                    }
                }
            }
        }

        edges
    }

    /// Compute an installation order from a solution.
    ///
    /// Returns packages in topological order respecting dependency classes:
    /// DEPEND and BDEPEND must be built before the package, RDEPEND and
    /// PDEPEND are satisfied at runtime. IDEPEND must be present at install
    /// time.
    ///
    /// The ordering considers build-time edges (DEPEND, BDEPEND) as hard
    /// constraints. Packages with no build-time dependencies between them
    /// may appear in any order.
    pub fn install_order(
        &self,
        solution: &pubgrub::SelectedDependencies<PortagePackage, Version>,
    ) -> Vec<(PortagePackage, Version)> {
        let graph = self.dependency_graph(solution);

        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        let mut key_of: HashMap<String, (PortagePackage, Version)> = HashMap::new();

        for (pkg, ver) in solution.iter() {
            let key = format!("{}-{}", pkg, ver);
            in_degree.entry(key.clone()).or_insert(0);
            adj.entry(key.clone()).or_default();
            key_of.insert(key, (pkg.clone(), ver.clone()));
        }

        for edge in &graph {
            match edge.class {
                DepClass::Depend | DepClass::Bdepend => {
                    let dep_key = format!("{}-{}", edge.to.0, edge.to.1);
                    let from_key = format!("{}-{}", edge.from.0, edge.from.1);
                    adj.entry(dep_key).or_default().push(from_key.clone());
                    *in_degree.entry(from_key).or_insert(0) += 1;
                }
                _ => {}
            }
        }

        // BinaryHeap is a max-heap: pop() yields the lexicographically largest
        // key first, giving deterministic output without O(n) Vec::insert.
        let mut queue: BinaryHeap<String> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(k, _)| k.clone())
            .collect();

        let mut result = Vec::new();
        while let Some(key) = queue.pop() {
            if let Some((pkg, ver)) = key_of.remove(&key) {
                result.push((pkg, ver));
            }
            if let Some(neighbors) = adj.get(&key) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(neighbor.clone());
                        }
                    }
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{InMemoryRepository, PackageDeps};
    use crate::use_config::UseConfig;
    use crate::version_set::PortageVersionSet;
    use portage_atom::{Cpn, Dep, DepEntry};

    #[test]
    fn install_order_and_dependency_graph_work() {
        let mut repo = InMemoryRepository::new();
        let empty = || PackageDeps {
            depend: vec![],
            rdepend: vec![],
            bdepend: vec![],
            pdepend: vec![],
            idepend: vec![],
        };

        repo.add_version(
            portage_atom::Cpv::parse("app-misc/top-1.0").unwrap(),
            None,
            None,
            PackageDeps {
                depend: vec![DepEntry::Atom(Dep::parse("dev-libs/bottom-1.0").unwrap())],
                rdepend: vec![],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/bottom-1.0").unwrap(),
            None,
            None,
            empty(),
        );

        let mut provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let top = PortagePackage::unslotted(Cpn::parse("app-misc/top").unwrap());

        let solution = provider
            .resolve_targets(vec![(top, PortageVersionSet::any())])
            .unwrap();

        let edges = provider.dependency_graph(&solution);
        assert!(
            edges.iter().any(|e| e.class == DepClass::Depend),
            "should have a DEPEND edge"
        );

        let order = provider.install_order(&solution);
        let names: Vec<&str> = order
            .iter()
            .map(|(p, _)| p.cpn().package.as_str())
            .collect();
        let bottom_pos = names.iter().position(|&n| n == "bottom").unwrap();
        let top_pos = names.iter().position(|&n| n == "top").unwrap();
        assert!(
            bottom_pos < top_pos,
            "bottom must come before top in install order, got: {:?}",
            names
        );
    }

    #[test]
    fn dependency_graph_returns_labeled_edges() {
        let mut repo = InMemoryRepository::new();
        let empty = || PackageDeps {
            depend: vec![],
            rdepend: vec![],
            bdepend: vec![],
            pdepend: vec![],
            idepend: vec![],
        };

        repo.add_version(
            portage_atom::Cpv::parse("app-misc/app-1.0").unwrap(),
            None,
            None,
            PackageDeps {
                depend: vec![DepEntry::Atom(Dep::parse("dev-libs/lib-1.0").unwrap())],
                rdepend: vec![DepEntry::Atom(Dep::parse("dev-libs/runtime-1.0").unwrap())],
                bdepend: vec![],
                pdepend: vec![],
                idepend: vec![],
            },
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/lib-1.0").unwrap(),
            None,
            None,
            empty(),
        );
        repo.add_version(
            portage_atom::Cpv::parse("dev-libs/runtime-1.0").unwrap(),
            None,
            None,
            empty(),
        );

        let mut provider = PortageDependencyProvider::new(repo, UseConfig::new(), &[]);
        let app = PortagePackage::unslotted(Cpn::parse("app-misc/app").unwrap());

        let solution = provider
            .resolve_targets(vec![(app, PortageVersionSet::any())])
            .unwrap();
        let edges = provider.dependency_graph(&solution);

        let dep_classes: Vec<_> = edges.iter().map(|e| e.class).collect();
        assert!(
            dep_classes.contains(&DepClass::Depend),
            "should have DEPEND edge"
        );
        assert!(
            dep_classes.contains(&DepClass::Rdepend),
            "should have RDEPEND edge"
        );
    }
}
