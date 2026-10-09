//! Party screens used outside battle: equipment and items (`FUN_00447490`),
//! the status list (`FUN_0042f620`), auto-equip (`FUN_0044bcb0`), the
//! member panel of the church lists (`FUN_004476d1`) and the class-change
//! screens (`FUN_00453986`, `FUN_00454368`, `FUN_00454594`, `FUN_00454aea`).

use std::rc::Rc;

use anyhow::Result;
use s2data::gfx_portrait::PortraitKind;
use s2data::gfx_unit::UnitSprites;
use s2data::sprite::Image;

use super::town::large_number;
use crate::battle::rules;
use crate::battle::unit::flag;
use crate::battle::{Side, UnitRef};
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// Arrow between old and new values (sysimg 148, `DAT_00493a98`).
const ARROW: usize = 148;

/// Port of `FUN_00447490(army, mode, unit, fade)`: the equipment (mode 0)
/// or item (mode 1) screen. `unit` = `None` browses every member (town);
/// `Some(r)` opens on that member with the member list dimmed (battle).
/// Returns 1 when something changed.
pub fn equipment(game: &mut Game, mode: i32, unit: Option<UnitRef>, fade: bool) -> Result<i32> {
    let list: Vec<UnitRef> = super::stage::units_of(game, Side::Player)
        .into_iter()
        .filter(|&r| {
            let u = game.battle.unit(r);
            if unit.is_none() {
                u.npc != 1 && u.flags & (flag::DEAD | flag::RESERVE | flag::SUMMON) == 0
            } else {
                u.flags & (flag::DEAD | flag::RESERVE) == 0
            }
        })
        .collect();
    let n = list.len();
    if n == 0 {
        return Ok(0);
    }
    let (mut top, mut sel) = (0usize, 0usize);
    if let Some(r) = unit {
        let Some(i) = list.iter().position(|&m| m == r) else { return Ok(0) };
        if n < 0xb {
            sel = i;
        } else if n < i + 10 {
            top = n - 10;
            sel = i - top;
        } else {
            top = i;
        }
    }
    let ml = MemberList { list, top, sel, locked: unit.is_some() };
    if mode == 0 {
        equip_screen(game, ml, fade)
    } else {
        item_screen(game, ml, fade)
    }
}

/// The member list of the party screens (`param_2..param_5` of `FUN_00447d3f`).
struct MemberList {
    list: Vec<UnitRef>,
    top: usize,
    sel: usize,
    /// Opened for one unit in battle: the list is dimmed and inactive.
    locked: bool,
}

impl MemberList {
    fn cur(&self) -> UnitRef {
        self.list[self.top + self.sel]
    }

    /// The top panel on the current page: `FUN_004476d1`, the scroll thumb,
    /// and the dimming when locked.
    fn draw(&self, game: &mut Game) {
        unit_info_panel(game, 0, 0, &self.list, self.top, self.sel);
        let n = self.list.len() as i32;
        let mut ty = 0x28;
        if self.top + self.sel > 0 {
            ty += ((self.top + 1 + self.sel) as i32 * 0x87) / n;
        }
        if let Ok(t) = game.res().ui_image(THUMB) {
            gfx::blit(game.scr.page(), &t, 0xda, ty, false);
        }
        if self.locked {
            let lut = game.res().tints[6];
            let p = game.scr.page();
            for px in p.pixels[..0x25300].iter_mut() {
                *px = lut[*px as usize];
            }
        }
    }

    /// Left click on the member list or its scrollbar. `Some(changed)` when
    /// the click was on them (a change rebuilds the screen; `false` = the
    /// caller waits for the button release), `None` otherwise.
    /// `quirk_sel` is the item screen's down-arrow test, which compares its
    /// own item row instead of the member row (`FUN_0044957b`).
    fn click(&mut self, game: &mut Game, x: i32, y: i32, quirk_sel: Option<usize>) -> Option<bool> {
        if self.locked {
            return None;
        }
        let n = self.list.len();
        if 0xf9 < x && x < 0x1be && 0x15 < y && y < 0xde {
            let row = ((y - 0x16) / 0x14) as usize;
            if row != self.sel && self.top + row < n {
                self.sel = row;
                return Some(true);
            }
            return Some(false);
        }
        if 0xd9 < x && x < 0xee && 0xe < y && y < 0xe4 {
            if y <= 0x26 {
                if self.sel >= 1 {
                    self.sel -= 1;
                } else if self.top > 0 {
                    self.top -= 1;
                } else {
                    return Some(false);
                }
                game.sleep_cs(8);
                return Some(true);
            }
            if y < 0xcd {
                if n < 0xb || !(0x28..=0xcb).contains(&y) {
                    return Some(false);
                }
                let mut t = (n as i32 * (y - 0x28)) / 0xa3;
                if (n as i32) < t + 10 {
                    t = n as i32 - 10;
                }
                self.top = t as usize;
                self.sel = if y < 0xaf { 0 } else { 9 };
                return Some(true);
            }
            if n - 1 <= self.sel + self.top {
                return Some(false);
            }
            if quirk_sel.unwrap_or(self.sel) < 9 {
                self.sel += 1;
            } else {
                self.top += 1;
            }
            game.sleep_cs(8);
            return Some(true);
        }
        None
    }
}

/// Small scroll thumb (sysimg 147, `DAT_00493a94`).
const THUMB: usize = 147;

/// Port of `FUN_00448aab(army, &n, lists, slot)`: owned items for equipment
/// slot `slot` (0 = weapons, kinds 1..=6; k = kind k+6), 1-based ids.
fn slot_items(game: &Game, slot: usize) -> Vec<u16> {
    let inv = &game.battle.army(Side::Player).inventory;
    let n = game.res().item_count();
    (0..n)
        .filter(|&i| {
            let k = game.res().item_rec(i + 1).get(0xe).copied().unwrap_or(0) as i8 as i32;
            inv.get(i).copied().unwrap_or(0) != 0 && (k == slot as i32 + 6 || (slot == 0 && 0 < k && k < 7))
        })
        .map(|i| i as u16 + 1)
        .collect()
}

fn icons(game: &Game, ids: &[u16]) -> Vec<Option<Image>> {
    ids.iter().map(|&id| game.res().ui_image(id as usize + 0x1ec).ok()).collect()
}

/// The 5-row item list with its thumb and up/down logic, shared by the
/// equipment (`FUN_00447d3f`) and item (`FUN_0044957b`) screens.
struct ItemList {
    ids: Vec<u16>,
    icons: Vec<Option<Image>>,
    top: usize,
    sel: usize,
}

impl ItemList {
    fn new(game: &Game, ids: Vec<u16>) -> ItemList {
        let icons = icons(game, &ids);
        ItemList { ids, icons, top: 0, sel: 0 }
    }

    /// After the list shrank: keep the selection inside it.
    fn clamp(&mut self) {
        if self.ids.len() <= self.top + self.sel {
            if self.top < 1 {
                if self.sel > 0 {
                    self.sel -= 1;
                }
            } else {
                self.top -= 1;
            }
        }
    }
}

/// Port of `FUN_00448cdc`: the owned items for the slot (unusable ones dim)
/// and, for a usable selection, the stat comparison (`FUN_00448e63`).
fn draw_slot_items(game: &mut Game, r: UnitRef, y: i32, slot: usize, il: &ItemList) {
    let u = game.battle.unit(r).clone();
    let inv = game.battle.army(Side::Player).inventory.clone();
    for k in 0..5usize {
        let Some(&id) = il.ids.get(il.top + k) else { break };
        let mut c = if k != il.sel { 0x34 } else { 0xf2 };
        if !game.battle.item_equippable(&u, id) {
            c = 0x2d;
        }
        if let Some(i) = &il.icons[il.top + k] {
            gfx::blit(game.scr.page(), i, 0x10f, y + 0x34 + k as i32 * 0x23, false);
        }
        let name = game.res().item_name(id as usize).to_vec();
        text::large(game, 0x131, y + 0x3b + k as i32 * 0x23, &name, c, 0, 1, 2);
        let cnt = inv.get(id as usize - 1).copied().unwrap_or(0) as i32;
        large_number(game, 0x1ae, y + 0x3b + k as i32 * 0x23, cnt, c, 0, 1, 2, 0);
    }
    let id = il.ids[il.top + il.sel];
    if game.battle.item_equippable(&u, id) {
        super::shop::compare_panel(game, 0, y, r, slot + 1, id as i32);
    }
}

/// Port of `FUN_00447d3f`: the equipment screen.
fn equip_screen(game: &mut Game, mut ml: MemberList, fade: bool) -> Result<i32> {
    let top_panel = game.res().ui_image(0xd9)?;
    game.scr.set_page(2);
    gfx::blit(game.scr.page(), &top_panel, 0, 0, false);
    let panel = game.res().ui_image(0xda)?;
    let py = 0x1e0 - panel.height as i32;
    gfx::blit(game.scr.page(), &panel, 0, py, false);
    game.scr.set_page(0);
    let mut lists: Vec<ItemList> = (0..6).map(|k| ItemList::new(game, slot_items(game, k))).collect();
    let thumb = game.res().ui_image(THUMB).ok();
    let (mut full, mut lower, mut redraw, mut moved, mut fade_pending) = (true, false, false, false, true);
    let mut slot = 0usize;
    let mut changed = 0;
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if full {
            game.scr.copy_page(1, 2);
            game.scr.set_page(1);
            ml.draw(game);
            game.scr.set_page(0);
            lower = true;
            slot = 0;
            lists[0].top = 0;
            lists[0].sel = 0;
        }
        if lower {
            game.scr.set_page(1);
            if !full {
                gfx::blit(game.scr.page(), &panel, 0, py, false);
            }
            super::shop::equipment_slots(game, ml.cur(), 0x3e, py + 0x12);
            let t = game.msg(0x78);
            text::large(game, 0x1f7, py + 0x25, &t, 0x2d, 0, 1, 2);
            game.scr.set_page(0);
            lower = false;
            full = false;
            redraw = true;
        }
        let r = ml.cur();
        let cur = game.battle.unit(r).equip[slot];
        if redraw {
            game.scr.copy_page(0, 1);
            let off = game.msg(0x4b);
            text::large(game, 0x115, py + 0x14, &off, if cur == 0 { 0x2d } else { 0xf2 }, 0, 1, 2);
            let s = slot as i32 * 0x23;
            gfx::rect(game.scr.page(), 0x3e, py + 0x14 + s, 0xe9, py + 0x34 + s, 0x3c, false);
            let il = &lists[slot];
            if !il.ids.is_empty() {
                let il = ItemList { ids: il.ids.clone(), icons: il.icons.clone(), top: il.top, sel: il.sel };
                draw_slot_items(game, r, py, slot, &il);
            }
            let il = &lists[slot];
            let mut ty = py + 0x4c;
            if !il.ids.is_empty() && il.top + il.sel > 0 {
                ty += ((il.top + 1 + il.sel) as i32 * 100) / il.ids.len() as i32;
            }
            if let Some(t) = &thumb {
                gfx::blit(game.scr.page(), t, 0xef, ty, false);
            }
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
            if fade && fade_pending {
                let m = game.res().master.clone();
                gfx::fade(game, &m, 0, -4);
                fade_pending = false;
            }
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if !game.get_event(0xffff) || !(1..=0x20).contains(&game.ev.kind) {
            continue;
        }
        let e = game.ev;
        match e.kind {
            ev::KEY_DOWN | ev::KEY_REPEAT => {
                if e.scan() == scan::ESC {
                    game.wait_event(ev::KEY_UP);
                    break;
                }
            }
            ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                if e.kind == ev::MOUSE_MOVE {
                    moved = true;
                }
                if e.mods & ev::M_RBUTTON != 0 {
                    game.flush_events(ev::MOUSE_DOWN | ev::MOUSE_UP);
                    break;
                }
                let (x, y) = (e.x, e.y);
                let n_items = lists[slot].ids.len();
                // The item list: hover selects, a click on the selection equips.
                if 0x10c < x && x < 0x1c6 && py + 0x34 <= y && y < py + 0xe3 {
                    let row = ((y - py - 0x34) / 0x23) as usize;
                    let il = &lists[slot];
                    if il.top + row < n_items && game.battle.item_equippable(game.battle.unit(r), il.ids[il.top + row]) {
                        if row == il.sel {
                            if e.mods & ev::M_LBUTTON != 0 {
                                game.wait_event(ev::MOUSE_UP);
                                let id = il.ids[il.top + il.sel];
                                if cur > 0 {
                                    game.battle.unequip_slot(r, slot + 1, false);
                                }
                                game.battle.equip_item(r, id);
                                let (t, s) = (lists[slot].top, lists[slot].sel);
                                lists[slot] = ItemList::new(game, slot_items(game, slot));
                                lists[slot].top = t;
                                lists[slot].sel = s;
                                lists[slot].clamp();
                                lower = true;
                                changed = 1;
                            }
                        } else {
                            lists[slot].sel = row;
                            redraw = true;
                        }
                        continue;
                    }
                }
                if e.mods & ev::M_LBUTTON == 0 {
                    continue;
                }
                let mut release = true;
                if !(0x3e..=0xeb).contains(&x) || y < py + 0x16 || py + 0xe4 < y {
                    let il = &mut lists[slot];
                    if n_items > 0 && 0xee < x && x < 0x103 && py + 0x33 <= y && y <= py + 0xe6 {
                        if y <= py + 0x4a {
                            let mut moved_list = false;
                            if il.sel >= 1 {
                                il.sel -= 1;
                                moved_list = true;
                            } else if il.top > 0 {
                                il.top -= 1;
                                moved_list = true;
                            }
                            if moved_list {
                                redraw = true;
                                game.sleep_cs(8);
                                release = false;
                            }
                        } else if y < py + 0xcf {
                            if py + 0x4c <= y && y <= py + 0xcd && n_items >= 6 {
                                let mut t = (n_items as i32 * (y - py - 0x4c)) / 0x81;
                                if (n_items as i32) < t + 5 {
                                    t = n_items as i32 - 5;
                                }
                                il.top = t as usize;
                                il.sel = if y < py + 0xaf { 0 } else { 4 };
                                redraw = true;
                                release = false;
                            }
                        } else if il.sel + il.top < n_items - 1 {
                            if il.sel < 4 {
                                il.sel += 1;
                            } else {
                                il.top += 1;
                            }
                            redraw = true;
                            game.sleep_cs(8);
                            release = false;
                        }
                    } else if let Some(ch) = ml.click(game, x, y, None) {
                        if ch {
                            full = true;
                            release = false;
                        }
                    } else if cur > 0 && 0x105 < x && x < 0x1cc && py + 0x11 <= y && y <= py + 0x2d {
                        // 卸下: back to the party's items.
                        game.battle.unequip_slot(r, slot + 1, false);
                        lists[slot] = ItemList::new(game, slot_items(game, slot));
                        lower = true;
                        changed = 1;
                    }
                } else {
                    for k in 0..6usize {
                        let y0 = py + 0x16 + k as i32 * 0x23;
                        if y0 <= y && y <= y0 + 0x1f {
                            if k != slot {
                                slot = k;
                                redraw = true;
                                lists[slot].top = 0;
                                lists[slot].sel = 0;
                            }
                            break;
                        }
                    }
                }
                if release {
                    game.wait_event(ev::MOUSE_UP);
                }
            }
            _ => {}
        }
    }
    Ok(changed)
}

/// Port of `FUN_00449e4f(army, unit, &n, &ids)`: usable items: (in battle)
/// the unit's equipped items with a use, then owned consumables (kind 12).
fn usable_items(game: &Game, r: UnitRef) -> Vec<u16> {
    let rec = |id: usize| game.res().item_rec(id).to_vec();
    let mut out = Vec::new();
    if !game.campaign.between_stages {
        for &id in game.battle.unit(r).equip.iter() {
            if id != 0 && rec(id as usize).get(0xf).copied().unwrap_or(0) != 0 {
                out.push(id);
            }
        }
    }
    let inv = &game.battle.army(Side::Player).inventory;
    for i in 0..game.res().item_count() {
        let rr = rec(i + 1);
        if inv.get(i).copied().unwrap_or(0) != 0 && rr.get(0xf).copied().unwrap_or(0) != 0 && rr.get(0xe).copied() == Some(0xc) {
            out.push(i as u16 + 1);
        }
    }
    out
}

/// Port of `FUN_0044a106`: the usable items and the selection's help text.
fn draw_usable(game: &mut Game, r: UnitRef, y: i32, il: &ItemList) {
    let u = game.battle.unit(r).clone();
    let inv = game.battle.army(Side::Player).inventory.clone();
    for k in 0..5usize {
        let Some(&id) = il.ids.get(il.top + k) else { break };
        let mut c = if k != il.sel { 0x34 } else { 0xf2 };
        if !game.battle.item_equippable(&u, id) {
            c = 0x2d;
        }
        if let Some(i) = &il.icons[il.top + k] {
            gfx::blit(game.scr.page(), i, 0x36, y + 0x25 + k as i32 * 0x23, false);
        }
        let name = game.res().item_name(id as usize).to_vec();
        text::large(game, 0x58, y + 0x2c + k as i32 * 0x23, &name, c, 0, 1, 2);
        let cnt = inv.get(id as usize - 1).copied().unwrap_or(0) as i32;
        if cnt != 0 {
            large_number(game, 0xd5, y + 0x2c + k as i32 * 0x23, cnt, c, 0, 1, 2, 0);
        }
    }
    let id = il.ids[il.top + il.sel];
    if game.battle.item_equippable(&u, id) {
        let head = game.res().item_help_text(0).to_vec();
        text::large(game, 0x1be - head.len() as i32 * 6, y + 0x1b, &head, 0x2d, 0, 1, 2);
        let help = game.res().item_help_text(id as usize).to_vec();
        text::block(game, 0x111, y + 0x3a, &help, 0x2d, 0, 1, 2, 7, 0xc, 0x1c);
    }
}

/// Port of `FUN_0044a300(army, unit, id)` between stages: confirm, apply
/// (`Battle::use_item`), a consumable is used up. Returns 1 when used.
fn use_item(game: &mut Game, r: UnitRef, id: u16) -> i32 {
    let rec = game.res().item_rec(id as usize).to_vec();
    let (kind, usage) = (rec.get(0xe).copied().unwrap_or(0), rec.get(0xf).copied().unwrap_or(0));
    if !game.campaign.between_stages {
        // The in-battle branches (aiming FUN_004067e1, summons FUN_0045fdbf,
        // the use animation) are the battle menu's.
        return crate::battle::cast::use_item_in_battle(game, r, id);
    }
    if usage == 2 {
        return 0;
    }
    let q = game.msg(0x50);
    if !crate::ui::yes_no(game, 200, 400, true, &q, false, false) {
        return 0;
    }
    let mut hooks = Learned::default();
    game.battle.use_item(r, id, true, &mut hooks);
    // FUN_0044bfb2's per-spell window for items 0x41/0x42 (spell books).
    for (u, spells, draw_unit) in hooks.learned {
        crate::battle::hooks::show_learned(game, u, &spells, draw_unit);
    }
    if kind == 0xc {
        let inv = &mut game.battle.army_mut(Side::Player).inventory;
        if let Some(c) = inv.get_mut(id as usize - 1) {
            *c = c.wrapping_sub(1);
        }
    }
    1
}

/// Port of `FUN_0044957b`: the item screen.
fn item_screen(game: &mut Game, mut ml: MemberList, fade: bool) -> Result<i32> {
    let mut il = ItemList::new(game, usable_items(game, ml.cur()));
    if il.ids.is_empty() {
        return Ok(0);
    }
    let top_panel = game.res().ui_image(0xd9)?;
    let panel = game.res().ui_image(0xe5)?;
    let py = 0x1e0 - panel.height as i32;
    let thumb = game.res().ui_image(THUMB).ok();
    let (mut full, mut redraw, mut moved, mut fade_pending) = (true, false, false, fade);
    let mut result = 0;
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if full {
            game.scr.set_page(1);
            gfx::blit(game.scr.page(), &top_panel, 0, 0, false);
            gfx::blit(game.scr.page(), &panel, 0, py, false);
            ml.draw(game);
            game.scr.set_page(0);
            redraw = true;
            full = false;
        }
        let r = ml.cur();
        if redraw {
            game.scr.copy_page(0, 1);
            if !il.ids.is_empty() {
                draw_usable(game, r, py, &il);
            }
            let mut ty = py + 0x2a;
            if !il.ids.is_empty() && il.top + il.sel > 0 {
                ty += ((il.top + 1 + il.sel) as i32 * 0x86) / il.ids.len() as i32;
            }
            if let Some(t) = &thumb {
                gfx::blit(game.scr.page(), t, 0xf, ty, false);
            }
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
            if fade_pending {
                let m = game.res().master.clone();
                gfx::fade(game, &m, 0, -4);
                fade_pending = false;
            }
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if !game.get_event(0xffff) || !(1..=0x20).contains(&game.ev.kind) {
            continue;
        }
        let e = game.ev;
        match e.kind {
            ev::KEY_DOWN | ev::KEY_REPEAT => {
                if e.scan() == scan::ESC {
                    game.wait_event(ev::KEY_UP);
                    break;
                }
            }
            ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                if e.kind == ev::MOUSE_MOVE {
                    moved = true;
                }
                let (x, y) = (e.x, e.y);
                let n_items = il.ids.len();
                if (0x2d..=0xf4).contains(&x) && py + 0x25 <= y && y < py + 0xd4 {
                    let row = ((y - py - 0x25) / 0x23) as usize;
                    if il.top + row < n_items && game.battle.item_equippable(game.battle.unit(r), il.ids[il.top + row]) {
                        if row != il.sel {
                            il.sel = row;
                            redraw = true;
                            continue;
                        }
                        if e.mods & ev::M_LBUTTON != 0 {
                            let id = il.ids[il.top + il.sel];
                            game.wait_event(ev::MOUSE_UP);
                            if use_item(game, r, id) == 1 {
                                if !ml.locked {
                                    full = true;
                                    let (t, s) = (il.top, il.sel);
                                    il = ItemList::new(game, usable_items(game, ml.cur()));
                                    il.top = t;
                                    il.sel = s;
                                    il.clamp();
                                    if il.ids.is_empty() {
                                        break;
                                    }
                                } else {
                                    result = 1;
                                    break;
                                }
                            } else {
                                full = true;
                            }
                            continue;
                        }
                    }
                }
                if e.mods & ev::M_LBUTTON == 0 {
                    if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        break;
                    }
                    continue;
                }
                let mut release = true;
                if n_items >= 1 && (0xf..=0x22).contains(&x) && py + 0x11 <= y && y <= py + 0xe6 {
                    if y <= py + 0x28 {
                        if il.sel >= 1 {
                            il.sel -= 1;
                            redraw = true;
                        } else if il.top > 0 {
                            il.top -= 1;
                            redraw = true;
                        }
                        if redraw {
                            game.sleep_cs(8);
                            release = false;
                        }
                    } else if y < py + 0xcf {
                        if py + 0x2a <= y && y <= py + 0xcd && n_items >= 6 {
                            let mut t = ((y - py - 0x2a) * n_items as i32) / 0xa3;
                            if (n_items as i32) < t + 5 {
                                t = n_items as i32 - 5;
                            }
                            il.top = t as usize;
                            il.sel = if y < py + 0xaf { 0 } else { 4 };
                            redraw = true;
                            release = false;
                        }
                    } else if il.sel + il.top < n_items - 1 {
                        if il.sel < 4 {
                            il.sel += 1;
                        } else {
                            il.top += 1;
                        }
                        redraw = true;
                        game.sleep_cs(8);
                        release = false;
                    }
                } else if let Some(true) = ml.click(game, x, y, Some(il.sel)) {
                    full = true;
                    il.top = 0;
                    il.sel = 0;
                    release = false;
                }
                if release {
                    game.wait_event(ev::MOUSE_UP);
                }
            }
            _ => {}
        }
    }
    game.flush_events(0x1f);
    Ok(result)
}

/// Port of `FUN_0042f620(town)`: the party status list (see `flow::status`).
pub fn status_list(game: &mut Game, town: bool) -> Result<i32> {
    super::status::status_list(game, town)
}

/// Port of `FUN_0044bcb0(army)`: auto-equip every member.
pub fn auto_equip_all(game: &mut Game) {
    let n = game.battle.army(Side::Player).units.len();
    for i in 0..n {
        game.battle.auto_equip(UnitRef::new(Side::Player, i));
    }
}

/// The unit sprite banks (PRG 1+2, CEL 4+5): the view's when a stage was
/// set up, otherwise read once and kept (between stages after a load).
fn sprite_banks(game: &mut Game) -> Option<(Rc<UnitSprites>, Rc<UnitSprites>)> {
    if game.ui.unit_sprites.is_none() {
        let arcs = &game.data.arcs;
        let banks = UnitSprites::load_normal(arcs).ok().zip(UnitSprites::load_summon(arcs).ok());
        game.ui.unit_sprites = banks.map(|(a, b)| (Rc::new(a), Rc::new(b)));
    }
    game.ui.unit_sprites.clone()
}

/// `FUN_00452898`: the army's portrait for the unit, loaded by `FUN_0043fea5`
/// from its character id (generic units: the class's sprite set).
pub(crate) fn portrait(game: &Game, r: UnitRef) -> Option<Image> {
    let u = game.battle.unit(r);
    let id = if (1..200).contains(&u.char_id) {
        u.char_id as usize
    } else {
        game.battle.tables.as_ref()?.kinds.class(u.kind as u16)?.sprite_set as usize
    };
    game.res().portraits.get(id, PortraitKind::Battle).ok()
}

/// The frame `FUN_0045292a` draws for a unit. Between stages (`DAT_0049466c`)
/// its decoded current frame (`unit+0xad`; without one in the view, the frame
/// a load decodes: `FUN_0043e83e` → `FUN_0043f96a` starts the action of the
/// facing byte and `FUN_0043a34f` decodes its first frame); in battle the
/// first frame of its sprite type. In battle a unit of the enemy army whose
/// class is recoloured (`FUN_0043c0dc`) is drawn through tint table 0xb
/// (`FUN_00470259`).
pub(crate) fn unit_frame(game: &mut Game, r: UnitRef) -> Option<Image> {
    let between = game.campaign.between_stages;
    let cached = game.battle.view.units[r.side.index()].get(r.idx).and_then(|g| g.sprite.clone()).filter(|_| between);
    let u = game.battle.unit(r);
    let (summon, kind, action) = (u.flags & flag::SUMMON != 0, u.kind, u.facing as usize);
    let mut img = match cached {
        Some(img) => img,
        None => {
            let (normal, summons) = sprite_banks(game)?;
            let (bank, id) = if summon { (summons, (kind as usize).checked_sub(0x70)?) } else { (normal, kind as usize) };
            let n = if between {
                bank.unit_type(id).ok()?.actions.get(action).and_then(|a| a.first().copied()).unwrap_or(1).max(1)
            } else {
                1
            };
            bank.frame(id, n as usize).ok()?.0
        }
    };
    let enemy = game.battle.army(r.side).side != Side::Player;
    if !game.campaign.between_stages && enemy && crate::battle::view::unit::enemy_recolor(kind) {
        let lut = game.res().tints[0xb];
        for p in img.pixels.iter_mut().filter(|p| **p != 0) {
            *p = lut[*p as usize];
        }
    }
    Some(img)
}

/// Port of `FUN_0045292a(army, unit, x, y)`: the unit's sprite standing on
/// (x, y). `x - ((*(short *)img + 1) >> 1)`, `y - (*(short *)(img + 2) + 1)`:
/// the header holds size - 1, so half the width left and the full height up.
pub(crate) fn draw_unit(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    if let Some(img) = unit_frame(game, r) {
        let (w, h) = (img.width as i32, img.height as i32);
        gfx::blit(game.scr.page(), &img, x - (w >> 1), y - h, false);
    }
}

/// Port of `FUN_004476d1(army, x, y, list, n, top, sel)`: the selected
/// member's portrait, sprite and stats, and a ten-row name/class list.
pub fn unit_info_panel(game: &mut Game, x: i32, y: i32, list: &[UnitRef], top: usize, sel: usize) {
    let r = list[top + sel];
    let u = game.battle.unit(r).clone();
    if let Some(face) = portrait(game, r) {
        gfx::blit(game.scr.page(), &face, x + 0x1b, y + 0x1e, false);
    }
    draw_unit(game, r, x + 0x221, y + 0x8b);
    let summon = |f: u32| f & flag::SUMMON != 0;
    if !summon(u.flags) {
        let name = s2data::text::cstr(&u.name).to_vec();
        text::large(game, x + 0x48 - name.len() as i32 * 6, y + 0xa5, &name, 0xaa, 0, 1, 2);
    }
    let class = game.res().class_name(u.kind as usize).to_vec();
    text::large(game, x + 0x48 - class.len() as i32 * 6, y + 0xc3, &class, 0xaa, 0, 1, 2);
    let hp = if u.flags & flag::DEAD == 0 { u.hp } else { 0 };
    let rows: [(Vec<u8>, i16); 10] = [
        (b"HP".to_vec(), hp),
        (b"MP".to_vec(), u.mp),
        (b"ST".to_vec(), u.st),
        (b"AP".to_vec(), u.ap),
        (game.msg(7), u.atk_cur),
        (game.msg(8), u.def_cur),
        (game.msg(10), u.react),
        (game.msg(11), u.mag_cur),
        (game.msg(9), u.skill_cur),
        (game.msg(12), u.sword),
    ];
    for (k, (label, v)) in rows.iter().enumerate() {
        let ry = y + 0x14 + k as i32 * 0x14;
        text::small(game, x + 0x89, ry, label, 0x38, 0x31, 1, 0);
        text::number(game, x + 0xaf, ry, *v as i32, 0x69, 0, 1, 2, 0);
    }
    for k in 0..10usize {
        let Some(&m) = list.get(top + k) else { break };
        let (cn, cc) = if k == sel { (0x38, 0x38) } else { (0xb1, 0x8c) };
        let mu = game.battle.unit(m).clone();
        let ry = y + 0x18 + k as i32 * 0x14;
        if !summon(mu.flags) {
            let name = s2data::text::cstr(&mu.name).to_vec();
            text::small(game, x + 0x100, ry, &name, cn, 0, 1, 2);
        }
        let class = game.res().class_name(mu.kind as usize).to_vec();
        text::small(game, x + 0x15d, ry, &class, cc, 0, 1, 2);
    }
    let (mut lx, mut ly) = (x + 0x1d1, y + 0x91);
    let schools = game.res().school_names.clone();
    for (k, name) in schools.iter().enumerate() {
        if u.learned.get(k).copied().unwrap_or(0) != 0 {
            text::small(game, lx, ly, name, 0x38, 0, 1, 2);
            ly += 0x14;
            if y + 0xcd < ly {
                ly = y + 0x91;
                lx += 0x33;
            }
        }
    }
}

/// Port of `FUN_00443a4e`: three frames of each target class's sprite
/// (the first, then the two the selected card alternates between).
fn class_frames(game: &mut Game, classes: &[u16]) -> Vec<[Option<Image>; 3]> {
    let banks = match game.battle.view.assets.as_ref() {
        Some(_) => None,
        None => sprite_banks(game),
    };
    let bank: &UnitSprites = match (&game.battle.view.assets, &banks) {
        (Some(a), _) => &a.sprites.normal,
        (None, Some((n, _))) => n,
        _ => return classes.iter().map(|_| [None, None, None]).collect(),
    };
    classes
        .iter()
        .map(|&c| {
            let mut i = c as usize - 1;
            if bank.types.len() <= i {
                i = 0;
            }
            let first = bank.types[i].first_frame;
            [0usize, 4, 5].map(|d| bank.frame_abs(first + d).ok().map(|f| f.0))
        })
        .collect()
}

/// Card centre of target `i` (`FUN_00454368`: 0x91 apart; the animated
/// frame of `FUN_00453986` steps 0x92).
fn card_origin(i: usize, step: i32) -> (i32, i32) {
    if i < 4 {
        (0x65 + i as i32 * step, 0x40)
    } else {
        (0x65 + (i as i32 - 4) * step, 0x10e)
    }
}

/// Port of `FUN_00454368(sel, n, classes, frames)`: the target cards on page 1.
fn class_cards(game: &mut Game, sel: usize, classes: &[u16], frames: &[[Option<Image>; 3]]) {
    game.scr.set_page(1);
    for (i, &c) in classes.iter().enumerate() {
        let (cx, cy) = card_origin(i, 0x91);
        let name = game.res().class_name(c as usize).to_vec();
        let nx = cx - name.len() as i32 * 6;
        if i == sel {
            text::large(game, nx, cy, &name, 0x28, 0, 1, 2);
            gfx::rect(game.scr.page(), cx - 0x41, cy - 8, cx + 0x41, cy + 0xc0, 0x28, false);
            let n = game.res().class_rec(c as usize).get(0x22).copied().unwrap_or(0);
            let hint = if n == 0 { game.msg(0xfe) } else { super::shop::format_msg(&game.msg(0xff), &[super::shop::FmtArg::D(n as i32)]) };
            text::large(game, 0x1ea, 0xe, &hint, 0x28, 0, 1, 2);
        } else {
            text::large(game, nx, cy, &name, 0x33, 0, 1, 2);
            if let Some(f) = &frames[i][0] {
                let (w, h) = (f.width as i32, f.height as i32);
                gfx::blit(game.scr.page(), f, cx - w / 2, cy + 0xbe - h, false);
            }
        }
    }
    game.scr.set_page(0);
}

/// Port of `FUN_00454aea(unit, class, x, y, eligible)`: old and new stats,
/// the growth over ten levels and the schools the class may study.
fn class_detail(game: &mut Game, r: UnitRef, class: u16, x: i32, y: i32) -> [bool; 11] {
    let t = game.battle.t();
    let ctx = rules::Ctx::new(&t, game.battle.difficulty, game.battle.vs);
    // FUN_00455b8e: ten levels of growth; then FUN_0042345b at level 1 (both roll the RNG).
    let mut ten = [0i32; 9];
    for lv in 1..=10 {
        let g = rules::growth(&ctx, &mut game.battle.rng, class, lv);
        for k in 0..9 {
            ten[k] = (ten[k] as i16).wrapping_add(g[k] as i16) as i32;
        }
    }
    let mut gain = rules::growth(&ctx, &mut game.battle.rng, class, 1);
    let base = t.growth.base.get(class as usize - 1).map_or([0i16; 9], |b| b.stats);
    let u = game.battle.unit(r).clone();
    // BIN 8 order: HP, MP, ST, 攻擊, 防禦, 反應, 戰技, 魔力, 劍術.
    let cur = [u.hp_max, u.mp_max, u.st_max, u.atk, u.def, u.react, u.skill, u.mag, u.sword];
    for k in 0..9 {
        if cur[k] < base[k] {
            gain[k] = (gain[k] as i16).wrapping_add(((base[k] as i32 - cur[k] as i32) / 2) as i16) as i32;
        }
    }
    let name = s2data::text::cstr(&u.name).to_vec();
    let (old_c, new_c) = (game.res().class_name(u.kind as usize).to_vec(), game.res().class_name(class as usize).to_vec());
    let title = super::shop::format_msg(&game.msg(0xb3), &[super::shop::FmtArg::S(&name), super::shop::FmtArg::S(&old_c), super::shop::FmtArg::S(&new_c)]);
    text::large(game, x + 10, y + 0xd, &title, 0x74, 0, 1, 2);
    for (msg, cx) in [(0xb4usize, 0x7c), (0xb5, 0x12d), (0xb6, 0x1af), (0xb7, 0x22b)] {
        let s = game.msg(msg);
        text::small(game, x + cx - s.len() as i32 * 4, y + 0x36, &s, 0x84, 0, 1, 2);
    }
    let ok = game.msg(0xb8);
    text::large(game, x + 0x62 - ok.len() as i32 * 6, y + 0x189, &ok, 0x45, 0, 1, 2);
    let arrow = game.res().ui_image(ARROW).ok();
    let mut ry = y + 0x5c;
    let mut row = |game: &mut Game, label: &[u8], c: u8, now: i32, new: i32, growth: i32| {
        text::large(game, x + 10, ry, label, c, 0, 1, 2);
        large_number(game, x + 0x4a, ry, now, c, 0, 1, 2, 3);
        if let Some(a) = &arrow {
            gfx::blit(game.scr.page(), a, x + 0x88, ry + 10, false);
        }
        large_number(game, x + 0xa6, ry, new, c, 0, 1, 2, 3);
        large_number(game, x + 0x10e, ry, growth, c, 0, 1, 2, 3);
        ry += 0x1c;
    };
    let nv = |k: usize| (cur[k] as i32 + gain[k] as i16 as i32) as i16 as i32;
    row(game, b"HP", 0xa2, cur[0] as i32, nv(0), ten[0]);
    row(game, b"MP", 0xb9, cur[1] as i32, nv(1), ten[1]);
    row(game, b"ST", 0xa2, cur[2] as i32, nv(2), ten[2]);
    let new_rec = game.res().class_rec(class as usize).to_vec();
    let old_rec = game.res().class_rec(u.kind as usize).to_vec();
    let at = |r: &[u8], o: usize| r.get(o).copied().unwrap_or(0);
    let mut d = (at(&new_rec, 0x15) as i8 as i32 - (u.ap_max as i32 - u.eq_ap as i32)).max(0);
    if at(&new_rec, 0x11) != 5 && at(&old_rec, 0x11) == 5 {
        d = -4;
    }
    let l = game.msg(0xd);
    row(game, &l, 0xb9, u.ap_max as i32, (u.ap_max as i32 + d) as i16 as i32, 0);
    for (msg, c, k) in [(7usize, 0xa2u8, 3usize), (8, 0xb9, 4), (10, 0xa2, 5), (11, 0xb9, 7), (9, 0xa2, 6), (12, 0xb9, 8)] {
        let l = game.msg(msg);
        row(game, &l, c, cur[k] as i32, nv(k), ten[k]);
    }
    let icons: Vec<Option<Image>> = (0..11).map(|k| game.res().ui_image(0xf2 + 2 * k).ok()).collect();
    let mask = u16::from_le_bytes([at(&new_rec, 0x23), at(&new_rec, 0x24)]);
    let schools = game.res().school_names.clone();
    let mut eligible = [false; 11];
    let mut sy = y + 0x5d;
    for k in 0..11usize {
        if let Some(i) = &icons[k] {
            gfx::blit(game.scr.page(), i, x + 0x16e, sy, false);
        }
        eligible[k] = mask & (1 << k) != 0;
        let (cn, cv) = if eligible[k] { (0x36, 0xb8) } else { (0xf5, 0xb1) };
        let name = schools.get(k).cloned().unwrap_or_default();
        text::large(game, x + 0x19c, sy, &name, cn, 0, 1, 2);
        large_number(game, x + 0x220, sy, (u.prof[6 + k] / 10) as i32, cv, 0, 1, 2, 0);
        sy += 0x1a;
    }
    let skill = at(&new_rec, 0x13) as i8;
    if skill != 0 {
        let mut s = game.msg(0x9e);
        s.push(b' ');
        s.extend_from_slice(game.res().ability_name(skill as usize));
        text::large(game, x + 0x172, y + 0x18a, &s, 0x36, 0, 1, 2);
    }
    eligible
}

/// Port of `FUN_00454594(unit, class, panel, &school)`: the detail panel,
/// pick a school for the bonus, confirm. True when confirmed.
fn class_confirm(game: &mut Game, r: UnitRef, class: u16, panel: &Image, school: &mut Option<usize>) -> bool {
    // The image header holds width - 1: `(*(short *)img + 1) / 2` is width / 2.
    let px = 0x140 - panel.width as i32 / 2;
    let py = 0xf0 - panel.height as i32 / 2;
    game.scr.set_page(1);
    gfx::blit(game.scr.page(), panel, px, py, false);
    let eligible = class_detail(game, r, class, px, py);
    game.scr.set_page(0);
    *school = eligible.iter().position(|&e| e);
    let icons: Vec<Option<Image>> = (0..11).map(|k| game.res().ui_image(0xf1 + 2 * k).ok()).collect();
    let schools = game.res().school_names.clone();
    let mut hover = 0xb;
    let (mut redraw, mut moved) = (true, false);
    game.flush_events(0x1f);
    cursor::warp(game, px + 0xa8, py + 400);
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if redraw {
            game.scr.copy_page(0, 1);
            if let Some(s) = *school {
                let sy = py + 0x5d + s as i32 * 0x1a;
                if let Some(i) = &icons[s] {
                    gfx::blit(game.scr.page(), i, px + 0x16e, sy, false);
                }
                let name = schools.get(s).cloned().unwrap_or_default();
                text::large(game, px + 0x19c, sy, &name, 0x45, 0, 1, 2);
                let p = game.battle.unit(r).prof[6 + s] / 10;
                large_number(game, px + 0x220, sy, p as i32, 0x45, 0, 1, 2, 0);
            }
            if hover == 0xb {
                gfx::rect(game.scr.page(), px + 8, py + 0x185, px + 0xb8, py + 0x1a5, 0x28, false);
            } else {
                let h = hover as i32;
                gfx::rect(game.scr.page(), px + 0x16c, py + 0x5c + h * 0x1a, px + 0x25c, py + 0x74 + h * 0x1a, 0x28, false);
            }
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
                ev::KEY_DOWN | ev::KEY_REPEAT => {
                    if e.scan() == scan::ESC {
                        game.wait_event(ev::KEY_UP);
                        action = 1;
                    }
                }
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (x, y) = (e.x, e.y);
                    if px + 8 <= x && x <= px + 0xba && py + 0x185 <= y && y <= py + 0x1a7 {
                        if hover != 0xb {
                            hover = 0xb;
                            redraw = true;
                        }
                        if e.mods & ev::M_LBUTTON != 0 {
                            game.wait_event(ev::MOUSE_UP);
                            action = 0x39;
                        }
                    } else if px + 0x16e <= x && x <= px + 0x262 && py + 0x5d <= y && y < py + 0x17b {
                        let k = ((y - py - 0x5d) / 0x1a) as usize;
                        if eligible[k] {
                            if hover != k {
                                redraw = true;
                                hover = k;
                            }
                            if e.mods & ev::M_LBUTTON != 0 {
                                game.wait_event(ev::MOUSE_UP);
                                *school = Some(k);
                                redraw = true;
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
            1 => {
                game.flush_events(0x1f);
                return false;
            }
            0x39 => {
                game.flush_events(0x1f);
                return true;
            }
            _ => {}
        }
    }
}

/// Port of `FUN_00453986(army, unit, keep_ap)`: the class change screen.
/// Returns 2 when the class changed, 0 otherwise. AI/NPC units show the
/// screen for a second and take the first target (`ai_promotion_choice`).
pub fn class_change(game: &mut Game, r: UnitRef, keep_ap: bool) -> Result<i32> {
    let kind = game.battle.unit(r).kind as u16;
    let has_slots = game.battle.t().kinds.class(kind).is_some_and(|c| c.promotions.first().is_some_and(|&p| p != 0));
    if !has_slots {
        let u = game.battle.unit_mut(r);
        if u.exp_next < u.exp {
            u.exp = u.exp_next;
        }
        return Ok(0);
    }
    super::stop_music(game);
    let targets = game.battle.promotion_targets(r);
    let n = targets.len();
    cursor::erase(game);
    let lut = game.res().tints[6];
    gfx::remap_page(game.scr.page(), &lut);
    let bg = game.res().ui_image(0xd2)?;
    gfx::blit(game.scr.page(), &bg, 0, 0, false);
    let name = s2data::text::cstr(&game.battle.unit(r).name).to_vec();
    let title = super::shop::format_msg(&game.msg(0xb2), &[super::shop::FmtArg::S(&name)]);
    text::large(game, 0x22, 0xe, &title, 0xba, 0, 1, 2);
    game.scr.copy_page(2, 0);
    let frames = class_frames(game, &targets);
    let panel = game.res().ui_image(0xe7)?;
    let ai = game.battle.unit(r).npc == 1 || r.side != Side::Player;
    let t_start = game.ticks();
    game.flush_events(0x1f);
    ui_sound(game, 0x7b);
    let (mut sel, mut frame) = (0usize, 0usize);
    let mut school: Option<usize> = None;
    let (mut warp, mut full, mut anim, mut moved) = (true, true, true, false);
    let mut t_anim = 0;
    let mut result = 2;
    'outer: loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if warp && n > 0 {
            let (cx, cy) = if sel < 4 { (sel as i32 * 0x92 + 0xab, 0xa4) } else { ((sel as i32 - 4) * 0x92 + 0xab, 0x172) };
            cursor::warp(game, cx, cy);
            warp = false;
        }
        if full {
            game.scr.copy_page(1, 2);
            class_cards(game, sel, &targets, &frames);
            anim = true;
            full = false;
            frame = 0;
        }
        if anim {
            game.scr.copy_page(0, 1);
            if let Some(f) = frames.get(sel).and_then(|f| f[frame + 1].as_ref()) {
                let (cx, cy) = card_origin(sel, 0x92);
                let (w, h) = (f.width as i32, f.height as i32);
                gfx::blit(game.scr.page(), f, cx - w / 2, cy + 0xbe - h, false);
            }
            cursor::draw_present(game, None, false);
            moved = false;
            anim = false;
            t_anim = game.ticks();
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if !game.still_waiting(t_anim, 0x13) {
            anim = true;
            frame ^= 1;
        }
        if ai {
            if !game.still_waiting(t_start, 100) {
                break;
            }
            game.sys.idle();
            continue;
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
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 0x39;
                    }
                    scan::TAB | scan::RIGHT => {
                        if n > 1 {
                            sel = if sel + 1 >= n { 0 } else { sel + 1 };
                            warp = true;
                            full = true;
                        }
                    }
                    scan::LEFT => {
                        if n > 1 {
                            sel = if sel == 0 { n - 1 } else { sel - 1 };
                            warp = true;
                            full = true;
                        }
                    }
                    scan::UP | scan::DOWN
                        if n > 4 => {
                            let s = if sel < 4 { sel + 4 } else { sel - 4 };
                            sel = s.min(n - 1);
                            warp = true;
                            full = true;
                        }
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    if e.mods & ev::M_RBUTTON != 0 {
                        action = 1;
                        game.wait_event(ev::MOUSE_UP);
                    } else {
                        let (mut cx, mut cy) = (0x24, 0x37);
                        for i in 0..n {
                            if i == 4 {
                                cx = 0x24;
                                cy = 0x105;
                            }
                            if cx <= e.x && e.x <= cx + 0x82 && cy <= e.y && e.y <= cy + 200 {
                                if i == sel {
                                    if e.mods & ev::M_LBUTTON != 0 {
                                        game.wait_event(ev::MOUSE_UP);
                                        action = 0x39;
                                    }
                                } else {
                                    sel = i;
                                    full = true;
                                }
                                break;
                            }
                            cx += 0x92;
                        }
                    }
                }
                _ => {}
            }
        }
        match action {
            0x39 => {
                if class_confirm(game, r, targets[sel], &panel, &mut school) {
                    break 'outer;
                }
                warp = true;
                full = true;
            }
            1 => {
                result = 0;
                break 'outer;
            }
            _ => {}
        }
    }
    if result == 2 {
        let (class, pick) = if ai {
            match crate::battle::classchange::ai_promotion_choice(&targets) {
                Some(c) => c,
                None => return Ok(0),
            }
        } else {
            (targets[sel], school)
        };
        game.battle.change_class_from_menu(r, class, pick, keep_ap);
        crate::battle::view::reload_sprite(game, r);
    }
    game.sys.audio.stop_sounds();
    let cue = game.campaign.music;
    super::music(game, cue);
    game.flush_events(0x1f);
    Ok(result)
}

fn ui_sound(game: &mut Game, id: u16) {
    crate::ui::sound(game, id);
}

/// Effect hooks for item use between stages: deaths are only marked, and
/// what a spell book taught is kept so its window can follow the use.
#[derive(Default)]
struct Learned {
    dead: crate::battle::spells::MarkDead,
    learned: Vec<(UnitRef, Vec<usize>, bool)>,
}

impl crate::battle::spells::EffectHooks for Learned {
    fn die(&mut self, b: &mut crate::battle::Battle, u: UnitRef, mode: i32) {
        self.dead.die(b, u, mode);
    }

    fn learned(&mut self, _b: &mut crate::battle::Battle, unit: UnitRef, _school: i32, spells: &[usize], draw_unit: bool) {
        self.learned.push((unit, spells.to_vec(), draw_unit));
    }
}
