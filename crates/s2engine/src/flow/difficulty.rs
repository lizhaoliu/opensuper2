//! Difficulty selection for a new game, port of `FUN_0040de96`.

use anyhow::Result;

use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// Footer values per row: ＡＩ強度, 簡單規則 (msg 0xe5 是 / 0xe6 否), 敵人強度, 經驗點數.
const AI: [&[u8]; 5] = [b"5%", b"25%", b"50%", b"80%", b"100%"];
const SIMPLE: [usize; 5] = [0xe5, 0xe6, 0xe6, 0xe6, 0xe6];
const ENEMY: [&[u8]; 5] = [b"-20%", b"-10%", b"0%", b"+10%", b"+20%"];
const EXP: [&[u8]; 5] = [b"300%", b"200%", b"100%", b"100%", b"100%"];

/// Port of `FUN_0040de96`: true when confirmed (sets `campaign.difficulty`).
pub fn difficulty_menu(game: &mut Game) -> Result<bool> {
    super::stop_music(game);
    // FUN_0042267c(&imgs, 2, {4, 3}): DAT 7 images 4 and 3, drawn on page 0.
    let panel = game.res().dat7_image(4)?;
    let bar = game.res().dat7_image(3)?;
    gfx::blit(game.scr.page(), &panel, 0xad, 0xdf, false);
    for x in [9, 0xa9, 0x149, 0x1e9] {
        gfx::blit(game.scr.page(), &bar, x, 0x1c5, false);
    }
    let title = game.msg(0xd7);
    text::large(game, 0x127, 0xed, &title, 0x73, 0, 1, 2);
    let go = game.msg(0xdd);
    text::large(game, 0x13f, 0x19a, &go, 0x6b, 0, 1, 2);
    for i in 0..5 {
        let s = game.msg(0xd8 + i);
        text::large(game, 0x118, i as i32 * 0x1c + 0x10b, &s, 0x34, 0, 1, 2);
    }
    for k in 0..4 {
        let s = game.msg(0xde + k);
        text::small(game, k as i32 * 0xa0 + 0x12, 0x1c9, &s, 0x71, 0, 1, 2);
    }
    game.scr.copy_page(1, 0);
    let mut changed = true;
    let mut sel: i32 = 2;
    let mut action = 0;
    let mut done = false;
    let mut warp_pending = true;
    let mut cursor_dirty = false;
    let mut first = true;
    let mut confirmed = false;
    super::music(game, 0x19);
    loop {
        if done {
            game.flush_events(0x1f);
            return Ok(confirmed);
        }
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if warp_pending {
            cursor::warp(game, 0x140, sel * 0x1c + 0x117);
            warp_pending = false;
            cursor_dirty = true;
        }
        if !changed {
            if cursor_dirty {
                cursor::draw_present(game, None, true);
                cursor_dirty = false;
            }
        } else {
            game.scr.copy_page(0, 1);
            let s = game.msg(0xd8 + sel as usize);
            text::large(game, 0x118, sel * 0x1c + 0x10b, &s, 0x3c, 0x6c, 1, 0);
            gfx::rect(game.scr.page(), 0xf9, sel * 0x1c + 0x10a, 0x10a, sel * 0x1c + 0x122, 0x3c, true);
            let r = sel as usize;
            text::small(game, 0x6e, 0x1c9, AI[r], 0x71, 0, 1, 2);
            let simple = game.msg(SIMPLE[r]);
            text::small(game, 0x10e, 0x1c9, &simple, 0x71, 0, 1, 2);
            text::small(game, 0x1ae, 0x1c9, ENEMY[r], 0x71, 0, 1, 2);
            text::small(game, 0x24e, 0x1c9, EXP[r], 0x71, 0, 1, 2);
            if first {
                cursor::capture(game);
            }
            cursor::draw(game, false);
            gfx::present(game);
            if first {
                first = false;
                let master = game.res().master.clone();
                gfx::fade(game, &master, 0, -4);
            }
            cursor_dirty = false;
            changed = false;
        }
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::ESC => {
                        action = 1;
                        game.wait_event(ev::KEY_UP);
                    }
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 0x39;
                    }
                    scan::UP if sel > 0 => {
                        sel -= 1;
                        warp_pending = true;
                        changed = true;
                    }
                    scan::DOWN if sel < 4 => {
                        sel += 1;
                        warp_pending = true;
                        changed = true;
                    }
                    _ => {}
                },
                ev::MOUSE_MOVE | ev::MOUSE_DOWN => {
                    if e.kind == ev::MOUSE_MOVE {
                        cursor_dirty = true;
                    }
                    if e.mods & ev::M_LBUTTON == 0 {
                        if e.mods & ev::M_RBUTTON != 0 {
                            game.wait_event(ev::MOUSE_UP);
                            action = 1;
                        }
                    } else {
                        game.wait_event(ev::MOUSE_UP);
                        let (x, y) = (e.x, e.y);
                        if !(0xf8..=0x1b5).contains(&x) || !(0x109..=0x194).contains(&y) {
                            if x > 0x10f && y > 0x19a && x < 0x19d && y < 0x1b7 {
                                action = 0x39;
                            }
                        } else {
                            let row = (y - 0x109) / 0x1c;
                            if row != sel {
                                changed = true;
                                sel = row;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if action == 0x39 {
            done = true;
            confirmed = true;
            game.campaign.difficulty = sel as u8 + 1;
        } else if action == 1 {
            done = true;
            confirmed = false;
        }
    }
}
