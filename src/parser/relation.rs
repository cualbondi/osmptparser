use serde_json::json;
use std::collections::HashMap;
use std::f64::INFINITY;
use std::fmt;

use super::parse_status::ParseStatus;

#[derive(Debug)]
pub struct ParseError;
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error Parsing")
    }
}
impl std::error::Error for ParseError {}

/// OSM node representation with all the relevant osm data (tags and id)
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u64,
    pub tags: HashMap<String, String>,
    pub lat: f64,
    pub lon: f64,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Node {}

/// OSM way representation with all the relevant osm data (tags and ids of all ways and nodes)
#[derive(Clone, Debug)]
pub struct Way {
    pub id: u64,
    pub tags: HashMap<String, String>,
    pub info: HashMap<String, String>,
    pub nodes: Vec<Node>,
}

/// OSM relation representation with all the relevant osm data (tags and ids of relation and all ways and nodes)
#[derive(Clone, Debug)]
pub struct Relation {
    pub id: u64,
    pub tags: HashMap<String, String>,
    pub info: HashMap<String, String>,
    pub ways: Vec<Way>,
    pub stops: Vec<Node>,
}

type LonLat = (f64, f64);

/// Public transport simple model
#[derive(Clone, Debug)]
pub struct PublicTransport {
    /// osm id
    pub id: u64,
    /// osm tags of the public transport relation
    pub tags: HashMap<String, String>,
    /// osm metadata of the public transport relation
    pub info: HashMap<String, String>,
    /// stop nodes of the public transport relation with latlng and internal tags
    pub stops: Vec<Node>,
    /// geometry (linestring/multilinestring), best effort fixed
    pub geometry: Vec<Vec<LonLat>>,
    /// parse status, info on workarounds applied when parsing to fix semi-broken osm route
    pub parse_status: ParseStatus,
}

/// Area simple model
#[derive(Clone, Debug)]
pub struct Area {
    /// osm id
    pub id: u64,
    /// osm object type (n=node, w=way, r=relation)
    pub id_type: char,
    /// osm tags of the Area
    pub tags: HashMap<String, String>,
    /// osm metadata of the Area
    pub info: HashMap<String, String>,
    /// geometry (polygon/multipolygon), best effort fixed
    pub geometry: Vec<Vec<LonLat>>,
    /// parse status, info on workarounds applied when parsing to fix semi-broken osm area
    pub parse_status: ParseStatus,
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
    ]
    .iter()
    .fold(-1f64, |a, b| if a < *b { a } else { *b })
}

/// try to reverse directions in linestrings
/// to join segments into a single linestring
/// - This is normal in openstreetmap format
/// - Also ST_LineMerge() should do this already
fn first_pass(ways: &Vec<Vec<Node>>) -> Result<Vec<Vec<Node>>, ()> {
    // try to flatten by joining most ways as possible
    let n = ways.len();
    let mut ordered_ways = vec![ways[0].clone()];
    for i in 1..n {
        let way = ways[i].clone();
        let mut prev_way = ordered_ways[ordered_ways.len() - 1].clone();
        // if its the first segment on the linestring,
        // try reversing it if it matches with the second
        if ordered_ways[ordered_ways.len() - 1] == ways[i - 1]
            && (way[0] == prev_way[0] || way[way.len() - 1] == prev_way[0])
        {
            let mut rev = prev_way.clone();
            rev.reverse();
            let lasti = ordered_ways.len() - 1;
            ordered_ways[lasti] = rev;
            prev_way = ordered_ways[lasti].clone();
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

/// move ways from one place to another to get the closest ones together
/// the first one is taken as important (the one that sets the direction)
/// Also joins the way if extreme points are the same
/// - This is not "expected" by osm. We are trying to fix the way now
/// - Nevertheless I think this is also done by ST_LineMerge()
/// TODO: this can probably be made with some std function like vec.sort(|nodesa, nodesb| edgedistance(nodesa, nodesb))
fn sort_ways(ways: &Vec<Vec<Node>>) -> Result<Vec<Vec<Node>>, ()> {
    let mut ws = ways.to_owned();
    let mut sorted_ways = vec![ws[0].clone()];
    ws = ws[1..].to_vec();
    while !ws.is_empty() {
        let mut mindist = INFINITY;
        let mut minidx = 0usize;
        for (i, _) in ws.iter().enumerate() {
            let w = ws[i].clone();
            let dist = edgedistance(&w, &sorted_ways[sorted_ways.len() - 1]);
            if dist < mindist {
                mindist = dist;
                minidx = i;
            }
        }
        sorted_ways.push(ws[minidx].clone());
        ws.remove(minidx);
    }
    Ok(sorted_ways)
}

/// calculate haversine distance between two nodes
fn dist_haversine(p1: &Node, p2: &Node) -> f64 {
    let lon1 = p1.lon;
    let lat1 = p1.lat;
    let lon2 = p2.lon;
    let lat2 = p2.lat;

    let radius = 6_371_000_f64; // meters
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2_f64).sin() * (dlat / 2_f64).sin()
        + lat1.to_radians().cos()
            * lat2.to_radians().cos()
            * (dlon / 2_f64).sin()
            * (dlon / 2_f64).sin();
    let c = 2_f64 * a.sqrt().atan2((1_f64 - a).sqrt());
    radius * c
}

/// Join adjacent ways that have their extreme points
/// closer than <tolerance> in meters
/// - This is not "expected". We are trying to fix the way now
/// - This is not done by ST_LineMerge()
/// - I'm not sure if this conserves the direction from first to last
fn join_ways(ways: &Vec<Vec<Node>>, tolerance: f64) -> Result<Vec<Vec<Node>>, ()> {
    let mut joined = vec![ways[0].clone()];
    for w in ways[1..].to_vec() {
        let joined_len = joined.len();
        let joinedlast = joined[joined_len - 1].clone();
        if dist_haversine(&joinedlast[joinedlast.len() - 1], &w[0]) < tolerance {
            joined[joined_len - 1].extend(w);
        } else if dist_haversine(&joinedlast[joinedlast.len() - 1], &w[w.len() - 1]) < tolerance {
            let mut wrev = w.clone();
            wrev.reverse();
            joined[joined_len - 1].extend(wrev);
        } else if dist_haversine(&joinedlast[0], &w[0]) < tolerance {
            joined[joined_len - 1].reverse();
            joined[joined_len - 1].extend(w);
        } else if dist_haversine(&joinedlast[0], &w[w.len() - 1]) < tolerance {
            joined[joined_len - 1].reverse();
            let mut wrev = w.clone();
            wrev.reverse();
            joined[joined_len - 1].extend(wrev);
        } else {
            joined.push(w);
        }
    }
    Ok(joined)
}

fn flatten_ways(
    ways: &Vec<Vec<Node>>,
    tolerance: f64,
) -> Result<(Vec<Vec<Node>>, ParseStatus), ()> {
    if ways.is_empty() {
        return Ok((Vec::new(), ParseStatus::new(501, "Broken")));
    }
    let passed = first_pass(ways)?;
    if passed.len() == 1 {
        return Ok((passed, ParseStatus::ok()));
    }
    let sorted = sort_ways(&passed)?;
    let sorted_passed = first_pass(&sorted)?;
    if sorted_passed.len() == 1 {
        return Ok((sorted_passed, ParseStatus::new(101, "Sorted")));
    }
    let joined = join_ways(&passed, tolerance)?;
    if joined.len() == 1 {
        return Ok((joined, ParseStatus::new(102, "Joined")));
    }
    let joined_sorted = join_ways(&sorted, tolerance)?;
    if joined_sorted.len() == 1 {
        return Ok((joined_sorted, ParseStatus::new(103, "Joined Sorted")));
    }
    Ok((Vec::new(), ParseStatus::new(501, "Broken")))
}

/// assert closedness of a linestring within a tolerance
/// if it is not closed but in tolerance, close it
fn close_linestring(way: &Vec<Node>, tolerance: f64) -> Result<(Vec<Node>, ParseStatus), ()> {
    let mut closed = way.to_owned();
    let mut closed_status = ParseStatus::ok();
    if closed[0] == closed[closed.len() - 1] {
        return Ok((closed, closed_status));
    }
    if dist_haversine(&closed[0], &closed[closed.len() - 1]) <= tolerance {
        closed.push(closed[0].clone());
        closed_status = ParseStatus::new(102, "Joined");
        return Ok((closed, closed_status));
    }
    Ok((Vec::new(), ParseStatus::new(501, "Broken")))
}

impl Relation {
    /// best effort get a linestring or multilinestring from all the ways that compose this relation
    /// if `tolerance` is > 0, then it also join gaps in the ways into one linestring when possible
    /// `tolerance` is in meters
    /// param `closed` to assert that the linestring last first element is within tolerance to the first element
    pub fn flatten_ways(
        &self,
        tolerance: f64,
        closed: bool,
    ) -> Result<(Vec<Vec<Node>>, ParseStatus), ParseError> {
        let ways: Vec<Vec<Node>> = self.ways.iter().map(|w| w.nodes.clone()).collect();
        let (f_ways, f_status) = flatten_ways(&ways, tolerance).unwrap();

        // check and close if needed
        if closed && f_status.code != 501 {
            let mut f_ways_closed = Vec::new();
            let mut f_status_closed = f_status;
            for w in f_ways {
                let (w_closed, w_status) = close_linestring(&w, tolerance).unwrap();
                if w_status.code == 501 {
                    f_status_closed = ParseStatus::new(501, "Broken");
                }
                if w_status.code != 501 && f_status_closed.code != 501 {
                    f_status_closed = w_status;
                }
                f_ways_closed.push(w_closed);
            }
            Ok((f_ways_closed, f_status_closed))
        } else {
            Ok((f_ways, f_status))
        }
    }
}

impl Way {
    /// best effort get a closed linestring from the nodes that compose this way
    /// if `tolerance` is > 0, then it also join gap in the way into one closed linestring when possible
    /// `tolerance` is in meters
    pub fn flatten_ways(
        &self,
        tolerance: f64,
        closed: bool,
    ) -> Result<(Vec<Vec<Node>>, ParseStatus), ()> {
        let ways: Vec<Vec<Node>> = vec![self.nodes.clone()];
        let (f_ways, f_status) = flatten_ways(&ways, tolerance).unwrap();

        // check and close if needed
        if closed && f_status.code != 501 {
            let mut f_ways_closed = Vec::new();
            let mut f_status_closed = f_status;
            for w in f_ways {
                let (w_closed, w_status) = close_linestring(&w, tolerance).unwrap();
                if w_status.code == 501 {
                    f_status_closed = ParseStatus::new(501, "Broken");
                }
                if w_status.code != 501 && f_status_closed.code != 501 {
                    f_status_closed = w_status;
                }
                f_ways_closed.push(w_closed);
            }
            Ok((f_ways_closed, f_status_closed))
        } else {
            Ok((f_ways, f_status))
        }
    }
}

impl Area {
    pub fn to_geojson(&self) -> String {
        json!({
            "type": "Feature",
            "properties": {
                "id": self.id,
                "id_type": self.id_type,
                "tags": self.tags,
                "info": self.info,
                "parse_status": {
                    "code": self.parse_status.code,
                    "detail": self.parse_status.detail,
                }
            },
            "geometry": {
                "type": "Polygon",
                "coordinates": self.geometry
            }
        })
        .to_string()
    }
}

impl PublicTransport {
    pub fn to_geojson(&self) -> String {
        json!({
            "type": "FeatureCollection",
            "properties": {
                "id": self.id,
                "tags": self.tags,
                "info": self.info,
                "parse_status": {
                    "code": self.parse_status.code,
                    "detail": self.parse_status.detail,
                }
            },
            "features": [
                {
                    "type": "Feature",
                    "geometry": {
                        "type": "LineString",
                        "coordinates": self.geometry
                    }
                },
                {
                    "type": "FeatureCollection",
                    "features": self.stops.iter().map(|s| json!({
                        "type": "Feature",
                        "properties": {
                            "id": s.id,
                            "tags": s.tags,
                        },
                        "geometry": {
                            "type": "Point",
                            "coordinates": [s.lon, s.lat]
                        }
                    })).collect::<Vec<_>>()
                },
            ]
        })
        .to_string()
    }
}
