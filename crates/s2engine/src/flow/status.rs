//! The party status list (`FUN_0042f620`) and the unit status screen
//! (`FUN_0044feb2` with `FUN_004503f2`, `FUN_00450de0`, `FUN_00450ffe`).

use anyhow::Result;
use s2data::sprite::Image;

use crate::battle::unit::flag;
use crate::battle::{view, Side, UnitRef};
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// Small scroll thumb (sysimg 147, `DAT_00493a94`).
const THUMB: usize = 147;

/// `sprintf(buf, fmt, ...)` for the panels' `%s %d%s` / `%d+%d` lines.
fn class_line(game: &Game, r: UnitRef) -> Vec<u8> {
    let u = game.battle.unit(r);
    let mut s = game.res().class_name(u.kind as usize).to_vec();
    s.push(b' ');
    s.extend_from_slice(u.level.to_string().as_bytes());
    s.extend_from_slice(&game.msg(0x71));
    s
}

fn is_enemy(game: &Game, r: UnitRef) -> bool {
    game.battle.army(r.side).side != Side::Player
}

/// Port of `FUN_004517b5`: HP/MP/ST/AP with values and the missing part of each bar.
pub(crate) fn stat_bars(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    let u = game.battle.unit(r).clone();
    for (k, l) in [b"HP", b"MP", b"ST", b"AP"].iter().enumerate() {
        let ry = y + 6 + 0x11 * k as i32;
        text::small(game, x + 0xd, ry, *l, 0xbb, 0, 1, 2);
        text::small(game, x + 0xd8, ry, b"/", 0x4f, 0, 1, 2);
    }
    let cur = [u.hp, u.mp, u.st, u.ap];
    let max = [u.hp_max, u.mp_max, u.st_max, u.ap_max];
    for (k, &c) in cur.iter().enumerate() {
        text::number(game, x + 0xb8, y + 6 + 0x11 * k as i32, c as i32, 0x4f, 0, 1, 2, 0);
    }
    for (k, &m) in max.iter().take(3).enumerate() {
        text::number(game, x + 0xe0, y + 6 + 0x11 * k as i32, m as i32, 0x4f, 0, 1, 2, 0);
    }
    if u.ap_carry < 1 {
        text::number(game, x + 0xe0, y + 0x39, u.ap_max as i32, 0x4f, 0, 1, 2, 0);
    } else {
        let s = format!("{}+{}", u.ap_max, u.ap_carry);
        text::small(game, x + 0xe0, y + 0x39, s.as_bytes(), 0x4f, 0, 1, 2);
    }
    for k in 0..4 {
        let (c, m) = (cur[k] as i32, max[k] as i32);
        if c < m || c == 0 {
            let miss = if c == 0 || m == 0 { 0x6b } else { (m - c) * 0x6b / m };
            let by = y + 0xb + 0x11 * k as i32;
            let page = game.scr.page();
            gfx::hline(page, by, x + 0x9f - miss, x + 0x9f, 0xdd);
            gfx::rect(page, x + 0x9f - miss, by + 1, x + 0x9f, by + 2, 0x2b, true);
            gfx::rect(page, x + 0x9f - miss, by + 3, x + 0x9f, by + 5, 0x2e, true);
        }
    }
}

/// Port of `FUN_00451efd`: 攻擊/防禦/反應 and 魔力/戰技/劍術.
pub(crate) fn combat_stats(game: &mut Game, r: UnitRef, x: i32, y: i32, color: u8) {
    let u = game.battle.unit(r).clone();
    let rows = [(7usize, u.atk_cur, 11usize, u.mag_cur), (8, u.def_cur, 9, u.skill_cur), (10, u.react, 12, u.sword)];
    for (k, &(l1, v1, l2, v2)) in rows.iter().enumerate() {
        let ry = y + 0x16 * k as i32;
        let (m1, m2) = (game.msg(l1), game.msg(l2));
        text::small(game, x, ry, &m1, 0x36, 0, 1, 2);
        text::small(game, x + 0x41, ry, &m2, 0x36, 0, 1, 2);
        let ny = [y + 1, y + 0x16, y + 0x2b][k];
        text::number(game, x + 0x26, ny, v1 as i32, color, 0, 1, 2, 0);
        text::number(game, x + 0x68, ny, v2 as i32, color, 0, 1, 2, 0);
    }
}

/// `FUN_00451772`: both stat blocks.
fn stats(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    stat_bars(game, r, x, y);
    combat_stats(game, r, x + 0x10b, y + 9, 0xca);
}

/// `FUN_00452898` / `FUN_0045292a`: portrait at (x, y), sprite standing on (x2, y2).
fn portrait_and_sprite(game: &mut Game, r: UnitRef, x: i32, y: i32, x2: i32, y2: i32) {
    if let Some(face) = super::party::portrait(game, r) {
        gfx::blit(game.scr.page(), &face, x, y, false);
    }
    super::party::draw_unit(game, r, x2, y2);
}

/// Port of `FUN_004514a8(army, unit, x, y)`: one row of the status list.
fn status_row(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    portrait_and_sprite(game, r, x + 7, y + 4, x + 0x230, y + 0x76);
    let u = game.battle.unit(r).clone();
    if u.flags & flag::SUMMON == 0 {
        let name = s2data::text::cstr(&u.name).to_vec();
        let c = if is_enemy(game, r) { 0x3c } else { 0xa7 };
        text::large(game, x + 0x74, y + 7, &name, c, 0, 1, 2);
    }
    let line = class_line(game, r);
    text::large(game, x + 0x1f6 - line.len() as i32 * 0xc, y + 7, &line, 0xb3, 0, 1, 2);
    stats(game, r, x + 0x6e, y + 0x21);
}

/// Port of `FUN_0042f620(town)`: four members per page, details on a
/// portrait click, and (in battle, with the deployed enemies listed too) a
/// jump of the map cursor to the clicked unit. Returns 1 after a jump.
pub fn status_list(game: &mut Game, town: bool) -> Result<i32> {
    // FUN_0043af1c / FUN_0043afc2: town = the usable party; battle = everyone deployed.
    let pick = |game: &Game, side: Side, town: bool| -> Vec<UnitRef> {
        super::stage::units_of(game, side)
            .into_iter()
            .filter(|&r| {
                let u = game.battle.unit(r);
                u.flags & flag::DEAD == 0
                    && if town {
                        u.flags & (flag::RESERVE | flag::SUMMON) == 0 && u.npc != 1
                    } else {
                        u.flags & flag::DEPLOYED != 0
                    }
            })
            .collect()
    };
    let mut list = pick(game, Side::Player, town);
    if !town {
        list.extend(pick(game, Side::Enemy, false));
    }
    let n = list.len();
    if n == 0 {
        return Ok(0);
    }
    let bg = game.res().ui_image(0xcd)?;
    let row_img = game.res().ui_image(0xce)?;
    let saved = (!town).then(|| game.scr.pages[2].clone());
    let thumb = game.res().ui_image(THUMB).ok();
    let (mut top, mut sel) = (0usize, 0usize);
    let (mut full, mut redraw, mut moved, mut first) = (true, false, false, true);
    let mut result = 0;
    let mut last = (0, 0);
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if full {
            game.scr.set_page(2);
            gfx::blit(game.scr.page(), &bg, 0, 0, true);
            for k in 0..4usize {
                let Some(&r) = list.get(top + k) else { break };
                gfx::blit(game.scr.page(), &row_img, 5, k as i32 * 0x76 + 5, false);
                status_row(game, r, 5, k as i32 * 0x76 + 5);
            }
            game.scr.set_page(0);
            redraw = true;
            full = false;
        }
        if redraw {
            game.scr.copy_page(0, 2);
            let s = sel as i32 * 0x76;
            gfx::rect(game.scr.page(), 0xc, s + 9, 0x68, s + 0x76, 0x3c, false);
            let mut ty = 0x1f;
            if top + sel > 0 {
                ty += ((top + 1 + sel) as i32 * 0x187) / n as i32;
            }
            if let Some(t) = &thumb {
                gfx::blit(game.scr.page(), t, 0x268, ty, false);
            }
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
            if first {
                game.flush_events(0x1f);
                first = false;
                if town {
                    let m = game.res().master.clone();
                    gfx::fade(game, &m, 0, -4);
                }
            }
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        let mut action = 0;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => {
                    action = match e.scan() {
                        scan::ESC => 1,
                        scan::SPACE => 0x39,
                        scan::ENTER => 0x1c,
                        k @ (scan::HOME | scan::END | scan::UP | scan::DOWN | scan::PGUP | scan::PGDN) => k as i32,
                        _ => 0,
                    };
                    if matches!(action, 1 | 0x39 | 0x1c) {
                        game.wait_event(ev::KEY_UP);
                    }
                }
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (x, y) = (e.x, e.y);
                    last = (x, y);
                    if x < 600 {
                        let row = if (5..=0x7a).contains(&y) {
                            Some(0)
                        } else if top + 1 < n && 0x7a < y && y < 0xf1 {
                            Some(1)
                        } else if top + 2 < n && 0xf0 < y && y < 0x167 {
                            Some(2)
                        } else if top + 3 < n && 0x166 < y && y < 0x1dd {
                            Some(3)
                        } else {
                            None
                        };
                        if let Some(row) = row.filter(|&r| r != sel) {
                            sel = row;
                            redraw = true;
                        }
                    }
                    if e.mods & ev::M_LBUTTON == 0 {
                        if e.mods & ev::M_RBUTTON != 0 {
                            game.wait_event(ev::MOUSE_UP);
                            action = 1;
                        }
                    } else {
                        game.wait_event(ev::MOUSE_UP);
                        let in_row = (8 < y && y < 0x78)
                            || (sel == 1 && 0x7e < y && y < 0xee)
                            || (sel == 2 && 0xf4 < y && y < 0x164)
                            || (sel == 3 && 0x16a < y && y < 0x1da);
                        if !(0x268..=0x27b).contains(&x) {
                            if !(0xc..=0x68).contains(&x) {
                                if 0x207 < x && x < 600 && in_row {
                                    action = 0x1c;
                                }
                            } else if in_row {
                                action = 0x39;
                            }
                        } else if (6..=0x1d).contains(&y) {
                            action = 0x48;
                        } else if (0x1c4..=0x1db).contains(&y) {
                            action = 0x50;
                        } else if 0x1e < y && y < 0x1c3 {
                            let mut t = ((y - 0x1f) * n as i32) / 0x1a3;
                            if (n as i32) < t + 4 {
                                t = n as i32 - 4;
                            }
                            top = t.max(0) as usize;
                            sel = if y < 0x1a6 { 0 } else { 3 };
                            if n <= sel {
                                sel = n - 1;
                                top = 0;
                            }
                            full = true;
                        }
                    }
                }
                _ => {}
            }
        }
        match action {
            1 => break,
            0x39 => {
                unit_status(game, list[top + sel])?;
                cursor::warp(game, last.0, last.1);
                full = true;
            }
            0x1c => {
                let r = list[top + sel];
                if !town && (!is_enemy(game, r) || !game.battle.is_hidden(r.side, game.battle.unit(r))) {
                    let (cx, cy) = (game.battle.unit(r).x, game.battle.unit(r).y);
                    game.battle.cursor = (cx, cy);
                    if let Some(m) = game.battle.map.as_mut() {
                        m.center_on(cx, cy, true);
                    }
                    let (sx, sy) = view::cell_screen(game, cx, cy);
                    cursor::warp(game, sx + 0x20, sy + 0x10);
                    view::cursor_set(game, cx, cy);
                    result = 1;
                    break;
                }
            }
            0x48 => {
                if sel < 1 {
                    if top > 0 {
                        top -= 1;
                        full = true;
                    }
                } else {
                    sel -= 1;
                    redraw = true;
                }
            }
            0x50 => {
                if sel < 3 {
                    if sel + top < n - 1 {
                        sel += 1;
                        redraw = true;
                    }
                } else if sel + top < n - 1 {
                    top += 1;
                    full = true;
                }
            }
            0x49 => {
                if top < 1 {
                    if sel > 0 {
                        sel = 0;
                        redraw = true;
                    }
                } else {
                    top = top.saturating_sub(4);
                    full = true;
                }
            }
            0x51 => {
                if sel + 8 + top < n - 1 {
                    top += 4;
                    full = true;
                } else if n - 1 < top + 4 {
                    if sel + top < n - 1 {
                        sel = n - top - 1;
                        redraw = true;
                    }
                } else {
                    top = n - 4;
                    full = true;
                }
            }
            0x47 => {
                if sel != 0 || top != 0 {
                    top = 0;
                    sel = 0;
                    full = true;
                }
            }
            0x4f
                if (top as i32) < n as i32 - 4 => {
                    top = n - 4;
                    sel = 0;
                    full = true;
                }
            _ => {}
        }
    }
    game.flush_events(0x1f);
    if let Some(p) = saved {
        game.scr.pages[2] = p;
    }
    Ok(result)
}

/// Port of `FUN_004503f2(army, unit, x, y)`: the unit status page on page 1
/// (the rest of the screen darkened below it).
fn status_page(game: &mut Game, r: UnitRef, x: i32, y: i32) -> Result<()> {
    cursor::erase(game);
    let panel = game.res().ui_image(0xdb)?;
    // `(*(short *)(img + 2) + 1) * 0x280`: the header holds height - 1.
    let off = panel.height * 640;
    let lut = game.res().tints[5];
    let cur = game.scr.cur;
    for px in game.scr.pages[cur].pixels[off..].iter_mut() {
        *px = lut[*px as usize];
    }
    let below = game.scr.pages[cur].pixels[off..].to_vec();
    game.scr.pages[1].pixels[off..].copy_from_slice(&below);
    game.scr.set_page(1);
    gfx::blit(game.scr.page(), &panel, x, y, false);
    portrait_and_sprite(game, r, x + 0x15, y + 0x14, x + 0x7d, y + 0x14a);
    stats(game, r, x + 0x7b, y + 0x36);
    let u = game.battle.unit(r).clone();
    let enemy = is_enemy(game, r);
    if u.flags & flag::SUMMON == 0 {
        let name = s2data::text::cstr(&u.name).to_vec();
        text::large(game, x + 0x82, y + 0x16, &name, if enemy { 0x3c } else { 0xa7 }, 0, 1, 2);
    }
    let line = class_line(game, r);
    text::large(game, x + 0x204 - line.len() as i32 * 0xc, y + 0x16, &line, 0xb3, 0, 1, 2);
    if !enemy {
        if u.npc == 0 {
            let m = game.msg(0x13);
            text::small(game, x + 0x246, y + 0x1e, &m, 0x28, 0x2b, 1, 0);
        } else {
            text::small(game, x + 0x246, y + 0x1e, b"NPC", 0x26, 0x2b, 1, 0);
        }
    } else {
        let m = game.msg(0x14);
        text::small(game, x + 0x246, y + 0x1e, &m, 0x3c, 0x2b, 1, 0);
    }
    // The power rating: always for the side whose turn it is (VS) or the
    // player, otherwise only when the opposing army has flag 4.
    let other = if r.side == Side::Player { Side::Enemy } else { Side::Player };
    let seen = game.battle.army(other).flags & 4 != 0;
    let show = if game.campaign.vs_mode { r.side.index() as i32 == game.battle.vs_side || seen } else { !enemy || seen };
    if show {
        let m = game.msg(0x73);
        text::small(game, x + 0x216, y + 0x3f, &m, 0xb3, 0, 1, 2);
        let p = game.battle.power(&u, 0, 0);
        text::number(game, x + 0x240, y + 0x3f, p, 0x3c, 0, 1, 2, 0);
    }
    let m = game.msg(4);
    text::small(game, x + 0x216, y + 0x55, &m, 0xb3, 0, 1, 2);
    text::number(game, x + 0x240, y + 0x55, u.exp as i32, 0x3c, 0, 1, 2, 0);
    let m = game.msg(0x72);
    text::small(game, x + 0x216, y + 0x6b, &m, 0xb3, 0, 1, 2);
    text::number(game, x + 0x240, y + 0x6b, u.exp_next as i32, 0x3c, 0, 1, 2, 0);
    // The class's unit type icon (class byte 0x12).
    let icons: Vec<Option<Image>> = (0xe0..=0xe4).map(|id| game.res().ui_image(id).ok()).collect();
    let utype = game.res().class_rec(u.kind as usize).get(0x12).copied().unwrap_or(0);
    let pick = match utype {
        1..=5 => Some(0),
        6 => Some(1),
        7 => Some(2),
        8 => Some(3),
        9 | 10 => Some(4),
        _ => None,
    };
    if let Some(i) = pick.and_then(|i| icons[i].clone()) {
        gfx::blit(game.scr.page(), &i, x + 0x226, y + 0x19, false);
        if utype == 10 {
            if let Some(i0) = &icons[0] {
                gfx::blit(game.scr.page(), i0, x + 0x226 - i.width as i32, y + 0x19, false);
            }
        }
    }
    super::shop::equipment_slots(game, r, x + 0xf6, y + 0x9f);
    // Status effects (unit+0x3c, 27 bits) then buffs (unit+0x38, 15 bits).
    let names = game.res().status_names.clone();
    let (mut lx, mut ly) = (x + 0x1c1, y + 0xcd);
    let mut shown = 0;
    let mut put = |game: &mut Game, name: &[u8], turns: i32, lx: &mut i32, ly: &mut i32| -> bool {
        text::small(game, *lx + 0x11 - name.len() as i32 * 4, *ly + 4, name, 0x36, 0, 1, 2);
        if turns > 0 {
            text::number(game, *lx + 0x15, *ly + 10, turns, 0x67, 0x2b, 1, 0, 2);
        }
        *lx += 0x2d;
        if 0x274 < *lx {
            *lx = x + 0x1c1;
            *ly += 0x1c;
        }
        shown += 1;
        0x17 < shown
    };
    let mut full = false;
    for k in 0..0x1busize {
        if u.status & (1 << k) != 0 {
            let name = names.get(k).cloned().unwrap_or_default();
            if put(game, &name, u.status_turns[k] as i32, &mut lx, &mut ly) {
                full = true;
                break;
            }
        }
    }
    if !full {
        for k in 0..0xfusize {
            if u.buffs & (1 << k) != 0 {
                let name = names.get(0x1b + k).cloned().unwrap_or_default();
                let turns = if k < 9 {
                    u.buff_a[k].turns
                } else if k < 0xe {
                    u.buff_b[k - 9].turns
                } else {
                    u.buff_c.turns
                } as i32;
                if put(game, &name, turns, &mut lx, &mut ly) {
                    break;
                }
            }
        }
    }
    game.scr.set_page(0);
    Ok(())
}

/// Port of `FUN_00450de0`: the unit's special abilities (隱技).
fn abilities_page(game: &mut Game, r: UnitRef) -> Result<()> {
    let img = game.res().ui_image(0xe6)?;
    let py = 0x1e0 - img.height as i32;
    gfx::blit(game.scr.page(), &img, 0, py, false);
    let t = game.msg(0x9e);
    text::large(game, 0x140 - t.len() as i32 * 6, py + 7, &t, 0x49, 0x37, 1, 0);
    let abil = game.battle.unit(r).abilities;
    let count = {
        let k = &game.res().kinds_raw;
        k.get(10..12).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]) as i32)
    };
    if abil == 0 {
        let s = game.res().ability_name(0).to_vec();
        text::large(game, 0x140 - s.len() as i32 * 6, py + 0x3d, &s, 0xe9, 0, 1, 2);
    } else {
        let n = (1..count).filter(|&i| abil & (1u32 << ((i - 1) & 31)) != 0).count() as i32;
        let mut lx = 0x144 - ((n - 1) / 2 + 1) * 0x24;
        let mut ly = py + 0x34;
        let mut second = false;
        for i in 1..count {
            if abil & (1u32 << ((i - 1) & 31)) != 0 {
                let s = game.res().ability_name(i as usize).to_vec();
                text::large(game, lx, ly, &s, 0xe9, 0, 1, 2);
                second = !second;
                if second {
                    ly += 0x1e;
                } else {
                    lx += 0x48;
                    ly = py + 0x34;
                }
            }
        }
    }
    gfx::present(game);
    crate::ui::click_to_continue(game, 0);
    Ok(())
}

/// Port of `FUN_00450ffe`: weapon and school proficiencies.
#[allow(clippy::needless_range_loop)] // index math as in the original's loop
fn proficiency_page(game: &mut Game, r: UnitRef) -> Result<()> {
    let img = game.res().ui_image(0xe8)?;
    let py = 0x1e0 - img.height as i32;
    gfx::blit(game.scr.page(), &img, 0, py, false);
    let t = game.msg(0x9f);
    text::large(game, 0x140 - t.len() as i32 * 6, py + 8, &t, 0x49, 0x37, 1, 0);
    let t = game.msg(0x2a);
    text::large(game, 0x87, py + 8, &t, 0x49, 0x37, 1, 0);
    let t = game.msg(0xa0);
    text::large(game, 0x1c6, py + 8, &t, 0x49, 0x37, 1, 0);
    let prof = game.battle.unit(r).prof;
    let (mut lx, mut ly) = (0x154, 0x181);
    for k in 0..6usize {
        let l = game.msg(0xac + k);
        text::small(game, lx, ly, &l, 0xe9, 0x37, 1, 2);
        text::number(game, lx + 0x50, ly, prof[k] as i32, 0x89, 0, 1, 2, 0);
        if k == 4 {
            lx = 0x1ea;
            ly = 0x181;
        } else {
            ly += 0x10;
        }
    }
    let (mut lx, mut ly) = (0x14, 0x181);
    for k in 0..9usize {
        let l = game.msg(0xa1 + k);
        text::small(game, lx, ly, &l, 0xe9, 0, 1, 2);
        text::number(game, lx + 0x60, ly, (prof[6 + k] / 10) as i32, 0x89, 0, 1, 2, 0);
        if k == 4 {
            lx = 0xa5;
            ly = 0x181;
        } else {
            ly += 0x10;
        }
    }
    for (k, (msg, yy)) in [(0xaausize, 0x1b1), (0xab, 0x1c1)].into_iter().enumerate() {
        let l = game.msg(msg);
        text::small(game, 0x1ea, yy, &l, 0xe9, 0, 1, 2);
        text::number(game, 0x23a, yy, (prof[15 + k] / 10) as i32, 0x89, 0, 1, 2, 0);
    }
    gfx::present(game);
    crate::ui::click_to_continue(game, 0);
    Ok(())
}

/// Port of `FUN_0044feb2(army, unit)`: the unit status screen with its four
/// tabs (abilities, proficiency, magic, techniques).
pub fn unit_status(game: &mut Game, r: UnitRef) -> Result<()> {
    status_page(game, r, 0, 0)?;
    let tabs: Vec<Option<Image>> = (0xdc..=0xdf).map(|id| game.res().ui_image(id).ok()).collect();
    let enabled = [true, true, game.battle.knows_skill_kind(r, 1), game.battle.knows_skill_kind(r, 2)];
    let mut tab = 0usize;
    let (mut warp, mut redraw, mut moved) = (true, true, false);
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if warp {
            cursor::warp(game, tab as i32 * 0x32 + 0x1d0, 0xae);
            warp = false;
        }
        if redraw {
            game.scr.copy_page(0, 1);
            let t = tab as i32 * 0x32;
            gfx::rect(game.scr.page(), t + 0x1b1, 0x9a, t + 0x1de, 0xb8, 0x3c, false);
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        let mut action = 0;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::LEFT => action = 2,
                    scan::RIGHT => action = 3,
                    scan::ESC => {
                        game.wait_event(ev::KEY_UP);
                        action = -1;
                    }
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 1;
                    }
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (x, y) = (e.x, e.y);
                    if 0x1b1 < x && x < 0x27a && 0x99 < y && y < 0xba {
                        let t = ((x - 0x1b2) / 0x32) as usize;
                        if t != tab && enabled[t] {
                            redraw = true;
                            tab = t;
                        }
                        if e.mods & ev::M_LBUTTON != 0 && t == tab {
                            game.wait_event(ev::MOUSE_UP);
                            action = 1;
                        }
                    }
                    if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        action = -1;
                    }
                }
                _ => {}
            }
        }
        match action {
            -1 => return Ok(()),
            1 => {
                cursor::erase(game);
                if let Some(i) = &tabs[tab] {
                    gfx::blit(game.scr.page(), i, tab as i32 * 0x32 + 0x1b1, 0x9a, true);
                }
                gfx::present(game);
                game.flush_events(0x1f);
                match tab {
                    0 => abilities_page(game, r)?,
                    1 => proficiency_page(game, r)?,
                    // FUN_00451417(unit, 0 / 1): browse magic / techniques.
                    t => super::spellview::browse(game, r, t == 3),
                }
                warp = true;
                redraw = true;
            }
            2 | 3 => {
                let step = |t: usize| if action == 2 { (t + 3) % 4 } else { (t + 1) % 4 };
                tab = step(tab);
                let mut k = 0;
                while k < 4 && !enabled[tab] {
                    tab = step(tab);
                    k += 1;
                }
                warp = true;
                redraw = true;
            }
            _ => {}
        }
    }
}

/// Port of `FUN_004301ae` with `FUN_0042aa7e`: the whole battle map at a
/// third of its size, centred on a cleared page, until a click.
pub fn map_overview(game: &mut Game) {
    let Some(mv) = game.battle.view.map.as_ref() else { return };
    let s = &mv.surface;
    let (w, h) = (s.width as i32 / 64, s.height as i32 / 32);
    let (ow, oh) = (((w - 1) * 0x40 / 3).max(0) as usize, ((h - 1) * 0x20 / 3).max(0) as usize);
    let mut img = Image::new(ow, oh);
    for r in 0..oh {
        let sy = 16 + 3 * r;
        for c in 0..ow {
            let sx = 0x1f + 3 * c;
            img.pixels[r * ow + c] = s.pixels.get(sy * s.width + sx).copied().unwrap_or(0);
        }
    }
    let cur = game.scr.cur;
    game.scr.clear_page(cur);
    gfx::blit(game.scr.page(), &img, 0x140 - (ow as i32 + 1) / 2, 0xf0 - (oh as i32 + 1) / 2, true);
    gfx::present(game);
    crate::ui::click_to_continue(game, 0);
}

/// Port of `FUN_0042f033` (總檢表 in the battle system menu): the unit
/// list or the map overview. Returns 1 when the unit list moved the map
/// cursor onto a unit.
pub fn overview_menu(game: &mut Game) -> Result<i32> {
    cursor::erase(game);
    let lut = game.res().tints[6];
    gfx::remap_page(game.scr.page(), &lut);
    let menu = game.res().ui_image(0xc2)?;
    // The image header holds width - 1: `(*(short *)img + 1) / 2` is width / 2.
    let (px, py) = (0x1c7 - menu.width as i32 / 2, 0x14e - menu.height as i32 / 2);
    let panel = game.res().ui_image(0xc9)?;
    gfx::blit(game.scr.page(), &panel, px, py, false);
    let t = game.msg(0x7a);
    text::large(game, 0x140 - t.len() as i32 * 6, py + 0x2e, &t, 0x4b, 0, 1, 2);
    let items = [game.msg(0x9c), game.msg(0x9d)];
    text::large(game, px + 0xc, py + 0x4d, &items[0], 0xe9, 0, 1, 2);
    text::large(game, px + 0xc, py + 0x6c, &items[1], 0xe9, 0, 1, 2);
    game.scr.copy_page(2, 0);
    let bar = game.res().ui_image(0xc3).ok();
    cursor::warp(game, 0x140, py + 99);
    let (mut sel, mut result) = (0usize, 0);
    let (mut first, mut redraw, mut moved) = (true, true, false);
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if redraw {
            game.scr.copy_page(0, 2);
            text::large(game, px + 0xc, py + 0x4d + sel as i32 * 0x1f, &items[sel], 0x77, 0, 1, 2);
            if first {
                cursor::capture(game);
            }
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
            if first {
                game.flush_events(0x1f);
                first = false;
            }
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        let mut action = 0;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::ESC => {
                        game.wait_event(ev::KEY_UP);
                        action = 1;
                    }
                    scan::UP | scan::DOWN => action = 0x48,
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 0x39;
                    }
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (x, y) = (e.x, e.y);
                    if px + 7 <= x && x <= px + 0x6f {
                        for (k, (y0, y1)) in [(py + 0x4e, py + 0x68), (py + 0x6d, py + 0x87)].into_iter().enumerate() {
                            if y0 <= y && y <= y1 {
                                if sel != k {
                                    sel = k;
                                    redraw = true;
                                } else if e.mods & ev::M_LBUTTON != 0 {
                                    game.wait_event(ev::MOUSE_UP);
                                    action = 0x39;
                                }
                            }
                        }
                    }
                    if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        action = 1;
                    }
                }
                _ => {}
            }
        }
        match action {
            0x48 => {
                sel ^= 1;
                cursor::warp(game, 0x140, py + 99 + sel as i32 * 0x1f);
                redraw = true;
            }
            1 => break,
            0x39 => {
                cursor::erase(game);
                if let Some(b) = &bar {
                    gfx::blit(game.scr.page(), b, px + 10, py + 0x4c + sel as i32 * 0x1f, true);
                }
                text::large(game, px + 0xc, py + 0x4d + sel as i32 * 0x1f, &items[sel], 0x77, 0, 1, 2);
                cursor::draw_present(game, None, false);
                crate::ui::sound(game, 100);
                if sel == 0 {
                    if status_list(game, false)? == 1 {
                        result = 1;
                        break;
                    }
                } else {
                    map_overview(game);
                }
                cursor::warp(game, 0x140, py + 99 + sel as i32 * 0x1f);
                first = true;
                redraw = true;
            }
            _ => {}
        }
    }
    game.flush_events(0x1f);
    Ok(result)
}
