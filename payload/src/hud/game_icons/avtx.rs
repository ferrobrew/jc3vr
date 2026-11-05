//! The game's texture files (`.ddsc`, an `AVTX` header and its streams), decoded to RGBA texels.
//!
//! The header names the DXGI format, the size, and the mip count, and lists up to eight streams of
//! the texture's data, each with an offset, a size, and a source: zero for data inside this file,
//! otherwise a companion high-resolution file. The UI's icons keep their single mip in the file
//! itself, block-compressed, so only BC1 and BC3 at the top mip are decoded here.

use std::fmt;

/// A decoded texture: straight-alpha RGBA texels, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// The straight RGBA of the texel at (`x`, `y`).
    pub(crate) fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }
}

/// Why a texture could not be decoded.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum AvtxError {
    /// Shorter than its header.
    Truncated,
    /// Not an `AVTX` header.
    NotAvtx,
    /// A DXGI format other than the block formats the UI's icons use.
    UnsupportedFormat(u32),
    /// A zero or oversized width or height, or a volume.
    BadSize { width: u16, height: u16, depth: u16 },
    /// The top mip's data is not in this file.
    External,
    /// The top mip's stream is shorter than the size needs, or past the file's end.
    ShortData { needed: usize, found: usize },
}

impl fmt::Display for AvtxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AvtxError::Truncated => f.write_str("texture: shorter than its header"),
            AvtxError::NotAvtx => f.write_str("texture: not an AVTX texture"),
            AvtxError::UnsupportedFormat(format) => {
                write!(f, "texture: DXGI format {format} is not decoded")
            }
            AvtxError::BadSize {
                width,
                height,
                depth,
            } => write!(f, "texture: unusable size {width}x{height}x{depth}"),
            AvtxError::External => {
                f.write_str("texture: the top mip is in a companion file, not this one")
            }
            AvtxError::ShortData { needed, found } => write!(
                f,
                "texture: the top mip needs {needed} bytes, the file holds {found}"
            ),
        }
    }
}

impl std::error::Error for AvtxError {}

/// Decode the top mip of the `.ddsc` in `bytes`.
pub(crate) fn decode(bytes: &[u8]) -> Result<Image, AvtxError> {
    let header = bytes.get(..HEADER).ok_or(AvtxError::Truncated)?;
    if header[..4] != *b"AVTX" {
        return Err(AvtxError::NotAvtx);
    }
    let format = u32_at(header, 8);
    let block = match format {
        DXGI_BC1_UNORM | DXGI_BC1_UNORM_SRGB => Block::Bc1,
        DXGI_BC3_UNORM | DXGI_BC3_UNORM_SRGB => Block::Bc3,
        other => return Err(AvtxError::UnsupportedFormat(other)),
    };
    let (width, height, depth) = (u16_at(header, 12), u16_at(header, 14), u16_at(header, 16));
    if width == 0 || height == 0 || depth > 1 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(AvtxError::BadSize {
            width,
            height,
            depth,
        });
    }
    // The first stream holds the top mip, largest first.
    let (offset, size, source) = (
        u32_at(header, STREAMS) as usize,
        u32_at(header, STREAMS + 4) as usize,
        header[STREAMS + 11],
    );
    if source != 0 {
        return Err(AvtxError::External);
    }
    let (width, height) = (u32::from(width), u32::from(height));
    let blocks = (width.div_ceil(4) * height.div_ceil(4)) as usize;
    let needed = blocks * block.bytes();
    let data = bytes
        .get(offset..)
        .map(|rest| &rest[..rest.len().min(size)])
        .unwrap_or_default();
    if data.len() < needed {
        return Err(AvtxError::ShortData {
            needed,
            found: data.len(),
        });
    }
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let across = width.div_ceil(4);
    for (index, bytes) in data[..needed].chunks_exact(block.bytes()).enumerate() {
        let texels = match block {
            Block::Bc1 => bc1_colours(bytes, true),
            Block::Bc3 => {
                let mut colours = bc1_colours(&bytes[8..], false);
                let alphas = bc3_alphas(&bytes[..8]);
                for (colour, alpha) in colours.iter_mut().zip(alphas) {
                    colour[3] = alpha;
                }
                colours
            }
        };
        let (bx, by) = (index as u32 % across * 4, index as u32 / across * 4);
        for (i, texel) in texels.iter().enumerate() {
            let (x, y) = (bx + i as u32 % 4, by + i as u32 / 4);
            if x < width && y < height {
                let at = ((y * width + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(texel);
            }
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// The header's size, up to the first stream's data.
const HEADER: usize = 0x80;

/// Where the stream table starts in the header.
const STREAMS: usize = 0x20;

/// The largest side decoded; the UI's icons are far smaller.
const MAX_SIDE: u16 = 4096;

/// The DXGI block formats decoded (Direct3D's own `DXGI_FORMAT` values).
const DXGI_BC1_UNORM: u32 = 71;
const DXGI_BC1_UNORM_SRGB: u32 = 72;
const DXGI_BC3_UNORM: u32 = 77;
const DXGI_BC3_UNORM_SRGB: u32 = 78;

#[derive(Copy, Clone)]
enum Block {
    Bc1,
    Bc3,
}

impl Block {
    const fn bytes(self) -> usize {
        match self {
            Block::Bc1 => 8,
            Block::Bc3 => 16,
        }
    }
}

/// The 16 texels of a BC1 colour block, row by row. `punch_through` allows BC1's transparent
/// mode; a BC3 colour block always uses four colours.
fn bc1_colours(block: &[u8], punch_through: bool) -> [[u8; 4]; 16] {
    let (c0, c1) = (u16_at(block, 0), u16_at(block, 2));
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |wa: u16, wb: u16, d: u16| {
        let channel = |i: usize| ((u16::from(a[i]) * wa + u16::from(b[i]) * wb) / d) as u8;
        [channel(0), channel(1), channel(2), 255]
    };
    let palette = if c0 > c1 || !punch_through {
        [a, b, mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [a, b, mix(1, 1, 2), [0, 0, 0, 0]]
    };
    let indices = u32_at(block, 4);
    std::array::from_fn(|i| palette[((indices >> (2 * i)) & 3) as usize])
}

/// The 16 alphas of a BC3 alpha block, row by row.
fn bc3_alphas(block: &[u8]) -> [u8; 16] {
    let (a0, a1) = (u16::from(block[0]), u16::from(block[1]));
    let palette: [u8; 8] = if a0 > a1 {
        std::array::from_fn(|i| match i {
            0 => a0 as u8,
            1 => a1 as u8,
            _ => {
                let i = i as u16;
                (((8 - i) * a0 + (i - 1) * a1) / 7) as u8
            }
        })
    } else {
        std::array::from_fn(|i| match i {
            0 => a0 as u8,
            1 => a1 as u8,
            6 => 0,
            7 => 255,
            _ => {
                let i = i as u16;
                (((6 - i) * a0 + (i - 1) * a1) / 5) as u8
            }
        })
    };
    let bits = block[2..8]
        .iter()
        .rev()
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    std::array::from_fn(|i| palette[((bits >> (3 * i)) & 7) as usize])
}

fn rgb565(c: u16) -> [u8; 4] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [
        ((r * 255 + 15) / 31) as u8,
        ((g * 255 + 31) / 63) as u8,
        ((b * 255 + 15) / 31) as u8,
        255,
    ]
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A texture file of `width` by `height` in `format` whose top mip is `data`.
    fn file(format: u32, width: u16, height: u16, data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER];
        bytes[..4].copy_from_slice(b"AVTX");
        bytes[4] = 1;
        bytes[7] = 2;
        bytes[8..12].copy_from_slice(&format.to_le_bytes());
        bytes[12..14].copy_from_slice(&width.to_le_bytes());
        bytes[14..16].copy_from_slice(&height.to_le_bytes());
        bytes[16..18].copy_from_slice(&1u16.to_le_bytes());
        bytes[20] = 1;
        bytes[21] = 1;
        bytes[STREAMS..STREAMS + 4].copy_from_slice(&(HEADER as u32).to_le_bytes());
        bytes[STREAMS + 4..STREAMS + 8].copy_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    /// A BC3 block: alpha endpoints and indices, then white and black colour endpoints and indices.
    fn bc3_block(a0: u8, a1: u8, alpha_bits: u64, colour_indices: u32) -> [u8; 16] {
        let mut block = [0u8; 16];
        block[0] = a0;
        block[1] = a1;
        block[2..8].copy_from_slice(&alpha_bits.to_le_bytes()[..6]);
        block[8..10].copy_from_slice(&0xffffu16.to_le_bytes());
        block[10..12].copy_from_slice(&0u16.to_le_bytes());
        block[12..16].copy_from_slice(&colour_indices.to_le_bytes());
        block
    }

    #[test]
    fn a_bc3_block_decodes_its_endpoints_and_steps() {
        // Texel 0 takes alpha 0 (endpoint a0) and colour 0 (white); texel 1 alpha 1 (a1) and colour
        // 1 (black); texel 2 the first interpolated alpha and colour.
        let alpha_bits = 0b010_001_000;
        let colour_indices = 0b10_01_00;
        let data = bc3_block(255, 0, alpha_bits, colour_indices);
        let image = decode(&file(DXGI_BC3_UNORM, 4, 4, &data)).unwrap();
        assert_eq!((image.width, image.height), (4, 4));
        assert_eq!(image.texel(0, 0), [255, 255, 255, 255]);
        assert_eq!(image.texel(1, 0), [0, 0, 0, 0]);
        assert_eq!(image.texel(2, 0), [170, 170, 170, 218]);
    }

    #[test]
    fn the_six_step_alpha_mode_has_transparent_and_opaque_codes() {
        let alpha_bits = (6u64 << 3) | 7;
        let data = bc3_block(10, 200, alpha_bits, 0);
        let image = decode(&file(DXGI_BC3_UNORM, 4, 4, &data)).unwrap();
        assert_eq!(image.texel(0, 0)[3], 255);
        assert_eq!(image.texel(1, 0)[3], 0);
    }

    #[test]
    fn a_size_that_is_not_a_block_multiple_is_cropped() {
        // 6 by 2 texels is two blocks across and one down.
        let block = bc3_block(255, 255, 0, 0);
        let data = [block, block].concat();
        let image = decode(&file(DXGI_BC3_UNORM, 6, 2, &data)).unwrap();
        assert_eq!(image.rgba.len(), 6 * 2 * 4);
        assert!(image.rgba.chunks(4).all(|t| t == [255, 255, 255, 255]));
    }

    #[test]
    fn bc1_has_a_punch_through_transparent_code() {
        // c0 <= c1 selects the three-colour mode, whose fourth code is transparent.
        let mut block = [0u8; 8];
        block[2..4].copy_from_slice(&0xffffu16.to_le_bytes());
        block[4..8].copy_from_slice(&3u32.to_le_bytes());
        let image = decode(&file(DXGI_BC1_UNORM, 4, 4, &block)).unwrap();
        assert_eq!(image.texel(0, 0), [0, 0, 0, 0]);
        assert_eq!(image.texel(1, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn unusable_files_are_refused() {
        assert_eq!(decode(b"AVTX"), Err(AvtxError::Truncated));
        let mut other = file(DXGI_BC3_UNORM, 4, 4, &[0; 16]);
        other[..4].copy_from_slice(b"DDS ");
        assert_eq!(decode(&other), Err(AvtxError::NotAvtx));
        assert_eq!(
            decode(&file(28, 4, 4, &[0; 64])),
            Err(AvtxError::UnsupportedFormat(28))
        );
        assert_eq!(
            decode(&file(DXGI_BC3_UNORM, 8, 4, &[0; 16])),
            Err(AvtxError::ShortData {
                needed: 32,
                found: 16
            })
        );
        let mut external = file(DXGI_BC3_UNORM, 4, 4, &[0; 16]);
        external[STREAMS + 11] = 1;
        assert_eq!(decode(&external), Err(AvtxError::External));
        assert!(matches!(
            decode(&file(DXGI_BC3_UNORM, 0, 4, &[])),
            Err(AvtxError::BadSize { .. })
        ));
    }
}
