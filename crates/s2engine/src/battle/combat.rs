//! Physical combat: target lists, the duel forecast and rolls, criticals,
//! instant death, on-hit effects, EXP (docs/battle.md §5, §7).
//!
//! The formulas themselves are `battle::rules` (TABLES port); this file
//! builds their inputs from the live units (`Fighter` snapshots taken at the
//! moment of each call, as the original reads the unit structs), keeps the
//! duel bookkeeping, and ports the parts outside the RULES API (targeting,
//! counter selection, on-hit effects, HP application, rest).

use super::army::{Side, UnitRef};
use super::rules;
use super::map::Pass;
use super::movement::facing_toward;
use super::unit::{buff, flag, status, Unit};
use super::Battle;

/// A unit in an attack's range (`TargetList+0x3c`, 16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub unit: UnitRef,
    pub x: i32,
    pub y: i32,
    /// Facing of the attacker toward this target (FUN_004576f7).
    pub facing: u8,
}

/// Attack range and targets for one unit and action (FUN_00406a36).
#[derive(Debug, Clone)]
pub struct TargetList {
    pub unit: UnitRef,
    /// 2 = ring area, otherwise four rays (3 = piercing).
    pub shape: i32,
    pub cells: Vec<(i32, i32)>,
    pub ray_len: [usize; 4],
    pub ray_start: [usize; 4],
    pub targets: Vec<Target>,
}

impl TargetList {
    /// Port of FUN_004072f1: 1-based index of the target at (x, y), 0 if none.
    pub fn target_at(&self, x: i32, y: i32) -> usize {
        self.targets.iter().position(|t| t.x == x && t.y == y).map_or(0, |i| i + 1)
    }

    /// Port of FUN_00407382: (start, len) of the ray that contains target `t`.
    pub fn ray_of(&self, t: usize) -> (usize, usize) {
        let Some(tg) = self.targets.get(t) else { return (0, 0) };
        let c = self.cells.iter().position(|&(x, y)| x == tg.x && y == tg.y).unwrap_or(self.cells.len());
        for d in 0..4 {
            if self.ray_len[d] > 0 && self.ray_start[d] <= c && c < self.ray_start[d] + self.ray_len[d] {
                return (self.ray_start[d], self.ray_len[d]);
            }
        }
        (0, 0)
    }
}

/// Combat forecast and resolution context (`DAT_00492b08`, 0x4a bytes):
/// the participants and actions, with the computed fields in `calc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duel {
    pub att: UnitRef,
    pub def: UnitRef,
    /// +0x10 attacker action (index into `KindTable::actions`).
    pub skill: usize,
    /// +0x14 defender counter action (`None` = −1).
    pub counter: Option<usize>,
    /// +0x40 1 front, 2 side, 3 back.
    pub facing: u8,
    /// +0x18..+0x4a
    pub calc: rules::DuelCalc,
}

/// `battle::army::Side` → `rules::Side`.
pub fn rules_side(s: Side) -> rules::Side {
    match s {
        Side::Player => rules::Side::Player,
        Side::Enemy => rules::Side::Enemy,
    }
}

/// Stat snapshot of a unit (no map context).
pub fn fighter_of(u: &Unit) -> rules::Fighter {
    rules::Fighter {
        char_id: u.char_id,
        kind: u.kind as u16,
        rank: u.rank,
        flags: u.flags,
        buffs: u.buffs,
        status: u.status,
        abilities: u.abilities,
        level: u.level as i32,
        kills: u.kills as i32,
        hp_max: u.hp_max as i32,
        mp_max: u.mp_max as i32,
        st_max: u.st_max as i32,
        mag: u.mag as i32,
        ap_max: u.ap_max as i32,
        react: u.react as i32,
        atk: u.atk as i32,
        def: u.def as i32,
        sword: u.sword as i32,
        skill: u.skill as i32,
        hp: u.hp as i32,
        mp: u.mp as i32,
        st: u.st as i32,
        mag_cur: u.mag_cur as i32,
        hp_before_hit: u.hp_before_hit as i32,
        atk_cur: u.atk_cur as i32,
        def_cur: u.def_cur as i32,
        skill_cur: u.skill_cur as i32,
        alignment: u.charm as i32,
        equip: u.equip,
        prof: u.prof,
        learned: u.learned,
        height: 0,
        terrain: [1, 0, 0],
        move_type: 0,
    }
}

impl Battle {
    fn action(&self, skill: usize) -> Option<s2data::tables::ActionDef> {
        self.tables.as_ref()?.kinds.actions.get(skill).cloned()
    }

    /// Signed `ActionDef::kind` as the original reads it (`char`).
    pub fn action_kind(&self, skill: usize) -> i32 {
        self.action(skill).map_or(0, |a| a.kind as i8 as i32)
    }

    /// Port of FUN_0043b464: (min, max, shape) of an action for a unit.
    pub fn skill_range(&self, u: &Unit, skill: usize) -> (i32, i32, i32) {
        let Some(a) = self.action(skill) else { return (1, 1, 0) };
        let k = a.kind as i8 as i32;
        let weapon = if u.equip[0] != 0 { self.item(u.equip[0]) } else { None };
        let (mut min, mut max, shape);
        match (!(1..=10).contains(&k), weapon) {
            (false, Some(w)) => {
                let code = w.reach as i8 as i32;
                max = code % 10;
                min = if code == 1 { 1 } else if code < 10 { 2 } else { 1 };
                shape = if (w.shape as i32) < 1 { a.shape as i32 } else { w.shape as i32 };
            }
            _ => {
                min = a.range.0 as i32;
                max = a.range.1 as i32;
                shape = a.shape as i32;
            }
        }
        if min == 0 {
            max = 1;
            min = 1;
        }
        (min, max, shape)
    }

    /// Port of FUN_00406a36: range cells and visible enemy targets.
    pub fn target_list(&self, r: UnitRef, skill: usize) -> TargetList {
        let u = self.unit(r);
        let (min, mut max, shape) = self.skill_range(u, skill);
        if max < min {
            max = min;
        }
        let mut tl = TargetList { unit: r, shape, cells: Vec::new(), ray_len: [0; 4], ray_start: [0; 4], targets: Vec::new() };
        if shape == 2 {
            self.ring_cells(u.x, u.y, min, max, &mut tl.cells);
        } else {
            self.ray_cells(u.x, u.y, min, max, &mut tl);
        }
        for &(x, y) in &tl.cells {
            if let Some(e) = self.unit_at_cell(r.side.other(), x, y, true) {
                tl.targets.push(Target { unit: e, x, y, facing: facing_toward(u.x, u.y, x, y, u.facing) });
            }
        }
        tl
    }

    /// Port of FUN_00406d0c: the four rays.
    fn ray_cells(&self, x0: i32, y0: i32, min: i32, max: i32, tl: &mut TargetList) {
        let m = self.map();
        let h0 = m.height(x0, y0);
        for dir in 0..4 {
            let (mut x, mut y) = (x0, y0);
            tl.ray_len[dir] = 0;
            tl.ray_start[dir] = tl.cells.len();
            for s in 0..max {
                match dir {
                    0 => {
                        if y & 1 != 0 {
                            x += 1;
                        }
                        y -= 1;
                    }
                    1 => {
                        if y & 1 == 0 {
                            x -= 1;
                        }
                        y += 1;
                    }
                    2 => {
                        if y & 1 == 0 {
                            x -= 1;
                        }
                        y -= 1;
                    }
                    _ => {
                        if y & 1 != 0 {
                            x += 1;
                        }
                        y += 1;
                    }
                }
                if min - 1 <= s {
                    let h = m.height(x, y);
                    let height_ok = (h0 < h && h - h0 <= 2) || (h <= h0 && h0 - h <= 3);
                    if !m.targetable(x, y) || m.blocked_by_spot(x, y) || !height_ok {
                        break;
                    }
                    tl.cells.push((x, y));
                    tl.ray_len[dir] += 1;
                }
            }
        }
    }

    /// Port of FUN_00406f27: the ring area (distance min..=max).
    fn ring_cells(&self, x0: i32, y0: i32, min: i32, max: i32, out: &mut Vec<(i32, i32)>) {
        let m = self.map();
        let ok = |x: i32, y: i32| m.targetable(x, y) && !m.blocked_by_spot(x, y);
        let mut lx = x0;
        let mut cy = y0;
        let mut c = 0;
        loop {
            c += 1;
            if c > max {
                break;
            }
            let l = if cy & 1 == 0 { lx - 1 } else { lx };
            lx = l;
            let top = cy - 1;
            if min <= c {
                for x in l..=l + c {
                    if ok(x, top) {
                        out.push((x, top));
                    }
                    if ok(x, top + c * 2) {
                        out.push((x, top + c * 2));
                    }
                }
                let mut y = cy + 1;
                while y <= cy - 3 + c * 2 {
                    if ok(l, y) {
                        out.push((l, y));
                    }
                    if ok(l + c, y) {
                        out.push((l + c, y));
                    }
                    y += 2;
                }
            }
            cy = top;
        }
    }

    /// Port of FUN_0043ae12: the action is usable with the unit's weapon.
    pub fn skill_usable(&self, u: &Unit, skill: usize) -> bool {
        let k = self.action_kind(skill);
        let w = u.equip[0];
        if w == 0x53 {
            return true;
        }
        if k == 1 {
            return w == 0;
        }
        if (2..=7).contains(&k) {
            return w != 0 && self.item(w).is_some_and(|it| it.kind as i8 as i32 == k - 1);
        }
        k >= 0x15
    }

    /// Port of FUN_00442c1c: the action has a target from the unit's cell.
    pub fn can_attack_with(&self, r: UnitRef, skill: usize) -> bool {
        self.skill_usable(self.unit(r), skill) && !self.target_list(r, skill).targets.is_empty()
    }

    /// Terrain triple under a unit (FUN_00427df2); `[1, 0, 0]` where blocked.
    pub fn terrain_under(&self, r: UnitRef) -> [i8; 3] {
        let u = self.unit(r);
        let t = self.t();
        let mt = self.move_type(u);
        let (p, e) = self.map().terrain(&t, u.x, u.y, u.kind, mt);
        if p == Pass::Blocked {
            [1, 0, 0]
        } else {
            [e.cost, e.attack, e.defense]
        }
    }

    /// Snapshot of a unit for `battle::rules`, with its cell height, terrain and move type.
    pub fn fighter(&self, r: UnitRef) -> rules::Fighter {
        let u = self.unit(r);
        let (height, terrain) = match self.map.as_ref() {
            Some(m) => (m.height(u.x, u.y), self.terrain_under(r)),
            None => (0, [1, 0, 0]),
        };
        let mut f = fighter_of(u);
        f.height = height;
        f.terrain = terrain;
        f.move_type = self.move_type(u);
        f
    }

    /// Port of FUN_00408078 (= FUN_00409488 + FUN_00409642): set up a duel
    /// and its forecast. Picks the defender's counter action at random.
    pub fn duel(&mut self, att: UnitRef, def: UnitRef, skill: usize) -> Duel {
        let mut d = Duel { att, def, skill, counter: None, facing: 1, calc: rules::DuelCalc::default() };
        self.pick_counter(&mut d);
        self.forecast(&mut d);
        d
    }

    /// Port of FUN_00409488: random usable counter action, and the facing relation.
    fn pick_counter(&mut self, d: &mut Duel) {
        d.counter = None;
        let def = self.unit(d.def).clone();
        if def.can_counter() {
            let t = self.t();
            let mut cands = Vec::new();
            if let Some(c) = t.kinds.class(def.kind as u16) {
                for k in c.actions.clone() {
                    let kind = self.action_kind(k);
                    if kind != 0xb && kind != 0xc && self.skill_usable(&def, k) {
                        let tl = self.target_list(d.def, k);
                        let a = self.unit(d.att);
                        if tl.target_at(a.x, a.y) != 0 {
                            cands.push(k);
                        }
                    }
                }
            }
            if !cands.is_empty() {
                let i = self.rng.rnd(cands.len() as i32);
                d.counter = Some(cands[i as usize]);
            }
        }
        let def = self.unit(d.def);
        d.facing = if def.abilities & 0x2000 == 0 {
            let a = self.unit(d.att);
            super::movement::facing_relation(a.x, a.y, def.x, def.y, def.facing, self.difficulty == 1) as u8
        } else {
            1
        };
    }

    /// Run `f` with the rules context and fresh snapshots of both duelists.
    fn with_duel<T>(&mut self, d: &Duel, f: impl FnOnce(&rules::Ctx, &mut super::rng::Rng, &rules::Duel) -> T) -> T {
        let t = self.t();
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        let (fa, fd) = (self.fighter(d.att), self.fighter(d.def));
        let rd = rules::Duel {
            att: &fa,
            def: &fd,
            att_side: rules_side(d.att.side),
            def_side: rules_side(d.def.side),
            action: d.skill,
            counter: d.counter,
            facing: d.facing,
            def_army_flags: self.army(d.def.side).flags,
        };
        f(&ctx, &mut self.rng, &rd)
    }

    /// Port of FUN_00409642 (via `rules::forecast`).
    pub fn forecast(&mut self, d: &mut Duel) {
        d.calc = self.with_duel(d, |ctx, _, rd| rules::forecast(ctx, rd));
    }

    /// Port of FUN_0040881a (via `rules::strike`): the attacker's roll (0 = miss).
    pub fn roll_attack(&mut self, d: &mut Duel) -> i32 {
        let mut calc = d.calc;
        let v = self.with_duel(d, |ctx, rng, rd| rules::strike(ctx, rng, rd, &mut calc));
        d.calc = calc;
        v
    }

    /// Port of FUN_00408bae (via `rules::counter_strike`).
    pub fn roll_counter(&mut self, d: &Duel) -> i32 {
        let calc = d.calc;
        self.with_duel(d, |ctx, rng, rd| rules::counter_strike(ctx, rng, rd, &calc))
    }

    /// Port of FUN_004091ab.
    pub fn roll_crit(&mut self, d: &Duel) -> bool {
        let calc = d.calc;
        self.with_duel(d, |ctx, rng, rd| rules::roll_crit(ctx, rng, rd, &calc, false))
    }

    /// Port of FUN_00409213.
    pub fn roll_crit_counter(&mut self, d: &Duel) -> bool {
        let calc = d.calc;
        self.with_duel(d, |ctx, rng, rd| rules::roll_crit(ctx, rng, rd, &calc, true))
    }

    /// Port of FUN_00409094.
    pub fn roll_instant_death(&mut self, d: &Duel) -> bool {
        let calc = d.calc;
        self.with_duel(d, |ctx, rng, rd| rules::instant_death(ctx, rng, rd, &calc))
    }

    /// Port of FUN_004083c4 (AI evaluation): the attack would inflict a status.
    pub fn status_proc_check(&mut self, x: &Unit, y: &Unit, skill: usize) -> bool {
        if y.status & status::BARRIER != 0 || y.abilities & 0x4000 != 0 {
            return false;
        }
        let kind = self.action_kind(skill);
        if kind < 0xb {
            let mut c = 0;
            if ((self.has_item_ability(x, 3) || y.abilities & 0x80000 != 0) && y.abilities & 0x1000 == 0 && y.rank != 1)
                || (self.has_item_ability(x, 7) && y.rank != 1)
                || (x.abilities & 0x100000 != 0 && y.rank & 3 == 0)
            {
                c = 0x1e;
            }
            if x.abilities & 0x800000 != 0 {
                return true;
            }
            if self.has_item_ability(y, 0xe) {
                c /= 2;
            }
            if c != 0 && 0x1e < c && self.rng.pct() > 0x32 {
                return true;
            }
        }
        let Some(a) = self.action(skill) else { return false };
        let (eff, chance, _) = a.status;
        if eff == 0 {
            return false;
        }
        let mut c = chance as i32;
        if self.has_item_ability(y, 0xe) {
            c /= 2;
        }
        if y.rank & 3 != 0 && eff == 8 {
            c = (c * 7) / 10;
        }
        if 0x1d < c && self.rng.pct() > 0x31 {
            return match eff {
                1 => y.abilities & 0x1000 == 0 && y.buffs & buff::POISON == 0 && y.rank != 1,
                2 => y.buffs & buff::PETRIFIED == 0 && y.status & status::HELPLESS == 0 && y.rank != 1,
                3 => y.buffs & buff::PETRIFIED == 0 && y.rank & 3 == 0,
                4 => y.buffs & buff::PETRIFIED == 0 && y.status & status::INCAPACITATED == 0 && y.rank != 1,
                5 => {
                    y.buffs & 0x202 == 0
                        && y.status & status::INCAPACITATED == 0
                        && self.power(y, 2, 1) > self.power(y, 2, 3) * 2
                }
                6 => y.rank != 1 && y.status & status::CURSE == 0,
                7 => true,
                8 => y.buffs & buff::PETRIFIED == 0 && y.status & 0x22000 == 0,
                9 => {
                    let t = self.t();
                    t.kinds.class(y.kind as u16).is_some_and(|c| c.unit_type == 0xe) && y.rank != 1
                }
                10 => y.rank & 3 == 0,
                11 => y.status & status::WEAK == 0,
                _ => false,
            };
        }
        false
    }

    /// Port of FUN_004082e4: AI value of the attack (`(hit [+10]) * dmg`).
    pub fn duel_expected(&mut self, d: &Duel) -> i32 {
        let (a, df) = (self.unit(d.att).clone(), self.unit(d.def).clone());
        if self.status_proc_check(&a, &df, d.skill) {
            (d.calc.fc_hit + 10) * d.calc.fc_dmg
        } else {
            d.calc.fc_hit * d.calc.fc_dmg
        }
    }

    /// Port of FUN_00408354: AI value of the counter.
    pub fn duel_expected_counter(&mut self, d: &Duel) -> i32 {
        let (a, df) = (self.unit(d.att).clone(), self.unit(d.def).clone());
        let hit = match d.counter {
            Some(c) if self.status_proc_check(&df, &a, c) => d.calc.fc_chit + 10,
            _ => d.calc.fc_chit,
        };
        hit * d.calc.fc_cdmg
    }

    /// Port of FUN_00408fc9 (via `rules::exp_gain`): EXP for `x` dealing `dmg` to `y`.
    pub fn exp_gain(&self, x: UnitRef, y: UnitRef, dmg: i32) -> i32 {
        let t = self.t();
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        rules::exp_gain(&ctx, &self.fighter(x), &self.fighter(y), dmg)
    }

    /// Port of FUN_00453608: bounty EXP and item drops when `victim` died.
    pub fn kill_bonus(&self, killer: UnitRef, victim: UnitRef, exp: &mut i32, items: &mut [u8]) {
        let v = self.unit(victim);
        if v.flags & flag::DEAD == 0 {
            return;
        }
        *exp += v.bounty as i32;
        let mut add = |id: u16| {
            if let Some(c) = (id as usize).checked_sub(1).and_then(|i| items.get_mut(i)) {
                *c = c.wrapping_add(1);
            }
        };
        if self.vs {
            for &id in &v.equip {
                if id != 0 {
                    add(id);
                }
            }
        } else if self.army(killer.side).side != Side::Player || self.unit(killer).npc != 1 {
            for &id in &v.drops {
                if id != 0 {
                    add(id);
                }
            }
        }
    }

    /// Port of FUN_00452e38 (state part): award EXP and items to `r`
    /// (scaling via `rules::award_exp`). Returns the EXP added.
    pub fn award_exp(&mut self, r: UnitRef, exp: i32, items: &[u8]) -> i32 {
        let t = self.t();
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        let e = rules::award_exp(&ctx, exp, self.unit(r).abilities, rules_side(self.army(r.side).side));
        if e != 0 {
            let u = self.unit_mut(r);
            let v = u.exp as i32 + e;
            u.exp = if v > 999 { 999 } else { v as u16 };
        }
        for (i, &n) in items.iter().enumerate() {
            if n != 0 {
                self.army_mut(r.side).add_item(i + 1, n);
            }
        }
        e
    }

    /// Port of FUN_00438c92: status effects inflicted by damaging hits.
    /// `dmg` entries may be negated (drain).
    pub fn apply_on_hit(&mut self, att: UnitRef, targets: &[UnitRef], dmg: &mut [i32], skill: usize) {
        let kind = self.action_kind(skill);
        let (eff, chance0, dur) = self.action(skill).map_or((0, 0, 0), |a| a.status);
        for (i, &tr) in targets.iter().enumerate() {
            if dmg[i] <= 0 || self.unit(tr).flags & flag::DEAD != 0 {
                continue;
            }
            {
                let t = self.unit_mut(tr);
                if t.status & status::SLEEP != 0 {
                    t.status &= !status::SLEEP;
                }
            }
            if self.unit(tr).status & status::PARALYSIS != 0 && self.rng.pct() < 0x47 {
                self.unit_mut(tr).status &= !status::PARALYSIS;
            }
            let tu = self.unit(tr).clone();
            if tu.status & status::BARRIER != 0 || tu.abilities & 0x4000 != 0 {
                continue;
            }
            let a = self.unit(att).clone();
            let resist = |b: &Battle| if b.has_item_ability(&tu, 0xe) { 0xf } else { 0x1e };
            if kind < 0xb
                && (self.has_item_ability(&a, 3) || a.abilities & 0x80000 != 0)
                && tu.abilities & 0x1000 == 0
                && tu.rank != 1
            {
                let c = resist(self);
                if self.rng.pct() <= c {
                    self.unit_mut(tr).apply_poison(3);
                }
            }
            if kind < 0xb && self.has_item_ability(&a, 7) && tu.rank != 1 {
                let c = resist(self);
                if self.rng.pct() <= c {
                    self.unit_mut(tr).apply_status(3, status::PARALYSIS);
                }
            }
            if kind < 0xb && a.abilities & 0x100000 != 0 && tu.rank & 3 == 0 {
                let c = resist(self);
                if self.rng.pct() <= c {
                    self.unit_mut(tr).apply_status(3, status::SEALED);
                }
            }
            if kind < 0xb && a.abilities & 0x800000 != 0 {
                dmg[i] = -dmg[i];
            }
            if a.abilities & 0x1000_0000 != 0 {
                let t = self.unit(tr).clone();
                let u = self.unit_mut(att);
                if 4 < t.mp {
                    u.mp += ((t.mp as i32 * 0x19) / 100) as i16;
                    if u.mp_max < u.mp {
                        u.mp = u.mp_max;
                    }
                }
                if 4 < t.st {
                    u.st += ((t.st as i32 * 0x19) / 100) as i16;
                    if u.st_max < u.st {
                        u.st = u.st_max;
                    }
                }
            }
            let mut c = chance0 as i32;
            let tu = self.unit(tr).clone();
            if self.has_item_ability(&tu, 0xe) {
                c /= 2;
            }
            if tu.rank & 3 != 0 && eff == 8 {
                c = (c * 7) / 10;
            }
            if self.rng.pct() <= c {
                let d = dur as i32;
                match eff {
                    1 => {
                        if tu.abilities & 0x1000 == 0 && tu.rank != 1 {
                            self.unit_mut(tr).apply_poison(d);
                        }
                    }
                    2 => {
                        if tu.rank != 1 {
                            self.unit_mut(tr).apply_status(d, status::PARALYSIS);
                        }
                    }
                    3 => {
                        // FUN_00440264 (redraw it as stone): actions::apply_on_hit.
                        if tu.rank & 3 == 0 {
                            self.unit_mut(tr).apply_petrify(d);
                        }
                    }
                    4 => {
                        if tu.rank & 3 == 0 {
                            self.unit_mut(tr).apply_status(d, status::BLIND);
                        }
                    }
                    5 => {
                        let half = tu.atk_cur / 2;
                        self.unit_mut(tr).apply_berserk(half, d);
                    }
                    6 => {
                        if tu.rank != 1 {
                            self.unit_mut(tr).apply_status(d, status::CURSE);
                        }
                    }
                    7 => {
                        if dmg[i] > 0 {
                            dmg[i] = -dmg[i];
                        }
                    }
                    8 => {
                        self.unit_mut(tr).apply_status(d, status::FROZEN);
                    }
                    9 => {
                        let t = self.t();
                        if t.kinds.class(tu.kind as u16).is_some_and(|c| c.unit_type == 0xe) && tu.rank != 1 {
                            dmg[i] = tu.hp_max as i32;
                        }
                    }
                    10 => {
                        if tu.rank & 3 == 0 {
                            dmg[i] = tu.hp as i32;
                        }
                    }
                    11 => {
                        self.unit_mut(tr).apply_status(d, status::WEAK);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Logic of FUN_00439e2a: apply `dmg` (negative = drain: the attacker
    /// heals what the target loses). `mode` 1 damages `def`, 0 damages `att`
    /// (a counter), 2 heals `def`. Returns the amount applied.
    pub fn apply_hp(&mut self, att: UnitRef, def: UnitRef, dmg: i32, mode: i32) -> i32 {
        let (drain, mut v) = if dmg < 0 { (true, -dmg) } else { (false, dmg) };
        {
            let t = self.unit_mut(def);
            if t.hp != 0 {
                t.hp_before_hit = t.hp;
            }
        }
        match mode {
            1 => {
                v = v.min(self.unit(def).hp as i32);
                self.unit_mut(def).hp -= v as i16;
                if drain {
                    let a = self.unit_mut(att);
                    if a.hp < a.hp_max {
                        a.hp += v as i16;
                        if a.hp_max < a.hp {
                            a.hp = a.hp_max;
                        }
                    }
                }
            }
            0 => {
                v = v.min(self.unit(att).hp as i32);
                self.unit_mut(att).hp -= v as i16;
            }
            _ => {
                let t = self.unit_mut(def);
                t.hp += v as i16;
                if t.hp_max < t.hp {
                    t.hp = t.hp_max;
                }
            }
        }
        // The 1, 2, 4, 8.. chunk count with its popup is actions::show_hp.
        v
    }

    /// Port of FUN_0044e460: rest heal (10–20% of HP, MP, ST).
    pub fn rest_heal(&mut self, r: UnitRef) -> (i32, i32, i32) {
        if self.unit(r).status & status::CURSE != 0 {
            return (0, 0, 0);
        }
        let p = self.rng.rnd(0xb) + 10;
        let mp_double = self.has_item_ability(self.unit(r), 0x3f);
        let u = self.unit_mut(r);
        let hp = if u.hp < u.hp_max {
            let mut v = ((u.hp_max as i32 * p) / 100).max(5);
            v = v.min(u.hp_max as i32 - u.hp as i32);
            v = v.min(300);
            u.hp += v as i16;
            v
        } else {
            0
        };
        let mp = if u.mp_max < 1 || u.mp_max <= u.mp {
            0
        } else {
            let mut v = (u.mp_max as i32 * p) / 100;
            if mp_double {
                v <<= 1;
            }
            v = v.max(5).min(u.mp_max as i32 - u.mp as i32).min(100);
            u.mp += v as i16;
            v
        };
        let st = if u.st_max < 1 || u.st_max <= u.st {
            0
        } else {
            let v = ((u.st_max as i32 * p) / 100).max(5).min(u.st_max as i32 - u.st as i32).min(100);
            u.st += v as i16;
            v
        };
        (hp, mp, st)
    }
}
