//! Benchmarks for portage-vdb.
//!
//! Run with: cargo bench -p portage-vdb

use criterion::{Criterion, criterion_group, criterion_main};
use portage_vdb::Vdb;
use std::path::Path;

fn bench_vdb_open(c: &mut Criterion) {
    c.bench_function("vdb_open", |b| {
        b.iter(|| Vdb::open(Path::new("/var/db/pkg")).unwrap())
    });
}

fn bench_vdb_iterate_all(c: &mut Criterion) {
    let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
    c.bench_function("vdb_iterate_all_packages", |b| {
        b.iter(|| vdb.packages().count())
    });
}

fn bench_vdb_category_names(c: &mut Criterion) {
    let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
    c.bench_function("vdb_category_names", |b| b.iter(|| vdb.category_names()));
}

fn bench_vdb_find(c: &mut Criterion) {
    let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
    c.bench_function("vdb_find_exact", |b| {
        b.iter(|| vdb.find("app-shells", "bash-5.3_p9-r2"))
    });
}

fn bench_vdb_find_by_cpn(c: &mut Criterion) {
    let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
    c.bench_function("vdb_find_by_cpn", |b| {
        b.iter(|| vdb.find_by_cpn("sys-libs", "glibc"))
    });
}

fn bench_vdb_owner(c: &mut Criterion) {
    let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
    c.bench_function("vdb_owner_scan", |b| {
        b.iter(|| vdb.owner(Path::new("/bin/bash")))
    });
}

fn bench_vdb_read_metadata(c: &mut Criterion) {
    let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
    c.bench_function("vdb_read_pkg_metadata", |b| {
        b.iter(|| {
            if let Some(pkg) = vdb.find("app-shells", "bash-5.3_p9-r2") {
                let _ = pkg.description();
                let _ = pkg.use_flags();
                let _ = pkg.slot();
                let _ = pkg.eapi();
                let _ = pkg.rdepend();
            }
        })
    });
}

fn bench_vdb_read_contents(c: &mut Criterion) {
    let vdb = Vdb::open(Path::new("/var/db/pkg")).unwrap();
    c.bench_function("vdb_read_contents", |b| {
        b.iter(|| {
            if let Some(pkg) = vdb.find("app-shells", "bash-5.3_p9-r2") {
                let _ = pkg.contents();
            }
        })
    });
}

criterion_group!(
    benches,
    bench_vdb_open,
    bench_vdb_iterate_all,
    bench_vdb_category_names,
    bench_vdb_find,
    bench_vdb_find_by_cpn,
    bench_vdb_owner,
    bench_vdb_read_metadata,
    bench_vdb_read_contents,
);
criterion_main!(benches);
