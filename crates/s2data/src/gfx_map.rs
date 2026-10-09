//! Isometric battle maps: definitions, tile banks and the background renderer.
//!
//! Archives involved (all PRG):
//!
//! | entry | content |
//! |-------|---------|
//! | 3 | 4284 ground tiles, raw 64x48 (32px diamond + 16px side) |
//! | 4 | 896 overlay tiles, raw 64x96 (walls, trees, props standing on a cell) |
//! | 5 | 360 object images, raw, various sizes (large props spanning cells) |
//! | 6 | 132 map definitions (`u16 n; (offset, size)[n]`) |
//! | 7 | tile attributes: `u16 n3, n4, n5; u16 ground[n3], overlay[n4], object[n5]` (PRG 9 is an identical copy) |
//! | 8 | 50 backdrops, flagged frames up to 1920x1280, drawn behind/over the map |
//!
//! Map definition (`FUN_00429336`, `FUN_004297f5`):
//!
//! ```text
//! i16 hdr[14]: width W (cells in an even row), half_rows H (rows = 2H),
//!              cell_count (= W*2H), 64, 32, 16 (tile metrics),
//!              object_count, flag_count,
//!              backdrop id/x/y (PRG 8, drawn opaque first),
//!              foreground id/x/y (PRG 8, drawn transparent last)
//! (u8 height, i16 ground, i16 overlay)[cell_count]       row-major, stride W
//! (i16 col, row, width, height, image)[object_count]     PRG 5 objects
//! (i16 col, row, u16 flags)[flag_count]                  sorted by row
//! 3x { u16 local_count; u16 usage[n_k]; (u32, u32)[local_count] }
//! ```
//! The usage/local tables only tell the game which tiles to page in.
//!
//! Geometry: cell (col, row) has its diamond's top-left at
//! `(col*64 + 32*(row&1), row*16)`; odd rows hold W-1 cells and the first and
//! last cell of even rows are off-map (`FUN_00425562`). A cell of height `h`
//! is raised by `(h-1)*16` pixels. The map surface is W*64 x H*32.
//!
//! Overlay attribute high byte = category: 5, 6 and 0xb are optional
//! decorations (toggled by settings bytes `DAT_00494509/08/0b`), 0xc are
//! doors/dynamic tiles drawn by stage logic (`FUN_0042aba8`/`FUN_0042ae85`),
//! 0x23 marks up to 20 special cells. Ground attribute low byte = terrain
//! type (for the TABLES side). Object attribute = number of rows at the
//! object's base that stay in front of overlays (`FUN_00429e2f`).
//!
//! Overlay and object pixels 0x20 / 0x21 are shadows: they darken what is
//! underneath through tint tables 6 / 4 (`FUN_0046fe5f`).

use crate::archive::{offset_table, Span};
use crate::sprite::{FrameKind, Image, ImageBank};
use crate::{format_err, le, Archives, Kind, Result};

pub const TILE_W: i32 = 64;
pub const ROW_H: i32 = 16;
pub const SHADOW_DARK: u8 = 0x20;
pub const SHADOW_LIGHT: u8 = 0x21;
pub const CAT_DOOR: u8 = 0x0c;
pub const CAT_MARKER: u8 = 0x23;
pub const FLAG_OVERLAY_FRONT: u16 = 0x4000;

#[derive(Debug, Clone)]
pub struct TileAttrs {
    pub ground: Vec<u16>,
    pub overlay: Vec<u16>,
    pub object: Vec<u16>,
}

impl TileAttrs {
    pub fn parse(b: &[u8]) -> Result<TileAttrs> {
        let n = [le::u16(b, 0)? as usize, le::u16(b, 2)? as usize, le::u16(b, 4)? as usize];
        let mut p = 6;
        let mut read = |n: usize| -> Result<Vec<u16>> {
            let v = (0..n).map(|i| le::u16(b, p + 2 * i)).collect::<Result<Vec<_>>>()?;
            p += 2 * n;
            Ok(v)
        };
        Ok(TileAttrs { ground: read(n[0])?, overlay: read(n[1])?, object: read(n[2])? })
    }

    fn get(v: &[u16], id: i32) -> u16 {
        if id >= 1 { v.get(id as usize - 1).copied().unwrap_or(0) } else { 0 }
    }

    pub fn overlay_category(&self, id: i32) -> u8 {
        (Self::get(&self.overlay, id) >> 8) as u8
    }

    pub fn ground_terrain(&self, id: i32) -> u8 {
        Self::get(&self.ground, id) as u8
    }

    pub fn object_front_rows(&self, id: i32) -> i32 {
        Self::get(&self.object, id) as i32
    }
}

/// All tile/object/backdrop images plus their attributes.
pub struct MapTiles {
    pub ground: ImageBank,
    pub overlay: ImageBank,
    pub object: ImageBank,
    pub backdrop: ImageBank,
    pub attrs: TileAttrs,
}

impl MapTiles {
    pub fn load(arcs: &Archives) -> Result<MapTiles> {
        Ok(MapTiles {
            ground: ImageBank::parse(arcs.read(Kind::Prg, 3)?, FrameKind::Raw)?,
            overlay: ImageBank::parse(arcs.read(Kind::Prg, 4)?, FrameKind::Raw)?,
            object: ImageBank::parse(arcs.read(Kind::Prg, 5)?, FrameKind::Raw)?,
            backdrop: ImageBank::parse(arcs.read(Kind::Prg, 8)?, FrameKind::Flagged)?,
            attrs: TileAttrs::parse(&arcs.read(Kind::Prg, 7)?)?,
        })
    }

    fn bank_get(bank: &ImageBank, id: i32) -> Option<Image> {
        if id < 1 || id as usize > bank.len() {
            return None;
        }
        bank.image(id as usize - 1).ok()
    }

    pub fn ground_tile(&self, id: i32) -> Option<Image> {
        Self::bank_get(&self.ground, id)
    }

    pub fn overlay_tile(&self, id: i32) -> Option<Image> {
        Self::bank_get(&self.overlay, id)
    }

    pub fn object_image(&self, id: i32) -> Option<Image> {
        Self::bank_get(&self.object, id)
    }

    pub fn backdrop_image(&self, id: i32) -> Option<Image> {
        Self::bank_get(&self.backdrop, id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub height: u8,
    pub ground: i16,
    pub overlay: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapObject {
    pub col: i16,
    pub row: i16,
    pub width: i16,
    pub height: i16,
    pub image: i16,
    /// Map-pixel rectangle computed at load (`FUN_00429336`).
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellFlag {
    pub col: i16,
    pub row: i16,
    pub flags: u16,
}

#[derive(Debug, Clone)]
pub struct BattleMap {
    pub header: [i16; 14],
    pub width: i32,
    pub half_rows: i32,
    pub cells: Vec<Cell>,
    pub objects: Vec<MapObject>,
    pub flags: Vec<CellFlag>,
    /// Per tile bank (ground, overlay, object): global id -> local slot (0 = unused).
    pub usage: [Vec<u16>; 3],
    row_first_flag: Vec<i16>,
}

/// Divide like C: truncate toward zero.
fn cdiv(a: i32, b: i32) -> i32 {
    a / b
}

impl BattleMap {
    /// Load map `id` (1-based) from PRG 6. `bank_counts` are the PRG 3/4/5 sizes.
    pub fn load(arcs: &Archives, id: usize, bank_counts: [usize; 3]) -> Result<BattleMap> {
        let all = arcs.read(Kind::Prg, 6)?;
        let spans: Vec<Span> = offset_table(&all)?;
        let s = id
            .checked_sub(1)
            .and_then(|i| spans.get(i))
            .ok_or_else(|| format_err("battle map", format!("map {id} out of range 1..={}", spans.len())))?;
        let b = all
            .get(s.offset as usize..(s.offset + s.size) as usize)
            .ok_or_else(|| format_err("battle map", "entry beyond PRG 6"))?;
        BattleMap::parse(b, bank_counts)
    }

    /// Number of maps in PRG 6.
    pub fn count(arcs: &Archives) -> Result<usize> {
        Ok(le::u16(&arcs.read(Kind::Prg, 6)?, 0)? as usize)
    }

    pub fn parse(b: &[u8], bank_counts: [usize; 3]) -> Result<BattleMap> {
        let mut header = [0i16; 14];
        for (i, h) in header.iter_mut().enumerate() {
            *h = le::i16(b, 2 * i)?;
        }
        let (width, half_rows) = (header[0] as i32, header[1] as i32);
        let ncell = header[2].max(0) as usize;
        if width <= 0 || half_rows <= 0 || ncell != (width * half_rows * 2) as usize {
            return Err(format_err("battle map", format!("inconsistent header {header:?}")));
        }
        let mut p = 0x1c;
        let mut cells = Vec::with_capacity(ncell);
        for i in 0..ncell {
            let q = p + 5 * i;
            let height = *b.get(q).ok_or_else(|| format_err("battle map", "truncated cells"))?;
            cells.push(Cell { height, ground: le::i16(b, q + 1)?, overlay: le::i16(b, q + 3)? });
        }
        p += 5 * ncell;
        let mut map = BattleMap {
            header,
            width,
            half_rows,
            cells,
            objects: Vec::new(),
            flags: Vec::new(),
            usage: [Vec::new(), Vec::new(), Vec::new()],
            row_first_flag: vec![-1; (half_rows * 2) as usize],
        };
        let mut raw_objs = Vec::new();
        for i in 0..header[6].max(0) as usize {
            let q = p + 10 * i;
            raw_objs.push([le::i16(b, q)?, le::i16(b, q + 2)?, le::i16(b, q + 4)?, le::i16(b, q + 6)?, le::i16(b, q + 8)?]);
        }
        p += 10 * raw_objs.len();
        for i in 0..header[7].max(0) as usize {
            let q = p + 6 * i;
            map.flags.push(CellFlag { col: le::i16(b, q)?, row: le::i16(b, q + 2)?, flags: le::u16(b, q + 4)? });
        }
        p += 6 * map.flags.len();
        for (k, &n) in bank_counts.iter().enumerate() {
            let local = le::u16(b, p)? as usize;
            map.usage[k] = (0..n).map(|i| le::u16(b, p + 2 + 2 * i)).collect::<Result<_>>()?;
            p += 2 + 2 * n + 8 * local;
        }
        if p != b.len() {
            return Err(format_err("battle map", format!("parsed {p} of {} bytes", b.len())));
        }
        // FUN_00428a18: first flag record per row.
        for (i, f) in map.flags.iter().enumerate() {
            if let Some(slot) = map.row_first_flag.get_mut(f.row as usize) {
                if *slot == -1 {
                    *slot = i as i16;
                }
            }
        }
        for [col, row, w, h, image] in raw_objs {
            let (c, r) = (col as i32, row as i32);
            let ht = map.height_at(c + cdiv(w as i32, 128), r - 2 + cdiv(h as i32, 16)) as i32;
            let x1 = c * TILE_W + if r & 1 != 0 { 32 } else { 0 };
            let y1 = r * ROW_H - if ht > 1 { (ht - 1) * ROW_H } else { 0 };
            map.objects.push(MapObject {
                col,
                row,
                width: w,
                height: h,
                image,
                x1,
                y1,
                x2: x1 + w as i32,
                y2: y1 + h as i32,
            });
        }
        Ok(map)
    }

    pub fn rows(&self) -> i32 {
        self.half_rows * 2
    }

    /// Size of the pre-rendered map surface.
    pub fn pixel_size(&self) -> (usize, usize) {
        ((self.width * 64) as usize, (self.half_rows * 32) as usize)
    }

    pub fn backdrop(&self) -> Option<(i32, i32, i32)> {
        (self.header[8] > 0).then(|| (self.header[8] as i32, self.header[9] as i32, self.header[10] as i32))
    }

    pub fn foreground(&self) -> Option<(i32, i32, i32)> {
        (self.header[11] > 0).then(|| (self.header[11] as i32, self.header[12] as i32, self.header[13] as i32))
    }

    /// `FUN_00425562`: 0-based index of a playable cell.
    pub fn cell_index(&self, col: i32, row: i32) -> Option<usize> {
        if row < 1 || row >= self.rows() || col < 0 {
            return None;
        }
        if row & 1 == 0 {
            if col == 0 || col > self.width - 1 {
                return None;
            }
        } else if col >= self.width - 1 {
            return None;
        }
        Some((row * self.width + col) as usize)
    }

    pub fn cell(&self, col: i32, row: i32) -> Option<&Cell> {
        self.cell_index(col, row).and_then(|i| self.cells.get(i))
    }

    /// `FUN_004255da`: terrain height, 0 off-map. (The game does not bound
    /// `col` on odd rows; neither do we, beyond staying inside the array.)
    pub fn height_at(&self, col: i32, row: i32) -> u8 {
        if row < 1 || row >= self.rows() {
            return 0;
        }
        if row & 1 == 0 && (col == 0 || col == self.width - 1) {
            return 0;
        }
        let i = row * self.width + col;
        if i < 0 {
            return 0;
        }
        self.cells.get(i as usize).map_or(0, |c| c.height)
    }

    /// `FUN_00425f1a`/`FUN_00425fbe`: flag record on a cell.
    pub fn flags_at(&self, col: i32, row: i32) -> Option<u16> {
        let first = *self.row_first_flag.get(row as usize)?;
        if first < 0 {
            return None;
        }
        self.flags[first as usize..]
            .iter()
            .take_while(|f| f.row as i32 <= row)
            .find(|f| f.col as i32 == col)
            .map(|f| f.flags)
    }

    /// `FUN_00427a10` without scrolling: top-left of the cell diamond in map pixels.
    pub fn cell_origin(&self, col: i32, row: i32) -> (i32, i32) {
        let x = col * TILE_W + if row & 1 != 0 { 32 } else { 0 };
        let h = self.height_at(col, row) as i32;
        let y = row * ROW_H - if h >= 1 { (h - 1) * ROW_H } else { 0 };
        (x, y)
    }

    /// `FUN_0042665a` without scrolling: where a `w`x`h` unit frame is drawn.
    pub fn unit_position(&self, col: i32, row: i32, w: usize, h: usize, dx: i32, lift: i32) -> (i32, i32) {
        let ht = self.height_at(col, row) as i32;
        let x = col * TILE_W + if row & 1 != 0 { 32 } else { 0 } - cdiv(w as i32 - 64, 2) + dx;
        let y = row * ROW_H - (ht - 1 + lift) * ROW_H - (h as i32 - 32);
        (x, y)
    }
}

/// Settings and state that change what gets drawn.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// Optional decoration categories 5, 6, 0xb (settings bytes; on by default).
    pub show_cat5: bool,
    pub show_cat6: bool,
    pub show_cat11: bool,
    /// Number of door tiles in the current stage (`DAT_0049478c`).
    pub door_count: i32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { show_cat5: true, show_cat6: true, show_cat11: true, door_count: 0 }
    }
}

impl RenderOptions {
    fn category_visible(&self, cat: u8) -> bool {
        match cat {
            5 => self.show_cat5,
            6 => self.show_cat6,
            0x0b => self.show_cat11,
            _ => true,
        }
    }
}

/// Shadow tables used when drawing overlays and objects.
#[derive(Debug, Clone)]
pub struct Shading {
    /// Applied under pixel 0x20 (tint table 6).
    pub dark: [u8; 256],
    /// Applied under pixel 0x21 (tint table 4).
    pub light: [u8; 256],
}

impl Shading {
    pub fn from_tints(tints: &[[u8; 256]]) -> Shading {
        Shading { dark: tints[6], light: tints[4] }
    }

    /// No shading (shadow pixels drawn as plain colours).
    pub fn none() -> Shading {
        let id: [u8; 256] = std::array::from_fn(|i| i as u8);
        Shading { dark: id, light: id }
    }
}

/// `FUN_0046fe5f`: transparent blit where 0x20/0x21 darken the destination.
pub fn blit_shadowed(dst: &mut Image, src: &Image, x: i32, y: i32, sh: &Shading) {
    for sy in 0..src.height as i32 {
        let dy = y + sy;
        if dy < 0 || dy >= dst.height as i32 {
            continue;
        }
        for sx in 0..src.width as i32 {
            let dx = x + sx;
            if dx < 0 || dx >= dst.width as i32 {
                continue;
            }
            let s = src.pixels[(sy as usize) * src.width + sx as usize];
            let d = &mut dst.pixels[dy as usize * dst.width + dx as usize];
            match s {
                0 => {}
                SHADOW_DARK => *d = sh.dark[*d as usize],
                SHADOW_LIGHT => *d = sh.light[*d as usize],
                _ => *d = s,
            }
        }
    }
}

/// How sprite pixels hidden behind scenery are removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutMode {
    /// Hidden pixels are cleared.
    Solid,
    /// Hidden pixels are cleared in a checkerboard (see-through), mode 1.
    Dither,
}

/// `FUN_0041fba0` (`skip_shadow = false`) / `FUN_0041fdd0` (`true`): clear
/// pixels of `dst` covered by non-zero pixels of `mask` placed at (x, y)
/// relative to `dst`.
pub fn cut(dst: &mut Image, mask: &Image, x: i32, y: i32, mode: CutMode, skip_shadow: bool) {
    let (dw, dh, mw, mh) = (dst.width as i32, dst.height as i32, mask.width as i32, mask.height as i32);
    let (mut d0, mut m0) = (0i32, 0i32);
    let rows;
    if y < 0 {
        if y + mh < 0 {
            return;
        }
        m0 += mw * -y;
        rows = (mh + y).min(dh);
    } else if y < 1 {
        rows = mh.min(dh);
    } else {
        if dh < y {
            return;
        }
        d0 += dw * y;
        rows = (dh - y).min(mh);
    }
    let cols;
    if x < 0 {
        if x + mw < 0 {
            return;
        }
        m0 += -x;
        cols = (mw + x).min(dw);
    } else if x < 1 {
        cols = mw.min(dw);
    } else {
        if dw < x {
            return;
        }
        d0 += x;
        cols = (dw - x).min(mw);
    }
    if rows <= 0 || cols <= 0 {
        return;
    }
    let mut phase = false;
    for r in 0..rows {
        let mut on = phase;
        let next = !phase;
        for c in 0..cols {
            let m = mask.pixels[(m0 + r * mw + c) as usize];
            let solid = m != 0 && !(skip_shadow && (m == SHADOW_DARK || m == SHADOW_LIGHT));
            let clear = match mode {
                CutMode::Solid => solid,
                CutMode::Dither => {
                    let v = solid && on;
                    on = !on;
                    v
                }
            };
            if clear {
                dst.pixels[(d0 + r * dw + c) as usize] = 0;
            }
        }
        phase = next;
    }
}

/// Renders map backgrounds and occludes sprites, porting `FUN_00424d81`,
/// `FUN_00425075`, `FUN_00429ba2`, `FUN_00429e2f` and the `FUN_00426417` family.
pub struct MapRenderer<'a> {
    pub map: &'a BattleMap,
    pub tiles: &'a MapTiles,
    pub shading: &'a Shading,
    pub opts: RenderOptions,
    /// `map+0x88`: 0 while the background is being built, 1 afterwards.
    building: bool,
}

/// Parameters of a sprite occlusion pass (`FUN_00426417`).
#[derive(Clone, Copy)]
pub struct Occlusion<'f> {
    pub dx: i32,
    pub mode: CutMode,
    /// Also cut by the overlay on the sprite's own cell (param 4).
    pub own_overlay: bool,
    /// When set, overlays on cells for which this returns true do not cut
    /// (param 5 together with `FUN_00402213`).
    pub skip_overlay: Option<&'f dyn Fn(i32, i32) -> bool>,
}

impl Default for Occlusion<'_> {
    fn default() -> Self {
        Occlusion { dx: 0, mode: CutMode::Solid, own_overlay: false, skip_overlay: None }
    }
}

impl<'a> MapRenderer<'a> {
    pub fn new(map: &'a BattleMap, tiles: &'a MapTiles, shading: &'a Shading, opts: RenderOptions) -> Self {
        MapRenderer { map, tiles, shading, opts, building: false }
    }

    fn overlay_drawable(&self, id: i32) -> bool {
        let cat = self.tiles.attrs.overlay_category(id);
        cat != CAT_DOOR && self.opts.category_visible(cat)
    }

    /// The pre-rendered background exactly as `FUN_00424d81` builds it
    /// (doors are left to the stage logic).
    pub fn render_background(&mut self) -> Image {
        let (w, h) = self.map.pixel_size();
        let mut surf = Image::new(w, h);
        self.building = true;
        if let Some((id, x, y)) = self.map.backdrop() {
            if let Some(img) = self.tiles.backdrop_image(id) {
                surf.blit_opaque(&img, x as isize, y as isize);
            }
        }
        let rows = self.map.rows();
        let wcells = self.map.width;
        for r in 0..rows {
            for o in self.map.objects.iter().filter(|o| r * 16 == o.row as i32 * 16 + o.height as i32) {
                self.draw_object(&mut surf, o);
            }
            let ncols = if r & 1 == 0 { wcells } else { wcells - 1 };
            for c in 0..ncols {
                let x = c * TILE_W + if r & 1 != 0 { 32 } else { 0 };
                self.draw_cell(&mut surf, x, r * ROW_H, (r * wcells + c) as usize);
            }
        }
        for o in self.map.objects.iter().filter(|o| (rows - 1) * 16 < o.row as i32 * 16 + o.height as i32) {
            self.draw_object(&mut surf, o);
        }
        if let Some((id, x, y)) = self.map.foreground() {
            if let Some(img) = self.tiles.backdrop_image(id) {
                surf.blit(&img, x as isize, y as isize);
            }
        }
        self.building = false;
        surf
    }

    /// `FUN_00429ba2`.
    fn draw_cell(&self, surf: &mut Image, x: i32, y: i32, idx: usize) {
        let Some(cell) = self.map.cells.get(idx) else { return };
        if cell.height == 0 {
            return;
        }
        let Some(tile) = self.tiles.ground_tile(cell.ground as i32) else { return };
        if cell.ground < 6 {
            surf.blit(&tile, x as isize, y as isize);
            return;
        }
        for k in 0..cell.height as i32 {
            surf.blit(&tile, x as isize, (y - k * ROW_H) as isize);
        }
        let ov = cell.overlay as i32;
        if ov != 0 && self.overlay_drawable(ov) {
            if let Some(img) = self.tiles.overlay_tile(ov) {
                blit_shadowed(surf, &img, x, y - (cell.height as i32 - 1) * ROW_H - 64, self.shading);
            }
        }
    }

    fn draw_object(&self, surf: &mut Image, o: &MapObject) {
        if let Some(img) = self.tiles.object_image(o.image as i32) {
            blit_shadowed(surf, &img, o.x1, o.y1, self.shading);
        }
        self.redraw_front_overlays(surf, o);
    }

    /// `FUN_00429e2f`: overlays that must stay in front of a just-drawn object.
    fn redraw_front_overlays(&self, surf: &mut Image, o: &MapObject) {
        let front = self.tiles.attrs.object_front_rows(o.image as i32);
        let c0 = o.col as i32;
        let ncols = cdiv(o.width as i32, 64) + 2;
        let nrows = cdiv(o.height as i32, 16) + 1;
        let rows = self.map.rows();
        let start_col = |k: i32, r: i32| if k & 1 == 0 || r & 1 == 0 { c0 - 1 } else { c0 };
        let visit = |surf: &mut Image, k: i32, r: i32, need_flag: bool| {
            for c in (start_col(k, r)..).take(ncols.max(0) as usize) {
                if c >= self.map.width {
                    break;
                }
                if c >= 0 {
                    self.redraw_overlay_cell(surf, c, r, need_flag);
                }
            }
        };
        if front == 0 {
            let mut r = o.row as i32 - 1;
            let mut k = 0;
            while k < nrows && r < rows {
                if r >= 0 {
                    visit(surf, k, r, true);
                }
                k += 1;
                r += 1;
            }
        } else {
            let mut r = o.row as i32 - 1 + nrows - front;
            let mut k = front;
            while k > 0 && r < rows {
                visit(surf, k, r, false);
                k -= 1;
                r += 1;
            }
        }
    }

    fn redraw_overlay_cell(&self, surf: &mut Image, c: i32, r: i32, need_flag: bool) {
        let Some(cell) = self.map.cell(c, r) else { return };
        let ov = cell.overlay as i32;
        if ov == 0 || !self.overlay_drawable(ov) {
            return;
        }
        if need_flag && self.map.flags_at(c, r).is_none_or(|f| f & FLAG_OVERLAY_FRONT == 0) {
            return;
        }
        let Some(mut img) = self.tiles.overlay_tile(ov) else { return };
        self.occlude(&mut img, c, r, Occlusion::default());
        let x = c * TILE_W + if r & 1 != 0 { 32 } else { 0 };
        let y = (r - cell.height as i32 + 1) * ROW_H - 64;
        blit_shadowed(surf, &img, x, y, self.shading);
    }

    /// `FUN_00426417`: clear the parts of `img` (a sprite standing on cell
    /// (col, row)) that scenery in front of it hides.
    pub fn occlude(&self, img: &mut Image, col: i32, row: i32, occ: Occlusion<'_>) {
        if occ.own_overlay {
            if let Some(ov) = self.map.cell(col, row).and_then(|c| self.tiles.overlay_tile(c.overlay as i32)) {
                cut(img, &ov, occ.dx, img.height as i32 - 1 - 0x60, occ.mode, true);
            }
        }
        let h0 = self.map.height_at(col, row) as i32;
        if h0 == 0 {
            return;
        }
        for k in 0..12 {
            self.occlude_ring(img, col, row, h0, k, occ);
        }
        if self.map.cell(col, row).is_some_and(|c| c.ground > 4) {
            self.occlude_objects(img, col, row, h0, occ);
        }
    }

    /// `FUN_0042a30c`: cells `k+1` steps in front.
    fn occlude_ring(&self, img: &mut Image, col: i32, row: i32, h0: i32, k: i32, occ: Occlusion<'_>) {
        let half = img.width as i32 >> 1;
        let mut ox = col * TILE_W + 32 - half + occ.dx;
        if row & 1 != 0 {
            ox += 32;
        }
        let oy = row * ROW_H + 32 - img.height as i32;
        let kk = k + 1;
        let r2 = row + 2 * kk;
        let r1 = row - 1 + 2 * kk;
        let even1 = r1 & 1 == 0;
        let mut a = |c: i32, r: i32| self.occlude_by_cell(img, c, r, ox, oy, h0, kk, occ);
        a(col, r2);
        a(if even1 { col } else { col - 1 }, r1);
        a(if even1 { col + 1 } else { col }, r1);
        if (half > 32 || occ.dx != 0) && kk != 12 {
            a(col - 1, r2);
            a(col + 1, r2);
            if occ.dx < 0 {
                a(if even1 { col - 1 } else { col - 2 }, r1);
            } else if occ.dx > 0 {
                a(if even1 { col + 2 } else { col + 1 }, r1);
            }
        }
    }

    /// `FUN_0042a5af`.
    #[allow(clippy::too_many_arguments)]
    fn occlude_by_cell(&self, img: &mut Image, c: i32, r: i32, ox: i32, oy: i32, h0: i32, kk: i32, occ: Occlusion<'_>) {
        let Some(cell) = self.map.cell(c, r) else { return };
        if cell.ground <= 4 || cell.height == 0 {
            return;
        }
        let mut lx = c * TILE_W - ox;
        if r & 1 != 0 {
            lx += 32;
        }
        let ly = r * ROW_H - oy;
        let hh = cell.height as i32;
        let e = (hh - h0) - 2 * kk;
        if e + 1 >= 0 {
            if let Some(tile) = self.tiles.ground_tile(cell.ground as i32) {
                if e == -1 {
                    cut(img, &tile, lx, ly - (hh - h0) * ROW_H, occ.mode, false);
                } else {
                    for j in -1..e + 1 {
                        cut(img, &tile, lx, ly - (j + 2 * kk) * ROW_H, occ.mode, false);
                    }
                }
            }
        }
        let ov = cell.overlay as i32;
        // The game requires e + 1 > -5 for the overlay cut.
        if occ.skip_overlay.is_some_and(|f| f(c, r)) || ov <= 0 || e < -5 {
            return;
        }
        let cat = self.tiles.attrs.overlay_category(ov);
        let door_ok = !self.building || self.opts.door_count < 1 || cat != CAT_DOOR;
        if door_ok && self.opts.category_visible(cat) {
            if let Some(t) = self.tiles.overlay_tile(ov) {
                cut(img, &t, lx, ly - (e + 2 * kk) * ROW_H - 64, occ.mode, true);
            }
        }
    }

    /// `FUN_0042a7da`: large objects in front.
    fn occlude_objects(&self, img: &mut Image, col: i32, row: i32, h0: i32, occ: Occlusion<'_>) {
        if self.map.objects.is_empty() || self.map.flags_at(col, row).is_some_and(|f| f & FLAG_OVERLAY_FRONT != 0) {
            return;
        }
        let (w, h) = (img.width as i32, img.height as i32);
        let mut ox = col * TILE_W + 32 - (w >> 1) + occ.dx;
        if row & 1 != 0 {
            ox += 32;
        }
        let oy = row * ROW_H + 32 - h - (h0 - 1) * ROW_H;
        for o in &self.map.objects {
            let front = self.tiles.attrs.object_front_rows(o.image as i32);
            let in_front = front == 0 || row < o.row as i32 + cdiv(o.height as i32, 16) - front;
            if in_front && oy < o.y2 && o.y1 < oy + h && ox < o.x2 && o.x1 < ox + w {
                if let Some(oimg) = self.tiles.object_image(o.image as i32) {
                    cut(img, &oimg, o.x1 - ox, o.y1 - oy, occ.mode, true);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: usize, h: usize, v: u8) -> Image {
        Image { width: w, height: h, pixels: vec![v; w * h] }
    }

    #[test]
    fn cut_solid_and_dither() {
        let mut d = img(4, 2, 9);
        cut(&mut d, &img(2, 2, 5), 1, 0, CutMode::Solid, false);
        assert_eq!(d.pixels, vec![9, 0, 0, 9, 9, 0, 0, 9]);
        let mut d = img(4, 2, 9);
        cut(&mut d, &img(4, 2, 5), 0, 0, CutMode::Dither, false);
        assert_eq!(d.pixels, vec![9, 0, 9, 0, 0, 9, 0, 9]);
        let mut d = img(2, 1, 9);
        cut(&mut d, &img(2, 1, SHADOW_DARK), 0, 0, CutMode::Solid, true);
        assert_eq!(d.pixels, vec![9, 9]);
    }

    #[test]
    fn shadows_darken_destination() {
        let mut sh = Shading::none();
        sh.dark[7] = 1;
        let mut d = img(2, 1, 7);
        let s = Image { width: 2, height: 1, pixels: vec![SHADOW_DARK, 3] };
        blit_shadowed(&mut d, &s, 0, 0, &sh);
        assert_eq!(d.pixels, vec![1, 3]);
    }

    #[test]
    fn cell_index_rules() {
        let map = BattleMap {
            header: [0; 14],
            width: 4,
            half_rows: 2,
            cells: vec![Cell { height: 1, ground: 7, overlay: 0 }; 16],
            objects: vec![],
            flags: vec![],
            usage: [vec![], vec![], vec![]],
            row_first_flag: vec![-1; 4],
        };
        assert_eq!(map.cell_index(1, 0), None);
        assert_eq!(map.cell_index(0, 2), None);
        assert_eq!(map.cell_index(1, 2), Some(9));
        assert_eq!(map.cell_index(2, 1), Some(6));
        assert_eq!(map.cell_index(3, 1), None);
    }
}
