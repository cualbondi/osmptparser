use serde_json::json;
use std::collections::HashMap;
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
    .fold(f64::INFINITY, |a, b| if a < *b { a } else { *b })
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
///
/// TODO: this can probably be made with some std function like vec.sort(|nodesa, nodesb| edgedistance(nodesa, nodesb))
fn sort_ways(ways: &Vec<Vec<Node>>) -> Result<Vec<Vec<Node>>, ()> {
    let mut ws = ways.to_owned();
    let mut sorted_ways = vec![ws[0].clone()];
    ws = ws[1..].to_vec();
    while !ws.is_empty() {
        let mut mindist = f64::INFINITY;
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
    for w in ways[1..].iter().cloned() {
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
        return Ok((Vec::new(), ParseStatus::Broken));
    }
    let passed = first_pass(ways)?;
    if passed.len() == 1 {
        return Ok((passed, ParseStatus::Ok));
    }
    let sorted = sort_ways(&passed)?;
    let sorted_passed = first_pass(&sorted)?;
    if sorted_passed.len() == 1 {
        return Ok((sorted_passed, ParseStatus::Sorted));
    }
    let joined = join_ways(&passed, tolerance)?;
    if joined.len() == 1 {
        return Ok((joined, ParseStatus::Joined));
    }
    let joined_sorted = join_ways(&sorted, tolerance)?;
    if joined_sorted.len() == 1 {
        return Ok((joined_sorted, ParseStatus::JoinedSorted));
    }
    Ok((Vec::new(), ParseStatus::Broken))
}

/// close every linestring in `f_ways` when `closed` is requested and the flatten
/// did not already fail, propagating a Broken status if any of them cannot be closed
fn close_if_needed(
    f_ways: Vec<Vec<Node>>,
    f_status: ParseStatus,
    tolerance: f64,
    closed: bool,
) -> (Vec<Vec<Node>>, ParseStatus) {
    if !closed || f_status == ParseStatus::Broken {
        return (f_ways, f_status);
    }
    let mut f_ways_closed = Vec::new();
    let mut f_status_closed = f_status;
    for w in f_ways {
        let (w_closed, w_status) = close_linestring(&w, tolerance).unwrap();
        if w_status == ParseStatus::Broken {
            f_status_closed = ParseStatus::Broken;
        }
        if w_status != ParseStatus::Broken && f_status_closed != ParseStatus::Broken {
            f_status_closed = w_status;
        }
        f_ways_closed.push(w_closed);
    }
    (f_ways_closed, f_status_closed)
}

/// assert closedness of a linestring within a tolerance
/// if it is not closed but in tolerance, close it
fn close_linestring(way: &Vec<Node>, tolerance: f64) -> Result<(Vec<Node>, ParseStatus), ()> {
    let mut closed = way.to_owned();
    let mut closed_status = ParseStatus::Ok;
    if closed[0] == closed[closed.len() - 1] {
        return Ok((closed, closed_status));
    }
    if dist_haversine(&closed[0], &closed[closed.len() - 1]) <= tolerance {
        closed.push(closed[0].clone());
        closed_status = ParseStatus::Joined;
        return Ok((closed, closed_status));
    }
    Ok((Vec::new(), ParseStatus::Broken))
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
        // skip degenerate (empty) ways: on a clipped extract a way's nodes can
        // all fall outside the downloaded area, and an empty way would panic the
        // index-based joining in flatten_ways
        let ways: Vec<Vec<Node>> = self
            .ways
            .iter()
            .map(|w| w.nodes.clone())
            .filter(|nodes| !nodes.is_empty())
            .collect();
        let (f_ways, f_status) = flatten_ways(&ways, tolerance).unwrap();

        // check and close if needed
        Ok(close_if_needed(f_ways, f_status, tolerance, closed))
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
        // skip a degenerate (empty) way: on a clipped extract the nodes can all
        // fall outside the downloaded area, which would panic the joining below
        let ways: Vec<Vec<Node>> = std::iter::once(self.nodes.clone())
            .filter(|nodes| !nodes.is_empty())
            .collect();
        let (f_ways, f_status) = flatten_ways(&ways, tolerance).unwrap();

        // check and close if needed
        Ok(close_if_needed(f_ways, f_status, tolerance, closed))
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
                    "code": self.parse_status.code(),
                    "detail": self.parse_status.detail(),
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
                    "code": self.parse_status.code(),
                    "detail": self.parse_status.detail(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// build a bare node (equality only depends on `id`)
    fn n(id: u64, lat: f64, lon: f64) -> Node {
        Node {
            id,
            tags: HashMap::new(),
            lat,
            lon,
        }
    }

    /// ids of the nodes in a single way, in order
    fn ids(way: &[Node]) -> Vec<u64> {
        way.iter().map(|node| node.id).collect()
    }

    /// id of the first node of each way
    fn first_ids(ways: &[Vec<Node>]) -> Vec<u64> {
        ways.iter().map(|w| w[0].id).collect()
    }

    const EPS: f64 = 1e-9;

    #[test]
    fn pointdistance_is_euclidean() {
        // 3-4-5 triangle in (lat, lon) space
        assert!((pointdistance(&n(1, 0.0, 0.0), &n(2, 3.0, 4.0)) - 5.0).abs() < EPS);
        assert!(pointdistance(&n(1, 0.0, 0.0), &n(2, 0.0, 0.0)).abs() < EPS);
    }

    #[test]
    fn dist_haversine_one_degree_of_latitude() {
        // one degree of latitude is ~111.19 km on a 6_371_000 m sphere
        let d = dist_haversine(&n(1, 0.0, 0.0), &n(2, 1.0, 0.0));
        assert!((d - 111_194.93).abs() < 1.0, "got {}", d);
        assert!(dist_haversine(&n(1, 0.0, 0.0), &n(2, 0.0, 0.0)).abs() < EPS);
    }

    #[test]
    fn edgedistance_returns_min_endpoint_distance() {
        // closest endpoints are (0,10)&(0,11) -> distance 1
        let w1 = vec![n(1, 0.0, 0.0), n(2, 0.0, 10.0)];
        let w2 = vec![n(3, 0.0, 11.0), n(4, 0.0, 20.0)];
        let d = edgedistance(&w1, &w2);
        assert!((d - 1.0).abs() < EPS, "got {}", d);
        // regression: seeded with -1.0 the fold always returned -1.0
        assert!(d > 0.0, "edgedistance must be a real distance, got {}", d);
    }

    #[test]
    fn first_pass_joins_a_forward_chain() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(2, 0.0, 1.0), n(3, 0.0, 2.0)],
        ];
        let out = first_pass(&ways).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(ids(&out[0]), vec![1, 2, 3]);
    }

    #[test]
    fn first_pass_reverses_the_second_segment() {
        // second way shares its *last* node with the chain, so it is reversed
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(3, 0.0, 2.0), n(2, 0.0, 1.0)],
        ];
        let out = first_pass(&ways).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(ids(&out[0]), vec![1, 2, 3]);
    }

    #[test]
    fn first_pass_reverses_the_first_segment() {
        // first way must be reversed so its end matches the second way's start
        let ways = vec![
            vec![n(2, 0.0, 1.0), n(1, 0.0, 0.0)],
            vec![n(2, 0.0, 1.0), n(3, 0.0, 2.0)],
        ];
        let out = first_pass(&ways).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(ids(&out[0]), vec![1, 2, 3]);
    }

    #[test]
    fn first_pass_keeps_disconnected_ways_separate() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(3, 0.0, 5.0), n(4, 0.0, 6.0)],
        ];
        let out = first_pass(&ways).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn sort_ways_orders_by_proximity() {
        // way1 is closest to way0's end, so it must come before the far way2.
        // with the old edgedistance bug every distance was -1.0 and the input
        // order [0, 2, 1] was preserved instead.
        let way0 = vec![n(10, 0.0, 0.0), n(11, 0.0, 1.0)];
        let way1 = vec![n(20, 0.0, 1.5), n(21, 0.0, 2.0)];
        let way2 = vec![n(30, 0.0, 10.0), n(31, 0.0, 11.0)];
        let sorted = sort_ways(&vec![way0, way2, way1]).unwrap();
        assert_eq!(first_ids(&sorted), vec![10, 20, 30]);
    }

    #[test]
    fn join_ways_bridges_a_gap_within_tolerance() {
        // ~11 m gap between the two ways
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001)],
            vec![n(3, 0.0, 0.0011), n(4, 0.0, 0.002)],
        ];
        let joined = join_ways(&ways, 50.0).unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(ids(&joined[0]), vec![1, 2, 3, 4]);
    }

    #[test]
    fn join_ways_reverses_a_segment_to_bridge_a_gap() {
        // second way is oriented backwards; its last node is the near one
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001)],
            vec![n(3, 0.0, 0.002), n(4, 0.0, 0.0011)],
        ];
        let joined = join_ways(&ways, 50.0).unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(ids(&joined[0]), vec![1, 2, 4, 3]);
    }

    #[test]
    fn join_ways_keeps_ways_apart_when_gap_exceeds_tolerance() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001)],
            vec![n(3, 0.0, 0.0011), n(4, 0.0, 0.002)],
        ];
        let joined = join_ways(&ways, 5.0).unwrap();
        assert_eq!(joined.len(), 2);
    }

    #[test]
    fn close_linestring_leaves_already_closed_ring_untouched() {
        let way = vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001), n(1, 0.0, 0.0)];
        let (closed, status) = close_linestring(&way, 50.0).unwrap();
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(ids(&closed), vec![1, 2, 1]);
    }

    #[test]
    fn close_linestring_closes_a_ring_within_tolerance() {
        // last node is ~11 m from the first, closeable within 50 m
        let way = vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001), n(3, 0.0, 0.0001)];
        let (closed, status) = close_linestring(&way, 50.0).unwrap();
        assert_eq!(status, ParseStatus::Joined);
        assert_eq!(closed.len(), 4);
        assert_eq!(closed[0], closed[closed.len() - 1]);
    }

    #[test]
    fn close_linestring_reports_broken_when_out_of_tolerance() {
        let way = vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)];
        let (closed, status) = close_linestring(&way, 50.0).unwrap();
        assert_eq!(status, ParseStatus::Broken);
        assert!(closed.is_empty());
    }

    #[test]
    fn flatten_ways_empty_is_broken() {
        let (out, status) = flatten_ways(&Vec::new(), 50.0).unwrap();
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }

    #[test]
    fn flatten_ways_connected_chain_is_ok() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(2, 0.0, 1.0), n(3, 0.0, 2.0)],
        ];
        let (out, status) = flatten_ways(&ways, 50.0).unwrap();
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(out.len(), 1);
        assert_eq!(ids(&out[0]), vec![1, 2, 3]);
    }

    #[test]
    fn flatten_ways_bridges_small_gap_as_joined() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001)],
            vec![n(3, 0.0, 0.0011), n(4, 0.0, 0.002)],
        ];
        let (out, status) = flatten_ways(&ways, 50.0).unwrap();
        assert_eq!(status, ParseStatus::Joined);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn flatten_ways_reports_broken_when_gaps_exceed_tolerance() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(3, 0.0, 10.0), n(4, 0.0, 11.0)],
        ];
        let (out, status) = flatten_ways(&ways, 50.0).unwrap();
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }

    /// build a way with the given nodes (tags/info irrelevant to geometry)
    fn way(id: u64, nodes: Vec<Node>) -> Way {
        Way {
            id,
            tags: HashMap::new(),
            info: HashMap::new(),
            nodes,
        }
    }

    #[test]
    fn relation_flatten_ways_skips_empty_ways_without_panic() {
        // a clipped extract can leave a way with no nodes; it must be ignored,
        // not panic the index-based joining pipeline
        let rel = Relation {
            id: 1,
            tags: HashMap::new(),
            info: HashMap::new(),
            ways: vec![
                way(2, Vec::new()),
                way(1, vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)]),
            ],
            stops: Vec::new(),
        };
        let (out, status) = rel.flatten_ways(50.0, false).unwrap();
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(out.len(), 1);
        assert_eq!(ids(&out[0]), vec![1, 2]);
    }

    #[test]
    fn relation_flatten_ways_all_empty_is_broken() {
        let rel = Relation {
            id: 1,
            tags: HashMap::new(),
            info: HashMap::new(),
            ways: vec![way(2, Vec::new())],
            stops: Vec::new(),
        };
        let (out, status) = rel.flatten_ways(50.0, true).unwrap();
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }

    #[test]
    fn way_flatten_ways_empty_is_broken_without_panic() {
        // closing an empty way used to panic on closed[0] in close_linestring
        let (out, status) = way(1, Vec::new()).flatten_ways(50.0, true).unwrap();
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }
}
