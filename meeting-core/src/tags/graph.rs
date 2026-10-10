//! F20-FR7 / FR9: the Tag Graph's model and layout. Nodes are tags, sized by how many
//! recordings use them; edges join tags used in the same recording. The layout is a seeded
//! force-directed one, run once for a given set of tags and counts (ADR D20): the same input
//! always gives the same picture, and the view only draws it (no live simulation, NFR2).
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// The smallest circle's radius as a share of the largest one's: a tag used once stays big
/// enough to tap beside one used a hundred times (the view also clamps it to ≥ 48 dp).
pub const MIN_RATIO: f64 = 0.25;
/// OQ30: past this many tags, edges shared by fewer than [`WEAK_BELOW`] recordings are weak
/// (hidden unless their tag is selected).
pub const MANY_TAGS: usize = 30;
pub const WEAK_BELOW: usize = 2;
/// The pull of every circle towards the middle, per step.
const GRAVITY: f64 = 0.12;
/// The gap kept between two circles, in units of the largest radius.
const PAD: f64 = 0.35;

/// One tag: its name, how many recordings use it, and its place (the largest radius is 1).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub name: String,
    pub count: usize,
    pub x: f64,
    pub y: f64,
    pub r: f64,
}

/// Two tags used together: indexes into [`Graph::nodes`], `a < b`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    /// How many recordings have both.
    pub shared: usize,
    /// OQ30: hidden unless one of its tags is selected.
    pub weak: bool,
}

/// The laid-out graph. Nodes are ordered most used first, then by name.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// The recordings with at least one tag.
    pub meetings: usize,
    /// `[x0, y0, x1, y1]` around every circle.
    pub bounds: [f64; 4],
}

/// F20-FR7: radius ∝ √count, the most used tag's being 1, never below [`MIN_RATIO`].
pub fn radius(count: usize, max: usize) -> f64 {
    let max = max.max(1) as f64;
    (count as f64 / max).sqrt().clamp(MIN_RATIO, 1.0)
}

/// The graph of `meetings`, each given as its tags (normalised, no duplicates).
pub fn build(meetings: &[Vec<String>]) -> Graph {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for tags in meetings {
        for t in tags {
            *counts.entry(t.as_str()).or_default() += 1;
        }
    }
    let mut order: Vec<(&str, usize)> = counts.into_iter().collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let max = order.first().map_or(1, |(_, c)| *c);
    let index: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, (n, _))| (*n, i))
        .collect();
    let mut nodes: Vec<Node> = order
        .iter()
        .map(|(name, count)| Node {
            name: (*name).to_owned(),
            count: *count,
            x: 0.0,
            y: 0.0,
            r: radius(*count, max),
        })
        .collect();
    let mut pairs: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for tags in meetings {
        let mut ix: Vec<usize> = tags
            .iter()
            .filter_map(|t| index.get(t.as_str()).copied())
            .collect();
        ix.sort_unstable();
        ix.dedup();
        for (i, a) in ix.iter().enumerate() {
            for b in &ix[i + 1..] {
                *pairs.entry((*a, *b)).or_default() += 1;
            }
        }
    }
    let many = nodes.len() > MANY_TAGS;
    let edges: Vec<Edge> = pairs
        .into_iter()
        .map(|((a, b), shared)| Edge {
            a,
            b,
            shared,
            weak: many && shared < WEAK_BELOW,
        })
        .collect();
    layout(&mut nodes, &edges);
    let bounds = bounds(&nodes);
    Graph {
        nodes,
        edges,
        meetings: meetings.iter().filter(|t| !t.is_empty()).count(),
        bounds,
    }
}

fn bounds(nodes: &[Node]) -> [f64; 4] {
    if nodes.is_empty() {
        return [0.0; 4];
    }
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for n in nodes {
        b[0] = b[0].min(n.x - n.r);
        b[1] = b[1].min(n.y - n.r);
        b[2] = b[2].max(n.x + n.r);
        b[3] = b[3].max(n.y + n.r);
    }
    b
}

/// F20-FR9: seeded on a sunflower spiral in node order (the most used tag in the middle),
/// then a fixed number of force steps (repulsion, attraction along edges by how much they are
/// shared, a pull to the centre) with a cooling step size, then overlaps pushed apart. No
/// randomness, so one input has one picture.
fn layout(nodes: &mut [Node], edges: &[Edge]) {
    let n = nodes.len();
    if n == 0 {
        return;
    }
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    let spacing = 2.0 + PAD;
    for (i, nd) in nodes.iter_mut().enumerate() {
        let (rr, a) = (spacing * (i as f64).sqrt(), i as f64 * golden);
        (nd.x, nd.y) = (rr * a.cos(), rr * a.sin());
    }
    let k = 2.0 + 2.0 * PAD; // the ideal distance between two linked unit circles
    let steps = if n > 300 { 80 } else { 240 };
    let start = spacing * (n as f64).sqrt() / 3.0 + 1.0;
    let mut disp = vec![(0.0f64, 0.0f64); n];
    for step in 0..steps {
        let temp = start * (1.0 - step as f64 / steps as f64) + 0.02;
        disp.iter_mut().for_each(|d| *d = (0.0, 0.0));
        for i in 0..n {
            for j in i + 1..n {
                let (dx, dy, d) = apart(&nodes[i], &nodes[j], i, j);
                let reach = nodes[i].r + nodes[j].r + PAD;
                // Repulsion reaches a few link lengths only, so unlinked groups sit near
                // each other instead of drifting to the corners.
                let mut f = if d < 3.0 * k {
                    k * k / d * 0.5 * (1.0 - d / (3.0 * k))
                } else {
                    0.0
                };
                if d < reach {
                    f += (reach - d) * 4.0;
                }
                let (fx, fy) = (dx / d * f, dy / d * f);
                disp[i].0 += fx;
                disp[i].1 += fy;
                disp[j].0 -= fx;
                disp[j].1 -= fy;
            }
        }
        for e in edges {
            let (dx, dy, d) = apart(&nodes[e.a], &nodes[e.b], e.a, e.b);
            let want = k + nodes[e.a].r + nodes[e.b].r - 2.0;
            let f = (d - want).max(0.0) * (1.0 + (e.shared as f64).ln()) * 0.5;
            let (fx, fy) = (dx / d * f, dy / d * f);
            disp[e.a].0 -= fx;
            disp[e.a].1 -= fy;
            disp[e.b].0 += fx;
            disp[e.b].1 += fy;
        }
        for (nd, d) in nodes.iter_mut().zip(&disp) {
            let (gx, gy) = (d.0 - nd.x * GRAVITY, d.1 - nd.y * GRAVITY);
            let len = (gx * gx + gy * gy).sqrt();
            if len > 0.0 {
                let s = len.min(temp) / len;
                nd.x += gx * s;
                nd.y += gy * s;
            }
        }
    }
    separate(nodes);
    // Centred on the origin, so the view's Fit is the bounds alone.
    let b = bounds(nodes);
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    for nd in nodes.iter_mut() {
        nd.x -= cx;
        nd.y -= cy;
    }
}

/// `a - b` and its length; two nodes on one spot are split along a fixed direction.
fn apart(a: &Node, b: &Node, i: usize, j: usize) -> (f64, f64, f64) {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    let d = (dx * dx + dy * dy).sqrt();
    if d > 1e-9 {
        (dx, dy, d)
    } else {
        let t = (i * 31 + j) as f64;
        (t.cos() * 1e-3, t.sin() * 1e-3, 1e-3)
    }
}

/// Push overlapping circles apart until none overlap (or a bounded number of passes).
fn separate(nodes: &mut [Node]) {
    let n = nodes.len();
    for _ in 0..200 {
        let mut moved = false;
        for i in 0..n {
            for j in i + 1..n {
                let (dx, dy, d) = apart(&nodes[i], &nodes[j], i, j);
                let reach = nodes[i].r + nodes[j].r + PAD * 0.5;
                if d < reach {
                    let push = (reach - d) / 2.0 + 1e-6;
                    let (px, py) = (dx / d * push, dy / d * push);
                    nodes[i].x += px;
                    nodes[i].y += py;
                    nodes[j].x -= px;
                    nodes[j].y -= py;
                    moved = true;
                }
            }
        }
        if !moved {
            return;
        }
    }
}

/// Whether any two circles overlap (tests and checks).
pub fn overlaps(g: &Graph) -> bool {
    let n = &g.nodes;
    (0..n.len()).any(|i| {
        (i + 1..n.len()).any(|j| {
            let (dx, dy) = (n[i].x - n[j].x, n[i].y - n[j].y);
            (dx * dx + dy * dy).sqrt() < n[i].r + n[j].r
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| (*t).to_owned()).collect()
    }

    /// F20-AC4: `gpu` (5) has the larger circle, both at least the minimum, joined by an edge.
    #[test]
    fn more_recordings_make_a_bigger_circle() {
        let ms = vec![
            m(&["gpu", "vram"]),
            m(&["gpu", "vram"]),
            m(&["gpu"]),
            m(&["gpu", "local-ai"]),
            m(&["gpu"]),
            m(&[]),
        ];
        let g = build(&ms);
        let names: Vec<&str> = g.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["gpu", "vram", "local-ai"]);
        let (gpu, vram) = (&g.nodes[0], &g.nodes[1]);
        assert_eq!((gpu.count, vram.count), (5, 2));
        assert!(gpu.r > vram.r);
        assert!(vram.r >= MIN_RATIO && g.nodes[2].r >= MIN_RATIO);
        assert_eq!(gpu.r, 1.0);
        let e = g.edges.iter().find(|e| (e.a, e.b) == (0, 1)).unwrap();
        assert_eq!(e.shared, 2);
        assert!(!e.weak);
        assert_eq!(g.edges.len(), 2, "gpu–vram and gpu–local-ai only");
        assert_eq!(g.meetings, 5);
        assert!(!overlaps(&g));
    }

    #[test]
    fn the_same_tags_give_the_same_picture() {
        let ms: Vec<Vec<String>> = (0..40)
            .map(|i| {
                m(&[
                    ["a", "b", "c", "d", "e", "f", "g"][i % 7],
                    ["x", "y", "z"][i % 3],
                ])
            })
            .collect();
        let one = build(&ms);
        let two = build(&ms);
        assert_eq!(one, two);
        assert!(!overlaps(&one));
        // The order the meetings come in doesn't matter either.
        let mut rev = ms.clone();
        rev.reverse();
        assert_eq!(build(&rev), one);
        let b = one.bounds;
        assert!(b[0] < 0.0 && b[2] > 0.0 && ((b[0] + b[2]) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn radius_grows_with_the_square_root() {
        assert_eq!(radius(4, 16), 0.5);
        assert_eq!(radius(1, 100), MIN_RATIO);
        assert_eq!(radius(9, 9), 1.0);
        assert_eq!(radius(0, 0), MIN_RATIO);
    }

    #[test]
    fn empty_and_single_graphs() {
        let g = build(&[]);
        assert!(g.nodes.is_empty() && g.edges.is_empty());
        assert_eq!(g.bounds, [0.0; 4]);
        let g = build(&[m(&["solo"])]);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!((g.nodes[0].x, g.nodes[0].y), (0.0, 0.0));
        assert_eq!(g.bounds, [-1.0, -1.0, 1.0, 1.0]);
    }

    /// OQ30: with more than 30 tags, an edge one recording makes is weak.
    #[test]
    fn many_tags_mark_weak_edges() {
        let mut ms: Vec<Vec<String>> = (0..32).map(|i| vec![format!("t{i:02}")]).collect();
        ms.push(m(&["t00", "t01"]));
        ms.push(m(&["t02", "t03"]));
        ms.push(m(&["t02", "t03"]));
        let g = build(&ms);
        assert_eq!(g.nodes.len(), 32);
        let weak: Vec<bool> = g.edges.iter().map(|e| e.weak).collect();
        assert_eq!(weak.iter().filter(|w| **w).count(), 1);
        assert!(g.edges.iter().any(|e| e.shared == 2 && !e.weak));
        assert!(!overlaps(&g));
    }

    #[test]
    fn coincident_nodes_are_split() {
        let a = Node {
            name: "a".into(),
            count: 1,
            x: 0.0,
            y: 0.0,
            r: 1.0,
        };
        let (_, _, d) = apart(&a, &a.clone(), 0, 1);
        assert!(d > 0.0);
        let mut ns = vec![a.clone(), a.clone(), a];
        separate(&mut ns);
        let g = Graph {
            bounds: bounds(&ns),
            nodes: ns,
            edges: vec![],
            meetings: 0,
        };
        assert!(!overlaps(&g));
    }
}
