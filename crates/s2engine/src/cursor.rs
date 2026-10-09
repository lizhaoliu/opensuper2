//! The software mouse cursor (docs/ui.md §5): drawn into the current page
//! with a 34x34 save-under, shape = resident UI image slot.

use s2data::sprite::Image;

use crate::game::Game;
use crate::gfx;

/// Default pointer shape (`FUN_00433906` sets `DAT_0048ec00 = 0xb`).
pub const SHAPE_POINTER: usize = 0xb;
/// Shape used on the title and town screens.
pub const SHAPE_ALT: usize = 0xc;

#[derive(Debug, Clone)]
pub struct Cursor {
    /// `DAT_004975a4`: software cursor drawn at all.
    pub enabled: bool,
    /// `DAT_004975a8`: enable counter.
    pub count: i32,
    /// `DAT_0048ec00`.
    pub shape: usize,
    /// `DAT_0048ec04/08`: where the save-under was taken (-1 = none).
    pub x: i32,
    pub y: i32,
    /// `DAT_004975a0`.
    pub under: Image,
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor { enabled: false, count: 0, shape: SHAPE_POINTER, x: -1, y: -1, under: Image::new(34, 34) }
    }
}

/// `FUN_00433985`.
pub fn set_shape(game: &mut Game, shape: usize) {
    game.cursor.shape = shape;
}

/// `FUN_00433a6b(on)`: nested enable/disable of the software cursor.
pub fn enable(game: &mut Game, on: bool) {
    let c = &mut game.cursor;
    c.count += if on { 1 } else { -1 };
    if c.enabled != on && ((on && c.count == 1) || (!on && c.count == 0)) {
        c.enabled = on;
    }
}

/// `FUN_004338f1(x, y)`: move the pointer.
pub fn warp(game: &mut Game, x: i32, y: i32) {
    game.sys.warp_mouse(x, y);
}

fn grab_under(game: &mut Game) {
    let (x, y) = game.sys.mouse();
    game.cursor.x = x;
    game.cursor.y = y;
    let page = &game.scr.pages[game.scr.cur];
    game.cursor.under = gfx::grab(page, x, y, x + 0x21, y + 0x21);
}

/// `FUN_004339cc`: capture the save-under at the pointer.
pub fn capture(game: &mut Game) {
    if game.cursor.enabled {
        grab_under(game);
    }
}

/// `FUN_00433a1f`: restore the save-under (erase the cursor).
pub fn erase(game: &mut Game) {
    let c = &game.cursor;
    if c.enabled && c.x != -1 && c.y != -1 {
        let (x, y) = (c.x, c.y);
        let under = c.under.clone();
        gfx::blit(game.scr.page(), &under, x, y, true);
    }
}

/// `FUN_00433ac7(restore)`: optionally erase the old cursor, then save the
/// background at the pointer and draw the shape there.
pub fn draw(game: &mut Game, restore: bool) {
    if !game.cursor.enabled {
        return;
    }
    if restore {
        erase(game);
    }
    grab_under(game);
    let (x, y) = (game.cursor.x, game.cursor.y);
    if let Ok(img) = game.res().ui_image(game.cursor.shape) {
        gfx::blit(game.scr.page(), &img, x, y, false);
    }
}

/// `FUN_0043399f(shape, restore)`: draw the cursor and present.
pub fn draw_present(game: &mut Game, shape: Option<usize>, restore: bool) {
    if let Some(s) = shape {
        set_shape(game, s);
    }
    draw(game, restore);
    gfx::present(game);
}
