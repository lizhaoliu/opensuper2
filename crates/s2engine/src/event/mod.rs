//! Stage event scripts: port of the stage script runner on `DAT_00492bd0`
//! and its condition handlers. Owned by the EVENT port.
//!
//! The original keeps a pointer to the stage's event table (20-byte records,
//! see `docs/formats-text.md`) and polls it from fixed places in the stage
//! loop. A record fires when its condition holds: its dialogue is shown,
//! its type byte is zeroed (one-shot), and its story flag is set. Control
//! flow below follows the original function by function, including the
//! re-entrant polling (`events_poll` <-> `events_flag_chain`) and the odd
//! corners (e.g. unit-down events are never consumed).
//!
//! # EVENT API
//!
//! Stable entry points for FLOW (stage loop) and BATTLE:
//!
//! ```text
//! // stage setup / teardown
//! pub fn attach_stage(game: &mut Game, stage: &Stage, fresh: bool)   // FUN_00422b8e part + FUN_00417556 (+ FUN_00422a97 when fresh)
//! pub fn detach(game: &mut Game)                                      // FUN_0041734c
//! pub fn clear_stage_flags(game: &mut Game)                           // FUN_00423aea: flags 1..=40, at stage end
//! pub fn clear_all_flags(game: &mut Game)                             // FUN_00423ad4: flags 1..=80, new game / reset
//! pub fn save_state(game: &Game) -> Vec<u8>                           // FUN_0041745a payload
//! pub fn load_state(game: &mut Game, data: &mut &[u8]) -> bool        // FUN_00417369
//!
//! // polling
//! pub fn poll(game: &mut Game, trigger: Trigger) -> Outcome
//!     Trigger::TurnStart                                   FUN_00417a1d  start of a turn (campaign.turn)
//!     Trigger::Actions                                     FUN_004178fa  after actions: types 4..=16
//!     Trigger::Victory                                     FUN_00417b74  after a win (uses win_set)
//!     Trigger::EnemyPhaseEnd                               FUN_00417cbd  after the enemy phase
//!     Trigger::UnitDown { unit, char_id }                  FUN_00417f26  unit defeated; Outcome.code 0 / 0x11 / 0x12
//!     Trigger::BattleTalk { attacker, attacker_char, defender_char }  FUN_00417d8e  before a battle
//!     Trigger::FlagChain { unit_context }                  FUN_0041be4c  run type-9 chains until quiet
//!
//! // stage result
//! pub fn check_stage(game: &mut Game) -> StageCheck       // FUN_0043311d (flag chain, then defeat, then victory)
//! pub fn check_defeat(game: &mut Game) -> bool            // FUN_00433161
//! pub fn check_victory(game: &mut Game) -> bool           // FUN_00433257 (sets events.win_set)
//! pub fn after_victory_media(game: &mut Game)             // FUN_0041bc3b: movie (flag 30) + picture (flags 27..29)
//!
//! // map event tiles touched by a move (DAT_00492cbc list, type 9/7)
//! pub fn touched_clear(game: &mut Game)                   // FUN_0041bd04
//! pub fn touched_add(game: &mut Game, id: u16) -> bool    // FUN_0041bd13
//! pub fn touch_cell(game: &mut Game, x: i32, y: i32) -> bool  // FUN_0041bd98
//! pub fn touched_contains(game: &Game, id: i16) -> bool   // FUN_0041be02
//! pub fn touched_is_empty(game: &Game) -> bool            // FUN_0041bdeb
//! ```
//!
//! UI goes through [`ui`] (function pointers in `EventState::ui`, defaulting
//! to FLOW's dialogue/choice code); battle-side effects go through
//! [`battle_hooks`].

pub mod battle_hooks;
mod check;
mod handlers;
pub mod ui;

#[cfg(test)]
mod tests;

pub use battle_hooks::{Side, UnitRef};
pub use check::{check_defeat, check_stage, check_victory, StageCheck};

use s2data::script::{Event, Reward, Stage};

use crate::game::Game;

/// Story flags are 1..=80 (`DAT_00494507 + n`).
pub const MAX_FLAG: i32 = 80;
/// Flag whose byte holds the last choice result (`DAT_00494513`).
pub const FLAG_CHOICE: usize = 12;
/// Flag that aborts the current event poll (`DAT_00494512`).
pub const FLAG_ABORT: usize = 11;
/// `DAT_0049450b`: script-forced victory.
pub const FLAG_FORCE_WIN: usize = 4;
/// `DAT_0049450c`: script-forced defeat.
pub const FLAG_FORCE_LOSE: usize = 5;
/// `DAT_00494522..24`: picture shown after a won stage (type 13/4).
pub const FLAG_PICTURE: usize = 27;
/// `DAT_00494525`: movie played after a won stage (type 13/6).
pub const FLAG_MOVIE: usize = 30;
/// Capacity of the touched-event-tile list (`DAT_00492cbc`, 0x28 entries).
pub const TOUCHED_MAX: usize = 0x28;

/// What a poll was triggered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    TurnStart,
    Actions,
    Victory,
    EnemyPhaseEnd,
    UnitDown { unit: UnitRef, char_id: i16 },
    BattleTalk { attacker: UnitRef, attacker_char: i16, defender_char: i16 },
    FlagChain { unit_context: bool },
}

/// Result of a poll. `code` is the original return value where there is
/// one (`FUN_00417f26`: 0x11 / 0x12 when a last-words event ran, else 0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
}

/// What happened, in order, for tests and debugging (cleared on attach).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Log {
    /// `FUN_004188ca(this, mode)` for event index `event`.
    Dialogue { event: usize, mode: u32 },
    Choice { event: usize, choice: u8 },
    FlagSet(u8),
    /// `FUN_0041be7a(flag)` ran.
    FlagEffect(u8),
    Sound(i16),
    Music(i16),
    Picture(i16, i16, i16),
    Movie(i16, i32),
    /// A battle hook was called (name, with its main operands).
    Battle(&'static str, [i32; 4]),
}

/// Runtime state of the script object `DAT_00492bd0` plus the event-side
/// globals the original keeps next to it.
#[derive(Debug, Default)]
pub struct EventState {
    /// `this+0x14 != 0`: a stage script is attached.
    pub attached: bool,
    /// The event table (`this+0x18`); `kind` is zeroed as events fire.
    pub events: Vec<Event>,
    /// `this+0`: index of the event being shown.
    pub current: usize,
    /// `this+0x30/0x34`: army and unit of the last unit-down event.
    pub context_unit: Option<UnitRef>,
    /// The attached stage (deployment records, win/lose lists).
    pub stage: Option<Stage>,
    /// `DAT_00494500`: deployment records before the first enemy-side one.
    pub player_records: usize,
    /// `DAT_004952cc`: which win set was satisfied (0 = forced win).
    pub win_set: i32,
    /// `DAT_00494598`: treasure spots, 4 shorts each; `[0] == 0` once taken.
    pub treasure: [[i16; 4]; 20],
    /// `DAT_0049478c`: number of treasure spots on this map.
    pub treasure_count: i32,
    /// `DAT_00492cbc` / `DAT_00492d28`: map event tiles touched by the current move.
    pub touched: Vec<u16>,
    pub ui: ui::UiHooks,
    pub log: Vec<Log>,
}

/// Port of `FUN_00417556` (attach) plus the event-related parts of
/// `FUN_00422b8e` (seed flags 1..=3 from the header, unit split) and, for a
/// freshly started stage (`FUN_00423033` mode 10), `FUN_00422a97`.
pub fn attach_stage(game: &mut Game, stage: &Stage, fresh: bool) {
    detach(game);
    let ev = &mut game.events;
    ev.log.clear();
    ev.events = stage.events.clone();
    ev.attached = true;
    ev.current = 0;
    ev.context_unit = None;
    // FUN_00422b8e: header bytes +0x12..=0x14 only ever set flags 1..=3.
    for (i, &v) in stage.header.init_flags.iter().enumerate() {
        if v != 0 {
            game.campaign.flags[1 + i] = 1;
        }
    }
    // FUN_00423033: DAT_00494500 = index of the first record whose control
    // byte (+0x31) is 2, 3 or 8.
    game.events.player_records = stage
        .units
        .iter()
        .position(|u| matches!(u.raw.get(0x31), Some(2 | 3 | 8)))
        .unwrap_or(stage.units.len());
    if fresh {
        copy_treasure(game, stage);
    }
    game.events.stage = Some(stage.clone());
}

/// Port of `FUN_00422a97`: copy the treasure spots (reward kind 5) into
/// `DAT_00494598`. The offset into the reward values sums the counts of the
/// preceding groups, as the original does.
fn copy_treasure(game: &mut Game, stage: &Stage) {
    let ev = &mut game.events;
    ev.treasure_count = 0;
    ev.treasure = [[0; 4]; 20];
    for r in &stage.rewards {
        if let Reward::Treasure(spots) = r {
            let n = spots.len() * 4;
            ev.treasure_count = (n / 4) as i32;
            if ev.treasure_count > 0x14 {
                // The original raises error 5 here.
                ev.treasure_count = 0x14;
            }
            for (dst, src) in ev.treasure.iter_mut().zip(spots) {
                *dst = *src;
            }
            return;
        }
    }
}

/// Port of `FUN_0041734c`.
pub fn detach(game: &mut Game) {
    let ev = &mut game.events;
    ev.attached = false;
    ev.events.clear();
    ev.stage = None;
    ev.context_unit = None;
}

/// Port of `FUN_00423aea`: clear story flags 1..=40 (end of every stage).
pub fn clear_stage_flags(game: &mut Game) {
    game.campaign.flags[1..=40].fill(0);
}

/// Port of `FUN_00423ad4`: clear all story flags (new game / reset).
pub fn clear_all_flags(game: &mut Game) {
    game.campaign.flags[1..=80].fill(0);
}

/// Port of `FUN_0041745a`: `i16 count` then each event's type byte as `i16`.
pub fn save_state(game: &Game) -> Vec<u8> {
    let ev = &game.events.events;
    let mut out = (ev.len() as i16).to_le_bytes().to_vec();
    for e in ev {
        out.extend_from_slice(&(e.kind as i8 as i16).to_le_bytes());
    }
    out
}

/// Port of `FUN_00417369`. Reads from `data` (advancing it). A count that
/// is 0 or differs from the attached table is accepted and nothing else is
/// read, as in the original. Returns false on a short read.
pub fn load_state(game: &mut Game, data: &mut &[u8]) -> bool {
    let Some(head) = data.get(..2) else { return false };
    let n = i16::from_le_bytes([head[0], head[1]]);
    *data = &data[2..];
    if n == 0 || n as usize != game.events.events.len() {
        return true;
    }
    let len = n as usize * 2;
    let Some(body) = data.get(..len) else { return false };
    for (i, e) in game.events.events.iter_mut().enumerate() {
        if i16::from_le_bytes([body[2 * i], body[2 * i + 1]]) == 0 {
            e.kind = 0;
        }
    }
    *data = &data[len..];
    true
}

/// Run the original routine for `trigger`.
pub fn poll(game: &mut Game, trigger: Trigger) -> Outcome {
    let code = match trigger {
        Trigger::TurnStart => {
            events_turn_start(game);
            0
        }
        Trigger::Actions => {
            events_poll(game);
            0
        }
        Trigger::Victory => {
            events_victory(game);
            0
        }
        Trigger::EnemyPhaseEnd => {
            events_enemy_wiped(game);
            0
        }
        Trigger::UnitDown { unit, char_id } => events_unit_down(game, unit, char_id),
        Trigger::BattleTalk { attacker, attacker_char, defender_char } => {
            events_battle_talk(game, attacker, attacker_char, defender_char);
            0
        }
        Trigger::FlagChain { unit_context } => flag_chain_all(game, unit_context as u32),
    };
    Outcome { code }
}

fn active(game: &Game) -> bool {
    !game.campaign.vs_mode && game.events.attached
}

fn count(game: &Game) -> usize {
    game.events.events.len()
}

pub(crate) fn set_flag(game: &mut Game, n: i32, v: u8) {
    if (1..=MAX_FLAG).contains(&n) {
        game.campaign.flags[n as usize] = v;
    }
}

fn aborted(game: &Game) -> bool {
    game.campaign.flags[FLAG_ABORT] != 0
}

/// Show event `i`'s lines if it has any: `this[0] = i; FUN_00418408;
/// FUN_004188ca(this, mode); FUN_00418465`.
fn show_if_lines(game: &mut Game, i: usize, mode: u32) {
    if !game.events.events[i].lines.is_empty() {
        game.events.current = i;
        ui::show_dialogue(game, i, mode);
    }
}

/// The common "fired" tail: consume, set the flag, run its side effect.
/// Returns whether a flag in 1..=80 was set.
fn consume_and_flag(game: &mut Game, i: usize) -> bool {
    game.events.events[i].kind = 0;
    let f = game.events.events[i].set_flag;
    if f > 0 && (f as i32) < 0x51 {
        set_flag(game, f as i32, 1);
        game.events.log.push(Log::FlagSet(f as u8));
        handlers::flag_side_effect(game, f as i32);
        true
    } else {
        false
    }
}

/// Port of `FUN_00417a1d`: type-1 events for the current turn.
fn events_turn_start(game: &mut Game) {
    if !active(game) {
        return;
    }
    let mut i = 0;
    while i < count(game) {
        let e = &game.events.events[i];
        if e.kind == 1 && e.sub == 0 && game.campaign.turn as i32 == e.a as i32 {
            show_if_lines(game, i, 0);
            consume_and_flag(game, i);
            events_flag_chain(game, 0);
        }
        events_poll(game);
        i += 1;
    }
}

/// Port of `FUN_00417b74`: type-2 events for the satisfied win set.
fn events_victory(game: &mut Game) {
    if !active(game) {
        return;
    }
    let mut i = 0;
    while i < count(game) {
        let e = &game.events.events[i];
        if e.kind == 2 && (e.sub as i32 == game.events.win_set || e.sub == 0) {
            show_if_lines(game, i, 0);
            consume_and_flag(game, i);
        }
        events_poll(game);
        i += 1;
    }
}

/// Port of `FUN_00417cbd`: the first type-3/1 event once every enemy is
/// down. The event is shown but not consumed and sets no flag.
fn events_enemy_wiped(game: &mut Game) {
    if !active(game) || !battle_hooks::army_all_down(game, Side::Enemy, 0) {
        return;
    }
    let mut i = 0;
    while i < count(game) {
        let e = &game.events.events[i];
        if e.kind == 3 && e.sub == 1 {
            if e.lines.is_empty() {
                return;
            }
            game.events.current = i;
            ui::show_dialogue(game, i, 0);
            return;
        }
        events_poll(game);
        i += 1;
    }
}

/// Port of `FUN_00417d8e`: type-3/3 "attacker `a` meets defender `b`".
fn events_battle_talk(game: &mut Game, attacker: UnitRef, attacker_char: i16, defender_char: i16) {
    if !active(game) {
        return;
    }
    for i in 0..count(game) {
        let e = &game.events.events[i];
        if e.kind == 3 && e.sub == 3 && e.a == attacker_char && e.b == defender_char {
            if e.lines.is_empty() {
                return;
            }
            game.events.context_unit = Some(attacker);
            let saved = battle_hooks::battle_talk_begin(game, attacker);
            game.events.current = i;
            ui::show_dialogue(game, i, 0);
            game.events.events[i].kind = 0;
            battle_hooks::battle_talk_end(game, attacker, saved);
            return;
        }
    }
}

/// Port of `FUN_00417f26`: type-3/2 last words when character `char_id`
/// goes down. The event is not consumed. Returns 0x12 when one ran (0x11
/// for sub 1, unreachable in the shipped code), else 0.
fn events_unit_down(game: &mut Game, unit: UnitRef, char_id: i16) -> i32 {
    if game.campaign.vs_mode {
        return 0;
    }
    game.events.context_unit = Some(unit);
    if !game.events.attached {
        return 0;
    }
    let mut code = 0;
    let mut i = 0;
    while i < count(game) {
        let e = &game.events.events[i];
        if e.kind != 3 || e.sub != 2 || e.a != char_id {
            i += 1;
            continue;
        }
        if e.lines.is_empty() {
            break;
        }
        let f = e.set_flag;
        if f > 0 && (f as i32) <= MAX_FLAG {
            set_flag(game, f as i32, 1);
            game.events.log.push(Log::FlagSet(f as u8));
        }
        // The original then switches on the same field for 1/2/3/4; only 2
        // can get here, which sets 0x12 and shows the lines in mode 1.
        code = 0x12;
        game.events.current = i;
        battle_hooks::unit_dialogue_begin(game, Some(unit));
        ui::show_dialogue(game, i, 1);
        battle_hooks::unit_dialogue_end(game);
        break;
    }
    battle_hooks::unit_dialogue_finish(game, unit);
    let r = flag_chain_all(game, 1);
    game.campaign.flags[FLAG_ABORT] = 0;
    if r != 0 {
        code = r;
    }
    code
}

/// Port of `FUN_00418203`: type-9/0 events whose flag is set. With
/// `mode == 1` the dialogue is drawn over the unit-down context. Returns 1
/// if a fired event set a flag.
fn events_flag_chain(game: &mut Game, mode: u32) -> i32 {
    if game.campaign.vs_mode || !game.events.attached {
        return 0;
    }
    let mut result = 0;
    let mut i = 0;
    while i < count(game) && !aborted(game) {
        let e = &game.events.events[i];
        if e.kind == 9 && e.sub == 0 && (1..0x51).contains(&(e.a as i32)) && game.campaign.flags[e.a as usize] != 0 {
            if !e.lines.is_empty() {
                game.events.current = i;
                if mode == 1 {
                    let unit = game.events.context_unit;
                    battle_hooks::unit_dialogue_begin(game, unit);
                }
                ui::show_dialogue(game, i, mode);
                if mode == 1 {
                    battle_hooks::unit_dialogue_end(game);
                }
            }
            if consume_and_flag(game, i) {
                result = 1;
            }
        }
        events_poll(game);
        i += 1;
    }
    result
}

/// Port of `FUN_0041be4c`: repeat the flag chain until nothing sets a flag.
/// Always returns 0.
fn flag_chain_all(game: &mut Game, mode: u32) -> i32 {
    while events_flag_chain(game, mode) != 0 {}
    0
}

/// Port of `FUN_004178fa`: evaluate types 4..=16 and fire what holds.
fn events_poll(game: &mut Game) {
    if !active(game) {
        return;
    }
    let mut i = 0;
    while i < count(game) && !aborted(game) {
        if handlers::event_check(game, i) {
            show_if_lines(game, i, 0);
            consume_and_flag(game, i);
            events_flag_chain(game, 0);
        }
        i += 1;
    }
}

/// Port of `FUN_0041bc3b` (after a won stage): play the movie in flag 30,
/// then the picture in flags 27..=29 if all three are set.
pub fn after_victory_media(game: &mut Game) {
    let f = game.campaign.flags;
    if f[FLAG_MOVIE] != 0 {
        ui::play_movie(game, f[FLAG_MOVIE] as i16, 1);
    }
    if f[FLAG_PICTURE] != 0 && f[FLAG_PICTURE + 1] != 0 && f[FLAG_PICTURE + 2] != 0 {
        ui::show_picture(game, f[FLAG_PICTURE] as i16, f[FLAG_PICTURE + 1] as i16, f[FLAG_PICTURE + 2] as i16);
    }
}

/// Port of `FUN_0041bd04`.
pub fn touched_clear(game: &mut Game) {
    game.events.touched.clear();
}

/// Port of `FUN_0041bd13`: remember event tile `id` (low 12 bits) unless it
/// is already listed; false once the list is full.
pub fn touched_add(game: &mut Game, id: u16) -> bool {
    let t = &mut game.events.touched;
    if t.contains(&id) {
        return true;
    }
    if t.len() < TOUCHED_MAX {
        t.push(id & 0xfff);
        true
    } else {
        false
    }
}

/// Port of `FUN_0041bd98`: record the event id of map cell (x, y), if it has one.
pub fn touch_cell(game: &mut Game, x: i32, y: i32) -> bool {
    match battle_hooks::map_cell_event(game, x, y) {
        Some(v) if v & 0xfff != 0 => touched_add(game, v & 0xfff),
        _ => true,
    }
}

/// Port of `FUN_0041be02`.
pub fn touched_contains(game: &Game, id: i16) -> bool {
    game.events.touched.iter().any(|&t| t as i16 == id)
}

/// Port of `FUN_0041bdeb`.
pub fn touched_is_empty(game: &Game) -> bool {
    game.events.touched.is_empty()
}
