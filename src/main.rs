extern crate num_cpus;
extern crate rayon;
use rayon::prelude::*;
mod parser;
use parser::relation::Relation;

fn main() {
    let pbf_filename_option = std::env::args().skip(1).next();
    if pbf_filename_option == None {
        return print!("Expected filename\n");
    }
    let pbf_filename = pbf_filename_option.unwrap();

    let nthreads = num_cpus::get();
    let parser = parser::Parser::new(&pbf_filename, nthreads);

    //let accum = 0usize;
    let rels: Vec<Relation> = parser.iter().collect();
    let accum = rels.par_iter()
        .fold(|| 0, |mut a, rel| {
            // accum += rel.ways.iter().fold(0f64, |_, way| {
            //     way.nodes.iter().fold(0f64, |c2, node| c2 + node.lat) / (way.nodes.len() as f64)
            // });
            if rel.flatten_ways(0f64).unwrap().len() == 1 {
                a += 1;
                // print!("OK {:?}\n", rel.id);
            }
            a
        })
        .reduce(|| 0, |mut a, b| {
            a += b;
            a
        });

    print!("OKs = {:?}\n", accum);
}
