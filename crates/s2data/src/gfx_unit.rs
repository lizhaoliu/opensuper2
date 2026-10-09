//! Unit (battle character) sprites.
//!
//! Two banks with the same layout (`FUN_0043d46b` loads both):
//!
//! | bank   | type index | frames | types | used for |
//! |--------|-----------|--------|-------|----------|
//! | normal | PRG 1     | PRG 2  | 136   | classes/monsters (`unit+4` = type id) |
//! | summon | CEL 4     | CEL 5  | 11    | summoned creatures, type ids 0x71.. (`unit+0x30 & 0x10000000`) |
//!
//! Type index: `u16 n; (u16 frame_count, u16 first_frame, u32 byte_size)[n];
//! u8 actions[n][200]`. Each 200-byte block holds NUL-terminated strings of
//! 1-based frame numbers (relative to the type's first frame), one string per
//! action; parsing stops at an empty string or after 30 (`FUN_0043e83e`).
//! Actions 0..3 stand, 4..7 walk, 8..11 attack, and later groups (hit/cast/...)
//! come in fours, one per facing.
//!
//! Frame table: `u16 n; (u32 offset, u16 size, i8 dx)[n]; RLE frames` with
//! offsets relative to the entry. `dx` shifts the frame horizontally when
//! drawn (`FUN_0043a34f` stores it in `unit+0xac`).
//!
//! Frames are 128x160 (some summons larger). A unit on cell (col, row) with
//! terrain height `h` is drawn at map pixel
//! `x = col*64 + 32*(row&1) + (64 - w)/2 + dx`,
//! `y = row*16 - (h-1+lift)*16 - (fh - 32)` (`FUN_0042665a`; `/2` truncates
//! toward zero), i.e. bottom-centred on the tile diamond.

use crate::sprite::{decode_rle, Image};
use crate::{format_err, le, Archives, Kind, Result};

pub const ACTION_BLOCK: usize = 200;
pub const MAX_ACTIONS: usize = 30;
/// First type id served by the summon bank.
pub const SUMMON_BASE: usize = 0x71;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitType {
    pub frame_count: usize,
    pub first_frame: usize,
    pub byte_size: u32,
    /// Frame numbers (1-based, relative to `first_frame`) per action.
    pub actions: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRec {
    pub offset: u32,
    pub size: u16,
    pub dx: i8,
}

pub struct UnitSprites {
    pub types: Vec<UnitType>,
    pub frames: Vec<FrameRec>,
    data: Vec<u8>,
}

/// Split one 200-byte action block like `FUN_0043e83e`.
pub fn parse_actions(block: &[u8]) -> Vec<Vec<u8>> {
    let strlen = |at: usize| block.get(at..).map_or(0, |s| s.iter().position(|&c| c == 0).unwrap_or(s.len()));
    let mut starts = vec![0usize];
    while starts.len() < MAX_ACTIONS {
        let prev = *starts.last().unwrap();
        let len = strlen(prev);
        if len == 0 || block.get(prev + len + 1).copied().unwrap_or(0) == 0 {
            break;
        }
        starts.push(prev + len + 1);
    }
    starts.iter().map(|&s| block[s..s + strlen(s)].to_vec()).filter(|a| !a.is_empty()).collect()
}

impl UnitSprites {
    fn parse(index: &[u8], frames: Vec<u8>) -> Result<UnitSprites> {
        let n = le::u16(index, 0)? as usize;
        let blocks = 2 + 8 * n;
        let mut types = Vec::with_capacity(n);
        for i in 0..n {
            let block = index
                .get(blocks + i * ACTION_BLOCK..blocks + (i + 1) * ACTION_BLOCK)
                .ok_or_else(|| format_err("unit index", "truncated action blocks"))?;
            types.push(UnitType {
                frame_count: le::u16(index, 2 + 8 * i)? as usize,
                first_frame: le::u16(index, 4 + 8 * i)? as usize,
                byte_size: le::u32(index, 6 + 8 * i)?,
                actions: parse_actions(block),
            });
        }
        let nf = le::u16(&frames, 0)? as usize;
        let mut recs = Vec::with_capacity(nf);
        for i in 0..nf {
            let p = 2 + 7 * i;
            let rec = FrameRec {
                offset: le::u32(&frames, p)?,
                size: le::u16(&frames, p + 4)?,
                dx: *frames.get(p + 6).ok_or_else(|| format_err("unit frames", "truncated"))? as i8,
            };
            if rec.offset as usize + rec.size as usize > frames.len() {
                return Err(format_err("unit frames", format!("frame {i} beyond entry")));
            }
            recs.push(rec);
        }
        if let Some(t) = types.iter().find(|t| t.first_frame + t.frame_count > nf) {
            return Err(format_err("unit index", format!("type uses frames past {nf}: {t:?}")));
        }
        Ok(UnitSprites { types, frames: recs, data: frames })
    }

    /// PRG 1 + PRG 2.
    pub fn load_normal(arcs: &Archives) -> Result<UnitSprites> {
        UnitSprites::parse(&arcs.read(Kind::Prg, 1)?, arcs.read(Kind::Prg, 2)?)
    }

    /// CEL 4 + CEL 5.
    pub fn load_summon(arcs: &Archives) -> Result<UnitSprites> {
        UnitSprites::parse(&arcs.read(Kind::Cel, 4)?, arcs.read(Kind::Cel, 5)?)
    }

    /// Type by 1-based id within this bank.
    pub fn unit_type(&self, id: usize) -> Result<&UnitType> {
        id.checked_sub(1)
            .and_then(|i| self.types.get(i))
            .ok_or_else(|| format_err("unit sprites", format!("type {id} out of range")))
    }

    /// Frame `n` (1-based, as stored in action strings) of type `id`, with its dx.
    pub fn frame(&self, id: usize, n: usize) -> Result<(Image, i8)> {
        let t = self.unit_type(id)?;
        if n == 0 || n > t.frame_count {
            return Err(format_err("unit sprites", format!("type {id} has no frame {n}")));
        }
        self.frame_abs(t.first_frame + n - 1)
    }

    /// Frame by absolute index in the frame table.
    pub fn frame_abs(&self, i: usize) -> Result<(Image, i8)> {
        let r = self.frames.get(i).ok_or_else(|| format_err("unit sprites", format!("no frame {i}")))?;
        let (img, _) = decode_rle(&self.data[r.offset as usize..r.offset as usize + r.size as usize])?;
        Ok((img, r.dx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_stop_at_empty_string() {
        let mut b = vec![0u8; ACTION_BLOCK];
        b[..9].copy_from_slice(&[1, 0, 2, 0, 5, 6, 0, 0, 9]);
        assert_eq!(parse_actions(&b), vec![vec![1], vec![2], vec![5, 6]]);
    }
}
