//! The frame stamp: the mirror's present counter drawn as a strip of black and white blocks in the
//! window's bottom-left corner, so that a screen recording shows which present each video frame
//! holds. The sidecar logs the same counter for each present ([`crate::recorder::note_present`]),
//! so a repeated video frame can be told apart: the same stamp twice is the recorder or the
//! compositor showing one present twice, and consecutive stamps on identical images are the mod
//! presenting a repeated eye image.
//!
//! The strip is [`STAMP_BITS`] blocks, least significant bit first from the left, each a white block
//! for a one and a black block for a zero, framed by a white start block and a black end block so a
//! reader can find it and its scale. The blocks are cleared straight into the back buffer with
//! `ID3D11DeviceContext1::ClearView` rectangles: no shader, no pipeline state.

use windows::Win32::Foundation::RECT;

/// The counter's bits the strip carries; the counter wraps there.
pub(super) const STAMP_BITS: u32 = 16;

/// The rectangles of one stamp, in back-buffer pixels: the white blocks and the black blocks.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Stamp {
    pub white: Vec<RECT>,
    pub black: Vec<RECT>,
}

/// The stamp of `counter` in a back buffer of `buffer` pixels shown in a window of `window`
/// pixels, with blocks [`BLOCK_PX`] window pixels square at the bottom-left corner.
pub(super) fn stamp(counter: u32, buffer: (u32, u32), window: (u32, u32)) -> Stamp {
    // The present stretches the buffer onto the window, so a block square in the window is this
    // many buffer pixels across and down.
    let sx = buffer.0 as f32 / window.0.max(1) as f32;
    let sy = buffer.1 as f32 / window.1.max(1) as f32;
    let width = ((BLOCK_PX as f32 * sx).round() as i32).max(1);
    let height = ((BLOCK_PX as f32 * sy).round() as i32).max(1);
    let bottom = buffer.1 as i32;
    let block = |index: u32| RECT {
        left: index as i32 * width,
        top: bottom - height,
        right: (index as i32 + 1) * width,
        bottom,
    };
    let mut white = vec![block(0)];
    let mut black = Vec::new();
    for bit in 0..STAMP_BITS {
        let rect = block(bit + 1);
        if (counter >> bit) & 1 == 1 {
            white.push(rect);
        } else {
            black.push(rect);
        }
    }
    black.push(block(STAMP_BITS + 1));
    Stamp { white, black }
}

/// A block's side, window pixels: large enough to survive the video's compression.
const BLOCK_PX: u32 = 8;

#[cfg(test)]
mod tests {
    use super::*;

    /// Read a stamp back as a reader of the video would: white blocks are ones.
    fn read(stamp: &Stamp, width: i32) -> u32 {
        stamp
            .white
            .iter()
            .map(|rect| rect.left / width)
            .filter(|&index| (1..=STAMP_BITS as i32).contains(&index))
            .fold(0, |value, index| value | 1 << (index - 1))
    }

    #[test]
    fn the_stamp_reads_back_as_its_counter() {
        for counter in [0, 1, 2, 0x5a5a, 0xffff] {
            let stamp = stamp(counter, (1920, 1080), (1920, 1080));
            assert_eq!(read(&stamp, BLOCK_PX as i32), counter);
            // The framing blocks and every bit: one block each.
            assert_eq!(
                stamp.white.len() + stamp.black.len(),
                STAMP_BITS as usize + 2
            );
        }
    }

    #[test]
    fn the_stamp_sits_at_the_bottom_left_and_stays_square_on_the_window() {
        // A buffer at half the window's width is stretched across it: half as many buffer pixels.
        let stamp = stamp(0, (960, 1080), (1920, 1080));
        let start = stamp.white[0];
        assert_eq!((start.left, start.bottom), (0, 1080));
        assert_eq!(start.right - start.left, BLOCK_PX as i32 / 2);
        assert_eq!(start.bottom - start.top, BLOCK_PX as i32);
    }

    #[test]
    fn the_counter_wraps_at_the_strips_width() {
        let wrapped = stamp(1 << STAMP_BITS | 3, (1920, 1080), (1920, 1080));
        assert_eq!(read(&wrapped, BLOCK_PX as i32), 3);
    }
}
