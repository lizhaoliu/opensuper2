//! The title screen, port of `FUN_0040d7e1`.

use anyhow::Result;

use super::code;
use crate::cursor::{self, SHAPE_ALT, SHAPE_POINTER};
use crate::game::Game;
use crate::gfx;
use crate::res::load_picture;
use crate::sys::{ev, scan};
use crate::text;
use crate::ui;

/// Menu items: 重新開始, 載入進度, 雙人對戰, 系統設定, 離開遊戲.
const ITEMS: [usize; 5] = [0xd4, 0xd5, 0xd6, 0x7b, 0x83];
/// `DAT_0048e194` "V%s" with `DAT_0048e5fc` "1.4".
const VERSION: &[u8] = b"V1.4";

/// Port of `FUN_0040d7e1`: returns the top-level state code.
pub fn title_menu(game: &mut Game) -> Result<i32> {
    let (title, pal) = load_picture(&game.data.arcs, -0x13, 3)?;
    let (dimmed, _) = load_picture(&game.data.arcs, -0x14, 3)?;
    super::stage::reset(game);
    let mut sel: i32 = 0;
    let mut action = 0;
    let mut first_capture = true;
    let mut warp_pending = true;
    let mut fade_in_pending = true;
    let mut full_redraw = true;
    let mut exit = false;
    let mut cursor_dirty = false;
    let mut changed = false;
    let mut result = 0;
    cursor::set_shape(game, SHAPE_ALT);
    super::music(game, 0x19);
    loop {
        if exit {
            super::stop_music(game);
            cursor::set_shape(game, SHAPE_POINTER);
            if result == code::NEW_STAGE && !game.campaign.vs_mode {
                super::newgame::newgame_start(game)?;
            }
            game.flush_events(0x1f);
            return Ok(result);
        }
        super::music_keepalive(game);
        if full_redraw {
            game.scr.set_page(2);
            gfx::blit(game.scr.page(), &title, 0, 0, true);
            text::small(game, 0xfa, 0x1cf, VERSION, 7, 0, 1, 2);
            for (i, &m) in ITEMS.iter().enumerate() {
                let s = game.msg(m);
                text::large(game, 0x116, i as i32 * 0x1e + 0x11e, &s, 0xf, 3, 1, 1);
            }
            game.scr.set_page(0);
            changed = true;
            full_redraw = false;
        }
        if warp_pending {
            cursor::warp(game, 0x140, sel * 0x1e + 0x12a);
            warp_pending = false;
            cursor_dirty = true;
        }
        if !changed {
            if cursor_dirty {
                cursor::draw_present(game, None, true);
                cursor_dirty = false;
            }
        } else {
            game.scr.copy_page(0, 2);
            let s = game.msg(ITEMS[sel as usize]);
            text::large(game, 0x116, sel * 0x1e + 0x11e, &s, 2, 3, 1, 1);
            if first_capture {
                cursor::capture(game);
                first_capture = false;
            }
            cursor::draw_present(game, None, false);
            cursor_dirty = false;
            changed = false;
        }
        if fade_in_pending {
            gfx::fade(game, &pal, 0, -4);
            fade_in_pending = false;
        }
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
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
                    if e.x > 0x109 && e.x < 0x183 && e.y > 0x11a && e.y < 0x1b1 {
                        let row = (e.y - 0x11b) / 0x1e;
                        if row == sel {
                            if e.mods & ev::M_LBUTTON != 0 {
                                game.wait_event(ev::MOUSE_UP);
                                action = 0x39;
                            }
                        } else {
                            changed = true;
                            sel = row;
                        }
                    }
                }
                _ => {}
            }
        }
        if action == 0x39 {
            gfx::blit(game.scr.page(), &dimmed, 0, 0, true);
            let any_save = super::init::scan_saves(game);
            if any_save || sel != 1 {
                gfx::fade(game, &pal, 1, -4);
                cursor::set_shape(game, SHAPE_POINTER);
                cursor::capture(game);
            }
            match sel {
                0 => {
                    if super::difficulty::difficulty_menu(game)? {
                        result = code::NEW_STAGE;
                        crate::event::clear_all_flags(game);
                        super::newgame::reset_campaign_counters(game);
                        exit = true;
                        game.campaign.vs_mode = false;
                    } else {
                        fade_in_pending = true;
                    }
                }
                1 => {
                    let r = super::save::load_game_menu(game, true)?;
                    if r == code::LOADED || r == code::LOADED_SLOT50 {
                        result = r;
                        exit = true;
                    } else if super::init::scan_saves(game) {
                        // FUN_00430244: rescan; fade back in when saves exist.
                        fade_in_pending = true;
                    }
                }
                2 => {
                    if super::versus::setup(game)? == 1 {
                        result = code::NEW_STAGE;
                        // FUN_00423ad4: story flags 1..=80 cleared.
                        crate::event::clear_all_flags(game);
                        exit = true;
                    } else {
                        fade_in_pending = true;
                    }
                }
                3 => {
                    super::settings::open(game, 0)?;
                    fade_in_pending = true;
                }
                _ => {
                    let q = game.msg(0x33);
                    if ui::yes_no(game, 200, 0x193, true, &q, true, true) {
                        super::stop_music(game);
                        return Ok(code::QUIT);
                    }
                    fade_in_pending = true;
                }
            }
            if fade_in_pending {
                let master = game.res().master.clone();
                gfx::fade(game, &master, 1, -4);
            }
            cursor::set_shape(game, SHAPE_ALT);
            warp_pending = true;
            full_redraw = true;
            action = 0;
        }
    }
}
