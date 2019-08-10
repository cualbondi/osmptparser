extern crate num_cpus;
use osmptparser::Parser;

fn main() {
    let pbf_filename_option = std::env::args().skip(1).next();
    if pbf_filename_option == None {
        return print!("Expected filename\n");
    }
    let pbf_filename = pbf_filename_option.unwrap();

    let nthreads = num_cpus::get();
    let parser = Parser::new(&pbf_filename, nthreads);

    let mut accum = 0usize;
    let v1 = parser.get_public_transports();
    for _ in v1 {
        accum += 1;
    }

    // OPTION2:
    // let v2 = parser.par_map(|r| r.flatten_ways(150_f64).unwrap());

    print!("OKs = {:?}\n", accum);
}
