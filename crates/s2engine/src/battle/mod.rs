// BATTLE STRUCTS READY
//! Battle system: map, armies, units, turns, combat. Owned by the BATTLE port.
//!
//! A faithful port of the original battle code (docs/battle.md, docs/structs.md,
//! docs/ai.md). Functions carry `/// Port of FUN_xxxxxxxx` and keep the
//! original control flow, constants, integer rounding and RNG call order.
//!
//! Module ownership: `unit`, `army`, `map`, `deploy`, `movement`, `combat`,
//! `hooks` and this file belong to the BATTLE port; `rules.rs` and `rng.rs`
//! to the TABLES port; `view*.rs` to the GRAPHICS port.
//!
//! # BATTLE API
//!
//! ```text
//! // data and stage set-up (Battle methods)
//! load_tables(data) / set_map(Map::load(arcs, id)) / start_stage(&tbl_stage::Stage)   tables, DAT_00497a68, FUN_0040f67f part
//! deploy_turn(turn) -> bool                                     FUN_004231be
//! create_from_record(side, rec, reuse) / create_summon(side, k, x, y, level)   FUN_0043d9e9 / FUN_0043e5dd
//! unit / unit_mut / army / army_mut / unit_at_cell / recalc_army_flags          queries, FUN_00440b16, FUN_0043abc2
//! battle::sync(game)                                            copy campaign difficulty/VS/turn/route/seal flags into the battle
//!
//! // stage loop (FUN_0040d108, flow/stage.rs) — Game-level functions
//! turn::adapt_difficulty(game)                FUN_0040d729 (adaptive difficulty: army power, scale_army)
//! turn::deploy_turn_units(game)               FUN_004233c7
//! turn::turn_upkeep(game) -> 0|1|2            FUN_00433831 (turn >= 2: both armies' upkeep, then the stage check)
//! turn::stage_check(game) -> 0|1|2            FUN_0043311d (via event::check_stage)
//! turn::phase_begin(game, side)               FUN_004411cf
//! turn::phase_banner(game, 0|1)               FUN_00433453
//! turn::wake_ai(game, side)                   FUN_004447e6
//! menu::player_phase(game) -> code            FUN_004323a3: 0 done, 1 win, 2 lose, 7/8 loaded, 0xb quit, 0xf retreat
//! turn::enemy_phase(game) -> 0|1|2            FUN_0043310e (AI pending: every enemy waits)
//! turn::phase_reset(game, side)               FUN_00440e71
//! turn::phase_terrain(game, side) -> 0|1|2    FUN_0045b29d (then the stage check)
//! turn::victory_rewards(game) / victory_exp(game)   FUN_0045cfad / FUN_0045d713
//!
//! // commands and sequences
//! menu::select / command_loop / attack_command / move_command / rest_command / system_menu
//! actions::attack(game, &TargetList, ti, Duel) FUN_00436e46 (+ counters FUN_00437b9c)
//! actions::die(game, r, mode)                  FUN_0044d14b (polls Trigger::UnitDown)
//! cast::magic_command(game, r, technique)      FUN_0045f6cf / FUN_0045f72d (spell menu, aim, cast, cost)
//! cast::use_item_in_battle(game, r, item)      FUN_0044a300 battle branches (called by FLOW's item screen)
//! turn::objectives_screen(game)                FUN_0042d479
//! turn::end_turn(game, side) / rest(game, r)   FUN_00441038 / FUN_0044e460
//! hooks::*                                     event::battle_hooks implementations
//! hooks::BattleEffects + finish_effects        spells::EffectHooks for cast_spell / use_item
//! ```
//!
//! Return codes follow `FUN_0040d108`: 0 continue, 1 victory, 2 defeat,
//! 7/8 load game, 0xb quit to title, 0xf retreat.

pub mod actions;
pub mod army;
pub mod camera;
pub mod cast;
pub mod combat;
pub mod deploy;
pub mod map;
pub mod hooks;
pub mod menu;
pub mod movement;
pub mod rng;
pub mod slots;
pub mod rules;
pub mod spells;
pub mod items;
pub mod classchange;
/// AI port: spell/item targeting and the enemy/NPC AI.
pub mod targeting;
pub mod ai;
pub mod ai_plus;
/// AI port: two-player battles (FUN_0043297f, FUN_0040f67f).
pub mod vs;
pub mod stats;
pub mod turn;
pub mod unit;
/// GRAPHICS port: battle map/unit/effect drawing (see the VIEW API in view.rs).
pub mod view;

use std::rc::Rc;

use anyhow::Result;
use s2data::tables::{ItemDef, RuleTables};
use s2data::tbl_stage::UnitPlacement;

pub use army::{Army, Side, UnitRef};
pub use map::Map;
pub use unit::Unit;

use unit::{flag, status};

/// A stage placement record plus the raw bytes the table parser does not expose.
#[derive(Debug, Clone)]
pub struct Placement {
    pub p: UnitPlacement,
    /// +0x34: last turn of the appear window (0 = only turn `p.b33`).
    pub appear_to: i8,
    /// +0x02..+0x0f: the Big5 name bytes as stored (copied to `Unit+0x10`).
    pub name_raw: [u8; 13],
}

/// Runtime battle state: the armies, the map and the battle globals.
pub struct Battle {
    /// Rule tables (KINDSP, ITEMSP, terrain, spells, growth, constants).
    pub tables: Option<Rc<RuleTables>>,
    /// The original RNG (`DAT_0048fc10`, `DAT_004a2690`, `DAT_004a2af8`).
    pub rng: rng::Rng,
    /// The stage's sprite-type list behind `unit+0xa0` (`slots.rs`, kept for save fidelity).
    pub slots: slots::SpriteSlots,
    /// `DAT_0049f9f8` (player) and `DAT_004976c8` (enemy).
    pub armies: [Army; 2],
    /// `DAT_00497a68`.
    pub map: Option<Map>,
    /// `DAT_004a2b48`: the movement-range scratch object.
    pub range: movement::MoveRange,
    /// `DAT_00494730`: placement records of the current stage.
    pub defs: Vec<Placement>,
    /// `DAT_00494500`: player-side records (prefix of `defs`).
    pub n_player_defs: usize,
    /// `DAT_00494504`: enemy-side records.
    pub n_enemy_defs: usize,
    /// `DAT_00494574`: first enemy-side record.
    pub enemy_defs_start: usize,
    /// `DAT_0049469d`: stage kind (header byte 0x15; 3 = no battle).
    pub stage_kind: u8,
    /// `DAT_0048eaa0` / `DAT_0048eaa4`: map cursor cell.
    pub cursor: (i32, i32),
    /// Mirror of `campaign.difficulty` (`DAT_00494684`), synced by the API functions.
    pub difficulty: u8,
    /// Not the original: `ai=enhanced` is on and this is not a two-player
    /// battle — the enemy army varies its picks and buffs its own side
    /// (`ai::fight`, `targeting::score_support`). Synced by [`sync`].
    pub ai_varied: bool,
    /// Mirror of `campaign.vs_mode` (`DAT_00494668`).
    pub vs: bool,
    /// Mirror of `campaign.turn` (`DAT_00494674`).
    pub turn: i32,
    /// Mirror of `campaign.route` (`DAT_00497b54`).
    pub route: u8,
    /// `DAT_004944e0`: VS mode, side currently acting.
    pub vs_side: i32,
    /// `DAT_00494510` / `DAT_00494511` (story flags 9 / 10, mirrored by
    /// `sync`): the player army may not use magic / techniques.
    pub seal_magic: bool,
    pub seal_tech: bool,
    /// `DAT_0049478c`: number of event points in use.
    pub event_count: i32,
    /// `DAT_004944c0..dc`: deployment power totals shown on the stage intro
    /// (sum, max, count) per side.
    pub power_totals: [(i32, i32, i32); 2],
    /// Level-ups waiting for their window (`actions::show_level_ups`).
    pub level_log: Vec<stats::LevelUp>,
    /// `!DAT_0048e944`: loading a map background failed (`FUN_0042b030` clears
    /// the original flag when its allocation fails); the next stage start
    /// shows msg 0x103 記憶體不足無法顯示背景場景 once and clears this.
    pub bg_failed_notice: bool,
    /// `DAT_00497580/84`: the cursor cell before the last keyboard move.
    pub cursor_prev: (i32, i32),
    /// `DAT_0048ea98` (−1 in .data): pointer mode, see `menu::set_click_mode`.
    pub map_click_mode: i32,
    /// `DAT_004a1f58+0x24`: the spell caster object's proficiency gain, kept
    /// from cast to cast (FUN_0045ff9e reuses it after a cell-by-cell sequence).
    pub spell_prof_gain: i32,
    /// `DAT_004a2580/84`: school row and spell column of the spell menu.
    pub spell_menu_pos: (i32, i32),
    /// `DAT_00492a68+0x3c/+0x40`: the cast confirmation panel (moved with Alt+arrows).
    pub cast_panel: (i32, i32),
    /// GRAPHICS port: decoded sprites, map surface, cursor and range pages.
    pub view: view::BattleView,
}

impl Default for Battle {
    fn default() -> Self {
        Battle {
            tables: None,
            rng: rng::Rng::default(),
            slots: slots::SpriteSlots::default(),
            armies: [Army::new(Side::Player, 0), Army::new(Side::Enemy, 0)],
            map: None,
            range: movement::MoveRange::default(),
            defs: Vec::new(),
            n_player_defs: 0,
            n_enemy_defs: 0,
            enemy_defs_start: 0,
            stage_kind: 1,
            // DAT_0048eaa0 / DAT_0048eaa4 start at -1 (.data).
            cursor: (-1, -1),
            difficulty: 3,
            ai_varied: false,
            vs: false,
            turn: 1,
            route: 0,
            vs_side: 0,
            seal_magic: false,
            seal_tech: false,
            event_count: 0,
            power_totals: [(0, 0, 0); 2],
            bg_failed_notice: false,
            cursor_prev: (0, 0),
            map_click_mode: -1,
            spell_prof_gain: 0,
            spell_menu_pos: (0, 0),
            cast_panel: (0xa6, 300),
            level_log: Vec::new(),
            view: view::BattleView::default(),
        }
    }
}

impl Battle {
    /// `DAT_004946a8 + 2 + (id − 1) * 0x26`: an item name as the font wants it.
    pub fn item_name_raw(&self, id: u16) -> Vec<u8> {
        let t = self.tables.as_ref();
        t.and_then(|t| (id as usize).checked_sub(1).and_then(|i| t.items.get(i))).map(|it| it.name_raw.clone()).unwrap_or_default()
    }

    /// `DAT_00494590 + 1 + k * 0x17`: an action name.
    pub fn action_name_raw(&self, k: usize) -> Vec<u8> {
        self.tables.as_ref().and_then(|t| t.kinds.actions.get(k)).map(|a| a.name_raw.clone()).unwrap_or_default()
    }

    /// `DAT_00494580 + (class − 1) * 0x25`: a class name.
    pub fn class_name_raw(&self, class: u16) -> Vec<u8> {
        self.tables.as_ref().and_then(|t| t.kinds.class(class)).map(|c| c.name_raw.clone()).unwrap_or_default()
    }

    /// Byte +0x0a of KINDSP action `k`: the animation set an attack plays
    /// (`unit.facing = (facing & 3) + set * 4`, FUN_00436e46).
    pub fn action_set(&self, k: usize) -> Option<u8> {
        self.tables.as_ref().and_then(|t| t.kinds.actions.get(k)).map(|a| a.anim_set)
    }
}

/// `S2_BATTLE_TRACE=1`: print battle events (selection, moves, hits,
/// deaths, phases) to stderr, for headless runs.
pub fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("S2_BATTLE_TRACE").is_some())
}

macro_rules! trace {
    ($($t:tt)*) => {
        if $crate::battle::tracing() {
            eprintln!("[battle] {}", format!($($t)*));
        }
    };
}
pub(crate) use trace;

/// Copy the campaign globals the battle code reads (`DAT_00494684`,
/// `DAT_00494668`, `DAT_00494674`, `DAT_00497b54`) into the mirrors.
pub fn sync(game: &mut crate::game::Game) {
    let c = &game.campaign;
    let b = &mut game.battle;
    b.difficulty = c.difficulty;
    b.vs = c.vs_mode;
    b.turn = c.turn as i32;
    b.route = c.route;
    // DAT_00494510 / DAT_00494511 are story flags 9 and 10 (0x494508 + k − 1).
    b.seal_magic = c.flags[9] != 0;
    b.seal_tech = c.flags[10] != 0;
    b.ai_varied = game.options.ai == crate::game::AiMode::Enhanced && !c.vs_mode;
}

impl std::fmt::Debug for Battle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Battle")
            .field("player_units", &self.armies[0].units.len())
            .field("enemy_units", &self.armies[1].units.len())
            .field("turn", &self.turn)
            .finish()
    }
}

impl Battle {
    /// Load the rule tables from the game data (idempotent).
    pub fn load_tables(&mut self, data: &crate::Data) -> Result<()> {
        if self.tables.is_none() {
            let t = RuleTables::load(&data.dir, &data.arcs)?;
            self.set_tables(Rc::new(t));
        }
        if self.slots.n_types == 0 {
            // DAT_0049fdb0: the type count heading PRG 1.
            let prg1 = data.arcs.read(s2data::Kind::Prg, 1)?;
            self.slots.n_types = i16::from_le_bytes([prg1.first().copied().unwrap_or(0), prg1.get(1).copied().unwrap_or(0)]);
        }
        Ok(())
    }

    /// Install already loaded tables (tests, or a shared copy).
    pub fn set_tables(&mut self, t: Rc<RuleTables>) {
        let n = t.items.len();
        for a in &mut self.armies {
            a.inventory.resize(n, 0);
        }
        self.tables = Some(t);
    }

    /// The rule tables. Panics before [`Battle::load_tables`].
    pub fn t(&self) -> Rc<RuleTables> {
        self.tables.clone().expect("battle tables not loaded")
    }

    pub fn set_map(&mut self, m: Map) {
        self.map = Some(m);
    }

    pub fn map(&self) -> &Map {
        self.map.as_ref().expect("no battle map")
    }

    pub fn map_mut(&mut self) -> &mut Map {
        self.map.as_mut().expect("no battle map")
    }

    pub fn army(&self, s: Side) -> &Army {
        &self.armies[s.index()]
    }

    pub fn army_mut(&mut self, s: Side) -> &mut Army {
        &mut self.armies[s.index()]
    }

    pub fn unit(&self, r: UnitRef) -> &Unit {
        &self.armies[r.side.index()].units[r.idx]
    }

    pub fn unit_mut(&mut self, r: UnitRef) -> &mut Unit {
        &mut self.armies[r.side.index()].units[r.idx]
    }

    /// Rules constant `DAT_0049456c[off]` as the signed byte most code reads.
    pub fn c(&self, off: usize) -> i32 {
        self.tables.as_ref().map_or(0, |t| t.params.get(off))
    }

    /// Rules constant read as an unsigned byte.
    pub fn cu(&self, off: usize) -> i32 {
        self.c(off) as u8 as i32
    }

    /// Item by 1-based id.
    pub fn item(&self, id: u16) -> Option<ItemDef> {
        let t = self.tables.as_ref()?;
        (id as usize).checked_sub(1).and_then(|i| t.items.get(i)).cloned()
    }

    /// Port of FUN_0043ab32: an equipped passive item (`usage == 0`) with effect `a`.
    pub fn has_item_ability(&self, u: &Unit, a: i32) -> bool {
        let Some(t) = self.tables.as_ref() else { return false };
        u.equip.iter().any(|&id| {
            id != 0
                && (id as usize)
                    .checked_sub(1)
                    .and_then(|i| t.items.get(i))
                    .is_some_and(|it| it.usage == 0 && it.effect as i32 == a)
        })
    }

    /// Port of FUN_0043aa26: movement type 0 ground, 1 levitate, 2, 3, 4 flying, 5.
    pub fn move_type(&self, u: &Unit) -> i32 {
        let mc = self.tables.as_ref().and_then(|t| t.kinds.class(u.kind as u16)).map_or(0, |c| c.move_type);
        if mc == 7 || u.status & status::LEVITATE != 0 {
            return 1;
        }
        if mc == 8 || self.has_item_ability(u, 0xf) {
            return 2;
        }
        if mc == 6 {
            return 3;
        }
        if u.status & status::FLYING != 0 || self.has_item_ability(u, 0x11) {
            return 4;
        }
        if u.flags & flag::MOVE5 != 0 {
            return 5;
        }
        0
    }

    /// Port of FUN_0043a7ea: enough AP to move at least one cell.
    pub fn can_move(&self, u: &Unit) -> bool {
        let extra = if self.move_type(u) == 0 { self.c(0xec) } else { self.c(0xe9) };
        u.ap as i32 - self.c(0xe8) - extra >= 0
    }

    /// Port of FUN_0043aad7: `u` (of `side`) is invisible to the opposing army.
    pub fn is_hidden(&self, side: Side, u: &Unit) -> bool {
        (u.abilities & 0x80 != 0 || u.status & status::INVISIBLE != 0)
            && !self.enemy_adjacent(side, u)
            && self.army(side.other()).flags & 9 == 0
    }

    /// Port of FUN_0043b9ff: an opposing unit stands on one of the four
    /// neighbours within the height window (−3..=+2 relative).
    pub fn enemy_adjacent(&self, side: Side, u: &Unit) -> bool {
        let Some(m) = self.map.as_ref() else { return false };
        let h = m.height(u.x, u.y);
        // order: up-right, down-left, up-left, down-right
        for dir in [0usize, 1, 2, 3] {
            let (x, y) = map::neighbour(u.x, u.y, dir);
            if self.army(side.other()).unit_at(x, y) > 0 {
                let h2 = m.height(x, y);
                if h2 != 0 && ((h2 <= h && h - h2 < 4) || (h < h2 && h2 - h < 3)) {
                    return true;
                }
            }
        }
        false
    }

    /// Port of FUN_00440b16: the deployed unit of `side` at (x, y). With
    /// `visible_only`, units hidden from the opponent (FUN_0043aad7) are ignored.
    pub fn unit_at_cell(&self, side: Side, x: i32, y: i32, visible_only: bool) -> Option<UnitRef> {
        let id = self.army(side).unit_at(x, y);
        if id == 0 {
            return None;
        }
        let r = UnitRef::new(side, id - 1);
        if visible_only && self.is_hidden(side, self.unit(r)) {
            return None;
        }
        Some(r)
    }

    /// Port of FUN_0043abc2: recompute the aggregated army flags. Returns
    /// how many points of 名聲 (`DAT_00497b48`, `Campaign::fame`) the
    /// player's equipped item ability 1 costs (mode 1, not VS); the caller
    /// takes them while the fame is > 0 (`turn::pay_fame`).
    pub fn recalc_army_flags(&mut self, side: Side, mode: i32) -> i32 {
        let t = self.t();
        let vs = self.vs;
        let mut flags = self.army(side).flags & 0xffff_ff81;
        let mut b48_dec = 0;
        let army_side = self.army(side).side;
        for u in &self.army(side).units {
            if u.flags & (flag::DEAD | flag::SUMMON) != 0 || !(mode == 2 || u.flags & flag::DEPLOYED != 0) {
                continue;
            }
            if u.abilities & 8 != 0 {
                flags |= 2;
            }
            if u.abilities & 0x20000 != 0 {
                flags |= 0x10;
            }
            if u.abilities & 0x400_0000 != 0 {
                flags |= 0xc;
            }
            if u.npc != 1 {
                if u.abilities & 0x40000 != 0 {
                    flags |= 0x20;
                }
                if u.abilities & 0x8000_0000 != 0 {
                    flags |= 0x40;
                }
            }
            for &id in &u.equip {
                let Some(it) = (id as usize).checked_sub(1).and_then(|i| t.items.get(i)) else { continue };
                if it.usage != 0 || it.effect == 0 || (it.effect as i8) <= 0 {
                    continue;
                }
                match it.effect {
                    1 => {
                        if mode == 1 && army_side == Side::Player && !vs {
                            b48_dec += 1;
                        }
                    }
                    0x1f => flags |= 8,
                    0x3a => flags |= 4,
                    _ => {}
                }
            }
        }
        self.army_mut(side).flags = flags;
        b48_dec
    }
}

#[cfg(test)]
mod tests;
