//! Game-level combat sequences: an attack with its counters, death, the HP
//! and EXP popups (docs/battle.md §5.7, §7, §9). State changes are `Battle`
//! methods (combat.rs, stats.rs); these functions keep the original order of
//! rolls, script hooks and presentation. Drawing goes through `battle::view`
//! and the FLOW toolkit; with no view assets loaded (headless runs) the
//! presentation and its waits are skipped and only the state changes run.

use super::army::UnitRef;
use super::combat::{Duel, TargetList};
use super::hooks::ev_ref;
use super::unit::{buff, flag, status};
use super::view;
use crate::event::{self, Trigger};
use crate::game::Game;
use crate::{gfx, text, ui};

/// Presentation runs only with the view and the UI resources loaded.
pub fn presenting(game: &Game) -> bool {
    game.res.is_some() && game.battle.view.assets.is_some() && game.battle.view.map.is_some()
}

/// FUN_00441a24: face `dir` (or advance one frame when already facing it).
pub fn face(game: &mut Game, r: UnitRef, dir: u8) {
    view::step_unit(game, r, dir);
    let u = game.battle.unit_mut(r);
    u.facing = (dir & 3) | (u.facing & 0xfc);
}

/// FUN_0043a1fe / FUN_00441ac4: animation set `set`, keeping the direction.
pub fn set_anim(game: &mut Game, r: UnitRef, set: u8) {
    view::set_anim(game, r, set);
    let u = game.battle.unit_mut(r);
    u.facing = (set << 2) | (u.facing & 3);
}

/// One argument of [`sprintf`].
pub enum Arg<'a> {
    S(&'a [u8]),
    D(i32),
}

/// `FUN_0047a1ce` (sprintf) for the conversions the battle strings use:
/// `%s`, `%d`, `%Nd` and `% Nd`.
pub fn sprintf(fmt: &[u8], args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut it = args.iter();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' || i + 1 >= fmt.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let space = fmt[j] == b' ';
        if space {
            j += 1;
        }
        let mut width = 0usize;
        while j < fmt.len() && fmt[j].is_ascii_digit() {
            width = width * 10 + (fmt[j] - b'0') as usize;
            j += 1;
        }
        match (fmt.get(j), it.next()) {
            (Some(b's'), Some(Arg::S(s))) => out.extend_from_slice(s.split(|&b| b == 0).next().unwrap_or(&[])),
            (Some(b'd'), Some(Arg::D(v))) => {
                let s = if space && *v >= 0 { format!(" {v}") } else { v.to_string() };
                out.extend(std::iter::repeat_n(b' ', width.saturating_sub(s.len())));
                out.extend_from_slice(s.as_bytes());
            }
            _ => {}
        }
        i = j + 1;
    }
    out
}

/// `FUN_004198a4` + `FUN_0046d2da`, the pump at the top of the original's
/// loops: the CD-audio check (nothing to do here) and the palette cycling
/// (water and the like), which presents when a range turns.
pub fn pump(game: &mut Game) {
    if presenting(game) {
        gfx::palette_cycle(game);
    }
}

/// Port of FUN_00439cc0: the rising popup over a unit (7 steps of 3 cs):
/// "MISS" for 0, the damage `v` > 0, a heal −`v` (shown positive).
pub fn show_miss(game: &mut Game, r: UnitRef, v: i32) {
    {
        let u = game.battle.unit_mut(r);
        u.hp_before_hit = u.hp;
    }
    if !presenting(game) {
        return;
    }
    let (x, y) = {
        let u = game.battle.unit(r);
        (u.x, u.y)
    };
    let (mut sx, mut sy) = view::cell_screen(game, x, y);
    // DAT_0048ec44 / DAT_0048ec4c: the miss text and a number format.
    let (s, fg, c2) = if v == 0 {
        (b"MISS".to_vec(), 0x68, 0x24)
    } else if v < 0 {
        (sprintf(b"%d", &[Arg::D(-v)]), 0x24, 0xbe)
    } else {
        (sprintf(b"%d", &[Arg::D(v)]), 0x3c, 0x56)
    };
    sx += 0x20 - s.len() as i32 * 8;
    sy -= 0x40;
    let mut t = game.ticks();
    for _ in 0..7 {
        pump(game);
        game.scr.copy_page(0, 1);
        text::large(game, sx, sy, &s, fg, c2, 1, 0);
        game.wait_until(t, 3);
        gfx::present(game);
        t = game.ticks();
        sy -= 8;
    }
    game.scr.copy_page(0, 1);
}

/// Port of FUN_00439e2a: apply `*dmg` with the rising number popup. Mode 1
/// damages `b` (a negative value drains: `a` heals the same), mode 0 damages
/// `a` (a counter), mode 2 heals `b`. `*dmg` becomes the amount applied.
/// `duel` is the exchange whose panel (FUN_00452215) is redrawn every frame.
pub fn show_hp(game: &mut Game, a: UnitRef, b: UnitRef, dmg: &mut i32, mode: i32, duel: Option<&Duel>) {
    let before = (game.battle.unit(a).hp, game.battle.unit(b).hp);
    let drain = *dmg < 0;
    let v = game.battle.apply_hp(a, b, *dmg, mode);
    *dmg = v;
    if !presenting(game) {
        return;
    }
    // The panel shows the HP counted down chunk by chunk: replay the change.
    let after = (game.battle.unit(a).hp, game.battle.unit(b).hp);
    game.battle.unit_mut(a).hp = before.0;
    game.battle.unit_mut(b).hp = before.1;
    popup(game, if mode == 0 { a } else { b }, v, mode, duel, Some((a, b, drain)));
    game.battle.unit_mut(a).hp = after.0;
    game.battle.unit_mut(b).hp = after.1;
}

/// The presentation of FUN_00439e2a for an amount already applied: the
/// number rising over `at` (none for heals, mode 2) in the HP-chunk timing.
pub fn hp_popup(game: &mut Game, at: UnitRef, v: i32, mode: i32, duel: Option<&Duel>) {
    popup(game, at, v, mode, duel, None);
}

/// FUN_00439e2a's frames. With `count` = (a, b, drain) the HP is changed
/// by each chunk as in the original (mode 1: `b` loses it and, draining,
/// `a` gains it up to its maximum; 0: `a` loses it; 2: `b` gains it).
fn popup(game: &mut Game, at: UnitRef, v: i32, mode: i32, duel: Option<&Duel>, count: Option<(UnitRef, UnitRef, bool)>) {
    if !presenting(game) {
        return;
    }
    let (x, y) = {
        let u = game.battle.unit(at);
        (u.x, u.y)
    };
    let (mut sx, mut sy) = view::cell_screen(game, x, y);
    let s = sprintf(b"%d", &[Arg::D(v)]);
    sx += 0x20 - s.len() as i32 * 8;
    sy -= 0x40;
    // The HP counts down in 1, 2, 4, 8 .. chunks while the number rises;
    // the state is already final, so only the timing is reproduced.
    let mut left = v;
    let mut i = 0;
    let mut wait = 3;
    let mut t = game.ticks();
    while left > 0 || i < 8 {
        pump(game);
        if left != 0 {
            let chunk = (1i32 << i.min(30)).min(left);
            left -= chunk;
            if let Some((a, b, drain)) = count {
                let c = chunk as i16;
                match mode {
                    1 => {
                        game.battle.unit_mut(b).hp -= c;
                        let u = game.battle.unit_mut(a);
                        if drain && u.hp < u.hp_max {
                            u.hp = (u.hp + c).min(u.hp_max);
                        }
                    }
                    0 => game.battle.unit_mut(a).hp -= c,
                    _ => {
                        let u = game.battle.unit_mut(b);
                        u.hp = (u.hp + c).min(u.hp_max);
                    }
                }
            }
        }
        game.scr.copy_page(0, 1);
        if i < 7 && mode != 2 {
            text::large(game, sx, sy, &s, 0x3c, 0x56, 1, 0);
        }
        if let Some(d) = duel {
            view::panel::attack_forecast(game, d);
        }
        if i < 6 {
            game.wait_until(t, wait);
        }
        gfx::present(game);
        t = game.ticks();
        i += 1;
        if i == 3 {
            wait = 2;
        } else if i > 3 {
            wait = 1;
        }
        if i < 8 {
            sy -= 8;
        }
    }
    game.scr.copy_page(0, 1);
}

impl super::Battle {
    /// Port of FUN_00439483: special attack animation of an action (weapon
    /// item +0x1c overrides action +0x14 for weapon kinds 1..=7).
    pub fn special_anim(&self, r: UnitRef, skill: usize) -> i32 {
        let Some(a) = self.tables.as_ref().and_then(|t| t.kinds.actions.get(skill).cloned()) else { return 0 };
        let k = a.kind as i8;
        let w = self.unit(r).equip[0];
        if (1..=7).contains(&k) && w != 0 {
            if let Some(it) = self.item(w) {
                if it.anim >= 1 {
                    return it.anim as i32;
                }
            }
        }
        a.anim as i32
    }

    /// FUN_00439ba8's effect id: weapon item +0x1d, else action +0x15.
    pub fn hit_effect_id(&self, r: UnitRef, skill: usize) -> usize {
        let Some(a) = self.tables.as_ref().and_then(|t| t.kinds.actions.get(skill).cloned()) else { return 0 };
        let k = a.kind as i8;
        let w = self.unit(r).equip[0];
        if (1..=7).contains(&k) && w != 0 {
            if let Some(it) = self.item(w) {
                if it.sound != 0 {
                    return it.sound as usize;
                }
            }
        }
        a.sound as u8 as usize
    }

    /// Attack sound of an action (action +0x0f, FUN_00439c83).
    pub fn action_sound(&self, skill: usize) -> u16 {
        self.tables.as_ref().and_then(|t| t.kinds.actions.get(skill).map(|a| a.motion.0 as i16 as u16)).unwrap_or(0)
    }

    /// The animation set an attack plays (`facing = (facing & 3) + set * 4`):
    /// action byte +0x0a, except that weapon 0x53 keeps the current set for
    /// weapon actions (classes other than 0x50).
    pub fn attack_set(&self, r: UnitRef, skill: usize) -> u8 {
        let u = self.unit(r);
        let k = self.action_kind(skill);
        if !(1..=10).contains(&k) || u.equip[0] != 0x53 || u.kind == 0x50 {
            self.action_set(skill).unwrap_or(2)
        } else {
            u.facing >> 2
        }
    }
}

/// Port of FUN_00439ba8: the hit effect on the target (with 動畫播放 on).
fn hit_effect(game: &mut Game, att: UnitRef, target: UnitRef, skill: usize) {
    if !game.cfg.battle_anim || !presenting(game) {
        return;
    }
    let id = game.battle.hit_effect_id(att, skill);
    if id != 0 {
        let (x, y) = {
            let u = game.battle.unit(target);
            (u.x, u.y)
        };
        let _ = view::play_effect(game, id, x, y, false);
    }
}

/// Port of FUN_00439c83: the swing sound (action +0x0f on a hit, 0x6f on a miss).
fn attack_sound(game: &mut Game, skill: usize, hit: bool) {
    let id = if hit { game.battle.action_sound(skill) } else { 0x6f };
    ui::sound(game, id);
}

/// Port of FUN_00439503: special attack animations 1..=9 (projectiles).
fn special_attack(game: &mut Game, att: UnitRef, target: UnitRef, skill: usize, kind: i32) {
    let (effect, mode, snd): (usize, i32, u16) = match kind {
        1 => (0x2b, 1, 0),
        2 => (0x3b, 1, 0),
        3 => (0x4b, 1, 0),
        4 => (0x5b, 1, 0),
        5 => (0x6b, 1, 0x7f),
        6 => (0x7b, 2, 0),
        7 => (0x7c, 2, 0),
        8 => (0x7d, 2, 0),
        9 => (0x7e, 1, 0),
        _ => return,
    };
    if presenting(game) {
        game.scr.copy_page(1, 0);
    }
    let snd = if snd == 0 { game.battle.action_sound(skill) } else { snd };
    ui::sound(game, snd);
    if presenting(game) {
        view::motion::projectile(game, att, target, effect, mode);
    }
}

/// The attack frame loop of FUN_00436e46 / FUN_00437b9c: the attacker
/// plays its attack set (FUN_0043a1fe) at 12 cs a frame; without a special
/// animation, from two frames before the set ends (FUN_0043a1b0(frame + 2)
/// = 0) the units in `shaken` are drawn shaking (flags 0xc0000000, their
/// frame redecoded by FUN_00441a24) until the last frame, then they and the
/// attacker return to idle.
/// One presentation frame of FUN_00436e46 / FUN_00437b9c: page 2 (the
/// window without the units lifted off it, FUN_00441e09) to page 0, then
/// `order` drawn one by one with FUN_0043ff88, not by depth. Each is cut by
/// the units in front in the state the original has them in at that point
/// (`acts`, their +0x2d; act 5 here only keeps them off the surface), so a
/// shaking unit's cut stays where it stands unshaken.
fn draw_combatants(game: &mut Game, order: &[UnitRef], acts: &[(UnitRef, u8)]) {
    game.scr.copy_page(0, 2);
    let saved: Vec<(UnitRef, u8)> = acts.iter().map(|&(r, _)| (r, game.battle.unit(r).act)).collect();
    for &(r, a) in acts {
        game.battle.unit_mut(r).act = a;
    }
    for &r in order {
        view::draw_unit(game, r, 0);
    }
    for (r, a) in saved {
        game.battle.unit_mut(r).act = a;
    }
}

/// `Battle::apply_on_hit` (FUN_00438c92), with its FUN_00440264 for each
/// target it turned to stone.
fn apply_on_hit(game: &mut Game, att: UnitRef, targets: &[UnitRef], dmg: &mut [i32], skill: usize) {
    let stone = |game: &Game, r: UnitRef| game.battle.unit(r).buffs & buff::PETRIFIED != 0;
    let before: Vec<bool> = targets.iter().map(|&r| stone(game, r)).collect();
    game.battle.apply_on_hit(att, targets, dmg, skill);
    for (k, &r) in targets.iter().enumerate() {
        if !before[k] && stone(game, r) {
            view::put_unit(game, r);
        }
    }
}

/// The swing frames of FUN_00436e46 / FUN_00437b9c: `all` (the struck
/// units, of which `shaken` shake) drawn, then the attacker, with the
/// states `acts` (see [`draw_combatants`]).
#[allow(clippy::too_many_arguments)]
fn swing(game: &mut Game, att: UnitRef, set: u8, shaken: &[UnitRef], all: &[UnitRef], acts: &[(UnitRef, u8)], special: i32, skill: usize, any_hit: bool) {
    let saved = game.battle.unit(att).facing;
    set_anim(game, att, set);
    if special == 0 {
        attack_sound(game, skill, any_hit);
    }
    if presenting(game) {
        loop {
            pump(game);
            if special == 0 && view::frame_ahead(game, att, 2) == 0 {
                for &t in shaken {
                    game.battle.unit_mut(t).render_flags |= 0xc000_0000;
                    let f = game.battle.unit(t).facing;
                    face(game, t, f);
                }
            }
            let done = view::frame_ahead(game, att, 0) == 0;
            if done {
                game.battle.unit_mut(att).facing = saved;
                if special == 0 {
                    for &t in shaken {
                        game.battle.unit_mut(t).render_flags &= !0xc000_0000;
                        set_anim(game, t, 0);
                    }
                }
                set_anim(game, att, 0);
            } else {
                view::step_frame(game, att);
            }
            game.scr.set_page(0);
            let order: Vec<UnitRef> = all.iter().copied().chain(std::iter::once(att)).collect();
            draw_combatants(game, &order, acts);
            let t0 = game.battle.unit(att).anim_timer;
            game.wait_until(t0, 0xc);
            gfx::present(game);
            game.battle.unit_mut(att).anim_timer = game.ticks();
            if done {
                break;
            }
        }
    }
    game.battle.unit_mut(att).facing = saved;
    set_anim(game, att, 0);
}

/// Port of FUN_00436e46: `att` attacks with `duel` (set up by the caller
/// with `Battle::duel`, like the global `DAT_00492b08` the forecast window
/// shows) the unit `duel.def`, which is target `ti` of `tl`. Piercing shapes
/// also hit every enemy on the ray before it, each with its own duel.
pub fn attack(game: &mut Game, tl: &TargetList, ti: usize, duel: Duel) {
    let att = duel.att;
    let main = duel.def;
    let skill = duel.skill;
    let (att_char, def_char) = (game.battle.unit(att).char_id, game.battle.unit(main).char_id);
    event::poll(game, Trigger::BattleTalk { attacker: ev_ref(att), attacker_char: att_char, defender_char: def_char });
    let mut targets = Vec::new();
    let mut duels = Vec::new();
    if tl.shape != 1 && tl.shape != 2 {
        let (start, len) = tl.ray_of(ti);
        for k in 0..len {
            let (x, y) = tl.cells[start + k];
            if let Some(r) = game.battle.unit_at_cell(att.side.other(), x, y, false) {
                if r == main {
                    break;
                }
                let d = game.battle.duel(att, r, skill);
                targets.push(r);
                duels.push(d);
            }
        }
    }
    targets.push(main);
    duels.push(duel);
    let n = targets.len();
    // Page 2: the window (FUN_00424c5f) with each target erased from it
    // (FUN_00441e09) as it is lifted; the surface keeps them.
    if presenting(game) {
        game.scr.set_page(2);
        view::motion::draw_window(game);
    }
    let mut dmg = vec![0i32; n];
    let mut crit = vec![false; n];
    let mut any_hit = false;
    let mut acts = vec![0u8; n];
    for k in 0..n {
        acts[k] = game.battle.unit(targets[k]).act;
        game.battle.unit_mut(targets[k]).act = 5;
        if presenting(game) {
            view::erase_on_page(game, targets[k]);
        }
        let b = &mut game.battle;
        dmg[k] = b.roll_attack(&mut duels[k]);
        crit[k] = b.roll_crit(&duels[k]);
        if b.roll_instant_death(&duels[k]) {
            dmg[k] = b.unit(targets[k]).hp as i32;
            crit[k] = false;
        }
        if crit[k] {
            dmg[k] += b.roll_attack(&mut duels[k]);
            if dmg[k] == 0 {
                crit[k] = false;
            }
        }
        if dmg[k] > 0 {
            any_hit = true;
        }
    }
    for k in 0..n {
        game.battle.unit_mut(targets[k]).act = acts[k];
    }
    let any_crit = crit.iter().any(|&c| c);
    if presenting(game) {
        game.scr.set_page(0);
    }
    let special = game.battle.special_anim(att, skill);
    let set = game.battle.attack_set(att, skill);
    // Lifted off the map surface for the presentation (the original's FUN_00441e09).
    let mut lifted: Vec<(UnitRef, u8)> = Vec::new();
    for &u in std::iter::once(&att).chain(targets.iter()) {
        if !lifted.iter().any(|&(v, _)| v == u) {
            lifted.push((u, game.battle.unit(u).act));
            game.battle.unit_mut(u).act = 5;
        }
    }
    // FUN_00436e46 restores the targets' state after the rolls and leaves the
    // attacker's: they cover each other (and the attacker) where they stand.
    let states = lifted.clone();
    for pass in 0..=(any_crit as usize) {
        let shaken: Vec<UnitRef> = (0..n).filter(|&k| pass == 0 || crit[k]).map(|k| targets[k]).collect();
        swing(game, att, set, &shaken, &targets, &states, special, skill, any_hit);
        if special > 0 {
            special_attack(game, att, main, skill, special);
        }
    }
    apply_on_hit(game, att, &targets, &mut dmg, skill);
    super::trace!("attack {:?} -> {:?} skill {} dmg {:?} crit {:?}", att, targets, skill, dmg, crit);
    for k in 0..n {
        let t = targets[k];
        pump(game);
        if presenting(game) {
            let order: Vec<UnitRef> = targets.iter().copied().chain(std::iter::once(att)).collect();
            draw_combatants(game, &order, &states);
            game.scr.copy_page(1, 0);
        }
        hit_effect(game, att, t, skill);
        // The original lifts the targets off the map surface (FUN_00441e09) but
        // keeps their state (+0x2d restored after the rolls), so the other
        // targets in front cover the flash, the popup and the death.
        for &(u, a) in lifted.iter().filter(|&&(u, _)| u != att) {
            game.battle.unit_mut(u).act = a;
        }
        let saved_act = game.battle.unit(att).act;
        game.battle.unit_mut(att).act = 0;
        view::hit_flash(game, t);
        {
            let u = game.battle.unit_mut(t);
            if u.ai_mode == 2 {
                u.ai_mode = 1;
                u.ai_sub = 0;
            }
        }
        if dmg[k] == 0 {
            show_miss(game, t, dmg[k]);
        } else {
            show_hp(game, att, t, &mut dmg[k], 1, Some(&duels[k]));
        }
        game.battle.unit_mut(att).act = saved_act;
        if game.battle.unit(t).hp == 0 {
            let tu = game.battle.unit(t).clone();
            if game.battle.has_item_ability(&tu, 0x19) {
                let mut v = tu.hp_max as i32;
                show_hp(game, att, t, &mut v, 2, Some(&duels[k]));
            } else if game.battle.has_item_ability(&tu, 0x1c) {
                let mut v = tu.hp_max as i32;
                show_hp(game, att, t, &mut v, 2, Some(&duels[k]));
                game.battle.consume_equipped(t, 0x1c);
            } else {
                // FUN_00441c0b: off the map surface before the attacker's act
                // is cleared for FUN_0044d14b.
                view::erase_unit(game, t);
                let saved_act = game.battle.unit(att).act;
                game.battle.unit_mut(att).act = 0;
                die(game, t, 0);
                game.battle.unit_mut(att).act = saved_act;
                game.battle.unit_mut(att).kills += 1;
            }
        } else {
            let tu = game.battle.unit(t).clone();
            if game.battle.has_item_ability(&tu, 0x1d) && game.battle.rng.pct() < 0x1e {
                let mut v = dmg[k] << 1;
                show_hp(game, att, t, &mut v, 2, Some(&duels[k]));
            }
        }
        for &(u, _) in lifted.iter().filter(|&&(u, _)| u != att) {
            game.battle.unit_mut(u).act = 5;
        }
    }
    for (u, a) in lifted {
        game.battle.unit_mut(u).act = a;
    }
    counters(game, &targets, &duels, att);
    if game.battle.unit(att).flags & flag::DEAD == 0 {
        if presenting(game) {
            game.scr.copy_page(1, 0);
        }
        let n_items = game.battle.army(att.side).inventory.len();
        let mut items = vec![0u8; n_items];
        let mut exp = 0;
        for k in 0..n {
            exp += game.battle.exp_gain(duels[k].att, duels[k].def, dmg[k]);
            game.battle.kill_bonus(att, targets[k], &mut exp, &mut items);
        }
        if presenting(game) {
            game.scr.copy_page(1, 0);
        }
        let kind = game.battle.action_kind(skill);
        super::Battle::proficiency_gain(game.battle.unit_mut(att), kind);
        award(game, att, exp, &items);
        level_check(game, att);
    }
}

/// Port of FUN_00437b9c: every target that survived, can act and has a
/// counter action strikes back at `att` in order (piercing counters also hit
/// the attacker's allies on the ray before it). Stops once `att` is dead.
/// EXP and drops go to each counter-attacker at the end.
fn counters(game: &mut Game, targets: &[UnitRef], duels: &[Duel], att: UnitRef) {
    let n = targets.len();
    let n_items = game.battle.army(targets[0].side).inventory.len();
    let mut exp = vec![0i32; n];
    let mut items = vec![vec![0u8; n_items]; n];
    for f in 0..n {
        let c = targets[f];
        let Some(ctr) = duels[f].counter else { continue };
        {
            let u = game.battle.unit(c);
            if u.hp == 0 || u.buffs & super::unit::buff::PETRIFIED != 0 || u.status & status::INCAPACITATED != 0 {
                continue;
            }
        }
        // FUN_00441c0b: the counter-attacker leaves the map surface (the
        // units redrawn around it include an attacker whose act a previous
        // counter cleared).
        view::erase_unit(game, c);
        let tl = game.battle.target_list(c, ctr);
        let dir = {
            let (u, a) = (game.battle.unit(c), game.battle.unit(att));
            super::movement::facing_toward(u.x, u.y, a.x, a.y, u.facing)
        };
        face(game, c, dir);
        let special = game.battle.special_anim(c, ctr);
        let mut hit = Vec::new();
        let mut ds = Vec::new();
        if tl.shape != 1 && tl.shape != 2 {
            let (ax, ay) = (game.battle.unit(att).x, game.battle.unit(att).y);
            let ti = tl.target_at(ax, ay);
            let (start, len) = tl.ray_of(ti.wrapping_sub(1));
            for k in 0..len {
                let (x, y) = tl.cells[start + k];
                if let Some(r) = game.battle.unit_at_cell(c.side.other(), x, y, false) {
                    if r == att {
                        break;
                    }
                    let d = game.battle.duel(c, r, ctr);
                    hit.push(r);
                    ds.push(d);
                }
            }
        }
        hit.push(att);
        ds.push(duels[f]);
        let m = hit.len();
        if presenting(game) {
            game.scr.set_page(2);
            view::motion::draw_window(game);
        }
        let mut dmg = vec![0i32; m];
        let mut any_hit = false;
        let mut acts = vec![0u8; m];
        for k in 0..m {
            let is_counter = k == m - 1;
            if !is_counter {
                acts[k] = game.battle.unit(hit[k]).act;
                game.battle.unit_mut(hit[k]).act = 5;
                if presenting(game) {
                    view::erase_on_page(game, hit[k]);
                }
            }
            let b = &mut game.battle;
            let cr;
            if is_counter {
                dmg[k] = b.roll_counter(&ds[k]);
                cr = b.roll_crit_counter(&ds[k]);
            } else {
                dmg[k] = b.roll_attack(&mut ds[k]);
                cr = b.roll_crit(&ds[k]);
            }
            if dmg[k] > 0 {
                any_hit = true;
            }
            if cr {
                dmg[k] += if is_counter { b.roll_counter(&ds[k]) } else { b.roll_attack(&mut ds[k]) };
            }
        }
        if presenting(game) {
            view::erase_on_page(game, c);
        }
        for k in 0..m - 1 {
            game.battle.unit_mut(hit[k]).act = acts[k];
        }
        if presenting(game) {
            game.scr.set_page(0);
        }
        let set = game.battle.attack_set(c, ctr);
        let saved: Vec<u8> = hit.iter().map(|&r| game.battle.unit(r).act).collect();
        let c_act = game.battle.unit(c).act;
        // The original zeroes their act byte after lifting them off the surface;
        // here act 5 keeps them drawn on the page during the presentation.
        for &r in hit.iter().chain(std::iter::once(&c)) {
            game.battle.unit_mut(r).act = 5;
        }
        // FUN_00437b9c zeroes the struck units' state for the swing; the
        // counter-attacker keeps its own.
        let swing_states: Vec<(UnitRef, u8)> = hit.iter().map(|&r| (r, 0)).chain(std::iter::once((c, c_act))).collect();
        swing(game, c, set, &hit, &hit, &swing_states, special, ctr, any_hit);
        // Then the others get theirs back and the attacker stays at 0.
        let mut hit_states: Vec<(UnitRef, u8)> = (0..m - 1).map(|j| (hit[j], saved[j])).collect();
        hit_states.push((att, 0));
        hit_states.push((c, c_act));
        if special > 0 {
            special_attack(game, c, att, ctr, special);
        }
        apply_on_hit(game, c, &hit, &mut dmg, ctr);
        for k in 0..m {
            let t = hit[k];
            if presenting(game) {
                let order: Vec<UnitRef> = std::iter::once(c).chain(hit.iter().copied()).collect();
                draw_combatants(game, &order, &hit_states);
                game.scr.copy_page(1, 0);
            }
            hit_effect(game, c, t, ctr);
            // FUN_00437b9c only lifts the units off the surface: the counter-
            // attacker keeps its state, the others get theirs back and the
            // attacker stays at 0, so they cover the flash, popup and death.
            for j in 0..m - 1 {
                game.battle.unit_mut(hit[j]).act = saved[j];
            }
            game.battle.unit_mut(att).act = 0;
            game.battle.unit_mut(c).act = c_act;
            view::hit_flash(game, t);
            if dmg[k] == 0 {
                show_miss(game, t, dmg[k]);
            } else if k == m - 1 {
                show_hp(game, t, c, &mut dmg[k], 0, Some(&ds[k]));
            } else {
                show_hp(game, c, t, &mut dmg[k], 1, Some(&ds[k]));
            }
            if game.battle.unit(t).hp == 0 {
                let tu = game.battle.unit(t).clone();
                if game.battle.has_item_ability(&tu, 0x19) {
                    game.battle.unit_mut(t).hp = tu.hp_max;
                } else if game.battle.has_item_ability(&tu, 0x1c) {
                    game.battle.unit_mut(t).hp = tu.hp_max;
                    game.battle.consume_equipped(t, 0x1c);
                } else {
                    view::erase_unit(game, t);
                    die(game, t, 0);
                    game.battle.unit_mut(c).kills += 1;
                }
            }
            for &r in hit.iter().chain(std::iter::once(&c)) {
                game.battle.unit_mut(r).act = 5;
            }
        }
        for k in 0..m {
            // FUN_00408fa2 (counter: the counter-attacker is the duel's
            // defender) / FUN_00408f7b (its own piercing duels).
            exp[f] += if k == m - 1 {
                game.battle.exp_gain(ds[k].def, ds[k].att, dmg[k])
            } else {
                game.battle.exp_gain(ds[k].att, ds[k].def, dmg[k])
            };
            game.battle.kill_bonus(c, hit[k], &mut exp[f], &mut items[f]);
        }
        for k in 0..m - 1 {
            game.battle.unit_mut(hit[k]).act = saved[k];
        }
        // The attacker's act byte stays 0, as in the original.
        game.battle.unit_mut(att).act = 0;
        game.battle.unit_mut(c).act = c_act;
        // FUN_00440264: back into the surface.
        view::put_unit(game, c);
        view::bake(game);
        let kind = game.battle.action_kind(ctr);
        super::Battle::proficiency_gain(game.battle.unit_mut(c), kind);
        if game.battle.unit(att).hp == 0 {
            break;
        }
    }
    if presenting(game) {
        game.scr.copy_page(1, 0);
    }
    for f in 0..n {
        if exp[f] != 0 {
            award(game, targets[f], exp[f], &items[f]);
            level_check(game, targets[f]);
            if presenting(game) {
                game.scr.copy_page(0, 1);
            }
        }
    }
}

/// Port of FUN_0044d14b: `r` is defeated. Runs the unit-down script hook
/// (`FUN_00417f26`): 0x11 = the script took the unit away (retreat message),
/// 0x13 = withdrawn with a flash; otherwise the death sound and dissolve.
/// Reserves (hidden) only leave. Frees a summon's slot and recomputes the
/// army flags. `mode` 0/2 lift the unit off the map surface first.
pub fn die(game: &mut Game, r: UnitRef, mode: i32) {
    game.battle.unit_mut(r).deaths += 1;
    super::trace!("die {:?} char {}", r, game.battle.unit(r).char_id);
    if (mode == 0 || mode == 2) && presenting(game) {
        // FUN_00441e09 on page 0, then page 0 to page 1.
        view::erase_on_page(game, r);
        game.scr.copy_page(1, 0);
    }
    let char_id = game.battle.unit(r).char_id;
    let code = event::poll(game, Trigger::UnitDown { unit: ev_ref(r), char_id }).code;
    if code != 0 && presenting(game) {
        game.scr.set_page(1);
        view::motion::draw_window(game);
        game.scr.set_page(0);
    }
    if game.battle.unit(r).flags & flag::RESERVE == 0 && code != 0x11 {
        if code == 0x13 {
            if presenting(game) {
                game.scr.copy_page(0, 1);
                view::draw_unit(game, r, 0);
            }
            view::flash_unit(game, r, 0xfe, false);
            let u = game.battle.unit_mut(r);
            u.flags &= !flag::DEPLOYED;
            u.flags |= flag::DEAD;
        } else {
            ui::sound(game, 0x8d);
            view::fade_unit(game, r, 0x3c, false);
            game.battle.unit_mut(r).flags |= flag::DEAD;
            if presenting(game) {
                // FUN_0044007a x2: the units around it redrawn on page 1.
                let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
                game.scr.set_page(1);
                view::draw_around(game, r.side, x, y, false);
                view::draw_around(game, r.side.other(), x, y, false);
                game.scr.set_page(0);
                game.scr.copy_page(0, 1);
                gfx::present(game);
            }
        }
    } else {
        if code == 0x11 && presenting(game) {
            gfx::present_page(game, 1);
        }
        retreat_message(game, r);
        game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
    }
    if game.battle.unit(r).flags & flag::SUMMON != 0 {
        let kind = game.battle.unit(r).kind;
        let a = game.battle.army_mut(r.side);
        if let Some(s) = a.summons.iter_mut().find(|s| s.kind == kind) {
            s.active = 0;
        }
    }
    game.battle.recalc_army_flags(r.side, 0);
}

/// Port of FUN_0044d384: "<name> <msg 0x70>" box, 40 cs, then the unit
/// flashes away.
fn retreat_message(game: &mut Game, r: UnitRef) {
    if presenting(game) {
        let name = game.battle.unit(r).name.to_vec();
        let m = game.msg(0x70);
        let s = sprintf(b"%s %s", &[Arg::S(&name), Arg::S(&m)]);
        ui::message_box(game, 0xb0, 0x50, &s, ui::Wait::None);
        ui::click_to_continue(game, 0x28);
        game.scr.copy_page(0, 1);
        view::draw_unit(game, r, 0);
    }
    view::flash_unit(game, r, 0xfe, false);
}

/// Port of FUN_00452e38: award EXP (scaled, see `Battle::award_exp`) and
/// items. Player-side gains are shown in a window over the unit; the
/// enemy's are silent.
pub fn award(game: &mut Game, r: UnitRef, exp: i32, items: &[u8]) {
    let silent = game.battle.army(r.side).side != super::Side::Player && !game.battle.vs;
    let gained = game.battle.award_exp(r, exp, items);
    if silent || !presenting(game) {
        return;
    }
    view::panel::reward_window(game, r, gained, None, items);
}

/// Port of FUN_00453725 with the level-up presentation of FUN_004537da.
pub fn level_check(game: &mut Game, r: UnitRef) {
    game.battle.check_level_up(r);
    show_level_ups(game);
}

/// FUN_00453725's windows for unit `r` alone (its entries leave the log).
pub fn show_level_ups_of(game: &mut Game, r: UnitRef) {
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut game.battle.level_log).into_iter().partition(|g| g.unit == r);
    game.battle.level_log = rest;
    let keep = std::mem::take(&mut game.battle.level_log);
    game.battle.level_log = mine;
    show_level_ups(game);
    game.battle.level_log = keep;
}

/// The level-up windows (FUN_004537da / FUN_00456549) for the levels
/// `Battle::level_up` recorded, one window sequence per unit.
pub fn show_level_ups(game: &mut Game) {
    let log = std::mem::take(&mut game.battle.level_log);
    if !presenting(game) {
        return;
    }
    let mut units: Vec<UnitRef> = Vec::new();
    for g in &log {
        if !units.contains(&g.unit) {
            units.push(g.unit);
        }
    }
    for r in units {
        let gains: Vec<view::panel::LevelGain> =
            log.iter().filter(|g| g.unit == r).map(|g| view::panel::LevelGain { level: g.level, old: g.old, gain: g.gain }).collect();
        view::panel::level_up_window(game, r, &gains);
    }
}
