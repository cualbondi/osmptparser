extern crate num_cpus;
use osmptparser::Parser;

fn main() {
    let pbf_filename_option = std::env::args().nth(1);
    if pbf_filename_option.is_none() {
        return println!("Expected filename");
    }
    let pbf_filename = pbf_filename_option.unwrap();

    let nthreads = num_cpus::get();
    let parser = Parser::new(&pbf_filename, nthreads, "natural=beach".to_string())
        .expect("could not open/parse pbf file");
    // let parser = Parser::new_aa(&pbf_filename, nthreads);

    let mut accum = 0usize;
    let v1 = parser.get_areas(150_f64);
    for _ in v1 {
        accum += 1;
    }

    // OPTION2:
    // let v2 = parser.par_map(|r| r.flatten_ways(150_f64).unwrap());

    println!("OKs = {:?}", accum);
}
