//! OpenSuper2 game logic.
//!
//! Code here is a port of the original program's behaviour (see `re/` and
//! `docs/` for the reverse-engineering notes it is based on). It draws into
//! [`sys::Sys`] and never touches the OS directly, so it runs the same under
//! the desktop frontend and the headless test harness.

pub mod audio;
pub mod battle;
pub mod cursor;
pub mod event;
pub mod flow;
pub mod game;
pub mod gfx;
pub mod qa;
pub mod res;
pub mod sys;
pub mod text;
pub mod ui;

use std::path::Path;

use anyhow::Result;
use s2data::{Archives, GameDir, Palette};

/// Everything loaded from the user's game directory.
pub struct Data {
    pub dir: GameDir,
    pub arcs: Archives,
    pub palette: Palette,
}

/// Game entry point (the original `WinMain`): runs until the player quits.
pub fn run(sys: &mut sys::Sys, data: &Data) -> Result<()> {
    run_with_options(sys, data, game::Options::default())
}

/// [`run`] with options outside the original (`game::Options`).
pub fn run_with_options(sys: &mut sys::Sys, data: &Data, options: game::Options) -> Result<()> {
    let mut game = game::Game::new(sys, data);
    game.options = options;
    flow::win_main(&mut game)
}

impl Data {
    pub fn open(path: &Path) -> Result<Data> {
        let dir = GameDir::open(path)?;
        let arcs = Archives::open(&dir)?;
        let palette = Palette::from_vga(&dir.read("supersp.pal")?);
        Ok(Data { dir, arcs, palette })
    }
}
