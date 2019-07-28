extern crate osm_pbf_iter;
extern crate num_cpus;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader};
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use std::thread;

use osm_pbf_iter::*;

#[derive(Clone)]
struct NodeData {
    id: u64,
    lat: f64,
    lon: f64,
}

#[derive(Clone)]
struct WayData {
    id: u64,
    nodes: Vec<NodeData>,
}

#[derive(Clone)]
struct RelationData {
    name: String,
    fixed_way: Vec<NodeData>,
    ways: HashMap<u64, WayData>,
    stops: HashMap<u64, NodeData>,
}

type WaysCache = HashMap<u64, Vec<u64>>;
type StopsCache = HashMap<u64, Vec<u64>>;
type NodesCache = HashMap<u64, Vec<u64>>;

#[derive(Clone)]
struct Data {
    pt: HashMap<u64, RelationData>, // { relation_id: { name: name, fixed_way: Vec<LatLon>, ways: { way_id: { name: name, nodes: { node_id: { lat: lat, lng: lng } } } } } }
    ways_cache: WaysCache, // aux structure { way_id: [ relation_id ] }
    stops_cache: StopsCache, // aux structure { node_id: [ relation_id ] }
    nodes_cache: NodesCache, // aux structure { node_id: [ way_id ] }
}

struct MessageWaysTX {
    blob: Blob,
    ways_cache: WaysCache,
}
struct MessageWaysRX {
    ways: HashMap<u64, WayData>,
    nodes_cache: NodesCache,
}

struct MessageNodesTX {
    blob: Blob,
    stops_cache: StopsCache,
    nodes_cache: NodesCache,
}
type MessageNodesRX = HashMap<u64, NodeData>;


// worker which processes one part of the data
fn relations_parser_worker(req_rx: Receiver<Blob>, res_tx: SyncSender<Data>) {
    let routetypes_stops = ["train", "subway", "monorail", "tram", "light_rail"];
    let routetypes_all   = ["train", "subway", "monorail", "tram", "light_rail", "bus", "trolleybus"];
    let wayroles         = ["", "forward", "backward", "alternate"];

    let mut collecteddata = Data {
        pt: HashMap::new(),
        ways_cache: HashMap::new(),
        stops_cache: HashMap::new(),
        nodes_cache: HashMap::new(),
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
                    let routemastertag = relation.tags().find(|&kv| kv.0 == "route_master");
                    let nametag = relation.tags().find(|&kv| kv.0 == "name");
                    if routemastertag == None && routetag != None && routetypes_all.contains(&routetag.unwrap().1) && nametag != None {
                        // condicion para saber si esta relation es un public transport
                        let mut rd = RelationData {
                            name: nametag.unwrap().1.to_string(),
                            ways: HashMap::new(),
                            stops: HashMap::new(),
                            fixed_way: Vec::new(),
                        };
                        for member in relation.members() {
                            // member = (role: &str, id: u64, type: RelationMemberType)
                            if member.2 == RelationMemberType::Way && wayroles.contains(&member.0) {
                                collecteddata.ways_cache
                                    .entry(member.1)
                                    .or_insert_with(Vec::new)
                                    .push(relation.id);
                                // this is wrong, should be a vector, it could be the same way more than once
                                rd.ways.insert(member.1, WayData {
                                    id: member.1,
                                    nodes: Vec::new(),
                                });
                            }
                            if member.2 == RelationMemberType::Node {
                                collecteddata.stops_cache
                                    .entry(member.1)
                                    .or_insert_with(Vec::new)
                                    .push(relation.id);
                                rd.stops.insert(member.1, NodeData {
                                    id: member.1,
                                    lat: 0f64,
                                    lon: 0f64,
                                });
                            }
                        }
                        if rd.ways.len() > 0 {
                            collecteddata.pt.insert(relation.id, rd);
                        }
                        else {
                            println!("WARNING: relation has no ways 'https://www.openstreetmap.org/relation/{:?}'", relation.id);
                        }
                    }
                },
                _ => {}
            }
        }
    }

    res_tx.send(collecteddata).unwrap();
}

// worker which processes one part of the data
fn ways_parser_worker(req_rx: Receiver<MessageWaysTX>, res_tx: SyncSender<MessageWaysRX>) {

    let mut collecteddata = MessageWaysRX {
        ways: HashMap::new(),
        nodes_cache: HashMap::new(),
    };
    loop {
        let message = match req_rx.recv() {
            Ok(message) => message,
            Err(_) => break,
        };

        let ways_cache = message.ways_cache;
        let blob = message.blob.into_data();
        let primitive_block = PrimitiveBlock::parse(&blob);
        for primitive in primitive_block.primitives() {
            match primitive {
                Primitive::Way(way) => {
                    if ways_cache.contains_key(&way.id) {
                        for node in way.refs() {
                            collecteddata.nodes_cache
                                .entry(node as u64)
                                .or_insert_with(Vec::new)
                                .push(way.id);
                        }
                        collecteddata.ways.insert(
                            way.id,
                            WayData {
                                id: way.id,
                                nodes: way.refs().map(|n| NodeData {
                                    id: n as u64,
                                    lat: 0f64,
                                    lon: 0f64,
                                }).collect(),
                            }
                        );
                    }
                },
                _ => {}
            }
        }
    }

    res_tx.send(collecteddata).unwrap();
}

fn main() {

    /*
        CLI options parse
    */

    let pbf_filename_option = std::env::args().skip(1).next();
    if pbf_filename_option == None {
        return print!("Expected filename\n");
    }
    let pbf_filename = pbf_filename_option.unwrap();



    let cpus = num_cpus::get();
    let mut collecteddata = Data {
        pt: HashMap::new(),
        ways_cache: HashMap::new(),
        stops_cache: HashMap::new(),
        nodes_cache: HashMap::new(),
    };

    /*
        pbf relations collect
    */
    {
        let mut workers = Vec::with_capacity(cpus);
        for _ in 0..cpus {
            let (req_tx, req_rx) = sync_channel(2);
            let (res_tx, res_rx) = sync_channel(0);
            workers.push((req_tx, res_rx));
            thread::spawn(move || {
                relations_parser_worker(req_rx, res_tx);
            });
        }

        let f = File::open(&pbf_filename).unwrap();
        let mut reader = BlobReader::new(BufReader::new(f));

        let mut w = 0;
        for blob in &mut reader {
            let req_tx = &workers[w].0;
            w = (w + 1) % cpus;
            req_tx.send(blob).unwrap();
        }

        // reduce / join all data from workers into one structure
        for (req_tx, res_rx) in workers.into_iter() {
            drop(req_tx);
            let worker_collecteddata = res_rx.recv().unwrap();
            collecteddata.pt.extend(worker_collecteddata.pt);
            // for (relation_id, relation_data) in worker_collecteddata.pt.iter() {
            //     collecteddata.pt.entry(*relation_id)
            //         .and_modify(|rd| {
            //             rd.ways.extend(relation_data.ways.clone());
            //             rd.stops.extend(relation_data.stops.clone());
            //         })
            //         .or_insert(relation_data.clone());
            // }
            for (way_id, relation_ids) in worker_collecteddata.ways_cache.iter() {
                collecteddata.ways_cache.entry(*way_id)
                    .or_insert_with(Vec::new)
                    .extend(relation_ids)
            }
            for (way_id, relation_ids) in worker_collecteddata.stops_cache.iter() {
                collecteddata.stops_cache.entry(*way_id)
                    .or_insert_with(Vec::new)
                    .extend(relation_ids)
            }
        }
    }

    /*
        pbf ways collect
    */
    {
        let mut workers = Vec::with_capacity(cpus);
        for _ in 0..cpus {
            let (req_tx, req_rx) = sync_channel(2);
            let (res_tx, res_rx) = sync_channel(0);
            workers.push((req_tx, res_rx));
            thread::spawn(move || {
                ways_parser_worker(req_rx, res_tx);
            });
        }

        let f = File::open(&pbf_filename).unwrap();
        let mut reader = BlobReader::new(BufReader::new(f));

        let mut w = 0;
        for blob in &mut reader {
            let req_tx = &workers[w].0;
            w = (w + 1) % cpus;
            req_tx.send(MessageWaysTX {
                blob: blob,
                ways_cache: collecteddata.ways_cache.clone(),
            }).unwrap();
        }

        // reduce / join all data from workers into one structure
        for (req_tx, res_rx) in workers.into_iter() {
            drop(req_tx);
            let worker_collecteddata = res_rx.recv().unwrap();
            for (node_id, ways_ids) in worker_collecteddata.nodes_cache.iter() {
                collecteddata.nodes_cache.entry(*node_id)
                    .or_insert_with(Vec::new)
                    .extend(ways_ids)
            }
            for (way_id, way_data) in worker_collecteddata.ways.iter() {
                for (_, v) in collecteddata.pt.iter_mut() {
                    v.ways.entry(*way_id)
                        .and_modify(|w| w.nodes = way_data.nodes.clone());
                }
            }
        }
    }

    let mut count = 0;
    for (key, value) in collecteddata.pt.iter() {
        count += 1;
        let ways = value.ways.iter().count();
        let stops = value.stops.iter().count();
        let mut nodes: Vec<u64> = value.ways
            .iter()
            .map(|w| {
                let ns: Vec<u64> = w.1.nodes.iter().map(|n| n.id).collect();
                ns
            })
            .flatten()
            .collect();
        nodes.sort_unstable();
        nodes.dedup();
        let nodes_count = nodes.iter().count();
        print!("{:?}: ways {:?}, stops {:?}, nodes {:?}, {:?}\n", key, ways, stops, nodes_count, value.name);
    }
    print!("\nFound {:?} relations\n", count);

}
