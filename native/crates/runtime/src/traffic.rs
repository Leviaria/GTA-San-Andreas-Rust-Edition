//! Ambient city traffic along the game's own car paths (`nodes*.dat`).
//!
//! Traffic cars do not run the full vehicle physics: each one follows the
//! road graph at a cruising speed, keeps to the right-hand lane, slows for
//! turns and stops behind anything in front of it. Cars appear out of the
//! player's immediate surroundings and vanish once far behind, so only a
//! handful exist at a time. That keeps the cost low on weak PCs.
use super::{GpuBatch, State};
use crate::car_contacts::Footprint;
use crate::streaming::ORIGIN;
use glam::Vec3;
use sa_scene::roads::RoadGraph;

/// Offset from the road centre line to the middle of the right-hand lane.
const LANE: f32 = 2.4;
const CRUISE: f32 = 13.0;
const ACCELERATION: f32 = 3.5;
const BRAKING: f32 = 9.0;
/// Cars appear this far from the player...
const SPAWN_NEAR: f32 = 70.0;
const SPAWN_FAR: f32 = 150.0;
/// ...and vanish beyond this.
const DESPAWN: f32 = 200.0;
/// Space kept to whatever is ahead, in metres, between centres at spawn and
/// from the front bumper while driving.
const GAP: f32 = 8.0;
const LOOK_AHEAD: f32 = 3.5;
/// Cars inside this radius of a junction node keep it to themselves.
const JUNCTION: f32 = 7.0;
/// After waiting this long at a junction a driver goes anyway, which breaks
/// rare circles of cars each waiting on the next.
const PATIENCE: f32 = 6.0;
/// How long a car waits after the player knocks into it.
const KNOCK_HOLD: f32 = 2.5;
const SPAWN_INTERVAL: f32 = 0.6;

#[derive(Clone, Debug)]
pub struct TrafficCar {
    /// Stable while the car exists, so the caller can keep its mesh.
    pub id: u64,
    /// Car catalog index this car is drawn with.
    pub catalog: usize,
    from: u32,
    to: u32,
    /// Metres travelled along the current link.
    along: f32,
    pub speed: f32,
    /// World position of the car's centre on the road, and its heading.
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Half width, half length and height, from the model once it has a mesh.
    pub size: [f32; 3],
    /// Seconds left standing still after a knock.
    hold: f32,
    /// Seconds spent waiting for other cars.
    waited: f32,
}

impl TrafficCar {
    fn footprint(&self) -> Footprint {
        Footprint::car(self.position, self.yaw, self.size)
    }
    /// The road just in front of the bumper this car must find clear.
    fn path_ahead(&self) -> Footprint {
        let look = LOOK_AHEAD + self.speed * 0.6;
        Footprint {
            center: self.position + self.forward() * (self.size[1] + look / 2.0),
            half: glam::Vec2::new(self.size[0], look / 2.0),
            ..self.footprint()
        }
    }
    fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }
    /// The player ran into this car: a hard enough hit makes the driver stop
    /// for a moment, as SA drivers do.
    pub fn knock(&mut self, impact: f32) {
        if impact > 2.0 {
            self.speed = 0.0;
            self.hold = KNOCK_HOLD;
        }
    }
}

#[derive(Default)]
pub struct Traffic {
    pub cars: Vec<TrafficCar>,
    seed: u32,
    since_spawn: f32,
    next_id: u64,
}

/// World position (X east, Y up, Z south) of a road node.
fn world(roads: &RoadGraph, node: u32) -> Vec3 {
    let [x, y, z] = roads.position(node);
    Vec3::new(x - ORIGIN[0], z, ORIGIN[1] - y)
}

impl Traffic {
    fn random(&mut self) -> u32 {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.seed >> 8
    }

    /// Move every car, retire distant ones and add new ones up to `limit`.
    /// `models` are the catalog indices traffic may use; `blocked` are other
    /// things on the road to stop for (the player, peers, parked cars).
    /// Returns the index in `cars` of a newly added car, if any, so the caller
    /// can give it a mesh.
    pub fn update(
        &mut self,
        roads: &RoadGraph,
        player: Vec3,
        blocked: &[Footprint],
        models: &[usize],
        limit: usize,
        dt: f32,
    ) -> Option<usize> {
        self.cars
            .retain(|car| car.position.distance(player) < DESPAWN);
        self.cars.truncate(limit);
        let bodies: Vec<Footprint> = self.cars.iter().map(TrafficCar::footprint).collect();
        let paths: Vec<Footprint> = self.cars.iter().map(TrafficCar::path_ahead).collect();
        let junctions: Vec<Junction> = self.cars.iter().map(|c| junction(c, roads)).collect();
        let cars = &self.cars;
        let waits_for = |me: usize, other: usize| {
            other != me
                && (paths[me].push_out_of(&bodies[other]).is_some()
                    || (cars[me].waited < PATIENCE && gives_way(me, other, cars, &junctions)))
        };
        let obstructed: Vec<bool> = (0..cars.len())
            .map(|me| {
                blocked.iter().any(|b| paths[me].push_out_of(b).is_some())
                    // Two cars waiting on each other would wait forever:
                    // the older one goes first.
                    || (0..cars.len()).any(|other| {
                        waits_for(me, other)
                            && !(waits_for(other, me) && cars[me].id < cars[other].id)
                    })
            })
            .collect();
        for (car, obstructed) in self.cars.iter_mut().zip(obstructed) {
            car.hold = (car.hold - dt).max(0.0);
            car.waited = if obstructed { car.waited + dt } else { 0.0 };
            let obstructed = obstructed || car.hold > 0.0;
            step(car, roads, obstructed, dt, &mut self.seed);
        }
        self.since_spawn += dt;
        if self.cars.len() >= limit || models.is_empty() || self.since_spawn < SPAWN_INTERVAL {
            return None;
        }
        self.since_spawn = 0.0;
        let at = [player.x + ORIGIN[0], ORIGIN[1] - player.z];
        let candidates = roads.nodes_between(at, SPAWN_NEAR, SPAWN_FAR);
        if candidates.is_empty() {
            return None;
        }
        let from = candidates[self.random() as usize % candidates.len()];
        let next = roads.neighbours(from);
        let to = next[self.random() as usize % next.len()];
        let catalog = models[self.random() as usize % models.len()];
        self.next_id += 1;
        let mut car = TrafficCar {
            id: self.next_id,
            catalog,
            from,
            to,
            along: 0.0,
            speed: CRUISE * 0.6,
            position: world(roads, from),
            yaw: 0.0,
            pitch: 0.0,
            size: sa_scene::vehicle::DEFAULT_SIZE,
            hold: 0.0,
            waited: 0.0,
        };
        place(&mut car, roads);
        // Never drop a car on top of another one, or on someone.
        if self
            .cars
            .iter()
            .any(|other| other.position.distance(car.position) < GAP)
            || blocked
                .iter()
                .any(|b| car.footprint().push_out_of(b).is_some())
        {
            return None;
        }
        self.cars.push(car);
        Some(self.cars.len() - 1)
    }
}

/// Where a car stands relative to the junction at the end of its link.
#[derive(Clone, Copy)]
struct Junction {
    node: Option<u32>,
    at: Vec3,
    /// Metres left to the node.
    distance: f32,
    /// Close enough that it must decide whether to go.
    approaching: bool,
}

fn junction(car: &TrafficCar, roads: &RoadGraph) -> Junction {
    let stopping = car.speed * car.speed / (2.0 * BRAKING);
    let reach = JUNCTION + stopping + 2.0;
    // A plain bend has two neighbours; anything else is a crossing or a
    // dead end where cars turn across each other. Look past bends so a car
    // starts braking in time even when the junction is a link or two away.
    let (mut previous, mut node) = (car.from, car.to);
    let mut distance = length_of(roads, car.from, car.to) - car.along;
    while roads.neighbours(node).len() == 2 && distance < reach {
        let next = roads
            .neighbours(node)
            .iter()
            .copied()
            .find(|&n| n != previous);
        let Some(next) = next else { break };
        distance += length_of(roads, node, next);
        (previous, node) = (node, next);
    }
    let found = roads.neighbours(node).len() != 2;
    Junction {
        node: found.then_some(node),
        at: world(roads, node),
        distance,
        approaching: found && distance < reach,
    }
}

/// Junctions take one car at a time, like a four-way stop: a car waits while
/// another is inside, or while another waiting car is nearer the node.
fn gives_way(me: usize, other: usize, cars: &[TrafficCar], junctions: &[Junction]) -> bool {
    let mine = junctions[me];
    let Some(node) = mine.node.filter(|_| mine.approaching) else {
        return false;
    };
    let inside = |car: &TrafficCar| {
        let offset = car.position - mine.at;
        Vec3::new(offset.x, 0.0, offset.z).length() < JUNCTION
    };
    if inside(&cars[me]) {
        return false;
    }
    let (car, theirs) = (&cars[other], junctions[other]);
    let bound_here = theirs.node == Some(node);
    // Still crossing: heading for this node or just turned out of it.
    (inside(car) && (bound_here || car.from == node))
        || (bound_here
            && theirs.approaching
            && (theirs.distance, car.id) < (mine.distance, cars[me].id))
}

/// Advance one car along its link, turning onto the next link at its end.
fn step(car: &mut TrafficCar, roads: &RoadGraph, obstructed: bool, dt: f32, seed: &mut u32) {
    let (a, b) = (world(roads, car.from), world(roads, car.to));
    let length = (b - a).length().max(0.1);
    // Ease off before a sharp corner at the end of this link.
    let turn = roads
        .neighbours(car.to)
        .iter()
        .filter(|&&n| n != car.from)
        .map(|&n| {
            let next = (world(roads, n) - b).normalize_or_zero();
            1.0 - next.dot((b - a).normalize_or_zero()).max(0.0)
        })
        .fold(0.0_f32, f32::max);
    let near_end = ((length - car.along) / 20.0).clamp(0.0, 1.0);
    let target = if obstructed {
        0.0
    } else {
        CRUISE * (1.0 - turn * 0.6 * (1.0 - near_end))
    };
    car.speed = if car.speed < target {
        (car.speed + ACCELERATION * dt).min(target)
    } else {
        (car.speed - BRAKING * dt).max(target)
    };
    car.along += car.speed * dt;
    while car.along >= length_of(roads, car.from, car.to) {
        car.along -= length_of(roads, car.from, car.to);
        let options: Vec<u32> = roads
            .neighbours(car.to)
            .iter()
            .copied()
            .filter(|&n| n != car.from)
            .collect();
        let next = if options.is_empty() {
            car.from // Dead end: turn around.
        } else {
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            options[(*seed >> 8) as usize % options.len()]
        };
        car.from = car.to;
        car.to = next;
    }
    place(car, roads);
}

fn length_of(roads: &RoadGraph, from: u32, to: u32) -> f32 {
    (world(roads, to) - world(roads, from)).length().max(0.1)
}

/// Set a car's position, heading and grade from where it is on its link.
fn place(car: &mut TrafficCar, roads: &RoadGraph) {
    let (a, b) = (world(roads, car.from), world(roads, car.to));
    let direction = (b - a).normalize_or_zero();
    let flat = Vec3::new(direction.x, 0.0, direction.z).normalize_or_zero();
    let right = Vec3::new(-flat.z, 0.0, flat.x);
    car.position = a + direction * car.along + right * LANE;
    if flat != Vec3::ZERO {
        car.yaw = flat.x.atan2(flat.z);
        car.pitch = direction
            .y
            .atan2(Vec3::new(direction.x, 0.0, direction.z).length());
    }
}

/// A traffic car's own copy of its mesh, moved on the CPU like remote players.
pub(super) struct TrafficMesh {
    id: u64,
    pub batches: Vec<GpuBatch>,
    clearance: f32,
}

impl State {
    /// Catalog indices of the civilian cars from the player's vehicles.ide.
    fn refresh_traffic_models(&mut self) {
        let civilian: std::collections::HashSet<String> =
            sa_scene::vehicle_list::original_cars(&self.resource_game_dir)
                .into_iter()
                .filter(|car| car.is_civilian())
                .map(|car| car.model)
                .collect();
        self.traffic_models = (0..self.menu.cars.len())
            .filter(|&index| {
                self.menu
                    .cars
                    .get(index)
                    .and_then(|name| self.original_car_models.get(name))
                    .is_some_and(|model| civilian.contains(model))
            })
            .collect();
        self.traffic_models_for = self.menu.cars.len();
    }

    pub(super) fn update_traffic(&mut self, dt: f32) {
        if self.traffic_models_for != self.menu.cars.len() {
            self.refresh_traffic_models();
        }
        let Some(roads) = self.roads.as_ref() else {
            self.traffic.cars.clear();
            self.traffic_meshes.clear();
            return;
        };
        let limit = if self.interior == 0 {
            self.menu.settings.traffic_cars as usize
        } else {
            0
        };
        let player = match (&self.car, &self.player) {
            (Some((car, _)), _) if self.driving => car.position,
            (_, Some(player)) if self.walking => player.feet,
            _ => self.position,
        };
        // The player's car counts whether it is driven or parked.
        let mut blocked: Vec<Footprint> = self
            .car
            .as_ref()
            .map(|(car, _)| {
                Footprint::car(car.position - Vec3::Y * car.clearance, car.yaw, car.size)
            })
            .into_iter()
            .collect();
        if let Some(player) = self
            .player
            .as_ref()
            .filter(|_| self.walking && !self.driving)
        {
            blocked.push(Footprint::person(player.feet));
        }
        for actor in &self.remote_actors {
            if let Some(car) = actor.current.vehicle.filter(|_| actor.car_visible) {
                blocked.push(Footprint::car(
                    Vec3::from_array(car.position),
                    car.yaw,
                    actor.car_size,
                ));
            }
            if actor.visible && !actor.current.driving {
                blocked.push(Footprint::person(Vec3::from_array(actor.current.position)));
            }
        }
        // Stop for people crossing too.
        blocked.extend(self.crowd.walkers.iter().map(|w| Footprint::person(w.feet)));
        let added = self
            .traffic
            .update(roads, player, &blocked, &self.traffic_models, limit, dt);
        let cars = &self.traffic.cars;
        self.traffic_meshes
            .retain(|mesh| cars.iter().any(|car| car.id == mesh.id));
        if let Some(index) = added {
            let (id, catalog) = (
                self.traffic.cars[index].id,
                self.traffic.cars[index].catalog,
            );
            self.ensure_car_loaded(catalog);
            let source = if catalog == self.active_car {
                self.car.as_ref()
            } else {
                self.car_catalog.get(catalog).and_then(|c| c.as_ref())
            };
            match source {
                Some((car, batches)) => {
                    self.traffic.cars[index].size = car.size;
                    self.traffic_meshes.push(TrafficMesh {
                        id,
                        batches: crate::multiplayer::duplicate(&self.device, batches),
                        clearance: car.clearance,
                    })
                }
                None => {
                    self.traffic.cars.remove(index);
                }
            }
        }
        for mesh in &self.traffic_meshes {
            let Some(car) = self.traffic.cars.iter().find(|c| c.id == mesh.id) else {
                continue;
            };
            let rotation = sa_scene::vehicle::model_rotation(car.yaw, car.pitch, 0.0);
            let offset = car.position + Vec3::Y * mesh.clearance;
            for batch in &mesh.batches {
                let mut raw = batch.base.clone();
                for v in raw.as_chunks_mut::<9>().0.iter_mut() {
                    let p = rotation * Vec3::new(v[0], v[1], v[2]) + offset;
                    v[..3].copy_from_slice(&p.to_array());
                }
                self.queue
                    .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight road north from the origin with a dead end at each side.
    fn road() -> RoadGraph {
        let o = ORIGIN;
        let nodes = (0..40)
            .map(|i| [o[0], o[1] + i as f32 * 10.0, 10.0])
            .collect::<Vec<_>>();
        let links = (0..40u32)
            .map(|i| {
                let mut l = Vec::new();
                if i > 0 {
                    l.push(i - 1);
                }
                if i < 39 {
                    l.push(i + 1);
                }
                l
            })
            .collect();
        RoadGraph::from_parts(nodes, links)
    }

    fn fill(traffic: &mut Traffic, roads: &RoadGraph, player: Vec3) {
        for _ in 0..200 {
            traffic.update(roads, player, &[], &[3, 4], 4, 0.1);
        }
    }

    #[test]
    fn cars_spawn_away_from_the_player_and_keep_right() {
        let roads = road();
        let mut traffic = Traffic::default();
        let player = Vec3::ZERO;
        let added = traffic.update(&roads, player, &[], &[3], 4, 1.0);
        assert_eq!(added, Some(0));
        assert_eq!(traffic.cars[0].catalog, 3);
        let car = &traffic.cars[0];
        assert!(car.position.distance(player) >= SPAWN_NEAR - LANE - 1.0);
        // Heading north (-Z) keeps right, which is east (+X); south keeps west.
        let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        assert!((car.position.x - right.x * LANE).abs() < 1e-3);
    }

    #[test]
    fn traffic_stays_within_the_limit_and_drives() {
        let roads = road();
        let mut traffic = Traffic::default();
        fill(&mut traffic, &roads, Vec3::ZERO);
        assert!(!traffic.cars.is_empty() && traffic.cars.len() <= 4);
        assert!(traffic.cars.iter().any(|c| c.speed > 5.0));
        assert!(traffic.cars.iter().all(|c| [3, 4].contains(&c.catalog)));
        // Lowering the limit retires cars at once.
        traffic.update(&roads, Vec3::ZERO, &[], &[3], 1, 0.1);
        assert!(traffic.cars.len() <= 1);
    }

    #[test]
    fn a_car_stops_for_the_player_in_its_lane() {
        let roads = road();
        let mut traffic = Traffic::default();
        traffic.update(&roads, Vec3::ZERO, &[], &[3], 1, 1.0);
        let car = traffic.cars[0].clone();
        let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
        let blocker = car.position + forward * 6.0;
        for _ in 0..30 {
            traffic.update(
                &roads,
                Vec3::ZERO,
                &[Footprint::person(blocker)],
                &[3],
                1,
                0.1,
            );
        }
        assert!(traffic.cars[0].speed < 0.1, "{}", traffic.cars[0].speed);
        assert!(traffic.cars[0].position.distance(blocker) > 1.0);
    }

    /// Two roads crossing at node 0: one along X, one along Z.
    fn crossing() -> RoadGraph {
        let o = ORIGIN;
        let mut nodes = vec![[o[0], o[1], 10.0]];
        let mut links: Vec<Vec<u32>> = vec![vec![]];
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let mut previous = 0u32;
            for i in 1..=8 {
                let id = nodes.len() as u32;
                nodes.push([
                    o[0] + dx * i as f32 * 10.0,
                    o[1] + dy * i as f32 * 10.0,
                    10.0,
                ]);
                links.push(vec![previous]);
                links[previous as usize].push(id);
                previous = id;
            }
        }
        RoadGraph::from_parts(nodes, links)
    }

    #[test]
    fn a_car_stops_behind_a_parked_car_in_its_lane() {
        let roads = road();
        let mut traffic = Traffic::default();
        traffic.update(&roads, Vec3::ZERO, &[], &[3], 1, 1.0);
        let car = traffic.cars[0].clone();
        let parked = Footprint::car(
            car.position + car.forward() * 9.0,
            car.yaw + 0.3,
            sa_scene::vehicle::DEFAULT_SIZE,
        );
        for _ in 0..40 {
            traffic.update(&roads, Vec3::ZERO, &[parked], &[3], 1, 0.1);
        }
        assert!(traffic.cars[0].speed < 0.1);
        assert_eq!(traffic.cars[0].footprint().push_out_of(&parked), None);
    }

    #[test]
    fn traffic_never_drives_through_traffic_at_a_crossing() {
        let roads = crossing();
        for seed in 0..8 {
            let mut traffic = Traffic {
                seed,
                ..Traffic::default()
            };
            let mut moved = 0.0;
            for _ in 0..3000 {
                let before: Vec<Vec3> = traffic.cars.iter().map(|c| c.position).collect();
                traffic.update(&roads, Vec3::ZERO, &[], &[3], 6, 0.05);
                for (a, car) in traffic.cars.iter().enumerate() {
                    for other in &traffic.cars[a + 1..] {
                        let depth = car
                            .footprint()
                            .push_out_of(&other.footprint())
                            .map_or(0.0, |p| p.length());
                        // Dead-end U-turns can graze; nothing may drive into another car.
                        assert!(depth < 0.6, "overlap {depth}: {car:?} {other:?}");
                    }
                }
                moved += traffic
                    .cars
                    .iter()
                    .zip(&before)
                    .map(|(c, b)| c.position.distance(*b))
                    .sum::<f32>();
            }
            // Nobody deadlocked: traffic kept flowing.
            assert!(moved > 4000.0, "seed {seed}: {moved}");
        }
    }

    #[test]
    fn a_hard_knock_makes_the_driver_wait() {
        let roads = road();
        let mut traffic = Traffic::default();
        fill(&mut traffic, &roads, Vec3::ZERO);
        traffic.cars[0].knock(8.0);
        for _ in 0..10 {
            traffic.update(&roads, Vec3::ZERO, &[], &[3], 4, 0.1);
        }
        assert_eq!(traffic.cars[0].speed, 0.0);
        for _ in 0..40 {
            traffic.update(&roads, Vec3::ZERO, &[], &[3], 4, 0.1);
        }
        assert!(traffic.cars[0].hold == 0.0);
    }

    #[test]
    fn far_cars_are_retired() {
        let roads = road();
        let mut traffic = Traffic::default();
        fill(&mut traffic, &roads, Vec3::ZERO);
        traffic.update(&roads, Vec3::new(5000.0, 0.0, 5000.0), &[], &[], 4, 0.1);
        assert!(traffic.cars.is_empty());
    }
}
