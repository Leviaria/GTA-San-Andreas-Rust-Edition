//! Area names from the player's own `data/info.zon` and `text/american.gxt`,
//! shown in the HUD like San Andreas does when you enter a new area.
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
struct Zone {
    min: [f32; 3],
    max: [f32; 3],
    name: String,
}

#[derive(Default)]
pub struct Zones {
    zones: Vec<Zone>,
}

impl Zones {
    pub fn load(game: &Path) -> Self {
        let result = (|| -> anyhow::Result<Self> {
            let text = std::fs::read_to_string(game.join("data/info.zon"))?;
            let gxt = load_gxt(game);
            Ok(Self {
                zones: parse_zones(&text, &gxt),
            })
        })();
        result.unwrap_or_else(|error| {
            eprintln!("Area names unavailable: {error:#}");
            Self::default()
        })
    }
    /// Name of the smallest zone containing an SA world position (Z up).
    pub fn name_at(&self, position: [f32; 3]) -> Option<&str> {
        self.zones
            .iter()
            .filter(|zone| (0..3).all(|i| (zone.min[i]..=zone.max[i]).contains(&position[i])))
            .min_by(|a, b| area(a).total_cmp(&area(b)))
            .map(|zone| zone.name.as_str())
    }
}

/// The player's English text table, keyed by JAMCRC of the GXT key.
pub fn load_gxt(game: &Path) -> HashMap<u32, String> {
    std::fs::read(game.join("text/american.gxt"))
        .ok()
        .and_then(|bytes| parse_gxt(&bytes).ok())
        .unwrap_or_default()
}

pub fn gxt_text<'a>(table: &'a HashMap<u32, String>, key: &str) -> Option<&'a str> {
    table
        .get(&jamcrc(key.to_ascii_uppercase().as_bytes()))
        .map(String::as_str)
}

fn area(zone: &Zone) -> f32 {
    (zone.max[0] - zone.min[0]) * (zone.max[1] - zone.min[1])
}

fn parse_zones(text: &str, gxt: &HashMap<u32, String>) -> Vec<Zone> {
    let mut zones = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() < 10 {
            continue;
        }
        let numbers: Option<Vec<f32>> = fields[2..8].iter().map(|v| v.parse().ok()).collect();
        let Some(n) = numbers else { continue };
        let key = fields[9].to_ascii_uppercase();
        let name = gxt
            .get(&jamcrc(key.as_bytes()))
            .cloned()
            .unwrap_or_else(|| key.clone());
        zones.push(Zone {
            min: [n[0].min(n[3]), n[1].min(n[4]), n[2].min(n[5])],
            max: [n[0].max(n[3]), n[1].max(n[4]), n[2].max(n[5])],
            name,
        });
    }
    zones
}

/// CRC-32 without the final inversion, as San Andreas hashes GXT keys.
fn jamcrc(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Reads the MAIN table of an 8-bit San Andreas GXT file.
fn parse_gxt(bytes: &[u8]) -> anyhow::Result<HashMap<u32, String>> {
    use anyhow::{ensure, Context};
    let u32at = |at: usize| -> anyhow::Result<u32> {
        Ok(u32::from_le_bytes(
            bytes.get(at..at + 4).context("truncated GXT")?.try_into()?,
        ))
    };
    ensure!(
        bytes.get(..4) == Some(&[4, 0, 8, 0][..]),
        "not an 8-bit SA GXT"
    );
    ensure!(bytes.get(4..8) == Some(b"TABL"), "missing GXT table list");
    let tables = u32at(8)? as usize;
    ensure!(tables >= 12, "empty GXT table list");
    // The first table is MAIN; its TKEY block starts directly at the offset.
    let mut at = u32at(12 + 8)? as usize;
    ensure!(bytes.get(at..at + 4) == Some(b"TKEY"), "missing GXT keys");
    let keys = u32at(at + 4)? as usize;
    let first_key = at + 8;
    at = first_key + keys;
    ensure!(bytes.get(at..at + 4) == Some(b"TDAT"), "missing GXT text");
    let data = at + 8;
    let data_end = (data + u32at(at + 4)? as usize).min(bytes.len());
    let mut out = HashMap::new();
    for entry in (first_key..first_key + keys).step_by(8) {
        let offset = data + u32at(entry)? as usize;
        let hash = u32at(entry + 4)?;
        let Some(text) = bytes.get(offset..data_end) else {
            continue;
        };
        let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
        out.insert(hash, text[..end].iter().map(|b| char::from(*b)).collect());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gxt(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut keys = Vec::new();
        let mut text = Vec::new();
        for (key, value) in entries {
            keys.extend((text.len() as u32).to_le_bytes());
            keys.extend(jamcrc(key.as_bytes()).to_le_bytes());
            text.extend(value.as_bytes());
            text.push(0);
        }
        let mut out = vec![4, 0, 8, 0];
        out.extend(b"TABL");
        out.extend(12u32.to_le_bytes());
        out.extend(b"MAIN\0\0\0\0");
        out.extend(24u32.to_le_bytes());
        out.extend(b"TKEY");
        out.extend((keys.len() as u32).to_le_bytes());
        out.extend(keys);
        out.extend(b"TDAT");
        out.extend((text.len() as u32).to_le_bytes());
        out.extend(text);
        out
    }
    #[test]
    fn names_come_from_gxt_and_the_smallest_zone_wins() {
        let names = parse_gxt(&gxt(&[("GAN", "Ganton"), ("LA", "Los Santos")])).unwrap();
        let zones = Zones {
            zones: parse_zones(
                "zone\nLA, 0, 44.6, -2892.9, -242.9, 2997.0, -768.0, 900.0, 0, LA\n\
                 GAN1, 0, 2222.5, -1852.8, -89.0, 2632.8, -1722.3, 110.9, 3, GAN\n\
                 XYZ, 0, 0, 0, 0, 1, 1, 1, 0, NOKEY\nend\n",
                &names,
            ),
        };
        assert_eq!(zones.name_at([2495.0, -1750.0, 13.0]), Some("Ganton"));
        assert_eq!(zones.name_at([1000.0, -1500.0, 13.0]), Some("Los Santos"));
        assert_eq!(zones.name_at([0.5, 0.5, 0.5]), Some("NOKEY"));
        assert_eq!(zones.name_at([-2000.0, 500.0, 10.0]), None);
    }
    #[test]
    fn rejects_other_gxt_formats() {
        assert!(parse_gxt(b"TABL").is_err());
    }
}
