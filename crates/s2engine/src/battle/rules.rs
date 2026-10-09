//! Battle formulas, ported from the original exe with its exact integer
//! arithmetic and RNG call order. Owned by the RULES port; docs/rules.md and
//! docs/battle.md describe the same formulas in prose.
//!
//! RULES API (stable; inputs are plain snapshots, not the runtime structs)
//! ----------------------------------------------------------------------
//! Context and inputs
//!   `Ctx { t: &RuleTables, difficulty, vs_mode }`   (`DAT_00494684`, `DAT_00494668`)
//!   `Fighter`      stat snapshot of one unit (unit-struct offsets in field docs)
//!   `Side`         Player / Enemy: which army acts (difficulty scaling, EXP)
//!   `Roll<'r>`     `Option<&mut Rng>`: `None` = expected value (forecast/AI), `Some` = random
//! Small helpers
//!   `has_item_ability(ctx, f, a)`           FUN_0043ab32
//!   `vary(ctx, roll, v, pair, side)`        FUN_004078df/797f/7a0a/7aaa/7b37/7bd7/7c64/7d04
//!   `desperation(rng, v, lost)`             FUN_00409e98
//!   `hit_mods(ctx, x, y, action, h)`        FUN_00409f0b
//!   `random_divisor(rng)`                   FUN_0040a2ed
//!   `damage_mods(ctx, v, dir, x, y, action)` FUN_0040a080
//!   `rating(ctx, f, mode, part)` / `Rating::compute`   FUN_0045781c (強度)
//! Physical combat (Duel = attacker, defender, actions, facing)
//!   `forecast(ctx, &duel) -> DuelCalc`      FUN_00409642 (no RNG)
//!   `strike(ctx, rng, &duel, &mut calc)`    FUN_0040881a (sets calc.att_acc/def_acc)
//!   `counter_strike(ctx, rng, &duel, &calc)` FUN_00408bae
//!   `crit_chance(ctx, &duel, &calc, counter)` / `roll_crit`   FUN_0040927b, FUN_004091ab/FUN_00409213
//!   `instant_death(ctx, rng, &duel, &calc)` FUN_00409094
//! EXP, growth, proficiency
//!   `exp_gain(ctx, att, victim, dmg)`       FUN_00408fc9
//!   `award_exp(ctx, e, abilities, side)`    FUN_00452e38 (scaling part) + FUN_00407880
//!   `growth(ctx, rng, class, level)`        FUN_0042345b / FUN_00423607 (sqrt, HP rolled twice)
//!   `apply_growth(&mut GrowStats, gains, bonus)`   FUN_00456c88
//!   `level_up(ctx, rng, class, &mut LevelState, &mut GrowStats, bonus)`   FUN_004537da
//!   `proficiency_slot(ctx, action)` / `gain_proficiency(&mut prof, slot)`   FUN_00452da5
//!   `ai_proficiency_floor(ctx, class, level)`  FUN_00452af0
//!   `can_promote(level, exp, exp_next, has_targets, vs)`   FUN_0043a522
//! Spells (SpellCast = caster, partner, spell id, sides, facing)
//!   `spell_ap_cost(ctx, already_used)`      FUN_0045f78b (AP part)
//!   `spell_resource_cost(cast)`             FUN_0045f78b (MP/ST/HP part)
//!   `spell_chance(ctx, rng, random, &cast, &target, same_army, &mut slot)`  FUN_00467fe9 (+FUN_004683f2, FUN_00468822)
//!   `spell_power(ctx, &cast, &target)`      FUN_0046932f
//!   `spell_value(ctx, roll, &cast, &target)` FUN_00468b0d (+FUN_00468d53)
//!   `spell_duration(ctx, &cast, &target)`   FUN_0046ae25 (+1 for the enemy side)
//! Terrain
//!   `terrain_for(difficulty, t)`            terrain triple honouring difficulty 1
//!
//! Division by zero (which crashed the original) yields 0 here.

use s2data::tables::{ActionDef, ItemDef, RuleTables, SpellDef};

use super::rng::Rng;

/// `Option<&mut Rng>`: `None` reproduces the original's "expected value" mode.
pub type Roll<'r> = Option<&'r mut Rng>;

/// `Some(rng)` when `random`, else `None`.
pub fn roll(rng: &mut Rng, random: bool) -> Roll<'_> {
    if random { Some(rng) } else { None }
}

#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub t: &'a RuleTables,
    /// `DAT_00494684`: 1..=5, 1 easiest (also disables terrain and height in combat).
    pub difficulty: u8,
    /// `DAT_00494668`: two-player mode.
    pub vs_mode: bool,
}

impl<'a> Ctx<'a> {
    pub fn new(t: &'a RuleTables, difficulty: u8, vs_mode: bool) -> Ctx<'a> {
        Ctx { t, difficulty, vs_mode }
    }

    /// Rules constant `P[i]` (`DAT_0049456c`).
    pub fn p(&self, i: usize) -> i32 {
        self.t.params.get(i)
    }

    fn simple(&self) -> bool {
        self.difficulty == 1
    }
}

/// Which army a value belongs to (`army == &DAT_0049f9f8` tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Player,
    Enemy,
}

/// Stat snapshot of a unit, named after docs/structs.md. Offsets are unit-struct offsets.
#[derive(Debug, Clone, Default)]
pub struct Fighter {
    /// +0x02 character id (1 and 0xb are the protagonists).
    pub char_id: i16,
    /// +0x04 class id (1-based).
    pub kind: u16,
    /// +0x2f 1 boss, 2 sub-boss.
    pub rank: u8,
    /// +0x30 flags (0x20000000 dead, 1 = bonus growth).
    pub flags: u32,
    /// +0x38 buffs, +0x3c statuses, +0x115 abilities (隱技 bits).
    pub buffs: u32,
    pub status: u32,
    pub abilities: u32,
    pub level: i32,
    /// +0x1f kills.
    pub kills: i32,
    /// Maxima/base block: +0xb7 HP, +0xb9 MP, +0xbb ST, +0xbd 魔力, +0xbf AP,
    /// +0xc1 反應, +0xc3 攻擊, +0xc5 防禦, +0xc7 劍術, +0xc9 戰技.
    pub hp_max: i32,
    pub mp_max: i32,
    pub st_max: i32,
    pub mag: i32,
    pub ap_max: i32,
    pub react: i32,
    pub atk: i32,
    pub def: i32,
    pub sword: i32,
    pub skill: i32,
    /// Current block: +0x124 HP, +0x126 MP, +0x128 ST, +0x12a 魔力, +0x132 HP
    /// before the last hit, +0x134 攻擊, +0x136 防禦, +0x138 戰技.
    pub hp: i32,
    pub mp: i32,
    pub st: i32,
    pub mag_cur: i32,
    pub hp_before_hit: i32,
    pub atk_cur: i32,
    pub def_cur: i32,
    pub skill_cur: i32,
    /// +0x123 alignment (BATTLE's docs call it "charm").
    pub alignment: i32,
    /// +0xcb equipped item ids, slot 0 = weapon.
    pub equip: [u16; 6],
    /// +0xd7 proficiencies: [0..6) weapon kinds, [6..17) school EXP, [17] specials.
    pub prof: [u16; 18],
    /// +0xfb learned-spell masks per school.
    pub learned: [u16; 11],
    /// Battle context: cell height (`FUN_004255da`), terrain triple of the
    /// unit's cell (`FUN_00427e1f`: cost, atk, def), move type (`FUN_0043aa26`).
    pub height: i32,
    pub terrain: [i8; 3],
    pub move_type: i32,
}

fn cdiv(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

fn item<'t>(ctx: &Ctx<'t>, id: u16) -> Option<&'t ItemDef> {
    (id as usize).checked_sub(1).and_then(|i| ctx.t.items.get(i))
}

fn action<'t>(ctx: &Ctx<'t>, idx: usize) -> &'t ActionDef {
    &ctx.t.kinds.actions[idx]
}

fn spell<'t>(ctx: &Ctx<'t>, id: u16) -> &'t SpellDef {
    &ctx.t.spells.spells[id as usize - 1]
}

/// Port of FUN_0043ab32: an equipped passive item (usage 0) with ability `a`.
pub fn has_item_ability(ctx: &Ctx, f: &Fighter, a: u8) -> bool {
    f.equip.iter().any(|&id| id != 0 && item(ctx, id).is_some_and(|it| it.usage == 0 && it.effect == a))
}

/// Unit type (兵種, 1-based) of a class.
pub fn unit_type(ctx: &Ctx, kind: u16) -> u8 {
    ctx.t.kinds.class(kind).map_or(0, |c| c.unit_type)
}

// ------------------------------------------------------------ variance

/// The nine `(base, spread)` pairs at P[0..0x12].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pair {
    /// 0/1 physical damage (FUN_0040797f)
    PhysDamage,
    /// 2/3 physical hit (FUN_004078df)
    PhysHit,
    /// 4/5 spell damage (FUN_00407aaa)
    SpellDamage,
    /// 6/7 technique hit (FUN_00407a0a)
    TechHit,
    /// 8/9 support amount (FUN_00407bd7)
    Support,
    /// 0xa/0xb magic hit (FUN_00407b37)
    MagicHit,
    /// 0xc/0xd heal amount (FUN_00407d04)
    Heal,
    /// 0xe/0xf (FUN_00407c64, chance)
    Pair7,
}

impl Pair {
    fn base(self) -> usize {
        match self {
            Pair::PhysDamage => 0,
            Pair::PhysHit => 2,
            Pair::SpellDamage => 4,
            Pair::TechHit => 6,
            Pair::Support => 8,
            Pair::MagicHit => 10,
            Pair::Heal => 12,
            Pair::Pair7 => 14,
        }
    }
    /// Chance pairs clamp to 1..=99, amount pairs only to >= 1.
    fn is_chance(self) -> bool {
        matches!(self, Pair::PhysHit | Pair::TechHit | Pair::MagicHit | Pair::Pair7)
    }
}

/// Port of FUN_00407d91: enemy-side scaling by difficulty (never in VS mode).
pub fn difficulty_scale(ctx: &Ctx, v: i32, side: Side) -> i32 {
    if !ctx.vs_mode && side != Side::Player {
        v * (ctx.p(0xf3 + ctx.difficulty as usize) + 100) / 100
    } else {
        v
    }
}

/// Port of the eight variance helpers (FUN_004078df .. FUN_00407d04):
/// `v * (P[2k] + rand(P[2k+1] + 3)) / 10`, difficulty scaling, then clamp.
pub fn vary(ctx: &Ctx, roll: Roll, v: i32, pair: Pair, side: Side) -> i32 {
    let k = pair.base();
    let m = match roll {
        None => ctx.p(k) + (ctx.p(k + 1) + 3) / 2,
        Some(rng) => {
            let b = ctx.p(k);
            b + rng.rnd(ctx.p(k + 1) + 3)
        }
    };
    let v = difficulty_scale(ctx, v * m / 10, side);
    if pair.is_chance() { v.clamp(1, 99) } else { v.max(1) }
}

// ------------------------------------------------------------ physical

/// Port of FUN_00409e98: 戰技 boost/slump at low HP (`lost` = % of max HP missing).
pub fn desperation(rng: &mut Rng, v: i32, lost: i32) -> i32 {
    if !(71..=90).contains(&lost) {
        if lost > 90 {
            let r = rng.pct();
            if r < 95 { v + (v >> 1) } else { v >> 1 }
        } else {
            v
        }
    } else {
        let r = rng.pct();
        (if r < 90 { v * 13 } else { v * 3 }) / 10
    }
}

/// Port of FUN_00409f0b: weapon/action hit bonus, defender evasion items, clamp 1..=100.
pub fn hit_mods(ctx: &Ctx, x: &Fighter, y: &Fighter, act: usize, mut h: i32) -> i32 {
    let a = action(ctx, act);
    let a_hit = a.hit as i8 as i32;
    if (a.kind as i8) < 0x15 {
        let w = if x.equip[0] == 0 { 0x5a } else { item(ctx, x.equip[0]).map_or(0, |it| it.hit as i8 as i32) };
        h += w * a_hit / 100;
    } else {
        h += a_hit;
    }
    for &id in &y.equip {
        if id != 0 {
            h -= item(ctx, id).map_or(0, |it| it.evasion as i32);
        }
    }
    if y.abilities & 2 != 0 {
        h -= 30;
    }
    if y.status & 4 != 0 || has_item_ability(ctx, y, 0x10) {
        h -= h / 10;
    }
    h.clamp(1, 100)
}

/// Port of FUN_0040a2ed: the random damage divisor (100..=149).
pub fn random_divisor(rng: &mut Rng) -> i32 {
    let r = rng.pct();
    let n = rng.rnd(10);
    let v = if r < 2 {
        if n < 5 { n + 100 } else { n + 140 }
    } else if r < 10 {
        if n < 5 { n + 105 } else { n + 135 }
    } else if r < 25 {
        if n < 5 { n + 110 } else { n + 130 }
    } else if r < 50 {
        if n < 5 { n + 115 } else { n + 125 }
    } else {
        n + 120
    };
    v.max(1)
}

/// Port of FUN_0040a080: type matrix, action power, soft cap, rating bonus,
/// defensive statuses. `x` hits `y`; `dir` is the facing term (6/3/0, negated for counters).
pub fn damage_mods(ctx: &Ctx, v: i32, dir: i32, x: &Fighter, y: &Fighter, act: usize) -> i32 {
    let mut adv = ctx.t.kinds.type_bonus(x.kind, y.kind);
    if adv > 0 && y.abilities & 0x800 != 0 {
        adv -= adv / 2;
    }
    let mut v = v * (dir * 10 + 100 + adv) / 100 * (action(ctx, act).power as i8 as i32) / 10;
    if v > 100 {
        v = if v < 201 {
            (v - 100) * 90 / 100 + 100
        } else if v < 301 {
            (v - 200) * 80 / 100 + 190
        } else if v < 401 {
            (v - 300) * 70 / 100 + 270
        } else {
            (v - 400) * 60 / 100 + 340
        };
    }
    v += (rating(ctx, x, RatingMode::Current, 0) / 1000).min(15);
    if y.status & 0x4000000 != 0 || has_item_ability(ctx, y, 0xd) {
        v -= v / 3;
    }
    if y.status & 0x20 != 0 {
        v = if v < 0x97 { 1 } else { v - 0x96 };
    }
    if y.status & 0x40 != 0 {
        v = if v < 0xb5 { 1 } else { v - 0xb4 };
    }
    if x.status & 0x40000 != 0 {
        v /= 2;
    }
    if y.buffs & 0x1000 != 0 {
        v = v * 80 / 100;
    }
    if y.buffs & 0x2000 != 0 {
        v = v * 70 / 100;
    }
    v
}

/// One attack exchange, as set up by FUN_00408078.
#[derive(Debug, Clone)]
pub struct Duel<'a> {
    pub att: &'a Fighter,
    pub def: &'a Fighter,
    pub att_side: Side,
    pub def_side: Side,
    /// Attacker's action index (0-based into `KindTable::actions`).
    pub action: usize,
    /// Defender's counter action (FUN_00409488 picks it, consuming one `rnd`), if any.
    pub counter: Option<usize>,
    /// FUN_004080b0: 1 front, 2 side, 3 back (attacker relative to defender).
    pub facing: u8,
    /// Defender army flags (`army+0xde`); bit 2 disables criticals against it.
    pub def_army_flags: u32,
}

/// The Duel struct's computed fields (`+0x18..+0x4a`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DuelCalc {
    pub fc_hit: i32,
    pub fc_dmg: i32,
    pub fc_chit: i32,
    pub fc_cdmg: i32,
    pub att_prof: i32,
    pub def_prof: i32,
    pub att_lost: i32,
    pub def_lost: i32,
    /// 戰技 after the desperation rolls of `strike`.
    pub att_acc: i32,
    pub def_acc: i32,
    pub att_terrain: [i8; 3],
    pub def_terrain: [i8; 3],
}

/// Weapon/special proficiency used for an action (forecast `param_1[10]`).
pub fn action_proficiency(ctx: &Ctx, f: &Fighter, act: usize) -> i32 {
    if (action(ctx, act).kind as i8) < 0x15 {
        if f.equip[0] == 0 {
            0
        } else {
            let k = item(ctx, f.equip[0]).map_or(0, |it| it.kind as i8 as i32);
            f.prof.get((k - 1).max(0) as usize).copied().unwrap_or(0) as i32
        }
    } else {
        f.prof[17] as i32
    }
}

fn facing_terms(facing: u8) -> (i32, i32) {
    match facing {
        3 => (5, 6),
        2 => (3, 3),
        _ => (0, 0),
    }
}

fn heights(ctx: &Ctx, d: &Duel) -> (i32, i32) {
    if ctx.simple() { (0, 0) } else { (d.att.height, d.def.height) }
}

/// Terrain triple honouring difficulty 1 (the duel zeroes it there).
pub fn terrain_for(difficulty: u8, t: [i8; 3]) -> [i8; 3] {
    if difficulty == 1 { [0; 3] } else { t }
}

fn ignores_def(x: &Fighter, y: &Fighter) -> bool {
    (x.status & 0x80 != 0 || x.abilities & 0x40000000 != 0) && y.rank != 1
}

fn lost_pct(f: &Fighter) -> i32 {
    cdiv((f.hp_max - f.hp) * 100, f.hp_max)
}

/// Port of FUN_00409642: the forecast shown to the player and used by the AI.
pub fn forecast(ctx: &Ctx, d: &Duel) -> DuelCalc {
    let (att, def) = (d.att, d.def);
    let def_for_att = if ignores_def(att, def) { 0 } else { def.def_cur };
    let def_for_ctr = if ignores_def(def, att) { 0 } else { att.def_cur };
    let mut c = DuelCalc {
        att_prof: action_proficiency(ctx, att, d.action),
        att_terrain: terrain_for(ctx.difficulty, att.terrain),
        def_terrain: terrain_for(ctx.difficulty, def.terrain),
        att_lost: lost_pct(att),
        def_lost: lost_pct(def),
        ..DuelCalc::default()
    };
    let (ha, hd) = heights(ctx, d);
    let dh = ha - hd;
    let (fh, fd) = facing_terms(d.facing);
    let hit = att.skill_cur * (dh * 1000 + 8000 + fh * 800 + c.att_prof) / 8000 - def.react;
    let hit = hit_mods(ctx, att, def, d.action, hit);
    c.fc_hit = vary(ctx, None, hit, Pair::PhysHit, d.att_side);
    if def.buffs & 0x200 != 0 || def.status & 0x2a000 != 0 {
        c.fc_hit = 99;
    }
    let cl = dh.clamp(-8, 4);
    let k = if cl < 0 { 125 } else { 250 };
    let base = (att.atk_cur - def_for_att)
        * (cl * k + 1000 + (c.att_terrain[1] as i32 - c.def_terrain[2] as i32) * 10 + c.att_prof)
        / 1000
        * 100
        / 125;
    c.fc_dmg = damage_mods(ctx, base, fd, att, def, d.action);
    if dh > 0 {
        c.fc_dmg += dh;
    }
    c.fc_dmg = vary(ctx, None, c.fc_dmg, Pair::PhysDamage, d.att_side);
    if def.abilities & 0x1000000 != 0 || def.status & 0x800 != 0 {
        c.fc_hit = 0;
        c.fc_dmg = 0;
    }
    let Some(ctr) = d.counter else { return c };
    c.def_prof = action_proficiency(ctx, def, ctr);
    let dh2 = hd - ha;
    let mut ch = def.skill_cur * (dh2 * 1000 + 8000 - fh * 800 + c.def_prof) / 8000 - att.react;
    ch = hit_mods(ctx, def, att, ctr, ch);
    if def.status & 0x1000 != 0 {
        ch /= 10;
    }
    ch = ch * 45 / 100;
    c.fc_chit = vary(ctx, None, ch, Pair::PhysHit, d.def_side);
    let cl2 = dh2.clamp(-8, 4);
    let k2 = if cl2 < 0 { 125 } else { 250 };
    let base2 = (def.atk_cur - def_for_ctr)
        * (cl2 * k2 + 1000 + (c.def_terrain[1] as i32 - c.att_terrain[2] as i32) * 10 + c.def_prof)
        / 1000
        * 100
        / 125;
    let mut cd = damage_mods(ctx, base2, -fd, def, att, ctr);
    cd = cd * 35 / 100;
    if dh2 > 0 {
        cd += dh2;
    }
    if def.abilities & 0x400 != 0 {
        cd = def.atk;
    }
    if has_item_ability(ctx, def, 0x1e) {
        cd *= 3;
    }
    c.fc_cdmg = vary(ctx, None, cd, Pair::PhysDamage, d.def_side);
    if att.abilities & 0x1000000 != 0 || att.status & 0x800 != 0 {
        c.fc_chit = 0;
        c.fc_cdmg = 0;
    }
    c
}

fn small_damage(rng: &mut Rng) -> i32 {
    let r = rng.pct();
    if r < 1 {
        5
    } else if r < 6 {
        4
    } else if r < 16 {
        3
    } else if r < 46 {
        2
    } else {
        1
    }
}

/// Port of FUN_0040881a: the attacker's real strike. Returns damage (0 = miss).
/// Rolls both sides' desperation first (stored in `calc` for the counter).
pub fn strike(ctx: &Ctx, rng: &mut Rng, d: &Duel, calc: &mut DuelCalc) -> i32 {
    if calc.fc_dmg == 0 && calc.fc_hit == 0 {
        return 0;
    }
    let (att, def) = (d.att, d.def);
    let (ha, hd) = heights(ctx, d);
    let dh = ha - hd;
    calc.att_acc = desperation(rng, att.skill_cur, calc.att_lost);
    calc.def_acc = desperation(rng, def.skill_cur, calc.def_lost);
    let (fh, fd) = facing_terms(d.facing);
    let mut hit = calc.att_acc * (dh * 1000 + 8000 + fh * 800 + calc.att_prof) / 8000 - def.react;
    hit = hit_mods(ctx, att, def, d.action, hit);
    hit = hit * 12 / 10;
    hit = vary(ctx, Some(rng), hit, Pair::PhysHit, d.att_side);
    if def.buffs & 0x200 != 0 || def.status & 0x2a000 != 0 {
        hit = 99;
    }
    if hit < rng.pct() {
        return 0;
    }
    if def.status & 0x800 != 0 {
        return 0;
    }
    let div = random_divisor(rng);
    let cl = dh.clamp(-8, 4);
    let k = if cl < 0 { 125 } else { 250 };
    let dv = if ignores_def(att, def) { 0 } else { def.def_cur };
    let mut dmg = (att.atk_cur - dv)
        * (cl * k + 1000 + (calc.att_terrain[1] as i32 - calc.def_terrain[2] as i32) * 10 + calc.att_prof)
        / 10
        / div;
    dmg = damage_mods(ctx, dmg, fd, att, def, d.action);
    if dh > 0 {
        dmg += dh;
    }
    dmg = vary(ctx, Some(rng), dmg, Pair::PhysDamage, d.att_side);
    if dmg < 3 {
        dmg = small_damage(rng);
    }
    dmg
}

/// Port of FUN_00408bae: the defender's counterattack. Returns damage (0 = miss/none).
pub fn counter_strike(ctx: &Ctx, rng: &mut Rng, d: &Duel, calc: &DuelCalc) -> i32 {
    let Some(ctr) = d.counter else { return 0 };
    if calc.fc_cdmg == 0 && calc.fc_chit == 0 {
        return 0;
    }
    let (att, def) = (d.att, d.def);
    let (ha, hd) = heights(ctx, d);
    let dh2 = hd - ha;
    let (fh, fd) = facing_terms(d.facing);
    let mut hit = calc.def_acc * (dh2 * 1000 + 8000 - fh * 800 + calc.def_prof) / 8000 - att.react;
    hit = hit_mods(ctx, def, att, ctr, hit);
    if def.status & 0x1000 != 0 {
        hit /= 10;
    }
    hit = hit * 12 / 10;
    let r = rng.rnd(4);
    hit = (r + 3) * hit / 10;
    hit = vary(ctx, Some(rng), hit, Pair::PhysHit, d.def_side);
    if hit < rng.pct() {
        return 0;
    }
    if att.status & 0x800 != 0 {
        return 0;
    }
    let div = random_divisor(rng);
    let cl = dh2.clamp(-8, 4);
    let k = if cl < 0 { 125 } else { 250 };
    let dv = if ignores_def(def, att) { 0 } else { att.def_cur };
    let base = (def.atk_cur - dv)
        * (cl * k + 1000 + (calc.def_terrain[1] as i32 - calc.att_terrain[2] as i32) * 10 + calc.def_prof)
        / 10
        / div;
    let m = damage_mods(ctx, base, -fd, def, att, ctr);
    let r = rng.rnd(4);
    let mut dmg = (r + 2) * m / 10 + dh2;
    if def.abilities & 0x400 != 0 {
        dmg = def.atk;
    }
    if has_item_ability(ctx, def, 0x1e) {
        dmg *= 3;
    }
    dmg = vary(ctx, Some(rng), dmg, Pair::PhysDamage, d.def_side);
    if dmg < 1 {
        dmg = small_damage(rng);
    }
    dmg
}

/// Port of FUN_0040927b: critical chance of `x` hitting `y` (counter = defender hits back).
/// The forecast-zero test and the army flag test use the duel's attack forecast
/// and defender army in both directions, as in the original.
pub fn crit_chance(ctx: &Ctx, d: &Duel, calc: &DuelCalc, counter: bool) -> i32 {
    let (x, y) = if counter { (d.def, d.att) } else { (d.att, d.def) };
    let fb = match (d.facing, counter) {
        (3, false) => 10,
        (2, false) => 5,
        (3, true) => -10,
        (2, true) => -5,
        _ => 0,
    };
    if calc.fc_dmg == 0 && calc.fc_hit == 0 {
        return 0;
    }
    if y.abilities & 4 != 0 || d.def_army_flags & 2 != 0 {
        return 0;
    }
    if y.status & 0x4000000 != 0 || has_item_ability(ctx, y, 0xd) {
        return 0;
    }
    let crit: i32 = x.equip.iter().filter(|&&id| id != 0).map(|&id| item(ctx, id).map_or(0, |it| it.crit as i32)).sum();
    let eva: i32 = y.equip.iter().filter(|&&id| id != 0).map(|&id| item(ctx, id).map_or(0, |it| it.evasion as i32)).sum();
    let mut c = crit + x.skill_cur + fb - y.react - eva;
    if x.abilities & 1 != 0 {
        c += 30;
    }
    if y.abilities & 2 != 0 {
        c -= 30;
    }
    if y.status & 4 != 0 || has_item_ability(ctx, y, 0x10) {
        c -= c / 10;
    }
    c.clamp(1, 50)
}

/// Ports of FUN_004091ab (attack) / FUN_00409213 (counter): crit when `pct() < chance`.
pub fn roll_crit(ctx: &Ctx, rng: &mut Rng, d: &Duel, calc: &DuelCalc, counter: bool) -> bool {
    let c = crit_chance(ctx, d, calc, counter);
    rng.pct() < c
}

/// Port of FUN_00409094: instant death of the defender.
pub fn instant_death(ctx: &Ctx, rng: &mut Rng, d: &Duel, calc: &DuelCalc) -> bool {
    if calc.fc_dmg == 0 && calc.fc_hit == 0 {
        return false;
    }
    let (att, def) = (d.att, d.def);
    if def.char_id == 1 || def.char_id == 0xb {
        return false;
    }
    if def.status & 0x4000000 != 0 || has_item_ability(ctx, def, 0xd) {
        return false;
    }
    let chance = if unit_type(ctx, def.kind) == 0xe && def.rank != 1 && has_item_ability(ctx, att, 0x3e) {
        50
    } else if att.abilities & 0x200 != 0 && def.rank & 3 == 0 {
        5
    } else {
        0
    };
    rng.pct() < chance
}

// ------------------------------------------------------------ rating

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatingMode {
    /// 0: current HP/MP/ST/魔力.
    Current,
    /// 1: like Current but HP before the last hit (EXP).
    BeforeHit,
    /// 2: maxima.
    Max,
}

/// The four components of FUN_0045781c.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rating {
    pub magic: i32,
    pub tech: i32,
    pub body: i32,
    pub skills: i32,
    pub total: i32,
}

impl Rating {
    /// Port of FUN_0045781c with part 0 (all components).
    pub fn compute(ctx: &Ctx, f: &Fighter, mode: RatingMode) -> Rating {
        let (hp, mag, mp, st) = match mode {
            RatingMode::Max => (f.hp_max, f.mag, f.mp_max, f.st_max),
            RatingMode::BeforeHit => (f.hp_before_hit, f.mag_cur, f.mp, f.st),
            RatingMode::Current => (f.hp, f.mag_cur, f.mp, f.st),
        };
        let mut m = mag;
        if has_item_ability(ctx, f, 0x12) {
            m += m / 2;
        }
        let b2 = ctx.t.spells.magic_schools as usize;
        let n_magic = (0..b2.min(11)).filter(|&k| f.learned[k] != 0).count() as i32;
        let magic = cdiv((n_magic + 10) * m * (mp + 1), ctx.p(0xd6) * 10 * ctx.p(0xd7));
        let n_tech = (b2..11).filter(|&k| f.learned[k] != 0).count() as i32;
        let tech = cdiv((n_tech + 10) * f.sword * (st + 1), ctx.p(0xd8) * 10 * ctx.p(0xd9));
        let body = cdiv(hp, ctx.p(0xda))
            + cdiv(cdiv((f.atk_cur + f.def_cur) * (f.skill_cur + f.react), ctx.p(0xdb)), ctx.p(0xdc));
        const EXCLUDED: u32 = 0x40 | 0x20000 | 0x40000 | 0x200000 | 0x4000000 | 0x8000000 | 0x80000000;
        let n = (f.abilities & !EXCLUDED).count_ones() as f64;
        let skills = ((n).sqrt() * 400.0 * n) as i32;
        let mult = ctx.t.kinds.class(f.kind).map_or(0, |c| c.rating_mult) as i32;
        let total = (cdiv((magic + tech + body + skills) * f.ap_max, ctx.p(0xdf)) * mult / 10).max(1);
        Rating { magic, tech, body, skills, total }
    }
}

/// Port of FUN_0045781c(f, mode, part): part 0 total, 1 magic, 2 tech, 3 body, 4 skills.
/// (Parts 5..8 return cached values in the original; call `Rating::compute` and keep it.)
pub fn rating(ctx: &Ctx, f: &Fighter, mode: RatingMode, part: u8) -> i32 {
    let r = Rating::compute(ctx, f, mode);
    match part {
        1 => r.magic,
        2 => r.tech,
        3 => r.body,
        4 => r.skills,
        _ => r.total,
    }
}

// ------------------------------------------------------------ EXP / growth

/// Port of FUN_00408fc9: EXP for one hit of `att` on `victim` dealing `dmg`.
pub fn exp_gain(ctx: &Ctx, att: &Fighter, victim: &Fighter, dmg: i32) -> i32 {
    if dmg == 0 && victim.flags & 0x20000000 == 0 {
        return 1;
    }
    let pa = rating(ctx, att, RatingMode::BeforeHit, 0);
    let pv = rating(ctx, victim, RatingMode::BeforeHit, 0);
    let mut e = cdiv(cdiv(dmg * pv * ctx.p(0xe5), pv + pa), ctx.p(0xe1)).max(1);
    if victim.hp == 0 {
        e += 3 + cdiv(pv * ctx.p(0xe2), pa);
    }
    e
}

/// Port of the scaling part of FUN_00452e38 (with FUN_00407880). Returns the
/// EXP to add (the caller caps unit EXP at 999).
pub fn award_exp(ctx: &Ctx, mut e: i32, abilities: u32, side: Side) -> i32 {
    if e == 0 {
        return 0;
    }
    if abilities & 0x40 != 0 {
        e += e / 3;
    }
    e = (e * (ctx.p(0x10) + 3 + ctx.p(0x11)) / 10).clamp(1, 999);
    if side == Side::Player && !ctx.vs_mode {
        match ctx.difficulty {
            1 => e *= 3,
            2 => e <<= 1,
            _ => {}
        }
    }
    e
}

/// Port of FUN_0042345b + FUN_00423607: the 9 stat gains of `class` reaching
/// `level`, in GROWTH_STATS order. HP is rolled twice (first result discarded).
pub fn growth(ctx: &Ctx, rng: &mut Rng, class: u16, level: i32) -> [i32; 9] {
    let Some(g) = (class as usize).checked_sub(1).and_then(|i| ctx.t.growth.growth.get(i)) else {
        return [0; 9];
    };
    let mut roll = |k: usize| {
        let d = g[k].0;
        if d[1] == 0 || d[3] == 0 {
            return 0;
        }
        let r1 = rng.rnd(d[4] as i32 + 1);
        let r2 = rng.rnd(d[5] as i32 + 1);
        g[k].roll(level, r1, r2)
    };
    let mut out = [0i32; 9];
    out[0] = roll(0);
    out[1] = roll(1);
    out[2] = roll(2);
    out[0] = roll(0);
    for (k, o) in out.iter_mut().enumerate().skip(3) {
        *o = roll(k);
    }
    out
}

/// The stat fields FUN_00456c88 touches (base and current copies).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrowStats {
    pub hp_max: i32,
    pub hp: i32,
    pub mp_max: i32,
    pub mp: i32,
    pub st_max: i32,
    pub st: i32,
    pub atk: i32,
    pub atk_cur: i32,
    pub def: i32,
    pub def_cur: i32,
    pub react: i32,
    pub skill: i32,
    pub skill_cur: i32,
    pub mag: i32,
    pub mag_cur: i32,
    pub sword: i32,
}

/// Port of FUN_00456c88: add gains (each +1 when the unit has growth flag 1);
/// HP 0..=5000, others 0..=999.
pub fn apply_growth(s: &mut GrowStats, g: [i32; 9], bonus: bool) {
    let b = bonus as i32;
    let add = |v: &mut i32, d: i32, max: i32| *v = (*v + d).clamp(0, max);
    add(&mut s.hp_max, g[0] + b, 5000);
    add(&mut s.hp, g[0] + b, 5000);
    add(&mut s.mp_max, g[1] + b, 999);
    add(&mut s.mp, g[1] + b, 999);
    add(&mut s.st_max, g[2] + b, 999);
    add(&mut s.st, g[2] + b, 999);
    add(&mut s.atk, g[3] + b, 999);
    add(&mut s.atk_cur, g[3] + b, 999);
    add(&mut s.def, g[4] + b, 999);
    add(&mut s.def_cur, g[4] + b, 999);
    add(&mut s.react, g[5] + b, 999);
    add(&mut s.skill, g[6] + b, 999);
    add(&mut s.skill_cur, g[6] + b, 999);
    add(&mut s.mag, g[7] + b, 999);
    add(&mut s.mag_cur, g[7] + b, 999);
    add(&mut s.sword, g[8] + b, 999);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelState {
    pub level: i32,
    pub exp: i32,
    pub exp_next: i32,
}

/// Ports of FUN_00453725 + FUN_004537da: level up while EXP allows. Returns the
/// gains of each level gained (for the level-up popup).
pub fn level_up(ctx: &Ctx, rng: &mut Rng, class: u16, lv: &mut LevelState, s: &mut GrowStats, bonus: bool) -> Vec<[i32; 9]> {
    let mut out = Vec::new();
    if lv.exp < lv.exp_next || lv.level >= 99 {
        return out;
    }
    loop {
        lv.level += 1;
        let g = growth(ctx, rng, class, lv.level);
        lv.exp -= lv.exp_next;
        apply_growth(s, g, bonus);
        out.push(g);
        if !(lv.exp_next <= lv.exp && lv.level < 99) {
            break;
        }
    }
    out
}

/// Port of FUN_00452da5 (slot part): proficiency slot trained by an action, if any.
pub fn proficiency_slot(ctx: &Ctx, act: usize) -> Option<usize> {
    let k = action(ctx, act).kind as i8 as i32;
    if !(1..=10).contains(&k) {
        if k == 0xb || k == 0xc { None } else { Some(17) }
    } else if (2..=7).contains(&k) {
        Some((k - 2) as usize)
    } else {
        None
    }
}

/// Port of FUN_00452da5 (update): +1 below the 1500 cap.
pub fn gain_proficiency(prof: &mut [u16; 18], slot: usize) {
    if prof[slot] < 0x5dc {
        prof[slot] += 1;
    }
}

/// Port of FUN_00452af0 (value): proficiency floor given to AI units. The
/// caller applies it to each action slot of the class (`proficiency_slot`)
/// when not in VS mode, the army is the enemy's or the unit is an NPC, and
/// the stage is not story-only (kind 3).
pub fn ai_proficiency_floor(ctx: &Ctx, class: u16, level: i32) -> i32 {
    let f = ctx.t.kinds.class(class).map_or(0, |c| c.proficiency_rate) as u32;
    // The original multiplies as unsigned.
    let a = ((ctx.p(0xe0) as u32).wrapping_mul(ctx.difficulty as u32).wrapping_mul(f) / 10) as i32;
    let b = ((level as u32).wrapping_mul(f) / 0x14) as i32;
    (a + b).clamp(0, 999)
}

/// Port of FUN_0043a522: class change offered (`has_targets` = promotion list non-empty).
/// The original also clamps EXP to `exp_next` when the level is high enough but there are no targets.
pub fn can_promote(level: i32, exp: i32, exp_next: i32, has_targets: bool, vs: bool) -> bool {
    if level < 11 && (exp < exp_next || level < 10) {
        return false;
    }
    !vs && has_targets
}

// ------------------------------------------------------------ spells

/// A cast (the SpellCaster object at `DAT_004a1f58`).
#[derive(Debug, Clone)]
pub struct SpellCast<'a> {
    pub caster: &'a Fighter,
    /// Combo partner (`+0x0c`) for spells with `combo & 7`.
    pub partner: Option<&'a Fighter>,
    /// 1-based spell id (`+0x10`).
    pub spell: u16,
    pub side: Side,
    /// FUN_004080b0 relation of the caster to the target: 1 front, 2 side, 3 back.
    pub facing: u8,
}

/// Port of FUN_0045f78b (AP part): AP spent by casting; `already_used` = the
/// unit's turn flag for this kind (4 magic, 8 technique) was already set.
pub fn spell_ap_cost(ctx: &Ctx, already_used: bool) -> i32 {
    ctx.p(0xef) + if already_used { ctx.p(0xf3) } else { 0 }
}

/// MP/ST/HP spent by a caster (FUN_0045f78b, skipped when BIN 10 per-spell byte == 2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceCost {
    pub mp: i32,
    pub st: i32,
    /// HP spent (HP is not lowered below 1).
    pub hp: i32,
}

/// Port of FUN_0045f78b (resource part) for one caster (partners pay the same, computed with their own abilities).
pub fn spell_resource_cost(ctx: &Ctx, spell_id: u16, caster_abilities: u32) -> ResourceCost {
    let sp = spell(ctx, spell_id);
    if ctx.t.params.per_spell[1].get(spell_id as usize - 1) == Some(&2) {
        return ResourceCost::default();
    }
    let c = sp.cost as i32;
    ResourceCost {
        mp: if sp.stat_flags & 1 != 0 { if caster_abilities & 0x20 != 0 { c / 2 } else { c } } else { 0 },
        st: if sp.stat_flags & 2 != 0 { c } else { 0 },
        hp: if sp.stat_flags & 4 != 0 { c } else { 0 },
    }
}

/// Map a resistance difference to a percentage (shared tail of FUN_004683f2/FUN_00468822).
fn resist_curve(v: i32) -> i32 {
    if v >= 0x2b {
        99
    } else if v >= 10 {
        ((v - 10) >> 2) + 0x5a
    } else if v >= 1 {
        v + 0x50
    } else if v == 0 {
        0x50
    } else if v >= -0x27 {
        v / 2 + 0x50
    } else if v >= -0x4f {
        v + 100
    } else if v >= -0x77 {
        (v + 0x50) / 2 + 0x15
    } else {
        1
    }
}

/// Result kind of FUN_004683f2 (`*param_4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicVs {
    /// 0: no contest; value is the spell's base chance.
    None(i32),
    /// 1: raw (MAG − armour)/2 for equipment-destroying spells.
    Raw(i32),
    /// 2: effect 1 (damage) percentage.
    Damage(i32),
    /// 3: other effects' percentage.
    Other(i32),
}

/// Port of FUN_004683f2. `same_army` = target is in the caster's army.
/// `slot` receives the equipment slot chosen by effects 0x22/0x26 (SpellCaster+0x600).
pub fn magic_vs(ctx: &Ctx, rng: &mut Rng, cast: &SpellCast, target: &Fighter, same_army: bool, slot: &mut u8) -> MagicVs {
    let sp = spell(ctx, cast.spell);
    let eff = sp.effect;
    let c = cast.caster;
    let base = sp.hit as i32;
    let school_slot = sp.school as usize + 5;
    let (mut v, generic) = if eff == 1 {
        // The original reads the caster's proficiency for both sides.
        let p1 = ((c.prof[school_slot] / 10) as i32).max(10);
        let p2 = p1;
        let m1 = c.mag_cur.clamp(10, 99);
        let m2 = target.mag_cur.clamp(10, 99);
        ((m1 * (p1 + 500) - m2 * (p2 + 500)) / 500, false)
    } else {
        if eff == 0x0e || eff == 0x12 || eff == 0x5a || same_army {
            return MagicVs::None(base);
        }
        if eff == 0x22 {
            let armour: Vec<(u16, u8)> =
                (1..6).filter(|&s| target.equip[s] != 0).map(|s| (target.equip[s], s as u8 + 1)).collect();
            let defv = if armour.is_empty() {
                *slot = 0;
                0
            } else {
                let i = rng.rnd(armour.len() as i32) as usize;
                *slot = armour[i].1;
                item(ctx, armour[i].0).map_or(0, |it| it.defense as i32)
            };
            return MagicVs::Raw((c.mag_cur - defv) / 2);
        }
        if eff == 0x26 {
            let v = if target.equip[0] == 0 {
                *slot = 0;
                c.mag_cur
            } else {
                *slot = 1;
                c.mag_cur - item(ctx, target.equip[0]).map_or(0, |it| it.attack as i32)
            };
            return MagicVs::Raw(v / 2);
        }
        (c.mag_cur - target.mag_cur - 100 + base, true)
    };
    v = resist_curve(v);
    if !generic {
        v = v * base / 100;
    }
    if target.rank & 3 != 0 && eff == 0x0d {
        v = v * 7 / 10;
    }
    if eff == 1 { MagicVs::Damage(v) } else { MagicVs::Other(v) }
}

/// Port of FUN_00468822: technique contest. Terrain triples come from each unit's cell.
pub fn tech_vs(ctx: &Ctx, cast: &SpellCast, target: &Fighter) -> i32 {
    let sp = spell(ctx, cast.spell);
    let slot = sp.school as usize + 5;
    let c = cast.caster;
    let (prof, sword, atk_t) = match (sp.combo & 7 != 0, cast.partner) {
        (true, Some(p)) => (
            ((p.prof[slot] / 10) as i32 + (c.prof[slot] / 10) as i32) / 2,
            (c.sword + p.sword) / 2,
            ((c.terrain[1] as i32 + p.terrain[1] as i32) / 2) as i8 as i32,
        ),
        _ => ((c.prof[slot] / 10) as i32, c.sword, c.terrain[1] as i32),
    };
    let a = sword as i64 * (prof as i64 + 1000) * (atk_t as i64 + 100);
    let b = ((target.skill_cur + target.sword) / 2) as i64
        * ((target.prof[slot] / 10) as i64 + 1000)
        * (target.terrain[2] as i64 + 100);
    let v = (a - b) as i32 / 100000;
    resist_curve(v) * sp.hit as i32 / 100
}

/// Port of FUN_00467fe9: chance (1..=99, 0 = immune) that a spell affects `target`.
/// `random` is SpellCaster+0x3c (false = AI evaluation). `rng` is needed even
/// then: equipment-destroying spells pick the slot with `rnd` either way.
/// `same_army`: the target belongs to the caster's army (`param_3 == *this`).
pub fn spell_chance(
    ctx: &Ctx,
    rng: &mut Rng,
    random: bool,
    cast: &SpellCast,
    target: &Fighter,
    same_army: bool,
    slot: &mut u8,
) -> i32 {
    let sp = spell(ctx, cast.spell);
    let flags = sp.flags;
    if target.move_type != 0 && flags & 1 != 0 {
        return 0;
    }
    if target.rank == 1 && flags & 2 != 0 {
        return 0;
    }
    if target.rank == 2 && flags & 0x20 != 0 {
        return 0;
    }
    if sp.target == 0 {
        return sp.hit as i32;
    }
    if sp.effect == 0x10 && target.abilities & 0x5000 != 0 {
        return 0;
    }
    if sp.effect == 1 && (target.buffs & 0x200 != 0 || target.status & 0x22000 != 0) {
        return 99;
    }
    if sp.effect == 0x12 && target.status & 0x10000 != 0 {
        return 0;
    }
    let mut c = 0;
    let mut n = 0;
    if sp.stat_flags & 4 != 0 {
        c = sp.hit as i32;
        n = 1;
    }
    if sp.stat_flags & 1 != 0 {
        let r = magic_vs(ctx, rng, cast, target, same_army, slot);
        let v = match r {
            MagicVs::Raw(v) => {
                let v = vary(ctx, roll(rng, random), v, Pair::MagicHit, cast.side);
                c = c.clamp(5, 99);
                v
            }
            MagicVs::Damage(v) => {
                c = vary(ctx, roll(rng, random), c, Pair::TechHit, cast.side);
                if c == 0 {
                    c = 1;
                }
                v
            }
            MagicVs::Other(v) => {
                c = vary(ctx, roll(rng, random), c, Pair::MagicHit, cast.side);
                if c == 0 {
                    c = 1;
                }
                v
            }
            MagicVs::None(v) => v,
        };
        c += v;
        n += 1;
    }
    if sp.stat_flags & 2 != 0 {
        let v = tech_vs(ctx, cast, target);
        c = vary(ctx, roll(rng, random), c, Pair::TechHit, cast.side);
        c += v;
        n += 1;
    }
    if n == 0 {
        return 0;
    }
    c /= n;
    if sp.stat_flags & 0x80 != 0 && !ctx.simple() {
        let dh = (cast.caster.height - target.height).clamp(-5, 5);
        c += c * 0x14 * dh / 100;
    }
    c.clamp(1, 99)
}

/// Power formula shapes of FUN_0046932f; `a` = first constant offset. The
/// `x * a / b + c` and `x * a / b - y * c / d + e` terms are its helpers
/// FUN_0046ada4 / FUN_0046adbf.
#[derive(Debug, Clone, Copy)]
enum Form {
    Base,
    L(usize),
    LL(usize),
    CasterHp(usize),
    Rating1(usize),
    TargetHp,
    Square(usize),
    Adeb(usize),
    Square0(usize),
    RatingCaster(usize),
    RatingTarget(usize),
    BodyCaster(usize),
    Align(usize),
    Align100(usize),
    Kills(usize),
    TargetHpL(usize),
}

fn form(id: u8) -> Form {
    use Form::*;
    match id {
        1..=4 => L(0x12 + 2 * (id as usize - 1)),
        5 => LL(0x1a),
        6 => L(0x1e),
        7..=0xc => LL(0x20 + 4 * (id as usize - 7)),
        0xd => L(0x38),
        0xe => L(0x3a),
        0xf => LL(0x3c),
        0x10 => L(0x40),
        0x11..=0x15 => LL(0x42 + 4 * (id as usize - 0x11)),
        0x16 => L(0x56),
        0x17 => CasterHp(0x58),
        0x18 => LL(0x5a),
        0x19 => L(0x5e),
        0x1a => LL(0x60),
        0x1b => Rating1(0x64),
        0x1c => L(0x66),
        0x1d => L(0x68),
        0x1e => LL(0x6a),
        0x1f => LL(0x6e),
        0x20 => L(0x72),
        0x21 => LL(0x74),
        0x22 => TargetHp,
        0x23 => Square(0x76),
        0x24..=0x2a => LL(0x7a + 4 * (id as usize - 0x24)),
        0x2b => CasterHp(0x96),
        0x2c..=0x41 => Adeb(0x98 + 2 * (id as usize - 0x2c)),
        0x42 => Square0(0xc4),
        0x43 => RatingCaster(0xc6),
        0x44 => RatingTarget(0xc8),
        0x45 => BodyCaster(0xca),
        0x46 => Align(0xcc),
        0x47 => Align100(0xce),
        0x48 => Kills(0xd0),
        0x49 => TargetHpL(0xd2),
        0x4a => TargetHpL(0xd4),
        _ => Base,
    }
}

/// Port of FUN_0046932f: raw spell value by formula id (before effect modifiers).
pub fn spell_power(ctx: &Ctx, cast: &SpellCast, target: &Fighter) -> i32 {
    let sp = spell(ctx, cast.spell);
    let c = cast.caster;
    let combo = if sp.combo & 7 != 0 { cast.partner } else { None };
    let (mut x, mut nx, mut y, mut ny) = (0, 0, 0, 0);
    if sp.stat_flags & 1 != 0 {
        x = c.mag_cur;
        nx = 1;
        if has_item_ability(ctx, c, 0x12) {
            x += x / 5;
        }
        if let Some(p) = combo {
            x += p.mag_cur;
            nx = 2;
            if has_item_ability(ctx, p, 0x12) {
                x += p.mag_cur / 5;
            }
        }
        y = target.mag_cur;
        ny = 1;
        if has_item_ability(ctx, target, 0x12) {
            y += target.mag_cur / 5;
        }
    }
    if sp.stat_flags & 2 != 0 {
        x += c.sword;
        nx += 1;
        if let Some(p) = combo {
            x += p.sword;
            nx += 1;
        }
        y += target.sword;
        ny += 1;
    }
    if nx == 0 || ny == 0 {
        return 0;
    }
    x = (x / nx).max(1);
    y = (y / ny).max(1);
    let mut align = target.alignment;
    if has_item_ability(ctx, target, 0x40) {
        align += 10;
    }
    let slot = sp.school as usize + 5;
    let mut prof = (c.prof[slot] / 10) as i32;
    if let Some(p) = combo {
        prof = ((p.prof[slot] / 10) as i32 + prof) / 2;
    }
    let b = sp.power as i32;
    let p = |i: usize| ctx.p(i);
    let l = |v: i32, a: usize, base: i32| cdiv(v * p(a), p(a + 1)) + base;
    match form(sp.formula) {
        Form::Base => b,
        Form::L(a) => l(x, a, b),
        Form::LL(a) => cdiv(x * p(a), p(a + 1)) - cdiv(y * p(a + 2), p(a + 3)) + b,
        Form::CasterHp(a) => l(c.hp, a, b),
        Form::Rating1(a) => l(rating(ctx, c, RatingMode::Current, 1), a, b),
        Form::TargetHp => target.hp,
        Form::Square(a) => cdiv(cdiv(x * x, y) * p(a), p(a + 1)) + b,
        Form::Adeb(a) => (prof + 1000) * x / 1000 - cdiv(y * p(a), p(a + 1)) - target.def_cur + b,
        Form::Square0(a) => l(cdiv(x * x, y), a, 0),
        Form::RatingCaster(a) => l(rating(ctx, c, RatingMode::Current, 0), a, 0),
        Form::RatingTarget(a) => l(rating(ctx, target, RatingMode::Current, 0), a, 0),
        Form::BodyCaster(a) => l(rating(ctx, c, RatingMode::Current, 3), a, 0),
        Form::Align(a) => l(align, a, b),
        Form::Align100(a) => l(100 - align, a, b),
        Form::Kills(a) => l((target.kills as u16 as i32) - align + 100, a, b),
        Form::TargetHpL(a) => l(target.hp, a, b),
    }
}

/// Port of FUN_00468d53: damage-spell modifiers (facing, shields, elements, movement, items, variance).
pub fn spell_damage_mods(ctx: &Ctx, roll: Roll, cast: &SpellCast, target: &Fighter, mut v: i32) -> i32 {
    let sp = spell(ctx, cast.spell);
    if sp.flags & 0x40 != 0 {
        return v;
    }
    let mt = target.move_type;
    if sp.stat_flags & 0x70 != 0 && !ctx.simple() && target.abilities & 0x2000 == 0 {
        let col = if sp.stat_flags & 0x10 != 0 {
            0
        } else if sp.stat_flags & 0x20 != 0 {
            1
        } else if sp.stat_flags & 0x40 != 0 {
            2
        } else {
            0
        };
        const T: [i32; 6] = [0x14, 0x19, 0x1e, 0x28, 0x2d, 0x32];
        match cast.facing {
            3 => v += v * T[col + 3] / 100,
            2 => v += v * T[col] / 100,
            _ => {}
        }
    }
    let school = sp.school;
    if target.status & 0x100 != 0 && school == 5 {
        v /= 2;
    }
    if let Some(cls) = ctx.t.kinds.class(target.kind) {
        if cls.resist_elements & sp.elements != 0 {
            v /= 10;
        }
        if cls.weak_elements & sp.elements != 0 {
            v *= 3;
        }
    }
    if mt == 0 && sp.flags & 4 != 0 {
        v += v / 5;
    }
    if (mt == 3 || mt == 4) && sp.flags & 8 != 0 {
        v += v / 5;
    }
    if (mt == 3 || mt == 4) && sp.flags & 0x10 != 0 {
        v /= 10;
    }
    if target.abilities & 0x10 != 0 && sp.stat_flags & 1 != 0 {
        v /= 2;
    }
    if cast.caster.abilities & 0x400000 != 0 && sp.stat_flags & 1 != 0 {
        v = v * 0x7d / 100;
    }
    if target.abilities & 0x2000000 != 0 && (school == 2 || school == 4) {
        v /= 2;
    }
    if has_item_ability(ctx, target, 0xb) && (3..=6).contains(&school) {
        v /= 2;
    }
    if target.status & 0x4000000 != 0 || has_item_ability(ctx, target, 0xd) {
        v = (v << 1) / 3;
    }
    if target.buffs & 0x1000 != 0 {
        v = v * 0x50 / 100;
    }
    if target.buffs & 0x2000 != 0 {
        v = v * 0x46 / 100;
    }
    if sp.stat_flags & 4 == 0
        && sp.stat_flags & 1 == 0
        && sp.stat_flags & 2 != 0
        && sp.combo & 7 != 0
        && has_item_ability(ctx, cast.caster, 2)
        && cast.partner.is_some_and(|p| has_item_ability(ctx, p, 2))
    {
        v += v / 2;
    }
    v = vary(ctx, roll, v, Pair::SpellDamage, cast.side);
    if target.status & 0x40 != 0 {
        v = if v < 0xb5 { 1 } else { v - 0xb4 };
    }
    v
}

/// Port of FUN_00468b0d: the amount a spell applies to `target`, by effect code.
pub fn spell_value(ctx: &Ctx, roll: Roll, cast: &SpellCast, target: &Fighter) -> i32 {
    let sp = spell(ctx, cast.spell);
    let v = spell_power(ctx, cast, target);
    match sp.effect {
        1 | 0x24 | 0x2a => {
            let v = spell_damage_mods(ctx, roll, cast, target, v);
            if target.status & 0x800 != 0 { 0 } else { v }
        }
        2 | 6 | 7 | 9 | 0xc | 0x29 | 0x31 => vary(ctx, roll, v, Pair::Support, cast.side),
        0x12 | 0x5a => {
            if target.status & 0x10000 != 0 {
                0
            } else {
                let v = if has_item_ability(ctx, target, 0x13) { v << 1 } else { v };
                vary(ctx, roll, v, Pair::Heal, cast.side)
            }
        }
        0x23 | 0x2b => v,
        0x30 => {
            let v = if target.status & 0x4000000 != 0 || has_item_ability(ctx, target, 0xd) { v - v / 3 } else { v };
            vary(ctx, roll, v, Pair::Support, cast.side)
        }
        _ => sp.power as i32,
    }
}

/// Port of FUN_0046ae25 plus the `+1` its three callers add when the caster's
/// army is the enemy side: status duration in turns; -1 = permanent (which
/// the `+1` turns into 0 for enemy casters, as in the original).
pub fn spell_duration(ctx: &Ctx, cast: &SpellCast, target: &Fighter) -> i32 {
    let d = spell(ctx, cast.spell).duration as i32;
    let t = if d < 0x65 {
        d
    } else if d < 0xc9 {
        cdiv(cast.caster.mag_cur, d - 100).max(1)
    } else if d < 0xff {
        cdiv(cast.caster.mag_cur - target.mag_cur, d - 200).max(1)
    } else {
        -1
    };
    if cast.side == Side::Enemy { t + 1 } else { t }
}

#[cfg(test)]
mod tests {
    use super::super::rng::Seeder;
    use super::*;
    use s2data::tables::{
        ClassBase, ClassDef, FormulaParams, GrowthDesc, GrowthTable, KindTable, SpellTable, TerrainTable,
    };

    fn class(unit_type: u8, rating_mult: u8) -> ClassDef {
        ClassDef {
            name: String::new(),
            name_raw: Vec::new(),
            sprite_set: 0,
            exp_per_level: 100,
            unit_type,
            move_type: 1,
            innate_skill: 0,
            proficiency_rate: 10,
            base_ap: 60,
            resist_elements: 0,
            weak_elements: 0,
            alignment: 50,
            change_item: 0,
            change_flags: 0,
            rating_mult,
            tier: 1,
            schools: 0,
            promotions: [0; 8],
            actions: 0..1,
            unknown: [0; 3],
        }
    }

    fn sword_action() -> ActionDef {
        ActionDef {
            kind: 2,
            name: "劍".into(),
            name_raw: Vec::new(),
            anim_set: 2,
            power: 10,
            hit: 100,
            range: (0, 0),
            motion: (0, 0),
            status: (0, 0, 0),
            anim: 0,
            sound: 0,
            shape: 1,
        }
    }

    /// Synthetic tables: P[0..] chosen so variance is exact in expected mode.
    fn tables() -> RuleTables {
        let mut p = vec![0i8; 274];
        for k in (0..0x12).step_by(2) {
            p[k] = 9; // base
            p[k + 1] = -1; // spread: (−1 + 3)/2 = 1 → m = 10 (×1.0)
        }
        for v in p.iter_mut().skip(0x12).take(0xd6 - 0x12) {
            *v = 10;
        }
        p[0xd6] = 10;
        p[0xd7] = 10;
        p[0xd8] = 10;
        p[0xd9] = 10;
        p[0xda] = 10;
        p[0xdb] = 10;
        p[0xdc] = 10;
        p[0xdf] = 100;
        p[0xe0] = 4;
        p[0xe1] = 10;
        p[0xe2] = 2;
        p[0xe5] = 10;
        p[0xef] = 20;
        p[0xf3] = 10;
        p[0xf4] = 0;
        p[0xf5] = 10;
        p[0xf6] = 20;
        RuleTables {
            kinds: KindTable {
                classes: vec![class(1, 10), class(2, 10)],
                actions: vec![sword_action()],
                unit_types: vec!["a".into(), "b".into()],
                skills: vec![],
                elements: vec![],
                type_matrix: vec![vec![0, 20], vec![-10, 0]],
            },
            items: vec![],
            item_help: vec![],
            terrain: TerrainTable { names: vec![], ground: 0, objects: 0, effects: vec![] },
            spells: SpellTable {
                schools: vec![],
                magic_schools: 8,
                tech_schools: 3,
                school_spells: vec![],
                spells: vec![],
                status_names: vec![],
            },
            growth: GrowthTable {
                id_limit: 272,
                growth: vec![[GrowthDesc([1, 3, 6, 1, 2, 0]); 9], [GrowthDesc([0, 0, 0, 0, 0, 0]); 9]],
                base: vec![
                    ClassBase { stats: [0; 9], equipment: [0; 5], spells: [0; 11] };
                    2
                ],
            },
            params: FormulaParams { per_spell: Default::default(), effect_sound: vec![], p },
            world: vec![],
        }
    }

    fn fighter(kind: u16) -> Fighter {
        Fighter {
            kind,
            hp: 100,
            hp_max: 100,
            hp_before_hit: 100,
            atk_cur: 60,
            def_cur: 20,
            skill_cur: 40,
            react: 10,
            ap_max: 60,
            ..Fighter::default()
        }
    }

    #[test]
    fn variance_expected_and_random() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        // expected: m = 9 + (−1+3)/2 = 10 → unchanged; enemy side at difficulty 3: +P[0xf6]=20%
        assert_eq!(vary(&ctx, None, 50, Pair::PhysHit, Side::Player), 50);
        assert_eq!(vary(&ctx, None, 50, Pair::PhysHit, Side::Enemy), 60);
        assert_eq!(vary(&ctx, None, 500, Pair::PhysHit, Side::Player), 99);
        assert_eq!(vary(&ctx, None, -5, Pair::PhysDamage, Side::Player), 1);
        // random: rnd(2) ∈ {0,1} → m ∈ {9,10}
        let mut rng = Rng::default();
        let v = vary(&ctx, Some(&mut rng), 100, Pair::PhysDamage, Side::Player);
        assert!(v == 90 || v == 100);
    }

    #[test]
    fn desperation_bands() {
        let mut rng = Rng::default();
        assert_eq!(desperation(&mut rng, 40, 70), 40);
        let v = desperation(&mut rng, 40, 80);
        assert!(v == 52 || v == 12);
        let v = desperation(&mut rng, 41, 95);
        assert!(v == 61 || v == 20);
    }

    #[test]
    fn damage_mods_matrix_and_soft_cap() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        let a = fighter(1);
        let b = fighter(2);
        // rating(a): body = 100/10 + ((60+20)*(40+10)/10)/10 = 10 + 40 = 50;
        // magic = (10*0*1)/1000 = 0; tech = 0; total = 50*60/100*10/10 = 30 → +0
        assert_eq!(rating(&ctx, &a, RatingMode::Current, 0), 30);
        // matrix[0][1] = +20: 100 * 120/100 = 120 → soft cap (20*90/100)+100 = 118
        assert_eq!(damage_mods(&ctx, 100, 0, &a, &b, 0), 118);
        // back attack dir 6: 50 * (60+100+20)/100 = 90
        assert_eq!(damage_mods(&ctx, 50, 6, &a, &b, 0), 90);
        // b hits a: −10 → 50*90/100 = 45
        assert_eq!(damage_mods(&ctx, 50, 0, &b, &a, 0), 45);
    }

    #[test]
    fn forecast_hand_computed() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        let a = fighter(1);
        let b = fighter(2);
        let d = Duel { att: &a, def: &b, att_side: Side::Player, def_side: Side::Enemy, action: 0, counter: Some(0), facing: 1, def_army_flags: 0 };
        let c = forecast(&ctx, &d);
        // hit = 40*(8000)/8000 − 10 = 30; + (90*100/100) unarmed = 120 → clamp 100; vary → 99
        assert_eq!(c.fc_hit, 99);
        // dmg = (60−20)*1000/1000*100/125 = 32 → ×120/100 = 38 (no soft cap), +0
        assert_eq!(c.fc_dmg, 38);
        // counter hit: 30 + 90 = 120 → 100, ×45/100 = 45, enemy +20% = 54
        assert_eq!(c.fc_chit, 54);
        // counter dmg: 32 ×90/100 = 28, ×35/100 = 9, enemy +20% → 10
        assert_eq!(c.fc_cdmg, 10);
    }

    #[test]
    fn strike_is_reproducible() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        let a = fighter(1);
        let b = fighter(2);
        let d = Duel { att: &a, def: &b, att_side: Side::Player, def_side: Side::Enemy, action: 0, counter: Some(0), facing: 1, def_army_flags: 0 };
        let mut c1 = forecast(&ctx, &d);
        let mut c2 = c1;
        let mut r1 = Rng::started(Seeder::fixed(5));
        let mut r2 = Rng::started(Seeder::fixed(5));
        let d1 = strike(&ctx, &mut r1, &d, &mut c1);
        let d2 = strike(&ctx, &mut r2, &d, &mut c2);
        assert_eq!(d1, d2);
        // Hand-computed replay of the draw order with a twin generator:
        // vary(hit): rnd(2); roll: pct; divisor: pct, rnd(10); vary(dmg): rnd(2).
        let mut r = Rng::started(Seeder::fixed(5));
        let hit = (100 * 12 / 10 * (9 + r.rnd(2)) / 10).clamp(1, 99);
        let landed = hit >= r.pct();
        assert_eq!(landed, d1 > 0);
        if landed {
            let div = random_divisor(&mut r);
            let base = 40 * 1000 / 10 / div; // (ATK−DEF)*factor/10/div
            let m = base * 120 / 100; // matrix +20%, power x1.0, below the soft cap
            let v = (m * (9 + r.rnd(2)) / 10).max(1);
            let v = if v < 3 { 0 } else { v };
            assert_eq!(d1, v);
        }
        assert_eq!(r.snapshot(), r1.snapshot());
        assert_eq!(counter_strike(&ctx, &mut r1, &d, &c1), counter_strike(&ctx, &mut r2, &d, &c2));
    }

    #[test]
    fn crit_and_death() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        let mut a = fighter(1);
        let b = fighter(2);
        a.abilities = 1;
        let d = Duel { att: &a, def: &b, att_side: Side::Player, def_side: Side::Enemy, action: 0, counter: None, facing: 3, def_army_flags: 0 };
        let c = forecast(&ctx, &d);
        // 0 + 40 + 10 − 10 − 0 + 30 = 70 → 50
        assert_eq!(crit_chance(&ctx, &d, &c, false), 50);
        // counter direction: 40 − 10 − 10 = 20
        assert_eq!(crit_chance(&ctx, &d, &c, true), 20);
        let d2 = Duel { def_army_flags: 2, ..d.clone() };
        assert_eq!(crit_chance(&ctx, &d2, &c, false), 0);
        let mut rng = Rng::default();
        assert!(!instant_death(&ctx, &mut rng, &d, &c));
    }

    #[test]
    fn exp_formulas() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        let a = fighter(1);
        let mut v = fighter(2);
        // both ratings 30: e = (20*30*10/(60))/10 = 10
        assert_eq!(exp_gain(&ctx, &a, &v, 20), 10);
        v.hp = 0;
        // + 3 + 30*2/30 = 5 → 15
        assert_eq!(exp_gain(&ctx, &a, &v, 20), 15);
        assert_eq!(exp_gain(&ctx, &a, &fighter(2), 0), 1);
        // award: (P[0x10]=9)+3+(P[0x11]=−1) = 11 → 10*11/10 = 11
        assert_eq!(award_exp(&ctx, 10, 0, Side::Player), 11);
        assert_eq!(award_exp(&Ctx::new(&t, 1, false), 10, 0, Side::Player), 33);
        assert_eq!(award_exp(&Ctx::new(&t, 2, false), 10, 0x40, Side::Player), 28);
        assert_eq!(award_exp(&Ctx::new(&t, 1, false), 10, 0, Side::Enemy), 11);
    }

    #[test]
    fn growth_rolls_hp_twice_and_level_up() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        let mut a = Rng::started(Seeder::fixed(1));
        let mut b = Rng::started(Seeder::fixed(1));
        let g = growth(&ctx, &mut a, 1, 3);
        // reference: 10 descriptors evaluated, each draws rnd(3) then rnd(1)
        let mut exp = [0; 9];
        let mut roll = || {
            let r1 = b.rnd(3);
            let r2 = b.rnd(1);
            GrowthDesc([1, 3, 6, 1, 2, 0]).roll(3, r1, r2)
        };
        exp[0] = roll();
        exp[1] = roll();
        exp[2] = roll();
        exp[0] = roll();
        for e in exp.iter_mut().skip(3) {
            *e = roll();
        }
        assert_eq!(g, exp);
        assert_eq!(a.snapshot(), b.snapshot());
        // class 2 has d1 = 0: no draws at all
        let before = a.snapshot();
        assert_eq!(growth(&ctx, &mut a, 2, 5), [0; 9]);
        assert_eq!(a.snapshot(), before);

        let mut lv = LevelState { level: 1, exp: 250, exp_next: 100 };
        let mut s = GrowStats { hp_max: 4999, hp: 4999, ..GrowStats::default() };
        let gains = level_up(&ctx, &mut a, 1, &mut lv, &mut s, true);
        assert_eq!(gains.len(), 2);
        assert_eq!((lv.level, lv.exp), (3, 50));
        assert_eq!(s.hp_max, 5000);
        assert_eq!(s.sword, gains[0][8] + gains[1][8] + 2, "every stat, 劍術 included, gets the +1 bonus");
    }

    #[test]
    fn proficiency_and_floor() {
        let t = tables();
        let ctx = Ctx::new(&t, 3, false);
        assert_eq!(proficiency_slot(&ctx, 0), Some(0));
        let mut prof = [0u16; 18];
        prof[0] = 1499;
        gain_proficiency(&mut prof, 0);
        gain_proficiency(&mut prof, 0);
        assert_eq!(prof[0], 1500);
        // (4*3*10)/10 + (20*10)/20 = 12 + 10
        assert_eq!(ai_proficiency_floor(&ctx, 1, 20), 22);
        assert!(can_promote(11, 0, 100, true, false));
        assert!(can_promote(10, 100, 100, true, false));
        assert!(!can_promote(10, 99, 100, true, false));
        assert!(!can_promote(20, 0, 100, false, false));
    }

    #[test]
    fn resist_curve_points() {
        assert_eq!(resist_curve(50), 99);
        assert_eq!(resist_curve(42), 98);
        assert_eq!(resist_curve(10), 90);
        assert_eq!(resist_curve(5), 85);
        assert_eq!(resist_curve(0), 80);
        assert_eq!(resist_curve(-10), 75);
        assert_eq!(resist_curve(-50), 50);
        assert_eq!(resist_curve(-100), 11);
        assert_eq!(resist_curve(-200), 1);
    }

    #[test]
    fn formula_table_offsets() {
        // spot-check against the switch in FUN_0046932f
        assert!(matches!(form(5), Form::LL(0x1a)));
        assert!(matches!(form(0xc), Form::LL(0x34)));
        assert!(matches!(form(0x15), Form::LL(0x52)));
        assert!(matches!(form(0x2a), Form::LL(0x92)));
        assert!(matches!(form(0x41), Form::Adeb(0xc2)));
        assert!(matches!(form(0x4a), Form::TargetHpL(0xd4)));
        assert!(matches!(form(0x4b), Form::Base));
    }
}
