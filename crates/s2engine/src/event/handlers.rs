//! Event condition / action handlers for types 4..=16 (`FUN_00417587` and
//! the `FUN_0041c267`..`FUN_0041d9ec` family) and the story-flag side
//! effects (`FUN_0041be7a`).
//!
//! A handler returns true when the event "fires"; the caller then shows its
//! lines, zeroes its type and sets its flag. Some handlers also zero the
//! type themselves first; that matters when they then return false (the
//! event is lost), which is kept.

use crate::game::Game;

use super::battle_hooks::{self as bh, EvArgs, MoveMember, Side, UnitRef, WalkMember};
use super::{set_flag, touched_contains, ui, Log, FLAG_CHOICE, MAX_FLAG};

/// Sound played when units appear (`FUN_00419b1d(0x68, 0)`).
const SOUND_APPEAR: u16 = 0x68;
/// Effect animation on appearing units.
const ANIM_APPEAR: i32 = 0xf9;

fn args(game: &Game, i: usize) -> EvArgs {
    let e = &game.events.events[i];
    EvArgs { sub: e.sub, a: e.a, b: e.b, c: e.c, d: e.d }
}

/// The usual precondition: story flag `a` (1..=80) is set.
fn flag_ok(game: &Game, a: i16) -> bool {
    (1..=MAX_FLAG).contains(&(a as i32)) && game.campaign.flags[a as usize] != 0
}

fn consume(game: &mut Game, i: usize) {
    game.events.events[i].kind = 0;
}

/// `FUN_00457578(&player, id, 0)` then the same on the enemy army.
fn find_either(game: &mut Game, id: i16) -> Option<UnitRef> {
    bh::find_unit(game, Side::Player, id).or_else(|| bh::find_unit(game, Side::Enemy, id))
}

/// `FUN_004576a9(&player, id)` then the same on the enemy army.
fn find_any_either(game: &mut Game, id: i16) -> Option<UnitRef> {
    bh::find_unit_any(game, Side::Player, id).or_else(|| bh::find_unit_any(game, Side::Enemy, id))
}

/// Port of `FUN_00417587`: does event `i` fire now (types 4..=16)?
pub(super) fn event_check(game: &mut Game, i: usize) -> bool {
    if game.campaign.vs_mode {
        return false;
    }
    match game.events.events[i].kind {
        4 => adjacent(game, args(game, i)),
        5 => choice(game, i),
        6 => deploy(game, i),
        7 => move_unit(game, i),
        8 => camera(game, i),
        9 => flags(game, i),
        10 => items(game, i),
        11 => unit_state(game, i),
        12 => unit_action(game, i),
        13 => system(game, i),
        14 => stats(game, i),
        15 => group_walk(game, i),
        16 => group_move(game, i),
        _ => false,
    }
}

/// Port of `FUN_0041c267` (type 4; the record is passed by value): sub 0,
/// chars `a` and `b` both found, and adjacent. No flag precondition.
fn adjacent(game: &mut Game, e: EvArgs) -> bool {
    if e.sub != 0 {
        return false;
    }
    let Some(u1) = find_either(game, e.a) else { return false };
    let Some(u2) = find_either(game, e.b) else { return false };
    bh::units_adjacent(game, u1, u2)
}

/// Type 5 in `FUN_00417587`: sub 0 opens the choice menu (`FUN_0041929b`,
/// result into the flag-12 byte) and consumes the event without firing;
/// subs 1..=4 fire when the last result equals sub, clearing flag `a` and
/// the result.
fn choice(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    match e.sub {
        0 => {
            game.events.current = i;
            let r = ui::choice(game, i);
            game.campaign.flags[FLAG_CHOICE] = r;
            consume(game, i);
            false
        }
        1..=4
            if game.campaign.flags[FLAG_CHOICE] as i16 == e.sub => {
                game.campaign.flags[e.a as usize] = 0;
                game.campaign.flags[FLAG_CHOICE] = 0;
                true
            }
        _ => false,
    }
}

/// Port of `FUN_0041c302` (type 6): consume, then deploy / withdraw chars
/// b, c, d (0, 1 animated; 2, 3 silent). Always fires.
fn deploy(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    let ids = [e.b, e.c, e.d];
    consume(game, i);
    match e.sub {
        0 => deploy_units(game, ids, false),
        1 => withdraw_units(game, ids, false),
        2 => deploy_units(game, ids, true),
        3 => withdraw_units(game, ids, true),
        _ => {}
    }
    true
}

/// Port of `FUN_0041c025`: create every stage deployment record whose char
/// id is one of `ids`. Nothing happens unless the first id is nonzero.
fn deploy_units(game: &mut Game, ids: [i16; 3], silent: bool) {
    if ids[0] == 0 {
        return;
    }
    let records: Vec<(i16, i16, i16)> = match &game.events.stage {
        Some(st) => st.units.iter().map(|u| (u.char_id, u.x, u.y)).collect(),
        None => return,
    };
    for (k, &(cid, x, y)) in records.iter().enumerate() {
        let hit = (ids[0] != 0 && cid == ids[0]) || (ids[1] != 0 && cid == ids[1]) || (ids[2] != 0 && cid == ids[2]);
        if !hit {
            continue;
        }
        if !silent {
            bh::camera_scroll(game, x as i32, y as i32, false);
        }
        let side = if k < game.events.player_records { Side::Player } else { Side::Enemy };
        if let Some(u) = bh::create_unit(game, side, k) {
            if !silent {
                game.events.log.push(Log::Sound(SOUND_APPEAR as i16));
                game.sys.audio.play_sound(&game.data.arcs, SOUND_APPEAR, 0);
                bh::unit_effect(game, u, ANIM_APPEAR, 1);
            }
            bh::place_unit(game, u);
            bh::redraw_map(game);
            ui::present(game);
        }
    }
}

/// Port of `FUN_0041c18c`: withdraw chars `ids` (animated, or erase and
/// clear the on-map bit when silent).
fn withdraw_units(game: &mut Game, ids: [i16; 3], silent: bool) {
    for id in ids {
        if id == 0 {
            continue;
        }
        let Some(u) = find_either(game, id) else { continue };
        if !silent {
            bh::withdraw_animated(game, u);
        } else {
            bh::remove_from_map(game, u);
            bh::redraw_map(game);
            ui::present(game);
            let f = bh::unit_flags(game, u);
            bh::set_unit_flags(game, u, f & 0x7fff_ffff);
        }
    }
}

/// Port of `FUN_0041c3ce` (type 7): try the player army, then the enemy army.
fn move_unit(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    army_move_unit(game, Side::Player, i, e) || army_move_unit(game, Side::Enemy, i, e)
}

/// Port of `FUN_00458703(army, event)`: move char `b` of this army; the
/// target of subs 2, 3, 5 is char `c` in this army or its opponent.
fn army_move_unit(game: &mut Game, side: Side, i: usize, e: EvArgs) -> bool {
    let Some(u) = bh::find_unit(game, side, e.b) else { return false };
    consume(game, i);
    let target = |game: &mut Game| bh::find_unit(game, side, e.c).or_else(|| bh::find_unit(game, side.opponent(), e.c));
    match e.sub {
        0 | 1 => bh::walk_unit(game, u, e.c as i32, e.d as i32, e.sub as i32),
        2 | 3 => {
            if let Some(t) = target(game) {
                let (x, y) = bh::unit_pos(game, t);
                bh::walk_unit(game, u, x, y, (e.sub - 2) as i32);
            }
        }
        4 => bh::warp_unit(game, u, e.c as i32, e.d as i32),
        5 => {
            if let Some(t) = target(game) {
                let (x, y) = bh::unit_pos(game, t);
                bh::warp_unit(game, u, x, y);
            }
        }
        _ => {}
    }
    true
}

/// Port of `FUN_0041c437` (type 8): camera and effects. Consumed before the
/// work, so a missing unit loses the event.
fn camera(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    consume(game, i);
    let (b, c, d) = (e.b as i32, e.c as i32, e.d as i32);
    match e.sub {
        0 => {
            bh::camera_center(game, b, c);
            bh::redraw_map(game);
            ui::present(game);
        }
        1 => bh::camera_scroll(game, b, c, true),
        2 | 3 => {
            let Some(u) = find_either(game, e.b) else { return false };
            let (x, y) = bh::unit_pos(game, u);
            if e.sub == 2 {
                bh::camera_center(game, x, y);
                bh::redraw_map(game);
                ui::present(game);
            } else {
                bh::camera_scroll(game, x, y, true);
            }
        }
        4 => {
            // With b == 0 the original uses uninitialised locals; (0, 0) here.
            let (mut x, mut y) = (0, 0);
            if b != 0 && c != 0 {
                (x, y) = (b, c);
            } else if b != 0 && c == 0 {
                let Some(u) = find_either(game, e.b) else { return false };
                (x, y) = bh::unit_pos(game, u);
            }
            bh::camera_center(game, x, y);
            bh::redraw_map(game);
            ui::present(game);
            ui::page_copy(game, 0, 1);
            bh::screen_effect(game, 100);
        }
        5 => {
            let Some(u) = find_either(game, e.c) else { return false };
            let (x, y) = bh::unit_pos(game, u);
            bh::camera_scroll(game, x, y, true);
            bh::redraw_map(game);
            bh::effect_on_unit(game, b, u);
        }
        6 => {
            bh::camera_scroll(game, c, d, true);
            bh::redraw_map(game);
            bh::effect_at(game, b, c, d);
        }
        7 => ui::wait_cs(game, e.b as u16 as i32),
        8 => {
            bh::redraw_map(game);
            ui::present(game);
        }
        _ => {}
    }
    true
}

/// Port of `FUN_0041c81c` (type 9): flag logic.
fn flags(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if e.sub == 7 {
        // No flag precondition: is map event tile `a` in the touched list?
        return !game.events.touched.is_empty() && touched_contains(game, e.a);
    }
    if !flag_ok(game, e.a) {
        return false;
    }
    let mut list = Vec::with_capacity(3);
    if (1..5).contains(&e.sub) {
        for v in [e.b, e.c, e.d] {
            if (1..=MAX_FLAG).contains(&(v as i32)) {
                list.push(v as usize);
            }
        }
        if list.is_empty() {
            return false;
        }
    }
    let f = &mut game.campaign.flags;
    match e.sub {
        0 => true,
        1 => list.iter().all(|&n| f[n] != 0),
        2 => {
            if !list.iter().all(|&n| f[n] != 0) {
                return false;
            }
            list.iter().for_each(|&n| f[n] = 0);
            true
        }
        3 => list.iter().any(|&n| f[n] != 0),
        4 => {
            if !list.iter().any(|&n| f[n] != 0) {
                return false;
            }
            list.iter().for_each(|&n| f[n] = 0);
            true
        }
        5 => {
            for v in [e.b, e.c, e.d] {
                set_flag(game, v as i32, 0);
            }
            true
        }
        6 => {
            game.campaign.next_stage = e.b as u16;
            true
        }
        _ => false,
    }
}

/// Port of `FUN_0041cb9a` (type 10): items.
fn items(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    let (b, c) = (e.b as i32, e.c as i32);
    match e.sub {
        0 => {
            bh::give_item(game, Side::Player, b, c);
            true
        }
        1 => bh::army_has_item(game, Side::Player, b),
        2 => !bh::army_has_item(game, Side::Player, b),
        3 => bh::army_has_item(game, Side::Enemy, b),
        4 | 5 => {
            let held = if bh::in_roster(game, Side::Player, e.c) {
                bh::unit_holds_item(game, Side::Player, c, b)
            } else if bh::in_roster(game, Side::Enemy, e.c) {
                bh::unit_holds_item(game, Side::Enemy, c, b)
            } else {
                return false;
            };
            held == (e.sub == 4)
        }
        6 => {
            let Some(u) = find_either(game, e.c) else { return false };
            if bh::find_unit(game, Side::Player, e.d).is_none() {
                return false;
            }
            let Some(k) = (0..4).find(|&k| bh::unit_item_slot(game, u, k) == b) else { return false };
            bh::set_unit_item_slot(game, u, k, 0);
            bh::inventory_add(game, Side::Player, b);
            true
        }
        7 => bh::inventory_check(game, Side::Player, b),
        _ => true,
    }
}

const DEAD: u32 = 0x2000_0000;
const RETREATED: u32 = 0x4000_0000;
const ON_MAP: u32 = 0x8000_0000;

/// Port of `FUN_0041ce89` (type 11): unit / party state.
fn unit_state(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    match e.sub {
        0 | 1 => {
            let Some(u) = find_any_either(game, e.b) else { return false };
            let dead = bh::unit_flags(game, u) & DEAD != 0;
            dead == (e.sub == 0)
        }
        2 => bh::in_roster(game, Side::Player, e.b) || bh::in_roster(game, Side::Enemy, e.b),
        3 => bh::find_unit_any(game, Side::Player, e.b).is_none(),
        4..=6 => {
            let Some(u) = bh::find_unit_any(game, Side::Player, e.b) else { return false };
            let f = bh::unit_flags(game, u);
            match e.sub {
                4 => f & RETREATED != 0 && f & ON_MAP == 0,
                5 => f & ON_MAP != 0,
                _ => f & RETREATED == 0 && f & ON_MAP == 0,
            }
        }
        7 => match bh::find_unit_any(game, Side::Player, e.b) {
            Some(u) => {
                bh::set_unit_mode(game, u, e.c as i32);
                true
            }
            None => false,
        },
        8 => {
            bh::party_char_op(game, e.b as i32, 0);
            true
        }
        9 => {
            bh::party_char_op(game, e.b as i32, 1);
            true
        }
        10 => {
            bh::party_op_10(game);
            true
        }
        11 => {
            game.campaign.fame += e.b as i32;
            true
        }
        12 => {
            game.campaign.rating += e.b as i32;
            true
        }
        13 => {
            consume(game, i);
            bh::party_op_13(game);
            true
        }
        14 => {
            consume(game, i);
            bh::party_setup_screen(game);
            true
        }
        _ => true,
    }
}

/// Port of `FUN_0041d217` (type 12): `FUN_0045bc6a` on the player army,
/// then on the enemy army.
fn unit_action(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    bh::army_unit_action(game, Side::Player, e) || bh::army_unit_action(game, Side::Enemy, e)
}

/// Port of `FUN_0041d280` (type 13): difficulty test, sound, music,
/// pictures, movies.
fn system(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    // `uVar2 = _DAT_00494653`: the music cue when the handler started.
    let music_before = game.campaign.music;
    if !flag_ok(game, e.a) {
        return false;
    }
    match e.sub {
        0 => {
            let diff = game.campaign.difficulty as i8 as i32;
            let c = e.c as i32;
            return match e.b {
                1 => diff < c,
                2 => c == diff,
                3 => c < diff,
                _ => false,
            };
        }
        1 => {
            game.events.log.push(Log::Sound(e.b));
            if e.b == 0 {
                game.sys.audio.stop_sounds();
            } else {
                game.sys.audio.play_sound(&game.data.arcs, e.b as u16, 0);
            }
        }
        2 => {
            game.events.log.push(Log::Music(e.b));
            if e.b == 0 {
                crate::flow::stop_music(game);
            } else if e.b > 0 {
                crate::flow::music(game, e.b as u16);
            }
        }
        3 => {
            ui::fade(game, true);
            ui::show_picture(game, e.b, e.c, e.d);
            bh::redraw_map_full(game);
            ui::fade(game, false);
            // `_DAT_00494653 = 0`, so the cue from before restarts.
            game.campaign.music = 0;
            game.sys.audio.current_music = 0;
            game.events.log.push(Log::Music(music_before as i16));
            crate::flow::music(game, music_before);
        }
        4 => {
            for (k, v) in [e.b, e.c, e.d].into_iter().enumerate() {
                game.campaign.flags[super::FLAG_PICTURE + k] = v as u8;
            }
        }
        5 => ui::play_movie(game, e.b, 0),
        6 => game.campaign.flags[super::FLAG_MOVIE] = e.b as u8,
        _ => {}
    }
    true
}

/// Port of `FUN_0041d484` (type 14): relation values `Unit+0x141 + c`.
fn stats(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    let Some(u) = find_any_either(game, e.b) else { return false };
    let (c, d) = (e.c as i32, e.d as i32);
    let in_range = |n: i32| (1..=0x28).contains(&n);
    match e.sub {
        0 => {
            if !in_range(c) {
                return false;
            }
            let v = (bh::unit_relation(game, u, c) as i32 + d).clamp(-0x80, 0x7f);
            bh::set_unit_relation(game, u, c, v as i8);
            true
        }
        1 => {
            if !in_range(e.b as i32) {
                return false;
            }
            bh::party_relation_op(game, e.b as i32, c);
            true
        }
        2 => in_range(c) && d < bh::unit_relation(game, u, c) as i32,
        3 => in_range(c) && bh::unit_relation(game, u, c) as i32 == d,
        4 => in_range(c) && (bh::unit_relation(game, u, c) as i32) < d,
        _ => true,
    }
}

/// Port of `FUN_0041d6b9` (type 15): a header (sub 0 player / 3 enemy,
/// `b` = member count) collects the following type-15 records with the
/// same `a`: sub 1/4 members, sub 2/5 ends the group. Header and members
/// are consumed.
fn group_walk(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    let (side, member, end) = match e.sub {
        0 => (Side::Player, 1, 2),
        3 => (Side::Enemy, 4, 5),
        _ => return false,
    };
    consume(game, i);
    let n = e.b as i32;
    if n < 1 {
        return false;
    }
    let mut members = Vec::new();
    for j in i + 1..game.events.events.len() {
        let r = &game.events.events[j];
        if r.kind != 15 || r.a != e.a {
            continue;
        }
        if r.sub == end {
            consume(game, j);
            break;
        }
        if r.sub == member {
            let m = args(game, j);
            consume(game, j);
            if let Some(u) = bh::find_unit(game, side, m.b) {
                if (members.len() as i32) < n {
                    members.push(WalkMember { unit: u, c: m.c as i32, d4: (m.d as i32) << 2 });
                }
            }
        }
    }
    if !members.is_empty() {
        bh::group_walk(game, side, &members);
    }
    true
}

/// Port of `FUN_0041d9ec` (type 16): like type 15 with header sub 0
/// player / 4 enemy (`b` = count, `c` = mode), members sub 1/5 (`c`, `d`)
/// and 2/6 (`c`), end 3/7.
fn group_move(game: &mut Game, i: usize) -> bool {
    let e = args(game, i);
    if !flag_ok(game, e.a) {
        return false;
    }
    let (side, m_pos, m_par, end) = match e.sub {
        0 => (Side::Player, 1, 2, 3),
        4 => (Side::Enemy, 5, 6, 7),
        _ => return false,
    };
    consume(game, i);
    let n = e.b as i32;
    let mode = e.c as i32;
    if n < 1 {
        return false;
    }
    let mut members = Vec::new();
    for j in i + 1..game.events.events.len() {
        let r = &game.events.events[j];
        if r.kind != 16 || r.a != e.a {
            continue;
        }
        if r.sub == end {
            consume(game, j);
            break;
        }
        if r.sub == m_pos || r.sub == m_par {
            let m = args(game, j);
            consume(game, j);
            if let Some(u) = bh::find_unit(game, side, m.b) {
                if (members.len() as i32) < n {
                    let mut mm = MoveMember { unit: u, p0: 0, p1: 0, p2: 0 };
                    if m.sub == m_pos {
                        mm.p1 = m.c as i32;
                        mm.p2 = m.d as i32;
                    } else {
                        mm.p0 = m.c as i32;
                    }
                    members.push(mm);
                }
            }
        }
    }
    if !members.is_empty() {
        bh::group_move(game, side, &members, mode);
    }
    true
}

/// Port of `FUN_0041be7a`: what setting story flag `f` does by itself.
pub(super) fn flag_side_effect(game: &mut Game, f: i32) {
    match f {
        1 | 3 => {
            game.events.log.push(Log::FlagEffect(f as u8));
            bh::reveal_special_tile(game, if f == 1 { 6 } else { 0xb });
        }
        7 => {
            game.events.log.push(Log::FlagEffect(7));
            bh::army_flag7(game);
        }
        _ => {}
    }
}
