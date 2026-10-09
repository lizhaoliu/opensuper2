//! Turn-1 deployment after the march: the record split (`FUN_0041641c`),
//! the deployment screen (`FUN_00444bea` with `FUN_00445c2e`, `FUN_00445dfe`,
//! `FUN_00445f73`, `FUN_00446293`, `FUN_00446095`), placing a member on the
//! battlefield (`FUN_00416ffb` with `FUN_00416967`, `FUN_00416a09`,
//! `FUN_00416a75`, `FUN_00416c1f`) and the late joiners (`FUN_0041671c`).

use anyhow::Result;
use s2data::gfx_portrait::PortraitKind;
use s2data::sprite::Image;

use crate::battle::unit::flag;
use crate::battle::{camera, menu, view, Side, UnitRef};
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;
use crate::ui::{self, Wait};

/// One deployment cell (`DAT_00492bb4`, 6 ints per slot).
#[derive(Debug, Clone, Copy)]
struct Slot {
    x: i32,
    y: i32,
    facing: u8,
    /// Record +0: the character the cell is meant for.
    character: i16,
    /// 1-based index of the member placed here (0 = free).
    member: usize,
    /// Record +0xf bit 0x10: that character is placed here unconditionally.
    forced: bool,
}

/// Records of the current stage whose appear window contains `turn`.
fn appearing(game: &Game, range: std::ops::Range<usize>, turn: i32) -> Vec<usize> {
    range
        .filter(|&i| {
            let d = &game.battle.defs[i];
            let from = d.p.b33 as i8 as i32;
            if d.appear_to == 0 {
                from == turn
            } else {
                from <= turn && turn <= d.appear_to as i32
            }
        })
        .collect()
}

/// Port of `FUN_0041641c`: create this turn's enemies and the player-side
/// units that need no choice; collect the cells the player fills
/// (control 0) and the joiners placed by hand (control 6, announced).
fn split_records(game: &mut Game) -> Vec<usize> {
    let turn = game.campaign.turn as i32;
    let (es, en, np) = (game.battle.enemy_defs_start, game.battle.n_enemy_defs, game.battle.n_player_defs);
    let enemies = appearing(game, es..es + en, turn);
    if !enemies.is_empty() {
        game.battle.create_units(Side::Enemy, &enemies);
    }
    let mut slots = Vec::new();
    let mut direct = Vec::new();
    let mut announced = false;
    for rec in appearing(game, 0..np, turn) {
        match game.battle.defs[rec].p.control {
            6 => {
                slots.push(rec);
                // FUN_0043d8b5: the joiner, kept off the map until placed.
                if let Some(&r) = game.battle.create_units(Side::Player, &[rec]).first() {
                    game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
                }
                if !announced {
                    game.scr.clear();
                    let m = game.res().master.clone();
                    gfx::set_palette(game, &m);
                }
                let mut s = s2data::text::cstr(&game.battle.defs[rec].name_raw).to_vec();
                s.push(b' ');
                s.extend_from_slice(&game.msg(0x6f));
                ui::message_box(game, 0x10c, 0x28, &s, Wait::Click(0));
                announced = true;
            }
            0 => slots.push(rec),
            7 => {}
            _ => direct.push(rec),
        }
    }
    if !direct.is_empty() {
        game.battle.create_units(Side::Player, &direct);
    }
    view::reset_surface(game);
    view::bake(game);
    if announced {
        let m = game.res().master.clone();
        gfx::fade(game, &m, 1, 0);
    }
    slots
}

/// Port of `FUN_0041671c`: the records that join after the choice (control 7).
/// The rest of the original frees the sprite sets no deployed unit uses
/// (`FUN_0044654d`, `FUN_004466ee`, `FUN_004467a8`), which the view does itself.
fn late_joiners(game: &mut Game) {
    let turn = game.campaign.turn as i32;
    let np = game.battle.n_player_defs;
    for rec in appearing(game, 0..np, turn) {
        if game.battle.defs[rec].p.control == 7 {
            game.battle.create_from_record(Side::Player, rec, 0);
        }
    }
    // FUN_0044654d / FUN_004467a8: the idle members' sprite types are dropped;
    // those turned back to facing 0 get FUN_00441ac4(army, unit, 0).
    for r in game.battle.slots_after_deploy() {
        view::set_anim(game, r, 0);
    }
    view::reset_surface(game);
    view::bake(game);
}

/// The deployment step of `FUN_00415fd2`: split the records, run the
/// screen when there are cells to fill, then the late joiners.
pub fn deploy(game: &mut Game) -> Result<()> {
    let recs = split_records(game);
    if recs.is_empty() {
        return Ok(());
    }
    let mut slots: Vec<Slot> = recs
        .iter()
        .map(|&rec| {
            let d = &game.battle.defs[rec];
            // Record +0xf: the parser keeps only the gender nibble; bit 0x10 is read raw.
            let flags = game.stage.as_ref().map_or(0, |st| {
                let base = st.battle_stage.header.units_offset as usize + 101 * rec;
                st.battle_stage.block.get(base + 0xf).copied().unwrap_or(0)
            });
            Slot { x: d.p.x as i32, y: d.p.y as i32, facing: d.p.facing, character: d.p.character, member: 0, forced: flags & 0x10 != 0 }
        })
        .collect();
    game.campaign.battles_fought += 1;
    let r = deploy_screen(game, &mut slots);
    game.campaign.battles_fought -= 1;
    r?;
    late_joiners(game);
    Ok(())
}

/// The members the screen offers (`FUN_0043af1c(army, 1)`).
fn members(game: &Game) -> Vec<UnitRef> {
    super::stage::units_of(game, Side::Player)
        .into_iter()
        .filter(|&r| {
            let u = game.battle.unit(r);
            u.flags & (flag::DEAD | flag::RESERVE | flag::SUMMON) == 0 && u.npc != 1
        })
        .collect()
}

/// Put member `r` on `slot` (the shared part of `FUN_00444bea` / `FUN_00446095`).
fn place(game: &mut Game, r: UnitRef, s: &Slot) {
    if game.battle.unit(r).facing as i32 != s.facing as i32 {
        view::step_unit(game, r, s.facing);
    }
    let u = game.battle.unit_mut(r);
    u.flags |= flag::DEPLOYED;
    u.x = s.x;
    u.y = s.y;
}

/// Screen state of `FUN_00444bea`.
struct Screen {
    units: Vec<UnitRef>,
    faces: Vec<Option<Image>>,
    /// `local_b4`: 1-based slot of each member (0 = not deployed).
    slot_of: Vec<usize>,
    /// `local_b0`: placed by the record itself (a forced cell).
    fixed: Vec<bool>,
    deployed: usize,
    max: usize,
    top: usize,
    sel: usize,
    /// `local_6c`: 0 = nothing focused yet (Enter and the arrows do nothing
    /// until Tab or the pointer picks a row), 1 = member cards, 2 = buttons.
    mode: i32,
    button: usize,
}

/// Port of `FUN_00445dfe`: five member cards with portrait, name and class.
fn draw_cards(game: &mut Game, s: &Screen, tint: &[u8; 256]) {
    for k in 0..5usize {
        let i = s.top + k;
        let Some(&r) = s.units.get(i) else { break };
        let cx = k as i32 * 0x7a + 0x4b;
        let color = if let Some(face) = &s.faces[i] {
            // `(*(short *)(face + 1) + 1) / 2`: the header holds width - 1.
            let x = cx - face.width as i32 / 2;
            if s.slot_of[i] == 0 {
                gfx::blit(game.scr.page(), face, x, 8, false);
                0x36
            } else {
                // FUN_0047004f: drawn through tint table 6 (FUN_0047009e raw / FUN_00470259 RLE).
                let mut dim = face.clone();
                for p in dim.pixels.iter_mut().filter(|p| **p != 0) {
                    *p = tint[*p as usize];
                }
                gfx::blit(game.scr.page(), &dim, x, 8, false);
                0x2a
            }
        } else if s.slot_of[i] == 0 {
            0x36
        } else {
            0x2a
        };
        let u = game.battle.unit(r).clone();
        let name = s2data::text::cstr(&u.name).to_vec();
        text::large(game, cx - name.len() as i32 * 6, 0x8e, &name, color, 0, 1, 2);
        let class = game.res().class_name(u.kind as usize).to_vec();
        text::large(game, cx - class.len() as i32 * 6, 0xab, &class, color, 0, 1, 2);
    }
}

/// Port of `FUN_00445c2e`: the counters' labels and the five buttons.
fn draw_labels(game: &mut Game) {
    let a = game.msg(0xb9);
    text::large(game, 0xfe, 0x109, &a, 0x2a, 0, 1, 2);
    let b = game.msg(0xba);
    text::large(game, 0x1c1, 0x109, &b, 0x2a, 0, 1, 2);
    for k in 0..5usize {
        let m = game.msg(0xbb + k);
        text::large(game, 0x41 + k as i32 * 0x70 - m.len() as i32 * 6, 0xd9, &m, 0x6c, 0x37, 1, 1);
    }
}

/// Port of `FUN_00445f73`: the selected member's sprite, stats and equipment.
fn draw_member(game: &mut Game, r: UnitRef) {
    super::party::draw_unit(game, r, 0x87, 0x1b3);
    super::status::stat_bars(game, r, 0x104, 0x137);
    super::status::combat_stats(game, r, 0x134, 0x18a, 0xc9);
    let u = game.battle.unit(r).clone();
    let mut s = u.level.to_string().into_bytes();
    s.push(b' ');
    s.extend_from_slice(&game.msg(0x71));
    text::large(game, 0x87 - s.len() as i32 * 6, 0x1c0, &s, 0x6c, 0x37, 1, 1);
    let mut y = 0x13f;
    for &id in u.equip.iter() {
        if id != 0 {
            let name = game.res().item_name(id as usize).to_vec();
            text::small(game, 0x20f, y, &name, 0x38, 0, 1, 2);
            y += 0x16;
        }
    }
}

/// Port of `FUN_00444bea(army, slots, n)`: choose who goes into battle.
fn deploy_screen(game: &mut Game, slots: &mut [Slot]) -> Result<()> {
    let bg = game.res().dat7_image(1)?;
    let units = members(game);
    let n = units.len();
    let max = slots.len().min(n);
    if n == 0 {
        return Ok(());
    }
    // FUN_00422353: dialogue portraits by character id (generic: class byte 0xd).
    let faces: Vec<Option<Image>> = units
        .iter()
        .map(|&r| {
            let u = game.battle.unit(r);
            let id = if (1..200).contains(&u.char_id) { u.char_id as usize } else { game.res().class_rec(u.kind as usize).get(0xd).copied().unwrap_or(0) as usize };
            game.res().portraits.get(id, PortraitKind::Dialogue).ok()
        })
        .collect();
    let mut s = Screen { units, faces, slot_of: vec![0; n], fixed: vec![false; n], deployed: 1, max, top: 0, sel: 0, mode: 0, button: 0, };
    // Forced cells take their character; without any, the first member
    // takes the first cell.
    let mut any = false;
    for k in 0..max {
        for i in 0..n {
            let r = s.units[i];
            if slots[k].forced && !s.fixed[i] && slots[k].character == game.battle.unit(r).char_id {
                s.slot_of[i] = 1;
                s.fixed[i] = true;
                slots[k].member = i + 1;
                place(game, r, &slots[k]);
                any = true;
            }
        }
    }
    if !any {
        s.fixed[0] = true;
        slots[0].member = 1;
        s.slot_of[0] = 1;
        let r = s.units[0];
        place(game, r, &slots[0]);
    }
    view::reset_surface(game);
    view::bake(game);
    ui::drop_presses(game);
    if max == 1 {
        super::stop_music(game);
        return Ok(());
    }
    super::music(game, 0x19);
    let tint = game.res().tints[6];
    let arrows = (game.res().ui_image(6).ok(), game.res().ui_image(2).ok());
    let (mut full, mut cards, mut redraw, mut moved, mut fade_pending) = (true, true, true, false, true);
    let mut delay = false;
    let mut t_delay = 0;
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if full {
            game.scr.set_page(2);
            gfx::blit(game.scr.page(), &bg, 0, 0, true);
            draw_labels(game);
            full = false;
            cards = true;
        }
        if cards {
            game.scr.copy_page(1, 2);
            game.scr.set_page(1);
            draw_cards(game, &s, &tint);
            game.scr.set_page(0);
            cards = false;
            redraw = true;
        }
        if redraw {
            game.scr.copy_page(0, 1);
            draw_member(game, s.units[s.top + s.sel]);
            if s.top > 0 {
                if let Some(a) = &arrows.0 {
                    gfx::blit(game.scr.page(), a, -5, 0xb5, false);
                }
            }
            if s.top + 5 < n {
                if let Some(a) = &arrows.1 {
                    gfx::blit(game.scr.page(), a, 0x26e, 0xb5, false);
                }
            }
            let c = s.sel as i32 * 0x7a;
            gfx::rect(game.scr.page(), c + 0x11, 5, c + 0x84, 0xc5, 0x3b, false);
            gfx::rect(game.scr.page(), c + 0x10, 4, c + 0x85, 0xc6, 0x3b, false);
            let a = format!("{}/{}", s.top + s.sel + 1, n);
            text::large(game, 0x165, 0x109, a.as_bytes(), 0x39, 0, 1, 2);
            let b = format!("{}/{}", s.deployed, s.max);
            text::large(game, 0x227, 0x109, b.as_bytes(), 0x39, 0, 1, 2);
            if s.mode == 2 {
                let b = s.button as i32 * 0x70;
                gfx::rect(game.scr.page(), b + 0xc, 0xd5, b + 0x77, 0xf3, 0x3b, false);
            }
            cursor::draw_present(game, None, false);
            if fade_pending {
                let m = game.res().master.clone();
                gfx::fade(game, &m, 0, 0);
                fade_pending = false;
            }
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if delay {
            game.wait_until(t_delay, 10);
            delay = false;
        }
        let mut action = 0;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::LEFT => action = 0x4b,
                    scan::RIGHT => action = 0x4d,
                    scan::TAB | scan::UP | scan::DOWN => {
                        s.mode = if s.mode == 1 { 2 } else { 1 };
                        redraw = true;
                    }
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 0x39;
                    }
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (x, y) = (e.x, e.y);
                    let left = e.mods & ev::M_LBUTTON != 0;
                    if (0x12..=0x26b).contains(&x) && (5..=0xc4).contains(&y) {
                        let col = ((x - 0x12) / 0x7a) as usize;
                        s.mode = 1;
                        if col != s.sel && col + s.top < n {
                            redraw = true;
                            s.sel = col;
                        }
                        if left {
                            game.wait_event(ev::MOUSE_UP);
                            action = 0x39;
                        }
                    } else if ((0x23c..=0x255).contains(&x) && (0xdb..=0xee).contains(&y)) || (x <= 0x11 && (5..=0xc4).contains(&y)) {
                        if left {
                            s.mode = 1;
                            action = 0x4b;
                            delay = true;
                            t_delay = game.ticks();
                        }
                    } else if ((0x259..=0x272).contains(&x) && (0xdb..=0xee).contains(&y)) || (0x26f <= x && (5..=0xc4).contains(&y)) {
                        if left {
                            s.mode = 1;
                            action = 0x4d;
                            delay = true;
                            t_delay = game.ticks();
                        }
                    } else if 0xc < x && 0xd4 < y && x < 0x238 && y < 0xf4 {
                        let b = match x {
                            _ if x < 0x78 => Some(0),
                            0x7c..=0xe7 => Some(1),
                            0xec..=0x157 => Some(2),
                            0x15d..=0x1c7 => Some(3),
                            0x1cc..=0x237 => Some(4),
                            _ => None,
                        };
                        if let Some(b) = b {
                            if s.button != b || s.mode != 2 {
                                s.button = b;
                                redraw = true;
                            }
                            s.mode = 2;
                            if left {
                                game.wait_event(ev::MOUSE_UP);
                                action = 0x39;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        match action {
            0x4b => {
                if s.mode == 1 {
                    if s.sel < 1 {
                        if s.top > 0 {
                            s.top -= 1;
                            cards = true;
                        }
                    } else {
                        s.sel -= 1;
                        redraw = true;
                    }
                } else if s.mode == 2 {
                    s.button = if s.button == 0 { 4 } else { s.button - 1 };
                    redraw = true;
                }
            }
            0x4d => {
                if s.mode == 1 {
                    if s.sel < 4 && s.sel + s.top < n - 1 {
                        s.sel += 1;
                        redraw = true;
                    } else if s.sel == 4 && s.top + 5 < n {
                        s.top += 1;
                        cards = true;
                    }
                } else if s.mode == 2 {
                    s.button = if s.button == 4 { 0 } else { s.button + 1 };
                    redraw = true;
                }
            }
            0x39 => {
                cursor::erase(game);
                if s.mode == 1 {
                    let i = s.top + s.sel;
                    toggle(game, &mut s, slots, i)?;
                    full = true;
                    if n == s.deployed || s.max == s.deployed {
                        s.mode = 2;
                        s.button = 2;
                        cursor::warp(game, 0x122, 0xe1);
                    }
                } else if s.mode == 2 {
                    let b = s.button;
                    let m = game.msg(0xbb + b);
                    text::large(game, b as i32 * 0x70 + 0x41 - m.len() as i32 * 6, 0xd9, &m, 0x3a, 0, 1, 2);
                    cursor::draw_present(game, None, false);
                    match b {
                        0 => {
                            if n != s.deployed {
                                auto_fill(game, slots, s.max, &s.units);
                            }
                            break;
                        }
                        1 => {
                            place_on_map(game, slots, None, false);
                        }
                        2 => {
                            let q = game.msg(6);
                            if s.deployed > 1 || ui::yes_no(game, 0x118, 0x104, true, &q, true, false) {
                                break;
                            }
                        }
                        3 => {
                            let q = game.msg(6);
                            if ui::yes_no(game, 0x186, 0x104, true, &q, false, false) {
                                super::party::auto_equip_all(game);
                            }
                        }
                        _ => {
                            super::party::equipment(game, 0, None, false)?;
                        }
                    }
                    full = true;
                }
            }
            _ => {}
        }
    }
    let m = game.res().master.clone();
    gfx::fade(game, &m, 1, 0);
    super::stop_music(game);
    view::reset_surface(game);
    view::bake(game);
    Ok(())
}

/// Port of `FUN_00446293`: take member `i` out of battle, or place them
/// (on the map, `FUN_00416ffb`). A member placed by a forced cell is moved.
fn toggle(game: &mut Game, s: &mut Screen, slots: &mut [Slot], i: usize) -> Result<()> {
    let r = s.units[i];
    if s.deployed == s.max && s.slot_of[i] == 0 {
        let m = game.msg(0x2b);
        ui::message_box(game, 0xa0, 0x58, &m, Wait::Click(0));
        return Ok(());
    }
    if s.slot_of[i] != 0 && !s.fixed[i] {
        // Withdraw.
        slots[s.slot_of[i] - 1].member = 0;
        s.slot_of[i] = 0;
        game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
        s.deployed -= 1;
        view::reset_surface(game);
        view::bake(game);
        return Ok(());
    }
    let moving = s.fixed[i];
    let old = (game.battle.unit(r).x, game.battle.unit(r).y);
    if moving {
        game.battle.cursor = old;
        slots[s.slot_of[i] - 1].member = 0;
        game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
        view::cursor_set(game, old.0, old.1);
        if let Some(m) = game.battle.map.as_mut() {
            m.center_on(old.0, old.1, true);
        }
    }
    view::set_anim(game, r, 1);
    let act = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = 1;
    let got = place_on_map(game, slots, Some(r), moving);
    game.battle.unit_mut(r).act = act;
    if got == 0 {
        if moving {
            let u = game.battle.unit_mut(r);
            u.x = old.0;
            u.y = old.1;
            u.flags |= flag::DEPLOYED;
            slots[s.slot_of[i] - 1].member = i + 1;
        } else {
            game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
        }
    } else {
        s.slot_of[i] = got;
        slots[got - 1].member = i + 1;
        if !moving {
            s.deployed += 1;
        }
    }
    view::set_anim(game, r, 0);
    game.battle.unit_mut(r).turn_flags = 0;
    view::reset_surface(game);
    view::bake(game);
    Ok(())
}

/// Port of `FUN_00446095`: fill the free cells in order, each with its own
/// character if available, else the first member off the map. (The
/// original's inner test looks at the current cell only — `slots[local_8]`
/// inside the `local_14` loop — so any idle member qualifies.) Stops at the
/// first cell nobody can take. Only the first `max` cells (the clamped
/// count the screen works with, `param_2`) are visited.
fn auto_fill(game: &mut Game, slots: &mut [Slot], max: usize, units: &[UnitRef]) {
    for k in 0..max.min(slots.len()) {
        if slots[k].member >= 1 {
            continue;
        }
        let idle = |game: &Game, r: UnitRef| game.battle.unit(r).flags & flag::DEPLOYED == 0;
        let ch = slots[k].character;
        let mut pick = units.iter().copied().find(|&r| idle(game, r) && game.battle.unit(r).char_id == ch);
        if pick.is_none() {
            pick = units.iter().copied().find(|&r| idle(game, r) && game.battle.unit(r).char_id != ch);
        }
        let Some(r) = pick else { return };
        slots[k].member = 1;
        let sl = slots[k];
        place(game, r, &sl);
    }
    view::reset_surface(game);
    view::bake(game);
}

/// `FUN_00416a09(x, y)`: the 1-based free cell at (x, y), or 0.
fn free_slot_at(slots: &[Slot], x: i32, y: i32) -> usize {
    slots.iter().position(|s| s.member == 0 && s.x == x && s.y == y).map_or(0, |i| i + 1)
}

/// Port of `FUN_00416a75`: every deployment cell, taken or not, marked on
/// the map. The units (without the one being placed) are baked first; each
/// marker (sysimg 33) is then cut by the scenery in front of its cell
/// (`FUN_00426417(map, x, y, buf, 0, 1, 0, 0)`: solid cuts, no own overlay)
/// and by the units of both armies on its row (`FUN_00440591(army, x, y,
/// buf, 3, 1, 0)`) and drawn into the map surface (`FUN_0042652e`), so it
/// covers ground shadows and later redraws take it as background. Page 2 gets
/// the marked window, as the facing box (FUN_0044d7c1 mode 1) expects.
fn mark_slots(game: &mut Game, slots: &[Slot], plain: &Image) {
    use crate::battle::view::unit::{cut_by_units, DrawGlobals, Placed};
    use s2data::gfx_map::{CutMode, Occlusion};
    let Some(marker) = game.res().ui_image(33).ok() else { return };
    if let Some(mv) = game.battle.view.map.as_mut() {
        mv.bg = plain.clone();
    }
    view::reset_surface(game);
    view::bake(game);
    let heights: Vec<i32> = slots.iter().map(|s| game.battle.map.as_ref().map_or(1, |m| m.height(s.x, s.y))).collect();
    let looks: Vec<Vec<_>> = [Side::Player, Side::Enemy]
        .iter()
        .map(|&side| (0..game.battle.army(side).units.len()).map(|i| view::look_of(&game.battle, UnitRef::new(side, i))).collect())
        .collect();
    let g = DrawGlobals { vs_mode: game.battle.vs, acting_side: game.battle.vs_side, mesh: game.cfg.mesh };
    let v = &mut game.battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_mut()) else { return };
    let placed: Vec<Vec<Placed>> = (0..2)
        .map(|s| looks[s].iter().enumerate().map(|(i, l)| Placed { look: l, sprite: v.units[s].get(i).and_then(|u| u.sprite.as_ref()), under: None }).collect())
        .collect();
    let mut surface = mv.surface.clone();
    let (sx, sy) = (mv.scroll_x, mv.scroll_y);
    mv.scroll_x = 0;
    mv.scroll_y = 0;
    {
        let r = mv.renderer(&assets.tiles, &assets.shading);
        for (s, &h) in slots.iter().zip(&heights) {
            let mut buf = marker.clone();
            for side in 0..2 {
                cut_by_units(mv, &placed[side], side as i32, &g, s.x, s.y, &mut buf, 3, 1, 0, None);
            }
            r.occlude(&mut buf, s.x, s.y, Occlusion { dx: 0, mode: CutMode::Solid, own_overlay: false, skip_overlay: None });
            mv.blit_at_cell(&mut surface, s.x, s.y, h, &buf);
        }
    }
    mv.scroll_x = sx;
    mv.scroll_y = sy;
    mv.surface = surface;
    game.scr.set_page(2);
    super::stage::draw_map(game);
    game.scr.set_page(0);
}

/// Port of `FUN_00416ffb(unit, moving)` (`FUN_0044627c` is the no-unit call): look at the battlefield (no unit),
/// or put `unit` on a free cell and choose its facing. Returns the 1-based
/// cell, or 0 when cancelled.
fn place_on_map(game: &mut Game, slots: &[Slot], unit: Option<UnitRef>, moving: bool) -> usize {
    let Some(plain) = game.battle.view.map.as_ref().map(|m| m.bg.clone()) else { return 0 };
    if !moving {
        // FUN_00416967: the cursor starts on the first free cell.
        let first = slots.iter().find(|s| s.member == 0).or(slots.first()).map(|s| (s.x, s.y)).unwrap_or((0, 0));
        game.battle.cursor = first;
        view::cursor_set(game, first.0, first.1);
        if let Some(m) = game.battle.map.as_mut() {
            m.center_on(first.0, first.1, true);
        }
    }
    let mut valid = unit.is_some();
    if let Some(r) = unit {
        let (cx, cy) = game.battle.cursor;
        let u = game.battle.unit_mut(r);
        u.x = cx;
        u.y = cy;
        u.flags |= flag::DEPLOYED;
    }
    let mut last = game.battle.cursor;
    let mut marks = true;
    let (mut x, mut y) = (0, 0);
    let result = loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        let code = menu::input(game, &mut x, &mut y, 1);
        match code {
            0 => {
                if marks {
                    if let Some(r) = unit {
                        game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
                    }
                    mark_slots(game, slots, &plain);
                    if let Some(r) = unit {
                        game.battle.unit_mut(r).flags |= flag::DEPLOYED;
                    }
                    marks = false;
                }
                let c = game.battle.cursor;
                if c != last {
                    if let Some(r) = unit {
                        if free_slot_at(slots, c.0, c.1) == 0 {
                            cursor::set_shape(game, 0xf);
                            valid = false;
                        } else {
                            cursor::set_shape(game, 0xb);
                            valid = true;
                            let u = game.battle.unit_mut(r);
                            u.x = c.0;
                            u.y = c.1;
                        }
                    }
                    last = c;
                }
                frame(game, unit, valid, 0);
            }
            -8 | -3 | 0x1c => {
                let (cx, cy) = game.battle.cursor;
                if let Some(r) = unit {
                    let k = free_slot_at(slots, cx, cy);
                    if k != 0 {
                        cursor::set_shape(game, 0xb);
                        if menu::rest_command(game, r, 1) {
                            break k;
                        }
                    }
                }
            }
            -6 => frame(game, unit, valid, 2),
            -5 => {
                let (cx, cy) = game.battle.cursor;
                camera::ensure_visible(game, cx, cy, None);
            }
            -4 | -1 => break 0,
            _ => {
                let moved = game.battle.map.as_mut().is_some_and(|m| m.scroll_key(code));
                if moved {
                    marks = true;
                    let t = game.ticks();
                    frame(game, unit, valid, 1);
                    let speed = game.cfg.screen_speed;
                    game.wait_until(t, speed);
                }
            }
        }
    };
    game.flush_events(0x1f);
    cursor::set_shape(game, 0xb);
    if let Some(mv) = game.battle.view.map.as_mut() {
        mv.bg = plain;
    }
    view::reset_surface(game);
    view::bake(game);
    result
}

/// Port of `FUN_00416c1f(mode)`: the map with the marked cells, the unit
/// being placed — only while the cursor is on a free cell (`shown`,
/// `DAT_00492b54`), its animation stepping every 0x16 cs (`FUN_00441a24`) —
/// the map cursor, which first follows the pointer if it moved since the last
/// look (`FUN_0042c121`), and the status bar. Mode 1 (after a scroll) draws
/// the map and the unit only; mode 2 first restores under the pointer.
fn frame(game: &mut Game, unit: Option<UnitRef>, shown: bool, mode: i32) {
    if mode == 2 {
        cursor::draw_present(game, None, true);
    }
    game.scr.set_page(0);
    if let Some(r) = unit.filter(|_| shown && mode != 1) {
        let u = game.battle.unit(r);
        if u.status & 1 == 0 && !game.still_waiting(u.anim_timer, 0x16) {
            let facing = u.facing;
            view::step_unit(game, r, facing);
            let t = game.ticks();
            game.battle.unit_mut(r).anim_timer = t;
        }
    }
    let hidden = unit.filter(|_| !shown);
    if let Some(r) = hidden {
        game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
    }
    if mode == 1 {
        view::draw_map(game);
    } else {
        view::cursor_track(game);
        view::draw_battle(game);
    }
    if let Some(r) = hidden {
        game.battle.unit_mut(r).flags |= flag::DEPLOYED;
    }
    if mode != 1 {
        view::panel::status_bar(game, None);
    }
    cursor::draw_present(game, None, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_fill_gives_a_missing_characters_cell_to_the_next_idle_member() {
        let dir = std::env::var_os("S2_GAME_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("games/super2sp/extracted/app")));
        let Some(data) = dir.and_then(|d| crate::Data::open(&d).ok()) else { return eprintln!("deploy test: no game data, skipped") };
        let mut sys = crate::sys::Sys::new_headless(crate::sys::Shared::new(), Box::new(|_, _| {}));
        let mut game = Game::new(&mut sys, &data);
        game.res = Some(crate::res::Res::load(&data, &game.cfg).unwrap());
        game.battle.load_tables(&data).unwrap();
        crate::flow::stage::load_stage(&mut game, 1, 10).unwrap();
        game.battle.deploy_turn(1);
        let units: Vec<UnitRef> = crate::flow::stage::units_of(&game, Side::Player).into_iter().take(4).collect();
        assert_eq!(units.len(), 4);
        let ids: Vec<i16> = units.iter().map(|&r| game.battle.unit(r).char_id).collect();
        for &r in &units {
            game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
        }
        // Stage-4 shape: cell 1 is meant for a character who is not here.
        let cell = |k: i32, ch: i16| Slot { x: k, y: 1, facing: 0, character: ch, member: 0, forced: false };
        let mut slots = vec![cell(0, ids[0]), cell(1, 999), cell(2, ids[2]), cell(3, ids[3]), cell(4, ids[1])];
        slots[0].member = 1;
        game.battle.unit_mut(units[0]).flags |= flag::DEPLOYED;
        auto_fill(&mut game, &mut slots, 4, &units);
        let at = |game: &Game, r: UnitRef| (game.battle.unit(r).flags & flag::DEPLOYED != 0, game.battle.unit(r).x);
        // FUN_00446095: cell 1 takes the first idle member (units[1]); cells 2, 3 their own;
        // cell 4 is past `max` and stays free.
        assert_eq!(at(&game, units[1]), (true, 1));
        assert_eq!(at(&game, units[2]), (true, 2));
        assert_eq!(at(&game, units[3]), (true, 3));
        assert_eq!(slots[4].member, 0);
    }
}
