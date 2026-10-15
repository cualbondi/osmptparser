use osmptparser::{Area, ParseStatus, Parser};

#[test]
fn get_public_transports() {
    let parser = Parser::new_ptv2("tests/test.pbf", 1).unwrap();
    let ptsvec = parser.get_public_transports(1500_f64);
    let mut pts = ptsvec.iter().collect::<Vec<_>>();
    pts.sort_by_key(|a| a.id);
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0].id, 85965);
    assert_eq!(pts[0].tags["name"], "Trolebus Quitumbe => La Y");
    assert_eq!(pts[0].info["version"], "226");
    assert_eq!(pts[0].info["timestamp"], "1552883955");
    assert_eq!(pts[0].stops.len(), 31);
    assert_eq!(pts[1].id, 2030162);
    assert_eq!(pts[1].tags["name"], "B6 Mapasingue Oeste Ida");
    assert_eq!(pts[1].info["version"], "13");
    assert_eq!(pts[1].info["timestamp"], "1555013271");
    assert_eq!(pts[1].stops.len(), 1);
}

#[test]
fn get_relation_from_id() {
    let parser = Parser::new_ptv2("tests/test.pbf", 1).unwrap();
    let relation = parser.get_relation_from_id(85965).unwrap();
    assert_eq!(relation.id, 85965);
    assert_eq!(relation.tags["name"], "Trolebus Quitumbe => La Y");
    // an id that was not parsed returns None instead of panicking
    assert!(parser.get_relation_from_id(1).is_none());
}

/// pbf coordinates are stored with 1e-7 precision
fn has_point(ring: &[(f64, f64)], p: (f64, f64)) -> bool {
    ring.iter()
        .any(|c| (c.0 - p.0).abs() < 1e-6 && (c.1 - p.1).abs() < 1e-6)
}

fn area(areas: &[Area], id_type: char, id: u64) -> &Area {
    areas
        .iter()
        .find(|a| a.id_type == id_type && a.id == id)
        .unwrap_or_else(|| panic!("area {}{} not found", id_type, id))
}

#[test]
fn get_areas_assembles_multipolygons_from_outer_and_inner_members() {
    let parser = Parser::new_aa("tests/areas.pbf", 1).unwrap();
    let mut areas = parser.get_areas(150.0);
    areas.sort_by_key(|a| (a.id_type, a.id));
    // 3 relations + 1 standalone closed way; the bus route is filtered out
    assert_eq!(
        areas.iter().map(|a| (a.id_type, a.id)).collect::<Vec<_>>(),
        vec![('r', 100), ('r', 200), ('r', 300), ('w', 6)]
    );

    // province: outer ring split in two ways given out of order + one hole
    let province = area(&areas, 'r', 100);
    assert_eq!(province.tags["name"], "Province");
    assert_eq!(province.tags["admin_level"], "4");
    assert_eq!(province.parse_status, ParseStatus::Ok);
    assert_eq!(province.geometry.len(), 1);
    assert_eq!(province.geometry[0].len(), 2);
    let outer = &province.geometry[0][0];
    assert_eq!(outer.len(), 5);
    assert_eq!(outer[0], outer[4]);
    assert!(has_point(outer, (0.0, 0.0)) && has_point(outer, (1.0, 1.0)));
    let hole = &province.geometry[0][1];
    assert_eq!(hole.len(), 5);
    assert!(has_point(hole, (0.4, 0.4)) && has_point(hole, (0.6, 0.6)));

    // town: two closed outer ways -> two polygons
    let town = area(&areas, 'r', 200);
    assert_eq!(town.parse_status, ParseStatus::Ok);
    assert_eq!(town.geometry.len(), 2);
    assert!(town
        .geometry
        .iter()
        .all(|p| p.len() == 1 && p[0].len() == 5));

    // broken: outer way that cannot be closed
    let broken = area(&areas, 'r', 300);
    assert_eq!(broken.parse_status, ParseStatus::Broken);
    assert!(broken.geometry.is_empty());

    // village: standalone closed way tagged as administrative boundary
    let village = area(&areas, 'w', 6);
    assert_eq!(village.tags["name"], "Village");
    assert_eq!(village.parse_status, ParseStatus::Ok);
    assert_eq!(village.geometry.len(), 1);
    assert_eq!(village.geometry[0][0].len(), 5);
}

#[test]
fn relation_members_keep_their_roles() {
    let parser = Parser::new_aa("tests/areas.pbf", 1).unwrap();
    let province = parser.get_relation_from_id(100).unwrap();
    assert_eq!(
        province
            .ways
            .iter()
            .map(|w| (w.id, w.role.as_str()))
            .collect::<Vec<_>>(),
        vec![(2, "outer"), (3, "inner"), (1, "outer")]
    );

    let parser = Parser::new_ptv2("tests/areas.pbf", 1).unwrap();
    let bus = parser.get_relation_from_id(400).unwrap();
    assert_eq!(bus.stops.len(), 1);
    assert_eq!(bus.stops[0].role, "stop");
    assert_eq!(bus.ways[0].role, "");
}
