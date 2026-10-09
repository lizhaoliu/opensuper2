//! Between stages: the world-map pictures, the town hot-spot screens (inn,
//! shop, church) and the step to the next stage (`FUN_0040ff7d`).

use anyhow::Result;
use s2data::script::Stage;
use s2data::sprite::Image;
use s2data::Kind;

use super::code;
use crate::cursor::{self, SHAPE_ALT, SHAPE_POINTER};
use crate::game::Game;
use crate::gfx;
use crate::res::load_picture;
use crate::sys::{ev, scan};
use crate::text;
use crate::ui::{self, Wait};

/// One BIN 4 location record (14 bytes at `DAT_00492b60`).
#[derive(Debug, Clone, Default)]
pub struct Location {
    /// Offset of the location's text block in BIN 4.
    pub text_off: u32,
    /// World-map pixel position of the marker.
    pub x: u16,
    pub y: u16,
    /// +10: 1 = the place has a town.
    pub town: u8,
    /// +11..13: number of weapon / armour / item ids for sale.
    pub shop: [u8; 3],
}

/// The pictures and tables `FUN_0040fd3c` loads for the world-map phase.
pub struct World {
    /// `DAT_00492b9c`: DAT 11 world map (1280x907).
    pub map: Image,
    /// `DAT_00492b8c..98`: DAT 12..15 town, inn, shop, church.
    pub scenes: Vec<Image>,
    /// `DAT_00492ba0` / `ba4`: DAT 16 / 17, dimmed inn and church backdrops.
    pub dim_inn: Image,
    pub dim_church: Image,
    /// `DAT_00492ba8` / `bac`: DAT 7 images 5 and 6 (world-map markers).
    pub marker: Image,
    pub marker2: Image,
    /// `DAT_00492b78`: the five 6-bit palettes (town, inn, shop, church, map).
    pub palettes: Vec<Vec<u8>>,
    /// BIN 4 (`DAT_00492bc4`) and its records.
    pub bin4: Vec<u8>,
    pub locations: Vec<Location>,
    /// `DAT_00492b84` / `b88`: world-map scroll in pixels.
    pub scroll: (i32, i32),
    /// `DAT_00492b58`: the location under the pointer in the viewer
    /// (cleared here, kept between viewer visits like the original).
    pub hover: usize,
}

impl World {
    /// The location record of 1-based `loc` (clamped like the original reads).
    pub fn location(&self, loc: u16) -> Location {
        self.locations.get((loc as usize).saturating_sub(1)).cloned().unwrap_or_default()
    }

    /// The text block of a location: name, then the shop id lists, then the
    /// two innkeeper texts.
    pub fn text(&self, loc: u16) -> &[u8] {
        let l = self.location(loc);
        self.bin4.get(l.text_off as usize..).unwrap_or(&[])
    }

    /// `FUN_00410d3a` id lists: the item ids for shop `kind` 1..=3.
    pub fn shop_items(&self, loc: u16, kind: usize) -> Vec<i32> {
        let l = self.location(loc);
        let t = self.text(loc);
        let name_len = t.iter().position(|&c| c == 0).unwrap_or(t.len());
        let skip: usize = l.shop[..kind - 1].iter().map(|&n| n as usize * 2).sum();
        let base = name_len + 1 + skip;
        (0..l.shop[kind - 1] as usize)
            .filter_map(|i| t.get(base + 2 * i..base + 2 * i + 2).map(|b| i16::from_le_bytes([b[0], b[1]]) as i32))
            .collect()
    }

    /// `FUN_004149ff`: innkeeper text 0 or 1.
    pub fn keeper_text(&self, loc: u16, which: usize) -> Vec<u8> {
        let l = self.location(loc);
        let t = self.text(loc);
        let name_len = t.iter().position(|&c| c == 0).unwrap_or(t.len());
        let ids: usize = l.shop.iter().map(|&n| n as usize).sum();
        let p0 = name_len + 1 + ids * 2;
        let first = s2data::text::cstr(t.get(p0..).unwrap_or(&[]));
        if which == 0 {
            first.to_vec()
        } else {
            s2data::text::cstr(t.get(p0 + first.len() + 1..).unwrap_or(&[])).to_vec()
        }
    }
}

/// Port of `FUN_0040fd3c`: load the world-map pictures and BIN 4, and
/// centre the map on the current location.
pub fn world_load(game: &mut Game) -> Result<()> {
    let arcs = &game.data.arcs;
    let (map, pal_map) = load_picture(arcs, -0xb, 3)?;
    let mut scenes = Vec::new();
    let mut palettes = Vec::new();
    for id in [-0xc, -0xd, -0xe, -0xf] {
        let (img, pal) = load_picture(arcs, id, 3)?;
        scenes.push(img);
        palettes.push(pal);
    }
    palettes.push(pal_map);
    let (dim_inn, _) = load_picture(arcs, -0x10, 3)?;
    let (dim_church, _) = load_picture(arcs, -0x11, 3)?;
    let marker = game.res().dat7_image(5)?;
    let marker2 = game.res().dat7_image(6)?;
    let bin4 = arcs.read(Kind::Bin, 4)?;
    let n = i16::from_le_bytes([bin4[0], bin4[1]]).max(0) as usize;
    let locations = (0..n)
        .map(|i| {
            let r = &bin4[2 + i * 14..2 + i * 14 + 14];
            Location {
                text_off: u32::from_le_bytes([r[0], r[1], r[2], r[3]]),
                x: u16::from_le_bytes([r[6], r[7]]),
                y: u16::from_le_bytes([r[8], r[9]]),
                town: r[10],
                shop: [r[11], r[12], r[13]],
            }
        })
        .collect::<Vec<_>>();
    let mut world = World {
        map,
        scenes,
        dim_inn,
        dim_church,
        marker,
        marker2,
        palettes,
        bin4,
        locations,
        scroll: (0, 0),
        hover: 0,
    };
    let l = world.location(game.campaign.location);
    let (w, h) = (world.map.width as i32, world.map.height as i32);
    let mut sx = l.x as i32 - 0x140;
    let mut sy = l.y as i32 - 0xf0;
    if sx < 0 {
        sx = 0;
    } else if w < l.x as i32 + 0x13f {
        sx = w - 0x280;
    }
    if sy < 0 {
        sy = 0;
    } else if h < l.y as i32 + 0xef {
        sy = h - 0x1e0;
    }
    world.scroll = (sx, sy);
    game.ui.world = Some(Box::new(world));
    Ok(())
}

/// Port of `FUN_00422e26(loc)`: the place name is the title of stage `loc`.
pub fn set_place_name(game: &mut Game, loc: u16) {
    let n = i16::from_le_bytes([game.res().stages[0], game.res().stages[1]]) as u16;
    let loc = if n < loc { 1 } else { loc };
    game.ui.place_name = match Stage::parse(&game.res().stages, loc as usize) {
        Ok(s) if s.name.len() < 0x29 => s.name,
        _ => b"\xa5\xbc\xaa\xbe".to_vec(),
    };
}

/// Port of `FUN_0040ff7d`: from a finished stage to the next one. A
/// story-only next stage is entered directly (`FUN_00415e0d`); otherwise
/// the world-map phase runs (town, march, deployment).
pub fn between_stages(game: &mut Game) -> Result<i32> {
    super::stage::reset(game);
    super::armies::stage_end_cleanup(game, false);
    game.battle.clear_army(crate::battle::Side::Enemy);
    let next = game.campaign.next_stage;
    let kind = Stage::parse(&game.res().stages, next as usize).map(|s| s.header.kind).unwrap_or(1);
    if kind == 3 {
        game.campaign.between_stages = false;
        super::stage::load_stage(game, next, 10)?;
        super::stage::map_init(game)?;
        return Ok(code::NEW_STAGE);
    }
    game.campaign.between_stages = true;
    world_load(game)?;
    // Test hook (oracle fixtures, docs/oracle.md): S2_TEST_SAVE=N writes 1-based
    // slot N + i on the i-th arrival at the between-stages loop, as the town's
    // 儲存進度 would. With S2_TEST_SAVE_KEEP=DIR every arrival writes slot N and
    // copies it to DIR/between_<stage>.sav (stage = the one just finished).
    static ARRIVALS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if let Some(n) = std::env::var("S2_TEST_SAVE").ok().and_then(|v| v.parse::<usize>().ok()) {
        let i = ARRIVALS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let keep = std::env::var_os("S2_TEST_SAVE_KEEP").map(std::path::PathBuf::from);
        let slot = if keep.is_some() { n } else { n + i };
        if slot <= 50 {
            set_place_name(game, game.campaign.location);
            super::save::write_save(game, slot)?;
            if let Some(dir) = keep {
                let src = game.data.dir.resolve(&super::save::slot_path(slot - 1))?;
                let dst = dir.join(format!("between_{:03}.sav", game.campaign.stage));
                std::fs::create_dir_all(&dir)?;
                std::fs::copy(&src, &dst)?;
            }
        }
    }
    let c;
    let has_town = game.ui.world.as_ref().map(|w| w.location(game.campaign.location).town == 1).unwrap_or(false);
    if has_town {
        loop {
            let loc = game.campaign.location;
            set_place_name(game, loc);
            let r = town_hub(game)?;
            if r != code::LOADED && r != code::LOADED_SLOT50 {
                super::stop_music(game);
            }
            if !(r == code::LOADED && game.campaign.between_stages) {
                c = r;
                break;
            }
        }
    } else {
        super::march::easy_penalty(game);
        c = super::march::march_and_deploy(game)?;
    }
    Ok(c)
}

/// Port of `FUN_00410060`: a picture with clickable rectangles and hover
/// labels. `labels[0]` is the screen title (unused here), `labels[1..]` the
/// hot-spots. Returns the 1-based hot-spot or 0 (cancelled).
pub fn hotspot_menu(game: &mut Game, rects: &[(i32, i32, i32, i32)], labels: &[usize], pic: usize, flags: u32, cancellable: bool) -> usize {
    let center = |r: (i32, i32, i32, i32)| ((r.0 + r.2) / 2, (r.1 + r.3) / 2);
    let (cx, cy) = center(rects[0]);
    cursor::warp(game, cx, cy);
    let mut sel = 1usize;
    let mut moved = false;
    let mut show_all = false;
    let mut full = true;
    let mut changed = false;
    let speed = if flags & 8 != 0 { -4 } else { 0 };
    let pal = game.ui.world.as_ref().map(|w| w.palettes[pic].clone()).unwrap_or_default();
    let fade_out = |game: &mut Game| {
        if flags & 2 != 0 {
            gfx::fade(game, &pal, 1, speed);
        }
    };
    loop {
        super::music_keepalive(game);
        if flags & 4 != 0 {
            gfx::palette_cycle(game);
        }
        if full {
            game.scr.set_page(1);
            if let Some(img) = game.ui.world.as_ref().map(|w| w.scenes[pic].clone()) {
                gfx::blit(game.scr.page(), &img, 0, 0, true);
            }
            game.scr.set_page(0);
            changed = true;
        }
        if !changed && (!moved || show_all || sel < 1) {
            if moved {
                cursor::draw_present(game, None, true);
                moved = false;
            }
        } else {
            game.scr.copy_page(0, 1);
            let label = |game: &Game, i: usize| game.msg(labels[i]);
            if show_all {
                for (i, &r) in rects.iter().enumerate() {
                    let (x, y) = center(r);
                    let s = label(game, i + 1);
                    let len = text::strlen(&s);
                    let mut lx = x - len * 4;
                    if lx < 0 {
                        lx = 0;
                    } else if 0x27f < lx + len * 8 {
                        lx = 0x27f - len * 8;
                    }
                    text::small(game, lx, y, &s, 3, 2, 1, 0);
                }
            } else if sel >= 1 {
                let (mx, my) = game.sys.mouse();
                let my = (my - 0x1e).max(0x1e);
                let s = label(game, sel);
                let len = text::strlen(&s);
                let mut lx = mx - len * 4;
                if lx < 0 {
                    lx = 0;
                } else if 0x27f < lx + len * 8 {
                    lx = 0x27f - len * 8;
                }
                text::small(game, lx, my, &s, 3, 2, 1, 0);
            }
            if full {
                cursor::capture(game);
            }
            cursor::draw_present(game, None, false);
            if full && flags & 1 != 0 {
                gfx::fade(game, &pal, 0, speed);
            }
            moved = false;
            full = false;
            changed = false;
        }
        if !(game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind)) {
            continue;
        }
        let e = game.ev;
        match e.kind {
            ev::KEY_DOWN | ev::KEY_REPEAT => match e.scan() {
                scan::ESC if cancellable => {
                    fade_out(game);
                    return 0;
                }
                scan::ENTER | scan::SPACE => {
                    fade_out(game);
                    return sel;
                }
                scan::TAB | scan::LEFT | scan::RIGHT => {
                    sel += 1;
                    if sel > rects.len() {
                        sel = 1;
                    }
                    changed = true;
                    let (x, y) = center(rects[sel - 1]);
                    cursor::warp(game, x, y);
                }
                _ => {}
            },
            ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                if e.kind == ev::MOUSE_MOVE {
                    moved = true;
                }
                if cancellable && e.mods & ev::M_RBUTTON != 0 {
                    game.wait_event(ev::MOUSE_UP);
                    fade_out(game);
                    game.flush_events(0x1f);
                    return 0;
                }
                let click = e.mods & ev::M_LBUTTON != 0;
                if click {
                    game.wait_event(ev::MOUSE_UP);
                }
                let hit = rects.iter().position(|r| r.0 <= e.x && e.x <= r.2 && r.1 <= e.y && e.y <= r.3);
                match hit {
                    Some(i) => {
                        if sel != i + 1 {
                            changed = true;
                            sel = i + 1;
                        }
                        if click {
                            fade_out(game);
                            game.flush_events(0x1f);
                            return i + 1;
                        }
                    }
                    None => {
                        if click {
                            show_all = !show_all;
                            changed = true;
                        }
                        if sel != 0 {
                            sel = 0;
                            changed = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Town hot-spots (`DAT_0048e0e0`).
const TOWN_RECTS: [(i32, i32, i32, i32); 4] = [(226, 211, 370, 296), (97, 277, 206, 366), (421, 272, 584, 402), (79, 419, 296, 479)];
/// Shop hot-spots (`DAT_0048e100`).
const SHOP_RECTS: [(i32, i32, i32, i32); 4] = [(124, 265, 391, 476), (496, 92, 609, 200), (227, 105, 393, 245), (404, 137, 494, 316)];
/// Inn hot-spots (`DAT_0048e120`).
const INN_RECTS: [(i32, i32, i32, i32); 10] = [
    (473, 60, 562, 176),
    (310, 106, 380, 216),
    (534, 184, 617, 303),
    (455, 179, 530, 299),
    (292, 243, 412, 479),
    (292, 206, 352, 240),
    (398, 209, 420, 252),
    (48, 167, 120, 302),
    (250, 158, 294, 202),
    (121, 151, 266, 444),
];
/// Church hot-spots (`DAT_0048e170`).
const CHURCH_RECTS: [(i32, i32, i32, i32); 3] = [(75, 314, 163, 383), (268, 305, 382, 400), (495, 204, 629, 317)];

/// Port of `FUN_00410763`: the town square.
pub fn town_hub(game: &mut Game) -> Result<i32> {
    game.battle.recalc_army_flags(crate::battle::Side::Player, 2);
    ui::drop_presses(game);
    super::music(game, 4);
    cursor::set_shape(game, SHAPE_ALT);
    loop {
        match hotspot_menu(game, &TOWN_RECTS, &[0x58, 0x59, 0x5a, 0x5b, 0x5c], 0, 3, false) {
            1 => {
                let r = inn(game)?;
                if r != 0 {
                    game.flush_events(0x1f);
                    return Ok(r);
                }
            }
            2 => shop_hub(game)?,
            3 => {
                church(game)?;
                super::music(game, 4);
            }
            4 => {
                cursor::set_shape(game, SHAPE_POINTER);
                super::stop_music(game);
                let r = super::march::march_and_deploy(game)?;
                game.flush_events(0x1f);
                return Ok(r);
            }
            _ => {}
        }
    }
}

/// Draw a dimmed backdrop opaque, as the sub-screens of the inn/church do.
fn backdrop(game: &mut Game, inn: bool) {
    if let Some(img) = game.ui.world.as_ref().map(|w| if inn { w.dim_inn.clone() } else { w.dim_church.clone() }) {
        gfx::blit(game.scr.page(), &img, 0, 0, true);
    }
}

/// Port of `FUN_004109b3`: the inn. Returns 7 / 8 after a load, 0xb to quit, 0 to leave.
pub fn inn(game: &mut Game) -> Result<i32> {
    let labels = [0x59, 0xc1, 0xc2, 0x80, 0x81, 0xc3, 0xc0, 0x50, 0x83, 0x7b, 0xbe];
    loop {
        let sel = hotspot_menu(game, &INN_RECTS, &labels, 1, 0xb, true);
        if sel == 0 {
            game.flush_events(0x1f);
            return Ok(0);
        }
        if sel != 1 {
            backdrop(game, true);
            cursor::set_shape(game, SHAPE_POINTER);
            cursor::capture(game);
        }
        match sel {
            1 => {
                super::worldmap::world_map_view(game)?;
                super::music(game, 4);
            }
            2 => {
                let which = game.battle.rng.rnd(2) as usize;
                keeper_talk(game, which)?;
            }
            3 => {
                let r = super::save::load_game_menu(game, true)?;
                if r == code::LOADED || r == code::LOADED_SLOT50 {
                    return Ok(r);
                }
            }
            4 => {
                super::save::save_game_menu(game, true)?;
            }
            5 => {
                super::party::equipment(game, 0, None, true)?;
                game.battle.recalc_army_flags(crate::battle::Side::Player, 2);
            }
            6 => {
                super::party::status_list(game, true)?;
            }
            7 => {
                super::party::equipment(game, 1, None, true)?;
            }
            8 => {
                let q = game.msg(0x33);
                if ui::yes_no(game, 300, 200, true, &q, true, true) {
                    return Ok(code::QUIT);
                }
            }
            9 => super::settings::open(game, 2)?,
            _ => {
                let q = game.msg(6);
                if ui::yes_no(game, 200, 200, true, &q, true, true) {
                    super::party::auto_equip_all(game);
                    game.battle.recalc_army_flags(crate::battle::Side::Player, 2);
                }
            }
        }
        let m = game.res().master.clone();
        gfx::fade(game, &m, 1, -4);
        cursor::set_shape(game, SHAPE_ALT);
    }
}

/// Port of `FUN_00410f2e(face, title, darken, gold)` (portrait via `FUN_00422335`): the shop/church
/// window — box at (6, 8), the keeper's portrait and title, optionally the
/// party gold panel.
pub fn keeper_window(game: &mut Game, face: usize, title: &[u8], darken: bool, gold_panel: bool) -> Result<()> {
    if darken {
        let lut = game.res().tints[6];
        gfx::remap_page(game.scr.page(), &lut);
    }
    let bx = game.res().ui_image(0xeb)?;
    gfx::blit(game.scr.page(), &bx, 6, 8, true);
    if gold_panel {
        let panel = game.res().ui_image(0xd4)?;
        gfx::blit(game.scr.page(), &panel, 0x160, 200, true);
        let s = game.msg(0xcd);
        text::large(game, 0x163, 0xcd, &s, 0xb3, 0, 1, 2);
    }
    if let Ok(img) = game.res().portraits.get(face, s2data::gfx_portrait::PortraitKind::Dialogue) {
        gfx::blit(game.scr.page(), &img, 0x50 - img.width as i32 / 2, 0x25, false);
    }
    text::large(game, 0x50 - text::strlen(title) * 6, 0xb0, title, 0x36, 0, 1, 2);
    Ok(())
}

/// `FUN_0041f6d3`: a number in the regular font ("%d".."%5d").
#[allow(clippy::too_many_arguments)]
pub fn large_number(game: &mut Game, x: i32, y: i32, v: i32, fg: u8, c2: u8, target: u32, style: u32, fmt: usize) {
    let s = if fmt == 0 { format!("{v}") } else { format!("{v:>w$}", w = fmt) };
    text::large(game, x, y, s.as_bytes(), fg, c2, target, style);
}

/// Port of `FUN_004149ff(which)`: the innkeeper says one of two texts.
pub fn keeper_talk(game: &mut Game, which: usize) -> Result<()> {
    let loc = game.campaign.location;
    let t = game.ui.world.as_ref().map(|w| w.keeper_text(loc, which)).unwrap_or_default();
    let title = game.msg(0xc2);
    keeper_window(game, 0x9a, &title, true, false)?;
    text::block(game, 0x95, 0x2a, &t, 0x2a, 0, 1, 2, 6, 0x10, 0x1c);
    gfx::present(game);
    let m = game.res().master.clone();
    gfx::fade(game, &m, 0, -4);
    ui::click_to_continue(game, 0);
    Ok(())
}

/// Port of `FUN_00410886`: the shop hot-spots.
pub fn shop_hub(game: &mut Game) -> Result<()> {
    loop {
        let sel = hotspot_menu(game, &SHOP_RECTS, &[0x5a, 0x5d, 0x5e, 0x5f, 199], 2, 9, true);
        if sel != 0 {
            let pal = game.ui.world.as_ref().map(|w| w.palettes[2].clone()).unwrap_or_default();
            gfx::fade(game, &pal, 1, -4);
            cursor::set_shape(game, SHAPE_POINTER);
        }
        match sel {
            0 => break,
            1..=3 => {
                game.scr.clear();
                super::shop::buy(game, sel)?;
            }
            _ => {
                game.scr.clear();
                super::shop::sell(game)?;
            }
        }
        let m = game.res().master.clone();
        gfx::fade(game, &m, 1, -4);
        cursor::set_shape(game, SHAPE_ALT);
    }
    let pal = game.ui.world.as_ref().map(|w| w.palettes[2].clone()).unwrap_or_default();
    gfx::fade(game, &pal, 1, 0);
    game.flush_events(0x1f);
    Ok(())
}

/// Port of `FUN_00410c21`: the church hot-spots.
pub fn church(game: &mut Game) -> Result<()> {
    super::music(game, 0x25);
    loop {
        let sel = hotspot_menu(game, &CHURCH_RECTS, &[0x5b, 0xc4, 0xc5, 0xc6], 3, 0xb, true);
        if sel == 0 {
            break;
        }
        backdrop(game, false);
        cursor::set_shape(game, SHAPE_POINTER);
        match sel {
            1 => super::shop::donate(game)?,
            2 => super::shop::temple(game, 0)?,
            _ => {
                super::shop::temple(game, 1)?;
                game.battle.recalc_army_flags(crate::battle::Side::Player, 2);
            }
        }
        let m = game.res().master.clone();
        gfx::fade(game, &m, 1, -4);
        cursor::set_shape(game, SHAPE_ALT);
    }
    game.flush_events(0x1f);
    Ok(())
}

/// Message box helper used by the town screens.
pub fn notice(game: &mut Game, x: i32, y: i32, msg: usize) {
    let s = game.msg(msg);
    ui::message_box(game, x, y, &s, Wait::Click(0));
}
