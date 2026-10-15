# Open Street Map Public Transport Parser

A tool to parse broken/disconnected relations from openstreetmap, reconnect them with some tolerance, sort the internal points, and output them as GeoJSON linestrings.

It can understand and reconstruct
 - relations that represent public transport v2
 - ways and relations that represent areas

Blazing fast by design: every step in the process pipeline is fully parallelized for maximum speed using all available cpus by default.

### How it works

1. Reads data from a .pbf file, extracting ways and relations filtered by attributes specified via the `--filter` argument.
2. Processes each public transport relation as follows:
   1. Sorts all the ways within the relation based on proximity.
   2. Joins ways that share identical lat/lng coordinates at their first or last nodes, combining them into a single LineString.
   3. If multiple LineStrings remain after this step:
      - It checks for gaps.
      - If the gaps are smaller than the specified gap threshold (in meters, default: 150), they are joined into one LineString.
3. Outputs a JSON array, with one GeoJSON feature per way or relation found, including:
   - For areas: a MultiPolygon feature. Ways with role `inner` are holes, every other way is part of an outer ring; rings are chained by shared nodes in any member order, a gap smaller than the threshold is bridged, and an area with any ring that cannot be closed is reported as `Broken` with no geometry.
   - For public transport: a LineString representing the full path and an array of points representing the stops.

[See the blogpost](https://jperelli.com.ar/post/2019/08/12/oxidizing-cualbondi/) for a very detailed description

### Status

[![Status](https://github.com/cualbondi/osmptparser/workflows/Test/badge.svg)](https://github.com/cualbondi/osmptparser/actions)
[![codecov](https://codecov.io/gh/cualbondi/osmptparser/branch/master/graph/badge.svg)](https://codecov.io/gh/cualbondi/osmptparser)

## Try it

```
git clone git@github.com:cualbondi/osmptparser.git
wget http://download.geofabrik.de/south-america/ecuador-latest.osm.pbf
cargo run --example main ecuador-latest.osm.pbf
```

Time it

```
cargo build --release --example main && /usr/bin/time -v target/release/examples/main ecuador-latest.osm.pbf
```

## CLI

```
cargo run --release ./ecuador-latest.osm.pbf --filter "boundary=national_park"
```
you should get a json list with one geojson per area that matches with the filter

```
cargo run --release ./ecuador-latest.osm.pbf --filter-ptv2
```
you should get a json list with one geojson per ptv2 containing a linestring and each stop

## Library

```rust
use osmptparser::{ParseStatus, Parser};

let parser = Parser::new_aa("portugal-latest.osm.pbf", 0)?; // name&admin_level&boundary=administrative
for area in parser.get_areas(150.0) {
    // area.id_type is 'r' (relation) or 'w' (standalone closed way)
    // area.geometry: Vec<polygon>, polygon = Vec<ring>, ring = Vec<(lon, lat)>, first ring is the outer one
    if area.parse_status != ParseStatus::Broken {
        println!("{}{} {} {} polygon(s)", area.id_type, area.id, area.tags["name"], area.geometry.len());
    }
}

let parser = Parser::new_ptv2("portugal-latest.osm.pbf", 0)?;
for pt in parser.get_public_transports(150.0) {
    // pt.geometry: Vec<(lon, lat)>; pt.stops[i].role is the member role ("stop", "platform", ...)
}

// raw relations with their members: Way.role / Node.role keep the osm member role
let relation = parser.get_relation_from_id(85965);
```

## Run CI linter + recommendations + tests

```
cargo fmt -- --check && cargo clippy --all-targets -- -D warnings -A clippy::ptr-arg && cargo test
```

## Build pbf test files

`tests/areas.pbf` is built from the hand-written `tests/areas.osm` (administrative areas with outer/inner rings, an exclave, a standalone closed way and a broken ring):

```
osmium cat -f pbf --overwrite -o tests/areas.pbf tests/areas.osm
```

`tests/test.pbf` (two ptv2 routes from Ecuador):

```
wget http://download.geofabrik.de/south-america/ecuador-latest.osm.pbf
osmconvert ecuador-latest.osm.pbf -o=ecuador.o5m
osmfilter ecuador.o5m --keep= --keep-relations="@id=85965 =2030162" > test.o5m
osmconvert test.o5m -o=test.pbf
```
