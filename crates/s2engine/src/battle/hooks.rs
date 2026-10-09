//! The battle side of `event::battle_hooks`: every stub there forwards to
//! the function of the same name here (same signature, event-side
//! `Side`/`UnitRef` with 1-based slots). Each is a port of the original
//! callee named in the stub's documentation; the scripted moves, warps
//! and group moves (`FUN_004588f2`, `FUN_00459131`, `FUN_0045c37d`) and the
//! unit action table (`FUN_0045bc6a`) live here because only the event
//! engine calls them.

use super::actions::{self, presenting};
use super::army::{Side, UnitRef};
use super::camera;
use super::unit::flag;
use super::view;
use crate::event::battle_hooks::{self as eh, EvArgs, MoveMember, TalkSave, WalkMember};
use crate::game::Game;
use crate::{gfx, text, ui};

// ---- conversions --------------------------------------------------------------

pub fn side_of(s: eh::Side) -> Side {
    match s {
        eh::Side::Player => Side::Player,
        eh::Side::Enemy => Side::Enemy,
    }
}

pub fn ev_side(s: Side) -> eh::Side {
    match s {
        Side::Player => eh::Side::Player,
        Side::Enemy => eh::Side::Enemy,
    }
}

/// Battle unit → event unit (1-based slot).
pub fn ev_ref(r: UnitRef) -> eh::UnitRef {
    eh::UnitRef { side: ev_side(r.side), slot: r.idx as u16 + 1 }
}

/// Event unit → battle unit, if the slot exists.
pub fn from_ev(game: &Game, u: eh::UnitRef) -> Option<UnitRef> {
    let side = side_of(u.side);
    let i = (u.slot as usize).checked_sub(1)?;
    (i < game.battle.army(side).units.len()).then(|| UnitRef::new(side, i))
}

/// Battle state exists (a battle map is loaded). Without it the hooks act
/// like the event engine's original stubs ("nothing there"), which its
/// headless tests rely on.
fn active(game: &Game) -> bool {
    game.battle.map.is_some()
}

/// The view needs the unit's sprite (`unit+0xad != 0`); every army member
/// has one here once the view is loaded, so the battle code's tests of that
/// pointer reduce to "the unit exists".
fn alive_on_map(game: &Game, r: UnitRef) -> bool {
    let u = game.battle.unit(r);
    u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0
}

// ---- queries ---------------------------------------------------------------------

pub fn find_unit(game: &mut Game, side: eh::Side, char_id: i16) -> Option<eh::UnitRef> {
    if !active(game) {
        return None;
    }
    let s = side_of(side);
    game.battle.army(s).find_char(char_id, 0).map(|i| ev_ref(UnitRef::new(s, i)))
}

/// Port of FUN_004576a9: first unit with the character id, in any state.
pub fn find_unit_any(game: &mut Game, side: eh::Side, char_id: i16) -> Option<eh::UnitRef> {
    if !active(game) {
        return None;
    }
    let s = side_of(side);
    game.battle.army(s).units.iter().position(|u| u.char_id == char_id).map(|i| ev_ref(UnitRef::new(s, i)))
}

/// Port of FUN_0043a860(army, char_id, 0): a unit with the character id
/// that has not retreated (or is back on the map).
pub fn in_roster(game: &mut Game, side: eh::Side, char_id: i16) -> bool {
    if !active(game) {
        return false;
    }
    in_roster_mode(game, side_of(side), char_id, 0)
}

/// Port of FUN_0043a860; mode 1 also excludes summons.
pub fn in_roster_mode(game: &Game, side: Side, char_id: i16, mode: i32) -> bool {
    game.battle.army(side).units.iter().any(|u| {
        u.char_id == char_id
            && (u.flags & flag::RESERVE == 0 || u.flags & flag::DEPLOYED != 0)
            && (mode != 1 || u.flags & flag::SUMMON == 0)
    })
}

pub fn unit_flags(game: &mut Game, u: eh::UnitRef) -> u32 {
    if !active(game) {
        return 0;
    }
    from_ev(game, u).map_or(0, |r| game.battle.unit(r).flags)
}

pub fn set_unit_flags(game: &mut Game, u: eh::UnitRef, v: u32) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        game.battle.unit_mut(r).flags = v;
    }
}

pub fn unit_pos(game: &mut Game, u: eh::UnitRef) -> (i32, i32) {
    if !active(game) {
        return (0, 0);
    }
    from_ev(game, u).map_or((0, 0), |r| {
        let u = game.battle.unit(r);
        (u.x, u.y)
    })
}

/// Port of FUN_0044e99d.
pub fn army_all_down(game: &mut Game, side: eh::Side, k: i32) -> bool {
    if !active(game) {
        return false;
    }
    let units = &game.battle.army(side_of(side)).units;
    let live = |u: &super::Unit| u.flags & flag::DEPLOYED != 0 && u.flags & flag::DEAD == 0;
    match k {
        -3 => !units.iter().any(|u| u.rank != 1 && live(u)),
        -2 => !units.iter().any(|u| u.rank == 1 && live(u)),
        -1 => !units.iter().any(|u| u.npc == 1 && live(u)),
        0 => !units.iter().any(live),
        _ => units
            .iter()
            .any(|u| u.char_id as i32 == k && u.flags & flag::DEPLOYED != 0 && u.flags & flag::DEAD != 0),
    }
}

/// Port of FUN_0045cbf8 (player army).
pub fn on_exit_tile(game: &mut Game, mode: i32) -> bool {
    if !active(game) {
        return false;
    }
    let b = &game.battle;
    let m = b.map();
    let p = b.army(Side::Player);
    if mode == 0 {
        let ch = if b.route == 0 { 0xb } else { 1 };
        match p.units.iter().find(|u| u.char_id == ch) {
            Some(u) => m.object_class(u.x, u.y) == 6,
            None => false,
        }
    } else {
        p.units.iter().any(|u| {
            u.flags & (flag::DEAD | flag::RESERVE) == 0
                && u.flags & flag::DEPLOYED != 0
                && u.npc != 1
                && u.flags & flag::SUMMON == 0
                && m.object_class(u.x, u.y) == 6
        })
    }
}

/// Port of FUN_00441326: `b` is a diagonal neighbour of `a` with a height
/// difference below 4.
pub fn units_adjacent(game: &mut Game, a: eh::UnitRef, b: eh::UnitRef) -> bool {
    if !active(game) {
        return false;
    }
    let (Some(a), Some(b)) = (from_ev(game, a), from_ev(game, b)) else { return false };
    let (ua, ub) = (game.battle.unit(a), game.battle.unit(b));
    let m = game.battle.map();
    if (m.height(ua.x, ua.y) - m.height(ub.x, ub.y)).abs() >= 4 {
        return false;
    }
    let odd = ua.y & 1 != 0;
    let right = if odd { ua.x + 1 } else { ua.x };
    let left = if odd { ua.x } else { ua.x - 1 };
    (ub.x == right && ub.y == ua.y - 1)
        || (ub.x == left && ub.y == ua.y + 1)
        || (ub.x == left && ub.y == ua.y - 1)
        || (ub.x == right && ub.y == ua.y + 1)
}

/// Port of FUN_00425fbe.
pub fn map_cell_event(game: &mut Game, x: i32, y: i32) -> Option<u16> {
    if !active(game) {
        return None;
    }
    let m = game.battle.map.as_ref()?;
    if x < 0 || y < 0 || x >= m.width() || y >= m.rows() {
        return None;
    }
    Some(m.spot_at(x, y).unwrap_or(0))
}

// ---- deploy / withdraw ------------------------------------------------------------

/// Port of FUN_00426b87 (`fastest`: 畫面速度 forced to 0 meanwhile).
pub fn camera_scroll(game: &mut Game, x: i32, y: i32, fastest: bool) {
    if !active(game) {
        return;
    }
    let saved = game.cfg.screen_speed;
    if fastest {
        game.cfg.screen_speed = 0;
    }
    camera::scroll_exact(game, x, y, None);
    game.cfg.screen_speed = saved;
}

/// Port of FUN_0042681e(map, x, y, 1).
pub fn camera_center(game: &mut Game, x: i32, y: i32) {
    if !active(game) {
        return;
    }
    game.battle.map_mut().center_on(x, y, true);
}

/// Port of FUN_0043d9e9(army, record, 1).
pub fn create_unit(game: &mut Game, side: eh::Side, record: usize) -> Option<eh::UnitRef> {
    if !active(game) {
        return None;
    }
    let r = game.battle.create_from_record(side_of(side), record, 1)?;
    // FUN_0043eb5f -> FUN_0043f9dd: FUN_00442022(army, unit, 0).
    view::capture(game, r);
    Some(ev_ref(r))
}

/// Port of FUN_0044f11c(army, unit, colour, mode).
pub fn unit_effect(game: &mut Game, u: eh::UnitRef, anim: i32, mode: i32) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        if presenting(game) {
            view::flash_unit(game, r, anim as u8, mode != 0);
        }
    }
}

/// Port of FUN_00440264(army, unit, 1): draw the unit into the map surface.
pub fn place_unit(game: &mut Game, u: eh::UnitRef) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        view::put_unit(game, r);
    }
}

/// Port of FUN_00441c0b: erase the unit from the map surface.
pub fn remove_from_map(game: &mut Game, u: eh::UnitRef) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        view::erase_unit(game, r);
    }
}

/// Port of FUN_0044cf97.
pub fn withdraw_animated(game: &mut Game, u: eh::UnitRef) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        withdraw(game, r);
    }
}

/// Port of FUN_0044cf97: scroll to the unit, dissolve it, take it off the map.
pub fn withdraw(game: &mut Game, r: UnitRef) {
    let (x, y) = {
        let u = game.battle.unit(r);
        (u.x, u.y)
    };
    view::erase_unit(game, r);
    camera::ensure_visible(game, x, y, Some(r));
    view::fade_unit(game, r, 0, false);
    game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
    camera::redraw_present(game);
}

/// Port of FUN_00424c5f.
pub fn redraw_map(game: &mut Game) {
    if !active(game) {
        return;
    }
    camera::redraw(game);
}

/// Port of FUN_004252c9.
pub fn redraw_map_full(game: &mut Game) {
    if !active(game) {
        return;
    }
    camera::redraw_full(game);
}

// ---- moves -----------------------------------------------------------------------------

/// Port of FUN_004588f2.
pub fn walk_unit(game: &mut Game, u: eh::UnitRef, x: i32, y: i32, mode: i32) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        script_walk(game, r, x, y, mode);
    }
}

impl super::Battle {
    /// Port of FUN_0045ca38 (and the path loop of FUN_004588f2): the cells
    /// of a scripted walk from `r` toward (tx, ty), ignoring units (range
    /// flag 0x8000). When the target is out of range the walk goes to the
    /// closest reachable cell and continues from there, at most 20 times.
    /// Empty when no step is possible.
    pub fn script_path(&mut self, r: UnitRef, tx: i32, ty: i32) -> Vec<(i32, i32, u8)> {
        let (x0, y0) = (self.unit(r).x, self.unit(r).y);
        let move5 = self.unit(r).flags & flag::MOVE5 != 0;
        let mut out: Vec<(i32, i32, u8)> = Vec::new();
        let mut tries = 0;
        loop {
            tries += 1;
            if tries > 20 {
                out.clear();
                break;
            }
            self.build_range(r, 0x8000);
            let k = self.range.find_stop(tx, ty, move5);
            let mut done = false;
            let ok = if k != 0 {
                self.range.build_path(k, move5);
                done = true;
                true
            } else {
                self.range_closest_to(tx, ty, 0)
            };
            if ok {
                out.extend(self.walk_steps());
            } else {
                out.clear();
            }
            self.range.reset();
            if out.is_empty() {
                done = true;
            } else {
                let &(lx, ly, _) = out.last().unwrap();
                let u = self.unit_mut(r);
                u.x = lx;
                u.y = ly;
            }
            if done {
                break;
            }
        }
        let u = self.unit_mut(r);
        u.x = x0;
        u.y = y0;
        out
    }
}

/// Footstep sound of FUN_0044c331 / FUN_00458c60 for a unit entering (x, y).
fn footstep(game: &mut Game, r: UnitRef, x: i32, y: i32, slot: usize) -> usize {
    let u = game.battle.unit(r).clone();
    let mt = game.battle.move_type(&u);
    let class_move = game.battle.tables.as_ref().and_then(|t| t.kinds.class(u.kind as u16)).map_or(0, |c| c.move_type);
    let id = if mt == 1 {
        0x7a
    } else if game.battle.map().no_stop_tile(x, y) {
        0x83
    } else if class_move == 4 {
        0x7c
    } else {
        0x67
    };
    game.sys.audio.play_sound(&game.data.arcs, id, slot)
}

/// Port of FUN_004588f2(army, unit, x, y, mode): the unit walks to the
/// free cell nearest (x, y); mode 0 keeps the camera on it.
pub fn script_walk(game: &mut Game, r: UnitRef, x: i32, y: i32, mode: i32) {
    let (tx, ty) = game.battle.displace(r.side, x, y);
    let steps = game.battle.script_path(r, tx, ty);
    if steps.is_empty() {
        return;
    }
    let (x0, y0) = (game.battle.unit(r).x, game.battle.unit(r).y);
    camera::scroll_exact(game, x0, y0, Some(r));
    view::erase_unit(game, r);
    actions::set_anim(game, r, 1);
    game.battle.unit_mut(r).act = 3;
    let mt = {
        let u = game.battle.unit(r).clone();
        game.battle.move_type(&u)
    };
    // FUN_00458bd7: FUN_00458e13 (flying, move types 3-5), FUN_00458ff3
    // (teleport, 2) or FUN_00458c60 (ground); mode 1 keeps the camera still.
    if presenting(game) && (3..=5).contains(&mt) {
        script_fly(game, r, &steps, mode);
    } else if presenting(game) && mt == 2 {
        script_teleport(game, r, &steps, mode);
    } else if presenting(game) && mode == 0 {
        view::animate_walk(game, r, &steps);
    } else {
        let mut slot = 0;
        for &(sx, sy, f) in &steps {
            if presenting(game) {
                slot = footstep(game, r, sx, sy, slot);
            }
            let u = game.battle.unit_mut(r);
            u.x = sx;
            u.y = sy;
            actions::face(game, r, f);
            view::capture(game, r);
            if presenting(game) {
                let t = game.battle.unit(r).anim_timer;
                camera::redraw(game);
                game.wait_until(t, 8);
                gfx::present(game);
                game.battle.unit_mut(r).anim_timer = game.ticks();
            }
        }
    }
    let &(lx, ly, _) = steps.last().unwrap();
    {
        let u = game.battle.unit_mut(r);
        u.x = lx;
        u.y = ly;
    }
    // FUN_00442022, act 0, FUN_00441c0b, FUN_00440264.
    view::capture(game, r);
    game.battle.unit_mut(r).act = 0;
    actions::set_anim(game, r, 0);
    view::erase_unit(game, r);
    view::put_unit(game, r);
    camera::redraw_full(game);
}

/// Port of FUN_00459131: vanish (effect 0xc2, sound 0x68) and reappear at
/// the free cell nearest (x, y).
pub fn warp_unit(game: &mut Game, u: eh::UnitRef, x: i32, y: i32) {
    if !active(game) {
        return;
    }
    let Some(r) = from_ev(game, u) else { return };
    let (tx, ty) = game.battle.displace(r.side, x, y);
    let (x0, y0) = (game.battle.unit(r).x, game.battle.unit(r).y);
    view::erase_unit(game, r);
    game.battle.map_mut().center_on(x0, y0, true);
    camera::redraw(game);
    ui::sound(game, 0x68);
    view::fade_unit(game, r, 0xc2, false);
    {
        let u = game.battle.unit_mut(r);
        u.x = tx;
        u.y = ty;
    }
    camera::ensure_visible(game, tx, ty, None);
    camera::redraw(game);
    // FUN_00442022; the original does not draw it back into the surface.
    view::capture(game, r);
    ui::sound(game, 0x68);
    view::fade_unit(game, r, 0xc2, true);
}

// ---- camera / effects ---------------------------------------------------------------

/// FUN_00466e71(&DAT_004a1f58, n): full-screen spell effect.
pub fn screen_effect(game: &mut Game, _n: i32) {
    if !active(game) {
        return;
    }
    if presenting(game) {
        view::motion::shake_screen(game, _n, 0);
    }
}

/// FUN_0046cd1c: effect animation on a unit's cell.
pub fn effect_on_unit(game: &mut Game, anim: i32, u: eh::UnitRef) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
        if presenting(game) {
            let _ = view::play_effect(game, anim as usize, x, y, false);
        }
    }
}

/// FUN_0046cd78: effect animation at a cell.
pub fn effect_at(game: &mut Game, anim: i32, x: i32, y: i32) {
    if !active(game) {
        return;
    }
    if presenting(game) {
        let _ = view::play_effect(game, anim as usize, x, y, false);
    }
}

// ---- items --------------------------------------------------------------------------------

/// Port of FUN_00446b84: add one item (the count operand is the sound
/// played) and announce "<item> <msg>" for 2 s.
pub fn give_item(game: &mut Game, side: eh::Side, item: i32, count: i32) {
    if !active(game) {
        return;
    }
    let s = side_of(side);
    let n = game.battle.army(s).inventory.len() as i32;
    if item < 1 || item > n {
        return;
    }
    inventory_add(game, side, item);
    ui::sound(game, count as u16);
    if presenting(game) {
        let name = game.battle.item_name_raw(item as u16);
        let m = game.msg(0x26);
        let s = actions::sprintf(b"%s %s", &[actions::Arg::S(&m), actions::Arg::S(&name)]);
        ui::message_box(game, 0xb0, 200, &s, ui::Wait::Click(200));
    }
}

/// Port of FUN_0043a96a: the army inventory holds the item.
pub fn army_has_item(game: &mut Game, side: eh::Side, item: i32) -> bool {
    if !active(game) {
        return false;
    }
    game.battle.army(side_of(side)).has_item(item)
}

/// Port of FUN_0043a9af: the deployed character has the item equipped.
pub fn unit_holds_item(game: &mut Game, side: eh::Side, char_id: i32, item: i32) -> bool {
    if !active(game) {
        return false;
    }
    let a = game.battle.army(side_of(side));
    if item < 1 || item as usize > a.inventory.len() {
        return false;
    }
    match a.find_char(char_id as i16, 0) {
        Some(i) => a.units[i].equip.iter().any(|&e| e as i32 == item),
        None => false,
    }
}

pub fn unit_item_slot(game: &mut Game, u: eh::UnitRef, k: usize) -> i32 {
    if !active(game) {
        return 0;
    }
    from_ev(game, u).map_or(0, |r| game.battle.unit(r).drops.get(k).copied().unwrap_or(0) as i32)
}

pub fn set_unit_item_slot(game: &mut Game, u: eh::UnitRef, k: usize, v: i32) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        if let Some(d) = game.battle.unit_mut(r).drops.get_mut(k) {
            *d = v as u16;
        }
    }
}

/// Port of FUN_00446c0c: one more of the item (cap 99).
pub fn inventory_add(game: &mut Game, side: eh::Side, item: i32) {
    if !active(game) {
        return;
    }
    let a = game.battle.army_mut(side_of(side));
    if let Some(c) = (item as usize).checked_sub(1).and_then(|i| a.inventory.get_mut(i)) {
        if *c < 99 {
            *c += 1;
        }
    }
}

/// Port of FUN_00446c6b: take one of the item if there is one.
pub fn inventory_check(game: &mut Game, side: eh::Side, item: i32) -> bool {
    if !active(game) {
        return false;
    }
    let a = game.battle.army_mut(side_of(side));
    match (item as usize).checked_sub(1).and_then(|i| a.inventory.get_mut(i)) {
        Some(c) if *c != 0 => {
            *c -= 1;
            true
        }
        _ => false,
    }
}

// ---- unit / party state -------------------------------------------------------------

/// Port of FUN_0045d922(&player, unit, n): add `n` EXP (no cap here) and
/// run the level-up check, with the camera on the unit when it is on the map.
pub fn set_unit_mode(game: &mut Game, u: eh::UnitRef, mode: i32) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        add_exp(game, r, mode);
    }
}

fn add_exp(game: &mut Game, r: UnitRef, n: i32) {
    {
        let u = game.battle.unit_mut(r);
        u.exp = (u.exp as i32 + n) as u16;
    }
    let on_map = alive_on_map(game, r);
    if on_map {
        let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
        game.battle.map_mut().center_on(x, y, true);
        camera::redraw(game);
        if presenting(game) {
            game.scr.copy_page(1, 0);
        }
    }
    actions::level_check(game, r);
    // FUN_00440264 (it may have changed class).
    if on_map {
        view::put_unit(game, r);
    }
}

/// Port of FUN_0045d8b2(&player, n, flag): every party member (not NPC) —
/// flag 0 only those on the map — gains `n` EXP (the first operand).
pub fn party_char_op(game: &mut Game, char_id: i32, flag: i32) {
    if !active(game) {
        return;
    }
    let n = game.battle.army(Side::Player).units.len();
    for i in 0..n {
        let r = UnitRef::new(Side::Player, i);
        let u = game.battle.unit(r);
        if u.npc == 0 && ((flag == 0 && u.flags & self::flag::DEPLOYED != 0) || flag == 1) {
            add_exp(game, r, char_id);
        }
    }
}

/// FUN_00442eb7(&player) only reads the army's gold; nothing to do.
pub fn party_op_10(_game: &mut Game) {}

/// Port of FUN_00441038(&player): every remaining unit rests.
pub fn party_op_13(game: &mut Game) {
    if !active(game) {
        return;
    }
    super::turn::end_turn(game, Side::Player);
}

/// Type 11/14: phase start for the player and FUN_00433d8f(&player).
pub fn party_setup_screen(game: &mut Game) {
    if !active(game) {
        return;
    }
    super::turn::phase_begin(game, Side::Player);
    super::turn::auto_phase(game, Side::Player);
}

// ---- unit actions (type 12) ---------------------------------------------------------

/// Port of FUN_0045bc6a(army, event). Returns false only when the unit does not exist.
pub fn army_unit_action(game: &mut Game, side: eh::Side, ev: EvArgs) -> bool {
    if !active(game) {
        return false;
    }
    let s = side_of(side);
    let Some(i) = game.battle.army(s).units.iter().position(|u| u.char_id == ev.b) else { return false };
    let r = UnitRef::new(s, i);
    if ev.sub >= 10 && !alive_on_map(game, r) {
        return true;
    }
    super::turn::phase_begin(game, s);
    if alive_on_map(game, r) {
        let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
        camera::ensure_visible(game, x, y, None);
    }
    let reset = |game: &mut Game| {
        let u = game.battle.unit_mut(r);
        u.flags = 0;
        u.npc = 1;
        u.char_id = 0;
    };
    match ev.sub {
        0 => {
            if alive_on_map(game, r) {
                view::erase_unit(game, r);
                camera::redraw_present(game);
            }
            reset(game);
        }
        1 => {
            if alive_on_map(game, r) {
                view::erase_unit(game, r);
                camera::redraw_present(game);
            }
            let u = game.battle.unit_mut(r);
            u.flags |= flag::RESERVE;
            u.flags &= !flag::DEPLOYED;
        }
        10 => {
            view::erase_unit(game, r);
            if presenting(game) {
                view::play_action(game, r, ev.d as u8, ev.c as u8);
            }
            view::put_unit(game, r);
            camera::redraw_full(game);
        }
        11 => {
            game.battle.unit_mut(r).act = 2;
            view::put_unit(game, r);
        }
        12 => {
            // FUN_0040a443(&DAT_00492af0, unit): the AI plays this unit.
            super::turn::ai_act(game, r);
        }
        13 | 14 | 15 => {
            let set = match ev.sub {
                13 => 1,
                14 => 2,
                _ => {
                    let t = game.battle.t();
                    let kind = game.battle.unit(r).kind;
                    t.kinds
                        .class(kind as u16)
                        .and_then(|c| c.actions.clone().find(|&a| t.kinds.actions[a].kind == 0xb))
                        .map_or(2, |a| t.kinds.actions[a].anim_set)
                }
            };
            view::erase_unit(game, r);
            if presenting(game) {
                view::play_action(game, r, ev.c as u8, set);
            }
            view::put_unit(game, r);
            camera::redraw_full(game);
        }
        16 => {
            view::erase_unit(game, r);
            actions::face(game, r, ev.c as u8);
            view::put_unit(game, r);
            camera::redraw_full(game);
        }
        17 | 18 => {
            if ev.sub == 17 {
                reset(game);
            }
            view::erase_unit(game, r);
            actions::die(game, r, 0);
        }
        19 | 20 => {
            if ev.sub == 19 {
                reset(game);
            }
            withdraw(game, r);
        }
        21 => {
            view::erase_unit(game, r);
            game.battle.unit_mut(r).npc = 1;
            switch_army(game, r);
            camera::redraw_present(game);
            reset(game);
        }
        22 => {
            let u = game.battle.unit_mut(r);
            u.npc = 0;
            u.ai_mode = 1;
            u.ai_sub = 0;
            u.flags &= !flag::RESERVE;
        }
        23 | 24 => {
            let u = game.battle.unit_mut(r);
            let d = if ev.sub == 23 { ev.c as i32 } else { u.hp_max as i32 * ev.c as i32 / 100 };
            u.hp = (u.hp as i32 - d) as i16;
            if u.hp < 1 {
                u.hp = 0;
                view::erase_unit(game, r);
                actions::die(game, r, 0);
            }
        }
        25 => game.battle.unit_mut(r).ai_mode = ev.c,
        26 => {
            let saved = game.battle.unit(r).ai_sub;
            game.battle.unit_mut(r).ai_sub = 1;
            super::turn::rest(game, r);
            game.battle.unit_mut(r).ai_sub = saved;
            let cap = game.battle.c(0xed);
            let u = game.battle.unit_mut(r);
            u.act = 0;
            u.turn_flags = 0;
            u.ap_carry = if (u.ap as i32) > cap { cap as i16 } else { u.ap };
            if u.ap_base as i32 + u.ap_carry as i32 > 0x6d {
                u.ap_carry = 0;
            }
            u.ap = u.ap_base + u.ap_carry;
        }
        27 | 28 => {
            // FUN_0044bfb2(unit, c, d or 1, d, 1): learn spells of school c.
            let from = if ev.sub == 27 { ev.d as i32 } else { 1 };
            let l = game.battle.learn_school_range(r, ev.c as i32, from, ev.d as i32);
            show_learned(game, r, &l, false);
        }
        _ => {}
    }
    true
}

/// Port of FUN_00459214(opponent, unit): a copy of the unit joins the other
/// army (on the map, not retreated; NPC there unless it is the player's).
pub fn switch_army(game: &mut Game, r: UnitRef) {
    let to = r.side.other();
    let mut u = game.battle.unit(r).clone();
    u.flags &= !flag::RESERVE;
    u.flags |= flag::DEPLOYED;
    let n = game.battle.army(to).units.len();
    u.slot = n as i16 + 1;
    u.npc = if to == Side::Player { 0 } else { 1 };
    game.battle.army_mut(to).units.push(u);
    // FUN_0043f9dd (sprite, FUN_00442022) / FUN_0043fc56 (portrait), FUN_00440264.
    let nr = UnitRef::new(to, n);
    view::capture(game, nr);
    view::put_unit(game, nr);
}

// ---- stats --------------------------------------------------------------------------------

pub fn unit_relation(game: &mut Game, u: eh::UnitRef, n: i32) -> i8 {
    if !active(game) {
        return 0;
    }
    match (from_ev(game, u), (n as usize).checked_sub(1)) {
        (Some(r), Some(i)) => game.battle.unit(r).affinity.get(i).map_or(0, |&v| v as i8),
        _ => 0,
    }
}

pub fn set_unit_relation(game: &mut Game, u: eh::UnitRef, n: i32, v: i8) {
    if !active(game) {
        return;
    }
    if let (Some(r), Some(i)) = (from_ev(game, u), (n as usize).checked_sub(1)) {
        if let Some(a) = game.battle.unit_mut(r).affinity.get_mut(i) {
            *a = v as u8;
        }
    }
}

/// Port of FUN_0045d9d1(&player, n, delta): party members (not NPC, not
/// retreated) change their relation toward character n, clamped to i8.
pub fn party_relation_op(game: &mut Game, b: i32, c: i32) {
    if !active(game) {
        return;
    }
    let Some(i) = (b as usize).checked_sub(1) else { return };
    for u in &mut game.battle.army_mut(Side::Player).units {
        if u.npc == 0 && u.flags & flag::RESERVE == 0 {
            if let Some(a) = u.affinity.get_mut(i) {
                *a = (*a as i8 as i32 + c).clamp(-0x80, 0x7f) as i8 as u8;
            }
        }
    }
}

// ---- group moves ----------------------------------------------------------------------

/// Type 15: the members play an action together (FUN_0044ecf8).
pub fn group_walk(game: &mut Game, side: eh::Side, members: &[WalkMember]) {
    if !active(game) {
        return;
    }
    let _ = side;
    let acts: Vec<(UnitRef, u8, u8)> =
        members.iter().filter_map(|m| from_ev(game, m.unit).map(|r| (r, m.c as u8, (m.d4 >> 2) as u8))).collect();
    let saved: Vec<u8> = acts.iter().map(|&(r, _, _)| game.battle.unit(r).act).collect();
    for &(r, _, _) in &acts {
        view::erase_unit(game, r);
        game.battle.unit_mut(r).act = 1;
    }
    if presenting(game) {
        view::play_actions(game, &acts);
    } else {
        for &(r, dir, _) in &acts {
            actions::face(game, r, dir);
        }
    }
    for (k, &(r, _, _)) in acts.iter().enumerate() {
        game.battle.unit_mut(r).act = saved[k];
        view::put_unit(game, r);
    }
}

/// Port of FUN_0045c37d(army, units, arrays, n, mode): the members walk at
/// the same time toward their targets (a character's cell when `p0` is a
/// character id, else (`p1`, `p2`)), one cell per 8 cs, drawn top to
/// bottom. Modes 1 and 3 keep the camera on the first member; modes 0 and 1
/// put everyone back to idle on the map afterwards.
pub fn group_move(game: &mut Game, side: eh::Side, members: &[MoveMember], mode: i32) {
    if !active(game) {
        return;
    }
    let s = side_of(side);
    let n = members.len();
    let mut units: Vec<Option<UnitRef>> = members.iter().map(|m| from_ev(game, m.unit)).collect();
    let mut paths: Vec<Vec<(i32, i32, u8)>> = vec![Vec::new(); n];
    let mut start = vec![(0, 0); n];
    for i in 0..n {
        let Some(r) = units[i] else { continue };
        let m = members[i];
        let target = if m.p0 == 0 {
            Some((m.p1, m.p2))
        } else {
            let ch = m.p0 as i16;
            let b = &game.battle;
            b.army(s)
                .find_char(ch, 0)
                .map(|k| (b.army(s).units[k].x, b.army(s).units[k].y))
                .or_else(|| b.army(s.other()).find_char(ch, 0).map(|k| (b.army(s.other()).units[k].x, b.army(s.other()).units[k].y)))
        };
        let Some((tx, ty)) = target else {
            units[i] = None;
            continue;
        };
        let (tx, ty) = game.battle.displace(s, tx, ty);
        start[i] = (game.battle.unit(r).x, game.battle.unit(r).y);
        paths[i] = game.battle.script_path(r, tx, ty);
        if paths[i].is_empty() {
            units[i] = None;
        } else {
            // Park the unit on its target so later members avoid that cell.
            let u = game.battle.unit_mut(r);
            u.x = tx;
            u.y = ty;
        }
    }
    let mut acts = vec![0u8; n];
    let mut order: Vec<usize> = (0..n).collect();
    let mut step = vec![0usize; n];
    for i in 0..n {
        if let Some(r) = units[i] {
            let u = game.battle.unit_mut(r);
            (u.x, u.y) = start[i];
            acts[i] = u.act;
        }
    }
    for &r in units.iter().flatten() {
        view::erase_unit(game, r);
        actions::set_anim(game, r, 1);
        game.battle.unit_mut(r).act = 1;
    }
    let mut slot = 0usize;
    let mut t = game.ticks();
    loop {
        // The original's exchange sort by current row.
        for a in 0..n {
            if units[a].is_none() {
                continue;
            }
            for b in a + 1..n {
                if units[b].is_none() {
                    continue;
                }
                let (ra, rb) = (units[order[a]], units[order[b]]);
                if let (Some(ra), Some(rb)) = (ra, rb) {
                    if game.battle.unit(rb).y < game.battle.unit(ra).y {
                        order.swap(a, b);
                    }
                }
            }
        }
        if mode == 1 || mode == 3 {
            if let Some(r0) = units[0] {
                let (x, y) = (game.battle.unit(r0).x, game.battle.unit(r0).y);
                game.battle.map_mut().center_on(x, y, true);
            }
        }
        camera::redraw(game);
        let mut moved = false;
        for &k in &order {
            let Some(r) = units[k] else { continue };
            if step[k] < paths[k].len() {
                let (x, y, _) = paths[k][step[k]];
                let f = {
                    let u = game.battle.unit(r);
                    super::movement::facing_toward(u.x, u.y, x, y, u.facing)
                };
                {
                    let u = game.battle.unit_mut(r);
                    u.x = x;
                    u.y = y;
                }
                if presenting(game) {
                    slot = footstep(game, r, x, y, slot);
                }
                actions::face(game, r, f);
                if presenting(game) {
                    view::draw_unit(game, r, 0);
                }
                moved = true;
                step[k] += 1;
            } else {
                actions::set_anim(game, r, 0);
                if mode < 2 && presenting(game) {
                    view::draw_unit(game, r, 0);
                }
            }
        }
        if !moved {
            break;
        }
        if presenting(game) {
            game.wait_until(t, 8);
            gfx::present(game);
            t = game.ticks();
        }
    }
    if mode < 2 {
        for i in 0..n {
            if let Some(r) = units[i] {
                view::capture(game, r);
                game.battle.unit_mut(r).act = acts[i];
                actions::set_anim(game, r, 0);
            }
        }
        for &r in units.iter().flatten() {
            view::put_unit(game, r);
        }
    }
}

// ---- story flag side effects ---------------------------------------------------------

/// Port of FUN_0041be7a for flags 1 and 3: show where the tile with object
/// class `attr` is and its location name.
pub fn reveal_special_tile(game: &mut Game, attr: i32) {
    if !active(game) {
        return;
    }
    let Some(m) = game.battle.map.as_ref() else { return };
    // FUN_00428774 goes through every cell (rows, then columns) and keeps
    // the last one of the class.
    let mut cell = None;
    for y in 0..m.rows() {
        for x in 0..m.width() {
            if m.object_class(x, y) as i32 == attr {
                cell = Some((x, y));
            }
        }
    }
    // FUN_00428774 raises error 5 when the map has no such tile.
    let Some((x, y)) = cell else { return };
    view::rebake_armies(game);
    camera::scroll_to(game, x, y, None);
    if presenting(game) {
        let name = view::motion::location_name(game, x, y);
        let (sx, sy) = view::cell_screen(game, x, y);
        let tx = (sx + 0x20 - name.len() as i32 * 6).clamp(5, 0x1fe);
        let ty = sy.clamp(5, 0x1c2);
        text::large(game, tx, ty, &name, 99, 0xa7, 1, 0);
        gfx::present(game);
        ui::click_to_continue(game, 400);
    }
}

/// Port of FUN_0045ceb9(&enemy): every living enemy on the map leaves,
/// nearest first (FUN_00433e6e mode 2), the camera following at full speed.
pub fn army_flag7(game: &mut Game) {
    if !active(game) {
        return;
    }
    let saved = game.cfg.screen_speed;
    game.cfg.screen_speed = 0;
    let (mut x, mut y) = (0, 0);
    loop {
        actions::pump(game);
        let Some(r) = super::turn::nearest_unit(&game.battle, Side::Enemy, x, y, 2) else { break };
        (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
        camera::ensure_visible(game, x, y, Some(r));
        game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
        camera::redraw_present(game);
    }
    game.cfg.screen_speed = saved;
}

// ---- dialogue context --------------------------------------------------------------

pub fn unit_dialogue_begin(game: &mut Game, u: Option<eh::UnitRef>) {
    if !active(game) {
        return;
    }
    if !presenting(game) {
        return;
    }
    game.scr.copy_page(2, 1);
    game.scr.set_page(1);
    if let Some(r) = u.and_then(|u| from_ev(game, u)) {
        view::draw_unit(game, r, 0);
    }
    game.scr.set_page(0);
}

pub fn unit_dialogue_end(game: &mut Game) {
    if !active(game) {
        return;
    }
    if presenting(game) {
        game.scr.copy_page(1, 2);
    }
}

pub fn unit_dialogue_finish(game: &mut Game, u: eh::UnitRef) {
    if !active(game) {
        return;
    }
    if !presenting(game) {
        return;
    }
    game.scr.copy_page(0, 1);
    if let Some(r) = from_ev(game, u) {
        view::draw_unit(game, r, 0);
    }
}

/// FUN_00417d8e's set-up: save the scroll and the unit's act byte, show
/// the unit idle on the map.
pub fn battle_talk_begin(game: &mut Game, u: eh::UnitRef) -> TalkSave {
    if !active(game) {
        return TalkSave::default();
    }
    let scroll = game.battle.map.as_ref().map_or((0, 0), |m| (m.scroll_x, m.scroll_y));
    let Some(r) = from_ev(game, u) else { return TalkSave { scroll, mode_2d: 0 } };
    let mode_2d = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = 0;
    // FUN_00442022 + FUN_00440264.
    view::capture(game, r);
    view::put_unit(game, r);
    TalkSave { scroll, mode_2d }
}

pub fn battle_talk_end(game: &mut Game, u: eh::UnitRef, saved: TalkSave) {
    if !active(game) {
        return;
    }
    if let Some(r) = from_ev(game, u) {
        // FUN_00441c0b, then the act byte back.
        view::erase_unit(game, r);
        game.battle.unit_mut(r).act = saved.mode_2d;
    }
    if let Some(m) = game.battle.map.as_mut() {
        (m.scroll_x, m.scroll_y) = saved.scroll;
    }
}

// ---- spells::EffectHooks ------------------------------------------------------------

/// The battle side of `spells::EffectHooks`. `cast_spell` / `use_item` only
/// hold the `Battle`, so a death is marked at once (later effects of the same
/// cast see the unit dead, as in the original) and the death sequence with
/// its script hook, the teleport presentation and the interactive effect
/// 0x27 run in [`finish_effects`] right after the cast.
#[derive(Debug, Default)]
pub struct BattleEffects {
    pub died: Vec<(UnitRef, i32)>,
    pub summoned: Vec<UnitRef>,
    /// (unit, from, to)
    pub teleported: Vec<(UnitRef, (i32, i32), (i32, i32))>,
    pub free_moves: Vec<UnitRef>,
    /// (unit drawn, message index)
    pub messages: Vec<(UnitRef, usize)>,
    /// (unit, school, spells learned, draw the unit)
    pub learned: Vec<(UnitRef, i32, Vec<usize>, bool)>,
    /// The casting unit, drawn behind a summon rising (FUN_004608e2).
    pub caster: Option<UnitRef>,
    /// FUN_0044b515: the quake item's user and the units it hit.
    pub quakes: Vec<(UnitRef, Vec<(UnitRef, i32)>)>,
    /// FUN_0044b41b: the slaying item's user and its victim.
    pub slays: Vec<(UnitRef, UnitRef)>,
}

impl super::spells::EffectHooks for BattleEffects {
    fn die(&mut self, b: &mut super::Battle, u: UnitRef, mode: i32) {
        b.unit_mut(u).flags |= flag::DEAD;
        self.died.push((u, mode));
    }

    fn slay(&mut self, b: &mut super::Battle, user: UnitRef, victim: UnitRef) {
        b.unit_mut(victim).flags |= flag::DEAD;
        self.slays.push((user, victim));
    }

    /// FUN_0043e5dd + FUN_00442022.
    fn summon(&mut self, b: &mut super::Battle, side: Side, kind: i32, x: i32, y: i32, level: i32) -> Option<UnitRef> {
        let r = b.create_summon(side, usize::try_from(kind).ok()?, x, y, level)?;
        self.summoned.push(r);
        Some(r)
    }

    /// FUN_00458334 (state part): the unit is moved at once.
    fn teleport(&mut self, b: &mut super::Battle, u: UnitRef, x: i32, y: i32) {
        let unit = b.unit_mut(u);
        let from = (unit.x, unit.y);
        unit.x = x;
        unit.y = y;
        self.teleported.push((u, from, (x, y)));
    }

    /// FUN_0045844b: recorded; the free move runs in `finish_effects`.
    fn effect_27(&mut self, _b: &mut super::Battle, caster: UnitRef) {
        self.free_moves.push(caster);
    }

    fn message(&mut self, _b: &mut super::Battle, unit: UnitRef, msg: usize) {
        self.messages.push((unit, msg));
    }

    fn learned(&mut self, _b: &mut super::Battle, unit: UnitRef, school: i32, spells: &[usize], draw_unit: bool) {
        self.learned.push((unit, school, spells.to_vec(), draw_unit));
    }

    fn quake(&mut self, _b: &mut super::Battle, user: UnitRef, hits: &[(UnitRef, i32)]) {
        self.quakes.push((user, hits.to_vec()));
    }
}

/// Play what a cast recorded in `fx`: deaths through `actions::die` (with
/// the unit-down script hook), summons and teleports appearing (FUN_00458334:
/// flash 0xc2 at the destination), and effect 0x27's free move.
pub fn finish_effects(game: &mut Game, mut fx: BattleEffects) {
    for (user, hits) in std::mem::take(&mut fx.quakes) {
        play_quake(game, user, &hits, &mut fx);
    }
    for (user, victim) in std::mem::take(&mut fx.slays) {
        play_slay(game, user, victim);
    }
    for (r, _, spells, draw_unit) in &fx.learned {
        show_learned(game, *r, spells, *draw_unit);
    }
    for (r, m) in fx.messages {
        // FUN_00424c5f, FUN_0043ff88, FUN_00431f6d(0xa0, 0x50, msg).
        if presenting(game) {
            camera::redraw(game);
            view::draw_unit(game, r, 0);
            let s = game.msg(m);
            crate::ui::message_box(game, 0xa0, 0x50, &s, crate::ui::Wait::Click(0));
        }
    }
    for r in fx.summoned {
        view::summon_sprite(game, r);
        match fx.caster {
            Some(c) => view::motion::summon_rise(game, c, r),
            None => {
                view::capture(game, r);
                view::put_unit(game, r);
            }
        }
    }
    for (r, from, to) in fx.teleported {
        // FUN_00458334 ran in the middle of the cast; replay it from the old cell.
        {
            let u = game.battle.unit_mut(r);
            (u.x, u.y) = from;
        }
        // FUN_00460fd0 puts the caster into the map surface for the move
        // (act 0, FUN_00440264) and takes it off again after (FUN_00441c0b).
        let caster = fx.caster.filter(|&c| c != r);
        if let Some(c) = caster {
            game.battle.unit_mut(c).act = 0;
            view::put_unit(game, c);
        }
        super::turn::teleport(game, r, to.0, to.1);
        if let Some(c) = caster {
            view::erase_unit(game, c);
            game.battle.unit_mut(c).act = 1;
        }
    }
    for (r, mode) in fx.died {
        game.battle.unit_mut(r).flags &= !flag::DEAD;
        view::erase_unit(game, r);
        actions::die(game, r, mode);
    }
    actions::show_level_ups(game);
    for r in fx.free_moves {
        free_move(game, r);
    }
}

/// Port of FUN_0045844b: the caster moves anywhere in its range (with
/// flag 0x4000000, MOVE5, set meanwhile: it may stay put) and must confirm a rest
/// there; AP is restored afterwards.
pub fn free_move(game: &mut Game, r: UnitRef) {
    let (x0, y0, f0, ap, carry) = {
        let u = game.battle.unit(r);
        (u.x, u.y, u.facing, u.ap, u.ap_carry)
    };
    game.battle.unit_mut(r).flags |= flag::MOVE5;
    game.battle.build_range(r, 0);
    loop {
        game.battle.map_mut().center_on(x0, y0, true);
        let k = super::menu::pick_destination_any(game);
        super::menu::walk(game, r, k);
        actions::set_anim(game, r, 0);
        view::capture(game, r);
        if super::menu::rest_command(game, r, 0) || !presenting(game) {
            break;
        }
        {
            let u = game.battle.unit_mut(r);
            u.x = x0;
            u.y = y0;
        }
        if game.battle.unit(r).facing != f0 {
            actions::face(game, r, f0);
        }
    }
    {
        let u = game.battle.unit_mut(r);
        u.ap = ap;
        u.ap_carry = carry;
        u.flags ^= flag::MOVE5;
    }
    camera::redraw(game);
    game.battle.range.reset();
}

/// FUN_0044bfb2's presentation (also between stages, for FLOW's item use):
/// over the map (not between stages; the unit redrawn when `draw_unit`), the EXP window (sysimg 0xd0) centred with its
/// shadow, then per learned spell "<name> <msg 3> <spell>" in colour 0x2a
/// at y 0xe4, presented, until a click or 250 cs.
pub fn show_learned(game: &mut Game, r: UnitRef, spells: &[usize], draw_unit: bool) {
    if spells.is_empty() || game.res.is_none() || game.battle.tables.is_none() {
        return;
    }
    let Ok(win) = game.res().ui_image(0xd0) else { return };
    let (w, h) = (win.width as i32, win.height as i32);
    let (x, y) = (0x140 - w / 2, 0xf0 - h / 2);
    if !game.campaign.between_stages && presenting(game) {
        camera::redraw(game);
        if draw_unit {
            view::draw_unit(game, r, 0);
        }
    }
    let lut = game.res().tints[6];
    gfx::window_shadow(game.scr.page(), &lut, x, y, w, h);
    let name = game.battle.unit(r).name.to_vec();
    let m = game.msg(3);
    for &s in spells {
        let sp = super::cast::spell_name_raw(game, s);
        let t = actions::sprintf(b"%s %s %s", &[actions::Arg::S(&name), actions::Arg::S(&m), actions::Arg::S(&sp)]);
        gfx::blit(game.scr.page(), &win, x, y, false);
        text::large(game, x + w / 2 - (t.len() as i32 - 2) * 6 - 0x10, 0xe4, &t, 0x2a, 0, 1, 2);
        gfx::present(game);
        ui::click_to_continue(game, 0xfa);
    }
}

/// Port of FUN_00458e13: a scripted flight, 8 cs a cell: per step the window
/// (centred on the unit in mode 0), the ground shadow, then the mount (move
/// type 4, sound 0x84 every step), the flying sword (5) or the unit lifted
/// 4 rows (sound 0x85), turned toward the step.
fn script_fly(game: &mut Game, r: UnitRef, steps: &[(i32, i32, u8)], mode: i32) {
    let mt = {
        let u = game.battle.unit(r).clone();
        game.battle.move_type(&u)
    };
    let mut slot = 0usize;
    for &(x, y, f) in steps {
        {
            let u = game.battle.unit_mut(r);
            u.x = x;
            u.y = y;
        }
        if mode == 0 {
            game.battle.map_mut().center_on(x, y, true);
        }
        game.scr.set_page(0);
        view::motion::draw_window(game);
        view::draw_ground_shadow(game, r);
        let turned = game.battle.unit(r).facing & 3 != f & 3;
        match mt {
            4 => {
                game.sys.audio.play_sound(&game.data.arcs, 0x84, 0);
                if turned {
                    view::step_unit(game, r, f);
                }
                view::motion::draw_flyer(game, r, true);
            }
            5 => {
                slot = game.sys.audio.play_sound(&game.data.arcs, 0x84, slot);
                if turned {
                    view::step_unit(game, r, f);
                }
                view::motion::draw_sword_rider(game, r);
            }
            _ => {
                slot = game.sys.audio.play_sound(&game.data.arcs, 0x85, slot);
                view::step_unit(game, r, f);
                view::motion::draw_lifted(game, r, 4);
            }
        }
        let t0 = game.battle.unit(r).anim_timer;
        game.wait_until(t0, 8);
        gfx::present(game);
        game.battle.unit_mut(r).anim_timer = game.ticks();
    }
}

/// Port of FUN_00458ff3: a scripted teleport: centred on the unit, sound
/// 0x68 and the dissolve (colour 0xc2; not for a hidden enemy outside VS),
/// then on the last step (scrolled there in mode 0 when off screen) sound
/// 0x68 and the reappearance.
fn script_teleport(game: &mut Game, r: UnitRef, steps: &[(i32, i32, u8)], mode: i32) {
    let Some(&(x, y, _)) = steps.last() else { return };
    let shown = |game: &Game| {
        let u = game.battle.unit(r);
        !game.battle.is_hidden(r.side, u) || game.battle.army(r.side).side == Side::Player || game.battle.vs
    };
    let (ox, oy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    game.battle.map_mut().center_on(ox, oy, true);
    game.scr.set_page(0);
    view::motion::draw_window(game);
    ui::sound(game, 0x68);
    if shown(game) {
        view::fade_unit(game, r, 0xc2, false);
    }
    {
        let u = game.battle.unit_mut(r);
        u.x = x;
        u.y = y;
    }
    if mode == 0 && !game.battle.map().on_screen(x, y, 2) {
        camera::scroll_to(game, x, y, None);
    }
    game.scr.set_page(0);
    view::motion::draw_window(game);
    view::capture(game, r);
    ui::sound(game, 0x68);
    if shown(game) {
        view::fade_unit(game, r, 0xc2, true);
    }
}

/// FUN_0044b41b's presentation: the user (act 0) into the map surface, the
/// camera to the victim when it is not well inside the view, the victim
/// off the surface and down (FUN_0044d14b), the user off the surface again.
fn play_slay(game: &mut Game, user: UnitRef, victim: UnitRef) {
    game.battle.unit_mut(user).act = 0;
    view::put_unit(game, user);
    let (x, y) = (game.battle.unit(victim).x, game.battle.unit(victim).y);
    camera::ensure_visible(game, x, y, None);
    game.battle.unit_mut(victim).flags &= !flag::DEAD;
    view::erase_unit(game, victim);
    actions::die(game, victim, 0);
    view::erase_unit(game, user);
}

/// FUN_0044b515 / FUN_0044b594's presentation: page 0 into 1, the shake
/// (0xe frames with the caster object's last effect sound; guess: effect 0
/// here), then per unit hit: the pump, the camera to it at full speed when
/// off screen (margin 2), its popup (FUN_00439cc0) and its death.
fn play_quake(game: &mut Game, user: UnitRef, hits: &[(UnitRef, i32)], fx: &mut BattleEffects) {
    if presenting(game) {
        game.scr.copy_page(1, 0);
        view::motion::shake_screen(game, 0xe, 0);
    }
    game.battle.unit_mut(user).act = 0;
    view::put_unit(game, user);
    let saved = game.cfg.screen_speed;
    game.cfg.screen_speed = 0;
    for &(r, v) in hits {
        actions::pump(game);
        let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
        if presenting(game) {
            if !game.battle.map().on_screen(x, y, 2) {
                camera::scroll_to(game, x, y, None);
            }
            game.scr.copy_page(1, 0);
            actions::show_miss(game, r, v);
        }
        if let Some(i) = fx.died.iter().position(|&(d, _)| d == r) {
            let (_, mode) = fx.died.remove(i);
            game.battle.unit_mut(r).flags &= !flag::DEAD;
            view::erase_unit(game, r);
            actions::die(game, r, mode);
        }
    }
    game.cfg.screen_speed = saved;
    view::erase_unit(game, user);
}
