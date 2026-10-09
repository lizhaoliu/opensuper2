//! Save files `SAVE\UJnn.SAV` (53 slots, nn = 00..=52). Writer: `FUN_004312c4`.
//!
//! The format is a dump of game globals and unit structs, so most blocks are
//! kept raw (and written back byte-for-byte); known fields get accessors.
//! The item-count array length equals the number of items in ITEMSP, so the
//! parser needs it.

use crate::tbl_util::Rec;
use crate::{format_err, Result};

/// "北方密使\0" in Big5.
pub const MAGIC: &[u8; 9] = b"\xa5\x5f\xa4\xe8\xb1\x4b\xa8\xcf\0";
pub const DESC_LEN: usize = 0x32;
pub const COPYRIGHT_LEN: usize = 0xff;
pub const UNIT_LEN: usize = 0x16a;
pub const SLOTS: usize = 53;

pub fn slot_file_name(slot: usize) -> String {
    format!("SAVE\\UJ{slot:02}.SAV")
}

/// One unit struct as stored in memory/saves (362 bytes). Offsets are those
/// used throughout the exe (see docs/formats-tables.md "Unit struct").
#[derive(Clone, PartialEq, Eq)]
pub struct UnitRecord(pub Vec<u8>);

impl std::fmt::Debug for UnitRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnitRecord")
            .field("name", &self.name())
            .field("class", &self.class())
            .field("level", &self.level())
            .finish_non_exhaustive()
    }
}

macro_rules! field {
    ($get:ident, $set:ident, i16, $off:expr) => {
        pub fn $get(&self) -> i16 {
            i16::from_le_bytes([self.0[$off], self.0[$off + 1]])
        }
        pub fn $set(&mut self, v: i16) {
            self.0[$off..$off + 2].copy_from_slice(&v.to_le_bytes());
        }
    };
    ($get:ident, $set:ident, u16, $off:expr) => {
        pub fn $get(&self) -> u16 {
            u16::from_le_bytes([self.0[$off], self.0[$off + 1]])
        }
        pub fn $set(&mut self, v: u16) {
            self.0[$off..$off + 2].copy_from_slice(&v.to_le_bytes());
        }
    };
    ($get:ident, $set:ident, u32, $off:expr) => {
        pub fn $get(&self) -> u32 {
            u32::from_le_bytes(self.0[$off..$off + 4].try_into().unwrap())
        }
        pub fn $set(&mut self, v: u32) {
            self.0[$off..$off + 4].copy_from_slice(&v.to_le_bytes());
        }
    };
    ($get:ident, $set:ident, i32, $off:expr) => {
        pub fn $get(&self) -> i32 {
            i32::from_le_bytes(self.0[$off..$off + 4].try_into().unwrap())
        }
        pub fn $set(&mut self, v: i32) {
            self.0[$off..$off + 4].copy_from_slice(&v.to_le_bytes());
        }
    };
    ($get:ident, $set:ident, u8, $off:expr) => {
        pub fn $get(&self) -> u8 {
            self.0[$off]
        }
        pub fn $set(&mut self, v: u8) {
            self.0[$off] = v;
        }
    };
}

impl UnitRecord {
    field!(character, set_character, i16, 0x02);
    field!(class, set_class, i16, 0x04);
    field!(ai_controlled, set_ai_controlled, i16, 0x08);
    field!(kills, set_kills, i16, 0x1f);
    field!(x, set_x, i32, 0x23);
    field!(y, set_y, i32, 0x27);
    field!(ai_mode, set_ai_mode, i16, 0x0a);
    field!(facing, set_facing, u8, 0x2b);
    field!(gender, set_gender, u8, 0x2e);
    field!(boss, set_boss, u8, 0x2f);
    field!(flags, set_flags, u32, 0x30);
    field!(buffs, set_buffs, u32, 0x38);
    field!(status, set_status, u32, 0x3c);
    field!(level, set_level, i16, 0xb5);
    field!(hp_max, set_hp_max, i16, 0xb7);
    field!(mp_max, set_mp_max, i16, 0xb9);
    field!(st_max, set_st_max, i16, 0xbb);
    field!(magic_base, set_magic_base, i16, 0xbd);
    field!(ap_max, set_ap_max, i16, 0xbf);
    field!(reaction_base, set_reaction_base, i16, 0xc1);
    field!(attack_base, set_attack_base, i16, 0xc3);
    field!(defense_base, set_defense_base, i16, 0xc5);
    field!(sword, set_sword, i16, 0xc7);
    field!(skill_base, set_skill_base, i16, 0xc9);
    field!(exp_per_level, set_exp_per_level, u16, 0x111);
    field!(exp, set_exp, u16, 0x113);
    field!(skill_flags, set_skill_flags, u32, 0x115);
    field!(alignment, set_alignment, u8, 0x123);
    field!(hp, set_hp, i16, 0x124);
    field!(mp, set_mp, i16, 0x126);
    field!(st, set_st, i16, 0x128);
    field!(magic, set_magic, i16, 0x12a);
    field!(ap_base, set_ap_base, i16, 0x12c);
    field!(ap, set_ap, i16, 0x12e);
    field!(ap_carry, set_ap_carry, i16, 0x130);
    field!(attack, set_attack, i16, 0x134);
    field!(defense, set_defense, i16, 0x136);
    field!(skill, set_skill, i16, 0x138);

    pub fn name(&self) -> String {
        crate::text::big5(&self.0[0x10..0x1f])
    }

    /// Equipment slot `i` (0..6) item id; slot 0 is the weapon.
    pub fn equipment(&self, i: usize) -> u16 {
        let o = 0xcb + 2 * i;
        u16::from_le_bytes([self.0[o], self.0[o + 1]])
    }

    /// Proficiency word `i`: 0..6 weapon types 1..6, 6..17 spell schools 1..11 (EXP, level = /10),
    /// 17 special moves.
    pub fn proficiency(&self, i: usize) -> u16 {
        let o = 0xd7 + 2 * i;
        u16::from_le_bytes([self.0[o], self.0[o + 1]])
    }

    /// Learned-spell bit mask for school `k` (0..11).
    pub fn learned(&self, k: usize) -> u16 {
        let o = 0xfb + 2 * k;
        u16::from_le_bytes([self.0[o], self.0[o + 1]])
    }
}

/// One army (`FUN_00442ef8`): the player side or the enemy side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmyRecord {
    pub units: Vec<UnitRecord>,
    /// army+0xcc
    pub gold: u32,
    /// army+0xde (bit 0x40: treasure search enabled at stage end)
    pub flags: u32,
    /// army+0xd6
    pub d6: u32,
    /// army+0xd4
    pub d4: u16,
    /// army+0xe2: stat scaling percent applied to units of this army.
    pub scale: i32,
    /// Owned count per item id (index = id - 1), capped at 99.
    pub items: Vec<u8>,
    /// army+0x286 (0x21 bytes)
    pub tail: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFile {
    /// Slot description shown in the load menu (Big5, 50 bytes raw).
    pub description: Vec<u8>,
    pub copyright: Vec<u8>,
    /// Globals at 0x494508 (0x50 bytes).
    pub globals_a: Vec<u8>,
    /// Globals at 0x494638 (0x1f bytes).
    pub globals_b: Vec<u8>,
    /// Globals at 0x494668 (0x1d bytes): mode flags, stage numbers, difficulty.
    pub globals_c: Vec<u8>,
    pub treasure_count: u32,
    /// Hidden-treasure candidates of the current stage (80 i16 = up to 20 x 4).
    pub treasure: Vec<u8>,
    /// Globals at 0x497b48 (0x1c bytes).
    pub globals_d: Vec<u8>,
    pub player: ArmyRecord,
    pub enemy: ArmyRecord,
    /// Event-script flags (one i16 per script event), absent in two-player saves.
    pub script_flags: Option<Vec<i16>>,
}

struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let r = Rec::at(self.b, self.p, n, "save")?;
        self.p += n;
        Ok(r.b)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
}

impl SaveFile {
    /// Parse a save. `n_items` is the ITEMSP item count (302 in the shipped data).
    pub fn parse(b: &[u8], n_items: usize) -> Result<SaveFile> {
        let mut c = Cursor { b, p: 0 };
        if c.take(MAGIC.len())? != MAGIC {
            return Err(format_err("save", "missing 北方密使 magic"));
        }
        let description = c.take(DESC_LEN)?.to_vec();
        let copyright = c.take(COPYRIGHT_LEN)?.to_vec();
        let globals_a = c.take(0x50)?.to_vec();
        let globals_b = c.take(0x1f)?.to_vec();
        let globals_c = c.take(0x1d)?.to_vec();
        let treasure_count = c.u32()?;
        let treasure = c.take(0xa0)?.to_vec();
        let globals_d = c.take(0x1c)?.to_vec();
        let army = |c: &mut Cursor| -> Result<ArmyRecord> {
            let n = c.u32()? as usize;
            if n > 200 {
                return Err(format_err("save", format!("implausible unit count {n}")));
            }
            let units = (0..n).map(|_| c.take(UNIT_LEN).map(|u| UnitRecord(u.to_vec()))).collect::<Result<_>>()?;
            Ok(ArmyRecord {
                units,
                gold: c.u32()?,
                flags: c.u32()?,
                d6: c.u32()?,
                d4: c.u16()?,
                scale: c.u32()? as i32,
                items: c.take(n_items)?.to_vec(),
                tail: c.take(0x21)?.to_vec(),
            })
        };
        let player = army(&mut c)?;
        let enemy = army(&mut c)?;
        let two_player = u32::from_le_bytes(globals_c[4..8].try_into().unwrap()) != 0;
        let script_flags = if two_player || c.p == b.len() {
            None
        } else {
            let n = c.u16()? as usize;
            Some((0..n).map(|_| c.u16().map(|v| v as i16)).collect::<Result<_>>()?)
        };
        if c.p != b.len() {
            return Err(format_err("save", format!("{} trailing bytes", b.len() - c.p)));
        }
        Ok(SaveFile {
            description,
            copyright,
            globals_a,
            globals_b,
            globals_c,
            treasure_count,
            treasure,
            globals_d,
            player,
            enemy,
            script_flags,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(MAGIC);
        for blk in [&self.description, &self.copyright, &self.globals_a, &self.globals_b, &self.globals_c] {
            o.extend_from_slice(blk);
        }
        o.extend_from_slice(&self.treasure_count.to_le_bytes());
        o.extend_from_slice(&self.treasure);
        o.extend_from_slice(&self.globals_d);
        for a in [&self.player, &self.enemy] {
            o.extend_from_slice(&(a.units.len() as u32).to_le_bytes());
            for u in &a.units {
                o.extend_from_slice(&u.0);
            }
            o.extend_from_slice(&a.gold.to_le_bytes());
            o.extend_from_slice(&a.flags.to_le_bytes());
            o.extend_from_slice(&a.d6.to_le_bytes());
            o.extend_from_slice(&a.d4.to_le_bytes());
            o.extend_from_slice(&a.scale.to_le_bytes());
            o.extend_from_slice(&a.items);
            o.extend_from_slice(&a.tail);
        }
        if let Some(f) = &self.script_flags {
            o.extend_from_slice(&(f.len() as u16).to_le_bytes());
            for v in f {
                o.extend_from_slice(&v.to_le_bytes());
            }
        }
        o
    }

    pub fn description_text(&self) -> String {
        crate::text::big5(&self.description)
    }

    /// Current stage id (global 0x494670).
    pub fn stage(&self) -> u32 {
        u32::from_le_bytes(self.globals_c[8..12].try_into().unwrap())
    }

    /// Difficulty (global 0x494684).
    pub fn difficulty(&self) -> u8 {
        self.globals_c[0x1c]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(n_items: usize) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        b.extend(vec![b'x'; DESC_LEN]);
        b.extend(vec![0; COPYRIGHT_LEN + 0x50 + 0x1f]);
        let mut gc = vec![0u8; 0x1d];
        gc[8] = 5; // stage
        gc[0x1c] = 2;
        b.extend(gc);
        b.extend(3u32.to_le_bytes());
        b.extend(vec![0; 0xa0 + 0x1c]);
        for units in [1u32, 0] {
            b.extend(units.to_le_bytes());
            for _ in 0..units {
                let mut u = vec![0u8; UNIT_LEN];
                u[0x04] = 81;
                u[0xb5] = 7;
                b.extend(u);
            }
            b.extend(1500u32.to_le_bytes());
            b.extend([0u8; 4 + 4 + 2]);
            b.extend(100i32.to_le_bytes());
            b.extend(vec![1; n_items]);
            b.extend(vec![0; 0x21]);
        }
        b.extend(2u16.to_le_bytes());
        b.extend([1, 0, 0xff, 0xff]);
        b
    }

    #[test]
    fn roundtrip() {
        let raw = synthetic(5);
        let s = SaveFile::parse(&raw, 5).unwrap();
        assert_eq!(s.stage(), 5);
        assert_eq!(s.difficulty(), 2);
        assert_eq!(s.player.units[0].class(), 81);
        assert_eq!(s.player.units[0].level(), 7);
        assert_eq!(s.player.gold, 1500);
        assert_eq!(s.script_flags, Some(vec![1, -1]));
        assert_eq!(s.to_bytes(), raw);
    }

    #[test]
    fn magic_is_big5() {
        assert_eq!(crate::text::big5(MAGIC), "北方密使");
    }
}
