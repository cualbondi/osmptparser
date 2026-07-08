use criterion::{criterion_group, criterion_main, Criterion};
use osmptparser::Parser;
use std::hint::black_box;

/// small fixture shipped with the tests (2 public transport relations)
const PBF: &str = "tests/test.pbf";
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

criterion_group!(benches, ptv2_e2e, ptv2_parse, ptv2_get_public_transports);
criterion_main!(benches);
