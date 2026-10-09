//! Battle panels and windows: the 狀況視窗 status bar (`FUN_0042b45f`), the
//! attack/spell forecast panel (`FUN_00452246`), the EXP/item reward window
//! (`FUN_00452e38`) and the level-up window (`FUN_004537da`/`FUN_00456549`).
//! Text goes through the FLOW text renderer (`crate::text`).

use std::collections::HashMap;
use std::sync::OnceLock;

use s2data::gfx_portrait::PortraitKind;
use s2data::sprite::Image;

use crate::game::Game;
use crate::{gfx, text, ui};

use super::super::map::Pass;
use super::super::{Side, UnitRef};

/// UI image slots used by the panels.
mod slot {
    /// Forecast panel (640x117).
    pub const DUEL: usize = 0x8e;
    /// Status bar with a unit / terrain only (640x117).
    pub const STATUS_UNIT: usize = 0x8f;
    pub const STATUS_TERRAIN: usize = 0x90;
    /// Level-up arrow.
    pub const ARROW: usize = 0x94;
    /// Terrain attack / defence icons.
    pub const ICON_ATK: usize = 0xbe;
    pub const ICON_DEF: usize = 0xbf;
    /// Forecast damage / hit icons.
    pub const ICON_DMG: usize = 0xc0;
    pub const ICON_HIT: usize = 0xc1;
    /// Reward windows (CEL 2 on demand).
    pub const EXP_WINDOW: usize = 0xd0;
    pub const ITEM_WINDOW: usize = 0xd1;
    pub const LEVEL_WINDOW: usize = 0xef;
    /// Item icons: `0x1ed + item index`.
    pub const ITEM_ICON: usize = 0x1ed;
}

/// Encode text from the decoded tables back to the Big5 bytes the fonts
/// draw (every two-byte Big5 code decoded once into a reverse table).
pub fn to_big5(s: &str) -> Vec<u8> {
    static MAP: OnceLock<HashMap<char, [u8; 2]>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m = HashMap::new();
        // The standard range first: the HKSCS rows (0x81..0xa0) repeat
        // characters the fonts only have at their standard codes (者).
        for hi in (0xa1u8..=0xfe).chain(0x81..=0xa0) {
            for lo in (0x40u8..=0x7e).chain(0xa1..=0xfe) {
                let d = s2data::text::big5(&[hi, lo]);
                let mut cs = d.chars();
                if let (Some(c), None) = (cs.next(), cs.next()) {
                    if c != '\u{fffd}' {
                        m.entry(c).or_insert([hi, lo]);
                    }
                }
            }
        }
        m
    });
    let mut out = Vec::new();
    for c in s.chars() {
        if (c as u32) < 0x80 {
            out.push(c as u8);
        } else if let Some(b) = map.get(&c) {
            out.extend_from_slice(b);
        }
    }
    out
}

/// `FUN_0047a1ce` for the panel formats (`%s`, `%d`, `% Nd`).
fn fmt(f: &[u8], args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut it = args.iter();
    let mut i = 0;
    while i < f.len() {
        let c = f[i];
        if c == 0 {
            break;
        }
        if c != b'%' || i + 1 >= f.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let space = f[j] == b' ';
        if space {
            j += 1;
        }
        let mut width = 0usize;
        while j < f.len() && f[j].is_ascii_digit() {
            width = width * 10 + (f[j] - b'0') as usize;
            j += 1;
        }
        match (f.get(j), it.next()) {
            (Some(b's'), Some(Arg::S(s))) => out.extend_from_slice(s2data::text::cstr(s)),
            (Some(b'd'), Some(Arg::D(v))) => {
                let s = if space && *v >= 0 { format!(" {v}") } else { v.to_string() };
                out.extend(std::iter::repeat_n(b' ', width.saturating_sub(s.len())));
                out.extend_from_slice(s.as_bytes());
            }
            _ => {}
        }
        i = j + 1;
    }
    out
}

enum Arg<'a> {
    S(&'a [u8]),
    D(i32),
}

fn img(game: &Game, s: usize) -> Option<Image> {
    game.res.as_ref()?.ui_image(s).ok()
}

fn blit(game: &mut Game, s: usize, x: i32, y: i32) {
    if let Some(i) = img(game, s) {
        gfx::blit(game.scr.page(), &i, x, y, false);
    }
}

/// `FUN_0041f6d3`: a number in the regular font.
fn large_number(game: &mut Game, x: i32, y: i32, v: i32, fg: u8) {
    text::large(game, x, y, v.to_string().as_bytes(), fg, 0, 1, 2);
}

fn class_name(game: &Game, kind: i16) -> Vec<u8> {
    game.battle.tables.as_ref().and_then(|t| t.kinds.class(kind as u16)).map_or(Vec::new(), |c| c.name_raw.clone())
}

/// The unit's battle portrait (`FUN_0043fea5`: its character id, or the
/// class default for generic units).
fn portrait(game: &Game, r: UnitRef) -> Option<Image> {
    let u = game.battle.unit(r);
    let id = if (1..200).contains(&u.char_id) {
        u.char_id as usize
    } else {
        game.battle.tables.as_ref()?.kinds.class(u.kind as u16)?.sprite_set as usize
    };
    game.res.as_ref()?.portraits.get(id, PortraitKind::Battle).ok()
}

/// `FUN_00452898`: portrait at (x, y).
fn draw_portrait(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    if let Some(p) = portrait(game, r) {
        gfx::blit(game.scr.page(), &p, x, y, false);
    }
}

/// `y` of the panel for `DAT_0048ea9c`: 1 = top, 2 = bottom.
fn panel_y(pos: i32) -> i32 {
    if pos != 1 { 0x16b } else { 0 }
}

/// Port of FUN_0042b3fc / FUN_0045210f: move the panel away from a cell
/// that comes near it.
fn update_panel_pos(game: &mut Game, x: i32, y: i32) {
    let (_, sy) = super::cell_screen(game, x, y);
    let v = &mut game.battle.view;
    if sy < 0xb8 && v.panel_pos == 1 {
        v.panel_pos = 2;
    } else if 0x158 < sy && v.panel_pos == 2 {
        v.panel_pos = 1;
    }
}

/// Port of FUN_004273e4: the terrain part of the status bar for cell
/// (x, y); with `unit`, also its attack/defence modifiers there.
pub fn terrain_info(game: &mut Game, x: i32, y: i32, pos: i32, unit: Option<UnitRef>) {
    if x == -1 && y == -1 {
        return;
    }
    let y0 = panel_y(pos);
    let Some(mv) = game.battle.view.map.as_ref() else { return };
    let w = mv.width();
    let Some(&cell) = mv.def.cells.get((y * w + x).max(0) as usize) else { return };
    if cell.ground == 1 {
        return;
    }
    let Some(assets) = game.battle.view.assets.as_deref() else { return };
    let attrs = &assets.tiles.attrs;
    let ground_attr = attrs.ground.get((cell.ground as i32 - 1).max(0) as usize).copied().unwrap_or(0);
    if cell.ground < 1 || ground_attr == 0 {
        let m = game.msg(0);
        text::large(game, 0x21f, y0 + 0x32, &m, 0xb3, 0, 1, 2);
        return;
    }
    let flags = game.campaign.flags;
    let shown = |cat: u8| !matches!((cat, flags[2], flags[1], flags[4]), (5, 0, _, _) | (6, _, 0, _) | (0x0b, _, _, 0));
    let ov = cell.overlay as i32;
    let ov_attr = if ov > 0 { attrs.overlay.get(ov as usize - 1).copied().unwrap_or(0) } else { 0 };
    let cat = (ov_attr >> 8) as u8;
    let (tile, ovt) = (assets.tiles.ground_tile(cell.ground as i32), assets.tiles.overlay_tile(ov));
    if let Some(t) = tile {
        if cell.ground == 5 || cell.ground == 6 {
            // FUN_00442182: the map surface under the cell with the units
            // pasted out again, i.e. the background, inside the tile shape.
            let (wx, wy) = mv.cell_world(x, y);
            let mut s = super::unit::grab(&mv.bg, wx, wy, 64, 48);
            for (p, &m) in s.pixels.iter_mut().zip(t.pixels.iter()).take(0xc00) {
                if m == 0 {
                    *p = 0;
                }
            }
            gfx::blit(game.scr.page(), &s, 0x20e, y0 + 0x28, false);
        } else {
            gfx::blit(game.scr.page(), &t, 0x20e, y0 + 0x28, false);
        }
    }
    if ov > 0 && shown(cat) {
        if let Some(o) = ovt {
            let part = Image { width: 64, height: 32, pixels: o.pixels[0x1000..0x1800].to_vec() };
            gfx::blit(game.scr.page(), &part, 0x20e, y0 + 0x28, false);
        }
    }
    text::small(game, 0x253, y0 + 0x30, format!("X={x}").as_bytes(), 0x89, 0, 1, 2);
    text::small(game, 0x253, y0 + 0x40, format!("Y={y}").as_bytes(), 0x89, 0, 1, 2);
    large_number(game, 0x213, y0 + 0xe, cell.height as i32, 0x8d);
    let Some(tables) = game.battle.tables.clone() else { return };
    let ground_n = tables.terrain.ground as i32;
    let name_idx = if ov == 0 || cat == 7 || cat == 0x0c || !shown(cat) || (ov_attr & 0xff) as i32 > tables.terrain.objects as i32 {
        let g = (ground_attr & 0xff) as i32;
        if g == 0 {
            return;
        }
        g - 1
    } else {
        (ov_attr & 0xff) as i32 - 1 + ground_n
    };
    let name = tables.terrain.names.get(name_idx.max(0) as usize).map(|s| to_big5(s)).unwrap_or_default();
    text::small(game, 0x253 - name.len() as i32 * 4, y0 + 0x10, &name, 0xb2, 0x36, 1, 0);
    let Some(r) = unit else { return };
    let u = game.battle.unit(r).clone();
    let mt = game.battle.move_type(&u);
    let (p, eff) = game.battle.map().terrain(&tables, x, y, u.kind, mt);
    if p != Pass::Ok {
        return;
    }
    let (atk, def) = (eff.attack as i32, eff.defense as i32);
    blit(game, slot::ICON_ATK, 0x20f, y0 + 0x5b);
    blit(game, slot::ICON_DEF, 0x241, y0 + 0x5b);
    text::small(game, 0x224, y0 + 0x5c, if atk < 0 { b"-" } else { b"+" }, 0x89, 0, 1, 2);
    text::small(game, 599, y0 + 0x5c, if def < 0 { b"-" } else { b"+" }, 0x89, 0, 1, 2);
    text::number(game, 0x22c, y0 + 0x5c, atk.abs(), 0x89, 0, 1, 2, 0);
    text::number(game, 0x25f, y0 + 0x5c, def.abs(), 0x89, 0, 1, 2, 0);
}

/// Port of FUN_004517b5: HP/MP/ST/AP with values and bars at (x, y).
fn stat_bars(game: &mut Game, r: UnitRef, x: i32, y: i32) {
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
        let s = fmt(b"%d+%d", &[Arg::D(u.ap_max as i32), Arg::D(u.ap_carry as i32)]);
        text::small(game, x + 0xe0, y + 0x39, &s, 0x4f, 0, 1, 2);
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

/// Port of FUN_00451efd: attack/defence/reaction and magic/skill/sword values.
fn combat_stats(game: &mut Game, r: UnitRef, x: i32, y: i32, color: u8) {
    let u = game.battle.unit(r).clone();
    let rows = [(7usize, u.atk_cur, 11usize, u.mag_cur), (8, u.def_cur, 9, u.skill_cur), (10, u.react, 12, u.sword)];
    for (k, &(l1, v1, l2, v2)) in rows.iter().enumerate() {
        let ry = y + 0x16 * k as i32;
        let m1 = game.msg(l1);
        let m2 = game.msg(l2);
        text::small(game, x, ry, &m1, 0x36, 0, 1, 2);
        text::small(game, x + 0x41, ry, &m2, 0x36, 0, 1, 2);
        let ny = if k == 0 { y + 1 } else if k == 1 { y + 0x16 } else { y + 0x2b };
        text::number(game, x + 0x26, ny, v1 as i32, color, 0, 1, 2, 0);
        text::number(game, x + 0x68, ny, v2 as i32, color, 0, 1, 2, 0);
    }
}

/// Port of FUN_0045161f: the unit part of the status bar.
pub fn unit_info(game: &mut Game, r: UnitRef, pos: i32) {
    let y0 = panel_y(pos);
    let u = game.battle.unit(r).clone();
    let side = game.battle.army(r.side).side;
    draw_portrait(game, r, 7, y0 + 3);
    if u.flags & super::unit::F_SUMMON == 0 {
        let c = if side != Side::Player { 0x3c } else { 0xa7 };
        text::large(game, 0x6f, y0 + 7, &u.name, c, 0, 1, 2);
    }
    let cname = class_name(game, u.kind);
    let lv = game.msg(0x71);
    let s = fmt(b"%s %d%s", &[Arg::S(&cname), Arg::D(u.level as i32), Arg::S(&lv)]);
    text::small(game, 0x114, y0 + 0xb, &s, 0xb2, 0, 1, 2);
    let m = game.msg(4);
    text::small(game, 0x198, y0 + 0xb, &m, 0xb2, 0x36, 1, 1);
    text::number(game, 0x1c1, y0 + 0xb, u.exp as i32, 0x3c, 0, 1, 2, 0);
    // FUN_00451772
    stat_bars(game, r, 0x68, y0 + 0x21);
    combat_stats(game, r, 0x68 + 0x10b, y0 + 0x21 + 9, 0xca);
}

/// Port of FUN_0042b45f: the 狀況視窗 bar for the cell under the map
/// cursor. `moving` is the unit whose move range is shown (the bar then
/// shows it and the terrain effects of the cursor cell for it).
pub fn status_bar(game: &mut Game, moving: Option<UnitRef>) {
    let (cx, cy) = game.battle.cursor;
    if (game.battle.view.cursor.col, game.battle.view.cursor.row) != (cx, cy) {
        super::cursor_set(game, cx, cy);
    }
    update_panel_pos(game, cx, cy);
    let cur = game.battle.view.cursor;
    let kind = if cur.flat {
        4
    } else if let Some(r) = game.battle.unit_at_cell(Side::Player, cx, cy, false) {
        let _ = r;
        2
    } else if let Some(r) = game.battle.unit_at_cell(Side::Enemy, cx, cy, false) {
        if game.battle.is_hidden(Side::Enemy, game.battle.unit(r)) { 1 } else { 3 }
    } else {
        1
    };
    if !(game.cfg.status_window || (kind != 4 && kind != 1) || moving.is_some()) {
        return;
    }
    let pos = game.battle.view.panel_pos;
    let y0 = panel_y(pos);
    let s = if (kind == 4 || kind == 1) && moving.is_none() { slot::STATUS_TERRAIN } else { slot::STATUS_UNIT };
    blit(game, s, 0, y0);
    match moving {
        None => match kind {
            2 | 3 => {
                let side = if kind == 2 { Side::Player } else { Side::Enemy };
                if let Some(r) = game.battle.unit_at_cell(side, cx, cy, false) {
                    // FUN_004515d4
                    let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
                    terrain_info(game, ux, uy, pos, Some(r));
                    unit_info(game, r, pos);
                }
            }
            _ => {
                if kind == 1 {
                    terrain_info(game, cx, cy, pos, None);
                } else if !cur.off_map {
                    let m = game.msg(0);
                    text::large(game, 0x21f, y0 + 0x32, &m, 0xa7, 0, 1, 2);
                }
                let f1 = game.msg(1);
                let turn = fmt(&f1, &[Arg::D(game.battle.turn)]);
                let title = game.events.stage.as_ref().map(|s| s.name.clone()).unwrap_or_default();
                let line = fmt(b"%d.%s %s", &[Arg::D(game.campaign.battles_fought as i32), Arg::S(&title), Arg::S(&turn)]);
                text::large(game, 0xfe - line.len() as i32 * 6, y0 + 0x32, &line, 99, 0, 1, 2);
            }
        },
        Some(r) => {
            unit_info(game, r, pos);
            if cur.flat {
                if !cur.off_map {
                    let m = game.msg(0);
                    text::large(game, 0x21f, y0 + 0x32, &m, 0xa7, 0, 1, 2);
                }
            } else {
                terrain_info(game, cx, cy, pos, Some(r));
            }
        }
    }
}

/// Forecast numbers (`Duel+0x18..+0x24`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Forecast {
    pub hit: i32,
    pub dmg: i32,
    pub counter_hit: i32,
    pub counter_dmg: i32,
}

/// Port of FUN_00452246: the forecast panel, `att` on the left and `def`
/// on the right with HP bars, and the attack (and counter) numbers.
/// `by_cursor` places it by the map cursor (FUN_0042b3fc, spells) instead
/// of by the defender (FUN_0045210f).
pub fn duel_panel(game: &mut Game, att: UnitRef, def: UnitRef, fc: Forecast, by_cursor: bool) {
    if by_cursor {
        let (cx, cy) = game.battle.cursor;
        update_panel_pos(game, cx, cy);
    } else {
        let (x, y) = (game.battle.unit(def).x, game.battle.unit(def).y);
        update_panel_pos(game, x, y);
    }
    let y0 = panel_y(game.battle.view.panel_pos);
    blit(game, slot::DUEL, 0, y0);
    draw_portrait(game, att, 9, y0 + 4);
    draw_portrait(game, def, 0x21b, y0 + 4);
    let same = att.side == def.side;
    let (ca, cd) = if game.battle.army(att.side).side == Side::Player {
        (0xa9u8, if same { 0xa9 } else { 0x3a })
    } else {
        (0x3a, if same { 0x3a } else { 0xa9 })
    };
    let lv = game.msg(0x71);
    let (a, d) = (game.battle.unit(att).clone(), game.battle.unit(def).clone());
    text::large(game, 0x74, y0 + 10, &a.name, ca, 0, 1, 2);
    let s = fmt(b"%s %d%s", &[Arg::S(&class_name(game, a.kind)), Arg::D(a.level as i32), Arg::S(&lv)]);
    text::small(game, 0x75, y0 + 0x24, &s, ca, 0, 1, 2);
    text::small(game, 0x77, y0 + 0x60, b"HP", 99, 0, 1, 2);
    let s = fmt(b"%d/%d", &[Arg::D(a.hp as i32), Arg::D(a.hp_max as i32)]);
    text::small(game, 0x8d, y0 + 0x59, &s, 99, 0, 1, 2);
    let miss = |c: i16, m: i16| if c == 0 || m == 0 { 0x6b } else { (m as i32 - c as i32) * 0x6b / m as i32 };
    if a.hp < a.hp_max || a.hp == 0 {
        let m = miss(a.hp, a.hp_max);
        let page = game.scr.page();
        gfx::hline(page, y0 + 0x69, 0x106 - m, 0x106, 0xdd);
        gfx::rect(page, 0x106 - m, y0 + 0x6a, 0x106, y0 + 0x6b, 0x2b, true);
        gfx::rect(page, 0x106 - m, y0 + 0x6c, 0x106, y0 + 0x6e, 0x2e, true);
    }
    let dn = text::strlen(&d.name);
    text::large(game, dn * -0xc + 0x208, y0 + 10, &d.name, cd, 0, 1, 2);
    let s = fmt(b"%s %d%s", &[Arg::S(&class_name(game, d.kind)), Arg::D(d.level as i32), Arg::S(&lv)]);
    text::small(game, s.len() as i32 * -8 + 0x208, y0 + 0x24, &s, cd, 0, 1, 2);
    text::small(game, 0x1f9, y0 + 0x60, b"HP", 99, 0, 1, 2);
    let s = fmt(b"%d/%d", &[Arg::D(d.hp as i32), Arg::D(d.hp_max as i32)]);
    text::small(game, s.len() as i32 * -8 + 0x1f1, y0 + 0x59, &s, 99, 0, 1, 2);
    if d.hp < d.hp_max || d.hp == 0 {
        let m = miss(d.hp, d.hp_max);
        let page = game.scr.page();
        gfx::hline(page, y0 + 0x69, 0x179, 0x179 + m, 0xdd);
        gfx::rect(page, 0x179, y0 + 0x6a, 0x179 + m, y0 + 0x6b, 0x2b, true);
        gfx::rect(page, 0x179, y0 + 0x6c, 0x179 + m, y0 + 0x6e, 0x2e, true);
    }
    if fc.hit != 0 {
        if fc.dmg > 0 {
            blit(game, slot::ICON_DMG, 0x78, y0 + 0x3e);
            text::number(game, 0x97, y0 + 0x40, fc.dmg, 0x89, 0, 1, 2, 0);
        }
        blit(game, slot::ICON_HIT, 0xc1, y0 + 0x3e);
        text::number(game, 0xe2, y0 + 0x40, fc.hit, 0x89, 0, 1, 2, 0);
    }
    if fc.counter_dmg != 0 && fc.counter_hit != 0 {
        blit(game, slot::ICON_DMG, 0x16e, y0 + 0x3e);
        blit(game, slot::ICON_HIT, 0x1b7, y0 + 0x3e);
        text::number(game, 399, y0 + 0x40, fc.counter_dmg, 0x89, 0, 1, 2, 0);
        text::number(game, 0x1d8, y0 + 0x40, fc.counter_hit, 0x89, 0, 1, 2, 0);
    }
}

/// Port of FUN_00452215: the forecast of a physical attack from a duel.
pub fn attack_forecast(game: &mut Game, duel: &super::super::combat::Duel) {
    let c = &duel.calc;
    let fc = Forecast { hit: c.fc_hit, dmg: c.fc_dmg, counter_hit: c.fc_chit, counter_dmg: c.fc_cdmg };
    duel_panel(game, duel.att, duel.def, fc, false);
}

/// Port of FUN_004521c5 / FUN_00452178: the forecast of a spell on one
/// target (`amount` from the spell code; hit shows 100 when it lands).
pub fn spell_forecast(game: &mut Game, caster: UnitRef, target: UnitRef, hit: i32, amount: i32) {
    duel_panel(game, caster, target, Forecast { hit, dmg: amount, ..Default::default() }, true);
}

/// Port of FUN_00452e38's presentation: the EXP box over `r` (`exp` > 0),
/// an optional "<name> <msg 3> <text>" box, then the item window pages
/// (`items[i]` = count of item i+1 gained). State changes are the caller's.
pub fn reward_window(game: &mut Game, r: UnitRef, exp: i32, text_line: Option<&[u8]>, items: &[u8]) {
    let (x, y) = (game.battle.unit(r).x, game.battle.unit(r).y);
    if !game.battle.map().on_screen(x, y, 2) {
        game.battle.map_mut().center_on(x, y, true);
    }
    let (sx, sy) = super::cell_screen(game, x, y);
    let Some(win) = img(game, slot::EXP_WINDOW) else { return };
    let (w, h) = (win.width as i32, win.height as i32);
    let sprite_h = game.battle.unit(r).sprite_h as i32;
    let mut wy = (sy + 0x20 - sprite_h) - h;
    if wy < 0 {
        wy = sy + 0x20;
    } else if 0x1d6 < wy + h {
        wy = 0x1d6 - h;
    }
    let mut wx = (sx + 0x20) - w / 2;
    if wx < 0x10 {
        wx = 0x10;
    } else if 0x27a < wx + w {
        wx = 0x27a - w;
    }
    let name = game.battle.unit(r).name.to_vec();
    let lut = game.res().tints[6];
    if exp > 0 {
        gfx::window_shadow(game.scr.page(), &lut, wx, wy, w, h);
        gfx::blit(game.scr.page(), &win, wx, wy, false);
        let f = game.msg(2);
        let s = fmt(&f, &[Arg::S(&name), Arg::D(exp)]);
        let tx = wx + w / 2 + (s.len() as i32 - 3) * -6 - 0x18;
        text::large(game, tx, wy - 0xc + h / 2, &s, 0x2a, 0, 1, 2);
        gfx::present(game);
        ui::click_to_continue(game, 0x5a);
    }
    if let Some(t) = text_line.filter(|t| !t.is_empty() && t[0] != 0) {
        gfx::window_shadow(game.scr.page(), &lut, wx, wy, w, h);
        gfx::blit(game.scr.page(), &win, wx, wy, false);
        let m = game.msg(3);
        let s = fmt(b"%s %s %s", &[Arg::S(&name), Arg::S(&m), Arg::S(t)]);
        let tx = wx + w / 2 + (s.len() as i32 - 2) * -6 - 0x10;
        text::large(game, tx, wy - 0xc + h / 2, &s, 0x2a, 0, 1, 2);
        gfx::present(game);
        ui::click_to_continue(game, 0xfa);
    }
    let gained: Vec<(usize, u8)> = items.iter().enumerate().filter(|(_, &n)| n != 0).map(|(i, &n)| (i, n)).collect();
    if gained.is_empty() {
        return;
    }
    let Some(win2) = img(game, slot::ITEM_WINDOW) else { return };
    let (w2, h2) = (win2.width as i32, win2.height as i32);
    let mut wy2 = wy + 0x5a;
    if wy2 < 0 {
        wy2 = sy + 0x20;
    }
    if 0x1d0 < wy2 + h2 {
        wy2 = 0x1d0 - h2;
    }
    let mut wx2 = (sx + 0x20) - w2 / 2;
    if wx2 < 0x10 {
        wx2 = 0x3c;
    } else if 0x27a < wx2 + w2 {
        wx2 = 0x27a - w2;
    }
    gfx::window_shadow(game.scr.page(), &lut, wx2, wy2, w2, h2);
    let m = game.msg(0x26);
    let head = fmt(b"%s %s", &[Arg::S(&name), Arg::S(&m)]);
    for page in gained.chunks(3) {
        gfx::blit(game.scr.page(), &win2, wx2, wy2, false);
        text::large(game, wx2 + 0x22, wy2 + 0x18, &head, 0x2a, 0, 1, 2);
        for (k, &(i, n)) in page.iter().enumerate() {
            let k = k as i32;
            blit(game, slot::ITEM_ICON + i, wx2 + 0x22, wy2 + 0x3c + k * 0x24);
            let iname = game.battle.item(i as u16 + 1).map(|it| to_big5(&it.name)).unwrap_or_default();
            text::large(game, wx2 + 0x52, wy2 + 0x3c + k * 0x24, &iname, 0x24, 0, 1, 2);
            let c = fmt(b"x% 2d", &[Arg::D(n as i32)]);
            text::small(game, wx2 + 0x116, wy2 + 0x3e + k * 0x1e, &c, 0x24, 0, 1, 2);
        }
        gfx::present(game);
        ui::click_to_continue(game, 0xfa);
    }
}

/// One level gained, for [`level_up_window`]: the stats before it and the
/// growth, both in the order HP, MP, ST, attack, defence, reaction, skill,
/// magic, sword.
#[derive(Debug, Clone, Copy)]
pub struct LevelGain {
    pub level: i16,
    pub old: [i16; 9],
    pub gain: [i16; 9],
}

impl LevelGain {
    /// The "old" stats of a unit in [`LevelGain`] order.
    pub fn stats_of(u: &super::super::unit::Unit) -> [i16; 9] {
        [u.hp_max, u.mp_max, u.st_max, u.atk, u.def, u.react, u.skill, u.mag, u.sword]
    }
}

/// Port of FUN_004537da's presentation with FUN_00456549: the level-up
/// window centred on the screen, one page per level gained, 120 cs each.
pub fn level_up_window(game: &mut Game, r: UnitRef, levels: &[LevelGain]) {
    if levels.is_empty() {
        return;
    }
    game.scr.copy_page(0, 1);
    let Some(win) = img(game, slot::LEVEL_WINDOW) else { return };
    let (x, y) = (0x140 - win.width as i32 / 2, 0xf0 - win.height as i32 / 2);
    let lut = game.res().tints[6];
    gfx::window_shadow(game.scr.page(), &lut, x, y, win.width as i32, win.height as i32);
    let name = game.battle.unit(r).name.to_vec();
    let m5 = game.msg(5);
    let labels: [Vec<u8>; 9] =
        [b"HP".to_vec(), b"MP".to_vec(), b"ST".to_vec(), game.msg(7), game.msg(8), game.msg(10), game.msg(9), game.msg(11), game.msg(12)];
    for lg in levels {
        gfx::blit(game.scr.page(), &win, x, y, false);
        let s = fmt(b"%s %s %d", &[Arg::S(&name), Arg::S(&m5), Arg::D(lg.level as i32)]);
        text::large(game, x + 0x1b, y + 0x1f, &s, 0x2a, 0, 1, 2);
        let mut ry = y + 0x40;
        for (k, label) in labels.iter().enumerate() {
            if lg.gain[k] == 0 {
                continue;
            }
            let c = if k == 0 { 0xea } else { 0xeb };
            text::large(game, x + 0x33, ry, label, c, 0, 1, 2);
            large_number(game, x + 0x88, ry, lg.old[k] as i32, 0x68);
            blit(game, slot::ARROW, x + 0xc9, ry + 7);
            large_number(game, x + 0x104, ry, lg.old[k] as i32 + lg.gain[k] as i32, 0x24);
            ry += 0x1a;
        }
        gfx::present(game);
        ui::click_to_continue(game, 0x78);
    }
}

/// The "thinking" line of FUN_0040aadc (UI string 0x3f0 / 4) while an AI
/// unit plans: centred over the unit (window and unit redrawn first) when
/// the player can see it, otherwise at the bottom of the screen; presented.
pub fn ai_thinking(game: &mut Game, r: UnitRef) {
    if !super::super::actions::presenting(game) {
        return;
    }
    let s = game.msg(0x3f0 / 4);
    let len = text::strlen(&s);
    let shown = {
        let b = &game.battle;
        b.army(r.side).side == Side::Player || !b.is_hidden(r.side, b.unit(r))
    };
    let (x, y) = if shown {
        let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
        let (cx, cy) = super::cell_screen(game, ux, uy);
        super::motion::draw_window(game);
        let h = game.battle.unit(r).sprite_h as i32;
        super::draw_unit(game, r, 0);
        (cx + 0x20 - len * 4, cy + 0x20 - h)
    } else {
        (0x140 - len * 4, 0xe6)
    };
    text::small(game, x, y, &s, 0x38, 0x44, 1, 0);
    gfx::present(game);
}

/// FUN_004351ad's drawing: every 0x13 cs the unit steps a frame and is
/// drawn over page 1 by its movement type. True when it stepped.
fn idle_step(game: &mut Game, r: UnitRef, mt: i32) -> bool {
    if game.still_waiting(game.battle.unit(r).anim_timer, 0x13) {
        return false;
    }
    let f = game.battle.unit(r).facing;
    super::step_unit(game, r, f);
    game.scr.copy_page(0, 1);
    game.scr.set_page(0);
    match mt {
        3 => {
            super::draw_ground_shadow(game, r);
            super::motion::draw_lifted(game, r, 4);
        }
        4 => super::motion::draw_flyer(game, r, true),
        5 => super::motion::draw_sword_rider(game, r),
        _ => super::draw_unit(game, r, 0),
    }
    game.battle.unit_mut(r).anim_timer = game.ticks();
    true
}

/// The forecast pause of FUN_00436b24 (an AI attack): for 0x3c cs from
/// `t0`, over the attack range in page 1 (FUN_0040720c), the attacker
/// (idle frames every 0x13 cs, FUN_004351ad), the marker on the target
/// cell (FUN_0044179b) and the forecast window (FUN_00452215).
pub fn ai_attack_pause(game: &mut Game, r: UnitRef, target: (i32, i32), duel: &super::super::combat::Duel, t0: i32) {
    if !super::super::actions::presenting(game) {
        game.wait_until(t0, 0x3c);
        return;
    }
    let mt = {
        let u = game.battle.unit(r).clone();
        game.battle.move_type(&u)
    };
    while game.still_waiting(t0, 0x3c) {
        gfx::palette_cycle(game);
        if idle_step(game, r, mt) {
            continue;
        }
        game.scr.copy_page(0, 1);
        game.scr.set_page(0);
        match mt {
            3 => {
                super::draw_ground_shadow(game, r);
                super::motion::draw_lifted(game, r, 4);
            }
            4 => super::motion::draw_flyer(game, r, true),
            _ => super::draw_unit(game, r, 0),
        }
        super::draw_target_marker(game, target.0, target.1);
        attack_forecast(game, duel);
        gfx::present(game);
    }
}

/// Port of FUN_0046313d (spell effect 0x23): the target's class sheet —
/// window 0xef with its shadow over the unit's column, labels msg
/// 0x18..=0x1d and the class's 兵種, resisted and weak elements (up to three,
/// msg 0x1e when none), innate skill, power rating (FUN_0045781c) and its
/// moves two per row; presented until a click.
pub fn class_sheet(game: &mut Game, r: UnitRef) {
    if !super::super::actions::presenting(game) {
        return;
    }
    let Some(win) = img(game, slot::LEVEL_WINDOW) else { return };
    let (w, h) = (win.width as i32, win.height as i32);
    let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    let (cx, _) = super::cell_screen(game, ux, uy);
    let mut x = cx + (0x20 - w / 2);
    if x < 10 {
        x = 10;
    } else if 0x27f < x + w {
        x = 0x276 - w;
    }
    let y = 0xf0 - h / 2;
    let lut = game.res().tints[6];
    gfx::window_shadow(game.scr.page(), &lut, x, y, w, h);
    gfx::blit(game.scr.page(), &win, x, y, false);
    for k in 0..6 {
        let m = game.msg(0x18 + k);
        text::large(game, x + 0x20, y + 0x20 + 0x20 * k as i32, &m, 0x6c, 0x36, 1, 1);
    }
    let t = game.battle.t();
    let u = game.battle.unit(r).clone();
    let Some(c) = t.kinds.class(u.kind as u16).cloned() else { return };
    let vx = x + 0x70;
    let ut = t.kinds.unit_types.get((c.unit_type as usize).wrapping_sub(1)).map(|s| to_big5(s)).unwrap_or_default();
    text::large(game, vx, y + 0x20, &ut, 0xb2, 0x36, 1, 1);
    for (row, mask) in [(0x40, c.resist_elements), (0x60, c.weak_elements)] {
        if mask == 0 {
            let m = game.msg(0x1e);
            text::large(game, vx, y + row, &m, 0xb2, 0x36, 1, 1);
            continue;
        }
        let (mut n, mut ex) = (0, vx);
        for (i, name) in t.kinds.elements.iter().enumerate() {
            if mask & (1 << (i & 0x1f)) == 0 {
                continue;
            }
            n += 1;
            let s = to_big5(name);
            text::large(game, ex, y + row, &s, 0xb2, 0x36, 1, 1);
            ex += 0xc + text::strlen(&s) * 0xc;
            if n > 2 {
                break;
            }
        }
    }
    let skill = t.kinds.skills.get(c.innate_skill as usize).map(|s| to_big5(s)).unwrap_or_default();
    text::large(game, vx, y + 0x80, &skill, 0xb2, 0x36, 1, 1);
    let rating = game.battle.power(&u, 0, 0);
    text::large(game, vx, y + 0xa0, rating.to_string().as_bytes(), 0xb2, 0x36, 1, 1);
    for (k, a) in c.actions.clone().enumerate() {
        let name = game.battle.action_name_raw(a);
        let name = name.split(|&b| b == 0).next().unwrap_or(&[]).to_vec();
        text::large(game, vx + (k as i32 % 2) * 0x78, y + 0xc0 + (k as i32 / 2) * 0x20, &name, 0xb2, 0x36, 1, 1);
    }
    gfx::present(game);
    ui::click_to_continue(game, 0);
}

/// FUN_00435697's 狀況視窗 part (with that setting on): the unit panel
/// (sysimg 0x8f) at the bottom when the unit's cell is above screen y 0xb8,
/// at the top when below 0x110, with the unit's and its cell's lines
/// (FUN_0045161f, FUN_004273e4); drawn into the current page.
pub fn menu_status(game: &mut Game, r: UnitRef) {
    if !game.cfg.status_window || !super::super::actions::presenting(game) {
        return;
    }
    let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    let (_, sy) = super::cell_screen(game, ux, uy);
    let v = &mut game.battle.view;
    if sy < 0xb8 && v.panel_pos == 1 {
        v.panel_pos = 2;
    } else if 0x110 < sy && v.panel_pos == 2 {
        v.panel_pos = 1;
    }
    let pos = v.panel_pos;
    blit(game, slot::STATUS_UNIT, 0, panel_y(pos));
    unit_info(game, r, pos);
    terrain_info(game, ux, uy, pos, Some(r));
}

/// FUN_0044f53e's window when `r` opens a chest holding item index `item`
/// (0-based): window 0xd1 with its shadow above the unit (below it when
/// there is no room; kept on screen), "<name> <msg 0x26>", the item's icon
/// (0x1ed + item), name and "x  1"; presented until a click. The chest
/// leaves the map surface with the treasure state (FUN_004285de).
pub fn treasure_window(game: &mut Game, r: UnitRef, item: usize) {
    if !super::super::actions::presenting(game) {
        return;
    }
    let Some(win) = img(game, slot::ITEM_WINDOW) else { return };
    let (w, h) = (win.width as i32, win.height as i32);
    let (ux, uy, sh) = {
        let u = game.battle.unit(r);
        (u.x, u.y, u.sprite_h as i32)
    };
    let (cx, cy) = super::cell_screen(game, ux, uy);
    let mut y = (cy + 0x20 - sh) - h;
    if y < 0 {
        y = cy + 0x20;
    }
    if 0x1d6 < y + h {
        y = 0x1d6 - h;
    }
    let mut x = (cx + 0x20) - w / 2;
    if x < 10 {
        x = 10;
    } else if 0x27f < x + w {
        x = 0x27f - w;
    }
    let lut = game.res().tints[6];
    gfx::window_shadow(game.scr.page(), &lut, x, y, w, h);
    gfx::blit(game.scr.page(), &win, x, y, false);
    let name = game.battle.unit(r).name.to_vec();
    let m = game.msg(0x26);
    let head = fmt(b"%s %s", &[Arg::S(&name), Arg::S(&m)]);
    text::large(game, x + 0x26, y + 0x1e, &head, 0x2a, 0, 1, 2);
    let iname = game.battle.item_name_raw(item as u16 + 1);
    text::large(game, x + 0x56, y + 0x3e, &iname, 0x4a, 0, 1, 2);
    blit(game, slot::ITEM_ICON + item, x + 0x26, y + 0x3e);
    text::large(game, x + 0x114, y + 0x3e, b"x  1", 0x4a, 0, 1, 2);
    gfx::present(game);
    ui::click_to_continue(game, 0);
}

/// Port of FUN_0044e75e: the rest recovery rising over unit `r` — "HP n"
/// (0x68), "MP n", "ST n" (0x26), only the non-zero ones stacked 0x18
/// apart and centred by the first; 16 frames of 3 cs over page 1, 4 px up
/// each.
pub fn recovery_popup(game: &mut Game, r: UnitRef, hp: i32, mp: i32, st: i32) {
    if !super::super::actions::presenting(game) {
        return;
    }
    let (ux, uy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    let (mut x, mut y) = super::cell_screen(game, ux, uy);
    let (sh, sm, ss) = (fmt(b"HP %d", &[Arg::D(hp)]), fmt(b"MP %d", &[Arg::D(mp)]), fmt(b"ST %d", &[Arg::D(st)]));
    let first = if hp != 0 { &sh } else if mp != 0 { &sm } else { &ss };
    x += 0x20 - text::strlen(first) * 8;
    y -= 0x40;
    let dm = if hp != 0 { 0x18 } else { 0 };
    let ds = dm + if mp != 0 { 0x18 } else { 0 };
    let mut t = game.ticks();
    for _ in 0..0x10 {
        gfx::palette_cycle(game);
        game.scr.copy_page(0, 1);
        if hp > 0 {
            text::large(game, x, y, &sh, 0x68, 0, 1, 2);
        }
        if mp > 0 {
            text::large(game, x, y + dm, &sm, 0x26, 0, 1, 2);
        }
        if st > 0 {
            text::large(game, x, y + ds, &ss, 0x26, 0, 1, 2);
        }
        game.wait_until(t, 3);
        gfx::present(game);
        t = game.ticks();
        y -= 4;
    }
    game.scr.copy_page(0, 1);
}
