//! The spell list of the unit status screen (tabs 魔法 / 招式): `FUN_0045fd41`,
//! i.e. the battle spell menu `FUN_00464aee` in browse mode (caster object
//! +0x38 = 1) with `FUN_00464018`, and the spell help window `FUN_00464727`
//! (with `FUN_0045f57c`). Battle state is read through BATTLE's public API.

use crate::battle::actions::{sprintf, Arg};
use crate::battle::UnitRef;
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// Menu origin (`DAT_004a2568`, `DAT_004a256c`).
const MX: i32 = 0;
const MY: i32 = 0x17c;

/// UI slot of image `k` of the spell menu set (`FUN_0045ef0b`: CEL 2 from entry 0x61).
fn img(k: usize) -> usize {
    142 + 0x61 + k
}

/// 0-based spell index of entry `col` of `lists[row]`.
fn spell_index(game: &Game, lists: &[(usize, Vec<u16>)], row: usize, col: usize) -> Option<usize> {
    let (school, spells) = lists.get(row)?;
    let first = game.battle.t().spells.school_spells.get(school - 1)?.0 as usize;
    Some(first + *spells.get(col)? as usize - 1)
}

/// Port of `FUN_00464018(this, row, col)`: panel, school names (current in
/// 0x3c), the row's spell icons (selected one highlighted), name, cost and class.
fn draw_menu(game: &mut Game, r: UnitRef, lists: &[(usize, Vec<u16>)], row: usize, col: usize) {
    game.scr.set_page(0);
    if let Ok(p) = game.res().ui_image(img(1)) {
        gfx::blit(game.scr.page(), &p, MX, MY, true);
    }
    let (mut x, mut y) = (MX + 0x1fc, MY + 0xe);
    for (k, &(school, _)) in lists.iter().enumerate() {
        let name = game.res().school_names.get(school - 1).cloned().unwrap_or_default();
        let fg = if k == row { 0x3c } else { 0x69 };
        text::small(game, x - text::strlen(&name) * 4, y, &name, fg, 0, 1, 2);
        if k % 3 == 2 {
            x = MX + 0x1fc;
            y += 0x1c;
        } else {
            x += 0x2d;
        }
    }
    let t = game.battle.t();
    let (school, spells) = &lists[row];
    let first = t.spells.school_spells.get(school - 1).map_or(0, |s| s.0 as usize);
    let mut ix = MX + 0x1a;
    for (k, &i) in spells.iter().enumerate() {
        let mut k_img = i as usize * 2 + 0x16 + first * 2;
        if k == col {
            k_img += 1;
        }
        if let Ok(p) = game.res().ui_image(img(k_img)) {
            gfx::blit(game.scr.page(), &p, ix, MY + 0xe, true);
        }
        ix += 0x22;
    }
    let label = game.msg(0xe2);
    text::large(game, MX + 0x4e - text::strlen(&label) * 6, MY + 0x3b, &label, 0x29, 0, 1, 2);
    let Some(idx) = spell_index(game, lists, row, col) else { return };
    let sp = t.spells.spells[idx].clone();
    let name = game.res().spell_names.get(idx).cloned().unwrap_or_default();
    text::large(game, MX + 0x94, MY + 0x3a, &name, 0xa8, 0, 1, 2);
    let u = game.battle.unit(r).clone();
    let n = (sp.stat_flags & 1 != 0) as i32 + (sp.stat_flags & 2 != 0) as i32 + (sp.stat_flags & 4 != 0) as i32;
    let mut cx = MX + 0xf0;
    if n != 0 {
        if n < 3 {
            let cost = if sp.stat_flags & 1 != 0 && u.abilities & 0x20 != 0 { sp.cost / 2 } else { sp.cost } as i32;
            let s = sprintf(b"%d /", &[Arg::D(cost)]);
            cx = MX + 0xfc;
            text::small(game, cx, MY + 0x3e, &s, 0x2b, 0, 1, 2);
            cx += s.len() as i32 * 8;
        }
        for (bit, label, have) in [(1u8, &b" MP:%d"[..], u.mp), (2, &b" ST:%d"[..], u.st), (4, &b" HP:%d"[..], u.hp)] {
            if sp.stat_flags & bit != 0 {
                let s = sprintf(label, &[Arg::D(have as i32)]);
                let fg = if (have as i32) < sp.cost as i32 { 0x3c } else { 0x2b };
                text::small(game, cx, MY + 0x3e, &s, fg, 0, 1, 2);
                cx += s.len() as i32 * 8;
            }
        }
    }
    let ex = if cx < MX + 0x19a { MX + 0x1a0 } else { MX + 0x1a4 };
    let m = game.msg(0xf + effect_class(sp.effect));
    text::large(game, ex, MY + 0x3b, &m, 0xc4, 0x36, 1, 1);
}

/// Effect class of a spell: 0 attack, 1 (0x0e/0x12/0x5a), 2 other (`DAT_004a1f2c`).
fn effect_class(effect: u8) -> usize {
    match effect {
        1 => 0,
        0x0e | 0x12 | 0x5a => 1,
        _ => 2,
    }
}

/// Port of `FUN_00464727(this, row, col)`: the spell help window (labels,
/// name, class, target, area, cost, description). Browsing darkens the
/// whole page (tint 8); from the battle menu the window gets a shadow.
pub fn spell_help(game: &mut Game, spell: usize, browse: bool) {
    cursor::erase(game);
    let Ok(panel) = game.res().ui_image(img(0)) else { return };
    if browse {
        let lut = game.res().tints[8];
        gfx::remap_page(game.scr.page(), &lut);
    } else {
        let lut = game.res().tints[6];
        gfx::window_shadow(game.scr.page(), &lut, 0x104, 0x24, panel.width as i32, panel.height as i32);
    }
    gfx::blit(game.scr.page(), &panel, 0x104, 0x24, false);
    let labels = game.msg(0x46);
    text::block(game, 0x121, 0x42, &labels, 0x6c, 0x36, 1, 1, 6, 6, 0x1c);
    let sp = game.battle.t().spells.spells[spell].clone();
    let name = game.res().spell_names.get(spell).cloned().unwrap_or_default();
    text::large(game, 0x16d, 0x42, &name, 0xb2, 0x36, 1, 1);
    let m = game.msg(0xf + effect_class(sp.effect));
    text::large(game, 0x16d, 0x5e, &m, 0xb2, 0x36, 1, 1);
    let m = game.msg(0x12 + sp.target as usize);
    text::large(game, 0x16d, 0x7a, &m, 0xb2, 0x36, 1, 1);
    let area = match sp.shape {
        0 => b"----".to_vec(),
        1 => b"1".to_vec(),
        s => format!("TYPE {:X}", s as u32 + 8).into_bytes(),
    };
    text::large(game, 0x16d, 0x96, &area, 0xb2, 0x36, 1, 1);
    let mut cost = (sp.cost as i32).to_string().into_bytes();
    for (bit, s) in [(1u8, &b"/MP"[..]), (2, b"/ST"), (4, b"/HP")] {
        if sp.stat_flags & bit != 0 {
            cost.extend_from_slice(s);
        }
    }
    text::large(game, 0x16d, 0xb2, &cost, 0xb2, 0x36, 1, 1);
    let help = game.res().spell_help.get(spell).cloned().unwrap_or_default();
    text::block(game, 0x16d, 0xce, &help, 0xb2, 0x36, 1, 1, 5, 8, 0x1c);
    gfx::present(game);
    crate::ui::click_to_continue(game, 0);
}

/// Port of `FUN_00451417(unit, technique)` → `FUN_0045fd41`: browse the
/// unit's magic (false) or techniques (true). Page 1 is kept as it was.
pub fn browse(game: &mut Game, r: UnitRef, technique: bool) {
    let saved = game.scr.pages[1].clone();
    let lists = game.battle.spell_lists(r, technique);
    if !lists.is_empty() {
        menu(game, r, &lists);
    }
    game.scr.pages[1] = saved;
}

/// Port of `FUN_00464aee(this, 1)` with +0x38 = 1: picking a spell opens
/// its help instead of casting. Row and column are the battle menu's
/// remembered position (`DAT_004a2580/84`, reset here).
fn menu(game: &mut Game, r: UnitRef, lists: &[(usize, Vec<u16>)]) {
    let n_rows = lists.len();
    let cols = |row: usize| lists[row].1.len();
    let (mut row, mut col) = (0usize, 0usize);
    game.scr.copy_page(2, 0);
    let (mut warp, mut full, mut redraw, mut moved) = (true, true, true, false);
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if warp {
            cursor::warp(game, MX + 0x2e + col as i32 * 0x22, MY + 0x22);
            warp = false;
        }
        if full {
            game.scr.copy_page(0, 2);
            redraw = true;
        }
        if redraw {
            if full {
                full = false;
            } else {
                cursor::erase(game);
            }
            draw_menu(game, r, lists, row, col);
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        let mut code = 0i32;
        let mut by_key = false;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::LEFT => {
                        if e.mods & ev::M_CTRL != 0 {
                            by_key = n_rows > 1;
                            code = by_key as i32;
                        } else if cols(row) > 1 {
                            code = 5;
                        }
                    }
                    scan::RIGHT => {
                        if e.mods & ev::M_CTRL != 0 {
                            by_key = n_rows > 1;
                            if by_key {
                                code = 2;
                            }
                        } else if cols(row) > 1 {
                            code = 6;
                        }
                    }
                    scan::ESC => {
                        game.wait_event(ev::KEY_UP);
                        code = -1;
                    }
                    scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        code = 4;
                    }
                    0x3b => {
                        game.wait_event(ev::KEY_UP);
                        code = 3;
                    }
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (x, y) = (e.x, e.y);
                    let icon = if !(MX + 0x1a..=MX + 0x1fa).contains(&x) || !(MY + 0xe..=MY + 0x2e).contains(&y) {
                        None
                    } else {
                        let c = ((x - MX - 0x1a) / 0x22) as usize;
                        if c != col && c < cols(row) && e.kind == ev::MOUSE_MOVE {
                            redraw = true;
                            col = c;
                        }
                        Some(c)
                    };
                    if e.mods & ev::M_LBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        if (MX + 0x1ea..MX + 0x271).contains(&x) && (MY + 0xb..MY + 0x5f).contains(&y) {
                            let k = ((x - MX - 0x1ea) / 0x2d + ((y - MY - 0xb) / 0x1c) * 3) as usize;
                            if k < n_rows && k != row {
                                col = 0;
                                redraw = true;
                                row = k;
                            }
                        }
                        if (MX + 0x14..MX + 0x88).contains(&x) && (MY + 0x37..MY + 0x58).contains(&y) {
                            code = 3;
                        } else if icon.is_some_and(|c| c < cols(row)) {
                            code = 4;
                        }
                    } else if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        code = -1;
                    }
                }
                _ => {}
            }
        }
        match code {
            1 | 2 => {
                row = if code == 1 { (row + n_rows - 1) % n_rows } else { (row + 1) % n_rows };
                col = 0;
                redraw = true;
                warp |= by_key;
                crate::ui::sound(game, 100);
            }
            3 | 4 => {
                if code == 3 {
                    crate::ui::sound(game, 100);
                }
                if let Some(idx) = spell_index(game, lists, row, col) {
                    spell_help(game, idx, true);
                }
                warp = true;
                full = true;
            }
            5 | 6 => {
                let n = cols(row);
                col = if code == 5 { (col + n - 1) % n } else { (col + 1) % n };
                warp = true;
                redraw = true;
                crate::ui::sound(game, 100);
            }
            -1 => break,
            _ => {}
        }
    }
    game.battle.spell_menu_pos = (row as i32, col as i32);
    game.flush_events(0x1f);
}
