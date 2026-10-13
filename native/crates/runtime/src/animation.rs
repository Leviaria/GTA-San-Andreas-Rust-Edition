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
    /// A full-body one-shot such as getting into a car.
    Action,
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
            Motion::Action => &[],
        }
    }

    fn looped(self) -> bool {
        !matches!(
            self,
            Motion::Stop | Motion::Jump | Motion::Land | Motion::Dead | Motion::Action
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
    clip: String,
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
    /// An upper-body one-shot (firing, reloading, a punch, a flinch) laid
    /// over whatever the legs are doing.
    overlay: Option<Track>,
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
            overlay: None,
        }
    }
}

/// How quickly an overlay takes over and lets go of the arms.
const OVERLAY_FADE: f32 = 0.08;

fn first_clip<'a>(
    names: &[&'a str],
    duration: &impl Fn(&str) -> Option<f32>,
) -> Option<(&'a str, f32)> {
    names
        .iter()
        .find_map(|&name| duration(name).map(|d| (name, d)))
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
    pub fn clip(&self) -> &str {
        self.tracks.last().map_or("idle_stance", |t| &t.clip)
    }

    /// Plays the first clip that exists as a full-body one-shot that holds
    /// off the gait until it ends. Returns its length, or `None` when the
    /// ped has none of them.
    pub fn perform(
        &mut self,
        names: &[&str],
        duration: impl Fn(&str) -> Option<f32>,
    ) -> Option<f32> {
        let (clip, length) = first_clip(names, &duration)?;
        self.tracks.push(Track {
            motion: Motion::Action,
            clip: clip.to_owned(),
            seconds: 0.0,
            duration: length,
            weight: if self.tracks.is_empty() { 1.0 } else { 0.0 },
        });
        self.fade = 0.1;
        Some(length)
    }

    /// Plays the first clip that exists on the spine and arms only, from
    /// its start. Returns its length, or `None` when the ped has none.
    pub fn overlay(
        &mut self,
        names: &[&str],
        duration: impl Fn(&str) -> Option<f32>,
    ) -> Option<f32> {
        let (clip, length) = first_clip(names, &duration)?;
        let weight = self.overlay.as_ref().map_or(0.0, |o| o.weight);
        self.overlay = Some(Track {
            motion: Motion::Action,
            clip: clip.to_owned(),
            seconds: 0.0,
            duration: length,
            weight,
        });
        Some(length)
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
            self.overlay = None;
            self.play(Motion::Dead, 0.1, &duration);
        } else if current == Motion::Action && !finished {
            // Let a one-shot such as getting into a car play out.
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
            clip: clip.to_owned(),
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
        if let Some(overlay) = &mut self.overlay {
            overlay.seconds += dt;
            let left = overlay.duration - overlay.seconds;
            overlay.weight = (overlay.weight + dt / OVERLAY_FADE)
                .min(1.0)
                .min(left / OVERLAY_FADE);
            if left <= 0.0 {
                self.overlay = None;
            }
        }
    }

    /// The clips to mix this frame, for `Ped::blend_with_clothing`.
    pub fn plays(&self) -> Vec<Play<'_>> {
        let mut plays: Vec<_> = self
            .tracks
            .iter()
            .map(|t| Play {
                clip: &t.clip,
                seconds: t.seconds,
                weight: t.weight,
                looped: t.motion.looped(),
                upper_body: false,
            })
            .collect();
        if plays.is_empty() {
            plays.push(Play::looped("idle_stance", 0.0));
        }
        if let Some(o) = self.overlay.as_ref().filter(|o| o.weight > 0.0) {
            plays.push(Play {
                clip: &o.clip,
                seconds: o.seconds,
                weight: o.weight,
                looped: false,
                upper_body: true,
            });
        }
        plays
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
        let phase = a.plays()[0].seconds / 1.0;
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
        let plays = a.plays();
        let last = plays.last().unwrap();
        assert!(!last.looped && last.seconds <= 1.0);
        let mut none = Animator::civilian(0.0);
        none.update(dead, DT, minimal);
        assert_ne!(none.motion(), Motion::Dead, "no death clip to play");
    }

    #[test]
    fn overlays_ride_on_the_gait_and_end_by_themselves() {
        let mut a = Animator::default();
        for _ in 0..30 {
            a.update(ground(RUN), DT, all);
        }
        assert_eq!(a.overlay(&["colt45_fire"], all), Some(1.0));
        assert_eq!(a.overlay(&["missing"], minimal), None);
        a.update(ground(RUN), DT, all);
        let plays = a.plays();
        assert_eq!(plays.len(), 2);
        assert!(plays[1].upper_body && plays[1].clip == "colt45_fire");
        assert_eq!(a.clip(), "run_player", "legs keep running");
        for _ in 0..70 {
            a.update(ground(RUN), DT, all);
        }
        assert_eq!(a.plays().len(), 1, "overlay ended");
    }

    #[test]
    fn actions_hold_off_the_gait_until_they_end() {
        let mut a = Animator::default();
        a.update(ground(0.0), DT, all);
        assert_eq!(a.perform(&["car_getin_lhs"], all), Some(1.0));
        for _ in 0..30 {
            a.update(ground(0.0), DT, all);
        }
        assert_eq!(a.clip(), "car_getin_lhs");
        for _ in 0..60 {
            a.update(ground(0.0), DT, all);
        }
        assert_eq!(a.motion(), Motion::Idle);
        assert_eq!(a.perform(&["nope"], minimal), None);
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
