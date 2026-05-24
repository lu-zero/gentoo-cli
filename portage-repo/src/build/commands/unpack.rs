use brush_core::builtins;
use clap::Parser;

// ── P4 unpack ─────────────────────────────────────────────────────────────────

/// `unpack <archive>...`  (PMS 12.3.11)
///
/// Extracts one or more archives into the current directory (`$S`).
/// Archives named without a path separator are looked up in `$DISTDIR`.
/// Dispatches to the appropriate extraction tool based on file extension.
/// Extension matching is case-insensitive for EAPI ≤ 5, case-sensitive for EAPI ≥ 6.
///
/// See [PMS 12.3.11](https://projects.gentoo.org/pms/9/pms.html#unpack).
#[derive(Parser)]
pub(crate) struct UnpackCommand {
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    archives: Vec<String>,
}

impl builtins::Command for UnpackCommand {
    type State = ();
    type SharedState = ();
    type Error = brush_core::Error;

    async fn execute<SE: brush_core::ShellExtensions>(
        &self,
        context: brush_core::ExecutionContext<'_, SE>,
    ) -> Result<brush_core::ExecutionResult, Self::Error> {
        let shell = context.shell;

        let get = |var: &str| {
            shell
                .env_str(var)
                .map(|s| s.into_owned())
                .unwrap_or_default()
        };
        let distdir = {
            let s = get("DISTDIR");
            if s.is_empty() {
                "/var/cache/distfiles".to_string()
            } else {
                s
            }
        };
        let eapi: u32 = get("EAPI").parse().unwrap_or(0);
        let cwd = shell.working_dir().to_path_buf();
        let archives = self.archives.clone();

        let exit = tokio::task::spawn_blocking(move || -> u8 {
            for archive in &archives {
                let src_path = if archive.starts_with('/') || archive.starts_with("./") {
                    if archive.starts_with('/') && eapi < 6 {
                        eprintln!("die: unpack: absolute paths not supported in EAPI {eapi}");
                        return 1;
                    }
                    std::path::PathBuf::from(archive)
                } else {
                    std::path::PathBuf::from(&distdir).join(archive)
                };

                if !src_path.exists() {
                    eprintln!("die: unpack: {} not found", src_path.display());
                    return 1;
                }

                eprintln!(">>> Unpacking {} to {}", archive, cwd.display());

                match unpack_archive(&src_path, &cwd, eapi) {
                    Ok(0) => {}
                    Ok(code) => {
                        eprintln!("die: unpack: {} failed with exit code {}", archive, code);
                        return 1;
                    }
                    Err(e) => {
                        eprintln!("die: unpack: {}", e);
                        return 1;
                    }
                }
            }
            0
        })
        .await
        .unwrap_or(1);

        Ok(brush_core::ExecutionResult::new(exit))
    }
}

/// Dispatch extraction of `src` into `cwd` based on file extension.
fn unpack_archive(src: &std::path::Path, cwd: &std::path::Path, eapi: u32) -> Result<u8, String> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let case_sensitive = eapi >= 6;
    let ext = |suffix: &str| -> bool {
        if case_sensitive {
            name.ends_with(suffix)
        } else {
            name.to_lowercase().ends_with(&suffix.to_lowercase())
        }
    };

    let src_s = src.to_string_lossy().into_owned();
    let stem = src.file_stem().unwrap_or_default();

    // Check .tar.* compound extensions before single-extension forms.
    if ext(".tar.gz") || ext(".tgz") || ext(".tar.Z") {
        unpack_cmd("tar", &["xzf", &src_s], cwd)
    } else if ext(".tar.bz2") || ext(".tbz2") || ext(".tbz") {
        unpack_cmd("tar", &["xjf", &src_s], cwd)
    } else if ext(".tar.xz") || ext(".txz") {
        if eapi < 3 {
            return Err(format!(".tar.xz not supported in EAPI {eapi}"));
        }
        unpack_cmd("tar", &["xJf", &src_s], cwd)
    } else if ext(".tar.zst") {
        unpack_cmd("tar", &["--zstd", "-xf", &src_s], cwd)
    } else if ext(".tar.lz") {
        unpack_cmd("tar", &["--lzip", "-xf", &src_s], cwd)
    } else if ext(".tar.lzma") {
        unpack_cmd("tar", &["--lzma", "-xf", &src_s], cwd)
    } else if ext(".zip") {
        unpack_cmd("unzip", &["-qo", &src_s], cwd)
    } else if ext(".gz") || ext(".Z") {
        unpack_piped("gzip", &["-d", "-c", &src_s], &cwd.join(stem), cwd)
    } else if ext(".bz2") {
        unpack_piped("bzip2", &["-d", "-c", &src_s], &cwd.join(stem), cwd)
    } else if ext(".xz") {
        if eapi < 3 {
            return Err(format!(".xz not supported in EAPI {eapi}"));
        }
        unpack_piped("xz", &["-d", "-c", &src_s], &cwd.join(stem), cwd)
    } else if ext(".lzma") {
        unpack_piped(
            "xz",
            &["--format=lzma", "-d", "-c", &src_s],
            &cwd.join(stem),
            cwd,
        )
    } else if ext(".zst") {
        unpack_piped("zstd", &["-d", "-c", &src_s], &cwd.join(stem), cwd)
    } else if ext(".7z") {
        if eapi > 7 {
            return Err(format!(".7z not supported in EAPI {eapi}"));
        }
        unpack_cmd("7z", &["x", &src_s], cwd)
    } else if ext(".rar") {
        if eapi > 7 {
            return Err(format!(".rar not supported in EAPI {eapi}"));
        }
        unpack_cmd("unrar", &["x", &src_s], cwd)
    } else if ext(".lha") || ext(".lzh") {
        if eapi > 7 {
            return Err(format!(".lha/.lzh not supported in EAPI {eapi}"));
        }
        unpack_cmd("lha", &["xfq", &src_s], cwd)
    } else {
        Err(format!("unknown archive type: {name}"))
    }
}

fn unpack_cmd(prog: &str, args: &[&str], cwd: &std::path::Path) -> Result<u8, String> {
    std::process::Command::new(prog)
        .args(args)
        .current_dir(cwd)
        .status()
        .map(|s| s.code().unwrap_or(1) as u8)
        .map_err(|e| format!("failed to run {prog}: {e}"))
}

fn unpack_piped(
    prog: &str,
    args: &[&str],
    out: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<u8, String> {
    let f = std::fs::File::create(out)
        .map_err(|e| format!("failed to create {}: {e}", out.display()))?;
    std::process::Command::new(prog)
        .args(args)
        .current_dir(cwd)
        .stdout(f)
        .status()
        .map(|s| s.code().unwrap_or(1) as u8)
        .map_err(|e| format!("failed to run {prog}: {e}"))
}
