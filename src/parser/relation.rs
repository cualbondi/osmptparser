extern crate fxhash;
use std::error::Error;
use std::f64::INFINITY;
use fxhash::FxHashMap;

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u64,
    pub tags: FxHashMap<String, String>,
    pub lat: f64,
    pub lon: f64,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Node {}

#[derive(Clone, Debug)]
pub struct Way {
    pub id: u64,
    pub tags: FxHashMap<String, String>,
    pub nodes: Vec<Node>,
}

#[derive(Clone, Debug)]
pub struct Relation {
    pub id: u64,
    pub tags: FxHashMap<String, String>,
    pub ways: Vec<Way>,
    pub stops: Vec<Node>,
    // pub flattened: Vec<Vec<Node>>,
}

fn pointdistance(p1: &Node, p2: &Node) -> f64 {
    ((p1.lat - p2.lat).powf(2f64) + (p1.lon - p2.lon).powf(2f64)).sqrt()
}

fn edgedistance(w1: &Vec<Node>, w2: &Vec<Node>) -> f64 {
    let w1p1 = &w1[0];
    let w1p2 = &w1[w1.len() - 1];
    let w2p1 = &w2[0];
    let w2p2 = &w2[w2.len() - 1];
    [
        pointdistance(w1p1, w2p1),
        pointdistance(w1p2, w2p2),
        pointdistance(w1p1, w2p2),
        pointdistance(w1p2, w2p1),
    ].iter().fold(-1f64, |a, b| {
        if a < *b {
            a
        }
        else {
            *b
        }
    })
}

/// move ways from one place to another to get the closest ones together
/// the first one is taken as important (the one that sets the direction)
/// Also joins the way if extreme points are the same
/// - This is not "expected" by osm. We are trying to fix the way now
/// - Nevertheless I think this is also done by ST_LineMerge()
/// TODO: this can probably be made with some std function like vec.sort(|nodesa, nodesb| edgedistance(nodesa, nodesb))
fn sort_ways(ways: &Vec<Vec<Node>>) -> Result<Vec<Vec<Node>>, Box<Error>> {
    let mut ws = ways.clone();
    let mut sorted_ways = Vec::new();
    sorted_ways.push(ws[0].clone());
    ws = ws[1..].to_vec();
    while ws.len() > 0 {
        let mut mindist = INFINITY;
        let mut minidx = 0usize;
        for i in 0..ws.len() {
            let w = ws[i].clone();
            let dist = edgedistance(&w, &sorted_ways[sorted_ways.len() - 1]);
            if dist < mindist {
                mindist = dist;
                minidx = i;
            }
        }
        sorted_ways.push(ws[minidx].clone());
        ws.remove(minidx);
    };
    Ok(sorted_ways)
}

/// try to reverse directions in linestrings
/// to join segments into a single linestring
/// - This is normal in openstreetmap format
/// - Also ST_LineMerge() should do this already
fn first_pass(ways: &Vec<Vec<Node>>) -> Result<Vec<Vec<Node>>, Box<Error>> {
    // try to flatten by joining most ways as possible
    let n = ways.len();
    let mut ordered_ways = Vec::new();
    ordered_ways.push(ways[0].clone());
    for i in 1..n {
        let way = ways[i].clone();
        let mut prev_way = ordered_ways[ordered_ways.len() - 1].clone();
        // if its the first segment on the linestring,
        // try reversing it if it matches with the second
        if ordered_ways[ordered_ways.len() - 1] == ways[i - 1] {
            if way[0] == prev_way[0] || way[way.len() - 1] == prev_way[0] {
                let mut rev = prev_way.clone();
                rev.reverse();
                let lasti = ordered_ways.len() - 1;
                ordered_ways[lasti] = rev;
                prev_way = ordered_ways[lasti].clone();
            }
        }
        // concat the second segment with the first one
        if prev_way[prev_way.len() - 1] == way[0] {
            let lasti = ordered_ways.len() - 1;
            let tail = &mut (way.clone()[1..].to_vec());
            ordered_ways[lasti].append(tail);
        }
        // concat the second segment reversig it
        else if prev_way[prev_way.len() - 1] == way[way.len() - 1] {
            let lasti = ordered_ways.len() - 1;
            let mut rev = way.clone();
            rev.reverse();
            let tail = &mut (rev[1..].to_vec());
            ordered_ways[lasti].append(tail);
        }
        // cannot form a single linestring, continue processing
        else {
            ordered_ways.push(way);
        }
    }

    Ok(ordered_ways)
}

impl Relation {
    pub fn flatten_ways(&self, tolerance: f64) -> Result<Vec<Vec<Node>>, Box<Error>> {
        let ways = self.ways.iter().map(|w| w.nodes.clone()).collect();
        let v = first_pass(&ways)?;
        if v.len() > 1 {
            let sorted = sort_ways(&v)?;
            let sorted_passed = first_pass(&sorted)?;
            return Ok(sorted_passed);
        }
        Ok(v)
    }
}