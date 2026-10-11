//! The original car path network (`nodes0.dat` to `nodes63.dat` in
//! `gta3.img`) and shortest routes over it, for the GPS line on the radar.
//!
//! Each file covers one 750 m square of the map. It starts with five counts
//! (all nodes, car nodes, ped nodes, navi nodes, links), then 28-byte path
//! nodes with car nodes first, 14-byte navi nodes, and 4-byte links that
//! name an (area, node) pair. Boat nodes are left out.
use anyhow::{ensure, Context, Result};
use sa_assets::Img;
use std::{cmp::Ordering, collections::BinaryHeap, collections::HashMap, path::Path};

const AREAS: u16 = 64;
const NODE_SIZE: usize = 28;
const NAVI_SIZE: usize = 14;
const LINK_SIZE: usize = 4;
const BOAT: u32 = 1 << 7;

#[derive(Default)]
pub struct RoadGraph {
    nodes: Vec<[f32; 3]>,
    links: Vec<Vec<u32>>,
}

struct Area {
    /// (position, first link, link count, boat) per car node.
    nodes: Vec<([f32; 3], usize, usize, bool)>,
    links: Vec<(u16, u16)>,
}

fn u16at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}
fn u32at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn parse_area(data: &[u8]) -> Result<Area> {
    ensure!(data.len() >= 20, "path file too short");
    let count = |index: usize| u32at(data, index * 4) as usize;
    let (nodes, cars, navis, links) = (count(0), count(1), count(3), count(4));
    ensure!(
        cars <= nodes && nodes <= 65_535 && navis <= 65_535 && links <= 262_144,
        "path file counts out of range"
    );
    let links_at = 20 + nodes * NODE_SIZE + navis * NAVI_SIZE;
    ensure!(
        data.len() >= links_at + links * LINK_SIZE,
        "path file truncated"
    );
    let cars = (0..cars)
        .map(|index| {
            let row = &data[20 + index * NODE_SIZE..20 + (index + 1) * NODE_SIZE];
            let coordinate = |at: usize| i16::from_le_bytes([row[at], row[at + 1]]) as f32 / 8.0;
            let flags = u32at(row, 24);
            (
                [coordinate(8), coordinate(10), coordinate(12)],
                u16at(row, 16) as usize,
                (flags & 15) as usize,
                flags & BOAT != 0,
            )
        })
        .collect();
    let links = (0..links)
        .map(|index| {
            let at = links_at + index * LINK_SIZE;
            (u16at(data, at), u16at(data, at + 2))
        })
        .collect();
    Ok(Area { nodes: cars, links })
}

fn build(areas: &[Option<Area>]) -> RoadGraph {
    let mut graph = RoadGraph::default();
    let mut index = HashMap::new();
    for (area_id, area) in areas.iter().enumerate() {
        for (node_id, node) in area.iter().flat_map(|a| a.nodes.iter().enumerate()) {
            if !node.3 {
                index.insert((area_id as u16, node_id as u16), graph.nodes.len() as u32);
                graph.nodes.push(node.0);
            }
        }
    }
    graph.links = vec![Vec::new(); graph.nodes.len()];
    for (area_id, area) in areas.iter().enumerate() {
        let Some(area) = area else { continue };
        for (node_id, &(_, first, count, _)) in area.nodes.iter().enumerate() {
            let Some(&from) = index.get(&(area_id as u16, node_id as u16)) else {
                continue;
            };
            for &(to_area, to_node) in area.links.iter().skip(first).take(count) {
                if let Some(&to) = index.get(&(to_area, to_node)) {
                    if to != from && !graph.links[from as usize].contains(&to) {
                        graph.links[from as usize].push(to);
                    }
                }
            }
        }
    }
    graph
}

#[derive(PartialEq)]
struct Open(f32, u32);
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0)
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl RoadGraph {
    /// Read the car paths from the game installation.
    pub fn load(game: &Path) -> Result<Self> {
        let mut archive = Img::open(&sa_assets::game_path::resolve(game, "models/gta3.img")?)?;
        let mut areas = Vec::with_capacity(AREAS as usize);
        for area in 0..AREAS {
            let name = format!("nodes{area}.dat");
            areas.push(if archive.has(&name) {
                Some(parse_area(&archive.read(&name)?).with_context(|| format!("reading {name}"))?)
            } else {
                None
            });
        }
        let graph = build(&areas);
        ensure!(!graph.nodes.is_empty(), "no car paths in gta3.img");
        Ok(graph)
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    /// The connected road node closest to `at` (x, y).
    fn nearest(&self, at: [f32; 2]) -> Option<u32> {
        (0..self.nodes.len() as u32)
            .filter(|&n| !self.links[n as usize].is_empty())
            .min_by(|&a, &b| self.flat(a, at).total_cmp(&self.flat(b, at)))
    }
    fn flat(&self, node: u32, at: [f32; 2]) -> f32 {
        let [x, y, _] = self.nodes[node as usize];
        (x - at[0]).hypot(y - at[1])
    }
    /// The shortest drive along the roads from `from` to `to`, as (x, y)
    /// points from the road nearest `from` to the road nearest `to`.
    pub fn route(&self, from: [f32; 2], to: [f32; 2]) -> Option<Vec<[f32; 2]>> {
        let (start, goal) = (self.nearest(from)?, self.nearest(to)?);
        let target = {
            let [x, y, _] = self.nodes[goal as usize];
            [x, y]
        };
        let mut cost = vec![f32::INFINITY; self.nodes.len()];
        let mut came = vec![u32::MAX; self.nodes.len()];
        let mut open = BinaryHeap::new();
        cost[start as usize] = 0.0;
        open.push(Open(self.flat(start, target), start));
        while let Some(Open(_, node)) = open.pop() {
            if node == goal {
                let mut path = vec![node];
                while let Some(&last) = path.last() {
                    let previous = came[last as usize];
                    if previous == u32::MAX {
                        break;
                    }
                    path.push(previous);
                }
                path.reverse();
                return Some(
                    path.into_iter()
                        .map(|n| {
                            let [x, y, _] = self.nodes[n as usize];
                            [x, y]
                        })
                        .collect(),
                );
            }
            let here = self.nodes[node as usize];
            for &next in &self.links[node as usize] {
                let step = self.flat(next, [here[0], here[1]]);
                let total = cost[node as usize] + step;
                if total < cost[next as usize] {
                    cost[next as usize] = total;
                    came[next as usize] = node;
                    open.push(Open(total + self.flat(next, target), next));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path file with car nodes at `positions` linked as `links` (within
    /// this area), plus one ped node that must be ignored.
    fn area_file(area: u16, positions: &[([i16; 2], u32)], links: &[&[u16]]) -> Vec<u8> {
        let nodes = positions.len() + 1;
        let total_links: usize = links.iter().map(|l| l.len()).sum();
        let mut data = Vec::new();
        for value in [nodes, positions.len(), 1, 0, total_links] {
            data.extend((value as u32).to_le_bytes());
        }
        let mut first = 0u16;
        for (index, &([x, y], flags)) in positions.iter().enumerate() {
            data.extend([0; 8]);
            for value in [x * 8, y * 8, 0, 0x7ffe] {
                data.extend(value.to_le_bytes());
            }
            for value in [first, area, index as u16] {
                data.extend(value.to_le_bytes());
            }
            data.extend([0, 0]);
            data.extend((flags | links[index].len() as u32).to_le_bytes());
            first += links[index].len() as u16;
        }
        data.extend([0; NODE_SIZE]);
        for list in links {
            for &node in *list {
                data.extend(area.to_le_bytes());
                data.extend(node.to_le_bytes());
            }
        }
        data.extend([0; 768]);
        data
    }

    #[test]
    fn routes_follow_the_road_instead_of_a_straight_line() {
        // A U-shaped road: 0 -> 1 -> 2 -> 3 around a block, and a boat node
        // (4) that would be a shortcut across the water.
        let file = area_file(
            0,
            &[
                ([0, 0], 0),
                ([0, 100], 0),
                ([100, 100], 0),
                ([100, 0], 0),
                ([50, 0], BOAT),
            ],
            &[&[1, 4], &[0, 2], &[1, 3], &[2, 4], &[0, 3]],
        );
        let area = parse_area(&file).unwrap();
        assert_eq!(area.nodes.len(), 5);
        assert_eq!(area.nodes[1].0, [0.0, 100.0, 0.0]);
        let graph = build(&[Some(area)]);
        assert_eq!(graph.len(), 4);
        let route = graph.route([-3.0, 2.0], [104.0, -1.0]).unwrap();
        assert_eq!(
            route,
            vec![[0.0, 0.0], [0.0, 100.0], [100.0, 100.0], [100.0, 0.0]]
        );
        assert_eq!(graph.route([0.0, 0.0], [1.0, 1.0]).unwrap().len(), 1);
    }

    #[test]
    fn links_across_areas_join_the_network_and_bad_files_are_refused() {
        let west = area_file(0, &[([0, 0], 0)], &[&[]]);
        let mut west = parse_area(&west).unwrap();
        west.nodes[0].2 = 1;
        west.links.push((1, 0));
        let east = parse_area(&area_file(1, &[([800, 0], 0)], &[&[0]])).unwrap();
        let mut east_links = east;
        east_links.links = vec![(0, 0)];
        let graph = build(&[Some(west), Some(east_links)]);
        assert_eq!(
            graph.route([0.0, 0.0], [800.0, 0.0]).unwrap(),
            vec![[0.0, 0.0], [800.0, 0.0]]
        );
        assert!(parse_area(&[0; 10]).is_err());
        let mut truncated = area_file(0, &[([0, 0], 0)], &[&[0]]);
        truncated.truncate(30);
        assert!(parse_area(&truncated).is_err());
        assert!(RoadGraph::default().route([0.0; 2], [1.0; 2]).is_none());
    }
}
