//! Benchmarks for portage-vdb.
//!
//! Run with: cargo bench -p portage-vdb

use camino::Utf8Path;
use criterion::{Criterion, criterion_group, criterion_main};
use portage_vdb::Vdb;

fn bench_vdb_open(c: &mut Criterion) {
    c.bench_function("vdb_open", |b| {
        b.iter(|| Vdb::open("/var/db/pkg").unwrap())
    });
}

fn bench_vdb_iterate_all(c: &mut Criterion) {
    let vdb = Vdb::open("/var/db/pkg").unwrap();
    c.bench_function("vdb_iterate_all_packages", |b| {
        b.iter(|| vdb.packages().into_iter().count())
    });
}

fn bench_vdb_categories(c: &mut Criterion) {
    let vdb = Vdb::open("/var/db/pkg").unwrap();
    c.bench_function("vdb_categories", |b| {
        b.iter(|| vdb.categories().collect_vec())
    });
}

fn bench_vdb_category_lookup(c: &mut Criterion) {
    let vdb = Vdb::open("/var/db/pkg").unwrap();
    c.bench_function("vdb_category_lookup", |b| {
        b.iter(|| vdb.category("app-shells"))
    });
}

fn bench_vdb_package_lookup(c: &mut Criterion) {
    let vdb = Vdb::open("/var/db/pkg").unwrap();
    c.bench_function("vdb_package_lookup", |b| {
        b.iter(|| {
            vdb.category("app-shells")
                .and_then(|c| c.package("bash-5.3_p9-r2"))
        })
    });
}

fn bench_vdb_owner(c: &mut Criterion) {
    let vdb = Vdb::open("/var/db/pkg").unwrap();
    c.bench_function("vdb_owner_scan", |b| {
        b.iter(|| vdb.owner(Utf8Path::new("/bin/bash")))
    });
}

fn bench_vdb_read_metadata(c: &mut Criterion) {
    let vdb = Vdb::open("/var/db/pkg").unwrap();
    c.bench_function("vdb_read_pkg_metadata", |b| {
        b.iter(|| {
            if let Some(pkg) = vdb
                .category("app-shells")
                .and_then(|c| c.package("bash-5.3_p9-r2"))
            {
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
    let vdb = Vdb::open("/var/db/pkg").unwrap();
    c.bench_function("vdb_read_contents", |b| {
        b.iter(|| {
            if let Some(pkg) = vdb
                .category("app-shells")
                .and_then(|c| c.package("bash-5.3_p9-r2"))
            {
                let _ = pkg.contents();
            }
        })
    });
}

criterion_group!(
    benches,
    bench_vdb_open,
    bench_vdb_iterate_all,
    bench_vdb_categories,
    bench_vdb_category_lookup,
    bench_vdb_package_lookup,
    bench_vdb_owner,
    bench_vdb_read_metadata,
    bench_vdb_read_contents,
);
criterion_main!(benches);
