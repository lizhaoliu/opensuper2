//! Turn structure (docs/battle.md §2, §11): phase start and end, upkeep,
//! the end-of-phase terrain effects, AI wake triggers, the phase banner,
//! "end turn", resting, and the phase entry points FLOW's stage loop
//! (`FUN_0040d108`) calls. See the BATTLE API block in `battle/mod.rs`.

use super::actions::{self, presenting, Arg};
use super::army::{Side, UnitRef};
use super::camera;
use super::unit::{flag, status, turn};
use super::view;
use crate::event::{self, StageCheck};
use s2data::script;
use crate::game::Game;
use crate::{gfx, text, ui};

/// Port of FUN_0043311d through the EVENT port: 0 continue, 1 victory, 2 defeat.
pub fn stage_check(game: &mut Game) -> i32 {
    match event::check_stage(game) {
        StageCheck::Continue => 0,
        StageCheck::Victory => 1,
        StageCheck::Defeat => 2,
    }
}

/// Port of FUN_004411cf(army): point the duel, spell and AI objects at
/// `side` and its opponent. Here those objects take the sides per call, so
/// this only refreshes the campaign mirrors.
pub fn phase_begin(game: &mut Game, _side: Side) {
    super::sync(game);
    test_hp_override(game);
}

/// Port of FUN_004233c7 → FUN_004231be: deploy the units of this turn.
pub fn deploy_turn_units(game: &mut Game) {
    super::sync(game);
    let t = game.battle.turn;
    if game.battle.deploy_turn(t) {
        view::redeploy(game);
    }
}

/// TEST-ONLY (`S2_TEST_UNIT_HP=char:hp`, for scripted defeat runs): set that
/// player character's HP once it is on the map. Not part of the original.
fn test_hp_override(game: &mut Game) {
    let Some(v) = std::env::var_os("S2_TEST_UNIT_HP") else { return };
    let v = v.to_string_lossy().to_string();
    let Some((c, h)) = v.split_once(':') else { return };
    let (Ok(c), Ok(h)) = (c.parse::<i16>(), h.parse::<i16>()) else { return };
    if let Some(i) = game.battle.army(Side::Player).find_char(c, 0) {
        let u = &mut game.battle.army_mut(Side::Player).units[i];
        if u.hp > h {
            u.hp = h;
        }
    }
}

/// Port of FUN_00433831: start-of-turn upkeep of both armies (turn ≥ 2),
/// then the stage check (its result is the return value).
pub fn turn_upkeep(game: &mut Game) -> i32 {
    phase_begin(game, Side::Player);
    upkeep(game, Side::Player);
    phase_begin(game, Side::Enemy);
    upkeep(game, Side::Enemy);
    stage_check(game)
}

/// Port of FUN_0045ad90(army): buff and status timers count down (expired
/// ones are removed and the stats recomputed), regeneration, MP regen,
/// poison damage (which can kill), and invisible units are redrawn while the
/// opponent's REVEAL flag is up; that flag is then cleared.
pub fn upkeep(game: &mut Game, side: Side) {
    let reveal = game.battle.army(side.other()).flags & 1 != 0;
    game.battle.army_mut(side.other()).flags &= !1;
    let n = game.battle.army(side).units.len();
    for i in 0..n {
        let r = UnitRef::new(side, i);
        {
            let u = game.battle.unit(r);
            if u.flags & flag::DEPLOYED == 0 || u.flags & flag::DEAD != 0 {
                continue;
            }
        }
        {
            let u = game.battle.unit_mut(r);
            if u.buffs != 0 {
                let mut bit = 1u32;
                for k in 0..9 {
                    let b = &mut u.buff_a[k];
                    if u.buffs & bit != 0 && b.turns > 0 {
                        b.turns -= 1;
                        if b.turns < 1 {
                            u.buffs ^= bit;
                        }
                    }
                    bit <<= 1;
                }
                for k in 0..5 {
                    let b = &mut u.buff_b[k];
                    if u.buffs & bit != 0 && b.turns > 0 {
                        b.turns -= 1;
                        if b.turns < 1 {
                            u.buffs ^= bit;
                        }
                    }
                    bit <<= 1;
                }
                let c = &mut u.buff_c;
                if u.buffs & bit != 0 && c.turns > 0 {
                    c.turns -= 1;
                    if c.turns < 1 {
                        u.buffs ^= bit;
                    }
                }
                u.recompute_buffs();
            }
            if u.status != 0 {
                let mut bit = 1u32;
                for k in 0..0x1b {
                    if u.status & bit != 0 && u.status_turns[k] > 0 {
                        u.status_turns[k] -= 1;
                        if u.status_turns[k] < 1 {
                            u.status ^= bit;
                        }
                    }
                    bit <<= 1;
                }
            }
        }
        let u = game.battle.unit(r).clone();
        if u.status & status::CURSE == 0
            && (u.abilities & 0x100 != 0 || u.status & status::REGEN != 0 || game.battle.has_item_ability(&u, 0x1a))
        {
            let v = ((u.hp_max as i32 * 0xf) / 100).max(10).min(u.hp_max as i32 - u.hp as i32);
            game.battle.unit_mut(r).hp += v as i16;
        }
        if game.battle.has_item_ability(&u, 0xc) {
            let u = game.battle.unit_mut(r);
            if (u.mp_max as i32 - u.mp as i32) < 0xb {
                u.mp = u.mp_max;
            } else {
                u.mp += 10;
            }
        }
        if game.battle.unit(r).buffs & super::unit::buff::POISON != 0 {
            let (x, y) = (u.x, u.y);
            if !game.battle.map().on_screen(x, y, 2) {
                camera::scroll_exact(game, x, y, None);
            }
            let hp = game.battle.unit(r).hp as i32;
            let d = (u.hp_max as i32 / 5).min(200).min(hp);
            if presenting(game) {
                game.scr.set_page(1);
                camera::redraw(game);
                game.scr.set_page(0);
            }
            actions::show_miss(game, r, d);
            game.battle.unit_mut(r).hp -= d as i16;
            if game.battle.unit(r).hp == 0 {
                view::erase_unit(game, r);
                actions::die(game, r, 0);
                continue;
            }
        }
        let u = game.battle.unit(r).clone();
        if u.hp > 0 && game.battle.is_hidden(side, &u) && reveal {
            view::erase_unit(game, r);
            view::put_unit(game, r);
        }
    }
}

/// Port of FUN_004447e6(army): AI wake triggers. `ai_mode` 31..=100 wakes
/// when the unit tagged `ai_mode − 30` has died; 101..=150 on turn
/// `ai_mode − 100`. Waking sets `ai_mode = 1, ai_sub = 0`.
pub fn wake_ai(game: &mut Game, side: Side) {
    super::sync(game);
    let turn_no = game.battle.turn;
    let n = game.battle.army(side).units.len();
    for i in 0..n {
        let u = game.battle.army(side).units[i].clone();
        if u.flags & (flag::DEAD | flag::SUMMON) != 0 || u.flags & flag::DEPLOYED == 0 {
            continue;
        }
        let m = u.ai_mode as i32;
        let wake = if (0x1f..=100).contains(&m) {
            let a = game.battle.army(side);
            a.find_tag(u.ai_mode - 0x1e).is_some_and(|k| a.units[k].flags & flag::DEAD != 0)
        } else {
            m > 100 && m < 0x97 && m - 100 == turn_no
        };
        if wake {
            let w = &mut game.battle.army_mut(side).units[i];
            w.ai_mode = 1;
            w.ai_sub = 0;
        }
    }
}

/// Port of FUN_004448f2(army): after a player action, AI units with
/// `ai_mode` 21..=30 whose trigger tile was touched wake up; then the
/// "actions" event poll runs and the touched list is cleared.
pub fn wake_on_touch(game: &mut Game, side: Side) {
    if game.campaign.vs_mode || event::touched_is_empty(game) {
        return;
    }
    let n = game.battle.army(side).units.len();
    for i in 0..n {
        let u = &game.battle.army(side).units[i];
        if u.flags & (flag::DEAD | flag::SUMMON) != 0 || u.flags & flag::DEPLOYED == 0 {
            continue;
        }
        let m = u.ai_mode;
        if m > 0x14 && m < 0x1f && event::touched_contains(game, m) {
            let w = &mut game.battle.army_mut(side).units[i];
            w.ai_mode = 1;
            w.ai_sub = 0;
        }
    }
    event::poll(game, event::Trigger::Actions);
    event::touched_clear(game);
}

/// Port of FUN_00433453(side): the phase banner. VS: a message box naming
/// the side; otherwise the dimmed map with the side's emblem sliding in and
/// "TURN n" (1 cs per frame).
pub fn phase_banner(game: &mut Game, side: u8) {
    super::trace!("turn {} phase {}", game.campaign.turn, side);
    if !presenting(game) {
        return;
    }
    if game.campaign.vs_mode {
        let a = game.msg(if side == 0 { 0x109 } else { 0x10a });
        let b = game.msg(0x36);
        let s = actions::sprintf(b"%s%s", &[Arg::S(&a), Arg::S(&b)]);
        ui::message_box(game, 0xb0, 200, &s, ui::Wait::Click(300));
        return;
    }
    let imgs = (game.res().ui_image(if side == 0 { 150 } else { 149 }), game.res().ui_image(152), game.res().ui_image(153));
    let (Ok(emblem), Ok(left), Ok(right)) = imgs else {
        super::trace!("phase banner images missing: {:?}", (imgs.0.is_ok(), imgs.1.is_ok(), imgs.2.is_ok()));
        return;
    };
    game.scr.set_page(1);
    camera::redraw(game);
    {
        let (t5, t6) = (game.res().tints[5], game.res().tints[6]);
        let page = &mut game.scr.pages[1].pixels;
        let w = crate::sys::SCREEN_W;
        let start = 121 * w;
        for p in &mut page[start..start + w] {
            *p = t6[*p as usize];
        }
        for p in &mut page[start + w..start + w + 176 * w] {
            *p = t5[*p as usize];
        }
        for p in &mut page[start + 177 * w..start + 178 * w] {
            *p = t6[*p as usize];
        }
    }
    game.scr.set_page(0);
    let num = actions::sprintf(b"%d", &[Arg::D(game.campaign.turn as i32)]);
    let (mut ey, mut tx, mut nx, mut sx) = (0i32, 0i32, 0x26ci32, 1i32);
    let (mut e_done, mut t_done) = (false, false);
    loop {
        actions::pump(game);
        let t0 = game.ticks();
        game.scr.copy_page(0, 1);
        gfx::blit(game.scr.page(), &emblem, 0xd0, ey, false);
        text::large(game, tx, 0xfa, b"TURN", 0x21, 0x24, 1, 0);
        text::large(game, nx, 0xfa, &num, 0x21, 0x24, 1, 0);
        gfx::blit(game.scr.page(), &left, sx - 0x140, 0xea, true);
        gfx::blit(game.scr.page(), &right, 0x27f - sx, 0xea, true);
        gfx::present(game);
        if e_done && t_done {
            break;
        }
        game.wait_until(t0, 1);
        if ey < 100 {
            ey += 0x18;
        } else if ey < 0x8c {
            ey += 0x10;
        } else if ey < 0x96 {
            ey += 8;
        } else {
            e_done = true;
            ey = 0x9f;
        }
        ey = ey.min(0x9f);
        if tx < 200 {
            tx += 0x28;
        } else if tx < 0xe6 {
            tx += 0x1c;
        } else {
            t_done = true;
            tx = 0xf9;
        }
        tx = tx.min(0xf9);
        if nx < 0x1b8 {
            nx = if nx < 0x183 { 0x166 } else { nx - 0x1c };
        } else {
            nx -= 0x28;
        }
        nx = nx.max(0x166);
        if sx < 0x104 {
            sx += 0x3c;
        } else if sx < 0x140 {
            sx += 0x1e;
        }
        sx = sx.min(0x140);
    }
}

/// Port of FUN_00440e71(army): every unit that finished (`act == 2`) is
/// ready again: `act = 0`, turn flags cleared, AP carried over up to
/// rules[0xed] (none if base + carry > 109), `ap = ap_base + carry`.
pub fn phase_reset(game: &mut Game, side: Side) {
    let cap = game.battle.c(0xed);
    for u in &mut game.battle.army_mut(side).units {
        if u.flags & flag::DEPLOYED != 0 && u.flags & flag::DEAD == 0 && u.act == 2 {
            u.act = 0;
            u.turn_flags = 0;
            u.ap_carry = if cap < u.ap as i32 { cap as i16 } else { u.ap };
            if u.ap_base as i32 + u.ap_carry as i32 > 0x6d {
                u.ap_carry = 0;
            }
            u.ap = u.ap_base + u.ap_carry;
        }
    }
    view::bake_army(game, side);
}

/// Port of FUN_0045b29d(army): effects of the tile each unit ends the phase
/// on (overlay class): damage 0xb/0x11 (20%), 0x10 (10%), 0x12 (30%),
/// 0x13..0x15 (20/50/100); refills 0x16..0x19; cleanses 0x1a..0x1d; +10 EXP
/// 0x1e; statuses 0x1f..0x22; 0x23 random teleport; 0x24 withdraw. Shown in
/// a popup for the player's units and visible enemies. Returns the stage check.
pub fn phase_terrain(game: &mut Game, side: Side) -> i32 {
    let n = game.battle.army(side).units.len();
    for i in 0..n {
        let r = UnitRef::new(side, i);
        let u = game.battle.unit(r).clone();
        if u.flags & flag::DEPLOYED == 0 || u.flags & flag::DEAD != 0 {
            continue;
        }
        let class = game.battle.map().object_class(u.x, u.y);
        // (message index, kind: 0 none / 1 damage / 2 cleanse)
        let mut dmg = 0i32;
        let mut kind = 0;
        let mut msg: Option<usize> = None;
        let refill = |game: &mut Game| {
            let u = game.battle.unit_mut(r);
            u.hp = u.hp_max;
            u.mp = u.mp_max;
            u.st = u.st_max;
        };
        const B1: u32 = 0xffff_b974;
        const S1: u32 = 0xfef0_0fff;
        match class {
            0xb | 0x11 => {
                dmg = u.hp_max as i32 / 5;
                kind = 1;
            }
            0x10 => {
                dmg = u.hp_max as i32 / 10;
                kind = 1;
            }
            0x12 => {
                dmg = u.hp_max as i32 * 0x1e / 100;
                kind = 1;
            }
            0x13 => {
                dmg = 0x14;
                kind = 1;
            }
            0x14 => {
                dmg = 0x32;
                kind = 1;
            }
            0x15 => {
                dmg = 100;
                kind = 1;
            }
            0x16 => {
                let u = game.battle.unit_mut(r);
                u.hp = u.hp_max;
                msg = Some(0xef);
            }
            0x17 => {
                let u = game.battle.unit_mut(r);
                u.mp = u.mp_max;
                msg = Some(0xf0);
            }
            0x18 => {
                let u = game.battle.unit_mut(r);
                u.st = u.st_max;
                msg = Some(0xf1);
            }
            0x19 => {
                refill(game);
                msg = Some(0xf2);
            }
            0x1a => {
                {
                    let u = game.battle.unit_mut(r);
                    u.buffs &= B1;
                    u.status &= S1;
                }
                kind = 2;
                refill(game);
                msg = Some(0xf2);
            }
            0x1b => {
                let u = game.battle.unit_mut(r);
                u.buffs &= B1;
                u.status &= S1;
                kind = 2;
            }
            0x1c => {
                let u = game.battle.unit_mut(r);
                u.buffs &= 0xffff_c68b;
                u.status &= 0xf90f_f000;
                kind = 2;
            }
            0x1d => {
                let u = game.battle.unit_mut(r);
                u.buffs = 0;
                u.status = 0;
                kind = 2;
            }
            0x1e => {
                let u = game.battle.unit_mut(r);
                u.exp = u.exp.wrapping_add(10);
                msg = Some(0xf4);
            }
            0x1f => {
                game.battle.unit_mut(r).apply_status(3, status::BLESSING);
                msg = Some(0xf5);
            }
            0x20 | 0x21 => {
                if u.status & status::BARRIER != 0 || u.abilities & 0x4000 != 0 {
                    continue;
                }
                let um = game.battle.unit_mut(r);
                if class == 0x20 {
                    let d = (um.atk_cur as i32 * 0x1e / 100) as i16;
                    um.apply_berserk(d, 3);
                    msg = Some(0xf6);
                } else {
                    um.apply_poison(3);
                    msg = Some(0xf7);
                }
            }
            0x22 => {
                game.battle.unit_mut(r).apply_status(3, status::LIGHT_BODY);
                msg = Some(0xf8);
            }
            0x23 => {
                if let Some((x, y)) = game.battle.random_free_special() {
                    teleport(game, r, x, y);
                }
                continue;
            }
            0x24 => {
                super::hooks::withdraw(game, r);
                continue;
            }
            _ => continue,
        }
        if kind == 1 {
            let hp = game.battle.unit(r).hp as i32;
            if hp < dmg {
                dmg = hp;
            } else if dmg < 1 {
                dmg = 1;
            }
            game.battle.unit_mut(r).hp -= dmg as i16;
            msg = Some(0xee);
        } else if kind == 2 {
            game.battle.unit_mut(r).recompute_buffs();
            view::put_unit(game, r);
            msg = Some(0xf3);
        }
        let u = game.battle.unit(r).clone();
        if let Some(m) = msg {
            if (!game.battle.is_hidden(side, &u) || side == Side::Player) && presenting(game) {
                terrain_popup(game, r, m);
            }
        }
        if game.battle.unit(r).hp == 0 {
            view::erase_unit(game, r);
            actions::die(game, r, 0);
        }
    }
    stage_check(game)
}

/// The "<name> <msg>" window of FUN_0045b29d over a unit (UI image 0xd0, 1 s).
fn terrain_popup(game: &mut Game, r: UnitRef, m: usize) {
    let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
    if !game.battle.map().on_screen(x, y, 2) {
        camera::scroll_exact(game, x, y, None);
    }
    camera::redraw(game);
    let Ok(win) = game.res().ui_image(0xd0) else { return };
    // `*(short *)img + 1`: the header holds width − 1.
    let (w, h) = (win.width as i32, win.height as i32);
    let (sx, sy) = view::cell_screen(game, x, y);
    let sprite_h = game.battle.unit(r).sprite_h as i32;
    let mut wy = sy + 0x20 - sprite_h - h;
    if wy < 0 {
        wy = sy + 0x20;
    } else if wy + h > 0x1d6 {
        wy = 0x1d6 - h;
    }
    let mut wx = sx + 0x20 - w / 2;
    if wx < 0x10 {
        wx = 0x10;
    } else if wx + w > 0x27a {
        wx = 0x27a - w;
    }
    let lut = game.res().tints[6];
    gfx::window_shadow(game.scr.page(), &lut, wx, wy, win.width as i32, win.height as i32);
    gfx::blit(game.scr.page(), &win, wx, wy, false);
    let name = game.battle.unit(r).name.to_vec();
    let mm = game.msg(m);
    let s = actions::sprintf(b"%s %s", &[Arg::S(&name), Arg::S(&mm)]);
    let tx = wx + w / 2 - (s.len() as i32 - 3) * 6 - 0x18;
    text::large(game, tx, wy - 0xc + h / 2, &s, 0x2a, 0, 1, 2);
    gfx::present(game);
    ui::click_to_continue(game, 100);
}

/// Port of FUN_0044e460 with its popup (FUN_0044e75e): rest heal, shown
/// unless the unit is a hidden enemy or `ai_sub == 1`.
pub fn rest(game: &mut Game, r: UnitRef) {
    let (hp, mp, st) = game.battle.rest_heal(r);
    let u = game.battle.unit(r).clone();
    let hidden_enemy = !game.battle.vs && r.side == Side::Enemy && game.battle.is_hidden(r.side, &u);
    if !hidden_enemy && u.ai_sub != 1 && (hp != 0 || mp != 0 || st != 0) {
        game.battle.map_mut().center_on(u.x, u.y, true);
        if presenting(game) {
            game.scr.set_page(1);
            camera::redraw(game);
            game.scr.set_page(0);
            view::panel::recovery_popup(game, r, hp, mp, st);
        }
    }
}

/// Spend the rest cost (when affordable) and rest: the shared tail of
/// FUN_00441038, FUN_0044e210 and FUN_0044d7c1.
pub fn pay_and_rest(game: &mut Game, r: UnitRef) {
    let cost = game.battle.c(0xf0);
    if cost <= game.battle.unit(r).ap as i32 {
        {
            let u = game.battle.unit_mut(r);
            u.ap -= cost as i16;
            if u.ap < u.ap_carry {
                u.ap_carry = u.ap;
            }
        }
        rest(game, r);
    }
}

/// Port of FUN_00441038(army) (F6 / 回合結束): every unit that can still act
/// and is controllable rests and finishes; berserk units are played by the
/// AI instead (stops on victory or defeat).
pub fn end_turn(game: &mut Game, side: Side) {
    let npc_ok = game.battle.army(side).flags & super::army::aflag::NPC_CONTROL != 0;
    let n = game.battle.army(side).units.len();
    for i in 0..n {
        actions::pump(game);
        let r = UnitRef::new(side, i);
        let u = game.battle.unit(r).clone();
        if !((u.npc != 1 || npc_ok) && u.act == 0 && u.can_act() && u.flags & flag::DEPLOYED != 0) {
            continue;
        }
        if u.buffs & super::unit::buff::BERSERK == 0 {
            pay_and_rest(game, r);
            game.battle.unit_mut(r).act = 2;
            view::put_unit(game, r);
            if presenting(game) {
                game.scr.set_page(0);
                camera::redraw_present(game);
            }
        } else {
            ai_act(game, r);
            let c = stage_check(game);
            if c == 1 || c == 2 {
                return;
            }
        }
    }
}

/// Port of FUN_0044e210: the end of an AI unit's action: rest when it can
/// afford it, face the nearest threat, mark `turn_flags |= 0x80`.
pub fn ai_finish(game: &mut Game, r: UnitRef) {
    let u = game.battle.unit(r).clone();
    if u.flags & flag::DEAD != 0 || u.turn_flags & turn::END_DONE != 0 {
        return;
    }
    // FUN_00442022 + FUN_00441c0b, then FUN_00440264 after turning: the
    // unit is drawn into the map surface for the frame below.
    view::capture(game, r);
    view::erase_unit(game, r);
    super::menu::face_threat(game, r);
    actions::set_anim(game, r, 0);
    view::put_unit(game, r);
    if u.ai_sub != 1 {
        if !game.battle.map().on_screen(u.x, u.y, 2) {
            game.battle.map_mut().center_on(u.x, u.y, true);
        }
        camera::redraw_present(game);
    }
    pay_and_rest(game, r);
    view::erase_unit(game, r);
    super::menu::after_action(game, r);
    game.battle.unit_mut(r).turn_flags |= turn::END_DONE;
}

/// Port of FUN_0040a443(&DAT_00492af0, unit): the AI plays one unit
/// (`ai::decide`, FUN_0040a4d0), then the bookkeeping.
pub fn ai_act(game: &mut Game, r: UnitRef) {
    // FUN_0040a4d0 starts with the pump.
    actions::pump(game);
    super::ai::decide(game, r);
    view::hide_range(game);
    game.battle.unit_mut(r).act = 2;
    actions::set_anim(game, r, 0);
    view::capture(game, r);
    view::put_unit(game, r);
    view::bake(game);
    camera::redraw_full(game);
    super::menu::cave_check(game, r);
    super::menu::treasure_check(game, r);
}

/// Port of FUN_00440fa1(army): no unit is left that is ready (`act == 0`),
/// able to act, on the map and controllable.
pub fn all_done(b: &super::Battle, side: Side) -> bool {
    let a = b.army(side);
    let npc_ok = a.flags & super::army::aflag::NPC_CONTROL != 0;
    !a.units
        .iter()
        .any(|u| u.act == 0 && (u.npc != 1 || npc_ok) && u.can_act() && u.flags & flag::DEPLOYED != 0)
}

/// Port of FUN_00433e6e(army, x, y, mode): the next unit for the AI loop,
/// nearest to (x, y) — or, from (0, 0), nearest to an idle opponent. Mode 0
/// any ready unit, 1 ready NPCs or berserk units, 2 any living unit on the map.
pub fn nearest_unit(b: &super::Battle, side: Side, x: i32, y: i32, mode: i32) -> Option<UnitRef> {
    let a = b.army(side);
    let mut best: Option<(i32, usize)> = None;
    for (i, u) in a.units.iter().enumerate() {
        let ready = u.act == 0 && u.can_act() && u.flags & flag::DEPLOYED != 0;
        let pick = ((mode == 0 || ((u.npc == 1 || u.buffs & super::unit::buff::BERSERK != 0) && mode == 1)) && ready)
            || (mode == 2 && u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0);
        if !pick {
            continue;
        }
        let d = if x == 0 && y == 0 {
            nearest_idle_distance(b, side.other(), u.x, u.y)
        } else {
            super::map::distance(x, y, u.x, u.y)
        };
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| UnitRef::new(side, i))
}

/// Port of FUN_00433fa8(army, x, y): distance to the nearest idle (`act == 0`)
/// living unit of `side` on the map, 0xffff if none.
fn nearest_idle_distance(b: &super::Battle, side: Side, x: i32, y: i32) -> i32 {
    b.army(side)
        .units
        .iter()
        .filter(|u| u.act == 0 && u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0)
        .map(|u| super::map::distance(x, y, u.x, u.y))
        .fold(0xffff, i32::min)
}

/// Port of FUN_00433db9(army, mode) (FUN_00433d8f mode 0 = 自動作戰, FUN_00433da4
/// mode 1 = NPC allies / berserk units, FUN_0043310e = the enemy phase):
/// the AI plays units nearest-first until none is left. Returns 0 when done,
/// 1 victory, 2 defeat.
pub fn auto_phase(game: &mut Game, side: Side) -> i32 {
    auto_phase_mode(game, side, 0)
}

fn auto_phase_mode(game: &mut Game, side: Side, mode: i32) -> i32 {
    let (mut x, mut y) = (0, 0);
    let mut c = 0;
    loop {
        actions::pump(game);
        let Some(r) = nearest_unit(&game.battle, side, x, y, mode) else { break };
        (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
        game.battle.cursor = (x, y);
        view::cursor_set(game, x, y);
        ai_act(game, r);
        c = stage_check(game);
        if c == 1 {
            return 1;
        }
        if c == 2 {
            return 2;
        }
    }
    c
}

/// Port of FUN_00433da4(army): NPC allies and berserk units act (end of the player phase).
pub fn npc_phase(game: &mut Game, side: Side) -> i32 {
    auto_phase_mode(game, side, 1)
}

/// Port of FUN_0043310e: the enemy phase. Returns 0, 1 (victory) or 2 (defeat).
pub fn enemy_phase(game: &mut Game) -> i32 {
    npc_phase(game, Side::Enemy)
}

/// Port of FUN_0042b097: the items of the chests still on the map — one
/// random entry (of four) of each unlocked non-door spot's treasure, when
/// set (ids 1..=0x12e); 0-based item indices.
fn unopened_treasure(game: &mut Game) -> Vec<usize> {
    let count = game.events.treasure_count;
    let mut out = Vec::new();
    for code in game.battle.map().chest_spots() {
        let k = (code & 0xff) as i32;
        if k == 0 || k > count {
            continue;
        }
        let pick = game.battle.rng.rnd(4) as usize;
        let id = game.events.treasure.get(k as usize - 1).map_or(0, |t| t[pick]) as usize;
        if count > 0 && id != 0 && id < 0x12f {
            out.push(id - 1);
        }
    }
    out
}

/// Port of FUN_0045cfad(&player): the result window and the stage rewards
/// (party joins / leaves, gold, items), applied as they are listed; with
/// army flag 0x40 (treasure search) also the unopened chests' items.
pub fn victory_rewards(game: &mut Game) {
    let Some(st) = game.stage.as_ref() else { return };
    if st.stage.header.reward_groups == 0 {
        return;
    }
    let rewards = st.stage.rewards.clone();
    let stage_name = st.stage.name.clone();
    let show = presenting(game);
    let (mut x0, mut y0) = (0, 0);
    if show {
        game.scr.copy_page(0, 1);
        if let Ok(win) = game.res().ui_image(0xcf) {
            x0 = 0x140 - win.width as i32 / 2;
            y0 = 0xf0 - win.height as i32 / 2;
            gfx::blit(game.scr.page(), &win, x0, y0, false);
        }
        let t = game.msg(0x22);
        text::large(game, x0 + 0x136 - text::strlen(&t) * 6, y0 + 0x11, &t, 0x39, 0, 1, 2);
        let m = game.msg(0x30);
        text::large(game, x0 + 0x5a, y0 + 0x39, &m, 0x36, 0, 1, 2);
        text::large(game, x0 + 0xd5, y0 + 0x39, &stage_name, 0x36, 0, 1, 2);
    }
    let lx = x0 + 0x5a;
    let mut ly = y0 + 0x6c;
    let rows = |n: usize, h: i32| ((n as i32 - 1) / 2 + 1) * h;
    for r in rewards.iter().take(game.stage.as_ref().map_or(0, |s| s.stage.header.reward_groups as usize)) {
        match r {
            script::Reward::Join(ids) | script::Reward::Leave(ids) => {
                let join = matches!(r, script::Reward::Join(_));
                if show {
                    let m = game.msg(if join { 0x23 } else { 0x24 });
                    text::large(game, lx, ly, &m, 0x77, 0x2a, 1, 1);
                }
                for (k, &ch) in ids.iter().enumerate() {
                    let a = game.battle.army_mut(Side::Player);
                    let Some(i) = a.find_char(ch, if join { 1 } else { 2 }) else { continue };
                    let u = &mut a.units[i];
                    if join {
                        u.npc = 0;
                        u.ai_mode = 1;
                        u.ai_sub = 0;
                        u.flags &= !flag::RESERVE;
                    } else {
                        u.flags |= flag::RESERVE;
                        u.flags &= !flag::DEAD;
                    }
                    let name = u.name.to_vec();
                    if show {
                        text::large(game, lx + 0x73 + (k as i32 & 1) * 0xa3, ly + (k as i32 / 2) * 0x19, &name, 0x77, 0x2a, 1, 1);
                    }
                }
                ly += rows(ids.len(), 0x19);
            }
            script::Reward::Gold(g) => {
                if show {
                    let m = game.msg(0x25);
                    text::large(game, lx, y0 + 0x14a, &m, 0x77, 0x2a, 1, 1);
                    let s = actions::sprintf(b"%d", &[Arg::D(*g as i32)]);
                    text::large(game, lx + 0x82, y0 + 0x14a, &s, 0x77, 0x2a, 1, 1);
                }
                game.battle.army_mut(Side::Player).gold += *g as i32;
            }
            script::Reward::Items(ids) => {
                if show {
                    let m = game.msg(0x26);
                    text::large(game, lx, ly, &m, 0x77, 0x2a, 1, 1);
                }
                for (k, &id) in ids.iter().enumerate() {
                    if show {
                        let (ix, iy) = (lx + (k as i32 & 1) * 0xa8, ly + (k as i32 / 2) * 0x1e);
                        if let Ok(icon) = game.res().ui_image(id as usize + 0x1ec) {
                            gfx::blit(game.scr.page(), &icon, ix + 0x6c, iy, false);
                        }
                        let name = game.battle.item_name_raw(id as u16);
                        text::large(game, ix + 0x92, iy, &name, 0x77, 0x2a, 1, 1);
                    }
                    // The original adds without the 99 cap here.
                    if let Some(c) = (id as usize).checked_sub(1).and_then(|i| game.battle.army_mut(Side::Player).inventory.get_mut(i)) {
                        *c = c.wrapping_add(1);
                    }
                }
                ly += rows(ids.len(), 0x1e);
            }
            _ => {}
        }
    }
    if game.battle.army(Side::Player).flags & super::army::aflag::TREASURE != 0 && game.events.treasure_count > 0 {
        let found = unopened_treasure(game);
        if !found.is_empty() && show {
            let m = game.msg(0x27);
            text::large(game, lx, ly, &m, 0x77, 0x2a, 1, 1);
        }
        for (k, &i) in found.iter().enumerate() {
            if show {
                let (ix, iy) = ((k as i32 & 1) * 0xa8, ly + (k as i32 / 2) * 0x1e);
                if let Ok(icon) = game.res().ui_image(i + 0x1ed) {
                    gfx::blit(game.scr.page(), &icon, lx + 0x6c + ix, iy, false);
                }
                let name = game.battle.item_name_raw(i as u16 + 1);
                text::large(game, lx + 0x92 + ix, iy, &name, 0x77, 0x2a, 1, 1);
            }
            if let Some(c) = game.battle.army_mut(Side::Player).inventory.get_mut(i) {
                if *c < 99 {
                    *c += 1;
                }
            }
        }
    }
    if show {
        gfx::present(game);
        ui::click_to_continue(game, 0);
    }
}

/// Port of FUN_0045d713(&player): every survivor on the map gains 30 EXP
/// (with its level-up windows) and, through FUN_0045d7e1, rules[0xfb] of
/// affinity toward every other survivor.
pub fn victory_exp(game: &mut Game) {
    let n = game.battle.army(Side::Player).units.len();
    for i in 0..n {
        let r = UnitRef::new(Side::Player, i);
        let u = game.battle.unit(r);
        if u.flags & flag::DEPLOYED == 0 || u.flags & flag::DEAD != 0 {
            continue;
        }
        let (x, y) = (u.x, u.y);
        game.battle.unit_mut(r).exp = game.battle.unit(r).exp.wrapping_add(0x1e);
        game.battle.map_mut().center_on(x, y, true);
        if presenting(game) {
            camera::redraw(game);
            game.scr.copy_page(1, 0);
        }
        actions::level_check(game, r);
        view::put_unit(game, r);
        bond_survivors(game, r);
    }
}

/// Port of FUN_0045d7e1: `r`'s affinity toward each other deployed living
/// unit of its army (character ids 1..=40) grows by rules[0xfb], clamped
/// to a signed byte.
fn bond_survivors(game: &mut Game, r: UnitRef) {
    let gain = game.battle.c(0xfb);
    let ids: Vec<i16> = game
        .battle
        .army(r.side)
        .units
        .iter()
        .enumerate()
        .filter(|&(i, u)| i != r.idx && u.flags & flag::DEPLOYED != 0 && u.flags & flag::DEAD == 0)
        .map(|(_, u)| u.char_id)
        .collect();
    let u = game.battle.unit_mut(r);
    for id in ids {
        let k = id as i32 - 1;
        if (0..0x28).contains(&k) {
            let v = (u.affinity[k as usize] as i8 as i32 + gain).clamp(-0x80, 0x7f);
            u.affinity[k as usize] = v as i8 as u8;
        }
    }
}

/// Port of FUN_0040d729: the adaptive difficulty at stage start. With the
/// stage header's scale base and step (bytes 0x17/0x18, `DAT_0049469f/a0`),
/// `k = (average player power − base) / step` (clamped to ±4) gives an enemy
/// stat bonus of `25·k` % (or `−10·k` % for k < 0, as the original computes
/// it) plus 5 % per difficulty level above 3.
pub fn adapt_difficulty(game: &mut Game) {
    let Some(st) = game.stage.as_ref() else { return };
    let (base, step) = (st.stage.header.scale_base as i32, st.stage.header.scale_step as i32);
    if base == 0 || step == 0 {
        return;
    }
    super::sync(game);
    let avg = army_power(&game.battle, Side::Player);
    let k = ((avg - base) / step).clamp(-4, 4);
    if k == 0 {
        return;
    }
    let pct = if k < 1 { k * -10 } else { k * 0x19 } + (game.battle.difficulty as i32 - 3) * 5;
    if pct != 0 {
        scale_army(&mut game.battle, Side::Enemy, pct);
    }
}

/// Port of FUN_00457c25(army): average power of the deployed, non-NPC,
/// living, non-summon units (0 if none).
pub fn army_power(b: &super::Battle, side: Side) -> i32 {
    let (mut sum, mut n) = (0, 0);
    for u in &b.army(side).units {
        if u.flags & flag::DEPLOYED != 0 && u.npc != 1 && u.flags & (flag::DEAD | flag::SUMMON) == 0 {
            sum += b.power(u, 0, 0);
            n += 1;
        }
    }
    if n > 0 {
        sum / n
    } else {
        sum
    }
}

/// Port of FUN_00457ccb(army, pct): set the army's stat bonus and apply it
/// (FUN_00457d3c) to every deployed, living, non-summon unit.
pub fn scale_army(b: &mut super::Battle, side: Side, pct: i32) {
    b.army_mut(side).stat_bonus_pct = pct;
    for i in 0..b.army(side).units.len() {
        let u = &b.army(side).units[i];
        if u.flags & flag::DEPLOYED != 0 && u.flags & (flag::DEAD | flag::SUMMON) == 0 {
            b.army_stat_bonus(UnitRef::new(side, i));
        }
    }
}

/// Port of FUN_0042d479: the objectives window (勝利條件): stage number and
/// name, the win and lose texts, and the unit counts (own, NPC allies, enemies).
pub fn objectives_screen(game: &mut Game) {
    if game.campaign.vs_mode || !presenting(game) {
        return;
    }
    // FUN_00433a1f first: the pointer's background back.
    crate::cursor::erase(game);
    let Ok(win) = game.res().ui_image(0xcf) else { return };
    let x0 = 0x140 - win.width as i32 / 2;
    let oy = -(win.height as i32 / 2);
    gfx::blit(game.scr.page(), &win, x0, oy + 0xf0, false);
    let title = game.msg(0x2f);
    text::large(game, x0 + 0x140 - text::strlen(&title) * 6, oy + 0x101, &title, 0x39, 0, 1, 2);
    let (m30, m31, m32) = (game.msg(0x30), game.msg(0x31), game.msg(0x32));
    text::large(game, x0 + 0x5a, oy + 0x129, &m30, 0x36, 0, 1, 2);
    text::large(game, x0 + 0x5a, oy + 0x15c, &m31, 0x77, 0x2a, 1, 1);
    text::large(game, x0 + 0x5a, oy + 0x1c0, &m32, 0x57, 0x2a, 1, 1);
    let (name, win_text, lose_text) = match game.stage.as_ref() {
        Some(s) => (s.stage.name.clone(), s.stage.win_text.clone(), s.stage.lose_text.clone()),
        None => Default::default(),
    };
    let head = actions::sprintf(b"%d.%s", &[Arg::D(game.campaign.battles_fought as i32), Arg::S(&name)]);
    text::large(game, x0 + 0xd5, oy + 0x129, &head, 0x36, 0, 1, 2);
    text::block(game, x0 + 0xd5, oy + 0x15c, &win_text, 0x77, 0x2a, 1, 1, 4, 0xb, 0x1c);
    text::block(game, x0 + 0xd5, oy + 0x1c0, &lose_text, 0x57, 0x2a, 1, 1, 4, 0xb, 0x1c);
    let (m13, m14) = (game.msg(0x13), game.msg(0x14));
    text::large(game, x0 + 0x56, oy + 0x23a, &m13, 0x28, 0x2b, 1, 0);
    text::large(game, x0 + 0x108, oy + 0x23a, b"NPC", 0x26, 0x2b, 1, 0);
    text::large(game, x0 + 0x1b4, oy + 0x23a, &m14, 0x3c, 0x2b, 1, 0);
    // FUN_0043aea6 / FUN_0043af1c(army, 0): living units on the map (NPCs apart).
    let live = |game: &Game, side: Side, npc_only: bool| {
        game.battle
            .army(side)
            .units
            .iter()
            .filter(|u| u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0 && (!npc_only || u.npc == 1))
            .count() as i32
    };
    let npc = live(game, Side::Player, true);
    let own = live(game, Side::Player, false) - npc;
    let foes = live(game, Side::Enemy, false);
    crate::flow::town::large_number(game, x0 + 0x90, oy + 0x23a, own, 0xb9, 0, 1, 2, 0);
    crate::flow::town::large_number(game, x0 + 0x140, oy + 0x23a, npc, 0xb9, 0, 1, 2, 0);
    crate::flow::town::large_number(game, x0 + 0x1ee, oy + 0x23a, foes, 0xb9, 0, 1, 2, 0);
    gfx::present(game);
    ui::click_to_continue(game, 0);
}

/// Port of FUN_00458334(army, unit, x, y): scroll to the unit, take it off
/// the map, scroll to (x, y), put it there and let it appear (tint 0xc2).
pub fn teleport(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    let (x0, y0) = (game.battle.unit(r).x, game.battle.unit(r).y);
    camera::ensure_visible(game, x0, y0, None);
    let act = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = 1;
    view::erase_unit(game, r);
    camera::redraw_present(game);
    camera::ensure_visible(game, x, y, None);
    {
        let u = game.battle.unit_mut(r);
        u.x = x;
        u.y = y;
    }
    if presenting(game) {
        view::flash_unit(game, r, 0xc2, true);
        view::draw_unit(game, r, 0);
        gfx::present(game);
    }
    game.battle.unit_mut(r).act = act;
    view::capture(game, r);
    view::put_unit(game, r);
    if presenting(game) {
        game.scr.copy_page(1, 0);
    }
}

impl super::Battle {
    /// Port of FUN_0042b230(map, &x, &y): a random cell among the map's
    /// object-class 0x23 cells that no unit of either army stands on.
    pub fn random_free_special(&mut self) -> Option<(i32, i32)> {
        let cells: Vec<(i32, i32)> = self
            .map()
            .special
            .iter()
            .map(|&(x, y)| (x as i32, y as i32))
            .filter(|&(x, y)| self.army(Side::Player).unit_at(x, y) == 0 && self.army(Side::Enemy).unit_at(x, y) == 0)
            .collect();
        if cells.is_empty() {
            return None;
        }
        let k = self.rng.rnd(cells.len() as i32) as usize;
        Some(cells[k])
    }
}

/// FUN_0043abc2's `if (0 < DAT_00497b48) DAT_00497b48--` per equipped item
/// with ability 1, for the count `Battle::recalc_army_flags` returned.
pub fn pay_fame(game: &mut Game, n: i32) {
    for _ in 0..n {
        if game.campaign.fame > 0 {
            game.campaign.fame -= 1;
        }
    }
}
