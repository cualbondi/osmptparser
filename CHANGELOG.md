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
