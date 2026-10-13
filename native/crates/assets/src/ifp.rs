//! Bounded ANP3 animation clips, shortest-arc sampling and HAnim binding.
//! SA compressed translation uses 1024 units/metre, rotation 4096, time 60.
use super::*;
#[derive(Clone)]
pub struct Key {
    pub seconds: f32,
    pub rotation: [f32; 4],
    pub translation: Option<[f32; 3]>,
}
pub struct Track {
    pub name: String,
    pub bone_id: i32,
    pub keys: Vec<Key>,
}
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub tracks: Vec<Track>,
}
pub struct Package {
    pub name: String,
    pub clips: Vec<Clip>,
}
struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}
impl Reader<'_> {
    fn bytes(&mut self, size: usize) -> Result<&[u8]> {
        let result = slice(self.data, self.offset, size)?;
        self.offset += size;
        Ok(result)
    }
    fn name(&mut self) -> Result<String> {
        name(self.bytes(24)?)
    }
    fn uint(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
    fn signed(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
    fn short(&mut self, scale: f32) -> Result<f32> {
        Ok(f32::from(i16::from_le_bytes(self.bytes(2)?.try_into()?)) / scale)
    }
    fn float(&mut self) -> Result<f32> {
        let value = f32::from_le_bytes(self.bytes(4)?.try_into()?);
        ensure!(value.is_finite(), "nonfinite IFP value");
        Ok(value)
    }
}
pub fn decode(data: &[u8]) -> Result<Package> {
    ensure!(
        slice(data, 0, 4)? == b"ANP3" && data.len() <= 16 * 1024 * 1024,
        "unsupported IFP container"
    );
    let length = u32at(data, 4)? as usize;
    let mut reader = Reader {
        data: slice(data, 8, length)?,
        offset: 0,
    };
    let name = reader.name()?;
    let count = reader.uint()? as usize;
    ensure!((1..=4096).contains(&count), "IFP clip budget");
    let mut clips = Vec::with_capacity(count);
    let mut total_keys = 0usize;
    for _ in 0..count {
        let name = reader.name()?;
        let bones = reader.uint()? as usize;
        let frame_bytes = reader.uint()? as usize;
        let _flags = reader.uint()?;
        ensure!((1..=256).contains(&bones), "IFP track budget");
        let mut tracks = Vec::with_capacity(bones);
        let mut duration = 0.0f32;
        let mut read_frame_bytes = 0usize;
        for _ in 0..bones {
            let name = reader.name()?;
            let kind = reader.uint()?;
            let count = reader.uint()? as usize;
            let bone_id = reader.signed()?;
            ensure!(
                (1..=65535).contains(&count) && bone_id >= -1,
                "invalid IFP track header"
            );
            total_keys += count;
            ensure!(total_keys <= 2_000_000, "IFP key budget");
            let (compressed, translation, size) = match kind {
                1 => (false, false, 20),
                2 => (false, true, 32),
                3 => (true, false, 10),
                4 => (true, true, 16),
                _ => bail!("unsupported IFP frame type {kind}"),
            };
            read_frame_bytes += count * size;
            let mut keys = Vec::with_capacity(count);
            let mut last = -1.0;
            for _ in 0..count {
                let mut rotation = [0.0; 4];
                for value in &mut rotation {
                    *value = if compressed {
                        reader.short(4096.0)?
                    } else {
                        reader.float()?
                    };
                }
                let seconds = if compressed {
                    reader.short(60.0)?
                } else {
                    reader.float()?
                };
                ensure!(
                    seconds >= 0.0 && seconds >= last && seconds <= 3600.0,
                    "invalid or unordered IFP time"
                );
                last = seconds;
                let norm = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
                ensure!((0.5..=1.5).contains(&norm), "invalid IFP quaternion");
                for value in &mut rotation {
                    *value /= norm;
                }
                let translation = if translation {
                    let mut position = [0.0; 3];
                    for value in &mut position {
                        *value = if compressed {
                            reader.short(1024.0)?
                        } else {
                            reader.float()?
                        };
                    }
                    Some(position)
                } else {
                    None
                };
                keys.push(Key {
                    seconds,
                    rotation,
                    translation,
                });
                duration = duration.max(seconds);
            }
            tracks.push(Track {
                name,
                bone_id,
                keys,
            });
        }
        ensure!(
            frame_bytes == read_frame_bytes,
            "IFP frame byte count mismatch"
        );
        clips.push(Clip {
            name,
            duration,
            tracks,
        });
    }
    ensure!(reader.offset == reader.data.len(), "unsupported IFP tail");
    Ok(Package { name, clips })
}
impl Track {
    pub fn sample(&self, seconds: f32) -> Key {
        let after = self.keys.partition_point(|k| k.seconds <= seconds);
        let a = &self.keys[after.saturating_sub(1)];
        let b = &self.keys[after.min(self.keys.len() - 1)];
        let factor = if b.seconds > a.seconds {
            ((seconds - a.seconds) / (b.seconds - a.seconds)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut dot = a
            .rotation
            .iter()
            .zip(b.rotation)
            .map(|(x, y)| x * y)
            .sum::<f32>();
        let sign = if dot < 0.0 { -1.0 } else { 1.0 };
        dot = dot.abs().clamp(0.0, 1.0);
        let (wa, wb) = if dot > 0.9995 {
            (1.0 - factor, factor)
        } else {
            let angle = dot.acos();
            (
                ((1.0 - factor) * angle).sin() / angle.sin(),
                (factor * angle).sin() / angle.sin(),
            )
        };
        let mut rotation = std::array::from_fn(|i| a.rotation[i] * wa + b.rotation[i] * sign * wb);
        let norm = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
        for value in &mut rotation {
            *value /= norm;
        }
        Key {
            seconds,
            rotation,
            translation: a
                .translation
                .zip(b.translation)
                .map(|(a, b)| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * factor)),
        }
    }
}
/// One clip's contribution to a blended pose.
#[derive(Clone, Copy)]
pub struct Layer<'a> {
    pub clip: &'a Clip,
    pub seconds: f32,
    pub weight: f32,
    /// Looping clips wrap their time; one-shots hold their last frame.
    pub looped: bool,
}
fn quaternion_of(m: &skin::Matrix) -> [f32; 4] {
    // Columns are m[0..3], m[3..6], m[6..9]; R(row, col) = m[col * 3 + row].
    let (r00, r11, r22) = (m[0], m[4], m[8]);
    let q = if r00 + r11 + r22 > 0.0 {
        let s = (1.0 + r00 + r11 + r22).sqrt() * 2.0;
        [
            (m[5] - m[7]) / s,
            (m[6] - m[2]) / s,
            (m[1] - m[3]) / s,
            0.25 * s,
        ]
    } else if r00 > r11 && r00 > r22 {
        let s = (1.0 + r00 - r11 - r22).sqrt() * 2.0;
        [
            0.25 * s,
            (m[3] + m[1]) / s,
            (m[6] + m[2]) / s,
            (m[5] - m[7]) / s,
        ]
    } else if r11 > r22 {
        let s = (1.0 + r11 - r00 - r22).sqrt() * 2.0;
        [
            (m[3] + m[1]) / s,
            0.25 * s,
            (m[7] + m[5]) / s,
            (m[6] - m[2]) / s,
        ]
    } else {
        let s = (1.0 + r22 - r00 - r11).sqrt() * 2.0;
        [
            (m[6] + m[2]) / s,
            (m[7] + m[5]) / s,
            0.25 * s,
            (m[1] - m[3]) / s,
        ]
    };
    let norm = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / norm)
}
fn write_rotation(matrix: &mut skin::Matrix, [x, y, z, w]: [f32; 4]) {
    matrix[..9].copy_from_slice(&[
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ]);
}
/// Mixes several clips into one set of local bone matrices. Rotations are
/// blended as weighted, sign-aligned quaternions and translations linearly;
/// a bone a clip does not animate keeps its bind pose for that clip's share.
/// Root movement is optionally removed in XY while preserving vertical bob.
pub fn blend_pose(
    model: &skin::Model,
    layers: &[Layer],
    in_place: bool,
) -> Result<Vec<skin::Matrix>> {
    let mut pose = model.bind_pose();
    let total: f32 = layers.iter().map(|l| l.weight.max(0.0)).sum();
    ensure!(
        layers
            .iter()
            .all(|l| l.seconds.is_finite() && l.weight.is_finite()),
        "nonfinite clip time or weight"
    );
    if total <= 0.0 {
        return Ok(pose);
    }
    // Per frame: summed quaternion, summed translation, weight that had a key.
    let mut rotations = vec![[0.0f32; 4]; pose.len()];
    let mut translations = vec![[0.0f32; 3]; pose.len()];
    let mut rotated = vec![0.0f32; pose.len()];
    let mut moved = vec![0.0f32; pose.len()];
    let mut bind = vec![None; pose.len()];
    for layer in layers.iter().filter(|l| l.weight > 0.0) {
        let weight = layer.weight / total;
        let clip = layer.clip;
        let seconds = if clip.duration <= 0.0 {
            0.0
        } else if layer.looped {
            layer.seconds.rem_euclid(clip.duration)
        } else {
            layer.seconds.clamp(0.0, clip.duration)
        };
        for track in &clip.tracks {
            ensure!(!track.keys.is_empty(), "empty sampled track");
            let Some(frame) = model
                .frames
                .iter()
                .position(|f| f.bone_id == Some(track.bone_id))
            else {
                continue;
            };
            let key = track.sample(seconds);
            let reference = *bind[frame].get_or_insert_with(|| quaternion_of(&pose[frame]));
            let dot: f32 = key.rotation.iter().zip(reference).map(|(a, b)| a * b).sum();
            let sign = if dot < 0.0 { -1.0 } else { 1.0 };
            for (sum, value) in rotations[frame].iter_mut().zip(key.rotation) {
                *sum += value * sign * weight;
            }
            rotated[frame] += weight;
            if let Some(mut position) = key.translation {
                if in_place && track.bone_id == 0 {
                    position[0] = 0.0;
                    position[1] = 0.0;
                }
                for (sum, value) in translations[frame].iter_mut().zip(position) {
                    *sum += value * weight;
                }
                moved[frame] += weight;
            }
        }
    }
    for (frame, matrix) in pose.iter_mut().enumerate() {
        if rotated[frame] > 0.0 {
            let mut rotation = rotations[frame];
            let rest = 1.0 - rotated[frame];
            if rest > 1e-6 {
                let reference = bind[frame].unwrap_or_else(|| quaternion_of(matrix));
                for (sum, value) in rotation.iter_mut().zip(reference) {
                    *sum += value * rest;
                }
            }
            let norm = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
            ensure!(norm > 1e-6, "degenerate blended rotation");
            write_rotation(matrix, rotation.map(|v| v / norm));
        }
        if moved[frame] > 0.0 {
            let rest = 1.0 - moved[frame];
            for (value, sum) in matrix[9..].iter_mut().zip(translations[frame]) {
                *value = sum + *value * rest.max(0.0);
            }
        }
    }
    Ok(pose)
}
impl Package {
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }
}
impl Clip {
    /// Root movement is optionally removed in XY while preserving vertical bob.
    pub fn pose(
        &self,
        model: &skin::Model,
        seconds: f32,
        in_place: bool,
    ) -> Result<Vec<skin::Matrix>> {
        ensure!(seconds.is_finite(), "nonfinite clip time");
        blend_pose(
            model,
            &[Layer {
                clip: self,
                seconds,
                weight: 1.0,
                looped: true,
            }],
            in_place,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(compressed: bool) -> Vec<u8> {
        let mut body = Vec::new();
        let mut bytes = [0u8; 24];
        bytes[..4].copy_from_slice(b"test");
        body.extend(bytes);
        body.extend(1u32.to_le_bytes());
        let mut name = [0u8; 24];
        name[..4].copy_from_slice(b"walk");
        body.extend(name);
        for value in [
            1u32,
            if compressed { 32 } else { 64 },
            u32::from(compressed),
        ] {
            body.extend(value.to_le_bytes());
        }
        let mut name = [0u8; 24];
        name[..4].copy_from_slice(b"root");
        body.extend(name);
        for value in [if compressed { 4u32 } else { 2 }, 2, 0] {
            body.extend(value.to_le_bytes());
        }
        for end in [false, true] {
            if compressed {
                for value in [
                    0i16,
                    0,
                    0,
                    4096,
                    if end { 60 } else { 0 },
                    if end { 1024 } else { 0 },
                    0,
                    256,
                ] {
                    body.extend(value.to_le_bytes());
                }
            } else {
                for value in [
                    0.0f32,
                    0.0,
                    0.0,
                    1.0,
                    if end { 1.0 } else { 0.0 },
                    if end { 1.0 } else { 0.0 },
                    0.0,
                    0.25,
                ] {
                    body.extend(value.to_le_bytes());
                }
            }
        }
        let mut bytes = b"ANP3".to_vec();
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
        bytes
    }
    #[test]
    fn compressed_and_float_frames_match_and_truncation_is_rejected() {
        for compressed in [true, false] {
            let bytes = fixture(compressed);
            let package = decode(&bytes).unwrap();
            assert_eq!(package.clips[0].duration, 1.0);
            let key = package.clips[0].tracks[0].sample(0.5);
            assert_eq!(key.translation, Some([0.5, 0.0, 0.25]));
            assert_eq!(key.rotation, [0.0, 0.0, 0.0, 1.0]);
            for length in 0..bytes.len() {
                assert!(decode(&bytes[..length]).is_err());
            }
        }
        let mut bytes = fixture(false);
        bytes[108..112].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&bytes).is_err());
        let mut bytes = fixture(true);
        bytes[96..100].copy_from_slice(&99u32.to_le_bytes());
        assert!(decode(&bytes).is_err());
    }
    #[test]
    fn quaternion_sampling_takes_short_arc_and_root_can_stay_in_place() {
        let package = decode(&fixture(true)).unwrap();
        let mut track = Track {
            name: "root".into(),
            bone_id: 0,
            keys: vec![
                Key {
                    seconds: 0.0,
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation: None,
                },
                Key {
                    seconds: 1.0,
                    rotation: [0.0, 0.0, 0.0, -1.0],
                    translation: None,
                },
            ],
        };
        assert_eq!(track.sample(0.5).rotation, [0.0, 0.0, 0.0, 1.0]);
        track.keys[1].rotation = [0.0, 0.0, 1.0, 0.0];
        let rotation = track.sample(0.5).rotation;
        assert!((rotation[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
        let model = skin::Model {
            frames: vec![skin::Frame {
                local: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 3.0, 4.0, 5.0],
                parent: None,
                bone_id: Some(0),
            }],
            bone_frames: vec![0],
            parts: vec![],
        };
        let clip = &package.clips[0];
        assert_eq!(
            &clip.pose(&model, 0.5, false).unwrap()[0][9..],
            &[0.5, 0.0, 0.25]
        );
        assert_eq!(
            &clip.pose(&model, 1.5, true).unwrap()[0][9..],
            &[0.0, 0.0, 0.25]
        );
        assert!(clip.pose(&model, f32::NAN, true).is_err());
    }
    fn one_bone(bind_rotation: [f32; 4]) -> skin::Model {
        let mut local = [0.0; 12];
        write_rotation(&mut local, bind_rotation);
        local[9..].copy_from_slice(&[1.0, 2.0, 3.0]);
        skin::Model {
            frames: vec![skin::Frame {
                local,
                parent: None,
                bone_id: Some(5),
            }],
            bone_frames: vec![0],
            parts: vec![],
        }
    }
    fn still(rotation: [f32; 4], translation: Option<[f32; 3]>) -> Clip {
        Clip {
            name: "still".into(),
            duration: 1.0,
            tracks: vec![Track {
                name: "bone".into(),
                bone_id: 5,
                keys: vec![Key {
                    seconds: 0.0,
                    rotation,
                    translation,
                }],
            }],
        }
    }
    #[test]
    fn matrices_round_trip_through_quaternions() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        for q in [
            [0.0, 0.0, 0.0, 1.0],
            [h, 0.0, 0.0, h],
            [0.0, 1.0, 0.0, 0.0],
            [0.5, -0.5, 0.5, 0.5],
        ] {
            let mut m = [0.0; 12];
            write_rotation(&mut m, q);
            let back = quaternion_of(&m);
            let dot: f32 = back.iter().zip(q).map(|(a, b)| a * b).sum();
            assert!(dot.abs() > 0.9999, "{q:?} became {back:?}");
        }
    }
    #[test]
    fn half_and_half_blend_lands_between_clips_and_keeps_bind_for_missing_keys() {
        let model = one_bone([0.0, 0.0, 0.0, 1.0]);
        let s = (std::f32::consts::FRAC_PI_4).sin();
        let a = still([0.0, 0.0, 0.0, 1.0], Some([0.0, 0.0, 0.0]));
        let b = still([0.0, 0.0, s, s], None);
        let layer = |clip, weight| Layer {
            clip,
            seconds: 0.0,
            weight,
            looped: true,
        };
        let pose = blend_pose(&model, &[layer(&a, 1.0), layer(&b, 1.0)], false).unwrap();
        let q = quaternion_of(&pose[0]);
        // Halfway between 0 and 90 degrees about Z.
        let angle = 2.0 * q[2].atan2(q[3]);
        assert!(
            (angle - std::f32::consts::FRAC_PI_4).abs() < 1e-4,
            "{angle}"
        );
        // b has no translation, so its half keeps the bind position.
        assert_eq!(&pose[0][9..], &[0.5, 1.0, 1.5]);
        // A single full-weight layer matches the clip exactly.
        let only_b = blend_pose(&model, &[layer(&b, 0.0), layer(&b, 2.0)], false).unwrap();
        assert!((only_b[0][1] - 1.0).abs() < 1e-5);
        // No weight means the bind pose.
        assert_eq!(
            blend_pose(&model, &[layer(&a, 0.0)], false).unwrap(),
            model.bind_pose()
        );
    }
    #[test]
    fn one_shots_hold_their_last_frame() {
        let package = decode(&fixture(true)).unwrap();
        let model = one_bone([0.0, 0.0, 0.0, 1.0]);
        let mut model = model;
        model.frames[0].bone_id = Some(0);
        let layer = Layer {
            clip: &package.clips[0],
            seconds: 5.0,
            weight: 1.0,
            looped: false,
        };
        let pose = blend_pose(&model, &[layer], false).unwrap();
        assert_eq!(&pose[0][9..], &[1.0, 0.0, 0.25]);
        assert!(package.clip("WALK").is_some());
    }
}
