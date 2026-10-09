//! Battle maps (PRG entry 6, "map index") and tile attributes (PRG entry 7, "map attr").
//!
//! Maps are staggered isometric grids: `width` columns by `2 * height` rows of
//! 64x32 diamonds; odd rows are shifted right by 32 px and rows are 16 px apart
//! (`FUN_00429336`). Each cell references a ground tile and optionally an
//! object ("tree") tile by global tile id; a tile's attribute word gives its
//! terrain type and passability class. Tile graphics are in PRG 3/4/5.

use crate::tables::{ClassDef, TerrainEffect, TerrainTable};
use crate::tbl_util::Rec;
use crate::{format_err, le, Result};

/// Global tile counts of the three tile banks (ground, object, large).
pub const BANKS: usize = 3;

/// PRG entry 7: per-tile attribute words for each bank.
#[derive(Debug, Clone)]
pub struct TileAttrs {
    /// `banks[b][tile_id - 1]` = `(passability << 8) | terrain` (terrain 1-based).
    pub banks: [Vec<u16>; BANKS],
}

impl TileAttrs {
    pub fn parse(b: &[u8]) -> Result<TileAttrs> {
        let counts = [le::u16(b, 0)? as usize, le::u16(b, 2)? as usize, le::u16(b, 4)? as usize];
        let mut p = 6;
        let mut banks: [Vec<u16>; BANKS] = Default::default();
        for (bank, &n) in banks.iter_mut().zip(&counts) {
            *bank = (0..n).map(|i| le::u16(b, p + 2 * i)).collect::<Result<_>>()?;
            p += 2 * n;
        }
        Ok(TileAttrs { banks })
    }

    pub fn counts(&self) -> [usize; BANKS] {
        [self.banks[0].len(), self.banks[1].len(), self.banks[2].len()]
    }

    fn attr(&self, bank: usize, id: u16) -> u16 {
        (id as usize).checked_sub(1).and_then(|i| self.banks[bank].get(i)).copied().unwrap_or(0)
    }
}

/// One map cell (5 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    /// Height level; height differences modify hit and damage.
    pub height: u8,
    /// Global ground tile id (0 = void).
    pub ground: u16,
    /// Global object tile id (0 = none).
    pub object: u16,
}

/// A large object placed on the map (10 bytes): drawn from the large-tile bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapObject {
    pub col: i16,
    pub row: i16,
    /// Pixel size of the object image.
    pub w: i16,
    pub h: i16,
    /// Global large-tile id.
    pub tile: i16,
}

/// A search/treasure spot (6 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub col: i16,
    pub row: i16,
    /// Low byte: treasure index (1-based into the stage's treasure reward list,
    /// 0 = none); 0x100: already taken; 0x8000: set on non-treasure spots (meaning TBD).
    pub flags: u16,
}

/// Which local tiles a map uses from one bank.
#[derive(Debug, Clone)]
pub struct TileUsage {
    /// `remap[global_id - 1]` = local slot (1-based) or 0 if unused.
    pub remap: Vec<i16>,
    /// (offset, size) of each local slot's graphics in the bank buffer.
    pub entries: Vec<(u32, u32)>,
}

/// How a unit may use a cell (return value of `FUN_00427e1f`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passage {
    Blocked,
    /// Normal movement and standing.
    Walk,
    /// Reachable only by flying/levitating movement (e.g. over water or walls).
    Fly,
}

#[derive(Debug, Clone)]
pub struct BattleMap {
    pub width: usize,
    /// Half the number of rows (the grid has `2 * height` staggered rows).
    pub height: usize,
    pub tile_w: i16,
    pub tile_h: i16,
    pub row_step: i16,
    /// Header shorts at 0x10..0x1c (camera/cursor start and offsets; meaning TBD).
    pub extra: [i16; 6],
    pub cells: Vec<Cell>,
    pub objects: Vec<MapObject>,
    pub spots: Vec<Spot>,
    /// Tile usage for the ground, object and large banks.
    pub usage: [TileUsage; BANKS],
}

/// PRG entry 6: `u16 count; (u32 offset, u32 size)[count]; maps...`
pub fn map_count(prg6: &[u8]) -> Result<usize> {
    Ok(le::u16(prg6, 0)? as usize)
}

impl BattleMap {
    /// Parse map `id` (1-based) from the whole PRG entry 6. `bank_sizes` are
    /// the global tile counts (from `TileAttrs::counts`).
    pub fn parse(prg6: &[u8], id: usize, bank_sizes: [usize; BANKS]) -> Result<BattleMap> {
        const W: &str = "map";
        let n = map_count(prg6)?;
        if id == 0 || id > n {
            return Err(format_err(W, format!("map {id} out of range 1..={n}")));
        }
        let off = le::u32(prg6, 2 + 8 * (id - 1))? as usize;
        let size = le::u32(prg6, 6 + 8 * (id - 1))? as usize;
        let m = Rec::at(prg6, off, size, W)?.b;
        let h = Rec::new(m, W);
        let width = h.i16(0)?.max(0) as usize;
        let height = h.i16(2)?.max(0) as usize;
        let n_cells = h.i16(4)?.max(0) as usize;
        if n_cells != width * height * 2 {
            return Err(format_err(W, format!("map {id}: {n_cells} cells for {width}x{height}")));
        }
        let n_obj = h.i16(0x0c)?.max(0) as usize;
        let n_spots = h.i16(0x0e)?.max(0) as usize;
        let mut extra = [0i16; 6];
        for (k, v) in extra.iter_mut().enumerate() {
            *v = h.i16(0x10 + 2 * k)?;
        }
        let mut p = 28;
        let cells = (0..n_cells)
            .map(|i| {
                let r = Rec::at(m, p + 5 * i, 5, W)?;
                Ok(Cell { height: r.u8(0)?, ground: r.u16(1)?, object: r.u16(3)? })
            })
            .collect::<Result<_>>()?;
        p += 5 * n_cells;
        let objects = (0..n_obj)
            .map(|i| {
                let r = Rec::at(m, p + 10 * i, 10, W)?;
                Ok(MapObject { col: r.i16(0)?, row: r.i16(2)?, w: r.i16(4)?, h: r.i16(6)?, tile: r.i16(8)? })
            })
            .collect::<Result<_>>()?;
        p += 10 * n_obj;
        let spots = (0..n_spots)
            .map(|i| {
                let r = Rec::at(m, p + 6 * i, 6, W)?;
                Ok(Spot { col: r.i16(0)?, row: r.i16(2)?, flags: r.u16(4)? })
            })
            .collect::<Result<_>>()?;
        p += 6 * n_spots;
        let mut usage: [TileUsage; BANKS] = std::array::from_fn(|_| TileUsage { remap: Vec::new(), entries: Vec::new() });
        for (u, &globals) in usage.iter_mut().zip(&bank_sizes) {
            let used = le::u16(m, p)? as usize;
            p += 2;
            u.remap = (0..globals).map(|i| le::i16(m, p + 2 * i)).collect::<Result<_>>()?;
            p += 2 * globals;
            u.entries = (0..used).map(|i| Ok((le::u32(m, p + 8 * i)?, le::u32(m, p + 8 * i + 4)?))).collect::<Result<_>>()?;
            p += 8 * used;
        }
        if p != m.len() {
            return Err(format_err(W, format!("map {id}: parsed {p} of {} bytes", m.len())));
        }
        Ok(BattleMap {
            width,
            height,
            tile_w: h.i16(6)?,
            tile_h: h.i16(8)?,
            row_step: h.i16(10)?,
            extra,
            cells,
            objects,
            spots,
            usage,
        })
    }

    pub fn rows(&self) -> usize {
        self.height * 2
    }

    /// Cell at (col, row) honouring the game's edge rules (`FUN_004255da`):
    /// row 0 and the last row are off-map, and on even rows the first and last
    /// columns are off-map.
    pub fn cell(&self, col: usize, row: usize) -> Option<&Cell> {
        if row < 1 || row >= self.rows() || col >= self.width {
            return None;
        }
        if row.is_multiple_of(2) && (col == 0 || col == self.width - 1) {
            return None;
        }
        self.cells.get(row * self.width + col)
    }

    /// Top-left pixel of a cell's 64x32 diamond (before height offset).
    pub fn cell_origin(&self, col: usize, row: usize) -> (i32, i32) {
        let x = col as i32 * self.tile_w as i32 + if row % 2 == 1 { self.tile_w as i32 / 2 } else { 0 };
        (x, row as i32 * self.row_step as i32)
    }

    /// Terrain indices (into `TerrainTable::names`) of a cell's ground and object.
    pub fn terrain_of(&self, attrs: &TileAttrs, terrain: &TerrainTable, cell: &Cell) -> (Option<usize>, Option<usize>) {
        let g = attrs.attr(0, cell.ground);
        let ground = (cell.ground != 0 && g != 0).then(|| ((g & 0xff) as usize).saturating_sub(1));
        let o = attrs.attr(1, cell.object);
        let object = (cell.object != 0 && (o & 0xff) != 0 && (o & 0xff) as usize <= terrain.objects)
            .then(|| terrain.ground + (o & 0xff) as usize - 1);
        (ground, object)
    }

    /// Movement/terrain evaluation of `FUN_00427e1f` for a class at a cell.
    /// `levitating` is the result of the unit check `FUN_0043aa26` (flying item/spell).
    /// Returns the passage kind and the summed terrain effect (cost, atk%, def%).
    pub fn passage(
        &self,
        attrs: &TileAttrs,
        terrain: &TerrainTable,
        class_id: u16,
        class: &ClassDef,
        cell: &Cell,
        levitating: bool,
    ) -> (Passage, TerrainEffect) {
        let none = TerrainEffect { cost: 1, attack: 0, defense: 0 };
        let g = attrs.attr(0, cell.ground);
        if cell.ground == 0 || g == 0 {
            return (Passage::Blocked, none);
        }
        let g_pass = g >> 8;
        let g_ter = ((g & 0xff) as usize).saturating_sub(1);
        let Some(mut eff) = terrain.effect(class_id, g_ter) else { return (Passage::Blocked, none) };
        if g_pass == 5 || eff.cost == -1 {
            return (Passage::Blocked, eff);
        }
        let lev = |p: Passage| if levitating { p } else { Passage::Blocked };
        let mut passage = match class.move_type {
            1..=5 | 10 => {
                if matches!(g_pass, 0 | 1 | 2 | 4) { Passage::Walk } else { lev(Passage::Fly) }
            }
            6..=8 => {
                if matches!(g_pass, 2 | 3) { Passage::Fly } else { Passage::Walk }
            }
            9 => {
                if matches!(g_pass, 1 | 2) { Passage::Walk } else { lev(Passage::Fly) }
            }
            _ => Passage::Blocked,
        };
        if passage == Passage::Blocked {
            return (passage, eff);
        }
        if cell.object != 0 {
            let o = attrs.attr(1, cell.object);
            let o_pass = o >> 8;
            let o_ter = (o & 0xff) as usize;
            if o_ter > terrain.objects {
                return (Passage::Blocked, eff);
            }
            let idx = (o_ter as isize - 1 + terrain.ground as isize).max(0) as usize;
            // FUN_00427e1f indexes the object's effect without the class row
            // (`o_ter - 1 + ground`): every class gets class 1's object effects.
            let Some(oe) = terrain.effect(1, idx) else { return (Passage::Blocked, eff) };
            if o_pass == 1 || oe.cost == -1 {
                return (Passage::Blocked, eff);
            }
            if o_pass == 2 {
                passage = Passage::Fly;
            } else if o_pass == 10 {
                if !levitating {
                    return (Passage::Blocked, eff);
                }
                passage = Passage::Fly;
            }
            eff = TerrainEffect {
                cost: eff.cost.wrapping_add(oe.cost),
                attack: eff.attack.wrapping_add(oe.attack),
                defense: eff.defense.wrapping_add(oe.defense),
            };
        }
        (passage, eff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic() -> (Vec<u8>, [usize; BANKS]) {
        // 2 x 1 map => 4 cells, 1 object, 1 spot, banks of sizes 2/1/1 with one used tile each.
        let mut m = Vec::new();
        for v in [2i16, 1, 4, 64, 32, 16, 1, 1, 0, 0, 0, 0, 100, 300] {
            m.extend_from_slice(&v.to_le_bytes());
        }
        for i in 0..4u8 {
            m.extend_from_slice(&[i, 1, 0, 0, 0]);
        }
        for v in [1i16, 1, 64, 96, 1] {
            m.extend_from_slice(&v.to_le_bytes());
        }
        for v in [1i16, 1, 3] {
            m.extend_from_slice(&v.to_le_bytes());
        }
        let sizes = [2, 1, 1];
        for &n in &sizes {
            m.extend_from_slice(&1u16.to_le_bytes());
            for k in 0..n {
                m.extend_from_slice(&((k == 0) as i16).to_le_bytes());
            }
            m.extend_from_slice(&0u32.to_le_bytes());
            m.extend_from_slice(&100u32.to_le_bytes());
        }
        let mut prg6 = Vec::new();
        prg6.extend_from_slice(&1u16.to_le_bytes());
        prg6.extend_from_slice(&10u32.to_le_bytes());
        prg6.extend_from_slice(&(m.len() as u32).to_le_bytes());
        prg6.extend_from_slice(&m);
        (prg6, sizes)
    }

    #[test]
    fn parses_synthetic_map() {
        let (prg6, sizes) = synthetic();
        let map = BattleMap::parse(&prg6, 1, sizes).unwrap();
        assert_eq!((map.width, map.rows()), (2, 2));
        assert_eq!(map.cells[3].height, 3);
        assert_eq!(map.objects[0].tile, 1);
        assert_eq!(map.spots[0].flags, 3);
        assert_eq!(map.usage[0].remap, vec![1, 0]);
        assert_eq!(map.usage[2].entries, vec![(0, 100)]);
        // row 0 is off-map; row 1 col 1 is fine
        assert!(map.cell(0, 0).is_none());
        assert_eq!(map.cell(1, 1).unwrap().height, 3);
        assert_eq!(map.cell_origin(1, 1), (96, 16));
    }

    #[test]
    fn tile_attrs() {
        let mut b = Vec::new();
        for v in [2u16, 1, 0, 0x0102, 0x0503, 0x0201] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let a = TileAttrs::parse(&b).unwrap();
        assert_eq!(a.counts(), [2, 1, 0]);
        assert_eq!(a.attr(0, 2), 0x0503);
        assert_eq!(a.attr(1, 1), 0x0201);
        assert_eq!(a.attr(0, 0), 0);
    }
}
