//! Stage win / lose evaluation (`FUN_0043311d`, `FUN_00433161`, `FUN_00433257`).

use crate::game::Game;

use super::battle_hooks::{self as bh, Side};
use super::{flag_chain_all, FLAG_FORCE_LOSE, FLAG_FORCE_WIN};

/// Result of [`check_stage`] (the original's 0 / 1 / 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageCheck {
    Continue = 0,
    Victory = 1,
    Defeat = 2,
}

/// Port of `FUN_0043311d`: run the flag chain, then test defeat before victory.
pub fn check_stage(game: &mut Game) -> StageCheck {
    if flag_chain_all(game, 0) != 0 {
        // FUN_0041be4c always returns 0; kept for shape.
        return StageCheck::Continue;
    }
    if check_defeat(game) {
        StageCheck::Defeat
    } else if check_victory(game) {
        StageCheck::Victory
    } else {
        StageCheck::Continue
    }
}

/// Port of `FUN_00433161`. Lose codes: 0 main party down; 1..=0x2b that
/// character dead; 101..=190 turn `code-100` reached. Turn 100 and flag 5
/// always lose. In VS mode: the player army is wiped out.
pub fn check_defeat(game: &mut Game) -> bool {
    if game.campaign.vs_mode {
        return bh::army_all_down(game, Side::Player, 0);
    }
    let turn = game.campaign.turn as i32;
    if turn >= 100 {
        return true;
    }
    if game.campaign.flags[FLAG_FORCE_LOSE] != 0 {
        return true;
    }
    let codes = match &game.events.stage {
        Some(st) => st.lose_codes.clone(),
        None => return false,
    };
    for code in codes {
        let code = code as i32;
        if code == 0 {
            if bh::army_all_down(game, Side::Player, -1) {
                return true;
            }
        } else if !(1..=0x2b).contains(&code) {
            if 100 < code && code < 0xbf && code - 100 <= turn {
                return true;
            }
        } else if bh::army_all_down(game, Side::Player, code) {
            return true;
        }
    }
    false
}

/// Port of `FUN_00433257`. Either win set holds when all its codes hold
/// (an empty set never does); the set number (1 / 2) goes to
/// `events.win_set`. Flag 4 forces a win with `win_set = 0`. In VS mode:
/// the enemy army is wiped out.
pub fn check_victory(game: &mut Game) -> bool {
    if game.campaign.vs_mode {
        return bh::army_all_down(game, Side::Enemy, 0);
    }
    if game.campaign.flags[FLAG_FORCE_WIN] != 0 {
        game.events.win_set = 0;
        return true;
    }
    let sets = match &game.events.stage {
        Some(st) => st.win_sets.clone(),
        None => return false,
    };
    for (n, set) in sets.iter().enumerate() {
        if set.is_empty() {
            continue;
        }
        if set.iter().all(|&code| win_code(game, code as i32)) {
            game.events.win_set = n as i32 + 1;
            return true;
        }
    }
    false
}

/// One win code; anything not listed never holds.
fn win_code(game: &mut Game, code: i32) -> bool {
    match code {
        0 => bh::army_all_down(game, Side::Enemy, 0),
        1..=0x14 => {
            let n = game.events.treasure_count;
            n >= 1 && code <= n && game.events.treasure[code as usize - 1][0] == 0
        }
        0x1f => bh::on_exit_tile(game, 0) && game.campaign.flags[1] != 0,
        0x20 => bh::on_exit_tile(game, 1),
        0x21 => bh::army_all_down(game, Side::Enemy, -2),
        0x22 => bh::army_all_down(game, Side::Enemy, -3),
        _ => false,
    }
}
