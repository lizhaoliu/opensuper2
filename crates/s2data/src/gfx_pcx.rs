//! Full-screen pictures: ZSoft PCX (v5, RLE, 8-bit, 1 plane) in DAT entries.
//!
//! Loader `FUN_00420fab(id, palette_out, kind, ...)`: a negative id is an
//! absolute archive entry; a positive id is relative (DAT: entry = id + 0x19).
//! The 768-byte palette at the end is 8-bit; the game shifts it to 6-bit.
//!
//! | DAT entry | size | content | used by |
//! |-----------|------|---------|---------|
//! | 11 | 1280x907 | world map | `FUN_0040fd3c` |
//! | 12..=17 | 640x480 | town/shop/temple scenes (16, 17 share 15's palette slot) | `FUN_0040fd3c` |
//! | 18 | 1280x907 | world map, darker | `FUN_00415fd2` |
//! | 19 | 640x480 | title screen (its palette is used for 20) | `FUN_0040d7e1` |
//! | 20 | 640x480 | title screen, dimmed | `FUN_0040d7e1` |
//! | 26 | 640x480 | sea view | `FUN_0041a779` (id 1) |
//! | 27..=39 | 640x~1000 | full-body character art (ending/credits) | `FUN_0041ae08`, `FUN_0041b963` |
//! | 40 | 640x480 | world map, dark | `FUN_0041a779` (id 0xf) |

use crate::sprite::Image;
use crate::{format_err, le, Result};

/// DAT entry for a positive picture id (`local_b8[3] = 0x1a`).
pub const DAT_PICTURE_BASE: usize = 0x19;

#[derive(Debug, Clone)]
pub struct Pcx {
    pub image: Image,
    /// 8-bit RGB palette as stored.
    pub palette: [[u8; 3]; 256],
}

impl Pcx {
    /// The palette in the 6-bit form the game uses (`>> 2`).
    pub fn palette6(&self) -> Vec<u8> {
        self.palette.iter().flatten().map(|v| v >> 2).collect()
    }
}

pub fn decode_pcx(b: &[u8]) -> Result<Pcx> {
    if b.len() < 0x380 || b[0] != 10 || b[1] != 5 || b[2] != 1 {
        return Err(format_err("pcx", "not a v5 RLE PCX"));
    }
    let (x0, y0) = (le::u16(b, 4)? as usize, le::u16(b, 6)? as usize);
    let (x1, y1) = (le::u16(b, 8)? as usize, le::u16(b, 10)? as usize);
    if x1 < x0 || y1 < y0 {
        return Err(format_err("pcx", "bad window"));
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let bpl = le::u16(b, 0x42)? as usize;
    if bpl < w {
        return Err(format_err("pcx", "bytes per line smaller than width"));
    }
    let data = &b[128..b.len() - 0x300];
    let mut pixels = Vec::with_capacity(w * h);
    let mut p = 0;
    let mut row = Vec::with_capacity(bpl + 64);
    for _ in 0..h {
        row.clear();
        while row.len() < bpl {
            let c = *data.get(p).ok_or_else(|| format_err("pcx", "truncated"))?;
            if c & 0xc0 == 0xc0 {
                let v = *data.get(p + 1).ok_or_else(|| format_err("pcx", "truncated run"))?;
                row.extend(std::iter::repeat_n(v, (c & 0x3f) as usize));
                p += 2;
            } else {
                row.push(c);
                p += 1;
            }
        }
        pixels.extend_from_slice(&row[..w]);
    }
    let pal = &b[b.len() - 0x300..];
    let palette = std::array::from_fn(|i| [pal[i * 3], pal[i * 3 + 1], pal[i * 3 + 2]]);
    Ok(Pcx { image: Image { width: w, height: h, pixels }, palette })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_runs_and_literals() {
        let mut b = vec![0u8; 128];
        b[..3].copy_from_slice(&[10, 5, 1]);
        b[8] = 2; // x1 = 2 -> width 3
        b[0x42] = 4; // bytes per line (padded)
        b.extend_from_slice(&[0xc3, 7, 9, 0x0c]); // run of three 7s, literal 9 (padding), marker
        b.extend_from_slice(&[0u8; 0x300]);
        let n = b.len();
        b[n - 3] = 200;
        let pcx = decode_pcx(&b).unwrap();
        assert_eq!(pcx.image.pixels, vec![7, 7, 7]);
        assert_eq!(pcx.palette[255][0], 200);
        assert_eq!(pcx.palette6()[765], 50);
    }
}
