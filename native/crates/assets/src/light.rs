//! The PC SA geometry 2DFX light record: street lamps, neon and window glows.
use super::{chunks, f32at, slice, u32at};

const AT_DAY: u8 = 0x20;
const AT_NIGHT: u8 = 0x40;
const WITHOUT_CORONA: u8 = 0x08;

#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    /// Model-space position, Z up.
    pub position: [f32; 3],
    pub color: [u8; 4],
    pub range: f32,
    pub corona_size: f32,
    /// Radius of the light pool projected onto the ground below.
    pub shadow_size: f32,
    /// How far below the light the ground pool is drawn, in metres.
    pub shadow_distance: f32,
    pub corona: bool,
    /// Lit in daylight too (neon, interiors), not only at night.
    pub always: bool,
}

/// Light records of a geometry. Malformed or day-only records are skipped
/// rather than failing the model, since lights are decoration.
pub(super) fn from_geometry(data: &[u8]) -> Vec<Light> {
    let Ok(extensions) = chunks(data) else {
        return Vec::new();
    };
    let mut lights = Vec::new();
    for extension in extensions.into_iter().filter(|c| c.tag == 3) {
        let Ok(plugins) = chunks(extension.body) else {
            continue;
        };
        for plugin in plugins.into_iter().filter(|c| c.tag == 0x0253_f2f8) {
            lights.extend(decode(plugin.body));
        }
    }
    lights
}

fn decode(data: &[u8]) -> Vec<Light> {
    let mut lights = Vec::new();
    let Ok(count) = u32at(data, 0) else {
        return lights;
    };
    let mut offset = 4;
    for _ in 0..count.min(4096) {
        let Ok(header) = slice(data, offset, 20) else {
            break;
        };
        let Ok(size) = u32at(header, 16) else { break };
        let size = size as usize;
        offset += 20;
        let Ok(payload) = slice(data, offset, size) else {
            break;
        };
        offset += size;
        if u32at(header, 12).ok() != Some(0) || size < 76 {
            continue;
        }
        if let Some(light) = record(header, payload) {
            lights.push(light);
        }
    }
    lights
}

fn record(header: &[u8], payload: &[u8]) -> Option<Light> {
    let position = [
        f32at(header, 0).ok()?,
        f32at(header, 4).ok()?,
        f32at(header, 8).ok()?,
    ];
    let range = f32at(payload, 8).ok()?;
    let corona_size = f32at(payload, 12).ok()?;
    let shadow_size = f32at(payload, 16).ok()?;
    let flags = payload[24];
    let valid = position.iter().all(|v| v.is_finite() && v.abs() < 1000.0)
        && (0.0..=100.0).contains(&range)
        && (0.0..=50.0).contains(&corona_size)
        && (0.0..=50.0).contains(&shadow_size);
    if !valid || (flags & AT_DAY != 0 && flags & AT_NIGHT == 0) {
        return None;
    }
    Some(Light {
        position,
        color: payload[0..4].try_into().ok()?,
        range,
        corona_size,
        shadow_size,
        shadow_distance: f32::from(payload[73]),
        corona: flags & WITHOUT_CORONA == 0 && corona_size > 0.0,
        always: flags & AT_DAY != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn effect(kind: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [1.0f32, 2.0, 3.0] {
            out.extend(v.to_le_bytes());
        }
        out.extend(kind.to_le_bytes());
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend(payload);
        out
    }
    fn lamp(flags: u8) -> Vec<u8> {
        let mut p = vec![0u8; 80];
        p[0..4].copy_from_slice(&[255, 200, 120, 255]);
        p[8..12].copy_from_slice(&12.0f32.to_le_bytes());
        p[12..16].copy_from_slice(&1.5f32.to_le_bytes());
        p[16..20].copy_from_slice(&4.0f32.to_le_bytes());
        p[24] = flags;
        p[73] = 6;
        p
    }
    #[test]
    fn decodes_night_lamps_and_skips_other_effects() {
        let mut data = 4u32.to_le_bytes().to_vec();
        data.extend(effect(0, &lamp(AT_NIGHT)));
        data.extend(effect(7, &[0; 88]));
        data.extend(effect(0, &lamp(AT_DAY)));
        data.extend(effect(0, &lamp(AT_DAY | AT_NIGHT | WITHOUT_CORONA)));
        let lights = decode(&data);
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(lights[0].color, [255, 200, 120, 255]);
        assert_eq!(lights[0].shadow_distance, 6.0);
        assert!(lights[0].corona && !lights[0].always);
        assert!(!lights[1].corona && lights[1].always);
        assert!(decode(&[1, 0, 0, 0, 9]).is_empty());
    }
}
