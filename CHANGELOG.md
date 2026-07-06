## Unreleased

Bugfix and test coverage

 - **BREAKING**: `Parser::new`, `Parser::new_ptv2` and `Parser::new_aa` now return `std::io::Result<Self>` (previously it returned `Self` and panicked).
 - Fixed: `edgedistance`, `sort_ways` did not actually sort ways by proximity.
 - Added: unit tests pinning the geometry functions (pointdistance, dist_haversine, edgedistance, first_pass, sort_ways, join_ways, close_linestring, flatten_ways).
 - Fixed: `Debug for Parser` panicked on relations without a `name` tag.
 - Changed: `Parser::get_relation_from_id` now takes `&self` instead of consuming the `Parser`.
 - Changed: filter string is now parsed once into conditions instead of being re-parsed for every relation and way.

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
