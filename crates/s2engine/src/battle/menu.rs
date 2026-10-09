//! The player phase (`FUN_004323a3`) and the unit command menu
//! (`FUN_004346cb`, `FUN_00434893`, `FUN_00435a82`): map input, unit
//! selection, 行動/移動/道具/狀態/轉職/休息, target selection, moving,
//! resting, the battle system menu, and the checks after each command
//! (caves, treasure, tile flags, AI wake-ups). docs/battle.md §3.

use super::actions::{self, presenting};
use super::army::{aflag, Side, UnitRef};
use super::camera;
use super::turn;
use super::unit::{buff, flag, status, turn as tf};
use super::view::{self, overlay::RangeCell, overlay::RangeMark};
use crate::event;
use crate::game::Game;
use crate::sys::ev;
use crate::{cursor, gfx, text, ui};

// ---- map input -----------------------------------------------------------------------

/// Port of FUN_0042b9f8: keyboard → map command code (0 = none).
fn key_code(game: &mut Game) -> i32 {
    let (w, rows) = {
        let m = game.battle.map();
        (m.width(), m.rows())
    };
    if !game.get_event(ev::KEY_DOWN | ev::KEY_REPEAT | ev::KEY_UP) || game.ev.kind & 3 == 0 {
        return 0;
    }
    let sc = game.ev.scan();
    // Bit 0x20 of the modifiers marks the extended (grey) cursor keys here.
    if game.ev.mods & 0x20 != 0 {
        match sc {
            0x48 => return -0x10,
            0x50 => return -0xf,
            0x4b => return -0xe,
            0x4d => return -0xd,
            0x47 => return -0xc,
            0x49 => return -10,
            0x4f => return -0xb,
            0x51 => return -9,
            _ => {}
        }
    }
    if game.ev.mods & ev::M_ALT != 0 {
        match sc {
            0x1e => return -0x12,
            0x2d => return -0x11,
            _ => {}
        }
    }
    let (mut cx, mut cy) = game.battle.cursor;
    // The previous cell is kept in DAT_00497580/84 for FUN_00426056.
    let moved = |game: &mut Game, x: i32, y: i32| {
        game.battle.cursor_prev = game.battle.cursor;
        game.battle.cursor = (x, y);
        -6
    };
    // FUN_0042b9f8's jump table is read at 0x42bf84 + scan (the code
    // subtracts 1 and indexes 0x42bf85): Esc, Space/Enter, the eight cursor
    // keys 0x47..0x51, then , . numpad +/-, F2..F7, M, Tab, W, Q.
    match sc {
        0x01 => {
            game.wait_event(ev::KEY_UP);
            -1
        }
        0x39 | 0x1c => {
            if cx == -1 || cy == -1 {
                return 0;
            }
            game.wait_event(ev::KEY_UP);
            if sc == 0x39 {
                -8
            } else {
                0x1c
            }
        }
        0x48 if cy > 2 => moved(game, cx, cy - 2),
        0x50 if cy < rows - 2 => moved(game, cx, cy + 2),
        0x4b if (cy & 1 != 0 && cx > 0) || (cy & 1 == 0 && cx > 1) => moved(game, cx - 1, cy),
        0x4d if cx < w - 2 => moved(game, cx + 1, cy),
        0x47 if cy > 1 && cx > 0 => {
            if cy & 1 == 0 {
                cx -= 1;
            }
            moved(game, cx, cy - 1)
        }
        0x49 if cy > 1 && ((cx < w - 2 && cy & 1 != 0) || (cx < w - 1 && cy & 1 == 0)) => {
            if cy & 1 != 0 {
                cx += 1;
            }
            moved(game, cx, cy - 1)
        }
        0x4f if cy < rows - 1 && cx > 0 => {
            if cy & 1 == 0 {
                cx -= 1;
            }
            cy += 1;
            moved(game, cx, cy)
        }
        0x51 if cy < rows - 1 && ((cx < w - 2 && cy & 1 != 0) || (cx < w - 1 && cy & 1 == 0)) => {
            if cy & 1 != 0 {
                cx += 1;
            }
            moved(game, cx, cy + 1)
        }
        0x33 => -0x13,
        0x34 => -0x14,
        0x4e => -0x1e,
        0x4a => -0x1d,
        0x3c => -0x1c,
        0x3d => -0x1b,
        0x3e => -0x1a,
        0x3f => -0x19,
        0x40 => -0x18,
        0x41 => -0x17,
        0x32 => -0x16,
        0x0f => -0x15,
        0x11 => -0x20,
        0x10 => -0x1f,
        _ => 0,
    }
}

/// Port of FUN_00426056: after a keyboard move, a cursor whose top (the
/// cell's height, 1..=0x14, above its row) is above row 1 goes back to the
/// previous cell (−6); otherwise −5 when the cell has left the visible
/// window (scroll_x / 2 .. + 10 columns, rows scroll_y − 1 .. + 0x1e; the
/// callers scroll, FUN_00432ecf), else −6.
fn key_move_check(game: &mut Game) -> i32 {
    let (cx, cy) = game.battle.cursor;
    let m = game.battle.map();
    let h = match m.height(cx, cy) {
        0 => 1,
        h if h > 0x14 => 1,
        h => h,
    };
    let top = cy - (h - 1);
    if top < 1 {
        let (px, py) = game.battle.cursor_prev;
        game.battle.cursor = (px, py);
        view::cursor_set(game, px, py);
        return -6;
    }
    let (sx2, sy) = (m.scroll_x, m.scroll_y);
    let sx = sx2 >> 1;
    let out = if sx2 & 1 == 0 {
        (if cy & 1 == 0 { cx < sx } else { cx < sx - 1 }) || sx + 10 <= cx
    } else {
        cx < sx || if cy & 1 == 0 { sx + 10 < cx } else { sx + 10 <= cx }
    };
    if out || top < sy - 1 || sy + 0x1e <= top {
        -5
    } else {
        -6
    }
}

/// Port of FUN_0042b72e(&x, &y, mode): one map input step. Mouse at the
/// screen edge scrolls (−9..−0x10); a click on a cell selects it (left −3,
/// right −4); moving onto a new cell reports −6 / −0x21.
pub fn input(game: &mut Game, x: &mut i32, y: &mut i32, mode: i32) -> i32 {
    let mut code = key_code(game);
    if code != 0 {
        if code == -8 {
            (*x, *y) = game.battle.cursor;
            return -8;
        }
        if code != -6 {
            return code;
        }
        // FUN_0042bfd6: FUN_0042bfea, then FUN_00426056.
        let (cx, cy) = game.battle.cursor;
        view::cursor_set(game, cx, cy);
        return key_move_check(game);
    }
    let (mx, my) = game.sys.mouse();
    if !(10..=0x276).contains(&mx) || !(10..=0x1d6).contains(&my) {
        if (10..0x277).contains(&mx) {
            if my < 10 {
                return -0x10;
            }
            if my > 0x1d6 {
                return -0xf;
            }
        }
        if mx < 10 {
            return if my < 10 {
                -0xc
            } else if my > 0x1d6 {
                -0xb
            } else {
                -0xe
            };
        }
        if mx > 0x276 {
            return if my < 10 {
                -10
            } else if my > 0x1d6 {
                -9
            } else {
                -0xd
            };
        }
    }
    if !game.get_event(ev::MOUSE_DOWN | ev::MOUSE_UP | ev::MOUSE_MOVE) {
        return code;
    }
    let kind = game.ev.kind;
    if kind != ev::MOUSE_DOWN {
        if kind != ev::MOUSE_MOVE {
            return code;
        }
        code = -0x21;
    }
    let (ex, ey, mods) = (game.ev.x, game.ev.y, game.ev.mods);
    *x = ex;
    *y = ey;
    match view::screen_to_cell(game, ex, ey) {
        None => {
            *x = 0;
            *y = 0;
        }
        Some((cx, cy)) => {
            *x = cx;
            *y = cy;
            if kind == ev::MOUSE_MOVE && (cx, cy) != game.battle.cursor {
                view::cursor_set(game, cx, cy);
                code = -6;
            }
        }
    }
    if mods & ev::M_LBUTTON == 0 {
        let inner = (0x3c..=0x244).contains(&ex) && (0x28..=0x1b8).contains(&ey);
        if !inner && ((game.battle.map_click_mode < 8 && code == 0) || mode == 1) {
            code = 0;
        } else if (inner || mode == 0) && mods & ev::M_RBUTTON != 0 {
            code = if *x == 0 && *y == 0 && mode == 0 && !inner { 0 } else { -4 };
        }
    } else {
        game.wait_event(ev::MOUSE_UP);
        game.flush_events(0x18);
        code = -3;
    }
    code
}


// ---- the player phase ---------------------------------------------------------------

/// Port of FUN_004323a3: the player phase. Returns 0 when every unit has
/// acted (after the NPC allies' turn) or the stage code: 1 victory, 2
/// defeat, 7/8 loaded a save, 0xb quit, 0xf retreat.
pub fn player_phase(game: &mut Game) -> i32 {
    super::sync(game);
    let mut redraw_status = true;
    let mut cycle = 1usize;
    let mut sys = 0;
    let mut auto = 1;
    camera::redraw_full(game);
    loop {
        actions::pump(game);
        if !game.cfg.auto_switch {
            auto = 0;
        }
        if auto == 1 && next_unit(game, &mut cycle) == 0 {
            auto = 0;
        }
        let (mut x, mut y) = (0, 0);
        let code = input(game, &mut x, &mut y, 0);
        match code {
            0 => {
                if redraw_status {
                    status_redraw(game);
                    redraw_status = false;
                }
            }
            -0x21 => {
                // FUN_0043399f(-1, 1)
                if presenting(game) {
                    cursor::draw_present(game, None, true);
                }
            }
            -0x20 | -0x1f | -8 | -3 => {
                let (cx, cy) = game.battle.cursor;
                match select(game, cx, cy, None, code) {
                    3 => sys = system_menu(game),
                    0 => auto = 0,
                    1 => auto = 1,
                    2 => {
                        if !unit_info(game, Side::Player, cx, cy) {
                            unit_info(game, Side::Enemy, cx, cy);
                        }
                    }
                    4
                        if next_unit(game, &mut cycle) == 0 => {
                            auto = 0;
                        }
                    _ => {}
                }
                redraw_status = true;
            }
            -0x1e => game.cfg.sfx_volume = (game.cfg.sfx_volume + 1).min(100),
            -0x1d => game.cfg.sfx_volume = (game.cfg.sfx_volume - 1).max(0),
            -0x1c => {
                let _ = crate::flow::save::save_game_menu(game, false);
                status_redraw(game);
            }
            -0x1b => {
                let r = crate::flow::save::load_game_menu(game, false).unwrap_or(-1);
                if r == 8 || r == 7 {
                    return r;
                }
                status_redraw(game);
            }
            -0x1a => return turn::auto_phase(game, Side::Player),
            -0x19 => turn::end_turn(game, Side::Player),
            -0x18 => {
                game.cfg.status_window = !game.cfg.status_window;
                status_redraw(game);
            }
            -0x17 => {
                let (cx, cy) = game.battle.cursor;
                if !unit_info(game, Side::Player, cx, cy) {
                    unit_info(game, Side::Enemy, cx, cy);
                }
                status_redraw(game);
            }
            -0x16 => {
                crate::flow::status::map_overview(game);
                status_redraw(game);
            }
            -0x15 => {
                if next_unit(game, &mut cycle) == 0 {
                    auto = 0;
                }
            }
            -0x14 | -0x13 => {
                let d = if code == -0x14 { 1 } else { -1 };
                game.cfg.midi_volume = (game.cfg.midi_volume + d).clamp(0, 100);
                game.cfg.cd_volume = (game.cfg.cd_volume + d).clamp(0, 100);
                let v = game.cfg.midi_volume as u8;
                game.sys.audio.set_midi_volume(v);
            }
            -0x12 => {
                if presenting(game) {
                    // FUN_00433a1f, the banner, FUN_0043399f(-1, 0).
                    cursor::erase(game);
                    gfx::rect(game.scr.page(), 0, 0, 0x27f, 0x14, 0xfe, true);
                    text::small(
                        game,
                        0,
                        2,
                        b"The SuperSp Game V1.4 Copyright (c) 1997 by U&J TECHNOLOGY CO., LTD.",
                        0x2a,
                        0,
                        1,
                        2,
                    );
                    cursor::draw_present(game, None, false);
                }
            }
            -0x11 => return 0xb,
            -6 => redraw_status = true,
            -5 => {
                follow_cursor(game, &mut status_redraw);
                redraw_status = true;
            }
            -4 | 0x1c => {
                let (cx, cy) = game.battle.cursor;
                if !show_unit_range(game, Side::Player, cx, cy)
                    && game.battle.difficulty != 5
                    && !show_unit_range(game, Side::Enemy, cx, cy)
                {
                    game.battle.map_mut().center_on(cx, cy, true);
                }
                redraw_status = true;
            }
            -1 => {
                sys = system_menu(game);
                redraw_status = true;
            }
            c => {
                if game.battle.map_mut().scroll_key(c) {
                    redraw_status = true;
                    let t = game.ticks();
                    keep_cursor_in_view(game);
                    camera::redraw_full(game);
                    let s = game.cfg.screen_speed;
                    game.wait_until(t, s);
                }
            }
        }
        match sys {
            3 => turn::end_turn(game, Side::Player),
            6 => return turn::auto_phase(game, Side::Player),
            7 | 8 | 0xf | 0xb => return sys,
            _ => {}
        }
        event::poll(game, event::Trigger::Actions);
        sys = 0;
        let c = turn::stage_check(game);
        if c == 1 || c == 2 {
            return c;
        }
        if cycle == 0 || turn::all_done(&game.battle, Side::Player) {
            break;
        }
    }
    turn::npc_phase(game, Side::Player)
}

/// Port of FUN_004330e2: map window, cursor, status line.
pub(crate) fn status_redraw(game: &mut Game) {
    if !presenting(game) {
        return;
    }
    camera::redraw(game);
    view::cursor_track(game);
    view::draw_cursor(game);
    view::panel::status_bar(game, None);
    cursor::draw_present(game, None, false);
}

/// Port of FUN_00426009: when the cursor scrolled out of view, put it at
/// the centre of the view.
pub(crate) fn keep_cursor_in_view(game: &mut Game) {
    let (cx, cy) = game.battle.cursor;
    let m = game.battle.map();
    if !m.on_screen(cx, cy, 0) {
        let (x, y) = ((m.scroll_x >> 1) + 5, m.scroll_y + 0xf);
        view::cursor_set(game, x, y);
    }
}

/// Port of FUN_00433b80(army, &k): open the command menu for the next ready
/// unit, cycling from slot `k`. Returns the selection result (0 when no
/// ready unit is left, which also sets `k = 0`).
pub fn next_unit(game: &mut Game, k: &mut usize) -> i32 {
    next_unit_of(game, Side::Player, k)
}

/// FUN_00433b80 for army `side` (the VS phase uses both).
pub fn next_unit_of(game: &mut Game, side: Side, k: &mut usize) -> i32 {
    let ready = |game: &Game, i: usize| {
        let a = game.battle.army(side);
        let u = &a.units[i];
        u.act == 0
            && u.flags & flag::DEPLOYED != 0
            && u.buffs & buff::BERSERK == 0
            && u.can_act()
            && (u.npc != 1 || a.flags & aflag::NPC_CONTROL != 0)
    };
    let mut res = 0;
    loop {
        let n = game.battle.army(side).units.len();
        let mut found = false;
        for i in (k.saturating_sub(1)..n).chain(0..*k) {
            if i >= game.battle.army(side).units.len() || !ready(game, i) {
                continue;
            }
            found = true;
            let r = UnitRef::new(side, i);
            let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
            view::cursor_set(game, x, y);
            res = select_of(game, side, 0, 0, Some(r), 0);
            *k = i + 2;
            if game.battle.army(side).units.len() < *k {
                *k = 1;
            }
            if res != 4 {
                return res;
            }
        }
        if !found || res != 4 {
            *k = 0;
            return 0;
        }
    }
}

/// Port of FUN_004346cb(army, x, y, unit): act on a map cell (or `unit`).
/// 3 = nothing there, 2 = only show info (enemy, finished, NPC or berserk
/// unit), 1 = the unit lost its turn to fear, else the command loop result.
/// `code` is the map input that selected it: W (−0x1f) / E (−0x20) open
/// 行動 / 移動 straight away (FUN_00436186).
pub fn select(game: &mut Game, x: i32, y: i32, unit: Option<UnitRef>, code: i32) -> i32 {
    select_of(game, Side::Player, x, y, unit, code)
}

/// FUN_004346cb for army `side` (the VS phase uses both).
pub fn select_of(game: &mut Game, side: Side, x: i32, y: i32, unit: Option<UnitRef>, code: i32) -> i32 {
    let r = match unit {
        Some(r) => {
            let u = game.battle.unit(r);
            game.battle.cursor = (u.x, u.y);
            r
        }
        None => {
            let id = game.battle.army(side).unit_at(x, y);
            if id == 0 {
                return if game.battle.army(side.other()).unit_at(x, y) == 0 { 3 } else { 2 };
            }
            let r = UnitRef::new(side, id - 1);
            let u = game.battle.unit(r);
            let npc_ok = game.battle.army(side).flags & aflag::NPC_CONTROL != 0;
            if u.act == 2 || (u.npc == 1 && !npc_ok) || u.buffs & buff::BERSERK != 0 || !u.can_act() {
                return 2;
            }
            r
        }
    };
    if game.battle.unit(r).status & status::FEAR == 0 || game.battle.rng.pct() < 0x33 {
        if super::tracing() {
            let u = game.battle.unit(r).clone();
            let (sx, sy) = view::cell_screen(game, u.x, u.y);
            super::trace!("select {} ({},{}) screen ({},{}) ap {} hp {}", u.name_str().trim_end_matches('\0'), u.x, u.y, sx, sy, u.ap, u.hp);
            for side in [Side::Player, Side::Enemy] {
                for (i, v) in game.battle.army(side).units.clone().iter().enumerate() {
                    if v.flags & flag::DEPLOYED != 0 && v.flags & flag::DEAD == 0 {
                        let (vx, vy) = view::cell_screen(game, v.x, v.y);
                        super::trace!("  {:?}#{} {} ({},{}) scr ({},{}) hp {}/{} ap {} act {} npc {} flags {:x} buffs {:x} status {:x}", side, i, v.name_str().trim_end_matches('\0'), v.x, v.y, vx, vy, v.hp, v.hp_max, v.ap, v.act, v.npc, v.flags, v.buffs, v.status);
                    }
                }
            }
        }
        game.battle.unit_mut(r).act = 1;
        // FUN_00441c0b: off the map surface while it is commanded, then
        // FUN_00440264 back into it.
        view::erase_unit(game, r);
        actions::set_anim(game, r, 1);
        let res = command_loop(game, r, code);
        view::put_unit(game, r);
        camera::redraw_full(game);
        cave_check(game, r);
        treasure_check(game, r);
        after_action(game, r);
        turn::wake_on_touch(game, side.other());
        res
    } else {
        let u = game.battle.unit_mut(r);
        u.ap = 0;
        u.act = 2;
        view::put_unit(game, r);
        camera::redraw_full(game);
        1
    }
}

// ---- the command menu -----------------------------------------------------------------

/// One menu entry: sysimg icon slot (+1 = highlighted), label index, action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub icon: u16,
    pub label: u8,
    pub skill: usize,
}

/// The menu state (`DAT_0049fd9c`, `DAT_0049f9e8`, `DAT_0049fdc8`, `DAT_0049fdd8`, ...).
#[derive(Debug, Clone, Default)]
pub struct Menu {
    /// Main menu (`DAT_0049fd9c` / `DAT_0049fda8`).
    pub main: Vec<Entry>,
    /// 行動 submenu (`DAT_0049f9e8` / `DAT_0049fdd0` / `DAT_0049fde0`).
    pub actions: Vec<Entry>,
    /// `DAT_0049fdc8`: 0 main, 1 行動, 2 道具.
    pub level: u8,
    /// `DAT_0049fdd8`: selected entry, 1-based.
    pub sel: usize,
    /// `DAT_004975f4`: entry drawn highlighted.
    shown: usize,
    /// `DAT_0049fd98` / `DAT_0049fdec`: top-left of the icon row.
    x: i32,
    y: i32,
    /// `DAT_004975ac`: labels (msg 0x36.., weapon / skill names patched in).
    labels: Vec<Vec<u8>>,
}

impl Menu {
    /// `DAT_004975f0`: entries of the current level.
    fn count(&self) -> usize {
        match self.level {
            0 => self.main.len(),
            1 => self.actions.len(),
            _ => 2,
        }
    }

    /// The (icon, label, skill) of entry `sel` (1-based) at the current level.
    fn entry(&self, sel: usize) -> Entry {
        match self.level {
            0 => self.main.get(sel.wrapping_sub(1)).copied(),
            1 => self.actions.get(sel.wrapping_sub(1)).copied(),
            _ => Some(Entry { icon: if sel == 1 { 0xb8 } else { 0xba }, label: if sel == 1 { 0xe } else { 0xf }, skill: 0 }),
        }
        .unwrap_or(Entry { icon: 0, label: 0, skill: 0 })
    }
}

/// Port of FUN_00435a82: the main menu and 行動 submenu for `r`.
pub fn build_menu(game: &mut Game, r: UnitRef) -> Menu {
    let b = &game.battle;
    let u = b.unit(r).clone();
    let t = b.t();
    let mut labels: Vec<Vec<u8>> = (0..16).map(|i| game.msg(0x36 + i)).collect();
    labels.push(game.msg(0xc6));
    let mut bits: u32 = 0x1800_0000;
    if u.turn_flags & tf::MOVED == 0 && b.can_move(&u) {
        bits |= 0x4000_0000;
    }
    let ap = u.ap as i32;
    if u.npc != 1 && u.status & status::CURSE == 0 && (b.c(0xf1) <= ap || b.c(0xf2) <= ap) {
        bits |= 0x2000_0000;
    }
    let class = t.kinds.class(u.kind as u16);
    if u.flags & flag::SUMMON == 0 && u.npc != 1 {
        if let Some(c) = class {
            let can = super::rules::can_promote(u.level as i32, u.exp as i32, u.exp_next as i32, !c.promotions.is_empty(), b.vs);
            if can {
                bits |= 0x10000;
            }
        }
    }
    let spell_cost = b.c(0xef) + if u.turn_flags & (tf::MAGIC | tf::TECHNIQUE) != 0 { b.c(0xf3) } else { 0 };
    if b.knows_skill_kind(r, 1) && u.status & status::SEALED == 0 && !b.seal_magic && spell_cost <= ap {
        bits |= 0x0100_0000;
    }
    if b.knows_skill_kind(r, 2) && !b.seal_tech && spell_cost <= ap {
        bits |= 0x0080_0000;
    }
    let mut skills = [0usize; 9];
    if let Some(c) = class {
        for k in c.actions.clone() {
            let kind = t.kinds.actions[k].kind as i8 as i32;
            if kind == 0xb || kind == 0xc || !b.can_attack_with(r, k) {
                continue;
            }
            let mut cost = b.c(0xee);
            if u.abilities & 0x2000_0000 != 0 {
                cost -= 4;
            }
            if u.turn_flags & tf::ATTACKED != 0 && u.abilities & 0x8000 == 0 {
                cost += b.c(0xf3);
            }
            if cost > ap {
                continue;
            }
            let slot = if kind == 5 {
                bits |= 0x0200_0000;
                1
            } else if (1..=7).contains(&kind) {
                bits |= 0x0400_0000;
                labels[6] = match b.item(u.equip[0]) {
                    Some(_) if u.equip[0] != 0 => b.item_name_raw(u.equip[0]),
                    _ => b.action_name_raw(k),
                };
                0
            } else if kind == 0x17 {
                bits |= 0x0010_0000;
                6
            } else if kind == 0x20 {
                bits |= 0x0020_0000;
                5
            } else if kind == 0x24 {
                bits |= 0x0040_0000;
                4
            } else {
                bits |= 0x0008_0000;
                labels[13] = b.action_name_raw(k);
                7
            };
            skills[slot] = k;
        }
    }
    if bits & 0x07f8_0000 != 0 {
        bits |= 0x8000_0000;
    }
    let main_bits = [0x8000_0000u32, 0x4000_0000, 0x2000_0000, 0x1000_0000, 0x10000, 0x0800_0000];
    let main_icons = [0x9e, 0xa0, 0xa2, 0xa4, 0xbc, 0xa6];
    let main_labels = [0u8, 1, 2, 3, 0x10, 4];
    let mut main = Vec::new();
    for k in 0..6 {
        if bits & main_bits[k] != 0 {
            let mut label = main_labels[k];
            if ap < b.c(0xf0) && main_bits[k] == 0x0800_0000 {
                label += 1;
            }
            main.push(Entry { icon: main_icons[k], label, skill: 0 });
        }
    }
    let act_bits = [0x0400_0000u32, 0x0200_0000, 0x0100_0000, 0x0080_0000, 0x0040_0000, 0x0020_0000, 0x0010_0000, 0x0008_0000];
    let act_icons = [0xa8, 0xaa, 0xac, 0xae, 0xb0, 0xb2, 0xb4, 0xb6];
    let mut actions = Vec::new();
    for k in 0..8 {
        if bits & act_bits[k] != 0 {
            actions.push(Entry { icon: act_icons[k], label: 6 + k as u8, skill: skills[k] });
        }
    }
    Menu { main, actions, level: 0, sel: 1, shown: 0, x: 0, y: 0, labels }
}

/// Port of FUN_00435697: centre on the unit and draw the current menu
/// level next to it (page 1 holds the background); select its first entry
/// (the last one when the main menu has fewer than three).
fn draw_menu(game: &mut Game, r: UnitRef, m: &mut Menu) {
    let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    game.battle.map_mut().center_on(ux, uy, true);
    if !presenting(game) {
        m.sel = if m.level == 0 && m.main.len() < 3 { m.main.len() } else { 1 };
        m.shown = 0;
        return;
    }
    let (sx, mut sy) = view::cell_screen(game, ux, uy);
    game.scr.set_page(1);
    camera::redraw(game);
    view::panel::menu_status(game, r);
    let width = m.count() as i32 * 0x20;
    let w = if m.level == 2 { 0x40 } else { width };
    sy += 0x20;
    let mt = {
        let u = game.battle.unit(r).clone();
        game.battle.move_type(&u)
    };
    if (3..=5).contains(&mt) {
        sy -= 0x40;
    }
    sy = sy.max(0);
    if sy > 0x14a {
        sy -= game.battle.unit(r).sprite_h as i32 + 0x48;
    }
    let mut x = sx + (0x20 - w / 2);
    if x < 8 {
        x = 8;
    } else if x + w > 0x27f {
        x = 0x27f - w;
    }
    let lut = game.res().tints[6];
    let icons: Vec<u16> = if m.level == 2 { vec![0xb8, 0xba] } else { (1..=m.count()).map(|k| m.entry(k).icon).collect() };
    for (k, &id) in icons.iter().enumerate() {
        if let Ok(img) = game.res().ui_image(id as usize) {
            gfx::window_shadow(game.scr.page(), &lut, x + k as i32 * 0x20, sy, img.width as i32, img.height as i32);
        }
    }
    for (k, &id) in icons.iter().enumerate() {
        if let Ok(img) = game.res().ui_image(id as usize) {
            gfx::blit(game.scr.page(), &img, x + k as i32 * 0x20, sy, true);
        }
    }
    game.scr.set_page(0);
    m.x = x;
    m.y = sy;
    m.shown = 0;
    if m.level == 0 && m.main.len() < 3 {
        cursor::warp(game, x + 0x10 + m.main.len() as i32 * 0x14, sy + 0x14);
        m.sel = m.main.len();
    } else {
        cursor::warp(game, x + 0x10, sy + 0x14);
        m.sel = 1;
    }
    cursor::set_shape(game, 0xb);
}

/// Port of FUN_004351ad: every 0x13 cs the selected unit steps one frame.
fn idle_anim(game: &mut Game, r: UnitRef) -> bool {
    let t = game.battle.unit(r).anim_timer;
    if game.still_waiting(t, 0x13) {
        return false;
    }
    let f = game.battle.unit(r).facing;
    actions::face(game, r, f);
    if presenting(game) {
        game.scr.copy_page(0, 1);
        draw_by_move_type(game, r);
    }
    game.battle.unit_mut(r).anim_timer = game.ticks();
    true
}

/// The selected unit as FUN_004351ad / FUN_004352a8 / FUN_0044d7c1 draw it,
/// by its move type (FUN_0043aa26): 3 its ground shadow (FUN_00442811(.., 0))
/// and the unit 4 rows up (FUN_0042665a), 4 on its mount stepping now
/// (FUN_00457345(.., 1)), 5 on the flying sword (FUN_004573cb), otherwise
/// FUN_0043ff88.
fn draw_by_move_type(game: &mut Game, r: UnitRef) {
    let mt = {
        let u = game.battle.unit(r).clone();
        game.battle.move_type(&u)
    };
    match mt {
        3 => {
            view::draw_ground_shadow(game, r);
            view::motion::draw_lifted(game, r, 4);
        }
        4 => view::motion::draw_flyer(game, r, true),
        5 => view::motion::draw_sword_rider(game, r),
        _ => view::draw_unit(game, r, 0),
    }
}

/// Port of FUN_004352a8: highlight the selected entry and show its label.
fn hover(game: &mut Game, r: UnitRef, m: &mut Menu) {
    let ticked = idle_anim(game, r);
    if !presenting(game) {
        m.shown = m.sel;
        return;
    }
    if !ticked && (m.shown == m.sel || m.sel > m.count()) {
        return;
    }
    if m.sel > m.count() {
        cursor::draw_present(game, None, false);
        return;
    }
    if !ticked {
        game.scr.copy_page(0, 1);
        draw_by_move_type(game, r);
    }
    m.shown = m.sel;
    let e = m.entry(m.sel);
    let x = m.x + (m.sel as i32 - 1) * 0x20;
    if let Ok(img) = game.res().ui_image(e.icon as usize + 1) {
        gfx::blit(game.scr.page(), &img, x, m.y, true);
    }
    let label = m.labels.get(e.label as usize).cloned().unwrap_or_default();
    let len = text::strlen(&label);
    let mut lx = x - len * 4 + 0x10;
    if lx < 0 {
        lx = 0;
    } else if lx + len * 8 > 0x27f {
        lx = 0x27f - len * 8;
    }
    text::small(game, lx, m.y + 0x28, &label, 0x88, 0x9a, 1, 0);
    cursor::draw_present(game, None, false);
}

/// Port of FUN_00436186: menu input → 1 cancel, 0x39 choose `m.sel`, 0xf
/// (Tab) next unit, −0x21 pointer moved, 0 nothing.
fn menu_input(game: &mut Game, m: &mut Menu, code: &mut i32) -> i32 {
    // The W / E shortcut chosen on the map selects 行動 / 移動 once.
    if *code == -0x20 || *code == -0x1f {
        let want = if *code == -0x20 { 0xa0 } else { 0x9e };
        *code = 0;
        if m.level == 0 {
            if let Some(k) = m.main.iter().position(|e| e.icon == want) {
                m.sel = k + 1;
                return 0x39;
            }
        }
    }
    if !game.get_event(0xffff) || !(1..=0x20).contains(&game.ev.kind) {
        return 0;
    }
    let kind = game.ev.kind;
    if kind & 3 != 0 && kind < 4 {
        let sc = game.ev.scan();
        let warp = |game: &mut Game, m: &Menu| cursor::warp(game, m.x + (m.sel as i32 - 1) * 0x20 + 0x10, m.y + 0x14);
        return match sc {
            0x01 => {
                game.wait_event(ev::KEY_UP);
                1
            }
            0x1c | 0x39 => {
                game.wait_event(ev::KEY_UP);
                0x39
            }
            0x0f => {
                game.wait_event(ev::KEY_UP);
                0xf
            }
            0x4b => {
                m.sel = if m.sel <= 1 { m.count() } else { m.sel - 1 };
                warp(game, m);
                0
            }
            0x4d => {
                m.sel = if m.sel >= m.count() { 1 } else { m.sel + 1 };
                warp(game, m);
                0
            }
            _ => 0,
        };
    }
    if kind != ev::MOUSE_DOWN && kind != ev::MOUSE_MOVE {
        return 0;
    }
    let ret = if kind == ev::MOUSE_MOVE { -0x21 } else { 0 };
    let mods = game.ev.mods;
    if mods & ev::M_RBUTTON != 0 {
        game.wait_event(ev::MOUSE_UP);
        return 1;
    }
    let (dx, dy) = (game.ev.x - m.x, game.ev.y - m.y);
    if dx < 0 || dy < 0 || dx >= m.count() as i32 * 0x20 || dy >= 0x20 {
        return ret;
    }
    let k = (dx / 0x20 + 1) as usize;
    if k > m.count() {
        return ret;
    }
    if kind == ev::MOUSE_MOVE {
        m.sel = k;
    }
    if mods & ev::M_LBUTTON != 0 {
        game.wait_event(ev::MOUSE_UP);
        ui::sound(game, 100);
        return 0x39;
    }
    ret
}

/// Port of FUN_00434893(army, unit, code): the command loop. Returns 0 when the
/// unit was put back without acting, 4 to switch to the next unit (Tab
/// before acting), 1 when it finished.
pub fn command_loop(game: &mut Game, r: UnitRef, mut code: i32) -> i32 {
    super::sync(game);
    let mut result = 1;
    let mut can_undo = true;
    let mut sub_after_move = false;
    let mut first = true;
    let save = |game: &Game| {
        let u = game.battle.unit(r);
        (u.ap, u.ap_carry, u.x, u.y, u.facing)
    };
    let mut saved = save(game);
    let mt = {
        let u = game.battle.unit(r).clone();
        game.battle.move_type(&u)
    };
    if mt == 3 {
        view::motion::lift_off(game, r, false);
    }
    game.battle.unit_mut(r).anim_timer = game.ticks() + 0x13;
    let mut menu = build_menu(game, r);
    let mut rebuild = true;
    loop {
        actions::pump(game);
        if rebuild {
            menu = build_menu(game, r);
            draw_menu(game, r, &mut menu);
            rebuild = false;
            if menu.level == 0 && first && menu.main.first().map(|e| e.icon) == Some(0xa4) {
                // Nothing but 狀態 / 休息: go straight to resting.
                first = false;
                if rest_command(game, r, 0) {
                    game.battle.unit_mut(r).act = 2;
                    game.flush_events(0x1f);
                    return result;
                }
                if !rebuild {
                    draw_menu(game, r, &mut menu);
                }
                continue;
            }
        }
        hover(game, r, &mut menu);
        let code = menu_input(game, &mut menu, &mut code);
        match code {
            1 => {
                if game.battle.unit(r).turn_flags == 0 {
                    if menu.level == 0 {
                        game.battle.unit_mut(r).act = 0;
                        event::touched_clear(game);
                        game.battle.unit_mut(r).facing = saved.4;
                        return finish(game, r, 0);
                    }
                    menu.level = 0;
                    draw_menu(game, r, &mut menu);
                } else if can_undo {
                    if sub_after_move {
                        menu.level = 0;
                        draw_menu(game, r, &mut menu);
                        sub_after_move = false;
                    } else {
                        // Undo the move.
                        {
                            let u = game.battle.unit_mut(r);
                            (u.x, u.y, u.facing) = (saved.2, saved.3, saved.4);
                            u.turn_flags &= !tf::MOVED;
                            (u.ap, u.ap_carry) = (saved.0, saved.1);
                            u.act = 0;
                        }
                        // FUN_00442022 back on the cell it left.
                        view::capture(game, r);
                        actions::set_anim(game, r, 1);
                        event::touched_clear(game);
                        rebuild = true;
                        first = true;
                    }
                } else {
                    menu.level = 0;
                    draw_menu(game, r, &mut menu);
                }
            }
            0xf => {
                if game.battle.unit(r).turn_flags == 0 {
                    game.battle.unit_mut(r).act = 0;
                    result = 4;
                    event::touched_clear(game);
                    game.battle.unit_mut(r).facing = saved.4;
                    return finish(game, r, result);
                }
            }
            0x39 => {
                let e = menu.entry(menu.sel);
                match e.icon {
                    0x9e => {
                        if game.battle.unit(r).turn_flags & tf::MOVED != 0 {
                            sub_after_move = true;
                        }
                        menu.level = 1;
                    }
                    0xa0 => {
                        game.flush_events(0x1f);
                        saved = save(game);
                        if move_command(game, r) {
                            game.battle.unit_mut(r).turn_flags |= tf::MOVED;
                            can_undo = true;
                        }
                        game.battle.unit_mut(r).act = 1;
                        rebuild = true;
                    }
                    0xa2 => {
                        if game.battle.unit(r).turn_flags & tf::MOVED != 0 {
                            sub_after_move = true;
                        }
                        menu.level = 2;
                    }
                    0xa4 => {
                        // FUN_0044feb2: the unit status screen (FLOW's port).
                        set_click_mode(game, 0xb);
                        let _ = crate::flow::status::unit_status(game, r);
                    }
                    0xa6 => {
                        if rest_command(game, r, 0) {
                            game.battle.unit_mut(r).act = 2;
                            game.flush_events(0x1f);
                            return result;
                        }
                    }
                    0xac | 0xae => {
                        // FUN_0045f6cf (魔法) / FUN_0045f72d (招式).
                        if super::cast::magic_command(game, r, e.icon == 0xae) {
                            if game.battle.unit(r).flags & flag::DEAD != 0 {
                                game.battle.unit_mut(r).act = 2;
                                return finish(game, r, result);
                            }
                            view::erase_unit(game, r);
                            rebuild = true;
                            can_undo = false;
                            actions::set_anim(game, r, 1);
                        }
                    }
                    0xb8 => {
                        // FUN_00447490(army, 1, unit, 0): the item screen (FLOW's port).
                        if crate::flow::party::equipment(game, 1, Some(r), false).unwrap_or(0) == 1 {
                            let (c, cf3) = (game.battle.c(0xf1), game.battle.c(0xf3));
                            let _ = cf3;
                            {
                                let u = game.battle.unit_mut(r);
                                u.ap -= c as i16;
                                if u.ap < u.ap_carry {
                                    u.ap_carry = u.ap;
                                }
                                u.turn_flags |= tf::ITEM;
                            }
                            actions::set_anim(game, r, 1);
                            rest_command(game, r, 2);
                            game.battle.unit_mut(r).act = 2;
                            game.flush_events(0x1f);
                            return result;
                        }
                        rebuild = true;
                    }
                    0xba => {
                        // FUN_00447490(army, 0, unit, 0): the equipment screen (FLOW's port).
                        if crate::flow::party::equipment(game, 0, Some(r), false).unwrap_or(0) == 1 {
                            let c = game.battle.c(0xf2);
                            let u = game.battle.unit_mut(r);
                            u.ap -= c as i16;
                            if u.ap < u.ap_carry {
                                u.ap_carry = u.ap;
                            }
                            u.turn_flags |= tf::EQUIP;
                            can_undo = false;
                        }
                        rebuild = true;
                    }
                    0xbc => {
                        // FUN_00453986(army, unit, 0): the class change screen (FLOW's port).
                        if crate::flow::party::class_change(game, r, false).unwrap_or(0) == 2 {
                            view::reload_sprite(game, r);
                            view::motion::wipe_in(game, r);
                            actions::set_anim(game, r, 1);
                            rest_command(game, r, 2);
                            game.battle.unit_mut(r).act = 2;
                            game.flush_events(0x1f);
                            return result;
                        }
                        rebuild = true;
                    }
                    _ => {
                        if attack_command(game, r, e.skill) {
                            if game.battle.unit(r).flags & flag::DEAD == 0 {
                                pay_attack(game, r);
                                let u = game.battle.unit(r);
                                if u.buffs & buff::PETRIFIED == 0 && u.status & status::INCAPACITATED == 0 {
                                    set_click_mode(game, 0xb);
                                    actions::set_anim(game, r, 1);
                                    rebuild = true;
                                    can_undo = false;
                                    if !rebuild {
                                        draw_menu(game, r, &mut menu);
                                    }
                                    continue;
                                }
                            }
                            game.battle.unit_mut(r).act = 2;
                            return finish(game, r, result);
                        }
                    }
                }
                if !rebuild {
                    draw_menu(game, r, &mut menu);
                }
            }
            -0x21
                if presenting(game) => {
                    cursor::draw_present(game, None, true);
                }
            _ => {}
        }
    }
}

/// The common exit of FUN_00434893: idle animation, off the surface, flush.
fn finish(game: &mut Game, r: UnitRef, result: i32) -> i32 {
    actions::set_anim(game, r, 0);
    view::erase_unit(game, r);
    game.flush_events(0x1f);
    result
}

/// AP for an attack (FUN_00434893 after FUN_00436537): rules[0xee] (4 back
/// with ability 0x20000000); the second and later attacks of a turn also
/// pay rules[0xf3] unless ability 0x8000; the carry is capped by what is left.
fn pay_attack(game: &mut Game, r: UnitRef) {
    let (c_ee, c_f3) = (game.battle.c(0xee), game.battle.c(0xf3));
    let u = game.battle.unit_mut(r);
    u.ap -= c_ee as i16;
    if u.abilities & 0x2000_0000 != 0 {
        u.ap += 4;
    }
    if u.turn_flags & tf::ATTACKED == 0 {
        u.turn_flags |= tf::ATTACKED;
    } else if u.abilities & 0x8000 == 0 {
        u.ap -= c_f3 as i16;
    }
    if u.ap < u.ap_carry {
        u.ap_carry = u.ap;
    }
}

// ---- attack, move, rest -------------------------------------------------------------

/// Port of FUN_00436537(army, unit, skill): pick a target in range (Tab /
/// arrows cycle, the pointer picks one), showing the forecast, then attack.
/// Returns true when the attack happened.
pub fn attack_command(game: &mut Game, r: UnitRef, skill: usize) -> bool {
    let tl = game.battle.target_list(r, skill);
    if tl.targets.is_empty() {
        return false;
    }
    game.battle.unit_mut(r).act = 5;
    cursor::set_shape(game, 10);
    view::show_attack_range(game, &tl);
    game.battle.unit_mut(r).anim_timer = game.ticks() + 0x13;
    let mut ti = 0usize;
    let mut changed = true;
    let (tx0, ty0) = (tl.targets[0].x, tl.targets[0].y);
    if presenting(game) {
        let (sx, sy) = view::cell_screen(game, tx0, ty0);
        cursor::warp(game, sx + 0x20, sy + 4);
    }
    actions::face(game, r, tl.targets[0].facing);
    let mut duel = game.battle.duel(r, tl.targets[0].unit, skill);
    loop {
        actions::pump(game);
        let ticked = idle_anim(game, r);
        if ticked || changed {
            let t = tl.targets[ti];
            view::cursor_set(game, t.x, t.y);
            if changed {
                duel = game.battle.duel(r, t.unit, skill);
            }
            if presenting(game) {
                game.scr.copy_page(0, 1);
                view::draw_unit(game, r, 0);
                // FUN_0044179b: the tall cursor on the target's cell, then the forecast.
                view::draw_target_marker(game, t.x, t.y);
                view::panel::attack_forecast(game, &duel);
                cursor::draw_present(game, None, false);
            }
            changed = false;
        }
        let mut code = 0;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let kind = game.ev.kind;
            if kind < 4 {
                match game.ev.scan() {
                    0x01 => {
                        game.wait_event(ev::KEY_UP);
                        code = 1;
                    }
                    0x1c | 0x39 => {
                        game.wait_event(ev::KEY_UP);
                        code = 0x39;
                    }
                    0x0f | 0x48 | 0x4b | 0x4d | 0x50 => code = 0xf,
                    _ => {}
                }
            } else if kind == ev::MOUSE_DOWN || kind == ev::MOUSE_MOVE {
                let (ex, ey, mods) = (game.ev.x, game.ev.y, game.ev.mods);
                if kind == ev::MOUSE_MOVE {
                    if let Some((cx, cy)) = view::screen_to_cell(game, ex, ey) {
                        let k = tl.target_at(cx, cy);
                        if k != 0 && k - 1 != ti {
                            ti = k - 1;
                            changed = true;
                            actions::face(game, r, tl.targets[ti].facing);
                            continue;
                        }
                    }
                }
                if mods & ev::M_LBUTTON != 0 {
                    game.wait_event(ev::MOUSE_UP);
                    code = 0x39;
                } else if mods & ev::M_RBUTTON != 0 {
                    game.wait_event(ev::MOUSE_UP);
                    code = 1;
                }
            }
        }
        match code {
            1 => {
                game.flush_events(0x1f);
                return false;
            }
            0x39 => break,
            0xf => {
                ti = (ti + 1) % tl.targets.len();
                actions::face(game, r, tl.targets[ti].facing);
                changed = true;
            }
            _ => {}
        }
    }
    game.flush_events(0x1f);
    actions::attack(game, &tl, ti, duel);
    true
}

/// The visible stop cells of the current range (FUN_004726c8 / FUN_0047280f).
fn range_cells(game: &Game) -> Vec<RangeCell> {
    let b = &game.battle;
    let rg = &b.range;
    let move5 = rg.unit.is_some_and(|r| b.unit(r).flags & flag::MOVE5 != 0);
    let start = if move5 { 0 } else { 1 };
    (start..rg.count)
        .map(|k| rg.nodes[rg.order[k] as usize])
        .filter(|n| n.can_stop != 0 && b.map().on_screen(n.x as i32, n.y as i32, 0))
        .map(|n| RangeCell {
            col: n.x as i32,
            row: n.y as i32,
            height: n.height as i32,
            mark: match n.mark {
                1 => RangeMark::Attack,
                2 => RangeMark::Spell,
                _ => RangeMark::None,
            },
        })
        .collect()
}

/// Port of FUN_00470d26: show the range of `r` (the hard-mode variant for
/// enemies at difficulty ≥ 4 outside VS).
pub(crate) fn show_range(game: &mut Game, r: UnitRef) {
    let cells = range_cells(game);
    let hard = r.side == Side::Enemy && game.battle.difficulty >= 4 && !game.battle.vs;
    let u = game.battle.unit(r);
    let cell = (u.x, u.y);
    view::show_range(game, &cells, r.side, cell, hard);
}

/// Port of FUN_0044c240: show the range, pick a destination, walk there.
pub fn move_command(game: &mut Game, r: UnitRef) -> bool {
    game.flush_events(0x1f);
    let (x, y, f) = {
        let u = game.battle.unit(r);
        (u.x, u.y, u.facing)
    };
    game.battle.build_range(r, 0);
    game.battle.map_mut().center_on(x, y, true);
    show_range(game, r);
    let k = pick_destination(game);
    view::hide_range(game);
    if k == 0 {
        game.battle.unit_mut(r).facing = f;
        actions::set_anim(game, r, 1);
    } else {
        walk(game, r, k);
        view::capture(game, r);
        face_threat(game, r);
    }
    game.battle.range.reset();
    k != 0
}

/// Port of FUN_0044cd52: the destination cursor loop. Returns the node
/// (index + 1) of the chosen stop cell, 0 when cancelled.
fn pick_destination(game: &mut Game) -> usize {
    game.flush_events(0x1f);
    if super::tracing() {
        let cells: Vec<(i32, i32, i16)> = game.battle.range.reachable().filter(|(_, n)| n.can_stop == 1).map(|(_, n)| (n.x as i32, n.y as i32, n.budget)).collect();
        let mut best: Vec<(i32, i32, i16)> = cells.iter().copied().filter(|&(x, y, _)| game.battle.enemy_adjacent_cell(Side::Player, x, y)).collect();
        if best.is_empty() {
            best = cells.clone();
            best.sort_by_key(|c| -c.1);
        }
        best.sort_by_key(|c| -c.2);
        for &(x, y, b) in best.iter().take(4) {
            let (sx, sy) = view::cell_screen(game, x, y);
            super::trace!("  stop ({},{}) budget {} centre ({},{})", x, y, b, sx + 32, sy + 16);
        }
    }
    let mut last = game.battle.cursor;
    let mut scrolled = false;
    loop {
        actions::pump(game);
        let (mut x, mut y) = (0, 0);
        let code = input(game, &mut x, &mut y, 1);
        match code {
            0 => {
                if scrolled {
                    if let Some(r) = game.battle.range.unit {
                        show_range(game, r);
                    }
                    scrolled = false;
                }
                if game.battle.cursor != last {
                    let (cx, cy) = game.battle.cursor;
                    let move5 = game.battle.range.unit.is_some_and(|r| game.battle.unit(r).flags & flag::MOVE5 != 0);
                    let shape = match game.battle.range.mark_at(cx, cy, move5) {
                        0 => 0x11,
                        1 => 0x13,
                        2 => 0x12,
                        _ => 0xf,
                    };
                    cursor::set_shape(game, shape);
                    last = (cx, cy);
                }
                range_frame(game, 0);
            }
            -0x21 => {
                if presenting(game) {
                    cursor::draw_present(game, None, true);
                }
            }
            -8 | -3 | 0x1c => {
                let (cx, cy) = game.battle.cursor;
                let move5 = game.battle.range.unit.is_some_and(|r| game.battle.unit(r).flags & flag::MOVE5 != 0);
                let k = game.battle.range.find_stop(cx, cy, move5);
                if k != 0 {
                    cursor::set_shape(game, 0xb);
                    return k;
                }
            }
            -6 => {}
            -5 => follow_cursor(game, &mut |g| {
                if let Some(r) = g.battle.range.unit {
                    show_range(g, r);
                }
                range_frame(g, 0);
            }),
            -4 | -1 => {
                cursor::set_shape(game, 0xb);
                return 0;
            }
            c => {
                if game.battle.map_mut().scroll_key(c) {
                    scrolled = true;
                    let t = game.ticks();
                    keep_cursor_in_view(game);
                    range_frame(game, 1);
                    let s = game.cfg.screen_speed;
                    game.wait_until(t, s);
                }
            }
        }
    }
}

/// FUN_0044cd52 repeated until a cell is chosen (FUN_0045844b cannot be cancelled).
/// Headless, the farthest stop cell is taken.
pub fn pick_destination_any(game: &mut Game) -> usize {
    if let Some(r) = game.battle.range.unit {
        show_range(game, r);
    }
    if !presenting(game) {
        return game.battle.range.reachable().filter(|(_, n)| n.can_stop == 1).map(|(i, _)| i + 1).last().unwrap_or(1);
    }
    loop {
        let k = pick_destination(game);
        if k != 0 {
            view::hide_range(game);
            return k;
        }
    }
}

/// Port of FUN_0044cccd + FUN_0044c331: walk `r` to node `k - 1` of the
/// range (event tiles touched on the way are recorded).
pub fn walk(game: &mut Game, r: UnitRef, k: usize) {
    // The walk is drawn over the plain map (the range pages go away).
    view::hide_range(game);
    event::touched_clear(game);
    game.battle.unit_mut(r).act = 3;
    let move5 = game.battle.unit(r).flags & flag::MOVE5 != 0;
    game.battle.range.build_path(k, move5);
    let steps = game.battle.walk_steps();
    if presenting(game) {
        view::motion::animate_move(game, r, &steps);
    }
    // FUN_0044c331 / FUN_0044c631 record each cell stepped on, FUN_0044c963
    // (move type 2) only the last; none in VS or for the enemy army
    // (army+0xda != 0).
    if !game.battle.vs && game.battle.army(r.side).side == Side::Player {
        let mt = {
            let u = game.battle.unit(r).clone();
            game.battle.move_type(&u)
        };
        if mt == 2 {
            if let Some(&(x, y, _)) = steps.last() {
                event::touch_cell(game, x, y);
            }
        } else {
            for &(x, y, _) in &steps {
                event::touch_cell(game, x, y);
            }
        }
    }
    game.battle.execute_move(r);
    super::trace!("moved to ({},{}) ap {}", game.battle.unit(r).x, game.battle.unit(r).y, game.battle.unit(r).ap);
}

/// Port of FUN_0044e37d: after moving, face the nearest enemy when it could
/// reach the unit (its distance ≤ its longest attack reach + its AP / rules[0xec]).
pub fn face_threat(game: &mut Game, r: UnitRef) {
    let Some(e) = nearest_enemy(game, r) else { return };
    let (u, eu) = (game.battle.unit(r).clone(), game.battle.unit(e).clone());
    let d = super::map::distance(u.x, u.y, eu.x, eu.y);
    let reach = (eu.ap_base as i32 / game.battle.c(0xec).max(1)).max(0);
    if d <= max_reach(game, e) + reach {
        let f = super::movement::facing_toward(u.x, u.y, eu.x, eu.y, u.facing);
        game.battle.unit_mut(r).facing = (u.facing & 0xfc) | f;
        actions::face(game, r, f);
        actions::set_anim(game, r, 1);
    }
}

/// FUN_00434138(opponent, unit, 0): the nearest visible enemy (`ai::nearest_to`).
fn nearest_enemy(game: &Game, r: UnitRef) -> Option<UnitRef> {
    super::ai::nearest_to(&game.battle, r.side.other(), r, 0)
}

/// Port of FUN_00458283: the longest max range among the unit's usable actions.
fn max_reach(game: &Game, r: UnitRef) -> i32 {
    let b = &game.battle;
    let u = b.unit(r);
    let t = b.t();
    let Some(c) = t.kinds.class(u.kind as u16) else { return 0 };
    c.actions.clone().filter(|&k| b.skill_usable(u, k)).map(|k| b.skill_range(u, k).1).fold(0, i32::max)
}

/// Port of FUN_0044d7c1(army, unit, mode): the 休息 box — choose a facing
/// (numpad 7/9/1/3 or the arrows round the unit) and confirm. Mode 0 rests
/// from the menu (and touches the cell's events), 2 ends the turn after an
/// item (no cancel), 1 only sets the facing. Returns true when confirmed.
pub fn rest_command(game: &mut Game, r: UnitRef, mode: i32) -> bool {
    let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    if mode == 0 || mode == 2 {
        camera::scroll_exact(game, ux, uy, Some(r));
    }
    let saved_facing = game.battle.unit(r).facing;
    face_threat(game, r);
    let mut yes = true;
    let mut confirmed = None;
    if presenting(game) {
        let (mut ax, mut ay) = view::cell_screen(game, ux, uy);
        let Ok(bar) = game.res().ui_image(197) else { return false };
        // Image headers hold width − 1 and height − 1 (FUN_0044d7c1 uses them raw).
        let bx = 0x140 - ((bar.width as i32 - 1) >> 1);
        let mut by = ay + 0x32;
        if by + bar.height as i32 - 1 > 0x1de {
            let mt = {
                let u = game.battle.unit(r).clone();
                game.battle.move_type(&u)
            };
            let base = ay - game.battle.unit(r).sprite_h as i32 - (bar.height as i32 - 1);
            by = if (3..=5).contains(&mt) { base - 0x20 } else { base + 0x20 };
            by = by.max(0);
        }
        game.scr.set_page(1);
        if mode == 0 || mode == 2 {
            camera::redraw(game);
        } else {
            game.scr.copy_page(1, 2);
        }
        ax -= 0xc;
        ay -= 0xc;
        if let Ok(ring) = game.res().ui_image(28) {
            view::image_shadow(game, &ring, ax - 4, ay + 4);
            gfx::blit(game.scr.page(), &ring, ax, ay, false);
        }
        gfx::blit(game.scr.page(), &bar, bx, by, true);
        let m = game.msg(6);
        text::large(game, bx + 0x58 - text::strlen(&m) * 6, by + 8, &m, 0x2a, 0, 1, 2);
        game.scr.set_page(0);
        cursor::warp(game, bx + 0xb8, by + 0x14);
        game.battle.unit_mut(r).anim_timer = game.ticks() + 0x13;
        let arrows = [(29, 0, 0x24), (30, 0x3f, 0x24), (31, 0x3c, 0), (32, 0, 0)];
        let mut dirty = true;
        let mut moved = false;
        while confirmed.is_none() {
            actions::pump(game);
            let ticked = idle_anim(game, r);
            if moved && !(ticked || dirty) {
                // FUN_0043399f(-1, 1) after a pointer move.
                cursor::draw_present(game, None, true);
                moved = false;
            }
            if ticked || dirty {
                moved = false;
                if !ticked {
                    game.scr.copy_page(0, 1);
                    draw_by_move_type(game, r);
                }
                let f = (game.battle.unit(r).facing & 3) as usize;
                let (slot, dx, dy) = arrows[f];
                if let Ok(a) = game.res().ui_image(slot) {
                    gfx::blit(game.scr.page(), &a, ax + dx, ay + dy, false);
                }
                let sel = if yes { 0 } else { 1 };
                gfx::rect(game.scr.page(), bx + 0xa9 + sel * 0x1f, by + 5, bx + 0xc4 + sel * 0x1f, by + 0x20, 0x3c, false);
                cursor::draw_present(game, None, false);
                dirty = false;
            }
            if !game.get_event(0xffff) {
                continue;
            }
            let (kind, mods, ex, ey) = (game.ev.kind, game.ev.mods, game.ev.x, game.ev.y);
            let cur = game.battle.unit(r).facing & 3;
            let set_face = |game: &mut Game, f: u8| {
                if cur != f {
                    game.battle.unit_mut(r).anim_timer = game.ticks() + 0x13;
                    actions::face(game, r, f);
                }
            };
            if kind < 4 && kind & 3 != 0 {
                match game.ev.scan() {
                    0x01 => {
                        game.wait_event(ev::KEY_UP);
                        if mode != 2 {
                            confirmed = Some(false);
                        }
                    }
                    0x1c | 0x39 => {
                        game.wait_event(ev::KEY_UP);
                        confirmed = Some(yes);
                    }
                    0x0f | 0x4b | 0x4d => {
                        yes = !yes;
                        let sel = if yes { 0 } else { 1 };
                        cursor::warp(game, bx + 0xb8 + sel * 0x1f, by + 0x14);
                        dirty = true;
                    }
                    // A new facing shows at the next idle step (FUN_004351ad).
                    0x49 => set_face(game, 2),
                    0x4f => set_face(game, 0),
                    0x51 => set_face(game, 1),
                    0x47 => set_face(game, 3),
                    _ => {}
                }
            } else if kind == ev::MOUSE_DOWN || kind == ev::MOUSE_MOVE {
                let in_row = ey >= by + 4 && ey <= by + 0x20;
                if kind == ev::MOUSE_MOVE {
                    moved = true;
                }
                if kind == ev::MOUSE_MOVE && in_row {
                    if (bx + 0xa9..=bx + 0xc4).contains(&ex) && !yes {
                        yes = true;
                        dirty = true;
                    } else if (bx + 0xc8..=bx + 0xe3).contains(&ex) && yes {
                        yes = false;
                        dirty = true;
                    }
                }
                if mods & ev::M_LBUTTON != 0 {
                    if in_row && (bx + 0xa9..=bx + 0xc4).contains(&ex) {
                        confirmed = Some(true);
                    } else if in_row && (bx + 0xc8..=bx + 0xe3).contains(&ex) {
                        if mode != 2 {
                            confirmed = Some(false);
                        }
                    } else if ex >= ax - 4 && ey >= ay - 4 && ex <= ax + 0x60 && ey <= ay + 0x3c {
                        let f = if ex <= ax + 0x30 && ey <= ay + 0x1c {
                            Some(3)
                        } else if ey <= ay + 0x1c {
                            Some(2)
                        } else if ex <= ax + 0x2c {
                            Some(0)
                        } else if ex <= ax + 0x5c {
                            Some(1)
                        } else {
                            None
                        };
                        if let Some(f) = f {
                            set_face(game, f);
                        }
                    }
                    game.wait_event(ev::MOUSE_UP);
                } else if mods & ev::M_RBUTTON != 0 {
                    if mode != 2 {
                        confirmed = Some(false);
                    }
                    game.wait_event(ev::MOUSE_UP);
                }
            }
        }
        let pressed = if confirmed == Some(true) { 198 } else { 199 };
        cursor::erase(game);
        if let Ok(p) = game.res().ui_image(pressed) {
            let x = if pressed == 198 { bx + 0xa9 } else { bx + 0xc8 };
            gfx::blit(game.scr.page(), &p, x, by + 5, true);
            cursor::draw_present(game, None, false);
        }
        // FUN_0044cb5f(unit, 1): a flyer lands where it rests.
        let mt = {
            let u = game.battle.unit(r).clone();
            game.battle.move_type(&u)
        };
        if (3..=5).contains(&mt) && confirmed == Some(true) {
            view::motion::lift_off(game, r, true);
        }
    } else {
        confirmed = Some(true);
    }
    let ok = confirmed == Some(true);
    if ok {
        actions::set_anim(game, r, 0);
        if mode == 0 || mode == 2 {
            if game.battle.c(0xf0) <= game.battle.unit(r).ap as i32 {
                view::erase_unit(game, r);
                view::put_unit(game, r);
            }
            turn::pay_and_rest(game, r);
            if mode == 0 {
                event::touch_cell(game, ux, uy);
            }
        }
        game.battle.unit_mut(r).turn_flags |= tf::END_DONE;
    } else {
        game.battle.unit_mut(r).facing = saved_facing;
    }
    game.flush_events(0x1f);
    ok
}

// ---- after a command ----------------------------------------------------------------

/// Port of FUN_0044f9a1: a player unit (not in VS) entering a hidden-stage
/// spot (code 0xff / 0xfe with header targets) finds the cave.
pub fn cave_check(game: &mut Game, r: UnitRef) {
    let b = &game.battle;
    let u = b.unit(r);
    let npc_ok = b.army(r.side).flags & aflag::NPC_CONTROL != 0;
    if game.campaign.vs_mode || r.side != Side::Player || (u.npc == 1 && !npc_ok) {
        return;
    }
    if game.campaign.alt_next_stage == 0 || game.campaign.cave_found {
        return;
    }
    let Some(code) = b.map().spot_at(u.x, u.y) else { return };
    let targets = game.stage.as_ref().map(|s| s.stage.header.secret_next).unwrap_or_default();
    // DAT_00494698 / DAT_00494699: header +0x10 / +0x11.
    let target = match code & 0xff {
        0xff => targets[0],
        0xfe => targets[1],
        _ => 0,
    };
    if target != 0 {
        game.campaign.cave_found = true;
        game.campaign.alt_next_stage = target as u16;
        if presenting(game) {
            let m = game.msg(0x4d);
            ui::message_box(game, 0xa0, 0x50, &m, ui::Wait::Warp);
        }
    }
}

/// Port of FUN_0044f53e: a controllable unit on a treasure spot opens it:
/// one of its four items at random (a locked chest, spot bit 0x100, needs
/// ability 0x200000). The item goes to the army and the spot is emptied.
pub fn treasure_check(game: &mut Game, r: UnitRef) {
    let b = &game.battle;
    let u = b.unit(r).clone();
    let npc_ok = b.army(r.side).flags & aflag::NPC_CONTROL != 0;
    if !(game.campaign.vs_mode || r.side != Side::Enemy) || (u.npc == 1 && !npc_ok) {
        return;
    }
    let Some(code) = b.map().spot_at(u.x, u.y) else { return };
    let locked = code & 0x100 != 0;
    let k = (code & 0xff) as i32;
    let count = game.events.treasure_count;
    if k == 0 || k > count || !(1..0x15).contains(&count) {
        return;
    }
    let spot = (k - 1) as usize;
    let pick = game.battle.rng.rnd(4) as usize;
    let item = game.events.treasure[spot][pick] as i32 - 1;
    if !(0..0x12e).contains(&item) {
        return;
    }
    if locked && u.abilities & 0x20_0000 == 0 {
        if presenting(game) {
            let m = game.msg(0x104);
            ui::message_box(game, 0xa0, 0x50, &m, ui::Wait::Warp);
        }
        return;
    }
    game.events.treasure[spot] = [0; 4];
    game.battle.army_mut(r.side).add_item(item as usize + 1, 1);
    view::panel::treasure_window(game, r, item as usize);
}

/// Port of FUN_0044fae0: story flag 8 (`DAT_0049450f`: a player unit, not
/// an NPC unless the army controls them, on object class 4) and 6
/// (`DAT_0049450d`: an enemy on class 3).
pub fn after_action(game: &mut Game, r: UnitRef) {
    if game.campaign.vs_mode {
        return;
    }
    let u = game.battle.unit(r);
    let class = game.battle.map().object_class(u.x, u.y);
    let npc_ok = game.battle.army(r.side).flags & aflag::NPC_CONTROL != 0;
    if r.side == Side::Player {
        if (u.npc != 1 || npc_ok) && class == 4 && game.campaign.flags[8] == 0 {
            game.campaign.flags[8] = 1;
        }
    } else if class == 3 && game.campaign.flags[6] == 0 {
        game.campaign.flags[6] = 1;
    }
}

/// Port of FUN_0044fe15(army, x, y): the status screen of the unit there
/// (not for hidden enemies). True when there was a unit.
pub(crate) fn unit_info(game: &mut Game, side: Side, x: i32, y: i32) -> bool {
    let id = game.battle.army(side).unit_at(x, y);
    if id == 0 {
        return false;
    }
    let u = game.battle.army(side).units[id - 1].clone();
    let hidden = game.battle.is_hidden(side, &u);
    #[allow(clippy::nonminimal_bool)] // the original's two cases (VS / campaign) kept apart
    let unseen = (game.campaign.vs_mode && side.index() as i32 != game.battle.vs_side && hidden) || (!game.campaign.vs_mode && side == Side::Enemy && hidden);
    if unseen {
        return false;
    }
    set_click_mode(game, 0xb);
    let _ = crate::flow::status::unit_status(game, UnitRef::new(side, id - 1));
    true
}

/// Port of FUN_0044fb84(army, x, y): while a button is held, show the move
/// range of the unit there (with its full AP). True when there was a unit.
pub(crate) fn show_unit_range(game: &mut Game, side: Side, x: i32, y: i32) -> bool {
    let id = game.battle.army(side).unit_at(x, y);
    if id == 0 {
        return false;
    }
    let r = UnitRef::new(side, id - 1);
    let u = game.battle.unit(r).clone();
    let hidden = game.battle.is_hidden(side, &u);
    #[allow(clippy::nonminimal_bool)] // the original's two cases (VS / campaign) kept apart
    let unseen = (game.campaign.vs_mode && side.index() as i32 != game.battle.vs_side && hidden) || (!game.campaign.vs_mode && side == Side::Enemy && hidden);
    if unseen {
        return false;
    }
    let act = u.act;
    game.battle.unit_mut(r).act = 1;
    view::erase_unit(game, r);
    actions::set_anim(game, r, 1);
    // FUN_0044cb5f(unit, 0): a move type 3 flyer takes off.
    if game.battle.move_type(&u) == 3 {
        view::motion::lift_off(game, r, false);
    }
    let ap = u.ap;
    game.battle.unit_mut(r).ap = u.ap_base;
    game.battle.build_range(r, 0);
    show_range(game, r);
    if presenting(game) {
        let mut scrolled = false;
        loop {
            actions::pump(game);
            let (mut cx, mut cy) = (0, 0);
            match input(game, &mut cx, &mut cy, 0) {
                0 => {
                    if scrolled {
                        show_range(game, r);
                        scrolled = false;
                    }
                    range_frame(game, 0);
                }
                -0x21 => cursor::draw_present(game, None, true),
                -8 | -4 | -3 | -1 => break,
                -5 => follow_cursor(game, &mut |g| {
                    show_range(g, r);
                    range_frame(g, 0);
                }),
                c => {
                    if game.battle.map_mut().scroll_key(c) {
                        scrolled = true;
                        let t = game.ticks();
                        range_frame(game, 1);
                        let s = game.cfg.screen_speed;
                        game.wait_until(t, s);
                    }
                }
            }
        }
    }
    view::hide_range(game);
    game.battle.range.reset();
    game.battle.unit_mut(r).act = act;
    actions::set_anim(game, r, 0);
    view::erase_unit(game, r);
    view::put_unit(game, r);
    game.battle.unit_mut(r).ap = ap;
    true
}

// ---- the battle system menu ---------------------------------------------------------

/// Port of FUN_0042cc2c: 勝利條件 / 總檢表 / 系統設定 / 自動作戰 / 回合結束
/// (/ 撤離戰場 on stage kind 2). Returns 0, 3 (end turn), 6 (auto battle),
/// 7/8 (a save was loaded) or 0xf (retreat).
pub fn system_menu(game: &mut Game) -> i32 {
    if !presenting(game) {
        return 0;
    }
    let Ok(panel) = game.res().ui_image(0xc2) else { return 0 };
    let ox = -(panel.width as i32 / 2);
    let oy = -(panel.height as i32 / 2);
    let last = if game.battle.stage_kind == 2 { 5 } else { 4 };
    cursor::warp(game, 0x140, oy + 0x191);
    let mut sel: i32 = 0;
    let mut dirty = true;
    let mut full = true;
    let mut moved = false;
    let mut first = true;
    loop {
        actions::pump(game);
        if full {
            game.scr.set_page(1);
            camera::redraw(game);
            gfx::blit(game.scr.page(), &panel, ox + 0x140, oy + 0xf0, false);
            if last == 5 {
                if let Ok(p) = game.res().ui_image(0xc4) {
                    gfx::blit(game.scr.page(), &p, ox + 0x1d1, oy + 0x215, true);
                }
            }
            for k in 0..=last {
                let m = game.msg(0x79 + k as usize);
                text::large(game, 0x140 - text::strlen(&m) * 6, oy + 0x17c + k * 0x1f, &m, 0xe9, 0, 1, 2);
            }
            game.scr.set_page(0);
            full = false;
            dirty = true;
        }
        if dirty {
            game.scr.copy_page(0, 1);
            let m = game.msg(0x79 + sel as usize);
            text::large(game, 0x140 - text::strlen(&m) * 6, oy + 0x17c + sel * 0x1f, &m, 0x77, 0, 1, 2);
            cursor::draw_present(game, None, false);
            dirty = false;
            moved = false;
            // FUN_0042cc2c flushes pending input once, after the first draw
            // (FUN_00472f9e(0x1f)).
            if first {
                game.flush_events(0x1f);
                first = false;
            }
        } else if moved {
            // FUN_0043399f(-1, 1) after a pointer move.
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if !game.get_event(0xffff) || !(1..=0x20).contains(&game.ev.kind) {
            continue;
        }
        let (kind, mods, ex, ey) = (game.ev.kind, game.ev.mods, game.ev.x, game.ev.y);
        let mut code = 0;
        if kind < 4 {
            match game.ev.scan() {
                0x01 => {
                    game.wait_event(ev::KEY_UP);
                    code = 1;
                }
                0x48 => code = 0x48,
                0x50 => code = 0x50,
                0x1c | 0x39 => {
                    game.wait_event(ev::KEY_UP);
                    code = 0x39;
                }
                _ => {}
            }
        } else if kind == ev::MOUSE_DOWN || kind == ev::MOUSE_MOVE {
            if kind == ev::MOUSE_MOVE {
                moved = true;
            }
            if ex >= ox + 0x140 + 0x91 && ex <= ox + 0x140 + 0xf5 {
                let rows = [(0x8b, 0xa5), (0xaa, 0xc4), (0xc9, 0xe3), (0xe8, 0x102), (0x107, 0x121), (0x126, 0x140)];
                let hit = rows.iter().position(|&(a, b)| ey >= oy + 0xf0 + a && ey <= oy + 0xf0 + b).filter(|&k| k <= last as usize);
                if let Some(k) = hit {
                    if k as i32 != sel {
                        sel = k as i32;
                        dirty = true;
                    }
                    if mods & ev::M_LBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        code = 0x39;
                    }
                }
            }
            if mods & ev::M_RBUTTON != 0 {
                game.wait_event(ev::MOUSE_UP);
                code = 1;
            }
        }
        match code {
            0x48 => {
                sel = if sel == 0 { last } else { sel - 1 };
                cursor::warp(game, 0x140, oy + 0xf0 + 0xa1 + sel * 0x1f);
                dirty = true;
            }
            0x50 => {
                sel = if sel >= last { 0 } else { sel + 1 };
                cursor::warp(game, 0x140, oy + 0xf0 + 0xa1 + sel * 0x1f);
                dirty = true;
            }
            1 => {
                game.flush_events(0x1f);
                return 0;
            }
            0x39 => {
                game.flush_events(0x1f);
                // The pressed entry: sysimg 0xc3 and the label in colour 0x77.
                cursor::erase(game);
                if let Ok(p) = game.res().ui_image(0xc3) {
                    gfx::blit(game.scr.page(), &p, ox + 0x140 + 0x91, oy + 0xf0 + 0x8b + sel * 0x1f, true);
                }
                let m = game.msg(0x79 + sel as usize);
                text::large(game, 0x140 - text::strlen(&m) * 6, oy + 0xf0 + 0x8c + sel * 0x1f, &m, 0x77, 0, 1, 2);
                cursor::draw_present(game, None, false);
                ui::sound(game, 100);
                let res = match sel {
                    0 => {
                        turn::objectives_screen(game);
                        0
                    }
                    // FUN_0042f033 (總檢表) and FUN_0042d7cc (系統設定): FLOW's ports.
                    1 => crate::flow::status::overview_menu(game).unwrap_or(0),
                    2 => crate::flow::settings::system_submenu(game).unwrap_or(0),
                    3 => {
                        let m = game.msg(0x2d);
                        if ui::yes_no(game, 0xd4, oy + 0xf0 + 0x103, true, &m, false, false) { 6 } else { 0 }
                    }
                    4 => {
                        let m = game.msg(0x2c);
                        if ui::yes_no(game, 0xd4, oy + 0xf0 + 0x122, true, &m, false, false) { 3 } else { 0 }
                    }
                    _ => {
                        let m = game.msg(0x2e);
                        if ui::yes_no(game, 0xd4, oy + 0xf0 + 0x141, true, &m, false, false) {
                            ui::sound(game, 0x82);
                            0xf
                        } else {
                            0
                        }
                    }
                };
                if res != 0 {
                    return res;
                }
                cursor::warp(game, 0x140, oy + 0xf0 + 0xa1 + sel * 0x1f);
                full = true;
            }
            _ => {}
        }
    }
}




/// Port of FUN_00470dad(range, mode): one frame of a shown movement range.
/// Mode 0: the window with the range pages, the ranged unit stepping every
/// 0x13 cs, the cursor cell tracked, the status bar for the unit under the
/// cursor (or the ranged one); mode 1 (while scrolling): window, unit, cursor.
fn range_frame(game: &mut Game, mode: i32) {
    if !presenting(game) {
        return;
    }
    let r = game.battle.range.unit;
    game.scr.set_page(0);
    if mode == 0 {
        if let Some(r) = r {
            let mt = {
                let u = game.battle.unit(r).clone();
                game.battle.move_type(&u)
            };
            let t = game.battle.unit(r).anim_timer;
            if mt != 4 && mt != 5 && !game.still_waiting(t, 0x13) {
                let f = game.battle.unit(r).facing;
                actions::face(game, r, f);
                game.battle.unit_mut(r).anim_timer = game.ticks();
            }
        }
    }
    view::draw_battle(game);
    view::cursor_track(game);
    if mode == 0 {
        view::panel::status_bar(game, r);
    }
    cursor::draw_present(game, None, false);
}

/// Port of FUN_0042b3dd: `DAT_0048ea98`, the pointer mode FUN_0042b72e
/// tests (`< 8`: clicks outside the inner map area are ignored). It starts
/// at −1 (.data) and the screens set it to 0xb (FUN_0044feb2, FUN_0042f620,
/// FUN_004312c4, FUN_00434893 after an attack, ...).
pub fn set_click_mode(game: &mut Game, v: i32) {
    game.battle.map_click_mode = v;
}

/// Port of FUN_00432ecf(mode): after a −5 (the cursor left the window),
/// scroll step by step toward the cursor (diagonal, then straight, as the
/// original's chain of FUN_00426fdb calls) with `redraw` after each step
/// (mode 1 FUN_004330e2, 2 the range frame, 3 the aim, 4 the deploy
/// screen), 1 cs apart, until the cursor cell is on screen (margin 1) or a
/// needed scroll is impossible.
pub(crate) fn follow_cursor(game: &mut Game, redraw: &mut dyn FnMut(&mut Game)) {
    let (cx, cy) = game.battle.cursor;
    let h = match game.battle.map().height(cx, cy) {
        0 => 1,
        h if h > 0x14 => 1,
        h => h,
    };
    let c = cy - (h - 1);
    loop {
        actions::pump(game);
        let t0 = game.ticks();
        let (l, sy) = {
            let m = game.battle.map();
            (m.scroll_x >> 1, m.scroll_y)
        };
        let steps = [
            (cx <= l && c <= sy, -0xc),
            (cx <= l && sy + 0x19 <= c, -0xb),
            (l + 10 <= cx && c <= sy, -10),
            (l + 10 <= cx && sy + 0x19 <= c, -9),
            (c <= sy, -0x10),
            (sy + 0x19 <= c, -0xf),
            (cx <= l, -0xe),
            (l + 10 <= cx, -0xd),
        ];
        for (need, code) in steps {
            if need && !game.battle.map_mut().scroll_key(code) {
                return;
            }
        }
        redraw(game);
        if game.battle.map().on_screen(cx, cy, 1) || !presenting(game) {
            break;
        }
        game.wait_until(t0, 1);
    }
}

/// Port of FUN_0042c38a (aiming, after the map scrolled): a cursor outside
/// the window is put on its edge — the top-left cell for the corners (as
/// the original assigns them), the top / bottom row or the left / right
/// column otherwise — and stepped inward (FUN_00426056) until it is in.
pub(crate) fn pull_cursor_into_view(game: &mut Game) {
    let (l, sy) = {
        let m = game.battle.map();
        (m.scroll_x >> 1, m.scroll_y)
    };
    let (mut cx, mut cy) = game.battle.cursor;
    let h = match game.battle.map().height(cx, cy) {
        0 => 1,
        h if h > 0x14 => 1,
        h => h,
    };
    let c = cy - (h - 1);
    let step: Option<(i32, i32)> = if cx <= l && c <= sy {
        (cx, cy) = (l, sy);
        Some((1, 2))
    } else if cx <= l && sy + 0x19 <= c {
        (cx, cy) = (l, sy);
        Some((1, -2))
    } else if l + 10 <= cx && c <= sy {
        (cx, cy) = (l, sy);
        Some((-1, 2))
    } else if l + 10 <= cx && sy + 0x19 <= c {
        (cx, cy) = (l, sy);
        Some((-1, -2))
    } else if c <= sy {
        cy = sy;
        Some((0, 2))
    } else if sy + 0x19 <= c {
        cy = sy + 0x18;
        Some((0, -2))
    } else if cx <= l {
        cx = l;
        Some((1, 0))
    } else if l + 10 <= cx {
        cx = l + 9;
        Some((-1, 0))
    } else {
        None
    };
    game.battle.cursor = (cx, cy);
    for _ in 0..0x100 {
        if key_move_check(game) == -6 {
            break;
        }
        let Some((dx, dy)) = step else { break };
        let (x, y) = game.battle.cursor;
        game.battle.cursor = (x + dx, y + dy);
    }
    let (x, y) = game.battle.cursor;
    view::cursor_set(game, x, y);
}