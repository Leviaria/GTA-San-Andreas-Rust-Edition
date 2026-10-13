//! On-foot movement: CJ jogs by default, walks with Alt and sprints with
//! Shift. Starting, stopping and turning are immediate, the direct control
//! testers preferred over a momentum and stamina model.

use glam::Vec3;

pub const WALK: f32 = 1.6;
pub const RUN: f32 = 4.4;
pub const SPRINT: f32 = 7.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gait {
    Walk,
    Run,
    Sprint,
}

#[derive(Clone, Debug, Default)]
pub struct Locomotion {
    pub velocity: Vec3,
}

impl Locomotion {
    /// `direction` is the requested horizontal direction with length up to 1
    /// (a half-pushed stick asks for half speed). Returns the horizontal
    /// velocity to hand to the collision step.
    pub fn update(&mut self, direction: Vec3, gait: Gait, _dt: f32) -> Vec3 {
        let direction = Vec3::new(direction.x, 0.0, direction.z).clamp_length_max(1.0);
        let top = match gait {
            Gait::Walk => WALK,
            Gait::Run => RUN,
            Gait::Sprint => SPRINT,
        };
        self.velocity = direction * top;
        self.velocity
    }

    pub fn stop(&mut self) {
        self.velocity = Vec3::ZERO;
    }
}

/// The body faces where it is moving straight away.
pub fn turn_toward(_yaw: f32, target: f32, _dt: f32) -> f32 {
    target
}

#[cfg(test)]
mod tests {
    use super::*;
    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn movement_starts_and_stops_immediately() {
        let mut l = Locomotion::default();
        assert_eq!(l.update(Vec3::Z, Gait::Run, DT), Vec3::Z * RUN);
        assert_eq!(l.update(Vec3::ZERO, Gait::Run, DT), Vec3::ZERO);
    }

    #[test]
    fn gaits_and_half_stick_set_the_speed() {
        let mut l = Locomotion::default();
        assert_eq!(l.update(Vec3::X, Gait::Walk, DT).length(), WALK);
        assert_eq!(l.update(Vec3::X, Gait::Sprint, DT).length(), SPRINT);
        assert!((l.update(Vec3::X * 0.5, Gait::Run, DT).length() - RUN * 0.5).abs() < 1e-5);
    }
}
