//! Saving and loading: the save writer `FUN_004312c4`, the loader
//! `FUN_00431845` and the slot screen `FUN_00430405` (docs/ui.md §8.9).
//!
//! A save is a dump of four global blocks, the treasure table, both armies
//! and (inside a stage) the event table state; see `s2data::save`.

use anyhow::{Context, Result};
use s2data::save::{ArmyRecord, SaveFile, UnitRecord, COPYRIGHT_LEN, DESC_LEN};

use super::code;
use crate::battle::army::{Army, SummonSlot};
use crate::battle::unit::{Unit, UNIT_SIZE};
use crate::battle::{Side, UnitRef};
use crate::cursor::{self, SHAPE_POINTER};
use crate::game::Game;
use crate::gfx;
use crate::sys::{ev, scan};
use crate::text;
use crate::ui::{self, Wait};

/// Slot files: 0-based slot `i` is `SAVE\UJ{i+1}.SAV` (`FUN_00430244` formats `i + 1`).
pub fn slot_path(slot: usize) -> String {
    format!("SAVE\\UJ{:02}.SAV", slot + 1)
}

/// Special slots (0-based): 50 = turn-1 autosave (`FUN_00431154`), 51 = every-turn
/// autosave (`FUN_0043117c`), 52 = backup of the last overwritten save (`FUN_0043119b`).
pub const SLOT_TURN1: usize = 50;
pub const SLOT_TURN: usize = 51;
pub const SLOT_BACKUP: usize = 52;
/// Hourglass cursor shown while saving/loading.
const SHAPE_BUSY: usize = 0x10;
/// "The SuperSp Game V1.4 ..." written after the description.
const COPYRIGHT: &[u8] = b"The SuperSp Game V1.4 Copyright (c) 1997 by U&J TECHNOLOGY CO., LTD.";

fn put_i32(b: &mut Vec<u8>, v: i32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn get_i32(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// `DAT_00494638`, 0x1f bytes: config values plus the music cue.
fn block_b(game: &Game) -> Vec<u8> {
    let c = &game.cfg;
    let mut b = Vec::with_capacity(0x1f);
    for v in [c.screen_speed, c.text_speed, c.midi_volume, c.cd_volume, c.sfx_volume] {
        put_i32(&mut b, v);
    }
    for v in [c.confirm, c.status_window, c.quick_switch, c.auto_switch, c.cd_audio, c.battle_anim, c.mesh] {
        b.push(v as u8);
    }
    put_i32(&mut b, game.campaign.music as i32);
    b
}

fn apply_block_b(game: &mut Game, b: &[u8]) {
    let c = &mut game.cfg;
    c.screen_speed = get_i32(b, 0);
    c.text_speed = get_i32(b, 4);
    c.midi_volume = get_i32(b, 8);
    c.cd_volume = get_i32(b, 0xc);
    c.sfx_volume = get_i32(b, 0x10);
    c.confirm = b[0x14] != 0;
    c.status_window = b[0x15] != 0;
    c.quick_switch = b[0x16] != 0;
    c.auto_switch = b[0x17] != 0;
    c.cd_audio = b[0x18] != 0;
    c.battle_anim = b[0x19] != 0;
    c.mesh = b[0x1a] != 0;
    game.campaign.music = get_i32(b, 0x1b) as u16;
}

/// `DAT_00494668`, 0x1d bytes: modes, stage numbers, difficulty.
fn block_c(game: &Game) -> Vec<u8> {
    let c = &game.campaign;
    let mut b = Vec::with_capacity(0x1d);
    put_i32(&mut b, c.vs_mode as i32);
    put_i32(&mut b, c.between_stages as i32);
    put_i32(&mut b, c.stage as i32);
    put_i32(&mut b, c.turn as i32);
    put_i32(&mut b, c.next_stage as i32);
    put_i32(&mut b, c.alt_next_stage as i32);
    b.extend_from_slice(&c.location.to_le_bytes());
    b.push(c.cave_found as u8);
    b.push(c.donated);
    b.push(c.difficulty);
    b
}

fn apply_block_c(game: &mut Game, b: &[u8]) {
    let c = &mut game.campaign;
    c.vs_mode = get_i32(b, 0) != 0;
    c.between_stages = get_i32(b, 4) != 0;
    c.stage = get_i32(b, 8) as u16;
    c.turn = get_i32(b, 0xc) as u16;
    c.next_stage = get_i32(b, 0x10) as u16;
    c.alt_next_stage = get_i32(b, 0x14) as u16;
    c.location = u16::from_le_bytes([b[0x18], b[0x19]]);
    c.cave_found = b[0x1a] != 0;
    c.donated = b[0x1b];
    c.difficulty = b[0x1c];
}

/// `DAT_00497b48`, 0x1c bytes.
fn block_d(game: &Game) -> Vec<u8> {
    let c = &game.campaign;
    let mut b = Vec::with_capacity(0x1c);
    for v in [c.fame, c.rating, c.battles_fought as i32, c.route as i32, c.disc as i32] {
        put_i32(&mut b, v);
    }
    b.extend_from_slice(&c.block_d_tail);
    b
}

fn apply_block_d(game: &mut Game, b: &[u8]) {
    let c = &mut game.campaign;
    c.fame = get_i32(b, 0);
    c.rating = get_i32(b, 4);
    c.battles_fought = get_i32(b, 8) as u16;
    c.route = get_i32(b, 0xc) as u8;
    c.disc = get_i32(b, 0x10) as u8;
    c.block_d_tail.copy_from_slice(&b[0x14..0x1c]);
}

/// `FUN_00442ef8`: an army as stored.
fn army_record(a: &Army) -> ArmyRecord {
    let mut tail = Vec::with_capacity(0x21);
    for s in &a.summons {
        tail.push(s.active);
        tail.extend_from_slice(&s.kind.to_le_bytes());
    }
    ArmyRecord {
        units: a.units.iter().map(|u| UnitRecord(u.to_bytes().to_vec())).collect(),
        gold: a.gold as u32,
        flags: a.flags,
        d6: a._d6 as u32,
        d4: a._d4 as u16,
        scale: a.stat_bonus_pct,
        items: a.inventory.clone(),
        tail,
    }
}

/// `FUN_0044306f`: restore an army.
fn apply_army(a: &mut Army, r: &ArmyRecord) {
    a.units = r
        .units
        .iter()
        .map(|u| {
            let mut b = [0u8; UNIT_SIZE];
            b.copy_from_slice(&u.0[..UNIT_SIZE]);
            Unit::from_bytes(&b)
        })
        .collect();
    a.gold = r.gold as i32;
    a.flags = r.flags;
    a._d6 = r.d6 as i32;
    a._d4 = r.d4 as i16;
    a.stat_bonus_pct = r.scale;
    a.inventory = r.items.clone();
    for (i, s) in a.summons.iter_mut().enumerate() {
        let t = &r.tail[i * 3..i * 3 + 3];
        *s = SummonSlot { active: t[0], kind: i16::from_le_bytes([t[1], t[2]]) };
    }
}

/// The slot description `"%s %d.%s"` (劇情, battles fought, stage or place
/// name), padded to 25 bytes, plus "第 %2d 回" inside a stage (`FUN_004312c4`).
pub fn describe(game: &Game) -> Vec<u8> {
    let mut name = if game.campaign.between_stages {
        game.ui.place_name.clone()
    } else {
        game.stage.as_ref().map(|s| s.stage.name.clone()).unwrap_or_default()
    };
    if name.len() > 0x28 {
        name.clear();
    }
    let mut d = game.msg(0x90);
    d.extend_from_slice(format!(" {}.", game.campaign.battles_fought).as_bytes());
    d.extend_from_slice(&name);
    while d.len() < 0x19 {
        d.push(b' ');
    }
    if !game.campaign.between_stages {
        let fmt = game.msg(1);
        d.extend_from_slice(&sprintf_d(&fmt, game.campaign.turn as i32));
    }
    d.truncate(DESC_LEN - 1);
    d
}

/// `sprintf` of a Big5 format with a single `%d` / `%2d`.
fn sprintf_d(fmt: &[u8], v: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] == b'%' {
            let mut j = i + 1;
            let mut width = 0usize;
            while j < fmt.len() && fmt[j].is_ascii_digit() {
                width = width * 10 + (fmt[j] - b'0') as usize;
                j += 1;
            }
            if j < fmt.len() && fmt[j] == b'd' {
                out.extend_from_slice(format!("{v:>width$}").as_bytes());
                i = j + 1;
                continue;
            }
        }
        out.push(fmt[i]);
        if fmt[i] >= 0x81 && i + 1 < fmt.len() {
            out.push(fmt[i + 1]);
            i += 1;
        }
        i += 1;
    }
    out
}

/// The whole save image for the current state (the body of `FUN_004312c4`).
pub fn save_bytes(game: &Game, desc: &[u8]) -> Vec<u8> {
    let mut description = desc.to_vec();
    description.resize(DESC_LEN, 0);
    let mut copyright = COPYRIGHT.to_vec();
    copyright.resize(COPYRIGHT_LEN, 0);
    let mut globals_a = game.campaign.flags[1..=80].to_vec();
    globals_a.resize(0x50, 0);
    let mut treasure = Vec::with_capacity(0xa0);
    for spot in &game.events.treasure {
        for v in spot {
            treasure.extend_from_slice(&v.to_le_bytes());
        }
    }
    let script_flags = if game.campaign.between_stages {
        None
    } else {
        let b = crate::event::save_state(game);
        Some(b[2..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
    };
    SaveFile {
        description,
        copyright,
        globals_a,
        globals_b: block_b(game),
        globals_c: block_c(game),
        treasure_count: game.events.treasure_count as u32,
        treasure,
        globals_d: block_d(game),
        player: army_record(game.battle.army(Side::Player)),
        enemy: army_record(game.battle.army(Side::Enemy)),
        script_flags,
    }
    .to_bytes()
}

/// Restore the blocks of a save image (the data part of `FUN_00431845`).
/// Stage reloading and redraw are done by [`load_game`].
pub fn apply_save(game: &mut Game, b: &[u8]) -> Result<()> {
    let n_items = game.battle.tables.as_ref().map(|t| t.items.len()).context("battle tables not loaded")?;
    let s = SaveFile::parse(b, n_items)?;
    game.campaign.flags[1..=80].copy_from_slice(&s.globals_a[..0x50]);
    apply_block_b(game, &s.globals_b);
    apply_block_c(game, &s.globals_c);
    game.events.treasure_count = (s.treasure_count as i32).clamp(0, 0x14);
    for (i, spot) in game.events.treasure.iter_mut().enumerate() {
        for (k, v) in spot.iter_mut().enumerate() {
            let o = (i * 4 + k) * 2;
            *v = i16::from_le_bytes([s.treasure[o], s.treasure[o + 1]]);
        }
    }
    apply_block_d(game, &s.globals_d);
    apply_army(game.battle.army_mut(Side::Player), &s.player);
    apply_army(game.battle.army_mut(Side::Enemy), &s.enemy);
    game.ui.pending_script_state = s.script_flags.map(|f| {
        let mut v = (f.len() as i16).to_le_bytes().to_vec();
        for x in f {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v
    });
    sync_battle_mirrors(game);
    Ok(())
}

/// Copy the campaign globals BATTLE mirrors.
pub fn sync_battle_mirrors(game: &mut Game) {
    let c = &game.campaign;
    let b = &mut game.battle;
    b.difficulty = c.difficulty;
    b.vs = c.vs_mode;
    b.turn = c.turn as i32;
    b.route = c.route;
}

fn write_slot_file(game: &Game, slot: usize, bytes: &[u8]) -> Result<()> {
    let dir = game.data.dir.root().join("SAVE");
    std::fs::create_dir_all(&dir)?;
    let name = format!("UJ{:02}.SAV", slot + 1);
    let path = game.data.dir.resolve(&slot_path(slot)).unwrap_or_else(|_| dir.join(&name));
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// Port of `FUN_0043119b(n)`: before overwriting 1-based slot `n`, keep the
/// old file as the backup slot 52 (and its description).
fn backup_slot(game: &mut Game, n: usize) {
    if game.ui.save_desc[n - 1].is_empty() {
        return;
    }
    if let Ok(b) = game.data.dir.read(&slot_path(n - 1)) {
        if write_slot_file(game, SLOT_BACKUP, &b).is_ok() {
            game.ui.save_desc[SLOT_BACKUP] = game.ui.save_desc[n - 1].clone();
        }
    }
}

/// Port of `FUN_004312c4(n)`: write 1-based slot `n`.
pub fn write_save(game: &mut Game, n: usize) -> Result<()> {
    backup_slot(game, n);
    let desc = describe(game);
    game.ui.save_desc[n - 1] = desc.clone();
    let bytes = save_bytes(game, &desc);
    write_slot_file(game, n - 1, &bytes)
}

/// `FUN_0043174d`: the loading progress frame.
fn progress_frame(game: &mut Game, pct: i32) {
    let p = game.scr.page();
    gfx::rect(p, 0xd7, 0x10e, 0x1a9, 0x128, 0xe9, false);
    gfx::rect(p, 0xd8, 0x10f, 0x1a8, 0x127, 0xfd, false);
    gfx::rect(p, 0xd9, 0x110, 0x1a7, 0x126, 0xfa, true);
    gfx::hline(p, 0x112, 0xdb, 0x1a5, 0xf8);
    gfx::hline(p, 0x124, 0xdb, 0x1a5, 0xfd);
    gfx::vline(p, 0xdb, 0x113, 0x123, 0xf8);
    gfx::vline(p, 0x1a5, 0x112, 0x123, 0xf8);
    progress(game, pct);
}

/// `FUN_004316b4`: fill the bar to `pct` and print "pct%".
fn progress(game: &mut Game, pct: i32) {
    let p = game.scr.page();
    gfx::rect(p, 0xdc, 0x113, 0x1a4, 0x123, 0xfa, true);
    gfx::rect(p, 0xdc, 0x113, pct * 2 + 0xdc, 0x123, 0xa7, true);
    text::small(game, 0x130, 0x113, format!("{pct}%").as_bytes(), 100, 0, 1, 2);
    gfx::present(game);
}

/// Port of `FUN_00431845`: load the save at 0-based `slot`.
pub fn load_game(game: &mut Game, slot: usize) -> Result<()> {
    let msg = game.msg(0xe7);
    ui::message_box(game, 0xb0, 200, &msg, Wait::Warp);
    cursor::draw_present(game, Some(SHAPE_BUSY), false);
    super::stage::reset(game);
    progress_frame(game, 0);
    super::stop_music(game);
    let was_between = game.campaign.between_stages;
    let disc_before = game.campaign.disc;
    let b = game.data.dir.read(&slot_path(slot))?;
    game.battle.load_tables(game.data)?;
    apply_save(game, &b)?;
    if !game.campaign.between_stages && was_between {
        // FUN_0040ff73: the world-map pictures are freed.
        game.ui.world = None;
    }
    // The original reads the file block by block, showing progress between
    // reads (one present each); `apply_save` took it in one go.
    for pct in [10, 0x14, 0x1e, 0x28] {
        progress(game, pct);
    }
    if game.campaign.disc != disc_before {
        // FUN_0046de06 (with a CD device it only probes the drive), then the
        // loading box again.
        let msg = game.msg(0xe7);
        ui::message_box(game, 0xb0, 200, &msg, Wait::Warp);
        progress_frame(game, 0x32);
    }
    let between = game.campaign.between_stages;
    if !between {
        let id = game.campaign.stage;
        super::stage::load_stage(game, id, 7)?;
        progress(game, 0x37);
        super::stage::map_init(game)?;
    }
    // Both armies (FUN_0044306f: sprite slots on a list FUN_00424946 emptied,
    // portraits renumbered by FUN_0043fa24(army, 0)), then FUN_0043f5de / FUN_0043f693.
    game.battle.slots.reset();
    for side in [Side::Player, Side::Enemy] {
        game.battle.slots_loaded(side, between);
    }
    for pct in [0x3c, 0x3c, 0x41, 0x46, 0x50] {
        progress(game, pct);
    }
    if !between {
        // FUN_00417369: the event script state.
        if let Some(st) = game.ui.pending_script_state.take() {
            let mut p = st.as_slice();
            crate::event::load_state(game, &mut p);
        }
    }
    progress(game, 0x5a);
    if !between {
        // FUN_004403b5 per army, with 0x5f between them.
        super::stage::after_load(game);
        progress(game, 0x5f);
    }
    progress(game, 100);
    if game.campaign.between_stages {
        let m = game.res().master.clone();
        gfx::fade(game, &m, 1, 0);
    }
    game.sys.audio.set_midi_volume(game.cfg.midi_volume.clamp(0, 100) as u8);
    cursor::set_shape(game, SHAPE_POINTER);
    Ok(())
}

/// Screen modes of [`saveload_screen`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Load = 0,
    Save = 1,
    BlueArmy = 2,
    RedArmy = 3,
}

/// Row `r` (0..13) → 0-based slot and screen y.
fn row_slot(game: &Game, r: i32, top: i32) -> (usize, i32) {
    if r < 10 {
        ((game.ui.save_page * 10 + r) as usize, top + 0x2e + r * 0x1e)
    } else {
        ((r + 0x28) as usize, top + 0x16e + (r - 10) * 0x1e)
    }
}

/// Port of `FUN_00430405(panel, mode, fade_in)`. Returns 0 (cancel), 7 / 8
/// (loaded; 8 = the turn-1 autosave), 9 (saved) or 1-based slot for the army picks.
pub fn saveload_screen(game: &mut Game, mode: Mode, fade_in: bool) -> Result<i32> {
    if game.campaign.vs_mode && (mode as i32) < 2 {
        return Ok(0);
    }
    let panel = game.res().ui_image(0xcc)?;
    game.flush_events(0x1f);
    let x0 = 0x140 - panel.width as i32 / 2;
    let y0 = 0xf0 - panel.height as i32 / 2;
    let cur = game.scr.cur;
    game.scr.copy_page(1, cur);
    super::init::scan_saves(game);
    let titles = [0x80usize, 0x81, 0x10f, 0x110];
    let mut first = true;
    let mut warp_pending = true;
    let mut full = true;
    let mut done = false;
    let mut moved = false;
    let mut row: i32 = 0;
    let mut result = 0;
    let mut changed = false;
    let empty = |game: &Game, slot: usize| game.ui.save_desc.get(slot).is_none_or(|d| d.is_empty());
    loop {
        if done {
            game.flush_events(0x1f);
            return Ok(result);
        }
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        if full {
            game.scr.set_page(1);
            gfx::blit(game.scr.page(), &panel, x0, y0, false);
            let t = game.msg(titles[mode as usize]);
            text::large(game, x0 + 0xb4, y0 + 0xc, &t, 0x49, 0x37, 1, 0);
            for r in 0..13 {
                let (slot, y) = row_slot(game, r, y0);
                text::number(game, x0 + 0x4a, y, slot as i32 + 1, 0xe5, 0, 1, 2, 0);
                let d = if empty(game, slot) { game.msg(0x91) } else { game.ui.save_desc[slot].clone() };
                text::small(game, x0 + 0x60, y, &d, 0xe5, 0, 1, 2);
            }
            if game.ui.save_page > 0 {
                let s = game.msg(0x9a);
                text::small(game, x0 + 0x56, y0 + 0x155, &s, 0xaf, 0, 1, 2);
            }
            if game.ui.save_page < 4 {
                let s = game.msg(0x9b);
                text::small(game, x0 + 0x96, y0 + 0x155, &s, 0xaf, 0, 1, 2);
            }
            let pg = format!("{}/{}", game.ui.save_page + 1, 5);
            text::small(game, x0 + 0x13c, y0 + 0x155, pg.as_bytes(), 0xaf, 0, 1, 2);
            game.scr.set_page(0);
            full = false;
            changed = true;
        }
        if warp_pending {
            let y = if row < 10 { y0 + 0x3d + row * 0x1e } else { y0 + 0x17d + (row - 10) * 0x1e };
            cursor::warp(game, 0x140, y);
            warp_pending = false;
            moved = true;
        }
        if !changed {
            if moved {
                cursor::draw_present(game, None, true);
                moved = false;
            }
        } else {
            game.scr.copy_page(0, 1);
            let (slot, y) = row_slot(game, row, y0);
            text::number(game, x0 + 0x4a, y, slot as i32 + 1, 0xaf, 0, 1, 2, 0);
            let d = if empty(game, slot) { game.msg(0x91) } else { game.ui.save_desc[slot].clone() };
            text::small(game, x0 + 0x60, y, &d, 0xaf, 0, 1, 2);
            if first {
                cursor::capture(game);
            }
            cursor::draw(game, false);
            gfx::present(game);
            if first && fade_in {
                let m = game.res().master.clone();
                gfx::fade(game, &m, 0, -4);
            }
            first = false;
            moved = false;
            changed = false;
        }
        let mut action = 0;
        if game.get_event(0xffff) && (1..=0x20).contains(&game.ev.kind) {
            let e = game.ev;
            match e.kind {
                ev::KEY_DOWN | ev::KEY_REPEAT => {
                    match e.ch() {
                        0 => {
                            action = match e.scan() {
                                scan::UP => 0x48,
                                scan::DOWN => 0x50,
                                scan::PGUP => 0x49,
                                scan::PGDN => 0x51,
                                _ => 0,
                            }
                        }
                        0x0d | 0x20 => {
                            action = 0x39;
                            game.wait_event(ev::KEY_UP);
                        }
                        0x1b => {
                            action = 1;
                            game.wait_event(ev::KEY_UP);
                        }
                        _ => {}
                    }
                    game.flush_events(ev::KEY_DOWN | ev::KEY_REPEAT | ev::KEY_UP);
                }
                ev::MOUSE_DOWN | ev::MOUSE_MOVE => {
                    if e.kind == ev::MOUSE_MOVE {
                        moved = true;
                    }
                    let (mx, my) = (e.x, e.y);
                    if my < y0 + 0x154 || y0 + 0x168 < my {
                        if x0 + 0x4a <= mx && mx <= x0 + 0x16b {
                            let r = if my < y0 + 0x29 || y0 + 0x155 <= my {
                                if my < y0 + 0x169 || y0 + 0x1c3 <= my || mode == Mode::Save {
                                    -1
                                } else {
                                    (my - y0 - 0x169) / 0x1e + 10
                                }
                            } else {
                                (my - y0 - 0x29) / 0x1e
                            };
                            if r != -1 {
                                let (slot, _) = row_slot(game, r, y0);
                                if mode == Mode::Save || !empty(game, slot) {
                                    if row == r {
                                        if e.mods & ev::M_LBUTTON != 0 {
                                            game.wait_event(ev::MOUSE_UP);
                                            action = 0x39;
                                        }
                                    } else {
                                        row = r;
                                        changed = true;
                                    }
                                }
                            }
                        }
                    } else if e.mods & ev::M_LBUTTON != 0 {
                        game.wait_event(ev::MOUSE_UP);
                        if x0 + 0x54 <= mx && mx <= x0 + 0x88 {
                            action = 0x49;
                        } else if x0 + 0x96 <= mx && mx <= x0 + 200 {
                            action = 0x51;
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
        let mut repage = false;
        match action {
            1 => {
                result = 0;
                done = true;
            }
            0x39 => {
                let (slot, _) = row_slot(game, row, y0);
                match mode {
                    Mode::Load if !empty(game, slot) => {
                        super::stop_music(game);
                        load_game(game, slot)?;
                        cursor::set_shape(game, SHAPE_POINTER);
                        result = if slot == SLOT_TURN1 { code::LOADED_SLOT50 } else { code::LOADED };
                        done = true;
                    }
                    Mode::Save => {
                        let mut ok = true;
                        if !empty(game, slot) {
                            let q = game.msg(0xfd);
                            ok = ui::yes_no(game, 0xb0, row * 0x1e + 0x4c, true, &q, false, false);
                        }
                        if ok {
                            write_save(game, slot + 1)?;
                            let m = game.msg(0x34);
                            ui::message_box(game, 0xb0, 0x50, &m, Wait::Click(0x32));
                            result = 9;
                            done = true;
                        } else {
                            full = true;
                        }
                    }
                    Mode::BlueArmy | Mode::RedArmy if !empty(game, slot) => {
                        result = slot as i32 + 1;
                        done = true;
                    }
                    _ => {}
                }
            }
            0x48 | 0x50 => {
                let step = if action == 0x48 { -1 } else { 1 };
                if mode == Mode::Save {
                    row = (row + step).rem_euclid(10);
                } else {
                    for _ in 0..0xe {
                        row = (row + step).rem_euclid(13);
                        let (slot, _) = row_slot(game, row, y0);
                        if !empty(game, slot) {
                            break;
                        }
                    }
                }
                changed = true;
                warp_pending = true;
            }
            0x49 if game.ui.save_page > 0 => {
                game.ui.save_page -= 1;
                full = true;
                repage = true;
            }
            0x51 if game.ui.save_page < 4 => {
                game.ui.save_page += 1;
                full = true;
                repage = true;
            }
            _ => {}
        }
        if repage && mode != Mode::Save {
            // A page change moves the selection to the next occupied slot.
            for _ in 0..0xe {
                let (slot, _) = row_slot(game, row, y0);
                if !empty(game, slot) {
                    break;
                }
                row = (row + 1) % 13;
            }
        }
    }
}

/// Port of `FUN_004322bc(fade_in)`: the load screen, if any save exists.
/// Returns 7 / 8 when a save was loaded, -1 when there is nothing to load.
pub fn load_game_menu(game: &mut Game, fade_in: bool) -> Result<i32> {
    if !super::init::scan_saves(game) {
        return Ok(-1);
    }
    if !fade_in {
        cursor::erase(game);
        let lut = game.res().tints[6];
        gfx::remap_page(game.scr.page(), &lut);
    }
    saveload_screen(game, Mode::Load, fade_in)
}

/// Port of `FUN_0043225f(fade_in)`: the save screen.
pub fn save_game_menu(game: &mut Game, fade_in: bool) -> Result<i32> {
    if !fade_in {
        cursor::erase(game);
        let lut = game.res().tints[6];
        gfx::remap_page(game.scr.page(), &lut);
    }
    saveload_screen(game, Mode::Save, fade_in)
}

/// Port of `FUN_00432331(side)`: pick a save as the blue (0) / red (1) army.
/// Returns the 1-based slot (0 = none).
pub fn pick_army_save(game: &mut Game, side: usize) -> Result<usize> {
    if !super::init::scan_saves(game) {
        return Ok(0);
    }
    let mode = if side == 0 { Mode::BlueArmy } else { Mode::RedArmy };
    Ok(saveload_screen(game, mode, false)?.max(0) as usize)
}

/// Every unit of both armies (for sprite reloads after a load).
pub fn all_units(game: &Game) -> Vec<UnitRef> {
    let mut v = Vec::new();
    for side in [Side::Player, Side::Enemy] {
        for i in 0..game.battle.army(side).units.len() {
            v.push(UnitRef::new(side, i));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2data::save::SLOTS;

    #[test]
    fn slot_files_are_one_based() {
        assert_eq!(slot_path(0), "SAVE\\UJ01.SAV");
        assert_eq!(slot_path(SLOT_BACKUP), "SAVE\\UJ53.SAV");
        assert_eq!(SLOTS, 53);
    }

    fn data() -> Option<crate::Data> {
        let dir = std::env::var_os("S2_GAME_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("games/super2sp/extracted/app")))?;
        if !dir.join("SUPERSP.PAL").exists() {
            eprintln!("skipping: no game data at {}", dir.display());
            return None;
        }
        Some(crate::Data::open(&dir).expect("open game data"))
    }

    /// Save then load restores the same state: campaign globals, config
    /// block, story flags, treasure, both armies and the event table.
    #[test]
    fn save_then_load_round_trips() {
        let Some(data) = data() else { return };
        let mut sys = crate::sys::Sys::new_headless(crate::sys::Shared::new(), Box::new(|_, _| {}));
        let mut game = Game::new(&mut sys, &data);
        game.res = Some(crate::res::Res::load(&data, &game.cfg).unwrap());
        game.battle.load_tables(&data).unwrap();
        // A stage in progress with some state.
        super::super::stage::load_stage(&mut game, 1, 10).unwrap();
        game.battle.deploy_turn(1);
        let c = &mut game.campaign;
        c.difficulty = 4;
        c.turn = 3;
        c.fame = 17;
        c.rating = -2;
        c.battles_fought = 5;
        c.route = 1;
        c.music = 9;
        c.flags[7] = 1;
        c.flags[44] = 1;
        c.block_d_tail = [1, 2, 3, 4, 5, 6, 7, 8];
        game.cfg.text_speed = 25;
        game.cfg.mesh = false;
        game.events.treasure[3] = [1, 2, 3, 4];
        game.events.treasure_count = 4;
        if let Some(e) = game.events.events.get_mut(0) {
            e.kind = 0;
        }
        let a = game.battle.army_mut(Side::Player);
        a.gold = 1234;
        a.inventory[10] = 7;
        a.summons[2] = SummonSlot { active: 1, kind: 0x75 };
        assert!(!game.battle.army(Side::Player).units.is_empty(), "stage 1 deploys player units");
        game.battle.army_mut(Side::Player).units[0].hp = 3;
        let bytes = save_bytes(&game, b"test");

        let mut sys2 = crate::sys::Sys::new_headless(crate::sys::Shared::new(), Box::new(|_, _| {}));
        let mut g2 = Game::new(&mut sys2, &data);
        g2.res = Some(crate::res::Res::load(&data, &g2.cfg).unwrap());
        g2.battle.load_tables(&data).unwrap();
        super::super::stage::load_stage(&mut g2, 1, 10).unwrap();
        apply_save(&mut g2, &bytes).unwrap();
        let st = g2.ui.pending_script_state.take().expect("script state saved inside a stage");
        crate::event::load_state(&mut g2, &mut st.as_slice());

        let (c1, c2) = (&game.campaign, &g2.campaign);
        assert_eq!((c2.difficulty, c2.turn, c2.fame, c2.rating, c2.battles_fought, c2.route, c2.music), (4, 3, 17, -2, 5, 1, 9));
        assert_eq!(c1.flags, c2.flags);
        assert_eq!(c2.block_d_tail, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!((g2.cfg.text_speed, g2.cfg.mesh), (25, false));
        assert_eq!(g2.events.treasure[3], [1, 2, 3, 4]);
        assert_eq!(g2.events.events.first().map(|e| e.kind), Some(0));
        let (a1, a2) = (game.battle.army(Side::Player), g2.battle.army(Side::Player));
        assert_eq!((a2.gold, a2.inventory[10], a2.summons[2]), (1234, 7, SummonSlot { active: 1, kind: 0x75 }));
        assert_eq!(a1.units.len(), a2.units.len());
        for (u1, u2) in a1.units.iter().zip(&a2.units) {
            assert_eq!(u1.to_bytes(), u2.to_bytes());
        }
        // And the image itself is reproduced byte for byte.
        assert_eq!(save_bytes(&g2, b"test"), bytes);
    }

    #[test]
    fn sprintf_handles_width_and_big5() {
        assert_eq!(sprintf_d(b"\xb2\xc4 %2d \xa6^", 7), b"\xb2\xc4  7 \xa6^".to_vec());
    }
}
