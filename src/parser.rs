extern crate osm_pbf_iter;
extern crate fxhash;

use std::fs::File;
use std::io::{BufReader};
use std::sync::mpsc::{sync_channel};
use std::thread;
use std::sync::{Arc, RwLock};
use std::fmt;

use osm_pbf_iter::{PrimitiveBlock, Blob, Primitive, RelationMemberType, BlobReader};
use fxhash::FxHashSet;
use fxhash::FxHashMap;

#[derive(Clone, Debug)]
struct NodeData {
    id: u64,
    lat: f64,
    lon: f64,
    tags: FxHashMap<String, String>,
}

#[derive(Clone, Debug)]
struct WayData {
    id: u64,
    tags: FxHashMap<String, String>,
    nodes: Vec<u64>,
}

#[derive(Clone, Debug)]
struct RelationData {
    id: u64,
    tags: FxHashMap<String, String>,
    ways: Vec<u64>,
    stops: Vec<u64>,
}

type WayIdsSet = FxHashSet<u64>;
type NodeIdsSet = FxHashSet<u64>;

struct MessageRelations {
    relations: Vec<RelationData>,
    stop_ids: NodeIdsSet,
    way_ids: WayIdsSet,
}

struct MessageWays {
    ways: FxHashMap<u64, WayData>,
    node_ids: NodeIdsSet,
}

struct MessageNodes {
    nodes: FxHashMap<u64, NodeData>,
}

pub struct Parser {
    relations: Vec<RelationData>,
    ways: FxHashMap<u64, WayData>,
    nodes: FxHashMap<u64, NodeData>,
    iter_index: usize,
}

#[derive(Clone, Debug)]
struct Node {
    id: u64,
    tags: FxHashMap<String, String>,
    lat: f64,
    lon: f64,
}

#[derive(Clone, Debug)]
struct Way {
    id: u64,
    tags: FxHashMap<String, String>,
    nodes: Vec<Node>,
}

#[derive(Clone, Debug)]
pub struct Relation {
    id: u64,
    tags: FxHashMap<String, String>,
    ways: Vec<Way>,
    stops: Vec<Node>,
}

impl Parser {

    pub fn new(pbf_filename: &str, cpus: usize) -> Self {

        let mut relations = Vec::new() as Vec<RelationData>;
        let mut ways = FxHashMap::default() as FxHashMap<u64, WayData>;
        let mut nodes = FxHashMap::default() as FxHashMap<u64, NodeData>;
        let way_ids = Arc::new(RwLock::new(FxHashSet::default() as WayIdsSet));
        let node_ids = Arc::new(RwLock::new(FxHashSet::default() as NodeIdsSet));

        /*
            pbf relations collect
        */
        {
            print!("START relations map\n");
            let mut workers = Vec::with_capacity(cpus);
            for _ in 0..cpus {
                let (req_tx, req_rx) = sync_channel(2);
                let (res_tx, res_rx) = sync_channel(0);
                workers.push((req_tx, res_rx));
                thread::spawn(move || {
                    // relations_parser_worker(req_rx, res_tx);
                    // let routetypes_stops = ["train", "subway", "monorail", "tram", "light_rail"];
                    let routetypes_all   = ["train", "subway", "monorail", "tram", "light_rail", "bus", "trolleybus"];
                    let wayroles         = ["", "forward", "backward", "alternate"];

                    let mut relations = Vec::new() as Vec<RelationData>;
                    let mut stop_ids = FxHashSet::default() as NodeIdsSet;
                    let mut way_ids = FxHashSet::default() as WayIdsSet;
                    loop {
                        let blob = match req_rx.recv() {
                            Ok(blob) => blob,
                            Err(_) => break,
                        };

                        let data = (blob as Blob).into_data();
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
                                            id: relation.id,
                                            tags: relation.tags().map(|t| (t.0.to_string(), t.1.to_string())).collect(),
                                            ways: Vec::new(),
                                            stops: Vec::new(),
                                        };
                                        for member in relation.members() {
                                            // member = (role: &str, id: u64, type: RelationMemberType)
                                            if member.2 == RelationMemberType::Way && wayroles.contains(&member.0) {
                                                rd.ways.push(member.1);
                                                way_ids.insert(member.1);
                                            }
                                            if member.2 == RelationMemberType::Node {
                                                rd.stops.push(member.1);
                                                stop_ids.insert(member.1);
                                            }
                                        }
                                        if rd.ways.len() > 0 {
                                            relations.push(rd);
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

                    res_tx.send(MessageRelations {
                        relations,
                        stop_ids,
                        way_ids,
                    }).unwrap();

                });
            }

            let f = File::open(pbf_filename).unwrap();
            let mut reader = BlobReader::new(BufReader::new(f));

            let mut w = 0;
            for blob in &mut reader {
                let req_tx = &workers[w].0;
                w = (w + 1) % cpus;
                req_tx.send(blob).unwrap();
            }

            print!("START relations reduce\n");
            // reduce / join all data from workers into one structure
            {
                // write lock
                let mut node_ids_write = node_ids.write().unwrap();
                let mut way_ids_write = way_ids.write().unwrap();
                for (req_tx, res_rx) in workers.into_iter() {
                    drop(req_tx);
                    let worker_data = res_rx.recv().unwrap();
                    relations.extend(worker_data.relations);
                    node_ids_write.extend(worker_data.stop_ids);
                    way_ids_write.extend(worker_data.way_ids);
                }
            } // write lock
        }

        /*
            pbf ways collect
        */
        {
            print!("START ways map\n");
            let mut workers = Vec::with_capacity(cpus);
            for _ in 0..cpus {
                let (req_tx, req_rx) = sync_channel(2);
                let (res_tx, res_rx) = sync_channel(0);
                workers.push((req_tx, res_rx));
                let way_ids_local = way_ids.clone();
                thread::spawn(move || {

                    let mut ways = FxHashMap::default() as FxHashMap<u64, WayData>;
                    let mut node_ids = FxHashSet::default() as NodeIdsSet;
                    let way_ids_read = way_ids_local.read().unwrap();
                    loop {
                        let blob = match req_rx.recv() {
                            Ok(blob) => blob,
                            Err(_) => break,
                        };

                        let blob = (blob as Blob).into_data();
                        let primitive_block = PrimitiveBlock::parse(&blob);
                        for primitive in primitive_block.primitives() {
                            match primitive {
                                Primitive::Way(way) => {
                                    if way_ids_read.contains(&way.id) {
                                        for node in way.refs() {
                                            node_ids.insert(node as u64);
                                        }
                                        ways.insert(
                                            way.id,
                                            WayData {
                                                id: way.id,
                                                tags: way.tags().map(|t| (t.0.to_string(), t.1.to_string())).collect(),
                                                nodes: way.refs().map(|id| id as u64).collect(),
                                            }
                                        );
                                    }
                                },
                                _ => {}
                            }
                        }
                    }

                    res_tx.send(MessageWays {
                        ways,
                        node_ids,
                    }).unwrap();
                });
            }

            let f = File::open(pbf_filename).unwrap();
            let mut reader = BlobReader::new(BufReader::new(f));

            let mut w = 0;
            for blob in &mut reader {
                let req_tx = &workers[w].0;
                w = (w + 1) % cpus;
                req_tx.send(blob).unwrap();
            }

            print!("START ways reduce\n");
            // reduce / join all data from workers into one structure
            {
                let mut node_ids_write = node_ids.write().unwrap();
                for (req_tx, res_rx) in workers.into_iter() {
                    drop(req_tx);
                    let worker_data = res_rx.recv().unwrap();
                    ways.extend(worker_data.ways);
                    node_ids_write.extend(worker_data.node_ids);
                }
            } // write lock
        }


        /*
            pbf nodes collect
        */
        {
            print!("START nodes map\n");
            let mut workers = Vec::with_capacity(cpus);
            for _ in 0..cpus {
                let (req_tx, req_rx) = sync_channel(2);
                let (res_tx, res_rx) = sync_channel(0);
                workers.push((req_tx, res_rx));
                let node_ids_local = node_ids.clone();
                thread::spawn(move || {
                    // nodes_parser_worker(req_rx, res_tx);
                    let node_ids_read = node_ids_local.read().unwrap();
                    let mut nodes = FxHashMap::default() as FxHashMap<u64, NodeData>;
                    loop {
                        let blob = match req_rx.recv() {
                            Ok(blob) => blob,
                            Err(_) => break,
                        };

                        let blob = (blob as Blob).into_data();
                        let primitive_block = PrimitiveBlock::parse(&blob);
                        for primitive in primitive_block.primitives() {
                            match primitive {
                                Primitive::Node(node) => {
                                    if node_ids_read.contains(&node.id) {
                                        nodes.insert(
                                            node.id,
                                            NodeData {
                                                id: node.id,
                                                tags: node.tags.into_iter().map(|t| (t.0.to_string(), t.1.to_string())).collect(),
                                                lat: node.lat,
                                                lon: node.lon,
                                            }
                                        );
                                    }
                                },
                                _ => {}
                            }
                        }
                    }

                    res_tx.send(MessageNodes {
                        nodes,
                    }).unwrap();
                });
            }

            let f = File::open(pbf_filename).unwrap();
            let mut reader = BlobReader::new(BufReader::new(f));

            let mut w = 0;
            for blob in &mut reader {
                let req_tx = &workers[w].0;
                w = (w + 1) % cpus;
                req_tx.send(blob).unwrap();
            }

            print!("START nodes reduce\n");
            // reduce / join all data from workers into one structure
            {
                for (req_tx, res_rx) in workers.into_iter() {
                    drop(req_tx);
                    let worker_data = res_rx.recv().unwrap();
                    nodes.extend(worker_data.nodes);
                }
            } // write lock
        } // local vars block


        Parser {
            relations,
            ways,
            nodes,
            iter_index: 0usize,
        }
    }

}

impl fmt::Debug for Parser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Preparing to print\n")?;
        let mut count = 0;
        for relation in self.relations.clone() {
            count += 1;
            let ways_count = relation.ways.len();
            let stops_count = relation.stops.len();
            let nodes_count = relation.ways
                .iter()
                .map(|wid| {
                    match self.ways.get(wid) {
                        Some(way) => way.nodes.len(),
                        None => 0
                    }
                })
                .fold(0, |a, b| a + b);
            write!(f, "{:?}: ways {:?}, stops {:?}, nodes {:?}, {:?}\n", relation.id, ways_count, stops_count, nodes_count, relation.tags["name"])?;
        }
        write!(f, "\nFound {:?} relations\n", count)?;
        Ok(())
    }
}

impl Iterator for Parser {
    type Item = Relation;

    fn next(&mut self) -> Option<Relation> {
        if self.iter_index >= self.relations.len() {
            None
        }
        else {
            let relation_data = &self.relations[self.iter_index];
            let relation = Relation {
                id: relation_data.id,
                tags: relation_data.tags.clone(),
                ways: relation_data.ways.iter().map(|wid| Way {
                    id: *wid,
                    tags: self.ways[&wid].tags.clone(),
                    nodes: self.ways[&wid].nodes.iter().map(|nid| Node {
                        id: *nid,
                        tags: self.nodes[&nid].tags.clone(),
                        lat: self.nodes[&nid].lat,
                        lon: self.nodes[&nid].lon,
                    }).collect(),
                }).collect(),
                stops: relation_data.stops.iter().map(|nid| Node {
                    id: *nid,
                    tags: self.nodes[&nid].tags.clone(),
                    lat: self.nodes[&nid].lat,
                    lon: self.nodes[&nid].lon,
                }).collect(),
            };
            // TODO: call fix_ways here
            // TODO: implement fix_ways within the iterator maybe?, maybe another struct is needed to represent a vec of ways that implements fix() or maybe a better name flatten() or flattenfix()
            self.iter_index += 1usize;
            Some(relation)
        }
    }
}
