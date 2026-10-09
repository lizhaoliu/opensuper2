//! The ending (`FUN_0041a779` with `FUN_0041b5dc`, `FUN_0041b489`,
//! `FUN_0041ae08`, `FUN_0041afa3`, `FUN_0041abf4`) and the scripted
//! picture with scrolling text (`FUN_0041b963`, `FUN_0041b659`, `FUN_0041b711`).

use anyhow::Result;
use s2data::msg::Endings;
use s2data::sprite::Image;
use s2data::Kind;

use crate::battle::unit::flag;
use crate::battle::{Side, UnitRef};
use crate::game::Game;
use crate::gfx;
use crate::res::load_picture;
use crate::sys::{ev, scan};
use crate::text;
use crate::ui;

/// `FUN_0041b5dc`: BIN 3, the ending texts.
fn endings(game: &Game) -> Result<Endings> {
    Ok(Endings::parse(&game.data.arcs.read(Kind::Bin, 3)?)?)
}

/// `FUN_00457578(army, char_id, 3)`: the member with that character, not
/// dead and not in reserve.
fn member(game: &Game, char_id: i32) -> Option<UnitRef> {
    let a = game.battle.army(Side::Player);
    a.units
        .iter()
        .position(|u| u.char_id as i32 == char_id && u.flags & (flag::DEAD | flag::RESERVE) == 0)
        .map(|i| UnitRef::new(Side::Player, i))
}

/// Draw `img` (a full 640x480 picture) on the current page.
fn backdrop(game: &mut Game, img: &Image) {
    gfx::blit(game.scr.page(), img, 0, 0, true);
}

/// Port of `FUN_0041ae08(n)`: a tall character picture (DAT 0x19+n) that
/// scrolls up from its bottom, 3 rows per 10 cs, until the top or a press.
fn scroll_picture(game: &mut Game, n: i32) -> Result<()> {
    let black = vec![0u8; 0x300];
    gfx::set_palette(game, &black);
    let (pic, pal) = load_picture(&game.data.arcs, n, 3)?;
    // `local_c[1] - 0x1df`: the header holds height - 1.
    let mut row = pic.height as i32 - 1 - 0x1df;
    let show = |game: &mut Game, row: i32| {
        let r0 = row.max(0) as usize;
        let page = &mut game.scr.pages[0];
        for y in 0..480usize {
            let src = r0 + y;
            if src >= pic.height {
                break;
            }
            let w = pic.width.min(640);
            page.pixels[y * 640..y * 640 + w].copy_from_slice(&pic.pixels[src * pic.width..src * pic.width + w]);
        }
        gfx::present_page(game, 0);
    };
    let mut first = true;
    let mut t = 0;
    let mut reached = true;
    loop {
        super::music_keepalive(game);
        if first {
            show(game, row);
            gfx::fade(game, &pal, 0, 0);
            first = false;
            ui::drop_presses(game);
        } else {
            game.wait_until(t, 10);
            show(game, row);
        }
        t = game.ticks();
        if row <= 0 {
            break;
        }
        row = (row - 3).max(0);
        if game.get_event(ev::KEY_DOWN | ev::KEY_REPEAT | ev::MOUSE_DOWN) {
            reached = false;
            break;
        }
    }
    if reached {
        ui::wait_key(game, 500);
    }
    gfx::fade(game, &pal, 1, 0);
    ui::drop_presses(game);
    Ok(())
}

/// Port of `FUN_0041afa3(unit)`: a member's epilogue page, then their
/// friendships.
fn epilogue(game: &mut Game, bg: &Image, end: &Endings, rank: usize, r: UnitRef, kills: &mut u32) {
    game.scr.set_page(0);
    backdrop(game, bg);
    if let Some(face) = super::party::portrait(game, r) {
        gfx::blit(game.scr.page(), &face, 0x35, 0x4c, false);
    }
    super::party::draw_unit(game, r, 0xe1, 0xb4);
    let u = game.battle.unit(r).clone();
    let name = s2data::text::cstr(&u.name).to_vec();
    text::large(game, 0x16f, 0x2a, &name, 0xaf, 0, 1, 2);
    let mut line = game.res().class_name(u.kind as usize).to_vec();
    line.push(b' ');
    line.extend_from_slice(u.level.to_string().as_bytes());
    line.push(b' ');
    line.extend_from_slice(&game.msg(0x71));
    text::large(game, 0x16f, 0x48, &line, 0xaf, 0, 1, 2);
    let power = game.battle.power(&u, 1, 0);
    for (msg, v, y) in [(0x1cusize, power, 0x66), (0x100, u.kills as i32, 0x84), (0x101, u.deaths as i32, 0xa2)] {
        let mut s = game.msg(msg);
        s.extend_from_slice(v.to_string().as_bytes());
        text::large(game, 0x16f, y, &s, 0xaf, 0, 1, 2);
    }
    *kills += u.kills as u16 as u32;
    game.scr.copy_page(1, 0);
    if let Some(e) = end.epilogues.get(rank).and_then(|v| v.iter().find(|e| e.char_id == u.char_id)) {
        text::block_small(game, 0x78, 0xe6, &e.text, 0xaf, 0, 1, 2, 0x1e, 0x19, 0x14);
    }
    gfx::present(game);
    let m = game.res().master.clone();
    gfx::fade(game, &m, 0, 0);
    ui::drop_presses(game);
    ui::click_to_continue(game, 0);
    game.scr.copy_page(0, 1);
    let mut s = name.clone();
    s.push(b' ');
    s.extend_from_slice(&game.msg(0x105));
    text::large(game, 0x22, 0xdc, &s, 0xaf, 0, 1, 2);
    let mut count = 0u32;
    let mut y = 0xfa;
    for k in 0..0x28i32 {
        if u.char_id as i32 == k + 1 {
            continue;
        }
        let Some(o) = member(game, k + 1) else { continue };
        let oc = game.battle.unit(o).char_id as i32 - 1;
        if !(0..0x28).contains(&oc) {
            continue;
        }
        count += 1;
        let x = if count.is_multiple_of(2) { 0x140 } else { 0x22 };
        let on = s2data::text::cstr(&game.battle.unit(o).name).to_vec();
        text::large(game, x, y, &on, 0xaf, 0, 1, 2);
        let a = format!(":{}", u.affinity[oc as usize] as i8);
        text::large(game, x + 0x78, y, a.as_bytes(), 0xaf, 0, 1, 2);
        if count.is_multiple_of(2) {
            y += 0x1a;
        }
        if 0xf < count {
            break;
        }
    }
    gfx::present(game);
    ui::drop_presses(game);
    ui::click_to_continue(game, 0);
    gfx::fade(game, &m, 1, 0);
}

/// Port of `FUN_0041b489`: the epilogues, first the thirteen leading
/// characters (each with their picture), then everyone else still with the party.
fn epilogues(game: &mut Game, bg: &Image, end: &Endings, rank: usize) -> Result<u32> {
    const LEADS: [i32; 13] = [0xb, 1, 0x16, 2, 3, 4, 0xe, 0xf, 0x12, 0x20, 0x10, 0x11, 0x2a];
    game.campaign.between_stages = true;
    game.flush_events(0x1f);
    super::music(game, 0x14);
    let mut kills = 0;
    for (k, &ch) in LEADS.iter().enumerate() {
        if let Some(r) = member(game, ch) {
            scroll_picture(game, k as i32 + 2)?;
            epilogue(game, bg, end, rank, r, &mut kills);
        }
    }
    for ch in 1..0x25 {
        if LEADS.contains(&ch) {
            continue;
        }
        if let Some(r) = member(game, ch) {
            epilogue(game, bg, end, rank, r, &mut kills);
        }
    }
    Ok(kills)
}

/// Port of `FUN_0041abf4`: the campaign record, then the staff page
/// (donations: `FUN_00444ba6`, the army's +0xd6 total).
fn results(game: &mut Game, bg: &Image, end: &Endings, kills: u32) {
    super::music(game, 0xd);
    game.scr.set_page(0);
    backdrop(game, bg);
    let head = game.msg(0x102);
    text::block(game, 0x46, 0x32, &head, 0xaf, 0, 1, 2, 0x32, 0x14, 0x1c);
    let c = &game.campaign;
    let values = [c.battles_fought as i32, c.difficulty as i32, c.fame, c.rating, game.battle.army(Side::Player)._d6, kills as i32];
    for (k, v) in values.into_iter().enumerate() {
        let y = [0x32, 0x4e, 0x6a, 0x82, 0x9e, 0xba][k];
        super::town::large_number(game, 400, y, v, 0xaf, 0, 1, 2, 0);
    }
    gfx::present(game);
    ui::drop_presses(game);
    let m = game.res().master.clone();
    gfx::fade(game, &m, 0, 0);
    ui::click_to_continue(game, 0);
    gfx::fade(game, &m, 1, 0);
    backdrop(game, bg);
    if let Some(p) = end.pages.get(2) {
        text::block(game, 0x3c, 0x32, p, 0xaf, 0, 1, 2, 0x32, 0x14, 0x1c);
    }
    gfx::present(game);
    gfx::fade(game, &m, 0, 0);
    ui::click_to_continue(game, 0);
    gfx::fade(game, &m, 1, 0);
}

/// Port of `FUN_0041a779`: the two closing pages (with movies 4 and 5),
/// the epilogues for the ending variant (the first of story flags 73..=80
/// that is set), the record and credits, movie 3. The program ends after it.
pub fn ending(game: &mut Game) -> Result<()> {
    let x = if game.cfg.mainland { 0x7d } else { 0x19 };
    super::stage::reset(game);
    super::stop_music(game);
    let end = endings(game)?;
    let (pic, pal) = load_picture(&game.data.arcs, 0xf, 3)?;
    game.scr.pages[1].pixels.copy_from_slice(&pic.pixels[..640 * 480]);
    for (k, y) in [(0usize, 0x3c), (1, 0x78)] {
        game.scr.copy_page(0, 1);
        game.scr.set_page(0);
        if let Some(p) = end.pages.get(k) {
            text::block(game, x, y, p, 0xff, 0, 1, 2, 0x1c, 0x15, 0x1c);
        }
        gfx::present(game);
        gfx::fade(game, &pal, 0, 0);
        ui::wait_key(game, 0);
        gfx::fade(game, &pal, 1, 0);
        if game.cfg.movies {
            super::movie::play(game, 4 + k, 0);
        }
    }
    // FUN_0044414b(player, 0) / FUN_0043deeb(enemy): the party as it leaves the last stage.
    super::armies::stage_end_cleanup(game, false);
    game.battle.clear_army(Side::Enemy);
    let rank = (73..=80).position(|f| game.campaign.flags[f] != 0).unwrap_or(0);
    let (bg, _) = load_picture(&game.data.arcs, 1, 3)?;
    let kills = epilogues(game, &bg, &end, rank)?;
    results(game, &bg, &end, kills);
    if game.cfg.movies {
        super::movie::play(game, 3, 0);
    }
    Ok(())
}

/// `FUN_0041b659(text, cols)`: the number of line breaks the wrapped text makes.
fn count_lines(s: &[u8], cols: usize) -> usize {
    let (mut lines, mut used, mut p) = (0usize, 0usize, 0usize);
    while let Some(&c) = s.get(p) {
        if c == b'\n' || c == 0 {
            break;
        }
        if c < 0x81 {
            if c == b'*' {
                lines += 1;
                used = 0;
            } else {
                used += 1;
            }
            p += 1;
        } else {
            used += 2;
            p += 2;
        }
        if cols * 2 <= used {
            lines += 1;
            used -= cols * 2;
        }
    }
    lines
}

/// Port of `FUN_0041b711(text, h, top)`: the text image rises from the
/// bottom of the screen over page 2 into the window below `top`, then
/// scrolls through it. Esc or a right click stops; a held left button
/// (or a key with modifier 0x40) skips the 12 cs frame wait.
fn scroll_text(game: &mut Game, txt: &Image, top: i32) {
    let win = 0x1e0 - top;
    let (mut rows, mut y, mut left) = (2i32, 0x1dei32, txt.height as i32);
    let mut src = 0usize;
    let mut t = game.ticks();
    loop {
        let mut wait = true;
        let mut done = false;
        if game.get_event(0xffff) && game.ev.kind != 0 {
            let e = game.ev;
            if e.kind < 3 {
                if e.scan() == scan::ESC {
                    game.wait_event(ev::KEY_UP);
                    done = true;
                }
                wait = e.mods & 0x40 == 0;
            } else if e.kind == ev::MOUSE_DOWN {
                if e.mods & ev::M_RBUTTON != 0 {
                    game.wait_event(ev::MOUSE_UP);
                    done = true;
                } else if e.mods & ev::M_LBUTTON != 0 {
                    wait = false;
                }
            }
        }
        if done {
            return;
        }
        game.scr.copy_page(0, 2);
        {
            let page = &mut game.scr.pages[0];
            let n = (rows.max(0) as usize) * 640;
            let dst0 = y.max(0) as usize * 640;
            for k in 0..n {
                let c = txt.pixels.get(src + k).copied().unwrap_or(0);
                if c != 0 {
                    if let Some(d) = page.pixels.get_mut(dst0 + k) {
                        *d = c;
                    }
                }
            }
        }
        if wait {
            game.wait_until(t, 0xc);
        }
        gfx::present_page(game, 0);
        t = game.ticks();
        if win < left || win < rows {
            if rows < win {
                rows += 1;
            }
        } else {
            rows -= 1;
        }
        rows = rows.clamp(0, win);
        if top < y {
            y -= 1;
        } else {
            src += 640;
            if left > 0 {
                left -= 1;
            }
        }
        y = y.max(top);
        left = left.max(0);
        if left <= 0 && rows <= 0 {
            return;
        }
    }
}

/// Port of `FUN_0041b963(picture, page, music, 0)`: a picture (DAT 0x19+n,
/// or black) with BIN 3 page `page` scrolling over its lower part.
pub fn show_picture(game: &mut Game, picture: i16, page: i16, cue: i16) {
    let _ = picture_inner(game, picture as i32, page as i32, cue);
}

fn picture_inner(game: &mut Game, picture: i32, page: i32, cue: i16) -> Result<()> {
    super::stop_music(game);
    let end = endings(game)?;
    let (pal, color) = if picture < 1 {
        game.scr.clear_page(2);
        (game.res().master.clone(), 0xfe)
    } else {
        let (pic, pal) = load_picture(&game.data.arcs, picture, 3)?;
        let n = 640 * 480;
        game.scr.pages[2].pixels.copy_from_slice(&pic.pixels[..n.min(pic.pixels.len())]);
        (pal, 0xff)
    };
    let txt_src = end.pages.get((page - 1).max(0) as usize).cloned().unwrap_or_default();
    let lines = count_lines(&txt_src, 0x15);
    let h = (lines + 1) * 0x1c + 2;
    // FUN_0046e957 / FUN_0041ded6: the text goes into a 640 x h surface.
    let cur = game.scr.cur;
    let saved = std::mem::replace(&mut game.scr.pages[cur], Image::new(640, h));
    text::block(game, 0x19, 0, &txt_src, color, 0, 1, 2, lines + 5, 0x15, 0x1c);
    let txt = std::mem::replace(&mut game.scr.pages[cur], saved);
    gfx::present_page(game, 2);
    gfx::fade(game, &pal, 0, 0);
    if cue > 0 {
        super::music(game, cue as u16);
    }
    ui::drop_presses(game);
    scroll_text(game, &txt, 0x182);
    super::stop_music(game);
    gfx::fade(game, &pal, 1, 0);
    Ok(())
}
