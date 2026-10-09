//! Leaving a town for the next stage: the easy-mode revival (`FUN_00413e9a`),
//! the march on the world map and the loading screen (`FUN_00415fd2` with
//! `FUN_004156b1`, `FUN_00415d1d`, `FUN_00415ec8`, `FUN_00415e41`, `FUN_00415f8a`)
//! and the deployment that follows.

use anyhow::{Context, Result};
use s2data::sprite::Image;

use super::code;
use crate::battle::unit::flag;
use crate::battle::{Side, UnitRef};
use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::res::load_picture;
use crate::text;

/// Port of `FUN_00413e9a`: on the easiest difficulty fallen members come
/// back, each costing a tenth of its power rating in gold.
pub fn easy_penalty(game: &mut Game) {
    if game.campaign.difficulty != 1 {
        return;
    }
    let mut gold = game.battle.army(Side::Player).gold;
    let dead: Vec<usize> = game
        .battle
        .army(Side::Player)
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.flags & flag::DEAD != 0 && u.flags & flag::SUMMON == 0)
        .map(|(i, _)| i)
        .collect();
    if dead.is_empty() {
        return;
    }
    for i in dead {
        let r = UnitRef::new(Side::Player, i);
        game.battle.unit_mut(r).flags &= !flag::DEAD;
        let p = game.battle.power(game.battle.unit(r), 0, 0);
        gold -= p / 10;
    }
    game.battle.army_mut(Side::Player).gold = gold.max(0);
}

/// `FUN_00414d39(page)`: the 640x480 window of the world map at its scroll.
fn draw_world(game: &mut Game, page: usize, map: &Image, scroll: (i32, i32)) {
    let dst = &mut game.scr.pages[page];
    for y in 0..480usize {
        let sy = scroll.1 as usize + y;
        if sy >= map.height {
            break;
        }
        let row = &map.pixels[sy * map.width..(sy + 1) * map.width];
        let sx = scroll.0 as usize;
        let n = 640.min(map.width.saturating_sub(sx));
        dst.pixels[y * 640..y * 640 + n].copy_from_slice(&row[sx..sx + n]);
    }
}

/// The 2x zoom of `FUN_004156b1` / `FUN_00415d1d`: page 1's 320x240 window at
/// (`x`, `y`) doubled onto page 0.
fn zoom_to_page0(game: &mut Game, x: i32, y: i32) {
    let src = game.scr.pages[1].pixels.clone();
    let dst = &mut game.scr.pages[0].pixels;
    for row in 0..240usize {
        let s = (y as usize + row) * 640 + x as usize;
        let d = row * 2 * 640;
        for col in 0..320usize {
            let c = src[s + col];
            dst[d + col * 2] = c;
            dst[d + col * 2 + 1] = c;
        }
        let (a, b) = dst.split_at_mut(d + 640);
        b[..640].copy_from_slice(&a[d..d + 640]);
    }
}

/// `FUN_00415ec8(pct)`: the loading bar frame (with `FUN_00415e41`).
fn load_bar_frame(game: &mut Game, pct: i32) {
    let p = game.scr.page();
    gfx::rect(p, 0xd7, 0x5a, 0x1a9, 0x74, 8, false);
    gfx::rect(p, 0xd8, 0x5b, 0x1a8, 0x73, 5, false);
    gfx::rect(p, 0xd9, 0x5c, 0x1a7, 0x72, 0xfc, true);
    gfx::hline(p, 0x5e, 0xdb, 0x1a5, 0x33);
    gfx::hline(p, 0x70, 0xdb, 0x1a5, 5);
    gfx::vline(p, 0xdb, 0x5f, 0x6f, 0x33);
    gfx::vline(p, 0x1a5, 0x5e, 0x6f, 0x33);
    load_bar(game, pct);
}

/// `FUN_00415e41(pct)`.
fn load_bar(game: &mut Game, pct: i32) {
    let p = game.scr.page();
    gfx::rect(p, 0xdc, 0x5f, 0x1a4, 0x6f, 0xfc, true);
    gfx::rect(p, 0xdc, 0x5f, pct * 2 + 0xdc, 0x6f, 0x1e, true);
    text::small(game, 0x130, 0x5f, format!("{pct}%").as_bytes(), 3, 0, 1, 2);
    gfx::present(game);
}

/// Port of `FUN_004156b1`: a dotted trail from the current place to the
/// next stage's marker on the world map, then zoom in on the destination.
/// Returns the zoom window origin (`DAT_00492b68`, `DAT_00492b64`).
fn march_animation(game: &mut Game) -> Result<(i32, i32)> {
    let w = game.ui.world.as_ref().context("world map not loaded")?;
    let (from, to) = (w.location(game.campaign.location), w.location(game.campaign.next_stage));
    let (mw, mh) = (w.map.width as i32, w.map.height as i32);
    let cx = (to.x as i32 + from.x as i32) / 2;
    let cy = (to.y as i32 + from.y as i32) / 2;
    let mut sx = cx - 0x140;
    let mut sy = cy - 0xf0;
    if sx < 0 {
        sx = 0;
    } else if mw < cx + 0x13f {
        sx = mw - 0x280;
    }
    if sy < 0 {
        sy = 0;
    } else if mh < cy + 0xef {
        sy = mh - 0x1e0;
    }
    let map = w.map.clone();
    let (dest_marker, trail_marker) = (w.marker.clone(), w.marker2.clone());
    let pal = w.palettes[4].clone();
    if let Some(w) = game.ui.world.as_mut() {
        w.scroll = (sx, sy);
    }
    draw_world(game, 1, &map, (sx, sy));
    let (tx, ty) = (to.x as i32 - sx, to.y as i32 - sy);
    gfx::blit(&mut game.scr.pages[1], &dest_marker, tx - 7, ty - 0x1e, false);
    let (fx, fy) = (from.x as i32 - sx, from.y as i32 - sy);
    gfx::present_page(game, 1);
    gfx::fade(game, &pal, 0, 0);
    let mut t = game.ticks() - 10;
    let mode = if fx == tx {
        1
    } else if fy == ty {
        2
    } else {
        3
    };
    let slope = if mode == 3 { ((fy - ty).abs() * 100) / (fx - tx).abs() } else { 0 };
    let (mut x, mut y) = (fx, fy);
    while x != tx || y != ty {
        {
            let p = &mut game.scr.pages[1];
            gfx::rect(p, x - 3, y - 3, x + 2, y + 2, 9, false);
            gfx::rect(p, x - 2, y - 2, x + 1, y + 1, 3, true);
        }
        game.scr.copy_page(0, 1);
        gfx::blit(&mut game.scr.pages[0], &trail_marker, x - 7, y - 0x19, false);
        while game.still_waiting(t, 10) {
            game.sys.idle();
        }
        game.scr.set_page(0);
        gfx::present(game);
        t = game.ticks();
        let (mut nx, mut ny) = (tx, ty);
        match mode {
            1 => {
                if (y - ty).abs() > 0x10 {
                    nx = x;
                    ny = if ty < y { y - 0x10 } else { y + 0x10 };
                }
            }
            2 => {
                if (x - tx).abs() > 0x10 {
                    ny = y;
                    nx = if tx < x { x - 0x10 } else { x + 0x10 };
                }
            }
            _ => {
                if (y - ty) * (y - ty) + (x - tx) * (x - tx) > 0x3f {
                    if slope < 0x33 {
                        x += if tx < x { -8 } else { 8 };
                        ny = ((ty - fy) * (x - fx)) / (tx - fx) + fy;
                        nx = x;
                    } else {
                        y += if ty < y { -8 } else { 8 };
                        nx = ((tx - fx) * (y - fy)) / (ty - fy) + fx;
                        ny = y;
                    }
                }
            }
        }
        x = nx;
        y = ny;
    }
    {
        let p = &mut game.scr.pages[1];
        gfx::rect(p, tx - 3, ty - 3, tx + 2, ty + 2, 9, false);
        gfx::rect(p, tx - 2, ty - 2, tx + 1, ty + 1, 3, true);
        gfx::blit(p, &trail_marker, tx - 7, ty - 0x19, false);
    }
    gfx::present_page(game, 1);
    game.scr.set_page(0);
    let zx = if tx - 0xa0 < 0 {
        0
    } else if 0x280 < tx + 0x9f {
        0x140
    } else {
        tx - 0xa0
    };
    let zy = if ty - 0x78 < 0 {
        0
    } else if 0x1e0 < ty + 0x77 {
        0xf0
    } else {
        ty - 0x78
    };
    zoom_to_page0(game, zx, zy);
    gfx::present(game);
    Ok((zx, zy))
}

/// Port of `FUN_00415fd2`: march, loading screen, next stage, briefing, deployment.
pub fn march_and_deploy(game: &mut Game) -> Result<i32> {
    // The disc the next stage is on (route 1 after six battles, or the route-0
    // stages 0x3c/0x3d).
    let c = &game.campaign;
    let disc = ((c.route == 1 && 6 < c.battles_fought) || (c.route == 0 && (c.next_stage == 0x3c || c.next_stage == 0x3d))) as u8;
    cursor::enable(game, false);
    super::stop_music(game);
    let (zx, zy) = march_animation(game)?;
    let scroll = game.ui.world.as_ref().map(|w| w.scroll).unwrap_or((0, 0));
    // FUN_0040ff73: the world-map pictures are freed.
    game.ui.world = None;
    let (dark, pal) = load_picture(&game.data.arcs, -0x12, 3)?;
    game.scr.copy_page(1, 0);
    game.scr.set_page(2);
    game.scr.copy_page(2, 1);
    let banner = game.res().ui_image(0xcb)?;
    gfx::blit(game.scr.page(), &banner, 0xb0, 0x14, false);
    let msg = game.msg(0xe7);
    let x = (banner.width as i32) / 2 - text::strlen(&msg) * 6 + 0xb0;
    let y = (banner.height as i32) / 2 + 8;
    text::large(game, x, y, &msg, 9, 0, 1, 2);
    load_bar_frame(game, 10);
    game.scr.set_page(0);
    // FUN_00415d1d: the darker map at the same zoom on page 0.
    draw_world(game, 1, &dark, scroll);
    zoom_to_page0(game, zx, zy);
    game.scr.set_page(2);
    load_bar(game, 0x1e);
    // FUN_00415f8a: load the next stage.
    game.campaign.turn = 1;
    let next = game.campaign.next_stage;
    super::stage::load_stage(game, next, 10)?;
    load_bar(game, 0x3c);
    super::stage::map_init(game)?;
    load_bar(game, 0x5a);
    load_bar(game, 100);
    game.scr.set_page(0);
    gfx::present(game);
    super::narration::narration(game, 2);
    gfx::fade(game, &pal, 1, 0);
    if disc != game.campaign.disc {
        // FUN_0046de06: without a CD device the disc is just noted; with one the
        // original only probes DISK.ID. Either way a blank screen fades out.
        game.scr.clear();
        gfx::present(game);
        let m = game.res().master.clone();
        gfx::set_palette(game, &m);
        if !game.cfg.cd_mode {
            game.campaign.disc = disc;
        }
        gfx::fade(game, &m, 1, 0);
    }
    cursor::enable(game, true);
    game.campaign.between_stages = false;
    game.campaign.donated = 0;
    deploy(game)?;
    Ok(code::FROM_WORLD_MAP)
}

/// `FUN_0041641c` + `FUN_00444bea` + `FUN_0041671c` (see `flow::deploy`).
fn deploy(game: &mut Game) -> Result<()> {
    super::save::sync_battle_mirrors(game);
    super::deploy::deploy(game)
}
