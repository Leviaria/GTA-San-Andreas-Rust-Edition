//! Ambient city traffic along the game's own car paths (`nodes*.dat`).
//!
//! Traffic cars do not run the full vehicle physics: each one follows the
//! road graph at a cruising speed, keeps to the right-hand lane, slows for
//! turns and stops behind anything in front of it. Cars appear out of the
//! player's immediate surroundings and vanish once far behind, so only a
//! handful exist at a time. That keeps the cost low on weak PCs.
use super::{GpuBatch, State};
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
/// Space kept to whatever is ahead, in metres.
const GAP: f32 = 8.0;
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
    /// things on the road to stop for (the player, peers).
    /// Returns the index in `cars` of a newly added car, if any, so the caller
    /// can give it a mesh.
    pub fn update(
        &mut self,
        roads: &RoadGraph,
        player: Vec3,
        blocked: &[Vec3],
        models: &[usize],
        limit: usize,
        dt: f32,
    ) -> Option<usize> {
        self.cars
            .retain(|car| car.position.distance(player) < DESPAWN);
        self.cars.truncate(limit);
        let positions: Vec<Vec3> = self.cars.iter().map(|c| c.position).collect();
        for (index, car) in self.cars.iter_mut().enumerate() {
            let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
            let ahead = |point: Vec3| {
                let offset = point - car.position;
                let distance = offset.dot(forward);
                distance > 0.5
                    && distance < GAP + car.speed * 0.6
                    && (offset - forward * distance).length() < 2.2
            };
            let obstructed = blocked.iter().copied().any(ahead)
                || positions
                    .iter()
                    .enumerate()
                    .any(|(other, &p)| other != index && ahead(p));
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
        };
        place(&mut car, roads);
        // Never drop a car on top of another one.
        if self
            .cars
            .iter()
            .any(|other| other.position.distance(car.position) < GAP)
        {
            return None;
        }
        self.cars.push(car);
        Some(self.cars.len() - 1)
    }
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
        let mut blocked = vec![player];
        blocked.extend(self.remote_actors.iter().filter(|a| a.visible).map(|a| {
            a.current
                .vehicle
                .map_or(Vec3::from_array(a.current.position), |car| {
                    Vec3::from_array(car.position)
                })
        }));
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
                Some((car, batches)) => self.traffic_meshes.push(TrafficMesh {
                    id,
                    batches: crate::multiplayer::duplicate(&self.device, batches),
                    clearance: car.clearance,
                }),
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
            traffic.update(&roads, Vec3::ZERO, &[blocker], &[3], 1, 0.1);
        }
        assert!(traffic.cars[0].speed < 0.1, "{}", traffic.cars[0].speed);
        assert!(traffic.cars[0].position.distance(blocker) > 1.0);
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
