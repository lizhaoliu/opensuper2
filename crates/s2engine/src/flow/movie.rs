//! Cutscenes: port of the FLC player `FUN_00459566` and the opening `FUN_0041a370`.

use s2data::anim::{deobfuscate_header, load_cutscenes, open_cutscene, FlcHeader, CUTSCENE_DAT_BASE, MAGIC_FLC};
use s2data::Kind;

use crate::game::Game;
use crate::gfx;
use crate::sys::ev;
use crate::ui;

/// Player flags (`param_2` of `FUN_00459566`).
pub mod flag {
    /// Keep the music playing before and after.
    pub const KEEP_MUSIC: u32 = 1;
    /// Keep sound effects after.
    pub const KEEP_SOUNDS: u32 = 2;
    /// No fade out at the end.
    pub const NO_FADE_OUT: u32 = 4;
    /// No fade in on the first frame.
    pub const NO_FADE_IN: u32 = 8;
    /// After the last frame wait for a key; with `WAIT_TIMEOUT` at most 4 s.
    pub const WAIT_KEY: u32 = 0x40;
    pub const WAIT_TIMEOUT: u32 = 0x80;
    /// Cannot be skipped.
    pub const NO_SKIP: u32 = 0x200;
    /// Negative sound triggers loop their sample.
    pub const LOOP_SOUNDS: u32 = 0x400;
}

/// `FUN_004593a0`: wait `delay` cs after `t0`; false if a key or button was pressed.
fn wait_skippable(game: &mut Game, t0: i32, delay: i32) -> bool {
    loop {
        if game.get_event(ev::KEY_DOWN | ev::MOUSE_DOWN) {
            return false;
        }
        if !game.still_waiting(t0, delay) {
            return true;
        }
    }
}

/// Which frames of cutscene `id` carry chunks. The player reads each 16-byte
/// frame header and decodes and presents only when the frame has a body
/// (`*local_20 -= 0x10; if (*local_20 != 0)`): an empty frame just waits.
fn frames_with_body(game: &Game, id: usize) -> Vec<bool> {
    let Ok(data) = game.data.arcs.read(Kind::Dat, id + CUTSCENE_DAT_BASE) else { return Vec::new() };
    let Ok(header) = deobfuscate_header(&data).and_then(|h| FlcHeader::parse(&h)) else { return Vec::new() };
    let mut f = if header.magic == MAGIC_FLC { header.oframe1 as usize } else { 128 };
    let mut out = Vec::new();
    for _ in 0..header.frames {
        let Some(size) = data.get(f..f + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize) else { break };
        out.push(size != 16);
        f += size.max(16);
    }
    out
}

/// Port of `FUN_00459566(this, id, flags, preload)` (player object `FUN_00459340`;
/// chunk decoders `FUN_0045a355`..`FUN_0045acec` are s2data::anim). Returns false when skipped.
pub fn play(game: &mut Game, id: usize, flags: u32) -> bool {
    if flags & flag::KEEP_MUSIC == 0 {
        super::stop_music(game);
    }
    let events = load_cutscenes(&game.data.arcs).ok().and_then(|v| v.into_iter().find(|c| c.id == id)).map(|c| c.events).unwrap_or_default();
    let Ok(mut flc) = open_cutscene(&game.data.arcs, id) else { return true };
    let mut pal = vec![0u8; 0x300];
    game.scr.clear();
    let has_body = frames_with_body(game, id);
    let delay = flc.delay_cs() as i32;
    let mut t = game.ticks();
    let mut completed = true;
    let mut next_event = 0usize;
    let mut loop_sound: u16 = 0;
    let mut loop_slot = 0usize;
    for frame in 0..flc.frame_count() {
        super::music_keepalive(game);
        match flc.next_frame() {
            Ok(true) => {}
            _ => break,
        }
        if flc.palette_changed {
            pal.copy_from_slice(&flc.palette6);
            if frame != 0 {
                gfx::set_palette(game, &pal);
            }
        }
        if has_body.get(frame).copied().unwrap_or(true) {
            let (img, x, y) = flc.screen_frame();
            gfx::blit(game.scr.page(), &img, x as i32, y as i32, true);
            gfx::present(game);
        }
        if let Some(e) = events.get(next_event).copied() {
            if e.frame as usize == frame + 1 {
                if e.music >= 1 {
                    super::music(game, e.music as u16);
                } else if e.music == -1 {
                    super::stop_music(game);
                }
                if e.sound >= 1 {
                    ui::sound(game, e.sound as u16);
                } else if e.sound == -1 {
                    game.sys.audio.stop_sounds();
                } else if e.sound < -1 && flags & flag::LOOP_SOUNDS != 0 {
                    loop_sound = e.sound.unsigned_abs();
                    let arcs = &game.data.arcs;
                    loop_slot = game.sys.audio.play_sound(arcs, loop_sound, loop_slot);
                }
                next_event += 1;
            }
        }
        if flags & flag::LOOP_SOUNDS != 0 && loop_sound > 0 && loop_slot > 0 {
            // FUN_00419b1d with a slot restarts that voice once it has finished.
            let arcs = &game.data.arcs;
            loop_slot = game.sys.audio.play_sound(arcs, loop_sound, loop_slot);
        }
        if frame == 0 && flags & flag::NO_FADE_IN == 0 {
            gfx::fade(game, &pal, 0, 1);
        }
        if flags & flag::NO_SKIP == 0 {
            if !wait_skippable(game, t, delay) {
                completed = false;
                break;
            }
        } else {
            game.wait_until(t, delay);
        }
        t = game.ticks();
    }
    if flags & (flag::WAIT_KEY | flag::WAIT_TIMEOUT) != 0 && completed {
        let t3 = game.ticks();
        loop {
            if game.get_event(ev::KEY_DOWN | ev::MOUSE_DOWN) {
                break;
            }
            if flags & flag::WAIT_TIMEOUT != 0 && !game.still_waiting(t3, 400) {
                break;
            }
        }
    }
    if flags & flag::KEEP_MUSIC == 0 {
        super::stop_music(game);
    }
    if flags & flag::KEEP_SOUNDS == 0 {
        game.sys.audio.stop_sounds();
    }
    if flags & flag::NO_FADE_OUT == 0 {
        gfx::fade(game, &pal, 1, 1);
    }
    completed
}

/// Port of `FUN_0041a370`: the logo and opening movies when movies are on.
pub fn play_opening(game: &mut Game) {
    if game.cfg.movies {
        super::stop_music(game);
        play(game, 1, flag::LOOP_SOUNDS);
        play(game, 2, flag::LOOP_SOUNDS);
        super::stop_music(game);
    }
}

/// Port of `FUN_0041a631(id, mode)`: a movie started by a stage script
/// (mode 0, redraws the map and restores the music afterwards) or after a
/// won stage (mode 1).
pub fn script_movie(game: &mut Game, id: i16, mode: i32) {
    if !game.cfg.movies {
        return;
    }
    super::stop_music(game);
    let music = game.campaign.music;
    if mode == 0 {
        let m = game.res().master.clone();
        gfx::fade(game, &m, 1, 0);
    }
    game.flush_events(0x1f);
    play(game, id.max(0) as usize, 0);
    if mode == 0 {
        super::stage::draw_map_present(game);
        let m = game.res().master.clone();
        gfx::fade(game, &m, 0, 0);
        game.campaign.music = 0;
        super::music(game, music);
    }
}
