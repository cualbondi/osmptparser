extern crate crossbeam;
extern crate osm_pbf_iter;
use std::io::{self, Write};
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

use relation::{Area, Node, PublicTransport, Relation, Way};

#[derive(Clone, Debug)]
struct NodeData {
    // id: u64,
    lat: f64,
    lon: f64,
    tags: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct WayData {
    id: u64,
    tags: HashMap<String, String>,
    info: HashMap<String, String>,
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

/// One accepted value of a filter condition (the part after `=`, split by `,`)
struct ConditionValue {
    /// whether this value was negated with a leading `!`
    negated: bool,
    /// value to compare against (without the leading `!`)
    value: String,
}

/// A single parsed filter condition (one segment between `&`)
struct Condition {
    /// tag key to look up (keeps a leading `!` when the key is negated)
    key: String,
    /// whether the key was negated with a leading `!`
    key_negated: bool,
    /// accepted values (after `=`); `None` when the condition only checks key presence/absence
    values: Option<Vec<ConditionValue>>,
}

#[derive(Default)]
struct MessageRelations {
    relations: Vec<RelationData>,
    stop_ids: NodeIdsSet,
    way_ids: WayIdsSet,
}

#[derive(Default)]
struct MessageWays {
    ways: Vec<WayData>,
    relations_ways: HashMap<u64, WayData>,
    node_ids: NodeIdsSet,
}

#[derive(Default)]
struct MessageNodes {
    nodes: HashMap<u64, NodeData>,
}

/// Main class that parses a pbf file and maintains a cache of relations/ways/nodes
/// then provides methods to access the public transports (PTv2) inside that (cached) file
#[derive(Clone)]
pub struct Parser {
    relations: Vec<RelationData>,
    relations_ways: HashMap<u64, WayData>,
    ways: Vec<WayData>,
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
    /// parse a filter string into conditions once, so it is not re-parsed for every element.
    /// format of tag_conditions:
    /// "tag_key" (has the tag with any value)
    /// "!tag_key" (does not have the tag)
    /// "tag_key=tag_value" (equality with one value)
    /// "tag_key=!tag_value" (negation)
    /// "tag_key=tag_value,tag_value2&tag_key2=tag_value3" (comma means OR, & means AND)
    /// "tag_key=!tag_value,!tag_value2&tag_key2=tag_value3" (negation works with individual values)
    fn parse_filters(filters: &str) -> Vec<Condition> {
        filters
            .split('&')
            .map(|condition| {
                let mut condition_split = condition.split('=');
                let key = condition_split.next().unwrap().to_string();
                let key_negated = key.starts_with('!');
                let values = condition_split.next().map(|values| {
                    values
                        .split(',')
                        .map(|value| match value.strip_prefix('!') {
                            Some(stripped) => ConditionValue {
                                negated: true,
                                value: stripped.to_string(),
                            },
                            None => ConditionValue {
                                negated: false,
                                value: value.to_string(),
                            },
                        })
                        .collect()
                });
                Condition {
                    key,
                    key_negated,
                    values,
                }
            })
            .collect()
    }

    /// check an element's tags against pre-parsed filter conditions (see `parse_filters`).
    /// `get_tags` yields the element's tags as `(key, value)` pairs and is called once per
    /// condition, matching the way both relations and ways expose their tags.
    fn matches_conditions<'a, I, F>(mut get_tags: F, conditions: &[Condition]) -> bool
    where
        F: FnMut() -> I,
        I: Iterator<Item = (&'a str, &'a str)>,
    {
        for condition in conditions {
            let tag = get_tags().find(|&kv| kv.0 == condition.key);
            if (condition.key_negated && tag.is_some()) || (!condition.key_negated && tag.is_none())
            {
                return false;
            }
            if let Some(values) = &condition.values {
                let tag_value = tag.unwrap().1;
                let found = values.iter().any(|cv| {
                    (cv.negated && tag_value != cv.value.as_str())
                        || (!cv.negated && tag_value == cv.value.as_str())
                });
                if !found {
                    return false;
                }
            }
        }
        true
    }

    /// collect the optional osm metadata (version/timestamp/changeset/uid/user/visible)
    /// of a relation or way into a string map, skipping absent fields
    fn info_map(info: Option<&osm_pbf_iter::info::Info>) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(info) = info {
            if let Some(version) = info.version {
                map.insert("version".to_string(), version.to_string());
            }
            if let Some(timestamp) = info.timestamp {
                map.insert("timestamp".to_string(), timestamp.to_string());
            }
            if let Some(changeset) = info.changeset {
                map.insert("changeset".to_string(), changeset.to_string());
            }
            if let Some(uid) = info.uid {
                map.insert("uid".to_string(), uid.to_string());
            }
            if let Some(user) = info.user {
                map.insert("user".to_string(), user.to_string());
            }
            if let Some(visible) = info.visible {
                map.insert("visible".to_string(), visible.to_string());
            }
        }
        map
    }

    /// Streams `pbf_filename`, dispatching its blobs round-robin across `cpus` worker
    /// threads. Each worker folds the blobs it receives into a per-worker `S` (starting
    /// from `S::default()`) by calling `process` once per blob with the decompressed
    /// bytes, and the per-worker states are returned for the caller to reduce.
    /// `label` is only used for the progress line printed to stderr.
    fn collect_from_pbf<S, P>(
        label: &str,
        pbf_filename: &str,
        cpus: usize,
        process: P,
    ) -> io::Result<Vec<S>>
    where
        S: Default + Send + 'static,
        P: Fn(&mut S, &[u8]) + Send + Sync + Clone + 'static,
    {
        eprint!("START {} map, ", label);
        io::stderr().flush().unwrap();

        let f = File::open(pbf_filename)?;
        let mut reader = BlobReader::new(BufReader::new(f));

        let mut workers = Vec::with_capacity(cpus);
        for _ in 0..cpus {
            let (req_tx, req_rx) = sync_channel::<Blob>(2);
            let (res_tx, res_rx) = sync_channel::<S>(0);
            workers.push((req_tx, res_rx));
            let process = process.clone();
            thread::spawn(move || {
                let mut state = S::default();
                while let Ok(blob) = req_rx.recv() {
                    let data = (blob as Blob).into_data();
                    process(&mut state, &data);
                }
                res_tx.send(state).unwrap();
            });
        }

        let mut w = 0;
        for blob in &mut reader {
            let req_tx = &workers[w].0;
            w = (w + 1) % cpus;
            req_tx.send(blob).unwrap();
        }

        eprint!("reduce, ");
        io::stderr().flush().unwrap();

        // drop each request sender so its worker's recv loop ends, then collect its state
        let mut results = Vec::with_capacity(cpus);
        for (req_tx, res_rx) in workers.into_iter() {
            drop(req_tx);
            results.push(res_rx.recv().unwrap());
        }
        Ok(results)
    }

    /// creates internal cache by parsing public transport v2 from pbf file in the `pbf_filename` path, in parallel with `cpus` threads
    pub fn new_ptv2(pbf_filename: &str, cpus: usize) -> io::Result<Self> {
        Self::new(
            pbf_filename,
            cpus,
            "name&!route_master&route=bus,tram,train,subway,light_rail,monorail,trolleybus"
                .to_string(),
        )
    }

    /// creates internal cache by parsing administrative areas from pbf file in the `pbf_filename` path, in parallel with `cpus` threads
    pub fn new_aa(pbf_filename: &str, cpus: usize) -> io::Result<Self> {
        Self::new(
            pbf_filename,
            cpus,
            "name&admin_level&boundary=administrative".to_string(),
        )
    }

    /// creates internal cache by parsing the pbf file in the `pbf_filename` path, in parallel with `cpus` threads, filtering by tags in `filters`
    /// filters examples:
    /// "tag_key"
    ///     the relation must have the tag "tag_key"
    /// "tag_key=tag_value"
    ///     the relation must have the tag "tag_key" with value "tag_value"
    /// "tag_key=tag_value,tag_value2&tag_key2=tag_value3"
    ///     the relation must have the tag "tag_key" with value "tag_value" or "tag_value2" and the tag "tag_key2" with value "tag_value3"
    pub fn new(pbf_filename: &str, cpus: usize, filters: String) -> io::Result<Self> {
        let mut relations = Vec::new() as Vec<RelationData>;
        let mut ways = Vec::new() as Vec<WayData>;
        let mut relations_ways = HashMap::default() as HashMap<u64, WayData>;
        let mut nodes = HashMap::default() as HashMap<u64, NodeData>;
        let way_ids = Arc::new(RwLock::new(HashSet::default() as WayIdsSet));
        let node_ids = Arc::new(RwLock::new(HashSet::default() as NodeIdsSet));
        let filters_arc = Arc::new(Self::parse_filters(&filters));

        /*
            pbf relations collect
        */
        {
            let filters_local = filters_arc.clone();
            let worker_results = Self::collect_from_pbf(
                "Relations",
                pbf_filename,
                cpus,
                move |state: &mut MessageRelations, data: &[u8]| {
                    let primitive_block = PrimitiveBlock::parse(data);
                    for primitive in primitive_block.primitives() {
                        if let Primitive::Relation(relation) = primitive {
                            if Self::matches_conditions(|| relation.tags(), filters_local.as_ref())
                            {
                                let info = Self::info_map(relation.info.as_ref());
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
                                    if member.2 == RelationMemberType::Way {
                                        rd.ways.push(member.1);
                                        state.way_ids.insert(member.1);
                                    }
                                    if member.2 == RelationMemberType::Node {
                                        rd.stops.push(member.1);
                                        state.stop_ids.insert(member.1);
                                    }
                                }
                                if !rd.ways.is_empty() {
                                    state.relations.push(rd);
                                } else {
                                    // println!("WARNING: relation has no ways 'https://www.openstreetmap.org/relation/{:?}'", relation.id);
                                }
                            }
                        }
                    }
                },
            )?;

            // reduce / join all data from workers into one structure
            let mut node_ids_write = node_ids.write().unwrap();
            let mut way_ids_write = way_ids.write().unwrap();
            for worker_data in worker_results {
                relations.extend(worker_data.relations);
                node_ids_write.extend(worker_data.stop_ids);
                way_ids_write.extend(worker_data.way_ids);
            }
            drop(node_ids_write);
            drop(way_ids_write);
            eprintln!("found {}", relations.len());
        }

        /*
            pbf ways collect
        */
        {
            let filters_local = filters_arc.clone();
            let way_ids_reader = way_ids.clone();
            let worker_results = Self::collect_from_pbf(
                "Ways",
                pbf_filename,
                cpus,
                move |state: &mut MessageWays, data: &[u8]| {
                    let way_ids_read = way_ids_reader.read().unwrap();
                    let primitive_block = PrimitiveBlock::parse(data);
                    for primitive in primitive_block.primitives() {
                        if let Primitive::Way(way) = primitive {
                            // relations_ways, collect ways that are part of relations previously found
                            if way_ids_read.contains(&way.id) {
                                for node in way.refs() {
                                    state.node_ids.insert(node as u64);
                                }
                                state.relations_ways.insert(
                                    way.id,
                                    WayData {
                                        id: way.id,
                                        tags: way
                                            .tags()
                                            .map(|t| (t.0.to_string(), t.1.to_string()))
                                            .collect(),
                                        info: HashMap::new(),
                                        nodes: way.refs().map(|id| id as u64).collect(),
                                    },
                                );
                            }
                            // ways, collect ways that are not part of relations previously found but conform to filters
                            if Self::matches_conditions(|| way.tags(), filters_local.as_ref()) {
                                let info = Self::info_map(way.info.as_ref());
                                let wd = WayData {
                                    id: way.id,
                                    tags: way
                                        .tags()
                                        .map(|t| (t.0.to_string(), t.1.to_string()))
                                        .collect(),
                                    info,
                                    nodes: way.refs().map(|id| id as u64).collect(),
                                };
                                if !wd.nodes.is_empty() {
                                    for node in way.refs() {
                                        state.node_ids.insert(node as u64);
                                    }
                                    state.ways.push(wd);
                                } else {
                                    // println!("WARNING: way has no nodes 'https://www.openstreetmap.org/way/{:?}'", way.id);
                                }
                            }
                        }
                    }
                },
            )?;

            // reduce / join all data from workers into one structure
            let mut node_ids_write = node_ids.write().unwrap();
            for worker_data in worker_results {
                ways.extend(worker_data.ways);
                relations_ways.extend(worker_data.relations_ways);
                node_ids_write.extend(worker_data.node_ids);
            }
            drop(node_ids_write);
            eprintln!(
                "found {} for relations +{} new",
                relations_ways.len(),
                ways.len()
            );
        }

        /*
            pbf nodes collect
        */
        {
            let node_ids_reader = node_ids.clone();
            let worker_results = Self::collect_from_pbf(
                "Nodes",
                pbf_filename,
                cpus,
                move |state: &mut MessageNodes, data: &[u8]| {
                    let node_ids_read = node_ids_reader.read().unwrap();
                    let primitive_block = PrimitiveBlock::parse(data);
                    for primitive in primitive_block.primitives() {
                        if let Primitive::Node(node) = primitive {
                            if node_ids_read.contains(&node.id) {
                                state.nodes.insert(
                                    node.id,
                                    NodeData {
                                        // id: node.id,
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
                },
            )?;

            // reduce / join all data from workers into one structure
            for worker_data in worker_results {
                nodes.extend(worker_data.nodes);
            }
        }
        eprintln!("found {}", nodes.len());

        Ok(Parser {
            relations,
            relations_ways,
            ways,
            nodes,
            cpus,
        })
    }

    /// Builds a vector in parallel with all the public transport ways normalized and "fixed".
    /// It works in parallel using the same amount of threads that were configured on new()
    pub fn get_public_transports(&self, gap: f64) -> Vec<PublicTransport> {
        self.par_map(&move |r| {
            // println!("{:?}",r.id);
            let (f, s) = r.flatten_ways(gap, false);
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
        self.par_map_indexed(self.relations.len(), |index| {
            func(self.get_relation_at(index))
        })
    }

    /// Parallel scaffold shared by `par_map` (relations) and `get_areas` (ways):
    /// hands out indices `0..length` across the configured worker threads,
    /// applies `process` to each, and collects the results into a vector.
    fn par_map_indexed<R, P>(&self, length: usize, process: P) -> Vec<R>
    where
        P: Fn(usize) -> R + Sync + Send + Clone,
        R: Send,
    {
        let cpus = self.cpus;
        let mut workers = Vec::with_capacity(cpus);
        let index = Arc::new(AtomicUsize::new(0));
        crossbeam::scope(|s| {
            for _ in 0..cpus {
                let (res_tx, res_rx) = sync_channel(200);
                workers.push(res_rx);
                let index_local = index.clone();
                let process = process.clone();
                s.spawn(move |_| loop {
                    let index = index_local.fetch_add(1, Ordering::SeqCst);
                    if index >= length {
                        break;
                    }
                    let processed = process(index);
                    res_tx.send(processed).unwrap();
                });
            }

            // reduce / join all data from workers into one structure
            let mut results = Vec::with_capacity(length);
            let mut errors = 0;
            while errors < cpus {
                errors = 0;
                for res_rx in workers.iter() {
                    match res_rx.recv() {
                        Ok(worker_data) => results.push(worker_data),
                        Err(_) => errors += 1,
                    };
                }
            }
            results
        })
        .unwrap()
    }

    /// Builds a vector in parallel with all the areas normalized and "fixed".
    /// It works in parallel using the same amount of threads that were configured on new()
    pub fn get_areas(&self, gap: f64) -> Vec<Area> {
        let relations_ways = self.par_map(&move |r| {
            let (f, s) = r.flatten_ways(gap, true);
            Area {
                id: r.id,
                id_type: 'r',
                tags: r.tags,
                info: r.info,
                geometry: f
                    .iter()
                    .map(|v| v.iter().map(|n| (n.lon, n.lat)).collect())
                    .collect(),
                parse_status: s,
            }
        });

        // iterates over all ways and creates a vector of areas
        let areas_ways = self.par_map_indexed(self.ways.len(), |index| {
            let way = self.get_way_at(index);
            let (f, s) = way.flatten_ways(gap, true);
            Area {
                id: way.id,
                id_type: 'w',
                tags: way.tags,
                info: way.info,
                geometry: f
                    .iter()
                    .map(|v| v.iter().map(|n| (n.lon, n.lat)).collect())
                    .collect(),
                parse_status: s,
            }
        });

        // returns relations_ways + areas_ways
        relations_ways.into_iter().chain(areas_ways).collect()
    }

    /// Builds the Relation from the provided osm_id `id`, or `None` if no relation with that id was parsed
    pub fn get_relation_from_id(&self, id: u64) -> Option<Relation> {
        let rel = self.relations.iter().find(|rel| rel.id == id)?;
        Some(self.get_relation_from(rel))
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
                .filter(|wid| self.relations_ways.contains_key(wid))
                .map(|wid| Way {
                    id: *wid,
                    tags: self.relations_ways[wid].tags.clone(),
                    info: self.relations_ways[wid].info.clone(),
                    nodes: self.relations_ways[wid]
                        .nodes
                        .iter()
                        .filter(|nid| self.nodes.contains_key(nid))
                        .map(|nid| Node {
                            id: *nid,
                            tags: self.nodes[nid].tags.clone(),
                            lat: self.nodes[nid].lat,
                            lon: self.nodes[nid].lon,
                        })
                        .collect(),
                })
                .collect(),
            stops: relation_data
                .stops
                .iter()
                .filter(|nid| self.nodes.contains_key(nid))
                .map(|nid| Node {
                    id: *nid,
                    tags: self.nodes[nid].tags.clone(),
                    lat: self.nodes[nid].lat,
                    lon: self.nodes[nid].lon,
                })
                .collect(),
        }
    }

    /// Builds the Way providing waydata internal cache
    fn get_way_from(&self, way_data: &WayData) -> Way {
        Way {
            id: way_data.id,
            tags: way_data.tags.clone(),
            info: way_data.info.clone(),
            nodes: way_data
                .nodes
                .iter()
                .filter(|nid| self.nodes.contains_key(nid))
                .map(|nid| Node {
                    id: *nid,
                    tags: self.nodes[nid].tags.clone(),
                    lat: self.nodes[nid].lat,
                    lon: self.nodes[nid].lon,
                })
                .collect(),
        }
    }

    /// Builds the Relation at position `index` in the internal cache
    pub fn get_relation_at(&self, index: usize) -> Relation {
        let rel = &self.relations[index];
        self.get_relation_from(rel)
    }

    /// Builds the Way at position `index` in the internal cache
    pub fn get_way_at(&self, index: usize) -> Way {
        let way = &self.ways[index];
        self.get_way_from(way)
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
                .map(|wid| match self.relations_ways.get(wid) {
                    Some(way) => way.nodes.len(),
                    None => 0,
                })
                .sum();
            writeln!(
                f,
                "{:?}: ways {:?}, stops {:?}, nodes {:?}, {:?}",
                relation.id,
                ways_count,
                stops_count,
                nodes_count,
                relation.tags.get("name").map(String::as_str).unwrap_or("")
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
            let relation = self.data.get_relation_at(self.index);
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
