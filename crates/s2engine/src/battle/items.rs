//! Items: equippability, equipment stat totals, (un)equipping, auto-equip and
//! consumable effects. Owned by the RULES port; ported from the original
//! with its rounding, caps (and quirks) intact.
//!
//! ITEMS API (stable)
//! ------------------
//!   `Battle::item_equippable(&u, id) -> bool`        FUN_0043a5d3
//!   `Battle::weapon_usable(&u, id) -> bool`          FUN_0043a748
//!   `alignment_masks(&u, has_0x40) -> (sex, align)`  FUN_0043cf34
//!   `Battle::add_item_stats(&mut u, id)`             FUN_00446d1a
//!   `Battle::equip_item(r, id)`                      FUN_00446fe6 (takes it from the army inventory)
//!   `Battle::unequip_slot(r, slot 1..=6, consume)`   FUN_004470e7 (returns it unless `consume`)
//!   `Battle::consume_equipped(r, ability)`           FUN_0044bf12
//!   `Battle::auto_equip(r)`                          FUN_0044bd12
//!   `Battle::use_item(r, id, between_stages, hooks) -> i32`   FUN_0044a619 (amount for the popup)
//!   `Battle::learn_school_range(r, school, from, to)`   FUN_0044bfb2 (state part)
//!
//! The inventory decrement for a used consumable and all drawing are the
//! caller's (FUN_00447490 / view).

use super::army::UnitRef;
use super::rules;
use super::spells::EffectHooks;
use super::unit::{buff, flag, status, Unit};
use super::Battle;

/// Port of FUN_0043cf34: (gender mask 8/0x10/0x20, alignment mask 1/2/4).
/// `has_0x40` = the unit has item ability 0x40 (+10 alignment).
pub fn alignment_masks(u: &Unit, has_0x40: bool) -> (u16, u16) {
    let sex = match u.sex {
        0 => 8,
        1 => 0x10,
        _ => 0x20,
    };
    let a = u.charm as i32 + if has_0x40 { 10 } else { 0 };
    let al = if a < 0x1f {
        1
    } else if a < 0x46 {
        2
    } else {
        4
    };
    (sex, al)
}

impl Battle {
    /// Port of FUN_0043a5d3: the unit may wear/wield item `id`.
    pub fn item_equippable(&self, u: &Unit, id: u16) -> bool {
        let Some(it) = self.item(id) else { return false };
        let r = it.restrict;
        if r & 0x40 != 0 && u.char_id != 0xb {
            return false;
        }
        if r & 0x80 != 0 && u.char_id != 0x16 {
            return false;
        }
        if r & 0x100 != 0 && u.char_id != 1 {
            return false;
        }
        let ty = self.t().kinds.class(u.kind as u16).map_or(0, |c| c.unit_type) as u32;
        let any_armour = u.abilities & 0x800_0000 != 0;
        if (!any_armour || (it.kind as i8) < 7) && it.unit_types & (1u32 << (ty.wrapping_sub(1) & 0x1f)) == 0 {
            return false;
        }
        let (sex, al) = alignment_masks(u, self.has_item_ability(u, 0x40));
        if r & sex == 0 {
            return false;
        }
        any_armour || r & al != 0
    }

    /// Port of FUN_0043a748: the class has an action of the weapon's kind
    /// (action kind = item kind + 1). The original's guard is always true.
    pub fn weapon_usable(&self, u: &Unit, id: u16) -> bool {
        let Some(it) = self.item(id) else { return false };
        let t = self.t();
        let Some(c) = t.kinds.class(u.kind as u16) else { return false };
        let want = it.kind as i8 as i32 + 1;
        t.kinds.actions[c.actions.clone()].iter().any(|a| a.kind as i8 as i32 == want)
    }

    /// Port of FUN_00446d1a: add an item's ATK/DEF/MAG/AP to base, current and equipment totals.
    pub fn add_item_stats(&self, u: &mut Unit, id: u16) {
        let Some(it) = self.item(id) else { return };
        let atk = if (u.atk as i32) + (it.attack as i32) < 1000 { it.attack } else { 999 - u.atk };
        u.atk_cur += atk;
        u.atk += atk;
        u.eq_atk += atk;
        let d = it.defense as i16;
        let def = if (u.def as i32) + (d as i32) < 1000 { d } else { 999 - u.def };
        u.def_cur += def;
        u.def += def;
        u.eq_def += def;
        let m = it.magic as i16;
        let mag = if (u.mag as i32) + (m as i32) < 1000 { m } else { 999 - u.mag };
        u.mag_cur += mag;
        u.mag += mag;
        u.eq_mag += mag;
        let a = it.ap as i16;
        // The original's cap expression really is `999 - ap_max` against a 110 limit.
        let ap = if (u.ap_max as i32) + (a as i32) < 0x6f { a } else { 999 - u.ap_max };
        u.ap_base = (u.ap_base + ap).max(0);
        u.ap = (u.ap + ap).max(0);
        u.ap_max += ap;
        u.eq_ap += ap;
    }

    /// Port of FUN_004470e7: take the item out of `slot` (1..=6), removing its
    /// stats; back to the inventory unless `consume` (count < 99).
    pub fn unequip_slot(&mut self, r: UnitRef, slot: usize, consume: bool) {
        if !(1..=6).contains(&slot) {
            return;
        }
        let id = {
            let u = self.unit_mut(r);
            let id = u.equip[slot - 1];
            u.equip[slot - 1] = 0;
            id
        };
        let n_items = self.tables.as_ref().map_or(0, |t| t.items.len());
        if id == 0 || id as usize > n_items {
            return;
        }
        if !consume {
            let inv = &mut self.army_mut(r.side).inventory;
            if let Some(c) = inv.get_mut(id as usize - 1) {
                if *c < 99 {
                    *c += 1;
                }
            }
        }
        let it = self.item(id).expect("item id checked");
        let u = self.unit_mut(r);
        if it.attack != 0 {
            let v = it.attack;
            u.eq_atk -= v;
            u.atk_cur = (u.atk_cur - v).max(0);
            u.atk = (u.atk - v).max(0);
        }
        if it.defense != 0 {
            let v = it.defense as i16;
            u.eq_def -= v;
            u.def_cur = (u.def_cur - v).max(0);
            u.def = (u.def - v).max(0);
        }
        if it.magic != 0 {
            let v = it.magic as i16;
            u.eq_mag -= v;
            u.mag_cur -= v;
            // Quirk: a negative 魔力 zeroes 防禦 instead (as in the original).
            if u.mag_cur < 0 {
                u.def_cur = 0;
            }
            u.mag -= v;
            if u.mag < 0 {
                u.def = 0;
            }
        }
        if it.ap != 0 {
            let v = it.ap as i16;
            u.eq_ap -= v;
            u.ap_base = (u.ap_base - v).max(0);
            u.ap = (u.ap - v).max(0);
            u.ap_max = (u.ap_max - v).max(0);
        }
        if it.usage == 0 && (it.effect as i8) > 0 {
            self.recalc_army_flags(r.side, 0);
        }
    }

    /// Port of FUN_00446fe6: equip item `id` from the army inventory into
    /// its slot (weapon kinds 1..=6 → slot 1, armour kinds 7..=11 → slots 2..=6).
    /// The slot must already be empty (callers unequip first).
    pub fn equip_item(&mut self, r: UnitRef, id: u16) {
        let n_items = self.tables.as_ref().map_or(0, |t| t.items.len());
        if id == 0 || id as usize > n_items || self.army(r.side).inventory[id as usize - 1] == 0 {
            return;
        }
        self.army_mut(r.side).inventory[id as usize - 1] -= 1;
        let it = self.item(id).expect("item id checked");
        let k = it.kind as i8 as i32;
        let slot = match k {
            1..=6 => 0,
            7..=11 => (k - 6) as usize,
            _ => return,
        };
        let mut u = self.unit(r).clone();
        u.equip[slot] = id;
        self.add_item_stats(&mut u, id);
        *self.unit_mut(r) = u;
        if it.usage == 0 && (it.effect as i8) > 0 {
            self.recalc_army_flags(r.side, 0);
        }
    }

    /// Port of FUN_0044bf12: consume the first equipped item (usage 0 or 1) with `ability`.
    pub fn consume_equipped(&mut self, r: UnitRef, ability: u8) {
        let u = self.unit(r);
        let slot = (0..6).find(|&s| {
            let id = u.equip[s];
            id != 0 && self.item(id).is_some_and(|it| it.effect == ability && (it.usage == 0 || it.usage == 1))
        });
        if let Some(s) = slot {
            self.unequip_slot(r, s + 1, true);
        }
    }

    /// Port of FUN_0044bd12: equip the highest-rank owned item for every slot,
    /// then drop the shield (slot 5) when wielding a bow (item kind 4).
    pub fn auto_equip(&mut self, r: UnitRef) {
        let t = self.t();
        for slot in 0..6usize {
            let mut best = 0u16;
            let mut best_rank = 0u8;
            for (i, it) in t.items.iter().enumerate() {
                let id = (i + 1) as u16;
                let k = it.kind as i8 as i32;
                if self.army(r.side).inventory[i] == 0 {
                    continue;
                }
                let fits = (slot > 0 && slot as i32 == k - 6)
                    || (slot == 0 && (1..7).contains(&k) && self.weapon_usable(self.unit(r), id));
                if fits && self.item_equippable(self.unit(r), id) && best_rank < it.rank {
                    best = id;
                    best_rank = it.rank;
                }
            }
            let cur = self.unit(r).equip[slot];
            if cur != 0 {
                let u = self.unit(r);
                let keep = self.item_equippable(u, cur)
                    && (slot != 0 || self.weapon_usable(u, cur))
                    && self.item(cur).is_some_and(|it| it.rank > best_rank);
                if keep {
                    continue;
                }
                self.unequip_slot(r, slot + 1, false);
            }
            if best != 0 {
                self.equip_item(r, best);
            }
        }
        let u = self.unit(r);
        if u.equip[4] != 0 && u.equip[0] != 0 && self.item(u.equip[0]).is_some_and(|w| w.kind == 4) {
            self.unequip_slot(r, 5, false);
        }
    }

    /// Port of FUN_0044bfb2 (state part): learn spells `from..=to` (1-based)
    /// of `school` (1-based) and raise the school EXP to each spell's level.
    /// Returns the learned spell ids (0-based into the spell table) for the popups.
    pub fn learn_school_range(&mut self, r: UnitRef, school: i32, from: i32, to: i32) -> Vec<usize> {
        let t = self.t();
        let sp = &t.spells;
        let mut out = Vec::new();
        if !(school >= 1 && school <= sp.schools.len() as i32 && from > 0) {
            return out;
        }
        let (first, count) = sp.school_spells[school as usize - 1];
        if to > count as i32 {
            return out;
        }
        let u = self.unit_mut(r);
        for k in (from - 1)..to {
            let bit = ((1u32 << (k & 0x1f)) & 0xffff) as u16;
            let mask = &mut u.learned[school as usize - 1];
            if *mask & bit == 0 {
                *mask |= bit;
                let s = first as usize + k as usize;
                let lv = sp.spells[s].level as i32;
                let slot = (school + 5) as usize;
                if ((u.prof[slot] / 10) as i32) < lv {
                    u.prof[slot] = (lv * 10) as u16;
                }
                out.push(s);
            }
        }
        out
    }

    /// Port of FUN_0044a619: apply consumable `id` to unit `r` (used by its own
    /// army). Draws `rnd(6 - difficulty)` first, as the original does for every item.
    /// Returns the amount the original shows in the popup (0 = none).
    /// `between_stages` is `DAT_0049466c` (items used from the camp/world-map menu).
    pub fn use_item(&mut self, r: UnitRef, id: u16, between_stages: bool, hooks: &mut dyn EffectHooks) -> i32 {
        let turns = self.rng.rnd(6 - self.difficulty as i32) + 1;
        let Some(it) = self.item(id) else { return 0 };
        let amount = it.attack as i32;
        let cursed = self.unit(r).status & status::CURSE != 0;
        let mut shown = 0;
        // Gain capped to the room below a maximum.
        let room = |cur: i16, max: i16, v: i32| -> i32 { if (max as i32) - (cur as i32) < v { (max - cur) as i32 } else { v } };
        match it.effect {
            4 => {
                self.unit_mut(r).apply_status(turns, status::REFLECT);
                shown = turns;
            }
            5 if !cursed => {
                let u = self.unit_mut(r);
                shown = ((u.hp_max - u.hp) as i32).max(0);
                u.hp = u.hp_max;
            }
            6 | 10 if !cursed => {
                let u = self.unit_mut(r);
                shown = room(u.mp, u.mp_max, if it.effect == 6 { 0x3c } else { 0x1e });
                u.mp += shown as i16;
            }
            9 => {
                self.unit_mut(r).apply_status(turns, status::BARRIER);
                shown = turns;
            }
            0x14 => {
                self.unit_mut(r).apply_status(turns, status::LEVITATE);
                shown = turns;
            }
            0x1b if !between_stages => self.item_slay_random(r, hooks),
            0x20 if !between_stages => self.item_quake(r, hooks),
            0x21 if !cursed => {
                let u = self.unit_mut(r);
                shown = room(u.hp, u.hp_max, amount);
                u.hp += shown as i16;
            }
            0x22 if !cursed => {
                let u = self.unit_mut(r);
                shown = room(u.mp, u.mp_max, amount);
                u.mp += shown as i16;
            }
            0x23 if !cursed => {
                let u = self.unit_mut(r);
                u.buffs &= 0xffff_b974;
                u.status &= 0xfef0_0fff;
                u.recompute_buffs();
                shown = amount;
                u.hp += room(u.hp, u.hp_max, amount) as i16;
                u.mp += room(u.mp, u.mp_max, amount) as i16;
                u.st += room(u.st, u.st_max, amount) as i16;
            }
            0x24 => {
                let u = self.unit_mut(r);
                u.buffs &= !buff::POISON;
                u.recompute_buffs();
            }
            0x25 => {
                self.unit_mut(r).cure_petrify();
            }
            0x26 | 0x27 => {
                let u = self.unit_mut(r);
                u.buffs &= if it.effect == 0x26 { 0xffff_bb74 } else { 0xffff_b974 };
                u.status &= 0xfef0_0fff;
                u.recompute_buffs();
            }
            0x28 => {
                shown = amount;
                let u = self.unit_mut(r);
                // prof[13] (school 8, 魔合) += amount*10, cap 15000
                let v = (u.prof[13] as i32 + amount * 10) as u16;
                u.prof[13] = v.min(15000);
            }
            0x2b => {
                let u = self.unit_mut(r);
                shown = u.atk_cur as i32 / 2;
                u.apply_berserk(shown as i16, turns);
            }
            0x2c..=0x31 => {
                // Permanent +amount (cap 999) to the base stat and, where it has one, the current copy.
                let gain = |base: i16| -> i16 { (if 999 < base as i32 + amount { 999 - base as i32 } else { amount }).max(0) as i16 };
                let u = self.unit_mut(r);
                let v = match it.effect {
                    0x2c => {
                        let v = gain(u.atk);
                        u.atk += v;
                        u.atk_cur += v;
                        v
                    }
                    0x2d => {
                        let v = gain(u.def);
                        u.def += v;
                        u.def_cur += v;
                        v
                    }
                    0x2e => {
                        let v = gain(u.react);
                        u.react += v;
                        v
                    }
                    0x2f => {
                        let v = gain(u.mag);
                        u.mag += v;
                        u.mag_cur += v;
                        v
                    }
                    0x30 => {
                        let v = gain(u.skill);
                        u.skill += v;
                        u.skill_cur += v;
                        v
                    }
                    _ => {
                        let v = gain(u.sword);
                        u.sword += v;
                        v
                    }
                };
                shown = v as i32;
            }
            0x32 | 0x33 if !cursed => {
                let u = self.unit_mut(r);
                let (max, cur, cap) =
                    if it.effect == 0x32 { (&mut u.hp_max, &mut u.hp, 5000) } else { (&mut u.mp_max, &mut u.mp, 999) };
                let mut v = amount;
                if cap < *max as i32 + v {
                    v = cap - *max as i32;
                }
                v = v.max(0);
                *max += v as i16;
                *cur += v as i16;
                shown = v;
            }
            0x34 => {
                let u = self.unit_mut(r);
                let mut v = amount;
                if 0x6e < u.ap_max as i32 + v {
                    v = 0x6e - u.ap_max as i32;
                }
                v = v.max(0);
                u.ap_max += v as i16;
                u.ap_base += v as i16;
                if between_stages {
                    u.ap += v as i16;
                }
                shown = v;
            }
            0x35 => {
                self.unit_mut(r).apply_status(turns, status::INVISIBLE);
                shown = turns;
            }
            0x36 => self.unit_mut(r).flags |= flag::BONUS_GROWTH,
            0x3b => self.unit_mut(r).status &= !status::PARALYSIS,
            0x3c if !cursed => {
                let u = self.unit_mut(r);
                shown = room(u.st, u.st_max, amount);
                u.st += shown as i16;
            }
            0x3d => {
                let u = self.unit_mut(r);
                let mut v = 100;
                if 5000 < u.hp_max as i32 + 100 {
                    v = 5000 - u.hp_max as i32;
                }
                v = v.max(0);
                u.hp_max += v as i16;
                u.hp += v as i16;
                shown = v;
                transform_stats(u);
            }
            0x41 => {
                let to = it.hit as i8 as i32;
                let l = self.learn_school_range(r, amount, to, to);
                hooks.learned(self, r, amount, &l, true);
            }
            0x42 => {
                let to = it.hit as i8 as i32;
                let l = self.learn_school_range(r, amount, 1, to);
                hooks.learned(self, r, amount, &l, true);
            }
            0x43 => {
                let t = self.t();
                let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
                let p = rules::rating(&ctx, &self.fighter(r), rules::RatingMode::Current, 0);
                let u = self.unit_mut(r);
                let v = u.exp as i32 + if p < 3000 { 999 } else { 100 };
                u.exp = v.min(999) as u16;
            }
            _ => {}
        }
        // The caller shows FUN_00439cc0(unit, -shown) when not between stages.
        shown
    }

    /// Port of FUN_0044b41b (item ability 0x1b): 30% to slay a random
    /// non-boss deployed enemy (FUN_0043cfce).
    fn item_slay_random(&mut self, r: UnitRef, hooks: &mut dyn EffectHooks) {
        let foe = r.side.other();
        let cands: Vec<usize> = self
            .army(foe)
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0 && u.rank != 1)
            .map(|(i, _)| i)
            .collect();
        let victim = if cands.is_empty() { None } else { Some(cands[self.rng.rnd(cands.len() as i32) as usize]) };
        // No victim, or the 30% failed: msg 0xe4 (0x390 / 4).
        let Some(v) = victim else {
            hooks.message(self, r, 0xe4);
            return;
        };
        if self.rng.pct() > 0x1e {
            hooks.message(self, r, 0xe4);
            return;
        }
        hooks.slay(self, r, UnitRef::new(foe, v));
    }

    /// Port of FUN_0044b515 + FUN_0044b594 (item ability 0x20): every deployed
    /// ground unit of both armies loses half its max HP (capped at HP); the
    /// item is consumed.
    fn item_quake(&mut self, r: UnitRef, hooks: &mut dyn EffectHooks) {
        let mut hits = Vec::new();
        for side in [r.side, r.side.other()] {
            for i in 0..self.army(side).units.len() {
                let u = &self.army(side).units[i];
                if u.flags & flag::DEAD != 0 || u.flags & flag::DEPLOYED == 0 || self.move_type(u) != 0 {
                    continue;
                }
                let ur = UnitRef::new(side, i);
                let u = self.unit_mut(ur);
                let v = (u.hp_max as i32 / 2).min(u.hp as i32);
                u.hp -= v as i16;
                hits.push((ur, v));
                if u.hp == 0 {
                    hooks.die(self, ur, 0);
                }
            }
        }
        hooks.quake(self, r, &hits);
        self.consume_equipped(r, 0x20);
    }
}

/// Port of FUN_0044b725: pull stats a third of the way toward 250 (攻擊) / 130 (others).
fn transform_stats(u: &mut Unit) {
    fn fix(v: &mut i16) {
        if *v < 0 {
            *v = 1;
        } else if *v > 999 {
            *v = 999;
        }
    }
    let d = (0xfa - u.atk) / 3;
    u.atk += d;
    fix(&mut u.atk);
    u.atk_cur += d;
    fix(&mut u.atk_cur);
    if u.mp_max > 0 {
        let d = (0x82 - u.mp_max) / 3;
        u.mp_max += d;
        fix(&mut u.mp_max);
        u.mp += d;
        fix(&mut u.mp);
    }
    if u.st_max > 0 {
        let d = (0x82 - u.st_max) / 3;
        u.st_max += d;
        fix(&mut u.st_max);
        u.st += d;
        fix(&mut u.st);
    }
    let d = (0x82 - u.mag) / 3;
    u.mag += d;
    fix(&mut u.mag);
    u.mag_cur += d;
    fix(&mut u.mag_cur);
    let d = (0x82 - u.def) / 3;
    u.def += d;
    fix(&mut u.def);
    u.def_cur += d;
    fix(&mut u.def_cur);
    let d = (0x82 - u.skill) / 3;
    u.skill += d;
    fix(&mut u.skill);
    u.skill_cur += d;
    fix(&mut u.skill_cur);
    let d = (0x82 - u.sword) / 3;
    u.sword += d;
    fix(&mut u.sword);
    let d = (0x82 - u.react) / 3;
    u.react += d;
    fix(&mut u.react);
}

#[cfg(test)]
mod tests {
    use super::super::army::{Side, UnitRef};
    use super::super::spells::tests::stage1;
    use super::super::spells::MarkDead;

    #[test]
    fn unequip_equip_round_trip() {
        let Some(mut b) = stage1() else { return eprintln!("items tests: no game data, skipped") };
        let r = UnitRef::new(Side::Player, 0); // 志超
        let before = b.unit(r).clone();
        let w = before.equip[0];
        assert!(w != 0);
        let inv0 = b.army(Side::Player).inventory[w as usize - 1];
        let watk = b.item(w).unwrap().attack;
        b.unequip_slot(r, 1, false);
        assert_eq!(b.unit(r).equip[0], 0);
        assert_eq!(b.unit(r).atk, before.atk - watk);
        assert_eq!(b.unit(r).eq_atk, before.eq_atk - watk);
        assert_eq!(b.army(Side::Player).inventory[w as usize - 1], inv0 + 1);
        assert!(b.item_equippable(b.unit(r), w));
        assert!(b.weapon_usable(b.unit(r), w));
        b.equip_item(r, w);
        assert_eq!(b.unit(r), &before);
        assert_eq!(b.army(Side::Player).inventory[w as usize - 1], inv0);
        // consuming does not return the item
        b.unequip_slot(r, 1, true);
        assert_eq!(b.army(Side::Player).inventory[w as usize - 1], inv0);
    }

    #[test]
    fn restrictions_and_auto_equip() {
        let Some(mut b) = stage1() else { return eprintln!("items tests: no game data, skipped") };
        let t = b.t();
        let hero = UnitRef::new(Side::Player, 0);
        // a staff (kind 3) is not for a 劍士-type unit
        let staff = t.items.iter().find(|i| i.kind == 3).map(|i| i.id).unwrap();
        assert!(!b.item_equippable(b.unit(hero), staff));
        // give the army a better sword than the current one and auto-equip
        let cur = b.unit(hero).equip[0];
        let cur_rank = b.item(cur).unwrap().rank;
        let better = t
            .items
            .iter()
            .filter(|i| i.kind == 1 && i.rank > cur_rank && i.restrict & 0x1c0 == 0)
            .find(|i| b.item_equippable(b.unit(hero), i.id))
            .map(|i| i.id);
        if let Some(better) = better {
            b.army_mut(Side::Player).inventory[better as usize - 1] = 1;
            b.auto_equip(hero);
            let best_rank = t.items[b.unit(hero).equip[0] as usize - 1].rank;
            assert!(best_rank > cur_rank);
            assert_eq!(b.army(Side::Player).inventory[cur as usize - 1], 1, "old weapon returned");
        }
    }

    #[test]
    fn consumables() {
        let Some(mut b) = stage1() else { return eprintln!("items tests: no game data, skipped") };
        let t = b.t();
        let r = UnitRef::new(Side::Player, 0);
        let potion = t.items.iter().find(|i| i.effect == 0x21 && i.usage != 0).map(|i| (i.id, i.attack as i32));
        if let Some((id, amount)) = potion {
            b.unit_mut(r).hp -= 10;
            let shown = b.use_item(r, id, false, &mut MarkDead::default());
            assert_eq!(shown, amount.min(10));
            assert_eq!(b.unit(r).hp, b.unit(r).hp_max - 10 + shown as i16);
        }
        // +ATK tonic is permanent and capped
        let tonic = t.items.iter().find(|i| i.effect == 0x2c).map(|i| i.id);
        if let Some(id) = tonic {
            let a0 = b.unit(r).atk;
            let v = b.use_item(r, id, true, &mut MarkDead::default());
            assert_eq!(b.unit(r).atk, a0 + v as i16);
        }
    }
}
