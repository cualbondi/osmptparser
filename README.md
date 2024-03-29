# Open Street Map Public Transport Parser

[![Status](https://github.com/cualbondi/osmptparser/workflows/Test/badge.svg)](https://github.com/cualbondi/osmptparser/actions)
[![codecov](https://codecov.io/gh/cualbondi/osmptparser/branch/master/graph/badge.svg)](https://codecov.io/gh/cualbondi/osmptparser)

## Test how it works

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
cargo fmt -- --check && cargo clippy -- -D warnings -A clippy::ptr-arg && cargo test
```

## Build pbf test file

```
wget http://download.geofabrik.de/south-america/ecuador-latest.osm.pbf
osmconvert ecuador-latest.osm.pbf -o=ecuador.o5m
osmfilter ecuador.o5m --keep= --keep-relations="@id=85965 =2030162" > test.o5m
osmconvert test.o5m -o=test.pbf
```
