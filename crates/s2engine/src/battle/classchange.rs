//! Class change (轉職). Owned by the RULES port.
//!
//! CLASSCHANGE API (stable)
//! ------------------------
//!   `Battle::promotion_available(r) -> bool`    FUN_0043a522 (clamps EXP when there are no targets)
//!   `Battle::can_change_to(&u, class) -> bool`  FUN_00455c85 (hidden classes: ability mask / required item)
//!   `Battle::promotion_targets(r) -> Vec<u16>`  the list FUN_00453986 offers (promotion slots passing FUN_00455c85)
//!   `Battle::change_class(r, class, school, keep_ap)`   FUN_00455d95 (+ FUN_00455d21)
//!   `Battle::change_class_from_menu(r, class, school, keep_ap)`   FUN_00453986 tail:
//!                                   change, auto-equip (FUN_0044bd12), army flags (FUN_0043abc2)
//!   `ai_promotion_choice(&targets) -> Option<(u16, Option<usize>)>`  the AI/NPC path of FUN_00453986
//!
//! `school` is the 0-based spell school the player picked for the +300 bonus
//! (`None` = no pick, as for the AI). Sprite reloads are the view's.

use super::army::UnitRef;
use super::rules;
use super::unit::Unit;
use super::Battle;

/// FUN_00453986 for AI/NPC units: after a one-second wait it takes the first
/// eligible target and no school bonus.
pub fn ai_promotion_choice(targets: &[u16]) -> Option<(u16, Option<usize>)> {
    targets.first().map(|&c| (c, None))
}

impl Battle {
    /// Port of FUN_0043a522: a class change is offered (level ≥ 11, or level
    /// 10 with a full EXP bar, not VS mode, class has promotion targets).
    /// When the level qualifies but the class has no targets, EXP is clamped
    /// to `exp_next` as in the original.
    pub fn promotion_available(&mut self, r: UnitRef) -> bool {
        let t = self.t();
        let u = self.unit(r);
        if (u.level as i32) < 11 && (u.exp < u.exp_next || u.level < 10) {
            return false;
        }
        if self.vs {
            return false;
        }
        let has = t.kinds.class(u.kind as u16).is_some_and(|c| c.promotions[0] != 0);
        if !has {
            let u = self.unit_mut(r);
            if u.exp_next < u.exp {
                u.exp = u.exp_next;
            }
        }
        has
    }

    /// Port of FUN_00455c85: `u` may change into `class`: its ability mask
    /// (+0x1d) is 0 or shared with the unit, and its item (+0x1b) is 0 or equipped.
    pub fn can_change_to(&self, u: &Unit, class: u16) -> bool {
        let t = self.t();
        let Some(c) = t.kinds.class(class) else { return false };
        if c.change_flags != 0 && u.abilities & c.change_flags == 0 {
            return false;
        }
        c.change_item == 0 || u.equip.contains(&c.change_item)
    }

    /// The targets FUN_00453986 lists: promotion slots up to the first empty one, filtered by FUN_00455c85.
    pub fn promotion_targets(&self, r: UnitRef) -> Vec<u16> {
        let t = self.t();
        let u = self.unit(r);
        let Some(c) = t.kinds.class(u.kind as u16) else { return Vec::new() };
        c.promotions
            .iter()
            .take_while(|&&p| p != 0)
            .map(|&p| p as u16)
            .filter(|&p| self.can_change_to(u, p))
            .collect()
    }

    /// Port of FUN_00455d95: change unit `r` into class `new`.
    pub fn change_class(&mut self, r: UnitRef, new: u16, school: Option<usize>, keep_ap: bool) {
        let t = self.t();
        let (Some(old_c), Some(new_c)) = (t.kinds.class(self.unit(r).kind as u16).cloned(), t.kinds.class(new).cloned())
        else {
            return;
        };
        // FUN_00455d21: the required item is used up.
        if new_c.change_item != 0 {
            for s in 0..6 {
                if self.unit(r).equip[s] == new_c.change_item {
                    self.unequip_slot(r, s + 1, true);
                }
            }
        }
        for s in 1..=6 {
            self.unequip_slot(r, s, false);
        }
        {
            let u = self.unit_mut(r);
            let a = u.charm.saturating_sub(old_c.alignment);
            u.charm = if (a as u32) + (new_c.alignment as u32) < 0x6f { a + new_c.alignment } else { 0x6e };
        }
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        let mut g = rules::growth(&ctx, &mut self.rng, new, 1);
        let base = t.growth.base.get(new as usize - 1).map_or([0i16; 9], |b| b.stats);
        {
            let u = self.unit(r);
            // HP, MP, ST, 攻擊, 防禦, 反應, 戰技, 魔力, 劍術 (BIN 8 order)
            let cur = [u.hp_max, u.mp_max, u.st_max, u.atk, u.def, u.react, u.skill, u.mag, u.sword];
            for k in 0..9 {
                if cur[k] < base[k] {
                    g[k] += (base[k] as i32 - cur[k] as i32) / 2;
                }
            }
        }
        Battle::apply_growth(self.unit_mut(r), g);
        let u = self.unit_mut(r);
        let mut d = (new_c.base_ap as i32 - u.ap_max as i32).max(0);
        if new_c.unit_type != 5 && old_c.unit_type == 5 {
            d = -4;
        }
        if (u.ap_max as i32) + d < 0x6e {
            u.ap_max += d as i16;
            u.ap_base += d as i16;
        }
        if keep_ap {
            u.ap += d as i16;
        }
        if u.ap < 1 {
            u.ap_carry = 0;
            u.ap = 0;
        }
        if new_c.innate_skill != 0 {
            u.abilities |= 1u32 << ((new_c.innate_skill as u32).wrapping_sub(1) & 0x1f);
        }
        u.level = 1;
        u.exp = 0;
        u.exp_next = new_c.exp_per_level;
        if let Some(k) = school.filter(|&k| k < 0xb) {
            let slot = 6 + k;
            u.prof[slot] = (u.prof[slot] as u32 + 300).min(15000) as u16;
            let (first, count) = t.spells.school_spells[k];
            let lv = (u.prof[slot] / 10) as i32;
            for i in 0..count as usize {
                let sp = &t.spells.spells[first as usize + i];
                let bit = ((1u32 << (i & 0x1f)) & 0xffff) as u16;
                if sp.level as i32 <= lv && u.learned[k] & bit == 0 && sp.combo & 0x80 == 0 {
                    u.learned[k] |= bit;
                }
            }
        }
        u.kind = new as i16;
        unlock_summons(u, &t.spells);
        // FUN_00443c4c (reload the sprite) and, unless `keep_ap`, FUN_004563e8
        // (the wipe) are the caller's: view::reload_sprite / view::motion::wipe_in.
        if u.exp_next <= u.exp {
            if (u.exp_next as u32) * 3 < u.exp as u32 {
                u.exp = u.exp_next * 3;
            }
            self.level_up(r, false);
        }
    }

    /// FUN_00453986 after a choice: change class, re-equip the best owned
    /// gear (FUN_0044bd12) and recompute army flags (FUN_0043abc2(army, 0)).
    pub fn change_class_from_menu(&mut self, r: UnitRef, new: u16, school: Option<usize>, keep_ap: bool) {
        self.change_class(r, new, school, keep_ap);
        self.auto_equip(r);
        self.recalc_army_flags(r.side, 0);
    }
}

/// Port of FUN_0045721e: a unit that knows a 喚術 (school 7) spell may get
/// its spells 5..=10 (bits 4..=9): spell 5 + k once 喚術 proficiency / 10
/// reaches its level and the unit knows a spell of school k + 1 — the
/// original tests bit k (not k + 4) for "not yet learned".
fn unlock_summons(u: &mut Unit, spells: &s2data::tables::SpellTable) {
    if u.learned[6] == 0 {
        return;
    }
    let first = spells.school_spells.get(6).map_or(0, |s| s.0 as usize) + 4;
    let lv = (u.prof[12] / 10) as i32;
    for k in 0..6 {
        let Some(sp) = spells.spells.get(first + k) else { continue };
        if sp.level as i32 <= lv && u.learned[6] & (1 << k) == 0 && u.learned[k] != 0 {
            u.learned[6] |= 1 << (k + 4);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::army::{Side, UnitRef};
    use super::super::spells::tests::stage1;

    #[test]
    fn promotion_resets_level_and_moves_alignment() {
        let Some(mut b) = stage1() else { return eprintln!("classchange tests: no game data, skipped") };
        let t = b.t();
        let r = UnitRef::new(Side::Player, 0); // 志超, 劍手
        assert!(!b.promotion_available(r), "level 1 cannot change class");
        b.unit_mut(r).level = 11;
        assert!(b.promotion_available(r));
        let targets = b.promotion_targets(r);
        assert!(!targets.is_empty());
        let new = targets[0];
        let old = t.kinds.class(b.unit(r).kind as u16).unwrap().clone();
        let nc = t.kinds.class(new).unwrap().clone();
        let al0 = b.unit(r).charm as i32;
        let hp_unequipped = b.unit(r).hp_max;
        let n_eq = b.unit(r).equip.iter().filter(|&&e| e != 0).count();
        let inv0: u32 = b.army(Side::Player).inventory.iter().map(|&c| c as u32).sum();
        b.change_class(r, new, Some(9), false);
        let u = b.unit(r);
        assert_eq!(u.kind as u16, new);
        assert_eq!((u.level, u.exp, u.exp_next), (1, 0, nc.exp_per_level));
        let a = (al0 - old.alignment as i32).max(0);
        assert_eq!(u.charm as i32, (a + nc.alignment as i32).min(0x6e));
        assert!(u.equip.iter().all(|&e| e == 0), "everything unequipped");
        let inv1: u32 = b.army(Side::Player).inventory.iter().map(|&c| c as u32).sum();
        assert_eq!(inv1, inv0 + n_eq as u32);
        assert!(u.hp_max >= hp_unequipped, "growth and base catch-up never lower HP");
        assert!(u.prof[6 + 9] >= 300, "chosen school +300");
        // auto-equip brings gear back
        b.auto_equip(r);
        assert!(b.unit(r).equip.iter().any(|&e| e != 0));
    }

    #[test]
    fn hidden_class_requirements() {
        let Some(b) = stage1() else { return eprintln!("classchange tests: no game data, skipped") };
        let t = b.t();
        let u = b.unit(UnitRef::new(Side::Player, 0)).clone();
        for (i, c) in t.kinds.classes.iter().enumerate() {
            let id = (i + 1) as u16;
            let flags_ok = c.change_flags == 0 || u.abilities & c.change_flags != 0;
            let item_ok = c.change_item == 0 || u.equip.contains(&c.change_item);
            assert_eq!(b.can_change_to(&u, id), flags_ok && item_ok, "{}", c.name);
        }
    }
}
