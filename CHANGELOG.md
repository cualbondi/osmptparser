## v4.1.1

Bugfix: whole files parsed

 - Fixed: parsing could stop silently partway through a .pbf file, returning only the elements read until then (or none at all). osm_pbf_iter's `BlobReader` takes a short read of a blob's 4-byte length as the end of the file, and the `BufReader` it reads from returns one whenever that length straddles its 8 KiB buffer, so whether and where a file was cut depended on its blob layout. The reader is now wrapped so every read is filled until the end of the file (the 2026-10-16 Geofabrik Australia extract stopped after 14401 of 18468 blobs, before any relation).

## v4.1.0

Deterministic output order

 - Changed: `Parser::get_public_transports`, `Parser::get_areas` and `Parser::par_map` now return their results in a deterministic order: relations sorted by osm id, then (for `get_areas`) standalone ways sorted by osm id, whatever the number of threads. Before, the order depended on which worker thread finished first, so two runs on the same file gave the same elements in a different order.

## v4.0.0

Areas as real multipolygons, relation member roles

 - **BREAKING**: `Area::geometry` is now a multipolygon, `Vec<Vec<Vec<(lon, lat)>>>` (polygons -> rings -> coordinates, first ring of each polygon is the outer one, the rest are holes), instead of a list of linestrings. `Area::to_geojson()` outputs a `MultiPolygon` instead of a `Polygon`. The CLI output for `--filter` changes accordingly.
 - **BREAKING**: `Node` and `Way` have a new `role: String` field with the osm relation member role (`outer`, `inner`, `stop`, `platform`, ...; empty for the nodes of a way and for standalone ways). Code that constructs these structs directly must set it. `PublicTransport::to_geojson()` includes it in each stop's properties.
 - Added: `Relation::polygons(tolerance)` and `Way::polygons(tolerance)` build a multipolygon following the osm multipolygon model: ways with role `inner` are holes, every other way is part of an outer ring; rings are chained by shared end nodes in any member order (`Sorted` when a way is taken out of order), a gap up to `tolerance` meters is bridged (`Joined`), each hole is assigned to the smallest outer ring that contains it, and any ring that cannot be closed makes the whole area `Broken` with an empty geometry. `Parser::get_areas` uses it, so areas with several outer rings (exclaves, islands) and holes are no longer flattened into one ring, and the inner rings are no longer mixed into the outer one.
 - Added: `Node` and `Way` are now exported at the crate root (they were only reachable through `Relation` fields).
 - Added: `tests/areas.osm` / `tests/areas.pbf` fixture and integration tests for `get_areas` and member roles; unit tests for the ring assembly.

## v3.0.0

Bugfix and test coverage

 - **BREAKING**: `Parser::new`, `Parser::new_ptv2` and `Parser::new_aa` now return `std::io::Result<Self>` (previously it returned `Self` and panicked).
 - **BREAKING**: `ParseStatus` is now an enum (`Ok`, `Sorted`, `Joined`, `JoinedSorted`, `Broken`) instead of a struct of magic numeric codes; use `ParseStatus::code()` / `ParseStatus::detail()` for the previous `code`/`detail` values (JSON output is unchanged).
 - **BREAKING**: `Parser::get_relation_from_id` now returns `Option<Relation>` (`None` when no relation with that id was parsed) instead of panicking on a missing id.
 - **BREAKING**: `Relation::flatten_ways` and `Way::flatten_ways` now return `(Vec<Vec<Node>>, ParseStatus)` directly instead of a `Result`; they never returned `Err`, and the unused `ParseError` type was removed.
 - **BREAKING**: *[Performance +35%]*. the `tags` and `info` fields on `Node`, `Way`, `Relation`, `PublicTransport` and `Area` are now `Tags` (an exported alias for `Arc<HashMap<String, String>>`) instead of `HashMap<String, String>`. Materializing relations/ways/nodes from the cache now shares these maps via reference-count bumps instead of deep-copying them. Reading tags (`tags["key"]`, `tags.get(...)`) is unchanged; code that constructs these structs directly must wrap the maps in `Arc::new(...)`.
 - *[Performance +14%]* The u64-keyed id sets and node/way caches now use rustc-hash's `FxHashMap`/`FxHashSet` instead of the default SipHash, speeding up the millions of osm-id inserts and lookups during parsing.
 - Changed: `sort_ways` sorts by index and clones each way once.
 - Changed: updated all dependencies to their latest versions.
 - Fixed: `info.timestamp` stays in epoch seconds with osm_pbf_iter 0.2.3 (which returns milliseconds).
 - Added: `profile` example that writes a pprof flamegraph of the ptv2 pipeline.
 - Internal: CI runs on the `rust:1` image with tarpaulin and codecov-action.
 - Changed: progress logging (`START ... map, reduce, found N`) is now silent by default and only printed to stderr when the `OSMSM_VERBOSE` env var is set.
 - Fixed: `edgedistance`, `sort_ways` did not actually sort ways by proximity.
 - Fixed: empty ways from clipped extracts no longer panic the flatten pipeline; they are skipped and the geometry is reported as `Broken` when nothing usable remains.
 - Fixed: closing a linestring no longer discards the flatten workaround status; the flatten and close steps' workarounds are now unioned (e.g. a sorted ring that also needed joining now reports `JoinedSorted` instead of `Joined`).
 - Added: unit tests pinning the geometry functions (pointdistance, dist_haversine, edgedistance, first_pass, sort_ways, join_ways, close_linestring, flatten_ways).
 - Added: criterion end-to-end benchmark (`cargo bench`) over `tests/test.pbf`, measuring the full ptv2 parse+process pipeline as well as the parse and process phases separately.
 - Added: an `ecuador` benchmark group that runs the ptv2 pipeline over a real ~114 MB country extract, downloaded (via curl/wget) to `ecuador-latest.osm.pbf` on first `cargo bench` and reused afterwards; unlike the tiny `test.pbf`, its node count is large enough to reflect real parsing costs.
 - Fixed: `Debug for Parser` panicked on relations without a `name` tag.
 - Internal: dropped the redundant `RwLock` around the parallel work-index `AtomicUsize` (the atomic is already thread-safe), removing needless lock contention.
 - Refactor: extracter the three near-identical PBF collect passes (relations/ways/nodes) into a shared `collect_from_pbf` scaffold that handles the worker threads, blob dispatch and reduce, leaving each pass to supply only its per-blob processing.
 - Changed: `Parser::get_relation_from_id` now takes `&self` instead of consuming the `Parser`.
 - Changed: filter string is now parsed once into conditions instead of being re-parsed for every relation and way.
 - Changed: merged the near-identical `filter_relation` and `filter_way` into a single `matches_conditions` helper.
 - Refactored: extracted the shared close-linestring loop from `Relation::flatten_ways` and `Way::flatten_ways` into a `close_if_needed` helper.
 - Refactored: deduplicated the parallel worker/reduce scaffold; `get_areas` and `par_map` now share the private `par_map_indexed` helper (no behavior change).
 - Refactored: deduplicated the triplicated node-building code (relation ways, relation stops, way nodes) into a shared `build_nodes` helper.
 - Refactored: extracted the duplicated osm metadata extraction from the relations and ways passes into a single `info_map` helper (no behavior change).

## v2.2.0

Added filtering by negation

 - Added: Filtering accepts negation with `!` before the key.
 - Fixed: new_ptv2 was not getting the public transport relations.

## v2.1.0

Added minor features

 - Added: Filtering accepts multiple valid values separated by commas.
 - Added: Also returning osm_type (r/w/n) together with osm_id in the JSON.
 - Fixed: JSON output is now valid JSON.

## v2.0.0

Added generic filtering

 - Added new CLI
 - Added parameter to filter with any key=value of osm tags
 - Added: Parser::get_areas(tolerance)
 - Modified: struct relation::AdministrativeArea to relation::Area

## v1.3.0 (unreleased)

Added Administrative areas parsing

 - Added: struct relation::AdministrativeArea
 - Added: Parser::get_administrative_areas()
 - Added: Parser::new_aa()

## v1.2.2

Maintenance fix

 - Fixed issue when osm data is buggy. Relation without ways

## v1.2.0

Added info HashMap to expose public transport relation metadata

 - Added: info HashMap attribute to PublicTransport struct

## v1.1.0

Added status struct to know if the parser applied workarounds

 - Added: struct parse_status::ParseStatus
 - Added: parse_status attribute to PublicTransport struct

## v1.0.0

First functional version.

 - Added: struct relation::PublicTransport
 - Added: struct relation::Relation
 - Added: struct Parser
 - Added: struct ParserRelationIterator
