extern crate osm_pbf_iter;
extern crate num_cpus;

use std::process;

use std::collections::HashMap;
use std::env::args;
use std::fs::File;
use std::io::{Seek, SeekFrom, BufReader};
use std::time::Instant;
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use std::thread;

use osm_pbf_iter::*;

struct Data {
    nodeslat: HashMap<u64, f64>,
    nodeslon: HashMap<u64, f64>,
}

fn blobs_worker(req_rx: Receiver<Blob>, res_tx: SyncSender<Data>) {
    let mut collecteddata = Data {
        nodeslat: HashMap::new(),
        nodeslon: HashMap::new(),
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
                Primitive::Node(node) => {
                    collecteddata.nodeslat.insert(node.id, node.lat);
                    collecteddata.nodeslon.insert(node.id, node.lon);
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

        println!("Open {}", arg);
        let f = File::open(&arg).unwrap();
        let mut reader = BlobReader::new(BufReader::new(f));
        let start = Instant::now();

        let mut w = 0;
        for blob in &mut reader {
            let req_tx = &workers[w].0;
            w = (w + 1) % cpus;

            req_tx.send(blob).unwrap();
        }


        let mut collecteddata = Data {
            nodeslat: HashMap::new(),
            nodeslon: HashMap::new(),
        };
        for (req_tx, res_rx) in workers.into_iter() {
            drop(req_tx);
            let worker_collecteddata = res_rx.recv().unwrap();
            collecteddata.nodeslat.extend(worker_collecteddata.nodeslat);
            collecteddata.nodeslon.extend(worker_collecteddata.nodeslon);
        }

        let mut avglat = 0.0;
        let mut count = 0;
        for (key, value) in collecteddata.nodeslat.iter() {
            count += 1;
            avglat += value;
        }
        print!("avg lat: {:?}", avglat / count as f64);

        let mut avglon = 0.0;
        let mut count = 0;
        for (key, value) in collecteddata.nodeslon.iter() {
            count += 1;
            avglon += value;
        }
        print!("avg lon: {:?}", avglon / count as f64);



        let stop = Instant::now();
        let duration = stop.duration_since(start);
        let duration = duration.as_secs() as f64 + (duration.subsec_nanos() as f64 / 1e9);
        let mut f = reader.to_inner();
        match f.seek(SeekFrom::Current(0)) {
            Ok(pos) => {
                let rate = pos as f64 / 1024f64 / 1024f64 / duration;
                println!("Processed {} MB in {:.2} seconds ({:.2} MB/s)",
                         pos / 1024 / 1024, duration, rate);
            },
            Err(_) => (),
        }

        // println!("{} - {} nodes, {} ways, {} relations", arg, stats[0], stats[1], stats[2]);
    }
}


// extern crate pbf_reader;
// use pbf_reader::*;

// use std::collections::HashMap;
// use std::thread;
// use std::sync::mpsc;

// #[derive(Debug)]
// struct MapData {
//     nodes: HashMap<IDType, NodeF>,
//     ways: HashMap<IDType, Way>,
//     strings: HashMap<IDType, Strings>,
// }

// fn main() {

//     let mut map_data = MapData {
//         nodes: HashMap::with_capacity(900),
//         ways: HashMap::with_capacity(90),
//         strings: HashMap::with_capacity(5),
//     };

//     let (mut node_tx, node_rx) = mpsc::channel::<PBFData>();

//     let h = thread::spawn(move || {
//         return read_pbf(&("./ecuador-latest.osm.pbf".to_string()), 8, &mut node_tx);
//     });

//     let mut count = 0;
//     let mut info = Vec::<PbfInfo>::new();

//     loop {
//         match node_rx.recv().unwrap() {
//             PBFData::NodesSet(set) => {
//                 for (id, node) in set {
//                     count = count + 1;

//                     // if max.lat < node.coord.lat || count == 1 {
//                     //     max.lat = node.coord.lat
//                     // }
//                     // if max.lon < node.coord.lon || count == 1 {
//                     //     max.lon = node.coord.lon
//                     // }
//                     // if min.lat > node.coord.lat || count == 1 {
//                     //     min.lat = node.coord.lat
//                     // }
//                     // if min.lon > node.coord.lon || count == 1 {
//                     //     min.lon = node.coord.lon
//                     // }
//                     // println!("node {:?}", node);
//                     match map_data.nodes.insert(id, node) {
//                         Some(onode) => {
//                             // println!("W node id {} exists!!!! {:?} ", id, onode);
//                         }
//                         None => {}
//                     }
//                 }
//             }
//             PBFData::WaysSet(set) => {
//                 for (id, way) in set {
//                     // println!("got way {:?}", way );
//                     match map_data.ways.insert(id, way) {
//                         Some(oway) => {
//                             // println!("W node id {} exists!!!! {:?} ", id, oway);
//                         }
//                         None => {}
//                     }
//                 }
//             }
//             PBFData::RelationsSet(_) => {}
//             PBFData::Strings(id, strings) => match map_data.strings.insert(id, strings) {
//                 Some(ostrings) => {
//                     println!("Error: strings id {} exists!!!! {:?} ", id, ostrings);
//                 }
//                 None => {
//                     // println!("got strings {}", id);
//                 }
//             },
//             PBFData::PbfInfo(inf) => {
//                 // println!("BBox '{:?}'", inf);
//                 info.push(inf);
//             }

//             PBFData::ParseEnd => {
//                 break;
//             }
//         }
//     }

//     let r = h.join().unwrap();
//     println!("DONE!!!! {:?}", r);
//     println!("count {:?}", count);


// }
