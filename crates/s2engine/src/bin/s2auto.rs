//! Automated campaign QA: plays the real game flow headlessly (virtual
//! clock, like s2run) with `s2engine::qa`'s driver — new game, the chosen
//! route, dialogue, choices, 自動作戰 in every battle, the shortest way
//! between stages — and reports per stage what happened, plus every crash
//! (panic), hang (no new frame for N virtual minutes, or no output for N
//! real seconds) and stall (M turns without an attack).
//!
//!   s2auto --game DIR --out DIR [--route 0|1|both] [--seed N] [--max-stages N]
//!          [--from-save SLOT [--save-file PATH]] [--random-choices] [--retries N]
//!          [--hang-minutes N] [--stall-turns N] [--timeout-secs N] [--difficulty 0..5]
//!          [--passive] [--god] [--exit-cheat] [--recover N] [--reseed]
//!          [--roundtrip K,K,..] [--no-cd] [--caves] [--weak-enemies] [--boss-rush]
//!          [--set-flag STAGE:FLAG[=VALUE]].. [--choice STAGE:OPTION]..
//!          [--enemy-ai original|enhanced] [--run NAME]
//!   s2auto --game DIR --out DIR --oracle-check SAVE|DIR [--oracle-mode battle|town|vs:K]
//!          [--frames N] [--shot-every N] [--record-only] [--run NAME]
//!
//! Defaults: route 0, seed 1, difficulty 0 (the screen's default, 3), 3 retries
//! of a lost stage from its turn-1 autosave with other seeds, hang after 5
//! virtual minutes without a new frame or 120 s without output, stall after
//! 25 turns without an attack. `--passive` keeps the party's own AI modes;
//! `--god` strengthens the player side at each stage start (the AI alone
//! loses many stages). `--recover N`: after a stall, continue up to N times
//! from the stall's autosave with `--exit-cheat` (the protagonist starts on
//! the exit of exit-goal stages). `--save-file PATH` puts PATH into the run's
//! SAVE folder as slot `--from-save`. `--roundtrip K,..` (implies `--reseed`,
//! which reseeds the battle RNG at every stage start from seed and stage):
//! after the run, resume from the turn-1 autosave of record K and check the
//! next stages repeat exactly. `--no-cd` sets the copy's SUPERSP.CFG drive
//! to 0 (no CD device), the setting under which disc changes are recorded.
//! `--caves`: in a stage with a hidden stage the protagonist starts on the
//! cave spot and finds it, so the cave stage follows (coverage).
//! `--weak-enemies`: enemies on the map start each stage with 1 HP (to reach
//! branches that need a boss beaten in time, e.g. stage 60/130 → 61/131);
//! `--boss-rush`: the protagonist starts next to the first enemy leader.
//! `--set-flag S:F[=V]`: story flag F set to V (1) at stage S's turn 1
//! (branch coverage, e.g. 60:15 = 克雷布特里 beaten, 61:4 = stage won).
//! `--choice S:K`: every choice in stage S takes option K (story branches:
//! 29:1 → 33 (cave 63), 31:2 → 35/36 (cave 65), 99:1 → 102 (cave 135)).
//! A stall with no player unit left on the map (a stage whose lose codes
//! name another route's protagonist) is not killed: the battle runs on to
//! turn 100, a defeat, as the original does.
//!
//! `--oracle-check SAVE` is a differential run against the original (see
//! `oracle_check`): SAVE is loaded from the title, 自動作戰 plays N frames
//! (default 30000) in the port while its inputs are recorded as an s2run
//! script, and tools/oracle/compare.py runs that script through s2run and
//! the original (the oracle needs tools/.venv); shots every `--shot-every`
//! frames (300) and every RNG draw are compared. `--record-only` stops after
//! writing the script. Expect the oracle at roughly 5-40 frames a second.
//! `--oracle-mode town`: SAVE is a between-stages save; the inn, the shop,
//! the departure, march and deployment come first. `vs:K`: SAVE is a folder
//! with UJ01 (blue) and UJ02 (red), played in 雙人對戰 on battlefield K.
//!
//! Each route runs in a child process (`--worker`) on a private copy of the
//! game directory (files linked, SAVE copied), so autosaves never touch the
//! original and a crash or a killed hang does not end the other runs.
//! Output: OUT/RUN/route<r>/ (screenshots NNN_SSS_start/_end.png, hang
//! shots), OUT/RUN/route<r>.log (the child's full stderr with the battle
//! trace), OUT/RUN/summary.txt.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use s2engine::qa::{self, Driver, Opts};
use s2engine::sys::{spawn_headless, vk, Shared};
use s2engine::Data;

#[derive(Debug, Clone)]
struct Args {
    game: PathBuf,
    out: PathBuf,
    run: String,
    routes: Vec<u8>,
    seed: u32,
    max_stages: usize,
    from_save: Option<usize>,
    random_choices: bool,
    retries: usize,
    hang_minutes: u64,
    stall_turns: u32,
    timeout_secs: u64,
    passive: bool,
    difficulty: u8,
    god: bool,
    /// `--enemy-ai enhanced`: the enemy plays with `battle::ai_plus` (not the original).
    enhanced_ai: bool,
    exit_cheat: bool,
    recover: usize,
    reseed: bool,
    save_file: Option<PathBuf>,
    roundtrip: Vec<usize>,
    no_cd: bool,
    index_base: usize,
    worker: bool,
    caves: bool,
    weak_enemies: bool,
    boss_rush: bool,
    set_flags: Vec<(u16, usize, u8)>,
    choices: Vec<(u16, u8)>,
    oracle_check: Option<PathBuf>,
    oracle_mode: String,
    record_only: bool,
    frames: u64,
    shot_every: u64,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        game: std::env::var_os("S2_GAME_DIR").map(PathBuf::from).unwrap_or_default(),
        out: std::env::var_os("S2_AUTO_OUT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("auto-runs")),
        run: String::new(),
        routes: vec![0],
        seed: 1,
        max_stages: 0,
        from_save: None,
        random_choices: false,
        retries: 3,
        hang_minutes: 5,
        stall_turns: 25,
        timeout_secs: 120,
        passive: false,
        difficulty: 0,
        god: false,
        enhanced_ai: false,
        exit_cheat: false,
        recover: 0,
        reseed: false,
        save_file: None,
        roundtrip: Vec::new(),
        no_cd: false,
        index_base: 0,
        worker: false,
        caves: false,
        weak_enemies: false,
        boss_rush: false,
        set_flags: Vec::new(),
        choices: Vec::new(),
        oracle_check: None,
        oracle_mode: "battle".into(),
        record_only: false,
        frames: 30000,
        shot_every: 300,
    };
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        let mut val = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
        match s.as_str() {
            "--game" => a.game = PathBuf::from(val("--game")?),
            "--out" => a.out = PathBuf::from(val("--out")?),
            "--run" => a.run = val("--run")?,
            "--route" => {
                a.routes = match val("--route")?.as_str() {
                    "0" => vec![0],
                    "1" => vec![1],
                    "both" => vec![0, 1],
                    r => bail!("--route 0|1|both, not {r}"),
                }
            }
            "--seed" => a.seed = val("--seed")?.parse()?,
            "--max-stages" => a.max_stages = val("--max-stages")?.parse()?,
            "--from-save" => a.from_save = Some(val("--from-save")?.parse()?),
            "--random-choices" => a.random_choices = true,
            "--retries" => a.retries = val("--retries")?.parse()?,
            "--hang-minutes" => a.hang_minutes = val("--hang-minutes")?.parse()?,
            "--stall-turns" => a.stall_turns = val("--stall-turns")?.parse()?,
            "--timeout-secs" => a.timeout_secs = val("--timeout-secs")?.parse()?,
            "--passive" => a.passive = true,
            "--god" => a.god = true,
            "--enemy-ai" => {
                a.enhanced_ai = match it.next().as_deref() {
                    Some("enhanced") => true,
                    Some("original") => false,
                    other => anyhow::bail!("--enemy-ai: original or enhanced, not {other:?}"),
                }
            }
            "--exit-cheat" => a.exit_cheat = true,
            "--recover" => a.recover = val("--recover")?.parse()?,
            "--reseed" => a.reseed = true,
            "--no-cd" => a.no_cd = true,
            "--index-base" => a.index_base = val("--index-base")?.parse()?,
            "--save-file" => a.save_file = Some(PathBuf::from(val("--save-file")?)),
            "--roundtrip" => {
                a.roundtrip = val("--roundtrip")?.split(',').map(|v| v.trim().parse()).collect::<Result<_, _>>()?;
                a.reseed = true;
            }
            "--difficulty" => a.difficulty = val("--difficulty")?.parse()?,
            "--worker" => a.worker = true,
            "--caves" => a.caves = true,
            "--weak-enemies" => a.weak_enemies = true,
            "--boss-rush" => a.boss_rush = true,
            "--choice" => {
                let v = val("--choice")?;
                let (st, k) = v.split_once(':').context("--choice STAGE:OPTION")?;
                a.choices.push((st.parse()?, k.parse()?));
            }
            "--set-flag" => {
                let v = val("--set-flag")?;
                let (st, f) = v.split_once(':').context("--set-flag STAGE:FLAG[=VALUE]")?;
                let (f, val) = f.split_once('=').unwrap_or((f, "1"));
                a.set_flags.push((st.parse()?, f.parse()?, val.parse()?));
            }
            "--oracle-check" => a.oracle_check = Some(PathBuf::from(val("--oracle-check")?)),
            "--record-only" => a.record_only = true,
            "--oracle-mode" => a.oracle_mode = val("--oracle-mode")?,
            "--frames" => a.frames = val("--frames")?.parse()?,
            "--shot-every" => a.shot_every = val("--shot-every")?.parse()?,
            other => bail!("unknown argument {other}"),
        }
    }
    if a.game.as_os_str().is_empty() {
        bail!("--game DIR (or S2_GAME_DIR) is required");
    }
    if a.run.is_empty() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        a.run = format!("run-{t}");
    }
    Ok(a)
}

// ---- worker ---------------------------------------------------------------------------

fn worker(a: &Args) -> Result<()> {
    let route = a.routes[0];
    std::fs::create_dir_all(&a.out)?;
    let data = Data::open(&a.game)?;
    let opts = Opts {
        route,
        seed: a.seed,
        random_choices: a.random_choices,
        max_stages: a.max_stages,
        from_save: a.from_save,
        retries: a.retries,
        out: a.out.clone(),
        aggressive: !a.passive,
        caves: a.caves,
        weak_enemies: a.weak_enemies,
        boss_rush: a.boss_rush,
        set_flags: a.set_flags.clone(),
        choices: a.choices.clone(),
        difficulty: a.difficulty,
        god: a.god,
        exit_cheat: a.exit_cheat,
        reseed: a.reseed,
        index_base: a.index_base,
    };
    // `snap NAME` on stdin: a screenshot of the next frame.
    std::thread::spawn(|| {
        for l in std::io::stdin().lines().map_while(Result::ok) {
            if let Some(n) = l.strip_prefix("snap ") {
                qa::request_snapshot(n.trim());
            }
        }
    });
    let mut driver = Driver::new(a.out.clone(), a.hang_minutes);
    let hook = Box::new(move |info: &s2engine::sys::FlipInfo, sh: &Shared| driver.on_frame(info, sh));
    let enhanced_ai = a.enhanced_ai;
    let handle = spawn_headless(Shared::new(), hook, move |sys| {
        let mut game = s2engine::game::Game::new(sys, &data);
        game.options.enhanced_ai = enhanced_ai;
        qa::run(&mut game, &opts).map(|_| ())
    });
    match handle.join() {
        Ok(Ok(())) => {
            eprintln!("QA EXIT ok");
            Ok(())
        }
        Ok(Err(e)) => {
            eprintln!("QA EXIT error {e:#}");
            std::process::exit(3)
        }
        Err(_) => {
            eprintln!("QA EXIT panic");
            std::process::exit(101)
        }
    }
}

// ---- oracle check ---------------------------------------------------------------------

/// `--oracle-check SAVE`: a differential run against the original under
/// the oracle (tools/oracle, docs/oracle.md). The port plays SAVE (loaded
/// from the title as slot 1, as tools/oracle/scripts/town.s2 does) with
/// 自動作戰 chosen from the system menu whenever the game waits for input,
/// and records its inputs as an s2run script (OUT/RUN/check.s2, shots every
/// `--shot-every` frames up to `--frames`); tools/oracle/compare.py then runs
/// that script through s2run and the original, and the two programs' RNG
/// draws (`S2_RNG_TRACE`, the oracle's `--watch 46d776,46d7ca`) are compared
/// draw by draw. Report: OUT/RUN/oracle-check.txt.
fn oracle_check(a: &Args, save: &Path, base: &Path) -> Result<()> {
    let fixture = base.join("fixture");
    std::fs::create_dir_all(&fixture)?;
    if save.is_dir() {
        // A SAVE folder (VS mode: blue UJ01, red UJ02).
        for e in std::fs::read_dir(save)?.flatten() {
            if e.path().is_file() {
                std::fs::copy(e.path(), fixture.join(e.file_name().to_string_lossy().to_ascii_uppercase()))?;
            }
        }
    } else {
        std::fs::copy(save, fixture.join("UJ01.SAV")).with_context(|| format!("copying {}", save.display()))?;
    }
    let fixture = fixture.canonicalize()?;
    // The recording runs on a private game directory whose SAVE is the fixture
    // alone, as compare.py's s2run run does.
    let game = base.join("game");
    overlay(&a.game, &game)?;
    let saves = game.join(std::fs::read_dir(&game)?.flatten().map(|e| e.file_name()).find(|n| n.to_string_lossy().eq_ignore_ascii_case("SAVE")).unwrap_or_else(|| "SAVE".into()));
    if saves.exists() {
        std::fs::remove_dir_all(&saves)?;
    }
    std::fs::create_dir_all(&saves)?;
    for e in std::fs::read_dir(&fixture)?.flatten() {
        std::fs::copy(e.path(), saves.join(e.file_name()))?;
    }

    let data = Data::open(&game)?;
    let mode = OracleMode::parse(&a.oracle_mode)?;
    let rec = std::sync::Arc::new(std::sync::Mutex::new(Recorder::new(mode, a.frames, a.shot_every)));
    let r2 = rec.clone();
    let hook = Box::new(move |info: &s2engine::sys::FlipInfo, sh: &Shared| r2.lock().unwrap().on_frame(info, sh));
    let handle = spawn_headless(Shared::new(), hook, move |sys| s2engine::run(sys, &data));
    match handle.join() {
        Ok(r) => r?,
        Err(_) => bail!("the port panicked while recording"),
    }
    let rec = rec.lock().unwrap();
    let mut script = format!(
        "#! saves {}\n# s2auto --oracle-check {} --oracle-mode {}: {}, then 自動作戰 from the\n\
         # system menu whenever the game waits (inputs recorded from the port).\n",
        fixture.display(),
        save.display(),
        a.oracle_mode,
        mode.describe()
    );
    for l in &rec.steps {
        script += l;
        script.push('\n');
    }
    let script_path = base.join("check.s2");
    std::fs::write(&script_path, &script)?;
    eprintln!("oracle-check: {} steps recorded ({} system-menu bursts) -> {}", rec.steps.len(), rec.bursts, script_path.display());
    if a.record_only {
        return Ok(());
    }

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let cmp = base.join("cmp");
    let out = Command::new(repo.join("tools/.venv/bin/python"))
        .arg("-I")
        .arg(repo.join("tools/oracle/compare.py"))
        .arg("--game")
        .arg(&a.game)
        .arg("--out")
        .arg(&cmp)
        .arg("--script-file")
        .arg(&script_path)
        .arg("--oracle-arg=--watch=46d776,46d7ca")
        .arg("--oracle-arg=--wall-limit=86400")
        .env("S2_RNG_TRACE", "1")
        .env("S2_BATTLE_TRACE", "1")
        .output()
        .context("running tools/oracle/compare.py")?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    std::fs::write(base.join("compare.txt"), &text)?;
    let mut report = String::new();
    let lines: Vec<&str> = text.lines().collect();
    let bad: Vec<&&str> = lines.iter().filter(|l| l.contains("px differ") || l.contains("MISSING")).collect();
    report += &format!("shots: {}\n", lines.iter().rev().find(|l| l.contains("shots identical")).copied().unwrap_or("(no result)"));
    if let Some(first) = bad.first() {
        report += &format!("first differing shot: {}\n", first.trim());
    }
    for l in lines.iter().filter(|l| l.contains("SAVE") || l.contains("clocks part")) {
        report += &format!("{}\n", l.trim());
    }
    let rng = rng_diff(&cmp.join("s2run.txt"), &cmp.join("oracle").join("oracle.log"))?;
    report += &rng;
    // One line for tools/oracle/sweep.sh.
    let shots: Vec<&str> = lines.iter().filter(|l| l.contains(" identical") || l.contains("px differ") || l.contains("MISSING")).filter(|l| !l.contains("SAVE") && !l.contains("shots identical")).copied().collect();
    let same = shots.iter().filter(|l| l.trim_end().ends_with("identical")).count();
    let saves_ok = lines.iter().filter(|l| l.contains("SAVE\\")).all(|l| l.contains("identical") || l.contains("written, identical"));
    let draws = rng.lines().next().unwrap_or("").replace("rng draws: ", "");
    let draws_same = rng.contains("rng draws identical");
    let first = bad.first().map(|l| l.split_whitespace().next().unwrap_or("?").to_string()).unwrap_or_else(|| "-".into());
    report += &format!(
        "SUMMARY frames={} shots={same}/{} draws_identical={draws_same} draws=({draws}) saves_identical={saves_ok} first_diff={first}\n",
        a.frames,
        shots.len()
    );
    std::fs::write(base.join("oracle-check.txt"), &report)?;
    print!("{report}");
    Ok(())
}

/// What an oracle check plays (`--oracle-mode`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum OracleMode {
    /// Slot 1 loaded from the title (a battle save), then auto battle.
    Battle,
    /// 雙人對戰 on battlefield K (1..=10) with slots 1 (blue) and 2 (red).
    Vs(u8),
    /// Slot 1 loaded from the title (a between-stages save): the town's inn
    /// and shop, departure, world map, narration, deployment, then auto battle.
    Town,
}

impl OracleMode {
    fn parse(s: &str) -> Result<OracleMode> {
        Ok(match s {
            "battle" => OracleMode::Battle,
            "town" => OracleMode::Town,
            v if v.starts_with("vs:") => {
                let k: u8 = v[3..].parse()?;
                if !(1..=10).contains(&k) {
                    bail!("--oracle-mode vs:K needs K in 1..=10");
                }
                OracleMode::Vs(k)
            }
            _ => bail!("--oracle-mode battle|town|vs:K"),
        })
    }

    fn describe(&self) -> String {
        match self {
            OracleMode::Battle => "slot 1 loaded from the title".into(),
            OracleMode::Town => "slot 1 (between stages) loaded from the title, inn, shop, departure, deployment".into(),
            OracleMode::Vs(k) => format!("雙人對戰 on battlefield {k}, blue slot 1, red slot 2"),
        }
    }

    /// The fixed opening inputs and the frame the closed loop starts at.
    fn prefix(&self) -> (Vec<(u64, Act)>, u64) {
        use Act::*;
        // Esc, Esc (movies), then the title menu.
        let mut v = vec![(300, Key(vk::ESCAPE)), (600, Key(vk::ESCAPE))];
        match *self {
            OracleMode::Battle => {
                // DOWN + RETURN (載入進度), RETURN (slot 1), 300 frames for the load.
                v.extend([(1000, Key(vk::DOWN)), (1030, Key(vk::RETURN)), (1230, Key(vk::RETURN))]);
                (v, 1530)
            }
            OracleMode::Town => {
                v.extend([(1000, Key(vk::DOWN)), (1030, Key(vk::RETURN)), (1230, Key(vk::RETURN))]);
                // As tools/oracle/scripts/town.s2 and shop.s2: the inn and its
                // status list, the shop's first buy list, then 出發 and the
                // march / narration / deployment of after1.s2.
                let tour = [
                    (1530, Move(298, 253)),
                    (1590, Click(0, 298, 253)),
                    (1790, Click(0, 322, 223)),
                    (1990, Click(1, 320, 240)),
                    (2190, Click(1, 320, 240)),
                    (2390, Click(0, 151, 321)),
                    (2590, Click(0, 257, 370)),
                    (2790, Click(1, 320, 240)),
                    (2990, Click(1, 320, 240)),
                    (3290, Click(0, 187, 449)),
                    (3690, Key(vk::RETURN)),
                    (3890, Key(vk::RETURN)),
                    (4090, Key(vk::RETURN)),
                    (4290, Click(0, 67, 228)),
                    (4490, Click(0, 293, 228)),
                    (4790, Key(vk::RETURN)),
                    (5090, Key(vk::RETURN)),
                ];
                v.extend(tour);
                (v, 5400)
            }
            OracleMode::Vs(k) => {
                // As tools/oracle/scripts/battle_vs.s2, with the battlefield
                // picked from the list first (rows 25 pixels apart).
                v.extend([
                    (1000, Key(vk::DOWN)),
                    (1030, Key(vk::DOWN)),
                    (1060, Key(vk::RETURN)),
                    (1260, Click(0, 300, 152)),
                    (1460, Click(0, 455, 152 + 25 * (k as i32 - 1))),
                    (1660, Click(0, 300, 222)),
                    (1860, Click(0, 320, 66)),
                    (2060, Click(0, 300, 292)),
                    (2260, Click(0, 320, 96)),
                    (2460, Click(0, 230, 440)),
                    (2760, Click(0, 320, 240)),
                ]);
                (v, 3360)
            }
        }
    }
}

/// One recorded input.
#[derive(Debug, Clone, Copy)]
enum Act {
    Key(u8),
    Move(i32, i32),
    /// Button (0 left, 1 right) at a point.
    Click(u8, i32, i32),
}

/// Records the inputs of an oracle-check run (see `oracle_check`).
struct Recorder {
    steps: Vec<String>,
    first: Option<u64>,
    last: u64,
    queue: std::collections::VecDeque<(u64, Act)>,
    next: u64,
    end: u64,
    shot_every: u64,
    done: bool,
    bursts: usize,
}

/// Frames between two looks of the recorder at the screen.
const ORACLE_PERIOD: u64 = 240;

impl Recorder {
    fn new(mode: OracleMode, end: u64, shot_every: u64) -> Recorder {
        let (prefix, start) = mode.prefix();
        Recorder {
            steps: Vec::new(),
            first: None,
            last: 0,
            queue: prefix.into_iter().collect(),
            next: start,
            end,
            shot_every: shot_every.max(1),
            done: false,
            bursts: 0,
        }
    }

    /// Steps run at frame `t` (relative to the first frame), as s2run counts.
    fn at(&mut self, t: u64) {
        if t > self.last {
            self.steps.push(format!("frames {}", t - self.last));
            self.last = t;
        }
    }

    fn on_frame(&mut self, info: &s2engine::sys::FlipInfo, sh: &Shared) {
        let first = *self.first.get_or_insert(info.serial);
        let t = info.serial - first;
        if self.done {
            return;
        }
        if t > 0 && t % self.shot_every == 0 {
            self.at(t);
            self.steps.push(format!("shot f{t:06}"));
        }
        if t >= self.end {
            self.at(t);
            self.steps.push("shot end".into());
            self.done = true;
            sh.request_quit();
            return;
        }
        // The screen never settles (the cursor and the units animate), so the
        // recorder looks every ORACLE_PERIOD frames instead of waiting for idle.
        if self.queue.is_empty() && t >= self.next {
            self.next = t + ORACLE_PERIOD;
            if qa::system_menu_open(info.pixels) {
                // 自動作戰 is the fourth entry; RETURN again confirms (as combat.s2).
                let k = |d: u64, key: u8| (t + d, Act::Key(key));
                self.queue.extend([k(0, vk::DOWN), k(10, vk::DOWN), k(20, vk::DOWN), k(30, vk::RETURN), k(90, vk::RETURN)]);
                self.bursts += 1;
            } else {
                // Esc opens the system menu on the map (a second one when the
                // first only left a unit) and closes windows that wait for a key.
                self.queue.push_back((t, Act::Key(vk::ESCAPE)));
            }
        }
        while self.queue.front().is_some_and(|&(when, _)| when <= t) {
            let (_, a) = self.queue.pop_front().unwrap();
            self.at(t);
            match a {
                Act::Key(k) => {
                    self.steps.push(format!("key {}", key_name(k)));
                    sh.push_input(s2engine::sys::InputEvent::KeyDown(k));
                    sh.push_input(s2engine::sys::InputEvent::KeyUp(k));
                }
                Act::Move(x, y) => {
                    self.steps.push(format!("move {x} {y}"));
                    sh.set_mouse(x, y);
                }
                Act::Click(b, x, y) => {
                    self.steps.push(format!("{} {x} {y}", if b == 0 { "click" } else { "rclick" }));
                    sh.set_mouse(x, y);
                    sh.push_input(s2engine::sys::InputEvent::MouseDown(b));
                    sh.push_input(s2engine::sys::InputEvent::MouseUp(b));
                }
            }
        }
    }
}

fn key_name(k: u8) -> &'static str {
    match k {
        vk::ESCAPE => "ESCAPE",
        vk::RETURN => "RETURN",
        vk::DOWN => "DOWN",
        _ => "SPACE",
    }
}

/// Compares the port's `[rng]` trace with the oracle's watch log: one entry
/// per draw (`pct`, or `rnd(n)`; the original's `rnd(0)` draws nothing).
fn rng_diff(port_log: &Path, oracle_log: &Path) -> Result<String> {
    let port_text = std::fs::read_to_string(port_log).unwrap_or_default();
    let mut port = Vec::new();
    let mut turn = String::from("before the first turn");
    for l in port_text.lines() {
        if let Some(r) = l.strip_prefix("[rng] ") {
            let kind = if r.starts_with("pct") { "pct".to_string() } else { r.split(" ->").next().unwrap_or(r).to_string() };
            port.push((kind, r.to_string(), turn.clone()));
        } else if let Some(t) = l.strip_prefix("[battle] turn ") {
            turn = format!("turn {t}");
        }
    }
    let oracle_text = std::fs::read_to_string(oracle_log).unwrap_or_default();
    let mut orig = Vec::new();
    for l in oracle_text.lines() {
        let Some(i) = l.find("watch FUN_") else { continue };
        let w = &l[i + 10..];
        if w.starts_with("0046d776") {
            orig.push(("pct".to_string(), w.to_string()));
        } else if let Some(args) = w.strip_prefix("0046d7ca(") {
            let n = args.split(',').next().and_then(|v| u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok()).unwrap_or(0) as i32;
            if n != 0 {
                orig.push((format!("rnd({n})"), w.to_string()));
            }
        }
    }
    let mut s = format!("rng draws: port {}, original {}\n", port.len(), orig.len());
    let n = port.len().min(orig.len());
    match (0..n).find(|&i| port[i].0 != orig[i].0) {
        Some(i) => {
            s += &format!("first differing draw #{i} ({}):\n", port[i].2);
            for j in i.saturating_sub(3)..(i + 4).min(n) {
                s += &format!("  #{j}{} port {:<14} | original {}\n", if j == i { "*" } else { " " }, port[j].1, orig[j].1);
            }
        }
        None if port.len() == orig.len() => s += "rng draws identical\n",
        None => s += &format!("rng draws identical for the first {n} (one side stopped earlier)\n"),
    }
    Ok(s)
}

// ---- parent ---------------------------------------------------------------------------

/// A private game directory: every entry of `src` linked, SAVE and the CFG copied.
fn overlay(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        std::fs::remove_dir_all(dst)?;
    }
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let name = e.file_name();
        let upper = name.to_string_lossy().to_ascii_uppercase();
        // Other tools may be creating or deleting files here meanwhile.
        let Ok(from) = e.path().canonicalize() else { continue };
        let to = dst.join(&name);
        if upper == "SAVE" {
            std::fs::create_dir_all(&to)?;
            // Other tools may be writing these files: copy what can be read.
            for f in std::fs::read_dir(&from)?.flatten() {
                if f.path().is_file() {
                    let _ = std::fs::copy(f.path(), to.join(f.file_name()));
                }
            }
        } else if upper.ends_with(".CFG") {
            std::fs::copy(&from, &to).with_context(|| format!("copying {}", from.display()))?;
        } else {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&from, &to).with_context(|| format!("linking {}", from.display()))?;
            #[cfg(windows)]
            {
                let linked = if from.is_dir() {
                    std::os::windows::fs::symlink_dir(&from, &to)
                } else {
                    std::os::windows::fs::symlink_file(&from, &to)
                };
                if linked.is_err() {
                    if from.is_dir() {
                        bail!("cannot link {} (symlinks need developer mode on Windows)", from.display());
                    }
                    std::fs::copy(&from, &to)?;
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct StageRow {
    index: usize,
    stage: u16,
    name: String,
    kind: u8,
    turns: u16,
    code: Option<i32>,
    vms: u64,
    route: u8,
    attempts: usize,
}

#[derive(Debug, Clone, Default)]
struct Problem {
    kind: String,
    stage: u16,
    name: String,
    turn: u16,
    detail: String,
    context: Vec<String>,
}

#[derive(Debug, Default)]
struct RouteResult {
    route: u8,
    rows: Vec<StageRow>,
    problems: Vec<Problem>,
    notes: Vec<String>,
    invariants_ok: usize,
    exit: String,
    wall: Duration,
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let k = format!("{key}=");
    let i = line.find(&k)? + k.len();
    let rest = &line[i..];
    Some(if key == "name" { rest } else { rest.split_whitespace().next().unwrap_or("") })
}

/// How a worker segment ended.
#[derive(Debug, Default)]
struct SegEnd {
    stalled: bool,
}

/// Run one route: the first worker, then (after stalls, up to `--recover`
/// times) workers resuming from the stall's every-turn autosave with the
/// exit cheat.
fn run_route(a: &Args, route: u8, base: &Path, tag: &str) -> Result<RouteResult> {
    let game = base.join(format!("game{route}{tag}"));
    overlay(&a.game, &game).with_context(|| format!("copying the game directory to {}", game.display()))?;
    if a.no_cd {
        // DEVICE=prg,mainland,drive: drive '0' = no CD device, so disc
        // changes are recorded (FUN_0046de06).
        for e in std::fs::read_dir(&game)?.flatten() {
            if e.file_name().to_string_lossy().eq_ignore_ascii_case("SUPERSP.CFG") {
                let t = std::fs::read(e.path())?;
                let t = String::from_utf8_lossy(&t).into_owned();
                let mut out = String::new();
                for l in t.split_inclusive('\n') {
                    match l.strip_prefix("DEVICE=") {
                        Some(v) => {
                            let p: Vec<&str> = v.trim_end().splitn(3, ',').collect();
                            let eol = &l[l.trim_end().len()..];
                            out += &format!("DEVICE={},{},0{eol}", p.first().unwrap_or(&"1"), p.get(1).unwrap_or(&"0"));
                        }
                        None => out += l,
                    }
                }
                std::fs::write(e.path(), out)?;
            }
        }
    }
    if let (Some(f), Some(slot)) = (&a.save_file, a.from_save) {
        std::fs::copy(f, game.join("SAVE").join(format!("UJ{slot:02}.SAV")))?;
    }
    let out = base.join(format!("route{route}{tag}"));
    std::fs::create_dir_all(&out)?;
    let mut log = std::fs::File::create(base.join(format!("route{route}{tag}.log"))).context("creating the log")?;
    let mut r = RouteResult { route, ..Default::default() };
    let t0 = Instant::now();
    let mut seg = a.clone();
    let mut segments = 0;
    loop {
        let end = run_segment(&seg, route, base, &game, &out, &mut log, &mut r)?;
        segments += 1;
        let left = if a.max_stages == 0 { usize::MAX } else { a.max_stages.saturating_sub(r.rows.iter().filter(|x| x.code.is_some()).count()) };
        if !(end.stalled && segments <= a.recover && left > 0) {
            break;
        }
        eprintln!("route {route}: recovering from the stall (segment {})", segments + 1);
        r.notes.push(format!("segment {} resumed from the stall autosave with --exit-cheat", segments + 1));
        seg.from_save = Some(52);
        seg.save_file = None;
        seg.exit_cheat = true;
        seg.max_stages = if a.max_stages == 0 { 0 } else { left };
        seg.index_base = r.rows.len();
    }
    r.wall = t0.elapsed();
    Ok(r)
}

/// The worker executable: a copy of this one taken at start, since other
/// builds of the workspace may replace (unlink) the original meanwhile.
static WORKER_EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn worker_cmd(a: &Args, route: u8, game: &Path, out: &Path) -> Result<Command> {
    let exe = match WORKER_EXE.get() {
        Some(p) => p.clone(),
        None => std::env::current_exe()?,
    };
    let mut cmd = Command::new(exe);
    cmd.arg("--worker")
        .arg("--game")
        .arg(game)
        .arg("--out")
        .arg(out)
        .arg("--route")
        .arg(route.to_string())
        .arg("--seed")
        .arg(a.seed.to_string())
        .arg("--max-stages")
        .arg(a.max_stages.to_string())
        .arg("--retries")
        .arg(a.retries.to_string())
        .arg("--hang-minutes")
        .arg(a.hang_minutes.to_string())
        .arg("--difficulty")
        .arg(a.difficulty.to_string())
        .arg("--index-base")
        .arg(a.index_base.to_string())
        .env("S2_BATTLE_TRACE", "1")
        .env("RUST_BACKTRACE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(s) = a.from_save {
        cmd.arg("--from-save").arg(s.to_string());
    }
    for (on, flag) in [
        (a.random_choices, "--random-choices"),
        (a.passive, "--passive"),
        (a.god, "--god"),
        (a.exit_cheat, "--exit-cheat"),
        (a.reseed, "--reseed"),
        (a.caves, "--caves"),
        (a.weak_enemies, "--weak-enemies"),
        (a.boss_rush, "--boss-rush"),
    ] {
        if on {
            cmd.arg(flag);
        }
    }
    if a.enhanced_ai {
        cmd.arg("--enemy-ai").arg("enhanced");
    }
    for (st, f, v) in &a.set_flags {
        cmd.arg("--set-flag").arg(format!("{st}:{f}={v}"));
    }
    for (st, k) in &a.choices {
        cmd.arg("--choice").arg(format!("{st}:{k}"));
    }
    Ok(cmd)
}

fn run_segment(a: &Args, route: u8, base: &Path, game: &Path, out: &Path, log: &mut std::fs::File, r: &mut RouteResult) -> Result<SegEnd> {
    let mut child = worker_cmd(a, route, game, out)?.spawn()?;
    let mut stdin = child.stdin.take();
    let stderr = child.stderr.take().context("child stderr")?;
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for l in BufReader::new(stderr).lines() {
            match l {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let base_index = r.rows.len();
    let mut cur: Option<StageRow> = None;
    let mut turn: u16 = 0;
    let mut last_attack_turn: u16 = 0;
    let mut recent: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut panic_lines: Vec<String> = Vec::new();
    let mut in_panic = false;
    let mut killed: Option<String> = None;
    let mut stalled = false;
    let mut wiped = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(a.timeout_secs)) {
            Ok(line) => {
                writeln!(log, "{line}")?;
                let is_qa = line.starts_with("QA ");
                if !is_qa && !line.starts_with("[battle]") || line.contains("panicked at") {
                    if line.contains("panicked at") {
                        in_panic = true;
                    }
                    if in_panic && panic_lines.len() < 60 {
                        panic_lines.push(line.clone());
                    }
                }
                if !in_panic {
                    recent.push_back(line.clone());
                    if recent.len() > 12 {
                        recent.pop_front();
                    }
                }
                if let Some(rest) = line.strip_prefix("[battle] turn ") {
                    if let Some(n) = rest.split_whitespace().next().and_then(|v| v.parse::<u16>().ok()) {
                        // A new stage, or a segment resumed mid-stage.
                        if n < turn || turn == 0 {
                            last_attack_turn = n;
                        }
                        turn = n;
                        if let Some(c) = cur.as_mut() {
                            c.turns = n;
                        }
                        if killed.is_none() && !wiped && a.stall_turns > 0 && turn.saturating_sub(last_attack_turn) as u32 >= a.stall_turns {
                            // No player unit left (the stage's lose codes name someone
                            // else): the original plays on to turn 100, a defeat.
                            if player_units_on_map(game) == Some(0) {
                                wiped = true;
                                let stage = cur.as_ref().map_or(0, |c| c.stage);
                                r.notes.push(format!(
                                    "stage {stage}: the player army was wiped out by turn {turn} without a lose code firing; the battle runs on to turn 100 (defeat), as in the original"
                                ));
                                continue;
                            }
                            killed = Some(format!("STALL: {} turns without an attack", turn - last_attack_turn));
                            stalled = true;
                            let name = format!("stall_{}_t{turn}", cur.as_ref().map_or(0, |c| c.stage));
                            snap_then_kill(&mut stdin, &rx, log, &mut child, &name)?;
                        }
                    }
                } else if line.starts_with("[battle] attack") || line.starts_with("[battle] cast") {
                    last_attack_turn = turn;
                } else if is_qa {
                    let body = &line[3..];
                    let tag = body.split_whitespace().next().unwrap_or("");
                    match tag {
                        "STAGE-START" => {
                            turn = 0;
                            last_attack_turn = 0;
                            wiped = false;
                            let stage: u16 = field(body, "stage").and_then(|v| v.parse().ok()).unwrap_or(0);
                            let same = r.rows.last().is_some_and(|p| p.stage == stage && matches!(p.code, Some(2) | Some(-1) | None));
                            let attempts = if same { r.rows.last().map_or(1, |p| p.attempts + 1) } else { 1 };
                            cur = Some(StageRow {
                                index: field(body, "index").and_then(|v| v.parse::<usize>().ok()).unwrap_or(base_index),
                                stage,
                                name: field(body, "name").unwrap_or("").to_string(),
                                kind: field(body, "kind").and_then(|v| v.parse().ok()).unwrap_or(0),
                                route,
                                attempts,
                                ..Default::default()
                            });
                            eprintln!("route {route}: stage {stage} {} (attempt {attempts})", field(body, "name").unwrap_or(""));
                        }
                        "STAGE-END" => {
                            if let Some(mut c) = cur.take() {
                                c.code = field(body, "code").and_then(|v| v.parse().ok());
                                c.turns = field(body, "turns").and_then(|v| v.parse().ok()).unwrap_or(c.turns);
                                c.vms = field(body, "vms").and_then(|v| v.parse().ok()).unwrap_or(0);
                                c.route = field(body, "route").and_then(|v| v.parse().ok()).unwrap_or(route);
                                r.rows.push(c);
                            }
                        }
                        "HANG" => {
                            r.problems.push(Problem {
                                kind: "HANG (no new frame)".into(),
                                stage: cur.as_ref().map_or(0, |c| c.stage),
                                name: cur.as_ref().map_or(String::new(), |c| c.name.clone()),
                                turn,
                                detail: body.to_string(),
                                context: recent.iter().cloned().collect(),
                            });
                        }
                        "EXIT" => r.exit = body.to_string(),
                        "PROTAGONIST-DIED" => {
                            if let Some(last) = r.rows.last_mut() {
                                last.code = Some(-1);
                            }
                        }
                        "INVARIANT-FAIL" | "DISC" | "EXIT-CHEAT" | "CAMPAIGN-DONE" => r.notes.push(body.to_string()),
                        "INVARIANTS" => r.invariants_ok += 1,
                        _ => {}
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                killed = Some(format!("HANG (no output for {} s real time)", a.timeout_secs));
                let name = format!("timeout_{}_t{turn}", cur.as_ref().map_or(0, |c| c.stage));
                snap_then_kill(&mut stdin, &rx, log, &mut child, &name)?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait()?;
    let stage = cur.as_ref().map_or(0, |c| c.stage);
    let name = cur.as_ref().map_or(String::new(), |c| c.name.clone());
    if let Some(mut c) = cur.take() {
        c.code = None;
        r.rows.push(c);
    }
    let first_new = r.problems.len();
    let problem_state = |what: &str| dump_autosave(game, &base.join(format!("route{route}_{what}_stage{stage}_t{turn}.sav")));
    if !panic_lines.is_empty() {
        let head = panic_lines.iter().take(3).cloned().collect::<Vec<_>>().join(" | ");
        let ours: Vec<String> = panic_lines.iter().filter(|l| l.contains("s2engine::") || l.contains("s2data::")).take(12).cloned().collect();
        let st = problem_state("crash");
        r.problems.push(Problem { kind: "CRASH (panic)".into(), stage, name: name.clone(), turn, detail: head, context: ours.into_iter().chain(recent.iter().cloned()).chain(st).collect() });
    }
    if let Some(k) = killed {
        let st = problem_state("killed");
        r.problems.push(Problem { kind: k, stage, name, turn, detail: String::new(), context: recent.iter().cloned().chain(st).collect() });
    }
    for p in r.problems[..first_new].iter_mut().filter(|p| p.kind.starts_with("HANG (no new") && !p.context.iter().any(|c| c.starts_with("autosave"))) {
        let st = dump_autosave(game, &base.join(format!("route{route}_hang_stage{}_t{}.sav", p.stage, p.turn)));
        p.context.extend(st);
    }
    if r.exit.is_empty() || !r.exit.starts_with("EXIT") {
        r.exit = format!("status {status}");
    }
    Ok(SegEnd { stalled })
}

/// `--roundtrip`: resume from the turn-1 autosave of record `k` with the
/// same seed and compare the next `n` stages with the full run.
fn roundtrip(a: &Args, base: &Path, full: &RouteResult, k: usize) -> Result<String> {
    let route = full.route;
    // The first battle at or after record k that was a first attempt (a
    // retry's seed depends on the attempt) and completed.
    let Some(k) = (k..full.rows.len()).find(|&i| full.rows[i].kind == 1 && full.rows[i].attempts == 1 && full.rows[i].code.is_some()) else {
        return Ok(format!("route {route} #{k}: no battle record to resume from"));
    };
    let row = &full.rows[k];
    let save = base.join(format!("route{route}")).join(format!("turn1_{k:03}_{:03}.sav", row.stage));
    if !save.exists() {
        return Ok(format!("route {route} #{k}: {} missing", save.display()));
    }
    let n = (k..full.rows.len()).take_while(|&i| full.rows[i].code.is_some()).count().min(4);
    let mut b = a.clone();
    b.from_save = Some(51);
    b.save_file = Some(save);
    b.max_stages = n;
    b.recover = 0;
    let rt = run_route(&b, route, base, &format!("_rt{k}"))?;
    let want: Vec<(u16, Option<i32>, u16)> = full.rows[k..k + n].iter().map(|x| (x.stage, x.code, x.turns)).collect();
    let got: Vec<(u16, Option<i32>, u16)> = rt.rows.iter().take(n).map(|x| (x.stage, x.code, x.turns)).collect();
    Ok(if want == got {
        format!("route {route} #{k} stage {}: resumed run repeats {n} stages exactly {got:?}", row.stage)
    } else {
        format!("route {route} #{k} stage {}: MISMATCH full {want:?} resumed {got:?}", row.stage)
    })
}

/// Ask the worker for a screenshot (answered within a few frames unless it
/// is spinning without presenting), then kill it.
fn snap_then_kill(
    stdin: &mut Option<std::process::ChildStdin>,
    rx: &mpsc::Receiver<String>,
    log: &mut std::fs::File,
    child: &mut std::process::Child,
    name: &str,
) -> Result<()> {
    if let Some(i) = stdin.as_mut() {
        let _ = writeln!(i, "snap {name}");
        let _ = i.flush();
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(l) => {
                    writeln!(log, "{l}")?;
                    if l.starts_with("QA SNAP") {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    Ok(())
}

/// Living player units on the map in the every-turn autosave.
fn player_units_on_map(game_dir: &Path) -> Option<usize> {
    let dir = s2data::GameDir::open(game_dir).ok()?;
    let n_items = s2data::tables::parse_items(&dir.read("itemsp.bin").ok()?).ok()?.len();
    let b = std::fs::read(dir.resolve("SAVE\\UJ52.SAV").ok()?).ok()?;
    let sf = s2data::save::SaveFile::parse(&b, n_items).ok()?;
    let mut n = 0;
    for rec in &sf.player.units {
        let mut raw = [0u8; s2engine::battle::unit::UNIT_SIZE];
        let k = rec.0.len().min(raw.len());
        raw[..k].copy_from_slice(&rec.0[..k]);
        if s2engine::battle::unit::Unit::from_bytes(&raw).on_map() {
            n += 1;
        }
    }
    Some(n)
}

/// The every-turn autosave (slot 51, UJ52.SAV) of the worker's game copy:
/// stage, turn and every living unit on the map, for the problem report;
/// the file is kept next to the summary for `--from-save` repros.
fn dump_autosave(game_dir: &Path, keep: &Path) -> Vec<String> {
    let go = || -> Result<Vec<String>> {
        let dir = s2data::GameDir::open(game_dir)?;
        let n_items = s2data::tables::parse_items(&dir.read("itemsp.bin")?)?.len();
        let path = dir.resolve("SAVE\\UJ52.SAV")?;
        let b = std::fs::read(&path)?;
        std::fs::copy(&path, keep)?;
        let sf = s2data::save::SaveFile::parse(&b, n_items)?;
        let turn = u32::from_le_bytes(sf.globals_c[0xc..0x10].try_into().unwrap());
        let mut out = vec![format!("autosave (kept as {}): stage {} turn {turn}", keep.display(), sf.stage())];
        for (label, army) in [("P", &sf.player), ("E", &sf.enemy)] {
            for rec in &army.units {
                let mut raw = [0u8; s2engine::battle::unit::UNIT_SIZE];
                let n = rec.0.len().min(raw.len());
                raw[..n].copy_from_slice(&rec.0[..n]);
                let u = s2engine::battle::unit::Unit::from_bytes(&raw);
                if !u.on_map() {
                    continue;
                }
                out.push(format!(
                    "  {label} {} char {} class {} hp {}/{} at ({},{}) ai {}/{}/{} npc {} rank {} ap {}",
                    u.name_str().trim_end_matches('\0'),
                    u.char_id,
                    u.kind,
                    u.hp,
                    u.hp_max,
                    u.x,
                    u.y,
                    u.ai_mode,
                    u.ai_sub,
                    u.ai_state,
                    u.npc,
                    u.rank,
                    u.ap
                ));
            }
        }
        Ok(out)
    };
    go().unwrap_or_else(|e| vec![format!("autosave unreadable: {e:#}")])
}

fn code_name(c: Option<i32>) -> String {
    match c {
        Some(1) => "victory".into(),
        Some(2) => "defeat".into(),
        Some(0xe) => "cave".into(),
        Some(0xf) => "retreat".into(),
        Some(0x17) => "campaign done".into(),
        Some(-1) => "won, hero dead".into(),
        Some(c) => format!("code {c:#x}"),
        None => "(unfinished)".into(),
    }
}

fn summary(results: &[RouteResult]) -> String {
    let mut s = String::new();
    for r in results {
        s += &format!("== route {} ({}, {:.0} s wall)\n", r.route, r.exit, r.wall.as_secs_f64());
        s += "  #   stage  kind  turns  result          vtime   try  name\n";
        for row in &r.rows {
            s += &format!(
                "  {:<3} {:<6} {:<5} {:<6} {:<15} {:>6.0}s  {:<4} {}\n",
                row.index,
                row.stage,
                row.kind,
                row.turns,
                code_name(row.code),
                row.vms as f64 / 1000.0,
                row.attempts,
                row.name
            );
        }
        let furthest = r.rows.iter().filter(|x| x.code == Some(1)).last();
        s += &format!(
            "  furthest won stage: {}\n",
            furthest.map_or("none".to_string(), |x| format!("#{} stage {} {}", x.index, x.stage, x.name))
        );
        let distinct: std::collections::BTreeSet<u16> = r.rows.iter().filter(|x| x.code.is_some()).map(|x| x.stage).collect();
        s += &format!("  distinct stages played: {}; invariant checks passed: {}\n", distinct.len(), r.invariants_ok);
        for n in &r.notes {
            s += &format!("  -- {n}\n");
        }
        for p in &r.problems {
            s += &format!("  !! {} at stage {} {} turn {}: {}\n", p.kind, p.stage, p.name, p.turn, p.detail);
            for c in &p.context {
                s += &format!("       {c}\n");
            }
        }
    }
    s
}

fn main() -> Result<()> {
    let a = parse_args()?;
    if a.worker {
        return worker(&a);
    }
    let base = a.out.join(&a.run);
    std::fs::create_dir_all(&base)?;
    if let Some(save) = &a.oracle_check {
        return oracle_check(&a, save, &base);
    }
    let exe = base.join(if cfg!(windows) { "s2auto-worker.exe" } else { "s2auto-worker" });
    std::fs::copy(std::env::current_exe()?, &exe).context("copying the worker executable")?;
    let _ = WORKER_EXE.set(exe);
    let mut results = Vec::new();
    for &route in &a.routes {
        eprintln!("route {route}: starting ({})", base.display());
        results.push(run_route(&a, route, &base, "")?);
    }
    let mut s = summary(&results);
    if !a.roundtrip.is_empty() {
        s += "== save/load round trips\n";
        for r in &results {
            for &k in &a.roundtrip {
                s += &format!("  {}\n", roundtrip(&a, &base, r, k)?);
            }
        }
    }
    std::fs::write(base.join("summary.txt"), &s)?;
    print!("{s}");
    Ok(())
}
