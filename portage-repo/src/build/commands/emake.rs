use brush_core::builtins;
use clap::Parser;

// ── P2 build helpers ──────────────────────────────────────────────────────────

/// `emake [args...]`  (PMS 12.3.2)
///
/// Runs `${MAKE:-make} ${MAKEOPTS} ${EXTRA_EMAKE} [args...]`.
#[derive(Parser)]
pub(crate) struct EmakeCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

impl builtins::Command for EmakeCommand {
    type State = ();
    type SharedState = ();
    type Error = brush_core::Error;

    async fn execute<SE: brush_core::ShellExtensions>(
        &self,
        context: brush_core::ExecutionContext<'_, SE>,
    ) -> Result<brush_core::ExecutionResult, Self::Error> {
        let shell = context.shell;
        let make = shell
            .env_str("MAKE")
            .map(|s| s.into_owned())
            .unwrap_or_else(|| "make".to_string());
        let makeopts_str = shell
            .env_str("MAKEOPTS")
            .map(|s| s.into_owned())
            .unwrap_or_default();
        let extra_str = shell
            .env_str("EXTRA_EMAKE")
            .map(|s| s.into_owned())
            .unwrap_or_default();
        let makeopts: Vec<String> = makeopts_str
            .split_whitespace()
            .map(|s| s.to_owned())
            .collect();
        let extra: Vec<String> = extra_str.split_whitespace().map(|s| s.to_owned()).collect();
        let args = self.args.clone();
        let cwd = shell.working_dir().to_path_buf();

        let exit = tokio::task::spawn_blocking(move || {
            std::process::Command::new(&make)
                .current_dir(&cwd)
                .args(&makeopts)
                .args(&extra)
                .args(&args)
                .status()
                .map(|s| s.code().unwrap_or(1) as u8)
                .unwrap_or(127)
        })
        .await
        .unwrap_or(127);

        Ok(brush_core::ExecutionResult::new(exit))
    }
}
