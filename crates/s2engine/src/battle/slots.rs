//! The per-unit sprite and portrait slots (`unit+0xa0`, `unit+0xa2`) and the
//! stage's sprite-type list they index (`DAT_00497b68`, count `DAT_0049fddc`,
//! shared by both armies), ported so saves match the original byte for byte.
//! The original uses them to share decoded sprite banks; the view here
//! decodes frames itself and nothing reads them back. (Ported by FLOW.)
//!
//! When the original sets them:
//! - an army is cleared (`FUN_0043deeb`): the list empties (`FUN_0043ee88`);
//! - a stage loads from the world map (`FUN_0043f42c` from `FUN_00423033`
//!   mode 10): every living player unit takes a slot (`FUN_0043f34f`, which
//!   also clears its turn flags) and the stage records' types are listed;
//! - units are created (`FUN_0043e83e` / `FUN_0043eb5f` → `FUN_0043f23c`):
//!   find or append; portraits `FUN_0043fa24(army, first)` / `FUN_0043fc56`;
//! - a summon appears (`FUN_0043eef6`): slot 1 of the summon bank;
//! - the deployment ends (`FUN_0041671c`): idle members get slot 1, types
//!   nobody on the map or in the enemy records uses leave the list
//!   (`FUN_0044654d`, `FUN_004466ee`, `FUN_004467a8`), then both armies'
//!   deployed units are renumbered (`FUN_004440cd`);
//! - an army is loaded (`FUN_0044306f`): find or append in battle, 0 between
//!   stages or when dead (`FUN_00443c84`), portraits renumbered.

use super::unit::flag;
use super::{Battle, Side, UnitRef};

#[derive(Debug, Clone, Default)]
pub struct SpriteSlots {
    /// Loaded sprite types, in load order (`DAT_00497b68[i * 0x77]`).
    pub types: Vec<i16>,
    /// `DAT_0049fdb0`: types in the normal sprite bank (PRG 1); 0 = unknown.
    pub n_types: i16,
}

impl SpriteSlots {
    fn limit(&self) -> i16 {
        if self.n_types > 0 { self.n_types } else { i16::MAX }
    }

    /// Port of `FUN_0043ee88`.
    pub fn reset(&mut self) {
        self.types.clear();
    }

    /// Port of `FUN_0043f2ef(kind)`: the 1-based slot of `kind`, 0 when not listed.
    pub fn find(&self, kind: i16) -> u16 {
        let k = if self.limit() < kind { 0x2b } else { kind };
        self.types.iter().position(|&t| t == k).map_or(0, |i| i as u16 + 1)
    }

    /// `FUN_0043f23c`'s slot: listed, or appended (unknown kinds as type 0x2c).
    pub fn assign(&mut self, kind: i16) -> u16 {
        match self.find(kind) {
            0 => {
                let t = if kind - 1 >= self.limit() { 0x2b } else { kind - 1 };
                self.types.push(t + 1);
                self.types.len() as u16
            }
            s => s,
        }
    }

    /// `FUN_0043f34f` / `FUN_0044306f`'s slot: listed, or appended as is.
    pub fn assign_raw(&mut self, kind: i16) -> u16 {
        match self.find(kind) {
            0 => {
                self.types.push(kind);
                self.types.len() as u16
            }
            s => s,
        }
    }

    /// `FUN_0043f42c`'s preload of a record's type (unknown kinds as type 1).
    pub fn preload(&mut self, kind: i16) {
        if self.find(kind) == 0 {
            self.types.push(if self.limit() < kind { 1 } else { kind });
        }
    }

    /// `FUN_004467a8` for one type: drop it, the later ones move up.
    pub fn unload(&mut self, kind: i16) {
        let s = self.find(kind);
        if s != 0 {
            self.types.remove(s as usize - 1);
        }
    }
}

impl Battle {
    /// Port of `FUN_0043deeb`'s effect here: clear army `side`; clearing any
    /// army also empties the stage's sprite list (`FUN_0043ee88`).
    pub fn clear_army(&mut self, side: Side) {
        self.army_mut(side).clear();
        self.slots.reset();
    }

    /// Port of `FUN_0043f42c(player, records, n)` (`FUN_00423033` mode 10):
    /// `FUN_0043f34f` on the player army, then every stage record's type
    /// listed (a named record's type is its character's current class).
    pub fn slots_stage_start(&mut self) {
        let mut slots = std::mem::take(&mut self.slots);
        for u in self.army_mut(Side::Player).units.iter_mut() {
            if u.flags & (flag::DEAD | flag::SUMMON) == 0 {
                u.turn_flags = 0;
                u.sprite = slots.assign_raw(u.kind);
            }
        }
        for i in 0..self.defs.len() {
            let p = &self.defs[i].p;
            let mut kind = p.class;
            if p.character >= 1 {
                for side in [Side::Player, Side::Enemy] {
                    if let Some(k) = self.army(side).find_char(p.character, 1) {
                        kind = self.army(side).units[k].kind;
                        break;
                    }
                }
            }
            slots.preload(kind);
        }
        self.slots = slots;
    }

    /// Port of `FUN_0043e83e(army, first)` (`FUN_0043f23c` per unit) and
    /// `FUN_0043fa24(army, first)`: slots for the units from `first` on.
    pub fn slots_created(&mut self, side: Side, first: usize) {
        let mut slots = std::mem::take(&mut self.slots);
        for u in self.army_mut(side).units.iter_mut().skip(first) {
            u.sprite = slots.assign(u.kind);
        }
        self.slots = slots;
        self.assign_portraits(side, first);
    }

    /// Port of `FUN_0043eb5f` (`FUN_0043f23c`) and `FUN_0043fc56` for one unit.
    pub fn slots_created_one(&mut self, r: UnitRef) {
        let kind = self.unit(r).kind;
        let s = self.slots.assign(kind);
        self.unit_mut(r).sprite = s;
        self.assign_portrait_one(r);
    }

    /// `FUN_0043eef6` (a summon's own bank, slot 1) and `FUN_0043fc56`.
    pub fn slots_summon(&mut self, r: UnitRef) {
        self.unit_mut(r).sprite = 1;
        self.assign_portrait_one(r);
    }

    /// Port of the slot part of `FUN_0044306f` (army `side` read from a save).
    pub fn slots_loaded(&mut self, side: Side, between_stages: bool) {
        let mut slots = std::mem::take(&mut self.slots);
        for u in self.army_mut(side).units.iter_mut() {
            let (dead, summon) = (u.flags & flag::DEAD != 0, u.flags & flag::SUMMON != 0);
            if side == Side::Enemy && !summon && dead && u.npc == 1 && u.char_id == 0 {
                // No frame is loaded for it: the sizes FUN_0044306f cleared
                // (+0xa4 / +0xa6) stay 0.
                u.sprite = 0;
                u.sprite_w = 0;
                u.sprite_h = 0;
            } else if !summon {
                if !dead && !between_stages {
                    u.sprite = slots.assign_raw(u.kind);
                } else {
                    // FUN_00443c84 (the unit's own frame): slot 0, facing byte
                    // 0, unknown kinds as type 1.
                    u.sprite = 0;
                    u.facing = 0;
                    if slots.n_types > 0 && u.kind - 1 >= slots.n_types {
                        u.kind = 1;
                    }
                }
            } else if !dead && u.flags & flag::DEPLOYED != 0 {
                u.sprite = 1;
            }
        }
        self.slots = slots;
        self.assign_portraits(side, 0);
    }

    /// Port of the end of `FUN_0041671c`: idle members get slot 1 (and face
    /// 0; the caller redraws those listed in the result), and the types no
    /// deployed member, enemy record or enemy unit uses leave the list
    /// (`FUN_0044654d`, `FUN_004466ee`, `FUN_004467a8`); after an eviction
    /// both armies' deployed units are renumbered (`FUN_004440cd`).
    /// Returns the members whose facing was reset.
    pub fn slots_after_deploy(&mut self) -> Vec<UnitRef> {
        let mut turned = Vec::new();
        let mut kinds: Vec<i16> = Vec::new();
        for i in 0..self.army(Side::Player).units.len() {
            let u = &self.army(Side::Player).units[i];
            if u.flags & (flag::DEPLOYED | flag::DEAD | flag::SUMMON) == 0 && self.slots.find(u.kind) != 0 {
                let kind = u.kind;
                let u = self.unit_mut(UnitRef::new(Side::Player, i));
                if u.facing != 0 {
                    u.facing = 0;
                    turned.push(UnitRef::new(Side::Player, i));
                }
                u.sprite = 1;
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
            }
        }
        let used = |kinds: &mut Vec<i16>, kind: i16| kinds.retain(|&k| k != kind);
        for u in self.army(Side::Player).units.iter().filter(|u| u.flags & flag::DEPLOYED != 0) {
            used(&mut kinds, u.kind);
        }
        if kinds.is_empty() {
            return turned;
        }
        for i in self.enemy_defs_start..self.enemy_defs_start + self.n_enemy_defs {
            if let Some(d) = self.defs.get(i) {
                used(&mut kinds, d.p.class);
            }
        }
        if kinds.is_empty() {
            return turned;
        }
        for u in self.army(Side::Enemy).units.iter().filter(|u| u.flags & flag::SUMMON == 0) {
            used(&mut kinds, u.kind);
        }
        if kinds.is_empty() {
            return turned;
        }
        for &k in &kinds {
            self.slots.unload(k);
        }
        for side in [Side::Player, Side::Enemy] {
            let slots = &self.slots;
            for u in self.armies[side.index()].units.iter_mut() {
                if u.flags & (flag::DEAD | flag::SUMMON) == 0 && u.flags & flag::DEPLOYED != 0 {
                    let s = slots.find(u.kind);
                    if s != 0 {
                        u.sprite = s;
                    }
                }
            }
        }
        turned
    }

    /// Port of the numbering in `FUN_0043fa24(army, first)` (`FUN_0043fea5`
    /// per unit): each unit from `first` on gets in `+0xa2` the 1-based index
    /// of its face in the army's portrait list, appending faces not listed
    /// yet. The face is the character id when 1..=199, else the class's
    /// (class record +0xd, `sprite_set`). From 0 the list starts empty;
    /// otherwise it is the faces the units before `first` carry.
    pub fn assign_portraits(&mut self, side: Side, first: usize) {
        let face = self.face_fn();
        let units = &mut self.army_mut(side).units;
        let first = first.min(units.len());
        let mut list: Vec<u16> = Vec::new();
        for u in &units[..first] {
            let k = u.portrait as usize;
            if k > 0 {
                if list.len() < k {
                    list.resize(k, 0);
                }
                list[k - 1] = face(u.char_id, u.kind);
            }
        }
        for u in units[first..].iter_mut() {
            let f = face(u.char_id, u.kind);
            let k = match list.iter().position(|&x| x == f) {
                Some(p) => p + 1,
                None => {
                    list.push(f);
                    list.len()
                }
            };
            u.portrait = k as u16;
        }
    }

    /// `FUN_0043fc56` → `FUN_0043fea5` for one unit: its face's index in the
    /// army's list (the faces the other units carry), appended if missing.
    pub fn assign_portrait_one(&mut self, r: UnitRef) {
        let face = self.face_fn();
        let units = &mut self.army_mut(r.side).units;
        let mut list: Vec<u16> = Vec::new();
        for (i, u) in units.iter().enumerate() {
            let k = u.portrait as usize;
            if i != r.idx && k > 0 {
                if list.len() < k {
                    list.resize(k, 0);
                }
                list[k - 1] = face(u.char_id, u.kind);
            }
        }
        let u = &mut units[r.idx];
        let f = face(u.char_id, u.kind);
        u.portrait = match list.iter().position(|&x| x == f) {
            Some(p) => p as u16 + 1,
            None => list.len() as u16 + 1,
        };
    }

    /// A unit's portrait id: the character id when 1..=199, else its class's.
    fn face_fn(&self) -> impl Fn(i16, i16) -> u16 {
        let t = self.tables.clone();
        move |char_id: i16, kind: i16| -> u16 {
            if (1..=199).contains(&char_id) {
                char_id as u16
            } else {
                t.as_ref()
                    .and_then(|t| t.kinds.classes.get((kind as usize).wrapping_sub(1)))
                    .map_or(0, |c| c.sprite_set as u16)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_evicts_types_nobody_uses_and_renumbers() {
        // The stage-2 autosave of the oracle run: [81, 61, 10, 50, 100, 126]
        // listed for the party, then the enemy's 49, 5, 40; only 81 deployed.
        let mut s = SpriteSlots { types: vec![], n_types: 0x90 };
        for k in [81, 61, 10, 50, 100, 126] {
            s.assign_raw(k);
        }
        for k in [49, 5, 5, 40] {
            s.assign(k);
        }
        for k in [61, 10, 50, 100, 126] {
            s.unload(k);
        }
        assert_eq!(s.types, vec![81, 49, 5, 40]);
        assert_eq!((s.find(81), s.find(49), s.find(5), s.find(40)), (1, 2, 3, 4));
    }
}
