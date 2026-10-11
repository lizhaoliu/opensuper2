//! Movement and one-off battle animations: the move dispatcher with the
//! flying and teleport routines, projectiles, the class-change reveal, the
//! screen shake and the location name of a cell.

use s2data::sprite::Image;

use crate::game::Game;
use crate::gfx;

use super::super::{Side, UnitRef};
use super::unit::{grab, unit_screen_pos, ACT_WALKING};

/// `DAT_00494652`-independent window copy (FUN_00424c5f) into the current
/// page, after rebuilding the surface if the units changed.
pub fn draw_window(game: &mut Game) {
    super::sync_view(game);
    let page = game.scr.cur;
    let Game { battle, scr, .. } = game;
    if let Some(mv) = battle.view.map.as_ref() {
        mv.blit_window(&mut scr.pages[page]);
    }
}

fn sound(game: &mut Game, id: u16, slot: usize) -> usize {
    game.sys.audio.play_sound(&game.data.arcs, id, slot)
}

/// Port of FUN_0044cccd's presentation: move `r` along `steps` with the
/// routine for its movement type (3, 4, 5 fly; 2 teleports to the last
/// step; others walk).
pub fn animate_move(game: &mut Game, r: UnitRef, steps: &[(i32, i32, u8)]) {
    let u = game.battle.unit(r).clone();
    match game.battle.move_type(&u) {
        3..=5 => animate_fly(game, r, steps),
        2 => {
            if let Some(&(x, y, _)) = steps.last() {
                animate_teleport(game, r, x, y);
            }
        }
        _ => super::animate_walk(game, r, steps),
    }
}

/// Port of FUN_004573f6: the ground shadow, the mount image lifted 4 rows
/// (3 with movement override 5) and the unit's raw frame lifted 4 rows.
fn draw_mounted(game: &mut Game, r: UnitRef, mount: Option<Image>) {
    super::draw_ground_shadow(game, r);
    let page = game.scr.cur;
    let (x, y, dx, flags, shift) = placement(game, r);
    let Some(mv) = game.battle.view.map.as_ref() else { return };
    if let Some(m) = mount {
        let lift = if flags & super::super::unit::flag::MOVE5 == 0 { 4 } else { 3 };
        let (px, py) = unit_screen_pos(mv, x, y, &m, lift, dx);
        game.scr.pages[page].blit(&m, (px + shift.0) as isize, (py + shift.1) as isize);
    }
    if let Some(s) = game.battle.view.units[r.side.index()].get(r.idx).and_then(|g| g.sprite.as_ref()) {
        let (px, py) = unit_screen_pos(mv, x, y, s, 4, dx);
        game.scr.pages[page].blit(s, (px + shift.0) as isize, (py + shift.1) as isize);
    }
}

/// Cell, frame offset and flags of a unit, and the smooth-motion shift it
/// is drawn with (its cell then being the nudge's).
fn placement(game: &Game, r: UnitRef) -> (i32, i32, i32, u32, (i32, i32)) {
    let u = game.battle.unit(r);
    match super::smooth::nudge_of(game, r) {
        Some(n) => (n.cell.0, n.cell.1, u.remap as i32, u.flags, n.shift),
        None => (u.x, u.y, u.remap as i32, u.flags, (0, 0)),
    }
}

fn sysimg(game: &Game, slot: usize) -> Option<Image> {
    match game.res.as_ref() {
        Some(r) => r.ui_image(slot).ok(),
        None => game.battle.view.assets.as_deref().and_then(|a| a.sysimg.get(slot).cloned()),
    }
}

/// Port of FUN_00457345: a flyer on its alternating mount (sysimg 23/24);
/// `step` flips the frame now, otherwise every 19 cs.
pub fn draw_flyer(game: &mut Game, r: UnitRef, step: bool) {
    let m = sysimg(game, 0x17 + game.battle.view.mount_phase);
    draw_mounted(game, r, m);
    if step {
        game.battle.view.mount_phase ^= 1;
    } else if !game.still_waiting(game.battle.unit(r).anim_timer, 0x13) {
        game.battle.unit_mut(r).anim_timer = game.ticks();
        game.battle.view.mount_phase ^= 1;
    }
}

/// Port of FUN_004573cb: a unit riding the flying sword (sysimg 39 + facing).
pub fn draw_sword_rider(game: &mut Game, r: UnitRef) {
    let f = (game.battle.unit(r).facing & 3) as usize;
    let m = sysimg(game, 0x27 + f);
    draw_mounted(game, r, m);
}

/// FUN_0042665a with the unit's current frame (`unit+0xad`): the raw
/// sprite lifted `lift` rows, as flyers are drawn while they move.
pub fn draw_lifted(game: &mut Game, r: UnitRef, lift: i32) {
    let page = game.scr.cur;
    let (x, y, dx, _, shift) = placement(game, r);
    if let (Some(mv), Some(s)) =
        (game.battle.view.map.as_ref(), game.battle.view.units[r.side.index()].get(r.idx).and_then(|g| g.sprite.as_ref()))
    {
        let (px, py) = unit_screen_pos(mv, x, y, s, lift, dx);
        game.scr.pages[page].blit(s, (px + shift.0) as isize, (py + shift.1) as isize);
    }
}

/// Port of FUN_0044cb5f: a flying unit takes off (`down`: lands) in four
/// frames of 8 cs over the map window with its ground shadow: move type 4
/// on its mount, 5 on the flying sword (sound 0x84), the others stepping
/// their frames lifted 1, 2, 3, 4 rows (3, 2, 1, 0 landing; sound 0x85).
pub fn lift_off(game: &mut Game, r: UnitRef, down: bool) {
    if !super::super::actions::presenting(game) {
        return;
    }
    let u = game.battle.unit(r).clone();
    let mt = game.battle.move_type(&u);
    // Drawn over the window: keep the unit out of the surface meanwhile.
    let saved_act = u.act;
    if !matches!(saved_act, 1 | 3 | 5) {
        game.battle.unit_mut(r).act = super::unit::ACT_COMBAT;
    }
    let mut lift = if down { 3 } else { 1 };
    let mut slot = 0usize;
    for _ in 0..4 {
        draw_window(game);
        super::draw_ground_shadow(game, r);
        match mt {
            4 => {
                slot = sound(game, 0x84, slot);
                draw_flyer(game, r, true);
            }
            5 => {
                slot = sound(game, 0x84, slot);
                draw_sword_rider(game, r);
            }
            _ => {
                slot = sound(game, 0x85, slot);
                let f = game.battle.unit(r).facing;
                super::step_unit(game, r, f);
                draw_lifted(game, r, lift);
            }
        }
        let t0 = game.battle.unit(r).anim_timer;
        game.wait_until(t0, 8);
        gfx::present(game);
        game.battle.unit_mut(r).anim_timer = game.ticks();
        lift += if down { -1 } else { 1 };
    }
    game.battle.unit_mut(r).act = saved_act;
}

/// Port of FUN_0044c631: flying movement, 12 cs per cell. Hidden enemy
/// units move without being shown.
pub fn animate_fly(game: &mut Game, r: UnitRef, steps: &[(i32, i32, u8)]) {
    let saved_act = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = ACT_WALKING;
    let u = game.battle.unit(r).clone();
    let mt = game.battle.move_type(&u);
    let hidden = game.battle.army(r.side).side == Side::Enemy && game.battle.is_hidden(r.side, &u);
    let mut slot = 0usize;
    for &(x, y, dir) in steps {
        let from = super::smooth::From::now(game, Some(r));
        {
            let u = game.battle.unit_mut(r);
            u.x = x;
            u.y = y;
        }
        game.scr.set_page(0);
        let pre = super::smooth::Snapshot::take(game);
        if !hidden {
            game.battle.map_mut().center_on(x, y, true);
            draw_window(game);
            super::draw_ground_shadow(game, r);
        }
        match mt {
            4 | 5 => {
                slot = sound(game, 0x84, slot);
                if game.battle.unit(r).facing != dir {
                    super::step_unit(game, r, dir);
                }
                if !hidden {
                    if mt == 4 {
                        draw_flyer(game, r, true);
                    } else {
                        draw_sword_rider(game, r);
                    }
                }
            }
            _ => {
                slot = sound(game, 0x85, slot);
                super::step_unit(game, r, dir);
                if !hidden {
                    draw_lifted(game, r, 4);
                }
            }
        }
        let t0 = game.battle.unit(r).anim_timer;
        if !hidden {
            // The frame as drawn above, for the in-between positions.
            let start = super::smooth::step_start(game, t0, 0xc);
            super::smooth::tween(game, from, &pre, start, 120, &mut |game| {
                draw_window(game);
                super::draw_ground_shadow(game, r);
                match mt {
                    4 => draw_flyer(game, r, true),
                    5 => draw_sword_rider(game, r),
                    _ => draw_lifted(game, r, 4),
                }
            });
        }
        game.wait_until(t0, 0xc);
        gfx::present(game);
        game.battle.unit_mut(r).anim_timer = game.ticks();
    }
    game.battle.unit_mut(r).act = saved_act;
}

/// Port of FUN_0044c963: teleport. The unit dissolves (colour 0xc2) on its
/// cell and reappears on (x, y), with sound 0x68 each time; a hidden enemy
/// is not shown.
pub fn animate_teleport(game: &mut Game, r: UnitRef, x: i32, y: i32) {
    let saved_act = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = ACT_WALKING;
    let shown = |game: &Game| {
        let u = game.battle.unit(r);
        !game.battle.is_hidden(r.side, u) || game.battle.army(r.side).side == Side::Player || game.battle.vs
    };
    game.scr.set_page(0);
    let (ox, oy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    game.battle.map_mut().center_on(ox, oy, true);
    draw_window(game);
    sound(game, 0x68, 0);
    if shown(game) {
        super::fade_unit(game, r, 0xc2, false);
    }
    {
        let u = game.battle.unit_mut(r);
        u.x = x;
        u.y = y;
    }
    game.battle.map_mut().center_on(x, y, true);
    draw_window(game);
    super::capture(game, r);
    sound(game, 0x68, 0);
    if shown(game) {
        super::fade_unit(game, r, 0xc2, true);
    }
    game.battle.unit_mut(r).act = saved_act;
}

/// Port of FUN_00439651: a projectile from `att` to `target`. `base` is
/// the first sysimg slot of its images; mode 1 picks one of 16
/// directional images, mode 2 always the first. Moves 32 px (straight) or
/// 16 px along the major axis per frame, 1 cs apart, restoring what it
/// covered; the last image stays on the page.
pub fn projectile(game: &mut Game, att: UnitRef, target: UnitRef, base: usize, mode: i32) {
    let (ax, ay) = (game.battle.unit(att).x, game.battle.unit(att).y);
    let (tx, ty) = (game.battle.unit(target).x, game.battle.unit(target).y);
    let (mut x1, mut y1) = super::cell_screen(game, ax, ay);
    let (mut x2, mut y2) = super::cell_screen(game, tx, ty);
    x1 += 0x20;
    x2 += 0x20;
    y1 -= 0x20;
    y2 -= 0x20;
    let straight = |x1: i32, y1: i32, x2: i32, y2: i32, d: &mut i32| -> i32 {
        if x1 == x2 {
            if mode == 1 {
                *d = if y1 <= y2 { 9 } else { 1 };
            }
            1
        } else if y1 == y2 {
            if mode == 1 {
                *d = if x1 <= x2 { 5 } else { 13 };
            }
            2
        } else {
            3
        }
    };
    let mut d = 0i32;
    let mut slope = 0i32;
    let mut kind = straight(x1, y1, x2, y2, &mut d);
    if kind == 3 {
        slope = (y1 - y2).abs() * 100 / (x1 - x2).abs();
        if mode == 1 {
            let quad = |a: i32, b: i32, c: i32, e: i32| {
                if x1 < x2 && y2 < y1 {
                    a
                } else if x2 < x1 && y2 < y1 {
                    b
                } else if x2 < x1 && y1 < y2 {
                    c
                } else {
                    e
                }
            };
            d = if slope < 7 {
                if x2 <= x1 { 13 } else { 5 }
            } else if slope < 0x1e {
                quad(4, 14, 12, 6)
            } else if slope < 0x53 {
                quad(3, 15, 11, 7)
            } else if slope < 0x15e {
                quad(2, 16, 10, 8)
            } else if y1 <= y2 {
                9
            } else {
                1
            };
        }
    }
    if d < 10 {
        if d == 9 {
            y1 += 0x30;
        }
    } else {
        x1 -= 0x20;
    }
    d = if mode == 2 { 0 } else { d - 1 };
    let (mut px, mut py) = (x1, y1);
    if kind == 3 {
        kind = straight(x1, y1, x2, y2, &mut d);
    }
    let Some(img) = sysimg(game, (base as i32 + d).max(0) as usize) else { return };
    let (w, h) = (img.width, img.height);
    let mut t = game.ticks();
    let mut prev: Option<(i32, i32, Image)> = None;
    let mut guard = 0;
    while (px, py) != (x2, y2) && guard < 4096 {
        guard += 1;
        match kind {
            1 => {
                if (py - y2).abs() < 0x21 {
                    (px, py) = (x2, y2);
                } else if y2 < py {
                    py -= 0x20;
                } else {
                    py += 0x20;
                }
            }
            2 => {
                if (px - x2).abs() < 0x21 {
                    (px, py) = (x2, y2);
                } else if x2 < px {
                    px -= 0x20;
                } else {
                    px += 0x20;
                }
            }
            _ => {
                if (py - y2) * (py - y2) + (px - x2) * (px - x2) < 0x40 {
                    (px, py) = (x2, y2);
                } else if slope < 0x33 {
                    px += if x2 < px { -0x10 } else { 0x10 };
                    py = (y2 - y1) * (px - x1) / (x2 - x1) + y1;
                } else {
                    py += if y2 < py { -0x10 } else { 0x10 };
                    px = (x2 - x1) * (py - y1) / (y2 - y1) + x1;
                }
            }
        }
        let page = game.scr.cur;
        if let Some((ox, oy, saved)) = prev.take() {
            game.scr.pages[page].blit_opaque(&saved, ox as isize, oy as isize);
        }
        let saved = grab(&game.scr.pages[page], px, py, w, h);
        game.scr.pages[page].blit(&img, px as isize, py as isize);
        prev = Some((px, py, saved));
        gfx::present(game);
        game.wait_until(t, 1);
        t = game.ticks();
    }
}

/// Port of FUN_004563e8: after a class change the new sprite appears from
/// the top, 60 rows first and then 1, 1, 2, 2, 3 ... more rows every 5 cs.
pub fn wipe_in(game: &mut Game, r: UnitRef) {
    // Lifted off the map surface while it appears (as in combat).
    let saved_act = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = super::unit::ACT_COMBAT;
    draw_window(game);
    let Some(full) = game.battle.view.units[r.side.index()].get(r.idx).and_then(|g| g.sprite.clone()) else {
        game.battle.unit_mut(r).act = saved_act;
        return;
    };
    let (w, h) = (full.width, full.height);
    let mut t = game.ticks();
    let mut inc = 2usize;
    let mut rows = 0x3c;
    while rows < h {
        let mut part = Image::new(w, h);
        part.pixels[..rows * w].copy_from_slice(&full.pixels[..rows * w]);
        game.battle.view.units[r.side.index()][r.idx].sprite = Some(part);
        super::draw_unit(game, r, 0);
        game.wait_until(t, 5);
        gfx::present(game);
        t = game.ticks();
        rows += inc / 2;
        inc += 1;
    }
    game.battle.view.units[r.side.index()][r.idx].sprite = Some(full);
    super::draw_unit(game, r, 0);
    gfx::present(game);
    game.battle.unit_mut(r).act = saved_act;
}

/// Port of FUN_00466e71: shake the scene in page 1 left and right by 2 px,
/// `n` frames of 2 cs, with the sound of effect `effect` (n = 100 means 14
/// frames with effect 4's sound).
pub fn shake_screen(game: &mut Game, n: i32, effect: usize) {
    game.battle.view.fx_last = game.ticks();
    let (mut n, effect) = if n == 100 { (0xe, 4) } else { (n, effect) };
    let snd = game.battle.view.assets.as_deref().map_or(0, |a| a.fx.sound(effect)) as u16;
    sound(game, snd, 0);
    const N: usize = 0x4affe;
    while n > 0 {
        gfx::palette_cycle(game);
        let src = game.scr.pages[1].pixels.clone();
        let dst = &mut game.scr.pages[0].pixels;
        if n & 1 == 0 {
            dst[2..2 + N].copy_from_slice(&src[..N]);
        } else {
            dst[..N].copy_from_slice(&src[2..2 + N]);
        }
        game.scr.set_page(0);
        let t0 = game.battle.view.fx_last;
        game.wait_until(t0, 2);
        gfx::present(game);
        game.battle.view.fx_last = game.ticks();
        n -= 1;
    }
}

/// Port of FUN_004608e2's presentation: summoned unit `r` rises out of the
/// ground beside `caster` — page 1 = window + caster, sound 0x88, then the
/// top `n` + 1 rows of its frame (n = height % 3, +3 each frame, 1 cs apart)
/// standing on its cell without a ground shadow (flag 0x8000000), then the
/// whole frame into both pages.
pub fn summon_rise(game: &mut Game, caster: UnitRef, r: UnitRef) {
    if !super::super::actions::presenting(game) {
        return;
    }
    use super::super::unit::flag;
    let Some(full) = super::sprite_of(game, r) else { return };
    // Kept off the map surface until it has risen (FUN_00440264 at the end).
    let saved_act = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = super::unit::ACT_COMBAT;
    game.battle.unit_mut(r).flags |= flag::SUMMONING;
    let mut t = game.ticks();
    game.scr.set_page(1);
    draw_window(game);
    super::draw_unit(game, caster, 0);
    game.scr.set_page(0);
    sound(game, 0x88, 0);
    let h = full.height;
    let mut n = h % 3;
    while n < h {
        gfx::palette_cycle(game);
        game.scr.copy_page(0, 1);
        let mut part = Image::new(full.width, n + 1);
        part.pixels.copy_from_slice(&full.pixels[..full.width * (n + 1)]);
        super::set_sprite(game, r, part);
        super::draw_unit(game, r, 0);
        game.wait_until(t, 1);
        gfx::present(game);
        t = game.ticks();
        n += 3;
    }
    super::set_sprite(game, r, full);
    game.battle.unit_mut(r).flags &= !flag::SUMMONING;
    game.scr.set_page(1);
    super::draw_unit(game, r, 0);
    gfx::present_page(game, 1);
    game.scr.set_page(0);
    game.scr.copy_page(0, 1);
    game.battle.unit_mut(r).act = saved_act;
    // act 2, FUN_00442022 + FUN_00440264.
    super::capture(game, r);
    super::put_unit(game, r);
}

/// Port of FUN_0046c92c (`won`, effect rules[0xfc]) / FUN_0046c8cb (lost,
/// rules[0xfd]; FUN_0041a765 then stops the music): the stage's last
/// frame — the map window darkened through tint 6 into pages 0 and 1 —
/// with the victory / defeat effect played over it (FUN_0046c98d →
/// FUN_0046c9cd, the effect player without a spell, so 動畫播放 does not
/// apply), then a click (FUN_0046d8a8(0)).
pub fn stage_end(game: &mut Game, won: bool) {
    if !super::super::actions::presenting(game) {
        return;
    }
    let effect = game.battle.cu(if won { 0xfc } else { 0xfd }) as usize;
    draw_window(game);
    let cur = game.scr.cur;
    {
        let Game { battle, scr, .. } = &mut *game;
        let lut = &battle.view.assets().tints[6];
        for p in scr.pages[cur].pixels.iter_mut() {
            *p = lut[*p as usize];
        }
    }
    game.scr.copy_page(1, cur);
    let _ = super::play_effect(game, effect, 0, 0, false);
    crate::ui::click_to_continue(game, 0);
}

/// Port of FUN_0042574c: the location (object terrain) name of a cell,
/// Big5, as the event messages show it.
pub fn location_name(game: &Game, x: i32, y: i32) -> Vec<u8> {
    let (Some(mv), Some(a), Some(t)) = (game.battle.view.map.as_ref(), game.battle.view.assets.as_deref(), game.battle.tables.as_ref()) else {
        return Vec::new();
    };
    let Some(cell) = mv.def.cells.get((y * mv.width() + x).max(0) as usize) else { return Vec::new() };
    let ov = cell.overlay as i32;
    if ov < 1 {
        return Vec::new();
    }
    let attr = a.tiles.attrs.overlay.get(ov as usize - 1).copied().unwrap_or(0);
    let idx = (attr & 0xff) as i32 - 1 + t.terrain.ground as i32;
    t.terrain.names.get(idx.max(0) as usize).map(|s| super::panel::to_big5(s)).unwrap_or_default()
}
