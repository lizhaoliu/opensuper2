//! Spell and item targeting (`DAT_00492a68`, the code at 0x401000–0x406a00):
//! the cells a spell can be aimed at (its range around the caster), the
//! area an aimed cell covers for each area shape, the cell order the cast
//! effects iterate, and the AI's per-spell scoring over those cells
//! (docs/ai.md §3.2). Owned by the AI port.
//!
//! The interactive cursor loops (`FUN_00401634`, `FUN_004041d7`,
//! `FUN_004052e3`) and the range/area/panel drawing (`FUN_00401bfa`,
//! `FUN_00401d75`, `FUN_00403e52`, `FUN_00403f82`) are the UI's; the checks
//! those loops make are here ([`Battle::check_cast_cell`], [`Battle::finish_aim`]).
//! Attack (weapon/action) ranges are `combat::TargetList` (`FUN_00406a36`).
//!
//! TARGETING API
//! -------------
//! ```text
//! Battle::spell_target(side, caster, spell, mode, caller, from_item) -> SpellTarget   FUN_004010ce
//! Battle::item_target(side, caster, item) -> SpellTarget         FUN_004067e1 set-up (spell = −item, mode 2)
//! Battle::target_range(&mut t, range)          FUN_004010ff: the range diamond (−1 = every visible unit, FUN_004014d7)
//! Battle::target_area(&mut t)                  FUN_00402310: the area at `Battle::cursor`
//! SpellTarget::count() / cells()               FUN_0040507b / FUN_004050bc + FUN_00405105 (shape 8: the chosen cone)
//! SpellTarget::in_range_or_area(x, y) / in_area(x, y)     FUN_00402213 / FUN_004022b6
//! Battle::range_has_foe / range_has_own / area_has_foe / area_has_own(&t)   FUN_00401573 / FUN_004015d4 / FUN_00403d91 / FUN_00403df2
//! Battle::next_foe_in_area(&mut t) -> Option<(x, y)>   FUN_004051b3 (Tab)
//! Battle::cursor_unit(&t) -> Option<UnitRef>   FUN_00401a4b: the unit the info panel shows
//! Battle::check_cast_cell(&mut t) -> CellCheck   the checks of FUN_004041d7 before its confirm loop
//! Battle::finish_aim(&mut t)                   FUN_004041d7 after a confirmed line/cone aim (ray, facing)
//! Battle::aim_ai_cast(&mut t, x, y, dir)       FUN_004055a4 (logic): the area of an AI cast
//! Battle::item_amount(item, r) -> i32          FUN_0040684c: what a restoring item would give
//! Battle::ai_score_spell(&mut t, &cast, &mut best, &mut plan) -> bool
//!                                              FUN_0040576b (+ FUN_004059a6, FUN_00405ebe, FUN_004063c4, FUN_004065d3)
//! Battle::support_value(&cast, side, r) -> (kind, value)   FUN_0046be07 (+ FUN_0046be92 filter)
//! Battle::valid_target_cell(x, y) / can_stand(x, y, class0)   FUN_0042855a / FUN_004282e6 (map queries)
//! ring_cells(x, y, n)                          the ring order shared by FUN_004010ff, FUN_0040284d, FUN_0040c5d8
//! ```
//!
//! Shapes (SpellRec +0x12): 0/1 one cell; 2/3/4 a 3×3/5×5/7×7 diamond
//! (camera recentred); 5 four rays from the caster of length +0x11 (one is
//! picked); 6 a cross of four length-6 rays; 7 every cell on screen;
//! 8 four cones from the caster (one is picked). Effect 4 (teleport) aims at
//! a unit, then at a free cell within `魔力/10 + 1` of it; summons
//! (effects 0x15..=0x1f) take the first free neighbour of the caster.

use super::ai::{Best, SpellPlan};
use super::army::{Side, UnitRef};
use super::spells::Cast;
use super::unit::{flag, status};
use super::Battle;

/// Capacity of the area list per shape (`FUN_00402310`, `local_24`).
const AREA_CAP: [usize; 8] = [1, 1, 9, 0x19, 0x31, 0x19, 0x19, 0x15e];

/// The spell fields the targeting code reads: SpellRec +0x11 range,
/// +0x12 shape, +0x13 effect, +0x1f target side, +0x20 combo bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Aim {
    pub range: i32,
    pub shape: u8,
    pub effect: u8,
    pub target: u8,
    pub combo: u8,
}

/// The spell targeting object `DAT_00492a68`.
#[derive(Debug, Clone)]
pub struct SpellTarget {
    /// +0x00 the caster's army (+0x04 is its opponent).
    pub side: Side,
    /// +0x08
    pub caster: UnitRef,
    /// +0x14 1-based spell id; −item id when aiming an item.
    pub spell: i32,
    /// Fields of the spell record (zero for items).
    pub aim: Aim,
    /// +0x68 0 magic, 1 technique, 2 item (single cell).
    pub mode: u8,
    /// +0x69 caller: 0 AI evaluation, 1 player, 2 AI cast.
    pub caller: u8,
    /// +0x6a cast from an item.
    pub from_item: bool,
    /// +0x20 / +0x24 / +0x28 cells the spell can be aimed at (the caster's cell first).
    pub range_cap: usize,
    pub range: Vec<(i32, i32)>,
    /// +0x2c / +0x30 / +0x38 cells the aimed area covers ((−1, −1) marks a
    /// cut-off cross ray).
    pub area_cap: usize,
    pub area: Vec<(i32, i32)>,
    /// +0x34 iteration / Tab position.
    pub iter: usize,
    /// +0x44 1 area built, 0 none, −1 rejected.
    pub ready: i32,
    /// +0x48 confirm (1) / cancel (0) button of the panel.
    pub confirm: i32,
    /// +0x4c chosen ray / cone direction (0 down-left, 1 down-right, 2 up-right, 3 up-left; read by FUN_0046cf30).
    pub dir: usize,
    /// +0x60 / +0x64 the unit cell chosen first by a teleport (read by FUN_0046cf00).
    pub target_cell: (i32, i32),
    /// +0x6b / +0x6c start and length of each cone in `area`.
    pub cone: [(u8, u8); 4],
}

impl SpellTarget {
    /// Port of FUN_0040507b: number of cells a cast iterates.
    pub fn count(&self) -> usize {
        if self.aim.shape == 8 {
            self.cone[self.dir & 3].1 as usize
        } else {
            self.area.len()
        }
    }

    /// FUN_004050bc + FUN_00405105 run `count()` times: the cells a cast
    /// iterates (shape 8 only the chosen cone).
    pub fn cells(&self) -> &[(i32, i32)] {
        if self.aim.shape == 8 {
            let (s, n) = self.cone[self.dir & 3];
            let e = (s as usize + n as usize).min(self.area.len());
            &self.area[(s as usize).min(e)..e]
        } else {
            &self.area
        }
    }

    /// Port of FUN_00402213: (x, y) is a range cell or an area cell.
    pub fn in_range_or_area(&self, x: i32, y: i32) -> bool {
        self.range.contains(&(x, y)) || self.area.contains(&(x, y))
    }

    /// Port of FUN_004022b6: (x, y) is an area cell.
    pub fn in_area(&self, x: i32, y: i32) -> bool {
        self.area.contains(&(x, y))
    }

    /// Port of FUN_00403262: drop the area.
    pub fn free_area(&mut self) {
        self.area.clear();
        self.area_cap = 0;
        self.ready = 0;
    }

    /// Port of FUN_0040145c: drop range and area.
    pub fn free(&mut self) {
        self.range.clear();
        self.range_cap = 0;
        self.spell = 0;
        self.free_area();
    }
}

/// What `FUN_004041d7` decides about the aimed cell before its confirm loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellCheck {
    /// Outside the range (message `DAT_00492a50[mode]`, `DAT_00492a58` for
    /// items), or no fitting unit there.
    OutOfRange,
    /// No target in the area (UI string 0x150 / 4).
    NoTarget,
    /// A summon has no free cell next to the caster (UI string 0x154 / 4).
    NoRoom,
    /// Teleport: the unit is chosen and the area holds the destinations
    /// (`FUN_004052e3` picks one).
    PickDestination,
    /// The area is built: show it and wait for the confirmation.
    Confirm,
}

/// The ring order of FUN_004010ff / FUN_0040284d / FUN_0040c5d8: for rings
/// 1..=n around (x0, y0), the top and bottom rows left to right (top cell
/// first), then the two sides top to bottom (left first).
pub fn ring_cells(x0: i32, y0: i32, n: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let (mut lx, mut cy) = (x0, y0);
    for c in 1..=n {
        let l = if cy & 1 == 0 { lx - 1 } else { lx };
        lx = l;
        let top = cy - 1;
        for x in l..=l + c {
            out.push((x, top));
            out.push((x, top + c * 2));
        }
        let mut y = cy + 1;
        while y <= cy - 3 + c * 2 {
            out.push((l, y));
            out.push((l + c, y));
            y += 2;
        }
        cy = top;
    }
    out
}

/// One step of FUN_004032c1 / FUN_00403a18 (0 down-left, 1 down-right,
/// 2 up-right, 3 up-left).
fn step(x: i32, y: i32, dir: usize) -> (i32, i32) {
    let odd = y & 1 != 0;
    match dir {
        0 => (if odd { x } else { x - 1 }, y + 1),
        1 => (if odd { x + 1 } else { x }, y + 1),
        2 => (if odd { x + 1 } else { x }, y - 1),
        _ => (if odd { x } else { x - 1 }, y - 1),
    }
}

/// C division as the original's integer code needs it (÷0 → 0 instead of a fault).
fn div(a: i32, b: i32) -> i32 {
    if b == 0 {
        0
    } else {
        a.wrapping_div(b)
    }
}

impl Battle {
    /// SpellRec fields for `spell` (zero for items and unknown ids).
    pub fn spell_aim(&self, spell: i32) -> Aim {
        let Some(t) = self.tables.as_ref() else { return Aim::default() };
        if spell < 1 {
            return Aim::default();
        }
        t.spells.spells.get(spell as usize - 1).map_or(Aim::default(), |s| Aim {
            range: s.range as i32,
            shape: s.shape,
            effect: s.effect,
            target: s.target,
            combo: s.combo,
        })
    }

    /// Port of FUN_004010ce (and the army part of FUN_004010ff): a fresh
    /// targeting object for `caster` casting `spell`.
    pub fn spell_target(&self, side: Side, caster: UnitRef, spell: i32, mode: u8, caller: u8, from_item: bool) -> SpellTarget {
        SpellTarget {
            side,
            caster,
            spell,
            aim: self.spell_aim(spell),
            mode,
            caller,
            from_item,
            range_cap: 0,
            range: Vec::new(),
            area_cap: 0,
            area: Vec::new(),
            iter: 0,
            ready: 0,
            confirm: 0,
            dir: 0,
            target_cell: (0, 0),
            cone: [(0, 0); 4],
        }
    }

    /// The set-up of FUN_004067e1: aim item `item` (1-based) with its range
    /// (ItemRec +0x10); the cursor loop that follows is the UI's.
    pub fn item_target(&mut self, side: Side, caster: UnitRef, item: u16) -> SpellTarget {
        let mut t = self.spell_target(side, caster, -(item as i32), 2, 1, false);
        let range = self.item(item).map_or(0, |it| it.reach as i8 as i32);
        self.target_range(&mut t, range);
        t
    }

    /// Port of FUN_0042855a: a cell inside the map array whose ground tile
    /// id is ≥ 2 and that carries no blocking (0x8000) spot.
    pub fn valid_target_cell(&self, x: i32, y: i32) -> bool {
        let m = self.map();
        let idx = y * m.width() + x;
        if idx < 0 || idx as usize >= m.data.cells.len() {
            return false;
        }
        if (m.data.cells[idx as usize].ground as i16) < 2 {
            return false;
        }
        !m.blocked_by_spot(x, y)
    }

    /// Port of FUN_004282e6(map, x, y, class0): class index `class0`
    /// (0-based) may stand on the cell — ground and object terrain without
    /// the unit's movement modifiers. The object cost is read from class
    /// row 0, as in the original.
    pub fn can_stand(&self, x: i32, y: i32, class0: i32) -> bool {
        let m = self.map();
        let t = self.t();
        let idx = y * m.width() + x;
        if idx < 0 || idx as usize >= m.data.cells.len() {
            return false;
        }
        let cell = m.data.cells[idx as usize];
        if cell.ground == 0 {
            return false;
        }
        let attr = |bank: usize, id: u16| (id as usize).checked_sub(1).and_then(|i| m.attrs.banks[bank].get(i)).copied().unwrap_or(0);
        let g = attr(0, cell.ground) as i16;
        if g == 0 {
            return false;
        }
        let g_pass = g >> 8;
        let lo = g as i32 & 0xff;
        let g_ter = if lo == 0 { 0 } else { lo - 1 };
        let cost = |row: i32, ter: i32| -> i8 {
            usize::try_from(row)
                .ok()
                .and_then(|r| t.terrain.effects.get(r))
                .and_then(|r| usize::try_from(ter).ok().and_then(|i| r.get(i)))
                .map_or(-1, |e| e.cost)
        };
        if g_pass == 5 || cost(class0, g_ter) == -1 {
            return false;
        }
        let mt = u16::try_from(class0 + 1).ok().and_then(|c| t.kinds.class(c)).map_or(0, |c| c.move_type);
        let ok = match mt {
            1..=8 | 10 => matches!(g_pass, 0 | 1 | 2 | 4),
            9 => matches!(g_pass, 1 | 2),
            _ => false,
        };
        if !ok {
            return false;
        }
        if cell.object != 0 {
            let o = attr(1, cell.object);
            let o_pass = (o >> 8) as i32;
            let o_ter = (o & 0xff) as i32;
            if (t.terrain.objects as i32) < o_ter {
                return false;
            }
            if matches!(o_pass, 1 | 2 | 10) || cost(0, o_ter - 1 + t.terrain.ground as i32) == -1 {
                return false;
            }
        }
        true
    }

    /// FUN_004261bd + FUN_0042855a: the test most area builders make.
    fn area_cell_ok(&self, x: i32, y: i32) -> bool {
        self.map().targetable(x, y) && self.valid_target_cell(x, y)
    }

    /// Port of FUN_004010ff: the cells around the caster the spell can be
    /// aimed at. Shapes 5 and 8 aim from the caster only (range 0); −1
    /// takes every visible unit of both armies (FUN_004014d7).
    pub fn target_range(&mut self, t: &mut SpellTarget, range: i32) {
        t.range.clear();
        if range == -1 {
            // FUN_004014d7 / FUN_004577a2 / FUN_004581eb
            for side in [t.side, t.side.other()] {
                let a = self.army(side);
                for u in &a.units {
                    if u.flags & flag::DEPLOYED != 0 && u.flags & flag::DEAD == 0 && !self.is_hidden(side, u) {
                        t.range.push((u.x, u.y));
                    }
                }
            }
            t.range_cap = t.range.len();
            return;
        }
        let mut range = range;
        if t.spell > 0 && (t.aim.shape == 5 || t.aim.shape == 8) {
            range = 0;
        }
        t.range_cap = (range * 2 * (range + 1) + 1).max(0) as usize;
        let c = self.unit(t.caster);
        t.range.push((c.x, c.y));
        for (x, y) in ring_cells(c.x, c.y, range) {
            if self.area_cell_ok(x, y) {
                t.range.push((x, y));
            }
        }
    }

    /// Port of FUN_00402310: build the area the cursor cell covers.
    pub fn target_area(&mut self, t: &mut SpellTarget) {
        t.area.clear();
        let (cx, cy) = self.cursor;
        if t.mode == 2 {
            t.area_cap = 1;
            t.area.push((cx, cy));
        } else {
            let a = t.aim;
            let shape = a.shape as usize;
            if a.effect == 4 {
                let n = (self.unit(t.caster).mag_cur as i32 / 10 + 1).clamp(1, 10);
                t.area_cap = (n * 2 * (n + 1)) as usize;
            } else if shape == 5 {
                t.area_cap = (a.range * 4 + 1).max(0) as usize;
            } else if shape == 8 {
                t.area_cap = (a.range * a.range * 4 + 1).max(0) as usize;
            } else {
                // Shapes above 8 read past the original's table; nothing is built for them.
                t.area_cap = AREA_CAP.get(shape).copied().unwrap_or(0);
            }
            if a.effect == 4 {
                self.teleport_area(t);
                return;
            }
            if !(0x15..=0x1f).contains(&a.effect) {
                match shape {
                    0 | 1 => t.area.push((cx, cy)),
                    2..=4 => {
                        self.square_area(t, cx, cy, shape as i32 * 2 - 1);
                        self.map_mut().center_on(cx, cy, true);
                    }
                    5 | 6 => {
                        t.area.push((cx, cy));
                        let h = self.map().height(cx, cy);
                        let len = if shape == 5 { a.range } else { 6 };
                        self.cross_area(t, cx, cy, h, len);
                    }
                    7 => {
                        self.map_mut().center_on(cx, cy, true);
                        self.screen_area(t);
                    }
                    8 => self.cone_area(t, cx, cy),
                    _ => {}
                }
            } else {
                self.summon_area(t);
            }
            // (The original stops with error 4 when the area outgrows `area_cap`.)
        }
        t.ready = 1;
        t.confirm = 1;
        t.dir = 0;
    }

    /// Shapes 2..=4 of FUN_00402310: a `side`×`side` diamond whose top
    /// corner is `side − 1` rows above the centre; rows run down-right.
    fn square_area(&self, t: &mut SpellTarget, cx: i32, cy: i32, side: i32) {
        let mut rx = cx;
        let mut ry = cy - side;
        for _ in 0..side {
            ry += 1;
            let (row_x, row_y) = (rx, ry);
            let (mut x, mut y) = (rx, ry);
            for _ in 0..side {
                if self.area_cell_ok(x, y) {
                    t.area.push((x, y));
                }
                if y & 1 != 0 {
                    x += 1;
                }
                y += 1;
            }
            rx = if row_y & 1 == 0 { row_x - 1 } else { row_x };
        }
    }

    /// Port of FUN_00403427: four rays (up-right, up-left, down-left,
    /// down-right) interleaved step by step; a ray stops at an invalid cell
    /// or a height step (up ≤ 2, down ≤ 3) and pads with (−1, −1).
    fn cross_area(&self, t: &mut SpellTarget, x0: i32, y0: i32, h0: i32, len: i32) {
        let mut alive = [true; 4];
        let mut pos = [(x0, y0); 4];
        for _ in 0..len {
            for d in 0..4 {
                if !alive[d] {
                    t.area.push((-1, -1));
                    continue;
                }
                let (x, y) = pos[d];
                let odd = y & 1 != 0;
                pos[d] = match d {
                    0 => (if odd { x + 1 } else { x }, y - 1),
                    1 => (if odd { x } else { x - 1 }, y - 1),
                    2 => (if odd { x } else { x - 1 }, y + 1),
                    _ => (if odd { x + 1 } else { x }, y + 1),
                };
                let (x, y) = pos[d];
                let h = self.map().height(x, y);
                if self.area_cell_ok(x, y) && ((h <= h0 && h0 - h < 4) || (h0 < h && h - h0 < 3)) {
                    t.area.push((x, y));
                } else {
                    t.area.push((-1, -1));
                    alive[d] = false;
                }
            }
        }
    }

    /// Port of FUN_004032c1: one ray of up to `len` cells from (x, y) in
    /// direction `dir` (stops at an invalid cell or a height step).
    fn ray_area(&self, t: &mut SpellTarget, x: i32, y: i32, dir: usize, h0: i32, len: i32) {
        let (mut x, mut y) = (x, y);
        for _ in 0..len {
            (x, y) = step(x, y, dir);
            let h = self.map().height(x, y);
            if !self.area_cell_ok(x, y) {
                return;
            }
            if !((h <= h0 && h0 - h <= 3) || (h0 < h && h - h0 <= 2)) {
                return;
            }
            t.area.push((x, y));
        }
    }

    /// Port of FUN_00403ce9: add (x, y) when valid and within 10 height levels.
    fn cone_push(&self, t: &mut SpellTarget, x: i32, y: i32, h0: i32) -> bool {
        let h = self.map().height(x, y);
        if self.area_cell_ok(x, y) && (h0 - h).abs() < 0xb {
            t.area.push((x, y));
            true
        } else {
            false
        }
    }

    /// Port of FUN_00403a18: the cone in direction `dir`: step `s` adds the
    /// centre cell and `s` cells to each side; it ends after the first step
    /// whose centre is invalid. Records start and length in `t.cone[dir]`.
    fn cone_dir(&self, t: &mut SpellTarget, x: i32, y: i32, dir: usize, h0: i32, len: i32) {
        t.cone[dir].0 = t.area.len() as u8;
        let (mut x, mut y) = (x, y);
        for s in 0..len {
            (x, y) = step(x, y, dir);
            let stop = !self.cone_push(t, x, y, h0);
            let (mut ax, mut ay) = (x, y);
            let (mut bx, mut by) = (x, y);
            for _ in 0..s {
                let a_odd = ay & 1 != 0;
                (ax, ay) = match dir {
                    0 => (if a_odd { ax + 1 } else { ax }, ay + 1),
                    1 => (if a_odd { ax + 1 } else { ax }, ay - 1),
                    2 => (if a_odd { ax } else { ax - 1 }, ay - 1),
                    _ => (if a_odd { ax } else { ax - 1 }, ay + 1),
                };
                self.cone_push(t, ax, ay, h0);
                let b_odd = by & 1 != 0;
                (bx, by) = match dir {
                    0 => (if b_odd { bx } else { bx - 1 }, by - 1),
                    1 => (if b_odd { bx } else { bx - 1 }, by + 1),
                    2 => (if b_odd { bx + 1 } else { bx }, by + 1),
                    _ => (if b_odd { bx + 1 } else { bx }, by - 1),
                };
                self.cone_push(t, bx, by, h0);
            }
            if stop {
                break;
            }
        }
        t.cone[dir].1 = (t.area.len() as u8).wrapping_sub(t.cone[dir].0);
    }

    /// Port of FUN_0040391b: the cursor cell, then the four cones.
    fn cone_area(&self, t: &mut SpellTarget, cx: i32, cy: i32) {
        t.area.push((cx, cy));
        let h = self.map().height(cx, cy);
        for d in 0..4 {
            self.cone_dir(t, cx, cy, d, h, t.aim.range);
        }
    }

    /// Port of FUN_004036f2: every valid cell inside the screen window
    /// (rows `scroll_y ..` in pairs, 9 or 10 columns per row parity).
    fn screen_area(&self, t: &mut SpellTarget) {
        let m = self.map();
        let rows = m.rows();
        let (sx, sy) = (m.scroll_x, m.scroll_y);
        let mut c = sx >> 1;
        let (start_a, n_a, start_b, n_b);
        if (sx & 1 == 0 || sy & 1 == 0) && (sx & 1 != 0 || sy & 1 != 0) {
            if sx & 1 != 0 {
                c += 1;
            }
            start_a = c;
            n_a = 9;
            start_b = if sy & 1 == 0 { c - 1 } else { c };
            n_b = 10;
        } else {
            start_a = c;
            n_a = 10;
            start_b = if sy & 1 != 0 { c + 1 } else { c };
            n_b = 9;
        }
        let mut row = sy;
        while row < sy + 0x24 && row <= rows {
            for x in start_a..start_a + n_a {
                if m.on_screen(x, row, 0) && self.valid_target_cell(x, row) {
                    t.area.push((x, row));
                }
            }
            for x in start_b..start_b + n_b {
                if m.on_screen(x, row + 1, 0) && self.valid_target_cell(x, row + 1) {
                    t.area.push((x, row + 1));
                }
            }
            row += 2;
        }
    }

    /// Port of FUN_00402dc9: the first free neighbour of the caster (top
    /// then bottom, left then right) where the summoned class can stand
    /// within 4 height levels. The class row is the summon slot's class id
    /// used as a 0-based index, as in the original.
    fn summon_area(&self, t: &mut SpellTarget) {
        let k = t.aim.effect as usize - 0x15;
        let kind = self.army(t.side).summons.get(k).map_or(0, |s| s.kind as i32);
        let c = self.unit(t.caster);
        let (x, y) = (c.x, c.y);
        let h0 = self.map().height(x, y);
        let l = if y & 1 == 0 { x - 1 } else { x };
        let ok = |cx: i32, cy: i32| {
            self.map().targetable(cx, cy)
                && self.unit_at_cell(t.side, cx, cy, false).is_none()
                && self.unit_at_cell(t.side.other(), cx, cy, false).is_none()
                && self.valid_target_cell(cx, cy)
                && self.can_stand(cx, cy, kind)
                && (h0 - self.map().height(cx, cy)).abs() < 5
        };
        for cx in l..=l + 1 {
            for cy in [y - 1, y + 1] {
                if ok(cx, cy) {
                    t.area.push((cx, cy));
                    return;
                }
            }
        }
    }

    /// Port of FUN_0040284d: teleport destinations — free cells within
    /// `clamp(魔力cur/10 + 1, 1, 10)` rings of the chosen unit where it can
    /// stand, within 9 height levels of its cell.
    fn teleport_area(&self, t: &mut SpellTarget) {
        let n = (self.unit(t.caster).mag_cur as i32 / 10 + 1).clamp(1, 10);
        t.area.clear();
        let (tx, ty) = t.target_cell;
        let who = self.unit_at_cell(t.side, tx, ty, false).or_else(|| self.unit_at_cell(t.side.other(), tx, ty, false));
        // No unit there: the original stops with error 5.
        if let Some(w) = who {
            let class0 = self.unit(w).kind as i32 - 1;
            let h0 = self.map().height(tx, ty);
            for (x, y) in ring_cells(tx, ty, n) {
                if self.map().targetable(x, y)
                    && self.can_stand(x, y, class0)
                    && (h0 - self.map().height(x, y)).abs() < 10
                    && self.unit_at_cell(t.side, x, y, false).is_none()
                    && self.unit_at_cell(t.side.other(), x, y, false).is_none()
                    && self.valid_target_cell(x, y)
                {
                    t.area.push((x, y));
                }
            }
        }
        t.ready = 0;
    }

    /// Port of FUN_00401573: a visible enemy on a range cell other than the caster's.
    pub fn range_has_foe(&self, t: &SpellTarget) -> bool {
        t.range.iter().skip(1).any(|&(x, y)| self.unit_at_cell(t.side.other(), x, y, true).is_some())
    }

    /// Port of FUN_004015d4: an own unit on a range cell other than the caster's.
    pub fn range_has_own(&self, t: &SpellTarget) -> bool {
        t.range.iter().skip(1).any(|&(x, y)| self.unit_at_cell(t.side, x, y, false).is_some())
    }

    /// Port of FUN_00403d91: a visible enemy in the area.
    pub fn area_has_foe(&self, t: &SpellTarget) -> bool {
        t.area.iter().any(|&(x, y)| self.unit_at_cell(t.side.other(), x, y, true).is_some())
    }

    /// Port of FUN_00403df2: an own unit in the area.
    pub fn area_has_own(&self, t: &SpellTarget) -> bool {
        t.area.iter().any(|&(x, y)| self.unit_at_cell(t.side, x, y, false).is_some())
    }

    /// Port of FUN_004051b3: the next area cell (from `t.iter`, wrapping)
    /// holding a visible enemy; `t.iter` moves past it.
    pub fn next_foe_in_area(&self, t: &mut SpellTarget) -> Option<(i32, i32)> {
        let n = t.area.len();
        let hit = |k: usize| {
            let (x, y) = t.area[k];
            self.unit_at_cell(t.side.other(), x, y, true).map(|_| (x, y))
        };
        let found = (t.iter..n).find_map(|k| hit(k).map(|c| (k, c))).or_else(|| (0..t.iter.min(n)).find_map(|k| hit(k).map(|c| (k, c))));
        let (k, c) = found?;
        t.iter = if k + 1 < n { k + 1 } else { 0 };
        Some(c)
    }

    /// Port of FUN_00401a4b: the unit under the cursor the info panel shows
    /// (an own unit for items; otherwise one the spell may target).
    pub fn cursor_unit(&self, t: &SpellTarget) -> Option<UnitRef> {
        let (x, y) = self.cursor;
        if t.mode == 2 {
            return self.unit_at_cell(t.side, x, y, false);
        }
        match self.unit_at_cell(t.side, x, y, false) {
            Some(u) => match t.aim.target {
                1 | 3 => Some(u),
                0 if u == t.caster => Some(u),
                _ => None,
            },
            None => self.unit_at_cell(t.side.other(), x, y, true).filter(|_| matches!(t.aim.target, 2 | 3)),
        }
    }

    /// The checks FUN_004041d7 makes on the aimed cell (`Battle::cursor`)
    /// before its confirm loop; builds the area. The loop itself (Enter /
    /// pointer / direction keys, auto-confirm while `DAT_0049464c == 0`) is
    /// the UI's, followed by [`Battle::finish_aim`] when confirmed.
    pub fn check_cast_cell(&mut self, t: &mut SpellTarget) -> CellCheck {
        let (cx, cy) = self.cursor;
        let a = t.aim;
        if !t.in_range_or_area(cx, cy) {
            return CellCheck::OutOfRange;
        }
        if a.effect == 4 && t.mode != 2 {
            t.target_cell = (cx, cy);
            if self.unit_at_cell(t.side, cx, cy, false).is_none() && self.unit_at_cell(t.side.other(), cx, cy, true).is_none() {
                return CellCheck::OutOfRange;
            }
            self.target_area(t);
            if t.area.is_empty() {
                return CellCheck::OutOfRange;
            }
            return CellCheck::PickDestination;
        }
        self.target_area(t);
        if t.mode == 2 {
            if !self.area_has_own(t) {
                t.ready = -1;
                return CellCheck::OutOfRange;
            }
            return CellCheck::Confirm;
        }
        let shape = a.shape;
        if (shape == 5 || shape == 8) && !self.area_has_foe(t) {
            return CellCheck::NoTarget;
        }
        let any_side = matches!(a.effect, 0x23 | 0x3a | 0x28 | 0x2f);
        if (any_side && !self.area_has_foe(t) && !self.area_has_own(t)) || (!any_side && matches!(a.target, 2 | 3) && !self.area_has_foe(t)) {
            t.ready = -1;
            return CellCheck::NoTarget;
        }
        let summon = (0x15..0x20).contains(&a.effect);
        if a.target == 1 && !summon && !self.area_has_own(t) {
            t.ready = -1;
            return CellCheck::OutOfRange;
        }
        if summon && t.area.is_empty() {
            t.ready = -1;
            return CellCheck::NoRoom;
        }
        CellCheck::Confirm
    }

    /// FUN_004041d7 after a confirmed line or cone aim (not for items): the
    /// cursor goes to the caster cell, shape 5 rebuilds the chosen ray
    /// (`t.dir`), and the caster turns that way.
    pub fn finish_aim(&mut self, t: &mut SpellTarget) {
        let shape = t.aim.shape;
        if t.mode == 2 || (shape != 5 && shape != 8) || t.area.is_empty() {
            return;
        }
        self.cursor = t.area[0];
        let (cx, cy) = self.cursor;
        if shape == 5 {
            t.area.truncate(1);
            let h = self.map().height(cx, cy);
            self.ray_area(t, cx, cy, t.dir & 3, h, t.aim.range);
        }
        self.unit_mut(t.caster).facing = t.dir as u8;
    }

    /// Port of FUN_004055a4 (logic; the range, area and panel drawing and
    /// its pauses are the view's): build the area of an AI cast at (x, y)
    /// and, for rays and cones, pick direction `dir` and face it.
    pub fn aim_ai_cast(&mut self, t: &mut SpellTarget, x: i32, y: i32, dir: i32) {
        // FUN_00426267 / FUN_00426b87: scroll to the cell (view).
        self.cursor = (x, y);
        self.target_area(t);
        match t.aim.shape {
            5 => {
                self.cursor = t.area.first().copied().unwrap_or((x, y));
                let (cx, cy) = self.cursor;
                t.area.truncate(1);
                let d = if (0..=3).contains(&dir) { dir as usize } else { 0 };
                let h = self.map().height(cx, cy);
                self.ray_area(t, cx, cy, d, h, t.aim.range);
                t.dir = d;
                self.unit_mut(t.caster).facing = d as u8;
            }
            8 => {
                self.cursor = t.area.first().copied().unwrap_or((x, y));
                t.dir = dir as usize & 3;
                self.unit_mut(t.caster).facing = dir as u8;
            }
            _ => {}
        }
    }

    /// Port of FUN_0040684c: HP (effect 0x21), MP (effects 6, 10, 0x22) or
    /// both (0x23, uncapped) item `item` would restore on `r`, capped by
    /// what is missing.
    pub fn item_amount(&self, item: u16, r: UnitRef) -> i32 {
        let Some(it) = self.item(item) else { return 0 };
        let (kind, mut v) = match it.effect {
            6 => (2, 0x3c),
            10 => (2, 0x1e),
            0x21 => (1, it.attack as i32),
            0x22 => (2, it.attack as i32),
            0x23 => (3, it.attack as i32),
            _ => (0, 0),
        };
        let u = self.unit(r);
        if kind == 1 {
            v = v.min(u.hp_max as i32 - u.hp as i32);
        } else if kind == 2 {
            v = v.min(u.mp_max as i32 - u.mp as i32);
        }
        v
    }

    // ---- AI scoring (docs/ai.md §3.2) ----------------------------------------------

    /// FUN_00467f96(.., 0): expected (chance, amount) of `cast` on `r`.
    fn spell_expect(&mut self, cast: &Cast, r: UnitRef, side: Side) -> (i32, i32) {
        let (c, v, _) = self.spell_roll(cast, r, side, false);
        (c, v)
    }

    /// Port of FUN_0040576b: score `cast` (whose range is in `t`) and offer
    /// it to `best`; on acceptance the aimed cell and direction go to `plan`
    /// (the caller records the spell).
    pub fn ai_score_spell(&mut self, t: &mut SpellTarget, cast: &Cast, best: &mut Best, plan: &mut SpellPlan) -> bool {
        let a = t.aim;
        if a.shape == 1 && matches!(a.target, 2 | 3) && !self.range_has_foe(t) {
            return false;
        }
        let saved = self.cursor;
        let (mut cell, mut dir, mut score, mut kills) = ((0, 0), 0, 0, 0);
        let kind = match a.effect {
            1 | 3 | 0x24 | 0x29 | 0x2a | 0x30 | 0x40 => {
                if a.shape == 5 || a.shape == 8 {
                    self.score_line(t, cast, &mut cell, &mut score, &mut kills, &mut dir)
                } else {
                    self.score_area(t, cast, &mut cell, &mut score, &mut kills)
                }
            }
            0x12 | 0x31 | 0x5a => self.score_heal(t, cast, &mut cell, &mut score),
            4 => 0,
            _ => self.score_support(t, cast, &mut cell, &mut score),
        };
        self.cursor = saved;
        if best.offer(kind, score, kills) {
            plan.cell = cell;
            plan.dir = dir;
            true
        } else {
            false
        }
    }

    /// The ally term of FUN_004059a6 / FUN_00405ebe: `None` rejects the
    /// cell (the spell would kill the ally or leave it under 20%), else the
    /// value to subtract.
    fn ally_penalty(&mut self, cast: &Cast, o: UnitRef, side: Side, vip_mul: i32) -> Option<i32> {
        let (ch, am) = self.spell_expect(cast, o, side);
        let u = self.unit(o);
        let (hp, hpm) = (u.hp as i32, u.hp_max as i32);
        if hp < am || (hpm > 0 && div((hp - am).wrapping_mul(100), hpm) < 0x14) {
            return None;
        }
        let m = div(am.wrapping_mul(100), hp) + 100;
        let mut v = div(ch.wrapping_mul(am).wrapping_mul(m), 100);
        if u.is_vip() {
            v = v.wrapping_mul(vip_mul);
        }
        Some(v)
    }

    /// The cell comparison of FUN_004059a6 / FUN_00405ebe (state 0 none,
    /// 1 a VIP dies, 2 other).
    fn better_cell(state: i32, vk: i32, kills: i32, s: i32, best_k: i32, best_s: i32) -> bool {
        state == 0
            || (state > 1 && vk > 0)
            || (state == 1 && vk > 0 && (best_k < kills || (kills == best_k && best_s < s)))
            || (state == 2 && (best_k < kills || (kills == best_k && best_s < s)))
    }

    /// Port of FUN_004059a6: damage spells with a fixed area, tried on every
    /// range cell. Returns 0, 1 (a VIP dies) or 2.
    fn score_area(&mut self, t: &mut SpellTarget, cast: &Cast, cell: &mut (i32, i32), best_s: &mut i32, best_k: &mut i32) -> i32 {
        let a = t.aim;
        let d = self.difficulty as usize;
        let (vip_mul, kill_mul) = (self.cu(0x100 + d), self.cu(0x105 + d));
        let foe = t.side.other();
        let mut state = 0;
        for k in 0..t.range.len() {
            self.cursor = t.range[k];
            self.target_area(t);
            if !matches!(a.target, 2 | 3) || self.area_has_foe(t) {
                let (mut vk, mut kills, mut s) = (0, 0, 0i32);
                for i in 0..t.area.len() {
                    let (x, y) = t.area[i];
                    match self.unit_at_cell(foe, x, y, true) {
                        None => {
                            if a.target != 3 {
                                continue;
                            }
                            let Some(o) = self.unit_at_cell(t.side, x, y, false) else { continue };
                            if o == t.caster || (a.combo & 7 != 0 && Some(o) == cast.partner) {
                                continue;
                            }
                            match self.ally_penalty(cast, o, t.side, vip_mul) {
                                Some(v) => s = s.wrapping_sub(v),
                                None => {
                                    (vk, kills, s) = (0, 0, 0);
                                    break;
                                }
                            }
                        }
                        Some(e) => {
                            let is_vip = self.unit(e).is_vip();
                            let (ch, mut am) = self.spell_expect(cast, e, foe);
                            let hp = self.unit(e).hp as i32;
                            am = am.min(hp);
                            let mut v = ch.wrapping_mul(am);
                            if is_vip {
                                v = v.wrapping_mul(vip_mul);
                            }
                            if am < hp || ch < 0x33 {
                                if a.effect == 0x29 {
                                    v = 0;
                                }
                            } else {
                                kills += 1;
                                v = v.wrapping_mul(kill_mul);
                                if is_vip {
                                    vk += 1;
                                }
                            }
                            if a.effect == 0x2a {
                                let c = self.unit(t.caster);
                                let room = c.hp_max as i32 - c.hp as i32;
                                if room < am {
                                    am = room;
                                }
                                v = v.wrapping_add(am.wrapping_mul(ch));
                            }
                            s = s.wrapping_add(v);
                        }
                    }
                }
                if s > 0 && Self::better_cell(state, vk, kills, s, *best_k, *best_s) {
                    *best_k = kills;
                    *best_s = s;
                    *cell = self.cursor;
                    state = if vk < 1 { 2 } else { 1 };
                }
            }
            t.free_area();
        }
        state
    }

    /// Port of FUN_00405ebe: rays (shape 5) and cones (shape 8) from the
    /// caster, each direction scored. The first check uses the area at the
    /// cursor as it is on entry, as in the original.
    fn score_line(&mut self, t: &mut SpellTarget, cast: &Cast, cell: &mut (i32, i32), best_s: &mut i32, best_k: &mut i32, best_dir: &mut i32) -> i32 {
        self.target_area(t);
        if !self.area_has_foe(t) {
            t.free_area();
            return 0;
        }
        let a = t.aim;
        let d = self.difficulty as usize;
        let (vip_mul, kill_mul) = (self.cu(0x100 + d), self.cu(0x105 + d));
        let foe = t.side.other();
        let mut state = 0;
        let (x0, y0) = t.range[0];
        for dir in 0..4 {
            t.area.clear();
            let (mut vk, mut kills, mut s) = (0, 0, 0i32);
            let h0 = self.map().height(x0, y0);
            if a.shape == 5 {
                self.ray_area(t, x0, y0, dir, h0, a.range);
            } else {
                self.cone_dir(t, x0, y0, dir, h0, a.range);
            }
            for i in 0..t.area.len() {
                let (x, y) = t.area[i];
                match self.unit_at_cell(foe, x, y, true) {
                    None => {
                        let Some(o) = self.unit_at_cell(t.side, x, y, false) else { continue };
                        if o == t.caster || (a.combo & 7 != 0 && Some(o) == cast.partner) || self.unit(o).hp == 0 {
                            continue;
                        }
                        match self.ally_penalty(cast, o, t.side, vip_mul) {
                            Some(v) => s = s.wrapping_sub(v),
                            None => {
                                (vk, kills, s) = (0, 0, 0);
                                break;
                            }
                        }
                    }
                    Some(e) => {
                        let is_vip = self.unit(e).is_vip();
                        let (ch, mut am) = self.spell_expect(cast, e, foe);
                        let hp = self.unit(e).hp as i32;
                        am = am.min(hp);
                        let mut v = ch.wrapping_mul(am);
                        if is_vip {
                            v = v.wrapping_mul(vip_mul);
                        }
                        if hp <= am && ch > 0x32 {
                            kills += 1;
                            v = v.wrapping_mul(kill_mul);
                            if is_vip {
                                vk += 1;
                            }
                        }
                        s = s.wrapping_add(v);
                    }
                }
            }
            if s > 0 && Self::better_cell(state, vk, kills, s, *best_k, *best_s) {
                *best_k = kills;
                *best_s = s;
                *best_dir = dir as i32;
                state = if vk < 1 { 2 } else { 1 };
            }
        }
        if state > 0 {
            *cell = t.range[0];
        }
        t.free_area();
        state
    }

    /// Port of FUN_004065d3: heals. Every hurt own unit in the area adds
    /// `chance · min(amount, missing) · (missing% + 50) / 100` (×VIP); the
    /// best cell gives type 3.
    fn score_heal(&mut self, t: &mut SpellTarget, cast: &Cast, cell: &mut (i32, i32), best_s: &mut i32) -> i32 {
        let d = self.difficulty as usize;
        let vip_mul = self.cu(0x100 + d);
        let mut state = 0;
        for k in 0..t.range.len() {
            self.cursor = t.range[k];
            self.target_area(t);
            let mut s = 0i32;
            for i in 0..t.area.len() {
                let (x, y) = t.area[i];
                let Some(o) = self.unit_at_cell(t.side, x, y, false) else { continue };
                let (hp, hpm) = (self.unit(o).hp as i32, self.unit(o).hp_max as i32);
                if hpm == hp || hpm < 1 {
                    continue;
                }
                let (ch, mut am) = self.spell_expect(cast, o, t.side);
                let miss = hpm - hp;
                if miss < am {
                    am = miss;
                }
                let pct = div(miss * 100, hpm);
                let mut v = div(ch.wrapping_mul(am).wrapping_mul(pct + 0x32), 100);
                if self.unit(o).is_vip() {
                    v = v.wrapping_mul(vip_mul);
                }
                s = s.wrapping_add(v);
            }
            if s > 0 && (state == 0 || *best_s < s) {
                state = 3;
                *best_s = s;
                *cell = self.cursor;
            }
            t.free_area();
        }
        state
    }

    /// Port of FUN_004063c4: other support and status spells. Enemy-target
    /// spells add each enemy's value (FUN_0046be07); spells on own units
    /// subtract theirs, so the AI never picks them — the original's sign.
    /// The value of a unit the filter rejects is the previous one (the
    /// original's uninitialised local, 0 here at first).
    fn score_support(&mut self, t: &mut SpellTarget, cast: &Cast, cell: &mut (i32, i32), best_s: &mut i32) -> i32 {
        let tk = t.aim.target;
        let foe = t.side.other();
        let mut state = 0;
        let mut value = 0;
        for k in 0..t.range.len() {
            self.cursor = t.range[k];
            self.target_area(t);
            if tk != 2 || self.area_has_foe(t) {
                let (mut s, mut first) = (0i32, 0);
                for i in 0..t.area.len() {
                    let (x, y) = t.area[i];
                    let e = if tk == 2 { self.unit_at_cell(foe, x, y, true) } else { None };
                    if let Some(e) = e {
                        let (kind, v) = self.support_value(cast, foe, e);
                        if kind > 0 {
                            value = v;
                        }
                        if first == 0 {
                            first = kind;
                        }
                        s = s.wrapping_add(value);
                    } else if matches!(tk, 0 | 1 | 3) {
                        if let Some(o) = self.unit_at_cell(t.side, x, y, false) {
                            let (kind, v) = self.support_value(cast, t.side, o);
                            if kind > 0 {
                                value = v;
                            }
                            s = s.wrapping_sub(value);
                        }
                    }
                }
                if s > 0 && ((state == 0 && first > 0) || *best_s < s) {
                    state = first;
                    *best_s = s;
                    *cell = self.cursor;
                }
            }
            t.free_area();
        }
        state
    }

    /// Port of FUN_0046be07: (kind, value) of a support spell on `r` of
    /// army `side`: the kind is SpellRec +0x22 when the filter
    /// (FUN_0046be92) passes, and the value
    /// `SpellRec+0x23 · (rules[0x10d] + 100) / 100 · chance`.
    pub fn support_value(&mut self, cast: &Cast, side: Side, r: UnitRef) -> (i32, i32) {
        let kind = self.support_kind(cast, side, r);
        if kind < 1 {
            return (kind, 0);
        }
        let t = self.t();
        let sp = &t.spells.spells[cast.spell as usize - 1];
        let base = i16::from_le_bytes([sp.extra[1], sp.extra[2]]) as i32;
        let v = base * (self.cu(0x10d) + 100) / 100;
        let (ch, _) = self.spell_expect(cast, r, side);
        (kind, v.wrapping_mul(ch))
    }

    /// Port of FUN_0046be92: is the support spell of `cast` worth casting
    /// on `r` (of army `side`)? Returns SpellRec +0x22 (its AI type) or 0:
    /// not on the wrong side, not when the effect is already there or the
    /// unit is immune, buffs only within 15 cells of an idle enemy, summons
    /// only when none is active.
    fn support_kind(&self, cast: &Cast, side: Side, r: UnitRef) -> i32 {
        let t = self.t();
        let sp = &t.spells.spells[cast.spell as usize - 1];
        if sp.target == 2 && side == cast.side {
            return 0;
        }
        if (sp.target == 1 || sp.target == 0) && side == cast.side.other() {
            return 0;
        }
        let u = self.unit(r);
        let near = super::ai::nearest_idle_distance(self, cast.side.other(), u.x, u.y);
        let far = 0xf < near;
        let mt = self.move_type(u);
        let (b, s, ab) = (u.buffs, u.status, u.abilities);
        let reject = match sp.effect {
            2 | 0x2c => b & 0x3010 != 0 || s & 0x860 != 0 || far,
            5 => b & 0x200 != 0 || s & 0x100000 != 0 || ab & 0x4000 != 0,
            6 | 0x28 => b & 0x206 != 0 || s & 0x2b000 != 0 || far,
            7 => b & 1 != 0 || s & 0x100000 != 0 || ab & 0x4000 != 0,
            8 | 0x2b | 0x3d | 0x43 => b & 0x200 != 0 || s & 0x1ab000 != 0 || ab & 0x4000 != 0,
            9 => b & 0x20 != 0 || far,
            10 => b & 0x240 != 0 || s & 0x2b000 != 0 || far,
            0xc => b & 0x300 != 0 || s & 0x2b000 != 0 || far,
            0xd => b & 0x200 != 0 || s & 0x12b000 != 0 || ab & 0x4000 != 0,
            0xe => b & 0x200 == 0,
            0xf => b & 8 != 0 || s & 0x100000 != 0 || ab & 0x4000 != 0,
            0x10 => b & 0x400 != 0 || s & 0x100000 != 0 || ab & 0x5000 != 0,
            0x11 => {
                let p = |k| self.power(u, 0, k);
                let (p1, p2, p3, p4) = (p(1), p(2), p(3), p(4));
                s & status::SEALED != 0 || p1 < p2 || p1 < p3 || p1 < p4 || s & 0x100000 != 0 || ab & 0x4000 != 0
            }
            0x13 => s & 4 != 0 || far,
            0x14 | 0x2f => {
                (sp.effect == 0x2f && side == cast.side.other()) || b & 0x448b != 0 || s & 0x10ff000 != 0
            }
            // FUN_0046bd93: no summon while one of the army's is out.
            0x15..=0x1f => self.army(cast.side).summons.iter().any(|m| m.active == 1),
            0x21 => {
                b & 0x200 != 0
                    || s & 0x2b010 != 0
                    || ab & 0x80 != 0
                    || self.enemy_adjacent(side, u)
                    || self.army(cast.side.other()).flags & 9 != 0
                    || far
            }
            0x25 => mt != 0 || u.act != 0,
            0x2d => super::ai::nearest_of(self, cast.side.other(), u.x, u.y, 0, 0).is_some(),
            0x2e => b & 0x280 != 0 || s & 0x12b000 != 0 || ab & 0x4000 != 0,
            0x37 => b & 0x200 != 0 || s & 0x13b000 != 0 || ab & 0x4000 != 0,
            0x38 => b & 0x600 != 0 || s & 0x13f000 != 0 || ab & 0x4000 != 0,
            0x39 => b & 0x208 != 0 || s & 0x12f000 != 0 || ab & 0x4000 != 0,
            0x3c => s & 0x18000 == 0 && b & 0x402 == 0,
            0x3e => s & 8 != 0 || far,
            0x3f => b & 0x800 != 0 || far,
            0x41 => b & 0x2000 != 0 || s & 0x860 != 0 || far,
            0x45 => b & 0x200 != 0 || s & 0x16f000 != 0 || ab & 0x4000 != 0,
            0x46 => s & 0x80 != 0 || ab & 0x4000_0000 != 0 || far,
            0x4c => s & 0x200 != 0 || ab & 0x100 != 0 || far,
            0x54 => b & 0x3000 != 0 || s & 0x860 != 0 || far,
            0x55 => b & 0x200 != 0 || s & 0x2b400 != 0 || ab & 0x10000 != 0 || far,
            0x56 => b & 0x2000 != 0 || s & 0x820 != 0 || far,
            0x57 => s & 0x800 != 0 || far,
            _ => true,
        };
        if reject {
            0
        } else {
            sp.extra[0] as i32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::spells::tests::stage1;

    #[test]
    fn ring_order_matches_the_original_loops() {
        // Ring 1 around an even row: top/bottom for x−1..=x, no side cells.
        assert_eq!(ring_cells(5, 10, 1), vec![(4, 9), (4, 11), (5, 9), (5, 11)]);
        // Ring 2 adds the 3-wide rows two rows out and the side cells of row 10.
        let r2 = ring_cells(5, 10, 2);
        assert_eq!(r2.len(), 4 + 8);
        assert_eq!(&r2[4..], &[(4, 8), (4, 12), (5, 8), (5, 12), (6, 8), (6, 12), (4, 10), (6, 10)]);
        // Diamond sizes: 2r(r+1) cells around the centre.
        for r in 1..6 {
            assert_eq!(ring_cells(20, 21, r).len() as i32, 2 * r * (r + 1));
        }
    }

    /// The caster stands somewhere in the middle of the stage 1 map.
    fn caster(b: &Battle) -> UnitRef {
        assert!(!b.army(Side::Enemy).units.is_empty());
        UnitRef::new(Side::Enemy, 0)
    }

    fn by_name(b: &Battle, name: &str) -> i32 {
        b.t().spells.spells.iter().position(|s| s.name == name).expect(name) as i32 + 1
    }

    #[test]
    fn area_shapes_follow_the_spell_tables() {
        let Some(mut b) = stage1() else { return };
        let r = caster(&b);
        let (x, y) = (b.unit(r).x, b.unit(r).y);
        // Every area cell of a big open map: put the cursor in the interior.
        let (w, rows) = (b.map().width(), b.map().rows());
        let (cx, cy) = (w / 2, rows / 2);
        b.cursor = (cx, cy);
        let size = |b: &mut Battle, name: &str| {
            let id = by_name(b, name);
            let mut t = b.spell_target(Side::Enemy, r, id, 0, 0, false);
            b.target_area(&mut t);
            (t.aim.shape, t.area.clone(), t)
        };
        // 治療 shape 1: the cursor cell only.
        let (sh, a, _) = size(&mut b, "治療");
        assert_eq!((sh, a), (1, vec![(cx, cy)]));
        // 祝福術 shape 2: a 3×3 diamond centred on the cursor (rows cy−2..=cy+2).
        let (sh, a, _) = size(&mut b, "祝福術");
        assert_eq!(sh, 2);
        assert!(a.len() <= 9 && a.contains(&(cx, cy)));
        assert!(a.iter().all(|&(_, yy)| (yy - cy).abs() <= 2));
        // (FUN_00472d46 gives 80 for the cell itself.)
        assert!(a.iter().all(|&(xx, yy)| (xx, yy) == (cx, cy) || crate::battle::map::distance(cx, cy, xx, yy) <= 2));
        let mut expect = vec![(cx, cy - 2), (cx, cy - 1), (cx + 1, cy), (cx - 1, cy - 1), (cx, cy), (cx, cy + 1), (cx - 1, cy), (cx - 1, cy + 1), (cx, cy + 2)];
        if cy & 1 != 0 {
            expect = vec![(cx, cy - 2), (cx + 1, cy - 1), (cx + 1, cy), (cx, cy - 1), (cx, cy), (cx + 1, cy + 1), (cx - 1, cy), (cx, cy + 1), (cx, cy + 2)];
        }
        assert_eq!(a.len() == 9, a == expect, "3×3 order: rows run down-right from the top corner");
        // 集體治療 shape 3: 5×5 (≤ 25 cells, ring distance ≤ 4 from the centre).
        let (sh, a, _) = size(&mut b, "集體治療");
        assert_eq!(sh, 3);
        assert!(a.len() <= 25 && a.len() > 9);
        // 氣功波 shape 6: the centre plus four rays of 6 (25 entries incl. pads).
        let (sh, a, _) = size(&mut b, "氣功波");
        assert_eq!(sh, 6);
        assert_eq!(a.len(), 25);
        assert_eq!(a[0], (cx, cy));
        // 邪月氣流 shape 8, range 4: centre + four cones of 1+3+5+7 cells at most.
        let (sh, a, t) = size(&mut b, "邪月氣流");
        assert_eq!(sh, 8);
        assert_eq!(t.aim.range, 4);
        assert_eq!(a[0], (cx, cy));
        let total: usize = t.cone.iter().map(|c| c.1 as usize).sum();
        assert_eq!(total + 1, a.len());
        assert!(t.cone.iter().all(|c| c.1 as usize <= 16));
        // Cone 0 starts right after the centre; cone 1 follows cone 0.
        assert_eq!(t.cone[0].0, 1);
        assert_eq!(t.cone[1].0 as usize, 1 + t.cone[0].1 as usize);
        // Shapes 5/8 aim from the caster only.
        let id = by_name(&b, "閃電光束");
        let mut t = b.spell_target(Side::Enemy, r, id, 0, 0, false);
        let range = t.aim.range;
        b.target_range(&mut t, range);
        assert_eq!(t.range, vec![(x, y)]);
        // A range-5 diamond holds at most 61 cells, the caster's first.
        let id = by_name(&b, "治療");
        let mut t = b.spell_target(Side::Enemy, r, id, 0, 0, false);
        b.target_range(&mut t, 5);
        assert_eq!(t.range[0], (x, y));
        assert!(t.range.len() <= 61);
        assert!(t.range.iter().all(|&(xx, yy)| crate::battle::map::distance(x, y, xx, yy) <= 5 || (xx, yy) == (x, y)));
    }

    #[test]
    fn line_aim_keeps_one_ray() {
        let Some(mut b) = stage1() else { return };
        let r = caster(&b);
        let id = by_name(&b, "閃電光束");
        let mut t = b.spell_target(Side::Enemy, r, id, 0, 2, false);
        let range = t.aim.range;
        b.target_range(&mut t, range);
        let (x, y) = (b.unit(r).x, b.unit(r).y);
        b.aim_ai_cast(&mut t, x, y, 1);
        assert_eq!(t.area[0], (x, y));
        assert!(t.area.len() <= 1 + 7);
        // Down-right steps: every next cell is one row lower.
        for w in t.area.windows(2) {
            assert_eq!(w[1].1, w[0].1 + 1);
        }
        assert_eq!(b.unit(r).facing, 1);
    }
}
