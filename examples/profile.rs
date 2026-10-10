//! CPU profiler for the end-to-end ptv2 pipeline over the ecuador extract.
//!
//! Run (release + debug symbols so the flamegraph has readable frames):
//!   RUSTFLAGS="-C debuginfo=2" cargo run --release --example profile
//!
//! Optional first arg = number of cpus (default 1, so the SIGPROF sampler
//! captures the whole workload on a single worker and the flamegraph reflects
//! true per-work CPU distribution instead of being split across threads).
//!
//! Writes `flamegraph.svg` and prints the hottest self-time functions.

use osmptparser::Parser;
use std::collections::HashMap;
use std::fs::File;

const PBF: &str = "ecuador-latest.osm.pbf";
const GAP: f64 = 1500.0;

fn main() {
    let cpus = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1);

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(1000)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .unwrap();

    let parser = Parser::new_ptv2(PBF, cpus).unwrap();
    let pts = parser.get_public_transports(GAP);
    println!("parsed {} public transports (cpus={})", pts.len(), cpus);

    let report = guard.report().build().unwrap();

    // flamegraph svg
    let svg = File::create("flamegraph.svg").unwrap();
    report.flamegraph(svg).unwrap();
    println!("wrote flamegraph.svg");

    // text summary: aggregate samples by leaf (self-time) function
    let mut self_counts: HashMap<String, isize> = HashMap::new();
    let mut total = 0isize;
    for (frames, count) in report.data.iter() {
        total += *count;
        if let Some(leaf) = frames.frames.first().and_then(|f| f.first()) {
            *self_counts.entry(leaf.name()).or_default() += *count;
        }
    }
    let mut rows: Vec<_> = self_counts.into_iter().collect();
    rows.sort_by_key(|a| std::cmp::Reverse(a.1));
    println!("\ntop self-time functions ({total} samples):");
    for (name, count) in rows.into_iter().take(25) {
        println!("{:6.2}%  {}", 100.0 * count as f64 / total as f64, name);
    }
}
