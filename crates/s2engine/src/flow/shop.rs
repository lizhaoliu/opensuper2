//! Shops and the church: buy (`FUN_00410d3a`, `FUN_00411068`, `FUN_00411aa2`,
//! `FUN_004120f8`), sell (`FUN_00412b55`, `FUN_00412caa`, `FUN_00413556`),
//! donation (`FUN_00413c01`, `FUN_004449cd`), revival and class change (`FUN_00413f6d`).

use anyhow::Result;
use s2data::sprite::Image;

use super::town::{keeper_window, large_number};
use crate::battle::unit::flag;
use crate::battle::{Side, UnitRef};
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;
use crate::ui::{self, Wait};

/// Small scroll thumb of the lists (sysimg 147, `DAT_00493a94`).
const THUMB: usize = 147;
/// Arrow between old and new values (sysimg 148, `DAT_00493a98`).
const ARROW: usize = 148;

fn price(game: &Game, id: i32) -> i32 {
    let r = game.res().item_rec(id as usize);
    r.get(0x1a..0x1c).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as i32)
}

/// `FUN_00442eb7` / `FUN_00442ecb` (army gold get / set), `FUN_00446b70`
/// (inventory base) and `FUN_00442ee4` (army flags) are these field reads.
fn gold(game: &Game) -> i32 {
    game.battle.army(Side::Player).gold
}

fn set_gold(game: &mut Game, v: i32) {
    game.battle.army_mut(Side::Player).gold = v;
}

fn inv(game: &Game, id: i32) -> i32 {
    game.battle.army(Side::Player).inventory.get(id as usize - 1).copied().unwrap_or(0) as i32
}

fn set_inv(game: &mut Game, id: i32, v: i32) {
    if let Some(c) = game.battle.army_mut(Side::Player).inventory.get_mut(id as usize - 1) {
        *c = v.clamp(0, 99) as u8;
    }
}

/// `army+0xde & 0x10`: the party gets the 20% discount.
fn discount(game: &Game) -> bool {
    game.battle.army(Side::Player).flags & 0x10 != 0
}

/// Port of `FUN_004118e0`: the strip of up to 8 item icons with prices.
#[allow(clippy::too_many_arguments)]
fn item_strip(game: &mut Game, x: i32, y: i32, ids: &[i32], top: usize, sell: bool, rate: i32) {
    let g = gold(game);
    let label = game.msg(if sell { 0xcb } else { 0xca });
    text::large(game, x + 0x192 - text::strlen(&label) * 6, y + 0x2b, &label, 0x8b, 0, 1, 2);
    for k in 0..8usize {
        let Some(&id) = ids.get(k + top) else { break };
        if let Ok(icon) = game.res().ui_image(id as usize + 0x1ec) {
            gfx::blit(game.scr.page(), &icon, x + 200 + k as i32 * 0x36, y + 0x4d, false);
        }
        let p = price(game, id) * rate / 10;
        let color = if sell || g >= p { 0xbe } else { 0x44 };
        let s = format!("{p}");
        text::small(game, x + 0xd8 - s.len() as i32 * 4 + k as i32 * 0x36, y + 0x7a, s.as_bytes(), color, 0, 1, 2);
    }
}

/// Port of `FUN_00410d3a(kind)`: buy from shop `kind` (1 weapons, 2 armour, 3 items).
pub fn buy(game: &mut Game, kind: usize) -> Result<()> {
    let loc = game.campaign.location;
    let Some(mut ids) = game.ui.world.as_ref().map(|w| w.shop_items(loc, kind)) else { return Ok(()) };
    if ids.is_empty() {
        return Ok(());
    }
    let faces = [0, 0x2f, 0x98, 0x2c];
    let title = game.msg(0xc2);
    keeper_window(game, faces[kind], &title, false, true)?;
    game.scr.copy_page(2, 0);
    // Ids outside the item table are dropped (replaced by the last one).
    let n_items = game.res().item_count() as i32;
    let mut n = ids.len();
    let mut i = 0;
    while i < n {
        if ids[i] > n_items || ids[i] < 1 {
            if i < n - 1 {
                ids[i] = ids[n - 1];
            }
            n -= 1;
        } else {
            i += 1;
        }
    }
    ids.truncate(n);
    let imgs: Vec<Image> = [0xd3, 0xd5, 0xd8, 0xda].iter().map(|&s| game.res().ui_image(s)).collect::<Result<_>>()?;
    buy_list(game, &imgs, &ids, kind)
}

/// State of the icon-strip list screens.
struct Strip {
    sel: usize,
    top: usize,
}

impl Strip {
    /// `0x4b` / `0x4d` moves; returns (moved, scrolled).
    fn step(&mut self, left: bool, n: usize) -> (bool, bool) {
        if left {
            if self.sel < 1 {
                if self.top > 0 {
                    self.top -= 1;
                    return (true, true);
                }
            } else {
                self.sel -= 1;
                return (true, false);
            }
        } else if self.sel < 7 && self.top + self.sel + 1 < n {
            self.sel += 1;
            return (true, false);
        } else if self.sel == 7 && self.top + 8 < n {
            self.top += 1;
            return (true, true);
        }
        (false, false)
    }

    /// Click on the scrollbar at `x`: returns the step code or repositions.
    fn bar(&mut self, x: i32, n: usize) -> Option<i32> {
        if x < 0xde {
            return Some(0x4b);
        }
        if x >= 600 {
            return Some(0x4d);
        }
        let mut top = (n as i32 * (x - 0xde)) / 0x15d;
        if (n as i32) < top + 8 {
            top = n as i32 - 8;
        }
        self.top = top.max(0) as usize;
        self.sel = if x < 0x23b { 0 } else { 7 };
        if n.saturating_sub(1) <= self.sel {
            self.sel = n.saturating_sub(1);
            self.top = 0;
        }
        None
    }
}

/// Shared mouse handling of `FUN_00411068` / `FUN_00412caa`: returns an action
/// code (0x1c select, 0x4b / 0x4d step) and whether the screen must be rebuilt.
fn strip_mouse(game: &mut Game, st: &mut Strip, n: usize, redraw: &mut bool) -> (i32, bool) {
    let e = game.ev;
    let (x, y) = (e.x, e.y);
    if x < 0xc4 || 0x273 < x || y < 0x133 || 0x17b < y {
        if e.mods & ev::M_LBUTTON != 0 && 0xc3 < x && x < 0x272 && 0x17d < y && y < 0x192 {
            return match st.bar(x, n) {
                Some(code) => (code, false),
                None => (0, true),
            };
        }
        return (0, false);
    }
    let k = ((x - 0xc4) / 0x36) as usize;
    if k + st.top < n {
        if k != st.sel {
            *redraw = true;
            st.sel = k;
        }
        if e.mods & ev::M_LBUTTON != 0 {
            game.wait_event(ev::MOUSE_UP);
            return (0x1c, false);
        }
    }
    (0, false)
}

/// Port of `FUN_00411068(imgs, ids, n, kind)`: the buy list.
fn buy_list(game: &mut Game, imgs: &[Image], ids: &[i32], kind: usize) -> Result<()> {
    let mut g = gold(game);
    let rate = if discount(game) { 8 } else { 10 };
    let n = ids.len();
    let mut st = Strip { sel: 0, top: 0 };
    let (mut done, mut fade_pending, mut warp_pending, mut full, mut moved, mut delay) = (false, true, true, true, false, false);
    let mut redraw = false;
    let mut t_delay = 0;
    loop {
        let mut action = 0;
        if done {
            game.flush_events(0x1f);
            return Ok(());
        }
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if full {
            game.scr.copy_page(1, 2);
            game.scr.set_page(1);
            gfx::blit(game.scr.page(), &imgs[0], 4, 0xec, true);
            item_strip(game, 4, 0xec, ids, st.top, false, rate);
            game.scr.set_page(0);
            redraw = true;
        }
        if warp_pending {
            cursor::warp(game, st.sel as i32 * 0x36 + 0xe0, 0x164);
            warp_pending = false;
            moved = true;
        }
        if !redraw {
            if moved {
                cursor::draw_present(game, None, true);
                moved = false;
            }
        } else {
            game.scr.copy_page(0, 1);
            let m = game.msg(0x64);
            text::large(game, 0x95, 0x2a, &m, 0x2a, 0, 1, 2);
            large_number(game, 0x1e0, 0xcd, g, 0xb3, 0, 1, 2, 0);
            let s = st.sel as i32;
            gfx::rect(game.scr.page(), s * 0x36 + 0xc4, 0x133, s * 0x36 + 0xf7, 0x17b, 0x3c, false);
            let mut tx = 0xde;
            if st.top + st.sel > 0 {
                tx = ((st.top + 1 + st.sel) as i32 * 0x15d) / n as i32 + 0xde;
            }
            gfx::blit(game.scr.page(), &imgs[1], tx, 0x17e, true);
            let id = ids[st.top + st.sel];
            let name = game.res().item_name(id as usize).to_vec();
            text::large(game, 0x5d - text::strlen(&name) * 6, 0x10b, &name, 0x36, 0, 1, 2);
            let help = game.res().item_help_text(id as usize).to_vec();
            text::block_small(game, 0x1d, 0x12e, &help, 0x2a, 0, 1, 2, 9, 8, 0x10);
            let own = game.msg(0xfb);
            text::large(game, 0x108, 0x1a0, &own, 0x2a, 0, 1, 2);
            let have = inv(game, id);
            large_number(game, text::strlen(&own) * 0xc + 0x112, 0x1a0, have, 0x2a, 0, 1, 2, 0);
            if full {
                cursor::capture(game);
            }
            cursor::draw_present(game, None, false);
            if fade_pending {
                let m = game.res().master.clone();
                gfx::fade(game, &m, 0, -4);
                fade_pending = false;
            }
            full = false;
            moved = false;
            redraw = false;
        }
        if delay {
            game.wait_until(t_delay, 10);
            delay = false;
        }
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::ESC => {
                        done = true;
                        game.wait_event(ev::KEY_UP);
                    }
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 0x1c;
                    }
                    scan::LEFT => {
                        warp_pending = true;
                        action = 0x4b;
                    }
                    scan::RIGHT => {
                        warp_pending = true;
                        action = 0x4d;
                    }
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    if e.mods & ev::M_RBUTTON == 0 {
                        let (a, rebuild) = strip_mouse(game, &mut st, n, &mut redraw);
                        action = a;
                        full |= rebuild;
                    } else {
                        game.wait_event(ev::MOUSE_UP);
                        done = true;
                    }
                }
                _ => {}
            }
        }
        match action {
            0x1c => {
                let id = ids[st.top + st.sel];
                if g < price(game, id) * rate / 10 {
                    let m = game.msg(0x68);
                    ui::message_box(game, 0x10c, 0x17e, &m, Wait::Click(0));
                } else {
                    if kind == 3 {
                        buy_quantity(game, 0x102, 0x19d, &imgs[2], id);
                    } else {
                        buy_equip(game, &imgs[3], id);
                    }
                    g = gold(game);
                    cursor::warp(game, st.sel as i32 * 0x36 + 0xe0, 0x164);
                }
                full = true;
            }
            0x4b | 0x4d => {
                let (stepped, scrolled) = st.step(action == 0x4b, n);
                if stepped {
                    delay = true;
                    t_delay = game.ticks();
                    if scrolled {
                        full = true;
                    } else {
                        redraw = true;
                    }
                }
            }
            _ => {}
        }
    }
}

/// `sprintf` of a Big5 format with `%s` and/or `%d` arguments in order.
pub(crate) fn format_msg(fmt: &[u8], args: &[FmtArg]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut a = args.iter();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c == b'%' && i + 1 < fmt.len() {
            match fmt[i + 1] {
                b's' | b'd' => {
                    match a.next() {
                        Some(FmtArg::S(s)) => out.extend_from_slice(s),
                        Some(FmtArg::D(v)) => out.extend_from_slice(v.to_string().as_bytes()),
                        None => {}
                    }
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
        if c >= 0x81 && i + 1 < fmt.len() {
            out.push(fmt[i + 1]);
            i += 1;
        }
        i += 1;
    }
    out
}

pub(crate) enum FmtArg<'a> {
    S(&'a [u8]),
    D(i32),
}

/// The bottom part of page 0 (rows 234..479) copied to page 1 before a
/// quantity box opens (`FUN_00411aa2` / `FUN_00413556`).
fn keep_lower_half(game: &mut Game) {
    cursor::erase(game);
    let (a, b) = (234 * 640, 234 * 640 + 0x26480);
    let src = game.scr.pages[0].pixels[a..b].to_vec();
    game.scr.pages[1].pixels[a..b].copy_from_slice(&src);
}

/// The +/- quantity box shared by buying and selling. Returns the chosen
/// quantity on confirm, `None` on cancel.
#[allow(clippy::too_many_arguments)]
fn quantity_box(game: &mut Game, x: i32, y: i32, img: &Image, prompt: &[u8], unit: i32, max: impl Fn(&Game, i32) -> bool, draw_extra: &dyn Fn(&mut Game, i32)) -> Option<i32> {
    keep_lower_half(game);
    game.scr.set_page(1);
    gfx::blit(game.scr.page(), img, x, y, true);
    text::large(game, 0x95, 0x5a, prompt, 0x2a, 0, 1, 2);
    game.scr.set_page(0);
    cursor::warp(game, x + 0x104, y + 0xf);
    let mut qty = 1;
    let (mut redraw, mut moved, mut delay) = (true, false, false);
    let mut t_delay = 0;
    loop {
        let mut action = 0;
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if redraw {
            game.scr.copy_page(0, 1);
            large_number(game, x + 0xe, y + 2, qty, 0x3b, 0, 1, 2, 0);
            let total = format!("${}", qty * unit);
            text::large(game, x + 0x46, y + 2, total.as_bytes(), 0x3b, 0, 1, 2);
            let ok = game.msg(0xce);
            text::large(game, x + 0xe6, y + 3, &ok, 0x2a, 0, 1, 2);
            draw_extra(game, qty);
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if delay {
            game.wait_until(t_delay, 10);
            delay = false;
        }
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::ESC => {
                        game.wait_event(ev::KEY_UP);
                        game.flush_events(0x1f);
                        return None;
                    }
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 0x1c;
                    }
                    scan::LEFT => action = 0x4b,
                    scan::RIGHT => action = 0x4d,
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        game.flush_events(0x1f);
                        return None;
                    }
                    if e.mods & ev::M_LBUTTON != 0 {
                        let (mx, my) = (e.x, e.y);
                        if x + 0xe3 <= mx && mx <= x + 0x124 && y <= my && my <= y + 0x1e {
                            game.wait_event(ev::MOUSE_UP);
                            action = 0x39;
                        } else if x + 0xab <= mx && mx <= x + 0xc3 && y + 5 <= my && my <= y + 0x18 && qty >= 2 {
                            action = 0x4b;
                        } else if x + 200 <= mx && mx <= x + 0xe1 && y + 5 <= my && my <= y + 0x18 {
                            action = 0x4d;
                        }
                    }
                }
                _ => {}
            }
        }
        match action {
            0x1c | 0x39 => {
                game.flush_events(0x1f);
                return Some(qty);
            }
            0x4b if qty > 1 => {
                qty -= 1;
                t_delay = game.ticks();
                redraw = true;
                delay = true;
            }
            0x4d if max(game, qty + 1) => {
                qty += 1;
                t_delay = game.ticks();
                redraw = true;
                delay = true;
            }
            _ => {}
        }
    }
}

/// Port of `FUN_00411aa2(x, y, img, id)`: how many to buy.
fn buy_quantity(game: &mut Game, x: i32, y: i32, img: &Image, id: i32) {
    let g = gold(game);
    let mut unit = price(game, id);
    if discount(game) {
        unit = (unit << 3) / 10;
    }
    let name = game.res().item_name(id as usize).to_vec();
    let prompt = format_msg(&game.msg(0xc8), &[FmtArg::S(&name)]);
    let extra = |game: &mut Game, qty: i32| large_number(game, 0x1e0, 0xcd, g - qty * unit, 0xb3, 0, 1, 2, 0);
    if let Some(qty) = quantity_box(game, x, y, img, &prompt, unit, |_, q| q * unit <= g, &extra) {
        set_gold(game, g - qty * unit);
        let have = inv(game, id);
        set_inv(game, id, if have + qty < 99 { have + qty } else { 99 });
    }
}

/// Port of `FUN_0045132d(unit, x, y)`: the six equipment slots of a unit.
pub(crate) fn equipment_slots(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    let eq = game.battle.unit(r).equip;
    for (k, &id) in eq.iter().enumerate() {
        if id == 0 {
            continue;
        }
        let name = game.res().item_name(id as usize).to_vec();
        text::large(game, x + 0x2a, y + 7 + k as i32 * 0x23, &name, 0xe9, 0, 1, 2);
        if let Ok(icon) = game.res().ui_image(id as usize + 0x1ec) {
            gfx::blit(game.scr.page(), &icon, x + 8, y + 2 + k as i32 * 0x23, false);
        }
    }
}

/// Port of `FUN_00448e63(x, y, unit, slot, id)`: what changes when `id`
/// replaces the item in `slot` (only differing values are listed).
pub(crate) fn compare_panel(game: &mut Game, x: i32, y: i32, r: UnitRef, slot: usize, id: i32) {
    let u = game.battle.unit(r).clone();
    let cur = u.equip[slot - 1];
    let old: Vec<u8> = if cur == 0 { vec![0; 0x26] } else { game.res().item_rec(cur as usize).to_vec() };
    let new = game.res().item_rec(id as usize).to_vec();
    if old.len() < 0x26 || new.len() < 0x26 {
        return;
    }
    let arrow = game.res().ui_image(ARROW).ok();
    let (lx, vx, ax, nx) = (x + 0x1eb, x + 0x214, x + 0x230, x + 0x24c);
    let mut ly = y + 0x50;
    let color = |better: bool| if better { 0xa7 } else { 0x3b };
    let i16at = |b: &[u8], o: usize| i16::from_le_bytes([b[o], b[o + 1]]) as i32;
    let row = |game: &mut Game, ly: &mut i32, msg: usize, cur_v: i32, new_v: i32, better: bool| {
        let label = game.msg(msg);
        text::small(game, lx, *ly, &label, 0x89, 0, 1, 2);
        text::number(game, vx, *ly, cur_v, 0x6d, 0, 1, 2, 0);
        if let Some(a) = &arrow {
            gfx::blit(game.scr.page(), a, ax, *ly + 4, false);
        }
        text::number(game, nx, *ly, new_v, color(better), 0, 1, 2, 0);
        *ly += 0x18;
    };
    if i16at(&old, 0x11) != i16at(&new, 0x11) {
        let v = (u.atk as i32 + new[0x11] as i32 - old[0x11] as i32).max(0);
        row(game, &mut ly, 7, u.atk as i32, v, i16at(&old, 0x11) <= i16at(&new, 0x11));
        // The first row uses y+0x54 for the arrow and moves to y+0x68.
        ly = y + 0x68;
    }
    if old[0x15] != new[0x15] {
        let v = (u.def as i32 + new[0x15] as i32 - old[0x15] as i32).max(0);
        row(game, &mut ly, 8, u.def as i32, v, (old[0x15] as i8) <= (new[0x15] as i8));
    }
    if old[0x16] != new[0x16] {
        row(game, &mut ly, 0xb, old[0x16] as i8 as i32, new[0x16] as i8 as i32, (old[0x16] as i8) <= (new[0x16] as i8));
    }
    if old[0x10] != new[0x10] {
        row(game, &mut ly, 0x77, (old[0x10] % 10) as i32, (new[0x10] % 10) as i32, (old[0x10] as i8) <= (new[0x10] as i8));
    }
    for (o, msg) in [(0x13usize, 0x74usize), (0x14, 0x75), (0x17, 0x76)] {
        if old[o] != new[o] {
            row(game, &mut ly, msg, old[o] as i32, new[o] as i32, (old[o] as i8) <= (new[o] as i8));
        }
    }
    if y + 0x50 < ly {
        let t = game.msg(0x78);
        text::large(game, x + 0x1f7, y + 0x25, &t, 0xe9, 0, 1, 2);
    }
}

/// Port of `FUN_004120f8(img, id)`: who should carry the new equipment.
fn buy_equip(game: &mut Game, img: &Image, id: i32) {
    let rec = game.res().item_rec(id as usize).to_vec();
    let k = rec.get(0xe).copied().unwrap_or(0) as i8;
    let slot = match k {
        1..=6 => 1,
        7..=11 => (k - 5) as usize,
        _ => return,
    };
    // FUN_0043b071: members who may use it and are not dead, in reserve, NPCs or summons.
    let list: Vec<UnitRef> = (0..game.battle.army(Side::Player).units.len())
        .map(|i| UnitRef::new(Side::Player, i))
        .filter(|&r| {
            let u = game.battle.unit(r);
            game.battle.item_equippable(u, id as u16)
                && u.flags & (flag::DEAD | flag::RESERVE | flag::SUMMON) == 0
                && u.npc != 1
        })
        .collect();
    let n = list.len();
    let mut g = gold(game);
    let mut unit_price = price(game, id);
    if discount(game) {
        unit_price = (unit_price << 3) / 10;
    }
    let py = 0x1e0 - img.height as i32;
    game.scr.set_page(1);
    gfx::blit(game.scr.page(), img, 0, py, true);
    let name = game.res().item_name(id as usize).to_vec();
    let prompt = format_msg(&game.msg(0xd0), &[FmtArg::D(unit_price), FmtArg::S(&name)]);
    text::large(game, 0x95, 0x5a, &prompt, 0x2a, 0, 1, 2);
    let public = game.msg(0xcc);
    text::large(game, 0x115, py + 0x14, &public, 0x2d, 0, 1, 2);
    large_number(game, 0x1e0, 0xcd, g - unit_price, 0xb3, 0, 1, 2, 0);
    game.scr.set_page(0);
    cursor::warp(game, 0x170, py + 0x28);
    let thumb = game.res().ui_image(THUMB).ok();
    let (mut mode, mut sel, mut top) = (0, 0usize, 0usize);
    let (mut redraw, mut moved, mut delay) = (true, false, false);
    let mut t_delay = 0;
    let mut action = 0;
    loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if redraw {
            game.scr.copy_page(0, 1);
            for k in 0..5usize {
                let Some(&r) = list.get(k + top) else { break };
                let c = if k == sel && mode == 1 { 0xf2 } else { 0x34 };
                let nm = s2data::text::cstr(&game.battle.unit(r).name).to_vec();
                text::large(game, 0x131, py + 0x3b + k as i32 * 0x23, &nm, c, 0, 1, 2);
            }
            if mode == 0 {
                text::large(game, 0x115, py + 0x14, &public, 0xf2, 0, 1, 2);
            } else if n != 0 {
                let r = list[top + sel];
                equipment_slots(game, r, 0x3e, py + 0x12);
                compare_panel(game, 0, py, r, slot, id);
            }
            if n != 0 {
                let mut ty = py + 0x4c;
                if top + sel > 0 {
                    ty += ((top + 1 + sel) as i32 * 100) / n as i32;
                }
                if let Some(t) = &thumb {
                    gfx::blit(game.scr.page(), t, 0xef, ty, false);
                }
            }
            cursor::draw_present(game, None, false);
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if delay {
            game.wait_until(t_delay, 10);
            delay = false;
        }
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
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
                        game.wait_event(ev::MOUSE_UP);
                        break;
                    }
                    let (x, y) = (e.x, e.y);
                    if 0x105 < x && x < 0x1cc && py + 0x11 <= y && y <= py + 0x2d {
                        if mode == 1 {
                            mode = 0;
                            redraw = true;
                        }
                        if e.mods & ev::M_LBUTTON != 0 {
                            game.wait_event(ev::MOUSE_UP);
                            let q = game.msg(0x60);
                            if ui::yes_no(game, 0xd4, 0xe4, true, &q, false, false) {
                                action = 1;
                            } else {
                                redraw = true;
                            }
                        }
                    }
                    if n != 0 {
                        if (0x10d..=0x1c5).contains(&x) && py + 0x34 <= y && y < py + 0xe3 {
                            let row = ((y - py - 0x34) / 0x23) as usize;
                            if top + row < n {
                                if row == sel && mode != 0 {
                                    if e.mods & ev::M_LBUTTON != 0 {
                                        game.wait_event(ev::MOUSE_UP);
                                        let q = game.msg(0xcf);
                                        if ui::yes_no(game, 0xd4, 0xe4, true, &q, false, false) {
                                            action = 2;
                                        } else {
                                            redraw = true;
                                        }
                                    }
                                } else {
                                    redraw = true;
                                    mode = 1;
                                    sel = row;
                                }
                            }
                        } else if e.mods & ev::M_LBUTTON != 0 && 0xee < x && x < 0x103 && py + 0x33 <= y && y <= py + 0xe6 {
                            if y <= py + 0x4a {
                                let mut stepped = false;
                                if sel >= 1 {
                                    sel -= 1;
                                    stepped = true;
                                } else if top > 0 {
                                    top -= 1;
                                    stepped = true;
                                }
                                if stepped || redraw {
                                    redraw = true;
                                    t_delay = game.ticks();
                                    delay = true;
                                    mode = 1;
                                }
                            } else if y >= py + 0xcf {
                                if sel + top < n - 1 {
                                    if sel < 4 {
                                        sel += 1;
                                    } else {
                                        top += 1;
                                    }
                                    t_delay = game.ticks();
                                    redraw = true;
                                    delay = true;
                                    mode = 1;
                                }
                            } else if py + 0x4c <= y && y <= py + 0xcd && n > 5 {
                                let mut t = (n as i32 * (y - py - 0x4c)) / 0x81;
                                if (n as i32) < t + 5 {
                                    t = n as i32 - 5;
                                }
                                top = t.max(0) as usize;
                                sel = if y < py + 0xaf { 0 } else { 4 };
                                if n - 1 <= sel {
                                    sel = n - 1;
                                    top = 0;
                                }
                                redraw = true;
                                mode = 1;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if action != 0 {
            g -= unit_price;
            set_gold(game, g);
            let have = inv(game, id);
            set_inv(game, id, if have + 1 < 99 { have + 1 } else { 99 });
            if action == 2 {
                let r = list[top + sel];
                if game.battle.unit(r).equip[slot - 1] != 0 {
                    game.battle.unequip_slot(r, slot, false);
                }
                game.battle.equip_item(r, id as u16);
                game.battle.recalc_army_flags(Side::Player, 2);
            }
            break;
        }
    }
    game.flush_events(0x1f);
}

/// Port of `FUN_00412b55`: the sell counter.
pub fn sell(game: &mut Game) -> Result<()> {
    let title = game.msg(0xc2);
    keeper_window(game, 0xa7, &title, false, true)?;
    let bar = game.res().ui_image(0xd3)?;
    gfx::blit(game.scr.page(), &bar, 4, 0xec, true);
    game.scr.copy_page(2, 0);
    let n_items = game.res().item_count() as i32;
    let ids: Vec<i32> = (1..=n_items).filter(|&id| inv(game, id) != 0 && price(game, id) != 0).collect();
    let imgs: Vec<Image> = [0xd5, 0xd8].iter().map(|&s| game.res().ui_image(s)).collect::<Result<_>>()?;
    sell_list(game, &imgs, ids);
    Ok(())
}

/// Port of `FUN_00412caa(imgs, ids, n)`: the sell list.
fn sell_list(game: &mut Game, imgs: &[Image], mut ids: Vec<i32>) {
    let mut g = gold(game);
    let rate = if discount(game) { 7 } else { 5 };
    let mut st = Strip { sel: 0, top: 0 };
    let (mut done, mut fade_pending, mut warp_pending, mut full, mut moved, mut delay) = (false, true, true, true, false, false);
    let mut redraw = false;
    let mut t_delay = 0;
    loop {
        let mut action = 0;
        let n = ids.len();
        if done {
            game.flush_events(0x1f);
            return;
        }
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if full {
            game.scr.copy_page(1, 2);
            game.scr.set_page(1);
            if n != 0 {
                item_strip(game, 4, 0xec, &ids, st.top, true, rate);
            }
            game.scr.set_page(0);
            full = false;
            redraw = true;
        }
        if warp_pending {
            cursor::warp(game, st.sel as i32 * 0x36 + 0xe0, 0x164);
            warp_pending = false;
        }
        if !redraw {
            if moved {
                cursor::draw_present(game, None, true);
                moved = false;
            }
        } else {
            game.scr.copy_page(0, 1);
            large_number(game, 0x1e0, 0xcd, g, 0xb3, 0, 1, 2, 0);
            if n == 0 {
                let m = game.msg(0x61);
                text::large(game, 0x95, 0x2a, &m, 0x2a, 0, 1, 2);
            } else {
                let m = game.msg(0x62);
                text::large(game, 0x95, 0x2a, &m, 0x2a, 0, 1, 2);
                let s = st.sel as i32;
                gfx::rect(game.scr.page(), s * 0x36 + 0xc4, 0x133, s * 0x36 + 0xf7, 0x17b, 0x3c, false);
                let mut tx = 0xde;
                if st.top + st.sel > 0 {
                    tx = ((st.top + 1 + st.sel) as i32 * 0x15d) / n as i32 + 0xde;
                }
                gfx::blit(game.scr.page(), &imgs[0], tx, 0x17e, true);
                let id = ids[st.top + st.sel];
                let name = game.res().item_name(id as usize).to_vec();
                text::large(game, 0x5d - text::strlen(&name) * 6, 0x10b, &name, 0x36, 0, 1, 2);
                let help = game.res().item_help_text(id as usize).to_vec();
                text::block_small(game, 0x1d, 0x12e, &help, 0x2a, 0, 1, 2, 9, 8, 0x10);
                let cnt = game.msg(0x6e);
                text::large(game, 0x108, 0x1a0, &cnt, 0x2a, 0, 1, 2);
                let have = inv(game, id);
                large_number(game, text::strlen(&cnt) * 0xc + 0x112, 0x1a0, have, 0x2a, 0, 1, 2, 0);
            }
            cursor::draw_present(game, None, false);
            if fade_pending {
                let m = game.res().master.clone();
                gfx::fade(game, &m, 0, -4);
                fade_pending = false;
            }
            moved = false;
            redraw = false;
        }
        if delay {
            game.wait_until(t_delay, 10);
            delay = false;
        }
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                    scan::ESC => {
                        done = true;
                        game.wait_event(ev::KEY_UP);
                    }
                    scan::ENTER | scan::SPACE => {
                        game.wait_event(ev::KEY_UP);
                        action = 0x1c;
                    }
                    scan::LEFT => {
                        warp_pending = true;
                        action = 0x4b;
                    }
                    scan::RIGHT => {
                        warp_pending = true;
                        action = 0x4d;
                    }
                    _ => {}
                },
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        done = true;
                    }
                    if n != 0 {
                        let (a, rebuild) = strip_mouse(game, &mut st, n, &mut redraw);
                        action = a;
                        full |= rebuild;
                    }
                }
                _ => {}
            }
        }
        if n == 0 {
            continue;
        }
        match action {
            0x1c => {
                let id = ids[st.top + st.sel];
                if sell_quantity(game, 0xc6, 0x19d, &imgs[1], id) {
                    if inv(game, id) == 0 {
                        ids.remove(st.top + st.sel);
                        let n = ids.len();
                        if n < st.top + 8 && st.top > 0 {
                            st.top -= 1;
                        }
                        if n <= st.top + st.sel && st.sel > 0 {
                            st.sel -= 1;
                        }
                    }
                    g = gold(game);
                    cursor::warp(game, st.sel as i32 * 0x36 + 0xe0, 0x164);
                }
                full = true;
            }
            0x4b | 0x4d => {
                let (stepped, scrolled) = st.step(action == 0x4b, n);
                if stepped {
                    delay = true;
                    t_delay = game.ticks();
                    if scrolled {
                        full = true;
                    } else {
                        redraw = true;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Port of `FUN_00413556(x, y, img, id)`: how many to sell. True when sold.
fn sell_quantity(game: &mut Game, x: i32, y: i32, img: &Image, id: i32) -> bool {
    let have = inv(game, id);
    let g = gold(game);
    let unit = if discount(game) { price(game, id) * 7 / 10 } else { price(game, id) >> 1 };
    let name = game.res().item_name(id as usize).to_vec();
    let prompt = format_msg(&game.msg(0xc9), &[FmtArg::S(&name)]);
    let extra = |game: &mut Game, qty: i32| {
        large_number(game, 0x1e0, 0xcd, g + qty * unit, 0xb3, 0, 1, 2, 0);
        let cnt = game.msg(0x6e);
        text::large(game, 0x212, 0x1a0, &cnt, 0x2a, 0, 1, 2);
        large_number(game, text::strlen(&cnt) * 0xc + 0x21c, 0x1a0, have - qty, 0x2a, 0, 1, 2, 0);
    };
    match quantity_box(game, x, y, img, &prompt, unit, |_, q| q <= have, &extra) {
        Some(qty) => {
            set_gold(game, g + qty * unit);
            set_inv(game, id, have - qty);
            true
        }
        None => false,
    }
}

/// Port of `FUN_004449cd(army, amount)`: a donation. 100 gold or more also
/// gives every member in reserve some EXP.
fn army_donate(game: &mut Game, amount: i32) {
    let difficulty = game.campaign.difficulty;
    let a = game.battle.army_mut(Side::Player);
    a.gold -= amount;
    if (a._d6 as u32) < 0x1fffff {
        a._d6 += amount;
    }
    a._d4 += 1;
    if amount <= 99 {
        return;
    }
    let base = match difficulty {
        1 => 0x32,
        2 => 0x28,
        3 => 0x1e,
        4 => 0x14,
        _ => 10,
    };
    let bonus = (base + (amount - 100) / 100).min(0x96);
    for i in 0..game.battle.army(Side::Player).units.len() {
        let r = UnitRef::new(Side::Player, i);
        let u = game.battle.unit_mut(r);
        if u.flags & (flag::DEAD | flag::RESERVE | flag::DEPLOYED | flag::SUMMON) == 0 && u.npc != 1 {
            u.exp = u.exp.wrapping_add(bonus as u16);
        }
        if u.exp > 999 {
            u.exp = 999;
        }
        if u.exp_next <= u.exp && u.level < 99 {
            game.battle.level_up(r, false);
        }
    }
}

/// Port of `FUN_00413c01`: the donation box.
pub fn donate(game: &mut Game) -> Result<()> {
    let g = gold(game);
    let title = game.msg(0xd1);
    keeper_window(game, 0x99, &title, false, true)?;
    let m = game.res().master.clone();
    if game.campaign.donated == 1 || g == 0 {
        let msg = game.msg(if game.campaign.donated == 1 { 0x65 } else { 0x68 });
        text::large(game, 0x95, 0x3e, &msg, 0x2a, 0, 1, 2);
        large_number(game, 0x1e0, 0xcd, if game.campaign.donated == 1 { g } else { 0 }, 0xb3, 0, 1, 2, 0);
        gfx::present(game);
        gfx::fade(game, &m, 0, -4);
        ui::click_to_continue(game, 0);
        return Ok(());
    }
    game.scr.copy_page(1, 0);
    let mut amount = g / 10;
    if amount < 100 && g > 99 {
        amount = 100;
    }
    if g < 10 {
        amount = g;
    }
    let s = format_msg(&game.msg(0x69), &[FmtArg::D(amount)]);
    text::large(game, 0x95, 0x3e, &s, 0x2a, 0, 1, 2);
    large_number(game, 0x1e0, 0xcd, g, 0xb3, 0, 1, 2, 0);
    gfx::present(game);
    cursor::capture(game);
    let q = game.msg(0x6b);
    if ui::yes_no(game, 200, 0x116, true, &q, false, true) {
        army_donate(game, amount);
        game.campaign.donated = 1;
        game.scr.set_page(1);
        large_number(game, 0x1e0, 0xcd, g - amount, 0xb3, 0, 1, 2, 0);
        let thanks = game.msg(0x6c);
        text::large(game, 0x95, 0x3e, &thanks, 0x2a, 0, 1, 2);
        gfx::present(game);
        game.scr.set_page(0);
        ui::click_to_continue(game, 0);
    }
    Ok(())
}

/// Revival cost of a unit: its power rating scaled by difficulty (`FUN_00413f6d`).
fn revive_cost(game: &Game, r: UnitRef) -> i32 {
    let p = game.battle.power(game.battle.unit(r), 0, 0);
    let c = match game.campaign.difficulty {
        1 => p / 10,
        2 => p / 5,
        4 => p << 1,
        5 => p * 3,
        _ => p,
    };
    c.max(1)
}

/// `FUN_0043b351` (dead members) / `FUN_0043b1e5` (members who may change class).
fn temple_list(game: &mut Game, mode: i32) -> Vec<UnitRef> {
    let n = game.battle.army(Side::Player).units.len();
    (0..n)
        .map(|i| UnitRef::new(Side::Player, i))
        .filter(|&r| {
            let (f, npc) = (game.battle.unit(r).flags, game.battle.unit(r).npc);
            if mode == 0 {
                // FUN_0043b351
                f & flag::DEAD != 0 && f & flag::SUMMON == 0
            } else {
                // FUN_0043b1e5
                game.battle.promotion_available(r) && f & (flag::DEAD | flag::RESERVE | flag::SUMMON) == 0 && npc != 1
            }
        })
        .collect()
}

/// Port of `FUN_00413f6d(mode)`: the revival spring (0) or class change (1).
pub fn temple(game: &mut Game, mode: i32) -> Result<()> {
    let mut g = gold(game);
    let mut list = temple_list(game, mode);
    let title = game.msg(0xd1);
    keeper_window(game, 0x99, &title, false, mode == 0)?;
    let m = game.res().master.clone();
    if list.is_empty() {
        let msg = game.msg(if mode != 0 { 0xd2 } else { 0x66 });
        text::large(game, 0x95, 0x3e, &msg, 0x2a, 0, 1, 2);
        if mode == 0 {
            large_number(game, 0x1e0, 0xcd, g, 0xb3, 0, 1, 2, 0);
        }
        gfx::present(game);
        gfx::fade(game, &m, 0, -4);
        ui::click_to_continue(game, 0);
        return Ok(());
    }
    let panel = game.res().ui_image(0xd9)?;
    let py0 = -(panel.height as i32); // `-(*(short *)(img + 2) + 1)`: the header holds height - 1
    let py = py0 + 0x1e0;
    gfx::blit(game.scr.page(), &panel, 0, py, false);
    game.scr.copy_page(2, 0);
    let thumb = game.res().ui_image(THUMB).ok();
    let (mut redraw, mut moved, mut delay, mut fade_pending) = (true, false, false, true);
    let (mut sel, mut top) = (0usize, 0usize);
    let mut t_delay = 0;
    let mut cost = 0;
    loop {
        let mut action = 0;
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        let n = list.len();
        if redraw {
            game.scr.copy_page(0, 2);
            super::party::unit_info_panel(game, 0, py, &list, top, sel);
            let mut ty = py0 + 0x208;
            if top + sel > 0 {
                ty += ((top + 1 + sel) as i32 * 0x87) / n as i32;
            }
            if let Some(t) = &thumb {
                gfx::blit(game.scr.page(), t, 0xda, ty, false);
            }
            if mode == 0 {
                large_number(game, 0x1e0, 0xcd, g, 0xb3, 0, 1, 2, 0);
                cost = revive_cost(game, list[top + sel]);
                let nm = s2data::text::cstr(&game.battle.unit(list[top + sel]).name).to_vec();
                let s = format_msg(&game.msg(0x67), &[FmtArg::S(&nm), FmtArg::D(cost)]);
                text::large(game, 0x95, 0x3e, &s, 0x2a, 0, 1, 2);
            } else {
                let s = game.msg(0xd3);
                text::large(game, 0x95, 0x3e, &s, 0x2a, 0, 1, 2);
            }
            // FUN_0043399f(-1, 0), then FUN_0046eb48: two presents.
            cursor::draw_present(game, None, false);
            gfx::present(game);
            if fade_pending {
                gfx::fade(game, &m, 0, -4);
                fade_pending = false;
            }
            moved = false;
            redraw = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if delay {
            game.wait_until(t_delay, 10);
            delay = false;
        }
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
                    if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        action = 1;
                    } else if (0xfa..=0x1bd).contains(&x) && py0 + 0x1f6 <= y && y < py0 + 0x2be {
                        let row = ((y - py - 0x16) / 0x14) as usize;
                        if top + row < n {
                            if row == sel {
                                if e.mods & ev::M_LBUTTON != 0 {
                                    game.wait_event(ev::MOUSE_UP);
                                    action = 0x1c;
                                }
                            } else {
                                redraw = true;
                                sel = row;
                            }
                        }
                    } else if e.mods & ev::M_LBUTTON != 0 && 0xd9 < x && x < 0xee && py0 + 0x1ef <= y && y < py0 + 0x2c4 {
                        if y <= py0 + 0x206 {
                            if sel >= 1 {
                                sel -= 1;
                                redraw = true;
                            } else if top > 0 {
                                top -= 1;
                                redraw = true;
                            }
                            if redraw {
                                delay = true;
                                t_delay = game.ticks();
                            }
                        } else if y >= py0 + 0x2ad {
                            if sel + top + 1 < n {
                                if sel < 9 {
                                    sel += 1;
                                } else {
                                    top += 1;
                                }
                                delay = true;
                                redraw = true;
                                t_delay = game.ticks();
                            }
                        } else if n > 10 && py0 + 0x208 <= y && y <= py0 + 0x2ab {
                            let mut t = ((y - py - 0x28) * n as i32) / 0xa3;
                            if (n as i32) < t + 10 {
                                t = n as i32 - 10;
                            }
                            top = t.max(0) as usize;
                            sel = if y < py0 + 0x28f { 0 } else { 9 };
                            redraw = true;
                        }
                    }
                }
                _ => {}
            }
        }
        match action {
            1 => break,
            0x1c => {
                cursor::erase(game);
                let r = list[top + sel];
                if mode == 0 {
                    if g < cost {
                        let s = game.msg(0x68);
                        text::large(game, 0x95, 0x5c, &s, 0x2a, 0, 1, 2);
                        cursor::draw_present(game, None, false);
                    } else {
                        let q = game.msg(0x6a);
                        if ui::yes_no(game, 0xdb, sel as i32 * 0x14 + 0xd9, true, &q, false, false) {
                            game.battle.unit_mut(r).flags &= !flag::DEAD;
                            g -= cost;
                            set_gold(game, g);
                            list = temple_list(game, 0);
                            if list.is_empty() {
                                break;
                            }
                            if list.len() < top + 10 && top > 0 {
                                top -= 1;
                            }
                            if list.len() <= top + sel && sel > 0 {
                                sel -= 1;
                            }
                        }
                        redraw = true;
                    }
                } else {
                    if super::party::class_change(game, r, true)? == 2 {
                        list = temple_list(game, 1);
                        if list.is_empty() {
                            break;
                        }
                        if list.len() < top + 10 && top > 0 {
                            top -= 1;
                        }
                        if list.len() <= top + sel && sel > 0 {
                            sel -= 1;
                        }
                    }
                    redraw = true;
                    keeper_window(game, 0x99, &title, false, false)?;
                    gfx::blit(game.scr.page(), &panel, 0, py, false);
                    game.scr.copy_page(2, 0);
                }
            }
            _ => {}
        }
    }
    game.flush_events(0x1f);
    Ok(())
}
