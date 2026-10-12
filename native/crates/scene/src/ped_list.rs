//! Every ped model listed in the player's own `data/peds.ide`. Only names are
//! read up front; models load when a ped is first chosen or seen.
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalPed {
    /// File name in gta3.img, e.g. `bmyst`.
    pub model: String,
    /// The `peds.ide` ped type, e.g. `CIVMALE` or `GANG2`.
    pub kind: String,
}

pub fn original_peds(game: &Path) -> Vec<OriginalPed> {
    sa_assets::game_path::resolve(game, "data/peds.ide")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse_peds(&text))
        .unwrap_or_default()
}

fn parse_peds(ide: &str) -> Vec<OriginalPed> {
    let mut peds: Vec<OriginalPed> = Vec::new();
    let mut in_peds = false;
    for line in ide.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        match line {
            "peds" => {
                in_peds = true;
                continue;
            }
            "end" => {
                in_peds = false;
                continue;
            }
            _ => {}
        }
        if !in_peds {
            continue;
        }
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() < 4 {
            continue;
        }
        let model = fields[1].to_ascii_lowercase();
        // Placeholders the story swaps at run time have no model of their own.
        if model.is_empty()
            || model == "null"
            || model.starts_with("special")
            || !model.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || peds.iter().any(|p| p.model == model)
        {
            continue;
        }
        peds.push(OriginalPed {
            model,
            kind: fields[3].to_ascii_uppercase(),
        });
    }
    peds
}

/// A readable group name for a `peds.ide` ped type.
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "CIVMALE" => "Man",
        "CIVFEMALE" => "Woman",
        "COP" => "Police",
        "GANG1" => "Ballas",
        "GANG2" => "Grove Street",
        "GANG3" => "Vagos",
        "GANG4" => "Rifa",
        "GANG5" => "Da Nang Boys",
        "GANG6" => "Mafia",
        "GANG7" => "Triads",
        "GANG8" => "Aztecas",
        "DEALER" => "Dealer",
        "MEDIC" => "Medic",
        "FIREMAN" => "Firefighter",
        "CRIMINAL" => "Criminal",
        "PROSTITUTE" => "Working girl",
        "MISSION1" | "MISSION2" | "MISSION3" | "MISSION4" | "MISSION5" | "MISSION6"
        | "MISSION7" | "MISSION8" => "Special",
        _ => "Ped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_models_and_types_and_skips_placeholders() {
        let ide = "peds\n\
            0, null, null, CIVMALE, STAT_STREET_GUY, man, 0, 0, null, 9,9, PED_TYPE_GEN, VOICE_GEN_RANDOM, VOICE_GEN_RANDOM\n\
            7, male01, male01, CIVMALE, STAT_STREET_GUY, man, 1fff, 0, null, 9,9, PED_TYPE_GEN, VOICE_GEN_BMYST, VOICE_GEN_BMYST\n\
            105, fam1, fam1, GANG2, STAT_GANG2, gang1, 1fff, 0, null, 9,9, PED_TYPE_GANG, VOICE_GNG_FAM1, VOICE_GNG_FAM5\n\
            290, special01, special01, CIVMALE, STAT_STREET_GUY, man, 0, 0, null, 9,9, PED_TYPE_GEN, VOICE_GEN_RANDOM, VOICE_GEN_RANDOM\n\
            7, MALE01, male01, CIVMALE, STAT_STREET_GUY, man, 1fff, 0, null, 9,9, PED_TYPE_GEN, VOICE_GEN_BMYST, VOICE_GEN_BMYST\n\
            end\n";
        let peds = parse_peds(ide);
        assert_eq!(
            peds,
            [
                OriginalPed {
                    model: "male01".into(),
                    kind: "CIVMALE".into()
                },
                OriginalPed {
                    model: "fam1".into(),
                    kind: "GANG2".into()
                },
            ]
        );
        assert_eq!(kind_label(&peds[1].kind), "Grove Street");
    }

    #[test]
    fn missing_file_gives_an_empty_list() {
        assert!(original_peds(Path::new("/definitely/not/a/game")).is_empty());
    }
}
