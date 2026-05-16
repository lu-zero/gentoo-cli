# Architecture

## Crate Ecosystem

### Published on crates.io (usable now)

| Crate | Version | Purpose | Status |
|-------|---------|---------|--------|
| `gentoo-interner` | 0.1.2 | String interning (leaf crate) | Complete |
| `gentoo-core` | 0.4.0 | Architecture types, variants | Complete |
| `portage-atom` | 0.8.0 | PMS atom parsing (Cpn, Cpv, Dep, Version) | Complete |
| `portage-metadata` | 0.6.0 | md5-cache entry parsing, EAPI, phases, keywords | Complete |
| `portage-atom-resolvo` | 0.6.0 | SAT dependency solver (resolvo bridge) | Complete |
| `gentoo-stages` | 0.4.0 | Stage3 image fetch/cache | Complete |

### Local only (not on crates.io)

| Crate | Version | Purpose | Status | Blocker |
|-------|---------|---------|--------|---------|
| `portage-repo` | 0.1.0 | Repo layout, ebuilds, profiles, manifests | Functional | Depends on `brush-*` via local paths |
| `portage-atom-pubgrub` | 0.1.0 | Alternative solver (PubGrub bridge) | Complete | Depends on `portage-atom` via local path |
| `portage-bench` | 0.1.0 | Benchmark harness | Functional | Dev tool, not a library |
| `portage-cli` | 0.1.0 | The `em` binary | Stubs only | Needs `portage-repo` + solver |

## Dependency Graph

```
gentoo-interner ──┐
                  ├── gentoo-core ── gentoo-stages
portage-atom ─────┤
   │              ├── portage-metadata ──┐
   │              │                      ├── portage-repo (brush-*)
   ├── portage-atom-resolvo (resolvo)   │
   └── portage-atom-pubgrub (pubgrub)   └── portage-cli
```

## Crate Details

### `gentoo-interner` (v0.1.2, crates.io)

String interning foundation crate.

**Public API:**

- `trait Interner` — `get_or_intern(&str) -> Key`, `resolve(&Key) -> &str`
- `struct NoInterner` — non-interning fallback (Key = `Box<str>`)
- `struct GlobalInterner` *(feature: interner)* — process-global interner backed by `lasso::ThreadedRodeo`
- `type DefaultInterner` — alias: `GlobalInterner` with feature on, `NoInterner` without
- `struct Interned<I>` — interned string key, implements `Deref<Target=str>`, `Display`

### `gentoo-core` (v0.4.0, crates.io)

Architecture and release-variant types.

**Public API:**

- `enum KnownArch` — 18 official Gentoo architectures, methods: `as_keyword()`, `parse()`, `bitness()`, `current()`
- `enum Arch<I>` — known or exotic architecture, methods: `intern()`, `from_chost()`, `as_str()`
- `type ExoticKey<I>` — alias for `Interned<I>`
- `struct Variant<I>` — release media variant (`arch-flavor`), methods: `parse()`, `flavor()`, `keyword()`

### `portage-atom` (v0.8.0, crates.io)

Core PMS atom parser — the foundation for everything else.

**Public API:**

- `struct Cpn` — Category/Package Name (`dev-lang/rust`). Methods: `new()`, `parse()`, `try_new()`
- `struct Cpv` — Category/Package/Version (`dev-lang/rust-1.75.0`). Methods: `new()`, `parse()`, `try_new()`
- `struct Dep` — Full dependency atom with blocker, operator, version, slot, USE, repo. Methods: `new()`, `parse()`, `category()`, `package()`, `cpv()`
- `enum Blocker` — `Weak` (!) or `Strong` (!!)
- `enum DepEntry` — Dependency tree node: `Atom(Dep)`, `UseConditional`, `AllOf`, `AnyOf`, `ExactlyOneOf`, `AtMostOneOf`
- `struct Version` — PMS version with suffixes and revision. Methods: `new()`, `parse()`, `glob_matches()`, `base()`
- `enum Operator` — `<`, `<=`, `=`, `~`, `>=`, `>`
- `struct Revision(u64)` — Package revision (`-rN`)
- `struct Slot` — Slot + optional subslot
- `enum SlotDep` / `enum SlotOperator` — `:=`, `:*`
- `struct UseDep` — USE flag constraint with kind and default
- `enum UseDepKind` — `Enabled`, `Disabled`, `Conditional`, `Equal`, etc.
- Builder types (feature: `builder`): `CpnBuilder`, `CpvBuilder`, `DepBuilder`, etc.

### `portage-metadata` (v0.6.0, crates.io)

Ebuild metadata cache parser.

**Public API:**

- `struct CacheEntry<I>` — Parsed md5-cache entry. Methods: `parse()`, `from_kv_pairs()`, `serialize()`
- `struct EbuildMetadata<I>` — 21 metadata fields: `eapi`, `description`, `slot`, `homepage`, `src_uri`, `license`, `keywords`, `iuse`, `required_use`, `restrict`, `properties`, `depend`, `rdepend`, `bdepend`, `pdepend`, `idepend`, `inherit`, `inherited`, `defined_phases`
- `enum Eapi` — EAPI 0-9 with feature-query methods: `has_bdepend()`, `has_idepend()`, etc.
- `enum Phase` — 15 ebuild phase functions
- `struct Keyword<I>` / `enum Stability` — `Stable`, `Testing`, `Disabled`, `DisabledAll`
- `struct IUse<I>` / `enum IUseDefault` — USE flag with default (+/-)
- `struct LicenseExpr`, `struct RequiredUseExpr`, `struct RestrictExpr`, `struct SrcUriEntry`

### `portage-atom-resolvo` (v0.6.0, crates.io)

SAT-based dependency solver bridge using resolvo.

**Public API:**

- `struct PortageDependencyProvider` — Main solver bridge, implements resolvo's `Interner` + `DependencyProvider`. Methods: `new()`, `with_installed()`, `dependency_graph()`, `install_order()`
- `struct PortagePool` — Arena storage for solver IDs
- `struct PackageMetadata` — Per-version metadata: `cpv`, `slot`, `subslot`, `iuse`, `use_flags`, `repo`, `dependencies`
- `struct PackageDeps` — 5 dep classes: `depend`, `rdepend`, `bdepend`, `pdepend`, `idepend`
- `struct UseConfig` — USE flag evaluation: `enabled`, `disabled`, `solver_decided` HashSets
- `trait PackageRepository` — `all_packages()`, `versions_for()`
- `struct InMemoryRepository` — HashMap-backed test impl
- `fn version_matches()` — PMS version matching

**Tested features:** `||`/`^^`/`??` groups, USE-conditionals, slot separation, subslot matching, version operators, blockers, rebuild triggers, USE-dep constraints, repo constraints, circular deps via PDEPEND, install ordering.

### `portage-atom-pubgrub` (v0.1.0, local only)

Alternative solver bridge using PubGrub. Same feature set as the resolvo bridge.

**Blocker:** Depends on `portage-atom` via local path. Needs either publishing `portage-atom` path deps or switching to version deps.

### `portage-repo` (v0.1.0, local only)

Repository layout reader — the most complex crate.

**Public API:**

- `struct Repository` — Main entry point. Methods: `open()`, `name()`, `layout()`, `categories()`, `ebuilds()`, `cache_entry()`, `profiles()`, `arch()`
- `struct Category` / `struct Package` / `struct Ebuild` — Directory hierarchy
- `struct Ebuilds` / `EbuildsIter` — Lazy ebuild discovery with filtering
- `struct LayoutConf` — `metadata/layout.conf` parser
- `struct Manifest` / `ManifestEntry` — `Manifest` file parser with BLAKE2/SHA256/MD5 verification
- `struct PkgMetadata` — `metadata/pkg_desc_index` parsing
- `struct Profile` / `ProfileDesc` / `ProfileStack` / `ProfileStatus` — Profile directory resolution
- `struct EbuildShell` — Embedded bash shell via brush for ebuild sourcing
- `struct UseExpand` — USE_EXPAND handling

**Blocker:** Depends on `brush-core`/`brush-builtins`/`brush-parser` via local paths. Those crates are not published to crates.io.

### `gentoo-stages` (v0.4.0, crates.io)

Stage3 tarball fetch and cache management.

**Public API:**

- `struct Stage3` — Stage3 image info. Methods: `is_cached()`, `file_path()`
- `struct Client` / `ClientBuilder` — HTTP client for mirror listings
- `struct Cache` — Local filesystem cache

### `portage-bench` (v0.1.0, local only)

CLI benchmark tool. Loads a real Gentoo repo, compares resolvo vs PubGrub solver performance. Not a library.

## What `portage-cli` Needs From Each Crate

| CLI applet | `portage-atom` | `portage-metadata` | `portage-repo` | `portage-atom-resolvo` |
|------------|:-:|:-:|:-:|:-:|
| `emerge` | parse atoms | cache lookup | repo walk, profile | resolve deps |
| `query *` | parse atoms | metadata fields | installed db, repo | some subcmds |
| `sync` | - | - | repo rsync/git | - |
| `depclean` | parse atoms | - | installed db | reverse deps |
| `search` | parse pattern | descriptions | repo walk | - |
| `atom` | **done** | - | - | - |
| `news` | - | - | profile/news dir | - |
| `glsa` | parse atoms | - | glsa dir | - |
| `ebuild` | parse cpv | phase info | ebuild sourcing | - |
| `clean` | - | - | distfiles/pkg dir | - |
| `log` | parse atoms | - | emerge.log | - |

## Critical Path

`portage-repo` is the key blocker for implementing real functionality in `portage-cli`. It's the only crate that reads a Gentoo repository from disk. It depends on `brush-*` (embedded bash shell for ebuild sourcing) via local paths. Options:

1. **Publish `brush-*` to crates.io** — then `portage-repo` can have normal dependencies
2. **Vendor/generate metadata** — skip ebuild sourcing, only use pre-generated md5-cache
3. **Git dependency with submodules** — `portage-repo` includes brush as a git submodule
4. **Path dependencies** — keep everything as a workspace (local-only development)
