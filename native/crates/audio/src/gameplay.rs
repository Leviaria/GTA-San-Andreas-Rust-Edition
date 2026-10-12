//! Original classic-PC samples; no audio files are bundled with the runtime.
use crate::{archive::SfxArchive, SoundEffect};
use anyhow::{ensure, Result};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Sedan,
    Sports,
}

pub struct EngineSounds {
    pub(crate) idle: SoundEffect,
    pub(crate) rev: SoundEffect,
}

pub struct GameplaySounds {
    sedan: EngineSounds,
    sports: EngineSounds,
    footsteps: Vec<SoundEffect>,
    /// The generic weapon bank, when the install has it.
    weapons: Vec<SoundEffect>,
}

/// The weapon bank in GENRL. Modding tools list it as `bank_137`, counting
/// from one, the way they list the footsteps (bank 0 here) as `bank_001`.
const WEAPON_BANK: usize = 136;

/// Slot of the firing sound for a weapon.dat animation group, as modders
/// have catalogued the bank (their `sound_001` is slot 0 here).
fn gunshot_slot(group: &str) -> usize {
    match group {
        "uzi" | "tec" => 0,
        "ak47" | "m4" | "rifle" | "sniper" => 3,
        "mp5" => 17,
        "shotgun" | "buddy" | "sawnoff" | "spas" => 21,
        "silenced" => 24,
        "python" | "deagle" => 26,
        _ => 6,
    }
}
/// Slot of a reload click.
const RELOAD_SLOT: usize = 31;

impl GameplaySounds {
    pub fn load(game: &Path) -> Result<Self> {
        let archive = SfxArchive::open(game)?;
        // Classic GENRL: MERC_D=86 (Taxi/Admiral), COBRA_D=37 (Infernus).
        // Within both engine banks, 0 is rev and 1 is idle.
        let engines = |bank| -> Result<EngineSounds> {
            let samples = archive.read_bank(bank)?;
            ensure!(samples.len() >= 2, "engine bank {bank} is incomplete");
            Ok(EngineSounds {
                rev: loop_sample(&samples[0])?,
                idle: loop_sample(&samples[1])?,
            })
        };
        let feet = archive.read_bank(0)?;
        ensure!(feet.len() >= 6, "generic footsteps bank is incomplete");
        // Gun sounds are optional: a modded or trimmed install still plays.
        let weapons = archive
            .read_bank(WEAPON_BANK)
            .ok()
            .filter(|bank| bank.len() > RELOAD_SLOT)
            .map(|bank| bank.iter().map(SoundEffect::from_pcm).collect())
            .unwrap_or_default();
        Ok(Self {
            weapons,
            sedan: engines(86)?,
            sports: engines(37)?,
            // Generic/concrete footsteps are slots 1..=5; slot 0 is not a step.
            footsteps: feet[1..=5]
                .iter()
                .map(|pcm| {
                    let mut sound = SoundEffect::from_pcm(pcm);
                    sound.data = sound.data.volume(-12.0);
                    sound
                })
                .collect(),
        })
    }

    pub fn engine(&self, kind: EngineKind) -> &EngineSounds {
        match kind {
            EngineKind::Sedan => &self.sedan,
            EngineKind::Sports => &self.sports,
        }
    }

    /// The firing sound for a gun's animation group, if the bank loaded.
    pub fn gunshot(&self, group: &str) -> Option<&SoundEffect> {
        self.weapons.get(gunshot_slot(group))
    }

    pub fn reload(&self) -> Option<&SoundEffect> {
        self.weapons.get(RELOAD_SLOT)
    }

    pub fn footstep(&self, index: usize) -> &SoundEffect {
        &self.footsteps[index % self.footsteps.len()]
    }
}

fn loop_sample(pcm: &crate::archive::PcmSound) -> Result<SoundEffect> {
    ensure!(
        !pcm.samples.is_empty() && pcm.sample_rate > 0,
        "empty engine sample"
    );
    let start = pcm.loop_start.unwrap_or(0);
    ensure!(
        start < pcm.samples.len(),
        "engine loop begins past sample end"
    );
    let mut sound = SoundEffect::from_pcm(pcm);
    sound.data = sound
        .data
        .loop_region(start as f64 / f64::from(pcm.sample_rate)..);
    Ok(sound)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_loop_keeps_intro_and_rejects_out_of_bounds_start() {
        let mut pcm = crate::archive::PcmSound {
            sample_rate: 8000,
            samples: vec![1; 100],
            loop_start: Some(20),
            headroom_hundredths_db: 0,
        };
        let sound = loop_sample(&pcm).unwrap();
        assert_eq!(sound.data.frames.len(), 100);
        assert!(sound.data.settings.loop_region.is_some());
        pcm.loop_start = Some(100);
        assert!(loop_sample(&pcm).is_err());
        pcm.samples.clear();
        assert!(loop_sample(&pcm).is_err());
    }
}

#[cfg(test)]
mod gun_tests {
    use super::*;

    #[test]
    fn each_gun_group_has_its_own_shot() {
        assert_eq!(gunshot_slot("colt45"), 6);
        assert_eq!(gunshot_slot("python"), 26);
        assert_eq!(gunshot_slot("ak47"), gunshot_slot("m4"));
        assert_ne!(gunshot_slot("shotgun"), gunshot_slot("uzi"));
        assert!(RELOAD_SLOT > gunshot_slot("python"));
    }
}
