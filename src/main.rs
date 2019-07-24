extern crate osm_pbf_iter;
extern crate num_cpus;

use std::collections::HashMap;
use std::env::args;
use std::fs::File;
use std::io::{BufReader};
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use std::thread;

use osm_pbf_iter::*;

struct NodeData {
    lat: f64,
    lon: f64,
}

struct WayData {
    nodes: HashMap<u64, NodeData>,
}

struct RelationData {
    name: String,
    fixed_way: Vec<NodeData>,
    ways: HashMap<u64, WayData>,
    stops: HashMap<u64, NodeData>,
}

struct Data {
    pt: HashMap<u64, RelationData>, // { relation_id: { name: name, fixed_way: Vec<LatLon>, ways: { way_id: { name: name, nodes: { node_id: { lat: lat, lng: lng } } } } } }
    ways: HashMap<u64, Vec<u64>>, // aux structure { way_id: [ relation_id ] }
    stops: HashMap<u64, Vec<u64>>, // aux structure { node_id: [ relation_id ] }
}

// worker which processes one part of the data
fn relations_parser_worker(req_rx: Receiver<Blob>, res_tx: SyncSender<Data>) {
    let routetypes_stops = ["train", "subway", "monorail", "tram", "light_rail"];
    let routetypes_all   = ["train", "subway", "monorail", "tram", "light_rail", "bus", "trolleybus"];
    let wayroles         = ["", "forward", "backward", "alternate"];

    let mut collecteddata = Data {
        pt: HashMap::new(),
        ways: HashMap::new(),
        stops: HashMap::new(),
    };
    loop {
        let blob = match req_rx.recv() {
            Ok(blob) => blob,
            Err(_) => break,
        };

        let data = blob.into_data();
        let primitive_block = PrimitiveBlock::parse(&data);
        for primitive in primitive_block.primitives() {
            match primitive {
                Primitive::Relation(relation) => {
                    let routetag = relation.tags().find(|&kv| kv.0 == "route");
                    let nametag = relation.tags().find(|&kv| kv.0 == "name");
                    if routetag != None && routetypes_all.contains(&routetag.unwrap().1) && nametag != None {
                        // condicion para saber si esta relation es un public transport
                        let mut rd = RelationData {
                            name: nametag.unwrap().1.to_string(),
                            ways: HashMap::new(),
                            stops: HashMap::new(),
                            fixed_way: Vec::new(),
                        };
                        for member in relation.members() {
                            // member = (role: &str, id: u64, type: RelationMemberType)
                            // print!("'{:?}' - '{:?}'\n", wayroles, member.0);
                            if member.2 == RelationMemberType::Way { // && wayroles.contains(&member.0) { // TODO: bug in the library maybe? see https://github.com/astro/rust-osm-pbf-iter/issues/2
                                collecteddata.ways
                                    .entry(member.1)
                                    .or_insert_with(Vec::new)
                                    .push(relation.id);
                                rd.ways.insert(member.1, WayData {
                                    nodes: HashMap::new(),
                                });
                            }
                            if member.2 == RelationMemberType::Node {
                                collecteddata.stops
                                    .entry(member.1)
                                    .or_insert_with(Vec::new)
                                    .push(relation.id);
                                rd.stops.insert(member.1, NodeData {
                                    lat: 0.0,
                                    lon: 0.0,
                                });
                            }
                        }
                        collecteddata.pt.insert(relation.id, rd);
                    }
                },
                _ => {}
            }
        }
    }

    res_tx.send(collecteddata).unwrap();
}

fn main() {
    let cpus = num_cpus::get();

    for arg in args().skip(1) {
        let mut workers = Vec::with_capacity(cpus);
        for _ in 0..cpus {
            let (req_tx, req_rx) = sync_channel(2);
            let (res_tx, res_rx) = sync_channel(0);
            workers.push((req_tx, res_rx));
            thread::spawn(move || {
                relations_parser_worker(req_rx, res_tx);
            });
        }

        let f = File::open(&arg).unwrap();
        let mut reader = BlobReader::new(BufReader::new(f));

        let mut w = 0;
        for blob in &mut reader {
            let req_tx = &workers[w].0;
            w = (w + 1) % cpus;

            req_tx.send(blob).unwrap();
        }

        // reduce / join all data from workers into one structure
        let mut collecteddata = Data {
            pt: HashMap::new(),
            ways: HashMap::new(),
            stops: HashMap::new(),
        };
        for (req_tx, res_rx) in workers.into_iter() {
            drop(req_tx);
            let worker_collecteddata = res_rx.recv().unwrap();
            collecteddata.pt.extend(worker_collecteddata.pt);
            for (way_id, relation_ids) in worker_collecteddata.ways.iter() {
                collecteddata.ways.entry(*way_id)
                    .or_insert_with(Vec::new)
                    .extend(relation_ids)
            }
            for (way_id, relation_ids) in worker_collecteddata.stops.iter() {
                collecteddata.stops.entry(*way_id)
                    .or_insert_with(Vec::new)
                    .extend(relation_ids)
            }
        }

        let mut count = 0;
        for (key, value) in collecteddata.pt.iter() {
            count += 1;
            let ways = value.ways.iter().count();
            let stops = value.stops.iter().count();
            print!("{:?}: ways {:?}, stops {:?}, {:?}\n", key, ways, stops, value.name);
        }
        print!("\nFound {:?} relations\n", count);

    }
}
