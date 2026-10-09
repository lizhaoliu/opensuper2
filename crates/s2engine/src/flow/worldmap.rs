//! The world-map viewer at the inn, port of `FUN_00414b17` with
//! `FUN_00414db5`, `FUN_00415128`, `FUN_004152dc`, `FUN_00415413`,
//! `FUN_0041502d` and `FUN_00414ca1`.

use anyhow::{Context, Result};

use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// `FUN_00414d39(page)`: the 640x480 window of the world map at its scroll.
pub fn draw_window(game: &mut Game, page: usize) {
    let Some(w) = game.ui.world.as_ref() else { return };
    let (map, (sx, sy)) = (&w.map, w.scroll);
    let dst = &mut game.scr.pages[page];
    for y in 0..480usize {
        let row = sy as usize + y;
        if row >= map.height {
            break;
        }
        let x0 = sx as usize;
        let n = 640.min(map.width.saturating_sub(x0));
        dst.pixels[y * 640..y * 640 + n].copy_from_slice(&map.pixels[row * map.width + x0..row * map.width + x0 + n]);
    }
}

/// State of the viewer: `DAT_00492b58` (hovered location) and `DAT_00492bc8`
/// (page 0 holds a direct map draw, page 1 is stale).
struct View {
    hover: usize,
    direct: bool,
}

/// Port of `FUN_00414db5(mode)`: 0 full frame (page 1 copy, markers, the
/// hovered place name), 1 map straight onto page 0 while scrolling, 2 cursor only.
fn draw(game: &mut Game, v: &mut View, mode: i32) {
    let Some(w) = game.ui.world.as_ref() else { return };
    let (sx, sy) = w.scroll;
    let here = w.location(game.campaign.location);
    let next = w.location(game.campaign.next_stage);
    let (mk_here, mk_next) = (w.marker2.clone(), w.marker.clone());
    let (hx, hy) = (here.x as i32 - sx - 7, here.y as i32 - sy - 0x19);
    let (nx, ny) = (next.x as i32 - sx - 7, next.y as i32 - sy - 0x1e);
    let label = (v.hover != 0).then(|| {
        let l = w.location(v.hover as u16);
        let name = s2data::text::cstr(w.text(v.hover as u16)).to_vec();
        (l.x as i32 - sx, l.y as i32 - sy, name)
    });
    match mode {
        0 => {
            if v.direct {
                draw_window(game, 1);
            }
            v.direct = false;
            game.scr.copy_page(0, 1);
            gfx::blit(&mut game.scr.pages[0], &mk_here, hx, hy, false);
            gfx::blit(&mut game.scr.pages[0], &mk_next, nx, ny, false);
            if let Some((lx, ly, name)) = label {
                let len = name.len() as i32;
                let mut x = lx - len * 4;
                let mut y = ly - 0x18;
                if x < 0 {
                    x = 0;
                } else if 0x27f < x + len * 8 {
                    x = 0x27f - len * 8;
                }
                if y < 0 {
                    y = 0x18;
                }
                game.scr.set_page(0);
                text::small(game, x, y, &name, 3, 9, 1, 0);
            }
            cursor::draw_present(game, None, false);
        }
        1 => {
            draw_window(game, 0);
            v.direct = true;
            gfx::blit(&mut game.scr.pages[0], &mk_here, hx, hy, false);
            gfx::blit(&mut game.scr.pages[0], &mk_next, nx, ny, false);
            cursor::draw_present(game, None, false);
        }
        _ => cursor::draw_present(game, None, true),
    }
}

/// Port of `FUN_004152dc`: arrow / Home / End / PgUp / PgDn scroll codes, Esc = -1.
fn key_code(game: &mut Game) -> i32 {
    if !game.get_event(ev::KEY_DOWN | ev::KEY_REPEAT | ev::KEY_UP) || game.ev.kind & (ev::KEY_DOWN | ev::KEY_REPEAT) == 0 {
        return 0;
    }
    match game.ev.scan() {
        scan::UP => -0x10,
        scan::DOWN => -0xf,
        scan::LEFT => -0xe,
        scan::RIGHT => -0xd,
        scan::HOME => -0xc,
        scan::PGUP => -10,
        scan::END => -0xb,
        scan::PGDN => -9,
        scan::ESC => {
            game.wait_event(ev::KEY_UP);
            -1
        }
        _ => 0,
    }
}

/// Port of `FUN_00415128(&x, &y)`: one input poll. -1 / -4 leave, -3 jump to
/// the clicked point, -6 pointer moved, -9..-0x10 scroll (keys or the
/// pointer at a screen edge).
fn input(game: &mut Game, pos: &mut (i32, i32)) -> i32 {
    let k = key_code(game);
    if k != 0 {
        return k;
    }
    let mut code = 0;
    if game.get_event(ev::MOUSE_DOWN | ev::MOUSE_UP | ev::MOUSE_MOVE) {
        let e = game.ev;
        if e.kind == ev::MOUSE_UP {
            cursor::set_shape(game, 0xc);
        }
        *pos = (e.x, e.y);
        if e.mods & ev::M_LBUTTON != 0 {
            return -3;
        }
        if 0x3b < e.x && e.x < 0x245 && 0x27 < e.y && e.y < 0x1b9 && e.mods & ev::M_RBUTTON != 0 {
            game.wait_event(ev::MOUSE_UP);
            return -4;
        }
        if e.kind == ev::MOUSE_MOVE {
            code = -6;
        }
    }
    let (x, y) = game.sys.mouse();
    let inner_x = 4 < x && x < 0x27c;
    if inner_x && 4 < y && y < 0x1dc {
        return code;
    }
    if inner_x {
        if y < 5 {
            return -0x10;
        }
        if 0x1db < y {
            return -0xf;
        }
    }
    if x < 5 {
        code = if y < 5 { -0xc } else if y < 0x1dc { -0xe } else { -0xb };
    } else if 0x27b < x {
        code = if y < 5 { -10 } else if y < 0x1dc { -0xd } else { -9 };
    }
    code
}

/// Port of `FUN_00415413(code)`: scroll 4 pixels; true when the view moved.
fn scroll(game: &mut Game, code: i32) -> bool {
    let Some(w) = game.ui.world.as_mut() else { return false };
    let (mw, mh) = (w.map.width as i32, w.map.height as i32);
    let (x, y) = &mut w.scroll;
    let up = |y: &mut i32| {
        let b = 0 < *y;
        if b {
            *y -= 4;
        }
        b
    };
    let down = |y: &mut i32| {
        let b = *y < mh - 0x1e0;
        if b {
            *y += 4;
        }
        b
    };
    let left = |x: &mut i32| {
        let b = 0 < *x;
        if b {
            *x -= 4;
        }
        b
    };
    let right = |x: &mut i32| {
        let b = *x < mw - 0x280;
        if b {
            *x += 4;
        }
        b
    };
    let moved = match code {
        -0x10 => up(y),
        -0xf => down(y),
        -0xe => left(x),
        -0xd => right(x),
        -0xc => {
            let a = up(y);
            left(x) || a
        }
        -0xb => {
            let a = left(x);
            down(y) || a
        }
        -10 => {
            let a = right(x);
            up(y) || a
        }
        -9 => {
            let a = right(x);
            down(y) || a
        }
        _ => return false,
    };
    if moved {
        if *x < 0 {
            *x = 0;
        } else if mw - 0x280 <= *x {
            *x = mw - 0x281;
        }
        if *y < 0 {
            *y = 0;
        } else if mh - 0x1e0 <= *y {
            *y = mh - 0x1e1;
        }
    }
    moved
}

/// Port of `FUN_0041502d(x, y)`: the location within 20 px of the pointer;
/// true when the hovered location changed.
fn hover(game: &mut Game, v: &mut View, x: i32, y: i32) -> bool {
    let Some(w) = game.ui.world.as_ref() else { return false };
    let (px, py) = (x + w.scroll.0, y + w.scroll.1);
    let hit = w.locations.iter().position(|l| {
        let (lx, ly) = (l.x as i32, l.y as i32);
        !(px < lx - 0x14 || py < ly - 0x14 || lx + 0x14 < px || ly + 0x14 < py)
    });
    match hit {
        None if v.hover == 0 => false,
        None => {
            v.hover = 0;
            true
        }
        Some(i) if v.hover == i + 1 => false,
        Some(i) => {
            v.hover = i + 1;
            true
        }
    }
}

/// Port of `FUN_00414ca1(x, y)`: centre the view on the clicked point.
fn jump(game: &mut Game, x: i32, y: i32) {
    let Some(w) = game.ui.world.as_mut() else { return };
    let (mw, mh) = (w.map.width as i32, w.map.height as i32);
    let mut nx = w.scroll.0 - 0x140 + x;
    let mut ny = w.scroll.1 - 0xf0 + y;
    if nx < 0 {
        nx = 0;
    } else if mw < nx + 0x280 {
        nx = mw - 0x280;
    }
    if ny < 0 {
        ny = 0;
    } else if mh < ny + 0x1e0 {
        ny = mh - 0x1e0;
    }
    w.scroll = (nx, ny);
}

/// Port of `FUN_00414b17`: browse the world map until Esc or a right click.
pub fn world_map_view(game: &mut Game) -> Result<()> {
    let pal = game.ui.world.as_ref().map(|w| w.palettes[4].clone()).context("world map not loaded")?;
    let last = game.ui.world.as_ref().map_or(0, |w| w.hover);
    let mut v = View { hover: last, direct: false };
    draw_window(game, 1);
    draw(game, &mut v, 0);
    gfx::fade(game, &pal, 0, 0);
    super::music(game, 0x24);
    let mut pos = (0, 0);
    loop {
        super::music_keepalive(game);
        let code = input(game, &mut pos);
        match code {
            0 => {}
            -8 | -4 | -1 | 0x1c => break,
            -6 => {
                let changed = hover(game, &mut v, pos.0, pos.1);
                draw(game, &mut v, if changed { 0 } else { 2 });
            }
            -3 => {
                cursor::set_shape(game, 8);
                jump(game, pos.0, pos.1);
                draw_window(game, 1);
                draw(game, &mut v, 0);
            }
            _ => {
                if scroll(game, code) {
                    let t = game.ticks();
                    draw(game, &mut v, 1);
                    let speed = game.cfg.screen_speed;
                    game.wait_until(t, speed);
                }
            }
        }
        // Nothing else to wait on: the two input polls of FUN_00415128 are
        // the loop's only pause, as in the original.
    }
    gfx::fade(game, &pal, 1, 0);
    if let Some(w) = game.ui.world.as_mut() {
        w.hover = v.hover;
    }
    Ok(())
}
