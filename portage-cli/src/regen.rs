use std::path::PathBuf;

use portage_repo::{RegenOpts, Repository, SourceOpts, regen_cache};

use crate::error::{Error, Result};

pub async fn run(
    repo_path: &str,
    repos_dir: Option<&str>,
    output: Option<PathBuf>,
    jobs: Option<usize>,
    dedup: bool,
) -> Result<()> {
    let (repo, masters) = if let Some(dir) = repos_dir {
        Repository::open_with_masters(repo_path, dir)
            .map_err(|e| Error::Other(format!("open repo: {e}")))?
    } else {
        let repo =
            Repository::open(repo_path).map_err(|e| Error::Other(format!("open repo: {e}")))?;
        (repo, vec![])
    };

    let ebuilds: Vec<_> = repo
        .ebuilds()
        .map_err(|e| Error::Other(format!("list ebuilds: {e}")))?
        .into_iter()
        .collect();

    let out = output.unwrap_or_else(|| repo.path().join("metadata/md5-cache").into_std_path_buf());

    let opts = RegenOpts {
        source: SourceOpts { jobs, dedup },
        output_dir: Some(out),
    };

    let stats = regen_cache(&repo, &masters, ebuilds, &opts, |done, total| {
        eprint!("\r[{done}/{total}]");
    })
    .await
    .map_err(|e| Error::Other(format!("regen: {e}")))?;

    eprintln!();
    if stats.errors > 0 {
        return Err(Error::Other(format!("{} sourcing errors", stats.errors)));
    }
    Ok(())
}
