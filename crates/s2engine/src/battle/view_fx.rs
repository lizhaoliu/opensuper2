//! Spell/effect animations (CEL entry 1 frame sets), ported from the effect
//! player `FUN_00466507` of the caster object `DAT_004a1f58`.

use anyhow::Result;
use s2data::archive::offset_table;
use s2data::gfx_map::{MapTiles, Occlusion, Shading};
use s2data::gfx_palette::{BlendCache, ColorTree};
use s2data::sprite::{FrameSet, Image};
use s2data::{Archives, Kind};

use super::map::MapView;
use super::unit::{cut_by_units, DrawGlobals, Placed};

/// All effect frame sets plus the effect → sound table (BIN 10).
pub struct EffectBank {
    group: Vec<u8>,
    spans: Vec<s2data::archive::Span>,
    /// `DAT_004a1f50`: sound id per effect (index = effect id - 1).
    pub sounds: Vec<u8>,
}

impl EffectBank {
    pub fn load(arcs: &Archives) -> Result<EffectBank> {
        let group = arcs.read(Kind::Cel, 1)?;
        let spans = offset_table(&group)?;
        let sounds = arcs
            .read(Kind::Bin, s2data::audio::EFFECT_SOUND_ENTRY)
            .ok()
            .and_then(|b| s2data::audio::parse_effect_sounds(&b).ok())
            .unwrap_or_default();
        Ok(EffectBank { group, spans, sounds })
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// `FUN_0045e9c1`: frame set `id` (1-based).
    pub fn set(&self, id: usize) -> Result<FrameSet> {
        let s = *self.spans.get(id.wrapping_sub(1)).ok_or_else(|| anyhow::anyhow!("effect {id} out of range"))?;
        Ok(FrameSet::parse(self.group[s.offset as usize..(s.offset + s.size) as usize].to_vec())?)
    }

    /// Sound cue of effect `id` (0 = none).
    pub fn sound(&self, id: usize) -> u8 {
        self.sounds.get(id.wrapping_sub(1)).copied().unwrap_or(0)
    }
}

/// How an effect frame is placed (from the set's anchor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxMode {
    /// Anchor (-1, -1): full screen at (0, 0).
    Screen,
    /// Anchor (0, 0): bottom-centred on the cell, occluded like a unit.
    OnUnit,
    /// Anything else: anchored to the cell's diamond.
    Anchored,
}

pub fn fx_mode(fs: &FrameSet) -> FxMode {
    match fs.anchor {
        (-1, -1) => FxMode::Screen,
        (0, 0) => FxMode::OnUnit,
        _ => FxMode::Anchored,
    }
}

/// The frame numbers (0-based) one pass of the set plays.
pub fn pass_frames(fs: &FrameSet) -> Vec<usize> {
    fs.playback_order()
}

/// Number of passes: 1 + the extra loops of a negative sequence length.
pub fn passes(fs: &FrameSet) -> usize {
    1 + fs.loops.max(0) as usize
}

/// Draws one frame of an effect onto a page; the timing/presenting loop
/// lives in `view::play_effect`.
pub struct FxPainter<'a> {
    pub map: &'a MapView,
    pub tiles: &'a MapTiles,
    pub shading: &'a Shading,
    pub tree: &'a ColorTree,
    pub g: DrawGlobals,
    pub armies: [&'a [Placed<'a>]; 2],
}

impl FxPainter<'_> {
    /// Port of the per-frame body of FUN_00466507 for target cell
    /// (col, row). `behind_units` is the spell type 7 case
    /// (`DAT_004a1f54[spell-1] == 7`): anchored placement, then the caller
    /// redraws the units on top (FUN_00442e33).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, page: &mut Image, fs: &FrameSet, frame: usize, cache: &mut BlendCache, col: i32, row: i32, behind_units: bool) -> Result<()> {
        let img = fs.frame(frame)?;
        let (cx, cy) = self.map.cell_screen(col, row);
        let mode = if behind_units { FxMode::Anchored } else { fx_mode(fs) };
        match mode {
            FxMode::Screen => cache.blit(self.tree, page, &img, 0, 0),
            FxMode::Anchored => {
                cache.blit(self.tree, page, &img, cx + 0x20 - fs.anchor.0 as i32, cy + 0x20 - fs.anchor.1 as i32);
            }
            FxMode::OnUnit => self.on_cell(page, fs, img, cache, col, row),
        }
        Ok(())
    }

    /// Frame `frame` of `fs` bottom-centred on cell (col, row), occluded by
    /// the scenery and the units in front (the anchor-(0, 0) case of
    /// FUN_00466507, and every cell of FUN_004668dd / FUN_00467be7).
    pub fn draw_on_cell(&self, page: &mut Image, fs: &FrameSet, frame: usize, cache: &mut BlendCache, col: i32, row: i32) -> Result<()> {
        let img = fs.frame(frame)?;
        self.on_cell(page, fs, img, cache, col, row);
        Ok(())
    }

    fn on_cell(&self, page: &mut Image, fs: &FrameSet, mut raw: Image, cache: &mut BlendCache, col: i32, row: i32) {
        let (cx, cy) = self.map.cell_screen(col, row);
        let r = self.map.renderer(self.tiles, self.shading);
        r.occlude(&mut raw, col, row, Occlusion::default());
        for (side, army) in self.armies.iter().enumerate() {
            cut_by_units(self.map, army, side as i32, &self.g, col, row, &mut raw, 1, 0, 0, None);
        }
        let x = cx + 0x20 - fs.size.0 as i32 / 2;
        let y = cy + 0x20 - fs.size.1 as i32;
        cache.blit(self.tree, page, &raw, x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_from_anchor() {
        let mut b = Vec::new();
        for v in [0i16, -1, -1, 1, 1, 0, -2, 5] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let fs = FrameSet::parse(b.clone()).unwrap();
        assert_eq!(fx_mode(&fs), FxMode::Screen);
        assert_eq!(passes(&fs), 3);
        b[2..6].fill(0);
        assert_eq!(fx_mode(&FrameSet::parse(b).unwrap()), FxMode::OnUnit);
    }
}
