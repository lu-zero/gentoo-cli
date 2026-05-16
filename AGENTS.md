# Project Conventions

## Build Commands

```bash
cargo build                        # Build the em binary
cargo test                         # Run all tests
cargo clippy -- -D warnings        # Lint — must be warning-free
cargo fmt --check                  # Format check — must pass
```

## Architecture

- Binary crate producing the `em` command
- CLI built with [clap](https://crates.io/crates/clap) derive macros
- Commands are subcommands of the top-level `Cli` struct
- Business logic delegated to library crates (`portage-atom`; `portage-repo` and `portage-atom-resolvo` to be added later)
- Keep `main.rs` thin; extract modules as complexity grows
- The [architecture](./ARCHITECTURE.md) contains more details when needed and can be updated as changes are made.

## Dependencies

- `portage-atom` — PMS atom parsing (Cpn, Cpv, Dep, etc.)
- `clap` — CLI argument parsing
- `tokio` — async runtime (prepared for future portage-repo integration)
- `thiserror` — error derive macros

> **Note**: `portage-atom-resolvo` and `portage-repo` are planned but not yet
> depended on (portage-repo is not yet published to crates.io). Add them back
> when implementing real emerge/resolve logic.

## Coding Style

- `rustfmt` — all code must be formatted
- No dead code, no unused dependencies
- Doc comments on all public types and functions
- Tests live in a `#[cfg(test)] mod tests` block

## Commits

[Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — new functionality
- `fix:` — bug fix
- `refactor:` — code restructuring without behaviour change
- `docs:` — documentation only
- `test:` — adding or updating tests
- `ci:` — CI/CD changes
- `chore:` — maintenance (dependencies, tooling)

## MSRV

Minimum Supported Rust Version is **1.85** (edition 2024).

## Slop Warning

This codebase was largely AI-generated. Be skeptical of existing code — it may
contain bugs or surprising behaviour. Do not assume existing patterns are
correct.
