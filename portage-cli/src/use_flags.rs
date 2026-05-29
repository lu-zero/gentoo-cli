use std::path::Path;

use portage_repo::{DEFAULT_MAKE_CONF, LEGACY_MAKE_CONF, MakeConf};

use crate::error::{Error, Result};

/// Add and/or remove USE flags from make.conf, then show the result.
///
/// With no flags to add or remove, just prints the current USE value.
pub fn run(add: &[String], remove: &[String], make_conf: Option<&Path>) -> Result<()> {
    let path = resolve_path(make_conf)?;

    if add.is_empty() && remove.is_empty() {
        return show(&path);
    }

    let mut mc = MakeConf::load(&path)
        .map_err(|e| Error::Other(format!("reading {}: {e}", path.display())))?;

    let current = mc.get("USE").unwrap_or("").to_string();
    let mut flags: Vec<String> = current.split_whitespace().map(str::to_string).collect();

    for flag in remove {
        let flag = flag.trim_start_matches('+');
        flags.retain(|f| f != flag && f != &format!("+{flag}"));
    }

    for flag in add {
        let flag = flag.trim_start_matches('+');
        // Remove any existing occurrence (including -flag negation) first.
        flags.retain(|f| {
            f != flag && f != &format!("+{flag}") && f != &format!("-{flag}")
        });
        flags.push(flag.to_string());
    }

    let new_use = flags.join(" ");
    mc.set("USE", &new_use);
    mc.save(&path)
        .map_err(|e| Error::Other(format!("writing {}: {e}", path.display())))?;

    println!("USE=\"{}\"", new_use);
    Ok(())
}

fn show(path: &Path) -> Result<()> {
    let mc = MakeConf::load(path)
        .map_err(|e| Error::Other(format!("reading {}: {e}", path.display())))?;

    match mc.get("USE") {
        Some(val) => println!("USE=\"{}\"", val),
        None => println!("USE not set in {}", path.display()),
    }
    Ok(())
}

fn resolve_path(override_path: Option<&Path>) -> Result<std::path::PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.to_owned());
    }
    for candidate in [DEFAULT_MAKE_CONF, LEGACY_MAKE_CONF] {
        let p = Path::new(candidate);
        if p.exists() {
            return Ok(p.to_owned());
        }
    }
    Err(Error::Other(format!(
        "no make.conf found at {} or {}",
        DEFAULT_MAKE_CONF, LEGACY_MAKE_CONF
    )))
}
