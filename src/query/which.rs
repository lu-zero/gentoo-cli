use std::cmp::Ordering;
use std::path::Path;

use portage_atom::{Cpv, Dep, Operator};
use portage_repo::Repository;

use crate::error::{Error, Result};

pub fn run(repo_path: &Path, atoms: &[String]) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(|e| Error::Other(e.to_string()))?;

    let ebuilds: Vec<_> = repo
        .ebuilds()
        .map_err(|e| Error::Other(e.to_string()))?
        .into_iter()
        .collect();

    for raw in atoms {
        let dep = Dep::parse(raw).map_err(|e| Error::Other(format!("bad atom '{raw}': {e}")))?;

        let best = ebuilds
            .iter()
            .filter(|e| dep_matches_cpv(&dep, e.cpv()))
            .max_by(|a, b| a.cpv().version.cmp(&b.cpv().version));

        match best {
            Some(e) => println!("{}", e.path()),
            None => eprintln!("em: no ebuild found for '{raw}'"),
        }
    }
    Ok(())
}

pub fn dep_matches_cpv(dep: &Dep, cpv: &Cpv) -> bool {
    if dep.cpn != cpv.cpn {
        return false;
    }
    match (&dep.version, &dep.op) {
        (None, _) => true,
        (Some(v), Some(Operator::Equal)) if dep.glob => cpv.version.glob_matches(v),
        (Some(v), Some(op)) => {
            let ord = cpv.version.cmp(v);
            match op {
                Operator::Less => ord == Ordering::Less,
                Operator::LessOrEqual => ord != Ordering::Greater,
                Operator::Equal => ord == Ordering::Equal,
                Operator::Approximate => {
                    let mut base = v.clone();
                    base.revision = Default::default();
                    let mut cv = cpv.version.clone();
                    cv.revision = Default::default();
                    cv == base
                }
                Operator::GreaterOrEqual => ord != Ordering::Less,
                Operator::Greater => ord == Ordering::Greater,
            }
        }
        _ => true,
    }
}
