//! Map overlays: the cell cursor (`FUN_0042bfea` / `FUN_0042c121`) and the
//! move/attack range diamonds (`FUN_0047280f`, shown by `FUN_00470dad`).

use s2data::gfx_map::{MapTiles, Occlusion, Shading};
use s2data::sprite::Image;

use super::map::MapView;
use super::unit::{cut_by_units, DrawGlobals, Placed};

/// Cursor images (sysimg 25, 26, 27).
pub struct CursorArt<'a> {
    /// Flat 64x32 diamond, used on cells without a full tile.
    pub flat: &'a Image,
    /// Tall cursor, back half (64x137) and front half (64x137).
    pub back: &'a Image,
    pub front: &'a Image,
}

/// The map cursor state (`DAT_0048eaa0/a4` cell, `DAT_0048eaac/b0` last
/// mouse position, `DAT_00497588` height, `DAT_004952d0` flat, `DAT_00497590`
/// off-map, `DAT_0048eaa8` moved, `DAT_0049758c` shown).
#[derive(Debug, Clone, Copy, Default)]
pub struct MapCursor {
    pub col: i32,
    pub row: i32,
    pub mouse: (i32, i32),
    pub height: i32,
    pub flat: bool,
    pub off_map: bool,
    pub moved: bool,
    pub shown: bool,
}

impl MapCursor {
    /// Port of FUN_0042bfea: re-read the cell under the cursor.
    pub fn refresh(&mut self, map: &MapView) {
        let (c, r, w, h2) = (self.col, self.row, map.width(), map.rows());
        // FUN_004257ac: rows 1..=2h-2, not the first/last cell of even rows.
        let cell = if r < 1 || r > h2 - 2 || (r & 1 == 0 && (c == 0 || c == w - 1)) {
            None
        } else {
            map.def.cells.get((r * w + c).max(0) as usize).copied()
        };
        match cell {
            None => {
                self.flat = true;
                self.off_map = true;
            }
            Some(cell) => {
                self.height = cell.height as i32;
                if self.height == 0 || self.height > 0x14 {
                    self.height = 1;
                }
                if cell.ground == 1 {
                    self.flat = true;
                    self.off_map = true;
                } else {
                    self.off_map = false;
                    self.flat = map.def.flags_at(c, r).is_some_and(|f| f & 0x8000 != 0);
                }
            }
        }
        self.shown = true;
        self.moved = true;
    }

    /// Port of FUN_0042bfea's callers: put the cursor on a cell.
    pub fn set_cell(&mut self, map: &MapView, col: i32, row: i32) {
        self.col = col;
        self.row = row;
        self.refresh(map);
    }

    /// First half of FUN_0042c121: follow the mouse; true if the cell changed.
    pub fn track_mouse(&mut self, map: &MapView, mouse: (i32, i32), mesh: bool) -> bool {
        if mouse == self.mouse {
            return false;
        }
        self.mouse = mouse;
        match map.screen_to_cell(mouse.0, mouse.1, mesh) {
            Some((c, r)) if (c, r) != (self.col, self.row) => {
                self.set_cell(map, c, r);
                true
            }
            _ => false,
        }
    }
}

/// What the cursor and range passes need to occlude their images.
pub struct OverlayScene<'a> {
    pub map: &'a MapView,
    pub tiles: &'a MapTiles,
    pub shading: &'a Shading,
    pub g: DrawGlobals,
    pub armies: [&'a [Placed<'a>]; 2],
}

impl OverlayScene<'_> {
    fn occlude(&self, img: &mut Image, col: i32, row: i32, own_overlay: bool, mode: s2data::gfx_map::CutMode) {
        let r = self.map.renderer(self.tiles, self.shading);
        r.occlude(img, col, row, Occlusion { own_overlay, mode, ..Default::default() });
    }

    /// Second half of FUN_0042c121: draw the cursor onto a page.
    pub fn draw_cursor(&self, page: &mut Image, cur: &mut MapCursor, art: &CursorArt) {
        if !(0 < cur.height && cur.height < 0x15) || !(cur.moved || cur.shown) {
            return;
        }
        cur.moved = false;
        let (c, r, h) = (cur.col, cur.row, cur.height);
        if cur.flat {
            if !cur.off_map {
                self.map.blit_at_cell(page, c, r, h, art.flat);
            }
            return;
        }
        let mode = self.g.cut_mode();
        let mut back = art.back.clone();
        self.occlude(&mut back, c, r, true, mode);
        for side in 0..2 {
            cut_by_units(self.map, self.armies[side], side as i32, &self.g, c, r, &mut back, 3, 1, 0, None);
        }
        self.map.blit_at_cell(page, c, r, h, &back);
        let mut front = art.front.clone();
        self.occlude(&mut front, c, r, false, mode);
        cut_by_units(self.map, self.armies[0], 0, &self.g, c, r, &mut front, 1, 2, 0, None);
        cut_by_units(self.map, self.armies[1], 1, &self.g, c, r, &mut front, 1, 0, 0, None);
        self.map.blit_at_cell(page, c, r, h, &front);
    }
}

impl OverlayScene<'_> {
    /// Port of FUN_0044179b: the tall cursor on cell (c, r) at the cell's
    /// height, independent of the cursor state (the attack target marker
    /// of the AI's forecast pause).
    pub fn draw_marker(&self, page: &mut Image, c: i32, r: i32, art: &CursorArt) {
        let h = self.map.height_at(c, r);
        let mut back = art.back.clone();
        self.occlude(&mut back, c, r, true, s2data::gfx_map::CutMode::Solid);
        for side in 0..2 {
            cut_by_units(self.map, self.armies[side], side as i32, &self.g, c, r, &mut back, 3, 1, 0, None);
        }
        self.map.blit_at_cell(page, c, r, h, &back);
        let mut front = art.front.clone();
        self.occlude(&mut front, c, r, false, s2data::gfx_map::CutMode::Solid);
        for side in 0..2 {
            cut_by_units(self.map, self.armies[side], side as i32, &self.g, c, r, &mut front, 1, 0, 0, None);
        }
        self.map.blit_at_cell(page, c, r, h, &front);
    }

    /// Port of FUN_00427b53: cell (c, r) as it looks on the map surface
    /// (ground tiles 7 and up from the tile itself), cut to the mask of
    /// ground tile 1 with the cell's overlay base, then occluded by the
    /// scenery and the units in front, drawn onto `page`. Meant to light the
    /// attack range on a darkened window; tile 1 is blank in the game data,
    /// so (as in the original) nothing is drawn.
    pub fn light_cell(&self, page: &mut Image, c: i32, r: i32) {
        let w = self.map.width();
        let idx = r * w + c;
        let Some(cell) = (idx >= 0).then(|| self.map.def.cells.get(idx as usize)).flatten().copied() else { return };
        if cell.ground <= 0 {
            return;
        }
        let mut buf = if cell.ground < 7 {
            let x = c * 64 + if r & 1 != 0 { 32 } else { 0 };
            let y = (r - (cell.height as i32 - 1)) * 16;
            super::unit::grab(&self.map.surface, x, y, 64, 32)
        } else {
            // The tile's top 64x32 (the copy keeps 0x800 bytes, height 0x1f).
            match self.tiles.ground_tile(cell.ground as i32) {
                Some(t) => top32(&t),
                None => return,
            }
        };
        let Some(mask) = self.tiles.ground_tile(1).map(|t| top32(&t)) else { return };
        if cell.overlay == 0 {
            for (o, &m) in buf.pixels.iter_mut().zip(mask.pixels.iter()) {
                if m == 0 {
                    *o = 0;
                }
            }
        } else if let Some(ov) = self.tiles.overlay_tile(cell.overlay as i32) {
            for (i, (o, &m)) in buf.pixels.iter_mut().zip(mask.pixels.iter()).enumerate() {
                let v = ov.pixels.get(0x1000 + i).copied().unwrap_or(0);
                if m == 0 {
                    *o = 0;
                } else if v != 0 && *o != 0 {
                    *o = v;
                }
            }
        }
        self.map.renderer(self.tiles, self.shading).occlude(&mut buf, c, r, Occlusion::default());
        for side in 0..2 {
            cut_by_units(self.map, self.armies[side], side as i32, &self.g, c, r, &mut buf, 2, 1, 0, None);
        }
        let (x, y) = self.map.cell_screen(c, r);
        page.blit(&buf, x as isize, y as isize);
    }
}

/// `node+0x12` of a move-range node: what the unit could still do there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeMark {
    None,
    Attack,
    Spell,
}

/// One cell of a movement range (`DAT_004a2b48` node: x, y, height, can_stop, mark).
#[derive(Debug, Clone, Copy)]
pub struct RangeCell {
    pub col: i32,
    pub row: i32,
    pub height: i32,
    pub mark: RangeMark,
}

/// The two pre-composed pages of a range display (`FUN_00470dad` keeps the
/// scroll they were built at in `+0x0c/+0x10`).
pub struct RangePages {
    /// Page 1: map window + diamonds occluded by scenery and units.
    pub occluded: Image,
    /// Page 2: map window + plain cell shapes cut by units only (hard mode).
    pub plain: Image,
    pub scroll: (i32, i32),
    /// `+0x28` page shown in hard mode (1 or 2), `+0x2a`/`+0x2c` pulse level/direction, `+0x20/+0x24` timers.
    pub page: u16,
    pub pulse: u16,
    pub pulse_down: bool,
    pub t_page: i32,
    pub t_pulse: i32,
    /// A spell aim page (FUN_00401bfa, page 2): `occluded` is copied whole
    /// over the window, shifted by the scroll since it was built
    /// (FUN_00401d75).
    pub aim: bool,
}

/// Diamond images (sysimg 34..37) and the cell mask (ground tile 1).
pub struct RangeArt<'a> {
    /// sysimg 0x22: enemy range; 0x23 own move; 0x24 attackable; 0x25 spell.
    pub diamonds: [&'a Image; 4],
}

impl OverlayScene<'_> {
    /// Port of FUN_0047280f for every cell: build the range pages from the
    /// current map window. `own_unit` is the moving unit (excluded from the
    /// unit cut like FUN_00440894); `hard` is the enemy-range style used at
    /// difficulty >= 4 outside VS mode.
    #[allow(clippy::too_many_arguments)]
    pub fn build_range(&self, cells: &[RangeCell], army_side: i32, own_unit: (i32, i32), hard: bool, art: &RangeArt, window: &Image) -> RangePages {
        let mut occluded = window.clone();
        let mut plain = window.clone();
        let mask = self.tiles.ground_tile(1);
        for rc in cells {
            let (c, r) = (rc.col, rc.row);
            let img = if !hard {
                let mut k = match rc.mark {
                    RangeMark::Spell => 3,
                    RangeMark::Attack => 2,
                    RangeMark::None => 1,
                };
                if army_side == 1 && k == 1 {
                    k = 0;
                }
                art.diamonds[k].clone()
            } else {
                match self.hard_cell(c, r, mask.as_ref()) {
                    Some(i) => i,
                    None => continue,
                }
            };
            let mut a = img;
            cut_by_units(self.map, self.armies[army_side as usize & 1], army_side, &self.g, c, r, &mut a, 1, 0, 0, Some(own_unit));
            let opp = (army_side as usize & 1) ^ 1;
            cut_by_units(self.map, self.armies[opp], opp as i32, &self.g, c, r, &mut a, 1, 0, 0, None);
            self.map.blit_at_cell(&mut plain, c, r, rc.height, &a);
            let r2 = self.map.renderer(self.tiles, self.shading);
            r2.occlude(&mut a, c, r, Occlusion::default());
            self.map.blit_at_cell(&mut occluded, c, r, rc.height, &a);
        }
        RangePages { occluded, plain, scroll: (self.map.scroll_x, self.map.scroll_y), page: 1, pulse: 0, pulse_down: false, t_page: 0, t_pulse: 0, aim: false }
    }

    /// Port of FUN_00401bfa (+ FUN_00403e52 for the area): the spell aim
    /// page — the map window with diamond `range_img` (sysimg 0x21) on every
    /// range cell in view and `area_img` (0x22) on every area cell, each cut
    /// by the caster's army (not the caster, FUN_00440894) and the
    /// opponent's units on the same row, and by the scenery except
    /// overlays on range/area cells (FUN_00402213).
    #[allow(clippy::too_many_arguments)]
    pub fn build_aim(&self, range: &[(i32, i32)], area: &[(i32, i32)], side: usize, caster: (i32, i32), range_img: &Image, area_img: &Image, window: &Image) -> RangePages {
        let mut page = window.clone();
        let marked = |x: i32, y: i32| range.contains(&(x, y)) || area.contains(&(x, y));
        let cells = range.iter().map(|&c| (c, range_img)).chain(area.iter().filter(|&&c| c.0 != -1 && c.1 != -1).map(|&c| (c, area_img)));
        for ((x, y), img) in cells {
            if !self.map.cell_in_view(x, y, 0) {
                continue;
            }
            let mut a = img.clone();
            let h = self.map.height_at(x, y);
            cut_by_units(self.map, self.armies[side & 1], side as i32, &self.g, x, y, &mut a, 3, 1, 0, Some(caster));
            let opp = (side & 1) ^ 1;
            cut_by_units(self.map, self.armies[opp], opp as i32, &self.g, x, y, &mut a, 3, 1, 0, None);
            let occ = Occlusion { skip_overlay: Some(&marked), ..Default::default() };
            self.map.renderer(self.tiles, self.shading).occlude(&mut a, x, y, occ);
            self.map.blit_at_cell(&mut page, x, y, h, &a);
        }
        let plain = Image::new(1, 1);
        RangePages { occluded: page, plain, scroll: (self.map.scroll_x, self.map.scroll_y), page: 1, pulse: 0, pulse_down: false, t_page: 0, t_pulse: 0, aim: true }
    }

    /// The hard-mode cell image of FUN_0047280f: the cell's own ground
    /// (and overlay) pixels inside the diamond mask.
    fn hard_cell(&self, c: i32, r: i32, mask: Option<&Image>) -> Option<Image> {
        let mask = mask?;
        let (w, h2) = (self.map.width(), self.map.rows());
        if r < 1 || r > h2 - 2 || (r & 1 == 0 && (c == 0 || c == w - 1)) {
            return None;
        }
        let cell = *self.map.def.cells.get((r * w + c).max(0) as usize)?;
        let mut out = Image::new(64, 32);
        if cell.ground > 0 && cell.ground < 5 && cell.height != 0 {
            let mut x = c * 64;
            if r & 1 != 0 {
                x += 32;
            }
            let y = (r - (cell.height as i32 - 1)) * 16;
            out = super::unit::grab(&self.map.surface, x, y, 64, 32);
            for (o, m) in out.pixels.iter_mut().zip(mask.pixels.iter()) {
                if *m == 0 {
                    *o = 0;
                }
            }
        } else if cell.ground > 4 {
            let t = self.tiles.ground_tile(cell.ground as i32)?;
            let ov = self.tiles.overlay_tile(cell.overlay as i32);
            for i in 0..64 * 32 {
                let (tp, mp) = (t.pixels[i], mask.pixels[i]);
                out.pixels[i] = if tp == 0 || mp == 0 {
                    0
                } else {
                    match &ov {
                        Some(o) if o.pixels[0x1000 + i] != 0 => o.pixels[0x1000 + i],
                        _ => tp,
                    }
                };
            }
        } else {
            return None;
        }
        Some(out)
    }
}

impl RangePages {
    /// Port of FUN_00472c10: advance the hard-mode page flip and pulse.
    pub fn tick(&mut self, now: i32) {
        let period = if self.page != 1 { 0x3c } else { 0x64 };
        if (self.t_page - now).abs() > period {
            self.t_page = now;
            self.page = if self.page != 1 { 1 } else { 2 };
        }
        if (self.t_pulse - now).abs() > 0x19 {
            self.t_pulse = now;
            if !self.pulse_down {
                if self.pulse == 4 {
                    self.pulse_down = true;
                    self.pulse -= 1;
                } else {
                    self.pulse += 1;
                }
            } else if self.pulse == 0 {
                self.pulse_down = false;
                self.pulse += 1;
            } else {
                self.pulse -= 1;
            }
        }
    }

    /// The range part of FUN_00470dad: compose the page shown this frame
    /// onto `page` (which already holds the current map window), allowing
    /// for the camera having moved since the pages were built.
    pub fn compose(&mut self, page: &mut Image, now: i32, hard: bool, tints: &[[u8; 256]], scroll: (i32, i32)) {
        let (sx, sy) = (scroll.0 * 32, scroll.1 * 16);
        let (bx, by) = (self.scroll.0 * 32, self.scroll.1 * 16);
        let dxp = (bx - sx).abs();
        let dyp = (by - sy).abs();
        if dxp >= 640 || dyp >= 480 {
            return;
        }
        if self.aim {
            // FUN_00401d75: page 2 over the window where they overlap.
            let (mut d0, mut s0) = (0usize, 0usize);
            if sy < by {
                d0 += dyp as usize * 640;
            } else if by < sy {
                s0 += dyp as usize * 640;
            }
            if sx < bx {
                d0 += dxp as usize;
            } else if bx < sx {
                s0 += dxp as usize;
            }
            let n = (640 - dxp) as usize;
            for r in 0..(480 - dyp) as usize {
                page.pixels[d0 + r * 640..d0 + r * 640 + n].copy_from_slice(&self.occluded.pixels[s0 + r * 640..s0 + r * 640 + n]);
            }
            return;
        }
        if !hard {
            // FUN_0046ec6b(scr, 1, 0): page 1 replaces the window.
            page.pixels.copy_from_slice(&self.occluded.pixels);
            return;
        }
        self.tick(now);
        let src = if self.page == 1 { &self.occluded } else { &self.plain };
        let lut = &tints[self.pulse as usize];
        let (mut d0, mut s0) = (0usize, 0usize);
        if sy < by {
            d0 += dyp as usize * 640;
        } else if by < sy {
            s0 += dyp as usize * 640;
        }
        if sx < bx {
            d0 += dxp as usize;
        } else if bx < sx {
            s0 += dxp as usize;
        }
        for r in 0..(480 - dyp) as usize {
            for c in 0..(640 - dxp) as usize {
                let v = src.pixels[s0 + r * 640 + c];
                if v != 0 {
                    page.pixels[d0 + r * 640 + c] = lut[v as usize];
                }
            }
        }
    }
}

/// The top 64x32 rows of a ground tile (its diamond).
fn top32(t: &Image) -> Image {
    let mut out = Image::new(64, 32);
    let w = t.width.min(64);
    for y in 0..32.min(t.height) {
        out.pixels[y * 64..y * 64 + w].copy_from_slice(&t.pixels[y * t.width..y * t.width + w]);
    }
    out
}
