# portage-vdb

Reader for the Gentoo Portage installed-package database (VDB).

The VDB lives at `/var/db/pkg` and contains one subdirectory per category,
each holding one subdirectory per installed package version. This crate
provides a typed, lazy-reading API over that on-disk structure.

## Quick start

```rust
use portage_vdb::Vdb;
use std::path::Path;

// Open the default VDB
let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();

// Iterate every installed package
for pkg in vdb.packages() {
    println!("{} (slot {})", pkg, pkg.slot().unwrap());
}

// Find which package owns a file
if let Some(pkg) = vdb.owner(Path::new("/bin/bash")) {
    println!("/bin/bash is owned by {}", pkg);
}

// Find all installed versions of a package
let versions = vdb.find_by_cpn("app-shells", "bash");

// Pattern-based lookup (used by CLI tools)
let matched = vdb.find_by_pattern("app-shells/bash");
```

## Crate family

- [`portage-atom`](https://crates.io/crates/portage-atom) — PMS atom/dep parser
- [`portage-metadata`](https://crates.io/crates/portage-metadata) — metadata cache types
- [`portage-repo`](https://crates.io/crates/portage-repo) — repository layout reader
- `portage-vdb` (this crate) — installed package database reader

Part of the [portage-cli](https://github.com/lu-zero/portage-cli) workspace.

## License

MIT
