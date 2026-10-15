use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use super::parse_status::ParseStatus;

/// Shared, cheaply-cloneable map of OSM tags / metadata.
/// Wrapped in `Arc` so materializing relations/ways/nodes from the cache
/// bumps a refcount instead of deep-copying the map.
pub type Tags = Arc<HashMap<String, String>>;

/// OSM node representation with all the relevant osm data (tags and id)
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u64,
    pub tags: Tags,
    pub lat: f64,
    pub lon: f64,
    /// relation member role (e.g. `stop`, `platform`) when the node is a stop of a
    /// relation; empty for the nodes of a way
    pub role: String,
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
    pub tags: Tags,
    pub info: Tags,
    pub nodes: Vec<Node>,
    /// relation member role (e.g. `outer`, `inner`) when the way is a member of a
    /// relation; empty for a standalone way
    pub role: String,
}

/// OSM relation representation with all the relevant osm data (tags and ids of relation and all ways and nodes)
#[derive(Clone, Debug)]
pub struct Relation {
    pub id: u64,
    pub tags: Tags,
    pub info: Tags,
    pub ways: Vec<Way>,
    pub stops: Vec<Node>,
}

type LonLat = (f64, f64);
/// closed ring of coordinates (first == last)
type Ring = Vec<LonLat>;
/// polygon: one outer ring followed by its inner rings (holes)
type Polygon = Vec<Ring>;

/// Public transport simple model
#[derive(Clone, Debug)]
pub struct PublicTransport {
    /// osm id
    pub id: u64,
    /// osm tags of the public transport relation
    pub tags: Tags,
    /// osm metadata of the public transport relation
    pub info: Tags,
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
    pub tags: Tags,
    /// osm metadata of the Area
    pub info: Tags,
    /// multipolygon geometry, best effort fixed: one polygon per `outer` ring (several
    /// when the area has exclaves), each followed by the `inner` rings (holes) it contains
    pub geometry: Vec<Polygon>,
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
fn first_pass(ways: &Vec<Vec<Node>>) -> Vec<Vec<Node>> {
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

    ordered_ways
}

/// move ways from one place to another to get the closest ones together
/// the first one is taken as important (the one that sets the direction)
/// Also joins the way if extreme points are the same
/// - This is not "expected" by osm. We are trying to fix the way now
/// - Nevertheless I think this is also done by ST_LineMerge()
///
/// TODO: this can probably be made with some std function like vec.sort(|nodesa, nodesb| edgedistance(nodesa, nodesb))
fn sort_ways(ways: &Vec<Vec<Node>>) -> Vec<Vec<Node>> {
    // work on indices so we only compare endpoints (edgedistance) instead of
    // cloning whole ways on every comparison; clone each way once at the end
    let mut remaining: Vec<usize> = (1..ways.len()).collect();
    let mut sorted_indices = vec![0usize];
    while !remaining.is_empty() {
        let last = &ways[*sorted_indices.last().unwrap()];
        let mut mindist = f64::INFINITY;
        let mut min_pos = 0usize;
        for (pos, &idx) in remaining.iter().enumerate() {
            let dist = edgedistance(&ways[idx], last);
            if dist < mindist {
                mindist = dist;
                min_pos = pos;
            }
        }
        sorted_indices.push(remaining.remove(min_pos));
    }
    sorted_indices
        .into_iter()
        .map(|i| ways[i].clone())
        .collect()
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
fn join_ways(ways: &Vec<Vec<Node>>, tolerance: f64) -> Vec<Vec<Node>> {
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
    joined
}

fn flatten_ways(ways: &Vec<Vec<Node>>, tolerance: f64) -> (Vec<Vec<Node>>, ParseStatus) {
    if ways.is_empty() {
        return (Vec::new(), ParseStatus::Broken);
    }
    let passed = first_pass(ways);
    if passed.len() == 1 {
        return (passed, ParseStatus::Ok);
    }
    let sorted = sort_ways(&passed);
    let sorted_passed = first_pass(&sorted);
    if sorted_passed.len() == 1 {
        return (sorted_passed, ParseStatus::Sorted);
    }
    let joined = join_ways(&passed, tolerance);
    if joined.len() == 1 {
        return (joined, ParseStatus::Joined);
    }
    let joined_sorted = join_ways(&sorted, tolerance);
    if joined_sorted.len() == 1 {
        return (joined_sorted, ParseStatus::JoinedSorted);
    }
    (Vec::new(), ParseStatus::Broken)
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
        let (w_closed, w_status) = close_linestring(&w, tolerance);
        // union the workarounds so the flatten status (Sorted/Joined/...) is not
        // overwritten by the closing status; Broken stays absorbing
        f_status_closed = f_status_closed.combine(w_status);
        f_ways_closed.push(w_closed);
    }
    (f_ways_closed, f_status_closed)
}

/// assert closedness of a linestring within a tolerance
/// if it is not closed but in tolerance, close it
fn close_linestring(way: &Vec<Node>, tolerance: f64) -> (Vec<Node>, ParseStatus) {
    let mut closed = way.to_owned();
    let mut closed_status = ParseStatus::Ok;
    if closed[0] == closed[closed.len() - 1] {
        return (closed, closed_status);
    }
    if dist_haversine(&closed[0], &closed[closed.len() - 1]) <= tolerance {
        closed.push(closed[0].clone());
        closed_status = ParseStatus::Joined;
        return (closed, closed_status);
    }
    (Vec::new(), ParseStatus::Broken)
}

/// whether `ring` is closed (first node == last node)
fn is_closed(ring: &[Node]) -> bool {
    ring.len() > 1 && ring[0] == ring[ring.len() - 1]
}

/// position in `remaining` of the first open way that shares a node id with `end`
fn touching(end: &Node, remaining: &[Vec<Node>]) -> Option<usize> {
    remaining
        .iter()
        .position(|w| !is_closed(w) && (w[0] == *end || w[w.len() - 1] == *end))
}

/// (position in `remaining`, distance) of the open way whose endpoint is nearest to `end`
fn nearest(end: &Node, remaining: &[Vec<Node>]) -> Option<(usize, f64)> {
    remaining
        .iter()
        .enumerate()
        .filter(|(_, w)| !is_closed(w))
        .map(|(pos, w)| {
            let d = dist_haversine(end, &w[0]).min(dist_haversine(end, &w[w.len() - 1]));
            (pos, d)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// append `way` to the end of `ring`, reversing it so its nearest endpoint comes first;
/// the shared node is not repeated when the two touch exactly
fn attach(ring: &mut Vec<Node>, mut way: Vec<Node>) {
    let last = &ring[ring.len() - 1];
    if dist_haversine(last, &way[way.len() - 1]) < dist_haversine(last, &way[0]) {
        way.reverse();
    }
    let skip = if way[0] == *last { 1 } else { 0 };
    ring.extend(way.into_iter().skip(skip));
}

/// Assemble closed rings out of ways, the way the osm multipolygon model expects:
/// ways are chained by shared end nodes regardless of their member order, closed
/// ways are rings on their own. When a chain cannot be continued exactly, the
/// nearest endpoint within `tolerance` meters is used (`Joined`); taking a way
/// out of member order is reported as `Sorted`. Any ring that cannot be closed
/// makes the whole result `Broken` (empty).
fn assemble_rings(ways: &[Vec<Node>], tolerance: f64) -> (Vec<Vec<Node>>, ParseStatus) {
    let mut remaining: Vec<Vec<Node>> = ways.iter().filter(|w| !w.is_empty()).cloned().collect();
    if remaining.is_empty() {
        return (Vec::new(), ParseStatus::Broken);
    }
    let mut rings = Vec::new();
    let mut status = ParseStatus::Ok;
    while !remaining.is_empty() {
        let mut ring = remaining.remove(0);
        loop {
            if is_closed(&ring) {
                break;
            }
            if let Some(pos) = touching(&ring[ring.len() - 1], &remaining) {
                if pos != 0 {
                    status = status.combine(ParseStatus::Sorted);
                }
                attach(&mut ring, remaining.remove(pos));
                continue;
            }
            if touching(&ring[0], &remaining).is_some() {
                ring.reverse();
                continue;
            }
            // no exact continuation: bridge the smallest gap within tolerance,
            // either to another way or by closing the ring on itself
            let at_end = nearest(&ring[ring.len() - 1], &remaining);
            let at_start = nearest(&ring[0], &remaining);
            let close_gap = dist_haversine(&ring[0], &ring[ring.len() - 1]);
            let best_way = match (at_end, at_start) {
                (Some(e), Some(s)) if s.1 < e.1 => Some((s, true)),
                (Some(e), _) => Some((e, false)),
                (None, Some(s)) => Some((s, true)),
                (None, None) => None,
            };
            match best_way {
                Some(((pos, d), reverse_ring)) if d <= tolerance && d <= close_gap => {
                    if reverse_ring {
                        ring.reverse();
                    }
                    attach(&mut ring, remaining.remove(pos));
                    status = status.combine(ParseStatus::Joined);
                }
                _ if close_gap <= tolerance => {
                    ring.push(ring[0].clone());
                    status = status.combine(ParseStatus::Joined);
                }
                _ => return (Vec::new(), ParseStatus::Broken),
            }
        }
        // a closed ring needs at least 3 distinct nodes to enclose anything
        if ring.len() < 4 {
            return (Vec::new(), ParseStatus::Broken);
        }
        rings.push(ring);
    }
    (rings, status)
}

/// even-odd (ray casting) point in closed ring test on plain lon/lat
fn point_in_ring(p: &Node, ring: &[Node]) -> bool {
    let mut inside = false;
    for i in 1..ring.len() {
        let (a, b) = (&ring[i - 1], &ring[i]);
        if (a.lat > p.lat) != (b.lat > p.lat) {
            let lon = a.lon + (p.lat - a.lat) / (b.lat - a.lat) * (b.lon - a.lon);
            if p.lon < lon {
                inside = !inside;
            }
        }
    }
    inside
}

/// bounding box area of a ring, used to pick the innermost of nested outer rings
fn bbox_area(ring: &[Node]) -> f64 {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for n in ring {
        minx = minx.min(n.lon);
        maxx = maxx.max(n.lon);
        miny = miny.min(n.lat);
        maxy = maxy.max(n.lat);
    }
    (maxx - minx) * (maxy - miny)
}

/// group rings into polygons: one per outer ring, each inner ring goes to the
/// smallest outer ring that contains its first node (inner rings that fall in no
/// outer ring are dropped)
fn build_polygons(outers: Vec<Vec<Node>>, inners: Vec<Vec<Node>>) -> Vec<Vec<Vec<Node>>> {
    let areas: Vec<f64> = outers.iter().map(|o| bbox_area(o)).collect();
    let mut polygons: Vec<Vec<Vec<Node>>> = outers.into_iter().map(|o| vec![o]).collect();
    for inner in inners {
        let container = polygons
            .iter()
            .enumerate()
            .filter(|(_, p)| point_in_ring(&inner[0], &p[0]))
            .min_by(|a, b| areas[a.0].total_cmp(&areas[b.0]))
            .map(|(i, _)| i);
        if let Some(i) = container {
            polygons[i].push(inner);
        }
    }
    polygons
}

/// assemble a multipolygon out of `outer` and `inner` ways (see `assemble_rings`);
/// a broken ring on either side makes the whole area `Broken`
fn polygons_from_ways(
    outer: &[Vec<Node>],
    inner: &[Vec<Node>],
    tolerance: f64,
) -> (Vec<Vec<Vec<Node>>>, ParseStatus) {
    let (outer_rings, outer_status) = assemble_rings(outer, tolerance);
    if outer_status == ParseStatus::Broken {
        return (Vec::new(), ParseStatus::Broken);
    }
    let (inner_rings, inner_status) = if inner.is_empty() {
        (Vec::new(), ParseStatus::Ok)
    } else {
        assemble_rings(inner, tolerance)
    };
    if inner_status == ParseStatus::Broken {
        return (Vec::new(), ParseStatus::Broken);
    }
    (
        build_polygons(outer_rings, inner_rings),
        outer_status.combine(inner_status),
    )
}

impl Relation {
    /// best effort multipolygon from the ways of this (multipolygon / boundary) relation:
    /// ways with role `inner` are holes, every other way is part of an outer ring; rings
    /// are chained by shared nodes in any member order and gaps up to `tolerance` meters
    /// are bridged. Returns one polygon (outer ring + its holes) per outer ring.
    pub fn polygons(&self, tolerance: f64) -> (Vec<Vec<Vec<Node>>>, ParseStatus) {
        let (inner, outer): (Vec<&Way>, Vec<&Way>) = self
            .ways
            .iter()
            .filter(|w| !w.nodes.is_empty())
            .partition(|w| w.role == "inner");
        let nodes = |ways: Vec<&Way>| {
            ways.into_iter()
                .map(|w| w.nodes.clone())
                .collect::<Vec<_>>()
        };
        polygons_from_ways(&nodes(outer), &nodes(inner), tolerance)
    }
}

impl Way {
    /// best effort polygon from a closed (or closeable within `tolerance` meters) way
    pub fn polygons(&self, tolerance: f64) -> (Vec<Vec<Vec<Node>>>, ParseStatus) {
        polygons_from_ways(std::slice::from_ref(&self.nodes), &[], tolerance)
    }
}

impl Relation {
    /// best effort get a linestring or multilinestring from all the ways that compose this relation
    /// if `tolerance` is > 0, then it also join gaps in the ways into one linestring when possible
    /// `tolerance` is in meters
    /// param `closed` to assert that the linestring last first element is within tolerance to the first element
    pub fn flatten_ways(&self, tolerance: f64, closed: bool) -> (Vec<Vec<Node>>, ParseStatus) {
        // skip degenerate (empty) ways: on a clipped extract a way's nodes can
        // all fall outside the downloaded area, and an empty way would panic the
        // index-based joining in flatten_ways
        let ways: Vec<Vec<Node>> = self
            .ways
            .iter()
            .map(|w| w.nodes.clone())
            .filter(|nodes| !nodes.is_empty())
            .collect();
        let (f_ways, f_status) = flatten_ways(&ways, tolerance);

        // check and close if needed
        close_if_needed(f_ways, f_status, tolerance, closed)
    }
}

impl Way {
    /// best effort get a closed linestring from the nodes that compose this way
    /// if `tolerance` is > 0, then it also join gap in the way into one closed linestring when possible
    /// `tolerance` is in meters
    pub fn flatten_ways(&self, tolerance: f64, closed: bool) -> (Vec<Vec<Node>>, ParseStatus) {
        // skip a degenerate (empty) way: on a clipped extract the nodes can all
        // fall outside the downloaded area, which would panic the joining below
        let ways: Vec<Vec<Node>> = std::iter::once(self.nodes.clone())
            .filter(|nodes| !nodes.is_empty())
            .collect();
        let (f_ways, f_status) = flatten_ways(&ways, tolerance);

        // check and close if needed
        close_if_needed(f_ways, f_status, tolerance, closed)
    }
}

impl Area {
    pub fn to_geojson(&self) -> String {
        json!({
            "type": "Feature",
            "properties": {
                "id": self.id,
                "id_type": self.id_type,
                "tags": self.tags.as_ref(),
                "info": self.info.as_ref(),
                "parse_status": {
                    "code": self.parse_status.code(),
                    "detail": self.parse_status.detail(),
                }
            },
            "geometry": {
                "type": "MultiPolygon",
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
                "tags": self.tags.as_ref(),
                "info": self.info.as_ref(),
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
                            "role": s.role,
                            "tags": s.tags.as_ref(),
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
            tags: Arc::new(HashMap::new()),
            lat,
            lon,
            role: String::new(),
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
        let out = first_pass(&ways);
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
        let out = first_pass(&ways);
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
        let out = first_pass(&ways);
        assert_eq!(out.len(), 1);
        assert_eq!(ids(&out[0]), vec![1, 2, 3]);
    }

    #[test]
    fn first_pass_keeps_disconnected_ways_separate() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(3, 0.0, 5.0), n(4, 0.0, 6.0)],
        ];
        let out = first_pass(&ways);
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
        let sorted = sort_ways(&vec![way0, way2, way1]);
        assert_eq!(first_ids(&sorted), vec![10, 20, 30]);
    }

    #[test]
    fn join_ways_bridges_a_gap_within_tolerance() {
        // ~11 m gap between the two ways
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001)],
            vec![n(3, 0.0, 0.0011), n(4, 0.0, 0.002)],
        ];
        let joined = join_ways(&ways, 50.0);
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
        let joined = join_ways(&ways, 50.0);
        assert_eq!(joined.len(), 1);
        assert_eq!(ids(&joined[0]), vec![1, 2, 4, 3]);
    }

    #[test]
    fn join_ways_keeps_ways_apart_when_gap_exceeds_tolerance() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001)],
            vec![n(3, 0.0, 0.0011), n(4, 0.0, 0.002)],
        ];
        let joined = join_ways(&ways, 5.0);
        assert_eq!(joined.len(), 2);
    }

    #[test]
    fn close_linestring_leaves_already_closed_ring_untouched() {
        let way = vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001), n(1, 0.0, 0.0)];
        let (closed, status) = close_linestring(&way, 50.0);
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(ids(&closed), vec![1, 2, 1]);
    }

    #[test]
    fn close_linestring_closes_a_ring_within_tolerance() {
        // last node is ~11 m from the first, closeable within 50 m
        let way = vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001), n(3, 0.0, 0.0001)];
        let (closed, status) = close_linestring(&way, 50.0);
        assert_eq!(status, ParseStatus::Joined);
        assert_eq!(closed.len(), 4);
        assert_eq!(closed[0], closed[closed.len() - 1]);
    }

    #[test]
    fn close_linestring_reports_broken_when_out_of_tolerance() {
        let way = vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)];
        let (closed, status) = close_linestring(&way, 50.0);
        assert_eq!(status, ParseStatus::Broken);
        assert!(closed.is_empty());
    }

    #[test]
    fn flatten_ways_empty_is_broken() {
        let (out, status) = flatten_ways(&Vec::new(), 50.0);
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }

    #[test]
    fn flatten_ways_connected_chain_is_ok() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(2, 0.0, 1.0), n(3, 0.0, 2.0)],
        ];
        let (out, status) = flatten_ways(&ways, 50.0);
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
        let (out, status) = flatten_ways(&ways, 50.0);
        assert_eq!(status, ParseStatus::Joined);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn flatten_ways_reports_broken_when_gaps_exceed_tolerance() {
        let ways = vec![
            vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)],
            vec![n(3, 0.0, 10.0), n(4, 0.0, 11.0)],
        ];
        let (out, status) = flatten_ways(&ways, 50.0);
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }

    /// build a way with the given nodes (tags/info irrelevant to geometry)
    fn way(id: u64, nodes: Vec<Node>) -> Way {
        Way {
            id,
            tags: Arc::new(HashMap::new()),
            info: Arc::new(HashMap::new()),
            nodes,
            role: String::new(),
        }
    }

    /// a way with a relation member role
    fn way_role(id: u64, role: &str, nodes: Vec<Node>) -> Way {
        Way {
            role: role.to_string(),
            ..way(id, nodes)
        }
    }

    /// a square ring of side `side` degrees with its south-west corner at (lat, lon);
    /// node ids start at `id0`
    fn square(id0: u64, lat: f64, lon: f64, side: f64) -> Vec<Node> {
        vec![
            n(id0, lat, lon),
            n(id0 + 1, lat, lon + side),
            n(id0 + 2, lat + side, lon + side),
            n(id0 + 3, lat + side, lon),
            n(id0, lat, lon),
        ]
    }

    #[test]
    fn relation_flatten_ways_skips_empty_ways_without_panic() {
        // a clipped extract can leave a way with no nodes; it must be ignored,
        // not panic the index-based joining pipeline
        let rel = Relation {
            id: 1,
            tags: Arc::new(HashMap::new()),
            info: Arc::new(HashMap::new()),
            ways: vec![
                way(2, Vec::new()),
                way(1, vec![n(1, 0.0, 0.0), n(2, 0.0, 1.0)]),
            ],
            stops: Vec::new(),
        };
        let (out, status) = rel.flatten_ways(50.0, false);
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(out.len(), 1);
        assert_eq!(ids(&out[0]), vec![1, 2]);
    }

    #[test]
    fn relation_flatten_ways_all_empty_is_broken() {
        let rel = Relation {
            id: 1,
            tags: Arc::new(HashMap::new()),
            info: Arc::new(HashMap::new()),
            ways: vec![way(2, Vec::new())],
            stops: Vec::new(),
        };
        let (out, status) = rel.flatten_ways(50.0, true);
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }

    #[test]
    fn way_flatten_ways_empty_is_broken_without_panic() {
        // closing an empty way used to panic on closed[0] in close_linestring
        let (out, status) = way(1, Vec::new()).flatten_ways(50.0, true);
        assert_eq!(status, ParseStatus::Broken);
        assert!(out.is_empty());
    }

    #[test]
    fn close_if_needed_keeps_the_flatten_status_for_an_already_closed_ring() {
        // a ring rescued by sorting that is already closed must stay Sorted,
        // not be downgraded to Ok by the closing step
        let ring = vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001), n(1, 0.0, 0.0)];
        let (out, status) = close_if_needed(vec![ring], ParseStatus::Sorted, 50.0, true);
        assert_eq!(status, ParseStatus::Sorted);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn close_if_needed_unions_sorted_flatten_with_a_joined_close() {
        // sorted during flatten, then the ring ends had to be joined within
        // tolerance -> both workarounds must be reported
        let open_ring = vec![n(1, 0.0, 0.0), n(2, 0.0, 0.001), n(3, 0.0, 0.0001)];
        let (out, status) = close_if_needed(vec![open_ring], ParseStatus::Sorted, 50.0, true);
        assert_eq!(status, ParseStatus::JoinedSorted);
        assert_eq!(out[0][0], out[0][out[0].len() - 1]);
    }

    fn relation_with_ways(ways: Vec<Way>) -> Relation {
        Relation {
            id: 1,
            tags: Arc::new(HashMap::new()),
            info: Arc::new(HashMap::new()),
            ways,
            stops: vec![],
        }
    }

    fn ring_ids(ring: &[Node]) -> Vec<u64> {
        ring.iter().map(|n| n.id).collect()
    }

    #[test]
    fn polygons_outer_split_in_unordered_ways_is_sorted() {
        let sq = square(1, 0.0, 0.0, 1.0);
        // ring 1-2-3-4-1 split as [3,4,1] + [1,2,3], given in the wrong order
        let r = relation_with_ways(vec![
            way_role(
                1,
                "outer",
                vec![sq[2].clone(), sq[3].clone(), sq[0].clone()],
            ),
            way_role(
                2,
                "outer",
                vec![sq[0].clone(), sq[1].clone(), sq[2].clone()],
            ),
        ]);
        let (polys, status) = r.polygons(50.0);
        // the second way continues the first one by its start, so nothing was out of order
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0].len(), 1);
        assert_eq!(ring_ids(&polys[0][0]), vec![3, 4, 1, 2, 3]);
    }

    #[test]
    fn polygons_reports_sorted_when_a_later_way_is_needed_first() {
        let sq = square(1, 0.0, 0.0, 1.0);
        let r = relation_with_ways(vec![
            way_role(1, "outer", vec![sq[0].clone(), sq[1].clone()]),
            way_role(
                2,
                "outer",
                vec![sq[2].clone(), sq[3].clone(), sq[0].clone()],
            ),
            way_role(3, "outer", vec![sq[1].clone(), sq[2].clone()]),
        ]);
        let (polys, status) = r.polygons(50.0);
        assert_eq!(status, ParseStatus::Sorted);
        assert_eq!(ring_ids(&polys[0][0]), vec![1, 2, 3, 4, 1]);
    }

    #[test]
    fn polygons_inner_ring_becomes_a_hole_of_its_outer() {
        let r = relation_with_ways(vec![
            way_role(1, "outer", square(1, 0.0, 0.0, 1.0)),
            way_role(2, "inner", square(11, 0.4, 0.4, 0.2)),
        ]);
        let (polys, status) = r.polygons(50.0);
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0].len(), 2);
        assert_eq!(ring_ids(&polys[0][0]), vec![1, 2, 3, 4, 1]);
        assert_eq!(ring_ids(&polys[0][1]), vec![11, 12, 13, 14, 11]);
    }

    #[test]
    fn polygons_two_closed_outers_make_a_multipolygon_and_holes_go_to_the_right_one() {
        let r = relation_with_ways(vec![
            way_role(1, "outer", square(1, 0.0, 0.0, 1.0)),
            way_role(2, "outer", square(21, 5.0, 5.0, 1.0)),
            way_role(3, "inner", square(31, 5.4, 5.4, 0.2)),
        ]);
        let (polys, status) = r.polygons(50.0);
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(polys.len(), 2);
        assert_eq!(polys[0].len(), 1);
        assert_eq!(polys[1].len(), 2);
        assert_eq!(ring_ids(&polys[1][1]), vec![31, 32, 33, 34, 31]);
    }

    #[test]
    fn polygons_inner_ring_outside_every_outer_is_dropped() {
        let r = relation_with_ways(vec![
            way_role(1, "outer", square(1, 0.0, 0.0, 1.0)),
            way_role(2, "inner", square(11, 5.0, 5.0, 0.2)),
        ]);
        let (polys, _) = r.polygons(50.0);
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0].len(), 1);
    }

    #[test]
    fn polygons_bridges_a_small_gap_as_joined() {
        let mut sq = square(1, 0.0, 0.0, 1.0);
        // move the closing node a few meters away so the ring is not exactly closed
        sq[4] = n(5, 0.00001, 0.00001);
        let r = relation_with_ways(vec![way_role(1, "outer", sq)]);
        let (polys, status) = r.polygons(50.0);
        assert_eq!(status, ParseStatus::Joined);
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0][0][0], polys[0][0][polys[0][0].len() - 1]);
    }

    #[test]
    fn polygons_is_broken_when_an_outer_ring_cannot_be_closed() {
        let sq = square(1, 0.0, 0.0, 1.0);
        let r = relation_with_ways(vec![way_role(
            1,
            "outer",
            vec![sq[0].clone(), sq[1].clone(), sq[2].clone()],
        )]);
        let (polys, status) = r.polygons(50.0);
        assert_eq!(status, ParseStatus::Broken);
        assert!(polys.is_empty());
    }

    #[test]
    fn polygons_is_broken_when_an_inner_ring_cannot_be_closed() {
        let hole = square(11, 0.4, 0.4, 0.2);
        let r = relation_with_ways(vec![
            way_role(1, "outer", square(1, 0.0, 0.0, 1.0)),
            way_role(2, "inner", vec![hole[0].clone(), hole[1].clone()]),
        ]);
        let (_, status) = r.polygons(50.0);
        assert_eq!(status, ParseStatus::Broken);
    }

    #[test]
    fn polygons_without_roles_treats_every_way_as_outer() {
        let sq = square(1, 0.0, 0.0, 1.0);
        let r = relation_with_ways(vec![
            way(1, vec![sq[0].clone(), sq[1].clone(), sq[2].clone()]),
            way(2, vec![sq[2].clone(), sq[3].clone(), sq[0].clone()]),
        ]);
        let (polys, status) = r.polygons(50.0);
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(ring_ids(&polys[0][0]), vec![1, 2, 3, 4, 1]);
    }

    #[test]
    fn polygons_empty_relation_is_broken() {
        let (polys, status) = relation_with_ways(vec![]).polygons(50.0);
        assert_eq!(status, ParseStatus::Broken);
        assert!(polys.is_empty());
    }

    #[test]
    fn way_polygons_closed_way_is_a_single_ring() {
        let (polys, status) = way(1, square(1, 0.0, 0.0, 1.0)).polygons(50.0);
        assert_eq!(status, ParseStatus::Ok);
        assert_eq!(polys.len(), 1);
        assert_eq!(ring_ids(&polys[0][0]), vec![1, 2, 3, 4, 1]);
    }

    #[test]
    fn way_polygons_open_way_is_broken() {
        let sq = square(1, 0.0, 0.0, 1.0);
        let (polys, status) =
            way(1, vec![sq[0].clone(), sq[1].clone(), sq[2].clone()]).polygons(50.0);
        assert_eq!(status, ParseStatus::Broken);
        assert!(polys.is_empty());
    }

    #[test]
    fn point_in_ring_even_odd() {
        let sq = square(1, 0.0, 0.0, 1.0);
        assert!(point_in_ring(&n(9, 0.5, 0.5), &sq));
        assert!(!point_in_ring(&n(9, 1.5, 0.5), &sq));
        assert!(!point_in_ring(&n(9, 0.5, -0.5), &sq));
    }
}
