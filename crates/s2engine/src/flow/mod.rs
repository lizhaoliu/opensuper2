//! Game flow: startup, title, menus, between-stage screens, stage loop,
//! ending. Owned by the FLOW/UI port (docs/flow.md).
//!
//! # EVENT CONTRACT
//!
//! Stable entry points the EVENT port (stage scripts) calls for dialogue:
//!
//! ```text
//! pub fn show_dialogue(game: &mut Game, lines: &[s2data::script::Line], mode: u32)
//!     Port of FUN_004188ca(this, mode) with the portrait loading of
//!     FUN_00418408/FUN_00418478 and FUN_00418465 around it.
//!     mode 0: map dialogue (box next to the speaker's unit, open/close animation);
//!     mode 1: battle-scene dialogue (box at the default position, no map redraw).
//!
//! pub fn choose(game: &mut Game, lines: &[s2data::script::Line], option_count: usize) -> u8
//!     Port of FUN_0041929b: lines[0] is the prompt (speaker + 2-row text), lines[1..]
//!     the options (`option_count` = event field +5). Returns the 1-based choice; the
//!     event engine stores it in story flag 12 (DAT_00494513), which the original did here.
//! ```
//!
//! FLOW calls `crate::event::{attach_stage, poll(Trigger::TurnStart), ...}`.
//!
//! # FLOW API (screens the battle code may open)
//!
//! ```text
//! party::equipment(game, mode, unit: Option<UnitRef>, fade) -> Result<i32>  FUN_00447490: 0 equipment, 1 items;
//!                                       Some(r) = that unit only (battle); 1 = something changed
//! party::class_change(game, r, keep_ap) -> Result<i32>   FUN_00453986: 2 = changed (AI/NPC: auto after 1 s)
//! status::unit_status(game, r) -> Result<()>             FUN_0044feb2: unit status screen and its tabs
//! status::status_list(game, town) -> Result<i32>         FUN_0042f620: 1 = map cursor moved to a unit (battle)
//! status::overview_menu(game) -> Result<i32>             FUN_0042f033 (總檢表): unit list / map overview
//! status::map_overview(game)                             FUN_004301ae: the map at 1/3 size
//! save::save_game_menu / load_game_menu(game, fade_in)   FUN_0043225f / FUN_004322bc
//! settings::open(game, place)                            FUN_0042efcd
//! settings::system_submenu(game) -> Result<i32>          FUN_0042d7cc: 系統設定 entry of the battle
//!                                       system menu (load / save / settings / quit): 0, 7/8, 9, 0xb
//! ending::show_picture(game, picture, page, music)       FUN_0041b963(.., 0)
//! spellview::spell_help(game, spell_index0, browse)      FUN_00464727: the spell help window
//!                                       (browse = false from the battle spell menu: shadowed window)
//! ```

pub mod armies;
pub mod deploy;
pub mod dialogue;
pub mod ending;
pub mod difficulty;
pub mod init;
pub mod march;
pub mod movie;
pub mod narration;
pub mod newgame;
pub mod party;
pub mod save;
pub mod settings;
pub mod shop;
pub mod spellview;
pub mod stage;
pub mod status;
pub mod title;
pub mod town;
pub mod versus;
pub mod worldmap;

pub use dialogue::{choose, show_dialogue};

use anyhow::Result;

use crate::game::Game;

/// Top-level state codes (docs/flow.md "Top-level state codes").
pub mod code {
    pub const WIN: i32 = 1;
    pub const LOSE: i32 = 2;
    pub const LOADED: i32 = 7;
    pub const LOADED_SLOT50: i32 = 8;
    pub const NEW_STAGE: i32 = 10;
    pub const QUIT: i32 = 0xb;
    pub const FROM_WORLD_MAP: i32 = 0xd;
    pub const CAVE: i32 = 0xe;
    pub const RETREAT: i32 = 0xf;
    pub const VS_DONE: i32 = 0x14;
    pub const CAMPAIGN_DONE: i32 = 0x17;
}

/// `FUN_004198d4(cue, -1, 1)`: start a music cue (1..=0x25) unless it already plays.
pub fn music(game: &mut Game, cue: u16) {
    if (1..0x26).contains(&cue) {
        let arcs = &game.data.arcs;
        game.sys.audio.play_music(arcs, cue, true);
        game.campaign.music = cue;
    }
}

/// `FUN_00419a7b`: stop music.
pub fn stop_music(game: &mut Game) {
    game.sys.audio.stop_music();
}

/// `FUN_004198a4`: per-iteration CD-audio restart check. The MIDI backend
/// loops by itself, so there is nothing to do.
pub fn music_keepalive(_game: &mut Game) {}

/// Port of `WinMain` (`FUN_004247da`) from the point the window exists.
pub fn win_main(game: &mut Game) -> Result<()> {
    init::game_init(game)?;
    init::scan_saves(game);
    let master = game.res().master.clone();
    crate::gfx::fade(game, &master, 1, 0);
    if let Some(screen) = std::env::var_os("S2_TEST_SCREEN") {
        return test_screen(game, &screen.to_string_lossy());
    }
    movie::play_opening(game);
    let mut c = title::title_menu(game)?;
    if c != code::NEW_STAGE && c != code::QUIT && game.campaign.between_stages {
        c = town::between_stages(game)?;
    }
    state_loop(game, c)
}

/// Not in the original: `S2_TEST_SCREEN` jumps straight to a screen that is
/// otherwise only reached late in a campaign, for headless checks.
/// `ending[:slot]` loads save `slot` (1-based) and plays the ending;
/// `picture:b,c,d` shows `FUN_0041b963(b, c, d, 0)`.
fn test_screen(game: &mut Game, spec: &str) -> Result<()> {
    let (what, arg) = spec.split_once(':').unwrap_or((spec, ""));
    match what {
        "ending" => {
            if let Ok(n) = arg.parse::<usize>() {
                save::load_game(game, n - 1)?;
            }
            ending::ending(game)
        }
        "picture" => {
            let v: Vec<i16> = arg.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            ending::show_picture(game, v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(1), v.get(2).copied().unwrap_or(0));
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Port of `FUN_0040d010`: dispatch the codes returned by the stage loop.
fn state_loop(game: &mut Game, mut c: i32) -> Result<()> {
    loop {
        c = stage::stage_loop(game, c)?;
        match c {
            code::WIN | code::RETREAT => c = town::between_stages(game)?,
            code::LOADED | code::LOADED_SLOT50 => {
                if game.campaign.between_stages {
                    c = town::between_stages(game)?;
                }
            }
            code::CAVE => {
                stage::cave_stage_setup(game)?;
                c = code::FROM_WORLD_MAP;
            }
            0x15 | code::LOSE | code::VS_DONE => {
                stage::reset(game);
                c = title::title_menu(game)?;
                if c != code::NEW_STAGE && c == code::LOADED && game.campaign.between_stages {
                    c = town::between_stages(game)?;
                }
            }
            0x16 => c = code::NEW_STAGE,
            code::CAMPAIGN_DONE => {
                ending::ending(game)?;
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}
