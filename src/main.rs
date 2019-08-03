extern crate num_cpus;
mod parser;

fn main() {
    let pbf_filename_option = std::env::args().skip(1).next();
    if pbf_filename_option == None {
        return print!("Expected filename\n");
    }
    let pbf_filename = pbf_filename_option.unwrap();

    let parser = parser::Parser::new(&pbf_filename, num_cpus::get());

    for p in parser {
        print!("{:?}", p);
    }
}
