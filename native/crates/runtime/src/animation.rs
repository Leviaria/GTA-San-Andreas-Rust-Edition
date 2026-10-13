//! Which ped.ifp clips a walking ped plays, and how they fade into each other.
//! Clips are read from the player's own ped.ifp; this only picks them by
//! name and mixes them, so a change of gait or a landing crossfades instead
//! of snapping, and walk, run and sprint keep the feet in step.

use crate::locomotion::{RUN, SPRINT, WALK};
use sa_scene::ped::Play;

/// Most of a crossfade between gaits; long enough to hide the cut, short
/// enough that control still feels direct.
const FADE: f32 = 0.15;
/// Time in the air before a fall pose starts (steps and kerbs stay grounded).
const FALL_AFTER: f32 = 0.25;
/// Time in the air before landing plays a recovery clip.
const HARD_LANDING: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Idle,
    Walk,
    Run,
    Sprint,
    Stop,
    Aim,
    Jump,
    Glide,
    Fall,
    Land,
    Dead,
}

impl Motion {
    /// Clip names to try, best first. Not every ped.ifp or custom player
    /// package has every clip, so each motion has fallbacks.
    fn clips(self, civilian: bool) -> &'static [&'static str] {
        match self {
            Motion::Idle => &["idle_stance"],
            Motion::Walk if civilian => &["walk_civi", "walk_player"],
            Motion::Walk => &["walk_player", "walk_civi"],
            Motion::Run if civilian => &["run_civi", "run_player"],
            Motion::Run => &["run_player", "run_civi"],
            Motion::Sprint => &["sprint_civi", "sprint_panic", "run_player"],
            Motion::Stop => &["run_stop", "idle_stance"],
            Motion::Aim => &["gun_stand", "idle_stance"],
            Motion::Jump => &["jump_launch", "jump_glide", "fall_fall", "idle_stance"],
            Motion::Glide => &["jump_glide", "fall_fall", "idle_stance"],
            Motion::Fall => &["fall_fall", "jump_glide", "idle_stance"],
            Motion::Land => &["jump_land", "fall_land", "idle_stance"],
            Motion::Dead => &["ko_shot_front", "ko_skid_back"],
        }
    }

    fn looped(self) -> bool {
        !matches!(
            self,
            Motion::Stop | Motion::Jump | Motion::Land | Motion::Dead
        )
    }

    fn gait(self) -> bool {
        matches!(self, Motion::Walk | Motion::Run | Motion::Sprint)
    }

    /// Ground speed the clip was made for, so playback can follow the
    /// actual speed and the feet slide less.
    fn nominal_speed(self) -> Option<f32> {
        match self {
            Motion::Walk => Some(WALK),
            Motion::Run => Some(RUN),
            Motion::Sprint => Some(SPRINT),
            _ => None,
        }
    }
}

/// What the ped is doing this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub speed: f32,
    pub grounded: bool,
    pub vertical_speed: f32,
    pub aiming: bool,
    pub dead: bool,
}

#[derive(Clone, Debug)]
struct Track {
    motion: Motion,
    clip: &'static str,
    seconds: f32,
    duration: f32,
    weight: f32,
}

#[derive(Clone, Debug)]
pub struct Animator {
    /// The newest track is last and fades in; older ones fade out.
    tracks: Vec<Track>,
    fade: f32,
    airborne: f32,
    jumped: bool,
    /// Pedestrians prefer the civilian walk and run over the player's.
    civilian: bool,
    /// Clock offset for the first clip, so a crowd does not step in unison.
    start: f32,
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            fade: FADE,
            airborne: 0.0,
            jumped: false,
            civilian: false,
            start: 0.0,
        }
    }
}

pub fn gait_for_speed(speed: f32) -> Motion {
    if speed < 0.15 {
        Motion::Idle
    } else if speed < (WALK + RUN) * 0.5 {
        Motion::Walk
    } else if speed < (RUN + SPRINT) * 0.5 {
        Motion::Run
    } else {
        Motion::Sprint
    }
}

impl Animator {
    pub fn civilian(start: f32) -> Self {
        Self {
            civilian: true,
            start,
            ..Self::default()
        }
    }

    pub fn motion(&self) -> Motion {
        self.tracks.last().map_or(Motion::Idle, |t| t.motion)
    }

    /// The clip that dominates the pose now.
    pub fn clip(&self) -> &'static str {
        self.tracks.last().map_or("idle_stance", |t| t.clip)
    }

    pub fn reset(&mut self) {
        *self = Self {
            civilian: self.civilian,
            ..Self::default()
        };
    }

    /// Advances clocks and picks the next motion. `duration` gives a clip's
    /// length, or `None` when the ped's package does not have it.
    pub fn update(&mut self, input: Input, dt: f32, duration: impl Fn(&str) -> Option<f32>) {
        let dt = dt.clamp(0.0, 0.25);
        let current = self.motion();
        let finished = self
            .tracks
            .last()
            .is_some_and(|t| !t.motion.looped() && t.seconds >= t.duration);
        if input.dead {
            self.play(Motion::Dead, 0.1, &duration);
        } else if input.grounded {
            let was_airborne = self.airborne;
            self.airborne = 0.0;
            self.jumped = false;
            if matches!(current, Motion::Glide | Motion::Fall | Motion::Jump)
                && was_airborne >= HARD_LANDING
                && input.speed < 0.15
            {
                self.play(Motion::Land, 0.08, &duration);
            } else if current == Motion::Land && !finished && input.speed < 0.15 {
                // Let the landing finish unless the player moves off.
            } else {
                let gait = gait_for_speed(input.speed);
                let next = if input.aiming && gait == Motion::Idle {
                    Motion::Aim
                } else if gait == Motion::Idle
                    && ((matches!(current, Motion::Run | Motion::Sprint)
                        && duration("run_stop").is_some())
                        || (current == Motion::Stop && !finished))
                {
                    Motion::Stop
                } else {
                    gait
                };
                let fade = if current == Motion::Land { 0.2 } else { FADE };
                self.play(next, fade, &duration);
            }
        } else {
            self.airborne += dt;
            if input.vertical_speed > 0.5 && self.airborne <= dt + 1e-4 {
                self.jumped = true;
                self.play(Motion::Jump, 0.08, &duration);
            } else if current == Motion::Jump && (finished || input.vertical_speed < -2.0) {
                self.play(Motion::Glide, 0.15, &duration);
            } else if !self.jumped && self.airborne > FALL_AFTER {
                self.play(Motion::Fall, 0.2, &duration);
            } else if !self.jumped && current != Motion::Fall {
                // A short drop off a kerb keeps the gait going.
                self.play(gait_for_speed(input.speed), FADE, &duration);
            }
        }
        self.advance(input.speed, dt);
    }

    /// Starts `motion` unless it is already the newest track.
    fn play(&mut self, motion: Motion, fade: f32, duration: &impl Fn(&str) -> Option<f32>) {
        if self.tracks.last().is_some_and(|t| t.motion == motion) {
            return;
        }
        let Some((clip, length)) = motion
            .clips(self.civilian)
            .iter()
            .find_map(|&name| duration(name).map(|d| (name, d)))
        else {
            return;
        };
        // Changing gait keeps the walk cycle's phase, so the new clip puts
        // down the same foot the old one was about to.
        let seconds = match self.tracks.last() {
            Some(old) if old.motion.gait() && motion.gait() && old.duration > 0.0 => {
                (old.seconds / old.duration).fract() * length
            }
            None if motion.looped() => self.start.rem_euclid(length.max(1e-3)),
            _ => 0.0,
        };
        let first = self.tracks.is_empty();
        self.tracks.push(Track {
            motion,
            clip,
            seconds,
            duration: length,
            weight: if first { 1.0 } else { 0.0 },
        });
        self.fade = fade.max(0.01);
    }

    fn advance(&mut self, speed: f32, dt: f32) {
        let fade = self.fade.max(0.01);
        let newest = self.tracks.len().saturating_sub(1);
        for (index, track) in self.tracks.iter_mut().enumerate() {
            let rate = track
                .motion
                .nominal_speed()
                .map_or(1.0, |nominal| (speed / nominal).clamp(0.6, 1.4));
            track.seconds += dt * rate;
            if !track.motion.looped() {
                track.seconds = track.seconds.min(track.duration);
            }
            if index == newest {
                track.weight = (track.weight + dt / fade).min(1.0);
            }
        }
        // Older tracks give up whatever the newest has taken, oldest first,
        // so the weights always add up to one.
        let mut left = 1.0 - self.tracks.last().map_or(0.0, |t| t.weight);
        for track in self.tracks.iter_mut().rev().skip(1) {
            track.weight = track.weight.min(left);
            left -= track.weight;
        }
        let newest = self.tracks.len().saturating_sub(1);
        let mut index = 0;
        self.tracks.retain(|t| {
            index += 1;
            index - 1 == newest || t.weight > 0.0
        });
        if self.tracks.len() > 4 {
            let drop = self.tracks.len() - 4;
            self.tracks.drain(..drop);
        }
    }

    /// The clips to mix this frame, for `Ped::blend_with_clothing`.
    pub fn plays(&self) -> Vec<Play<'static>> {
        if self.tracks.is_empty() {
            return vec![Play::looped("idle_stance", 0.0)];
        }
        self.tracks
            .iter()
            .map(|t| Play {
                clip: t.clip,
                seconds: t.seconds,
                weight: t.weight,
                looped: t.motion.looped(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DT: f32 = 1.0 / 60.0;

    fn all(name: &str) -> Option<f32> {
        Some(match name {
            "walk_player" => 1.0,
            "run_player" => 0.8,
            "sprint_civi" => 0.6,
            "jump_launch" => 0.3,
            "jump_land" => 0.4,
            "run_stop" => 0.5,
            _ => 1.0,
        })
    }

    fn minimal(name: &str) -> Option<f32> {
        ["idle_stance", "walk_player", "run_player"]
            .contains(&name)
            .then_some(1.0)
    }

    fn ground(speed: f32) -> Input {
        Input {
            speed,
            grounded: true,
            ..Input::default()
        }
    }

    fn total(a: &Animator) -> f32 {
        a.plays().iter().map(|p| p.weight).sum()
    }

    #[test]
    fn gait_changes_crossfade_and_weights_add_up() {
        let mut a = Animator::default();
        a.update(ground(0.0), DT, all);
        assert_eq!(a.clip(), "idle_stance");
        a.update(ground(RUN), DT, all);
        assert_eq!(a.clip(), "run_player");
        let plays = a.plays();
        assert_eq!(plays.len(), 2, "idle fades out while run fades in");
        assert!(plays[1].weight > 0.0 && plays[1].weight < 1.0);
        for _ in 0..30 {
            a.update(ground(RUN), DT, all);
            assert!((total(&a) - 1.0).abs() < 1e-5);
        }
        assert_eq!(a.plays().len(), 1);
    }

    #[test]
    fn walk_to_run_keeps_the_step_phase() {
        let mut a = Animator::default();
        a.update(ground(WALK), DT, all);
        for _ in 0..15 {
            a.update(ground(WALK), DT, all);
        }
        let walk = a.plays()[0];
        let phase = walk.seconds / 1.0;
        a.update(ground(RUN), 0.0, all);
        let run = *a.plays().last().unwrap();
        assert_eq!(run.clip, "run_player");
        assert!((run.seconds / 0.8 - phase).abs() < 1e-4);
    }

    #[test]
    fn jumping_launches_glides_and_lands() {
        let mut a = Animator::default();
        a.update(ground(0.0), DT, all);
        a.update(
            Input {
                vertical_speed: 5.0,
                ..Input::default()
            },
            DT,
            all,
        );
        assert_eq!(a.motion(), Motion::Jump);
        for _ in 0..60 {
            a.update(
                Input {
                    vertical_speed: -3.0,
                    ..Input::default()
                },
                DT,
                all,
            );
        }
        assert_eq!(a.motion(), Motion::Glide);
        a.update(ground(0.0), DT, all);
        assert_eq!(a.motion(), Motion::Land);
        for _ in 0..60 {
            a.update(ground(0.0), DT, all);
        }
        assert_eq!(a.motion(), Motion::Idle);
    }

    #[test]
    fn moving_off_a_landing_goes_straight_to_the_gait() {
        let mut a = Animator::default();
        a.update(ground(0.0), DT, all);
        for _ in 0..60 {
            a.update(
                Input {
                    vertical_speed: -3.0,
                    ..Input::default()
                },
                DT,
                all,
            );
        }
        assert_eq!(a.motion(), Motion::Fall);
        a.update(ground(RUN), DT, all);
        assert_eq!(a.motion(), Motion::Run);
    }

    #[test]
    fn stopping_from_a_run_plays_the_stop_then_idles() {
        let mut a = Animator::default();
        for _ in 0..30 {
            a.update(ground(RUN), DT, all);
        }
        a.update(ground(0.0), DT, all);
        assert_eq!(a.motion(), Motion::Stop);
        for _ in 0..60 {
            a.update(ground(0.0), DT, all);
        }
        assert_eq!(a.motion(), Motion::Idle);
    }

    #[test]
    fn missing_clips_fall_back() {
        let mut a = Animator::default();
        a.update(ground(SPRINT), DT, minimal);
        assert_eq!(a.clip(), "run_player");
        for _ in 0..30 {
            a.update(ground(SPRINT), DT, minimal);
        }
        a.update(ground(0.0), DT, minimal);
        assert_eq!(a.motion(), Motion::Idle, "no run_stop clip");
        a.update(
            Input {
                vertical_speed: 5.0,
                ..Input::default()
            },
            DT,
            minimal,
        );
        assert_eq!(a.clip(), "idle_stance");
    }

    #[test]
    fn pedestrians_walk_like_civilians_and_die_once() {
        let mut a = Animator::civilian(0.0);
        a.update(ground(WALK), DT, all);
        assert_eq!(a.clip(), "walk_civi");
        let dead = Input {
            dead: true,
            ..ground(0.0)
        };
        for _ in 0..120 {
            a.update(dead, DT, all);
        }
        assert_eq!(a.motion(), Motion::Dead);
        let last = *a.plays().last().unwrap();
        assert!(!last.looped && last.seconds <= 1.0);
        let mut none = Animator::civilian(0.0);
        none.update(dead, DT, minimal);
        assert_ne!(none.motion(), Motion::Dead, "no death clip to play");
    }

    #[test]
    fn aiming_while_still_holds_the_gun_pose() {
        let mut a = Animator::default();
        a.update(
            Input {
                aiming: true,
                ..ground(0.0)
            },
            DT,
            all,
        );
        assert_eq!(a.clip(), "gun_stand");
        a.update(
            Input {
                aiming: true,
                ..ground(RUN)
            },
            DT,
            all,
        );
        assert_eq!(a.clip(), "run_player");
    }
}
