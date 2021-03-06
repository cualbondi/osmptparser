extern crate crossbeam;
extern crate osm_pbf_iter;
pub mod parse_status;
pub mod relation;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, RwLock};
use std::thread;

use osm_pbf_iter::{Blob, BlobReader, Primitive, PrimitiveBlock, RelationMemberType};

use relation::{Node, PublicTransport, Relation, Way};

#[derive(Clone, Debug)]
struct NodeData {
    id: u64,
    lat: f64,
    lon: f64,
    tags: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct WayData {
    id: u64,
    tags: HashMap<String, String>,
    nodes: Vec<u64>,
}

#[derive(Clone, Debug)]
struct RelationData {
    id: u64,
    tags: HashMap<String, String>,
    info: HashMap<String, String>,
    ways: Vec<u64>,
    stops: Vec<u64>,
}

type WayIdsSet = HashSet<u64>;
type NodeIdsSet = HashSet<u64>;

struct MessageRelations {
    relations: Vec<RelationData>,
    stop_ids: NodeIdsSet,
    way_ids: WayIdsSet,
}

struct MessageWays {
    ways: HashMap<u64, WayData>,
    node_ids: NodeIdsSet,
}

struct MessageNodes {
    nodes: HashMap<u64, NodeData>,
}

/// Main class that parses a pbf file and maintains a cache of relations/ways/nodes
/// then provides methods to access the public transports (PTv2) inside that (cached) file
#[derive(Clone)]
pub struct Parser {
    relations: Vec<RelationData>,
    ways: HashMap<u64, WayData>,
    nodes: HashMap<u64, NodeData>,
    cpus: usize,
}

/// Sequential iterator that returns a Relation on each turn
pub struct ParserRelationIterator {
    index: usize,
    data: Parser,
}

/// Main class, it parses the pbf file on new() and maintains an internal cache
/// of relations / ways / nodes, to build on the fly PublicTransport representations
impl Parser {
    /// creates internal cache by parsing the pbf file in the `pbf_filename` path, in parallel with `cpus` threads
    pub fn new(pbf_filename: &str, cpus: usize) -> Self {
        let mut relations = Vec::new() as Vec<RelationData>;
        let mut ways = HashMap::default() as HashMap<u64, WayData>;
        let mut nodes = HashMap::default() as HashMap<u64, NodeData>;
        let way_ids = Arc::new(RwLock::new(HashSet::default() as WayIdsSet));
        let node_ids = Arc::new(RwLock::new(HashSet::default() as NodeIdsSet));

        /*
            pbf relations collect
        */
        {
            println!("START relations map");
            let mut workers = Vec::with_capacity(cpus);
            for _ in 0..cpus {
                let (req_tx, req_rx) = sync_channel(2);
                let (res_tx, res_rx) = sync_channel(0);
                workers.push((req_tx, res_rx));
                thread::spawn(move || {
                    // relations_parser_worker(req_rx, res_tx);
                    // let routetypes_stops = ["train", "subway", "monorail", "tram", "light_rail"];
                    let routetypes_all = [
                        "train",
                        "subway",
                        "monorail",
                        "tram",
                        "light_rail",
                        "bus",
                        "trolleybus",
                    ];
                    let wayroles = ["", "forward", "backward", "alternate"];

                    let mut relations = Vec::new() as Vec<RelationData>;
                    let mut stop_ids = HashSet::default() as NodeIdsSet;
                    let mut way_ids = HashSet::default() as WayIdsSet;
                    while let Ok(blob) = req_rx.recv() {
                        let data = (blob as Blob).into_data();
                        let primitive_block = PrimitiveBlock::parse(&data);
                        for primitive in primitive_block.primitives() {
                            if let Primitive::Relation(relation) = primitive {
                                let routetag = relation.tags().find(|&kv| kv.0 == "route");
                                let routemastertag =
                                    relation.tags().find(|&kv| kv.0 == "route_master");
                                let nametag = relation.tags().find(|&kv| kv.0 == "name");
                                if routemastertag == None
                                    && routetag != None
                                    && routetypes_all.contains(&routetag.unwrap().1)
                                    && nametag != None
                                {
                                    let mut info: HashMap<String, String> = HashMap::new();
                                    if let Some(info_data) = relation.info.clone() {
                                        if let Some(version) = info_data.version {
                                            info.insert("version".to_string(), version.to_string());
                                        }
                                        if let Some(timestamp) = info_data.timestamp {
                                            info.insert(
                                                "timestamp".to_string(),
                                                timestamp.to_string(),
                                            );
                                        }
                                        if let Some(changeset) = info_data.changeset {
                                            info.insert(
                                                "changeset".to_string(),
                                                changeset.to_string(),
                                            );
                                        }
                                        if let Some(uid) = info_data.uid {
                                            info.insert("uid".to_string(), uid.to_string());
                                        }
                                        if let Some(user) = info_data.user {
                                            info.insert("user".to_string(), user.to_string());
                                        }
                                        if let Some(visible) = info_data.visible {
                                            info.insert("visible".to_string(), visible.to_string());
                                        }
                                    }
                                    // condicion para saber si esta relation es un public transport
                                    let mut rd = RelationData {
                                        id: relation.id,
                                        tags: relation
                                            .tags()
                                            .map(|t| (t.0.to_string(), t.1.to_string()))
                                            .collect(),
                                        info,
                                        ways: Vec::new(),
                                        stops: Vec::new(),
                                    };
                                    for member in relation.members() {
                                        // member = (role: &str, id: u64, type: RelationMemberType)
                                        if member.2 == RelationMemberType::Way
                                            && wayroles.contains(&member.0)
                                        {
                                            rd.ways.push(member.1);
                                            way_ids.insert(member.1);
                                        }
                                        if member.2 == RelationMemberType::Node {
                                            rd.stops.push(member.1);
                                            stop_ids.insert(member.1);
                                        }
                                    }
                                    if !rd.ways.is_empty() {
                                        relations.push(rd);
                                    } else {
                                        // println!("WARNING: relation has no ways 'https://www.openstreetmap.org/relation/{:?}'", relation.id);
                                    }
                                }
                            }
                        }
                    }

                    res_tx
                        .send(MessageRelations {
                            relations,
                            stop_ids,
                            way_ids,
                        })
                        .unwrap();
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

            println!("START relations reduce");
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
            println!("START ways map");
            let mut workers = Vec::with_capacity(cpus);
            for _ in 0..cpus {
                let (req_tx, req_rx) = sync_channel(2);
                let (res_tx, res_rx) = sync_channel(0);
                workers.push((req_tx, res_rx));
                let way_ids_local = way_ids.clone();
                thread::spawn(move || {
                    let mut ways = HashMap::default() as HashMap<u64, WayData>;
                    let mut node_ids = HashSet::default() as NodeIdsSet;
                    let way_ids_read = way_ids_local.read().unwrap();
                    while let Ok(blob) = req_rx.recv() {
                        let blob = (blob as Blob).into_data();
                        let primitive_block = PrimitiveBlock::parse(&blob);
                        for primitive in primitive_block.primitives() {
                            if let Primitive::Way(way) = primitive {
                                if way_ids_read.contains(&way.id) {
                                    for node in way.refs() {
                                        node_ids.insert(node as u64);
                                    }
                                    ways.insert(
                                        way.id,
                                        WayData {
                                            id: way.id,
                                            tags: way
                                                .tags()
                                                .map(|t| (t.0.to_string(), t.1.to_string()))
                                                .collect(),
                                            nodes: way.refs().map(|id| id as u64).collect(),
                                        },
                                    );
                                }
                            }
                        }
                    }

                    res_tx.send(MessageWays { ways, node_ids }).unwrap();
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

            println!("START ways reduce");
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
            println!("START nodes map");
            let mut workers = Vec::with_capacity(cpus);
            for _ in 0..cpus {
                let (req_tx, req_rx) = sync_channel(2);
                let (res_tx, res_rx) = sync_channel(0);
                workers.push((req_tx, res_rx));
                let node_ids_local = node_ids.clone();
                thread::spawn(move || {
                    // nodes_parser_worker(req_rx, res_tx);
                    let node_ids_read = node_ids_local.read().unwrap();
                    let mut nodes = HashMap::default() as HashMap<u64, NodeData>;
                    while let Ok(blob) = req_rx.recv() {
                        let blob = (blob as Blob).into_data();
                        let primitive_block = PrimitiveBlock::parse(&blob);
                        for primitive in primitive_block.primitives() {
                            if let Primitive::Node(node) = primitive {
                                if node_ids_read.contains(&node.id) {
                                    nodes.insert(
                                        node.id,
                                        NodeData {
                                            id: node.id,
                                            tags: node
                                                .tags
                                                .into_iter()
                                                .map(|t| (t.0.to_string(), t.1.to_string()))
                                                .collect(),
                                            lat: node.lat,
                                            lon: node.lon,
                                        },
                                    );
                                }
                            }
                        }
                    }

                    res_tx.send(MessageNodes { nodes }).unwrap();
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

            println!("START nodes reduce");
            // reduce / join all data from workers into one structure
            {
                for (req_tx, res_rx) in workers.into_iter() {
                    drop(req_tx);
                    let worker_data = res_rx.recv().unwrap();
                    nodes.extend(worker_data.nodes);
                }
            } // write lock
        } // local vars block
        println!("END processing");

        Parser {
            relations,
            ways,
            nodes,
            cpus,
        }
    }

    /// Builds a vector in parallel with all the public transport ways normalized and "fixed".
    /// It works in parallel using the same amount of threads that were configured on new()
    pub fn get_public_transports(&self, gap: f64) -> Vec<PublicTransport> {
        self.par_map(&move |r| {
            // println!("{:?}",r.id);
            let (f, s) = r.flatten_ways(gap).unwrap();
            PublicTransport {
                id: r.id,
                tags: r.tags.clone(),
                info: r.info.clone(),
                stops: r.stops,
                geometry: f
                    .iter()
                    .map(|v| v.iter().map(|n| (n.lon, n.lat)).collect())
                    .collect(),
                parse_status: s,
            }
        })
    }

    /// Iterates in parallel through the cache of public transports and
    /// offers the possibility to further process them with the `func` function being executed in parallel
    /// It works in parallel using the same amount of threads that were configured on new()
    pub fn par_map<R, F>(&self, func: &F) -> Vec<R>
    where
        F: Fn(Relation) -> R + Sync + Send + Clone + 'static,
        R: Send + 'static,
    {
        let cpus = self.cpus;
        let length = self.relations.len();
        let mut workers = Vec::with_capacity(cpus);
        let index = Arc::new(RwLock::new(AtomicUsize::new(0)));
        crossbeam::scope(|s| {
            for _ in 0..cpus {
                let (res_tx, res_rx) = sync_channel(200);
                workers.push(res_rx);
                let index_local = index.clone();
                s.spawn(move |_| loop {
                    let index;
                    {
                        let index_write = index_local.write().unwrap();
                        index = index_write.fetch_add(1, Ordering::SeqCst);
                    }
                    let len = self.relations.len();
                    if index >= len {
                        break;
                    }
                    let relation = self.get_at(index);
                    let processed = func(relation);
                    res_tx.send(processed).unwrap();
                });
            }

            // reduce / join all data from workers into one structure
            let mut relations = Vec::with_capacity(length);
            let mut errors = 0;
            while errors < cpus {
                errors = 0;
                for res_rx in workers.iter() {
                    match res_rx.recv() {
                        Ok(worker_data) => relations.push(worker_data),
                        Err(_) => errors += 1,
                    };
                }
            }
            relations
        })
        .unwrap()
    }

    /// Builds the Relation from the provided osm_id `id`
    pub fn get_relation_from_id(self, id: u64) -> Relation {
        let relopt = self.relations.iter().find(|rel| rel.id == id);
        let rel = relopt.as_ref().unwrap();
        self.get_relation_from(rel)
    }

    /// Builds the Relation providing relationdata internal cache
    fn get_relation_from(&self, relation_data: &RelationData) -> Relation {
        Relation {
            id: relation_data.id,
            tags: relation_data.tags.clone(),
            info: relation_data.info.clone(),
            ways: relation_data
                .ways
                .iter()
                .filter(|wid| self.ways.contains_key(&wid))
                .map(|wid| Way {
                    id: *wid,
                    tags: self.ways[&wid].tags.clone(),
                    nodes: self.ways[&wid]
                        .nodes
                        .iter()
                        .filter(|nid| self.nodes.contains_key(&nid))
                        .map(|nid| Node {
                            id: *nid,
                            tags: self.nodes[&nid].tags.clone(),
                            lat: self.nodes[&nid].lat,
                            lon: self.nodes[&nid].lon,
                        })
                        .collect(),
                })
                .collect(),
            stops: relation_data
                .stops
                .iter()
                .filter(|nid| self.nodes.contains_key(&nid))
                .map(|nid| Node {
                    id: *nid,
                    tags: self.nodes[&nid].tags.clone(),
                    lat: self.nodes[&nid].lat,
                    lon: self.nodes[&nid].lon,
                })
                .collect(),
        }
    }

    /// Builds the Relation at position `index` in the internal cache
    pub fn get_at(&self, index: usize) -> Relation {
        let rel = &self.relations[index];
        self.get_relation_from(rel)
    }

    /// Returns a sequential iterator that returns a Relation on each turn
    pub fn iter(self) -> ParserRelationIterator {
        ParserRelationIterator {
            data: self,
            index: 0,
        }
    }
}

impl fmt::Debug for Parser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Preparing to print")?;
        let mut count = 0;
        for relation in self.relations.clone() {
            count += 1;
            let ways_count = relation.ways.len();
            let stops_count = relation.stops.len();
            let nodes_count: usize = relation
                .ways
                .iter()
                .map(|wid| match self.ways.get(wid) {
                    Some(way) => way.nodes.len(),
                    None => 0,
                })
                .sum();
            writeln!(
                f,
                "{:?}: ways {:?}, stops {:?}, nodes {:?}, {:?}",
                relation.id, ways_count, stops_count, nodes_count, relation.tags["name"]
            )?;
        }
        writeln!(f, "\nFound {:?} relations", count)?;
        Ok(())
    }
}

impl Iterator for ParserRelationIterator {
    type Item = Relation;

    fn next(&mut self) -> Option<Relation> {
        if self.index >= self.data.relations.len() {
            None
        } else {
            let relation = self.data.get_at(self.index);
            self.index += 1usize;
            Some(relation)
        }
    }
}

impl IntoIterator for Parser {
    type Item = Relation;
    type IntoIter = ParserRelationIterator;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
