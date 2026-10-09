//! Common widgets (docs/ui.md §8): message box, banner, click-to-continue,
//! wait-for-key and the yes/no box.

use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// Resident UI image slots used here.
pub mod img {
    pub const PROMPT: usize = 13;
    pub const MESSAGE_BOX: usize = 154;
    pub const BANNER_LEFT: usize = 155;
    pub const BANNER_MID: usize = 156;
    pub const BANNER_RIGHT: usize = 157;
    pub const YES_NO_BAR: usize = 197;
    pub const YES_PRESSED: usize = 198;
    pub const NO_PRESSED: usize = 199;
}

/// Sound effect id of a menu confirm / toggle.
pub const SFX_CONFIRM: u16 = 100;

/// `FUN_00419b1d(_, id, 0)`.
pub fn sound(game: &mut Game, id: u16) {
    let arcs = &game.data.arcs;
    game.sys.audio.play_sound(arcs, id, 0);
}

/// `FUN_0042b390`: drop pending key-down, repeat and mouse-down events.
pub fn drop_presses(game: &mut Game) {
    while game.get_event(ev::KEY_DOWN | ev::KEY_REPEAT | ev::MOUSE_DOWN) {}
}

/// Wait mode of [`message_box`] / [`banner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// -1: move the pointer to (320, 400) and return.
    Warp,
    /// -2: return at once.
    None,
    /// Click-to-continue with a timeout in cs (0 = none).
    Click(i32),
}

/// `FUN_00431f6d(x, y, text, wait)`.
pub fn message_box(game: &mut Game, x: i32, y: i32, s: &[u8], wait: Wait) {
    let x = x.max(0x10);
    cursor::erase(game);
    let Ok(bg) = game.res().ui_image(img::MESSAGE_BOX) else { return };
    let lut = game.res().tints[6];
    gfx::window_shadow(game.scr.page(), &lut, x, y, bg.width as i32, bg.height as i32);
    gfx::blit(game.scr.page(), &bg, x, y, false);
    let tx = x + (bg.width as i32) / 2 - text::strlen(s) * 6;
    let ty = y - 0xc + (bg.height as i32) / 2;
    text::large(game, tx, ty, s, 0x2a, 0, 1, 2);
    gfx::present(game);
    match wait {
        Wait::Warp => cursor::warp(game, 0x140, 400),
        Wait::None => {}
        Wait::Click(t) => click_to_continue(game, t),
    }
}

/// `FUN_00432054(cx, cy, text, wait)`: three-piece banner centred on `cx`.
pub fn banner(game: &mut Game, cx: i32, cy: i32, s: &[u8], wait: Wait) {
    let cx = cx.max(0x10);
    cursor::erase(game);
    let (Ok(left), Ok(mid), Ok(right)) =
        (game.res().ui_image(img::BANNER_LEFT), game.res().ui_image(img::BANNER_MID), game.res().ui_image(img::BANNER_RIGHT))
    else {
        return;
    };
    let len = text::strlen(s);
    let x0 = cx - left.width as i32 - (((mid.width as i32) * len) >> 2);
    let y0 = cy - (left.height as i32) / 2;
    gfx::blit(game.scr.page(), &left, x0, y0, false);
    let mut x = x0 + left.width as i32;
    for _ in 0..len / 2 {
        gfx::blit(game.scr.page(), &mid, x, y0, false);
        x += mid.width as i32;
    }
    gfx::blit(game.scr.page(), &right, x, y0, false);
    text::large(game, cx - len * 6, cy - 0xc, s, 0x2a, 0, 1, 2);
    gfx::present(game);
    match wait {
        Wait::Warp => cursor::warp(game, 0x140, 400),
        Wait::None => click_to_continue(game, -2),
        Wait::Click(t) => click_to_continue(game, t),
    }
}

/// `FUN_0046d8a8(timeout_cs)`: blinking prompt that follows the pointer.
pub fn click_to_continue(game: &mut Game, timeout: i32) {
    let Ok(frames) = (0..2).map(|i| game.res().ui_image(img::PROMPT + i)).collect::<anyhow::Result<Vec<_>>>() else { return };
    let (w, h) = (frames[0].width as i32, frames[0].height as i32);
    let mut blink = 0usize;
    let (mut px, mut py) = game.sys.mouse();
    let mut under = gfx::grab(&game.scr.pages[game.scr.cur], px, py, px + w - 1, py + h - 1);
    gfx::blit(game.scr.page(), &frames[0], px, py, false);
    let mut t_blink = game.ticks();
    let t0 = game.ticks();
    let mut done = false;
    let mut dirty = true;
    while !done {
        crate::flow::music_keepalive(game);
        if dirty {
            gfx::present(game);
            dirty = false;
        }
        if game.get_event(0xffff) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => {
                    if e.mods & ev::M_SHIFT == 0 {
                        if e.scan() != scan::ALT {
                            game.wait_event(ev::KEY_UP);
                        }
                        done = true;
                    }
                }
                ev::MOUSE_DOWN => {
                    game.wait_event(ev::MOUSE_UP);
                    done = true;
                }
                ev::MOUSE_MOVE => {
                    gfx::blit(game.scr.page(), &under, px, py, true);
                    px = e.x;
                    py = e.y;
                    under = gfx::grab(&game.scr.pages[game.scr.cur], px, py, px + w - 1, py + h - 1);
                    gfx::blit(game.scr.page(), &frames[blink], px, py, false);
                    dirty = true;
                }
                _ => {}
            }
        }
        if done || (timeout > 0 && !game.still_waiting(t0, timeout)) {
            break;
        }
        if (game.ticks() - t_blink).abs() > 5 && !game.still_waiting(t_blink, 0x1e) {
            blink ^= 1;
            gfx::blit(game.scr.page(), &under, px, py, true);
            gfx::blit(game.scr.page(), &frames[blink], px, py, false);
            t_blink = game.ticks();
            dirty = true;
        }
    }
    game.flush_events(0x1f);
    gfx::blit(game.scr.page(), &under, px, py, true);
    drop_presses(game);
}

/// `FUN_0046d81f(timeout_cs)`: wait for a key or button press (or timeout, ≤0 = none).
pub fn wait_key(game: &mut Game, timeout: i32) {
    let t0 = game.ticks();
    loop {
        crate::flow::music_keepalive(game);
        if game.get_event(0xffff) && (game.ev.kind == ev::KEY_DOWN || game.ev.kind == ev::MOUSE_DOWN) {
            game.flush_events(ev::MOUSE_DOWN | ev::MOUSE_UP);
            break;
        }
        if timeout >= 1 && !game.still_waiting(t0, timeout) {
            break;
        }
    }
    game.flush_events(0x1f);
}

/// `FUN_0042c663(x, y, start_yes, text, force, set_pal)` → true for 是.
pub fn yes_no(game: &mut Game, x: i32, y: i32, start_yes: bool, s: &[u8], force: bool, set_pal: bool) -> bool {
    if !game.cfg.confirm && !force {
        return true;
    }
    let Ok(bar) = game.res().ui_image(img::YES_NO_BAR) else { return true };
    // FUN_00419b08: stop sound effects.
    game.sys.audio.stop_sounds();
    cursor::erase(game);
    let lut = game.res().tints[6];
    gfx::window_shadow(game.scr.page(), &lut, x, y, bar.width as i32, bar.height as i32);
    let mut sel: i32 = if start_yes { 0 } else { 1 };
    cursor::warp(game, x + 0xb8 + 0x1f * sel, y + 0x14);
    let mut redraw = true;
    let mut moved = false;
    let mut first = true;
    let mut result = 0;
    cursor::capture(game);
    while result == 0 {
        crate::flow::music_keepalive(game);
        if redraw {
            cursor::erase(game);
            gfx::blit(game.scr.page(), &bar, x, y, true);
            let len = text::strlen(s);
            if len < 0xf {
                text::large(game, x + 0x58 - len * 6, y + 8, s, 0x2a, 0, 1, 2);
            } else {
                text::small(game, x + 0x58 - len * 4, y + 8, s, 0x2a, 0, 1, 2);
            }
            gfx::rect(game.scr.page(), x + 0xa9 + sel * 0x1f, y + 5, x + 0xc4 + sel * 0x1f, y + 0x21, 0x3c, false);
            cursor::draw_present(game, None, false);
            if set_pal && first {
                let pal = game.res().master.clone();
                gfx::set_palette(game, &pal);
                first = false;
            }
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        let mut toggle = false;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::ESC => {
                        game.wait_event(ev::KEY_UP);
                        result = 1;
                    }
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        result = 0x39;
                    }
                    scan::TAB | scan::LEFT | scan::RIGHT => toggle = true,
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let in_row = (y + 4..=y + 0x20).contains(&e.y);
                    if in_row && e.kind == ev::MOUSE_MOVE {
                        if (x + 0xa9..=x + 0xc4).contains(&e.x) && sel == 1 {
                            sel = 0;
                            redraw = true;
                        } else if (x + 200..=x + 0xe3).contains(&e.x) && sel == 0 {
                            sel = 1;
                            redraw = true;
                        }
                    }
                    if e.mods & ev::M_LBUTTON != 0 {
                        // A click on 是 confirms the current selection, which
                        // the hover above normally moved there.
                        if in_row && (x + 0xa9..=x + 0xc4).contains(&e.x) {
                            result = 0x39;
                        } else if in_row && (x + 200..=x + 0xe3).contains(&e.x) {
                            result = 1;
                        }
                        game.wait_event(ev::MOUSE_UP);
                    } else if e.mods & ev::M_RBUTTON != 0 {
                        result = 1;
                        game.wait_event(ev::MOUSE_UP);
                    }
                }
                _ => {}
            }
        }
        if toggle {
            sel ^= 1;
            redraw = true;
            sound(game, SFX_CONFIRM);
            cursor::warp(game, x + 0xb8 + sel * 0x1f, y + 0x14);
        }
    }
    cursor::erase(game);
    let yes = if result == 1 {
        false
    } else {
        sound(game, SFX_CONFIRM);
        sel == 0
    };
    if let Ok(pressed) = game.res().ui_image(if yes { img::YES_PRESSED } else { img::NO_PRESSED }) {
        gfx::blit(game.scr.page(), &pressed, if yes { x + 0xa9 } else { x + 200 }, y + 5, true);
    }
    cursor::draw_present(game, None, false);
    game.flush_events(0x1f);
    yes
}
