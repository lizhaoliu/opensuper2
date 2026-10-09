//! Starting a new campaign: the prologue and the first stage
//! (`FUN_0041a456`, `FUN_0041a583`).

use anyhow::Result;

use super::stage;
use crate::cursor;
use crate::event::{self, Trigger};
use crate::game::Game;
use crate::gfx;
use crate::ui::{self, Wait};

/// `DAT_0049456c` offsets: first stage of route 0 / 1 and the prologue stage.
const P_FIRST_STAGE: usize = 0xfe;
const P_PROLOGUE: usize = 0x100;
/// Story flag 41 (`DAT_00494530`), set by the prologue when 娜塔莉雅 is chosen.
const FLAG_ROUTE: usize = 41;

/// `DAT_0049456c` offsets of the starting 名聲 / 評價.
const P_FAME: usize = 0xf9;
const P_RATING: usize = 0xfa;

/// Port of `FUN_0042441d`: campaign counters for a new game.
pub fn reset_campaign_counters(game: &mut Game) {
    game.campaign.fame = game.res().param(P_FAME) as i8 as i32;
    game.campaign.rating = game.res().param(P_RATING) as i8 as i32;
    game.campaign.battles_fought = 0;
}

/// Port of `FUN_0041a583(stage)`: run a story-only stage's turn-1 events
/// on its map, then drop it.
pub fn run_script_stage(game: &mut Game, id: u16) -> Result<()> {
    stage::load_stage(game, id, 10)?;
    stage::map_init(game)?;
    stage::deploy_turn_units(game);
    stage::center_on_first_unit(game);
    stage::draw_map_present(game);
    let master = game.res().master.clone();
    gfx::fade(game, &master, 0, 0);
    game.campaign.music = 0;
    let cue = game.stage.as_ref().map(|s| s.stage.header.music).unwrap_or(0);
    super::music(game, cue);
    event::poll(game, Trigger::TurnStart);
    super::stop_music(game);
    gfx::fade(game, &master, 1, 0);
    event::clear_stage_flags(game);
    stage::reset(game);
    // FUN_0043deeb on both armies: the script stage's units do not carry over.
    game.battle.clear_army(crate::battle::Side::Player);
    game.battle.clear_army(crate::battle::Side::Enemy);
    Ok(())
}

/// Port of `FUN_0041a456`: prologue (protagonist choice), loading message,
/// first stage of the chosen route and its briefing.
pub fn newgame_start(game: &mut Game) -> Result<()> {
    cursor::enable(game, false);
    game.scr.clear();
    // FUN_0046de06(0): disc 1. With a CD device the original only probes the
    // drive and keeps the disc it detected at start-up (`DAT_00497b58`).
    if !game.cfg.cd_mode {
        game.campaign.disc = 0;
    }
    cursor::enable(game, true);
    let master = game.res().master.clone();
    gfx::fade(game, &master, 1, 0);
    let prologue = game.res().param(P_PROLOGUE) as u16;
    run_script_stage(game, prologue)?;
    game.scr.clear();
    cursor::enable(game, false);
    let msg = game.msg(0xe7);
    ui::message_box(game, 0xb0, 200, &msg, Wait::Warp);
    gfx::present(game);
    gfx::set_palette(game, &master);
    cursor::enable(game, true);
    game.campaign.route = (game.campaign.flags[FLAG_ROUTE] != 0) as u8;
    let first = game.res().param(P_FIRST_STAGE + game.campaign.route as usize) as u16;
    stage::load_stage(game, first, 10)?;
    stage::map_init(game)?;
    gfx::fade(game, &master, 1, 0);
    super::narration::narration(game, 1);
    Ok(())
}
