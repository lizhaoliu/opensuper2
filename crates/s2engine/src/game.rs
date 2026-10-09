//! The global game context passed to every ported routine.
//!
//! Ownership of sub-state (who edits which module):
//! - `res`, `cfg`, `scr`, `cursor`, `text`, `ev`, `ui`, `stage`, `flow::*` — FLOW/UI port
//! - `events` (`event/`)                                                   — EVENT port (stage scripts)
//! - `battle` (`battle::*`)                                                — BATTLE port
//! - `campaign` is shared: add fields with a comment naming the original global.

use crate::cursor::Cursor;
use crate::gfx::Screen;
use crate::res::Res;
use crate::sys::{Event, Sys};
use crate::text::TextState;
use crate::Data;

/// Campaign-wide globals of the original (see docs/flow.md "Important globals").
#[derive(Debug, Clone)]
pub struct Campaign {
    /// `DAT_00494668`: two-player VS mode.
    pub vs_mode: bool,
    /// `DAT_0049466c`: between stages (world map / town).
    pub between_stages: bool,
    /// `DAT_00494670`: current stage id.
    pub stage: u16,
    /// `DAT_00494674`: turn number (1-based).
    pub turn: u16,
    /// `DAT_00494678`: next stage id (0 = campaign over).
    pub next_stage: u16,
    /// `DAT_0049467c`: alternate / hidden next stage.
    pub alt_next_stage: u16,
    /// `DAT_00494680`: current world-map location.
    pub location: u16,
    /// `DAT_00494682`: a cave was discovered this stage.
    pub cave_found: bool,
    /// `DAT_00494684`: difficulty 1..=5 (default 3).
    pub difficulty: u8,
    /// `DAT_00497b54`: protagonist route.
    pub route: u8,
    /// `DAT_00497b50`: battles fought.
    pub battles_fought: u16,
    /// `DAT_00497b58`: current disc (0 = disc 1).
    pub disc: u8,
    /// `DAT_00494508..`: story flags 1..=80 (index 0 unused). 1–40 are
    /// cleared at the end of every stage, 41–80 persist.
    pub flags: [u8; 81],
    /// `DAT_00494653`: music cue playing (restored when a save resumes a battle).
    pub music: u16,
    /// `DAT_00497b48` 名聲 (fame), changed by event type 11/11; saved in the 0x1c-byte block.
    pub fame: i32,
    /// `DAT_00497b4c` 評價 (rating), event type 11/12.
    pub rating: i32,
    /// `DAT_00494683`: donated at the church this visit (`FUN_00413c01`; the march clears it).
    pub donated: u8,
    /// `DAT_00497b5c..63`: the rest of the 0x1c-byte block, kept as saved.
    pub block_d_tail: [u8; 8],
}

impl Default for Campaign {
    fn default() -> Self {
        Campaign {
            vs_mode: false,
            between_stages: false,
            stage: 0,
            turn: 0,
            next_stage: 0,
            alt_next_stage: 0,
            location: 0,
            cave_found: false,
            difficulty: 3,
            route: 0,
            battles_fought: 0,
            disc: 0,
            flags: [0; 81],
            music: 0,
            fame: 0,
            rating: 0,
            donated: 0,
            block_d_tail: [0; 8],
        }
    }
}

/// SUPERSP.CFG settings (`FUN_00423b00`; docs/flow.md "Config").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `DAT_00494638` 畫面速度: map scroll step delay in cs.
    pub screen_speed: i32,
    /// `DAT_0049463c` 訊息速度: text delay in cs per character.
    pub text_speed: i32,
    /// `DAT_00494640` / `44` / `48`: MIDI, CD and SFX volume 0..=100.
    pub midi_volume: i32,
    pub cd_volume: i32,
    pub sfx_volume: i32,
    /// `DAT_0049464c` 確定詢問: ask confirmations.
    pub confirm: bool,
    /// `DAT_0049464d` 狀況視窗.
    pub status_window: bool,
    /// `DAT_0049464e` 快速切換: jump instead of scrolling.
    pub quick_switch: bool,
    /// `DAT_0049464f` 自動切換.
    pub auto_switch: bool,
    /// `DAT_00494650` CD音軌: CD-audio music.
    pub cd_audio: bool,
    /// `DAT_00494651` 動畫播放: battle animations.
    pub battle_anim: bool,
    /// `DAT_00494652` 遮掩網點: mesh shading.
    pub mesh: bool,
    /// `DAT_004944ec`: movies (CDDA= second field).
    pub movies: bool,
    /// `DAT_004944e8`: CD mode (DEVICE drive letter is not '0').
    pub cd_mode: bool,
    /// `DAT_004944f4` (DEVICE= second number): mainland wording / alternate fonts.
    pub mainland: bool,
    /// `DAT_004944f0` (DEVICE= first number): SUPERSP.PRG installed next to the exe.
    pub prg_local: bool,
    /// DEVICE= third field: the CD drive letter (the repack uses ".").
    pub drive: String,
}

impl Default for Config {
    /// Defaults set before the file is read (`FUN_00423b00`).
    fn default() -> Self {
        Config {
            screen_speed: 0,
            text_speed: 10,
            midi_volume: 100,
            cd_volume: 100,
            sfx_volume: 100,
            confirm: true,
            status_window: false,
            quick_switch: false,
            auto_switch: true,
            cd_audio: true,
            battle_anim: true,
            mesh: true,
            movies: true,
            cd_mode: true,
            mainland: false,
            prg_local: false,
            drive: "D".into(),
        }
    }
}

/// Small UI globals that outlive one screen.
pub struct UiState {
    /// `DAT_00494870`: save slot descriptions (53 x 50 bytes; empty = unused).
    pub save_desc: Vec<Vec<u8>>,
    /// `DAT_00497594`: save screen page 0..=4.
    pub save_page: i32,
    /// `DAT_0048e294` / `DAT_0048e298`: dialogue box origin.
    pub box_x: i32,
    pub box_y: i32,
    /// `DAT_00494790`: name of the current world-map place (`FUN_00422e26`).
    pub place_name: Vec<u8>,
    /// Event-table state read from a save, applied once the stage is attached.
    pub pending_script_state: Option<Vec<u8>>,
    /// World-map and town pictures (`FUN_0040fd3c`), while between stages.
    pub world: Option<Box<crate::flow::town::World>>,
    /// Unit sprite banks (normal, summon) for screens shown when no stage
    /// view is set up (the class change cards, the church panel).
    pub unit_sprites: Option<(std::rc::Rc<s2data::gfx_unit::UnitSprites>, std::rc::Rc<s2data::gfx_unit::UnitSprites>)>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            save_desc: vec![Vec::new(); s2data::save::SLOTS],
            save_page: 0,
            box_x: 6,
            box_y: 0x7d,
            place_name: Vec::new(),
            pending_script_state: None,
            world: None,
            unit_sprites: None,
        }
    }
}

/// Which AI the enemy army plays with (`ai=` in opensuper2.ini).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AiMode {
    /// The original AI, frame-identical with the original program.
    #[default]
    Original,
    /// `ai=enhanced` (not the original): the original AI's decisions, with
    /// varied spell / technique / attack picks among near-best candidates,
    /// spells always weighed from every casting cell, and buffs on its own
    /// side (`ai::fight`, `targeting::score_support`).
    Enhanced,
    /// `ai=tactical` (not the original): the threat-map tactical AI of
    /// `battle::ai_plus`.
    Tactical,
}

impl AiMode {
    pub fn parse(s: &str) -> Option<AiMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "original" => Some(AiMode::Original),
            "enhanced" => Some(AiMode::Enhanced),
            "tactical" => Some(AiMode::Tactical),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AiMode::Original => "original",
            AiMode::Enhanced => "enhanced",
            AiMode::Tactical => "tactical",
        }
    }
}

/// Runtime options outside the original (set by the frontend, e.g. from
/// opensuper2.ini; never saved).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub ai: AiMode,
    /// `enemy_boost=N` (not the original): N % more enemy HP/MP/ST and
    /// attack/defence/剣術/戰技, added to the original's per-stage army bonus
    /// (`turn::adapt_difficulty`); never in two-player battles. 0 = off.
    pub enemy_boost: u8,
}

pub struct Game<'a> {
    pub sys: &'a mut Sys,
    pub data: &'a Data,
    pub campaign: Campaign,
    pub events: crate::event::EventState,
    pub battle: crate::battle::Battle,
    /// Resources loaded by `flow::init` (`FUN_00424450`); see [`Game::res`].
    pub res: Option<Res>,
    pub cfg: Config,
    /// The screen object `DAT_004947c8` and the palette state.
    pub scr: Screen,
    pub cursor: Cursor,
    /// The text renderer `DAT_00492d38`.
    pub text: TextState,
    /// `DAT_004952d8`: the last event taken from the queue.
    pub ev: Event,
    pub ui: UiState,
    /// The stage being played, until the BATTLE port owns the map and armies.
    pub stage: Option<crate::flow::stage::StageState>,
    /// Options outside the original ([`Options`]).
    pub options: Options,
}

impl<'a> Game<'a> {
    pub fn new(sys: &'a mut Sys, data: &'a Data) -> Game<'a> {
        Game {
            sys,
            data,
            campaign: Campaign::default(),
            events: Default::default(),
            battle: Default::default(),
            res: None,
            cfg: Config::default(),
            scr: Screen::new(),
            cursor: Cursor::default(),
            text: TextState::default(),
            ev: Event::default(),
            ui: UiState::default(),
            stage: None,
            options: Options::default(),
        }
    }

    /// The resources loaded at start-up. Panics if `flow::init` has not run.
    pub fn res(&self) -> &Res {
        self.res.as_ref().expect("resources not loaded (flow::init)")
    }

    /// `FUN_00472f69(&DAT_004952d8, mask)`: take an event into `self.ev`.
    pub fn get_event(&mut self, mask: u32) -> bool {
        match self.sys.input_get(mask) {
            Some(e) => {
                self.ev = e;
                true
            }
            None => false,
        }
    }

    /// `FUN_00472fb2(&DAT_004952d8, mask)`.
    pub fn wait_event(&mut self, mask: u32) {
        self.ev = self.sys.input_wait(mask);
    }

    /// `FUN_00472f9e(mask)`.
    pub fn flush_events(&mut self, mask: u32) {
        self.sys.input_flush(mask);
    }

    /// UI string `DAT_00494768[i]`.
    pub fn msg(&self, i: usize) -> Vec<u8> {
        self.res().ui.get(i).cloned().unwrap_or_default()
    }

    /// `FUN_0046d5d6`.
    pub fn ticks(&self) -> i32 {
        self.sys.ticks_cs()
    }

    /// `FUN_0046d688(t0, n)`: still within `n` cs of `t0`.
    pub fn still_waiting(&self, t0: i32, n: i32) -> bool {
        (t0 - self.ticks()).abs() < n
    }

    /// `FUN_0046d626(t0, n)`: spin while `|t0 - now| < n` (cs). So it waits
    /// until `n` cs after `t0`, but returns at once when `n <= 0` or when
    /// `t0` is already `n` or more ahead of the clock (e.g. a unit timer
    /// restored from a save; waiting for `t0 + n` there froze the game).
    pub fn wait_until(&mut self, t0: i32, n: i32) {
        if n <= 0 || (t0 - self.ticks()).abs() >= n {
            return;
        }
        let target = (t0 + n).max(0) as u32 * 10;
        let now = self.sys.ticks();
        if target > now {
            self.sys.wait_since(now, target - now);
        }
    }

    /// `FUN_0046d652(n)`: wait until the centisecond clock has advanced `n`
    /// (the original polls the clock, so the wait ends on a centisecond
    /// boundary, not `n * 10` ms from now).
    pub fn sleep_cs(&mut self, n: i32) {
        if n > 0 {
            let t0 = self.ticks();
            self.wait_until(t0, n);
        }
    }
}
