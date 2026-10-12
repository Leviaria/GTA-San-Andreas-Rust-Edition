//! On-foot movement tuned to feel like San Andreas: CJ jogs by default, walks
//! when asked, sprints on a stamina budget, and builds up and sheds speed over
//! a few tenths of a second instead of starting and stopping instantly.
//! The numbers come from watching the game, not from its code.

use glam::Vec3;

pub const WALK: f32 = 1.6;
pub const RUN: f32 = 4.4;
pub const SPRINT: f32 = 7.6;
/// Speed gained per second while the stick is held.
const ACCELERATION: f32 = 14.0;
/// Speed lost per second once the stick is released; SA stops a little faster
/// than it starts.
const BRAKING: f32 = 18.0;
/// Seconds of continuous sprinting on a full bar.
const STAMINA_SECONDS: f32 = 6.0;
/// Seconds to refill an empty bar while not sprinting.
const RECOVERY_SECONDS: f32 = 4.0;
/// Sprinting stays blocked until the bar refills this far after running out.
const RESUME_AT: f32 = 0.25;
/// Radians per second the body turns toward where it is moving.
const TURN_RATE: f32 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gait {
    Walk,
    Run,
    Sprint,
}

#[derive(Clone, Debug)]
pub struct Locomotion {
    pub velocity: Vec3,
    /// 0 = empty, 1 = full.
    pub stamina: f32,
    winded: bool,
}

impl Default for Locomotion {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            stamina: 1.0,
            winded: false,
        }
    }
}

impl Locomotion {
    /// `direction` is the requested horizontal direction with length up to 1
    /// (a half-pushed stick asks for half speed). Returns the horizontal
    /// velocity to hand to the collision step.
    pub fn update(&mut self, direction: Vec3, gait: Gait, dt: f32) -> Vec3 {
        let direction = Vec3::new(direction.x, 0.0, direction.z).clamp_length_max(1.0);
        let moving = direction.length_squared() > 1e-4;
        let sprinting = gait == Gait::Sprint && moving && !self.winded;
        if sprinting {
            self.stamina -= dt / STAMINA_SECONDS;
            if self.stamina <= 0.0 {
                self.stamina = 0.0;
                self.winded = true;
            }
        } else {
            self.stamina = (self.stamina + dt / RECOVERY_SECONDS).min(1.0);
            if self.winded && self.stamina >= RESUME_AT {
                self.winded = false;
            }
        }
        let top = match gait {
            Gait::Walk => WALK,
            Gait::Sprint if sprinting => SPRINT,
            _ => RUN,
        };
        let target = direction * top;
        let rate = if target.length_squared() >= self.velocity.length_squared() {
            ACCELERATION
        } else {
            BRAKING
        };
        let change = target - self.velocity;
        let step = rate * dt;
        self.velocity = if change.length() <= step {
            target
        } else {
            self.velocity + change.normalize() * step
        };
        self.velocity
    }

    pub fn stop(&mut self) {
        self.velocity = Vec3::ZERO;
    }
}

/// Turn `yaw` toward `target` by at most `TURN_RATE * dt`, the short way round.
pub fn turn_toward(yaw: f32, target: f32, dt: f32) -> f32 {
    let difference = (target - yaw).sin().atan2((target - yaw).cos());
    let step = TURN_RATE * dt;
    yaw + difference.clamp(-step, step)
}

/// The ped.ifp clip that matches a horizontal speed.
pub fn clip_for_speed(speed: f32) -> &'static str {
    if speed < 0.15 {
        "idle_stance"
    } else if speed < (WALK + RUN) * 0.5 {
        "walk_player"
    } else if speed < (RUN + SPRINT) * 0.5 {
        "run_player"
    } else {
        "sprint_civi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DT: f32 = 1.0 / 60.0;

    fn run(locomotion: &mut Locomotion, direction: Vec3, gait: Gait, seconds: f32) -> Vec3 {
        let mut velocity = Vec3::ZERO;
        for _ in 0..(seconds / DT).round() as usize {
            velocity = locomotion.update(direction, gait, DT);
        }
        velocity
    }

    #[test]
    fn speed_builds_up_and_settles_at_the_jog() {
        let mut l = Locomotion::default();
        let first = l.update(Vec3::Z, Gait::Run, DT);
        assert!(first.length() < RUN * 0.1, "no instant start: {first}");
        let settled = run(&mut l, Vec3::Z, Gait::Run, 1.0);
        assert!((settled.length() - RUN).abs() < 1e-3);
        assert!(settled.z > 0.0);
    }

    #[test]
    fn releasing_the_stick_stops_quickly_but_not_instantly() {
        let mut l = Locomotion::default();
        run(&mut l, Vec3::Z, Gait::Run, 1.0);
        let next = l.update(Vec3::ZERO, Gait::Run, DT);
        assert!(next.length() > 0.0);
        let stopped = run(&mut l, Vec3::ZERO, Gait::Run, 0.4);
        assert_eq!(stopped, Vec3::ZERO);
    }

    #[test]
    fn gaits_reach_their_own_speeds() {
        for (gait, speed) in [(Gait::Walk, WALK), (Gait::Run, RUN), (Gait::Sprint, SPRINT)] {
            let mut l = Locomotion::default();
            let v = run(&mut l, Vec3::X, gait, 1.5);
            assert!(
                (v.length() - speed).abs() < 1e-3,
                "{gait:?}: {}",
                v.length()
            );
        }
    }

    #[test]
    fn a_half_pushed_stick_moves_at_half_speed() {
        let mut l = Locomotion::default();
        let v = run(&mut l, Vec3::X * 0.5, Gait::Run, 1.0);
        assert!((v.length() - RUN * 0.5).abs() < 1e-3);
    }

    #[test]
    fn sprinting_runs_out_and_needs_a_breather() {
        let mut l = Locomotion::default();
        run(&mut l, Vec3::Z, Gait::Sprint, STAMINA_SECONDS + 2.0 * DT);
        assert!(l.winded && l.stamina < 0.01, "{}", l.stamina);
        let winded = run(&mut l, Vec3::Z, Gait::Sprint, 0.25);
        assert!(
            (winded.length() - RUN).abs() < 1e-3,
            "drops to a jog when winded"
        );
        // Recovering while winded: a short rest is not enough; a longer one lets the sprint resume.
        run(&mut l, Vec3::Z, Gait::Run, 0.25);
        assert!((run(&mut l, Vec3::Z, Gait::Sprint, 0.25).length() - RUN).abs() < 1e-3);
        run(&mut l, Vec3::ZERO, Gait::Run, RECOVERY_SECONDS);
        assert!((run(&mut l, Vec3::Z, Gait::Sprint, 1.0).length() - SPRINT).abs() < 1e-3);
    }

    #[test]
    fn turning_takes_the_short_way_and_is_rate_limited() {
        let yaw = turn_toward(3.0, -3.0, DT);
        assert!(yaw > 3.0, "wraps through pi instead of spinning back");
        let mut yaw = 0.0;
        for _ in 0..60 {
            yaw = turn_toward(yaw, 1.5, DT);
        }
        assert!((yaw - 1.5).abs() < 1e-4);
        assert!(turn_toward(0.0, 1.5, DT) < 0.2);
    }

    #[test]
    fn clips_follow_speed() {
        assert_eq!(clip_for_speed(0.0), "idle_stance");
        assert_eq!(clip_for_speed(WALK), "walk_player");
        assert_eq!(clip_for_speed(RUN), "run_player");
        assert_eq!(clip_for_speed(SPRINT), "sprint_civi");
    }
}
