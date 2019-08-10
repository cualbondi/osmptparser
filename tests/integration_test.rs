use osmptparser::Parser;

#[test]
fn count_ok() {
    let parser = Parser::new("tests/test.pbf", 2);
    let len = parser.iter().count();
    assert_eq!(2, len);
}
