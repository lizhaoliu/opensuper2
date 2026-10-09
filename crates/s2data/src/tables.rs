//! Game rule tables: unit classes, actions, items, terrain, spells, growth.
//!
//! Layouts are documented in `docs/formats-tables.md`; the code that reads
//! each field in the original exe is named there. All ids exposed here are
//! 1-based like the game's (index 0 of a Vec holds id 1).

use crate::archive::Kind;
use crate::tbl_util::{cstr_list, Rec};
use crate::{format_err, le, Archives, GameDir, Result};

/// Where each table lives. Loose files in the game directory take
/// precedence over the archive copy, as in the game (`_access` check).
pub mod source {
    use crate::archive::Kind;
    /// (loose file name, archive, entry id)
    pub const KIND: (&str, Kind, usize) = ("kindsp.bin", Kind::Dat, 4);
    pub const ITEM: (&str, Kind, usize) = ("itemsp.bin", Kind::Dat, 1);
    pub const ITEM_HELP: (&str, Kind, usize) = ("itemhlsp.bin", Kind::Dat, 3);
    pub const TERRAIN: (Kind, usize) = (Kind::Dat, 6);
    pub const SPELLS: (Kind, usize) = (Kind::Bin, 2);
    pub const GROWTH: (Kind, usize) = (Kind::Bin, 8);
    pub const PARAMS: (Kind, usize) = (Kind::Bin, 10);
    pub const WORLD_ROUTES: (Kind, usize) = (Kind::Bin, 4);
    pub const STAGES: (Kind, usize) = (Kind::Bin, 1);
    /// The two-player stage list is a loose file only.
    pub const VS_STAGES: &str = "fstage.bin";
}

/// Load a table the way the game does: loose file first, then the archive.
pub fn load_with_override(dir: &GameDir, arcs: &Archives, src: (&str, Kind, usize)) -> Result<Vec<u8>> {
    if dir.exists(src.0) {
        dir.read(src.0)
    } else {
        arcs.read(src.1, src.2)
    }
}

// ---------------------------------------------------------------- classes

/// One unit class (兵種/職業), 37-byte record in KINDSP.
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    /// The name as stored (Big5, the 13-byte field at +0; `DAT_00494580 + (class − 1) * 0x25`).
    pub name_raw: Vec<u8>,
    /// Battle sprite set ("man cell" index); named characters override it with their id.
    pub sprite_set: u8,
    /// EXP needed per level (copied to unit+0x111 on class change).
    pub exp_per_level: u16,
    /// 1-based index into `KindTable::unit_types` (row/column of the type matrix).
    pub unit_type: u8,
    /// Movement type 1..=10: 1-5 ground, 6-8 flying, 9 aquatic, 10 special (see rules.md).
    pub move_type: u8,
    /// Innate skill (隱技): index into `KindTable::skills`, 0 = none.
    pub innate_skill: u8,
    /// Weapon-proficiency floor factor for AI units (`FUN_00452af0`).
    pub proficiency_rate: u8,
    /// Base AP (行動) for this class.
    pub base_ap: i8,
    /// Elements this class resists (生魔): spell damage /10. Bit i = element i.
    pub resist_elements: u16,
    /// Elements this class is weak to (剋魔): spell damage x3.
    pub weak_elements: u16,
    /// Base alignment (unit+0x123, 0..=110): bands <31 / <70 / >=70 select
    /// item restriction bits 1/2/4 (`FUN_0043cf34`); holy classes ~80-95,
    /// undead/evil 5-25. Used by persuasion-type spell formulas 0x46..0x48.
    pub alignment: u8,
    /// Item that must be equipped (and is consumed) to change into this class; 0 = none.
    pub change_item: u16,
    /// Unit skill flags (unit+0x115) of which one is required to change into this class; 0 = none.
    pub change_flags: u32,
    /// Multiplier (x/10) in the strength rating (強度).
    pub rating_mult: u8,
    /// Promotion tier 1..=5; 0 for monsters/NPC-only classes.
    pub tier: u8,
    /// Spell schools this class may study (bit k = school k+1).
    pub schools: u16,
    /// Classes this one may change into (1-based class ids, 0 = unused slot).
    pub promotions: [u8; 8],
    /// Index range into `KindTable::actions` of this class's attacks/moves (招式).
    pub actions: std::ops::Range<usize>,
    /// Bytes at 0x0b, 0x0c, 0x10 (constant 0x1e, 0x2f, 0x05; unused by the code found so far).
    pub unknown: [u8; 3],
}

/// One attack/move (招式), 23-byte record in KINDSP.
#[derive(Debug, Clone)]
pub struct ActionDef {
    /// 1 natural, 2 sword, 3 blade, 4 staff, 5 bow/gun, 6 axe, 7 spear/whip,
    /// 11 magic, 12 sword/martial art, 13 brainwash, 40 special move.
    pub kind: u8,
    pub name: String,
    /// The name as stored (Big5, bytes 1..10; `DAT_00494590 + 1 + k * 0x17`).
    pub name_raw: Vec<u8>,
    /// Byte 0x0a: attack animation set (shifted into `unit.facing` bits 2.., `<< 2`).
    pub anim_set: u8,
    /// Damage multiplier in tenths (10 = x1.0).
    pub power: u8,
    /// Hit-rate multiplier in percent (applied to the weapon's hit, or added for kind >= 21).
    pub hit: u8,
    /// Range (min, max); 0,0 for weapon kinds means "use the equipped weapon's reach".
    pub range: (i8, i8),
    /// Bytes 0x0f, 0x10 (motion/icon id and a zero byte; meaning unconfirmed).
    pub motion: (i8, i8),
    /// Status effect inflicted (case labels of `FUN_004083c4`), chance %, duration.
    pub status: (i8, i8, i8),
    /// Hit animation id (overridden by weapon item+0x1c when > 0).
    pub anim: i8,
    /// Sound effect id (overridden by weapon item+0x1d when non-zero).
    pub sound: i8,
    /// Area/attack shape (overridden by weapon item+0x1e when > 0): 1 single, 2 ranged, 3 line.
    pub shape: i8,
}

/// KINDSP.BIN: classes, unit types, skills, elements and the type matrix.
#[derive(Debug, Clone)]
pub struct KindTable {
    pub classes: Vec<ClassDef>,
    pub actions: Vec<ActionDef>,
    /// 兵種 names (24), used for the type matrix.
    pub unit_types: Vec<String>,
    /// Innate skill names (33; index 0 = 無).
    pub skills: Vec<String>,
    /// Element names (16): bit i of the element masks.
    pub elements: Vec<String>,
    /// `matrix[atk_type-1][def_type-1]`: damage bonus in percent.
    pub type_matrix: Vec<Vec<i8>>,
}

impl KindTable {
    pub fn parse(b: &[u8]) -> Result<KindTable> {
        const W: &str = "kindsp";
        let h = Rec::new(b, W);
        let promo_off = h.i32(0)? as usize;
        let n_classes = h.i16(4)? as usize;
        let skills = cstr_list(b, h.i32(6)? as usize, h.i16(10)? as usize, W)?;
        let elements = cstr_list(b, h.i32(12)? as usize, h.i16(16)? as usize, W)?;
        let class_off = h.i32(0x12)? as usize;
        if h.i16(0x16)? as usize != n_classes {
            return Err(format_err(W, "class count mismatch"));
        }
        let matrix_off = h.i32(0x18)? as usize;
        let n_types = h.i16(0x1c)? as usize;
        let list_off = h.i32(0x1e)? as usize;
        let n_actions = h.i16(0x22)? as usize;

        let mut type_matrix = Vec::with_capacity(n_types);
        for r in 0..n_types {
            let row = Rec::at(b, matrix_off + r * n_types, n_types, W)?;
            type_matrix.push(row.b.iter().map(|&v| v as i8).collect());
        }
        let names_off = matrix_off + n_types * n_types;
        let unit_types =
            (0..n_types).map(|i| Rec::at(b, names_off + 9 * i, 9, W)?.str(0, 9)).collect::<Result<_>>()?;

        let actions_off = list_off + n_classes * 4;
        let mut actions = Vec::with_capacity(n_actions);
        for i in 0..n_actions {
            let r = Rec::at(b, actions_off + 23 * i, 23, W)?;
            actions.push(ActionDef {
                kind: r.u8(0)?,
                name: r.str(1, 9)?,
                name_raw: r.raw_str(1, 9)?,
                anim_set: r.u8(0x0a)?,
                power: r.u8(0x0b)?,
                hit: r.u8(0x0c)?,
                range: (r.i8(0x0d)?, r.i8(0x0e)?),
                motion: (r.i8(0x0f)?, r.i8(0x10)?),
                status: (r.i8(0x11)?, r.i8(0x12)?, r.i8(0x13)?),
                anim: r.i8(0x14)?,
                sound: r.i8(0x15)?,
                shape: r.i8(0x16)?,
            });
        }

        let mut classes = Vec::with_capacity(n_classes);
        for i in 0..n_classes {
            let r = Rec::at(b, class_off + 37 * i, 37, W)?;
            let l = Rec::at(b, list_off + 4 * i, 4, W)?;
            let start = l.u16(0)? as usize;
            let count = l.u16(2)? as usize;
            if start + count > n_actions {
                return Err(format_err(W, format!("class {} action range out of bounds", i + 1)));
            }
            let mut promotions = [0u8; 8];
            promotions.copy_from_slice(Rec::at(b, promo_off + 8 * i, 8, W)?.b);
            classes.push(ClassDef {
                name: r.str(0, 11)?,
                name_raw: r.raw_str(0, 13)?,
                sprite_set: r.u8(0x0d)?,
                exp_per_level: r.u16(0x0e)?,
                unit_type: r.u8(0x11)?,
                move_type: r.u8(0x12)?,
                innate_skill: r.u8(0x13)?,
                proficiency_rate: r.u8(0x14)?,
                base_ap: r.i8(0x15)?,
                resist_elements: r.u16(0x16)?,
                weak_elements: r.u16(0x18)?,
                alignment: r.u8(0x1a)?,
                change_item: r.u16(0x1b)?,
                change_flags: r.u32(0x1d)?,
                rating_mult: r.u8(0x21)?,
                tier: r.u8(0x22)?,
                schools: r.u16(0x23)?,
                promotions,
                actions: start..start + count,
                unknown: [r.u8(0x0b)?, r.u8(0x0c)?, r.u8(0x10)?],
            });
        }
        Ok(KindTable { classes, actions, unit_types, skills, elements, type_matrix })
    }

    /// Class by 1-based id.
    pub fn class(&self, id: u16) -> Option<&ClassDef> {
        (id as usize).checked_sub(1).and_then(|i| self.classes.get(i))
    }

    /// Type-matrix bonus (percent) when a unit of `atk` class hits one of `def` class.
    pub fn type_bonus(&self, atk: u16, def: u16) -> i32 {
        let (Some(a), Some(d)) = (self.class(atk), self.class(def)) else { return 0 };
        self.type_matrix
            .get(a.unit_type as usize - 1)
            .and_then(|row| row.get(d.unit_type as usize - 1))
            .map_or(0, |&v| v as i32)
    }
}

// ------------------------------------------------------------------ items

/// One item, 38-byte record in ITEMSP.
#[derive(Debug, Clone)]
pub struct ItemDef {
    pub id: u16,
    pub name: String,
    /// The name as stored (Big5, bytes 2..14; `DAT_004946a8 + 2 + (id − 1) * 0x26`).
    pub name_raw: Vec<u8>,
    /// 1..=6 weapon types (sword, blade, staff, bow, axe, spear = action kind - 1),
    /// 7..=11 armour/accessory for equipment slot (kind - 6), 12 stackable consumable.
    pub kind: u8,
    /// 0 for equipment whose `effect` is a passive ability; non-zero for usable items.
    pub usage: u8,
    /// Weapon reach code: max = v % 10; min = 1 if v == 1 or v >= 10, else 2.
    pub reach: u8,
    pub attack: i16,
    /// Weapon hit rate in percent (90 when unarmed).
    pub hit: u8,
    /// Critical-hit bonus, summed over equipped items (`FUN_0040927b`).
    pub crit: i8,
    pub defense: i8,
    pub magic: i8,
    /// Subtracted from the hit rate of attacks against the wearer.
    pub evasion: i8,
    pub ap: i8,
    /// Passive ability / use effect id (`FUN_0043ab32` lookups).
    pub effect: u8,
    pub price: u16,
    /// Overrides of the action's animation, sound and shape when > 0.
    pub anim: i8,
    pub sound: u8,
    pub shape: i8,
    /// Equippable unit types: bit (type-1).
    pub unit_types: u32,
    /// Extra restrictions: 0x40 only character 11, 0x80 only character 22,
    /// 0x100 only character 1; low bits matched against gender/tier masks.
    pub restrict: u16,
    /// Rank used when auto-equipping (higher is better).
    pub rank: u8,
}

pub fn parse_items(b: &[u8]) -> Result<Vec<ItemDef>> {
    const W: &str = "itemsp";
    let n = le::i16(b, 0)?.max(0) as usize;
    (0..n)
        .map(|i| {
            let r = Rec::at(b, 2 + 38 * i, 38, W)?;
            Ok(ItemDef {
                id: r.u16(0)?,
                name: r.str(2, 12)?,
                name_raw: r.raw_str(2, 12)?,
                kind: r.u8(0x0e)?,
                usage: r.u8(0x0f)?,
                reach: r.u8(0x10)?,
                attack: r.i16(0x11)?,
                hit: r.u8(0x13)?,
                crit: r.i8(0x14)?,
                defense: r.i8(0x15)?,
                magic: r.i8(0x16)?,
                evasion: r.i8(0x17)?,
                ap: r.i8(0x18)?,
                effect: r.u8(0x19)?,
                price: r.u16(0x1a)?,
                anim: r.i8(0x1c)?,
                sound: r.u8(0x1d)?,
                shape: r.i8(0x1e)?,
                unit_types: r.u32(0x1f)?,
                restrict: r.u16(0x23)?,
                rank: r.u8(0x25)?,
            })
        })
        .collect()
}

impl ItemDef {
    /// (min, max) weapon reach.
    pub fn reach_range(&self) -> (u8, u8) {
        let v = self.reach;
        let max = v % 10;
        let min = if v == 1 || v >= 10 { 1 } else { 2 };
        (min, max)
    }
}

/// `i16 count` followed by `count` NUL-terminated strings (ITEMHLSP: item help text).
pub fn parse_string_list(b: &[u8], what: &'static str) -> Result<Vec<String>> {
    let n = le::i16(b, 0)?.max(0) as usize;
    cstr_list(b, 2, n, what)
}

// ---------------------------------------------------------------- terrain

/// Effect of one terrain type on one class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainEffect {
    /// Movement cost; -1 = impassable.
    pub cost: i8,
    /// Attack modifier (used as `(atk_mod - def_mod) * 10` per mille in damage).
    pub attack: i8,
    /// Defence modifier.
    pub defense: i8,
}

/// DAT entry 6: terrain names and the per-class terrain effects.
#[derive(Debug, Clone)]
pub struct TerrainTable {
    /// All terrain names: `ground` ground types then `objects` object types.
    pub names: Vec<String>,
    pub ground: usize,
    pub objects: usize,
    /// `effects[class-1][terrain]`.
    pub effects: Vec<Vec<TerrainEffect>>,
}

impl TerrainTable {
    pub fn parse(b: &[u8], n_classes: usize) -> Result<TerrainTable> {
        const W: &str = "terrain";
        let total = le::i16(b, 0)?.max(0) as usize;
        let ground = le::i16(b, 2)?.max(0) as usize;
        let objects = le::i16(b, 4)?.max(0) as usize;
        if ground + objects != total {
            return Err(format_err(W, "ground + object counts != total"));
        }
        let mut effects = Vec::with_capacity(n_classes);
        for c in 0..n_classes {
            let row = Rec::at(b, 6 + c * total * 3, total * 3, W)?;
            effects.push(
                (0..total)
                    .map(|t| TerrainEffect {
                        cost: row.b[3 * t] as i8,
                        attack: row.b[3 * t + 1] as i8,
                        defense: row.b[3 * t + 2] as i8,
                    })
                    .collect(),
            );
        }
        let names_off = 6 + n_classes * total * 3;
        let names = (0..total).map(|i| Rec::at(b, names_off + 11 * i, 11, W)?.str(0, 11)).collect::<Result<_>>()?;
        Ok(TerrainTable { names, ground, objects, effects })
    }

    /// Effect for class id (1-based) on terrain index (0-based into `names`).
    pub fn effect(&self, class: u16, terrain: usize) -> Option<TerrainEffect> {
        self.effects.get((class as usize).checked_sub(1)?)?.get(terrain).copied()
    }
}

// ----------------------------------------------------------------- spells

/// One spell / sword art / combo (37-byte record in BIN 2).
#[derive(Debug, Clone)]
pub struct SpellDef {
    /// School 1..=11 (index into `SpellTable::schools` + 1).
    pub school: u8,
    /// 1-based position within the school.
    pub index: u8,
    pub name: String,
    /// MP cost (or ST cost when `stat_flags & 2`); halved by skill flag 0x20.
    pub cost: i16,
    /// Required school level (school EXP / 10).
    pub level: i16,
    /// Range / size parameter (byte 0x11).
    pub range: i8,
    /// Area shape code (byte 0x12; 1 = single target).
    pub shape: u8,
    /// Effect code (case labels in `FUN_00462144`; 1 = damage).
    pub effect: u8,
    /// Power formula (case labels in `FUN_0046932f`, constants in BIN 10).
    pub formula: u8,
    pub power: i16,
    /// Hit chance in percent.
    pub hit: i16,
    /// Duration code: <=100 turns; 101..=200 caster magic/(v-100); 201..=254
    /// (caster - target magic)/(v-200); >= 255 permanent.
    pub duration: i16,
    /// Element mask (matched against class resist/weak masks).
    pub elements: u16,
    pub flags: u16,
    /// Target side: 1 own side, 2 enemies, 3 everyone, 0 self.
    pub target: u8,
    /// Combo partner bits (& 7 != 0: needs a partner, 合擊).
    pub combo: u8,
    /// 1: magic stat / MP; 2: sword stat / ST; 0x10/0x20/0x40: height falloff class.
    pub stat_flags: u8,
    /// Bytes 0x22..=0x24 (status icon / help ids; meaning unconfirmed).
    pub extra: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct SpellTable {
    pub schools: Vec<String>,
    /// Header byte 2: schools `0..magic_schools` are magic (MP/魔力), the rest
    /// techniques (header byte 3 = their count).
    pub magic_schools: u8,
    pub tech_schools: u8,
    /// Per school: (first spell index 0-based, count).
    pub school_spells: Vec<(u8, u8)>,
    pub spells: Vec<SpellDef>,
    /// Status-effect names (飛行, 飄浮, 祝福, ...).
    pub status_names: Vec<String>,
}

impl SpellTable {
    pub fn parse(b: &[u8]) -> Result<SpellTable> {
        const W: &str = "spells";
        let h = Rec::new(b, W);
        let n_schools = h.u8(1)? as usize;
        let n_spells = h.u8(7)? as usize;
        let n_status = h.u8(8)? as usize;
        let school_spells = (0..n_schools).map(|i| Ok((h.u8(9 + 2 * i)?, h.u8(10 + 2 * i)?))).collect::<Result<_>>()?;
        let spells_off = 9 + 2 * n_schools;
        let mut spells = Vec::with_capacity(n_spells);
        for i in 0..n_spells {
            let r = Rec::at(b, spells_off + 37 * i, 37, W)?;
            let mut extra = [0u8; 3];
            extra.copy_from_slice(r.bytes(0x22, 3)?);
            spells.push(SpellDef {
                school: r.u8(0)?,
                index: r.u8(1)?,
                name: r.str(2, 11)?,
                cost: r.i16(0x0d)?,
                level: r.i16(0x0f)?,
                range: r.i8(0x11)?,
                shape: r.u8(0x12)?,
                effect: r.u8(0x13)?,
                formula: r.u8(0x14)?,
                power: r.i16(0x15)?,
                hit: r.i16(0x17)?,
                duration: r.i16(0x19)?,
                elements: r.u16(0x1b)?,
                flags: r.u16(0x1d)?,
                target: r.u8(0x1f)?,
                combo: r.u8(0x20)?,
                stat_flags: r.u8(0x21)?,
                extra,
            });
        }
        let strs_off = spells_off + 37 * n_spells;
        let base = strs_off + 2 * (n_schools + n_status);
        let mut names = Vec::with_capacity(n_schools + n_status);
        for i in 0..n_schools + n_status {
            let off = le::u16(b, strs_off + 2 * i)? as usize;
            names.push(crate::tbl_util::cstr_at(b, base + off, W)?.0);
        }
        let status_names = names.split_off(n_schools);
        Ok(SpellTable {
            schools: names,
            magic_schools: h.u8(2)?,
            tech_schools: h.u8(3)?,
            school_spells,
            spells,
            status_names,
        })
    }
}

// ----------------------------------------------------------------- growth

/// Per-level growth of one stat: see `GrowthDesc::roll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GrowthDesc(pub [i16; 6]);

impl GrowthDesc {
    /// Stat gain at `level` given two random draws `r1 = rand % (d[4]+1)` and
    /// `r2 = rand % (d[5]+1)`, drawn in that order (`FUN_00423607`):
    /// `trunc(sqrt(d0/d1 * level) + d2/d3 + r1/(r2+1) + 0.5)`; 0 if d1 or d3 is 0.
    pub fn roll(&self, level: i32, r1: i32, r2: i32) -> i32 {
        let d = self.0;
        if d[1] == 0 || d[3] == 0 {
            return 0;
        }
        let x = ((d[0] as f64 / d[1] as f64) * level as f64).sqrt() + d[2] as f64 / d[3] as f64;
        (r1 as f64 / (r2 + 1) as f64 + x + 0.5) as i32
    }
}

/// Stat order used by growth tables and base stats.
pub const GROWTH_STATS: [&str; 9] = ["HP", "MP", "ST", "攻擊", "防禦", "反應", "戰技", "魔力", "劍術"];

/// Class base record (50 bytes): 9 base stats, 5 default equipment ids
/// (bytes 18..27, copied to placement record +0x3f by FUN_0043b67e), 11
/// learned-spell masks (one per school).
#[derive(Debug, Clone)]
pub struct ClassBase {
    pub stats: [i16; 9],
    pub equipment: [i16; 5],
    pub spells: [u16; 11],
}

/// BIN 8: growth descriptors and base stats per class.
#[derive(Debug, Clone)]
pub struct GrowthTable {
    /// Upper bound the game checks class ids against (272 in the shipped data).
    pub id_limit: i16,
    pub growth: Vec<[GrowthDesc; 9]>,
    pub base: Vec<ClassBase>,
}

impl GrowthTable {
    pub fn parse(b: &[u8], n_classes: usize) -> Result<GrowthTable> {
        const W: &str = "growth";
        let id_limit = le::i16(b, 0)?;
        let mut growth = Vec::with_capacity(n_classes);
        let mut base = Vec::with_capacity(n_classes);
        for c in 0..n_classes {
            let r = Rec::at(b, 2 + c * 108, 108, W)?;
            let mut g = [GrowthDesc::default(); 9];
            for (s, d) in g.iter_mut().enumerate() {
                for k in 0..6 {
                    d.0[k] = r.i16(s * 12 + k * 2)?;
                }
            }
            growth.push(g);
            let r = Rec::at(b, 2 + n_classes * 108 + c * 50, 50, W)?;
            let mut stats = [0i16; 9];
            for (k, v) in stats.iter_mut().enumerate() {
                *v = r.i16(2 * k)?;
            }
            let mut equipment = [0i16; 5];
            for (k, v) in equipment.iter_mut().enumerate() {
                *v = r.i16(18 + 2 * k)?;
            }
            let mut spells = [0u16; 11];
            for (k, v) in spells.iter_mut().enumerate() {
                *v = r.u16(28 + 2 * k)?;
            }
            base.push(ClassBase { stats, equipment, spells });
        }
        Ok(GrowthTable { id_limit, growth, base })
    }
}

// ------------------------------------------------------------- parameters

/// BIN 10: per-spell presentation bytes, per-effect sounds and the formula
/// constants block (`DAT_0049456c`), kept raw with accessors for known slots.
#[derive(Debug, Clone)]
pub struct FormulaParams {
    /// Four parallel per-spell byte arrays (`DAT_004a255c`, `_1f54`, `_1f3c`, `_1f28`).
    pub per_spell: [Vec<u8>; 4],
    /// Sound id per CEL1 effect frame set (`DAT_004a1f50`).
    pub effect_sound: Vec<u8>,
    /// Signed formula constants; index with the offsets in rules.md.
    pub p: Vec<i8>,
}

impl FormulaParams {
    pub fn parse(b: &[u8]) -> Result<FormulaParams> {
        const W: &str = "params";
        let n = le::i16(b, 0)?.max(0) as usize;
        let arr = |k: usize| Rec::at(b, 2 + k * n, n, W).map(|r| r.b.to_vec());
        let per_spell = [arr(0)?, arr(1)?, arr(2)?, arr(3)?];
        let m_off = 2 + 4 * n;
        let m = le::i16(b, m_off)?.max(0) as usize;
        let effect_sound = Rec::at(b, m_off + 2, m, W)?.b.to_vec();
        // FUN_0042422b: `DAT_0049456c = DAT_004a1f50 + m + 2` — two bytes
        // follow the sound table before the constants.
        let p = b.get(m_off + 2 + m + 2..).unwrap_or(&[]).iter().map(|&v| v as i8).collect();
        Ok(FormulaParams { per_spell, effect_sound, p })
    }

    pub fn get(&self, off: usize) -> i32 {
        self.p.get(off).copied().unwrap_or(0) as i32
    }

    /// Damage randomisation: `dmg * (p[0] + rand(p[1] + 3)) / 10`.
    pub fn damage_scale(&self) -> (i32, i32) {
        (self.get(0), self.get(1) + 3)
    }

    /// Hit randomisation: `hit * (p[2] + rand(p[3] + 3)) / 10`.
    pub fn hit_scale(&self) -> (i32, i32) {
        (self.get(2), self.get(3) + 3)
    }

    /// Enemy stat modifier in percent for difficulty `d` (`p[0xf3 + d]`).
    pub fn difficulty_bonus(&self, d: usize) -> i32 {
        self.get(0xf3 + d)
    }
}

// ------------------------------------------------------------ world routes

/// BIN 4: one record per stage locating it on the world map.
#[derive(Debug, Clone)]
pub struct WorldStage {
    /// Route/description text shown on the world map (Big5, segments split by `segments`).
    pub text: String,
    pub raw_text: Vec<u8>,
    /// Map position in pixels (the view is centred here, clamped to the map).
    pub x: u16,
    pub y: u16,
    /// Byte 10: 1 = this stage offers a route choice (inferred).
    pub choice: u8,
    /// Bytes 11..=13: byte lengths of up to three text segments (inferred).
    pub segments: [u8; 3],
}

pub fn parse_world_stages(b: &[u8]) -> Result<Vec<WorldStage>> {
    const W: &str = "world";
    let n = le::i16(b, 0)?.max(0) as usize;
    (0..n)
        .map(|i| {
            let r = Rec::at(b, 2 + 14 * i, 14, W)?;
            let off = r.u32(0)? as usize;
            let len = r.u16(4)? as usize;
            let raw_text = Rec::at(b, off, len, W)?.b.to_vec();
            Ok(WorldStage {
                text: crate::text::big5(&raw_text),
                raw_text,
                x: r.u16(6)?,
                y: r.u16(8)?,
                choice: r.u8(10)?,
                segments: [r.u8(11)?, r.u8(12)?, r.u8(13)?],
            })
        })
        .collect()
}

/// Everything above, loaded from a game directory.
pub struct RuleTables {
    pub kinds: KindTable,
    pub items: Vec<ItemDef>,
    pub item_help: Vec<String>,
    pub terrain: TerrainTable,
    pub spells: SpellTable,
    pub growth: GrowthTable,
    pub params: FormulaParams,
    pub world: Vec<WorldStage>,
}

impl RuleTables {
    pub fn load(dir: &GameDir, arcs: &Archives) -> Result<RuleTables> {
        let kinds = KindTable::parse(&load_with_override(dir, arcs, source::KIND)?)?;
        let n = kinds.classes.len();
        Ok(RuleTables {
            items: parse_items(&load_with_override(dir, arcs, source::ITEM)?)?,
            item_help: parse_string_list(&load_with_override(dir, arcs, source::ITEM_HELP)?, "itemhlsp")?,
            terrain: TerrainTable::parse(&arcs.read(source::TERRAIN.0, source::TERRAIN.1)?, n)?,
            spells: SpellTable::parse(&arcs.read(source::SPELLS.0, source::SPELLS.1)?)?,
            growth: GrowthTable::parse(&arcs.read(source::GROWTH.0, source::GROWTH.1)?, n)?,
            params: FormulaParams::parse(&arcs.read(source::PARAMS.0, source::PARAMS.1)?)?,
            world: parse_world_stages(&arcs.read(source::WORLD_ROUTES.0, source::WORLD_ROUTES.1)?)?,
            kinds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_roll_matches_formula() {
        let d = GrowthDesc([1, 3, 6, 1, 2, 0]);
        // sqrt(1/3*3)=1, +6/1 -> 7, +r1/(r2+1) = 2/1 -> 9, +0.5 truncated -> 9
        assert_eq!(d.roll(3, 2, 0), 9);
        // 2/(1+1) = 1 -> 8.5 -> 8
        assert_eq!(d.roll(3, 2, 1), 8);
        // sqrt(1/3*12)=2 -> 8.5 -> 8
        assert_eq!(d.roll(12, 0, 0), 8);
        assert_eq!(GrowthDesc([1, 0, 1, 1, 0, 0]).roll(5, 0, 0), 0);
    }

    #[test]
    fn item_reach() {
        let mut it = parse_items(&{
            let mut b = vec![1, 0];
            b.extend_from_slice(&[0u8; 38]);
            b
        })
        .unwrap()
        .remove(0);
        it.reach = 1;
        assert_eq!(it.reach_range(), (1, 1));
        it.reach = 3;
        assert_eq!(it.reach_range(), (2, 3));
        it.reach = 12;
        assert_eq!(it.reach_range(), (1, 2));
    }

    #[test]
    fn string_list() {
        let b = [2, 0, b'a', 0, b'b', b'c', 0];
        assert_eq!(parse_string_list(&b, "t").unwrap(), vec!["a", "bc"]);
    }
}
