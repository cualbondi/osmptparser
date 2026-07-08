use osmptparser::Parser;

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
