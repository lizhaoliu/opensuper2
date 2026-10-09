//! Magic, techniques, combos and summons: gating, costs, per-target effect
//! switches and learning. Owned by the RULES port. Formulas are
//! `battle::rules`; this file applies them to the live units.
//!
//! SPELLS API (stable)
//! -------------------
//!   `trait EffectHooks`                          death / summon creation / teleports (BATTLE implements)
//!   `Cast { side, caster, partner, spell, cell, from_item, technique }`   the SpellCaster object (DAT_004a1f58)
//!   `Battle::spell_shortage(&u, spell) -> Shortage`      FUN_0046586d
//!   `Battle::combo_partner(r, spell) -> Option<UnitRef>`  FUN_0046552e / FUN_0046570c (合擊, 情侶合技 via affinity)
//!   `Battle::spell_listed(r, spell, listing, browse) -> bool`  FUN_0045f145 (one summon at a time, combo partner)
//!   `Battle::pay_spell_cost(&cast)`                       FUN_0045f78b
//!   `Battle::learn_from_proficiency(r, school, gain) -> Vec<usize>`  learning loop of FUN_0045ff9e / FUN_00455d95
//!   `Battle::spell_roll(&cast, target, target_side, random) -> (chance, value, slot)`   FUN_00467f96
//!   `Battle::cast_spell(&mut cast, cells, hooks) -> CastOutcome`    FUN_0045ff9e (effects, EXP, learning)
//!   `summon_kind(spells, id)`, `summon_level(mag)`         FUN_004608e2 parameters
//!
//! Animation, sounds and popups are recorded in `Cast::shows` ([`Show`]) at
//! the places the original draws them, and played by `cast::play_shows`.

use super::army::{Side, UnitRef};
use super::combat::{fighter_of, rules_side};
use super::movement::facing_relation;
use super::rules;
use super::unit::{buff, flag, status, Unit};
use super::Battle;

/// Effects that leave the RULES scope; the BATTLE port provides them.
pub trait EffectHooks {
    /// FUN_0044d14b: `u` dies (mode 2 for summon-style kills).
    fn die(&mut self, b: &mut Battle, u: UnitRef, mode: i32);
    /// FUN_0043e5dd: create summon creature `kind` (0-based) for `side` at (x, y)
    /// with `level`, deployed, and register it in the army summon slot
    /// (FUN_00442022). `cast_spell` then sets its `npc` from the caster and `act = 2`.
    fn summon(&mut self, _b: &mut Battle, _side: Side, _kind: i32, _x: i32, _y: i32, _level: i32) -> Option<UnitRef> {
        None
    }
    /// FUN_00458334 (effect 4): move `u` to (x, y).
    fn teleport(&mut self, _b: &mut Battle, _u: UnitRef, _x: i32, _y: i32) {}
    /// FUN_0045844b (effect 0x27).
    fn effect_27(&mut self, _b: &mut Battle, _caster: UnitRef) {}
    /// FUN_00431f6d(0xa0, 0x50, msg(`msg`)) over the map with `unit` drawn
    /// (the item rules' failure messages).
    fn message(&mut self, _b: &mut Battle, _unit: UnitRef, _msg: usize) {}
    /// FUN_0044bfb2's windows: `unit` learned `spells` (0-based) of `school`
    /// (`draw_unit`: param_5 == 0, the unit is redrawn over the map first).
    fn learned(&mut self, _b: &mut Battle, _unit: UnitRef, _school: i32, _spells: &[usize], _draw_unit: bool) {}
    /// FUN_0044b41b: `user`'s item slays `victim` (default: a plain death).
    fn slay(&mut self, b: &mut Battle, _user: UnitRef, victim: UnitRef) {
        self.die(b, victim, 0);
    }
    /// FUN_0044b515's presentation: `user`'s quake item, then per unit hit
    /// (in FUN_0044b594's order) the HP it lost (`hp == 0` afterwards: it died).
    fn quake(&mut self, _b: &mut Battle, _user: UnitRef, _hits: &[(UnitRef, i32)]) {}
}

/// Hooks that only mark deaths (tests, headless evaluation).
#[derive(Debug, Default)]
pub struct MarkDead {
    pub died: Vec<(UnitRef, i32)>,
}

impl EffectHooks for MarkDead {
    fn die(&mut self, b: &mut Battle, u: UnitRef, mode: i32) {
        b.unit_mut(u).flags |= flag::DEAD;
        self.died.push((u, mode));
    }
}

/// FUN_0046586d result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortage {
    Ok = 0,
    Mp = 1,
    St = 2,
    Ap = 3,
    Hp = 4,
}

/// A cast in progress (`DAT_004a1f58`).
#[derive(Debug, Clone)]
pub struct Cast {
    /// +0x00 the caster's army.
    pub side: Side,
    /// +0x08
    pub caster: UnitRef,
    /// +0x0c combo partner.
    pub partner: Option<UnitRef>,
    /// +0x10 1-based spell id.
    pub spell: u16,
    /// +0x14/+0x18 target cell.
    pub cell: (i32, i32),
    /// +0x576 cast from an item/scroll.
    pub from_item: bool,
    /// +0x575 technique (1) or magic (0).
    pub technique: bool,
    /// +0x600 equipment slot picked by 0x22/0x26.
    pub slot: u8,
    /// +0x1c / +0x20 EXP accumulated for caster / partner; +0x24 proficiency gain.
    pub exp: i32,
    pub exp_partner: i32,
    pub prof_gain: i32,
    /// +0x28 item drops collected (count per item id − 1).
    pub drops: Vec<u8>,
    /// Per-target results of the current cast.
    pub hits: Vec<TargetHit>,
    /// What the original shows in the middle of the effects, in order.
    pub shows: Vec<Show>,
}

/// A presentation step of the effect code (FUN_00460c85, FUN_004636cd, the
/// status spells), recorded in order in `Cast::shows` and played by the
/// caller (`cast::play_shows`) once the rule step is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Show {
    /// Sound 0x76 and the hit flash FUN_0044d024.
    Hit(UnitRef),
    /// FUN_004636cd's popup: `amount` (+ damage, − heal, 0 "MISS") over
    /// `target`, counted into the HP in 1, 2, 4.. chunks when `apply`
    /// (with the forecast panel); `sign` is the clamped amount that picks
    /// the direction; HP of target and caster before it.
    Hp { target: UnitRef, amount: i32, sign: i32, apply: bool, reflect: bool, hp_before: (i16, i16) },
    /// FUN_0044d14b: `unit` dies (mode).
    Die(UnitRef, i32),
    /// FUN_004403b5(opponent army): its units are drawn into the map surface.
    Redraw,
    /// FUN_00440264(army, unit, 1): the unit drawn into the map surface after
    /// its look changed; `show` 1 also draws it on page 1 and presents that
    /// (FUN_0046136c), 2 on pages 1 and 0 and presents (FUN_00462144).
    Put(UnitRef, u8),
    /// FUN_0046313d (effect 0x23): the target's class sheet, until a click.
    Sheet(UnitRef),
    /// FUN_004198a4 + FUN_0046d2da at the top of the effect passes' cell
    /// loops (the palette cycling).
    Pump,
}

impl Cast {
    pub fn new(side: Side, caster: UnitRef, spell: u16, cell: (i32, i32)) -> Cast {
        Cast {
            side,
            caster,
            partner: None,
            spell,
            cell,
            from_item: false,
            technique: false,
            slot: 0,
            exp: 0,
            exp_partner: 0,
            prof_gain: 0,
            drops: Vec::new(),
            hits: Vec::new(),
            shows: Vec::new(),
        }
    }
}

/// One chunk `v` of FUN_004636cd's HP count: the target loses (`sign` > 0)
/// or gains (`sign` < 0) `v`; damage also costs the caster `v` for blood
/// price (0x29, no clamp), heals it for drain (0x2a, up to its max, not
/// when reflected) or, reflected, costs it `v` (down to 0).
pub(crate) fn apply_hp_chunk(b: &mut Battle, caster: UnitRef, eff: u8, target: UnitRef, sign: i32, reflect: bool, v: i32) {
    let v = v as i16;
    if sign < 0 {
        b.unit_mut(target).hp += v;
    } else if sign > 0 {
        b.unit_mut(target).hp -= v;
        let c = b.unit_mut(caster);
        if eff == 0x29 {
            c.hp -= v;
        } else if eff == 0x2a && c.hp < c.hp_max {
            if !reflect {
                c.hp = (c.hp + v).min(c.hp_max);
            }
        } else if reflect {
            c.hp = (c.hp - v).max(0);
        }
    }
}

/// Per-target record of a cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetHit {
    pub unit: UnitRef,
    /// HP change applied (+ damage, − heal), or the buff amount for support spells.
    pub amount: i32,
    pub affected: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CastOutcome {
    pub targets: Vec<TargetHit>,
    /// Spells learned (0-based indices into the spell table) by caster / partner.
    pub learned: Vec<usize>,
    pub learned_partner: Vec<usize>,
    /// EXP actually awarded (after scaling) to caster / partner.
    pub exp: i32,
    pub exp_partner: i32,
    /// The caster died (blood spell or reflected damage).
    pub caster_died: bool,
}

/// FUN_004608e2: summon creature index (0-based) of spell `id`: `id − first_of_school_7 − 1`.
pub fn summon_kind(spells: &s2data::tables::SpellTable, id: u16) -> i32 {
    id as i32 - spells.school_spells.get(6).map_or(0, |s| s.0 as i32) - 1
}

/// FUN_004608e2: summoned creature level `clamp(魔力cur/20 + 1, 1, 10)`.
pub fn summon_level(mag_cur: i32) -> i32 {
    (mag_cur / 0x14 + 1).clamp(1, 10)
}

fn bit16(i: usize) -> u16 {
    ((1u32 << (i & 0x1f)) & 0xffff) as u16
}

impl Battle {
    fn spell_def(&self, id: u16) -> s2data::tables::SpellDef {
        self.t().spells.spells[id as usize - 1].clone()
    }

    /// Port of FUN_0046586d: can `u` pay for `spell`?
    pub fn spell_shortage(&self, u: &Unit, spell: u16) -> Shortage {
        let sp = self.spell_def(spell);
        let c = sp.cost as i32;
        if sp.stat_flags & 1 != 0 {
            let need = if u.abilities & 0x20 != 0 { c / 2 } else { c };
            if (u.mp as i32) < need {
                return Shortage::Mp;
            }
        }
        if sp.stat_flags & 2 != 0 && (u.st as i32) < c {
            return Shortage::St;
        }
        if sp.stat_flags & 4 != 0 && (u.hp as i32) <= c {
            return Shortage::Hp;
        }
        let mut ap = self.c(0xef);
        if u.turn_flags & 0xc != 0 {
            ap += self.c(0xf3);
        }
        if (u.ap as i32) < ap { Shortage::Ap } else { Shortage::Ok }
    }

    /// Port of FUN_0046570c: is the unit at (x, y) a usable partner?
    fn partner_at(&self, r: UnitRef, x: i32, y: i32, h: i32, spell: u16) -> Option<UnitRef> {
        let sp = self.spell_def(spell);
        let id = self.army(r.side).unit_at(x, y);
        if id == 0 {
            return None;
        }
        let pr = UnitRef::new(r.side, id - 1);
        let p = self.unit(pr);
        if p.act != 0 || p.learned[sp.school as usize - 1] & bit16(sp.index as usize - 1) == 0 {
            return None;
        }
        let hp = self.map.as_ref().map_or(0, |m| m.height(x, y));
        if (h - hp).abs() >= 4 {
            return None;
        }
        if sp.combo & 6 == 0 {
            return Some(pr);
        }
        let c = self.unit(r);
        let ci = c.char_id as i32 - 1;
        let pi = p.char_id as i32 - 1;
        if !(0..=0x27).contains(&ci) || !(0..=0x28).contains(&pi) {
            return None;
        }
        // The original lets pi reach 40 and reads one byte past the 40-entry
        // table; that byte (outside the unit) is treated as 0 here.
        let a = |u: &Unit, i: i32| u.affinity.get(i as usize).copied().unwrap_or(0) as i8;
        (a(c, pi) > 0x45 && a(p, ci) > 0x45).then_some(pr)
    }

    /// Port of FUN_0046552e: first partner among the four neighbours
    /// (up-right, down-left, up-left, down-right).
    pub fn combo_partner(&self, r: UnitRef, spell: u16) -> Option<UnitRef> {
        let u = self.unit(r);
        let h = self.map.as_ref().map_or(0, |m| m.height(u.x, u.y));
        (0..4).find_map(|d| {
            let (x, y) = super::map::neighbour(u.x, u.y, d);
            self.partner_at(r, x, y, h, spell)
        })
    }

    /// Port of FUN_0045f145: the spell may be cast. `listing` (param 1, the
    /// menu builders) accepts every learned spell; otherwise summons need no
    /// active summon in the army and combos an adjacent partner, and none
    /// while `browse` (caster object +0x38: set by the spell info screen
    /// FUN_0045fd41, cleared when casting, FUN_0045fc21).
    pub fn spell_listed(&self, r: UnitRef, spell: u16, listing: bool, browse: bool) -> bool {
        let sp = self.spell_def(spell);
        if sp.combo & 7 == 0 {
            if !listing && sp.effect > 0x14 && sp.effect < 0x20 && self.army(r.side).summons.iter().any(|s| s.active == 1) {
                return false;
            }
            true
        } else if listing {
            true
        } else if browse {
            false
        } else {
            self.combo_partner(r, spell).is_some()
        }
    }

    /// Port of FUN_0045f78b: spend AP and MP/ST/HP for a cast (partner too).
    pub fn pay_spell_cost(&mut self, cast: &Cast) {
        let sp = self.spell_def(cast.spell);
        let flag_bit = if cast.technique { 8 } else { 4 };
        let (ef, f3) = (self.c(0xef), self.c(0xf3));
        let pay_ap = |u: &mut Unit| {
            u.ap -= ef as i16;
            if u.turn_flags & flag_bit == 0 {
                u.turn_flags |= flag_bit;
            } else {
                u.ap -= f3 as i16;
            }
            if u.ap < u.ap_carry {
                u.ap_carry = u.ap;
            }
        };
        pay_ap(self.unit_mut(cast.caster));
        let combo = sp.combo & 7 != 0;
        if combo {
            if let Some(p) = cast.partner {
                pay_ap(self.unit_mut(p));
            }
        }
        let free = self.t().params.per_spell[1].get(cast.spell as usize - 1) == Some(&2);
        if free {
            return;
        }
        let c = sp.cost;
        let mut who = vec![cast.caster];
        if combo {
            who.extend(cast.partner);
        }
        for r in who {
            let u = self.unit_mut(r);
            if sp.stat_flags & 1 != 0 {
                u.mp -= if u.abilities & 0x20 != 0 { c / 2 } else { c };
            }
            if sp.stat_flags & 2 != 0 {
                u.st -= c;
            }
            if sp.stat_flags & 4 != 0 {
                u.hp -= c;
                if u.hp < 1 {
                    u.hp = 1;
                }
            }
        }
    }

    /// The learning loop of FUN_0045ff9e: add `gain` to the school EXP
    /// (school 1..=11, cap 15000; skipped when already at the cap) and learn
    /// every spell whose level is reached (`combo & 0x80` spells need
    /// FUN_00457301). Returns the newly learned spell indices (0-based).
    pub fn learn_from_proficiency(&mut self, r: UnitRef, school: u8, gain: i32) -> Vec<usize> {
        let t = self.t();
        let slot = school as usize + 5;
        let mut out = Vec::new();
        let u = self.unit_mut(r);
        if u.prof[slot] >= 15000 {
            return out;
        }
        u.prof[slot] = ((u.prof[slot] as i32 + gain).clamp(0, 0xffff) as u32).min(15000) as u16;
        let (first, count) = t.spells.school_spells[school as usize - 1];
        for k in 0..count as usize {
            let s = first as usize + k;
            let sp = &t.spells.spells[s];
            let known = u.learned[school as usize - 1] & bit16(k) != 0;
            if sp.level as i32 <= (u.prof[slot] / 10) as i32 && !known && (sp.combo & 0x80 == 0 || special_learn(u, school, k + 1)) {
                u.learned[school as usize - 1] |= bit16(k);
                out.push(s);
            }
        }
        out
    }

    fn spell_cast_view(&self, cast: &Cast, target: &Unit) -> (rules::Fighter, Option<rules::Fighter>, u8) {
        let c = self.fighter(cast.caster);
        let p = cast.partner.map(|p| self.fighter(p));
        let cu = self.unit(cast.caster);
        let facing = facing_relation(cu.x, cu.y, target.x, target.y, target.facing, self.difficulty == 1) as u8;
        (c, p, facing)
    }

    /// Port of FUN_00467f96 (random mode): (chance, value, destroy slot) of the cast on `target`.
    pub fn spell_roll(&mut self, cast: &Cast, target: UnitRef, target_side: Side, random: bool) -> (i32, i32, u8) {
        let t = self.t();
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        let (c, p, facing) = self.spell_cast_view(cast, self.unit(target));
        let tf = self.fighter(target);
        let sc = rules::SpellCast { caster: &c, partner: p.as_ref(), spell: cast.spell, side: rules_side(cast.side), facing };
        let mut slot = cast.slot;
        let chance = rules::spell_chance(&ctx, &mut self.rng, random, &sc, &tf, target_side == cast.side, &mut slot);
        if chance == 0 {
            return (0, 0, slot);
        }
        let value = rules::spell_value(&ctx, rules::roll(&mut self.rng, random), &sc, &tf);
        (chance, value, slot)
    }

    /// Port of FUN_004636cd (state part): apply `amount` to `target`
    /// (+ damage, − heal), with 反彈 reflection and the caster side effects
    /// of effects 0x29 (blood price) and 0x2a (drain). The heal clamp to the
    /// missing HP only changes the sign the original tests: the HP is
    /// counted by |amount| as passed (no heal at full HP, but a heal larger
    /// than the missing HP overshoots, as in the original; the heal spells
    /// clamp before calling). The popup is recorded in `cast.shows`.
    fn spell_apply_hp(&mut self, cast: &mut Cast, target: UnitRef, amount: i32, apply: bool) -> i32 {
        let eff = self.spell_def(cast.spell).effect;
        let reflect = self.unit(target).status & status::REFLECT != 0 && amount >= 1 && apply && !cast.technique;
        let hp_before = (self.unit(target).hp, self.unit(cast.caster).hp);
        let v = amount.abs();
        let mut sign = amount;
        {
            let t = self.unit_mut(target);
            t.hp_before_hit = t.hp;
            if amount < 0 && (t.hp_max as i32 - t.hp as i32) < v {
                sign = t.hp as i32 - t.hp_max as i32;
            }
        }
        cast.shows.push(Show::Hp { target, amount, sign, apply, reflect, hp_before });
        if !apply || amount == 0 {
            return 0;
        }
        apply_hp_chunk(self, cast.caster, eff, target, sign, reflect, v);
        sign
    }

    fn rating_before_hit(&self, r: UnitRef) -> i32 {
        let t = self.t();
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        rules::rating(&ctx, &fighter_of(self.unit(r)), rules::RatingMode::BeforeHit, 0)
    }

    /// Port of FUN_00463c84: EXP for affecting `target` by `amount` (`per_amount` = the
    /// heal/kill path), added to the caster (`acc.0`) and partner (`acc.1`) accumulators.
    fn spell_exp(&mut self, cast: &Cast, target: UnitRef, target_side: Side, amount: i32, per_amount: bool, acc: &mut (i32, i32)) {
        let sp = self.spell_def(cast.spell);
        let combo = sp.combo & 7 != 0;
        if amount == 0 && self.unit(target).hp != 0 {
            acc.0 += 1;
            if combo {
                acc.1 += 1;
            }
            return;
        }
        let pa = self.rating_before_hit(cast.caster);
        let pt = self.rating_before_hit(target);
        let pp = if combo { cast.partner.map_or(0, |p| self.rating_before_hit(p)) } else { 0 };
        let div = |a: i32, b: i32| if b == 0 { 0 } else { a / b };
        let same = target_side == cast.side;
        let mut ep = 0;
        let mut e = if per_amount {
            div(amount, self.c(0xe3)).min(0x3c)
        } else {
            match sp.effect {
                1 | 0x24 | 0x2a | 0x30 | 0x40 => {
                    let a = amount * if cast.technique { self.c(0xe6) } else { self.c(0xe7) };
                    if combo {
                        ep = if same { 0 } else { div(div(a * pt, pt + pp), self.c(0xe1)) };
                    }
                    if same { 0 } else { div(div(a * pt, pt + pa), self.c(0xe1)) }
                }
                _ => {
                    // FUN_00467fe9 against the caster's own army, in random mode.
                    let t = self.t();
                    let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
                    let (c, p, facing) = self.spell_cast_view(cast, self.unit(target));
                    let tf = self.fighter(target);
                    let sc = rules::SpellCast { caster: &c, partner: p.as_ref(), spell: cast.spell, side: rules_side(cast.side), facing };
                    let mut slot = cast.slot;
                    let ch = rules::spell_chance(&ctx, &mut self.rng, true, &sc, &tf, true, &mut slot).max(1);
                    let e = sp.cost as i32 * self.c(0xe4) / ch;
                    if combo {
                        ep = e;
                    }
                    e
                }
            }
        };
        if self.unit(target).hp == 0 && !same {
            e += div(pt * self.c(0xe2), pa) + 3;
            if combo {
                ep += div(pt * self.c(0xe2), pp) + 3;
            }
        }
        acc.0 += e.max(1);
        if combo {
            acc.1 += ep.max(1);
        }
    }

    /// `spell_exp` into the cast's own accumulators (+0x1c / +0x20).
    fn spell_exp_cast(&mut self, cast: &mut Cast, target: UnitRef, target_side: Side, amount: i32, per_amount: bool) {
        let mut acc = (cast.exp, cast.exp_partner);
        self.spell_exp(cast, target, target_side, amount, per_amount, &mut acc);
        (cast.exp, cast.exp_partner) = acc;
    }

    /// Port of FUN_00463b44: kill EXP and drops.
    fn spell_kill_rewards(&mut self, cast: &mut Cast, target: UnitRef, target_side: Side) {
        self.spell_exp_cast(cast, target, target_side, 0, true);
        let n = self.t().items.len();
        if cast.drops.len() < n {
            cast.drops.resize(n, 0);
        }
        let v = self.unit(target).clone();
        let mut add = |id: u16| {
            if let Some(c) = (id as usize).checked_sub(1).and_then(|i| cast.drops.get_mut(i)) {
                *c = c.wrapping_add(1);
            }
        };
        if self.vs {
            v.equip.iter().filter(|&&e| e != 0).for_each(|&e| add(e));
        } else if self.army(cast.side).side != Side::Player || self.unit(cast.caster).npc != 1 {
            v.drops.iter().filter(|&&e| e != 0).for_each(|&e| add(e));
        }
    }

    /// Revive items or death after HP reached 0 (shared tail of the effect functions).
    /// Returns true if the target died.
    fn spell_check_death(&mut self, cast: &mut Cast, target: UnitRef, side: Side, mode: i32, reset_exp: bool, hooks: &mut dyn EffectHooks) -> bool {
        let t = self.unit(target);
        if self.has_item_ability(t, 0x19) {
            let m = t.hp_max as i32;
            self.spell_apply_hp(cast, target, -m, true);
            return false;
        }
        if self.has_item_ability(t, 0x1c) {
            let m = t.hp_max as i32;
            self.spell_apply_hp(cast, target, -m, true);
            self.consume_equipped(target, 0x1c);
            return false;
        }
        hooks.die(self, target, mode);
        cast.shows.push(Show::Die(target, mode));
        if reset_exp {
            cast.exp = 0;
        }
        self.spell_kill_rewards(cast, target, side);
        self.unit_mut(cast.caster).kills += 1;
        true
    }

    /// Port of FUN_004630ab: a spell hit wakes the target and may cure paralysis.
    fn spell_wake(&mut self, spell: u16, target: UnitRef) {
        let eff = self.spell_def(spell).effect;
        let u = self.unit_mut(target);
        u.status &= !status::SLEEP;
        if matches!(eff, 1 | 0x24 | 0x2a | 0x30 | 0x40) && self.unit(target).status & status::PARALYSIS != 0 && self.rng.pct() < 0x47 {
            self.unit_mut(target).status &= !status::PARALYSIS;
        }
    }

    /// Port of FUN_00460c85: damage spell on one cell. Returns true if damage was dealt.
    pub(crate) fn spell_damage_cell(&mut self, cast: &mut Cast, x: i32, y: i32, hooks: &mut dyn EffectHooks) -> bool {
        let sp = self.spell_def(cast.spell);
        let foe = cast.side.other();
        let (target, side) = match self.unit_at_cell(foe, x, y, false) {
            Some(t) => {
                let u = self.unit_mut(t);
                if u.ai_mode == 2 {
                    u.ai_mode = 1;
                    u.ai_sub = 0;
                }
                (t, foe)
            }
            None => {
                if sp.target != 3 {
                    return false;
                }
                let Some(t) = self.unit_at_cell(cast.side, x, y, false) else { return false };
                if t == cast.caster || (sp.combo & 7 != 0 && Some(t) == cast.partner) {
                    return false;
                }
                (t, cast.side)
            }
        };
        // FUN_0046c85b
        let (chance, value, slot) = self.spell_roll(cast, target, side, true);
        cast.slot = slot;
        let dmg = if chance == 0 {
            0
        } else {
            let mut v = value.min(self.unit(target).hp as i32);
            if v == 0 {
                v = 1;
            }
            if chance < self.rng.pct() { 0 } else { v }
        };
        if dmg != 0 {
            cast.shows.push(Show::Hit(target));
            self.spell_wake(cast.spell, target);
        }
        self.spell_apply_hp(cast, target, dmg, true);
        cast.hits.push(TargetHit { unit: target, amount: dmg, affected: dmg != 0 });
        if self.unit(target).hp == 0 {
            let mode = if sp.shape == 5 || self.t().params.per_spell[1].get(cast.spell as usize - 1) == Some(&2) { 2 } else { 0 };
            let died = self.spell_check_death(cast, target, side, mode, false, hooks);
            if !died {
                self.spell_exp_cast(cast, target, side, dmg, false);
            }
        } else {
            if self.has_item_ability(self.unit(target), 0x1d) && self.rng.pct() < 0x1e {
                self.spell_apply_hp(cast, target, dmg * -2, true);
            }
            self.spell_exp_cast(cast, target, side, dmg, false);
        }
        dmg != 0
    }

    /// Port of FUN_004606d3 (one cell): heal; effect 0x5a also cleanses.
    fn spell_heal_cell(&mut self, cast: &mut Cast, x: i32, y: i32, healed: &mut i32, heal_exp: &mut i32) {
        let Some(t) = self.unit_at_cell(cast.side, x, y, false) else { return };
        if self.spell_def(cast.spell).effect == 0x5a {
            let u = self.unit_mut(t);
            u.buffs &= 0xffff_b974;
            u.status &= 0xfef0_0fff;
            u.recompute_buffs();
        }
        let (chance, mut v, _) = self.spell_roll(cast, t, cast.side, true);
        let u = self.unit(t);
        if u.hp < u.hp_max {
            v = v.min(u.hp_max as i32 - u.hp as i32);
            if chance < self.rng.pct() {
                v = 0;
            }
            self.spell_apply_hp(cast, t, -v, true);
            // FUN_00463c55 → FUN_00463c84(.., per_amount) into its own counter
            let mut acc = (*heal_exp, 0);
            self.spell_exp(cast, t, cast.side, v, true, &mut acc);
            *heal_exp = acc.0;
            if v > 0 {
                *healed += 1;
            }
            cast.hits.push(TargetHit { unit: t, amount: -v, affected: v > 0 });
        }
    }

    /// Port of FUN_0046136c: own-side effect on one unit. Returns "applied".
    fn spell_ally_effect(&mut self, cast: &mut Cast, t: UnitRef, hooks: &mut dyn EffectHooks) -> bool {
        let sp = self.spell_def(cast.spell);
        let turns = self.spell_turns(cast, t);
        let (chance, mut v, _) = self.spell_roll(cast, t, cast.side, true);
        if chance < self.rng.pct() {
            self.spell_apply_hp(cast, t, 0, true);
            return false;
        }
        let mt = self.move_type(self.unit(t));
        let mut heal_anim = false;
        let me = cast.caster;
        let partner = cast.partner;
        let ed = self.c(0xed);
        let u = self.unit_mut(t);
        let ok = match sp.effect {
            2 => buff_a(u, 4, 0x10, v, turns, |u, v| u.def_cur += v),
            6 => buff_a(u, 2, 4, v, turns, |u, v| u.atk_cur += v),
            9 => buff_a(u, 5, 0x20, v, turns, |u, v| u.mag_cur += v),
            10 => {
                if (u.ap_base as i32) + v < 0x6f || {
                    v = 0x6e - u.ap_base as i32;
                    v > 0
                } {
                    buff_a(u, 6, 0x40, v, turns, |u, v| {
                        u.ap_base += v;
                        u.ap += v;
                    })
                } else {
                    false
                }
            }
            0xb => {
                if mt == 0 || mt == 5 {
                    let i = 0; // FUN_0043c879(1)
                    if u.status & status::FLYING == 0 || ((u.status_turns[i] as i32) < turns && u.status_turns[i] != -1) {
                        u.status_turns[i] = turns as i8;
                    }
                    u.status |= status::FLYING;
                    true
                } else {
                    false
                }
            }
            0xc => buff_a(u, 8, 0x100, v, turns, |u, v| u.skill_cur += v),
            0xe => u.cure_petrify(),
            0x13 => u.apply_status(turns, status::BLESSING),
            0x14 => {
                u.buffs &= 0xffff_bb74;
                u.status &= 0xfef0_0fff;
                u.recompute_buffs();
                true
            }
            0x20 => u.apply_status(turns, status::BARRIER),
            0x21 => u.apply_status(turns, status::INVISIBLE),
            0x25 => u.apply_status(turns, status::LEVITATE),
            0x2c => u.apply_status(turns, status::GUARD150),
            0x31 => {
                u.apply_status(turns, status::INVISIBLE);
                for bit in [0x100000, 0x200000, 0x400000, 0x800000, 0x2000000, 0x20, 8] {
                    u.apply_status(turns, bit);
                }
                heal_anim = true;
                true
            }
            0x32 => u.apply_status(turns, 0x200000),
            0x33 => u.apply_status(turns, 0x400000),
            0x34 => u.apply_status(turns, 0x800000),
            0x36 => u.apply_status(turns, 0x2000000),
            0x39 => u.apply_status(turns, status::LIGHT_SHIELD),
            0x3c => {
                u.buffs &= 0xffff_fbfd;
                u.status &= 0xfffe_7fff;
                u.recompute_buffs();
                v = -v;
                heal_anim = true;
                true
            }
            0x3e => u.apply_status(turns, status::REFLECT),
            0x3f => {
                let (a, d) = (u.atk_cur / 2, u.def_cur / 2);
                u.apply_buff_b(turns, a, d, buff::POSSESSED)
            }
            0x41 => u.apply_status(turns, status::GUARD180),
            0x46 => u.apply_status(turns, status::SPIRIT_STRIKE),
            0x48 => u.apply_status(turns, status::SPIRIT_SHIELD),
            0x4a => {
                u.buffs = 0;
                u.status = 0;
                u.recompute_buffs();
                true
            }
            0x4c => u.apply_status(turns, status::REGEN),
            0x54 => u.apply_buff_b(turns, v as i16, v as i16, buff::CHI),
            0x55 => u.apply_status(turns, status::LIGHT_BODY),
            0x56 => u.apply_buff_b(turns, v as i16, v as i16, buff::CHI_RELEASE),
            0x57 => u.apply_status(turns, status::SELFLESS),
            0x58 => {
                let mut ok = buff_a(u, 8, 0x100, v, turns, |u, v| u.skill_cur += v);
                ok |= u.apply_status(turns, status::BLESSING);
                ok |= u.apply_status(turns, status::REGEN);
                ok
            }
            0x5b => {
                if t != me && Some(t) != partner && u.act == 2 && u.npc != 1 {
                    u.act = 0;
                    u.turn_flags = 0;
                    u.ap_carry = if ed < u.ap as i32 { ed as i16 } else { u.ap };
                    if 0x6d < u.ap_base as i32 + u.ap_carry as i32 {
                        u.ap_carry = 0;
                    }
                    u.ap = u.ap_base + u.ap_carry;
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        // FUN_0046136c draws the unit back into the surface for these.
        match sp.effect {
            0x21 | 0x31 if ok => cast.shows.push(Show::Put(t, 1)),
            0x4a => cast.shows.push(Show::Put(t, 0)),
            0x5b if ok => cast.shows.push(Show::Put(t, 0)),
            _ => {}
        }
        if v != 0 && ok {
            self.spell_apply_hp(cast, t, -v, heal_anim);
            if self.unit(t).hp == 0 {
                self.spell_check_death(cast, t, cast.side, 0, true, hooks);
            } else if heal_anim && self.has_item_ability(self.unit(t), 0x1d) && self.rng.pct() < 0x1e {
                self.spell_apply_hp(cast, t, v << 1, true);
            }
        }
        cast.hits.push(TargetHit { unit: t, amount: v, affected: ok });
        ok
    }

    /// FUN_0046ae25 + the enemy-side `+1` (rules::spell_duration).
    fn spell_turns(&self, cast: &Cast, t: UnitRef) -> i32 {
        let tables = self.t();
        let ctx = rules::Ctx::new(&tables, self.difficulty, self.vs);
        let c = self.fighter(cast.caster);
        let tf = self.fighter(t);
        let sc = rules::SpellCast { caster: &c, partner: None, spell: cast.spell, side: rules_side(cast.side), facing: 1 };
        rules::spell_duration(&ctx, &sc, &tf)
    }

    /// Port of FUN_00462144: enemy-side effect on one unit. Returns "applied".
    fn spell_enemy_effect(&mut self, cast: &mut Cast, t: UnitRef, hooks: &mut dyn EffectHooks) -> bool {
        let sp = self.spell_def(cast.spell);
        let foe = cast.side.other();
        {
            let u = self.unit_mut(t);
            if u.ai_mode == 2 {
                u.ai_mode = 1;
                u.ai_sub = 0;
            }
        }
        let turns = self.spell_turns(cast, t);
        let (chance, mut v, slot) = self.spell_roll(cast, t, foe, true);
        cast.slot = slot;
        let shielded = |u: &Unit| u.status & status::BARRIER != 0 || u.abilities & 0x4000 != 0;
        let mut ok = false;
        if sp.effect == 0x38 {
            if !shielded(self.unit(t)) {
                if self.rng.pct() <= chance && self.unit_mut(t).apply_poison(turns) {
                    ok = true;
                }
                for bit in [status::CURSE, status::SEALED, status::PARALYSIS, status::BLIND] {
                    if self.rng.pct() <= chance && self.unit_mut(t).apply_status(turns, bit) {
                        ok = true;
                    }
                }
            }
            if !ok {
                self.spell_apply_hp(cast, t, 0, true);
                cast.hits.push(TargetHit { unit: t, amount: 0, affected: false });
                return false;
            }
        } else if chance < self.rng.pct() {
            self.spell_apply_hp(cast, t, 0, true);
            cast.hits.push(TargetHit { unit: t, amount: 0, affected: false });
            return false;
        }
        self.spell_wake(cast.spell, t);
        cast.shows.push(Show::Hit(t));
        if sp.flags & 0x80 == 0 && v > 0 {
            v = v.min(self.unit(t).hp as i32);
            self.spell_apply_hp(cast, t, v, true);
            if self.unit(t).hp == 0 {
                let died = self.spell_check_death(cast, t, foe, 0, true, hooks);
                if died {
                    cast.hits.push(TargetHit { unit: t, amount: v, affected: true });
                    return true;
                }
            } else if self.has_item_ability(self.unit(t), 0x1d) && self.rng.pct() < 0x1e {
                self.spell_apply_hp(cast, t, v * -2, true);
            }
        }
        if sp.effect == 3 || !shielded(self.unit(t)) {
            let slot = cast.slot;
            match sp.effect {
                3 => {
                    hooks.die(self, t, 0);
                    cast.shows.push(Show::Die(t, 0));
                    cast.exp = 0;
                    self.spell_kill_rewards(cast, t, foe);
                    self.unit_mut(cast.caster).kills += 1;
                    ok = true;
                }
                5 => {
                    ok = self.unit_mut(t).apply_petrify(turns);
                    if ok {
                        cast.shows.push(Show::Put(t, 2));
                    }
                }
                7 => {
                    let u = self.unit_mut(t);
                    let i = 0; // FUN_0043c7a1(1)
                    if u.buffs & buff::DEF_DOWN == 0 {
                        let d = v.min(u.def_cur as i32);
                        u.def_cur -= d as i16;
                        u.buff_a[i].delta = -(d as i16);
                        u.buffs |= buff::DEF_DOWN;
                        ok = true;
                    }
                    set_turns_a(u, i, ok, turns);
                }
                8 => ok = self.unit_mut(t).apply_status(turns, status::BLIND),
                0xd => {
                    ok = self.unit_mut(t).apply_status(turns, status::FROZEN);
                    if ok {
                        cast.shows.push(Show::Put(t, 2));
                    }
                }
                0xf => {
                    let u = self.unit_mut(t);
                    if u.buffs & buff::EXHAUSTION == 0 {
                        u.buffs |= buff::EXHAUSTION;
                        u.buff_c.d_ap = -(v as i16);
                        u.ap_base += u.buff_c.d_ap;
                        u.buff_c.d_atk = (-(u.atk_cur as i32) / 10) as i16;
                        u.atk_cur += u.buff_c.d_atk;
                        u.buff_c.d_def = (-(u.def_cur as i32) / 10) as i16;
                        u.def_cur += u.buff_c.d_def;
                        ok = true;
                    }
                    if ok || ((u.buff_c.turns as i32) < turns && u.buff_c.turns != -1) {
                        u.buff_c.turns = turns as i8;
                    }
                }
                0x10 => ok = self.unit_mut(t).apply_poison(turns),
                0x11 => ok = self.unit_mut(t).apply_status(turns, status::SEALED),
                0x22 | 0x26 => {
                    if slot != 0 {
                        self.unequip_slot(t, slot as usize, true);
                    }
                    ok = true;
                    cast.slot = 0;
                }
                0x2b | 0x3b => ok = self.unit_mut(t).apply_status(turns, status::PARALYSIS),
                0x2e => {
                    let u = self.unit_mut(t);
                    let i = 7; // FUN_0043c7a1(0x80)
                    if u.buffs & buff::SLOW == 0 {
                        let d = v.min(u.ap_base as i32);
                        u.buff_a[i].delta = -(d as i16);
                        u.ap_base += u.buff_a[i].delta;
                        u.ap = (u.ap - d as i16).max(0);
                        if u.ap < u.ap_carry {
                            u.ap_carry = u.ap;
                        }
                        u.buffs |= buff::SLOW;
                        ok = true;
                    }
                    set_turns_a(u, i, ok, turns);
                }
                0x35 => ok = self.unit_mut(t).apply_status(turns, 0x100_0000),
                0x37 => ok = self.unit_mut(t).apply_status(turns, status::CURSE),
                0x3d => ok = self.unit_mut(t).apply_status(turns, status::SLEEP),
                0x43 => ok = self.unit_mut(t).apply_status(turns, status::FEAR),
                0x45 => ok = self.unit_mut(t).apply_status(turns, status::WEAK),
                _ => {}
            }
        }
        cast.hits.push(TargetHit { unit: t, amount: v, affected: ok });
        ok
    }

    /// Port of FUN_00462c42: effect on a unit of either side. Returns "applied".
    fn spell_any_effect(&mut self, cast: &mut Cast, t: UnitRef, side: Side, hooks: &mut dyn EffectHooks) -> bool {
        let sp = self.spell_def(cast.spell);
        let turns = self.spell_turns(cast, t);
        let (chance, mut v, _) = self.spell_roll(cast, t, side, true);
        if chance < self.rng.pct() {
            self.spell_apply_hp(cast, t, 0, true);
            cast.hits.push(TargetHit { unit: t, amount: 0, affected: false });
            return false;
        }
        cast.shows.push(Show::Hit(t));
        if sp.flags & 0x80 == 0 && v > 0 {
            v = v.min(self.unit(t).hp as i32);
            self.spell_apply_hp(cast, t, v, true);
            if self.unit(t).hp == 0 {
                if self.spell_check_death(cast, t, side, 0, true, hooks) {
                    cast.hits.push(TargetHit { unit: t, amount: v, affected: true });
                    return true;
                }
            } else if self.has_item_ability(self.unit(t), 0x1d) && self.rng.pct() < 0x1e {
                self.spell_apply_hp(cast, t, v * -2, true);
            }
        }
        self.spell_wake(cast.spell, t);
        let shielded = |u: &Unit| u.status & status::BARRIER != 0 || u.abilities & 0x4000 != 0;
        let mut ok = false;
        match sp.effect {
            0x23 => {
                // FUN_0046313d: the target's class sheet.
                cast.shows.push(Show::Sheet(t));
                ok = true;
            }
            0x28 => {
                if !shielded(self.unit(t)) {
                    ok = self.unit_mut(t).apply_berserk(v as i16, turns);
                }
            }
            0x2f => {
                let u = self.unit_mut(t);
                if side == cast.side {
                    u.buffs &= 0xffff_bb74;
                    u.status &= 0xfef0_0fff;
                    u.recompute_buffs();
                } else {
                    dispel_one(u);
                }
                ok = true;
            }
            0x3a => {
                if !shielded(self.unit(t)) {
                    let u = self.unit_mut(t);
                    let i = 3; // FUN_0043c7a1(8)
                    if u.buffs & buff::NAUSEA == 0 {
                        u.buff_a[i].delta = (-(v * u.atk as i32) / 100) as i16;
                        u.atk_cur += u.buff_a[i].delta;
                        u.buffs |= buff::NAUSEA;
                        ok = true;
                    }
                    set_turns_a(u, i, ok, turns);
                }
            }
            _ => {}
        }
        cast.hits.push(TargetHit { unit: t, amount: v, affected: ok });
        ok
    }

    /// Port of FUN_0045ff9e: run a cast over the target `cells`
    /// (`DAT_00492a68`, chosen by the BATTLE targeting code) and award EXP
    /// and learning. Costs are paid separately (`pay_spell_cost`).
    pub fn cast_spell(&mut self, cast: &mut Cast, cells: &[(i32, i32)], hooks: &mut dyn EffectHooks) -> CastOutcome {
        let mut out = CastOutcome::default();
        let sp = self.spell_def(cast.spell);
        let combo = sp.combo & 7 != 0;
        let n = self.t().items.len();
        cast.exp = 0;
        cast.exp_partner = 0;
        cast.prof_gain = 0;
        cast.drops = vec![0; n];
        cast.hits.clear();
        let diff = self.difficulty as i32;
        match sp.effect {
            1 => {
                // FUN_00460b73
                let kills0 = self.unit(cast.caster).kills as i32;
                let mut hits = 0;
                for &(x, y) in cells {
                    cast.shows.push(Show::Pump);
                    if self.spell_damage_cell(cast, x, y, hooks) {
                        hits += 1;
                    }
                }
                let kills = self.unit(cast.caster).kills as i32;
                cast.prof_gain = ((hits + kills - kills0 + 6 - diff) * 10) / 3 + 10;
            }
            0x12 | 0x5a => {
                // FUN_004606d3
                let (mut healed, mut heal_exp) = (0, 0);
                for &(x, y) in cells {
                    cast.shows.push(Show::Pump);
                    self.spell_heal_cell(cast, x, y, &mut healed, &mut heal_exp);
                }
                cast.exp += heal_exp;
                if combo {
                    cast.exp_partner = heal_exp;
                }
                cast.prof_gain = (healed + 6 - diff) * 5 + 10;
            }
            0x15..=0x1f => {
                // FUN_004608e2
                cast.exp = 5;
                let t = self.t();
                let kind = summon_kind(&t.spells, cast.spell);
                let level = summon_level(self.unit(cast.caster).mag_cur as i32);
                let npc = self.unit(cast.caster).npc != 0;
                if let Some(s) = hooks.summon(self, cast.side, kind, cast.cell.0, cast.cell.1, level) {
                    let u = self.unit_mut(s);
                    u.npc = npc as i16;
                    u.act = 2;
                }
                let known = (0..11).filter(|&k| self.unit(cast.caster).learned[6] & bit16(k) != 0).count() as i32;
                cast.prof_gain = (known + 6 - diff) * 5 + 0x14;
            }
            _ => self.spell_status(cast, cells, hooks),
        }
        out.targets = std::mem::take(&mut cast.hits);
        self.spell_prof_gain = cast.prof_gain;
        // The caster may have died (blood price, reflection).
        if self.unit(cast.caster).hp == 0 {
            let c = cast.caster;
            if self.has_item_ability(self.unit(c), 0x19) {
                let m = self.unit(c).hp_max;
                self.unit_mut(c).hp = m;
            } else if self.has_item_ability(self.unit(c), 0x1c) {
                let m = self.unit(c).hp_max;
                self.unit_mut(c).hp = m;
                self.consume_equipped(c, 0x1c);
            } else {
                hooks.die(self, c, 0);
                cast.shows.push(Show::Die(c, 0));
                out.caster_died = true;
                return out;
            }
        }
        self.cast_tail(cast, out)
    }

    /// FUN_0045ff9e when the cast sequence already applied the cells
    /// (FUN_00466f65 kinds 2/3/6, `SpellCaster+0x28` allocated): no effect
    /// pass and no caster death check; the proficiency gain is the one the
    /// caster object still holds from its previous cast (`+0x24`).
    pub fn cast_spell_applied(&mut self, cast: &mut Cast) -> CastOutcome {
        cast.prof_gain = self.spell_prof_gain;
        let out = CastOutcome { targets: std::mem::take(&mut cast.hits), ..CastOutcome::default() };
        self.cast_tail(cast, out)
    }

    /// The tail of FUN_0045ff9e: combo partner and caster learning, EXP and drops.
    fn cast_tail(&mut self, cast: &mut Cast, mut out: CastOutcome) -> CastOutcome {
        let sp = self.spell_def(cast.spell);
        let combo = sp.combo & 7 != 0;
        let diff = self.difficulty as i32;
        if combo {
            if let Some(p) = cast.partner {
                out.learned_partner = self.learn_from_proficiency(p, sp.school, cast.prof_gain);
                cast.exp_partner = (cast.exp_partner / 2).max(1);
                cast.exp = (cast.exp / 2).max(1);
                out.exp_partner = self.award_exp(p, cast.exp_partner, &[]);
            }
        }
        if !cast.from_item {
            out.learned = self.learn_from_proficiency(cast.caster, sp.school, cast.prof_gain);
        }
        if cast.from_item {
            cast.exp = (cast.exp / diff.max(1)).max(1);
        }
        let drops = std::mem::take(&mut cast.drops);
        out.exp = self.award_exp(cast.caster, cast.exp, &drops);
        // The level checks (FUN_00453725) follow each reward window, after the
        // cast's popups: `cast::cast_effects` runs them.
        out
    }

    /// Port of FUN_00460fd0: status/support spells.
    fn spell_status(&mut self, cast: &mut Cast, cells: &[(i32, i32)], hooks: &mut dyn EffectHooks) {
        let sp = self.spell_def(cast.spell);
        let diff = self.difficulty as i32;
        cast.prof_gain = ((sp.cost as i32 + 6 - diff) * 5) / 2 + 10;
        cast.exp = 5;
        if sp.combo & 7 != 0 {
            cast.exp_partner = 5;
        }
        let foe = cast.side.other();
        match sp.effect {
            4 => {
                let (x, y) = cells.first().copied().unwrap_or(cast.cell);
                let t = self.unit_at_cell(cast.side, x, y, false).or_else(|| self.unit_at_cell(foe, x, y, false));
                if let Some(t) = t {
                    hooks.teleport(self, t, cast.cell.0, cast.cell.1);
                }
                return;
            }
            0x27 => {
                hooks.effect_27(self, cast.caster);
                return;
            }
            0x2d => {
                self.army_mut(cast.side).flags |= 1; // FUN_0043c94a(army, 1)
                cast.shows.push(Show::Redraw);
            }
            _ => {}
        }
        for &(x, y) in cells {
            cast.shows.push(Show::Pump);
            match sp.target {
                0 | 1 => {
                    if let Some(t) = self.unit_at_cell(cast.side, x, y, false) {
                        self.spell_ally_effect(cast, t, hooks);
                    }
                }
                2 => {
                    if let Some(t) = self.unit_at_cell(foe, x, y, false) {
                        self.spell_enemy_effect(cast, t, hooks);
                    }
                }
                3 => {
                    if let Some(t) = self.unit_at_cell(cast.side, x, y, false) {
                        self.spell_any_effect(cast, t, cast.side, hooks);
                    } else if let Some(t) = self.unit_at_cell(foe, x, y, false) {
                        self.spell_any_effect(cast, t, foe, hooks);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Port of FUN_00457301: school 7 (喚術) spells 5..=10 additionally need some
/// spell of school `idx − 4` (1..=6) learned (`unit+0xf1+2*idx` = `learned[idx−5]`).
fn special_learn(u: &Unit, school: u8, idx: usize) -> bool {
    school == 7 && (5..=10).contains(&idx) && u.learned[idx - 5] != 0
}

/// A 3-byte buff slot (bits 0..8): apply `bit` with `v` via `apply`, extend turns.
fn buff_a(u: &mut Unit, i: usize, bit: u32, v: i32, turns: i32, apply: impl Fn(&mut Unit, i16)) -> bool {
    let fresh = u.buffs & bit == 0;
    if fresh {
        apply(u, v as i16);
        u.buffs |= bit;
        u.buff_a[i].delta = v as i16;
    }
    set_turns_a(u, i, fresh, turns);
    fresh
}

fn set_turns_a(u: &mut Unit, i: usize, ok: bool, turns: i32) {
    let t = u.buff_a[i].turns;
    if ok || ((t as i32) < turns && t != -1) {
        u.buff_a[i].turns = turns as i8;
    }
}

/// Port of FUN_0043c992: remove one positive buff (first of 嗜血, 護甲, 魔強,
/// 加速, 激勵, 附體, 運功, 發功) or else one positive status, then recompute.
pub fn dispel_one(u: &mut Unit) {
    const BUFFS: [u32; 8] = [4, 0x10, 0x20, 0x40, 0x100, 0x800, 0x1000, 0x2000];
    const STATUS: [u32; 18] = [
        1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800, 0x100000, 0x200000, 0x400000, 0x800000, 0x2000000,
        0x4000000,
    ];
    if u.buffs & 0x3974 == 0 {
        if u.status & 0x6f0_0fff == 0 {
            return;
        }
        if let Some(&b) = STATUS.iter().find(|&&b| u.status & b != 0) {
            u.status ^= b;
        }
    } else if let Some(&b) = BUFFS.iter().find(|&&b| u.buffs & b != 0) {
        u.buffs ^= b;
    }
    u.recompute_buffs();
}

#[cfg(test)]
pub(crate) mod tests {
    //! Tests against the user's game data (`S2_GAME_DIR`, default
    //! ~/games/super2sp/extracted/app); skipped when it is absent.
    use std::path::PathBuf;
    use std::rc::Rc;

    use s2data::tables::RuleTables;
    use s2data::tbl_stage::StageFile;

    use super::super::army::{Side, UnitRef};
    use super::super::map::Map;
    use super::super::rng::{Rng, Seeder};
    use super::super::Battle;
    use super::*;

    /// The game data, or None when it is absent.
    pub(crate) fn data() -> Option<crate::Data> {
        let dir = std::env::var_os("S2_GAME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("games/super2sp/extracted/app")))?;
        crate::Data::open(&dir).ok()
    }

    /// Stage 1 at turn 1 with a fixed RNG, or None without game data.
    pub(crate) fn stage1() -> Option<Battle> {
        let d = data()?;
        let mut b = Battle::default();
        b.set_tables(Rc::new(RuleTables::load(&d.dir, &d.arcs).ok()?));
        b.rng = Rng::started(Seeder::fixed(77));
        let st = StageFile::parse(d.arcs.read(s2data::Kind::Bin, 1).ok()?).ok()?.stage(1).ok()?;
        b.set_map(Map::load(&d.arcs, st.header.map as usize).ok()?);
        b.start_stage(&st);
        b.deploy_turn(1);
        Some(b)
    }

    fn by_char(b: &Battle, side: Side, ch: i16) -> UnitRef {
        UnitRef::new(side, b.army(side).find_char(ch, 0).expect("character deployed"))
    }

    #[test]
    fn learning_follows_school_level() {
        let Some(mut b) = stage1() else { return eprintln!("spells tests: no game data, skipped") };
        let r = by_char(&b, Side::Player, 13); // 羅莎莉, 神術 caster
        let before = b.unit(r).learned[0];
        let t = b.t();
        let (first, count) = t.spells.school_spells[0];
        let gain = 15000;
        let got = b.learn_from_proficiency(r, 1, gain);
        let u = b.unit(r);
        assert_eq!(u.prof[6], 15000);
        for k in 0..count as usize {
            let sp = &t.spells.spells[first as usize + k];
            let should = sp.level as i32 <= 1500 && sp.combo & 0x80 == 0;
            if should {
                assert!(u.learned[0] & (1 << k) != 0, "{} learned", sp.name);
            }
        }
        assert!(got.iter().all(|&s| before & (1 << (s - first as usize)) == 0), "only new spells reported");
        // at the cap nothing changes any more
        assert!(b.learn_from_proficiency(r, 1, 10).is_empty());
    }

    #[test]
    fn heal_cast_is_capped_and_deterministic() {
        let run = || -> Option<(i32, i32, i32)> {
            let mut b = stage1()?;
            let healer = by_char(&b, Side::Player, 13);
            let hurt = by_char(&b, Side::Player, 11);
            b.unit_mut(hurt).hp -= 50;
            let (x, y) = (b.unit(hurt).x, b.unit(hurt).y);
            let mut cast = Cast::new(Side::Player, healer, 1, (x, y)); // 治療
            assert_eq!(b.spell_shortage(b.unit(healer), 1), Shortage::Ok);
            b.pay_spell_cost(&cast);
            let out = b.cast_spell(&mut cast, &[(x, y)], &mut MarkDead::default());
            let hit = out.targets.iter().find(|h| h.unit == hurt).copied().expect("target recorded");
            let u = b.unit(hurt);
            assert!(hit.amount <= 0 && hit.amount >= -50);
            assert_eq!(u.hp as i32, u.hp_max as i32 - 50 - hit.amount);
            assert!(out.exp >= 1);
            Some((hit.amount, out.exp, b.unit(healer).mp as i32))
        };
        let Some(a) = run() else { return eprintln!("spells tests: no game data, skipped") };
        assert_eq!(Some(a), run(), "same seed, same outcome");
    }

    #[test]
    fn damage_cast_hits_enemy_cell() {
        let Some(mut b) = stage1() else { return eprintln!("spells tests: no game data, skipped") };
        let caster = by_char(&b, Side::Player, 13);
        let foe = UnitRef::new(Side::Enemy, 0);
        let (x, y) = (b.unit(foe).x, b.unit(foe).y);
        let hp0 = b.unit(foe).hp;
        // spell 11 邪惡之箭 (巫術, effect 1); learning is irrelevant for the effect itself
        let mut cast = Cast::new(Side::Player, caster, 11, (x, y));
        let out = b.cast_spell(&mut cast, &[(x, y)], &mut MarkDead::default());
        let hit = out.targets.iter().find(|h| h.unit == foe).copied().expect("target recorded");
        assert_eq!(b.unit(foe).hp as i32, (hp0 as i32 - hit.amount).max(0));
        assert!(cast.prof_gain > 0);
    }

    #[test]
    fn summon_menu_rule_and_partner_gating() {
        let Some(mut b) = stage1() else { return eprintln!("spells tests: no game data, skipped") };
        let r = by_char(&b, Side::Player, 11);
        let t = b.t();
        let summon = (t.spells.school_spells[6].0 + 1) as u16; // first 喚術 spell
        assert!(b.spell_listed(r, summon, false, false));
        b.army_mut(Side::Player).summons[0].active = 1;
        assert!(!b.spell_listed(r, summon, false, false), "只能召喚一個");
        assert!(b.spell_listed(r, summon, true, false), "the menu still lists it");
        // a 合擊 spell needs a learned partner on an adjacent cell
        let combo = (t.spells.school_spells[10].0 + 1) as u16;
        assert!(!b.spell_listed(r, combo, false, true), "hidden while browsing");
        assert_eq!(summon_level(45), 3);
        assert_eq!(summon_level(500), 10);
    }
}
