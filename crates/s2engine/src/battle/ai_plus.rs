//! The enhanced enemy AI (not the original; `Game::options.enhanced_ai`,
//! opensuper2.ini `ai=enhanced`). The original AI (`ai.rs`) plays every
//! unit unless the option is on; then it still decides everything outside
//! combat (the modes, their wake-up rules and the story moves) and hands the
//! fight itself (`ai::fight`, FUN_0040aadc) of enemy-army units to
//! [`fight`] here, outside VS and not for berserk units.
//!
//! It plays by the same rules: the same information the player has (visible
//! units, the forecasts the panels show), the same action costs and the same
//! battle code for moves, attacks and casts. Its evaluation consumes no RNG
//! (the stream is restored), so only the actions it takes draw numbers.
//!
//! Per action it scores every stop cell of the unit's range:
//! - attacks: expected damage relative to the target's HP (focus fire),
//!   kills, the target's priority (the protagonist, healers, casters,
//!   summoners, the units that threaten most), minus the expected counter
//!   (an attack whose counter could kill is avoided);
//! - spells: the original's aim per spell (`ai_score_spell`), then the area's
//!   value with the same terms (allies hit count against it), heals by the
//!   ally's need, debuffs by the target's priority, and buffs on the own side
//!   (which the original never casts, FUN_004063c4's sign);
//! - the cell: the threat map — what the opposing units could do to this
//!   unit there next turn (each one's move range at a turn's AP plus its
//!   reach, with its best expected hit), lowered by the cell's terrain
//!   defence; deadly cells are avoided unless the action is worth it.
//!
//! With nothing worth doing it repositions: badly hurt units step out of the
//! threat (toward allies); the others wait just outside the threat until
//! enough allies are close to engage together (up to `HOLD_TURNS`), then
//! advance on the best target over the safest cells.

use super::actions;
use super::ai::{self, Ctl, SpellPlan};
use super::army::{Side, UnitRef};
use super::map::distance;
use super::spells::{Cast, Shortage};
use super::turn;
use super::unit::{buff, flag, status, turn as tf};
use super::Battle;
use crate::game::Game;

/// Up to this campaign turn units wait for the group before engaging.
pub const HOLD_TURNS: i32 = 8;
/// Minimum value (in "HP points": 100 = a full HP bar) of an action.
const MIN_VALUE: f32 = 2.0;
/// At most this many cells besides the unit's own are tried for spells.
const SPELL_CELLS: usize = 16;

/// Whether the enhanced AI fights for `r`: the option on, an enemy-army
/// unit, not VS, not berserk.
pub fn applies(game: &Game, r: UnitRef) -> bool {
    let b = &game.battle;
    game.options.enhanced_ai && !b.vs && b.army(r.side).side == Side::Enemy && b.unit(r).buffs & buff::BERSERK == 0
}

// ---------------------------------------------------------------------------
// Threat map.

/// What the opposing side could do to one unit next turn, per map cell.
#[derive(Debug, Clone, Default)]
pub struct ThreatMap {
    pub width: i32,
    pub rows: i32,
    /// Expected damage at each cell: the two strongest opponents reaching
    /// it (more could not all strike one unit).
    pub dmg: Vec<i32>,
    /// The strongest and second strongest hit per cell.
    top: Vec<(i32, i32)>,
    /// Each opponent considered and its best expected hit on the unit.
    pub sources: Vec<(UnitRef, i32)>,
    /// What loses the stage for the opponents (shown to the player too).
    pub goals: Goals,
}

/// The stage's lose conditions as targets: characters whose death loses
/// (lose codes 1..=0x2b) and whether losing every escort (`+8 == 1`
/// units, code 0) does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Goals {
    pub chars: Vec<i16>,
    pub escorts: bool,
}

impl Goals {
    pub fn from_codes(codes: &[i16]) -> Goals {
        Goals { chars: codes.iter().copied().filter(|c| (1..=0x2b).contains(c)).collect(), escorts: codes.contains(&0) }
    }
}

impl ThreatMap {
    pub fn at(&self, x: i32, y: i32) -> i32 {
        if x < 0 || y < 0 || x >= self.width || y >= self.rows {
            return 0;
        }
        self.dmg[(y * self.width + x) as usize]
    }

    fn add(&mut self, k: usize, hit: i32) {
        let t = &mut self.top[k];
        if hit > t.0 {
            *t = (hit, t.0);
        } else if hit > t.1 {
            t.1 = hit;
        }
        self.dmg[k] = t.0 + t.1;
    }

    /// The expected hit of opponent `p` on the unit (0 if not a source).
    pub fn hit_of(&self, p: UnitRef) -> i32 {
        self.sources.iter().find(|s| s.0 == p).map_or(0, |s| s.1)
    }
}

const DAMAGE_EFFECTS: [u8; 7] = [1, 3, 0x24, 0x29, 0x2a, 0x30, 0x40];
const HEAL_EFFECTS: [u8; 3] = [0x12, 0x31, 0x5a];

fn damage_effect(e: u8) -> bool {
    DAMAGE_EFFECTS.contains(&e)
}

fn heal_effect(e: u8) -> bool {
    HEAL_EFFECTS.contains(&e)
}

fn summon_effect(e: u8) -> bool {
    (0x15..=0x1f).contains(&e)
}

/// The 1-based spell ids `r` has learned (magic and techniques).
fn known_spells(b: &Battle, r: UnitRef) -> Vec<(u16, bool)> {
    let t = b.t();
    let mut out = Vec::new();
    for technique in [false, true] {
        for (school, idxs) in b.spell_lists(r, technique) {
            let first = t.spells.school_spells[school - 1].0 as u16;
            out.extend(idxs.iter().map(|&i| (first + i, technique)));
        }
    }
    out
}

/// Can `p` act on its next turn (alive, on the map, not held by a status)?
fn able(b: &Battle, p: UnitRef) -> bool {
    let u = b.unit(p);
    u.on_map() && u.can_act() && u.status & status::FEAR == 0
}

/// `p`'s best expected hit on `me` (attacks with their forecast, damage
/// spells with their expectation) and the reach it needs (the longest
/// action range).
fn expected_hit(b: &mut Battle, p: UnitRef, me: UnitRef) -> (i32, i32) {
    let u = b.unit(p).clone();
    let hp = b.unit(me).hp.max(1) as i32;
    let t = b.t();
    let mut best = 0;
    let mut reach = 0;
    if let Some(c) = t.kinds.class(u.kind as u16) {
        for k in c.actions.clone() {
            let kind = b.action_kind(k);
            if kind == 0xb || kind == 0xc || !b.skill_usable(&u, k) {
                continue;
            }
            let du = b.duel(p, me, k);
            let v = du.calc.fc_hit.clamp(0, 100) * du.calc.fc_dmg.clamp(0, hp) / 100;
            best = best.max(v);
            reach = reach.max(b.skill_range(&u, k).1);
        }
    }
    for (spell, technique) in known_spells(b, p) {
        let aim = b.spell_aim(spell as i32);
        if !damage_effect(aim.effect) || aim.target == 0 || aim.target == 1 {
            continue;
        }
        if b.spell_shortage(&u, spell) != Shortage::Ok || (!technique && u.status & status::SEALED != 0) {
            continue;
        }
        let mut cast = Cast::new(p.side, p, spell, (b.unit(me).x, b.unit(me).y));
        cast.technique = technique;
        if aim.combo & 7 != 0 {
            match b.combo_partner(p, spell) {
                Some(q) => cast.partner = Some(q),
                None => continue,
            }
        }
        let (ch, val, _) = b.spell_roll(&cast, me, me.side, false);
        let v = ch.clamp(0, 100) * val.clamp(0, hp) / 100;
        if v > 0 {
            best = best.max(v);
            reach = reach.max(aim.range + 1);
        }
    }
    (best, reach.max(1))
}

/// Port-free helper: every map cell within `reach` of (x, y).
fn for_cells_within(width: i32, rows: i32, x: i32, y: i32, reach: i32, mut f: impl FnMut(i32, i32)) {
    for cy in (y - 2 * reach - 1).max(0)..=(y + 2 * reach + 1).min(rows - 1) {
        for cx in (x - reach - 1).max(0)..=(x + reach + 1).min(width - 1) {
            if distance(x, y, cx, cy) <= reach {
                f(cx, cy);
            }
        }
    }
}

/// The threat map of `me`: each able, visible opponent's move range (built
/// as at the start of its turn), every cell within its reach of a cell it
/// could stop on, with its best expected hit on `me`.
pub fn threat_map(b: &mut Battle, me: UnitRef) -> ThreatMap {
    let (width, rows) = (b.map().width(), b.map().rows());
    let n = (width * rows).max(0) as usize;
    let mut tm = ThreatMap { width, rows, dmg: vec![0; n], top: vec![(0, 0); n], sources: Vec::new(), goals: Goals::default() };
    let saved_range = b.range.clone();
    let snap = b.rng.snapshot();
    let foe = me.side.other();
    let mut seen = vec![false; tm.dmg.len()];
    for i in 0..b.army(foe).units.len() {
        let p = UnitRef::new(foe, i);
        if !able(b, p) || b.is_hidden(foe, b.unit(p)) {
            continue;
        }
        let (hit, reach) = expected_hit(b, p, me);
        if hit <= 0 {
            continue;
        }
        tm.sources.push((p, hit));
        // Its range with a whole turn's AP.
        let ap = b.unit(p).ap;
        {
            let u = b.unit_mut(p);
            u.ap = u.ap.max(u.ap_base + u.ap_carry);
        }
        b.build_range(p, 0);
        b.unit_mut(p).ap = ap;
        let mut cells: Vec<(i32, i32)> = vec![(b.unit(p).x, b.unit(p).y)];
        cells.extend(b.range.reachable().filter(|(_, n)| n.can_stop != 0).map(|(_, n)| (n.x as i32, n.y as i32)));
        seen.iter_mut().for_each(|s| *s = false);
        let mut hits: Vec<usize> = Vec::new();
        for (x, y) in cells {
            for_cells_within(width, rows, x, y, reach, |cx, cy| {
                let k = (cy * width + cx) as usize;
                if !seen[k] {
                    seen[k] = true;
                    hits.push(k);
                }
            });
        }
        for k in hits {
            tm.add(k, hit);
        }
    }
    b.range = saved_range;
    b.rng.restore(snap);
    tm
}

// ---------------------------------------------------------------------------
// Target priority.

/// How much the enemy side wants opponent `e` gone (1 = an ordinary unit):
/// the protagonists / leaders (their death usually loses the stage),
/// healers, casters and summoners, and the units that hit `me` hardest.
pub fn priority(b: &Battle, e: UnitRef, me: UnitRef, tm: &ThreatMap) -> f32 {
    let u = b.unit(e);
    let mut p = 1.0;
    if tm.goals.chars.contains(&u.char_id) {
        p += 2.0;
    } else if u.is_vip() {
        p += 1.2;
    }
    if tm.goals.escorts && u.npc == 1 {
        // Each escort counts more as fewer remain.
        let left = b.army(e.side).units.iter().filter(|o| o.npc == 1 && o.on_map()).count().max(1);
        p += 0.5 + 3.0 / left as f32;
    }
    let spells = known_spells(b, e);
    let effects: Vec<u8> = spells.iter().map(|&(s, _)| b.spell_aim(s as i32).effect).collect();
    if effects.iter().any(|&f| heal_effect(f)) {
        p += 0.6;
    } else if effects.iter().any(|&f| damage_effect(f)) {
        p += 0.35;
    }
    if effects.iter().any(|&f| summon_effect(f)) {
        p += 0.3;
    }
    let hp_max = b.unit(me).hp_max.max(1) as f32;
    p += (tm.hit_of(e) as f32 / hp_max).min(1.0);
    p
}

// ---------------------------------------------------------------------------
// Scoring.

/// Terrain defence of the cell `r` stands on.
fn defence_here(b: &Battle, r: UnitRef) -> i32 {
    b.terrain_under(r)[2] as i32
}

/// What standing on (x, y) at the end of the action costs `r` (in HP
/// points): the expected damage there next turn after the terrain's
/// defence, more when hurt, plus a penalty when it could be deadly.
fn cell_cost(b: &mut Battle, r: UnitRef, tm: &ThreatMap, x: i32, y: i32, def_now: i32) -> f32 {
    let (x0, y0) = (b.unit(r).x, b.unit(r).y);
    ai::set_pos(b, r, x, y);
    let def = defence_here(b, r);
    ai::set_pos(b, r, x0, y0);
    let u = b.unit(r);
    let hp = u.hp.max(1) as f32;
    let hurt = (u.hp as i32) * 2 < u.hp_max as i32;
    let raw = tm.at(x, y) as f32 * (100 - def).clamp(10, 190) as f32 / (100 - def_now).clamp(10, 190) as f32;
    // A leader (rank 1: its defeat usually wins the stage) keeps safer.
    let w = if hurt { 0.6 } else { 0.25 } * if u.rank == 1 { 2.5 } else { 1.0 };
    let mut c = 100.0 * raw.min(2.0 * hp) / hp * w;
    if raw >= hp {
        c += 35.0;
    }
    c - def as f32 * 0.3
}

/// The value of attack `du` on its defender (no position term).
fn attack_value(b: &mut Battle, r: UnitRef, du: &super::combat::Duel, prio: f32, goal: bool) -> f32 {
    let me = b.unit(r).clone();
    let e = b.unit(du.def).clone();
    let hit = du.calc.fc_hit.clamp(0, 100) as f32 / 100.0;
    let dmg = du.calc.fc_dmg.clamp(0, e.hp as i32) as f32;
    let ehp = (e.hp as f32).max(1.0);
    let emax = (e.hp_max as f32).max(1.0);
    let expected = hit * dmg;
    let pk = if du.calc.fc_dmg >= e.hp as i32 { hit } else { 0.0 };
    let mut v = prio * (60.0 * expected / ehp + 40.0 * expected / emax + 140.0 * pk);
    if du.counter.is_some() {
        let chit = du.calc.fc_chit.clamp(0, 100) as f32 / 100.0;
        let cdmg = du.calc.fc_cdmg.max(0) as f32;
        let mhp = (me.hp as f32).max(1.0);
        // Chipping at a unit whose death wins is worth more of a counter.
        let cw = if goal { 40.0 } else { 70.0 };
        v -= cw * (1.0 - pk) * chit * cdmg.min(mhp) / mhp;
        if cdmg >= mhp && chit >= 0.3 && pk < 0.95 {
            v -= 120.0 * chit * (1.0 - pk);
        }
    }
    v
}

/// The value of the area a spell plan covers.
fn spell_value(b: &mut Battle, r: UnitRef, tm: &ThreatMap, cast: &Cast, area: &[(i32, i32)], effect: u8, own_target: bool) -> f32 {
    let foe = r.side.other();
    let mut v = 0.0;
    for &(x, y) in area {
        if x < 0 {
            continue;
        }
        if let Some(e) = b.unit_at_cell(foe, x, y, true) {
            if own_target {
                continue;
            }
            let (ch, val, _) = b.spell_roll(cast, e, foe, false);
            let eu = b.unit(e).clone();
            let prio = priority(b, e, r, tm);
            let chance = ch.clamp(0, 100) as f32 / 100.0;
            if damage_effect(effect) {
                let dmg = val.clamp(0, eu.hp as i32) as f32;
                let expected = chance * dmg;
                let pk = if val >= eu.hp as i32 { chance } else { 0.0 };
                v += prio * (60.0 * expected / (eu.hp as f32).max(1.0) + 40.0 * expected / (eu.hp_max as f32).max(1.0) + 140.0 * pk);
            } else if !heal_effect(effect) {
                let (kind, _) = b.support_value(cast, foe, e);
                if kind > 0 {
                    v += 7.0 * prio * chance;
                }
            }
        } else if let Some(o) = b.unit_at_cell(r.side, x, y, false) {
            let (ch, val, _) = b.spell_roll(cast, o, r.side, false);
            let ou = b.unit(o).clone();
            let chance = ch.clamp(0, 100) as f32 / 100.0;
            let omax = (ou.hp_max as f32).max(1.0);
            if damage_effect(effect) {
                if o == r || Some(o) == cast.partner {
                    continue;
                }
                let dmg = val.clamp(0, ou.hp as i32) as f32;
                v -= 120.0 * chance * dmg / omax;
                if val >= ou.hp as i32 {
                    v -= 150.0;
                }
            } else if heal_effect(effect) {
                let need = (ou.hp_max - ou.hp).max(0) as f32;
                let heal = (chance * val.max(0) as f32).min(need);
                let urgent = if (ou.hp as i32) * 2 < ou.hp_max as i32 { 1.8 } else { 1.0 };
                v += 100.0 * heal / omax * urgent;
            } else if own_target {
                // Buffs on the own side (the original never casts them).
                let (kind, _) = b.support_value(cast, r.side, o);
                if kind > 0 {
                    let engaged = tm.at(ou.x, ou.y) > 0 || o == r;
                    v += if engaged { 5.0 } else { 1.5 } * chance;
                }
            }
        }
    }
    v
}

/// A planned action from node `node` (0 = stay).
#[derive(Debug, Clone, Copy)]
enum Act {
    Attack { target: UnitRef, skill: usize },
    Spell(SpellPlan),
}

#[derive(Debug, Clone, Copy)]
struct Plan {
    node: usize,
    act: Act,
    /// The action's own value (must reach `MIN_VALUE`).
    gain: f32,
    /// `gain` with the cell's risk: what ranks the plans.
    value: f32,
}

/// Rank of an action of value `gain` ending on a cell costing `pos`: the
/// cell only decides between worthwhile actions, mostly.
fn ranked(gain: f32, pos: f32) -> f32 {
    gain - POS_WEIGHT * pos
}

/// How much the cell's risk weighs against the action's value.
const POS_WEIGHT: f32 = 0.06;

/// The castable spells of `r` (cost, seals, partner), as FUN_0046b890 lists them.
fn castable(b: &Battle, r: UnitRef) -> Vec<(u16, bool, Option<UnitRef>)> {
    let mut cost = b.c(0xef);
    if b.unit(r).turn_flags & 0xc != 0 {
        cost += b.c(0xf3);
    }
    if (b.unit(r).ap as i32) < cost {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (spell, technique) in known_spells(b, r) {
        if !technique && b.unit(r).status & status::SEALED != 0 {
            continue;
        }
        if !b.spell_listed(r, spell, false, false) || b.spell_shortage(b.unit(r), spell) != Shortage::Ok {
            continue;
        }
        let aim = b.spell_aim(spell as i32);
        let mut partner = None;
        if aim.combo & 7 != 0 {
            partner = b.combo_partner(r, spell);
            if !partner.is_some_and(|p| b.spell_shortage(b.unit(p), spell) == Shortage::Ok) {
                continue;
            }
        }
        out.push((spell, technique, partner));
    }
    out
}

/// The best spell plan of `r` from where it stands and its value.
fn best_spell_here(b: &mut Battle, r: UnitRef, tm: &ThreatMap, limit: i32, spells: &[(u16, bool, Option<UnitRef>)]) -> Option<(SpellPlan, f32)> {
    let mut best: Option<(SpellPlan, f32)> = None;
    let facing = b.unit(r).facing;
    let saved_scroll = (b.map().scroll_x, b.map().scroll_y);
    let saved_cursor = b.cursor;
    for &(spell, technique, partner) in spells {
        let aim = b.spell_aim(spell as i32);
        let own_target = matches!(aim.target, 0 | 1) && !damage_effect(aim.effect) && !heal_effect(aim.effect) && !summon_effect(aim.effect);
        let mut cast = Cast::new(r.side, r, spell, (0, 0));
        cast.technique = technique;
        cast.partner = partner;
        let mut tg = b.spell_target(r.side, r, spell as i32, technique as u8, 0, false);
        b.target_range(&mut tg, aim.range);
        let mut cells: Vec<((i32, i32), i32)> = Vec::new();
        if own_target {
            // The original's scoring subtracts own units: try every cell.
            cells = tg.range.iter().map(|&c| (c, 0)).collect();
        } else {
            let mut ob = ai::Best::new(limit);
            let mut plan = SpellPlan::default();
            if b.ai_score_spell(&mut tg, &cast, &mut ob, &mut plan) {
                cells.push((plan.cell, plan.dir));
            }
        }
        for (cell, dir) in cells {
            let mut t2 = tg.clone();
            b.aim_ai_cast(&mut t2, cell.0, cell.1, dir);
            let area = t2.area.clone();
            t2.free_area();
            cast.cell = cell;
            let v = spell_value(b, r, tm, &cast, &area, aim.effect, own_target);
            if v > best.map_or(MIN_VALUE, |b| b.1) {
                best = Some((SpellPlan { spell, cell, dir }, v));
            }
        }
        tg.free();
    }
    b.unit_mut(r).facing = facing;
    b.cursor = saved_cursor;
    let m = b.map_mut();
    (m.scroll_x, m.scroll_y) = saved_scroll;
    best
}

/// The best action of `r` over its range (or None).
fn best_plan(b: &mut Battle, r: UnitRef, tm: &ThreatMap, limit: i32) -> Option<Plan> {
    let snap = b.rng.snapshot();
    let u = b.unit(r).clone();
    let (x0, y0, f0) = (u.x, u.y, u.facing);
    let def_now = defence_here(b, r);
    let mut atk_cost = b.c(0xee);
    if u.abilities & 0x2000_0000 != 0 {
        atk_cost -= 4;
    }
    if u.turn_flags & tf::ATTACKED != 0 && u.abilities & 0x8000 == 0 {
        atk_cost += b.c(0xf3);
    }
    let moved = u.turn_flags & tf::MOVED != 0;
    // (node, x, y, mark, AP left there): node 0 = stay.
    let mut cells: Vec<(usize, i32, i32, u8, i32)> = vec![(0, x0, y0, 2, u.ap as i32)];
    if !moved {
        for (i, n) in ai::stops(b) {
            if n.can_stop != 0 && n.mark != 0 && !b.no_stop_for(r, n.x as i32, n.y as i32) {
                cells.push((i + 1, n.x as i32, n.y as i32, n.mark, n.budget as i32 + b.c(0xe8)));
            }
        }
    }
    // An attack that leaves the AP for another one is worth more.
    let again = atk_cost + if u.abilities & 0x8000 == 0 { b.c(0xf3) } else { 0 };
    let second = move |ap: i32| ap - atk_cost >= again;
    let t = b.t();
    let skills: Vec<usize> = t.kinds.class(u.kind as u16).map_or(Vec::new(), |c| c.actions.clone().collect());
    let spells = castable(b, r);
    let mut best: Option<Plan> = None;
    let offer = |best: &mut Option<Plan>, p: Plan| {
        if p.gain >= MIN_VALUE && best.map_or(true, |b| p.value > b.value) {
            *best = Some(p);
        }
    };
    let mut spell_cells: Vec<(f32, usize, i32, i32)> = Vec::new();
    for &(node, x, y, mark, ap_there) in &cells {
        let pos = cell_cost(b, r, tm, x, y, def_now);
        ai::set_pos(b, r, x, y);
        if (node == 0 && (u.ap as i32) >= atk_cost) || (node > 0 && mark != 0) {
            for &k in &skills {
                let kind = b.action_kind(k);
                if kind == 0xb || kind == 0xc || !b.skill_usable(b.unit(r), k) {
                    continue;
                }
                let tl = b.target_list(r, k);
                for (ti, tgt) in tl.targets.iter().enumerate() {
                    b.unit_mut(r).facing = tgt.facing;
                    let du = b.duel(r, tgt.unit, k);
                    let prio = priority(b, tgt.unit, r, tm);
                    let goal = tm.goals.chars.contains(&b.unit(tgt.unit).char_id);
                    let mut v = attack_value(b, r, &du, prio, goal);
                    if tl.shape == 3 {
                        let (start, len) = tl.ray_of(ti);
                        for j in 0..len {
                            let (cx, cy) = tl.cells[start + j];
                            let Some(e2) = b.unit_at_cell(r.side.other(), cx, cy, false) else { continue };
                            if e2 == tgt.unit {
                                break;
                            }
                            let d2 = b.duel(r, e2, k);
                            let p2 = priority(b, e2, r, tm);
                            let g2 = tm.goals.chars.contains(&b.unit(e2).char_id);
                            v += attack_value(b, r, &d2, p2, g2);
                        }
                    }
                    let extra = if v > 0.0 && second(ap_there) { 0.4 * v } else { 0.0 };
                    offer(&mut best, Plan { node, act: Act::Attack { target: tgt.unit, skill: k }, gain: v, value: ranked(v + extra, pos) });
                }
            }
        }
        if !spells.is_empty() && (node == 0 || mark > 1) {
            let near = b.army(r.side.other()).units.iter().filter(|e| e.on_map()).map(|e| distance(x, y, e.x, e.y)).min().unwrap_or(99);
            spell_cells.push((near as f32 + pos * 0.05, node, x, y));
        }
    }
    // Spells from the own cell and the cells nearest the opponents.
    spell_cells.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let own = spell_cells.iter().position(|c| c.1 == 0);
    let mut tried: Vec<(usize, i32, i32)> = Vec::new();
    if let Some(i) = own {
        tried.push((spell_cells[i].1, spell_cells[i].2, spell_cells[i].3));
    }
    tried.extend(spell_cells.iter().filter(|c| c.1 != 0).take(SPELL_CELLS).map(|c| (c.1, c.2, c.3)));
    for (node, x, y) in tried {
        let pos = cell_cost(b, r, tm, x, y, def_now);
        ai::set_pos(b, r, x, y);
        if let Some((plan, v)) = best_spell_here(b, r, tm, limit, &spells) {
            offer(&mut best, Plan { node, act: Act::Spell(plan), gain: v, value: ranked(v, pos) });
        }
    }
    ai::set_pos(b, r, x0, y0);
    b.unit_mut(r).facing = f0;
    b.rng.restore(snap);
    best
}

// ---------------------------------------------------------------------------
// Movement without an action.

/// Allies of `r` (same army, on the map, able) within `d` of an opponent.
fn engaged_allies(b: &Battle, r: UnitRef, d: i32) -> usize {
    let foes: Vec<(i32, i32)> = b.army(r.side.other()).units.iter().filter(|e| e.on_map()).map(|e| (e.x, e.y)).collect();
    b.army(r.side)
        .units
        .iter()
        .enumerate()
        .filter(|&(i, u)| i != r.idx && u.on_map() && u.can_act())
        .filter(|(_, u)| foes.iter().any(|&(x, y)| distance(u.x, u.y, x, y) <= d))
        .count()
}

/// The cell `r` moves to when it has nothing worth doing (node index + 1,
/// or 0 to stay).
fn reposition_node(b: &mut Battle, r: UnitRef, tm: &ThreatMap, turn: i32) -> usize {
    let u = b.unit(r).clone();
    if u.turn_flags & tf::MOVED != 0 {
        return 0;
    }
    let def_now = defence_here(b, r);
    let foe = r.side.other();
    let targets: Vec<(UnitRef, i32, i32)> = (0..b.army(foe).units.len())
        .map(|i| UnitRef::new(foe, i))
        .filter(|&e| b.unit(e).on_map() && !b.is_hidden(foe, b.unit(e)))
        .map(|e| (e, b.unit(e).x, b.unit(e).y))
        .collect();
    if targets.is_empty() {
        return 0;
    }
    let healer = (0..b.army(r.side).units.len()).map(|i| UnitRef::new(r.side, i)).any(|a| {
        a != r && b.unit(a).on_map() && known_spells(b, a).iter().any(|&(s, _)| heal_effect(b.spell_aim(s as i32).effect))
    });
    let hurt = (u.hp as i32) * 100 < (u.hp_max as i32) * 35 && healer;
    let allies = b.army(r.side).units.iter().filter(|a| a.on_map() && a.can_act()).count();
    let ready = engaged_allies(b, r, 7) + 1 >= (allies / 3).max(2);
    // Wait for the group only early, while untouched and alone in front.
    // Wait for the group only early, while untouched and alone in front.
    let hold = !hurt && !ready && turn <= HOLD_TURNS && tm.at(u.x, u.y) == 0 && engaged_allies(b, r, 7) == 0;
    // The target to close in on: priority over distance.
    let goal = targets
        .iter()
        .map(|&(e, x, y)| (priority(b, e, r, tm) / (1.0 + distance(u.x, u.y, x, y) as f32 * 0.15), x, y))
        .fold(None::<(f32, i32, i32)>, |a, c| if a.map_or(true, |a| c.0 > a.0) { Some(c) } else { a })
        .map(|c| (c.1, c.2))
        .unwrap_or((targets[0].1, targets[0].2));
    let ally_cells: Vec<(i32, i32)> =
        b.army(r.side).units.iter().enumerate().filter(|&(i, a)| i != r.idx && a.on_map()).map(|(_, a)| (a.x, a.y)).collect();
    let mut cands: Vec<(usize, i32, i32)> = vec![(0, u.x, u.y)];
    for (i, n) in ai::stops(b) {
        if n.can_stop != 0 && !b.no_stop_for(r, n.x as i32, n.y as i32) {
            cands.push((i + 1, n.x as i32, n.y as i32));
        }
    }
    let mut best = (f32::MAX, 0usize);
    for (node, x, y) in cands {
        let risk = cell_cost(b, r, tm, x, y, def_now);
        let d_goal = distance(x, y, goal.0, goal.1) as f32;
        let d_ally = ally_cells.iter().map(|&(ax, ay)| distance(x, y, ax, ay)).min().unwrap_or(0) as f32;
        let score = if hurt {
            risk * 2.0 + d_ally * 1.5 - d_goal * 0.5
        } else if hold {
            // Wait at the edge of the threat: no risk, as close as that allows.
            let danger = if tm.at(x, y) > 0 { 1000.0 } else { 0.0 };
            danger + d_goal * 3.0 + d_ally * 0.5
        } else {
            risk + d_goal * 4.0 + d_ally * 0.3
        };
        if score < best.0 {
            best = (score, node);
        }
    }
    best.1
}

// ---------------------------------------------------------------------------
// The fight.

/// The enhanced FUN_0040aadc: act while something is worth doing (moving
/// at most once), then reposition and end the unit's turn.
pub fn fight(game: &mut Game, ctl: &mut Ctl) {
    let r = ctl.unit;
    for _ in 0..4 {
        if !game.battle.unit(r).on_map() {
            return;
        }
        ctl.situation = game.battle.assess(ctl);
        let mut tm = threat_map(&mut game.battle, r);
        tm.goals = game.events.stage.as_ref().map_or(Goals::default(), |st| Goals::from_codes(&st.lose_codes));
        let plan = best_plan(&mut game.battle, r, &tm, ctl.limit);
        let Some(plan) = plan else {
            // Go-to-point units (modes 6 / 7) keep their story move (FUN_0040a8fa).
            let m = game.battle.unit(r).ai_mode;
            if m == 6 || m == 7 {
                ai::approach(game, ctl);
                return;
            }
            let turn = game.campaign.turn as i32;
            let node = reposition_node(&mut game.battle, r, &tm, turn);
            if node > 0 {
                if ai::shown(game, r) {
                    ai::present_range(game, r);
                }
                let move5 = game.battle.unit(r).flags & flag::MOVE5 != 0;
                game.battle.range.build_path(node, move5);
                ai::walk_and_wake(game, r);
                game.battle.unit_mut(r).turn_flags |= tf::MOVED;
            }
            turn::ai_finish(game, r);
            return;
        };
        if ai::shown(game, r) {
            ai::present_range(game, r);
        }
        super::view::panel::ai_thinking(game, r);
        if plan.node > 0 {
            let move5 = game.battle.unit(r).flags & flag::MOVE5 != 0;
            game.battle.range.build_path(plan.node, move5);
            ai::walk_and_wake(game, r);
            game.battle.unit_mut(r).turn_flags |= tf::MOVED;
        }
        match plan.act {
            Act::Attack { target, skill } => {
                if !game.battle.unit(target).on_map() {
                    turn::ai_finish(game, r);
                    return;
                }
                game.battle.cursor = (game.battle.unit(target).x, game.battle.unit(target).y);
                ai::attack(game, r, target, skill);
            }
            Act::Spell(sp) => ai::cast(game, r, sp),
        }
        if !game.battle.unit(r).on_map() {
            return;
        }
        game.battle.range.reset();
        game.battle.build_range(r, 0);
        ai::show_range(game, r);
        actions::set_anim(game, r, 1);
    }
    turn::ai_finish(game, r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::spells::tests::stage1;

    fn first_enemy(b: &Battle) -> UnitRef {
        let i = b.army(Side::Enemy).units.iter().position(|u| u.on_map()).expect("an enemy on the map");
        UnitRef::new(Side::Enemy, i)
    }

    #[test]
    fn threat_map_covers_reach_and_nothing_far() {
        let Some(mut b) = stage1() else { return eprintln!("ai_plus tests: no game data, skipped") };
        let me = first_enemy(&b);
        let rng = b.rng.snapshot();
        let tm = threat_map(&mut b, me);
        assert_eq!(b.rng.snapshot(), rng, "the threat map draws no RNG");
        assert!(!tm.sources.is_empty(), "the party threatens the enemy");
        // Every source's own cell and its neighbours are threatened by it.
        for &(p, hit) in &tm.sources {
            let (x, y) = (b.unit(p).x, b.unit(p).y);
            assert!(tm.at(x, y) >= hit);
            for dir in 0..4 {
                let (nx, ny) = crate::battle::map::neighbour(x, y, dir);
                assert!(tm.at(nx, ny) >= hit, "neighbour ({nx}, {ny}) of ({x}, {y})");
            }
        }
        // A cell far from every source is safe.
        let far = (0..tm.rows)
            .flat_map(|y| (0..tm.width).map(move |x| (x, y)))
            .find(|&(x, y)| tm.sources.iter().all(|&(p, _)| distance(x, y, b.unit(p).x, b.unit(p).y) > 30));
        if let Some((x, y)) = far {
            assert_eq!(tm.at(x, y), 0);
        }
        // Adding a source only adds threat.
        let total: i64 = tm.dmg.iter().map(|&v| v as i64).sum();
        assert!(total > 0);
    }

    #[test]
    fn priority_prefers_leaders_and_healers() {
        let Some(mut b) = stage1() else { return eprintln!("ai_plus tests: no game data, skipped") };
        let me = first_enemy(&b);
        let tm = threat_map(&mut b, me);
        let party: Vec<UnitRef> = (0..b.army(Side::Player).units.len()).map(|i| UnitRef::new(Side::Player, i)).filter(|&p| b.unit(p).on_map()).collect();
        let vip = party.iter().copied().find(|&p| b.unit(p).is_vip()).expect("the protagonist is deployed");
        let pv = priority(&b, vip, me, &tm);
        for &p in &party {
            assert!(priority(&b, p, me, &tm) >= 1.0);
        }
        // The protagonist outranks an ordinary unit that threatens no more.
        if let Some(&o) = party.iter().find(|&&p| p != vip && !b.unit(p).is_vip() && tm.hit_of(p) <= tm.hit_of(vip)) {
            let heals = known_spells(&b, o).iter().any(|&(s, _)| heal_effect(b.spell_aim(s as i32).effect));
            if !heals {
                assert!(pv > priority(&b, o, me, &tm));
            }
        }
        // A healer outranks the same unit without its spells.
        if let Some(&h) = party.iter().find(|&&p| known_spells(&b, p).iter().any(|&(s, _)| heal_effect(b.spell_aim(s as i32).effect))) {
            let with = priority(&b, h, me, &tm);
            let learned = b.unit(h).learned;
            b.unit_mut(h).learned = [0; 11];
            let without = priority(&b, h, me, &tm);
            b.unit_mut(h).learned = learned;
            assert!(with > without);
        }
    }

    #[test]
    fn plans_consume_no_rng_and_value_kills() {
        let Some(mut b) = stage1() else { return eprintln!("ai_plus tests: no game data, skipped") };
        let me = first_enemy(&b);
        b.build_range(me, 0);
        let tm = threat_map(&mut b, me);
        let rng = b.rng.snapshot();
        let pos = (b.unit(me).x, b.unit(me).y);
        let _ = best_plan(&mut b, me, &tm, 0);
        assert_eq!(b.rng.snapshot(), rng);
        assert_eq!((b.unit(me).x, b.unit(me).y), pos, "the unit is put back");
    }
}
