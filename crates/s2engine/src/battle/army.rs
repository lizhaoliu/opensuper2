//! Armies `DAT_0049f9f8` (player) and `DAT_004976c8` (enemy), `docs/structs.md` "Army".
//!
//! The original holds up to 50 unit pointers; here units live in a `Vec` and
//! are addressed by [`UnitRef`] (side + 0-based index, i.e. `slot - 1`).
//! Sprite/portrait tables (0x0e8.., 0x27e, 0x2ad.., 0x399) belong to the view.

use super::unit::{flag, Unit};

/// Which army. `Player` is side 0 (`army+0xda == 0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Player = 0,
    Enemy = 1,
}

impl Side {
    /// `army+0x282`: the opposing army (also FUN_00472d2c).
    pub fn other(self) -> Side {
        match self {
            Side::Player => Side::Enemy,
            Side::Enemy => Side::Player,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A unit by army and 0-based index (the original passes `Unit*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitRef {
    pub side: Side,
    pub idx: usize,
}

impl UnitRef {
    pub fn new(side: Side, idx: usize) -> UnitRef {
        UnitRef { side, idx }
    }
}

/// `Army.flags` (0x0de) bits aggregated by FUN_0043abc2.
pub mod aflag {
    /// Invisible units of the opponent are revealed (set by spells, cleared at upkeep).
    pub const REVEAL: u32 = 0x01;
    /// No critical hits against this army.
    pub const NO_CRIT_TAKEN: u32 = 0x02;
    pub const ITEM_3A: u32 = 0x04;
    pub const ITEM_1F: u32 = 0x08;
    pub const ABILITY_20000: u32 = 0x10;
    /// NPC allies become player-controllable.
    pub const NPC_CONTROL: u32 = 0x20;
    /// Treasure search after victory.
    pub const TREASURE: u32 = 0x40;
}

/// One army summon slot (`army+0x286`, 3 bytes each).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SummonSlot {
    pub active: u8,
    pub kind: i16,
}

/// An army.
#[derive(Debug, Clone)]
pub struct Army {
    /// 0x000/0x004 units (≤ 50).
    pub units: Vec<Unit>,
    /// 0x0cc gold.
    pub gold: i32,
    /// 0x0d0 inventory: count per item id-1 (≤ 99).
    pub inventory: Vec<u8>,
    /// 0x0d4 (saved, meaning unknown).
    pub _d4: i16,
    /// 0x0d6 (saved, meaning unknown).
    pub _d6: i32,
    /// 0x0da side.
    pub side: Side,
    /// 0x0de see [`aflag`].
    pub flags: u32,
    /// 0x0e2 % bonus applied to the stats of units created in this army (FUN_00457d3c).
    pub stat_bonus_pct: i32,
    /// 0x286 summon slots.
    pub summons: [SummonSlot; 11],
}

pub const MAX_UNITS: usize = 50;

/// Port of FUN_0043d428: the 11 summon slots, creature classes 0x71..=0x7b, inactive.
pub fn summon_slots() -> [SummonSlot; 11] {
    std::array::from_fn(|k| SummonSlot { active: 0, kind: 0x71 + k as i16 })
}

impl Army {
    pub fn new(side: Side, n_items: usize) -> Army {
        Army {
            units: Vec::new(),
            gold: 0,
            inventory: vec![0; n_items],
            _d4: 0,
            _d6: 0,
            side,
            flags: 0,
            stat_bonus_pct: 0,
            summons: summon_slots(),
        }
    }

    /// Port of FUN_0043deeb (minus view resources): remove every unit and reset.
    pub fn clear(&mut self) {
        self.units.clear();
        self.gold = 0;
        self.summons = summon_slots();
        self.inventory.fill(0);
        self.stat_bonus_pct = 0;
    }

    /// Port of FUN_00440b16: 1-based id of the deployed unit at (x, y), 0 if none.
    /// Dead units count only while they still occupy the cell (`CORPSE`).
    /// With `visible_only`, units hidden from the opponent are skipped (the
    /// caller supplies that test because it needs the opponent army).
    pub fn unit_at(&self, x: i32, y: i32) -> usize {
        for (i, u) in self.units.iter().enumerate() {
            if u.flags & flag::DEPLOYED != 0
                && u.x == x
                && u.y == y
                && (u.flags & flag::DEAD == 0 || u.flags & flag::CORPSE != 0)
            {
                return i + 1;
            }
        }
        0
    }

    /// Port of FUN_00457578: unit by character id.
    /// mode 0 deployed, 1 any alive, 2 any incl. dead, 3 not dead/reserve, 4 any.
    pub fn find_char(&self, char_id: i16, mode: i32) -> Option<usize> {
        self.units.iter().position(|u| match mode {
            3 => u.char_id == char_id && u.flags & (flag::DEAD | flag::RESERVE) == 0,
            4 => u.char_id == char_id,
            _ => {
                u.char_id == char_id
                    && (u.flags & flag::DEAD == 0 || mode == 2)
                    && (mode == 1 || mode == 2 || (mode == 0 && u.flags & flag::DEPLOYED != 0))
            }
        })
    }

    /// Port of FUN_00457644: deployed unit with `tag == t`.
    pub fn find_tag(&self, t: i16) -> Option<usize> {
        self.units.iter().position(|u| u.tag == t && u.flags & flag::DEPLOYED != 0)
    }

    /// Port of FUN_0043a96a: the army owns at least one of item `id`.
    pub fn has_item(&self, id: i32) -> bool {
        id >= 1 && (id as usize) <= self.inventory.len() && self.inventory[id as usize - 1] != 0
    }

    /// Add `n` of item `id` to the inventory (cap 99), as FUN_00452e38 does.
    pub fn add_item(&mut self, id: usize, n: u8) {
        if let Some(c) = id.checked_sub(1).and_then(|i| self.inventory.get_mut(i)) {
            *c = if (*c as u32) + (n as u32) < 100 { *c + n } else { 99 };
        }
    }
}
