//! Shell-dependent profile operations.
//!
//! Methods that source bash files (make.defaults) or read shell state (USE
//! flags) cannot live in `repo/profile.rs` without creating an upward
//! dependency from the repo module into the build module.  They live here
//! instead and are added to `Profile` / `ProfileStack` via second impl blocks.

use std::collections::HashSet;

use super::shell::EbuildShell;
use crate::error::Result;
use crate::repo::profile::{Profile, ProfileStack};

impl Profile {
    /// Source this profile's `make.defaults` into the shell, if present.
    ///
    /// See [PMS 5.2.4](https://projects.gentoo.org/pms/9/pms.html#makedefaults).
    pub async fn make_defaults(&self, shell: &mut EbuildShell) -> Result<()> {
        let path = self.path().join("make.defaults");
        if path.is_file() {
            shell.source_make_defaults(&path).await?;
        }
        Ok(())
    }
}

impl ProfileStack {
    /// Source `make.defaults` from every profile in stack order.
    ///
    /// Variables set by ancestor profiles are visible to child profiles.
    ///
    /// See [PMS 5.2.4](https://projects.gentoo.org/pms/9/pms.html#make-defaults).
    pub async fn make_defaults(&self, shell: &mut EbuildShell) -> Result<()> {
        for p in self.profiles() {
            p.make_defaults(shell).await?;
        }
        Ok(())
    }

    /// Configure a shell with this profile stack's effective USE flags.
    ///
    /// See [`configure_shell`] for the full description.
    pub async fn configure_shell(
        &self,
        shell: &mut EbuildShell,
        extra_confs: &[&std::path::Path],
    ) -> Result<()> {
        configure_shell(shell, self, extra_confs).await
    }
}

/// Configure a shell with the effective USE flags from a profile stack.
///
/// `extra_confs` is a list of additional shell scripts (e.g.
/// `/etc/portage/make.conf`) sourced **after** the `make.defaults` chain but
/// **before** `use.force`/`use.mask` are applied.
///
/// Computation order:
/// 1. Profile `make.defaults` (ancestor → leaf)
/// 2. Each `extra_confs` script in order
/// 3. `USE_EXPAND_UNPREFIXED` values expanded directly into USE
/// 4. `USE_EXPAND` values expanded with lowercase prefix
/// 5. Profile `use.force` — unconditional add
/// 6. Profile `use.mask` — unconditional remove
///
/// See [PMS 5.2](https://projects.gentoo.org/pms/9/pms.html#profiles).
pub async fn configure_shell(
    shell: &mut EbuildShell,
    stack: &ProfileStack,
    extra_confs: &[&std::path::Path],
) -> Result<()> {
    stack.make_defaults(shell).await?;

    for conf in extra_confs {
        shell.source_make_defaults(conf).await?;
    }

    let mut flags = collect_use_flags(shell);

    let flag_set: HashSet<String> = flags.iter().cloned().collect();
    for flag in stack.use_force()? {
        if !flag_set.contains(&flag) {
            flags.push(flag);
        }
    }

    let mask: HashSet<String> = stack.use_mask()?.into_iter().collect();
    flags.retain(|f| !mask.contains(f.as_str()));

    let flag_refs: Vec<&str> = flags.iter().map(String::as_str).collect();
    shell.set_use_flags(&flag_refs)
}

fn collect_use_flags(shell: &EbuildShell) -> Vec<String> {
    let use_str = shell.get_var("USE").unwrap_or_default();
    let mut flags: Vec<String> = Vec::new();

    for token in use_str.split_whitespace() {
        if let Some(name) = token.strip_prefix('-') {
            flags.retain(|f| f != name);
        } else if !flags.iter().any(|f| f == token) {
            flags.push(token.to_string());
        }
    }

    let unprefixed = shell.get_var("USE_EXPAND_UNPREFIXED").unwrap_or_default();
    for var in unprefixed.split_whitespace() {
        let val = shell.get_var(var).unwrap_or_default();
        for v in val.split_whitespace() {
            if !flags.iter().any(|f| f == v) {
                flags.push(v.to_string());
            }
        }
    }

    let use_expand = shell.get_var("USE_EXPAND").unwrap_or_default();
    for var in use_expand.split_whitespace() {
        let val = shell.get_var(var).unwrap_or_default();
        let prefix = var.to_lowercase();
        for v in val.split_whitespace() {
            let flag = format!("{prefix}_{v}");
            if !flags.iter().any(|f| f == &flag) {
                flags.push(flag);
            }
        }
    }

    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::profile::ProfileStack;
    use crate::repo::repository::Repository;
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn make_profile(dir: &TempDir, name: &str, parents: &[&str]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        let parent_content = parents
            .iter()
            .map(|p| format!("{}\n", p))
            .collect::<String>();
        if !parent_content.is_empty() {
            std::fs::write(path.join("parent"), &parent_content).unwrap();
        }
        path
    }

    fn make_test_repo(dir: &TempDir) -> Repository {
        std::fs::create_dir_all(dir.path().join("metadata")).unwrap();
        std::fs::write(dir.path().join("metadata").join("layout.conf"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("profiles")).unwrap();
        Repository::open(dir.path()).unwrap()
    }

    #[tokio::test]
    async fn configure_shell_applies_make_defaults_use() {
        let dir = tempfile::tempdir().unwrap();
        let repo = make_test_repo(&dir);
        let profile = make_profile(&dir, "test", &[]);
        std::fs::write(profile.join("make.defaults"), "USE=\"foo bar\"\n").unwrap();

        let stack = ProfileStack::build(profile).unwrap();
        let mut shell = repo.shell().await.unwrap();
        configure_shell(&mut shell, &stack, &[]).await.unwrap();

        let use_val = shell.get_var("USE").unwrap_or_default();
        let flags: HashSet<&str> = use_val.split_whitespace().collect();
        assert!(flags.contains("foo"), "foo from make.defaults");
        assert!(flags.contains("bar"), "bar from make.defaults");
    }

    #[tokio::test]
    async fn configure_shell_applies_use_force_and_mask() {
        let dir = tempfile::tempdir().unwrap();
        let repo = make_test_repo(&dir);
        let profile = make_profile(&dir, "test", &[]);
        std::fs::write(profile.join("make.defaults"), "USE=\"foo bar\"\n").unwrap();
        std::fs::write(profile.join("use.force"), "forced\n").unwrap();
        std::fs::write(profile.join("use.mask"), "bar\n").unwrap();

        let stack = ProfileStack::build(profile).unwrap();
        let mut shell = repo.shell().await.unwrap();
        configure_shell(&mut shell, &stack, &[]).await.unwrap();

        let use_val = shell.get_var("USE").unwrap_or_default();
        let flags: HashSet<&str> = use_val.split_whitespace().collect();
        assert!(flags.contains("foo"));
        assert!(!flags.contains("bar"), "bar should be masked");
        assert!(
            flags.contains("forced"),
            "forced should be added by use.force"
        );
    }

    #[tokio::test]
    async fn configure_shell_expands_use_expand() {
        let dir = tempfile::tempdir().unwrap();
        let repo = make_test_repo(&dir);
        let profile = make_profile(&dir, "test", &[]);
        std::fs::write(
            profile.join("make.defaults"),
            "USE_EXPAND=\"CPU_FLAGS_X86\"\nCPU_FLAGS_X86=\"sse2 mmx\"\n",
        )
        .unwrap();

        let stack = ProfileStack::build(profile).unwrap();
        let mut shell = repo.shell().await.unwrap();
        configure_shell(&mut shell, &stack, &[]).await.unwrap();

        let use_val = shell.get_var("USE").unwrap_or_default();
        let flags: HashSet<&str> = use_val.split_whitespace().collect();
        assert!(flags.contains("cpu_flags_x86_sse2"), "sse2 expanded");
        assert!(flags.contains("cpu_flags_x86_mmx"), "mmx expanded");
    }

    #[tokio::test]
    async fn configure_shell_expands_use_expand_unprefixed() {
        let dir = tempfile::tempdir().unwrap();
        let repo = make_test_repo(&dir);
        let profile = make_profile(&dir, "test", &[]);
        std::fs::write(
            profile.join("make.defaults"),
            "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"amd64\"\n",
        )
        .unwrap();

        let stack = ProfileStack::build(profile).unwrap();
        let mut shell = repo.shell().await.unwrap();
        configure_shell(&mut shell, &stack, &[]).await.unwrap();

        let use_val = shell.get_var("USE").unwrap_or_default();
        let flags: HashSet<&str> = use_val.split_whitespace().collect();
        assert!(flags.contains("amd64"), "ARCH added unprefixed");
    }

    #[tokio::test]
    async fn configure_shell_make_conf_applied_before_force_mask() {
        let dir = tempfile::tempdir().unwrap();
        let repo = make_test_repo(&dir);
        let profile = make_profile(&dir, "test", &[]);
        std::fs::write(profile.join("make.defaults"), "USE=\"foo bar forced\"\n").unwrap();
        std::fs::write(profile.join("use.force"), "forced\n").unwrap();
        std::fs::write(profile.join("use.mask"), "bar\n").unwrap();

        let make_conf = dir.path().join("make.conf");
        std::fs::write(&make_conf, "USE=\"${USE} user_flag -forced\"\n").unwrap();

        let stack = ProfileStack::build(profile).unwrap();
        let mut shell = repo.shell().await.unwrap();
        configure_shell(&mut shell, &stack, &[make_conf.as_path()])
            .await
            .unwrap();

        let use_val = shell.get_var("USE").unwrap_or_default();
        let flags: HashSet<&str> = use_val.split_whitespace().collect();
        assert!(flags.contains("foo"));
        assert!(flags.contains("user_flag"), "user_flag from make.conf");
        assert!(!flags.contains("bar"), "bar masked by use.mask");
        assert!(
            flags.contains("forced"),
            "use.force overrides make.conf removal"
        );
    }
}
