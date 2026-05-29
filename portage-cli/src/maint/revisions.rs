use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{Error, Result};

const REPO_REVISIONS: &str = "var/lib/portage/repo_revisions";

/// Purge the repo_revisions file, which tracks git commit hashes from previous
/// syncs.  Matches `emaint revisions` behaviour.
///
/// Without `repos`, the entire file is deleted.  With `repos`, only those
/// specific repo entries are removed from the JSON object.
pub fn run(repos: &[String], root: Option<&Utf8Path>) -> Result<()> {
    let path = revisions_path(root);

    if !path.exists() {
        println!("No repo_revisions file found at {path}.");
        return Ok(());
    }

    if repos.is_empty() {
        std::fs::remove_file(&path).map_err(|e| {
            Error::Other(format!("removing {path}: {e}"))
        })?;
        println!("Purged {path}.");
    } else {
        purge_repos(&path, repos)?;
    }

    Ok(())
}

fn purge_repos(path: &Utf8Path, repos: &[String]) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::Other(format!("reading {path}: {e}")))?;

    let mut map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&content)
            .map_err(|e| Error::Other(format!("parsing {path}: {e}")))?;

    let mut removed = Vec::new();
    for repo in repos {
        if map.remove(repo).is_some() {
            removed.push(repo.as_str());
        } else {
            eprintln!("warning: repo '{repo}' not found in {path}");
        }
    }

    if removed.is_empty() {
        return Ok(());
    }

    if map.is_empty() {
        std::fs::remove_file(path)
            .map_err(|e| Error::Other(format!("removing {path}: {e}")))?;
        println!("Purged {path} (empty after removing {}).", removed.join(", "));
    } else {
        let out = serde_json::to_string(&map)
            .map_err(|e| Error::Other(format!("serialising: {e}")))?;
        std::fs::write(path, out)
            .map_err(|e| Error::Other(format!("writing {path}: {e}")))?;
        println!("Removed {} from {path}.", removed.join(", "));
    }

    Ok(())
}

fn revisions_path(root: Option<&Utf8Path>) -> Utf8PathBuf {
    match root {
        Some(r) => r.join(REPO_REVISIONS),
        None => Utf8PathBuf::from("/").join(REPO_REVISIONS),
    }
}
