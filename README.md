# Open Street Map Public Transport Parser

A tool to parse broken/disconnected relations from openstreetmap, reconnect them with some tolerance, sort the internal points, and output them as GeoJSON linestrings.

It can understand and reconstruct
 - relations that represent public transport v2
 - ways and relations that represent areas

Blazing fast by design: every step in the process pipeline is fully parallelized for maximum speed using all available cpus by default.

### How it works

1. Reads data from a .pbf file, extracting ways and relations filtered by attributes specified via the `--filter` argument.
2. Processes each relation as follows:
   1. Sorts all the ways within the relation based on proximity.
   2. Joins ways that share identical lat/lng coordinates at their first or last nodes, combining them into a single LineString.
   3. If multiple LineStrings remain after this step:
      - It checks for gaps.
      - If the gaps are smaller than the specified gap threshold (in meters, default: 150), they are joined into one LineString.
3. Outputs a JSON array, with one GeoJSON feature per way or relation found, including:
   - For areas: a single LineString or MultiLineString feature.
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

## Run CI linter + recommendations + tests

```
cargo fmt -- --check && cargo clippy --all-targets -- -D warnings -A clippy::ptr-arg && cargo test
```

## Build pbf test file

```
wget http://download.geofabrik.de/south-america/ecuador-latest.osm.pbf
osmconvert ecuador-latest.osm.pbf -o=ecuador.o5m
osmfilter ecuador.o5m --keep= --keep-relations="@id=85965 =2030162" > test.o5m
osmconvert test.o5m -o=test.pbf
```
