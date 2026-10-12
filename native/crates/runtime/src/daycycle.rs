//! Day/night cycle driven by the player's own `data/timecyc.dat`.
//!
//! San Andreas keys its colours at eight times per weather. Only the first
//! weather block (extra sunny Los Santos) is used for now. One game minute
//! passes per real second, as in the original game.
use glam::Vec3;
use std::path::Path;

/// Hours of the eight timecyc rows.
const HOURS: [f32; 8] = [0.0, 5.0, 6.0, 7.0, 12.0, 19.0, 20.0, 22.0];
const SUNRISE: f32 = 6.0;
const SUNSET: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub ambient: Vec3,
    pub ambient_objects: Vec3,
    pub sky_top: Vec3,
    pub sky_bottom: Vec3,
    pub sun_core: Vec3,
}

impl Palette {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            ambient: self.ambient.lerp(other.ambient, t),
            ambient_objects: self.ambient_objects.lerp(other.ambient_objects, t),
            sky_top: self.sky_top.lerp(other.sky_top, t),
            sky_bottom: self.sky_bottom.lerp(other.sky_bottom, t),
            sun_core: self.sun_core.lerp(other.sun_core, t),
        }
    }
}

/// Values sampled for one frame, ready for the scene uniform.
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    /// Direction towards the sun (or the moon at night), world space, Y up.
    pub sun: Vec3,
    /// 0 at night, 1 in full daylight.
    pub daylight: f32,
    pub sun_color: Vec3,
    /// Multiplier for the original prelit colours, which were baked for day.
    pub ambient: Vec3,
    pub sky: Vec3,
}

pub struct DayCycle {
    slots: [Palette; 8],
    /// Minutes since midnight.
    pub minutes: f32,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self {
            slots: fallback(),
            minutes: 12.0 * 60.0,
        }
    }
}

impl DayCycle {
    pub fn load(game: &Path) -> Self {
        let slots = std::fs::read_to_string(game.join("data/timecyc.dat"))
            .map_err(anyhow::Error::from)
            .and_then(|text| parse(&text))
            .unwrap_or_else(|error| {
                eprintln!("timecyc.dat unavailable, using built-in colours: {error:#}");
                fallback()
            });
        Self {
            slots,
            ..Default::default()
        }
    }
    pub fn advance(&mut self, seconds: f32) {
        self.minutes = (self.minutes + seconds).rem_euclid(24.0 * 60.0);
    }
    pub fn clock(&self) -> (u32, u32) {
        let minutes = self.minutes as u32;
        (minutes / 60 % 24, minutes % 60)
    }
    pub fn palette(&self, hour: f32) -> Palette {
        let hour = hour.rem_euclid(24.0);
        let next = HOURS.iter().position(|h| *h > hour).unwrap_or(0);
        let previous = (next + HOURS.len() - 1) % HOURS.len();
        let (start, mut end) = (HOURS[previous], HOURS[next]);
        if end <= start {
            end += 24.0;
        }
        let hour = if hour < start { hour + 24.0 } else { hour };
        self.slots[previous].lerp(self.slots[next], (hour - start) / (end - start))
    }
    pub fn lighting(&self, hour: f32) -> Lighting {
        let palette = self.palette(hour);
        let noon = self.palette(12.0);
        let hour = hour.rem_euclid(24.0);
        let day = (SUNRISE..SUNSET).contains(&hour);
        // Fraction of the sun's (or moon's) arc, rising in the east (+X),
        // passing to the south (+Z) and setting in the west.
        let arc = if day {
            (hour - SUNRISE) / (SUNSET - SUNRISE)
        } else {
            (hour - SUNSET).rem_euclid(24.0) / (24.0 - SUNSET + SUNRISE)
        };
        let angle = arc * std::f32::consts::PI;
        let sun = Vec3::new(angle.cos(), angle.sin() * 0.9 + 0.08, 0.45).normalize();
        // Soft fade around sunrise and sunset so shadows do not pop.
        let fade = |edge: f32| (1.0 - (hour - edge).abs() / 0.75).clamp(0.0, 1.0);
        let daylight = if day {
            1.0 - fade(SUNRISE).max(fade(SUNSET)) * 0.5
        } else {
            fade(SUNRISE).max(fade(SUNSET)) * 0.5
        };
        let ambient = (palette.ambient_objects + 8.0) / (noon.ambient_objects + 8.0);
        Lighting {
            sun,
            daylight,
            sun_color: if day {
                (palette.sun_core / 255.0).max(Vec3::splat(0.3))
            } else {
                Vec3::new(0.45, 0.55, 0.8)
            },
            ambient: ambient.clamp(Vec3::splat(0.22), Vec3::splat(1.25)),
            sky: (palette.sky_top * 0.35 + palette.sky_bottom * 0.65) / 255.0,
        }
    }
}

/// Reads the first weather block. Supports rows with and without the
/// directional colour column found in earlier games' files.
fn parse(text: &str) -> anyhow::Result<[Palette; 8]> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let values: Vec<f32> = line
            .split_whitespace()
            .map_while(|v| v.parse().ok())
            .collect();
        anyhow::ensure!(values.len() >= 40, "unexpected timecyc row: {line}");
        let rgb = |group: usize| {
            Vec3::new(
                values[group * 3],
                values[group * 3 + 1],
                values[group * 3 + 2],
            )
        };
        // San Andreas rows hold 52 values; a directional colour adds three.
        let skip = usize::from(values.len() >= 55);
        rows.push(Palette {
            ambient: rgb(0),
            ambient_objects: rgb(1),
            sky_top: rgb(2 + skip),
            sky_bottom: rgb(3 + skip),
            sun_core: rgb(4 + skip),
        });
        if rows.len() == 8 {
            break;
        }
    }
    let rows: [Palette; 8] = rows
        .try_into()
        .map_err(|rows: Vec<_>| anyhow::anyhow!("only {} timecyc rows", rows.len()))?;
    for row in &rows {
        for colour in [
            row.ambient,
            row.ambient_objects,
            row.sky_top,
            row.sky_bottom,
            row.sun_core,
        ] {
            anyhow::ensure!(
                colour.min_element() >= 0.0 && colour.max_element() <= 255.0,
                "timecyc colour out of range"
            );
        }
    }
    Ok(rows)
}

/// Approximate clear-weather colours used without a readable timecyc.dat.
fn fallback() -> [Palette; 8] {
    let p = |ambient: [f32; 3], sky_top: [f32; 3], sky_bottom: [f32; 3], sun: [f32; 3]| Palette {
        ambient: Vec3::from(ambient),
        ambient_objects: Vec3::from(ambient),
        sky_top: Vec3::from(sky_top),
        sky_bottom: Vec3::from(sky_bottom),
        sun_core: Vec3::from(sun),
    };
    [
        p(
            [25.0, 28.0, 40.0],
            [8.0, 12.0, 30.0],
            [20.0, 28.0, 50.0],
            [120.0, 140.0, 200.0],
        ),
        p(
            [30.0, 30.0, 40.0],
            [20.0, 25.0, 45.0],
            [45.0, 45.0, 60.0],
            [150.0, 150.0, 180.0],
        ),
        p(
            [60.0, 50.0, 45.0],
            [70.0, 90.0, 130.0],
            [200.0, 140.0, 90.0],
            [255.0, 170.0, 100.0],
        ),
        p(
            [80.0, 75.0, 70.0],
            [90.0, 130.0, 180.0],
            [190.0, 170.0, 150.0],
            [255.0, 210.0, 160.0],
        ),
        p(
            [100.0, 100.0, 100.0],
            [92.0, 140.0, 184.0],
            [150.0, 180.0, 205.0],
            [255.0, 245.0, 225.0],
        ),
        p(
            [95.0, 85.0, 75.0],
            [90.0, 120.0, 170.0],
            [220.0, 160.0, 110.0],
            [255.0, 190.0, 120.0],
        ),
        p(
            [70.0, 55.0, 55.0],
            [60.0, 60.0, 110.0],
            [200.0, 110.0, 80.0],
            [255.0, 130.0, 80.0],
        ),
        p(
            [35.0, 35.0, 50.0],
            [15.0, 20.0, 45.0],
            [40.0, 40.0, 70.0],
            [130.0, 140.0, 200.0],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noon_is_bright_and_midnight_is_dark() {
        let cycle = DayCycle::default();
        let noon = cycle.lighting(12.0);
        let night = cycle.lighting(0.0);
        assert_eq!(noon.daylight, 1.0);
        assert_eq!(night.daylight, 0.0);
        assert!((noon.ambient - Vec3::ONE).abs().max_element() < 1e-4);
        assert!(night.ambient.max_element() < 0.5);
        assert!(noon.sun.y > 0.8, "noon sun should be high: {:?}", noon.sun);
        assert!(
            cycle.lighting(7.0).sun.x > 0.5,
            "morning sun rises in the east"
        );
        assert!(
            cycle.lighting(19.0).sun.x < -0.5,
            "evening sun sets in the west"
        );
    }
    #[test]
    fn palette_wraps_past_midnight() {
        let cycle = DayCycle::default();
        let late = cycle.palette(23.0);
        let slots = fallback();
        assert_eq!(late, slots[7].lerp(slots[0], 0.5));
        assert_eq!(cycle.palette(12.0), slots[4]);
    }
    #[test]
    fn parses_san_andreas_rows() {
        let mut text = String::from("// EXTRASUNNY_LA\n");
        for hour in 0..8 {
            let mut row = vec![hour.to_string(); 6];
            row.extend((0..46).map(|_| "100".to_string()));
            text.push_str(&row.join(" "));
            text.push('\n');
        }
        let rows = parse(&text).unwrap();
        assert_eq!(rows[3].ambient, Vec3::splat(3.0));
        assert_eq!(rows[3].sky_top, Vec3::splat(100.0));
        assert!(parse("1 2 3").is_err());
    }
    #[test]
    fn clock_advances_and_wraps() {
        let mut cycle = DayCycle {
            minutes: 23.0 * 60.0 + 59.0,
            ..Default::default()
        };
        cycle.advance(2.0);
        assert_eq!(cycle.clock(), (0, 1));
    }
}
