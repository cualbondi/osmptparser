extern crate num_cpus;
use osmptparser::Parser;
use structopt::StructOpt;

/// Openstreetmap areas and public transport (ptv2) parser
#[derive(StructOpt, Debug)]
struct Cli {
    /// Path to the input file to read
    /// must be in OSM PBF format
    #[structopt(parse(from_os_str))]
    filename: std::path::PathBuf,

    /// Filter to use
    /// (mutually exclusive with filter-ptv2)
    /// Example values:
    /// - "natural=beach": only areas wich are beaches
    /// - "name&natural=beach": areas wich are beaches and have a name
    /// - "name&admin_level=1,2,3&boundary=administrative": administrative areas with name and level values of 1 or 2 or 3
    #[structopt(short = "f", long = "filter")]
    filter: String,

    /// get ptv2
    /// (mutually exclusive with filter)
    #[structopt(short = "p", long = "filter-ptv2")]
    filter_ptv2: bool,

    /// Number of cpus to use
    /// Defaults to the number of cpus available
    /// Set to 0 to use all available cpus
    #[structopt(short = "c", long = "cpus", default_value = "0")]
    cpus: usize,

    /// Gap tolerance
    /// When joining ways, the maximum distance between the end of one way and the start of the next way
    ///   to consider them as one continuous way.
    /// Unit: meters
    /// Defaults to 150m
    #[structopt(short = "g", long = "gap", default_value = "150.0")]
    gap: f64,
}

fn main() {
    let args = Cli::from_args();
    let cpus = if args.cpus == 0 {
        num_cpus::get()
    } else {
        args.cpus
    };
    let input_filename = &args.filename.into_os_string().into_string().unwrap();
    if args.filter_ptv2 {
        let parser = Parser::new_ptv2(input_filename, cpus);
        println!("[");
        let mut first = true;
        for pt in parser.get_public_transports(args.gap) {
            if !first {
                println!(",");
            } else {
                first = false;
            }
            print!("  {}", pt.to_geojson());
        }
        print!("]");
    } else {
        let parser = Parser::new(input_filename, cpus, args.filter);
        println!("[");
        let mut first = true;
        for area in parser.get_areas(args.gap) {
            if area.parse_status.code != 0 {
                continue;
            }
            if area.geometry.is_empty() {
                continue;
            }
            if !first {
                println!(",");
            } else {
                first = false;
            }
            print!("  {}", area.to_geojson());
        }
        println!();
        println!("]");
    }
}
