# portage-cli

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE-MIT)
[![Build Status](https://github.com/lu-zero/portage-cli/workflows/CI/badge.svg)](https://github.com/lu-zero/portage-cli/actions?query=workflow:CI)
[![dependency status](https://deps.rs/repo/github/lu-zero/portage-cli/status.svg)](https://deps.rs/repo/github/lu-zero/portage-cli)

> **⚠️ Pre-alpha: All applets are stubs** — This is a work in progress. No functionality is implemented yet (except `atom`).

A Rust-based reimplementation of Gentoo Portage command-line tools.

> **Note**: For a more mature Rust-based alternative, see [Pkgcraft](https://pkgcraft.github.io/) — an experimental but functional tooling ecosystem for Gentoo.

## Overview

`em` is a unified command-line frontend for Gentoo/Portage package management, providing subcommands that map to the traditional Portage tool suite.

### Applet Status

| Applet | Traditional tool | Status |
|--------|-----------------|--------|
| *(default)* | `emerge` | Stub |
| `ebuild` | `ebuild` | Stub |
| `maint` | `emaint` | Partial CLI |
| `portageq` | `portageq` | Stub |
| `sync` | `emerge --sync` | Stub |
| `depclean` | `emerge --depclean` | Stub |
| `regen` | `emerge --regen` | Stub |
| `quickpkg` | `quickpkg` | Stub |
| `mirror` | `emirrordist` | Stub |
| `query` | `equery` | Partial CLI |
| `clean` | `eclean` | Partial CLI |
| `use` | `euse` | Stub |
| `revdep` | `revdep-rebuild` | Stub |
| `read` | `eread` | Stub |
| `news` | `eselect news` | Partial CLI |
| `glsa` | `glsa-check` | Partial CLI |
| `file` | `e-file` / `pquery` | Stub |
| `list` | `equery list` | Stub |
| `size` | `equery size` | Stub |
| `check` | `equery check` | Stub |
| `log` | `genlop` | Partial CLI |
| `grep` | `egreplite` | Stub |
| `search` | `emerge --search` | Stub |
| `atom` | — | **Working** |
| `select` | `eselect` | Stub |
| `dispatch` | `dispatch-conf` | Stub |
| `etc` | `etc-update` | Stub |
| `env` | `env-update` | Stub |

**Only `atom` is functional** — parses and displays atom strings via `portage-atom`.

## Architecture

See [ARCHITECTURE.md](./ARCHITECTURE.md) for the complete crate ecosystem and dependency graph.

## Installation

```bash
cargo install --path .
```

## Local Development

The project expects sibling crates in the same directory:

```
./portage-cli          # This crate
./portage-repo         # Required for future implementation
./portage-atom-pubgrub
./portage-bench
./brush-*              # Required by portage-repo (bash shell)
```

Run checks before committing:
```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Usage

```bash
# Parse atom strings (only working feature)
em atom '>=dev-lang/rust-1.80'

# All other commands print their args and exit with "not implemented"
em search rust --description
em query uses dev-lang/rust
em sync
```

## Crate Family

### Published on crates.io

| Crate | Version | Purpose |
|-------|---------|---------|
| [gentoo-interner](https://crates.io/crates/gentoo-interner) | 0.1.2 | String interning (leaf crate) |
| [gentoo-core](https://crates.io/crates/gentoo-core) | 0.4.0 | Architecture types, variants |
| [portage-atom](https://crates.io/crates/portage-atom) | 0.8.0 | PMS atom parsing |
| [portage-metadata](https://crates.io/crates/portage-metadata) | 0.6.0 | md5-cache entry parsing, EAPI, phases, keywords |
| [portage-atom-resolvo](https://crates.io/crates/portage-atom-resolvo) | 0.6.0 | SAT dependency solver (resolvo bridge) |
| [gentoo-stages](https://crates.io/crates/gentoo-stages) | 0.4.0 | Stage3 image fetch/cache |

### Local only (not on crates.io)

| Crate | Version | Purpose | Status |
|-------|---------|---------|--------|
| `portage-repo` | 0.1.0 | Repository layout, ebuilds, profiles, manifests | Functional |
| `portage-atom-pubgrub` | 0.1.0 | Alternative solver (PubGrub bridge) | Complete |
| `portage-bench` | 0.1.0 | Benchmark harness | Functional |
| `portage-cli` | 0.1.0 | The `em` binary | Stubs only |

## Roadmap

1. **Integrate `portage-repo`** — Blocked on publishing `brush-*` crates
2. **Implement `emerge --search`** — Repository walking
3. **Implement `query` subcommands** — Metadata lookup
4. **Implement `sync`** — Repository sync
5. **Implement `emerge`** — Full dependency resolution via `portage-atom-resolvo`

## License

[MIT](LICENSE-MIT)

## Contributing

See [AGENTS.md](./AGENTS.md) for project conventions (Conventional Commits, style, checks).

## Author

Luca Barbato <lu_zero@gentoo.org>
