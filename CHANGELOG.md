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
