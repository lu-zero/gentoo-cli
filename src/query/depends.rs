use std::collections::BTreeSet;
use std::path::Path;

use portage_atom::{Dep, DepEntry};
use portage_repo::Repository;

use crate::error::{Error, Result};

pub fn run(repo_path: &Path, atoms: &[String]) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(|e| Error::Other(e.to_string()))?;

    for raw in atoms {
        let target =
            Dep::parse(raw).map_err(|e| Error::Other(format!("bad atom '{raw}': {e}")))?;

        let mut matches: BTreeSet<String> = BTreeSet::new();

        for ebuild in repo.ebuilds().map_err(|e| Error::Other(e.to_string()))? {
            let cpv = ebuild.cpv();
            let Ok(entry) = repo.cache_entry(cpv) else {
                continue;
            };
            let m = &entry.metadata;
            let dep_trees = [&m.depend, &m.rdepend, &m.bdepend, &m.pdepend, &m.idepend];

            if dep_trees.iter().any(|tree| tree_contains(&target, tree)) {
                matches.insert(cpv.cpn.to_string());
            }
        }

        if atoms.len() > 1 {
            println!("[{raw}]");
        }
        for cpn in &matches {
            println!("{cpn}");
        }
    }
    Ok(())
}

/// Recursively check whether any atom in `entries` matches `target` by CPN.
fn tree_contains(target: &Dep, entries: &[DepEntry]) -> bool {
    entries.iter().any(|e| entry_matches(target, e))
}

fn entry_matches(target: &Dep, entry: &DepEntry) -> bool {
    match entry {
        DepEntry::Atom(dep) => dep.cpn == target.cpn,
        DepEntry::UseConditional { children, .. }
        | DepEntry::AllOf(children)
        | DepEntry::AnyOf(children)
        | DepEntry::ExactlyOneOf(children)
        | DepEntry::AtMostOneOf(children) => tree_contains(target, children),
    }
}
