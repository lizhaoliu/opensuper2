//! The two-player setup (雙人對戰), port of `FUN_0040e5db` with
//! `FUN_0040f08e` (bevel boxes) and `FUN_0040f1aa` / `FUN_0040f3b1` (the
//! battlefield list). The battle itself is the AI port's `battle::vs`.

use anyhow::Result;

use crate::battle::vs;
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// Port of `FUN_0040f08e(x, y, w, h)`: a sunken field.
fn bevel(game: &mut Game, x: i32, y: i32, w: i32, h: i32) {
    let p = game.scr.page();
    gfx::rect(p, x, y, x - 1 + w, y - 1 + h, 0xf0, true);
    gfx::hline(p, y + 1, x + 1, x - 2 + w, 0xfe);
    gfx::vline(p, x + 1, y + 2, y - 2 + h, 0xfe);
    gfx::hline(p, y - 1 + h, x + 1, x - 1 + w, 0xed);
    gfx::vline(p, x - 1 + w, y + 1, y - 2 + h, 0xed);
    gfx::hline(p, y + h, x, x + w, 0x7d);
    gfx::vline(p, x + w, y, y - 1 + h, 0x7d);
}

/// `"%2d.%s"`.
fn numbered(k: usize, name: &[u8]) -> Vec<u8> {
    let mut s = format!("{k:2}.").into_bytes();
    s.extend_from_slice(name);
    s
}

/// The army field: `sscanf(desc + 5, "%s")`, the first word of the save
/// description after its 5-byte prefix.
fn army_label(game: &Game, slot: usize) -> Vec<u8> {
    let d = game.ui.save_desc.get(slot - 1).cloned().unwrap_or_default();
    let rest = d.get(5..).unwrap_or(&[]);
    let start = rest.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(rest.len());
    let rest = &rest[start..];
    let end = rest.iter().position(|c| c.is_ascii_whitespace()).unwrap_or(rest.len());
    rest[..end].to_vec()
}

/// Port of `FUN_0040e5db`: battlefield and the two armies' saves. Returns
/// 1 when a battle was set up (`battle::vs::start`), 0 when cancelled.
pub fn setup(game: &mut Game) -> Result<i32> {
    let names = vs::stage_list(game.data)?;
    let n = names.len();
    for (msg, x, y) in [(0x10eusize, 0x90, 0x8c), (0x10f, 0x78, 0xd2), (0x110, 0x78, 0x118)] {
        let m = game.msg(msg);
        text::large(game, x, y, &m, 0x36, 0, 1, 2);
    }
    bevel(game, 0xec, 0x8c, 0x96, 0x18);
    bevel(game, 0xec, 0xd2, 0x96, 0x18);
    bevel(game, 0xec, 0x118, 0x96, 0x18);
    bevel(game, 0xbe, 0x1a8, 0x52, 0x1e);
    bevel(game, 0x174, 0x1a8, 0x52, 0x1e);
    let ok = game.msg(0xce);
    text::large(game, 0xd0, 0x1ac, &ok, 0xf9, 0, 1, 2);
    let cancel = game.msg(0x111);
    text::large(game, 0x186, 0x1ac, &cancel, 0x7d, 0, 1, 2);
    game.scr.copy_page(2, 0);
    let none = game.msg(0x1e);
    let (mut stage, mut blue, mut red, mut hover) = (1usize, 0usize, 0usize, 0usize);
    let (mut rebuild, mut redraw, mut moved, mut first) = (true, true, false, true);
    let (mut open, mut ok_on, mut start) = (false, false, false);
    loop {
        super::music_keepalive(game);
        if rebuild {
            rebuild = false;
            redraw = true;
            if !ok_on && blue > 0 && red > 0 {
                game.scr.set_page(2);
                text::large(game, 0xd0, 0x1ac, &ok, 0x7d, 0, 1, 2);
                ok_on = true;
            }
            game.scr.copy_page(1, 2);
            game.scr.set_page(1);
            let s = numbered(stage, names.get(stage - 1).map_or(&[][..], |v| v));
            text::small(game, 0xf0, 0x90, &s, 0x7d, 0, 1, 2);
            let b = if blue == 0 { none.clone() } else { army_label(game, blue) };
            text::small(game, 0xf0, 0xd6, &b, 0x7d, 0, 1, 2);
            let r = if red == 0 { none.clone() } else { army_label(game, red) };
            text::small(game, 0xf0, 0x11c, &r, 0x7d, 0, 1, 2);
            if open {
                bevel(game, 0x182, 0x8c, 0x8c, n as i32 * 0x18);
                for (k, name) in names.iter().enumerate() {
                    let s = numbered(k + 1, name);
                    text::small(game, 0x186, k as i32 * 0x18 + 0x90, &s, 0x7d, 0, 1, 2);
                }
            }
            game.scr.set_page(0);
        }
        if redraw {
            game.scr.copy_page(0, 1);
            if open && hover != 0 {
                let y = (hover as i32 - 1) * 0x18;
                gfx::rect(game.scr.page(), 0x184, y + 0x8e, 0x20c, hover as i32 * 0x18 + 0x8c, 0xa9, true);
                let s = numbered(hover, names.get(hover - 1).map_or(&[][..], |v| v));
                text::small(game, 0x186, y + 0x90, &s, 0x21, 0, 1, 2);
            }
            if first {
                cursor::capture(game);
            }
            cursor::draw_present(game, None, false);
            if first {
                first = false;
                let m = game.res().master.clone();
                gfx::fade(game, &m, 0, -4);
            }
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        let mut leave = false;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => {
                    if e.scan() == scan::ESC {
                        if open {
                            open = false;
                            rebuild = true;
                        } else {
                            leave = true;
                        }
                        game.wait_event(ev::KEY_UP);
                    }
                }
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (x, y) = (e.x, e.y);
                    let list_bottom = n as i32 * 0x18 + 0x8c;
                    if open {
                        let in_field = 0xec <= x && 0x8c <= y && x <= 0x181 && y <= 0xa3;
                        let in_list = 0x182 <= x && 0x8c <= y && x <= 0x20d && y < list_bottom;
                        if !in_field && !in_list {
                            open = false;
                            rebuild = true;
                        } else if e.mods & ev::M_RBUTTON != 0 {
                            game.wait_event(ev::MOUSE_UP);
                            open = false;
                            rebuild = true;
                        } else if 0x181 < x && 0x8b < y && x < 0x20e && y < list_bottom {
                            let k = ((y - 0x8c) / 0x18 + 1) as usize;
                            if k != hover {
                                redraw = true;
                                hover = k;
                            }
                            if e.mods & ev::M_LBUTTON != 0 {
                                game.wait_event(ev::MOUSE_UP);
                                open = false;
                                rebuild = true;
                                stage = k;
                            }
                        }
                    } else if 0xec <= x && 0x8c <= y && x <= 0x177 && y <= 0xa3 {
                        open = true;
                        rebuild = true;
                        hover = 0;
                    } else if e.mods & ev::M_LBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        if (0xec..=0x181).contains(&x) && (0xd2..=0xe9).contains(&y) {
                            let s = super::save::pick_army_save(game, 0)?;
                            if s != 0 {
                                blue = s;
                            }
                            rebuild = true;
                        } else if (0xec..=0x181).contains(&x) && (0x118..=0x12f).contains(&y) {
                            let s = super::save::pick_army_save(game, 1)?;
                            if s != 0 {
                                red = s;
                            }
                            rebuild = true;
                        } else if (0xbe..=0x10f).contains(&x) && (0x1a8..=0x1c5).contains(&y) && ok_on {
                            start = true;
                            leave = true;
                        } else if 0x173 < x && 0x1a7 < y && x < 0x1c6 && y < 0x1c6 {
                            leave = true;
                        }
                    } else if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        leave = true;
                    }
                }
                _ => {}
            }
        }
        if leave {
            break;
        }
    }
    if start {
        game.campaign.vs_mode = true;
        super::stop_music(game);
        vs::start(game, stage, blue, red)?;
        return Ok(1);
    }
    Ok(0)
}
