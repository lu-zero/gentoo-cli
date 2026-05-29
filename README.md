# portage-cli

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE-MIT)
[![Build Status](https://github.com/lu-zero/portage-cli/workflows/CI/badge.svg)](https://github.com/lu-zero/portage-cli/actions?query=workflow:CI)
[![dependency status](https://deps.rs/repo/github/lu-zero/portage-cli/status.svg)](https://deps.rs/repo/github/lu-zero/portage-cli)

A Rust reimplementation of the Gentoo Portage command-line tools, built on a
family of purpose-built crates for parsing atoms, metadata, repositories, and
the installed package database.

> **Note**: For a more mature Rust-based alternative, see
> [Pkgcraft](https://pkgcraft.github.io/).

> **Warning**: This codebase was largely AI-generated and has not yet been
> thoroughly audited. Use at your own risk.

## The `em` binary

`em` is a unified front-end for the Portage tool suite. It dispatches to
subcommands corresponding to the traditional tools.

### Applet status

| Applet | Maps to | Status |
|--------|---------|--------|
| `atom` | — | Working |
| `query` | `equery` | Partial — see below |
| `use` | `euse` | Partial — see below |
| `maint` | `emaint` | Partial — see below |
| `regen` | `emerge --regen` | Working |
| `search` | `emerge --search` | Working |
| *(default)* | `emerge` | Stub |
| `ebuild` | `ebuild` | Stub |
| `depclean` | `emerge --depclean` | Stub |
| `quickpkg` | `quickpkg` | Stub |
| `mirror` | `emirrordist` | Stub |
| `clean` | `eclean` | Stub |
| `revdep` | `revdep-rebuild` | Stub |
| `news` | `eselect news` | Stub |
| `glsa` | `glsa-check` | Stub |
| `log` | `genlop` | Stub |
| `grep` | `egreplite` | Stub |
| `select` | `eselect` | Stub |
| `dispatch` | `dispatch-conf` | Stub |
| `etc` | `etc-update` | Stub |
| `env` | `env-update` | Stub |

---

### `em query` (equery)

| Subcommand | Alias | Status |
|---|---|---|
| `belongs` | `b` | Working — file → owning package via VDB CONTENTS |
| `check` | `k` | Working — MD5 checksum + mtime verification |
| `depends` | `d` | Working — reverse-dep search in metadata cache |
| `depgraph` | `g` | Working — full dep tree via SAT solver |
| `files` | `f` | Working — all files installed by a package |
| `has` | `a` | Working — VDB field search across installed packages |
| `hasuse` | `h` | Working — packages with a given USE flag in IUSE |
| `keywords` | `y` | Working — keyword status across architectures |
| `list` | `l` | Working — available packages; `-I` for installed only |
| `meta` | `m` | Working — maintainers, homepage, longdesc, installed info |
| `size` | `s` | Working — installed size + build timestamp |
| `uses` | `u` | Working — IUSE flags with descriptions + installed status |
| `which` | `w` | Working — path to best matching ebuild |

**Gaps vs equery:**
- `uses` descriptions come from `profiles/use.desc` + `profiles/use.local.desc`.
  Overlay packages not yet regen'd fall back to empty description (metadata.xml
  per-package lookup is not yet wired as a fallback).
- No `stats` subcommand.

---

### `em use` (euse)

| Flag | Status |
|---|---|
| `-a FLAG` | Working — add USE flag to `make.conf` |
| `-r FLAG` | Working — remove USE flag from `make.conf` |
| *(no flags)* | Working — print current USE value |
| `--make-conf PATH` | Working — override make.conf path |

**Gaps vs euse:**
- No `-p pkg` for package-specific USE flags (`/etc/portage/package.use`).
- `get()` returns the raw unexpanded value; `${COMMON_FLAGS}` references are
  not evaluated (brush-backed expansion is possible but not wired yet).

---

### `em maint` (emaint)

| Subcommand | Status | Notes |
|---|---|---|
| `world` | Working | Checks `world` + `world_sets`; validates `@set` refs against known sets from `/usr/share/portage/config/sets/`, `/etc/portage/sets.conf`, and `/etc/portage/sets/`; `--fix` rewrites both files |
| `revisions` | Working | Purges `repo_revisions` JSON (sync commit history); optional per-repo targeting |
| `moveinst` | Partial | Detects packages needing rename from `profiles/updates/`; does not apply moves or scan installed dependency metadata |
| `regen` | Working | Available as `em regen` |

**Gaps vs emaint:**

- `moveinst` — missing the second pass that walks every installed package's
  `DEPEND`/`RDEPEND`/etc. fields for stale atom references, and the `--fix`
  mode that writes to the VDB.
- `world` — `@set` references are validated by name but not by content (e.g.
  `@preserved-rebuild` is accepted as long as the name is known).
- `all`, `binhost`, `cleanconfmem`, `cleanresume`, `logs`, `merges`,
  `movebin`, `sync` — not implemented.

---

## Architecture

See [ARCHITECTURE.md](./ARCHITECTURE.md) for the full crate dependency graph
and API reference.

### Crate family

| Crate | Purpose | Status |
|-------|---------|--------|
| `portage-atom` | PMS atom parser (`Cpn`, `Cpv`, `Dep`, `Version`) | Published |
| `portage-metadata` | md5-cache entry parser, EAPI, phases, keywords | Published |
| `portage-repo` | Repo layout, profiles, metadata cache, ebuild sourcing | Local only |
| `portage-vdb` | Installed package database reader (`/var/db/pkg`) | Local only |
| `portage-atom-resolvo` | SAT dependency solver (resolvo bridge) | Published |
| `portage-atom-pubgrub` | Alternative solver (PubGrub bridge) | Local only |
| `gentoo-core` | Architecture types | Published |
| `gentoo-stages` | Stage3 tarball fetch/cache | Published |

### brush integration

`portage-repo` embeds [brush](https://github.com/lu-zero/brush) (the
`for-portage-repo` fork branch) — a Rust bash interpreter — for ebuild
sourcing and `make.conf` parsing. Additions to the fork:

- `Program.comments: Vec<SourceSpan>` — comment spans from the winnow parser,
  used by `MakeConf` for byte-precise round-trip editing.
- `ParseContext.comments` accumulator and comment-tracking whitespace parsers
  (`spaces_tracking`, `linebreak_tracking`, `newline_list_tracking`).

## Installation

```bash
cargo install --path portage-cli
```

## Local Development

The project expects sibling checkouts:

```
portage-cli/    # this workspace
brush/          # brush fork at for-portage-repo branch
```

The `.cargo/config.toml` at workspace root patches the brush crates to use the
local checkout.

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## License

[MIT](LICENSE-MIT)

## Contributing

See [AGENTS.md](./AGENTS.md) for project conventions (Conventional Commits,
style, checks).

## Author

Luca Barbato <lu_zero@gentoo.org>
