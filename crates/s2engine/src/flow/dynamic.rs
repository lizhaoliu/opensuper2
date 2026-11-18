//! `dynamic_difficulty=1` (not the original): the enemy army's stats follow
//! how the campaign is going.
//!
//! The original has an adaptive difficulty (`FUN_0040d729`,
//! `battle::turn::adapt_difficulty`) that compares the party's average power
//! with a per-stage base, but every stage of the shipped data leaves that
//! base at 0, so it never acts. A base cannot be supplied after the fact:
//! the party's power next to the enemy army's differs by a factor of ten
//! from one stage to the next (a few leaders here, a crowd there), far more
//! than between a strong and a weak party on one stage.
//!
//! So this follows results instead. A *pressure* in % travels with the
//! campaign: a battle won without losing anyone raises it, a battle that
//! cost two or more units lowers it, and so does every defeat on the way to
//! a win. At the start of a stage the enemy army gets `pressure` (plus
//! `enemy_boost`) as its stat bonus — the original's own field, saved with
//! the army, applied to its units as they are placed (`FUN_00457d3c`). A
//! stage lost and tried again is eased at once, for that stage only.
//!
//! The pressure is kept beside each save as `SAVE\UJnn.DYN` (the saves keep
//! the original's format); a save without one starts at 0.

use crate::battle::turn::scale_army;
use crate::battle::unit::flag;
use crate::battle::Side;
use crate::game::Game;

use super::code;

/// Pressure bounds in % of enemy stats.
pub const MIN: i32 = -20;
pub const MAX: i32 = 30;
/// A stage being retried may be eased further than the pressure goes.
pub const RETRY_MIN: i32 = -40;
/// A battle won without losing a unit.
pub const STEP_CLEAN: i32 = 3;
/// Per unit lost beyond the first, at most [`LOSS_CAP`] a battle.
pub const STEP_LOSS: i32 = 4;
pub const LOSS_CAP: i32 = 12;
/// Per defeat of the stage being retried (for that stage), and how much of
/// it stays in the pressure once the stage is won.
pub const STEP_RETRY: i32 = 8;
pub const STEP_DEFEAT: i32 = 4;

/// The state of `dynamic_difficulty` (`Game::dynamic`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dynamic {
    /// % added to the enemy's stats at the next stage start.
    pub pressure: i32,
    /// The stage the two counters below belong to (0 = none).
    pub stage: u16,
    /// The party's death count when that stage began.
    pub deaths_start: i32,
    /// Defeats on that stage since it was last won (this session only: a
    /// loaded save brings back its own pressure, not these).
    pub defeats: i32,
    /// The enemy's bonus is to be shown when the player's phase begins.
    pub announce: bool,
}

/// On, in the campaign, on a battle stage (kind 1).
fn on(game: &Game) -> bool {
    game.options.dynamic_difficulty && !game.campaign.vs_mode && game.stage.as_ref().is_some_and(|s| s.stage.header.kind == 1)
}

/// Deaths so far of the party's own units (`Unit::deaths`, the original's
/// counter; NPC allies and summons are not the player's to lose).
fn party_deaths(game: &Game) -> i32 {
    game.battle
        .army(Side::Player)
        .units
        .iter()
        .filter(|u| u.npc != 1 && u.flags & flag::SUMMON == 0)
        .map(|u| u.deaths as i32)
        .sum()
}

/// The pressure after a battle won with `lost` units down and `defeats`
/// failed attempts before it.
pub fn after_win(pressure: i32, lost: i32, defeats: i32) -> i32 {
    let d = match lost {
        0 => STEP_CLEAN,
        1 => 0,
        n => -((n - 1) * STEP_LOSS).min(LOSS_CAP),
    };
    // A win that took several attempts is not a clean one.
    let d = if defeats > 0 { d.min(0) - defeats * STEP_DEFEAT } else { d };
    (pressure + d).clamp(MIN, MAX)
}

/// The enemy stat bonus % for the stage at hand.
fn bonus(game: &Game) -> i32 {
    let d = &game.dynamic;
    let retries = if d.stage == game.campaign.stage { d.defeats } else { 0 };
    (d.pressure - retries * STEP_RETRY).clamp(if retries > 0 { RETRY_MIN } else { MIN }, MAX) + game.options.enemy_boost as i32
}

/// The player's phase begins: after a stage start or a load, show the
/// enemy army's bonus for a moment.
pub fn announce(game: &mut Game) {
    if !std::mem::take(&mut game.dynamic.announce) || !on(game) {
        return;
    }
    let text = match game.battle.army(Side::Enemy).stat_bonus_pct {
        0 => "動態難度：敵軍 標準".to_string(),
        p => format!("動態難度：敵軍 {p:+}%"),
    };
    game.sys.shared().post_notice(&text);
}

/// Turn 1 of a stage, once its first units stand on the map and before the
/// turn-1 save: give the enemy army its bonus.
pub fn stage_start(game: &mut Game) {
    if !on(game) || game.campaign.turn != 1 {
        return;
    }
    let stage = game.campaign.stage;
    if game.dynamic.stage != stage {
        game.dynamic.defeats = 0;
    }
    game.dynamic.stage = stage;
    game.dynamic.deaths_start = party_deaths(game);
    let pct = bonus(game);
    crate::battle::trace!("dynamic stage {stage} pressure {} defeats {} bonus {pct}", game.dynamic.pressure, game.dynamic.defeats);
    if pct != 0 {
        scale_army(&mut game.battle, Side::Enemy, pct);
    }
    game.dynamic.announce = true;
}

/// A save was loaded inside a battle: its army already has the bonus it
/// began with. After a defeat on this stage the bonus is lower now, and the
/// enemy's units are eased to it (never the other way round).
pub fn battle_loaded(game: &mut Game) {
    if !on(game) {
        return;
    }
    let stage = game.campaign.stage;
    if game.dynamic.stage != stage {
        // No side file for this save: count from here.
        game.dynamic.stage = stage;
        game.dynamic.deaths_start = party_deaths(game);
    }
    let had = game.battle.army(Side::Enemy).stat_bonus_pct;
    let want = bonus(game);
    crate::battle::trace!("dynamic loaded stage {stage} had {had} want {want}");
    game.dynamic.announce = true;
    if want >= had {
        return;
    }
    // From (100 + had) % of the base stats to (100 + want) %.
    let rel = (100 + want) * 100 / (100 + had).max(1) - 100;
    if rel != 0 {
        scale_army(&mut game.battle, Side::Enemy, rel);
    }
    game.battle.army_mut(Side::Enemy).stat_bonus_pct = want;
}

/// A battle stage ended with code `r`.
pub fn stage_result(game: &mut Game, r: i32) {
    if !on(game) || game.dynamic.stage != game.campaign.stage {
        return;
    }
    if r == code::LOSE {
        game.dynamic.defeats += 1;
    } else if r == code::WIN {
        let lost = (party_deaths(game) - game.dynamic.deaths_start).max(0);
        let before = game.dynamic.pressure;
        game.dynamic.pressure = after_win(before, lost, game.dynamic.defeats);
        crate::battle::trace!("dynamic won stage {} lost {lost} defeats {} pressure {before} -> {}", game.campaign.stage, game.dynamic.defeats, game.dynamic.pressure);
        game.dynamic.stage = 0;
        game.dynamic.defeats = 0;
    }
}

/// `SAVE\UJnn.DYN` beside 0-based slot `slot`.
fn side_path(game: &Game, slot: usize) -> std::path::PathBuf {
    let rel = format!("SAVE\\UJ{:02}.DYN", slot + 1);
    game.data.dir.resolve(&rel).unwrap_or_else(|_| game.data.dir.root().join("SAVE").join(format!("UJ{:02}.DYN", slot + 1)))
}

/// After 0-based slot `slot` was written: its side file.
pub fn save_side(game: &Game, slot: usize) {
    if !game.options.dynamic_difficulty || game.campaign.vs_mode {
        // A side file left from a time the option was on no longer
        // describes this slot.
        let _ = std::fs::remove_file(side_path(game, slot));
        return;
    }
    let d = &game.dynamic;
    let text = format!("pressure={}\nstage={}\ndeaths_start={}\n", d.pressure, d.stage, d.deaths_start);
    let _ = std::fs::write(side_path(game, slot), text);
}

/// After 0-based slot `slot` was loaded: the pressure it was saved with
/// (0 without a side file). Defeats of the stage in hand stay counted.
pub fn load_side(game: &mut Game, slot: usize) {
    if !game.options.dynamic_difficulty {
        return;
    }
    let defeats = (game.dynamic.stage, game.dynamic.defeats);
    let mut d = Dynamic::default();
    if let Ok(text) = std::fs::read_to_string(side_path(game, slot)) {
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Ok(v) = v.trim().parse::<i32>() else { continue };
            match k.trim() {
                "pressure" => d.pressure = v.clamp(MIN, MAX),
                "stage" => d.stage = v.clamp(0, u16::MAX as i32) as u16,
                "deaths_start" => d.deaths_start = v.max(0),
                _ => {}
            }
        }
    }
    // The save is of the stage that was just lost (its turn-1 save, or one
    // made during it), or of the town before it.
    d.announce = false;
    if defeats.1 > 0 && (d.stage == defeats.0 || d.stage == 0) {
        if d.stage == 0 {
            d.stage = defeats.0;
            d.deaths_start = party_deaths(game);
        }
        d.defeats = defeats.1;
    }
    game.dynamic = d;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_wins_raise_and_losses_lower() {
        assert_eq!(after_win(0, 0, 0), STEP_CLEAN);
        assert_eq!(after_win(0, 1, 0), 0);
        assert_eq!(after_win(0, 2, 0), -STEP_LOSS);
        assert_eq!(after_win(0, 9, 0), -LOSS_CAP);
        // Retries count against the win, and a clean win after them earns nothing.
        assert_eq!(after_win(8, 0, 2), 8 - 2 * STEP_DEFEAT);
        assert_eq!(after_win(8, 3, 1), 8 - 2 * STEP_LOSS - STEP_DEFEAT);
    }

    #[test]
    fn pressure_stays_in_bounds() {
        let mut p = 0;
        for _ in 0..40 {
            p = after_win(p, 0, 0);
        }
        assert_eq!(p, MAX);
        for _ in 0..40 {
            p = after_win(p, 5, 3);
        }
        assert_eq!(p, MIN);
    }
}
