use criterion::{criterion_group, criterion_main, Criterion};
use osmptparser::Parser;
use std::hint::black_box;
use std::path::Path;
use std::process::Command;

/// small fixture shipped with the tests (2 public transport relations)
const PBF: &str = "tests/test.pbf";
/// large real-world extract, downloaded on first run (see `ensure_ecuador_pbf`)
const ECUADOR_PBF: &str = "ecuador-latest.osm.pbf";
/// geofabrik url for the ecuador extract (302-redirects to the dated file)
const ECUADOR_URL: &str = "http://download.geofabrik.de/south-america/ecuador-latest.osm.pbf";
/// gap tolerance in meters, same value the integration test uses
const GAP: f64 = 1500.0;

/// full end-to-end run: parse the pbf into the cache and flatten every
/// public transport into its geojson-ready geometry
fn ptv2_e2e(c: &mut Criterion) {
    let cpus = num_cpus::get();
    c.bench_function("ptv2_e2e", |b| {
        b.iter(|| {
            let parser = Parser::new_ptv2(black_box(PBF), cpus).unwrap();
            black_box(parser.get_public_transports(black_box(GAP)));
        });
    });
}

/// parse phase only: stream the pbf and build the relations/ways/nodes cache
fn ptv2_parse(c: &mut Criterion) {
    let cpus = num_cpus::get();
    c.bench_function("ptv2_parse", |b| {
        b.iter(|| {
            black_box(Parser::new_ptv2(black_box(PBF), cpus).unwrap());
        });
    });
}

/// process phase only: flatten/fix the geometries from an already-parsed cache
fn ptv2_get_public_transports(c: &mut Criterion) {
    let cpus = num_cpus::get();
    let parser = Parser::new_ptv2(PBF, cpus).unwrap();
    c.bench_function("ptv2_get_public_transports", |b| {
        b.iter(|| {
            black_box(parser.get_public_transports(black_box(GAP)));
        });
    });
}

/// Ensure the large ecuador extract exists locally, downloading it on first run.
/// The tiny `tests/test.pbf` can't show parsing costs that scale with node count,
/// so this benchmark needs a real-world-sized file; it is downloaded once and then
/// reused (the file is gitignored). Requires `curl` or `wget` and network access.
fn ensure_ecuador_pbf() -> &'static str {
    if Path::new(ECUADOR_PBF).exists() {
        return ECUADOR_PBF;
    }
    eprintln!(
        "benchmark fixture '{}' missing; downloading {} (~114 MB)...",
        ECUADOR_PBF, ECUADOR_URL
    );
    // download to a temp path and rename on success so an interrupted download
    // never leaves a truncated file that later looks "already present"
    let part = format!("{}.part", ECUADOR_PBF);
    let status = Command::new("curl")
        .args(["-fL", "--retry", "3", "-o", &part, ECUADOR_URL])
        .status()
        .or_else(|_| {
            Command::new("wget")
                .args(["-O", &part, ECUADOR_URL])
                .status()
        })
        .expect("could not run curl or wget to download the benchmark fixture");
    assert!(status.success(), "download of {} failed", ECUADOR_URL);
    std::fs::rename(&part, ECUADOR_PBF).expect("could not move downloaded fixture into place");
    ECUADOR_PBF
}

/// full end-to-end ptv2 run over the large ecuador extract (downloaded on first run);
/// this is the representative benchmark, parsing ~114 MB the way real usage does
fn ecuador_ptv2_e2e(c: &mut Criterion) {
    let pbf = ensure_ecuador_pbf();
    let cpus = num_cpus::get();
    let mut group = c.benchmark_group("ecuador");
    // each iteration parses ~114 MB (~1s), so keep the sample count low
    group.sample_size(10);
    group.bench_function("ptv2_e2e", |b| {
        b.iter(|| {
            let parser = Parser::new_ptv2(black_box(pbf), cpus).unwrap();
            black_box(parser.get_public_transports(black_box(GAP)));
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    ptv2_e2e,
    ptv2_parse,
    ptv2_get_public_transports,
    ecuador_ptv2_e2e
);
criterion_main!(benches);
