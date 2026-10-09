//! Magic, techniques and special skills from the command menu, and items
//! used in battle (docs/battle.md §8, §12): the spell menu (FUN_00464aee /
//! FUN_00464018), the cast driver (FUN_0045fc21 with FUN_0045fe6c), aiming
//! (FUN_00401634 / FUN_004041d7 / FUN_004052e3 over `battle::targeting`),
//! the cast (`spells::cast_spell` = FUN_0045ff9e, cost FUN_0045f78b) with its
//! presentation, and the in-battle branch of FUN_0044a300.

use super::actions::{self, presenting, Arg};
use super::army::{Side, UnitRef};
use super::camera;
use super::hooks::{finish_effects, BattleEffects};
use super::menu::input;
use super::spells::{Cast, CastOutcome, Shortage, Show};
use super::targeting::{CellCheck, SpellTarget};
use super::view;
use crate::game::Game;
use crate::sys::ev;
use crate::{cursor, gfx, text, ui};

/// The name of spell `s` (0-based) as stored (`DAT_004a2560 + 2 + s * 0x25`);
/// re-encoding the decoded name can lose characters.
pub fn spell_name_raw(game: &Game, s: usize) -> Vec<u8> {
    match game.res.as_ref().and_then(|r| r.spell_names.get(s)) {
        Some(n) => n.clone(),
        None => game.battle.t().spells.spells.get(s).map(|sp| view::panel::to_big5(&sp.name)).unwrap_or_default(),
    }
}

/// UI image slot of image `k` of the spell menu set (FUN_0045ef0b: CEL 2
/// entries from 0x61; CEL 2 entry e is UI slot 142 + e).
fn spell_img(k: usize) -> usize {
    142 + 0x61 + k
}

/// Port of FUN_0045f212 (magic) / FUN_0045f3b0 (techniques): every school with
/// a learned spell, and the learned spells that may be listed (FUN_0045f145).
fn spell_menu_lists(game: &Game, r: UnitRef, technique: bool) -> Vec<(usize, Vec<u16>)> {
    let b = &game.battle;
    let t = b.t();
    b.spell_lists(r, technique)
        .into_iter()
        .map(|(school, idx)| {
            let first = t.spells.school_spells.get(school - 1).map_or(0, |s| s.0 as u16);
            let keep = idx.into_iter().filter(|&i| b.spell_listed(r, first + i, true, false)).collect();
            (school, keep)
        })
        .collect()
}

/// Port of FUN_0045f6cf (魔法, technique false) / FUN_0045f72d (招式, true).
/// Returns true when a spell was cast.
pub fn magic_command(game: &mut Game, r: UnitRef, technique: bool) -> bool {
    cursor::set_shape(game, if technique { 10 } else { 9 });
    let lists = spell_menu_lists(game, r, technique);
    if lists.is_empty() {
        return false;
    }
    let ok = cast_driver(game, r, technique, &lists);
    cursor::set_shape(game, 0xb);
    ok
}

/// Port of FUN_0045fc21: choose a spell, aim it, cast it; back to the menu
/// when the aim is cancelled.
fn cast_driver(game: &mut Game, r: UnitRef, technique: bool, lists: &[(usize, Vec<u16>)]) -> bool {
    camera::redraw(game);
    if presenting(game) {
        view::draw_unit(game, r, 0);
    }
    let mut spell = spell_menu(game, r, lists, false);
    loop {
        if spell == 0 {
            return false;
        }
        let side = r.side;
        let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
        let mut cast = Cast::new(side, r, spell, (x, y));
        cast.technique = technique;
        let sp = game.battle.t().spells.spells[spell as usize - 1].clone();
        if sp.combo & 7 != 0 {
            cast.partner = game.battle.combo_partner(r, spell);
        }
        if let Some(t) = aim(game, &mut cast) {
            perform(game, &mut cast, t);
            return true;
        }
        game.battle.cursor = (x, y);
        view::cursor_set(game, x, y);
        game.battle.map_mut().center_on(x, y, true);
        camera::redraw(game);
        if presenting(game) {
            view::draw_unit(game, r, 0);
        }
        spell = spell_menu(game, r, lists, false);
    }
}

/// FUN_00465f44 + FUN_0045ff9e + FUN_0040145c + FUN_0045f78b: animate and
/// apply (with the rewards), free the target, pay.
fn perform(game: &mut Game, cast: &mut Cast, mut t: SpellTarget) {
    cast_effects(game, cast, &mut t);
    t.free();
    game.battle.pay_spell_cost(cast);
    view::bake(game);
}

/// FUN_00465f44 then FUN_0045ff9e: the cast animation with the per-kind
/// sequence (which may apply the cells itself), the effects on the target's
/// cells (a teleport, effect 4: the unit on `target_cell`, FUN_0046cf00)
/// with what the original shows in between, then the rewards: the combo
/// partner's EXP window and level-ups, then the caster's (with the drops
/// and the name of a spell just learned).
pub fn cast_effects(game: &mut Game, cast: &mut Cast, tg: &mut SpellTarget) -> CastOutcome {
    let effect = game.battle.t().spells.spells.get(cast.spell as usize - 1).map_or(0, |s| s.effect);
    let cells: Vec<(i32, i32)> = if effect == 4 { vec![tg.target_cell] } else { tg.cells().to_vec() };
    let cells = &cells[..];
    let mut fx = BattleEffects { caster: Some(cast.caster), ..Default::default() };
    let mut applied = false;
    {
        let (vc, vt) = (cast.clone(), tg.clone());
        let mut seq = |g: &mut Game, e: &mut usize| sequence(g, cast, tg, e, &mut fx, &mut applied);
        if presenting(game) {
            view::cast::cast_animation(game, &vc, &vt, seq);
        } else {
            let mut e = view::cast::spell_fx(game, vc.spell).effect as usize + 1;
            seq(game, &mut e);
        }
    }
    let out = if applied {
        game.battle.cast_spell_applied(cast)
    } else {
        // The popups (FUN_004636cd's forecast panel, FUN_00452178) come before
        // FUN_0045ff9e's tail raises the school proficiency.
        let who: Vec<UnitRef> = std::iter::once(cast.caster).chain(cast.partner).collect();
        let before: Vec<[u16; 18]> = who.iter().map(|&u| game.battle.unit(u).prof).collect();
        let out = game.battle.cast_spell(cast, cells, &mut fx);
        let after: Vec<[u16; 18]> = who.iter().map(|&u| game.battle.unit(u).prof).collect();
        for (&u, p) in who.iter().zip(&before) {
            game.battle.unit_mut(u).prof = *p;
        }
        let shows = std::mem::take(&mut cast.shows);
        play_shows(game, cast, shows, &mut fx);
        for (&u, p) in who.iter().zip(&after) {
            game.battle.unit_mut(u).prof = *p;
        }
        out
    };
    super::trace!("cast {:?} spell {} cells {:?} hits {:?}", cast.caster, cast.spell, cells, out.targets);
    // Summons, teleports, the free move: inline in the original's effect pass.
    let levels = std::mem::take(&mut game.battle.level_log);
    finish_effects(game, fx);
    game.battle.level_log.splice(0..0, levels);
    if !out.caster_died {
        // FUN_0045ff9e: FUN_0046ec6b(1, 0) after the effect pass — page 1
        // (the scene without the last hit flash) back before the windows.
        if presenting(game) {
            game.scr.copy_page(0, 1);
        }
        let silent = game.battle.army(cast.side).side != Side::Player && !game.battle.vs;
        let learned_text = |game: &Game, learned: &[usize]| -> Option<Vec<u8>> {
            let t = game.battle.t();
            learned.iter().rev().find(|&&s| t.spells.spells[s].combo & 6 == 0).map(|&s| spell_name_raw(game, s))
        };
        let combo = game.battle.t().spells.spells[cast.spell as usize - 1].combo & 7 != 0;
        if let (true, Some(p)) = (combo, cast.partner) {
            // FUN_0045ff9e: the caster (act 0) goes into the map surface
            // (FUN_00440264) before the partner's window.
            game.battle.unit_mut(cast.caster).act = 0;
            view::put_unit(game, cast.caster);
            if presenting(game) && !silent {
                let text = learned_text(game, &out.learned_partner);
                view::panel::reward_window(game, p, out.exp_partner, text.as_deref(), &[]);
            }
            // FUN_00453725 after the partner's window.
            game.battle.check_level_up(p);
            actions::show_level_ups_of(game, p);
            // The partner goes into the surface, the caster off it again
            // (FUN_00441c0b), and FUN_0046ec6b(1, 0) takes the partner's
            // windows off before the caster's.
            view::put_unit(game, p);
            view::erase_unit(game, cast.caster);
            if presenting(game) {
                game.scr.copy_page(0, 1);
            }
        }
        if presenting(game) && !silent {
            let text = learned_text(game, &out.learned);
            let drops = std::mem::take(&mut cast.drops);
            view::panel::reward_window(game, cast.caster, out.exp, text.as_deref(), &drops);
        }
        // FUN_00453725 after the caster's window.
        game.battle.check_level_up(cast.caster);
        actions::show_level_ups_of(game, cast.caster);
    }
    actions::show_level_ups(game);
    out
}

/// Port of FUN_00466f65, the per-kind sequence of FUN_00465f44 (`effect`:
/// the effect id, advanced by kind 5). Kinds 2, 3 and 6 apply the cast cell
/// by cell between the effect frames (FUN_00460c85, with what it shows),
/// set `applied` and are the whole animation (true); the others are
/// GRAPHICS' `view::cast::draw_sequence`. Kind 2 chains over the area's
/// visible enemies (FUN_004051b3, wrapping) while caster and partner can
/// pay the cost again, paying it per hit; 3 skips the first cell; 6 takes
/// the cells after the first four at a time. Their EXP is the caster's
/// accumulated EXP capped at 0x3c, for both (kind 3: the partner only for
/// a combo); the proficiency gain stays the previous cast's.
fn sequence(game: &mut Game, cast: &mut Cast, tg: &mut SpellTarget, effect: &mut usize, fx: &mut BattleEffects, applied: &mut bool) -> bool {
    let sf = view::cast::spell_fx(game, cast.spell);
    let Some(sp) = game.battle.t().spells.spells.get(cast.spell as usize - 1).cloned() else { return false };
    let anim = game.cfg.battle_anim && presenting(game);
    let combo = sp.combo & 7 != 0;
    let begin = |game: &mut Game, cast: &mut Cast| {
        cast.exp = 0;
        cast.exp_partner = 0;
        cast.hits.clear();
        cast.drops = vec![0; game.battle.t().items.len()];
    };
    let mut hit = |game: &mut Game, cast: &mut Cast, x: i32, y: i32| {
        game.battle.spell_damage_cell(cast, x, y, fx);
        let shows = std::mem::take(&mut cast.shows);
        play_shows(game, cast, shows, fx);
    };
    match sf.kind {
        2 => {
            begin(game, cast);
            *applied = true;
            if anim {
                view::cast::effect_scene(game, cast, tg, 2);
            }
            tg.iter = 0;
            let cost = sp.cost;
            // +0x0c without a combo is not set by the cast (guess: no partner pays).
            let partner = if combo { cast.partner } else { None };
            while let Some((x, y)) = game.battle.next_foe_in_area(tg) {
                if anim {
                    let _ = view::play_effect(game, *effect, x, y, false);
                }
                hit(game, cast, x, y);
                game.battle.unit_mut(cast.caster).mp -= cost;
                if let Some(p) = partner {
                    game.battle.unit_mut(p).mp -= cost;
                }
                let short = |game: &Game, u: Option<UnitRef>| u.is_some_and(|u| game.battle.unit(u).mp < cost);
                // (Guard: a free chain would never end; no spell uses kind 2.)
                if short(game, Some(cast.caster)) || short(game, partner) || cost <= 0 {
                    break;
                }
            }
            let e = cast.exp.min(0x3c);
            (cast.exp, cast.exp_partner) = (e, e);
            true
        }
        3 => {
            begin(game, cast);
            *applied = true;
            if anim {
                view::cast::effect_scene(game, cast, tg, 3);
            }
            for &(x, y) in tg.cells().to_vec().iter().skip(1) {
                actions::pump(game);
                if anim {
                    let _ = view::play_effect(game, *effect, x, y, false);
                }
                hit(game, cast, x, y);
            }
            let e = cast.exp.min(0x3c);
            (cast.exp, cast.exp_partner) = (e, if combo { e } else { 0 });
            true
        }
        6 => {
            begin(game, cast);
            *applied = true;
            if anim {
                view::cast::effect_scene(game, cast, tg, 6);
            }
            let cells = tg.cells().to_vec();
            let mut k = 1;
            while k < cells.len() {
actions::pump(game);
                // FUN_00405105 gives (0, 0) past the end; the original's
                // (-1, -1) test never skips those, so they are hit too.
                let four: Vec<(i32, i32)> = (0..4).map(|j| cells.get(k + j).copied().unwrap_or((0, 0))).collect();
                if anim {
                    let _ = view::play_effect_cells(game, *effect, &four);
                }
                for &(x, y) in &four {
                    hit(game, cast, x, y);
                }
                k += 4;
            }
            let e = cast.exp.min(0x3c);
            (cast.exp, cast.exp_partner) = (e, e);
            true
        }
        _ if presenting(game) => view::cast::draw_sequence(game, cast, tg, effect),
        5 => {
            *effect += 1;
            false
        }
        9 | 10 => true,
        _ => false,
    }
}

/// Play what the effect code recorded (`Cast::shows`), in order: the hit
/// flash with sound 0x76, FUN_004636cd's popups counting the HP, deaths
/// (FUN_0044d14b with the caster unselected meanwhile; taken out of
/// `fx.died`), the map surface redraw. The popups replay the HP from the
/// values before each step; the final HP is restored at the end.
pub fn play_shows(game: &mut Game, cast: &Cast, shows: Vec<Show>, fx: &mut BattleEffects) {
    let mut units: Vec<UnitRef> = vec![cast.caster];
    for sh in &shows {
        if let Show::Hp { target, .. } = sh {
            if !units.contains(target) {
                units.push(*target);
            }
        }
    }
    let finals: Vec<(UnitRef, i16)> = units.iter().map(|&u| (u, game.battle.unit(u).hp)).collect();
    // The rule pass already marked the killed units dead; in the original they
    // die after their popup (FUN_004636cd, then FUN_0044d14b), so they stay on
    // the map until their Die.
    for sh in &shows {
        if let Show::Die(u, _) = *sh {
            game.battle.unit_mut(u).flags &= !super::unit::flag::DEAD;
        }
    }
    for sh in shows {
        match sh {
            Show::Hit(u) => {
                ui::sound(game, 0x76);
                if presenting(game) {
                    view::hit_flash(game, u);
                }
            }
            Show::Hp { target, amount, sign, apply, reflect, hp_before } => {
                if presenting(game) {
                    game.battle.unit_mut(target).hp = hp_before.0;
                    game.battle.unit_mut(cast.caster).hp = hp_before.1;
                    spell_hp_popup(game, cast, target, amount, sign, apply, reflect);
                }
            }
            Show::Die(u, mode) => {
                if let Some(i) = fx.died.iter().position(|&(d, _)| d == u) {
                    fx.died.remove(i);
                }
                for &(r, hp) in &finals {
                    if r == u {
                        game.battle.unit_mut(r).hp = hp;
                    }
                }
                game.battle.unit_mut(u).flags &= !super::unit::flag::DEAD;
                // FUN_00441c0b (target), the caster drawn on the page
                // (FUN_0043ff88), then its act cleared for FUN_0044d14b.
                view::erase_unit(game, u);
                if presenting(game) && u != cast.caster {
                    view::draw_unit(game, cast.caster, 0);
                }
                game.battle.unit_mut(cast.caster).act = 0;
                actions::die(game, u, mode);
                if u != cast.caster {
                    game.battle.unit_mut(cast.caster).act = 1;
                }
            }
            Show::Redraw => view::bake_army(game, cast.side.other()),
            Show::Put(u, show) => {
                view::put_unit(game, u);
                if show != 0 && presenting(game) {
                    game.scr.set_page(1);
                    view::draw_unit(game, u, 0);
                    game.scr.set_page(0);
                    if show == 2 {
                        view::draw_unit(game, u, 0);
                        gfx::present(game);
                    } else {
                        gfx::present_page(game, 1);
                    }
                }
            }
            Show::Pump => actions::pump(game),
            Show::Sheet(u) => view::panel::class_sheet(game, u),
        }
    }
    for (r, hp) in finals {
        game.battle.unit_mut(r).hp = hp;
    }
}

/// Port of FUN_004636cd's presentation: the camera to the target if it is
/// off screen, page 1 = the map window with the caster, then `amount` (or
/// "MISS") rises over the target for 7 frames (3, 3, 2, 1.. cs) in 0x3c /
/// 0x56 (damage), 0x24 / 0xbe (heal) or 0x68 / 0x24 (miss) while, when
/// `apply`, the HP is counted in 1, 2, 4.. chunks under the spell's
/// forecast panel (FUN_00452178).
fn spell_hp_popup(game: &mut Game, cast: &Cast, target: UnitRef, amount: i32, sign: i32, apply: bool, reflect: bool) {
    let (x, y) = (game.battle.unit(target).x, game.battle.unit(target).y);
    if !game.battle.map().on_screen(x, y, 0) {
        camera::scroll_exact(game, x, y, Some(cast.caster));
    }
    let (mut sx, mut sy) = view::cell_screen(game, x, y);
    let (s, fg, bg) = if amount == 0 {
        (b"MISS".to_vec(), 0x68, 0x24)
    } else if amount < 0 {
        (actions::sprintf(b"%d", &[Arg::D(amount.abs())]), 0x24, 0xbe)
    } else {
        (actions::sprintf(b"%d", &[Arg::D(amount.abs())]), 0x3c, 0x56)
    };
    sx += 0x20 - s.len() as i32 * 8;
    sy -= 0x40;
    let eff = game.battle.t().spells.spells[cast.spell as usize - 1].effect;
    // The cast object's frame clock (+0x5f8), shared with the effect frames
    // (FUN_00466507 / FUN_004668dd wait on it after the popup).
    game.battle.view.fx_last = game.ticks();
    let mut left = if apply { amount.abs() } else { 0 };
    let (mut i, mut wait) = (0, 3);
    game.scr.set_page(1);
    camera::redraw(game);
    view::draw_unit(game, cast.caster, 0);
    game.scr.set_page(0);
    while left > 0 || i < 8 {
        actions::pump(game);
        if left != 0 {
            let v = (1i32 << i.min(30)).min(left);
            left -= v;
            super::spells::apply_hp_chunk(&mut game.battle, cast.caster, eff, target, sign, reflect, v);
        }
        game.scr.copy_page(0, 1);
        if apply {
            let snap = game.battle.rng.snapshot();
            let (hit, amt, _) = game.battle.spell_roll(cast, target, target.side, false);
            game.battle.rng.restore(snap);
            // FUN_00452178(.., 0): the panel placed by the target (FUN_0045210f).
            let fc = view::panel::Forecast { hit, dmg: amt, ..Default::default() };
            view::panel::duel_panel(game, cast.caster, target, fc, false);
        }
        if i < 7 {
            text::large(game, sx, sy, &s, fg, bg, 1, 0);
        }
        if i < 6 {
            let t0 = game.battle.view.fx_last;
            game.wait_until(t0, wait);
        }
        gfx::present(game);
        game.battle.view.fx_last = game.ticks();
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

// ---- the spell menu --------------------------------------------------------------------

/// Port of FUN_00464aee(caster, reset): the spell menu at (0, 0x17c): the
/// learned spells of the current school as icons, the schools' names, the
/// spell name, cost and effect class. ←/→ spells, Ctrl+←/→ schools, Space
/// casts, Esc / right click cancel. Returns the 1-based spell id, 0 = cancel.
/// The row / column are kept between calls (`DAT_004a2580/84`).
fn spell_menu(game: &mut Game, r: UnitRef, lists: &[(usize, Vec<u16>)], reset: bool) -> u16 {
    let (mx, my) = (0i32, 0x17ci32);
    let n_rows = lists.len() as i32;
    let (mut row, mut col) = if reset { (0, 0) } else { game.battle.spell_menu_pos };
    row = row.clamp(0, n_rows - 1);
    let cols = |row: i32| lists[row as usize].1.len() as i32;
    col = col.clamp(0, (cols(row) - 1).max(0));
    let first = |game: &Game, row: i32| {
        game.battle.t().spells.school_spells.get(lists[row as usize].0 - 1).map_or(0, |s| s.0 as u16)
    };
    if presenting(game) {
        game.scr.copy_page(2, 0);
    }
    let mut warp = true;
    let mut full = true;
    let mut dirty = true;
    let mut moved = false;
    let result = loop {
        actions::pump(game);
        if warp {
            cursor::warp(game, mx + 0x2e + col * 0x22, my + 0x22);
            warp = false;
        }
        let was_full = full;
        if full {
            if presenting(game) {
                game.scr.copy_page(0, 2);
            }
            dirty = true;
            full = false;
        }
        if dirty {
            if presenting(game) && !was_full {
                cursor::erase(game);
            }
            draw_spell_menu(game, r, lists, row, col);
            if presenting(game) {
                cursor::draw_present(game, None, false);
            }
            dirty = false;
            moved = false;
        } else if moved {
            // FUN_0043399f(-1, 1) after a pointer move.
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if !presenting(game) {
            // Headless: take the selected spell.
            break if cols(row) > 0 { first(game, row) + lists[row as usize].1[col as usize] } else { 0 };
        }
        if !game.get_event(0xffff) || !(1..=0x20).contains(&game.ev.kind) {
            continue;
        }
        let (kind, mods, ex, ey) = (game.ev.kind, game.ev.mods, game.ev.x, game.ev.y);
        let mut code = 0;
        if kind < 4 {
            match game.ev.scan() {
                0x4b => {
                    if mods & 0x20 != 0 {
                        if n_rows > 1 {
                            code = 1;
                        }
                    } else if cols(row) > 1 {
                        code = 5;
                    }
                }
                0x4d => {
                    if mods & 0x20 != 0 {
                        if n_rows > 1 {
                            code = 2;
                        }
                    } else if cols(row) > 1 {
                        code = 6;
                    }
                }
                0x01 => {
                    game.wait_event(ev::KEY_UP);
                    code = -1;
                }
                0x39 => {
                    game.wait_event(ev::KEY_UP);
                    code = 4;
                }
                0x3b => {
                    game.wait_event(ev::KEY_UP);
                    code = 3;
                }
                _ => {}
            }
        } else if kind == ev::MOUSE_DOWN || kind == ev::MOUSE_MOVE {
            if kind == ev::MOUSE_MOVE {
                moved = true;
            }
            let mut hit_col = -1;
            if ex >= mx + 0x1a && ex <= mx + 0x1fa && ey >= my + 0xe && ey <= my + 0x2e {
                let c = (ex - mx - 0x1a) / 0x22;
                hit_col = c;
                if c != col && c < cols(row) && kind == ev::MOUSE_MOVE {
                    col = c;
                    dirty = true;
                }
            }
            if mods & ev::M_LBUTTON != 0 {
                game.wait_event(ev::MOUSE_UP);
                if ex >= mx + 0x1ea && ex < mx + 0x271 && ey >= my + 0xb && ey < my + 0x5f {
                    let k = (ex - mx - 0x1ea) / 0x2d + 3 * ((ey - my - 0xb) / 0x1c);
                    if k < n_rows && k != row {
                        row = k;
                        col = 0;
                        dirty = true;
                    }
                }
                if ex >= mx + 0x14 && ex < mx + 0x88 && ey >= my + 0x37 && ey < my + 0x58 {
                    code = 3;
                } else if hit_col != -1 && hit_col < cols(row) {
                    code = 4;
                }
            } else if mods & ev::M_RBUTTON != 0 {
                game.wait_event(ev::MOUSE_UP);
                code = -1;
            }
        }
        match code {
            -1 => break 0,
            1 => {
                row = if row == 0 { n_rows - 1 } else { row - 1 };
                col = 0;
                warp = true;
                dirty = true;
                ui::sound(game, 100);
            }
            2 => {
                row = if row == n_rows - 1 { 0 } else { row + 1 };
                col = 0;
                warp = true;
                dirty = true;
                ui::sound(game, 100);
            }
            3 => {
                ui::sound(game, 100);
                // FUN_00464727(row, col): FLOW's port.
                if cols(row) != 0 && presenting(game) {
                    let id = first(game, row) + lists[row as usize].1[col as usize];
                    crate::flow::spellview::spell_help(game, id as usize - 1, false);
                }
                full = true;
                warp = true;
            }
            4 => {
                if cols(row) == 0 {
                    continue;
                }
                let id = first(game, row) + lists[row as usize].1[col as usize];
                if let Some(m) = refuse(game, r, id) {
                    let s = game.msg(m);
                    ui::message_box(game, 0xb0, 0x28, &s, ui::Wait::Click(0));
                    full = true;
                    warp = true;
                    continue;
                }
                break id;
            }
            5 => {
                col = if col == 0 { cols(row) - 1 } else { col - 1 };
                warp = true;
                dirty = true;
                ui::sound(game, 100);
            }
            6 => {
                col = if col == cols(row) - 1 { 0 } else { col + 1 };
                warp = true;
                dirty = true;
                ui::sound(game, 100);
            }
            _ => {}
        }
    };
    game.battle.spell_menu_pos = (row, col);
    game.flush_events(0x1f);
    result
}

/// The checks of FUN_00464aee before a spell is taken: a summon while one
/// is out (msg 0x1f), a combo without a partner (0x21), MP / ST / AP short
/// for the caster or the partner (0x16 / 0x17 / 0xe3).
fn refuse(game: &Game, r: UnitRef, id: u16) -> Option<usize> {
    let b = &game.battle;
    let sp = b.t().spells.spells[id as usize - 1].clone();
    if (0x15..0x20).contains(&sp.effect) {
        let k = sp.effect as usize - 0x15;
        let a = b.army(r.side);
        if a.summons.get(k).is_some_and(|s| s.active == 1) || a.summons.iter().any(|s| s.active == 1) {
            return Some(0x1f);
        }
    } else if sp.combo & 7 != 0 && b.combo_partner(r, id).is_none() {
        return Some(0x21);
    }
    let mut s = b.spell_shortage(b.unit(r), id);
    if sp.combo & 7 != 0 && s == Shortage::Ok {
        if let Some(p) = b.combo_partner(r, id) {
            s = b.spell_shortage(b.unit(p), id);
        }
    }
    match s {
        Shortage::Ok => None,
        Shortage::Mp => Some(0x16),
        Shortage::St => Some(0x17),
        _ => Some(0xe3),
    }
}

/// Port of FUN_00464018: the menu panel, school names (current one in
/// colour 0x3c), the row's spell icons (selected one highlighted), the
/// spell name, its cost and its effect class.
fn draw_spell_menu(game: &mut Game, r: UnitRef, lists: &[(usize, Vec<u16>)], row: i32, col: i32) {
    if !presenting(game) {
        return;
    }
    let (mx, my) = (0i32, 0x17ci32);
    game.scr.set_page(0);
    if let Ok(p) = game.res().ui_image(spell_img(1)) {
        gfx::blit(game.scr.page(), &p, mx, my, true);
    }
    let t = game.battle.t();
    let (mut x, mut y) = (mx + 0x1fc, my + 0xe);
    for (k, &(school, _)) in lists.iter().enumerate() {
        let name = game.res().school_names.get(school - 1).cloned().unwrap_or_default();
        let fg = if k as i32 == row { 0x3c } else { 0x69 };
        text::small(game, x - text::strlen(&name) * 4, y, &name, fg, 0, 1, 2);
        if k % 3 == 2 {
            x = mx + 0x1fc;
            y += 0x1c;
        } else {
            x += 0x2d;
        }
    }
    let (school, ref spells) = lists[row as usize];
    let first = t.spells.school_spells.get(school - 1).map_or(0, |s| s.0 as usize);
    let mut ix = mx + 0x1a;
    for (k, &i) in spells.iter().enumerate() {
        let mut img = i as usize * 2 + 0x16 + first * 2;
        if k as i32 == col {
            img += 1;
        }
        if let Ok(p) = game.res().ui_image(spell_img(img)) {
            gfx::blit(game.scr.page(), &p, ix, my + 0xe, true);
        }
        ix += 0x22;
    }
    let label = game.msg(0xe2);
    text::large(game, mx + 0x4e - text::strlen(&label) * 6, my + 0x3b, &label, 0x29, 0, 1, 2);
    let Some(&i) = spells.get(col as usize) else { return };
    let idx = first + i as usize - 1;
    let sp = t.spells.spells[idx].clone();
    let name = spell_name_raw(game, idx);
    text::large(game, mx + 0x94, my + 0x3a, &name, 0xa8, 0, 1, 2);
    let u = game.battle.unit(r).clone();
    let n = (sp.stat_flags & 1 != 0) as i32 + (sp.stat_flags & 2 != 0) as i32 + (sp.stat_flags & 4 != 0) as i32;
    let mut cx = mx + 0xf0;
    if n != 0 {
        if n < 3 {
            let cost = if sp.stat_flags & 1 != 0 && u.abilities & 0x20 != 0 { sp.cost / 2 } else { sp.cost } as i32;
            let s = actions::sprintf(b"%d /", &[Arg::D(cost)]);
            cx = mx + 0xfc;
            text::small(game, cx, my + 0x3e, &s, 0x2b, 0, 1, 2);
            cx += s.len() as i32 * 8;
        }
        for (bit, label, have) in [(1u8, &b" MP:%d"[..], u.mp), (2, &b" ST:%d"[..], u.st), (4, &b" HP:%d"[..], u.hp)] {
            if sp.stat_flags & bit != 0 {
                let s = actions::sprintf(label, &[Arg::D(have as i32)]);
                let fg = if (have as i32) < sp.cost as i32 { 0x3c } else { 0x2b };
                text::small(game, cx, my + 0x3e, &s, fg, 0, 1, 2);
                cx += s.len() as i32 * 8;
            }
        }
    }
    let ex = if cx < mx + 0x19a { mx + 0x1a0 } else { mx + 0x1a4 };
    let class = match sp.effect {
        1 => 0,
        0x0e | 0x12 | 0x5a => 1,
        _ => 2,
    };
    let m = game.msg(0xf + class);
    text::large(game, ex, my + 0x3b, &m, 0xc4, 0x36, 1, 1);
}

// ---- aiming ---------------------------------------------------------------------------

/// Port of FUN_0045fe6c: a spell without range or area is confirmed with a
/// yes/no box on the caster's cell; others are aimed (FUN_00401634).
/// Returns the targeting object when the cast goes ahead.
fn aim(game: &mut Game, cast: &mut Cast) -> Option<SpellTarget> {
    let r = cast.caster;
    let sp = game.battle.t().spells.spells[cast.spell as usize - 1].clone();
    let mode = cast.technique as u8;
    let mut t = game.battle.spell_target(cast.side, r, cast.spell as i32, mode, 1, cast.from_item);
    if sp.range == 0 && sp.shape == 0 {
        cast.cell = (game.battle.unit(r).x, game.battle.unit(r).y);
        let m = if cast.from_item { game.msg(0x50) } else { game.msg(0x4e + mode as usize) };
        let ok = !presenting(game) || ui::yes_no(game, 0x7c, 0x2e, true, &m, false, false);
        if !ok {
            return None;
        }
        game.battle.cursor = cast.cell;
        game.battle.target_area(&mut t);
        return Some(t);
    }
    game.battle.target_range(&mut t, sp.range as i32);
    let (ok, cell) = aim_loop(game, &mut t)?;
    if !ok {
        return None;
    }
    cast.cell = cell;
    Some(t)
}

/// The range (and area) overlay of FUN_00401bfa / FUN_00403e52.
fn show_aim(game: &mut Game, t: &SpellTarget) {
    view::show_aim(game, t);
}

/// Port of FUN_004055a4: the AI aims `t` at (x, y) (`dir` for rays and
/// cones), shown as a player's aim: the camera to the cell when off
/// screen, the range, the cursor there and the panel with a 2 cs prompt;
/// then the area (FUN_00402310) shown again with a 5 cs prompt; then the
/// ray / cone of shapes 5 / 8 (`Battle::aim_ai_cast`).
pub fn ai_aim(game: &mut Game, t: &mut SpellTarget, x: i32, y: i32, dir: i32) {
    if presenting(game) {
        if !game.battle.map().on_screen(x, y, 0) {
            camera::scroll_exact(game, x, y, Some(t.caster));
        }
        show_aim(game, t);
        game.battle.cursor = (x, y);
        view::cursor_set(game, x, y);
        aim_panel(game, t);
        ui::click_to_continue(game, 2);
        game.battle.cursor = (x, y);
        game.battle.target_area(t);
        show_aim(game, t);
        aim_panel(game, t);
        ui::click_to_continue(game, 5);
    }
    game.battle.aim_ai_cast(t, x, y, dir);
}

/// The panel part of FUN_00401d75: the target's forecast while aiming.
fn aim_panel(game: &mut Game, t: &SpellTarget) {
    if !presenting(game) {
        return;
    }
    // FUN_00401d75(.., 0): the window with the aim, the caster stepping its
    // frame every 0xf cs (not move type 4) and drawn by its move type.
    view::draw_range_window(game);
    let r = t.caster;
    let mt = {
        let u = game.battle.unit(r).clone();
        game.battle.move_type(&u)
    };
    if mt != 4 && !game.still_waiting(game.battle.unit(r).anim_timer, 0xf) {
        let f = game.battle.unit(r).facing;
        view::step_unit(game, r, f);
        game.battle.unit_mut(r).anim_timer = game.ticks();
    }
    match mt {
        3 => {
            view::draw_ground_shadow_with(game, r, true);
            view::motion::draw_lifted(game, r, 4);
        }
        4 => view::motion::draw_flyer(game, r, false),
        _ => view::draw_unit(game, r, 0),
    }
    if t.ready == 0 {
        // FUN_0042c121: the cursor follows the pointer if it moved since the
        // last call (also while the AI aims, FUN_004055a4).
        view::cursor_track(game);
        view::draw_cursor(game);
        match game.battle.cursor_unit(t) {
            Some(target) => {
                // FUN_00452178 / FUN_004521c5 show the spell's forecast on the unit.
                let mut slot = 0u8;
                let cast = Cast::new(t.side, t.caster, t.spell.max(0) as u16, game.battle.cursor);
                let (hit, amount, _) = if t.mode == 2 {
                    (100, game.battle.item_amount((-t.spell) as u16, target), slot)
                } else {
                    let snap = game.battle.rng.snapshot();
                    let side = target.side;
                    let v = game.battle.spell_roll(&cast, target, side, false);
                    game.battle.rng.restore(snap);
                    slot = v.2;
                    (v.0, v.1, slot)
                };
                let _ = slot;
                view::panel::spell_forecast(game, t.caster, target, hit, amount);
            }
            None => view::panel::status_bar(game, None),
        }
    }
    cursor::draw_present(game, None, false);
}

/// Port of FUN_00401634: move the cursor over the range and pick the cell.
/// `None` when nothing in range can be targeted (msg 0x54); `Some((false, _))`
/// when cancelled.
fn aim_loop(game: &mut Game, t: &mut SpellTarget) -> Option<(bool, (i32, i32))> {
    let need_foe = t.mode != 2 && t.aim.shape == 1 && t.aim.target == 2;
    if need_foe && !game.battle.range_has_foe(t) {
        if presenting(game) {
            let m = game.msg(0x54);
            ui::message_box(game, 0xb0, 0x28, &m, ui::Wait::Click(0));
        }
        return None;
    }
    game.flush_events(0x1f);
    show_aim(game, t);
    let (cx, cy) = (game.battle.unit(t.caster).x, game.battle.unit(t.caster).y);
    game.battle.cursor = (cx, cy);
    view::cursor_set(game, cx, cy);
    let shape_of = |t: &SpellTarget| if t.mode == 2 { 0xb } else { 9 + (t.mode != 0) as usize };
    if t.range.len() != 1 || t.mode == 2 {
        if !presenting(game) {
            // Headless: aim at the first range cell holding a target.
            let pick = t.range.iter().copied().find(|&(x, y)| {
                game.battle.unit_at_cell(t.side, x, y, false).is_some() || game.battle.unit_at_cell(t.side.other(), x, y, true).is_some()
            });
            let Some(c) = pick else { return Some((false, (cx, cy))) };
            game.battle.cursor = c;
            let ok = confirm_cell(game, t) == 1;
            return Some((ok, game.battle.cursor));
        }
        let mut last = (cx, cy);
        let mut scrolled = false;
        loop {
            actions::pump(game);
            let (mut x, mut y) = (0, 0);
            let code = input(game, &mut x, &mut y, 1);
            match code {
                0 => {
                    if scrolled {
                        show_aim(game, t);
                        scrolled = false;
                        super::menu::pull_cursor_into_view(game);
                    }
                    if game.battle.cursor != last {
                        let (ux, uy) = game.battle.cursor;
                        let s = if t.in_range_or_area(ux, uy) { shape_of(t) } else { 0xf };
                        cursor::set_shape(game, s);
                        last = (ux, uy);
                    }
                    aim_panel(game, t);
                }
                -0x21 => cursor::draw_present(game, None, true),
                -8 | -3 | 0x1c => {
                    if confirm_cell(game, t) == 1 {
                        cursor::set_shape(game, shape_of(t));
                        view::hide_range(game);
                        return Some((true, game.battle.cursor));
                    }
                    let (ux, uy) = (game.battle.unit(t.caster).x, game.battle.unit(t.caster).y);
                    game.battle.map_mut().center_on(ux, uy, true);
                    show_aim(game, t);
                }
                -6 => {}
                -5 => {
                    super::menu::follow_cursor(game, &mut |g| {
                        show_aim(g, t);
                        aim_panel(g, t);
                    });
                    scrolled = true;
                }
                -4 | -1 => {
                    if code == -4 {
                        game.wait_event(ev::MOUSE_UP);
                    }
                    cursor::set_shape(game, shape_of(t));
                    view::hide_range(game);
                    return Some((false, game.battle.cursor));
                }
                c => {
                    if game.battle.map_mut().scroll_key(c) {
                        scrolled = true;
                        let t0 = game.ticks();
                        camera::redraw_present(game);
                        let sp = game.cfg.screen_speed;
                        game.wait_until(t0, sp);
                    }
                }
            }
        }
    }
    // Only the caster's own cell is in range.
    let mut r = confirm_cell(game, t);
    if r == -1 {
        r = 0;
    }
    view::hide_range(game);
    let mut cell = game.battle.cursor;
    if r == 1 && (0x15..0x20).contains(&t.aim.effect) {
        cell = t.area.first().copied().unwrap_or(cell);
    }
    Some((r == 1, cell))
}

/// Port of FUN_004041d7: check the aimed cell, show the area and wait for
/// the confirmation (Enter, Tab between 是/否, numpad 7/9/1/3 or the pointer
/// for the direction of rays and cones). 1 = cast, 0 = back, −1 = rejected.
fn confirm_cell(game: &mut Game, t: &mut SpellTarget) -> i32 {
    let shape = t.aim.shape;
    let ray = shape == 5 || shape == 8;
    let out_of_range = |game: &mut Game, t: &SpellTarget| {
        if presenting(game) {
            let m = if t.from_item { game.msg(0x53) } else { game.msg(0x51 + t.mode as usize) };
            ui::message_box(game, 0xb0, 0x28, &m, ui::Wait::Click(0));
        }
    };
    let msg = |game: &mut Game, k: usize| {
        if presenting(game) {
            let m = game.msg(k);
            ui::message_box(game, 0xb0, 0x28, &m, ui::Wait::Click(0));
        }
    };
    let res = match game.battle.check_cast_cell(t) {
        CellCheck::OutOfRange => {
            out_of_range(game, t);
            if t.ready == -1 {
                t.free_area();
                return -1;
            }
            return 0;
        }
        CellCheck::NoTarget => {
            msg(game, 0x54);
            t.free_area();
            return if ray { 0 } else { -1 };
        }
        CellCheck::NoRoom => {
            msg(game, 0x55);
            t.free_area();
            return 0;
        }
        CellCheck::PickDestination => return pick_destination(game, t),
        CellCheck::Confirm => {
            show_aim(game, t);
            let (px, py) = game.battle.cast_panel;
            if t.mode == 2 || !ray {
                cursor::warp(game, px + 0xb8, py + 0x14);
            }
            if (!game.cfg.confirm && !ray) || !presenting(game) {
                (t.confirm == 1) as i32
            } else {
                confirm_loop(game, t)
            }
        }
    };
    if res == 0 || res == -1 {
        t.free_area();
    }
    if t.mode != 2 && res == 1 && ray {
        game.battle.finish_aim(t);
    }
    game.flush_events(0x1f);
    res
}

/// The confirm loop of FUN_004041d7 (input table at 0x404fde).
fn confirm_loop(game: &mut Game, t: &mut SpellTarget) -> i32 {
    let ray = t.aim.shape == 5 || t.aim.shape == 8;
    let mut dirty = true;
    loop {
        actions::pump(game);
        if dirty {
            draw_confirm(game, t);
            dirty = false;
        }
        if !game.get_event(0xffff) || !(1..=0x20).contains(&game.ev.kind) {
            continue;
        }
        let (kind, mods, ex, ey) = (game.ev.kind, game.ev.mods, game.ev.x, game.ev.y);
        let (px, py) = game.battle.cast_panel;
        if kind < 4 {
            let sc = game.ev.scan();
            if mods & ev::M_ALT != 0 && (t.mode == 2 || !ray) {
                let p = &mut game.battle.cast_panel;
                match sc {
                    0x48 if p.1 > 8 => p.1 -= 8,
                    0x50 if p.1 < 0x1ae => p.1 += 8,
                    0x4b if p.0 > 8 => p.0 -= 8,
                    0x4d if p.0 < 0x17c => p.0 += 8,
                    _ => {}
                }
                dirty = true;
                continue;
            }
            if mods & 0x20 != 0 {
                let c = match sc {
                    0x48 => -0x10,
                    0x50 => -0xf,
                    0x4b => -0xe,
                    0x4d => -0xd,
                    0x47 => -0xc,
                    0x49 => -10,
                    0x4f => -0xb,
                    0x51 => -9,
                    _ => 0,
                };
                if c != 0 && game.battle.map_mut().scroll_key(c) {
                    show_aim(game, t);
                    dirty = true;
                }
                continue;
            }
            match sc {
                0x0f => t.confirm ^= 1,
                0x01 => {
                    game.wait_event(ev::KEY_UP);
                    return 0;
                }
                0x1c | 0x39 => {
                    game.wait_event(ev::KEY_UP);
                    return (t.confirm == 1) as i32;
                }
                0x49 => t.dir = 2,
                0x47 => t.dir = 3,
                0x4f => t.dir = 0,
                0x51 => t.dir = 1,
                _ => {}
            }
            dirty = true;
            continue;
        }
        if kind != ev::MOUSE_DOWN && kind != ev::MOUSE_MOVE {
            continue;
        }
        if mods & ev::M_RBUTTON != 0 {
            game.wait_event(ev::MOUSE_UP);
            return 0;
        }
        let quadrant = |game: &mut Game, t: &SpellTarget| -> Option<usize> {
            let &(ax, ay) = t.area.first()?;
            let (sx, sy) = view::cell_screen(game, ax, ay);
            let (sx, sy) = (sx - 0xc, sy - 0xc);
            if ex < sx - 4 || ey < sy - 4 || ex > sx + 0x60 || ey > sy + 0x3c {
                return None;
            }
            Some(if ex <= sx + 0x30 && ey <= sy + 0x1c {
                3
            } else if ey <= sy + 0x1c {
                2
            } else if ex <= sx + 0x2c {
                0
            } else {
                1
            })
        };
        if kind == ev::MOUSE_MOVE {
            if t.mode != 2 && ray {
                if let Some(d) = quadrant(game, t) {
                    t.dir = d;
                    dirty = true;
                }
            } else if ey >= py + 4 && ey <= py + 0x20 {
                if (px + 0xa9..=px + 0xc4).contains(&ex) && t.confirm == 0 {
                    t.confirm = 1;
                    dirty = true;
                } else if (px + 0xc8..=px + 0xe3).contains(&ex) && t.confirm == 1 {
                    t.confirm = 0;
                    dirty = true;
                }
            }
        }
        if mods & ev::M_LBUTTON != 0 {
            game.wait_event(ev::MOUSE_UP);
            if t.mode != 2 && ray {
                if quadrant(game, t).is_some() {
                    return 1;
                }
            } else if ey >= py + 4 && ey <= py + 0x20 {
                if (px + 0xa9..=px + 0xc4).contains(&ex) {
                    return 1;
                }
                if (px + 0xc8..=px + 0xe3).contains(&ex) {
                    return 0;
                }
            }
        }
    }
}

/// Port of FUN_00403f82 (via FUN_00401d75): the 是/否 panel with the
/// question, or for rays and cones the direction arrows round the caster.
fn draw_confirm(game: &mut Game, t: &SpellTarget) {
    if !presenting(game) {
        return;
    }
    camera::redraw(game);
    view::draw_unit(game, t.caster, 0);
    let ray = t.aim.shape == 5 || t.aim.shape == 8;
    if t.mode == 2 || !ray {
        let (px, py) = game.battle.cast_panel;
        if let Ok(bar) = game.res().ui_image(197) {
            gfx::blit(game.scr.page(), &bar, px, py, true);
        }
        let q = if t.from_item { game.msg(0x50) } else { game.msg(0x4e + t.mode as usize) };
        text::large(game, px + 0x58 - text::strlen(&q) * 6, py + 8, &q, 0x2a, 0, 1, 2);
        let k = (t.confirm ^ 1) * 0x1f;
        gfx::rect(game.scr.page(), px + 0xa9 + k, py + 5, px + 0xc4 + k, py + 0x21, 0x3c, false);
    } else if let Some(&(ax, ay)) = t.area.first() {
        let (sx, sy) = view::cell_screen(game, ax, ay);
        let (sx, sy) = (sx - 0xc, sy - 0xc);
        if let Ok(ring) = game.res().ui_image(28) {
            gfx::blit(game.scr.page(), &ring, sx, sy, false);
        }
        let (slot, dx, dy) = match t.dir & 3 {
            2 => (31, 0x3c, 0),
            0 => (29, 0, 0x20),
            3 => (32, 0, 0),
            _ => (30, 0x3c, 0x20),
        };
        if let Ok(a) = game.res().ui_image(slot) {
            gfx::blit(game.scr.page(), &a, sx + dx, sy + dy, false);
        }
    }
    cursor::draw_present(game, None, false);
}

/// Port of FUN_004052e3 (teleport): pick the destination among the area
/// cells round the chosen unit. The pointer shows 9 / 10 over an area cell
/// and 0xf elsewhere; Enter / click on an area cell asks msg 0x4e + mode
/// (0x50 for an item) and returns 1 on 是 (the cursor holds the cell), a
/// cell outside it says msg 0x51 + mode (0x53); Esc / right click → 0.
/// Headless the first area cell is taken.
fn pick_destination(game: &mut Game, t: &mut SpellTarget) -> i32 {
    game.flush_events(0x1f);
    show_aim(game, t);
    if !presenting(game) {
        return match t.area.first() {
            Some(&c) => {
                game.battle.cursor = c;
                1
            }
            None => 0,
        };
    }
    let shape_in = 9 + (t.mode != 0) as usize;
    let mut last = game.battle.cursor;
    let mut scrolled = false;
    loop {
        actions::pump(game);
        let (mut x, mut y) = (0, 0);
        let code = input(game, &mut x, &mut y, 1);
        match code {
            0 => {
                if scrolled {
                    show_aim(game, t);
                    scrolled = false;
                    super::menu::pull_cursor_into_view(game);
                }
                if game.battle.cursor != last {
                    let (cx, cy) = game.battle.cursor;
                    cursor::set_shape(game, if t.in_area(cx, cy) { shape_in } else { 0xf });
                    last = (cx, cy);
                }
                aim_panel(game, t);
            }
            -8 | -3 | 0x1c => {
                let (cx, cy) = game.battle.cursor;
                if t.in_area(cx, cy) {
                    cursor::set_shape(game, shape_in);
                    let q = if t.from_item { game.msg(0x50) } else { game.msg(0x4e + t.mode as usize) };
                    if ui::yes_no(game, 0xb0, 0x28, true, &q, false, false) {
                        return 1;
                    }
                } else {
                    let m = if t.from_item { game.msg(0x53) } else { game.msg(0x51 + t.mode as usize) };
                    ui::message_box(game, 0xb0, 0x28, &m, ui::Wait::Click(0));
                }
            }
            -6 => {}
            -5 => {
                super::menu::follow_cursor(game, &mut |g| {
                    show_aim(g, t);
                    aim_panel(g, t);
                });
                scrolled = true;
            }
            -4 | -1 => {
                t.free_area();
                return 0;
            }
            c => {
                if game.battle.map_mut().scroll_key(c) {
                    scrolled = true;
                    let t0 = game.ticks();
                    camera::redraw_present(game);
                    let sp = game.cfg.screen_speed;
                    game.wait_until(t0, sp);
                }
            }
        }
    }
}

// ---- items in battle -----------------------------------------------------------------------

/// Port of FUN_0044a300's battle branches (FLOW's item screen calls it): a
/// self-use item (not consumable, usage 1) is confirmed and used; a summoning
/// item (usage 2) casts its spell (FUN_0045fdbf); other items are aimed at an
/// own unit in range (FUN_004067e1) and used on it. A used consumable is
/// taken from the inventory. Returns 1 when used.
pub fn use_item_in_battle(game: &mut Game, r: UnitRef, id: u16) -> i32 {
    let Some(it) = game.battle.item(id) else { return 0 };
    let used = if it.kind != 0xc && it.usage == 1 {
        let q = game.msg(0x50);
        if presenting(game) && !ui::yes_no(game, 200, 400, true, &q, false, false) {
            return 0;
        }
        ui::sound(game, 0x7e);
        if presenting(game) {
            let f = game.battle.unit(r).facing & 3;
            view::play_action(game, r, f, 1);
            camera::redraw(game);
            view::draw_unit(game, r, 0);
        }
        apply_item(game, r, id);
        1
    } else if it.usage == 2 {
        item_cast(game, r, it.effect as u16)
    } else {
        let mut res = 0;
        loop {
            let mut t = game.battle.item_target(r.side, r, id);
            let Some((ok, (tx, ty))) = aim_loop(game, &mut t) else { break };
            t.free();
            if !ok {
                break;
            }
            let Some(target) = game.battle.unit_at_cell(r.side, tx, ty, false) else { break };
            let tu = game.battle.unit(target).clone();
            if game.battle.item_equippable(&tu, id) {
                let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
                let f = super::movement::facing_toward(ux, uy, tx, ty, game.battle.unit(r).facing);
                ui::sound(game, 0x7e);
                if presenting(game) {
                    view::play_action(game, r, f, 1);
                    camera::redraw(game);
                    let act = game.battle.unit(r).act;
                    game.battle.unit_mut(r).act = 0;
                    view::flash_unit(game, target, 0x61, false);
                    game.battle.unit_mut(r).act = act;
                    view::draw_unit(game, target, 0);
                    gfx::present(game);
                }
                apply_item(game, target, id);
                res = 1;
                break;
            }
            if presenting(game) {
                let m = game.msg(0x49);
                ui::message_box(game, 0xb0, 0x50, &m, ui::Wait::Click(0));
            }
        }
        res
    };
    if used == 1 && it.kind == 0xc {
        if let Some(c) = (id as usize).checked_sub(1).and_then(|i| game.battle.army_mut(r.side).inventory.get_mut(i)) {
            *c = c.wrapping_sub(1);
        }
    }
    used
}

/// FUN_0044a619 through `Battle::use_item` with the battle's effect hooks
/// and the amount popup.
fn apply_item(game: &mut Game, r: UnitRef, id: u16) {
    let mut fx = BattleEffects::default();
    let amount = game.battle.use_item(r, id, false, &mut fx);
    finish_effects(game, fx);
    // FUN_0044a619's end: page 0 to 1, FUN_00439cc0(unit, -shown).
    if amount != 0 && presenting(game) {
        game.scr.copy_page(1, 0);
        actions::show_miss(game, r, -amount);
    }
    view::bake(game);
}

/// Port of FUN_0045fdbf: an item that casts spell `spell` (no combo spells).
fn item_cast(game: &mut Game, r: UnitRef, spell: u16) -> i32 {
    let Some(sp) = game.battle.t().spells.spells.get((spell as usize).wrapping_sub(1)).cloned() else { return 0 };
    if sp.combo & 7 != 0 {
        return 0;
    }
    let mut cast = Cast::new(r.side, r, spell, (game.battle.unit(r).x, game.battle.unit(r).y));
    cast.from_item = true;
    let magic_schools = game.battle.t().spells.magic_schools as usize;
    let first_tech = game.battle.t().spells.school_spells.get(magic_schools).map_or(0, |s| s.0 as u16);
    cast.technique = first_tech < spell;
    match aim(game, &mut cast) {
        Some(t) => {
            perform_item(game, &mut cast, t);
            1
        }
        None => 0,
    }
}

/// FUN_0045fdbf's FUN_00465f44 + FUN_0045ff9e (an item cast pays nothing).
fn perform_item(game: &mut Game, cast: &mut Cast, mut t: SpellTarget) {
    cast_effects(game, cast, &mut t);
    t.free();
    view::bake(game);
}
