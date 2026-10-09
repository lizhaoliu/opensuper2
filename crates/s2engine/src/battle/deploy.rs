//! Stage setup and unit creation from placement records
//! (`FUN_0040f67f`, `FUN_004231be`, `FUN_0043d787`, `FUN_0043dc8b`, `FUN_0043e018`).

use s2data::tbl_stage::Stage;

use super::army::{Side, UnitRef, MAX_UNITS};
use super::unit::{flag, Unit};
use super::{view, Battle, Placement};

/// Placement control types that are player-side (`npc = 0`).
fn ctrl_is_player(c: u8) -> bool {
    matches!(c, 0 | 5 | 6 | 7)
}

impl Battle {
    /// The stage-record part of FUN_00423033 / FUN_0040f67f: split the
    /// stage's placement records into the player prefix (up to the first
    /// record of side 2, 3 or 8) and the enemy rest, reset the turn, clear
    /// the enemy army. Nobody is placed (see `start_stage_placed`).
    pub fn start_stage(&mut self, stage: &Stage) {
        self.defs = stage
            .units
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let base = stage.header.units_offset as usize + 101 * i;
                let mut name_raw = [0u8; 13];
                if let Some(b) = stage.block.get(base + 2..base + 15) {
                    name_raw.copy_from_slice(b);
                }
                Placement { p: p.clone(), appear_to: stage.block.get(base + 0x34).map_or(0, |&b| b as i8), name_raw }
            })
            .collect();
        self.stage_kind = stage.header.kind;
        let n = self.defs.iter().position(|d| matches!(d.p.control, 2 | 3 | 8)).unwrap_or(self.defs.len());
        self.n_player_defs = n;
        self.n_enemy_defs = self.defs.len() - n;
        self.enemy_defs_start = n;
        self.turn = 1;
        self.event_count = 0;
        self.power_totals = [(0, 0, 0); 2];
        self.clear_army(Side::Enemy);
    }

    /// The save-load start of FUN_0040f67f (VS, and the tests' fixtures):
    /// `start_stage`, then the party on its records (FUN_00443482) and the
    /// army flags. A campaign stage (FUN_00423033) places nobody: the
    /// deployment decides who is on the map.
    pub fn start_stage_placed(&mut self, stage: &Stage) {
        self.start_stage(stage);
        self.place_party(Side::Player);
        self.recalc_army_flags(Side::Player, 1);
    }

    /// Port of FUN_00443482 after the save is read: un-deploy everyone, then
    /// put living, non-NPC, named members into the side's records in order.
    pub(crate) fn place_party(&mut self, side: Side) {
        let (start, count) = match side {
            Side::Player => (0, self.n_player_defs),
            Side::Enemy => (self.enemy_defs_start, self.n_enemy_defs),
        };
        let mut k = 0;
        for i in 0..self.army(side).units.len() {
            let r = UnitRef::new(side, i);
            let u = self.unit_mut(r);
            u.flags &= 0x7fff_ffff;
            if u.flags & (flag::SUMMON | flag::DEAD) == 0 && u.npc != 1 && u.char_id != 0 && k < count {
                let d = self.defs[start + k].p.clone();
                let u = self.unit_mut(r);
                u.x = d.x as i32;
                u.y = d.y as i32;
                u.facing = d.facing;
                u.reset_for_stage();
                u.flags |= flag::DEPLOYED;
                let p = self.power(self.unit(r), 0, 0);
                let t = &mut self.power_totals[side.index()];
                t.0 += p;
                t.1 = t.1.max(p);
                t.2 += 1;
                k += 1;
            }
        }
    }

    /// Whether record `i`'s appear window contains `turn` (FUN_004231be).
    fn appears(&self, i: usize, turn: i32) -> bool {
        let d = &self.defs[i];
        let from = d.p.b33 as i8 as i32;
        if d.appear_to == 0 {
            from == turn
        } else {
            from <= turn && turn <= d.appear_to as i32
        }
    }

    /// Port of FUN_004231be: deploy the player and enemy records whose appear
    /// window contains `turn`. Returns true if anything was deployed.
    pub fn deploy_turn(&mut self, turn: i32) -> bool {
        self.turn = turn;
        let mut any = false;
        let list: Vec<usize> = (0..self.n_player_defs).filter(|&i| self.appears(i, turn)).collect();
        if !list.is_empty() {
            any = true;
            // FUN_00441222 (erase both armies from the surface before the batch) and
            // FUN_004403b5 (redraw them): view::redeploy, after this returns.
            self.create_units(Side::Player, &list);
        }
        let list: Vec<usize> =
            (self.enemy_defs_start..self.enemy_defs_start + self.n_enemy_defs).filter(|&i| self.appears(i, turn)).collect();
        if !list.is_empty() {
            any = true;
            self.create_units(Side::Enemy, &list);
        }
        any
    }

    /// Port of FUN_0043d787: create units for records `list` (after
    /// FUN_0043dc8b moved already present named characters instead).
    pub fn create_units(&mut self, side: Side, list: &[usize]) -> Vec<UnitRef> {
        let list = self.filter_existing(side, list);
        let first = self.army(side).units.len();
        let mut out = Vec::new();
        for (k, &rec) in list.iter().enumerate() {
            if self.army(side).units.len() >= MAX_UNITS {
                break;
            }
            let u = Unit { slot: (first + k + 1) as i16, ..Unit::default() };
            self.army_mut(side).units.push(u);
            let r = UnitRef::new(side, first + k);
            self.init_from_def(r, rec);
            out.push(r);
        }
        // FUN_0043e83e (battle sprites) / FUN_0043fa24 (portraits): the view decodes a
        // unit's frame on first use (view::sync_sprites); the slots are kept for saves.
        if !out.is_empty() {
            self.slots_created(side, first);
        }
        out
    }

    /// Port of FUN_0043dc8b: named records whose character is already in
    /// the army reposition that unit (unless dead) and are dropped from the list.
    fn filter_existing(&mut self, side: Side, list: &[usize]) -> Vec<usize> {
        let mut keep = Vec::new();
        for &rec in list {
            let d = self.defs[rec].p.clone();
            if d.character == 0 {
                keep.push(rec);
                continue;
            }
            let found = self.army(side).units.iter().position(|u| u.char_id == d.character);
            match found {
                Some(i) => {
                    let r = UnitRef::new(side, i);
                    if !self.unit(r).is_dead() {
                        let (x, y) = self.displace(side, d.x as i32, d.y as i32);
                        let u = self.unit_mut(r);
                        u.x = x;
                        u.y = y;
                        u.flags |= flag::DEPLOYED;
                        u.facing = d.facing;
                        // FUN_00442022(army, unit, 0) redraw: automatic (the move rebakes).
                        if d.control == 7 {
                            u.flags &= !flag::RESERVE;
                        }
                        // The facing is written directly (no FUN_0043a1fe): the frame
                        // byte stays, so the view must not start the new facing's action
                        // (it would for a unit it has not seen since view::init).
                        let key = view::unit::frame_key(&view::look_of(self, r));
                        let units = &mut self.view.units[side.index()];
                        if units.len() <= i {
                            units.resize_with(i + 1, Default::default);
                        }
                        units[i].synced = Some(key);
                    }
                }
                None => {
                    if self.stage_kind != 3 || side != Side::Player || d.control != 0 || d.character == 0 {
                        keep.push(rec);
                    }
                }
            }
        }
        keep
    }

    /// Port of FUN_0043d9e9: create (or re-initialise) the unit for record `rec`.
    /// `reuse == 0` first re-initialises any unit with the record's character
    /// id (even 0, as the original compares unconditionally); otherwise a
    /// same-id unit is reused only for named characters. Dead matches give `None`.
    pub fn create_from_record(&mut self, side: Side, rec: usize, reuse: i32) -> Option<UnitRef> {
        let ch = self.defs.get(rec)?.p.character;
        if reuse == 0 {
            if let Some(i) = self.army(side).units.iter().position(|u| u.char_id == ch) {
                let r = UnitRef::new(side, i);
                if self.unit(r).is_dead() {
                    return None;
                }
                self.init_from_def(r, rec);
                let u = self.unit_mut(r);
                u.flags |= flag::DEPLOYED;
                u.flags &= !flag::RESERVE;
                return Some(r);
            }
        }
        if !self.army(side).units.is_empty() && reuse != 0 && ch > 0 {
            if let Some(i) = self.army(side).units.iter().position(|u| u.char_id == ch) {
                let r = UnitRef::new(side, i);
                if self.unit(r).is_dead() {
                    return None;
                }
                self.init_from_def(r, rec);
                // FUN_0043eb5f / FUN_0043fc56 reload: the view redecodes a unit whose
                // kind changed (view::sync_sprites); the slots are kept for saves.
                self.unit_mut(r).flags |= flag::DEPLOYED;
                self.slots_created_one(r);
                return Some(r);
            }
        }
        if self.army(side).units.len() >= MAX_UNITS {
            return None;
        }
        let slot = self.army(side).units.len() as i16 + 1;
        self.army_mut(side).units.push(Unit { slot, ..Unit::default() });
        let r = UnitRef::new(side, slot as usize - 1);
        self.init_from_def(r, rec);
        // FUN_0043eb5f / FUN_0043fc56 (sprite, portrait): decoded / loaded on first use.
        self.unit_mut(r).flags |= flag::DEPLOYED;
        self.slots_created_one(r);
        Some(r)
    }

    /// Port of FUN_0043e5dd(army, k, x, y, level): put summon creature `k`
    /// (army summon slot, class `summons[k].kind`) on (x, y). An existing
    /// creature of that class is reset and reused; otherwise a unit is built
    /// from a synthesized placement record (FUN_0043b612: class base stats
    /// plus a growth roll for each level above 1; level 1 on the player side
    /// and in VS). Marks the slot active.
    pub fn create_summon(&mut self, side: Side, k: usize, x: i32, y: i32, level: i32) -> Option<UnitRef> {
        let kind = self.army(side).summons.get(k)?.kind;
        let existing = self.army(side).units.iter().position(|u| u.kind == kind && u.flags & flag::SUMMON != 0);
        let r = match existing {
            Some(i) => {
                let r = UnitRef::new(side, i);
                let u = self.unit_mut(r);
                u.x = x;
                u.y = y;
                u.facing = (side == Side::Player) as u8;
                u.reset_for_stage();
                u.flags = flag::DEPLOYED | flag::SUMMON;
                r
            }
            None => {
                let t = self.t();
                let lvl = if self.vs || side == Side::Player { 1 } else { level };
                let base = t.growth.base.get(kind as usize - 1)?.clone();
                let class = t.kinds.class(kind as u16)?.clone();
                // Record order (+0x1b..): HP MP ST 魔力 反應 攻擊 防禦 劍術 戰技;
                // the class base and growth rolls are in growth order.
                let to_rec = |g: [i32; 9]| [g[0], g[1], g[2], g[7], g[5], g[3], g[4], g[8], g[6]];
                let b = base.stats.map(|v| v as i32);
                let mut stats = to_rec(b);
                for l in 2..=lvl.max(1) {
                    let g = to_rec(self.roll_growth(kind, l));
                    for (k, s) in stats.iter_mut().enumerate() {
                        *s = (*s + g[k]).clamp(0, if k == 0 { 5000 } else { 999 });
                    }
                }
                let e = base.equipment;
                let eq = [e[0], e[1], e[2], e[3], e[4], 0];
                let name = self.class_name_raw(kind as u16);
                let mut name_raw = [0u8; 13];
                let n = name.len().min(12);
                name_raw[..n].copy_from_slice(&name[..n]);
                let p = s2data::tbl_stage::UnitPlacement {
                    character: 0,
                    name: String::new(),
                    gender: 2,
                    class: kind,
                    level: lvl as i16,
                    ai_mode: 1,
                    tag: 0,
                    ai_sub: 0,
                    x: x as i16,
                    y: y as i16,
                    stats: stats.map(|v| v as i16),
                    alignment: class.alignment as i16,
                    ap: class.base_ap as i16,
                    control: side.index() as u8,
                    facing: (side == Side::Player) as u8,
                    b33: 0,
                    appear_to: 0,
                    kill_exp: 0,
                    drops: [0; 4],
                    items: eq,
                    skill_flags: 0,
                    spells: base.spells.map(|v| v as i16),
                };
                let pl = super::Placement { p, appear_to: 0, name_raw };
                let i = self.army(side).units.len();
                let u = Unit { flags: flag::SUMMON, ..Unit::default() };
                self.army_mut(side).units.push(u);
                let r = UnitRef::new(side, i);
                self.init_from_placement(r, &pl);
                let u = self.unit_mut(r);
                u.slot = i as i16 + 1;
                u.flags |= flag::DEPLOYED;
                r
            }
        };
        // FUN_0043eef6 / FUN_0043fc56 / FUN_00442022: sprite, portrait, draw (the view).
        self.slots_summon(r);
        self.army_mut(side).summons[k].active = 1;
        Some(r)
    }

    /// Port of FUN_00444496: if (x, y) is occupied by either army, the first
    /// free targetable cell on the rings around it (distance 1..10).
    pub fn displace(&self, side: Side, x: i32, y: i32) -> (i32, i32) {
        let occupied = |b: &Battle, x: i32, y: i32| {
            b.army(side).unit_at(x, y) != 0 || b.army(side.other()).unit_at(x, y) != 0
        };
        if !occupied(self, x, y) {
            return (x, y);
        }
        let m = self.map();
        let free = |cx: i32, cy: i32| {
            m.targetable(cx, cy) && !occupied(self, cx, cy) && !m.blocked_by_spot(cx, cy)
        };
        let mut lx = x;
        let mut cy = y;
        for c in 1..0xb {
            let l = if cy & 1 == 0 { lx - 1 } else { lx };
            lx = l;
            let top = cy - 1;
            for cx in l..=l + c {
                if free(cx, top) {
                    return (cx, top);
                }
                if free(cx, top + c * 2) {
                    return (cx, top + c * 2);
                }
            }
            let mut yy = cy + 1;
            while yy <= cy - 3 + c * 2 {
                if free(l, yy) {
                    return (l, yy);
                }
                if free(l + c, yy) {
                    return (l + c, yy);
                }
                yy += 2;
            }
            cy = top;
        }
        (x, y)
    }

    /// Port of FUN_0043e018: initialise unit `r` from placement record `rec`.
    pub fn init_from_def(&mut self, r: UnitRef, rec: usize) {
        let p = self.defs[rec].clone();
        self.init_from_placement(r, &p);
    }

    /// FUN_0043e018 for a record that is not in the stage list (summons).
    pub fn init_from_placement(&mut self, r: UnitRef, pl: &super::Placement) {
        let t = self.t();
        let d = pl.p.clone();
        super::trace!("init {:?} char {} class {} control {} at ({},{})", r, d.character, d.class, d.control, d.x, d.y);
        let mut raw_name = [0u8; 15];
        let src = &pl.name_raw;
        let n = src.iter().position(|&c| c == 0).unwrap_or(src.len());
        raw_name[..n].copy_from_slice(&src[..n]);
        let (x, y) = self.displace(r.side, d.x as i32, d.y as i32);
        let class = t.kinds.class(d.class as u16).cloned();
        {
            let u = self.unit_mut(r);
            u.x = x;
            u.y = y;
            u.npc = if ctrl_is_player(d.control) { 0 } else { 1 };
            if d.control == 7 {
                u.flags &= !flag::RESERVE;
            }
            u.name = raw_name;
            match d.control {
                3 | 9 => u.rank = 1,
                8 | 10 => u.rank = 2,
                _ => {}
            }
            u.char_id = d.character;
            u.sex = d.gender & 0xf;
            u.kind = d.class;
            u.flags |= flag::DEPLOYED;
            if d.control == 4 {
                u.flags |= flag::RESERVE;
            }
            u.act = 0;
            u.facing = d.facing;
            u.charm = if d.alignment == 0 { class.as_ref().map_or(0, |c| c.alignment) } else { d.alignment as u8 };
            u.ai_mode = d.ai_mode as i16;
            u.tag = d.tag as i16;
            u.ai_sub = d.ai_sub as i16;
            u.level = if d.level == 0 { 1 } else { d.level };
            let s = d.stats;
            u.hp_max = s[0];
            u.hp = s[0];
            u.mp_max = s[1];
            u.mp = s[1];
            u.st_max = s[2];
            u.st = s[2];
            u.mag = s[3];
            u.mag_cur = s[3];
            let ap = if d.ap < 1 { class.as_ref().map_or(0, |c| c.base_ap as i16) } else { d.ap };
            u.ap = ap;
            u.ap_base = ap;
            u.ap_max = ap;
            u.react = s[4];
            u.atk = s[5];
            u.atk_cur = s[5];
            u.def = s[6];
            u.def_cur = s[6];
            u.sword = s[7];
            u.skill = s[8];
            u.skill_cur = s[8];
            u.exp = 0;
            u.exp_next = class.as_ref().map_or(0, |c| c.exp_per_level);
            u.bounty = d.kill_exp as u16;
            u.abilities = d.skill_flags;
            for k in 0..4 {
                u.drops[k] = d.drops[k] as u16;
            }
        }
        for &id in &d.items {
            if id == 0 {
                continue;
            }
            let kind = t.items.get(id as usize - 1).map_or(0, |it| it.kind);
            match kind {
                1..=6 => self.unit_mut(r).equip[0] = id as u16,
                7..=11 => self.unit_mut(r).equip[kind as usize - 6] = id as u16,
                12 => self.army_mut(r.side).add_item(id as usize, 1),
                _ => {} // the original raises error 5
            }
        }
        self.apply_all_equipment(r);
        for k in 0..11 {
            self.unit_mut(r).learned[k] = d.spells[k] as u16;
        }
        self.learned_prof_floor(r);
        if let Some(c) = &class {
            if c.innate_skill != 0 {
                self.unit_mut(r).abilities |= 1u32 << ((c.innate_skill as u32 - 1) & 0x1f);
            }
        }
        self.npc_proficiency(r);
        self.army_stat_bonus(r);
    }

    /// Port of FUN_00446cd4: add the bonuses of all six equipped items.
    pub fn apply_all_equipment(&mut self, r: UnitRef) {
        for k in 0..6 {
            let id = self.unit(r).equip[k];
            self.apply_equipment(r, id);
        }
    }

    /// Port of FUN_00446d1a: add one item's ATK/DEF/MAG/AP bonus (with the
    /// original's caps, including its 999−AP quirk above 110).
    pub fn apply_equipment(&mut self, r: UnitRef, id: u16) {
        let mut u = self.unit(r).clone();
        self.add_item_stats(&mut u, id);
        *self.unit_mut(r) = u;
    }

    /// Port of FUN_00452c93: raise a school's proficiency to the level of
    /// every learned spell in it.
    pub fn learned_prof_floor(&mut self, r: UnitRef) {
        let t = self.t();
        let mut idx = 0usize;
        for cat in 0..t.spells.school_spells.len().min(11) {
            let cnt = t.spells.school_spells[cat].1 as usize;
            let mask = self.unit(r).learned[cat];
            if mask == 0 {
                idx += cnt;
                continue;
            }
            for c in 0..cnt {
                if let Some(sp) = t.spells.spells.get(idx) {
                    let u = self.unit_mut(r);
                    if mask as u32 & (1 << (c & 0x1f)) != 0 && ((u.prof[6 + cat] / 10) as i32) < sp.level as i32 {
                        u.prof[6 + cat] = (sp.level as i32 * 10) as u16;
                    }
                }
                idx += 1;
            }
        }
    }

    /// Port of FUN_00452af0: computer-controlled units get a weapon/magic
    /// proficiency floor (`rules::ai_proficiency_floor`) for each class action.
    pub fn npc_proficiency(&mut self, r: UnitRef) {
        let t = self.t();
        let u = self.unit(r);
        if self.vs || !(self.army(r.side).side != Side::Player || u.npc == 1) || self.stage_kind == 3 {
            return;
        }
        let Some(class) = t.kinds.class(u.kind as u16) else { return };
        let ctx = super::rules::Ctx::new(&t, self.difficulty, self.vs);
        let lvl = super::rules::ai_proficiency_floor(&ctx, u.kind as u16, u.level as i32);
        for a in class.actions.clone() {
            let kind = t.kinds.actions[a].kind as i8 as i32;
            let u = self.unit_mut(r);
            if !(1..=10).contains(&kind) {
                if kind != 0xb && kind != 0xc && (u.prof[17] as i32) < lvl {
                    u.prof[17] = lvl as u16;
                }
            } else if (2..8).contains(&kind) && (u.prof[(kind - 2) as usize] as i32) < lvl {
                u.prof[(kind - 2) as usize] = lvl as u16;
            }
        }
    }

    /// Port of FUN_00457d3c: the army's stat bonus % (army+0xe2) for new units.
    pub fn army_stat_bonus(&mut self, r: UnitRef) {
        let pct = self.army(r.side).stat_bonus_pct;
        if pct == 0 {
            return;
        }
        let u = self.unit_mut(r);
        let add = |v: i16| ((v as i32 * pct) / 100) as i16;
        let d = add(u.hp_max);
        u.hp_max = (u.hp_max + d).clamp(1, 5000);
        u.hp = (u.hp + d).clamp(1, 5000);
        let d = add(u.mp_max);
        u.mp_max = (u.mp_max + d).min(999);
        u.mp = (u.mp + d).min(999);
        let d = add(u.st_max);
        u.st_max = (u.st_max + d).min(999);
        u.st = (u.st + d).min(999);
        let d = add(u.atk);
        u.atk = (u.atk + d).clamp(1, 999);
        u.atk_cur = (u.atk_cur + d).clamp(1, 999);
        let d = add(u.def);
        u.def = (u.def + d).clamp(1, 999);
        u.def_cur = (u.def_cur + d).clamp(1, 999);
        let d = add(u.sword);
        u.sword = (u.sword + d).clamp(1, 999);
        let d = add(u.skill);
        u.skill = (u.skill + d).clamp(1, 999);
        u.skill_cur = (u.skill_cur + d).clamp(1, 999);
    }
}
