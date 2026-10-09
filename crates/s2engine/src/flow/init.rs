//! Start-up: config, resources and the save-slot scan.

use anyhow::Result;

use crate::cursor;
use crate::game::{Config, Game};
use crate::gfx;
use crate::res::Res;
use crate::ui::{self, Wait};

/// Port of `FUN_00423b00`: read SUPERSP.CFG over the defaults.
pub fn load_config(game: &Game) -> Config {
    let mut c = Config::default();
    let Ok(raw) = game.data.dir.read("supersp.cfg") else { return c };
    let text = String::from_utf8_lossy(&raw);
    let nums = |s: &str| -> Vec<i32> { s.split(',').map(|v| v.trim().parse().unwrap_or(0)).collect() };
    let mut lines = text.lines();
    // Lines are read in this fixed order; a mismatch stops parsing.
    let Some(dev) = lines.next().and_then(|l| l.strip_prefix("DEVICE=")) else { return c };
    let parts: Vec<&str> = dev.splitn(3, ',').collect();
    if parts.len() < 3 {
        return c;
    }
    // DEVICE=%d,%d,%s → DAT_004944f0, DAT_004944f4, drive letter.
    c.prg_local = parts[0].trim().parse::<i32>().unwrap_or(0) != 0;
    c.mainland = parts[1].trim().parse::<i32>().unwrap_or(0) != 0;
    c.drive = parts[2].trim().to_string();
    if parts[2].trim().starts_with('0') {
        c.cd_mode = false;
    }
    let mut next = lines.next();
    if let Some(v) = next.and_then(|l| l.strip_prefix("VOLUME=")) {
        let v = nums(v);
        if v.len() == 3 {
            (c.midi_volume, c.cd_volume, c.sfx_volume) = (v[0], v[1], v[2]);
        }
        next = lines.next();
    }
    if let Some(v) = next.and_then(|l| l.trim().strip_prefix("CDDA=")) {
        c.cd_audio = v.starts_with("ON");
        let rest = if c.cd_audio { &v[2..] } else { v.get(3..).unwrap_or("") };
        if rest.starts_with(",OFF") {
            c.movies = false;
        }
        next = lines.next();
    }
    if let Some(v) = next.and_then(|l| l.strip_prefix("SET=")) {
        let v = nums(v);
        if v.len() == 8 {
            // Argument order verified in the disassembly at 0x423d9f.
            c.screen_speed = v[0];
            c.text_speed = v[1];
            c.confirm = v[2] != 0;
            c.auto_switch = v[3] != 0;
            c.status_window = v[4] != 0;
            c.quick_switch = v[5] != 0;
            c.battle_anim = v[6] != 0;
            c.mesh = v[7] != 0;
        }
    }
    c
}

/// Port of the data side of `FUN_00424450` (window, CD and audio device
/// setup belong to the frontend).
pub fn game_init(game: &mut Game) -> Result<()> {
    game.cfg = load_config(game);
    // FUN_00423b00 also sets the current music cue `DAT_00494653` to 1.
    game.campaign.music = 1;
    let res = Res::load(game.data, &game.cfg)?;
    game.res = Some(res);
    game.sys.audio.set_midi_volume(game.cfg.midi_volume.clamp(0, 100) as u8);
    game.sys.audio.sfx_volume = game.cfg.sfx_volume.clamp(0, 127) as u8;
    // FUN_00433906 + FUN_0046ecc2.
    cursor::set_shape(game, cursor::SHAPE_POINTER);
    // "資料讀取中" while the tables load, then the master palette.
    game.scr.clear();
    let msg = game.msg(0xe7);
    ui::message_box(game, 0xb0, 200, &msg, Wait::Warp);
    let master = game.res().master.clone();
    gfx::set_palette(game, &master);
    // FUN_004217a0: enable the software cursor.
    cursor::enable(game, true);
    // FUN_00423ad4 / DAT_00494684 / FUN_0042441d.
    game.campaign = Default::default();
    // FUN_00407490 (early in FUN_00424450): the disc in the drive; nothing
    // later resets it, so new games and saves carry it.
    if game.cfg.cd_mode {
        if let Some(disc) = probe_disc(game) {
            game.campaign.disc = disc;
        }
    }
    crate::event::clear_all_flags(game);
    super::newgame::reset_campaign_counters(game);
    game.events.ui = ui_hooks();
    Ok(())
}

/// Port of `FUN_0046dd42` (as `FUN_00407490` uses it): the disc in the CD
/// drive per its `DISK.ID` ("1" → disc 0, "2" → disc 1). This v1.4 build reads
/// `<drive>\DISK.ID` (template `".\DISK.ID"` with the drive letter over its
/// first byte), so the repack's `DEVICE=…,.` makes it the game folder's file.
/// `None` when unreadable: the original then asks for the disc
/// (`FUN_0040751c`, up to three rounds) and shows message 0xed; not ported.
pub fn probe_disc(game: &Game) -> Option<u8> {
    let drive = game.cfg.drive.chars().next()?.to_ascii_uppercase();
    let b = game.data.dir.read(&format!("{drive}\\DISK.ID")).ok()?;
    let text = String::from_utf8_lossy(&b);
    let digits: String = text.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
    match digits.parse::<i32>().ok()? {
        1 => Some(0),
        2 => Some(1),
        _ => None,
    }
}

/// The event engine's display hooks, pointed at FLOW's implementations.
pub fn ui_hooks() -> crate::event::ui::UiHooks {
    crate::event::ui::UiHooks {
        show_picture: super::ending::show_picture,
        play_movie: super::movie::script_movie,
        fade: |g, out| {
            let m = g.res().master.clone();
            gfx::fade(g, &m, out as i32, 0);
        },
        present: gfx::present,
        page_copy: |g, src, dst| g.scr.copy_page(dst as usize, src as usize),
        ..Default::default()
    }
}

/// Port of `FUN_00430244`: read the description of every save slot into
/// `ui.save_desc`. Returns whether any slot file exists.
pub fn scan_saves(game: &mut Game) -> bool {
    let mut any = false;
    for slot in 0..s2data::save::SLOTS {
        let name = super::save::slot_path(slot);
        let desc = match game.data.dir.read(&name) {
            Ok(b) => {
                any = true;
                let m = s2data::save::MAGIC.len();
                if b.len() >= m + s2data::save::DESC_LEN && &b[..m] == s2data::save::MAGIC {
                    let d = &b[m..m + s2data::save::DESC_LEN];
                    // A description that fills all 50 bytes is cut at 49.
                    let end = d.iter().position(|&c| c == 0).unwrap_or(0x31).min(0x31);
                    d[..end].to_vec()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        };
        game.ui.save_desc[slot] = desc;
    }
    any
}
