use osmptparser::Parser;

#[test]
fn get_public_transports() {
    let parser = Parser::new_ptv2("tests/test.pbf", 1);
    let ptsvec = parser.get_public_transports(1500_f64);
    let mut pts = ptsvec.iter().collect::<Vec<_>>();
    pts.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(pts[0].id, 85965);
    assert_eq!(pts[0].tags["name"], "Trolebus Quitumbe => La Y");
    assert_eq!(pts[0].info["version"], "226");
    assert_eq!(pts[0].info["timestamp"], "1552883955");
    assert_eq!(pts[0].stops.iter().count(), 31);
    assert_eq!(pts[1].id, 2030162);
    assert_eq!(pts[1].tags["name"], "B6 Mapasingue Oeste Ida");
    assert_eq!(pts[1].info["version"], "13");
    assert_eq!(pts[1].info["timestamp"], "1555013271");
    assert_eq!(pts[1].stops.iter().count(), 1);
}
