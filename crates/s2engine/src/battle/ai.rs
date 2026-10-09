//! Enemy / NPC AI (the controller `DAT_00492af0`, docs/ai.md): one unit's
//! decision — modes, the attack and spell search, the candidate comparison,
//! retreat / approach thresholds — and its execution through the BATTLE
//! API (moves, attacks, casts). Owned by the AI port. Spell areas and the
//! per-spell scoring are `targeting.rs`.
//!
//! AI API
//! ------
//! ```text
//! run_phase(game, side) -> i32        FUN_0043310e (enemy army) / FUN_00433da4 (player army: NPC allies and berserk
//!                                     units after the player phase): every ready AI unit acts, nearest first;
//!                                     0 done, 1 victory, 2 defeat (the loop is turn::npc_phase, FUN_00433db9 mode 1)
//! decide(game, r)                     FUN_0040a4d0: unit r's whole action. turn::ai_act (FUN_0040a443) wraps it with the
//!                                     done-bookkeeping and is what the phase loops, auto battle and turn::end_turn
//!                                     (berserk units) call.
//! nearest_of(b, side, x, y, mode, min_power) -> Option<UnitRef>     FUN_00434189 (mode 0/3 visible, 1 no class type 7)
//! nearest_to(b, side, r, mode) -> Option<UnitRef>                   FUN_00434138 (mode 3: power > 2× r's)
//! nearest_idle_distance(b, side, x, y) -> i32                       FUN_00433fa8
//! reset_state()                       clear the controller's between-units state (+0x10 / +0x14)
//! Best::new(limit), Best::offer(kind, score, kills) -> bool         FUN_00407dd9 (limit = offer_limit(game))
//! Battle::consider_spells(r, technique, &mut best, &mut plan) -> bool   FUN_0046b890
//! Battle::spell_lists(r, technique) -> Vec<(school, Vec<index>)>    FUN_0045f212 / FUN_0045f3b0
//! Battle::best_attack_target(r, skill, &mut best) -> Option<UnitRef>   FUN_0040b740
//! Battle::target_reachable(r, limit) -> bool                        FUN_0040bc9e
//! Battle::assess(&mut ctl) -> i32                                   FUN_0040c424: 0 danger, 1 threatened, 3 safe
//! Battle::fit_to_advance(r, eager) / should_retreat(r, eager) -> bool   FUN_0040c847 / FUN_0040ca2d
//! Battle::flee_cell(r, keep) / regroup_cell(r) / find_goal(class, x, y)  FUN_0040c18d / FUN_00471923 / FUN_0042890a
//! classchange::ai_promotion_choice(&targets)                        the AI branch of FUN_00453986
//! ```
//!
//! Notes: evaluation moves the unit (`x`, `y`) over its range cells as the
//! original does, so unit lookups see it there; the counter pick and status
//! checks of every forecast consume RNG in the original order. The
//! controller's last `assess` result survives between units like the
//! original global. Drawing ("thinking" text, range overlay pauses) is the
//! view's and only runs while presenting.

use std::cell::Cell;

use super::actions::{self, presenting};
use super::army::{Side, UnitRef};
use super::camera;
use super::map::distance;
use super::menu;
use super::movement::Node;
use super::spells::{Cast, Shortage};
use super::turn;
use super::unit::{buff, flag, status, turn as tf, Unit};
use super::Battle;
use crate::game::Game;

thread_local! {
    /// `DAT_00492af0` +0x10 / +0x14, kept between units.
    static STATE: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
}

/// Clear the controller state kept between units (a fresh program start).
pub fn reset_state() {
    STATE.with(|s| s.set((0, 0)));
}

/// The AI controller `DAT_00492af0` for one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ctl {
    /// +0x08
    pub unit: UnitRef,
    /// +0x0c longest path of the unit's range (FUN_0047188c).
    pub reach: i32,
    /// +0x10 reach of the threatening enemy (FUN_0040c459).
    pub enemy_reach: i32,
    /// +0x14 the last `assess` result.
    pub situation: i32,
    /// The threshold of `Best::offer`.
    pub limit: i32,
}

/// The best plan so far (`type`, `score`, `kills` of FUN_00407dd9). Types:
/// 1 kills a VIP, 2 other attacks/damage, 3 heals, > 3 support spells
/// (their SpellRec +0x22).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Best {
    pub kind: i32,
    pub score: i32,
    pub kills: i32,
    /// `rules[0x10b]·100 + rules[0x10c]·battles fought`.
    pub limit: i32,
}

impl Best {
    pub fn new(limit: i32) -> Best {
        Best { kind: 0, score: 0, kills: 0, limit }
    }

    /// Port of FUN_00407dd9: keep (kind, score, kills) when it beats the
    /// current best.
    pub fn offer(&mut self, kind: i32, s: i32, kills: i32) -> bool {
        let l = self.limit;
        let l2 = l / 2;
        let (t, sc, k) = (self.kind, self.score, self.kills);
        let ok = kind > 0
            && (t == 0
                || match kind {
                    1 => t != 1 || sc < s,
                    2 if kills < 1 => match t {
                        2 => k == 0 && sc < s,
                        3 => sc < s,
                        t if t > 3 => l < s || (s < l && sc < s),
                        _ => false,
                    },
                    2 => t > 2 || (t == 2 && (k < kills || (kills == k && sc < s))),
                    3 => match t {
                        2 => k == 0 && sc < s,
                        3 => sc < s,
                        t if t > 3 => l2 < s || (s < l2 && sc < s),
                        _ => false,
                    },
                    _ => match t {
                        2 => sc < l && sc < s,
                        3 => sc < l2 && sc < s,
                        t if t > 3 => sc < s,
                        _ => false,
                    },
                });
        if ok {
            self.kind = kind;
            self.score = s;
            self.kills = kills;
        }
        ok
    }
}

/// A spell plan: spell id (0 none), aimed cell and ray/cone direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpellPlan {
    pub spell: u16,
    pub cell: (i32, i32),
    pub dir: i32,
}

/// `rules[0x10b]·100 + rules[0x10c]·DAT_00497b50` (battles fought).
pub fn offer_limit(game: &Game) -> i32 {
    game.battle.cu(0x10b) * 100 + game.battle.cu(0x10c) * game.campaign.battles_fought as i32
}

/// C division without the fault on 0.
fn div(a: i32, b: i32) -> i32 {
    if b == 0 {
        0
    } else {
        a.wrapping_div(b)
    }
}

fn hp_pct(u: &Unit) -> i32 {
    div(u.hp as i32 * 100, u.hp_max as i32)
}

/// Port of FUN_0040c804: HP at most 29%.
fn critical(u: &Unit) -> bool {
    u.hp_max >= 1 && u.hp as i32 * 100 / u.hp_max as i32 <= 0x1d
}

fn unit_type(b: &Battle, u: &Unit) -> u8 {
    b.tables.as_ref().and_then(|t| t.kinds.class(u.kind as u16)).map_or(0, |c| c.unit_type)
}

pub(super) fn set_pos(b: &mut Battle, r: UnitRef, x: i32, y: i32) {
    let u = b.unit_mut(r);
    u.x = x;
    u.y = y;
}

/// Live range nodes in order (index, node), skipping the root.
pub(super) fn stops(b: &Battle) -> Vec<(usize, Node)> {
    b.range.reachable().map(|(i, n)| (i, *n)).collect()
}

/// Port of FUN_00434189(army, x, y, mode, min_power): the nearest deployed
/// living unit of `side` other than one at (x, y) whose power exceeds
/// `min_power`; mode 0 and 3 skip hidden units, mode 1 class type 7. Ties
/// keep the lower slot; distance 1 ends the search.
pub fn nearest_of(b: &Battle, side: Side, x: i32, y: i32, mode: i32, min_power: i32) -> Option<UnitRef> {
    let mut best: Option<(i32, usize)> = None;
    for (i, u) in b.army(side).units.iter().enumerate() {
        if u.flags & flag::DEAD != 0 || u.flags & flag::DEPLOYED == 0 || (u.x == x && u.y == y) {
            continue;
        }
        let ok = match mode {
            1 => unit_type(b, u) != 7,
            0 | 3 => !b.is_hidden(side, u),
            _ => true,
        };
        if !ok || b.power(u, 0, 0) <= min_power {
            continue;
        }
        let d = distance(x, y, u.x, u.y);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
            if d == 1 {
                break;
            }
        }
    }
    best.map(|(_, i)| UnitRef::new(side, i))
}

/// Port of FUN_00434138(army, unit, mode): `nearest_of` around `r`; mode 3
/// only counts units more than twice as strong.
pub fn nearest_to(b: &Battle, side: Side, r: UnitRef, mode: i32) -> Option<UnitRef> {
    let u = b.unit(r);
    let min = if mode == 3 { b.power(u, 0, 0) << 1 } else { 0 };
    nearest_of(b, side, u.x, u.y, mode, min)
}

/// Port of FUN_00433fa8(army, x, y): distance to the nearest idle
/// (`act == 0`) living deployed unit of `side`, 0xffff if none.
pub fn nearest_idle_distance(b: &Battle, side: Side, x: i32, y: i32) -> i32 {
    b.army(side)
        .units
        .iter()
        .filter(|u| u.act == 0 && u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0)
        .map(|u| distance(x, y, u.x, u.y))
        .fold(0xffff, i32::min)
}

/// Port of FUN_004342d9(army, mover, target): a neighbour of `target`
/// (up-right, down-left, up-left, down-right) `mover` could stand on — free
/// of both armies and walkable, within the climb window (up < 4, down < 3).
/// The original's "mover already there" test compares the mover's column
/// with the row; kept.
fn free_neighbour(b: &Battle, mover: UnitRef, target: UnitRef) -> bool {
    let tu = b.unit(target);
    let ht = b.map().height(tu.x, tu.y);
    let m = b.unit(mover);
    let mt = b.move_type(m);
    let t = b.t();
    for d in 0..4 {
        let (nx, ny) = super::map::neighbour(tu.x, tu.y, d);
        let free = (m.x == nx && m.x == ny)
            || (b.unit_at_cell(target.side.other(), nx, ny, false).is_none()
                && b.unit_at_cell(target.side, nx, ny, false).is_none()
                && b.map().terrain(&t, nx, ny, m.kind, mt).0 == super::map::Pass::Ok);
        if free {
            let h = b.map().height(nx, ny);
            if h != 0 && ((h <= ht && ht - h < 3) || (ht < h && h - ht < 4)) {
                return true;
            }
        }
    }
    false
}

/// Port of FUN_00434045(army, unit): the nearest idle unit of `side` with a
/// free neighbour `r` could stand on.
fn nearest_ready_reachable(b: &Battle, side: Side, r: UnitRef) -> Option<UnitRef> {
    let (x, y) = (b.unit(r).x, b.unit(r).y);
    let mut best: Option<(i32, usize)> = None;
    for (i, u) in b.army(side).units.iter().enumerate() {
        if u.act != 0 || u.flags & flag::DEAD != 0 || u.flags & flag::DEPLOYED == 0 {
            continue;
        }
        let d = distance(x, y, u.x, u.y);
        if free_neighbour(b, r, UnitRef::new(side, i)) && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
            if d == 1 {
                break;
            }
        }
    }
    best.map(|(_, i)| UnitRef::new(side, i))
}

/// Port of FUN_0043cd7f: 1 when the class has a magic command (action kind
/// 0xb) the unit can use, 2 a technique command (0xc), 0 neither.
fn command_kind(b: &Battle, r: UnitRef) -> i32 {
    let t = b.t();
    let Some(c) = t.kinds.class(b.unit(r).kind as u16) else { return 0 };
    for k in c.actions.clone() {
        match b.action_kind(k) {
            0xb if b.knows_skill_kind(r, 1) => return 1,
            0xc if b.knows_skill_kind(r, 2) => return 2,
            _ => {}
        }
    }
    0
}

/// Port of FUN_00458283: the longest max range of the unit's usable actions.
fn longest_reach(b: &Battle, r: UnitRef) -> i32 {
    let u = b.unit(r);
    let t = b.t();
    let Some(c) = t.kinds.class(u.kind as u16) else { return 0 };
    c.actions.clone().filter(|&k| b.skill_usable(u, k)).map(|k| b.skill_range(u, k).1).fold(0, i32::max)
}

impl Battle {
    /// Port of FUN_0045f212 (magic) / FUN_0045f3b0 (techniques): the spell
    /// menu — each school with anything learned, and the 1-based indices
    /// of its learned spells.
    pub fn spell_lists(&self, r: UnitRef, technique: bool) -> Vec<(usize, Vec<u16>)> {
        let t = self.t();
        let (m, n) = (t.spells.magic_schools as usize, t.spells.tech_schools as usize);
        let schools = if technique { m..m + n } else { 0..m };
        let u = self.unit(r);
        let mut out = Vec::new();
        for s in schools {
            let mut mask = u.learned.get(s).copied().unwrap_or(0) as u32;
            if mask == 0 {
                continue;
            }
            let count = t.spells.school_spells.get(s).map_or(0, |c| c.1 as usize);
            let mut list = Vec::new();
            for i in 0..count {
                if mask & 1 != 0 {
                    list.push(i as u16 + 1);
                }
                mask >>= 1;
            }
            out.push((s + 1, list));
        }
        out
    }

    /// Port of FUN_0046b890: try every castable spell of the kind on its
    /// whole range from the unit's cell; true when one improved `best`
    /// (recorded in `plan`). Needs the cast AP; no magic under 禁錮 or
    /// either kind under the stage seals (player side). Combos need an
    /// adjacent partner; the spell-info screen's browse flag (caster object
    /// +0x38), which would hide them, is taken as clear.
    pub fn consider_spells(&mut self, r: UnitRef, technique: bool, best: &mut Best, plan: &mut SpellPlan) -> bool {
        let mut cost = self.c(0xef);
        if self.unit(r).turn_flags & 0xc != 0 {
            cost += self.c(0xf3);
        }
        if (self.unit(r).ap as i32) < cost {
            return false;
        }
        let player = self.army(r.side).side == Side::Player;
        if !technique {
            if self.seal_magic && player {
                return false;
            }
            if self.unit(r).status & status::SEALED != 0 {
                return false;
            }
        } else if self.seal_tech && player {
            return false;
        }
        let lists = self.spell_lists(r, technique);
        if lists.is_empty() {
            return false;
        }
        let saved = (self.map().scroll_x, self.map().scroll_y);
        let t = self.t();
        let mut found = false;
        for (school, idxs) in &lists {
            let first = t.spells.school_spells[school - 1].0 as u16;
            for &i in idxs {
                let spell = first + i;
                let mut cast = Cast::new(r.side, r, spell, (0, 0));
                cast.technique = technique;
                if !self.spell_listed(r, spell, false, false) {
                    continue;
                }
                let aim = self.spell_aim(spell as i32);
                if aim.combo & 7 != 0 {
                    cast.partner = self.combo_partner(r, spell);
                }
                if self.spell_shortage(self.unit(r), spell) != Shortage::Ok {
                    continue;
                }
                if aim.combo & 7 != 0 && !cast.partner.is_some_and(|p| self.spell_shortage(self.unit(p), spell) == Shortage::Ok) {
                    continue;
                }
                let mut tg = self.spell_target(r.side, r, spell as i32, technique as u8, 0, false);
                self.target_range(&mut tg, aim.range);
                if self.ai_score_spell(&mut tg, &cast, best, plan) {
                    plan.spell = spell;
                    found = true;
                }
            }
        }
        let m = self.map_mut();
        (m.scroll_x, m.scroll_y) = saved;
        found
    }

    /// Port of FUN_0040b740: the best target of attack `skill` from the
    /// unit's current cell. Each target: forecast (with its random counter
    /// pick), `expected` ×VIP, ×kill when the hit likely kills, minus the
    /// expected counter; rejected when the counter could kill us (unless
    /// berserk); piercing attacks add the enemies on the ray before the
    /// target. Returns the last target `best` accepted.
    pub fn best_attack_target(&mut self, r: UnitRef, skill: usize, best: &mut Best) -> Option<UnitRef> {
        let d = self.difficulty as usize;
        let (vip_mul, kill_mul) = (self.cu(0x100 + d), self.cu(0x105 + d));
        let tl = self.target_list(r, skill);
        let mut found = None;
        for (ti, t) in tl.targets.iter().enumerate() {
            let (mut kills, mut vk) = (0, 0);
            self.unit_mut(r).facing = t.facing;
            let du = self.duel(r, t.unit, skill);
            let vip = self.unit(t.unit).is_vip();
            let mut s = self.duel_expected(&du);
            if vip {
                s = s.wrapping_mul(vip_mul);
            }
            if (self.unit(t.unit).hp as i32) < du.calc.fc_dmg && du.calc.fc_hit > 0x32 {
                s = s.wrapping_mul(kill_mul);
                if vip {
                    vk += 1;
                }
                kills += 1;
            }
            s = s.wrapping_sub(self.duel_expected_counter(&du));
            let me = self.unit(r);
            if du.calc.fc_cdmg <= me.hp as i32 || du.calc.fc_chit < 0x33 || me.buffs & buff::BERSERK != 0 {
                if tl.shape == 3 {
                    let (start, len) = tl.ray_of(ti);
                    for k in 0..len {
                        let (x, y) = tl.cells[start + k];
                        let Some(e) = self.unit_at_cell(r.side.other(), x, y, false) else { continue };
                        if e == t.unit {
                            break;
                        }
                        let d2 = self.duel(r, e, skill);
                        let vip2 = self.unit(e).is_vip();
                        let mut v = self.duel_expected(&d2);
                        if vip2 {
                            v = v.wrapping_mul(vip_mul);
                        }
                        if (self.unit(e).hp as i32) < d2.calc.fc_dmg && d2.calc.fc_hit > 0x32 {
                            v = v.wrapping_mul(kill_mul);
                            if vip2 {
                                vk += 1;
                            }
                            kills += 1;
                        }
                        s = s.wrapping_add(v);
                        s = s.wrapping_sub(self.duel_expected_counter(&d2));
                    }
                }
                if s > 0 && best.offer(if vk < 1 { 2 } else { 1 }, s, kills) {
                    found = Some(t.unit);
                }
            }
        }
        found
    }

    /// Port of FUN_0040bc9e: something to do — a spell plan from the
    /// current cell, or an attack with a target from the current cell or
    /// from a stop cell of the range (not a bridge for walkers). As in the
    /// original the unit is not put back between actions, only at the end.
    pub fn target_reachable(&mut self, r: UnitRef, limit: i32) -> bool {
        let (x0, y0) = (self.unit(r).x, self.unit(r).y);
        let probe = |b: &mut Battle, technique: bool| {
            let mut best = Best::new(limit);
            let mut plan = SpellPlan::default();
            b.consider_spells(r, technique, &mut best, &mut plan)
        };
        let mut found = false;
        if self.knows_skill_kind(r, 1) && probe(self, false) {
            found = true;
        } else if self.knows_skill_kind(r, 2) && probe(self, true) {
            found = true;
        } else {
            let t = self.t();
            let actions = t.kinds.class(self.unit(r).kind as u16).map_or(0..0, |c| c.actions.clone());
            let nodes = stops(self);
            'search: for k in actions {
                let kind = self.action_kind(k);
                if kind == 0xb || kind == 0xc {
                    continue;
                }
                if self.can_attack_with(r, k) {
                    found = true;
                    break;
                }
                for &(_, n) in &nodes {
                    if n.can_stop != 0 && !self.no_stop_for(r, n.x as i32, n.y as i32) {
                        set_pos(self, r, n.x as i32, n.y as i32);
                        if self.can_attack_with(r, k) {
                            found = true;
                            break 'search;
                        }
                    }
                }
            }
        }
        set_pos(self, r, x0, y0);
        found
    }

    /// Port of FUN_0040c424: 0 a strong enemy is close, 1 a much stronger
    /// one could reach us next turn, 3 safe.
    pub fn assess(&mut self, ctl: &mut Ctl) -> i32 {
        if self.threat_near(ctl) {
            0
        } else if self.could_reach(ctl) {
            1
        } else {
            3
        }
    }

    /// Port of FUN_0040c5d8: a visible enemy with more than a third of our
    /// power within `reach + 1` rings.
    fn threat_near(&self, ctl: &Ctl) -> bool {
        if ctl.reach <= 0 {
            return false;
        }
        let r = ctl.unit;
        let u = self.unit(r);
        let p3 = self.power(u, 0, 0) / 3;
        super::targeting::ring_cells(u.x, u.y, ctl.reach + 1).into_iter().any(|(x, y)| {
            self.unit_at_cell(r.side.other(), x, y, true).is_some_and(|e| p3 < self.power(self.unit(e), 0, 0))
        })
    }

    /// Port of FUN_0040c459: the nearest visible enemy more than twice as
    /// strong could reach us next turn (`dist − its reach − our reach − 2 < 1`).
    /// Casters (our class type 6/7) measure a technique user by its
    /// technique reach.
    fn could_reach(&self, ctl: &mut Ctl) -> bool {
        let r = ctl.unit;
        let Some(e) = nearest_to(self, r.side.other(), r, 3) else { return false };
        let (u, eu) = (self.unit(r), self.unit(e));
        let d = distance(u.x, u.y, eu.x, eu.y);
        let ty = unit_type(self, u);
        let a = (eu.ap_base as i32 + eu.ap_carry as i32 - self.c(0xe8)).max(0);
        let reach = if (ty == 6 || ty == 7) && command_kind(self, e) == 2 {
            div(a - self.c(0xef), self.c(0xe9)).max(0) + 5
        } else {
            div(a - self.c(0xee), self.c(0xe9)).max(0) + longest_reach(self, e)
        };
        ctl.enemy_reach = reach;
        d - reach - ctl.reach - 2 < 1
    }

    /// Port of FUN_0040c847: healthy enough to advance (`eager` 1 = the
    /// lower thresholds): hidden units above 39% (always for easy enemies);
    /// casters 70/90% with MP ≥ 10%, class types 4/0x17 70/80% with ST ≥ 10%,
    /// type 3 60/70%, others 45/50%.
    pub fn fit_to_advance(&self, r: UnitRef, eager: i32) -> bool {
        let u = self.unit(r);
        if u.hp_max < 1 {
            return false;
        }
        let hp = u.hp as i32 * 100 / u.hp_max as i32;
        if hp > 0x27 && (u.abilities & 0x80 != 0 || u.status & status::INVISIBLE != 0) {
            if self.difficulty < 3 && !self.vs && self.army(r.side).side == Side::Enemy {
                return true;
            }
            if self.army(r.side.other()).flags & 9 == 0 {
                return true;
            }
        }
        let low = |v: i16, max: i16| v > 0 && max > 0 && (v as i32 * 100) / (max as i32) < 10;
        let need = match unit_type(self, u) {
            6 | 7 | 0x13 | 0x14 => {
                if low(u.mp, u.mp_max) {
                    return false;
                }
                if eager == 1 { 0x46 } else { 0x5a }
            }
            4 | 0x17 => {
                if low(u.st, u.st_max) {
                    return false;
                }
                if eager == 1 { 0x46 } else { 0x50 }
            }
            3 => {
                if eager == 1 { 0x3c } else { 0x46 }
            }
            _ => {
                if eager == 1 { 0x2d } else { 0x32 }
            }
        };
        need <= hp
    }

    /// Port of FUN_0040ca2d: HP% at or below the weighted threshold
    /// `(P1·40 + P2·w2 + P3·w3 + P4·25) / power` (power parts of FUN_0045781c;
    /// w2 = 30/40, w3 = 25 for class type 3 else 15/20).
    pub fn should_retreat(&self, r: UnitRef, eager: i32) -> bool {
        let u = self.unit(r);
        if u.hp_max < 1 {
            return true;
        }
        let w2 = if eager == 1 { 0x1e } else { 0x28 };
        let w3 = if unit_type(self, u) == 3 {
            0x19
        } else if eager == 1 {
            0xf
        } else {
            0x14
        };
        let p0 = self.power(u, 0, 0);
        if p0 == 0 {
            return false;
        }
        let (p5, p6, p7, p8) = (self.power(u, 0, 5), self.power(u, 0, 6), self.power(u, 0, 7), self.power(u, 0, 8));
        let th = (p5 * 0x28 + p6 * w2 + p7 * w3 + p8 * 0x19) / p0;
        th >= u.hp as i32 * 100 / u.hp_max as i32
    }

    /// Port of FUN_0040c18d: the stop cell farthest from the nearest
    /// visible enemy; when hurt (`keep` 1 or HP < 90%) the farthest one
    /// that leaves the rest AP wins if it differs. Returns the enemy and
    /// the node (index + 1; −1 when the range is the unit's cell only).
    pub fn flee_cell(&mut self, r: UnitRef, keep: i32) -> (Option<UnitRef>, i32) {
        let u = self.unit(r).clone();
        let hurt = keep == 1 || div(u.hp as i32 * 100, u.hp_max as i32) < 0x5a;
        if self.range.count == 1 {
            return (None, -1);
        }
        let (mut best_d, mut node, mut threat) = (-1, 0, None);
        let (mut rest_d, mut rest_node, mut rest_threat) = (-1, 0, None);
        let rest = self.c(0xf0);
        for (i, n) in stops(self) {
            let (nx, ny) = (n.x as i32, n.y as i32);
            if n.can_stop == 0 || self.no_stop_for(r, nx, ny) {
                continue;
            }
            set_pos(self, r, nx, ny);
            let Some(e) = nearest_to(self, r.side.other(), r, 0) else {
                set_pos(self, r, u.x, u.y);
                return (None, node);
            };
            let d = distance(nx, ny, self.unit(e).x, self.unit(e).y);
            if (rest_d == -1 || rest_d < d) && hurt && rest <= n.budget as i32 {
                rest_node = i as i32 + 1;
                rest_threat = Some(e);
                rest_d = d;
            }
            if best_d == -1 || best_d < d {
                best_d = d;
                node = i as i32 + 1;
                threat = Some(e);
            }
        }
        if hurt && rest_d != -1 && rest_node != node {
            threat = rest_threat;
            node = rest_node;
        }
        set_pos(self, r, u.x, u.y);
        (threat, node)
    }

    /// Port of FUN_00471923(range, unit, 0): a cautious step — the stop
    /// cell (not next to an enemy) that gets closer to the nearest ally
    /// while staying away from the nearest enemy. Builds the path; false
    /// when there is no ally or no cell.
    pub fn regroup_cell(&mut self, r: UnitRef) -> bool {
        let Some(a) = nearest_to(self, r.side, r, 2) else { return false };
        let (ax, ay) = (self.unit(a).x, self.unit(a).y);
        let d0 = distance(ax, ay, self.unit(r).x, self.unit(r).y);
        let (mut ed, mut best_e, mut best_a, mut best) = (0, 0, 0, -1i32);
        for (i, n) in stops(self) {
            let (nx, ny) = (n.x as i32, n.y as i32);
            if n.can_stop == 0 || self.enemy_adjacent_cell(r.side, nx, ny) {
                continue;
            }
            let da = distance(ax, ay, nx, ny);
            if let Some(e) = nearest_of(self, r.side.other(), nx, ny, 1, 0) {
                ed = distance(self.unit(e).x, self.unit(e).y, nx, ny);
            }
            if best == -1
                || (da < best_a && best_e < ed)
                || (da < best_a && ed <= best_e && best_a < d0)
                || (ed <= best_e && da < best_a && 10 < ed)
            {
                best_e = ed;
                best_a = da;
                best = i as i32;
            }
        }
        if best == -1 {
            return false;
        }
        let move5 = self.unit(r).flags & flag::MOVE5 != 0;
        self.range.build_path(best as usize + 1, move5);
        true
    }

    /// Port of FUN_0042890a: the nearest cell (row-major scan, first on
    /// ties) whose object tile has class `class`. The original returns an
    /// uninitialised "found" when there is none; `None` here.
    pub fn find_goal(&self, class: u16, x0: i32, y0: i32) -> Option<(i32, i32)> {
        let m = self.map();
        let mut best = None;
        let mut bd = -1;
        for y in 0..m.rows() {
            for x in 0..m.width() {
                if m.object_class(x, y) == class && class != 0 {
                    let d = distance(x0, y0, x, y);
                    if bd == -1 || d < bd {
                        best = Some((x, y));
                        bd = d;
                    }
                }
            }
        }
        best
    }
}

// ---- the unit's turn -----------------------------------------------------------------

/// The unit is drawn for the opponent: not hidden, or a player-side unit.
pub(super) fn shown(game: &Game, r: UnitRef) -> bool {
    let b = &game.battle;
    !b.is_hidden(r.side, b.unit(r)) || b.army(r.side).side == Side::Player
}

/// FUN_00470d26 for an AI unit (view only): build the range pages.
pub(super) fn show_range(game: &mut Game, r: UnitRef) {
    if presenting(game) {
        menu::show_range(game, r);
    }
}

/// FUN_00471340 for an AI unit: one presented frame of its range.
pub(super) fn present_range(game: &mut Game, r: UnitRef) {
    if presenting(game) {
        super::view::range_present(game, r);
    }
}

/// FUN_0044cccd(army, unit, 0): walk the path already in the range, then
/// FUN_004448f2(opponent): wake the units whose trigger tiles were touched.
pub(super) fn walk_and_wake(game: &mut Game, r: UnitRef) {
    let k = game.battle.range.path.first().map_or(0, |&d| d + 1);
    menu::walk(game, r, k);
    turn::wake_on_touch(game, r.side.other());
}

/// Port of FUN_0040a4d0: decide and carry out the action of AI unit `r`.
pub fn decide(game: &mut Game, r: UnitRef) {
    super::sync(game);
    let limit = offer_limit(game);
    game.battle.unit_mut(r).act = 1;
    // FUN_00441c0b: the unit is lifted off the map surface.
    super::view::erase_unit(game, r);
    if game.battle.unit(r).status & status::FEAR != 0 && game.battle.rng.pct() > 0x32 {
        return;
    }
    game.battle.build_range(r, 0);
    let (enemy_reach, situation) = STATE.with(|s| s.get());
    let mut ctl = Ctl { unit: r, reach: game.battle.range.longest_path(), enemy_reach, situation, limit };
    actions::set_anim(game, r, 1);
    let mut top = true;
    loop {
        if top && shown(game, r) && game.battle.unit(r).ai_sub == 0 {
            // LAB_0040a570: bring the unit into view and show its range.
            let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
            if presenting(game) {
                if !game.battle.map().on_screen(x, y, 0) {
                    camera::scroll_to(game, x, y, Some(r));
                }
                camera::redraw(game);
                if game.battle.move_type(game.battle.unit(r)) == 3 { super::view::motion::lift_off(game, r, false); }
                game.battle.unit_mut(r).anim_timer = game.ticks();
            }
            show_range(game, r);
        }
        top = true;
        {
            let u = game.battle.unit_mut(r);
            if hp_pct(u) > 0x59 && u.ai_state == 5 {
                u.ai_state = 0;
            }
        }
        let u = game.battle.unit(r).clone();
        let berserk = u.buffs & buff::BERSERK != 0;
        let mut m = u.ai_mode as i32;
        if m != 1 && u.ai_state == 5 {
            m = 5;
        }
        if berserk {
            m = 1;
        }
        match m {
            1 => {
                if u.ai_sub == 1 {
                    if game.battle.target_reachable(r, limit) {
                        game.battle.unit_mut(r).ai_sub = 0;
                        continue;
                    }
                } else {
                    if shown(game, r) {
                        present_range(game, r);
                    }
                    fight(game, &mut ctl);
                }
            }
            3 => turn::ai_finish(game, r),
            4 => keep_away(game, &mut ctl),
            5 => retreat(game, &mut ctl, 0),
            6 => {
                if berserk || !go_to_point(game, &mut ctl, true) {
                    fight(game, &mut ctl);
                }
            }
            7 => approach(game, &mut ctl),
            _ => {
                // 2 (stand by) and the waiting modes: start once something is in reach.
                if game.battle.target_reachable(r, limit) {
                    let w = game.battle.unit_mut(r);
                    w.ai_mode = 1;
                    if w.ai_sub != 0 {
                        w.ai_sub = 0;
                    } else {
                        top = false;
                    }
                    continue;
                }
                if shown(game, r) && u.ai_sub != 1 {
                    present_range(game, r);
                }
                turn::ai_finish(game, r);
            }
        }
        break;
    }
    STATE.with(|s| s.set((ctl.enemy_reach, ctl.situation)));
    game.battle.range.reset();
}

/// Port of FUN_0040aadc: plan (spells from here and, by difficulty, from
/// the casting cells of the range; then every attack from here and, before
/// moving, from every attack cell), act, rebuild the range and repeat;
/// when nothing is worth doing, move by the health/threat rules.
fn fight(game: &mut Game, ctl: &mut Ctl) {
    // Not the original: the optional enhanced AI (`ai_plus.rs`).
    if super::ai_plus::applies(game, ctl.unit) {
        return super::ai_plus::fight(game, ctl);
    }
    let r = ctl.unit;
    loop {
        let u = game.battle.unit(r).clone();
        let berserk = u.buffs & buff::BERSERK != 0;
        let mut atk_cost = game.battle.c(0xee);
        if u.abilities & 0x2000_0000 != 0 {
            atk_cost -= 4;
        }
        if u.turn_flags & tf::ATTACKED != 0 && u.abilities & 0x8000 == 0 {
            atk_cost += game.battle.c(0xf3);
        }
        ctl.situation = game.battle.assess(ctl);
        let nodes = stops(&game.battle);
        let (x0, y0, f0) = (u.x, u.y, u.facing);
        let mut best = Best::new(ctl.limit);
        let mut plan = SpellPlan::default();
        let mut move_to = 0usize;
        let mut target: Option<(UnitRef, usize)> = None;
        if !berserk {
            super::view::panel::ai_thinking(game, r);
            let b = &mut game.battle;
            if b.knows_skill_kind(r, 1) {
                b.consider_spells(r, false, &mut best, &mut plan);
            }
            if b.knows_skill_kind(r, 2) {
                b.consider_spells(r, true, &mut best, &mut plan);
            }
            let p = match b.difficulty {
                1 => 5,
                2 => 0x1e,
                3 => 0x32,
                4 => 0x50,
                _ => 100,
            };
            if b.rng.pct() <= p {
                for &(i, n) in &nodes {
                    if n.can_stop != 0 && n.mark > 1 {
                        set_pos(b, r, n.x as i32, n.y as i32);
                        if b.knows_skill_kind(r, 1) && b.consider_spells(r, false, &mut best, &mut plan) {
                            move_to = i + 1;
                        }
                        if b.knows_skill_kind(r, 2) && b.consider_spells(r, true, &mut best, &mut plan) {
                            move_to = i + 1;
                        }
                    }
                }
            }
            if (best.kind > 0x1d && ctl.situation != 1) || (best.kind > 0 && best.score == 0) {
                move_to = 0;
                best = Best::new(ctl.limit);
                plan = SpellPlan::default();
            }
        }
        {
            let b = &mut game.battle;
            let t = b.t();
            let actions = t.kinds.class(u.kind as u16).map_or(0..0, |c| c.actions.clone());
            'search: for k in actions {
                let kind = b.action_kind(k);
                set_pos(b, r, x0, y0);
                if kind == 0xb || kind == 0xc || !b.skill_usable(b.unit(r), k) || (b.unit(r).ap as i32) < atk_cost {
                    continue;
                }
                if let Some(e) = b.best_attack_target(r, k, &mut best) {
                    target = Some((e, k));
                    move_to = 0;
                    plan.spell = 0;
                    if best.kind == 1 {
                        break;
                    }
                }
                if b.unit(r).turn_flags & tf::MOVED == 0 {
                    for &(i, n) in &nodes {
                        if n.can_stop != 0 && n.mark != 0 {
                            set_pos(b, r, n.x as i32, n.y as i32);
                            if let Some(e) = b.best_attack_target(r, k, &mut best) {
                                target = Some((e, k));
                                move_to = i + 1;
                                plan.spell = 0;
                                if best.kind == 1 {
                                    break 'search;
                                }
                            }
                        }
                    }
                }
            }
            set_pos(b, r, x0, y0);
            b.unit_mut(r).facing = f0;
        }
        let u = game.battle.unit(r).clone();
        let low = |v: i16, max: i16| max > 0 && v < max && (v as i32 * 100) / (max as i32) < 0x14;
        let low_res = low(u.mp, u.mp_max) || low(u.st, u.st_max);
        let hp = hp_pct(&u);
        let crit = critical(&u);
        if best.kind == 3 && best.score < 2000 {
            best.kind = 0;
        }
        let heal = plan.spell > 0 && matches!(game.battle.spell_aim(plan.spell as i32).effect, 0x12 | 0x31 | 0x5a);
        let act = best.kind > 0 && (!crit || best.kind == 1 || (best.kind == 2 && best.kills > 0) || (best.kind == 3 && heal));
        if !act {
            idle(game, ctl, hp, crit, low_res);
            return;
        }
        if move_to > 0 {
            let move5 = game.battle.unit(r).flags & flag::MOVE5 != 0;
            game.battle.range.build_path(move_to, move5);
            walk_and_wake(game, r);
            game.battle.unit_mut(r).turn_flags |= tf::MOVED;
        }
        if plan.spell < 1 {
            let Some((e, k)) = target else {
                idle(game, ctl, hp, crit, low_res);
                return;
            };
            game.battle.cursor = (game.battle.unit(e).x, game.battle.unit(e).y);
            attack(game, r, e, k);
        } else {
            cast(game, r, plan);
        }
        // Not in the original: a unit the counter (or its own spell) killed stops here.
        if !game.battle.unit(r).on_map() {
            return;
        }
        game.battle.range.reset();
        game.battle.build_range(r, 0);
        show_range(game, r);
        actions::set_anim(game, r, 1);
    }
}

/// LAB_0040b574 of FUN_0040aadc: nothing worth doing — move by the rules.
fn idle(game: &mut Game, ctl: &mut Ctl, hp: i32, crit: bool, low_res: bool) {
    let r = ctl.unit;
    let u = game.battle.unit(r).clone();
    if u.ai_mode == 6 || u.ai_mode == 7 || u.buffs & buff::BERSERK != 0 {
        approach(game, ctl);
    } else if crit {
        retreat(game, ctl, 0);
    } else if low_res {
        // FUN_0040bf2f
        if ctl.situation == 3 {
            close_in(game, ctl, false);
            turn::ai_finish(game, r);
        } else {
            retreat(game, ctl, 1);
        }
    } else if u.turn_flags & 0xe == 0 {
        approach(game, ctl);
    } else if u.turn_flags & tf::MOVED == 0 {
        ctl.situation = game.battle.assess(ctl);
        let (a, b, c) = if ctl.situation == 1 { (0x56, 0x4c, 0x33) } else { (0x60, 0x56, 0x3d) };
        if ctl.situation == 3 {
            approach(game, ctl);
        } else if hp >= a {
            close_in(game, ctl, true);
        } else if hp >= b {
            close_in(game, ctl, false);
        } else if hp >= c {
            step_toward(game, ctl);
        } else {
            retreat(game, ctl, 0);
        }
        turn::ai_finish(game, r);
    } else {
        turn::ai_finish(game, r);
    }
}

/// Port of FUN_0040a8fa: move toward the enemy (modes 6/7 toward the map
/// goal) unless already moved; berserk units and easy-difficulty enemies
/// head for the nearest idle unit they can stand next to, others decide by
/// health (FUN_0040cb85).
pub(super) fn approach(game: &mut Game, ctl: &mut Ctl) {
    let r = ctl.unit;
    let u = game.battle.unit(r).clone();
    let berserk = u.buffs & buff::BERSERK != 0;
    let b = &game.battle;
    let easy_enemy = b.difficulty == 1 && !b.vs && b.army(r.side).side == Side::Enemy;
    if u.turn_flags & tf::MOVED != 0 || !((!easy_enemy && !berserk) || u.turn_flags & 0xe == 0) {
        turn::ai_finish(game, r);
        return;
    }
    if (u.ai_mode == 6 || u.ai_mode == 7) && !berserk {
        go_to_point(game, ctl, false);
        return;
    }
    if shown(game, r) {
        present_range(game, r);
    }
    if !berserk && !easy_enemy {
        decide_close(game, ctl);
    } else if let Some(e) = nearest_ready_reachable(&game.battle, r.side.other(), r) {
        let (ex, ey) = (game.battle.unit(e).x, game.battle.unit(e).y);
        if game.battle.range_closest_to(ex, ey, 0) {
            walk_and_wake(game, r);
        }
    }
    turn::ai_finish(game, r);
}

/// Port of FUN_0040cb85: safe → close in (cautiously when hurt); healthy →
/// close in eagerly; badly hurt → retreat; else close in cautiously.
fn decide_close(game: &mut Game, ctl: &mut Ctl) {
    let r = ctl.unit;
    let m = if ctl.situation == 3 {
        let u = game.battle.unit(r);
        if u.hp < u.hp_max { 2 } else { 3 }
    } else if game.battle.fit_to_advance(r, ctl.situation) {
        3
    } else if game.battle.should_retreat(r, ctl.situation) {
        1
    } else {
        2
    };
    match m {
        1 => retreat(game, ctl, 0),
        2 => close_in(game, ctl, false),
        _ => close_in(game, ctl, true),
    }
}

/// Port of FUN_0040cc56: walk to the stop cell nearest the nearest visible
/// enemy that keeps `rules[0xed]` AP (+ the rest cost unless `eager`) and
/// is not next to an enemy.
fn close_in(game: &mut Game, ctl: &mut Ctl, eager: bool) {
    let r = ctl.unit;
    let Some(e) = nearest_to(&game.battle, r.side.other(), r, 0) else { return };
    let mut mb = game.battle.c(0xed);
    if !eager {
        mb += game.battle.c(0xf0);
    }
    let (ex, ey) = (game.battle.unit(e).x, game.battle.unit(e).y);
    if game.battle.range_closest_to(ex, ey, mb) {
        walk_and_wake(game, r);
    }
}

/// Port of FUN_0040aa76: one cautious step (FUN_00471923) unless already moved.
fn step_toward(game: &mut Game, ctl: &mut Ctl) {
    let r = ctl.unit;
    if game.battle.unit(r).turn_flags & tf::MOVED == 0 && game.battle.regroup_cell(r) {
        walk_and_wake(game, r);
        game.battle.unit_mut(r).turn_flags |= tf::MOVED;
    }
}

/// Port of FUN_0040bf6a: walk away from the nearest enemy (not after
/// moving; easy-difficulty enemies only before acting), then end the turn;
/// a unit below its retreat threshold starts retreating (`ai_state = 5`)
/// unless `keep`.
fn retreat(game: &mut Game, ctl: &mut Ctl, keep: i32) {
    let r = ctl.unit;
    let u = game.battle.unit(r).clone();
    let b = &game.battle;
    let allowed = b.difficulty > 1 || b.vs || b.army(r.side).side != Side::Enemy || u.turn_flags & 0xe == 0;
    if u.turn_flags & tf::MOVED != 0 || !allowed {
        turn::ai_finish(game, r);
        return;
    }
    if shown(game, r) {
        present_range(game, r);
    }
    let (threat, node) = game.battle.flee_cell(r, keep);
    if threat.is_some() {
        let move5 = u.flags & flag::MOVE5 != 0;
        game.battle.range.build_path(node.max(0) as usize, move5);
        walk_and_wake(game, r);
    }
    turn::ai_finish(game, r);
    let u = game.battle.unit(r);
    if u.buffs & buff::BERSERK == 0 && u.ai_state != 5 && keep == 0 && game.battle.should_retreat(r, 0) {
        game.battle.unit_mut(r).ai_state = 5;
    }
}

/// Port of FUN_0040c0b3 (mode 4): keep away from the enemy; with nowhere
/// to go and HP above 70% fight instead.
fn keep_away(game: &mut Game, ctl: &mut Ctl) {
    let r = ctl.unit;
    if shown(game, r) {
        present_range(game, r);
    }
    let (threat, node) = game.battle.flee_cell(r, 0);
    if node == -1 && hp_pct(game.battle.unit(r)) > 0x46 {
        fight(game, ctl);
    } else if threat.is_none() {
        turn::ai_finish(game, r);
    } else {
        let move5 = game.battle.unit(r).flags & flag::MOVE5 != 0;
        game.battle.range.build_path(node.max(0) as usize, move5);
        walk_and_wake(game, r);
        turn::ai_finish(game, r);
    }
}

/// Port of FUN_0040cd2a: head for the map goal (object class 3, else 5 for
/// the enemy side; 4, else 6 for the player side): to the goal itself or
/// the reachable cell nearest to it. `strict` only accepts the goal and
/// reports false otherwise (nothing done); else the turn ends.
fn go_to_point(game: &mut Game, ctl: &mut Ctl, strict: bool) -> bool {
    let r = ctl.unit;
    let player = game.battle.army(r.side).side == Side::Player;
    let (c1, c2) = if player { (4, 6) } else { (3, 5) };
    let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    let goal = game.battle.find_goal(c1, ux, uy).or_else(|| game.battle.find_goal(c2, ux, uy));
    let Some((gx, gy)) = goal.filter(|_| game.battle.range.count != 1) else {
        if strict {
            return false;
        }
        turn::ai_finish(game, r);
        return true;
    };
    let (mut bd, mut node) = (-1, 0usize);
    for (i, n) in stops(&game.battle) {
        if n.can_stop == 0 {
            continue;
        }
        let (nx, ny) = (n.x as i32, n.y as i32);
        if nx == gx && ny == gy {
            bd = 0;
            node = i + 1;
            break;
        }
        if !game.battle.no_stop_for(r, nx, ny) {
            let d = distance(nx, ny, gx, gy);
            if bd == -1 || d < bd {
                node = i + 1;
                bd = d;
            }
        }
    }
    if strict && (bd == -1 || { let n = game.battle.range.nodes[node - 1]; (n.x as i32, n.y as i32) != (gx, gy) }) {
        return false;
    }
    if bd != -1 {
        let move5 = game.battle.unit(r).flags & flag::MOVE5 != 0;
        game.battle.range.build_path(node, move5);
        walk_and_wake(game, r);
        game.battle.unit_mut(r).turn_flags |= tf::MOVED;
    }
    turn::ai_finish(game, r);
    true
}

/// Port of FUN_00436b24: the AI's attack — face the target, a fresh
/// forecast (another counter pick), the forecast pause, the attack
/// (FUN_00436e46), then the AP of an attack (rules[0xee], 4 back with
/// ability 0x20000000, + rules[0xf3] for the second attack of the turn
/// unless ability 0x8000).
pub(super) fn attack(game: &mut Game, r: UnitRef, e: UnitRef, skill: usize) {
    let (ex, ey) = (game.battle.unit(e).x, game.battle.unit(e).y);
    // The move range is drawn only by the range frames (FUN_00471340); the
    // scroll, the attack range and the attack draw the plain map.
    super::view::hide_range(game);
    if presenting(game) {
        camera::scroll_exact(game, ex, ey, Some(r));
    }
    let tl = game.battle.target_list(r, skill);
    game.battle.unit_mut(r).act = 5;
    super::view::show_attack_range(game, &tl);
    let t0 = game.ticks();
    game.battle.unit_mut(r).anim_timer = t0 + 0x13;
    let ti = tl.targets.iter().position(|t| t.x == ex && t.y == ey);
    if let Some(ti) = ti {
        actions::face(game, r, tl.targets[ti].facing);
        let duel = game.battle.duel(r, e, skill);
        if presenting(game) {
            super::view::panel::ai_attack_pause(game, r, (ex, ey), &duel, t0);
        }
        actions::attack(game, &tl, ti, duel);
    }
    // (The original reads past the target list when the target is gone.)
    let (ee, f3) = (game.battle.c(0xee), game.battle.c(0xf3));
    let u = game.battle.unit_mut(r);
    u.ap -= ee as i16;
    if u.abilities & 0x2000_0000 != 0 {
        u.ap += 4;
    }
    if u.turn_flags & tf::ATTACKED == 0 {
        u.turn_flags |= tf::ATTACKED;
    } else if u.abilities & 0x8000 == 0 {
        u.ap -= f3 as i16;
    }
    if u.ap < u.ap_carry {
        u.ap_carry = u.ap;
    }
}

/// Port of FUN_0046bb67: cast the planned spell — technique when its id is
/// past the magic schools, combos with their partner (or not at all),
/// aim (FUN_004055a4), the effects (FUN_0045ff9e), then the costs.
pub(super) fn cast(game: &mut Game, r: UnitRef, plan: SpellPlan) {
    let b = &mut game.battle;
    let t = b.t();
    let first_tech = t.spells.school_spells.get(t.spells.magic_schools as usize).map_or(0, |s| s.0 as i32);
    let technique = first_tech < plan.spell as i32;
    let aim = b.spell_aim(plan.spell as i32);
    let mut partner = None;
    if aim.combo & 7 != 0 {
        partner = b.combo_partner(r, plan.spell);
        if partner.is_none() {
            return;
        }
    }
    let mut tg = b.spell_target(r.side, r, plan.spell as i32, technique as u8, 2, false);
    b.target_range(&mut tg, aim.range);
    super::cast::ai_aim(game, &mut tg, plan.cell.0, plan.cell.1, plan.dir);
    let mut c = Cast::new(r.side, r, plan.spell, plan.cell);
    c.partner = partner;
    c.technique = technique;
    super::cast::cast_effects(game, &mut c, &mut tg);
    tg.free();
    game.battle.pay_spell_cost(&c);
    actions::set_anim(game, r, 1);
}

/// The enemy phase (FUN_0043310e) or, for the player army, the NPC allies
/// and berserk units after the player phase (FUN_00433da4): FUN_00433db9
/// mode 1 over `turn::ai_act`. Returns 0, 1 (victory) or 2 (defeat).
pub fn run_phase(game: &mut Game, side: Side) -> i32 {
    turn::npc_phase(game, side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::spells::tests::stage1;

    #[test]
    fn offer_follows_the_comparison_table() {
        let mut b = Best::new(1000);
        assert!(!b.offer(0, 50, 0), "type 0 is never kept");
        assert!(b.offer(2, 100, 0));
        assert!(!b.offer(2, 90, 0), "lower score");
        assert!(b.offer(2, 50, 1), "a kill beats a bigger score");
        assert!(!b.offer(2, 900, 0), "no kill never beats a kill");
        assert!(b.offer(1, 10, 1), "a VIP kill beats any type 2");
        assert!(!b.offer(2, 10_000, 3));
        assert!(b.offer(1, 11, 0));
        // Support types against a type 2 / 3 best: only below the limit (L, L/2).
        let mut b = Best::new(1000);
        b.offer(2, 400, 0);
        assert!(!b.offer(0x36, 300, 0), "S < s required");
        assert!(b.offer(0x36, 500, 0), "S < L and S < s");
        let mut b = Best::new(1000);
        b.offer(2, 1200, 0);
        assert!(!b.offer(0x36, 5000, 0), "S ≥ L");
        // Type 3 against support: L/2 decides.
        let mut b = Best { kind: 0x36, score: 100, kills: 0, limit: 1000 };
        assert!(b.offer(3, 600, 0), "above L/2");
        let mut b = Best { kind: 0x36, score: 450, kills: 0, limit: 1000 };
        assert!(!b.offer(3, 400, 0));
    }

    /// The heaviest enemy fighter of stage 1 next to a player unit, both
    /// with fixed stats: the AI's expected-damage scoring by hand.
    #[test]
    fn attack_score_matches_the_formula() {
        let Some(mut b) = stage1() else { return };
        let e = UnitRef::new(Side::Enemy, 0);
        let p = UnitRef::new(Side::Player, 0);
        // Put the player unit right next to the enemy (down-right neighbour).
        let (ex, ey) = (b.unit(e).x, b.unit(e).y);
        let (nx, ny) = crate::battle::map::neighbour(ex, ey, 3);
        b.unit_mut(p).x = nx;
        b.unit_mut(p).y = ny;
        let t = b.t();
        let class = t.kinds.class(b.unit(e).kind as u16).unwrap();
        let skill = class.actions.clone().find(|&k| b.skill_usable(b.unit(e), k) && b.target_list(e, k).target_at(nx, ny) != 0);
        let Some(skill) = skill else { return };
        if b.target_list(e, skill).shape == 3 {
            return; // the replay below leaves out the piercing ray
        }
        // Replay: the same RNG state gives the same forecast and checks.
        let snap = b.rng.snapshot();
        let mut best = Best::new(0);
        let got = b.best_attack_target(e, skill, &mut best);
        let after = b.rng.snapshot();
        b.rng.restore(snap);
        let tl = b.target_list(e, skill);
        let mut expect = None;
        let mut want = Best::new(0);
        let d = b.difficulty as usize;
        for tg in &tl.targets {
            b.unit_mut(e).facing = tg.facing;
            let du = b.duel(e, tg.unit, skill);
            let def = b.unit(tg.unit).clone();
            let mut s = b.duel_expected(&du);
            let (mut kills, mut vk) = (0, 0);
            if def.is_vip() {
                s *= b.cu(0x100 + d);
            }
            if (def.hp as i32) < du.calc.fc_dmg && du.calc.fc_hit > 50 {
                s *= b.cu(0x105 + d);
                kills = 1;
                vk = def.is_vip() as i32;
            }
            s -= b.duel_expected_counter(&du);
            let me = b.unit(e);
            if (du.calc.fc_cdmg <= me.hp as i32 || du.calc.fc_chit < 51) && s > 0 && want.offer(if vk > 0 { 1 } else { 2 }, s, kills) {
                expect = Some(tg.unit);
            }
        }
        assert_eq!(got, expect);
        assert_eq!(best, want);
        assert_eq!(b.rng.snapshot(), after, "same RNG consumption");
        eprintln!("attack {skill}: {got:?} {best:?}");
        if got.is_some() {
            assert!(best.kind == 1 || best.kind == 2);
            assert!(best.score > 0);
        }
    }

    /// 邪惡之箭 (shape 1, enemies) on the protagonist two cells away: one
    /// range cell scores `chance · min(amount, hp)` ×VIP (×kill), type 2.
    #[test]
    fn damage_spell_score_matches_the_formula() {
        let Some(mut b) = stage1() else { return };
        let e = UnitRef::new(Side::Enemy, 0);
        let p = UnitRef::new(Side::Player, 0);
        let (ex, ey) = (b.unit(e).x, b.unit(e).y);
        let (nx, ny) = crate::battle::map::neighbour(ex, ey, 3);
        let (px, py) = crate::battle::map::neighbour(nx, ny, 3);
        b.unit_mut(p).x = px;
        b.unit_mut(p).y = py;
        let t = b.t();
        let id = t.spells.spells.iter().position(|s| s.name == "邪惡之箭").unwrap() as u16 + 1;
        let sp = t.spells.spells[id as usize - 1].clone();
        b.unit_mut(e).learned[sp.school as usize - 1] |= 1 << (sp.index - 1);
        let cast = Cast::new(Side::Enemy, e, id, (0, 0));
        let mut tg = b.spell_target(Side::Enemy, e, id as i32, 0, 0, false);
        b.target_range(&mut tg, sp.range as i32);
        assert!(tg.range.contains(&(px, py)));
        let snap = b.rng.snapshot();
        let (mut best, mut plan) = (Best::new(0), SpellPlan::default());
        assert!(b.ai_score_spell(&mut tg, &cast, &mut best, &mut plan));
        let after = b.rng.snapshot();
        b.rng.restore(snap);
        let (ch, am, _) = b.spell_roll(&cast, p, Side::Player, false);
        let hp = b.unit(p).hp as i32;
        let am = am.min(hp);
        let d = b.difficulty as usize;
        assert!(b.unit(p).is_vip(), "志超 (character 11) is a VIP");
        let mut v = ch * am * b.cu(0x100 + d);
        let kill = am >= hp && ch > 50;
        if kill {
            v *= b.cu(0x105 + d);
        }
        eprintln!("邪惡之箭: chance {ch} amount {am} hp {hp} → {best:?} at {:?}", plan.cell);
        assert_eq!(best, Best { kind: if kill { 1 } else { 2 }, score: v, kills: kill as i32, limit: 0 });
        assert_eq!(plan.cell, (px, py));
        assert_eq!(b.rng.snapshot(), after, "one expected-value roll");
    }

    #[test]
    fn thresholds_by_class_type() {
        let Some(mut b) = stage1() else { return };
        let r = UnitRef::new(Side::Enemy, 0);
        let max = b.unit(r).hp_max;
        let ty = unit_type(&b, b.unit(r));
        let (eager_need, calm_need) = match ty {
            6 | 7 | 0x13 | 0x14 => (70, 90),
            4 | 0x17 => (70, 80),
            3 => (60, 70),
            _ => (45, 50),
        };
        b.unit_mut(r).abilities &= !0x80;
        b.unit_mut(r).status &= !status::INVISIBLE;
        b.unit_mut(r).mp = b.unit(r).mp_max;
        b.unit_mut(r).st = b.unit(r).st_max;
        // ceil → at least `pct`%, floor of one less → below it.
        b.unit_mut(r).hp = ((max as i32 * calm_need + 99) / 100) as i16;
        assert!(b.fit_to_advance(r, 0));
        b.unit_mut(r).hp = (max as i32 * (calm_need - 1) / 100) as i16;
        assert!(!b.fit_to_advance(r, 0));
        assert!(b.fit_to_advance(r, 1) == (b.unit(r).hp as i32 * 100 / max as i32 >= eager_need));
        // Full HP never retreats; 0 HP always does.
        b.unit_mut(r).hp = max;
        assert!(!b.should_retreat(r, 0));
        b.unit_mut(r).hp = 0;
        assert!(b.should_retreat(r, 0));
    }

    /// The whole enemy phase of stage 1 from the initial deployment, twice
    /// with the same seed: the same decisions, and every enemy has acted.
    #[test]
    fn stage1_enemy_phase_is_deterministic() {
        let Some(d) = crate::battle::spells::tests::data() else { return };
        if stage1().is_none() {
            return;
        }
        let run = || {
            reset_state();
            let mut sys = crate::sys::Sys::new(crate::sys::Shared::new(), crate::audio::Audio::silent());
            let mut game = Game::new(&mut sys, &d);
            game.events.ui = crate::event::ui::UiHooks::headless();
            let stages = s2data::script::Stage::parse_all(&d.arcs.read(s2data::Kind::Bin, s2data::script::BIN_STAGES).unwrap()).unwrap();
            crate::event::attach_stage(&mut game, &stages[0], true);
            game.battle = stage1().unwrap();
            game.campaign.turn = 1;
            game.campaign.difficulty = game.battle.difficulty;
            crate::battle::sync(&mut game);
            let before: Vec<(i32, i32, i16)> = game.battle.army(Side::Enemy).units.iter().map(|u| (u.x, u.y, u.ap)).collect();
            for u in &game.battle.army(Side::Enemy).units {
                eprintln!("enemy {} class {} mode {} sub {} npc {} at ({}, {})", u.name_str(), u.kind, u.ai_mode, u.ai_sub, u.npc, u.x, u.y);
            }
            for u in &game.battle.army(Side::Player).units {
                eprintln!("player {} at ({}, {}) hp {}", u.name_str(), u.x, u.y, u.hp);
            }
            let code = run_phase(&mut game, Side::Enemy);
            let after: Vec<(i32, i32, i16, u8, u32, i16)> =
                game.battle.army(Side::Enemy).units.iter().map(|u| (u.x, u.y, u.ap, u.act, u.turn_flags, u.hp)).collect();
            let players: Vec<i16> = game.battle.army(Side::Player).units.iter().map(|u| u.hp).collect();
            (code, before, after, players, game.battle.rng.snapshot())
        };
        let a = run();
        let b = run();
        assert_eq!(a.0, b.0);
        assert_eq!(a.2, b.2, "same enemy decisions");
        assert_eq!(a.3, b.3, "same damage to the party");
        assert_eq!(a.4, b.4, "same RNG consumption");
        assert!(a.0 == 0 || a.0 == 1 || a.0 == 2);
        for (k, (bef, aft)) in a.1.iter().zip(&a.2).enumerate() {
            let u = &aft;
            eprintln!("enemy {k}: ({}, {}) ap {} -> ({}, {}) ap {} act {} flags {:#x} hp {}", bef.0, bef.1, bef.2, u.0, u.1, u.2, u.3, u.4, u.5);
        }
        // Every deployed, able enemy took its turn (act 2 = done).
        for u in &a.2 {
            assert!(u.3 == 2 || u.3 == 0, "act {}", u.3);
        }
        let acted = a.2.iter().filter(|u| u.3 == 2).count();
        assert!(acted > 0);
    }
}
