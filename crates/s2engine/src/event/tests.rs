//! Event engine tests over the real stage scripts. They need the original
//! game data (`S2_GAME_DIR`, default ~/games/super2sp/extracted/app) and
//! are skipped when it is not there.

use std::path::PathBuf;

use s2data::script::{Stage, BIN_STAGES};
use s2data::Kind;

use super::*;
use crate::audio::Audio;
use crate::sys::{Shared, Sys};
use crate::Data;

fn data() -> Option<Data> {
    let dir = std::env::var_os("S2_GAME_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("games/super2sp/extracted/app")))?;
    if !dir.join("SUPERSP.PAL").exists() {
        eprintln!("skipping: no game data at {}", dir.display());
        return None;
    }
    Some(Data::open(&dir).expect("open game data"))
}

fn stages(data: &Data) -> Vec<Stage> {
    Stage::parse_all(&data.arcs.read(Kind::Bin, BIN_STAGES).unwrap()).unwrap()
}

fn dialogues(game: &Game) -> Vec<(usize, u32)> {
    game.events
        .log
        .iter()
        .filter_map(|l| match l {
            Log::Dialogue { event, mode } => Some((*event, *mode)),
            _ => None,
        })
        .collect()
}

fn flags_set(game: &Game) -> Vec<u8> {
    game.events.log.iter().filter_map(|l| if let Log::FlagSet(f) = l { Some(*f) } else { None }).collect()
}

fn unit(slot: u16) -> UnitRef {
    UnitRef { side: Side::Enemy, slot }
}

fn turn(game: &mut Game, t: u16) {
    game.campaign.turn = t;
    poll(game, Trigger::TurnStart);
}

/// Stage 1 (第一神劍): order of dialogue and flags must follow the listing
/// in `s2text stages`.
#[test]
fn stage1_sequence() {
    let Some(data) = data() else { return };
    let all = stages(&data);
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
    attach_stage(&mut game, &all[0], true);
    // Header seeds flags 1 and 2 (bytes 01 01 00).
    assert_eq!(&game.campaign.flags[1..4], &[1, 1, 0]);

    turn(&mut game, 1);
    assert_eq!(dialogues(&game), vec![(0, 0)]);
    assert_eq!(flags_set(&game), vec![13]);
    turn(&mut game, 2);
    turn(&mut game, 3);
    assert_eq!(dialogues(&game).len(), 1);
    turn(&mut game, 4);
    turn(&mut game, 5);
    assert_eq!(dialogues(&game), vec![(0, 0), (1, 0), (2, 0)]);

    // 飛賊 (char 90) goes down: last words (mode 1) set flag 14, and the
    // flag chain then fires event 4 (flags 13 and 14 both set).
    let r = poll(&mut game, Trigger::UnitDown { unit: unit(19), char_id: 90 });
    assert_eq!(r.code, 0x12);
    assert_eq!(&dialogues(&game)[3..], &[(3, 1), (4, 0)]);

    // 盜賊頭子 (64) attacks 刑天 (16).
    poll(&mut game, Trigger::BattleTalk { attacker: unit(7), attacker_char: 64, defender_char: 16 });
    assert_eq!(dialogues(&game)[5], (7, 0));
    assert_eq!(game.events.events[7].kind, 0);
    // The reverse pairing has no event.
    poll(&mut game, Trigger::BattleTalk { attacker: unit(1), attacker_char: 16, defender_char: 64 });
    assert_eq!(dialogues(&game).len(), 6);

    // 盜賊頭子 goes down: flag 15, then event 6 (no lines) clears flag 13.
    assert_eq!(game.campaign.flags[13], 1);
    let r = poll(&mut game, Trigger::UnitDown { unit: unit(7), char_id: 64 });
    assert_eq!(r.code, 0x12);
    assert_eq!(dialogues(&game)[6], (5, 1));
    assert_eq!(game.campaign.flags[13], 0);
    assert_eq!(game.events.events[6].kind, 0);
    assert_eq!(flags_set(&game), vec![13, 14, 15]);

    // Unit-down events are never consumed.
    assert_eq!(game.events.events[3].kind, 3);
    assert_eq!(game.events.events[5].kind, 3);

    // No battle yet, so no win; a win through set 1 shows event 8.
    assert_eq!(check_stage(&mut game), StageCheck::Continue);
    game.events.win_set = 1;
    poll(&mut game, Trigger::Victory);
    assert_eq!(dialogues(&game).last(), Some(&(8, 0)));

    // The rest of the stage's events are consumed, except the unit-down ones.
    let left: Vec<u8> = game.events.events.iter().map(|e| e.kind).collect();
    assert_eq!(left, vec![0, 0, 0, 3, 0, 3, 0, 0, 0]);

    clear_stage_flags(&mut game);
    assert!(game.campaign.flags[1..=40].iter().all(|&f| f == 0));
}

/// Stage 10 (罕泉哨所): choice, chained flags, a campaign flag, an
/// animated deploy, fame/rating and a script-forced victory.
#[test]
fn stage10_choice_paths() {
    let Some(data) = data() else { return };
    let all = stages(&data);
    let mut sys = Sys::new(Shared::new(), Audio::silent());

    // Option 2 (refuse): event 3 fires and sets flag 1, which reveals the exit.
    {
        let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
        game.events.ui.choice = |_, _| 2;
        attach_stage(&mut game, &all[9], true);
        turn(&mut game, 1);
        assert!(dialogues(&game).is_empty());
        turn(&mut game, 2);
        assert_eq!(dialogues(&game), vec![(0, 0), (3, 0)]);
        assert!(game.events.log.contains(&Log::Choice { event: 1, choice: 2 }));
        assert!(game.events.log.contains(&Log::FlagEffect(1)));
        assert_eq!(game.campaign.flags[13], 0, "choice result clears flag a");
        assert_eq!(game.campaign.flags[FLAG_CHOICE], 0);
    }

    // Option 1 with campaign flag 44 set: 14 -> 16 -> fame/rating, deploy
    // char 50 with 7 lines, flag 4 -> forced victory.
    let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
    game.events.ui.choice = |_, _| 1;
    game.campaign.flags[44] = 1;
    attach_stage(&mut game, &all[9], true);
    turn(&mut game, 2);
    assert_eq!(dialogues(&game), vec![(0, 0), (2, 0), (8, 0)]);
    assert_eq!(flags_set(&game), vec![13, 14, 16, 4]);
    assert_eq!((game.campaign.fame, game.campaign.rating), (5, 5));
    assert_eq!(game.campaign.flags[14], 0, "event 5 clears flag 14");
    let created = game.events.log.iter().filter(|l| matches!(l, Log::Battle("create_unit", _))).count();
    assert_eq!(created, game.events.stage.as_ref().unwrap().units.iter().filter(|u| u.char_id == 50).count());
    assert!(created > 0, "stage 10 has a record for char 50");
    // Flag 4 forces the win with set 0, so only "any set" victory events run.
    assert_eq!(check_stage(&mut game), StageCheck::Victory);
    assert_eq!(game.events.win_set, 0);
    let n = dialogues(&game).len();
    poll(&mut game, Trigger::Victory);
    assert_eq!(dialogues(&game).len(), n, "event 12 needs set 2");
}

/// Stage 4: victory sets flag 13, which the same poll turns into +5 fame / rating.
#[test]
fn stage4_victory_rewards() {
    let Some(data) = data() else { return };
    let all = stages(&data);
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
    attach_stage(&mut game, &all[3], true);
    game.events.win_set = 1;
    poll(&mut game, Trigger::Victory);
    assert_eq!(dialogues(&game), vec![(1, 0)]);
    assert_eq!((game.campaign.fame, game.campaign.rating), (5, 5));
}

#[test]
fn defeat_by_turn_limit_and_flags() {
    let Some(data) = data() else { return };
    let all = stages(&data);
    let st = all.iter().find(|s| s.lose_codes.iter().any(|&c| (101..=190).contains(&c))).expect("a stage with a turn limit");
    let limit = st.lose_codes.iter().copied().find(|c| (101..=190).contains(c)).unwrap() - 100;
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
    attach_stage(&mut game, st, true);
    game.campaign.turn = (limit - 1) as u16;
    assert!(!check_defeat(&mut game));
    game.campaign.turn = limit as u16;
    assert!(check_defeat(&mut game));
    game.campaign.turn = 1;
    game.campaign.flags[FLAG_FORCE_LOSE] = 1;
    assert_eq!(check_stage(&mut game), StageCheck::Defeat);
    game.campaign.flags[FLAG_FORCE_LOSE] = 0;
    game.campaign.turn = 100;
    assert!(check_defeat(&mut game));
}

#[test]
fn save_and_restore_event_state() {
    let Some(data) = data() else { return };
    let all = stages(&data);
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
    attach_stage(&mut game, &all[0], true);
    turn(&mut game, 1);
    let saved = save_state(&game);
    assert_eq!(saved.len(), 2 + 2 * game.events.events.len());
    attach_stage(&mut game, &all[0], false);
    assert_eq!(game.events.events[0].kind, 1);
    let mut r = saved.as_slice();
    assert!(load_state(&mut game, &mut r));
    assert!(r.is_empty());
    assert_eq!(game.events.events[0].kind, 0);
    // A table of a different size is accepted and only the count is read.
    attach_stage(&mut game, &all[1], false);
    let mut r = saved.as_slice();
    assert!(load_state(&mut game, &mut r));
    assert_eq!(r.len(), saved.len() - 2);
}

/// Every stage: run 30 turns of polls, both victory sets and the
/// end-of-stage checks without panicking; reward layouts match the
/// assumptions of `copy_treasure`.
#[test]
fn all_stages_run() {
    let Some(data) = data() else { return };
    let bin1 = data.arcs.read(Kind::Bin, BIN_STAGES).unwrap();
    let all = stages(&data);
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut total = 0;
    for st in &all {
        // Raw reward groups: gold always has count 1, treasure counts are multiples of 4.
        let blk = &bin1[st.header.offset as usize..];
        for g in 0..st.header.reward_groups as usize {
            let kind = i16::from_le_bytes([blk[4 * g], blk[4 * g + 1]]);
            let n = i16::from_le_bytes([blk[4 * g + 2], blk[4 * g + 3]]);
            if kind == 3 {
                assert_eq!(n, 1, "stage {} gold group", st.id);
            }
            if kind == 5 {
                assert_eq!(n % 4, 0, "stage {} treasure group", st.id);
            }
        }
        let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
        game.events.ui.choice = |_, _| 1;
        attach_stage(&mut game, st, true);
        for t in 1..=30 {
            turn(&mut game, t);
            poll(&mut game, Trigger::Actions);
            check_stage(&mut game);
            poll(&mut game, Trigger::EnemyPhaseEnd);
        }
        for set in [1, 2] {
            game.events.win_set = set;
            poll(&mut game, Trigger::Victory);
        }
        after_victory_media(&mut game);
        // Turn-start events need no state, victory events only the set number:
        // every one with lines must have been shown.
        let shown: std::collections::HashSet<usize> = dialogues(&game).iter().map(|d| d.0).collect();
        for (i, e) in st.events.iter().enumerate() {
            let must = !e.lines.is_empty()
                && ((e.kind == 1 && e.sub == 0 && (1..=30).contains(&e.a)) || (e.kind == 2 && (0..=2).contains(&e.sub)));
            if must {
                assert!(shown.contains(&i), "stage {} event {i} ({}) did not fire", st.id, s2data::script::describe_event(e));
            }
        }
        total += dialogues(&game).len();
        clear_stage_flags(&mut game);
    }
    assert!(total > 300, "only {total} dialogues fired");
}

#[test]
fn touched_tile_list() {
    let Some(data) = data() else { return };
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &data);
    game.events.ui = ui::UiHooks::headless();
    assert!(touched_is_empty(&game));
    assert!(touched_add(&mut game, 0x1005));
    assert!(touched_contains(&game, 5));
    assert!(touched_add(&mut game, 5), "already listed");
    assert_eq!(game.events.touched.len(), 1);
    for i in 0..60 {
        touched_add(&mut game, 100 + i);
    }
    assert_eq!(game.events.touched.len(), TOUCHED_MAX);
    assert!(!touched_add(&mut game, 999));
    touched_clear(&mut game);
    assert!(touched_is_empty(&game));
}
