//! Stage scenarios (SUPERSP.BIN entry 1): stage headers, deployment, win/lose
//! conditions, rewards, and the event table that drives story dialogue.
//!
//! There is no bytecode VM. Each stage carries a table of fixed 20-byte
//! *event records*; the engine polls them at fixed points (turn start,
//! after every action, on unit death, before a battle, after victory) and a
//! record fires when its condition holds. Firing shows the attached
//! dialogue lines, clears the record's type byte (one-shot), and sets an
//! optional story flag. Flags 1..=80 live at `DAT_00494507 + n`; 1..=40
//! are cleared at the end of every stage, 41..=80 persist for the campaign.
//!
//! See docs/formats-text.md for the complete event table.

use crate::archive::{Archives, Kind};
use crate::text::{cstr, display};
use crate::{format_err, le, Result};

pub const BIN_STAGES: usize = 1;
pub const HEADER_LEN: usize = 0x1e;
pub const UNIT_LEN: usize = 0x65;
pub const EVENT_LEN: usize = 0x14;
pub const LINE_LEN: usize = 0x17;
/// Highest story flag number.
pub const MAX_FLAG: i16 = 80;

/// 30-byte per-stage header, copied by the game to `DAT_00494688`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageHeader {
    /// Offset and size of the stage block within BIN entry 1.
    pub offset: u32,
    pub size: u32,
    /// +0x08 number of deployment records.
    pub unit_count: u8,
    /// +0x09 battle map id (PRG entry 6 sub-entry, `FUN_00429336`).
    pub map: u8,
    /// +0x0a music track at stage start (`FUN_004198d4`).
    pub music: u16,
    /// +0x0c next stage id after a normal clear. Only the low byte is read
    /// (`mov al, [0x494694]`); +0x0d is a separate value, see `byte_0d`.
    pub next: u16,
    /// +0x0d: not part of `next` (stage 155 has a nonzero value here).
    pub byte_0d: u8,
    /// +0x0e unused by the exe.
    pub unused_0e: u16,
    /// +0x10 / +0x11 stage reached through the hidden exit tiles 0xff / 0xfe (0 = none).
    pub secret_next: [u8; 2],
    /// +0x12..=0x14 initial values of story flags 1..=3.
    pub init_flags: [u8; 3],
    /// +0x15 stage kind: 1 battle (counts as a battle), 2 branch point, 3 no battle phase.
    pub kind: u8,
    /// +0x16 chapter / location number shown on the world map (not read by the exe's stage code).
    pub chapter: u8,
    /// +0x17 / +0x18 enemy level scaling: base party level and step (`FUN_0040d729`).
    pub scale_base: u8,
    pub scale_step: u8,
    /// +0x19 number of reward groups at the start of the block.
    pub reward_groups: u8,
    /// +0x1a offset of the deployment table within the block.
    pub units_offset: u32,
}

impl StageHeader {
    pub fn parse(b: &[u8]) -> Result<StageHeader> {
        let byte = |o: usize| b.get(o).copied().ok_or_else(|| format_err("stage header", "truncated"));
        Ok(StageHeader {
            offset: le::u32(b, 0)?,
            size: le::u32(b, 4)?,
            unit_count: byte(8)?,
            map: byte(9)?,
            music: le::u16(b, 0x0a)?,
            next: byte(0x0c)? as u16,
            byte_0d: byte(0x0d)?,
            unused_0e: le::u16(b, 0x0e)?,
            secret_next: [byte(0x10)?, byte(0x11)?],
            init_flags: [byte(0x12)?, byte(0x13)?, byte(0x14)?],
            kind: byte(0x15)?,
            chapter: byte(0x16)?,
            scale_base: byte(0x17)?,
            scale_step: byte(0x18)?,
            reward_groups: byte(0x19)?,
            units_offset: le::u32(b, 0x1a)?,
        })
    }
}

/// Reward group at the start of a stage block (shown on the result screen,
/// `FUN_0045cfad`; treasure spots copied by `FUN_00422a97`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reward {
    /// 1: these characters join the party.
    Join(Vec<i16>),
    /// 2: these characters leave.
    Leave(Vec<i16>),
    /// 3: gold.
    Gold(i16),
    /// 4: items.
    Items(Vec<i16>),
    /// 5: treasure spots on the map, 4 shorts each (at most 20; win codes 1..=20 refer to them).
    Treasure(Vec<[i16; 4]>),
    Unknown(i16, Vec<i16>),
}

/// One 0x65-byte deployment record (the full layout is decoded by the
/// army code, `FUN_0043f42c`/`FUN_0043d9e9`).
#[derive(Debug, Clone)]
pub struct Deploy {
    /// Character id (0 = generic soldier).
    pub char_id: i16,
    /// Name, Big5 (14 bytes).
    pub name: Vec<u8>,
    /// +0x10 class (兵種) id used when the character is not already in an army.
    pub class: i16,
    /// +0x17 / +0x19 map position.
    pub x: i16,
    pub y: i16,
    pub raw: Vec<u8>,
}

/// One dialogue line (0x17 bytes).
#[derive(Debug, Clone)]
pub struct Line {
    /// Speaker character id: picks the portrait (CEL entry 3, frame
    /// `id*2-2`) and the unit the box is placed next to.
    pub speaker: i16,
    /// Music track to start before / after the line (0 = none).
    pub music_before: i16,
    pub music_after: i16,
    /// Speaker name shown under the portrait.
    pub name: Vec<u8>,
    /// Text with markup (see [`crate::msg::parse_dialogue`]).
    pub text: Vec<u8>,
}

/// One 20-byte event record.
#[derive(Debug, Clone)]
pub struct Event {
    /// +0x00 event type (1..=16); the game zeroes it once fired.
    pub kind: u8,
    /// +0x01 subtype.
    pub sub: i16,
    /// +0x03 / +0x05 / +0x07 / +0x09 operands. For most types `a` is the
    /// story flag that must be set before the condition is even checked.
    pub a: i16,
    pub b: i16,
    pub c: i16,
    pub d: i16,
    /// +0x0b story flag to set when the event fires (1..=80; 0 = none).
    pub set_flag: i8,
    /// +0x0c offset of the line table from the script start.
    pub lines_offset: u32,
    /// +0x10 total size of the line table plus its text pool.
    pub data_size: u16,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone)]
pub struct Stage {
    pub id: usize,
    pub header: StageHeader,
    pub rewards: Vec<Reward>,
    pub units: Vec<Deploy>,
    pub name: Vec<u8>,
    /// Briefing narration shown before the stage.
    pub briefing: Vec<u8>,
    pub win_text: Vec<u8>,
    pub lose_text: Vec<u8>,
    /// Two alternative win-condition sets (all codes in a set must hold).
    pub win_sets: [Vec<i16>; 2],
    /// Lose codes (any one loses).
    pub lose_codes: Vec<i16>,
    pub events: Vec<Event>,
    /// Bytes of the script area beyond what the event table references.
    pub script_len: usize,
}

fn text_at(b: &[u8], off: usize, len: usize) -> Result<Vec<u8>> {
    b.get(off..off + len).map(|s| cstr(s).to_vec()).ok_or_else(|| format_err("stage", format!("text {off:#x}+{len} out of range")))
}

impl Stage {
    /// Parse stage `id` (1-based) from the whole BIN entry 1.
    pub fn parse(bin1: &[u8], id: usize) -> Result<Stage> {
        let count = le::i16(bin1, 0)?.max(0) as usize;
        if id == 0 || id > count {
            return Err(format_err("stages", format!("stage {id} out of range 1..={count}")));
        }
        let hb = bin1.get(2 + HEADER_LEN * (id - 1)..2 + HEADER_LEN * id).ok_or_else(|| format_err("stages", "header table truncated"))?;
        let header = StageHeader::parse(hb)?;
        let blk = bin1
            .get(header.offset as usize..(header.offset + header.size) as usize)
            .ok_or_else(|| format_err("stages", format!("stage {id} block out of range")))?;

        // Rewards: (i16 kind, i16 count)[groups], then the shorts they consume.
        let groups = header.reward_groups as usize;
        let mut vals = 4 * groups;
        let mut rewards = Vec::with_capacity(groups);
        for g in 0..groups {
            let kind = le::i16(blk, 4 * g)?;
            let n = le::i16(blk, 4 * g + 2)?.max(0) as usize;
            let take = |k: usize| (0..k).map(|i| le::i16(blk, vals + 2 * i)).collect::<Result<Vec<_>>>();
            let r = match kind {
                1 => Reward::Join(take(n)?),
                2 => Reward::Leave(take(n)?),
                3 => Reward::Gold(take(1)?[0]),
                4 => Reward::Items(take(n)?),
                5 => Reward::Treasure(take(n)?.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()),
                _ => Reward::Unknown(kind, take(n)?),
            };
            vals += 2 * if kind == 3 { 1 } else { n };
            rewards.push(r);
        }

        let mut p = header.units_offset as usize;
        let mut units = Vec::with_capacity(header.unit_count as usize);
        for _ in 0..header.unit_count {
            let r = blk.get(p..p + UNIT_LEN).ok_or_else(|| format_err("stage", "deployment table truncated"))?;
            units.push(Deploy {
                char_id: le::i16(r, 0)?,
                name: cstr(&r[2..0x10]).to_vec(),
                class: le::i16(r, 0x10)?,
                x: le::i16(r, 0x17)?,
                y: le::i16(r, 0x19)?,
                raw: r.to_vec(),
            });
            p += UNIT_LEN;
        }

        // Seven u16 section lengths (`DAT_004944fc`), then the sections.
        let lens: Vec<usize> = (0..7).map(|i| le::u16(blk, p + 2 * i).map(|v| v as usize)).collect::<Result<_>>()?;
        p += 14;
        let name = text_at(blk, p, lens[0])?;
        p += lens[0];
        let briefing = text_at(blk, p, lens[1])?;
        p += lens[1];
        let win_text = text_at(blk, p, lens[2])?;
        p += lens[2];
        let shorts = |p: usize, n: usize| (0..n).map(|i| le::i16(blk, p + 2 * i)).collect::<Result<Vec<_>>>();
        let win_sets = [shorts(p, lens[4])?, shorts(p + 2 * lens[4], lens[5])?];
        p += 2 * (lens[4] + lens[5]);
        let lose_text = text_at(blk, p, lens[3])?;
        p += lens[3];
        let lose_codes = shorts(p, lens[6])?;
        p += 2 * lens[6];

        let script = &blk[p.min(blk.len())..];
        let events = parse_events(script)?;
        Ok(Stage { id, header, rewards, units, name, briefing, win_text, lose_text, win_sets, lose_codes, events, script_len: script.len() })
    }

    /// Parse every stage of BIN entry 1.
    pub fn parse_all(bin1: &[u8]) -> Result<Vec<Stage>> {
        let count = le::i16(bin1, 0)?.max(0) as usize;
        (1..=count).map(|id| Stage::parse(bin1, id)).collect()
    }

    pub fn load_all(arcs: &Archives) -> Result<Vec<Stage>> {
        Stage::parse_all(&arcs.read(Kind::Bin, BIN_STAGES)?)
    }
}

/// Parse the script area: `i16 count, u16 size, Event[count]`, then line
/// tables and text pools addressed from the script start.
pub fn parse_events(script: &[u8]) -> Result<Vec<Event>> {
    if script.len() < 4 {
        return Ok(Vec::new());
    }
    let n = le::i16(script, 0)?.max(0) as usize;
    (0..n)
        .map(|i| {
            let r = 4 + EVENT_LEN * i;
            let rec = script.get(r..r + EVENT_LEN).ok_or_else(|| format_err("events", "event table truncated"))?;
            let nlines = le::i16(rec, 0x12)?.max(0) as usize;
            let lines_offset = le::u32(rec, 0x0c)?;
            let data_size = le::u16(rec, 0x10)?;
            let pool = lines_offset as usize + LINE_LEN * nlines;
            let lines = (0..nlines)
                .map(|k| {
                    let l = lines_offset as usize + LINE_LEN * k;
                    let lr = script.get(l..l + LINE_LEN).ok_or_else(|| format_err("events", "line table truncated"))?;
                    let toff = le::i16(lr, 0x13)?.max(0) as usize;
                    let tlen = le::i16(lr, 0x15)?.max(0) as usize;
                    Ok(Line {
                        speaker: le::i16(lr, 0)?,
                        music_before: le::i16(lr, 2)?,
                        music_after: le::i16(lr, 4)?,
                        name: cstr(&lr[6..0x13]).to_vec(),
                        text: text_at(script, pool + toff, tlen)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Event {
                kind: rec[0],
                sub: le::i16(rec, 1)?,
                a: le::i16(rec, 3)?,
                b: le::i16(rec, 5)?,
                c: le::i16(rec, 7)?,
                d: le::i16(rec, 9)?,
                set_flag: rec[0x0b] as i8,
                lines_offset,
                data_size,
                lines,
            })
        })
        .collect()
}

/// Human-readable meaning of a win code (`FUN_00433257`).
pub fn describe_win_code(code: i16) -> String {
    match code {
        0 => "all enemies defeated".into(),
        1..=20 => format!("treasure spot {code} taken"),
        0x1f => "protagonist reaches the exit tile (map attr 6) with flag 1 set".into(),
        0x20 => "any active non-leader unit reaches the exit tile".into(),
        0x21 => "all enemy leaders (unit+0x2f == 1) defeated".into(),
        0x22 => "all enemy non-leaders defeated".into(),
        _ => format!("code {code} (never true)"),
    }
}

/// Human-readable meaning of a lose code (`FUN_00433161`); turn 100 always loses.
pub fn describe_lose_code(code: i16) -> String {
    match code {
        0 => "every unit with +8 == 1 (main party) is down".into(),
        1..=0x2b => format!("character {code} dies"),
        101..=190 => format!("turn {} reached", code - 100),
        _ => format!("code {code} (never true)"),
    }
}

fn flag_cond(a: i16) -> String {
    format!("[flag {a}]")
}

/// One-line description of an event record, for listings. See the event
/// table in docs/formats-text.md for exact semantics.
pub fn describe_event(e: &Event) -> String {
    let (s, a, b, c, d) = (e.sub, e.a, e.b, e.c, e.d);
    let f = flag_cond(a);
    let what = match e.kind {
        0 => "(consumed)".to_string(),
        1 => format!("TurnStart turn={a}{}", if s != 0 { format!(" (sub {s}: never fires)") } else { String::new() }),
        2 => format!("Victory set={}", if s == 0 { "any".to_string() } else { s.to_string() }),
        3 => match s {
            1 => "EnemyWiped (after enemy phase, all enemies down)".to_string(),
            2 => format!("UnitDown char={a} (last words)"),
            3 => format!("BattleTalk attacker={a} defender={b}"),
            _ => format!("Unit sub={s} (never fires)"),
        },
        4 => format!("Adjacent char={a} char={b}{}", if s != 0 { " (sub != 0: never fires)" } else { "" }),
        5 => match s {
            0 => format!("{f} Choice (line 0 = prompt, lines 1.. = options)"),
            1..=4 => format!("{f} ChoiceResult == {s} (clears flag {a})"),
            _ => format!("{f} Choice sub={s} (never fires)"),
        },
        6 => match s {
            0 => format!("{f} Deploy chars {b},{c},{d} (animated)"),
            1 => format!("{f} Withdraw chars {b},{c},{d} (animated)"),
            2 => format!("{f} Deploy chars {b},{c},{d} (silent)"),
            3 => format!("{f} Withdraw chars {b},{c},{d} (silent)"),
            _ => format!("{f} Deploy sub={s} (no-op, fires)"),
        },
        7 => match s {
            0 => format!("{f} Move char={b} to ({c},{d})"),
            1 => format!("{f} Move char={b} to ({c},{d}) mode 1"),
            2 => format!("{f} Move char={b} to char {c}"),
            3 => format!("{f} Move char={b} to char {c} mode 1"),
            4 => format!("{f} Warp char={b} to ({c},{d})"),
            5 => format!("{f} Warp char={b} to char {c}"),
            _ => format!("{f} Move sub={s} (fires, no effect)"),
        },
        8 => match s {
            0 => format!("{f} Camera center ({b},{c})"),
            1 => format!("{f} Camera scroll to ({b},{c})"),
            2 => format!("{f} Camera center on char {b}"),
            3 => format!("{f} Camera scroll to char {b}"),
            4 => format!("{f} Camera center ({b},{c}) or char {b}, then flash"),
            5 => format!("{f} Effect anim {b} on char {c}"),
            6 => format!("{f} Effect anim {b} at ({c},{d})"),
            7 => format!("{f} Wait {b}"),
            8 => format!("{f} Redraw"),
            _ => format!("{f} Camera sub={s} (fires, no effect)"),
        },
        9 => match s {
            0 => format!("{f} FlagSet (dialogue chained after other events)"),
            1 => format!("{f} AllFlags {b},{c},{d}"),
            2 => format!("{f} AllFlags {b},{c},{d} then clear them"),
            3 => format!("{f} AnyFlag {b},{c},{d}"),
            4 => format!("{f} AnyFlag {b},{c},{d} then clear them"),
            5 => format!("{f} ClearFlags {b},{c},{d}"),
            6 => format!("{f} SetNextStage {b}"),
            7 => format!("InList {a} (DAT_00492cbc list; no flag check)"),
            _ => format!("{f} Flags sub={s} (never fires)"),
        },
        10 => match s {
            0 => format!("{f} GiveItem item={b} count={c}"),
            1 => format!("{f} PartyHasItem {b}"),
            2 => format!("{f} PartyLacksItem {b}"),
            3 => format!("{f} EnemyHasItem {b}"),
            4 => format!("{f} CharHoldsItem char={c} item={b}"),
            5 => format!("{f} CharLacksItem char={c} item={b}"),
            6 => format!("{f} TakeItem item={b} from char={c} (needs party char {d})"),
            7 => format!("{f} ItemCheck {b} (FUN_00446c6b)"),
            _ => format!("{f} Items sub={s} (fires)"),
        },
        11 => match s {
            0 => format!("{f} CharDead {b}"),
            1 => format!("{f} CharAlive {b}"),
            2 => format!("{f} CharInRoster {b}"),
            3 => format!("{f} CharNotInParty {b}"),
            4 => format!("{f} CharRetreated {b}"),
            5 => format!("{f} CharOnMap {b}"),
            6 => format!("{f} CharIdle {b}"),
            7 => format!("{f} SetCharMode char={b} mode={c}"),
            8 => format!("{f} PartyOp8 {b}"),
            9 => format!("{f} PartyOp9 {b}"),
            10 => format!("{f} PartyOp10"),
            11 => format!("{f} AddFame {b} (名聲, DAT_00497b48)"),
            12 => format!("{f} AddRating {b} (評價, DAT_00497b4c)"),
            13 => format!("{f} PartyOp13 (FUN_00441038)"),
            14 => format!("{f} PartySetup (FUN_00433d8f)"),
            _ => format!("{f} State sub={s} (fires)"),
        },
        12 => format!("{f} UnitAction char={b} op={s} c={c} d={d}{}", unit_action_name(s)),
        13 => match s {
            0 => format!("{f} Difficulty {} {c}", match b { 1 => "<", 2 => "==", 3 => ">", _ => "?" }),
            1 => format!("{f} PlaySound {b}"),
            2 => format!("{f} PlayMusic {b}"),
            3 => format!("{f} ShowPicture {b},{c},{d}"),
            4 => format!("{f} SetAfterPicture {b},{c},{d} (flags 27..29)"),
            5 => format!("{f} PlayMovie {b}"),
            6 => format!("{f} SetAfterMovie {b} (flag 30)"),
            _ => format!("{f} System sub={s} (fires)"),
        },
        14 => match s {
            0 => format!("{f} AddStat char={b} stat={c} += {d}"),
            1 => format!("{f} PartyStat {b},{c} (FUN_0045d9d1)"),
            2 => format!("{f} Stat char={b} stat={c} > {d}"),
            3 => format!("{f} Stat char={b} stat={c} == {d}"),
            4 => format!("{f} Stat char={b} stat={c} < {d}"),
            _ => format!("{f} Stat sub={s} (fires)"),
        },
        15 => match s {
            0 => format!("{f} GroupWalk party count={b}"),
            3 => format!("{f} GroupWalk enemy count={b}"),
            1 | 4 => format!("  walk char={b} to ({c}, facing {d})"),
            2 | 5 => "  end group".to_string(),
            _ => format!("GroupWalk sub={s}"),
        },
        16 => match s {
            0 => format!("{f} GroupMove party count={b} mode={c}"),
            4 => format!("{f} GroupMove enemy count={b} mode={c}"),
            1 | 5 => format!("  move char={b} to ({c},{d})"),
            2 | 6 => format!("  char={b} param={c}"),
            3 | 7 => "  end group".to_string(),
            _ => format!("GroupMove sub={s}"),
        },
        k => format!("Unknown type {k}"),
    };
    let mut out = what;
    if e.set_flag > 0 && e.set_flag as i16 <= MAX_FLAG {
        out.push_str(&format!(" => set flag {}", e.set_flag));
    }
    if !e.lines.is_empty() {
        out.push_str(&format!(" [{} lines]", e.lines.len()));
    }
    out
}

fn unit_action_name(op: i16) -> &'static str {
    match op {
        0 => " (reset to unit, clear state)",
        1 => " (mark retreated)",
        10 => " (face d, step c<<2)",
        11 => " (set +0x2d = 2)",
        12 => " (FUN_0040a443)",
        13 => " (turn to c, 4 steps)",
        14 => " (turn to c, 8 steps)",
        15 => " (turn to c, class step)",
        16 => " (set facing c)",
        17 => " (reset, then defeat)",
        18 => " (defeat)",
        19 => " (reset, then remove)",
        20 => " (remove)",
        21 => " (switch army)",
        22 => " (rejoin)",
        23 => " (damage c)",
        24 => " (damage c% of max)",
        25 => " (set +0x0a = c)",
        26 => " (full restore)",
        27 => " (FUN_0044bfb2 c,d,d)",
        28 => " (FUN_0044bfb2 c,1,d)",
        _ => " (no-op)",
    }
}

/// Text listing of a stage for dumps.
pub fn listing(stage: &Stage) -> String {
    use std::fmt::Write;
    let h = &stage.header;
    let mut s = String::new();
    let _ = writeln!(s, "=== stage {} {}", stage.id, display(&stage.name));
    let _ = writeln!(
        s,
        "map {} music {} next {} secret {:?} kind {} chapter {} flags1-3 {:?} scale {}/{}",
        h.map, h.music, h.next, h.secret_next, h.kind, h.chapter, h.init_flags, h.scale_base, h.scale_step
    );
    let _ = writeln!(s, "briefing: {}", display(&stage.briefing));
    let _ = writeln!(s, "win: {}", display(&stage.win_text));
    for (i, set) in stage.win_sets.iter().enumerate() {
        for c in set {
            let _ = writeln!(s, "  win set {}: {}", i + 1, describe_win_code(*c));
        }
    }
    let _ = writeln!(s, "lose: {}", display(&stage.lose_text));
    for c in &stage.lose_codes {
        let _ = writeln!(s, "  lose: {}", describe_lose_code(*c));
    }
    for r in &stage.rewards {
        let _ = writeln!(s, "reward: {r:?}");
    }
    for u in &stage.units {
        let _ = writeln!(s, "unit char {:>3} class {:>3} at ({:>2},{:>2}) {}", u.char_id, u.class, u.x, u.y, display(&u.name));
    }
    for (i, e) in stage.events.iter().enumerate() {
        let _ = writeln!(s, "ev {i:>2}: {}", describe_event(e));
        for l in &e.lines {
            let music = match (l.music_before, l.music_after) {
                (0, 0) => String::new(),
                (b, a) => format!(" [music {b}->{a}]"),
            };
            let _ = writeln!(s, "        {:>3} {}{}: {}", l.speaker, display(&l.name), music, display(&l.text).replace('\n', "⏎"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(v: i16) -> [u8; 2] {
        v.to_le_bytes()
    }

    #[test]
    fn parses_event_with_lines() {
        // one event: type 1, turn 3, one line by char 7 saying "hi"
        let mut s = vec![];
        s.extend_from_slice(&le16(1));
        s.extend_from_slice(&le16(0));
        let mut ev = vec![0u8; EVENT_LEN];
        ev[0] = 1;
        ev[3..5].copy_from_slice(&le16(3));
        ev[0x0c..0x10].copy_from_slice(&((4 + EVENT_LEN) as u32).to_le_bytes());
        ev[0x10..0x12].copy_from_slice(&((LINE_LEN + 3) as u16).to_le_bytes());
        ev[0x12..0x14].copy_from_slice(&le16(1));
        s.extend_from_slice(&ev);
        let mut line = vec![0u8; LINE_LEN];
        line[0..2].copy_from_slice(&le16(7));
        line[6..8].copy_from_slice(b"Al");
        line[0x15..0x17].copy_from_slice(&le16(3));
        s.extend_from_slice(&line);
        s.extend_from_slice(b"hi\0");
        let evs = parse_events(&s).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!((evs[0].kind, evs[0].a), (1, 3));
        assert_eq!(evs[0].lines[0].speaker, 7);
        assert_eq!(evs[0].lines[0].text, b"hi");
        assert_eq!(evs[0].lines[0].name, b"Al");
        assert!(describe_event(&evs[0]).starts_with("TurnStart turn=3"));
    }
}
