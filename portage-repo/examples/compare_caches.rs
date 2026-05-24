//! Structural comparison of two md5-dict cache directories.
//!
//! Compares every cache entry in two directories (e.g. pkgcraft output vs.
//! portage-repo output, or either vs. the portage reference) using the
//! portage-metadata parsers rather than raw text diff.
//!
//! Field-level comparison strategy (PMS §7.2, §7.6):
//!
//! | Field(s)                                      | Strategy                          |
//! |-----------------------------------------------|-----------------------------------|
//! | EAPI, DESCRIPTION, SLOT, HOMEPAGE             | Exact string equality             |
//! | IUSE, KEYWORDS, DEFINED_PHASES                | Token set equality (order ignored)|
//! | SRC_URI                                       | Parsed `SrcUriEntry` tree         |
//! | DEPEND, RDEPEND, BDEPEND, PDEPEND, IDEPEND,   | Parsed `DepEntry` tree, sorted    |
//! |   LICENSE, RESTRICT, PROPERTIES, REQUIRED_USE | at each level for order-independence|
//! | _eclasses_                                    | Eclass-name set (checksums differ |
//! |                                               | by implementation, ignored here)  |
//!
//! Non-PMS / implementation fields excluded: INHERIT, INHERITED, _md5_.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use portage_atom::DepEntry;
use portage_metadata::{LicenseExpr, RequiredUseExpr, SrcUriEntry};

// ── Field classification ──────────────────────────────────────────────────────

const EXACT_FIELDS: &[&str] = &["EAPI", "DESCRIPTION", "SLOT", "HOMEPAGE"];

const SET_FIELDS: &[&str] = &["IUSE", "KEYWORDS", "DEFINED_PHASES"];

const DEP_FIELDS: &[&str] = &[
    "DEPEND",
    "RDEPEND",
    "BDEPEND",
    "PDEPEND",
    "IDEPEND",
    "RESTRICT",
    "PROPERTIES",
    "REQUIRED_USE",
];

const EXCLUDE_FIELDS: &[&str] = &["INHERIT", "INHERITED", "_md5_"];

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(about = "Compare two md5-cache directories field by field")]
struct Args {
    /// First cache directory
    dir_a: PathBuf,
    /// Second cache directory
    dir_b: PathBuf,
    /// Number of parallel workers (default: available CPUs)
    #[arg(short = 'j', long)]
    jobs: Option<usize>,
    /// Deduplicate top-level dep entries before comparing.
    /// Use when comparing portage/egencache output (preserves duplicates)
    /// against pkgcraft output (deduplicates).
    #[arg(long)]
    dedup: bool,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn token_set(s: &str) -> BTreeSet<&str> {
    s.split_whitespace().collect()
}

fn eclasses_name_set(val: &str) -> BTreeSet<String> {
    let parts: Vec<&str> = val.split('\t').collect();
    parts
        .chunks(2)
        .filter_map(|c| {
            if c.len() == 2 {
                Some(c[0].to_owned())
            } else {
                None
            }
        })
        .collect()
}

fn normalize_src_uri(entries: &[SrcUriEntry], dedup: bool) -> String {
    let mut parts: Vec<String> = entries
        .iter()
        .map(|e| normalize_src_entry(e, dedup))
        .collect();
    parts.sort();
    if dedup {
        parts.dedup();
    }
    parts.join(" ")
}

fn normalize_src_entry(e: &SrcUriEntry, dedup: bool) -> String {
    match e {
        SrcUriEntry::Uri {
            url, restriction, ..
        } => match restriction {
            Some(r) => format!("{r}+{url}"),
            None => url.clone(),
        },
        SrcUriEntry::Renamed {
            url,
            target,
            restriction,
        } => match restriction {
            Some(r) => format!("{r}+{url} -> {target}"),
            None => format!("{url} -> {target}"),
        },
        SrcUriEntry::UseConditional {
            flag,
            negated,
            entries,
        } => {
            let prefix = if *negated {
                format!("!{flag}?")
            } else {
                format!("{flag}?")
            };
            format!("{prefix} ( {} )", normalize_src_uri(entries, dedup))
        }
        SrcUriEntry::Group(entries) => {
            format!("( {} )", normalize_src_uri(entries, dedup))
        }
    }
}

fn normalize_dep_entries(entries: &[DepEntry]) -> String {
    let mut parts: Vec<String> = entries.iter().map(normalize_dep_entry).collect();
    parts.sort();
    parts.join(" ")
}

fn normalize_dep_entry(e: &DepEntry) -> String {
    match e {
        DepEntry::Atom(dep) => dep.to_string(),
        DepEntry::UseConditional {
            flag,
            negate,
            children,
        } => {
            let prefix = if *negate {
                format!("!{flag}?")
            } else {
                format!("{flag}?")
            };
            format!("{prefix} ( {} )", normalize_dep_entries(children))
        }
        DepEntry::AllOf(entries) => format!("( {} )", normalize_dep_entries(entries)),
        DepEntry::AnyOf(entries) => format!("|| ( {} )", normalize_dep_entries(entries)),
        DepEntry::ExactlyOneOf(entries) => format!("^^ ( {} )", normalize_dep_entries(entries)),
        DepEntry::AtMostOneOf(entries) => format!("?? ( {} )", normalize_dep_entries(entries)),
    }
}

fn dedup_dep_recursive(entries: Vec<DepEntry>) -> Vec<DepEntry> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|e| seen.insert(e.clone()))
        .map(|e| match e {
            DepEntry::UseConditional {
                flag,
                negate,
                children,
            } => DepEntry::UseConditional {
                flag,
                negate,
                children: dedup_dep_recursive(children),
            },
            DepEntry::AllOf(ch) => DepEntry::AllOf(dedup_dep_recursive(ch)),
            DepEntry::AnyOf(ch) => DepEntry::AnyOf(dedup_dep_recursive(ch)),
            DepEntry::ExactlyOneOf(ch) => DepEntry::ExactlyOneOf(dedup_dep_recursive(ch)),
            DepEntry::AtMostOneOf(ch) => DepEntry::AtMostOneOf(dedup_dep_recursive(ch)),
            atom => atom,
        })
        .collect()
}

fn normalize_dep(s: &str, dedup: bool) -> String {
    if let Ok(entries) = DepEntry::parse(s) {
        let entries = if dedup {
            dedup_dep_recursive(entries)
        } else {
            entries
        };
        normalize_dep_entries(&entries)
    } else if let Ok(ru) = RequiredUseExpr::parse(s) {
        let ru = if dedup { ru.dedup() } else { ru };
        match &ru {
            RequiredUseExpr::All(entries) => normalize_required_use_entries(entries),
            _ => normalize_required_use_entry(&ru),
        }
    } else {
        s.to_owned()
    }
}

fn normalize_license_entries(entries: &[LicenseExpr]) -> String {
    let mut parts: Vec<String> = entries.iter().map(normalize_license_entry).collect();
    parts.sort();
    parts.join(" ")
}

fn normalize_license_entry(e: &LicenseExpr) -> String {
    match e {
        LicenseExpr::License(name) => name.clone(),
        LicenseExpr::AnyOf(entries) => format!("|| ( {} )", normalize_license_entries(entries)),
        LicenseExpr::UseConditional {
            flag,
            negated,
            entries,
        } => {
            let prefix = if *negated {
                format!("!{flag}?")
            } else {
                format!("{flag}?")
            };
            format!("{prefix} ( {} )", normalize_license_entries(entries))
        }
        LicenseExpr::All(entries) => normalize_license_entries(entries),
    }
}

fn normalize_license(s: &str, dedup: bool) -> String {
    if let Ok(lic) = LicenseExpr::parse(s) {
        let lic = if dedup { lic.dedup() } else { lic };
        match &lic {
            LicenseExpr::All(entries) => normalize_license_entries(entries),
            _ => normalize_license_entry(&lic),
        }
    } else {
        s.to_owned()
    }
}

fn normalize_required_use_entries(entries: &[RequiredUseExpr]) -> String {
    let mut parts: Vec<String> = entries.iter().map(normalize_required_use_entry).collect();
    parts.sort();
    parts.join(" ")
}

fn normalize_required_use_entry(e: &RequiredUseExpr) -> String {
    match e {
        RequiredUseExpr::Flag { name, negated } => {
            if *negated {
                format!("!{name}")
            } else {
                name.clone()
            }
        }
        RequiredUseExpr::AnyOf(entries) => {
            format!("|| ( {} )", normalize_required_use_entries(entries))
        }
        RequiredUseExpr::ExactlyOne(entries) => {
            format!("^^ ( {} )", normalize_required_use_entries(entries))
        }
        RequiredUseExpr::AtMostOne(entries) => {
            format!("?? ( {} )", normalize_required_use_entries(entries))
        }
        RequiredUseExpr::UseConditional {
            flag,
            negated,
            entries,
        } => {
            let prefix = if *negated {
                format!("!{flag}?")
            } else {
                format!("{flag}?")
            };
            format!("{prefix} ( {} )", normalize_required_use_entries(entries))
        }
        RequiredUseExpr::All(entries) => normalize_required_use_entries(entries),
    }
}

fn normalize_required_use(s: &str, dedup: bool) -> String {
    if let Ok(ru) = RequiredUseExpr::parse(s) {
        let ru = if dedup { ru.dedup() } else { ru };
        match &ru {
            RequiredUseExpr::All(entries) => normalize_required_use_entries(entries),
            _ => normalize_required_use_entry(&ru),
        }
    } else {
        s.to_owned()
    }
}

// ── Cache file parsing ────────────────────────────────────────────────────────

fn parse_cache_file(path: &Path) -> BTreeMap<String, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return BTreeMap::new(),
    };
    let mut map = BTreeMap::new();
    for line in content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.to_owned(), v.to_owned());
        }
    }
    map
}

// ── Per-file comparison ───────────────────────────────────────────────────────

struct FileDiff {
    cpv: String,
    field: String,
    a: String,
    b: String,
}

fn compare_cache_entries(
    cpv: &str,
    map_a: &BTreeMap<String, String>,
    map_b: &BTreeMap<String, String>,
    dedup: bool,
) -> Vec<FileDiff> {
    let mut diffs = Vec::new();
    let empty = String::new();

    for &key in EXACT_FIELDS {
        let va = map_a.get(key).unwrap_or(&empty);
        let vb = map_b.get(key).unwrap_or(&empty);
        if va != vb {
            diffs.push(FileDiff {
                cpv: cpv.to_owned(),
                field: key.to_owned(),
                a: va.clone(),
                b: vb.clone(),
            });
        }
    }

    for &key in SET_FIELDS {
        let va = map_a.get(key).unwrap_or(&empty);
        let vb = map_b.get(key).unwrap_or(&empty);
        if token_set(va) != token_set(vb) {
            diffs.push(FileDiff {
                cpv: cpv.to_owned(),
                field: key.to_owned(),
                a: va.clone(),
                b: vb.clone(),
            });
        }
    }

    for &key in DEP_FIELDS {
        let va = map_a.get(key).unwrap_or(&empty);
        let vb = map_b.get(key).unwrap_or(&empty);
        let na = normalize_dep(va, dedup);
        let nb = normalize_dep(vb, dedup);
        if na != nb {
            diffs.push(FileDiff {
                cpv: cpv.to_owned(),
                field: key.to_owned(),
                a: va.clone(),
                b: vb.clone(),
            });
        }
    }

    {
        let va = map_a.get("LICENSE").unwrap_or(&empty);
        let vb = map_b.get("LICENSE").unwrap_or(&empty);
        let na = normalize_license(va, dedup);
        let nb = normalize_license(vb, dedup);
        if na != nb {
            diffs.push(FileDiff {
                cpv: cpv.to_owned(),
                field: "LICENSE".to_owned(),
                a: va.clone(),
                b: vb.clone(),
            });
        }
    }

    {
        let va = map_a.get("SRC_URI").unwrap_or(&empty);
        let vb = map_b.get("SRC_URI").unwrap_or(&empty);
        let na = SrcUriEntry::parse(va)
            .map(|e| normalize_src_uri(&e, dedup))
            .unwrap_or_else(|_| va.clone());
        let nb = SrcUriEntry::parse(vb)
            .map(|e| normalize_src_uri(&e, dedup))
            .unwrap_or_else(|_| vb.clone());
        if na != nb {
            diffs.push(FileDiff {
                cpv: cpv.to_owned(),
                field: "SRC_URI".to_owned(),
                a: va.clone(),
                b: vb.clone(),
            });
        }
    }

    {
        let va = map_a.get("_eclasses_").unwrap_or(&empty);
        let vb = map_b.get("_eclasses_").unwrap_or(&empty);
        if eclasses_name_set(va) != eclasses_name_set(vb) {
            diffs.push(FileDiff {
                cpv: cpv.to_owned(),
                field: "_eclasses_".to_owned(),
                a: va.clone(),
                b: vb.clone(),
            });
        }
    }

    let known: BTreeSet<&str> = EXACT_FIELDS
        .iter()
        .chain(SET_FIELDS)
        .chain(DEP_FIELDS)
        .chain(EXCLUDE_FIELDS)
        .copied()
        .chain(["SRC_URI", "LICENSE", "_eclasses_"])
        .collect();

    for key in map_a.keys().chain(map_b.keys()) {
        let key = key.as_str();
        if known.contains(key) {
            continue;
        }
        let va = map_a.get(key).unwrap_or(&empty);
        let vb = map_b.get(key).unwrap_or(&empty);
        if va != vb {
            diffs.push(FileDiff {
                cpv: cpv.to_owned(),
                field: format!("{key} (unknown)"),
                a: va.clone(),
                b: vb.clone(),
            });
        }
    }

    diffs
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn collect_entries(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut map = BTreeMap::new();
    let Ok(walk) = jwalk::WalkDir::new(dir).sort(true).try_into_iter() else {
        return map;
    };
    for entry in walk.flatten() {
        if entry.file_type().is_file() {
            if let Ok(rel) = entry.path().strip_prefix(dir) {
                let cpv = rel.to_string_lossy().into_owned();
                map.insert(cpv, entry.path());
            }
        }
    }
    map
}

fn main() {
    let args = Args::parse();

    let jobs = args.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });

    let entries_a = collect_entries(&args.dir_a);
    let entries_b = collect_entries(&args.dir_b);

    let only_a: Vec<String> = entries_a
        .keys()
        .filter(|k| !entries_b.contains_key(*k))
        .cloned()
        .collect();
    let only_b: Vec<String> = entries_b
        .keys()
        .filter(|k| !entries_a.contains_key(*k))
        .cloned()
        .collect();
    let common: Vec<String> = entries_a
        .keys()
        .filter(|k| entries_b.contains_key(*k))
        .cloned()
        .collect();

    let total = common.len();
    eprintln!(
        "Comparing {total} common entries ({} only in a, {} only in b) with {jobs} workers…",
        only_a.len(),
        only_b.len()
    );

    for cpv in &only_a {
        eprintln!("ONLY-A: {cpv}");
    }
    for cpv in &only_b {
        eprintln!("ONLY-B: {cpv}");
    }

    let (tx, rx) = flume::bounded::<(String, PathBuf, PathBuf)>(jobs * 4);
    let progress = Arc::new(AtomicUsize::new(0));
    let dedup = args.dedup;

    let mut handles = Vec::new();
    for _ in 0..jobs {
        let rx = rx.clone();
        let progress = Arc::clone(&progress);
        handles.push(std::thread::spawn(move || {
            let mut all_diffs: Vec<FileDiff> = Vec::new();
            while let Ok((cpv, path_a, path_b)) = rx.recv() {
                let n = progress.fetch_add(1, Ordering::Relaxed) + 1;
                eprint!("\r[{n}/{total}] {cpv:<60}");
                let map_a = parse_cache_file(&path_a);
                let map_b = parse_cache_file(&path_b);
                all_diffs.extend(compare_cache_entries(&cpv, &map_a, &map_b, dedup));
            }
            all_diffs
        }));
    }
    drop(rx);

    for cpv in &common {
        let path_a = entries_a[cpv].clone();
        let path_b = entries_b[cpv].clone();
        if tx.send((cpv.clone(), path_a, path_b)).is_err() {
            break;
        }
    }
    drop(tx);

    let mut all_diffs: Vec<FileDiff> = Vec::new();
    for h in handles {
        all_diffs.extend(h.join().unwrap());
    }
    eprintln!();

    all_diffs.sort_by(|a, b| a.cpv.cmp(&b.cpv).then(a.field.cmp(&b.field)));

    let diff_count = all_diffs.len();
    for d in &all_diffs {
        println!("DIFF {} {}:", d.cpv, d.field);
        println!("  a: {}", d.a);
        println!("  b: {}", d.b);
    }

    println!(
        "\nTotal: {total}  Only-a: {}  Only-b: {}  Field diffs: {diff_count}",
        only_a.len(),
        only_b.len()
    );

    if !only_a.is_empty() || !only_b.is_empty() || diff_count > 0 {
        std::process::exit(1);
    }
}
