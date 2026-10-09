//! Map camera: the scroll animations of the map object (`DAT_00497a68`)
//! and the redraw entry points the battle code calls. The scroll state is
//! `Map::scroll_x/scroll_y` (`+0x6c/+0x70`); drawing is `battle::view`.

use super::actions::presenting;
use super::army::UnitRef;
use super::view;
use crate::game::Game;
use crate::{cursor, gfx};

/// Target scroll for cell (x, y) (shared head of FUN_00426905 /
/// FUN_00426b87); `min_row` is the lower clamp of the row (1 / 2).
fn scroll_target(game: &Game, x: i32, y: i32, min_row: i32) -> (i32, i32) {
    let m = game.battle.map();
    let h = m.height(x, y);
    let mut c = if y & 1 == 0 { x * 2 - 9 } else { x * 2 - 8 };
    let mut r = y - (h + 0xf);
    if m.width() <= (c + 0x15) >> 1 {
        c = m.width() * 2 - 0x15;
    }
    if m.rows() <= r + 0x1e {
        r = m.rows() - 0x1e;
    }
    if c < 1 {
        c = 1;
    }
    if r < 1 {
        r = min_row;
    }
    (c, r)
}

/// One scroll step toward (c, r), drawn and presented with the 畫面速度 delay;
/// `unit` (with its move type, taken before the loop) is drawn over the
/// window before the present.
fn scroll_step(game: &mut Game, c: i32, r: i32, unit: Option<(UnitRef, i32)>) {
    {
        let m = game.battle.map_mut();
        m.scroll_y += (r - m.scroll_y).signum();
        m.scroll_x += (c - m.scroll_x).signum();
    }
    if !presenting(game) {
        return;
    }
    let t = game.ticks();
    view::draw_map(game);
    if let Some((u, mt)) = unit {
        draw_scrolled_unit(game, u, mt);
    }
    gfx::present(game);
    let speed = game.cfg.screen_speed;
    game.wait_until(t, speed);
    super::actions::pump(game);
}

/// The unit FUN_00426905 / FUN_00426b87 draw after each redraw: one that has
/// just moved (act 4) as its move type shows it (a flyer lifted 4 rows over
/// its shadow, FUN_00457345 / FUN_004573cb for the mounts), else FUN_0043ff88.
fn draw_scrolled_unit(game: &mut Game, u: UnitRef, mt: i32) {
    if game.battle.unit(u).act == 4 {
        match mt {
            3 => {
                view::draw_ground_shadow(game, u);
                view::motion::draw_lifted(game, u, 4);
            }
            4 => view::motion::draw_flyer(game, u, true),
            5 => view::motion::draw_sword_rider(game, u),
            _ => view::draw_unit(game, u, 0),
        }
    } else {
        view::draw_unit(game, u, 0);
    }
}

fn with_move_type(game: &Game, unit: Option<UnitRef>) -> Option<(UnitRef, i32)> {
    unit.map(|u| {
        let un = game.battle.unit(u).clone();
        (u, game.battle.move_type(&un))
    })
}

/// Port of FUN_00426905(map, x, y, unit, army): scroll step by step until
/// (x, y) has been inside the view for 4 steps or the target is reached
/// (immediately with 快速切換). `unit`, if any, is drawn over each frame.
pub fn scroll_to(game: &mut Game, x: i32, y: i32, unit: Option<UnitRef>) {
    let (c, r) = scroll_target(game, x, y, 1);
    let unit = with_move_type(game, unit);
    if game.cfg.quick_switch || !presenting(game) {
        let m = game.battle.map_mut();
        m.scroll_x = c;
        m.scroll_y = r;
    }
    let mut seen = 0;
    loop {
        scroll_step(game, c, r, unit);
        let m = game.battle.map();
        let done = m.scroll_x == c && m.scroll_y == r;
        if m.on_screen(x, y, 0) {
            seen += 1;
            if seen == 4 {
                break;
            }
        }
        if done {
            break;
        }
    }
}

/// Port of FUN_00426b87(map, x, y, unit, army): scroll until the view is
/// exactly at the target for (x, y) (row clamp 2, a quirk of this variant).
pub fn scroll_exact(game: &mut Game, x: i32, y: i32, unit: Option<UnitRef>) {
    let (c, r) = scroll_target(game, x, y, 2);
    let unit = with_move_type(game, unit);
    if game.cfg.quick_switch || !presenting(game) {
        let m = game.battle.map_mut();
        m.scroll_x = c;
        m.scroll_y = r;
    }
    loop {
        scroll_step(game, c, r, unit);
        let m = game.battle.map();
        if m.scroll_x == c && m.scroll_y == r {
            break;
        }
    }
}

/// Scroll to (x, y) unless it is already well inside the view
/// (`FUN_00426267(.., 2) == 0` → `FUN_00426905`), as most callers do.
pub fn ensure_visible(game: &mut Game, x: i32, y: i32, unit: Option<UnitRef>) {
    if !game.battle.map().on_screen(x, y, 2) {
        scroll_to(game, x, y, unit);
    }
}

/// FUN_00424c5f: the map window (with units) into the current page. Page 1
/// is the backdrop the frames copy before drawing the lifted units, so it
/// gets the window without them (`view::draw_backdrop`).
pub fn redraw(game: &mut Game) {
    if presenting(game) {
        if game.scr.cur == 1 {
            view::draw_backdrop(game);
        } else {
            view::draw_map(game);
        }
    }
}

/// FUN_00424c5f + FUN_0046eb48: redraw and present.
pub fn redraw_present(game: &mut Game) {
    if presenting(game) {
        view::draw_map(game);
        gfx::present(game);
    }
}

/// Port of FUN_004252c9: the view on page 0, the cursor, and present.
pub fn redraw_full(game: &mut Game) {
    if presenting(game) {
        game.scr.set_page(0);
        view::draw_map(game);
        cursor::draw_present(game, None, false);
    }
}
