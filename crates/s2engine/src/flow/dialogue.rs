//! Story dialogue: the map speech box (`FUN_004188ca`), its typewriter
//! (`FUN_00418e73`) and the choice menu (`FUN_0041929b`). These are the
//! EVENT CONTRACT functions (see `flow/mod.rs`).

use s2data::gfx_portrait::PortraitKind;
use s2data::script::Line;
use s2data::sprite::Image;

use super::stage;
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;
use crate::ui;

/// CEL 2 pieces of the speech box (`FUN_004188ca`).
const OPEN_0: usize = 0xe9;
const OPEN_1: usize = 0xea;
const BOX: usize = 0xeb;
const TAIL_ABOVE: usize = 0xed;
const TAIL_BELOW: usize = 0xee;
/// "More" arrow of a full page (`FUN_00418e73`).
const MORE: usize = 0xec;
/// Story flag 2 (`DAT_00494509`): keep the box open after the last line.
const FLAG_KEEP_OPEN: usize = 2;

fn portrait(game: &Game, speaker: i16) -> Option<Image> {
    if speaker <= 0 {
        return None;
    }
    game.res().portraits.get(speaker as usize, PortraitKind::Dialogue).ok()
}

/// Port of `FUN_004188ca(this, mode)` for the lines of one event (EVENT CONTRACT).
pub fn show_dialogue(game: &mut Game, lines: &[Line], mode: u32) {
    let pieces: Vec<Image> = [OPEN_0, OPEN_1, BOX, TAIL_ABOVE, TAIL_BELOW].iter().filter_map(|&s| game.res().ui_image(s).ok()).collect();
    if pieces.len() != 5 {
        return;
    }
    let box_h = pieces[2].height as i32;
    game.scr.set_page(0);
    for (i, line) in lines.iter().enumerate() {
        if !stage::in_an_army(game, line.speaker) {
            continue;
        }
        let mut found = true;
        let (mut ux, mut uy) = (0, 0);
        if mode == 0 {
            let (f, x, y) = stage::unit_screen_pos(game, line.speaker);
            if !f {
                stage::unit_screen_pos(game, 1);
                found = false;
            }
            (ux, uy) = (x, y);
        } else {
            found = false;
        }
        let (mut tail_x, mut tail_y) = (0, 0);
        let mut tail = &pieces[4];
        if !found {
            game.ui.box_x = 6;
            game.ui.box_y = 0x7d;
        } else if uy < 0x137 {
            game.ui.box_y = uy - 2;
            if 0x1df < game.ui.box_y + box_h {
                game.ui.box_y = 0x1df - box_h;
            }
            tail = &pieces[4];
            tail_y = game.ui.box_y - 0x20;
            tail_x = ux + 0x21;
        } else {
            game.ui.box_y = (uy - 100 - box_h).max(0x20);
            tail = &pieces[3];
            tail_y = game.ui.box_y + 0xd7;
            tail_x = ux + 0xc;
        }
        tail_x = tail_x.max(0);
        if 0x27f < tail_x + 0x30 {
            tail_x = 0x24f;
        }
        let (bx, by) = (game.ui.box_x, game.ui.box_y);
        game.scr.set_page(1);
        if mode == 0 {
            stage::draw_map(game);
            for open in &pieces[..2] {
                let (w, h) = (open.width as i32, open.height as i32);
                gfx::blit(game.scr.page(), open, 0x140 - w / 2, by + box_h / 2 - h / 2, false);
                gfx::present(game);
                if game.get_event(0xffff) && game.ev.kind & (ev::KEY_DOWN | ev::MOUSE_DOWN) == 0 {
                    game.sleep_cs(4);
                }
            }
        }
        gfx::blit(game.scr.page(), &pieces[2], bx, by, true);
        if found {
            gfx::blit(game.scr.page(), tail, tail_x, tail_y, false);
        }
        if let Some(face) = portrait(game, line.speaker) {
            gfx::blit(game.scr.page(), &face, bx + 0x4a - face.width as i32 / 2, by + 0x1d, false);
        }
        let name_x = bx + 0x4a - text::strlen(&line.name) * 6;
        text::large(game, name_x, by + 0xa8, &line.name, 0x36, 0, 1, 2);
        game.scr.set_page(0);
        if line.music_before != 0 {
            super::music(game, line.music_before as u16);
        }
        typewriter(game, &line.text);
        if line.music_after != 0 {
            super::music(game, line.music_after as u16);
        }
        let last = i + 1 == lines.len();
        if !(game.campaign.flags[FLAG_KEEP_OPEN] == 1 && last) && mode != 1 {
            for open in pieces[..2].iter().rev() {
                stage::draw_map(game);
                let (w, h) = (open.width as i32, open.height as i32);
                gfx::blit(game.scr.page(), open, 0x140 - w / 2, by + box_h / 2 - h / 2, false);
                gfx::present(game);
                if game.get_event(0xffff) && game.ev.kind & (ev::KEY_DOWN | ev::MOUSE_DOWN) == 0 {
                    game.sleep_cs(3);
                }
            }
            stage::draw_map(game);
            gfx::present(game);
        }
    }
}

fn hex2(b: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(b.get(..2)?).ok()?;
    u8::from_str_radix(s, 16).ok()
}

/// Port of `FUN_00418e73`: type the text into the box on page 0 (the box
/// itself is on page 1). `@@W..@@`, `@@D..@@` (cs per character), `@@RW@@`,
/// `@@RD@@` control blocks; `*` starts a new line; Enter / Space / Alt / a
/// click show the rest of the page at once.
fn typewriter(game: &mut Game, t: &[u8]) {
    let t = s2data::text::cstr(t);
    let (bx, by) = (game.ui.box_x, game.ui.box_y);
    let x0 = bx + 0x8f;
    let y0 = by + 0x1d;
    let more = game.res().ui_image(MORE).ok();
    let mut pen = x0;
    let mut fast = false;
    game.scr.copy_page(0, 1);
    gfx::present(game);
    text::set(game, 0x2a, 0, 1);
    let mut count = 0;
    let mut delay = game.cfg.text_speed;
    let mut t0 = game.ticks();
    let mut y = y0;
    let mut p = 0usize;
    loop {
        if !game.still_waiting(t0, delay) || fast {
            // Control block, recognised only where the next character would print.
            let mut q = p;
            if t[p..].starts_with(b"@@") {
                q = p + 2;
                loop {
                    if t[q.min(t.len())..].starts_with(b"@@") {
                        q += 2;
                        break;
                    }
                    match t.get(q) {
                        None => break,
                        Some(b'W') => {
                            if let (Some(c), Some(s)) = (t.get(q + 1..).and_then(hex2), t.get(q + 4..).and_then(hex2)) {
                                text::set(game, c, s, 1);
                            }
                            q += 6;
                        }
                        Some(b'D') => {
                            if let Some(d) = t.get(q + 1..).and_then(hex2) {
                                delay = d as i32;
                            }
                            q += 3;
                        }
                        Some(b'R') => match t.get(q + 1) {
                            Some(b'W') => {
                                text::set(game, 0x2a, 0, 1);
                                q += 2;
                            }
                            Some(b'D') => {
                                delay = game.cfg.text_speed;
                                q += 2;
                            }
                            _ => {
                                q += 1;
                                break;
                            }
                        },
                        Some(_) => break,
                    }
                }
            }
            p = q.min(t.len());
            let rest = text::put_char(game, &mut pen, y, &t[p..], &mut count, 2);
            p = t.len() - rest.len();
            if !fast {
                t0 = game.ticks();
                gfx::present(game);
            }
            // A '\n' would make the original spin forever; treat it as the end.
            if p >= t.len() || t[p] == 0 || t[p] == b'\n' {
                if fast {
                    gfx::present(game);
                }
                ui::click_to_continue(game, 4000);
                return;
            }
            if y < by + 0xb2 || pen < bx + 0x23f {
                if bx + 0x23f <= pen || (count == -1 && x0 < pen) {
                    y += 0x19;
                    count = 0;
                    pen = x0;
                }
            } else {
                if let Some(m) = &more {
                    gfx::blit(game.scr.page(), m, bx + 0x244, by + 0xb7, false);
                }
                gfx::present(game);
                ui::click_to_continue(game, 4000);
                game.scr.copy_page(0, 1);
                count = 0;
                fast = false;
                y = y0;
                pen = x0;
            }
            if fast {
                continue;
            }
        }
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        fast = true;
                    }
                    scan::ALT => fast = true,
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.mods & ev::M_RBUTTON != 0 {
                        fast = true;
                    } else if e.mods & ev::M_LBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        fast = true;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Port of `FUN_0041929b` (EVENT CONTRACT): prompt from `lines[0]`, options
/// `lines[1..]`. Returns the 1-based choice (the caller stores it in flag 12).
pub fn choose(game: &mut Game, lines: &[Line], option_count: usize) -> u8 {
    let Some(prompt) = lines.first() else { return 0 };
    game.ui.box_x = 6;
    game.ui.box_y = 0x7d;
    let tx = 0x95;
    stage::draw_map(game);
    let (f, _, _) = stage::unit_screen_pos(game, prompt.speaker);
    if !f {
        stage::unit_screen_pos(game, 1);
    }
    let (bx, by) = (game.ui.box_x, game.ui.box_y);
    if let Ok(b) = game.res().ui_image(BOX) {
        gfx::blit(game.scr.page(), &b, bx, by, true);
    }
    if let Some(face) = portrait(game, prompt.speaker) {
        gfx::blit(game.scr.page(), &face, bx + 0x4a - face.width as i32 / 2, by + 0x1d, false);
    }
    text::large(game, bx + 0x4a - text::strlen(&prompt.name) * 6, by + 0xa8, &prompt.name, 0x36, 0, 1, 2);
    if option_count != lines.len().saturating_sub(1) {
        // DAT_0048e2f0, a leftover debug marker.
        text::large(game, bx + 0x4a, by + 0xbe, b"Shan", 0x36, 0, 1, 2);
    }
    text::block(game, tx, 0x96, &prompt.text, 0x2a, 0, 1, 2, 2, 0x10, 0x1c);
    let mut y = 0xd1;
    for l in &lines[1..] {
        text::large(game, tx, y, &l.text, 0x2a, 0, 1, 2);
        y += 0x1a;
    }
    let cur = game.scr.cur;
    game.scr.copy_page(1, cur);
    let mut done = false;
    let mut redraw = true;
    let mut sel: usize = 0;
    let mut moved = false;
    let mut choice = 0u8;
    cursor::warp(game, tx + 0x37, 0xd8);
    let ask = game.msg(6);
    loop {
        if done {
            return choice;
        }
        if !redraw {
            if moved {
                cursor::draw_present(game, None, true);
                moved = false;
            }
        } else {
            game.scr.copy_page(cur, 1);
            if let Some(l) = lines.get(sel + 1) {
                text::large(game, tx, sel as i32 * 0x1a + 0xd1, &l.text, 0x3c, 0, 1, 2);
            }
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
        }
        if !(game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind)) {
            continue;
        }
        let e = game.ev;
        let mut confirm = false;
        match e.kind {
            ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                scan::ENTER | scan::SPACE => {
                    game.wait_event(ev::KEY_UP);
                    confirm = true;
                }
                scan::UP if sel > 0 => {
                    sel -= 1;
                    redraw = true;
                }
                scan::DOWN if sel + 1 < option_count => {
                    sel += 1;
                    redraw = true;
                }
                _ => {}
            },
            ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                if e.kind == ev::MOUSE_MOVE {
                    moved = true;
                }
                if tx <= e.x && 0xd0 < e.y && e.y < option_count as i32 * 0x1a + 0xd1 {
                    let row = ((e.y - 0xd1) / 0x1a) as usize;
                    if row == sel {
                        if e.mods & ev::M_LBUTTON != 0 {
                            game.wait_event(ev::MOUSE_UP);
                            confirm = true;
                        }
                    } else {
                        redraw = true;
                        sel = row;
                    }
                }
            }
            _ => {}
        }
        if confirm {
            if ui::yes_no(game, tx, sel as i32 * 0x1a + 0xec, true, &ask, false, false) {
                done = true;
                // The original stores this in DAT_00494513 (flag 12) here;
                // the event engine stores the returned value instead.
                choice = sel as u8 + 1;
            }
            redraw = true;
        }
    }
}
