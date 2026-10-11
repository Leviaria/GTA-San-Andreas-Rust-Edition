//! The GPS line to a script's route blip (`SetBlipRoute`), along the
//! original car paths. Without them (no game files) it is a straight line.
use sa_scene::roads::RoadGraph;
use std::time::{Duration, Instant};

/// Further than this from the line counts as a wrong turn.
const OFF_ROUTE: f32 = 35.0;
/// Shortest wait between two searches.
const RECHECK: Duration = Duration::from_millis(750);

#[derive(Default)]
pub struct Gps {
    target: Option<[f32; 2]>,
    path: Vec<[f32; 2]>,
    searched: Option<Instant>,
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
/// Distance from `p` to the segment `a`-`b`.
fn to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    distance(p, [a[0] + dx * t, a[1] + dy * t])
}

impl Gps {
    /// Follow the player at `at` towards `target` (SA world x, y).
    pub fn update(&mut self, roads: Option<&RoadGraph>, at: [f32; 2], target: Option<[f32; 2]>) {
        let Some(target) = target else {
            *self = Self::default();
            return;
        };
        let moved = self.target.is_none_or(|old| distance(old, target) > 1.0);
        if !moved {
            self.trim(at);
        }
        let lost = self.path.is_empty() || self.off_route(at);
        let waited = self.searched.is_none_or(|t| t.elapsed() >= RECHECK);
        if moved || (lost && waited) {
            self.target = Some(target);
            self.searched = Some(Instant::now());
            self.path = roads
                .and_then(|roads| roads.route(at, target))
                .unwrap_or_default();
            self.trim(at);
        }
    }
    /// Drop the part of the route already driven past.
    fn trim(&mut self, at: [f32; 2]) {
        let ahead = self.path.len().min(40);
        if ahead < 2 {
            return;
        }
        let nearest = (0..ahead - 1)
            .min_by(|&a, &b| {
                to_segment(at, self.path[a], self.path[a + 1]).total_cmp(&to_segment(
                    at,
                    self.path[b],
                    self.path[b + 1],
                ))
            })
            .unwrap_or(0);
        self.path.drain(..nearest);
        // Past the first point of the nearest segment: start at the next.
        if self.path.len() >= 2
            && distance(at, self.path[1]) < distance(self.path[0], self.path[1])
            && distance(at, self.path[0]) > 1.0
        {
            self.path.remove(0);
        }
    }
    fn off_route(&self, at: [f32; 2]) -> bool {
        let first = self
            .path
            .first()
            .map_or(f32::INFINITY, |&p| distance(at, p));
        let along = self
            .path
            .windows(2)
            .take(40)
            .map(|w| to_segment(at, w[0], w[1]))
            .fold(first, f32::min);
        along > OFF_ROUTE
    }
    /// The line to draw, from the player to the target.
    pub fn line(&self, at: [f32; 2]) -> Vec<[f32; 2]> {
        let Some(target) = self.target else {
            return Vec::new();
        };
        let mut line = Vec::with_capacity(self.path.len() + 2);
        line.push(at);
        line.extend(&self.path);
        line.push(target);
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_roads_the_line_is_straight_and_clears_with_the_target() {
        let mut gps = Gps::default();
        gps.update(None, [0.0, 0.0], Some([100.0, 50.0]));
        assert_eq!(gps.line([1.0, 1.0]), vec![[1.0, 1.0], [100.0, 50.0]]);
        gps.update(None, [0.0, 0.0], None);
        assert!(gps.line([0.0, 0.0]).is_empty());
    }

    #[test]
    fn trimming_drops_the_driven_part_and_wrong_turns_are_noticed() {
        let mut gps = Gps {
            target: Some([300.0, 0.0]),
            path: vec![[0.0, 0.0], [100.0, 0.0], [200.0, 0.0], [300.0, 0.0]],
            searched: Some(Instant::now()),
        };
        gps.trim([150.0, 2.0]);
        assert_eq!(gps.path, vec![[200.0, 0.0], [300.0, 0.0]]);
        assert!(!gps.off_route([180.0, 10.0]));
        assert!(gps.off_route([180.0, 80.0]));
        // A wrong turn searches again (here without roads: a straight line).
        gps.searched = None;
        gps.update(None, [180.0, 80.0], Some([300.0, 0.0]));
        assert!(gps.path.is_empty());
        assert_eq!(gps.line([180.0, 80.0]).len(), 2);
    }
}
