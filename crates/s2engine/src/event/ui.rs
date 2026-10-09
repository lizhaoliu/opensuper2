//! The display side of events, reached through function pointers. The
//! defaults forward to FLOW (`crate::flow::show_dialogue` / `choose`, see
//! the EVENT CONTRACT in `flow/mod.rs`); [`UiHooks::headless`] replaces the
//! drawing with no-ops for tests. Every call is also recorded in
//! `game.events.log`.

use crate::game::Game;

use super::Log;

/// UI entry points used by the event engine.
#[derive(Clone, Copy)]
pub struct UiHooks {
    /// `this[0] = event; FUN_00418408; FUN_004188ca(this, mode); FUN_00418465`:
    /// show every line of `game.events.events[event]` (mode 0 on the map,
    /// 1 over the unit-down context).
    pub show_dialogue: fn(&mut Game, usize, u32),
    /// `FUN_0041929b(this, event)`: choice menu. Line 0 of the event is the
    /// prompt, lines 1.. are the options; returns the confirmed option, 1-based.
    pub choice: fn(&mut Game, usize) -> u8,
    /// `FUN_0041b963(b, c, d, 0)`: full-screen picture.
    pub show_picture: fn(&mut Game, i16, i16, i16),
    /// `FUN_0041a631(id, mode)`: play a movie (mode 0 from scripts, 1 after a won stage).
    pub play_movie: fn(&mut Game, i16, i32),
    /// `FUN_0046d442(0x4a0b08, out, 0)`: fade the master palette out (true) / in.
    pub fade: fn(&mut Game, bool),
    /// `FUN_0046eb48(0x4947c8)`: present the back page.
    pub present: fn(&mut Game),
    /// `FUN_0046ec6b(0x4947c8, src, dst)`: copy one page to another.
    pub page_copy: fn(&mut Game, u8, u8),
}

impl std::fmt::Debug for UiHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UiHooks")
    }
}

impl Default for UiHooks {
    /// The real UI (FLOW).
    fn default() -> Self {
        UiHooks {
            show_dialogue: flow_show_dialogue,
            choice: flow_choose,
            show_picture: |_, _, _, _| {},
            play_movie: |_, _, _| {},
            fade: |_, _| {},
            present: |_| {},
            page_copy: |_, _, _| {},
        }
    }
}

impl UiHooks {
    /// Nothing is drawn and every choice picks option 1.
    pub fn headless() -> Self {
        UiHooks {
            show_dialogue: |_, _, _| {},
            choice: |_, _| 1,
            show_picture: |_, _, _, _| {},
            play_movie: |_, _, _| {},
            fade: |_, _| {},
            present: |_| {},
            page_copy: |_, _, _| {},
        }
    }
}

fn flow_show_dialogue(game: &mut Game, event: usize, mode: u32) {
    let lines = game.events.events[event].lines.clone();
    crate::flow::show_dialogue(game, &lines, mode);
}

/// The option count is the event's `b` field (`FUN_0041929b`, `iVar1`).
fn flow_choose(game: &mut Game, event: usize) -> u8 {
    let e = &game.events.events[event];
    let (lines, n) = (e.lines.clone(), e.b.max(0) as usize);
    crate::flow::choose(game, &lines, n)
}

pub fn show_dialogue(game: &mut Game, event: usize, mode: u32) {
    game.events.log.push(Log::Dialogue { event, mode });
    (game.events.ui.show_dialogue)(game, event, mode);
}

pub fn choice(game: &mut Game, event: usize) -> u8 {
    let c = (game.events.ui.choice)(game, event);
    game.events.log.push(Log::Choice { event, choice: c });
    c
}

pub fn show_picture(game: &mut Game, b: i16, c: i16, d: i16) {
    game.events.log.push(Log::Picture(b, c, d));
    (game.events.ui.show_picture)(game, b, c, d);
}

pub fn play_movie(game: &mut Game, id: i16, mode: i32) {
    game.events.log.push(Log::Movie(id, mode));
    (game.events.ui.play_movie)(game, id, mode);
}

pub fn fade(game: &mut Game, out: bool) {
    (game.events.ui.fade)(game, out);
}

pub fn present(game: &mut Game) {
    (game.events.ui.present)(game);
}

pub fn page_copy(game: &mut Game, src: u8, dst: u8) {
    (game.events.ui.page_copy)(game, src, dst);
}

/// `FUN_0046d652(n)`: busy-wait n centiseconds (n <= 0 returns at once).
pub fn wait_cs(game: &mut Game, n: i32) {
    if n > 0 {
        game.sys.sleep(n as u32 * 10);
    }
}
