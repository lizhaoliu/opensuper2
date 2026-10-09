//! Power rating, growth, level up and proficiency (docs/battle.md §6–7).

use super::army::UnitRef;
use super::rules;
use super::unit::{flag, Unit};
use super::Battle;

/// Cached partial ratings of the last `power` call (`DAT_0049fdf0`, `_f8`, `_f4`, `_fc`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PowerCache {
    pub p1: i32,
    pub p2: i32,
    pub p3: i32,
    pub p4: i32,
}

/// `DAT_004a2564[2]`: schools 1..=8 are magic, 9..=11 techniques (BIN 2 header).
pub const MAGIC_SCHOOLS: usize = 8;

thread_local! {
    static CACHE: std::cell::Cell<PowerCache> = const { std::cell::Cell::new(PowerCache { p1: 0, p2: 0, p3: 0, p4: 0 }) };
}

impl Battle {
    /// Port of FUN_0045781c (via `rules::Rating`): the power rating.
    /// `mode` 2 uses max stats, 1 `hp_before_hit`, 0 current values. `part`
    /// 1..=4 returns magic/tech/body/skills, 0 the total, 5..=8 the parts
    /// cached by the previous call (as the original's globals do).
    pub fn power(&self, u: &Unit, mode: i32, part: i32) -> i32 {
        if (5..=8).contains(&part) {
            let c = CACHE.with(|c| c.get());
            return match part {
                5 => c.p1,
                6 => c.p2,
                7 => c.p3,
                _ => c.p4,
            };
        }
        let Some(t) = self.tables.clone() else { return 1 };
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        let m = match mode {
            2 => rules::RatingMode::Max,
            1 => rules::RatingMode::BeforeHit,
            _ => rules::RatingMode::Current,
        };
        let r = rules::Rating::compute(&ctx, &super::combat::fighter_of(u), m);
        CACHE.with(|c| c.set(PowerCache { p1: r.magic, p2: r.tech, p3: r.body, p4: r.skills }));
        match part {
            1 => r.magic,
            2 => r.tech,
            3 => r.body,
            4 => r.skills,
            _ => r.total,
        }
    }

    /// Port of FUN_0042345b (via `rules::growth`): the nine growth rolls.
    pub fn roll_growth(&mut self, kind: i16, level: i32) -> [i32; 9] {
        let t = self.t();
        let ctx = rules::Ctx::new(&t, self.difficulty, self.vs);
        rules::growth(&ctx, &mut self.rng, kind as u16, level)
    }

    /// Port of FUN_00456c88: add growth (order HP, MP, ST, ATK, DEF, 反應, 戰技, MAG, 劍術).
    pub fn apply_growth(u: &mut Unit, g: [i32; 9]) {
        let mut g = g.map(|v| v as i16);
        if u.flags & flag::BONUS_GROWTH != 0 {
            for v in &mut g {
                *v += 1;
            }
        }
        let cap = |v: i16, m: i16| v.clamp(0, m);
        u.hp_max = cap(u.hp_max + g[0], 5000);
        u.hp = cap(u.hp + g[0], 5000);
        u.mp_max = cap(u.mp_max + g[1], 999);
        u.mp = cap(u.mp + g[1], 999);
        u.st_max = cap(u.st_max + g[2], 999);
        u.st = cap(u.st + g[2], 999);
        u.atk = cap(u.atk + g[3], 999);
        u.atk_cur = cap(u.atk_cur + g[3], 999);
        u.def = cap(u.def + g[4], 999);
        u.def_cur = cap(u.def_cur + g[4], 999);
        u.react = cap(u.react + g[5], 999);
        u.skill = cap(u.skill + g[6], 999);
        u.skill_cur = cap(u.skill_cur + g[6], 999);
        u.mag = cap(u.mag + g[7], 999);
        u.mag_cur = cap(u.mag_cur + g[7], 999);
        u.sword = cap(u.sword + g[8], 999);
    }

    /// Port of FUN_00453725: level up while EXP allows. Returns true if the
    /// unit still has a full bar afterwards (FUN_004537da shows the panels).
    pub fn check_level_up(&mut self, r: UnitRef) -> bool {
        let u = self.unit(r);
        if u.exp < u.exp_next || u.level >= 99 {
            return false;
        }
        let show = !(u.npc == 1 || u.flags & flag::DEAD != 0 || u.flags & flag::DEPLOYED == 0);
        self.level_up(r, show);
        let u = self.unit(r);
        u.exp >= u.exp_next
    }

    /// Port of FUN_004537da.
    pub fn level_up(&mut self, r: UnitRef, show: bool) {
        loop {
            let (kind, level) = {
                let u = self.unit_mut(r);
                u.level += 1;
                (u.kind, u.level as i32)
            };
            let g = self.roll_growth(kind, level);
            let u = self.unit_mut(r);
            u.exp = u.exp.wrapping_sub(u.exp_next);
            let old = [u.hp_max, u.mp_max, u.st_max, u.atk, u.def, u.react, u.skill, u.mag, u.sword];
            let b = (u.flags & flag::BONUS_GROWTH != 0) as i32;
            Battle::apply_growth(u, g);
            if show {
                // The level-up window (FUN_00456549) is shown by `actions::show_level_ups`.
                let gain = g.map(|v| (v + b) as i16);
                self.level_log.push(LevelUp { unit: r, level: level as i16, old, gain });
            }
            let u = self.unit_mut(r);
            if !(u.exp_next <= u.exp && u.level < 99) {
                break;
            }
        }
    }

    /// Port of FUN_00452da5: +1 weapon/other proficiency for using action `kind`.
    pub fn proficiency_gain(u: &mut Unit, kind: i32) {
        let idx = if !(1..=10).contains(&kind) {
            if kind == 0xb || kind == 0xc {
                return;
            }
            0x11
        } else {
            if !(2..=7).contains(&kind) {
                return;
            }
            (kind - 2) as usize
        };
        if u.prof[idx] < 0x5dc {
            u.prof[idx] += 1;
        }
    }
}

/// One level gained while the level-up window would show (`Battle::level_log`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUp {
    pub unit: UnitRef,
    pub level: i16,
    /// HP, MP, ST max, ATK, DEF, 反應, 戰技, 魔力, 劍術 before the level.
    pub old: [i16; 9],
    /// The gains in the same order (growth plus the bonus point).
    pub gain: [i16; 9],
}
