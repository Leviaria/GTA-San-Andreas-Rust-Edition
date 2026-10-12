//! Rain streaks around the camera while the weather is wet. A fixed set of
//! drops sits in a box that follows the camera; each drop keeps its spot in
//! the world as the camera moves, so walking through rain looks natural.
//! The whole effect is one small dynamic buffer and one draw call.
use glam::Vec3;
use sa_scene::Vertex;

pub const DROPS: usize = 900;
pub const VERTICES: usize = DROPS * 6;
/// Half the width of the box of rain around the camera, in metres.
const REACH: f32 = 22.0;
const HEIGHT: f32 = 18.0;
const SPEED: f32 = 20.0;
const LENGTH: f32 = 0.75;
const WIDTH: f32 = 0.02;
/// A light slant, as if blown by a breeze.
const SLANT: Vec3 = Vec3::new(0.08, 0.0, 0.05);

pub struct Rain {
    /// World-space X/Z anchor and a height offset per drop.
    drops: Vec<Vec3>,
    pub vertices: Vec<Vertex>,
    /// Vertices to draw this frame.
    pub count: u32,
}

impl Default for Rain {
    fn default() -> Self {
        // A small fixed generator: the pattern only needs to look random.
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        let drops = (0..DROPS)
            .map(|_| Vec3::new(next() * REACH * 2.0, next() * HEIGHT, next() * REACH * 2.0))
            .collect();
        Self {
            drops,
            vertices: vec![Vertex::default(); VERTICES],
            count: 0,
        }
    }
}

impl Rain {
    /// `right` is the camera's horizontal right vector; `intensity` 0..1.
    pub fn update(&mut self, eye: Vec3, right: Vec3, intensity: f32, dt: f32) {
        let active = ((DROPS as f32) * intensity.clamp(0.0, 1.0)) as usize;
        self.count = (active * 6) as u32;
        if active == 0 {
            return;
        }
        let half = right * (WIDTH * 0.5);
        let streak = (Vec3::Y + SLANT) * (LENGTH * 0.5);
        let colour = [0.78, 0.82, 0.88, 0.28];
        for (index, drop) in self.drops.iter_mut().take(active).enumerate() {
            drop.y = (drop.y - SPEED * dt).rem_euclid(HEIGHT);
            // Wrap the world anchor into the box around the camera.
            let wrap = |anchor: f32, centre: f32| {
                centre + (anchor - centre).rem_euclid(REACH * 2.0) - REACH
            };
            let centre = Vec3::new(
                wrap(drop.x, eye.x),
                eye.y - HEIGHT * 0.35 + drop.y,
                wrap(drop.z, eye.z),
            );
            let (top, bottom) = (centre + streak, centre - streak);
            let corners = [
                bottom - half,
                bottom + half,
                top + half,
                bottom - half,
                top + half,
                top - half,
            ];
            for (vertex, corner) in self.vertices[index * 6..index * 6 + 6]
                .iter_mut()
                .zip(corners)
            {
                *vertex = Vertex {
                    position: corner.to_array(),
                    uv: [0.5, 0.5],
                    color: colour,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_weather_draws_nothing_and_rain_scales_with_intensity() {
        let mut rain = Rain::default();
        rain.update(Vec3::ZERO, Vec3::X, 0.0, 0.016);
        assert_eq!(rain.count, 0);
        rain.update(Vec3::ZERO, Vec3::X, 0.5, 0.016);
        assert_eq!(rain.count as usize, DROPS / 2 * 6);
        rain.update(Vec3::ZERO, Vec3::X, 1.0, 0.016);
        assert_eq!(rain.count as usize, VERTICES);
    }

    #[test]
    fn drops_stay_around_the_camera_and_fall() {
        let mut rain = Rain::default();
        let eye = Vec3::new(1000.0, 50.0, -400.0);
        rain.update(eye, Vec3::X, 1.0, 0.016);
        for vertex in &rain.vertices {
            let p = Vec3::from_array(vertex.position);
            assert!((p.x - eye.x).abs() <= REACH + 0.1);
            assert!((p.z - eye.z).abs() <= REACH + 0.1);
            assert!((p.y - eye.y).abs() <= HEIGHT);
        }
        let before = rain.drops[0].y;
        rain.update(eye, Vec3::X, 1.0, 0.01);
        let fell = (before - rain.drops[0].y).rem_euclid(HEIGHT);
        assert!((fell - SPEED * 0.01).abs() < 1e-3);
    }

    #[test]
    fn a_drop_keeps_its_world_spot_while_the_camera_moves() {
        let mut rain = Rain::default();
        rain.update(Vec3::ZERO, Vec3::X, 1.0, 0.0);
        let first = rain.vertices[0].position;
        rain.update(Vec3::new(0.5, 0.0, 0.5), Vec3::X, 1.0, 0.0);
        let moved = rain.vertices[0].position;
        assert!((first[0] - moved[0]).abs() < 1e-4 || (first[0] - moved[0]).abs() > 40.0);
        assert!((first[2] - moved[2]).abs() < 1e-4 || (first[2] - moved[2]).abs() > 40.0);
    }
}
