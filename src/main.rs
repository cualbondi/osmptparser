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
}

struct Data {
    pt: HashMap<u64, RelationData>, // { relation_id: { name: name, fixed_way: Vec<LatLon>, ways: { way_id: { name: name, nodes: { node_id: { lat: lat, lng: lng } } } } } }
    ways: HashMap<u64, Vec<u64>>, // aux structure { way_id: [ relation_id ] }
    stops: HashMap<u64, Vec<u64>>, // aux structure { node_id: [ relation_id ] }
}

// worker which processes one part of the data
fn blobs_worker(req_rx: Receiver<Blob>, res_tx: SyncSender<Data>) {
    let routetypes_stops = ["train", "subway", "monorail", "tram", "light_rail"];
    let routetypes_all   = ["train", "subway", "monorail", "tram", "light_rail", "bus", "trolleybus"];

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
                    // relation.members()
                    let routetag = relation.tags().find(|&kv| kv.0 == "route");
                    let nametag = relation.tags().find(|&kv| kv.0 == "name");
                    if routetag != None && routetypes_all.contains(&routetag.unwrap().1) && nametag != None {
                        // condicion para saber si esta relation es un public transport
                        collecteddata.pt.insert(relation.id, RelationData {
                            name: nametag.unwrap().1.to_string(),
                            ways: HashMap::new(),
                            fixed_way: Vec::new(),
                        });
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
                blobs_worker(req_rx, res_tx);
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
            // TODO: these two are wrong, we need to merge the Vecs for the same key
            // collecteddata.ways.extend(worker_collecteddata.ways);
            // collecteddata.stops.extend(worker_collecteddata.stops);
        }

        let mut count = 0;
        for (key, value) in collecteddata.pt.iter() {
            count += 1;
            print!("{:?}: {:?}\n", key, value.name);
        }
        print!("\nFound {:?} relations\n", count);

    }
}
