//! Automated campaign QA (`s2auto`): the top-level flow of
//! `flow::win_main` / `FUN_0040d010` with probes around each stage, and a
//! frame-driven input driver that plays the game without a human —
//! confirming menus, advancing dialogue, picking choices, letting the AI
//! play the player side with 自動作戰 (chosen in the system menu →
//! `turn::auto_phase`, the original's auto battle) and taking the shortest
//! way between stages.
//!
//! Not part of the original program. Everything here goes through the same
//! public flow functions and the same input queue a player would use; the
//! only settings changed are 自動切換 off (so the player phase waits on the
//! map) and the text speed.
//!
//! Records go to stderr as `QA ...` lines (one per event) so a parent
//! process can follow a run that later crashes or hangs.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::Result;

use crate::flow::{self, code};
use crate::game::Game;
use crate::sys::{vk, FlipInfo, InputEvent, Shared, SCREEN_H, SCREEN_W};

/// What the game is doing, as far as the QA flow knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    Boot,
    Title,
    /// Inside `stage::stage_loop` (battle or story stage).
    Stage,
    /// Inside `town::between_stages` (world map, town, march, deployment).
    Between,
    /// A dialogue box of the event engine.
    Dialogue,
    /// A choice menu; the option the driver should pick (1-based).
    Choice(u8),
    Ending,
    Done,
}

/// Shared between the game thread's QA flow and the frame driver.
#[derive(Debug)]
pub struct Probe {
    pub ctx: Ctx,
    pub stage: u16,
    pub stage_name: String,
    /// Stage records written so far (also the screenshot index).
    pub index: usize,
    /// A screenshot the driver should take the next time the game waits.
    pub shot: Option<String>,
    /// Save the last waiting screen of the stage under this name.
    pub end_shot: Option<String>,
    /// The driver gave up: no new frame for too long.
    pub hung: bool,
}

static PROBE: Mutex<Probe> =
    Mutex::new(Probe { ctx: Ctx::Boot, stage: 0, stage_name: String::new(), index: 0, shot: None, end_shot: None, hung: false });

/// A screenshot requested from outside (the parent process, through stdin).
static SNAP: Mutex<Option<String>> = Mutex::new(None);

/// Ask the driver for a screenshot of the next frame, saved as OUT/NAME.png.
pub fn request_snapshot(name: &str) {
    *SNAP.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_string());
}

fn probe() -> std::sync::MutexGuard<'static, Probe> {
    PROBE.lock().unwrap_or_else(|e| e.into_inner())
}

fn set_ctx(c: Ctx) {
    probe().ctx = c;
}

/// Options of a QA run.
#[derive(Debug, Clone)]
pub struct Opts {
    /// 0 白志超, 1 娜塔莉雅 (the prologue choice).
    pub route: u8,
    /// Battle RNG seed (also drives random choices).
    pub seed: u32,
    /// Pick random options in choices instead of the first.
    pub random_choices: bool,
    /// Stop after this many stages (0 = no limit).
    pub max_stages: usize,
    /// Start from this save slot (1-based) instead of a new game.
    pub from_save: Option<usize>,
    /// Retries of a lost stage from its turn-1 autosave (with another seed).
    pub retries: usize,
    /// Screenshots go here.
    pub out: PathBuf,
    /// Difficulty 1..=5 to play at (as if chosen on the difficulty screen);
    /// 0 keeps the screen's default (3) or the save's.
    pub difficulty: u8,
    /// Not the original (coverage runs): the player side's units get at
    /// least 3000 HP and 400 攻擊 / 防禦 and full MP/ST at each stage start,
    /// so that 自動作戰 wins and later stages are reached.
    pub god: bool,
    /// Not the original: if the first stage of the run is won by reaching
    /// the exit tile, put the route's protagonist on an exit cell (used to
    /// resume after stalls the original's auto battle also has).
    pub exit_cheat: bool,
    /// Reseed the battle RNG at every stage start from (seed, stage), so a run
    /// resumed from a turn-1 save can be compared with the original run.
    pub reseed: bool,
    /// Number of the first record (runs resumed by s2auto continue the numbering).
    pub index_base: usize,
    /// Not the original: party members waiting in AI mode 0 (act only when
    /// a target is already in reach) get mode 1 (fight, else approach) at
    /// each stage start, so 自動作戰 also finishes stages where the enemy holds.
    pub aggressive: bool,
    /// Not the original (coverage runs): in a stage with a hidden stage
    /// (header +0x10/+0x11), put the route's protagonist on the cave spot
    /// at its start and let it find the cave (FUN_0044f9a1), so the cave
    /// stage follows the victory.
    pub caves: bool,
    /// Not the original (coverage runs): every enemy on the map starts the
    /// stage with 1 HP (reaching the branches that need a boss beaten fast).
    pub weak_enemies: bool,
    /// Not the original (coverage runs): the route's protagonist starts each
    /// stage next to the first enemy leader (on a free diagonal neighbour).
    pub boss_rush: bool,
    /// Not the original (branch coverage): `(stage, flag, value)` story flags
    /// set at that stage's turn 1, e.g. (60, 15, 1) = 克雷布特里 beaten → 61.
    pub set_flags: Vec<(u16, usize, u8)>,
    /// `(stage, option)`: the option (1-based) every choice of that stage
    /// takes (story branches: 29:1 → 33, 31:2 → 35/36, 99:1 → 102).
    pub choices: Vec<(u16, u8)>,
}

/// One stage as played.
#[derive(Debug, Clone)]
pub struct StageRecord {
    pub index: usize,
    pub stage: u16,
    pub name: String,
    pub kind: u8,
    pub route: u8,
    pub turns: u16,
    /// The stage loop's code (1 win, 2 lose, 0xe cave, 0xf retreat, 0x17 campaign done, ...).
    pub code: i32,
    pub ticks_ms: u32,
}

fn emit(line: &str) {
    eprintln!("QA {line}");
}

/// Save the frame on screen (8-bit indexed PNG).
pub fn save_png(path: &Path, pixels: &[u8], palette: &s2data::Palette) -> Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, SCREEN_W as u32, SCREEN_H as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(palette.to_rgb_bytes());
    enc.write_header()?.write_image_data(pixels)?;
    Ok(())
}

// ---- the event engine's UI hooks --------------------------------------------------------

/// The first prologue choice decides the route; it is the only choice of stage `P_PROLOGUE`.
const P_PROLOGUE: usize = 0x100;

static CHOICE: Mutex<(bool, u32, u8)> = Mutex::new((false, 1, 0));
/// `Opts::choices`.
static FORCED: Mutex<Vec<(u16, u8)>> = Mutex::new(Vec::new());

fn qa_dialogue(game: &mut Game, event: usize, mode: u32) {
    let outer = {
        let mut p = probe();
        let o = p.ctx;
        p.ctx = Ctx::Dialogue;
        o
    };
    let lines = game.events.events[event].lines.clone();
    flow::show_dialogue(game, &lines, mode);
    set_ctx(outer);
}

fn qa_choice(game: &mut Game, event: usize) -> u8 {
    let e = &game.events.events[event];
    let (lines, n) = (e.lines.clone(), e.b.max(1) as usize);
    let prologue = game.res.as_ref().map_or(0, |r| r.param(P_PROLOGUE) as u16);
    let want = {
        let mut c = CHOICE.lock().unwrap_or_else(|e| e.into_inner());
        let forced = FORCED.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|f| f.0 == game.campaign.stage).map(|f| f.1);
        if game.campaign.stage == prologue {
            c.2 + 1
        } else if let Some(k) = forced {
            k
        } else if c.0 {
            // xorshift on the seed
            let mut s = c.1;
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            c.1 = s;
            (s % n as u32) as u8 + 1
        } else {
            1
        }
    }
    .clamp(1, n as u8);
    let outer = {
        let mut p = probe();
        let o = p.ctx;
        p.ctx = Ctx::Choice(want);
        o
    };
    let got = flow::choose(game, &lines, n);
    set_ctx(outer);
    emit(&format!("CHOICE stage={} event={event} options={n} want={want} got={got}", game.campaign.stage));
    got
}

// ---- the flow -------------------------------------------------------------------------

/// The settings a QA run plays with: 自動切換 off (the player phase waits on
/// the map for the system menu instead of opening each unit's menu), instant
/// text.
fn qa_config(game: &mut Game) {
    game.cfg.auto_switch = false;
    game.cfg.text_speed = 0;
}

fn stage_name(game: &Game) -> String {
    game.stage.as_ref().map(|s| s2data::text::big5(&s.stage.name)).unwrap_or_default()
}

/// Run the campaign: `flow::win_main` with the QA probes. Returns the stage records.
pub fn run(game: &mut Game, opts: &Opts) -> Result<Vec<StageRecord>> {
    set_ctx(Ctx::Boot);
    {
        let mut c = CHOICE.lock().unwrap_or_else(|e| e.into_inner());
        *c = (opts.random_choices, opts.seed.max(1), opts.route);
        *FORCED.lock().unwrap_or_else(|e| e.into_inner()) = opts.choices.clone();
    }
    flow::init::game_init(game)?;
    qa_config(game);
    if let Ok(i) = game.res().ui_image(crate::ui::img::YES_NO_BAR) {
        let _ = YES_NO.set(i);
    }
    game.events.ui.show_dialogue = qa_dialogue;
    game.events.ui.choice = qa_choice;
    game.battle.rng = crate::battle::rng::Rng::started(crate::battle::rng::Seeder::fixed(opts.seed));
    flow::init::scan_saves(game);
    let master = game.res().master.clone();
    crate::gfx::fade(game, &master, 1, 0);
    flow::movie::play_opening(game);
    let mut c;
    if let Some(slot) = opts.from_save {
        flow::save::load_game(game, slot.saturating_sub(1))?;
        // A save carries the settings block (DAT_00494638..): set ours again.
        qa_config(game);
        if opts.difficulty > 0 {
            game.campaign.difficulty = opts.difficulty;
        }
        c = if slot == flow::save::SLOT_TURN1 + 1 { code::LOADED_SLOT50 } else { code::LOADED };
        if game.campaign.between_stages {
            set_ctx(Ctx::Between);
            c = flow::town::between_stages(game)?;
        }
    } else {
        set_ctx(Ctx::Title);
        c = flow::title::title_menu(game)?;
        if opts.difficulty > 0 {
            game.campaign.difficulty = opts.difficulty;
        }
        emit(&format!("ROUTE {} difficulty {}", game.campaign.route, game.campaign.difficulty));
        if c != code::NEW_STAGE && c != code::QUIT && game.campaign.between_stages {
            set_ctx(Ctx::Between);
            c = flow::town::between_stages(game)?;
        }
    }
    let mut records = Vec::new();
    let mut retries = 0;
    let mut inv = Invariants::default();
    // AI modes the helpers change, restored when the stage ends (and after a
    // retry reloads the turn-1 autosave, which has them) so that the next
    // stage sees the party as the game left it.
    let mut changed_modes: Vec<(i16, i16, i16, i16)> = Vec::new();
    loop {
        let index = opts.index_base + records.len();
        let (stage, name) = (game.campaign.stage, stage_name(game));
        let kind = game.stage.as_ref().map_or(0, |s| s.stage.header.kind);
        {
            let mut p = probe();
            p.ctx = Ctx::Stage;
            p.stage = stage;
            p.stage_name = name.clone();
            p.index = index;
            p.shot = Some(format!("{index:03}_{stage:03}_start"));
        }
        let fresh = c == code::NEW_STAGE || c == code::FROM_WORLD_MAP;
        if fresh {
            check_invariants(game, &mut inv, index, stage);
        }
        if opts.reseed {
            let s = opts.seed ^ (stage as u32).wrapping_mul(0x9e37_79b9) ^ (retries as u32).wrapping_mul(0x85eb_ca6b);
            game.battle.rng = crate::battle::rng::Rng::started(crate::battle::rng::Seeder::fixed(s));
        }
        let mut modes = std::collections::BTreeMap::new();
        changed_modes.clear();
        for u in &mut game.battle.army_mut(crate::battle::Side::Player).units {
            if u.flags & crate::battle::unit::flag::DEAD != 0 {
                continue;
            }
            *modes.entry(u.ai_mode).or_insert(0) += 1;
            if opts.aggressive && u.npc != 1 && u.ai_mode == 0 {
                changed_modes.push((u.slot, u.char_id, u.kind, u.ai_mode));
                u.ai_mode = 1;
            }
            if opts.god {
                // Hidden stages (kind 2) hit for ~1000 a blow: more there.
                let (hp, ad) = if kind == 2 { (9999, 999) } else { (3000, 400) };
                u.hp_max = u.hp_max.max(hp);
                u.hp = u.hp_max;
                u.mp = u.mp_max;
                u.st = u.st_max;
                u.atk = u.atk.max(ad);
                u.def = u.def.max(ad);
                u.recompute_buffs();
            }
        }
        emit(&format!("STAGE-START index={index} stage={stage} kind={kind} party_modes={modes:?} name={name}"));
        // A stage won by reaching the exit tile (win codes 0x1f / 0x20): the
        // route's protagonist heads for it with the AI's own go-to-point mode 6.
        let exit_goal = game.stage.as_ref().is_some_and(|s| s.stage.win_sets.iter().flatten().any(|&w| w == 0x1f || w == 0x20));
        // Stages won by a script once a player unit reaches the 紅旗 point
        // (object class 4, story flag 8 via FUN_0044fae0): the same helper,
        // as mode 6 heads for class 4 first.
        let point_goal = game.stage.as_ref().is_some_and(|s| {
            let sets: Vec<i16> = s.stage.win_sets.iter().flatten().copied().collect();
            !sets.is_empty() && sets.iter().all(|&w| w == 35)
        }) && game.battle.map.as_ref().is_some_and(|m| (0..m.rows()).any(|y| (0..m.width()).any(|x| m.object_class(x, y) == 4)));
        if opts.aggressive && (exit_goal || point_goal) {
            let hero = if game.campaign.route == 0 { 11 } else { 1 };
            let a = game.battle.army_mut(crate::battle::Side::Player);
            if let Some(i) = a.find_char(hero, 1) {
                let u = &a.units[i];
                if !changed_modes.iter().any(|m| m.0 == u.slot && m.1 == u.char_id) {
                    // A run resumed mid-stage (s2auto --recover) loads an autosave
                    // the helper already changed: restore the party's default 1.
                    let orig = if u.ai_mode == 6 && c == code::LOADED { 1 } else { u.ai_mode };
                    changed_modes.push((u.slot, u.char_id, u.kind, orig));
                }
                a.units[i].ai_mode = 6;
                emit(&format!("EXIT-GOAL stage={stage} char={hero} ai_mode=6 point={point_goal}"));
            }
        }
        // Only the stage a recovered run resumes in (the original's auto
        // battle cannot finish it, see s2auto --recover).
        if opts.exit_cheat && exit_goal && records.is_empty() {
            exit_cheat(game, stage, 6);
        } else if opts.aggressive && retries >= 2 && (exit_goal || point_goal) {
            // Not the original: from the third attempt the protagonist starts
            // on the goal (the AI's go-to-point mode cannot always find the way).
            exit_cheat(game, stage, if point_goal { 4 } else { 6 });
        }
        if opts.weak_enemies && game.campaign.turn == 1 {
            let mut n = 0;
            for u in &mut game.battle.army_mut(crate::battle::Side::Enemy).units {
                if u.on_map() {
                    u.hp = 1;
                    n += 1;
                }
            }
            emit(&format!("WEAK-ENEMIES stage={stage} units={n}"));
        }
        if game.campaign.turn == 1 {
            let n = game.campaign.flags.len();
            for &(st, f, v) in opts.set_flags.iter().filter(|&&(st, f, _)| st == stage && f < n) {
                game.campaign.flags[f] = v;
                emit(&format!("SET-FLAG stage={st} flag={f} value={v}"));
            }
        }
        if opts.boss_rush && kind != 3 && game.campaign.turn == 1 {
            boss_rush(game, stage);
        }
        if opts.caves && kind == 1 && game.campaign.turn == 1 && !game.campaign.cave_found {
            cave_cheat(game, stage);
        }
        let t0 = game.sys.ticks();
        let entry = c;
        c = flow::stage::stage_loop(game, c)?;
        if opts.aggressive && (exit_goal || point_goal) {
            goal_report(game, stage);
        }
        restore_modes(game, &changed_modes);
        let rec = StageRecord {
            index,
            stage,
            name,
            kind,
            route: game.campaign.route,
            turns: game.campaign.turn,
            code: c,
            ticks_ms: game.sys.ticks().wrapping_sub(t0),
        };
        // The stage has faded out by now: the driver saves the last screen
        // the stage waited on.
        probe().end_shot = Some(format!("{index:03}_{stage:03}_end"));
        emit(&format!(
            "STAGE-END index={index} stage={stage} code={} turns={} vms={} route={} entry={entry} next={}",
            rec.code, rec.turns, rec.ticks_ms, rec.route, game.campaign.next_stage
        ));
        records.push(rec);
        // The turn-1 autosave of this battle or cave (story stages write
        // none), for save/load round-trip and oracle checks.
        if kind == 1 || kind == 2 {
            if let Ok(p) = game.data.dir.resolve(&flow::save::slot_path(flow::save::SLOT_TURN1)) {
                let dst = opts.out.join(format!("turn1_{index:03}_{stage:03}.sav"));
                // The copy gets the AI modes back the helpers changed before the
                // autosave was written (oracle checks start from the game's state).
                let restored = std::fs::read(&p).ok().and_then(|b| {
                    let n_items = game.battle.t().items.len();
                    let mut sf = s2data::save::SaveFile::parse(&b, n_items).ok()?;
                    for u in &mut sf.player.units {
                        let slot = i16::from_le_bytes([u.0[0], u.0[1]]);
                        if let Some(m) = changed_modes.iter().find(|m| m.1 == u.character() && (m.1 != 0 || (m.0 == slot && m.2 == u.class()))) {
                            u.set_ai_mode(m.3);
                        }
                    }
                    Some(sf.to_bytes())
                });
                match restored {
                    Some(b) => {
                        let _ = std::fs::write(&dst, b);
                    }
                    None => {
                        let _ = std::fs::copy(p, &dst);
                    }
                }
            }
        }
        if opts.max_stages > 0 && records.len() >= opts.max_stages {
            break;
        }
        // Not the original: a won stage that cost the route's protagonist
        // (the stage's lose code may name the other route's one) is replayed,
        // since the campaign cannot continue sensibly without them.
        if c == code::WIN && kind == 1 && opts.retries > 0 {
            let hero = if game.campaign.route == 0 { 11 } else { 1 };
            let a = game.battle.army(crate::battle::Side::Player);
            let dead = a.find_char(hero, 2).is_some_and(|i| a.units[i].flags & crate::battle::unit::flag::DEAD != 0);
            if dead {
                emit(&format!("PROTAGONIST-DIED stage={stage} char={hero}"));
                c = code::LOSE;
            }
        }
        match c {
            code::WIN | code::RETREAT => {
                retries = 0;
                set_ctx(Ctx::Between);
                c = flow::town::between_stages(game)?;
            }
            code::LOADED | code::LOADED_SLOT50 => {
                if game.campaign.between_stages {
                    set_ctx(Ctx::Between);
                    c = flow::town::between_stages(game)?;
                }
            }
            code::CAVE => {
                flow::stage::cave_stage_setup(game)?;
                c = code::FROM_WORLD_MAP;
            }
            code::LOSE => {
                // Not the original's path (back to the title): replay the stage
                // from its turn-1 autosave with another seed.
                if retries >= opts.retries {
                    break;
                }
                retries += 1;
                let seed = opts.seed.wrapping_add(retries as u32 * 7919);
                emit(&format!("RETRY stage={stage} attempt={retries} seed={seed}"));
                flow::stage::reset(game);
                flow::save::load_game(game, flow::save::SLOT_TURN1)?;
                restore_modes(game, &changed_modes);
                qa_config(game);
                game.battle.rng = crate::battle::rng::Rng::started(crate::battle::rng::Seeder::fixed(seed));
                c = code::LOADED_SLOT50;
            }
            0x16 => c = code::NEW_STAGE,
            code::CAMPAIGN_DONE => {
                // FUN_0041a779: the ending variant is the first story flag set among 73..=80.
                let rank = (73..=80).find(|&f| game.campaign.flags[f] != 0);
                let set: Vec<usize> = [42, 43, 63, 64, 65].into_iter().filter(|&f| game.campaign.flags[f] != 0).collect();
                emit(&format!("ENDING stage={stage} flag={rank:?} branch_flags={set:?}"));
                set_ctx(Ctx::Ending);
                flow::ending::ending(game)?;
                emit("CAMPAIGN-DONE");
                break;
            }
            _ => break,
        }
    }
    set_ctx(Ctx::Done);
    Ok(records)
}

/// Where the route's protagonist ended an exit / 紅旗 stage, against the
/// goal cells (object class 4, then 6) the go-to-point mode heads for.
fn goal_report(game: &Game, stage: u16) {
    let hero = if game.campaign.route == 0 { 11 } else { 1 };
    let a = game.battle.army(crate::battle::Side::Player);
    let at = a.find_char(hero, 2).map(|i| &a.units[i]).filter(|u| u.on_map()).map(|u| (u.x, u.y));
    let Some(m) = game.battle.map.as_ref() else { return };
    let cells = |class: u16| -> Vec<(i32, i32)> {
        (0..m.rows()).flat_map(|y| (0..m.width()).map(move |x| (x, y))).filter(|&(x, y)| m.object_class(x, y) == class).collect()
    };
    let (c4, c6) = (cells(4), cells(6));
    let near = at.and_then(|(x, y)| c4.iter().chain(&c6).map(|&(gx, gy)| crate::battle::map::distance(x, y, gx, gy)).min());
    emit(&format!("GOAL-END stage={stage} hero={hero} at={at:?} nearest_goal_distance={near:?} class4={c4:?} class6={} cells", c6.len()));
}

/// Undo the helpers' AI mode changes (`(slot, char, kind, mode)` entries).
fn restore_modes(game: &mut Game, changed: &[(i16, i16, i16, i16)]) {
    for &(slot, ch, kind, mode) in changed {
        let units = &mut game.battle.army_mut(crate::battle::Side::Player).units;
        // Named characters by id (a stage can move them to another slot or
        // class), generic units by slot and class.
        if let Some(u) = units.iter_mut().find(|u| u.char_id == ch && (ch != 0 || (u.slot == slot && u.kind == kind))) {
            u.ai_mode = mode;
        }
    }
}

/// State the stage-start invariants compare against.
#[derive(Debug, Default)]
struct Invariants {
    persistent: Option<[u8; 40]>,
    disc: Option<u8>,
}

/// Checks at the start of a fresh stage (after the between-stages phase):
/// army ≤ 50 units, no summons or dead NPC allies left over, gold and item
/// counts in range, stage flags 4..=40 clear (1..=3 come from the header),
/// story flags 41..=80 never cleared, disc changes.
fn check_invariants(game: &Game, inv: &mut Invariants, index: usize, stage: u16) {
    use crate::battle::unit::flag;
    let mut bad = Vec::new();
    let a = game.battle.army(crate::battle::Side::Player);
    if a.units.len() > crate::battle::army::MAX_UNITS {
        bad.push(format!("army has {} units", a.units.len()));
    }
    let summons = a.units.iter().filter(|u| u.flags & flag::SUMMON != 0).count();
    if summons > 0 {
        bad.push(format!("{summons} summons left over"));
    }
    let dead_npc = a.units.iter().filter(|u| u.npc == 1 && u.flags & flag::DEAD != 0).count();
    if dead_npc > 0 {
        bad.push(format!("{dead_npc} dead NPC allies left over"));
    }
    if !(0..=99_999_999).contains(&a.gold) {
        bad.push(format!("gold {}", a.gold));
    }
    if let Some((i, &n)) = a.inventory.iter().enumerate().find(|(_, &n)| n > 99) {
        bad.push(format!("item {} count {n}", i + 1));
    }
    let f = &game.campaign.flags;
    let set: Vec<usize> = (4..=40).filter(|&i| f[i] != 0).collect();
    if !set.is_empty() {
        bad.push(format!("stage flags set at start {set:?}"));
    }
    let mut now = [0u8; 40];
    now.copy_from_slice(&f[41..=80]);
    if let Some(prev) = inv.persistent {
        let lost: Vec<usize> = (0..40).filter(|&i| prev[i] != 0 && now[i] == 0).map(|i| i + 41).collect();
        if !lost.is_empty() {
            bad.push(format!("persistent flags cleared {lost:?}"));
        }
    }
    inv.persistent = Some(now);
    let disc = game.campaign.disc;
    if inv.disc.is_some_and(|d| d != disc) {
        emit(&format!("DISC {} -> {disc} at stage {stage}", inv.disc.unwrap_or(0)));
    }
    inv.disc = Some(disc);
    if bad.is_empty() {
        emit(&format!("INVARIANTS ok index={index} stage={stage} units={} gold={}", a.units.len(), a.gold));
    } else {
        emit(&format!("INVARIANT-FAIL index={index} stage={stage} {}", bad.join("; ")));
    }
}

/// `--boss-rush`: move the route's protagonist onto a free diagonal
/// neighbour (height step below 4, as FUN_00441326) of the first enemy leader.
fn boss_rush(game: &mut Game, stage: u16) {
    use crate::battle::Side;
    let hero = if game.campaign.route == 0 { 11 } else { 1 };
    let b = &game.battle;
    let Some(i) = b.army(Side::Player).find_char(hero, 0) else { return };
    let Some(boss) = b.army(Side::Enemy).units.iter().find(|u| u.rank == 1 && u.on_map()) else {
        emit(&format!("BOSS-RUSH stage={stage}: no leader on the map"));
        return;
    };
    let (bx, by) = (boss.x, boss.y);
    let m = b.map();
    let odd = by & 1 != 0;
    let cells = [
        (if odd { bx } else { bx - 1 }, by - 1),
        (if odd { bx + 1 } else { bx }, by - 1),
        (if odd { bx } else { bx - 1 }, by + 1),
        (if odd { bx + 1 } else { bx }, by + 1),
    ];
    let free = cells.into_iter().find(|&(x, y)| {
        m.move_cell(x, y).is_some()
            && (m.height(x, y) - m.height(bx, by)).abs() < 4
            && b.army(Side::Enemy).unit_at(x, y) == 0
            && b.army(Side::Player).unit_at(x, y) == 0
    });
    let (hx, hy) = (b.army(Side::Player).units[i].x, b.army(Side::Player).units[i].y);
    match free {
        Some((x, y)) => {
            let u = &mut game.battle.army_mut(Side::Player).units[i];
            u.x = x as _;
            u.y = y as _;
            crate::battle::view::bake(game);
            emit(&format!("BOSS-RUSH stage={stage} char={hero} ({hx},{hy}) -> ({x},{y}) next to ({bx},{by})"));
        }
        None => emit(&format!("BOSS-RUSH stage={stage}: no free cell next to ({bx},{by})")),
    }
}

/// `--caves`: move the route's protagonist (swapping with an own unit) onto
/// the first spot of the stage's hidden-stage codes (0xff → header +0x10,
/// 0xfe → +0x11) and run the cave check a unit ending its move there gets.
fn cave_cheat(game: &mut Game, stage: u16) {
    use crate::battle::{Side, UnitRef};
    let targets = game.stage.as_ref().map(|s| s.stage.header.secret_next).unwrap_or_default();
    if targets.iter().all(|&t| t == 0) {
        return;
    }
    let hero = if game.campaign.route == 0 { 11 } else { 1 };
    let b = &game.battle;
    let Some(i) = b.army(Side::Player).find_char(hero, 0) else { return };
    let m = b.map();
    let spot = m.data.spots.iter().map(|sp| (sp.col as i32, sp.row as i32, sp.flags & 0xff)).find(|&(x, y, code)| {
        let t = match code {
            0xff => targets[0],
            0xfe => targets[1],
            _ => 0,
        };
        t != 0 && m.move_cell(x, y).is_some() && b.army(Side::Enemy).unit_at(x, y) == 0
    });
    let Some((x, y, code)) = spot else {
        emit(&format!("CAVE-CHEAT stage={stage} targets={targets:?}: no spot"));
        return;
    };
    let other = b.army(Side::Player).unit_at(x, y);
    let (hx, hy) = (b.army(Side::Player).units[i].x, b.army(Side::Player).units[i].y);
    let a = game.battle.army_mut(Side::Player);
    if other != 0 && other - 1 != i {
        a.units[other - 1].x = hx;
        a.units[other - 1].y = hy;
    }
    a.units[i].x = x;
    a.units[i].y = y;
    crate::battle::view::bake(game);
    crate::battle::menu::cave_check(game, UnitRef::new(Side::Player, i));
    emit(&format!(
        "CAVE-CHEAT stage={stage} char={hero} ({hx},{hy}) -> ({x},{y}) code={code:#x} found={} next={}",
        game.campaign.cave_found, game.campaign.alt_next_stage
    ));
}

/// `--exit-cheat` (and late retries of goal stages): move the route's
/// protagonist onto the first cell of object class `class` (6 exit, 4 紅旗)
/// units can stand on that is free, or else swap places with an own unit
/// standing on one.
fn exit_cheat(game: &mut Game, stage: u16, class: u16) {
    use crate::battle::Side;
    let hero = if game.campaign.route == 0 { 11 } else { 1 };
    let b = &game.battle;
    let Some(i) = b.army(Side::Player).find_char(hero, 0) else { return };
    let m = b.map();
    // Exit stages without exit cells are won at the 紅旗 (class 4) by script.
    let has = |c: u16| (0..m.rows()).any(|y| (0..m.width()).any(|x| m.object_class(x, y) == c));
    let class = if class == 6 && !has(6) { 4 } else { class };
    let mut free = None;
    let mut taken = None;
    for y in 0..m.rows() {
        for x in 0..m.width() {
            if m.object_class(x, y) != class || m.move_cell(x, y).is_none() || b.army(Side::Enemy).unit_at(x, y) != 0 {
                continue;
            }
            match b.army(Side::Player).unit_at(x, y) {
                0 => free = free.or(Some((x, y))),
                k => taken = taken.or(Some((x, y, k - 1))),
            }
        }
    }
    let (hx, hy) = (b.army(Side::Player).units[i].x, b.army(Side::Player).units[i].y);
    let target = match (free, taken) {
        (Some((x, y)), _) => Some((x, y, None)),
        (None, Some((x, y, k))) if k != i => Some((x, y, Some(k))),
        _ => None,
    };
    if let Some((x, y, other)) = target {
        let a = game.battle.army_mut(Side::Player);
        if let Some(k) = other {
            a.units[k].x = hx;
            a.units[k].y = hy;
        }
        a.units[i].x = x;
        a.units[i].y = y;
        emit(&format!("EXIT-CHEAT stage={stage} char={hero} class={class} ({hx},{hy}) -> ({x},{y}) swapped={}", other.is_some()));
        crate::battle::view::bake(game);
    } else {
        emit(&format!("EXIT-CHEAT stage={stage} char={hero} class={class}: no free goal cell"));
    }
}

// ---- the frame driver ---------------------------------------------------------------------

/// Frames of no new picture before the driver acts (0.5 s).
const IDLE_FRAMES: u64 = 30;
/// Recent frame hashes remembered (blinking prompts and palette-free cycles repeat within it).
const RECENT: usize = 96;

/// Town hub / inn / shop / church hot-spot menus warp the pointer to their first hot-spot.
const WARP_TOWN: (i32, i32) = (298, 253);
const WARP_INN: (i32, i32) = (517, 118);
const WARP_SHOP: (i32, i32) = (257, 370);
const WARP_CHURCH: (i32, i32) = (119, 348);
/// 出發 in the town hub (fourth hot-spot).
const TOWN_DEPART: (i32, i32) = (187, 449);
/// The deployment screen's first button (自動 → start).
const DEPLOY_AUTO: (i32, i32) = (0x40, 0xe4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Key(u8),
    Click(i32, i32),
}

/// Plays the game from the presented frames: detects when the picture has
/// stopped changing (the game waits for input) and sends the next action
/// of the current context; reports a hang when nothing new is shown for
/// `hang_frames`.
pub struct Driver {
    recent: VecDeque<u64>,
    last_new: u64,
    last_input: u64,
    rot: usize,
    ctx: Ctx,
    hint: Option<(i32, i32)>,
    hang_frames: u64,
    out: PathBuf,
    shots_taken: usize,
    pending_start_shot: Option<String>,
    /// The last screen the game waited on (for the stage-end screenshot).
    last_idle: Option<(Vec<u8>, s2data::Palette)>,
}

/// The battle system menu (FUN_0042cc2c) is on screen: palette indices of
/// its top ornament.
pub fn system_menu_open(px: &[u8]) -> bool {
    const PROBE: [(usize, usize, u8); 6] = [(300, 58, 248), (310, 58, 255), (330, 58, 240), (296, 96, 235), (344, 96, 235), (320, 110, 251)];
    PROBE.iter().all(|&(x, y, v)| px.get(y * crate::sys::SCREEN_W + x) == Some(&v))
}

/// The yes/no bar (sysimg 197, `ui::yes_no`), stashed by `run`.
static YES_NO: OnceLock<s2data::sprite::Image> = OnceLock::new();

/// A yes/no bar is on screen (anywhere): the opaque pixels of its left
/// border on two rows.
pub fn yes_no_open(px: &[u8]) -> bool {
    use crate::sys::{SCREEN_H, SCREEN_W};
    let Some(bar) = YES_NO.get() else { return false };
    let (w, h) = (bar.width, bar.height);
    if w < 16 || h < 6 || w > SCREEN_W || h > SCREEN_H || px.len() < SCREEN_W * SCREEN_H {
        return false;
    }
    let probe: Vec<(usize, usize, u8)> =
        [2, h - 3].iter().flat_map(|&r| (2..14).map(move |c| (c, r, bar.pixels[r * w + c]))).filter(|p| p.2 != 0).collect();
    if probe.len() < 16 {
        return false;
    }
    (0..=SCREEN_H - h).any(|y| (0..=SCREEN_W - w).any(|x| probe.iter().all(|&(c, r, v)| px[(y + r) * SCREEN_W + x + c] == v)))
}

fn hash(px: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for c in px.chunks(8) {
        let mut w = 0u64;
        for (i, &b) in c.iter().enumerate() {
            w |= (b as u64) << (i * 8);
        }
        h = (h ^ w).wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl Driver {
    pub fn new(out: PathBuf, hang_minutes: u64) -> Driver {
        Driver {
            recent: VecDeque::new(),
            last_new: 0,
            last_input: 0,
            rot: 0,
            ctx: Ctx::Boot,
            hint: None,
            hang_frames: hang_minutes * 60 * 60,
            out,
            shots_taken: 0,
            pending_start_shot: None,
            last_idle: None,
        }
    }

    fn actions(&self) -> Vec<Vec<Act>> {
        use Act::*;
        match self.ctx {
            Ctx::Boot => vec![vec![Key(vk::ESCAPE)], vec![Key(vk::RETURN)], vec![Click(320, 240)]],
            Ctx::Title => vec![vec![Key(vk::RETURN)]],
            Ctx::Dialogue | Ctx::Ending => vec![vec![Key(vk::RETURN)], vec![Click(320, 420)]],
            Ctx::Choice(k) => {
                let mut v = vec![Key(vk::DOWN); k.saturating_sub(1) as usize];
                v.push(Key(vk::RETURN));
                vec![v]
            }
            // Esc opens the system menu on the map (a second one when the first
            // only left a unit) and closes click-to-continue boxes; RETURN for
            // the rest. 自動作戰 itself is chosen in the menu (see `on_frame`),
            // not with its function key, so the driver does not depend on the
            // key table (FUN_0042b9f8). No clicks, which could select a unit.
            Ctx::Stage => vec![vec![Key(vk::ESCAPE)], vec![Key(vk::ESCAPE)], vec![Key(vk::RETURN)]],
            Ctx::Between => match self.hint {
                Some(WARP_TOWN) => vec![vec![Click(TOWN_DEPART.0, TOWN_DEPART.1)]],
                Some(WARP_INN) | Some(WARP_SHOP) | Some(WARP_CHURCH) => vec![vec![Key(vk::ESCAPE)]],
                _ => vec![vec![Click(DEPLOY_AUTO.0, DEPLOY_AUTO.1)], vec![Key(vk::RETURN)], vec![Click(320, 420)], vec![Key(vk::ESCAPE)]],
            },
            Ctx::Done => vec![],
        }
    }

    fn send(sh: &Shared, acts: &[Act]) {
        for a in acts {
            match *a {
                Act::Key(k) => {
                    sh.push_input(InputEvent::KeyDown(k));
                    sh.push_input(InputEvent::KeyUp(k));
                }
                Act::Click(x, y) => {
                    sh.set_mouse(x, y);
                    sh.push_input(InputEvent::MouseDown(0));
                    sh.push_input(InputEvent::MouseUp(0));
                }
            }
        }
    }

    /// The frame hook.
    pub fn on_frame(&mut self, info: &FlipInfo, sh: &Shared) {
        let serial = info.serial;
        // Pixels plus the palette's total brightness: a fade changes the
        // latter (and must not count as waiting), palette cycling does not.
        let bright: u64 = info.palette.0.iter().map(|c| c[0] as u64 + c[1] as u64 + c[2] as u64).sum();
        let h = hash(info.pixels) ^ bright.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let fresh = !self.recent.contains(&h);
        if fresh {
            self.last_new = serial;
            self.rot = 0;
        }
        self.recent.push_back(h);
        if self.recent.len() > RECENT {
            self.recent.pop_front();
        }
        if let Some(w) = sh.take_warp() {
            self.hint = Some(w);
        }
        if let Some(name) = SNAP.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let p = self.out.join(format!("{name}.png"));
            match save_png(&p, info.pixels, info.palette) {
                Ok(()) => emit(&format!("SNAP {}", p.display())),
                Err(e) => emit(&format!("WARN screenshot {}: {e}", p.display())),
            }
        }
        let (ctx, shot, end_shot) = {
            let mut p = probe();
            (p.ctx, p.shot.take(), p.end_shot.take())
        };
        if let Some(name) = end_shot {
            let p = self.out.join(format!("{name}.png"));
            let r = match &self.last_idle {
                Some((px, pal)) => save_png(&p, px, pal),
                None => save_png(&p, info.pixels, info.palette),
            };
            if let Err(e) = r {
                emit(&format!("WARN screenshot {}: {e}", p.display()));
            }
            self.last_idle = None;
        }
        if ctx != self.ctx {
            self.ctx = ctx;
            self.rot = 0;
            if !matches!(ctx, Ctx::Between) {
                self.hint = None;
            }
        }
        if let Some(name) = shot {
            self.pending_start_shot = Some(name);
        }
        let idle = serial.saturating_sub(self.last_new) >= IDLE_FRAMES && serial.saturating_sub(self.last_input) >= IDLE_FRAMES;
        if idle {
            // The first time a stage waits for input: its start screenshot.
            if let Some(name) = self.pending_start_shot.take() {
                let p = self.out.join(format!("{name}.png"));
                if let Err(e) = save_png(&p, info.pixels, info.palette) {
                    emit(&format!("WARN screenshot {}: {e}", p.display()));
                }
                self.shots_taken += 1;
            }
            if self.ctx == Ctx::Stage || self.ctx == Ctx::Dialogue {
                self.last_idle = Some((info.pixels.to_vec(), info.palette.clone()));
            }
            // In battle: 自動作戰 is the system menu's fourth entry, and every
            // yes/no (its confirmation, 進入洞窟) is answered yes.
            let yes = self.ctx == Ctx::Stage && yes_no_open(info.pixels);
            let menu = self.ctx == Ctx::Stage && !yes && system_menu_open(info.pixels);
            if menu || yes {
                let a = if menu { vec![Act::Key(vk::DOWN), Act::Key(vk::DOWN), Act::Key(vk::DOWN), Act::Key(vk::RETURN)] } else { vec![Act::Key(vk::RETURN)] };
                if std::env::var_os("S2_QA_DEBUG").is_some() {
                    emit(&format!("ACT frame={serial} ctx={:?} {} {a:?}", self.ctx, if yes { "yes/no" } else { "system menu" }));
                }
                Self::send(sh, &a);
                self.last_input = serial;
                return;
            }
            let acts = self.actions();
            if !acts.is_empty() {
                let a = &acts[self.rot % acts.len()];
                if std::env::var_os("S2_QA_DEBUG").is_some() {
                    emit(&format!("ACT frame={serial} ctx={:?} hint={:?} {a:?}", self.ctx, self.hint));
                    if std::env::var_os("S2_QA_DEBUG_SHOTS").is_some() {
                        let _ = save_png(&self.out.join(format!("act_{serial}.png")), info.pixels, info.palette);
                    }
                }
                Self::send(sh, a);
                self.rot += 1;
            }
            // A warp hint names the screen it was made on; used once.
            self.hint = None;
            self.last_input = serial;
        }
        if self.hang_frames > 0 && serial.saturating_sub(self.last_new) > self.hang_frames && !probe().hung {
            probe().hung = true;
            let p = self.out.join(format!("hang_{serial}.png"));
            let _ = save_png(&p, info.pixels, info.palette);
            let (stage, name) = {
                let pr = probe();
                (pr.stage, pr.stage_name.clone())
            };
            emit(&format!(
                "HANG frame={serial} vms={} ctx={:?} stage={stage} name={name} hint={:?} shot={}",
                info.ticks,
                self.ctx,
                self.hint,
                p.display()
            ));
            // The hook runs on the game thread: where the game waits.
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            let frames: Vec<&str> = bt
                .lines()
                .filter_map(|l| l.trim().split_once(": ").map(|(_, f)| f))
                .filter(|f| f.starts_with("s2engine::") && !f.starts_with("s2engine::qa::") && !f.starts_with("s2engine::sys::"))
                .take(12)
                .collect();
            emit(&format!("HANG-STACK {}", frames.join(" <- ")));
            sh.request_quit();
        }
    }
}
