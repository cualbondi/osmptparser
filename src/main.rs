extern crate osm_pbf_iter;
extern crate num_cpus;

use std::collections::HashMap;
use std::env::args;
use std::fs::File;
use std::io::{BufReader};
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

        let f = File::open(&arg).unwrap();
        let mut reader = BlobReader::new(BufReader::new(f));

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
        for (_key, value) in collecteddata.nodeslat.iter() {
            count += 1;
            avglat += value;
        }
        print!("avg lat: {:?}", avglat / count as f64);

        let mut avglon = 0.0;
        let mut count = 0;
        for (_key, value) in collecteddata.nodeslon.iter() {
            count += 1;
            avglon += value;
        }
        print!("avg lon: {:?}", avglon / count as f64);

    }
}
