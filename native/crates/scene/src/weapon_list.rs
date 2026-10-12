//! Guns from the player's own `data/weapon.dat`.
//!
//! Gun rows start with `$` and read: name, fire type, target range, weapon
//! range, model, second model, slot, animation group, clip size, damage,
//! then aiming and animation details. The game lists most guns three times
//! (poor, standard and pro skill); the standard row is kept.
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Weapon {
    /// The game's own name, e.g. `PISTOL` or `AK47`.
    pub name: String,
    pub range: f32,
    /// Model id from default.ide, for drawing the gun in hand later.
    pub model: i32,
    pub slot: u8,
    /// Animation group in ped.ifp terms, e.g. `colt45` or `rifle`.
    pub group: String,
    pub clip: u32,
    pub damage: f32,
}

impl Weapon {
    /// Seconds between shots, from how the game's animation groups feel:
    /// automatics spray, pistols tap, shotguns and rifles pump or bolt.
    pub fn interval(&self) -> f32 {
        match self.group.as_str() {
            "uzi" | "tec" | "mp5" | "m4" | "ak47" => 0.1,
            "buddy" | "shotgun" | "sawnoff" | "spas" => 0.9,
            "rifle" | "sniper" => 1.2,
            "python" | "deagle" => 0.7,
            _ if self.clip >= 30 => 0.1,
            _ => 0.35,
        }
    }
    /// Pellets per shot; shotguns fire a spread.
    pub fn pellets(&self) -> u32 {
        if matches!(
            self.group.as_str(),
            "buddy" | "shotgun" | "sawnoff" | "spas"
        ) {
            8
        } else {
            1
        }
    }
    /// A readable name, e.g. `Desert Eagle` from `DESERT_EAGLE`.
    pub fn label(&self) -> String {
        self.name
            .split('_')
            .map(|word| {
                let lower = word.to_ascii_lowercase();
                let mut chars = lower.chars();
                chars
                    .next()
                    .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn original_guns(game: &Path) -> Vec<Weapon> {
    sa_assets::game_path::resolve(game, "data/weapon.dat")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse_weapons(&text))
        .unwrap_or_default()
}

fn parse_weapons(text: &str) -> Vec<Weapon> {
    // name -> rows seen, in file order.
    let mut rows: Vec<(String, Vec<Weapon>)> = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some(rest) = line.strip_prefix('$') else {
            continue;
        };
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 10 || fields[1] != "INSTANT_HIT" {
            continue;
        }
        let number = |index: usize| fields[index].parse::<f32>().ok();
        let (Some(range), Some(model), Some(slot), Some(clip), Some(damage)) =
            (number(3), number(4), number(6), number(8), number(9))
        else {
            continue;
        };
        if damage <= 0.0 || range <= 0.0 || clip < 1.0 {
            continue;
        }
        let weapon = Weapon {
            name: fields[0].to_ascii_uppercase(),
            range,
            model: model as i32,
            slot: slot as u8,
            group: fields[7].to_ascii_lowercase(),
            clip: clip as u32,
            damage,
        };
        match rows.iter_mut().find(|(name, _)| *name == weapon.name) {
            Some((_, list)) => list.push(weapon),
            None => rows.push((weapon.name.clone(), vec![weapon])),
        }
    }
    rows.into_iter()
        .filter_map(|(_, mut list)| {
            // Poor, standard, pro: take the middle one when all three exist.
            let index = if list.len() >= 3 { 1 } else { 0 };
            (index < list.len()).then(|| list.swap_remove(index))
        })
        .collect()
}

/// Weapon model id to (model, texture dictionary), from the `weap` section
/// of `data/default.ide`.
pub fn weapon_models(game: &Path) -> std::collections::HashMap<i32, (String, String)> {
    sa_assets::game_path::resolve(game, "data/default.ide")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse_weapon_models(&text))
        .unwrap_or_default()
}

fn parse_weapon_models(ide: &str) -> std::collections::HashMap<i32, (String, String)> {
    let mut models = std::collections::HashMap::new();
    let mut section = false;
    for line in ide.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        match line {
            "weap" => section = true,
            "end" => section = false,
            _ if section => {
                let fields: Vec<_> = line.split(',').map(str::trim).collect();
                if let (Some(Ok(id)), Some(model), Some(txd)) = (
                    fields.first().map(|f| f.parse::<i32>()),
                    fields.get(1),
                    fields.get(2),
                ) {
                    models.insert(id, (model.to_ascii_lowercase(), txd.to_ascii_lowercase()));
                }
            }
            _ => {}
        }
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_standard_skill_guns_only() {
        let text = "# weapon.dat\n\
            $ PISTOL INSTANT_HIT 35.0 35.0 346 -1 2 colt45 17 25 0.11 0.04 0.06 0 69 1.0 1.0 2 3 4 2 3 4 0 0 0 0 0 0\n\
            $ PISTOL INSTANT_HIT 35.0 35.0 346 -1 2 colt45 17 25 0.11 0.04 0.06 1 69 1.0 1.0 2 3 4 2 3 4 0 0 0 0 0 0\n\
            $ PISTOL INSTANT_HIT 35.0 35.0 346 -1 2 colt45 34 25 0.11 0.04 0.06 2 69 1.0 1.0 2 3 4 2 3 4 0 0 0 0 0 0\n\
            $ ROCKET PROJECTILE 55.0 55.0 359 -1 7 rocket 1 75 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n\
            $ SNIPERRIFLE INSTANT_HIT 100.0 100.0 358 -1 6 rifle 1 125 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n\
            $ DESERT_EAGLE INSTANT_HIT 35.0 35.0 348 -1 2 python 7 70 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n\
            ¶ not a weapon\n";
        let guns = parse_weapons(text);
        let names: Vec<_> = guns.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["PISTOL", "SNIPERRIFLE", "DESERT_EAGLE"]);
        assert_eq!(guns[0].clip, 17, "standard row, not the pro one");
        assert_eq!(guns[0].damage, 25.0);
        assert_eq!(guns[0].group, "colt45");
        assert_eq!(guns[2].label(), "Desert Eagle");
        assert!(guns[1].interval() > guns[0].interval());
    }

    #[test]
    fn reads_weapon_models_from_default_ide() {
        let ide = "objs\n1, x, x, 1, 0\nend\nweap\n346, colt45, colt45, colt45, 1, 50, 0\n356, M4, m4, rifle, 1, 50, 0\nend\n";
        let models = parse_weapon_models(ide);
        assert_eq!(models[&346], ("colt45".to_owned(), "colt45".to_owned()));
        assert_eq!(models[&356].0, "m4");
        assert!(!models.contains_key(&1));
    }

    #[test]
    fn missing_file_gives_no_guns() {
        assert!(original_guns(Path::new("/definitely/not/a/game")).is_empty());
    }
}
