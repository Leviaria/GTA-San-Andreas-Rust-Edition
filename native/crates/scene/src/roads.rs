//! The original car path network (`nodes0.dat` to `nodes63.dat` in
//! `gta3.img`) and shortest routes over it, for the GPS line on the radar.
//!
//! Each file covers one 750 m square of the map. It starts with five counts
//! (all nodes, car nodes, ped nodes, navi nodes, links), then 28-byte path
//! nodes with car nodes first, 14-byte navi nodes, and 4-byte links that
//! name an (area, node) pair. Boat nodes are left out.
//!
//! After the links (and 768 unused bytes) every link names its navi node,
//! which says how many lanes run each way along it. Motorway carriageways
//! are one-way roads, so traffic must only drive them in their direction.
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
    /// Lanes running from a node towards each of its `links`; 0 on the
    /// wrong way down a one-way road.
    lanes: Vec<Vec<u8>>,
}

struct Area {
    /// Index within the file of the first node in `nodes`.
    first_id: usize,
    /// (position, first link, link count, boat) per node.
    nodes: Vec<([f32; 3], usize, usize, bool)>,
    links: Vec<(u16, u16)>,
    /// (area, id) of the navi node of each link, when the file has them.
    navi_links: Vec<(u16, u16)>,
    /// Per navi node: the path node it points towards, then the lanes
    /// running away from that node and the lanes running towards it.
    navis: Vec<((u16, u16), u8, u8)>,
}

fn u16at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}
fn u32at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

#[cfg(test)]
fn parse_area(data: &[u8]) -> Result<Area> {
    parse_area_of(data, false)
}

/// `peds` picks the pedestrian nodes, which follow the car nodes in each file.
fn parse_area_of(data: &[u8], peds: bool) -> Result<Area> {
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
    let range = if peds { cars..nodes } else { 0..cars };
    let first_id = range.start;
    let cars = range
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
    let navis_at = 20 + nodes * NODE_SIZE;
    let navis = (0..navis)
        .map(|index| {
            let row = &data[navis_at + index * NAVI_SIZE..navis_at + (index + 1) * NAVI_SIZE];
            let flags = u32at(row, 10);
            (
                (u16at(row, 4), u16at(row, 6)),
                ((flags >> 8) & 7) as u8,
                ((flags >> 11) & 7) as u8,
            )
        })
        .collect();
    let navi_links_at = links_at + links * LINK_SIZE + 768;
    let navi_links = if data.len() >= navi_links_at + links * 2 {
        (0..links)
            .map(|index| {
                let value = u16at(data, navi_links_at + index * 2);
                (value >> 10, value & 0x3ff)
            })
            .collect()
    } else {
        Vec::new()
    };
    let links = (0..links)
        .map(|index| {
            let at = links_at + index * LINK_SIZE;
            (u16at(data, at), u16at(data, at + 2))
        })
        .collect();
    Ok(Area {
        first_id,
        nodes: cars,
        links,
        navi_links,
        navis,
    })
}

fn build(areas: &[Option<Area>]) -> RoadGraph {
    let mut graph = RoadGraph::default();
    let mut index = HashMap::new();
    for (area_id, area) in areas.iter().enumerate() {
        let Some(area) = area else { continue };
        for (node_id, node) in area.nodes.iter().enumerate() {
            if !node.3 {
                let id = (area.first_id + node_id) as u16;
                index.insert((area_id as u16, id), graph.nodes.len() as u32);
                graph.nodes.push(node.0);
            }
        }
    }
    graph.links = vec![Vec::new(); graph.nodes.len()];
    graph.lanes = vec![Vec::new(); graph.nodes.len()];
    for (area_id, area) in areas.iter().enumerate() {
        let Some(area) = area else { continue };
        for (node_id, &(_, first, count, _)) in area.nodes.iter().enumerate() {
            let id = (area.first_id + node_id) as u16;
            let Some(&from) = index.get(&(area_id as u16, id)) else {
                continue;
            };
            for link in first..first + count {
                let Some(&(to_area, to_node)) = area.links.get(link) else {
                    continue;
                };
                if let Some(&to) = index.get(&(to_area, to_node)) {
                    if to != from && !graph.links[from as usize].contains(&to) {
                        graph.links[from as usize].push(to);
                        let lanes = lanes_along(areas, area, link, (to_area, to_node));
                        graph.lanes[from as usize].push(lanes);
                    }
                }
            }
        }
    }
    graph
}

/// Lanes that run along link number `link` of `area` towards the path node
/// `to`, from its navi node: the navi node points at one end of the link and
/// counts the lanes heading towards that end and away from it. Roads without
/// navi data, or with no lanes either way, get one lane each way.
fn lanes_along(areas: &[Option<Area>], area: &Area, link: usize, to: (u16, u16)) -> u8 {
    let navi = area.navi_links.get(link).and_then(|&(navi_area, id)| {
        areas
            .get(navi_area as usize)?
            .as_ref()?
            .navis
            .get(id as usize)
    });
    match navi {
        Some(&(_, 0, 0)) | None => 1,
        Some(&(target, away, towards)) => {
            if target == to {
                towards
            } else {
                away
            }
        }
    }
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
        Self::load_kind(game, false)
    }
    /// Read the pedestrian paths (pavements, crossings, parks).
    pub fn load_peds(game: &Path) -> Result<Self> {
        Self::load_kind(game, true)
    }
    fn load_kind(game: &Path, peds: bool) -> Result<Self> {
        let mut archive = Img::open(&sa_assets::game_path::resolve(game, "models/gta3.img")?)?;
        let mut areas = Vec::with_capacity(AREAS as usize);
        for area in 0..AREAS {
            let name = format!("nodes{area}.dat");
            areas.push(if archive.has(&name) {
                Some(
                    parse_area_of(&archive.read(&name)?, peds)
                        .with_context(|| format!("reading {name}"))?,
                )
            } else {
                None
            });
        }
        let graph = build(&areas);
        ensure!(!graph.nodes.is_empty(), "no paths in gta3.img");
        Ok(graph)
    }
    /// A graph from explicit nodes and one-way links, for tests.
    pub fn from_parts(nodes: Vec<[f32; 3]>, links: Vec<Vec<u32>>) -> Self {
        assert_eq!(nodes.len(), links.len());
        let lanes = links.iter().map(|l| vec![1; l.len()]).collect();
        Self {
            nodes,
            links,
            lanes,
        }
    }
    /// Make the road from `from` to `to` carry `lanes` lanes in that
    /// direction (0 makes it one-way the other way), for tests.
    pub fn set_lanes(&mut self, from: u32, to: u32, lanes: u8) {
        let at = self.links[from as usize]
            .iter()
            .position(|&n| n == to)
            .expect("no such link");
        self.lanes[from as usize][at] = lanes;
    }
    /// Lanes running from `from` to its neighbour `to`; 0 when that is the
    /// wrong way down a one-way road.
    pub fn lanes(&self, from: u32, to: u32) -> u8 {
        self.links[from as usize]
            .iter()
            .position(|&n| n == to)
            .map_or(0, |at| self.lanes[from as usize][at])
    }
    /// Neighbours of `node` that traffic may drive to from it.
    pub fn exits(&self, node: u32) -> impl Iterator<Item = u32> + '_ {
        self.links[node as usize]
            .iter()
            .zip(&self.lanes[node as usize])
            .filter(|&(_, &lanes)| lanes > 0)
            .map(|(&n, _)| n)
    }
    /// A node's San Andreas position (X east, Y north, Z up).
    pub fn position(&self, node: u32) -> [f32; 3] {
        self.nodes[node as usize]
    }
    pub fn neighbours(&self, node: u32) -> &[u32] {
        &self.links[node as usize]
    }
    /// Connected nodes between `near` and `far` metres (flat distance) from `at`.
    pub fn nodes_between(&self, at: [f32; 2], near: f32, far: f32) -> Vec<u32> {
        (0..self.nodes.len() as u32)
            .filter(|&n| !self.links[n as usize].is_empty())
            .filter(|&n| (near..far).contains(&self.flat(n, at)))
            .collect()
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
            for next in self.exits(node) {
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
    fn ped_paths_use_the_nodes_after_the_car_nodes() {
        // Two car nodes, then the trailing ped node written by `area_file`;
        // give the ped node a link back to itself's neighbour car node 1.
        let mut file = area_file(0, &[([0, 0], 0), ([10, 0], 0)], &[&[1], &[0]]);
        let ped_row = 20 + 2 * NODE_SIZE;
        file[ped_row + 8..ped_row + 10].copy_from_slice(&(40i16 * 8).to_le_bytes());
        let peds = parse_area_of(&file, true).unwrap();
        assert_eq!(peds.first_id, 2);
        assert_eq!(peds.nodes.len(), 1);
        assert_eq!(peds.nodes[0].0[0], 40.0);
        let graph = build(&[Some(peds)]);
        assert_eq!(graph.len(), 1);
        assert_eq!(graph.position(0), [40.0, 0.0, 0.0]);
    }

    /// Add navi nodes (target node, lanes away, lanes towards) to a file
    /// from `area_file` for `area`, and the navi node of every link.
    fn with_navis(
        mut file: Vec<u8>,
        area: u16,
        navis: &[(u16, u32, u32)],
        of_link: &[u16],
    ) -> Vec<u8> {
        let nodes = u32at(&file, 0) as usize;
        file[12..16].copy_from_slice(&(navis.len() as u32).to_le_bytes());
        let mut rows = Vec::new();
        for &(node, away, towards) in navis {
            rows.extend([0; 4]);
            rows.extend(area.to_le_bytes());
            rows.extend(node.to_le_bytes());
            rows.extend([0, 0]);
            rows.extend((away << 8 | towards << 11).to_le_bytes());
        }
        let at = 20 + nodes * NODE_SIZE;
        file.splice(at..at, rows);
        for &navi in of_link {
            file.extend((area << 10 | navi).to_le_bytes());
        }
        file
    }

    #[test]
    fn one_way_roads_only_lead_one_way() {
        // 0 -> 1 is a one-way road with two lanes; 1 - 2 runs both ways.
        let file = area_file(
            0,
            &[([0, 0], 0), ([0, 100], 0), ([0, 200], 0)],
            &[&[1], &[0, 2], &[1]],
        );
        let file = with_navis(file, 0, &[(1, 0, 2), (1, 1, 1)], &[0, 0, 1, 1]);
        let graph = build(&[Some(parse_area(&file).unwrap())]);
        assert_eq!(graph.lanes(0, 1), 2);
        assert_eq!(graph.lanes(1, 0), 0);
        assert_eq!(graph.lanes(1, 2), 1);
        assert_eq!(graph.lanes(2, 1), 1);
        assert_eq!(graph.exits(1).collect::<Vec<_>>(), vec![2]);
        assert_eq!(graph.neighbours(1), &[0, 2]);
        // No route back up the one-way road.
        assert!(graph.route([0.0, 0.0], [0.0, 200.0]).is_some());
        assert!(graph.route([0.0, 200.0], [0.0, 0.0]).is_none());
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
