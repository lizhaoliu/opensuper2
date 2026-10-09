//! The battle map object `DAT_00497a68` (`docs/structs.md` "BattleMap").
//!
//! Static cell data comes from `s2data::map::BattleMap`; this adds the
//! runtime parts (scroll position, special/event point lists) and the cell
//! queries the battle code uses, each with the original's own bounds rules.

use s2data::map::{BattleMap as MapData, Passage, TileAttrs};
use s2data::tables::{RuleTables, TerrainEffect};

/// Terrain query result (`FUN_00427e1f` return value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// 0
    Blocked,
    /// 1: may move through and stop.
    Ok,
    /// 2: may move through but not stop.
    PassOnly,
}

/// The four edge-adjacent neighbours in the order used by the movement and
/// targeting code: up-right, down-left, up-left, down-right.
pub fn neighbour(x: i32, y: i32, dir: usize) -> (i32, i32) {
    let odd = y & 1 != 0;
    match dir {
        0 => (if odd { x + 1 } else { x }, y - 1),
        1 => (if odd { x } else { x - 1 }, y + 1),
        2 => (if odd { x } else { x - 1 }, y - 1),
        _ => (if odd { x + 1 } else { x }, y + 1),
    }
}

/// Port of FUN_00472d46: ring distance on the staggered grid (1.., capped at 80).
pub fn distance(x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
    let mut d = 1;
    let mut cx = x0;
    let mut cy = y0;
    loop {
        let lx = if cy & 1 == 0 { cx - 1 } else { cx };
        cx = lx;
        let top = cy - 1;
        if top == y1 || top + d * 2 == y1 {
            for x in lx..=lx + d {
                if x == x1 {
                    return d;
                }
            }
        }
        if lx == x1 || lx + d == x1 {
            let mut y = cy + 1;
            while y < top + d * 2 {
                if y == y1 {
                    return d;
                }
                y += 2;
            }
        }
        d += 1;
        cy = top;
        if d >= 0x50 {
            return d;
        }
    }
}

#[derive(Debug, Clone)]
pub struct Map {
    pub data: MapData,
    pub attrs: TileAttrs,
    /// 0x06c scroll in half cells (visible columns scroll_x/2 .. +10).
    pub scroll_x: i32,
    /// 0x070 scroll in rows (visible rows scroll_y .. +30).
    pub scroll_y: i32,
    /// 0x08d cells whose object class is 0x23 (≤ 20), FUN_00425075.
    pub special: Vec<(i16, i16)>,
    /// Event points (object class 0x0c), in scan order (`DAT_00494598` slots).
    pub event_points: Vec<(i16, i16)>,
}

impl Map {
    /// Load battle map `id` (1-based, PRG entry 6) with the tile attributes (PRG entry 7).
    pub fn load(arcs: &s2data::Archives, id: usize) -> anyhow::Result<Map> {
        let attrs = TileAttrs::parse(&arcs.read(s2data::Kind::Prg, 7)?)?;
        let data = MapData::parse(&arcs.read(s2data::Kind::Prg, 6)?, id, attrs.counts())?;
        Ok(Map::new(data, attrs))
    }

    /// Port of FUN_00425075 (scan part): build the special/event point lists.
    pub fn new(data: MapData, attrs: TileAttrs) -> Map {
        let mut m = Map { data, attrs, scroll_x: 1, scroll_y: 1, special: Vec::new(), event_points: Vec::new() };
        let w = m.data.width;
        for row in 0..m.data.rows() {
            let n = if row & 1 == 0 { w } else { w.saturating_sub(1) };
            for col in 0..n {
                let c = m.data.cells[row * w + col];
                if c.object == 0 {
                    continue;
                }
                match m.object_class_of(c.object) {
                    0x23 if m.special.len() < 20 => m.special.push((col as i16, row as i16)),
                    0x0c => m.event_points.push((col as i16, row as i16)),
                    _ => {}
                }
            }
        }
        m
    }

    pub fn width(&self) -> i32 {
        self.data.width as i32
    }

    /// Number of rows (`2 * h`).
    pub fn rows(&self) -> i32 {
        self.data.rows() as i32
    }

    fn raw(&self, x: i32, y: i32) -> Option<&s2data::map::Cell> {
        if x < 0 || y < 0 {
            return None;
        }
        self.data.cells.get((y * self.width() + x) as usize)
    }

    /// Port of FUN_004255da: cell height, 0 for cells off the map.
    pub fn height(&self, x: i32, y: i32) -> i32 {
        if y < 1 || y >= self.rows() {
            return 0;
        }
        if y & 1 == 0 && (x == 0 || x == self.width() - 1) {
            return 0;
        }
        self.raw(x, y).map_or(0, |c| c.height as i32)
    }

    /// Port of FUN_004257ac: cell used by the movement fill (rows 1..=2h−2).
    pub fn move_cell(&self, x: i32, y: i32) -> Option<&s2data::map::Cell> {
        if y < 1 || y > self.rows() - 2 {
            return None;
        }
        if y & 1 == 0 && (x == 0 || x == self.width() - 1) {
            return None;
        }
        self.raw(x, y)
    }

    /// Port of FUN_00425817: cell lookup for the movement root (rows 0..2h).
    pub fn root_cell(&self, x: i32, y: i32) -> Option<&s2data::map::Cell> {
        if y < 0 || y > self.rows() - 1 {
            return None;
        }
        if y & 1 == 0 && x == self.width() {
            return None;
        }
        self.raw(x, y)
    }

    /// Port of FUN_004261bd: a cell that can be targeted (on the map, not
    /// pushed off the top by its height).
    pub fn targetable(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        if y & 1 == 0 && x < 1 {
            return false;
        }
        if !(x < self.width() - 1 && y < self.rows() - 1) {
            return false;
        }
        let Some(c) = self.raw(x, y) else { return false };
        let mut yy = y;
        if c.height != 0 {
            yy -= c.height as i32 - 1;
        }
        yy >= 1
    }

    /// Object attribute class (`attr >> 8`) of an object tile id.
    fn object_class_of(&self, object: u16) -> u16 {
        (object as usize)
            .checked_sub(1)
            .and_then(|i| self.attrs.banks[1].get(i))
            .map_or(0, |a| a >> 8)
    }

    /// Port of FUN_004256db (>> 8): object class of the cell (0 = none).
    pub fn object_class(&self, x: i32, y: i32) -> u16 {
        self.raw(x, y).map_or(0, |c| if c.object == 0 { 0 } else { self.object_class_of(c.object) })
    }

    /// Port of FUN_00425fbe: the spot (map list at +0x24) at a cell, if any; returns its code.
    pub fn spot_at(&self, x: i32, y: i32) -> Option<u16> {
        self.data.spots.iter().find(|s| s.col as i32 == x && s.row as i32 == y).map(|s| s.flags)
    }

    /// FUN_0042b097's scan: the codes (low byte = treasure spot, 1-based) of
    /// the spots that are not locked (0x100) and lie on an overlay that is
    /// not a door (class 0xc), in list order.
    pub fn chest_spots(&self) -> Vec<u16> {
        self.data
            .spots
            .iter()
            .filter(|s| s.flags & 0x100 == 0)
            .filter(|s| {
                self.raw(s.col as i32, s.row as i32).is_some_and(|c| (c.object as i16) > 0 && self.object_class_of(c.object) != 0xc)
            })
            .map(|s| s.flags)
            .collect()
    }

    /// A spot with code bit 0x8000 blocks movement and targeting.
    pub fn blocked_by_spot(&self, x: i32, y: i32) -> bool {
        self.spot_at(x, y).is_some_and(|f| f & 0x8000 != 0)
    }

    /// Port of FUN_00428221: ground class 1/2 tiles with tile id > 4 (bridges etc.).
    pub fn no_stop_tile(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        let Some(c) = self.raw(x, y) else { return false };
        if c.ground == 0 {
            return false;
        }
        let a = (c.ground as usize).checked_sub(1).and_then(|i| self.attrs.banks[0].get(i)).copied().unwrap_or(0);
        matches!(a >> 8, 1 | 2) && c.ground > 4
    }

    /// Port of FUN_00427df2 / FUN_00427e1f: passability and (cost, atk, def)
    /// for a unit of class `kind` whose move type is `move_type` (`FUN_0043aa26`).
    pub fn terrain(&self, tables: &RuleTables, x: i32, y: i32, kind: i16, move_type: i32) -> (Pass, TerrainEffect) {
        let none = TerrainEffect { cost: 1, attack: 0, defense: 0 };
        let idx = y * self.width() + x;
        if idx < 0 || idx as usize >= self.data.cells.len() {
            return (Pass::Blocked, none);
        }
        let Some(class) = tables.kinds.class(kind as u16) else { return (Pass::Blocked, none) };
        let cell = &self.data.cells[idx as usize];
        let (p, eff) = self.data.passage(&self.attrs, &tables.terrain, kind as u16, class, cell, move_type != 0);
        let p = match p {
            Passage::Blocked => Pass::Blocked,
            Passage::Walk => Pass::Ok,
            Passage::Fly => Pass::PassOnly,
        };
        (p, if p == Pass::Blocked { none } else { eff })
    }

    /// Port of FUN_00427a10: cell → screen pixel (top-left of the 64×32 diamond).
    pub fn cell_to_screen(&self, x: i32, y: i32) -> (i32, i32) {
        let dy = y - self.scroll_y;
        let mut px = (x - (self.scroll_x >> 1)) * 0x40;
        let mut py = dy * 0x10;
        let sx_odd = self.scroll_x & 1 != 0;
        let sy_odd = self.scroll_y & 1 != 0;
        match (sx_odd, sy_odd) {
            (false, false) => {
                if dy & 1 != 0 {
                    px += 0x20;
                }
            }
            (false, true) => {
                if dy & 1 == 0 {
                    px += 0x20;
                }
            }
            (true, false) => {
                if dy & 1 == 0 {
                    px -= 0x20;
                }
            }
            (true, true) => {
                if dy & 1 != 0 {
                    px -= 0x20;
                }
            }
        }
        let h = self.height(x, y);
        if h > 0 {
            py -= (h - 1) * 0x10;
        }
        (px, py)
    }

    /// Port of FUN_0042681e: scroll so that a cell is near the centre.
    pub fn center_on(&mut self, x: i32, y: i32, use_height: bool) {
        let h = if use_height { self.height(x, y) - 1 } else { 0 };
        let mut sx = if y & 1 == 0 { x * 2 - 9 } else { x * 2 - 8 };
        let mut sy = if use_height { y - (h + 0x10) } else { y - 0x10 };
        if self.width() <= (sx + 0x15) >> 1 {
            sx = self.width() * 2 - 0x15;
        }
        if self.rows() <= sy + 0x1e {
            sy = self.rows() - 0x1e;
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

    /// Port of FUN_00426267: cell inside the visible window (`mode` 1 ignores
    /// the last row limit, 2 uses a 2-row top margin).
    pub fn on_screen(&self, x: i32, y: i32, mode: i32) -> bool {
        if y < 1 {
            return false;
        }
        if !(mode == 1 || y < self.rows() - 1) {
            return false;
        }
        if y & 1 == 0 {
            if x == 0 || x == self.width() - 1 {
                return false;
            }
        } else if self.width() - 1 <= x {
            return false;
        }
        let sx = self.scroll_x >> 1;
        if self.scroll_x & 1 == 0 {
            if y & 1 == 0 {
                if x < sx {
                    return false;
                }
            } else if x < sx - 1 {
                return false;
            }
            if sx + 10 <= x {
                return false;
            }
        } else {
            if x < sx {
                return false;
            }
            if y & 1 == 0 {
                if sx + 10 < x {
                    return false;
                }
            } else if sx + 10 <= x {
                return false;
            }
        }
        let mut yy = y;
        let h = self.raw(x, y).map_or(0, |c| c.height as i32);
        if h != 0 {
            yy -= h - 1;
        }
        if mode == 2 {
            !(yy < self.scroll_y + 2 || self.scroll_y + 0x1e <= yy)
        } else {
            !(yy < self.scroll_y - 1 || self.scroll_y + 0x1e <= yy)
        }
    }

    /// Port of FUN_00426fdb: scroll by a map key code (−0x10..−9); returns true if it moved.
    pub fn scroll_key(&mut self, code: i32) -> bool {
        let rows = self.rows();
        let up = |m: &mut Map| -> bool {
            if 2 < m.scroll_y {
                m.scroll_y = if m.scroll_y == 3 { 1 } else { m.scroll_y - 2 };
                true
            } else {
                false
            }
        };
        let down = |m: &mut Map| -> bool {
            if m.scroll_y + 0x1e < rows {
                m.scroll_y = if m.scroll_y + 0x1e < rows - 1 { m.scroll_y + 2 } else { rows - 0x1e };
                true
            } else {
                false
            }
        };
        let left = |m: &mut Map| -> bool {
            if 1 < m.scroll_x {
                m.scroll_x -= 1;
                true
            } else {
                false
            }
        };
        let right = |m: &mut Map| -> bool {
            if (m.scroll_x + 0x15) >> 1 < m.width() {
                m.scroll_x += 1;
                true
            } else {
                false
            }
        };
        match code {
            -0x10 => up(self),
            -0xf => down(self),
            -0xe => left(self),
            -0xd => right(self),
            -0xc => {
                let a = up(self);
                let b = left(self);
                a | b
            }
            -0xb => {
                let a = left(self);
                let b = down(self);
                a | b
            }
            -0xa => {
                let a = right(self);
                let b = up(self);
                a | b
            }
            -9 => {
                let a = right(self);
                let b = down(self);
                a | b
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_and_distance() {
        // even row 10: up-right of (5,10) is (5,9), up-left (4,9)
        assert_eq!(neighbour(5, 10, 0), (5, 9));
        assert_eq!(neighbour(5, 10, 2), (4, 9));
        assert_eq!(neighbour(5, 9, 0), (6, 8));
        assert_eq!(neighbour(5, 9, 3), (6, 10));
        for d in 0..4 {
            let (x, y) = neighbour(5, 10, d);
            assert_eq!(distance(5, 10, x, y), 1, "dir {d}");
            let (x, y) = neighbour(5, 9, d);
            assert_eq!(distance(5, 9, x, y), 1, "odd dir {d}");
        }
        // two steps up-right
        let (x, y) = neighbour(5, 10, 0);
        let (x, y) = neighbour(x, y, 0);
        assert_eq!(distance(5, 10, x, y), 2);
        // straight up (y-2) is also distance... one up-right + one up-left
        assert_eq!(distance(5, 10, 5, 8), 2);
    }
}
