//! The stage briefing typewriter, port of `FUN_00431ca5`.

use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::ev;
use crate::text;
use crate::ui;

/// Port of `FUN_00431ca5(mode)`: type the stage briefing (`DAT_00494744`).
/// Mode 1: on a cleared page, fading in; mode 0: over the darkened screen;
/// mode 2: on the world map, waiting for a key at the end.
pub fn narration(game: &mut Game, mode: i32) {
    if game.campaign.vs_mode {
        return;
    }
    let Some(t) = game.stage.as_ref().map(|s| s.stage.briefing.clone()) else { return };
    let x0 = if game.cfg.mainland { 0x9e } else { 0x3a };
    let master = game.res().master.clone();
    if mode == 1 {
        game.scr.clear();
    } else if mode == 0 {
        cursor::erase(game);
        let lut = game.res().tints[6];
        gfx::remap_page(game.scr.page(), &lut);
    }
    gfx::present(game);
    if mode == 1 {
        gfx::fade(game, &master, 0, 0);
    }
    let cur = game.scr.cur;
    game.scr.copy_page(1, cur);
    let mut pen = x0;
    let mut y = 0x96;
    let mut t0 = game.ticks();
    if mode == 2 {
        text::set(game, 10, 0, 1);
    } else {
        text::set(game, 0x38, 0, 1);
    }
    let mut lines = 0;
    let mut count = 0;
    let mut fast = false;
    let mut p = 0usize;
    loop {
        let wait_input = if game.still_waiting(t0, game.cfg.text_speed) && !fast {
            true
        } else {
            let rest = text::put_char(game, &mut pen, y, &t[p..], &mut count, 2);
            p = t.len() - rest.len();
            if !fast {
                t0 = game.ticks();
                gfx::present(game);
            }
            if p >= t.len() || t[p] == 0 || t[p] == b'\n' {
                if fast {
                    gfx::present(game);
                }
                if mode == 2 {
                    ui::wait_key(game, 0);
                } else {
                    ui::click_to_continue(game, 0);
                    gfx::fade(game, &master, 1, 0);
                }
                return;
            }
            if lines < 8 || count < 0x28 {
                if 0x27 < count || count == -1 {
                    y += 0x19;
                    pen = x0;
                    count = 0;
                    lines += 1;
                }
            } else {
                gfx::present(game);
                ui::click_to_continue(game, 500);
                game.scr.copy_page(cur, 1);
                lines = 0;
                count = 0;
                pen = x0;
                y = 0x96;
                fast = false;
            }
            !fast
        };
        if wait_input && game.get_event(0xffff) {
            let e = game.ev;
            if e.mods & ev::M_LBUTTON != 0 {
                fast = true;
            }
            if e.kind & (ev::KEY_DOWN | ev::KEY_REPEAT) != 0 && ((e.scan() & 0x39) != 0 || e.mods & ev::M_ALT != 0) {
                fast = true;
            }
        }
    }
}
