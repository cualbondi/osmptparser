use osmptparser::Parser;

#[test]
fn count_ok() {
    let parser = Parser::new("tests/test.pbf", 2);
    let len = parser.iter().count();
    assert_eq!(2, len);
}

#[test]
fn get_public_transports() {
    let parser = Parser::new("tests/test.pbf", 2);
    let pts = parser.get_public_transports(1500_f64);
    let mut pts_iter = pts.iter();
    // TODO: fix, this might not be in order!
    let first = pts_iter.next().unwrap();
    assert_eq!(first.id, 85965);
    let second = pts_iter.next().unwrap();
    assert_eq!(second.id, 2030162);
}
