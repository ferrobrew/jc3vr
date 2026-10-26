//! Bounded decoding of the model collection file's instance dictionary.

use glam::{Mat4, Vec3};
use jc3gi::graphics_engine::model_instance::ModelCollection;

pub(super) struct Dictionary {
    pub(super) count: usize,
    pub(super) grenades: Vec<usize>,
}

/// Decode only the proven collection and keep its file-order grenade indices.
pub(super) fn dictionary(name: &[u8], data: &[u8]) -> Option<Dictionary> {
    let expected_count = supported(name)?;
    if data.len() > MAX_BYTES {
        return None;
    }
    let mut reader = Reader { data, position: 0 };
    if reader.u32()? != ModelCollection::FILE_MAGIC
        || reader.u32()? != ModelCollection::FILE_VERSION
    {
        return None;
    }
    let name_count = usize::try_from(reader.u32()?).ok()?;
    let count = usize::try_from(reader.u32()?).ok()?;
    let name_bytes = usize::try_from(reader.u32()?).ok()?;
    for _ in 0..3 {
        reader.u32()?;
    }
    if name_count == 0 || name_count > MAX_NAMES || count != expected_count || count > MAX_INSTANCES
    {
        return None;
    }
    let chunk = reader.bytes(name_bytes)?;
    if chunk.last() != Some(&0) {
        return None;
    }
    let mut names = chunk.split(|byte| *byte == 0);
    let mut grenade_names = Vec::new();
    for index in 0..name_count {
        let name = names.next()?;
        if name.is_empty() || !name.is_ascii() {
            return None;
        }
        if path_eq(name, GRENADE_MODEL) {
            grenade_names.push(index);
        }
    }
    if names.any(|name| !name.is_empty()) || grenade_names.is_empty() {
        return None;
    }
    reader.align(ModelCollection::FILE_ALIGNMENT as usize)?;
    for _ in 0..count {
        if !reader.matrix()?.is_finite() {
            return None;
        }
    }
    let mut grenades = Vec::new();
    for index in 0..count {
        let model = usize::from(reader.u16()?);
        if model >= name_count {
            return None;
        }
        if grenade_names.contains(&model) {
            if grenades.len() >= MAX_GRENADES {
                return None;
            }
            grenades.push(index);
        }
    }
    for _ in 0..count {
        reader.u16()?;
    }
    for _ in 0..count {
        if !reader.vector()?.is_finite() {
            return None;
        }
    }
    for _ in 0..count {
        if !reader.float()?.is_finite() {
            return None;
        }
    }
    for _ in 0..count {
        let min = reader.vector()?;
        let max = reader.vector()?;
        if !min.is_finite() || !max.is_finite() || !min.cmple(max).all() {
            return None;
        }
    }
    (reader.position == data.len() && !grenades.is_empty())
        .then_some(Dictionary { count, grenades })
}

pub(super) const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_NAMES: usize = 16 * 1024;
const MAX_INSTANCES: usize = 256 * 1024;
const MAX_GRENADES: usize = 16;
const COLLECTIONS: [(&[u8], usize); 5] = [
    (
        b"locations/region_02/military/outposts/outpost_01/outpost_guardia_massos_01.nl.mdic",
        96,
    ),
    (
        b"locations/region_02/military/outposts/outpost_02/outpost_guardia_massos_02.nl.mdic",
        25,
    ),
    (
        b"locations/region_02/military/outposts/outpost_03/outpost_guardia_massos_04.nl.mdic",
        24,
    ),
    (
        b"locations/region_02/military/outposts/outpost_04/outpost_guardia_massos_03.nl.mdic",
        43,
    ),
    (
        b"locations/region_02/military/outposts/outpost_06/outpost_guardia_massos_05.nl.mdic",
        78,
    ),
];
const GRENADE_MODEL: &[u8] =
    b"models/jc_weapons/03_thrown/w301_grenade_frag/w301_grenade_frag_body.lod";

fn supported(name: &[u8]) -> Option<usize> {
    COLLECTIONS.iter().find_map(|(path, count)| {
        let location = &path[..path.len() - b".mdic".len()];
        (path_eq(name, path) || path_eq(name, location)).then_some(*count)
    })
}

fn path_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            let left = if *left == b'\\' { b'/' } else { *left };
            left.eq_ignore_ascii_case(right)
        })
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count)?;
        let bytes = self.data.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))
    }

    fn float(&mut self) -> Option<f32> {
        Some(f32::from_bits(self.u32()?))
    }

    fn matrix(&mut self) -> Option<Mat4> {
        let mut values = [0.0; 16];
        for value in &mut values {
            *value = self.float()?;
        }
        Some(Mat4::from_cols_array(&values))
    }

    fn vector(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.float()?, self.float()?, self.float()?))
    }

    fn align(&mut self, alignment: usize) -> Option<()> {
        if !alignment.is_power_of_two() {
            return None;
        }
        let aligned = self.position.checked_add(alignment - 1)? & !(alignment - 1);
        self.bytes(aligned.checked_sub(self.position)?)?;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &[u8] = COLLECTIONS[3].0;
    const COUNT: usize = COLLECTIONS[3].1;

    fn collection() -> Vec<u8> {
        let mut data = Vec::new();
        for value in [
            ModelCollection::FILE_MAGIC,
            ModelCollection::FILE_VERSION,
            2,
            COUNT as u32,
            (GRENADE_MODEL.len() + 1 + b"models/other.lod\0".len()) as u32,
            0,
            0,
            0,
        ] {
            data.extend(value.to_le_bytes());
        }
        data.extend(GRENADE_MODEL);
        data.push(0);
        data.extend(b"models/other.lod\0");
        while data.len() % ModelCollection::FILE_ALIGNMENT as usize != 0 {
            data.push(0);
        }
        for _ in 0..COUNT {
            for value in Mat4::IDENTITY.to_cols_array() {
                data.extend(value.to_le_bytes());
            }
        }
        for index in 0..COUNT {
            data.extend(u16::from(index != 1 && index != 2).to_le_bytes());
        }
        for _ in 0..COUNT {
            data.extend(0_u16.to_le_bytes());
        }
        for _ in 0..COUNT * 3 + COUNT {
            data.extend(0_f32.to_le_bytes());
        }
        for _ in 0..COUNT {
            for value in [-1_f32, -1.0, -1.0, 1.0, 1.0, 1.0] {
                data.extend(value.to_le_bytes());
            }
        }
        data
    }

    #[test]
    fn source_selection_uses_collection_provenance_and_dictionary_not_fixed_indices() {
        let data = collection();
        let parsed = dictionary(COLLECTION, &data).unwrap();
        assert_eq!(parsed.count, COUNT);
        assert_eq!(parsed.grenades, [1, 2]);
        assert!(dictionary(b"locations/another.nl.mdic", &data).is_none());
        assert!(dictionary(GRENADE_MODEL, &data).is_none());
        assert!(dictionary(COLLECTIONS[0].0, &data).is_none());
        let location = &COLLECTION[..COLLECTION.len() - b".mdic".len()];
        assert!(dictionary(location, &data).is_some());
        assert!(dictionary(b"outpost_guardia_massos_03.nl", &data).is_none());
        let alternate = COLLECTION
            .iter()
            .map(|byte| {
                if *byte == b'/' {
                    b'\\'
                } else {
                    byte.to_ascii_uppercase()
                }
            })
            .collect::<Vec<_>>();
        assert!(dictionary(&alternate, &data).is_some());
    }

    #[test]
    fn every_truncated_span_and_trailing_data_is_rejected() {
        let mut data = collection();
        for length in 0..data.len() {
            assert!(
                dictionary(COLLECTION, &data[..length]).is_none(),
                "{length}"
            );
        }
        data.push(0);
        assert!(dictionary(COLLECTION, &data).is_none());
    }

    #[test]
    fn invalid_name_indices_names_and_bounds_are_rejected() {
        let data = collection();
        let matrix = (32
            + GRENADE_MODEL.len()
            + 1
            + b"models/other.lod\0".len()
            + ModelCollection::FILE_ALIGNMENT as usize
            - 1)
            & !(ModelCollection::FILE_ALIGNMENT as usize - 1);
        let mut malformed = data.clone();
        let indices = matrix + COUNT * 64;
        malformed[indices..indices + 2].copy_from_slice(&2_u16.to_le_bytes());
        assert!(dictionary(COLLECTION, &malformed).is_none());
        let mut malformed = data.clone();
        malformed[32] = 0xff;
        assert!(dictionary(COLLECTION, &malformed).is_none());
        let mut malformed = data.clone();
        malformed[32 + GRENADE_MODEL.len() + b"models/other.lod\0".len()] = 1;
        assert!(dictionary(COLLECTION, &malformed).is_none());
        let mut malformed = data;
        let bounds = indices + COUNT * (2 + 2 + 12 + 4);
        malformed[bounds..bounds + 4].copy_from_slice(&2_f32.to_le_bytes());
        assert!(dictionary(COLLECTION, &malformed).is_none());
    }

    #[test]
    fn malformed_counts_version_and_nonfinite_transforms_are_rejected() {
        let data = collection();
        for (offset, value) in [(4, 0), (8, u32::MAX), (12, u32::MAX), (16, u32::MAX)] {
            let mut malformed = data.clone();
            malformed[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(dictionary(COLLECTION, &malformed).is_none());
        }
        let mut malformed = data;
        let matrix = (32
            + GRENADE_MODEL.len()
            + 1
            + b"models/other.lod\0".len()
            + ModelCollection::FILE_ALIGNMENT as usize
            - 1)
            & !(ModelCollection::FILE_ALIGNMENT as usize - 1);
        malformed[matrix..matrix + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(dictionary(COLLECTION, &malformed).is_none());
    }
}
