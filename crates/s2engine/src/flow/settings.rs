//! The settings screen `FUN_0042deda` and SUPERSP.CFG writing (`FUN_00423eb9`).

use anyhow::Result;

use crate::cursor;
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;

/// Port of `FUN_00423eb9`: write SUPERSP.CFG.
pub fn write_config(game: &Game) -> Result<()> {
    let c = &game.cfg;
    let on = |b: bool| if b { "ON" } else { "OFF" };
    let device = if c.cd_mode {
        format!("DEVICE={},{},{}\n", c.prg_local as i32, c.mainland as i32, c.drive)
    } else {
        format!("DEVICE=0,{},0\n", c.mainland as i32)
    };
    let s = format!(
        "{device}VOLUME={},{},{}\nCDDA={},{}\nSET={},{},{},{},{},{},{},{}\nVersion=1.4\n",
        c.midi_volume,
        c.cd_volume,
        c.sfx_volume,
        on(c.cd_audio),
        on(c.movies),
        c.screen_speed,
        c.text_speed,
        c.confirm as i32,
        c.auto_switch as i32,
        c.status_window as i32,
        c.quick_switch as i32,
        c.battle_anim as i32,
        c.mesh as i32
    );
    let path = game.data.dir.resolve("supersp.cfg").unwrap_or_else(|_| game.data.dir.root().join("SUPERSP.CFG"));
    // The original opens the file in text mode ("w+t"): CR LF line ends.
    std::fs::write(path, s.replace('\n', "\r\n"))?;
    Ok(())
}

/// Port of `FUN_0042efcd(where)`: 0 = title (writes the config afterwards),
/// 1 = battle (darkens the screen first), 2 = inn.
pub fn open(game: &mut Game, place: i32) -> Result<()> {
    if place == 1 {
        cursor::erase(game);
        let lut = game.res().tints[6];
        gfx::remap_page(game.scr.page(), &lut);
    }
    settings_screen(game, place)?;
    if place == 0 {
        write_config(game)?;
    }
    Ok(())
}

/// Slider position of a 0..=99 volume (`/ 11`, 9 at most).
fn vol_pos(v: i32) -> i32 {
    (v / 0xb).min(9)
}

/// Port of `FUN_0042deda(panel, where)`.
pub fn settings_screen(game: &mut Game, place: i32) -> Result<()> {
    let panel = game.res().ui_image(0xca)?;
    let ox = -(panel.width as i32 / 2);
    let oy = -(panel.height as i32 / 2);
    cursor::erase(game);
    gfx::blit(game.scr.page(), &panel, ox + 0x140, oy + 0xf0, false);
    let t = game.msg(0x82);
    text::large(game, ox + 0x246, oy + 0xfc, &t, 0x49, 0x37, 1, 0);
    for i in 0..4 {
        let a = game.msg(0x84 + i);
        text::large(game, ox + 0x18e, oy + 0x11c + i as i32 * 0x1f, &a, 0x33, 0x29, 1, 1);
        let b = game.msg(0x88 + i);
        text::large(game, ox + 0x18e, oy + 0x19d + i as i32 * 0x1f, &b, 0x33, 0x29, 1, 1);
        let c = game.msg(0x8c + i);
        text::large(game, ox + 0x27f, oy + 0x19d + i as i32 * 0x1f, &c, 0x33, 0x29, 1, 1);
    }
    for (x, y, m) in [
        (0x212, 0x11f, 0x92),
        (0x212, 0x13e, 0x92),
        (0x338, 0x11f, 0x93),
        (0x338, 0x13e, 0x93),
        (0x212, 0x15d, 0x94),
        (0x212, 0x17c, 0x94),
        (0x338, 0x15d, 0x95),
        (0x338, 0x17c, 0x95),
    ] {
        let s = game.msg(m);
        text::small(game, ox + x, oy + y, &s, 0xaf, 0xb5, 1, 0);
    }
    // 易 / 難: only the colours depend on the place (style 1 in both branches).
    let (fg, c2) = if place == 2 { (0xaf, 0xb5) } else { (0x33, 0x29) };
    let easy = game.msg(0x98);
    text::small(game, ox + 0x2f9, oy + 0x1fd, &easy, fg, c2, 1, 1);
    let hard = game.msg(0x99);
    text::small(game, ox + 0x343, oy + 0x1fd, &hard, fg, c2, 1, 1);
    if place != 2 {
        // Elsewhere the difficulty is drawn once here; at the inn the loop redraws it.
        let d = game.campaign.difficulty as i32;
        text::number(game, ox + 0x322, oy + 0x1fd, d, 0x33, 0x29, 1, 1, 0);
    }
    let cur = game.scr.cur;
    game.scr.copy_page(1, cur);
    let mesh_before = game.cfg.mesh;
    let mut done = false;
    let mut first = true;
    let mut redraw = true;
    let mut moved = false;
    loop {
        if done {
            game.flush_events(0x1f);
            if place == 1 && mesh_before != game.cfg.mesh {
                // FUN_0042deda: both armies erased and drawn again.
                crate::battle::view::redraw_armies(game);
            }
            return Ok(());
        }
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if redraw {
            game.scr.copy_page(cur, 1);
            let c = game.cfg.clone();
            let p = game.scr.page();
            let pos = [9 - (c.screen_speed / 3).min(9), 9 - (c.text_speed / 5).min(9)];
            for (k, (v, y)) in [(pos[0], 0x124), (pos[1], 0x143)].into_iter().enumerate() {
                let _ = k;
                gfx::rect(p, ox + 0x237 + v * 0x18, oy + y, ox + 0x244 + v * 0x18, oy + y + 7, 0xbd, true);
            }
            let music = if c.cd_audio { c.cd_volume } else { c.midi_volume };
            for (v, y) in [(vol_pos(music), 0x162), (vol_pos(c.sfx_volume), 0x181)] {
                gfx::rect(p, ox + 0x237 + v * 0x18, oy + y, ox + 0x244 + v * 0x18, oy + y + 7, 0xbd, true);
            }
            let toggles = [
                (c.confirm, 0x21a, 0x23a, 0x1a1),
                (c.mesh, 0x30a, 0x32a, 0x1a1),
                (c.status_window, 0x21a, 0x23a, 0x1c0),
                (c.battle_anim, 0x30a, 0x32a, 0x1c0),
                (c.quick_switch, 0x21a, 0x23a, 0x1df),
                (c.cd_audio, 0x30a, 0x32a, 0x1df),
                (c.auto_switch, 0x21a, 0x23a, 0x1fe),
            ];
            for (on, mx, tx, ty) in toggles {
                if on {
                    gfx::rect(game.scr.page(), ox + mx, oy + ty + 6, ox + mx + 0xd, oy + ty + 0xd, 0xbd, true);
                }
                let s = game.msg(if on { 0x96 } else { 0x97 });
                text::small(game, ox + tx, oy + ty, &s, 0xaf, 0xb5, 1, 0);
            }
            if place == 2 {
                let d = game.campaign.difficulty as i32;
                text::number(game, ox + 0x322, oy + 0x1fd, d, 0xaf, 0xb5, 1, 1, 0);
            }
            if first {
                cursor::capture(game);
            }
            cursor::draw(game, false);
            gfx::present(game);
            if first {
                game.flush_events(0x1f);
                if place != 1 {
                    let m = game.res().master.clone();
                    gfx::fade(game, &m, 0, -4);
                }
                first = false;
            }
            redraw = false;
            moved = false;
        } else if moved {
            cursor::draw_present(game, None, true);
            moved = false;
        }
        if !(game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind)) {
            continue;
        }
        let e = game.ev;
        match e.kind {
            ev::KEY_DOWN | ev::KEY_REPEAT => {
                if e.scan() == scan::ESC {
                    done = true;
                    game.wait_event(ev::KEY_UP);
                }
            }
            ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                if e.kind == ev::MOUSE_MOVE {
                    moved = true;
                }
                if e.mods & ev::M_LBUTTON == 0 {
                    if e.mods & ev::M_RBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        done = true;
                    }
                    continue;
                }
                game.wait_event(ev::MOUSE_UP);
                let (x, y) = (e.x, e.y);
                let inside = |x0: i32, x1: i32, y0: i32, y1: i32| ox + x0 <= x && x <= ox + x1 && oy + y0 <= y && y <= oy + y1;
                let c = &mut game.cfg;
                if ox + 0x234 <= x && x < ox + 0x324 && oy + 0x11f <= y && y <= oy + 400 {
                    let dx = x - (ox + 0x234);
                    if dx % 0x18 < 0x13 {
                        let k = dx / 0x18;
                        let vol = if k == 9 { 100 } else { k * 0xb };
                        if inside(0x234, 0x323, 0x11f, 0x136) {
                            let v = (9 - k) * 3;
                            redraw |= c.screen_speed != v;
                            c.screen_speed = v;
                        } else if inside(0x234, 0x323, 0x13d, 0x154) {
                            let v = (9 - k) * 5;
                            redraw |= c.text_speed != v;
                            c.text_speed = v;
                        } else if inside(0x234, 0x323, 0x15b, 0x172) {
                            if !c.cd_audio {
                                if c.midi_volume != vol {
                                    c.midi_volume = vol;
                                    game.sys.audio.set_midi_volume(vol as u8);
                                    redraw = true;
                                }
                            } else if c.cd_volume != vol {
                                c.cd_volume = vol;
                                redraw = true;
                            }
                        } else if inside(0x234, 0x323, 0x179, 400) && c.sfx_volume != vol {
                            c.sfx_volume = vol;
                            game.sys.audio.sfx_volume = vol as u8;
                            redraw = true;
                        }
                    }
                } else if inside(0x1ff, 0x272, 0x19e, 0x1b5) {
                    c.confirm ^= true;
                    redraw = true;
                } else if inside(0x1ff, 0x272, 0x1bc, 0x1d3) {
                    c.status_window ^= true;
                    redraw = true;
                } else if inside(0x1ff, 0x272, 0x1da, 0x1f1) {
                    c.quick_switch ^= true;
                    redraw = true;
                } else if inside(0x1ff, 0x272, 0x1f8, 0x20f) {
                    c.auto_switch ^= true;
                    redraw = true;
                } else if inside(0x2ef, 0x362, 0x19e, 0x1b5) {
                    c.mesh ^= true;
                    redraw = true;
                } else if inside(0x2ef, 0x362, 0x1bc, 0x1d3) {
                    c.battle_anim ^= true;
                    redraw = true;
                } else if inside(0x2ef, 0x362, 0x1da, 0x1f1) {
                    c.cd_audio ^= true;
                    let cue = game.campaign.music;
                    super::stop_music(game);
                    game.campaign.music = 0;
                    super::music(game, cue);
                    redraw = true;
                } else if place == 2 {
                    if inside(0x2ef, 0x31d, 0x1f8, 0x20f) {
                        if game.campaign.difficulty > 1 {
                            game.campaign.difficulty -= 1;
                            redraw = true;
                        }
                    } else if inside(0x331, 0x362, 0x1f8, 0x20f) && game.campaign.difficulty < 5 {
                        game.campaign.difficulty += 1;
                        redraw = true;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Port of `FUN_0042d7cc` (系統設定 in the battle system menu): 載入進度 /
/// 儲存進度 / 系統設定 / 離開遊戲. Returns 0, 7 / 8 (a save was loaded),
/// 9 (saved) or 0xb (quit).
pub fn system_submenu(game: &mut Game) -> Result<i32> {
    use super::save::{saveload_screen, Mode};
    cursor::erase(game);
    let lut = game.res().tints[6];
    gfx::remap_page(game.scr.page(), &lut);
    let menu = game.res().ui_image(0xc2)?;
    // The image header holds width - 1: `(*(short *)img + 1) / 2` is width / 2.
    let (px, py) = (0x1c7 - menu.width as i32 / 2, 0x14e - menu.height as i32 / 2);
    let panel = game.res().ui_image(200)?;
    gfx::blit(game.scr.page(), &panel, px, py, false);
    let t = game.msg(0x7f);
    text::large(game, px + 0x24, py + 0x2e, &t, 0x4b, 0, 1, 2);
    let items: Vec<Vec<u8>> = (0..4).map(|k| game.msg(0x80 + k)).collect();
    for (k, it) in items.iter().enumerate() {
        text::large(game, px + 0xc, py + 0x4d + k as i32 * 0x1f, it, 0xe9, 0, 1, 2);
    }
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
                    scan::UP => action = 0x48,
                    scan::DOWN => action = 0x50,
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
                        let row = (0..4).find(|&k| {
                            let y0 = py + 0x4e + k * 0x1f;
                            y0 <= y && y <= y0 + 0x1a
                        });
                        if let Some(k) = row.map(|k| k as usize) {
                            if k != sel {
                                sel = k;
                                redraw = true;
                            }
                            if e.mods & ev::M_LBUTTON != 0 {
                                game.wait_event(ev::MOUSE_UP);
                                action = 0x39;
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
        let mut done = false;
        match action {
            0x48 | 0x50 => {
                sel = if action == 0x48 { (sel + 3) % 4 } else { (sel + 1) % 4 };
                cursor::warp(game, 0x140, py + 99 + sel as i32 * 0x1f);
                redraw = true;
            }
            1 => {
                result = 0;
                done = true;
            }
            0x39 => {
                game.flush_events(0x1f);
                cursor::erase(game);
                if let Some(b) = &bar {
                    gfx::blit(game.scr.page(), b, px + 10, py + 0x4c + sel as i32 * 0x1f, true);
                }
                text::large(game, px + 0xc, py + 0x4d + sel as i32 * 0x1f, &items[sel], 0x77, 0, 1, 2);
                cursor::draw_present(game, None, false);
                crate::ui::sound(game, 100);
                match sel {
                    0 => {
                        if super::init::scan_saves(game) {
                            result = saveload_screen(game, Mode::Load, false)?;
                            done = result != 0;
                        }
                    }
                    1 => {
                        result = saveload_screen(game, Mode::Save, false)?;
                        done = result == 9;
                    }
                    2 => settings_screen(game, 1)?,
                    _ => {
                        let q = game.msg(0x33);
                        if crate::ui::yes_no(game, px - 0x2c, py + 0xac, true, &q, true, false) {
                            result = 0xb;
                            done = true;
                        }
                    }
                }
                if !done {
                    cursor::warp(game, 0x140, py + 99 + sel as i32 * 0x1f);
                    first = true;
                    redraw = true;
                }
            }
            _ => {}
        }
        if done {
            break;
        }
    }
    game.flush_events(0x1f);
    Ok(result)
}
