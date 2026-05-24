use brush_core::builtins;
use clap::Parser;

/// `econf [extra-args...]`
///
/// Runs `./configure` (or `$ECONF_SOURCE/configure`) with standard flags
/// derived from the current EAPI and portage environment variables.
#[derive(Parser)]
pub(crate) struct EconfCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

impl builtins::Command for EconfCommand {
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
        let eapi: u32 = get("EAPI").parse().unwrap_or(0);
        let econf_source = {
            let s = get("ECONF_SOURCE");
            if s.is_empty() { ".".to_string() } else { s }
        };
        let eprefix = get("EPREFIX");
        let pf = get("PF");
        let chost = get("CHOST");
        let cbuild = get("CBUILD");
        let ctarget = get("CTARGET");
        let esysroot = {
            let s = get("ESYSROOT");
            if s.is_empty() { "/".to_string() } else { s }
        };
        let extra_econf = get("EXTRA_ECONF");

        let mut env_vars: Vec<(String, String)> = Vec::new();
        for var in &[
            "CC",
            "CXX",
            "AR",
            "RANLIB",
            "NM",
            "CFLAGS",
            "CXXFLAGS",
            "CPPFLAGS",
            "LDFLAGS",
            "CONFIG_SHELL",
        ] {
            if let Some(val) = shell.env_str(var) {
                env_vars.push((var.to_string(), val.into_owned()));
            }
        }

        let user_args = self.args.clone();
        let cwd = shell.working_dir().to_path_buf();

        let exit = tokio::task::spawn_blocking(move || {
            // configure path: $ECONF_SOURCE/configure, defaulting to $S (cwd).
            let base = if econf_source == "." {
                cwd.clone()
            } else {
                std::path::PathBuf::from(&econf_source)
            };
            let configure = base.join("configure");
            if !configure.exists() {
                return 0u8;
            }

            // Probe EAPI-conditional flags from configure --help.
            let help = if eapi >= 4 {
                std::process::Command::new(&configure)
                    .arg("--help")
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let mut conf_args: Vec<String> = Vec::new();

            conf_args.push(format!("--prefix={eprefix}/usr"));
            if !cbuild.is_empty() {
                conf_args.push(format!("--build={cbuild}"));
            }
            if !chost.is_empty() {
                conf_args.push(format!("--host={chost}"));
            }
            if !ctarget.is_empty() {
                conf_args.push(format!("--target={ctarget}"));
            }
            conf_args.push(format!("--mandir={eprefix}/usr/share/man"));
            conf_args.push(format!("--infodir={eprefix}/usr/share/info"));
            conf_args.push(format!("--datadir={eprefix}/usr/share"));
            conf_args.push(format!("--sysconfdir={eprefix}/etc"));
            conf_args.push(format!("--localstatedir={eprefix}/var/lib"));

            if eapi >= 8 && help.contains("--datarootdir") {
                conf_args.push(format!("--datarootdir={eprefix}/usr/share"));
            }
            // Use word-boundary guard matching portage's pattern.
            if eapi >= 4 && contains_flag(&help, "--disable-dependency-tracking") {
                conf_args.push("--disable-dependency-tracking".to_string());
            }
            if eapi >= 5 && contains_flag(&help, "--disable-silent-rules") {
                conf_args.push("--disable-silent-rules".to_string());
            }
            if eapi >= 6 {
                if help.contains("--docdir") {
                    conf_args.push(format!("--docdir={eprefix}/usr/share/doc/{pf}"));
                }
                if help.contains("--htmldir") {
                    conf_args.push(format!("--htmldir={eprefix}/usr/share/doc/{pf}/html"));
                }
            }
            if eapi >= 7 && contains_flag(&help, "--with-sysroot") {
                conf_args.push(format!("--with-sysroot={esysroot}"));
            }
            // Portage requires both --enable-shared and --enable-static before adding
            // --disable-static, to avoid touching packages that don't support static builds.
            if eapi >= 8
                && contains_flag(&help, "--enable-shared")
                && contains_flag(&help, "--enable-static")
            {
                conf_args.push("--disable-static".to_string());
            }

            conf_args.extend(user_args);
            // EXTRA_ECONF is split on whitespace; quoted-whitespace in values is rare
            // in practice (portage eval's it, which we can't do safely here).
            conf_args.extend(extra_econf.split_whitespace().map(str::to_owned));

            let mut cmd = std::process::Command::new(&configure);
            cmd.current_dir(&cwd).args(&conf_args);
            for (k, v) in &env_vars {
                cmd.env(k, v);
            }

            cmd.status()
                .map(|s| s.code().unwrap_or(1) as u8)
                .unwrap_or(127)
        })
        .await
        .unwrap_or(127);

        Ok(brush_core::ExecutionResult::new(exit))
    }
}

/// Returns true if `flag` appears in `text` followed by a non-identifier character
/// (space, newline, `=`, end-of-string), matching portage's word-boundary guard.
/// Prevents `--disable-dependency-tracking` from matching `--disable-dependency-tracking-fast`.
fn contains_flag(text: &str, flag: &str) -> bool {
    let mut rest = text;
    while let Some(pos) = rest.find(flag) {
        let after = &rest[pos + flag.len()..];
        if after
            .chars()
            .next()
            .map_or(true, |c| !c.is_ascii_alphanumeric() && !"+_.-".contains(c))
        {
            return true;
        }
        rest = &rest[pos + 1..];
    }
    false
}
