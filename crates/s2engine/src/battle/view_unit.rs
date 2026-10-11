//! Unit sprites on the battle map, ported from `FUN_0043ff88` and helpers:
//! sprite banks and action sequences, animation stepping, the per-frame
//! preparation of a unit's image (team/state tints, ground shadow, hit
//! shake, invisibility) and the occlusion passes against scenery and other
//! units.
//!
//! The drawing code only reads a handful of unit fields; [`UnitLook`] is
//! that projection (field names follow `battle::unit::Unit`). The decoded
//! frame (`unit+0xad`) lives in [`UnitGfx`], owned by the view.

use s2data::gfx_map::{cut, CutMode, MapTiles, Occlusion, Shading};
use s2data::gfx_unit::{UnitSprites, ACTION_BLOCK, MAX_ACTIONS, SUMMON_BASE};
use s2data::sprite::Image;

use super::map::MapView;

/// `Unit::flags` bits the view reads.
pub const F_DEPLOYED: u32 = 0x8000_0000;
pub const F_DEAD: u32 = 0x2000_0000;
pub const F_SUMMON: u32 = 0x1000_0000;
pub const F_SUMMONING: u32 = 0x0800_0000;
/// `Unit::render_flags`: being struck (hit shake), and the shake phase.
pub const R_HIT: u32 = 0x8000_0000;
pub const R_HIT_PHASE: u32 = 0x4000_0000;
/// `Unit::act` values.
pub const ACT_DONE: u8 = 2;
pub const ACT_SELECTED: u8 = 1;
pub const ACT_WALKING: u8 = 3;
pub const ACT_COMBAT: u8 = 5;
/// `buffs` 石化 and `status` 凍結.
pub const B_PETRIFIED: u32 = 0x200;
pub const S_FROZEN: u32 = 0x2000;

/// The unit fields the drawing code reads or animates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitLook {
    /// `+0x23/+0x27` cell.
    pub x: i32,
    pub y: i32,
    /// `+0x04` class id; selects the sprite type.
    pub kind: i16,
    /// `+0x2b` bits 0-1 direction (0 down-left, 1 down-right, 2 up-right,
    /// 3 up-left), bits 2.. animation set; the whole byte is the action index.
    pub facing: u8,
    /// `+0x2c` byte offset of the current frame in the action block.
    pub frame: u8,
    /// `+0x2d`.
    pub act: u8,
    /// `+0x30`.
    pub flags: u32,
    /// `+0x38` / `+0x3c`.
    pub buffs: u32,
    pub status: u32,
    /// `+0x40`.
    pub render_flags: u32,
    /// `+0xac` horizontal draw offset of the current frame.
    pub remap: i8,
    /// `+0xa8` centisecond tick of the last animation step.
    pub anim_timer: i32,
    /// Army side (`army+0xda`): 0 player, 1 enemy.
    pub side: i32,
    /// `FUN_0043aad7(army, unit)`: hidden from the opponent.
    pub invisible: bool,
    /// `FUN_0043aa26(unit)` movement type (3/4 are lifted while selected).
    pub move_type: i32,
    /// Class record +0x12 (9 = does not leave ripples in water).
    pub class_move: i8,
}

impl UnitLook {
    pub fn on_map(&self) -> bool {
        self.flags & F_DEAD == 0 && self.flags & F_DEPLOYED != 0
    }

    /// Units drawn into the map surface rather than on top of the page.
    pub fn is_baked(&self) -> bool {
        self.on_map() && !matches!(self.act, ACT_SELECTED | ACT_WALKING | ACT_COMBAT)
    }

    pub fn is_summon(&self) -> bool {
        self.flags & F_SUMMON != 0
    }
}

/// The view-owned image state of one unit.
#[derive(Debug, Clone, Default)]
pub struct UnitGfx {
    /// `+0xad`: the decoded current frame (`None` until the sprite is loaded).
    pub sprite: Option<Image>,
    /// (kind, summon, facing, frame) the view last left the unit in; the
    /// battle code may change them directly (FUN_00441a24's logic part).
    pub synced: Option<(i16, bool, u8, u8)>,
    /// `+0xb1`: the save-under, what lies behind the frame on the map
    /// surface (`FUN_00442022` / `FUN_00441ef1`); `None` until first
    /// captured, and again once the frame changed size (the original
    /// reallocates both together, FUN_0043a34f).
    pub under: Option<Image>,
    /// Cell the save-under was captured on.
    pub under_at: Option<(i32, i32)>,
    /// What the unit last left in the map surface (`None`: erased, or never
    /// drawn into it).
    pub drawn: Option<Drawn>,
    /// Erased by the battle code itself (`view::erase_unit`): it stays out of
    /// the surface until the battle code puts it back, or it is done.
    pub held: bool,
}

/// A unit's image as last drawn into the map surface.
#[derive(Debug, Clone, Copy)]
pub struct Drawn {
    pub look: UnitLook,
    /// Frame size.
    pub w: usize,
    pub h: usize,
}

/// The fields of a unit that pick its decoded frame.
pub fn frame_key(l: &UnitLook) -> (i16, bool, u8, u8) {
    (l.kind, l.is_summon(), l.facing, l.frame)
}

/// One type's 200-byte action block and its string offsets (`FUN_0043e83e`).
#[derive(Debug, Clone)]
struct ActionBlock {
    bytes: [u8; ACTION_BLOCK],
    offsets: [u8; MAX_ACTIONS],
}

impl ActionBlock {
    fn from_actions(actions: &[Vec<u8>]) -> ActionBlock {
        let mut bytes = [0u8; ACTION_BLOCK];
        let mut offsets = [0u8; MAX_ACTIONS];
        let mut p = 0usize;
        for (k, a) in actions.iter().enumerate().take(MAX_ACTIONS) {
            if p + a.len() >= ACTION_BLOCK {
                break;
            }
            offsets[k] = p as u8;
            bytes[p..p + a.len()].copy_from_slice(a);
            p += a.len() + 1;
        }
        ActionBlock { bytes, offsets }
    }
}

/// Unit sprite banks (PRG 1/2 for classes, CEL 4/5 for summons).
pub struct SpriteBanks {
    pub normal: UnitSprites,
    pub summon: UnitSprites,
    normal_blocks: Vec<ActionBlock>,
    summon_blocks: Vec<ActionBlock>,
}

impl SpriteBanks {
    pub fn new(normal: UnitSprites, summon: UnitSprites) -> SpriteBanks {
        let blocks = |b: &UnitSprites| b.types.iter().map(|t| ActionBlock::from_actions(&t.actions)).collect();
        SpriteBanks { normal_blocks: blocks(&normal), summon_blocks: blocks(&summon), normal, summon }
    }

    /// Sprite type of a unit: the class id (falling back to type 0x2c like
    /// `FUN_0043f23c`), or `kind - 0x70` in the summon bank.
    fn bank(&self, look: &UnitLook) -> (&UnitSprites, &[ActionBlock], usize) {
        if look.is_summon() {
            let t = (look.kind as i32 - SUMMON_BASE as i32 + 1).clamp(1, self.summon.types.len() as i32) as usize;
            (&self.summon, &self.summon_blocks, t)
        } else {
            let k = look.kind as i32 - 1;
            let t = if k < 0 || k as usize >= self.normal.types.len() { 0x2c } else { k as usize + 1 };
            (&self.normal, &self.normal_blocks, t)
        }
    }

    /// Port of FUN_0043a1fe: start action `action` (= the facing byte).
    pub fn set_action(&self, look: &mut UnitLook, action: u8) {
        let (_, blocks, t) = self.bank(look);
        look.frame = blocks[t - 1].offsets.get(action as usize).copied().unwrap_or(0);
    }

    /// Port of FUN_0043a1b0: the frame number at block offset `pos` (0 = end).
    pub fn frame_at(&self, look: &UnitLook, pos: u8) -> u8 {
        self.frame_at_pos(look, pos as usize)
    }

    /// FUN_0043a1b0 with an unclamped offset (callers pass `frame + 2`).
    pub fn frame_at_pos(&self, look: &UnitLook, pos: usize) -> u8 {
        let (_, blocks, t) = self.bank(look);
        blocks[t - 1].bytes.get(pos).copied().unwrap_or(0)
    }

    /// Port of FUN_0043a34f: decode the current frame into `gfx.sprite` and
    /// store its horizontal offset in `look.remap`.
    pub fn decode(&self, look: &mut UnitLook, gfx: &mut UnitGfx) {
        let (bank, blocks, t) = self.bank(look);
        let Ok(ty) = bank.unit_type(t) else { return };
        let n = blocks[t - 1].bytes.get(look.frame as usize).copied().unwrap_or(0) as usize;
        // FUN_0043a34f indexes first + n - 1 without checking n.
        let idx = (ty.first_frame + n).saturating_sub(1);
        if let Ok((img, dx)) = bank.frame_abs(idx) {
            gfx.sprite = Some(img);
            look.remap = dx;
        }
    }

    /// Port of FUN_00441a24: face `dir` keeping the animation set; facing the
    /// same way steps to the next frame (wrapping to the action's first).
    pub fn step(&self, look: &mut UnitLook, gfx: &mut UnitGfx, dir: u8) {
        if gfx.sprite.is_none() {
            return;
        }
        let action = (dir & 3) | (look.facing & 0xfc);
        if look.facing == action {
            look.frame = look.frame.wrapping_add(1);
            if self.frame_at(look, look.frame) == 0 {
                self.set_action(look, action);
            }
        } else {
            self.set_action(look, action);
            look.facing = action;
        }
        self.decode(look, gfx);
    }

    /// Switch to animation `set` (0 idle, 1 active/walk, 2 attack, ...)
    /// keeping the direction, as the callers of FUN_0043a1fe do.
    pub fn set_anim(&self, look: &mut UnitLook, gfx: &mut UnitGfx, set: u8) {
        look.facing = (set << 2) | (look.facing & 3);
        self.set_action(look, look.facing);
        self.decode(look, gfx);
    }

    /// Number of frames in the action string starting at `look.frame`.
    pub fn action_len(&self, look: &UnitLook) -> usize {
        let (_, blocks, t) = self.bank(look);
        blocks[t - 1].bytes[look.frame as usize..].iter().take_while(|&&b| b != 0).count()
    }
}

/// Images and tables the unit pipeline needs.
pub struct UnitArt<'a> {
    /// sysimg 21: ground shadow ellipse (64x32).
    pub shadow: &'a Image,
    /// sysimg 22: water ripple (64x32).
    pub ripple: &'a Image,
    /// sysimg 38: stone texture for petrified units.
    pub stone: &'a Image,
    /// `DAT_0049fe08`: the 13 tint tables.
    pub tints: &'a [[u8; 256]],
}

/// Globals of the original that change how units are drawn.
#[derive(Debug, Clone, Copy, Default)]
pub struct DrawGlobals {
    /// `DAT_00494668`.
    pub vs_mode: bool,
    /// `DAT_004944e0`: side acting in VS mode.
    pub acting_side: i32,
    /// `DAT_00494652` 遮掩網點: units behind scenery are dithered instead of cut.
    pub mesh: bool,
}

impl DrawGlobals {
    pub fn cut_mode(&self) -> CutMode {
        if self.mesh { CutMode::Dither } else { CutMode::Solid }
    }
}

/// A unit together with its decoded frame, as seen by the occlusion passes.
#[derive(Clone, Copy)]
pub struct Placed<'a> {
    pub look: &'a UnitLook,
    pub sprite: Option<&'a Image>,
    /// `+0xb1`.
    pub under: Option<&'a Image>,
}

/// Map-surface rectangle of a unit's frame without its dx (`FUN_00442022`).
pub fn frame_origin(map: &MapView, col: i32, row: i32, w: usize, h: usize) -> (i32, i32) {
    let mut x = col * 64;
    if row & 1 != 0 {
        x += 32;
    }
    let ht = map.height_at(col, row);
    (x + 32 - (w as i32 >> 1), (row * 16 + 32 - h as i32) - (ht - 1) * 16)
}

/// `FUN_0046f907`: grab a rectangle (out-of-range parts zeroed).
pub fn grab(src: &Image, x: i32, y: i32, w: usize, h: usize) -> Image {
    let mut out = Image::new(w, h);
    for r in 0..h as i32 {
        let sy = y + r;
        if sy < 0 || sy >= src.height as i32 {
            continue;
        }
        for c in 0..w as i32 {
            let sx = x + c;
            if sx >= 0 && sx < src.width as i32 {
                out.pixels[(r * w as i32 + c) as usize] = src.pixels[(sy * src.width as i32 + sx) as usize];
            }
        }
    }
    out
}

/// Port of FUN_0046f907 into an existing buffer (`buf`'s size is the
/// rectangle's): rows above the surface are zeroed, and so is the part of
/// the first row left of it; what lies right of or below the surface keeps
/// the buffer's old contents, and a rectangle sticking out on both sides is
/// copied skewed, all as in the original.
pub fn grab_into(src: &Image, x: i32, y: i32, buf: &mut Image) {
    let (sw, sh) = (src.width as i32, src.height as i32);
    let (mut x, mut y, mut w, mut rows) = (x, y, buf.width as i32, buf.height as i32);
    if !(y < sh && x < sw) {
        return;
    }
    let mut d = 0usize;
    let mut skip = 0i32;
    let off = |buf: &mut Image| {
        for p in buf.pixels.iter_mut().take(4) {
            *p = 0xff;
        }
    };
    if y < 0 {
        if y + rows < 1 {
            return off(buf);
        }
        rows += y;
        let n = ((-y * w) as usize).min(buf.pixels.len());
        buf.pixels[..n].fill(0);
        d += n;
        y = 0;
    }
    if x < 0 {
        if x + w < 1 {
            return off(buf);
        }
        skip = -x;
        w += x;
        x = 0;
        let n = (skip as usize).min(buf.pixels.len() - d);
        buf.pixels[d..d + n].fill(0);
        d += n;
    }
    if sw < x + w {
        skip = (w - sw) + x;
        w = sw - x;
    }
    if sh <= y + rows {
        rows = sh - y;
    }
    for r in 0..rows {
        let s0 = ((y + r) * sw + x) as usize;
        let n = (w as usize).min(buf.pixels.len().saturating_sub(d));
        buf.pixels[d..d + n].copy_from_slice(&src.pixels[s0..s0 + n]);
        d += (w + skip) as usize;
        if d >= buf.pixels.len() {
            break;
        }
    }
}

/// Port of FUN_00442022: capture what lies behind unit `look` (frame size
/// = `buf`'s) on the map surface into `buf`. Unless `mode` is 1, the
/// save-unders of both armies' units nearby are pasted over it
/// (FUN_00442238), so units drawn into the surface drop out of it again;
/// `mode` 2 then adds the frames of the units standing just behind
/// (FUN_004423d3).
pub fn capture(map: &MapView, surface: &Image, look: &UnitLook, buf: &mut Image, mode: i32, armies: [&[Placed]; 2]) {
    let (x0, y0) = frame_origin(map, look.x, look.y, buf.width, buf.height);
    grab_into(surface, x0, y0, buf);
    if mode == 1 {
        return;
    }
    // The unit's own army first, then the other (`this`, `this+0x282`).
    for p in own_first(look, armies) {
        let o = p.look;
        let (Some(u), Some(s)) = (p.under, p.sprite) else { continue };
        if !o.is_baked() || (o.x == look.x && o.y == look.y) {
            continue;
        }
        if o.x < look.x - 2 || o.x > look.x + 2 || o.y > look.y + 0x18 || o.y < look.y - 0x18 {
            continue;
        }
        let (ox, oy) = frame_origin(map, o.x, o.y, s.width, s.height);
        buf.blit_opaque(u, (ox - x0) as isize, (oy - y0) as isize);
    }
    if mode == 2 {
        add_behind(map, look, buf, armies);
    }
}

/// Port of FUN_004423d3 for both armies: the frames of the units on the
/// 24 rows behind `look` (2 columns either side) into its save-under.
pub fn add_behind(map: &MapView, look: &UnitLook, under: &mut Image, armies: [&[Placed]; 2]) {
    let (x0, y0) = frame_origin(map, look.x, look.y, under.width, under.height);
    for p in own_first(look, armies) {
        let o = p.look;
        let Some(s) = p.sprite else { continue };
        if !o.is_baked() || (o.x == look.x && o.y == look.y) {
            continue;
        }
        if o.x < look.x - 2 || o.x > look.x + 2 || o.y >= look.y || o.y < look.y - 0x18 {
            continue;
        }
        let (ox, oy) = frame_origin(map, o.x, o.y, s.width, s.height);
        under.blit(s, (ox - x0) as isize, (oy - y0) as isize);
    }
}

/// Both armies' units, `look`'s own army first (the original passes the
/// unit's army, then `army+0x282`).
fn own_first<'a>(look: &UnitLook, armies: [&'a [Placed<'a>]; 2]) -> impl Iterator<Item = &'a Placed<'a>> {
    let s = (look.side as usize).min(1);
    armies[s].iter().chain(armies[1 - s].iter())
}

/// What lies behind a unit when it has no save-under yet: the bare map
/// background there.
pub fn bare_under(map: &MapView, look: &UnitLook, sprite: &Image) -> Image {
    let (x0, y0) = frame_origin(map, look.x, look.y, sprite.width, sprite.height);
    grab(&map.bg, x0, y0, sprite.width, sprite.height)
}

/// `FUN_0042060e`: remap every non-zero pixel through tint table `k`.
fn tint_all(img: &mut Image, lut: &[u8; 256]) {
    for p in img.pixels.iter_mut().filter(|p| **p != 0) {
        *p = lut[*p as usize];
    }
}

/// `FUN_00428221`: the cell's ground tile belongs to terrain class 1 or 2 (water).
pub fn is_water(map: &MapView, tiles: &MapTiles, col: i32, row: i32) -> bool {
    let i = row * map.width() + col;
    if i < 0 || i as usize >= map.def.cells.len() {
        return false;
    }
    let g = map.def.cells[i as usize].ground as i32;
    if g == 0 {
        return false;
    }
    let class = (tiles.attrs.ground.get((g - 1).max(0) as usize).copied().unwrap_or(0) >> 8) as i32;
    (class == 1 || class == 2) && g > 4
}

/// Port of FUN_004425f2: ground shadow (or water ripple) drawn into the
/// sprite's bottom 64x32 band around its centre.
fn add_shadow(look: &UnitLook, buf: &mut Image, under: &Image, water: bool, art: &UnitArt) {
    let (w, h) = (buf.width as isize, buf.height as isize);
    let band = (h - 32) * w;
    if water {
        if look.class_move == 9 || band + look.remap as isize <= 0 {
            return;
        }
        let p0 = band + look.remap as isize + (w >> 1) - 32;
        for r in 0..32isize {
            for c in 0..64isize {
                let e = art.ripple.pixels[(r * 64 + c) as usize];
                let i = p0 + r * w + c;
                if e != 0 && i >= 0 && (i as usize) < buf.pixels.len() {
                    buf.pixels[i as usize] = e;
                }
            }
        }
    } else {
        let p0 = band + (w >> 1) - 32 - look.remap as isize;
        let q0 = band + (w >> 1) - 32;
        let lut = &art.tints[5];
        for r in 0..32isize {
            for c in 0..64isize {
                let e = art.shadow.pixels[(r * 64 + c) as usize];
                let (i, q) = (p0 + r * w + c, q0 + r * w + c);
                if e == 0 || i < 0 || i as usize >= buf.pixels.len() || buf.pixels[i as usize] != 0 {
                    continue;
                }
                let bgc = if q >= 0 && (q as usize) < under.pixels.len() { under.pixels[q as usize] } else { 0 };
                buf.pixels[i as usize] = lut[bgc as usize];
            }
        }
    }
}

/// Port of FUN_0044d405: while struck, the unit is redrawn 128 px wide,
/// shaking 4 px each frame, its shadow-colour pixels showing the darkened
/// ground. Toggles the shake phase in `look.render_flags`.
fn hit_shake(look: &mut UnitLook, sprite: &Image, under: &Image, art: &UnitArt) -> Image {
    let (w, h) = (sprite.width as isize, sprite.height as isize);
    let mut tmp = sprite.pixels.clone();
    let base = w * h + (w >> 1) - 32 * w - 0x1c - 4;
    for r in 0..32isize {
        for c in 0..64isize {
            let e = art.shadow.pixels[(r * 64 + c) as usize];
            let i = base + r * w + c;
            if e != 0 && i >= 0 && (i as usize) < tmp.len() && tmp[i as usize] == 0 {
                tmp[i as usize] = e;
            }
        }
    }
    let mut out = Image::new(128, h as usize);
    let i3 = (128 - w) / 2;
    let i2 = i3 - 4;
    let lut = &art.tints[5];
    let px = |s: isize, b: isize| -> u8 {
        let v = tmp.get(s as usize).copied().unwrap_or(0);
        if v == 0x20 {
            lut[under.pixels.get(b as usize).copied().unwrap_or(0) as usize]
        } else {
            v
        }
    };
    let (mut src, mut bgp) = (0isize, 0isize);
    let cols = if i2 < 0 { w + i2 } else { w };
    if look.render_flags & R_HIT_PHASE == 0 {
        let mut dst = if i2 < 0 { 0 } else { 128 - w - i2 };
        for _ in 0..h {
            for c in 0..cols {
                let d = dst + c;
                if d >= 0 && (d as usize) < out.pixels.len() {
                    out.pixels[d as usize] = px(src, bgp);
                }
                src += 1;
                bgp += 1;
            }
            if i2 < 0 {
                src -= i2;
            }
            dst += 128;
        }
    } else {
        let mut dst = if i2 < 0 { 0 } else { i3 - 4 };
        for _ in 0..h {
            if i2 < 0 {
                src -= i2;
                bgp -= i2;
            }
            for c in 0..cols {
                let d = dst + c;
                if d >= 0 && (d as usize) < out.pixels.len() {
                    out.pixels[d as usize] = px(src, bgp);
                }
                src += 1;
                bgp += 1;
            }
            dst += 128;
        }
    }
    look.render_flags ^= R_HIT_PHASE;
    out
}

/// Port of FUN_0043be00: invisible units become a shadow silhouette (own
/// side, done), a darkened see-through image (own side) or vanish (enemy).
fn invisible(look: &UnitLook, g: &DrawGlobals, buf: &mut Image, under: &Image, art: &UnitArt) {
    let own = if g.vs_mode { look.side == g.acting_side } else { look.side == 0 };
    let mode = if !own {
        3
    } else if look.act == ACT_DONE {
        1
    } else {
        2
    };
    let lut = &art.tints[6];
    for (p, &b) in buf.pixels.iter_mut().zip(under.pixels.iter()) {
        if *p == 0 {
            continue;
        }
        *p = match mode {
            1 if *p == 0x20 => lut[b as usize],
            1 => 0x20,
            2 => lut[b as usize],
            _ => b,
        };
    }
}

/// Port of FUN_0043c0dc: classes whose enemy units are recoloured.
pub fn enemy_recolor(kind: i16) -> bool {
    !matches!(kind, 0x1a | 0x1f | 0x5d | 0x5e | 0x60 | 0x62 | 0x69..=0x70)
}

/// Port of FUN_0043bf41: the unit's frame as it will be drawn this time.
pub fn prepare(look: &mut UnitLook, sprite: &Image, under: &Image, water: bool, g: &DrawGlobals, art: &UnitArt) -> Image {
    let hit = look.render_flags & R_HIT != 0;
    let mut buf = if hit { hit_shake(look, sprite, under, art) } else { sprite.clone() };
    if g.vs_mode {
        tint_all(&mut buf, &art.tints[if look.side == 0 { 10 } else { 11 }]);
    } else if look.side == 1 && enemy_recolor(look.kind) {
        tint_all(&mut buf, &art.tints[11]);
    }
    if look.buffs & B_PETRIFIED != 0 {
        // FUN_00420b2f: stone texture through the silhouette.
        let sw = art.stone.width;
        for y in 0..buf.height {
            for x in 0..buf.width {
                let p = &mut buf.pixels[y * buf.width + x];
                if *p != 0 && *p != 0x20 {
                    if let Some(&t) = art.stone.pixels.get(y * sw + x).filter(|_| x < sw) {
                        *p = t;
                    }
                }
            }
        }
    } else if look.status & S_FROZEN != 0 {
        // FUN_0042067b(buf, 9, 0xc2).
        for p in buf.pixels.iter_mut().filter(|p| **p != 0 && **p != 0x20) {
            *p = if *p == 0x29 { 0xc2 } else { art.tints[9][*p as usize] };
        }
    } else if look.invisible {
        add_shadow(look, &mut buf, under, water, art);
        invisible(look, g, &mut buf, under, art);
        return buf;
    } else if look.act == ACT_DONE {
        tint_all(&mut buf, &art.tints[7]);
    }
    if !hit && look.flags & F_SUMMONING == 0 {
        add_shadow(look, &mut buf, under, water, art);
    }
    buf
}

/// Port of FUN_00440591 (`except` = None) and FUN_00440894 (`except` =
/// the unit whose own cell is skipped): clear the parts of `img`, standing
/// on (col, row) with offset `dx`, that units of `army` in front of it cover.
/// `mode` 1 = units on later rows, 2 = also the same row, 3 = only the same
/// row; `p5` 0 skips selected units, 2 keeps a unit on the same cell.
#[allow(clippy::too_many_arguments)]
pub fn cut_by_units(
    map: &MapView,
    army: &[Placed],
    army_side: i32,
    g: &DrawGlobals,
    col: i32,
    row: i32,
    img: &mut Image,
    mode: i32,
    p5: i32,
    dx: i32,
    except: Option<(i32, i32)>,
) {
    cut_by_units_at(map, army, army_side, g, col, row, img, mode, p5, dx, except, (0, 0));
}

/// [`cut_by_units`] for an image drawn `shift` pixels away from its cell
/// (smooth motion, not in the original).
#[allow(clippy::too_many_arguments)]
pub fn cut_by_units_at(
    map: &MapView,
    army: &[Placed],
    army_side: i32,
    g: &DrawGlobals,
    col: i32,
    row: i32,
    img: &mut Image,
    mode: i32,
    p5: i32,
    dx: i32,
    except: Option<(i32, i32)>,
    shift: (i32, i32),
) {
    let wide = dx + shift.0;
    let (lo, hi) = match except {
        Some(_) => (col - 2, col + 2),
        None if wide == 0 => (col - 2, col + 2),
        None if wide < 0 => (col - 3, col + 2),
        None => (col - 2, col + 3),
    };
    let h0 = map.height_at(col, row);
    let my_x = {
        let mut x = (col * 64 + 0x20) - (img.width as i32 >> 1) + if except.is_some() { 0 } else { dx } + shift.0;
        if row & 1 != 0 {
            x += 0x20;
        }
        x
    };
    let my_y = (row * 16 + 0x20) - img.height as i32 + shift.1;
    for p in army {
        let o = p.look;
        let Some(s) = p.sprite else { continue };
        if !o.on_map() || o.act == ACT_WALKING {
            continue;
        }
        if except.is_none() && ((o.act == ACT_SELECTED && p5 == 0) || o.act == ACT_COMBAT) {
            continue;
        }
        let hidden = if g.vs_mode { army_side != g.acting_side && o.invisible } else { army_side == 1 && o.invisible };
        if hidden {
            continue;
        }
        let same_cell = o.x == col && o.y == row;
        if except.is_some() {
            if (same_cell && p5 == 0) || except == Some((o.x, o.y)) {
                continue;
            }
        } else if same_cell && !(p5 != 0 && p5 != 2) {
            continue;
        }
        if o.x < lo || o.x > hi {
            continue;
        }
        let rows_ok = (row < o.y && o.y <= row + 0x18)
            || (mode == 2 && row <= o.y && o.y <= row + 0x18)
            || (mode == 3 && o.y == row);
        if !rows_ok {
            continue;
        }
        let mut h2 = map.height_at(o.x, o.y);
        if h2 == 0 {
            continue;
        }
        if except.is_none() && o.act == ACT_SELECTED && (o.move_type == 3 || o.move_type == 4) {
            h2 += 4;
        }
        let mut ox = (o.x * 64 + 0x20) - (s.width as i32 >> 1);
        if o.y & 1 != 0 {
            ox += 0x20;
        }
        let oy = (o.y * 16 + 0x20) - s.height as i32;
        cut(img, s, ox - my_x, (oy - my_y) - (h2 - h0) * 16, CutMode::Solid, false);
    }
}

/// Everything needed to draw units on one map.
pub struct UnitScene<'a> {
    pub map: &'a MapView,
    pub tiles: &'a MapTiles,
    pub shading: &'a Shading,
    pub art: UnitArt<'a>,
    pub g: DrawGlobals,
    /// Both armies (index = side).
    pub armies: [&'a [Placed<'a>]; 2],
}

impl UnitScene<'_> {
    /// FUN_0043bf41 + FUN_00426417 + FUN_0044055b x2: the finished image of
    /// a unit with save-under `under`.
    pub fn finished(&self, look: &mut UnitLook, sprite: &Image, under: &Image, skip_overlay: Option<&dyn Fn(i32, i32) -> bool>) -> Image {
        let buf = self.prepared(look, sprite, under);
        self.cut_all(buf, look, self.g.cut_mode(), skip_overlay)
    }

    /// FUN_00426417 (with the given cut mode) + FUN_0044055b for both armies.
    pub fn cut_all(&self, buf: Image, look: &UnitLook, mode: CutMode, skip_overlay: Option<&dyn Fn(i32, i32) -> bool>) -> Image {
        self.cut_all_at(buf, look, mode, skip_overlay, (0, 0))
    }

    /// [`Self::cut_all`] for an image drawn `shift` pixels from its cell.
    pub fn cut_all_at(
        &self,
        mut buf: Image,
        look: &UnitLook,
        mode: CutMode,
        skip_overlay: Option<&dyn Fn(i32, i32) -> bool>,
        shift: (i32, i32),
    ) -> Image {
        let r = self.map.renderer(self.tiles, self.shading);
        let occ = Occlusion { dx: look.remap as i32, mode, own_overlay: false, skip_overlay, shift };
        r.occlude(&mut buf, look.x, look.y, occ);
        for side in 0..2 {
            cut_by_units_at(self.map, self.armies[side], side as i32, &self.g, look.x, look.y, &mut buf, 1, 0, look.remap as i32, None, shift);
        }
        buf
    }

    /// The prepared image (FUN_0043bf41) without the occlusion passes.
    pub fn prepared(&self, look: &mut UnitLook, sprite: &Image, under: &Image) -> Image {
        let water = is_water(self.map, self.tiles, look.x, look.y);
        prepare(look, sprite, under, water, &self.g, &self.art)
    }

    /// Port of FUN_0044d024's image: every non-shadow pixel in colour 0x3c,
    /// plus the ground shadow, occluded with solid cuts.
    pub fn hit_flash_image(&self, look: &UnitLook, sprite: &Image, under: &Image) -> Image {
        let mut buf = sprite.clone();
        for p in buf.pixels.iter_mut().filter(|p| **p != 0 && **p != 0x20) {
            *p = 0x3c;
        }
        let water = is_water(self.map, self.tiles, look.x, look.y);
        add_shadow(look, &mut buf, under, water, &self.art);
        self.cut_all(buf, look, CutMode::Solid, None)
    }

    /// Smooth motion (not in the original): [`Self::draw_on_screen`] with the
    /// unit `shift` pixels from its cell (`look`'s), its shadow over what lies
    /// there on `surface` and the scenery cut where it is drawn.
    pub fn draw_shifted(&self, page: &mut Image, look: &mut UnitLook, sprite: &Image, surface: &Image, shift: (i32, i32)) {
        if !look.on_map() {
            return;
        }
        let (x0, y0) = frame_origin(self.map, look.x, look.y, sprite.width, sprite.height);
        let under = grab(surface, x0 + shift.0, y0 + shift.1, sprite.width, sprite.height);
        let buf = self.prepared(look, sprite, &under);
        let buf = self.cut_all_at(buf, look, self.g.cut_mode(), None, shift);
        let (x, y) = unit_screen_pos(self.map, look.x, look.y, &buf, 0, look.remap as i32);
        page.blit(&buf, (x + shift.0) as isize, (y + shift.1) as isize);
    }

    /// Port of FUN_0043ff88: draw a unit onto a screen page.
    pub fn draw_on_screen(&self, page: &mut Image, look: &mut UnitLook, sprite: &Image, under: &Image, lift: i32) {
        if !look.on_map() {
            return;
        }
        let buf = self.finished(look, sprite, under, None);
        let (x, y) = unit_screen_pos(self.map, look.x, look.y, &buf, lift, look.remap as i32);
        page.blit(&buf, x as isize, y as isize);
    }

    /// Port of FUN_00440264's drawing (and FUN_004403b5's body): draw a
    /// unit into the map surface, without its dx like the original; `full`
    /// = with the occlusion passes.
    pub fn put(&self, surface: &mut Image, look: &mut UnitLook, sprite: &Image, under: &Image, full: bool) {
        let mut buf = self.prepared(look, sprite, under);
        if full {
            buf = self.cut_all(buf, look, self.g.cut_mode(), None);
        }
        let (x, y) = frame_origin(self.map, look.x, look.y, buf.width, buf.height);
        surface.blit(&buf, x as isize, y as isize);
    }
}

/// Port of FUN_0042665a: screen position of a unit image (`lift` raises it
/// by whole rows of 16 px; `dx` is the frame's horizontal offset).
pub fn unit_screen_pos(map: &MapView, col: i32, row: i32, img: &Image, lift: i32, dx: i32) -> (i32, i32) {
    let ht = map.height_at(col, row);
    let x = (col * 64 + if row & 1 != 0 { 32 } else { 0 }) - map.scroll_x * 32;
    let u = row - map.scroll_y;
    let x = (x - (img.width as i32 - 1 - 0x3f) / 2) + dx;
    let y = (u * 16 + ((ht - 1) + lift) * -16) - (img.height as i32 - 1 - 0x1f);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_block_offsets() {
        let b = ActionBlock::from_actions(&[vec![1], vec![2], vec![5, 6]]);
        assert_eq!(&b.offsets[..3], &[0, 2, 4]);
        assert_eq!(&b.bytes[..7], &[1, 0, 2, 0, 5, 6, 0]);
    }

    #[test]
    fn recolor_table() {
        assert!(!enemy_recolor(0x1a));
        assert!(!enemy_recolor(0x6a));
        assert!(enemy_recolor(1));
        assert!(enemy_recolor(0x71));
    }
}
