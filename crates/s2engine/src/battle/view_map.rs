//! Battle map display: the map surface (`DAT_004947f0`), the camera kept in
//! the map object (`DAT_00497a68+0x6c/+0x70`) and the cell/screen transforms.
//!
//! The original composes the static background once per map
//! (`FUN_00424d81`), bakes every idle unit into that same surface
//! (`FUN_004403b5`) and each frame copies a 640x480 window of it to the
//! current page (`FUN_00424c5f`). We keep the composed background separately
//! (`bg`) and rebuild `surface` = `bg` + baked units whenever units change,
//! instead of the original's per-unit save-under/restore bookkeeping; the
//! resulting pixels are the same except where that bookkeeping produced
//! artefacts.

use anyhow::Result;
use s2data::gfx_map::{BattleMap as MapDef, MapRenderer, MapTiles, RenderOptions, Shading};
use s2data::sprite::Image;
use s2data::Archives;

use crate::sys::{SCREEN_H, SCREEN_W};

/// Scroll codes of `FUN_00426fdb` (the key/edge-scroll codes of the player phase).
pub mod scroll {
    pub const UP: i32 = -0x10;
    pub const DOWN: i32 = -0x0f;
    pub const LEFT: i32 = -0x0e;
    pub const RIGHT: i32 = -0x0d;
    pub const UP_LEFT: i32 = -0x0c;
    pub const DOWN_LEFT: i32 = -0x0b;
    pub const UP_RIGHT: i32 = -0x0a;
    pub const DOWN_RIGHT: i32 = -0x09;
}

/// C `int` division (truncates toward zero).
fn cdiv(a: i32, b: i32) -> i32 {
    a / b
}

pub struct MapView {
    /// The map definition (cells, objects, flags) from PRG 6.
    pub def: MapDef,
    /// 1-based PRG 6 id.
    pub id: usize,
    pub opts: RenderOptions,
    /// Static background as built by `FUN_00424d81` (no units).
    pub bg: Image,
    /// `DAT_004947f0`: background plus baked (idle) units.
    pub surface: Image,
    /// `+0x6c`: scroll in half cells (32 px).
    pub scroll_x: i32,
    /// `+0x70`: scroll in rows (16 px).
    pub scroll_y: i32,
    /// `+0x84`: cells with a category-0xc (treasure chest) overlay, in scan
    /// order (`FUN_00425075`); `None` once removed.
    pub chests: Vec<Option<(i32, i32)>>,
}

impl MapView {
    /// `FUN_00424d81`: load map `id` and build its background.
    pub fn load(arcs: &Archives, tiles: &MapTiles, shading: &Shading, id: usize, opts: RenderOptions) -> Result<MapView> {
        let counts = [tiles.ground.len(), tiles.overlay.len(), tiles.object.len()];
        let def = MapDef::load(arcs, id, counts)?;
        let mut chests = Vec::new();
        for r in 0..def.rows() {
            let n = if r & 1 == 0 { def.width } else { def.width - 1 };
            for c in 0..n {
                let ov = def.cells[(r * def.width + c) as usize].overlay as i32;
                if ov > 0 && tiles.attrs.overlay_category(ov) == s2data::gfx_map::CAT_DOOR {
                    chests.push(Some((c, r)));
                }
            }
        }
        // FUN_00425075 leaves the camera at (1, 1).
        let mut v = MapView { def, id, opts, bg: Image::new(0, 0), surface: Image::new(0, 0), scroll_x: 1, scroll_y: 1, chests };
        v.rebuild_background(tiles, shading);
        Ok(v)
    }

    /// Port of FUN_0042aba8: keep the chest of each of the first `count`
    /// chest cells whose treasure record still holds four valid item ids
    /// (`DAT_00494598`, `[0] == 0` once taken); clear the others' overlay.
    /// Returns true if a chest was removed.
    pub fn apply_treasure(&mut self, treasure: &[[i16; 4]], count: i32) -> bool {
        let mut changed = false;
        for i in 0..(count.max(0) as usize).min(self.chests.len()) {
            let Some((c, r)) = self.chests[i] else { continue };
            let Some(f) = self.def.flags_at(c, r) else { continue };
            let idx = (f & 0xff) as i32;
            if idx == 0 || idx > count {
                continue;
            }
            let rec = treasure.get(idx as usize - 1).copied().unwrap_or([0; 4]);
            let valid = rec.iter().take_while(|&&v| (v as u16) < 0x12f && v != 0).count();
            let ci = (r * self.width() + c) as usize;
            let keep = valid == 4 && self.def.cells[ci].overlay > 0;
            if !keep {
                self.def.cells[ci].overlay = 0;
                self.chests[i] = None;
                changed = true;
            }
        }
        changed
    }

    pub fn renderer<'a>(&'a self, tiles: &'a MapTiles, shading: &'a Shading) -> MapRenderer<'a> {
        MapRenderer::new(&self.def, tiles, shading, self.opts)
    }

    /// FUN_00424d81's drawing: the background (FUN_00425075 and the
    /// backdrops), then the chests still on the map (FUN_0042ae85). The
    /// surface is reset to it; rebake the units afterwards.
    pub fn rebuild_background(&mut self, tiles: &MapTiles, shading: &Shading) {
        let mut r = MapRenderer::new(&self.def, tiles, shading, self.opts);
        let mut bg = r.render_background();
        for &(c, row) in self.chests.iter().take(self.opts.door_count.max(0) as usize).flatten() {
            let Some(cell) = self.def.cell(c, row) else { continue };
            let Some(mut img) = tiles.overlay_tile(cell.overlay as i32) else { continue };
            r.occlude(&mut img, c, row, s2data::gfx_map::Occlusion::default());
            let x = c * 64 + if row & 1 != 0 { 32 } else { 0 };
            let y = row * 16 - 0x40 - (cell.height as i32 - 1) * 16;
            bg.blit(&img, x as isize, y as isize);
        }
        self.surface = bg.clone();
        self.bg = bg;
    }

    pub fn width(&self) -> i32 {
        self.def.width
    }

    pub fn rows(&self) -> i32 {
        self.def.rows()
    }

    /// `FUN_004255da`.
    pub fn height_at(&self, col: i32, row: i32) -> i32 {
        self.def.height_at(col, row) as i32
    }

    /// Top-left of the cell diamond in map-surface pixels, elevation applied.
    pub fn cell_world(&self, col: i32, row: i32) -> (i32, i32) {
        self.def.cell_origin(col, row)
    }

    /// Port of FUN_00427a10: cell -> screen position of its diamond's
    /// top-left, with scrolling and elevation. (The original's four parity
    /// cases all reduce to world position minus scroll.)
    pub fn cell_screen(&self, col: i32, row: i32) -> (i32, i32) {
        let (wx, wy) = self.cell_world(col, row);
        (wx - self.scroll_x * 32, wy - self.scroll_y * 16)
    }

    /// Port of FUN_0042652e: draw a raw image on cell (col, row) with the
    /// given height, bottom-aligned 32 px below the diamond's top (cursors,
    /// range diamonds).
    pub fn blit_at_cell(&self, dst: &mut Image, col: i32, row: i32, height: i32, img: &Image) {
        let x = (col * 64 + if row & 1 != 0 { 32 } else { 0 }) - self.scroll_x * 32;
        let u = row - self.scroll_y;
        let y = u * 16 - ((height - 1) * 16 - 0x1f + (img.height as i32 - 1));
        dst.blit(img, x as isize, y as isize);
    }

    /// Port of FUN_00424c5f: copy the visible 640x480 window of the surface.
    pub fn blit_window(&self, dst: &mut Image) {
        self.blit_window_offset(dst, 0, 0);
    }

    /// Port of FUN_00424cec: the window shifted by (dx, dy) pixels.
    pub fn blit_window_offset(&self, dst: &mut Image, dx: i32, dy: i32) {
        let sx = self.scroll_x * 32 + dx;
        let sy = self.scroll_y * 16 + dy;
        let (sw, sh) = (self.surface.width as i32, self.surface.height as i32);
        for y in 0..SCREEN_H as i32 {
            let row = &mut dst.pixels[y as usize * SCREEN_W..(y as usize + 1) * SCREEN_W];
            let src_y = sy + y;
            if src_y < 0 || src_y >= sh {
                row.fill(0);
                continue;
            }
            let x0 = sx.max(0);
            let x1 = (sx + SCREEN_W as i32).min(sw);
            row.fill(0);
            if x0 < x1 {
                let s = &self.surface.pixels[(src_y * sw + x0) as usize..(src_y * sw + x1) as usize];
                row[(x0 - sx) as usize..(x1 - sx) as usize].copy_from_slice(s);
            }
        }
    }

    /// Port of FUN_0042681e: centre the camera on a cell (`use_height`
    /// accounts for its elevation).
    pub fn center_on(&mut self, col: i32, row: i32, use_height: bool) {
        let hm1 = if use_height { self.height_at(col, row) - 1 } else { 0 };
        let mut sx = if row & 1 == 0 { col * 2 - 9 } else { col * 2 - 8 };
        let mut sy = if use_height { row - (hm1 + 0x10) } else { row - 0x10 };
        let (w, h2) = (self.width(), self.rows());
        if w <= (sx + 0x15) >> 1 {
            sx = w * 2 - 0x15;
        }
        if h2 <= sy + 0x1e {
            sy = h2 - 0x1e;
        }
        if sx < 1 {
            sx = 1;
        }
        if sy < 1 {
            sy = 2;
        }
        self.scroll_x = sx;
        self.scroll_y = sy;
    }

    /// Port of FUN_00426fdb: scroll by one step; false if already at the edge.
    pub fn scroll(&mut self, code: i32) -> bool {
        let (w, h2) = (self.width(), self.rows());
        let up = |v: &mut MapView| -> bool {
            if v.scroll_y > 2 {
                v.scroll_y = if v.scroll_y == 3 { 1 } else { v.scroll_y - 2 };
                true
            } else {
                false
            }
        };
        let down = |v: &mut MapView| -> bool {
            if v.scroll_y + 0x1e < h2 {
                v.scroll_y = if v.scroll_y + 0x1e < h2 - 1 { v.scroll_y + 2 } else { h2 - 0x1e };
                true
            } else {
                false
            }
        };
        let left = |v: &mut MapView| -> bool {
            let ok = v.scroll_x > 1;
            if ok {
                v.scroll_x -= 1;
            }
            ok
        };
        let right = |v: &mut MapView| -> bool {
            let ok = (v.scroll_x + 0x15) >> 1 < w;
            if ok {
                v.scroll_x += 1;
            }
            ok
        };
        match code {
            scroll::UP => up(self),
            scroll::DOWN => down(self),
            scroll::LEFT => left(self),
            scroll::RIGHT => right(self),
            // Diagonals take both steps and report whether either happened.
            scroll::UP_LEFT => {
                let a = up(self);
                left(self) | a
            }
            scroll::DOWN_LEFT => {
                let a = left(self);
                down(self) | a
            }
            scroll::UP_RIGHT => {
                let a = right(self);
                up(self) | a
            }
            scroll::DOWN_RIGHT => {
                let a = right(self);
                down(self) | a
            }
            _ => false,
        }
    }

    /// Port of FUN_00426267: is the cell inside the visible window? `mode`
    /// 1 also accepts the last row, 2 uses a tighter top margin.
    pub fn cell_in_view(&self, col: i32, row: i32, mode: i32) -> bool {
        let (w, h2, sx, sy) = (self.width(), self.rows(), self.scroll_x, self.scroll_y);
        if row < 1 || !(mode == 1 || row < h2 - 1) {
            return false;
        }
        if row & 1 == 0 {
            if col == 0 || col == w - 1 {
                return false;
            }
        } else if w - 1 <= col {
            return false;
        }
        if sx & 1 == 0 {
            if row & 1 == 0 {
                if col < sx >> 1 {
                    return false;
                }
            } else if col < (sx >> 1) - 1 {
                return false;
            }
            if (sx >> 1) + 10 <= col {
                return false;
            }
        } else {
            if col < sx >> 1 {
                return false;
            }
            if row & 1 == 0 {
                if (sx >> 1) + 10 < col {
                    return false;
                }
            } else if (sx >> 1) + 10 <= col {
                return false;
            }
        }
        let mut r = row;
        let i = row * w + col;
        if let Some(c) = self.def.cells.get(i.max(0) as usize) {
            if c.height != 0 {
                r -= c.height as i32 - 1;
            }
        }
        if mode == 2 {
            !(r < sy + 2 || sy + 0x1e <= r)
        } else {
            !(r < sy - 1 || sy + 0x1e <= r)
        }
    }

    /// Port of FUN_00425879: the cell under screen pixel (x, y), taking
    /// elevation into account. `mesh` (`DAT_00494652`, 遮掩網點) selects the
    /// lenient variant FUN_00425c7c that can pick cells behind scenery.
    pub fn screen_to_cell(&self, x: i32, y: i32, mesh: bool) -> Option<(i32, i32)> {
        let mut px = x;
        let mut py = y;
        if self.scroll_x & 1 != 0 {
            px += 0x20;
        }
        if self.scroll_y & 1 != 0 {
            py += 0x10;
        }
        let a = px + 2 * py - 0x20;
        let i2 = cdiv(a, 64) * -0x40 - 0x20;
        let b = (2 * py - px) + 0x260;
        let i3 = cdiv(b, 64) * 0x40 - 0x260;
        let p3 = cdiv(-i3 - i2, 2);
        let p4 = cdiv(i3 - i2, 4);
        self.pick(px, py, p3, p4, mesh)
    }

    /// FUN_00425984 (strict) / FUN_00425c7c (`mesh`).
    fn pick(&self, px: i32, py: i32, p3: i32, p4: i32, mesh: bool) -> Option<(i32, i32)> {
        let i4 = (px - p3) + 0x20;
        let i2 = py - p4;
        let quad = if (px - p3) - 0x20 + i2 * 2 < 0 {
            if i4 - 2 * i2 >= 0 { 1 } else { 3 }
        } else if i4 - 2 * i2 >= 0 {
            4
        } else {
            2
        };
        let sx = self.scroll_x;
        let col0 = if p3 == 0 && sx & 1 == 0 { (sx >> 1) - 1 } else { cdiv(p3 - 0x20, 64) + (sx >> 1) };
        let mut row0 = cdiv(p4, 16) + self.scroll_y;
        if self.scroll_y & 1 != 0 {
            row0 -= 1;
        }
        let (w, h2) = (self.width(), self.rows());
        let h = |c: i32, r: i32| self.height_at(c, r);
        let mut k = 0x14;
        while k >= 0 {
            let r = row0 + k;
            if r < h2 {
                let hv = h(col0, r);
                if hv == k + 1 || (hv == k && quad == 2) {
                    return Some((col0, r));
                }
                if hv == k + 2 {
                    if quad == 1 {
                        return Some((col0, r));
                    }
                    if !mesh {
                        return None;
                    }
                }
                if !mesh && k + 2 < hv {
                    return None;
                }
            }
            if k != 0 {
                let r = row0 - 1 + k;
                let c = if r & 1 == 0 { col0 } else { col0 - 1 };
                if r < h2 && c >= 0 {
                    let hv = h(c, r);
                    if hv == k {
                        if quad == 3 {
                            return Some((c, r));
                        }
                        if !mesh && i4 < 0x20 && 0xf < i2 {
                            return None;
                        }
                    }
                    if !mesh && k < hv && i4 < 0x20 {
                        return None;
                    }
                }
                let c = if r & 1 == 0 { col0 + 1 } else { col0 };
                if r < h2 && c <= w {
                    let hv = h(c, r);
                    if hv == k {
                        if quad == 4 {
                            return Some((c, r));
                        }
                        if !mesh && 0x1f < i4 && 0xf < i2 {
                            return None;
                        }
                    }
                    if !mesh && k < hv && 0x1f < i4 {
                        return None;
                    }
                }
            }
            k -= 2;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2data::gfx_map::Cell;

    fn flat_map(w: i32, h: i32) -> MapView {
        let n = (w * h * 2) as usize;
        let mut b = Vec::new();
        let hdr: [i16; 14] = [w as i16, h as i16, n as i16, 64, 32, 16, 0, 0, 0, 0, 0, 0, 0, 0];
        for v in hdr {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for _ in 0..n {
            b.push(1);
            b.extend_from_slice(&7i16.to_le_bytes());
            b.extend_from_slice(&0i16.to_le_bytes());
        }
        for _ in 0..3 {
            b.extend_from_slice(&0u16.to_le_bytes());
        }
        let def = MapDef::parse(&b, [0, 0, 0]).unwrap();
        let _ = Cell { height: 1, ground: 7, overlay: 0 };
        let img = Image::new((w * 64) as usize, (h * 32) as usize);
        MapView { def, id: 1, opts: RenderOptions::default(), bg: img.clone(), surface: img, scroll_x: 1, scroll_y: 1, chests: Vec::new() }
    }

    /// The literal four-case port of FUN_00427a10's x.
    fn literal_x(sx: i32, sy: i32, col: i32, row: i32) -> i32 {
        let u = row - sy;
        let mut x = (col - (sx >> 1)) * 64;
        if sx & 1 == 0 || sy & 1 == 0 {
            if sx & 1 == 0 && sy & 1 == 0 {
                if u & 1 != 0 {
                    x += 32;
                }
            } else if sx & 1 == 0 || sy & 1 != 0 {
                if sx & 1 == 0 && sy & 1 != 0 && u & 1 == 0 {
                    x += 32;
                }
            } else if u & 1 == 0 {
                x -= 32;
            }
        } else if u & 1 != 0 {
            x -= 32;
        }
        x
    }

    #[test]
    fn cell_screen_matches_literal_port() {
        let mut v = flat_map(18, 22);
        for sx in 1..6 {
            for sy in 1..6 {
                v.scroll_x = sx;
                v.scroll_y = sy;
                for row in 1..20 {
                    for col in 1..10 {
                        assert_eq!(v.cell_screen(col, row).0, literal_x(sx, sy, col, row));
                    }
                }
            }
        }
    }

    #[test]
    fn picking_inverts_cell_screen_on_flat_ground() {
        let mut v = flat_map(18, 22);
        for (sx, sy) in [(1, 1), (2, 2), (3, 4), (4, 3)] {
            v.scroll_x = sx;
            v.scroll_y = sy;
            for row in 4..24 {
                for col in 2..10 {
                    if v.def.cell_index(col, row).is_none() {
                        continue;
                    }
                    let (x, y) = v.cell_screen(col, row);
                    // Centre of the diamond.
                    let got = v.screen_to_cell(x + 32, y + 16, false);
                    assert_eq!(got, Some((col, row)), "scroll {sx},{sy}");
                }
            }
        }
    }

    #[test]
    fn scroll_limits() {
        let mut v = flat_map(18, 22);
        v.center_on(1, 1, false);
        assert_eq!((v.scroll_x, v.scroll_y), (1, 2));
        v.center_on(17, 43, false);
        assert_eq!((v.scroll_x, v.scroll_y), (18 * 2 - 21, 44 - 30));
        assert!(!v.scroll(scroll::DOWN));
        assert!(!v.scroll(scroll::RIGHT));
        assert!(v.scroll(scroll::UP));
    }
}
