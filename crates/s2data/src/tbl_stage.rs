//! Stage files: BIN entry 1 (campaign, 168 stages) and FSTAGE.BIN (two-player).
//!
//! ```text
//! i16 count
//! StageHeader[count]  (30 bytes each, copied to the globals at 0x494688)
//! stage blocks        (addressed by header.offset/size)
//! ```
//! Loader: `FUN_00422b8e` (campaign) / `FUN_0040f418` (FSTAGE).

use crate::tbl_util::{cstr_at, Rec};
use crate::{format_err, le, Result};

/// 30-byte stage header.
#[derive(Debug, Clone)]
pub struct StageHeader {
    pub offset: u32,
    pub size: u32,
    /// Number of 101-byte unit placement records in the block.
    pub unit_count: u8,
    /// Chapter/stage number shown to the player (byte 9).
    pub number: u8,
    /// Music track (byte 10, passed to the music player).
    pub music: u8,
    /// Default next stage: the *byte* at 0x0c (`FUN_00423033` loads it with
    /// `mov al, [0x494694]`); 0 = none / decided by the script. Byte 0x0d
    /// is a separate value (e.g. 0x0c in stages 141 and 155).
    pub next: i16,
    /// Bytes 0x0e..0x12 (mostly zero; 0x10 holds a stage-specific value in a few stages).
    pub unknown_0e: [u8; 4],
    /// Bytes 0x12..0x15: flags copied to DAT_00494508/9/a when set.
    pub flags: [u8; 3],
    /// Byte 0x15: 1 = battle, 3 = no battle (story only), 2 = other (inferred).
    pub kind: u8,
    /// Byte 0x16. NOT the battle map: the game takes the map from the
    /// scenario header (`script::StageHeader::map`, byte 0x09); the two only
    /// agree on early stages (stage 50: 27 here, map 50 there). Meaning unknown.
    pub map: u8,
    /// Bytes 0x17, 0x18.
    pub unknown_17: [u8; 2],
    /// Number of reward groups at the block start.
    pub reward_groups: u8,
    /// Offset of the unit records within the block.
    pub units_offset: u16,
    pub unknown_1c: [u8; 2],
}

impl StageHeader {
    fn parse(r: Rec) -> Result<StageHeader> {
        Ok(StageHeader {
            offset: r.u32(0)?,
            size: r.u32(4)?,
            unit_count: r.u8(8)?,
            number: r.u8(9)?,
            music: r.u8(10)?,
            next: r.u8(12)? as i16,
            unknown_0e: r.bytes(0x0e, 4)?.try_into().unwrap(),
            flags: r.bytes(0x12, 3)?.try_into().unwrap(),
            kind: r.u8(0x15)?,
            map: r.u8(0x16)?,
            unknown_17: r.bytes(0x17, 2)?.try_into().unwrap(),
            reward_groups: r.u8(0x19)?,
            units_offset: r.u16(0x1a)?,
            unknown_1c: r.bytes(0x1c, 2)?.try_into().unwrap(),
        })
    }
}

/// End-of-stage reward group (戰果報告), `FUN_00459f..` report screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reward {
    /// 夥伴加入: these character ids join.
    Join(Vec<i16>),
    /// 夥伴離開: these character ids leave.
    Leave(Vec<i16>),
    /// 得到金錢.
    Gold(i16),
    /// 得到物品: item ids.
    Items(Vec<i16>),
    /// 搜索寶物: hidden treasure; each entry lists 4 candidate item ids, one is
    /// picked at random (rand % 4) for every unopened treasure spot of that index.
    Treasure(Vec<[i16; 4]>),
    Other(i16, Vec<i16>),
}

/// One unit placement (101 bytes).
#[derive(Debug, Clone)]
pub struct UnitPlacement {
    /// Character id (portrait/sprite; 0 = generic unit). Copied to unit+2.
    pub character: i16,
    pub name: String,
    /// Low nibble: gender (0 male, 1 female). Copied to unit+0x2e.
    pub gender: u8,
    pub class: i16,
    pub level: i16,
    /// AI mode → unit+0x0a (1 attack, 2 standby, ...; 31..=100 wake when unit
    /// m-30 dies, 101..=150 wake on turn m-100 — see docs/ai.md).
    pub ai_mode: i8,
    /// Group tag → unit+0x06.
    pub tag: i8,
    /// AI sub-mode → unit+0x0c.
    pub ai_sub: i8,
    pub x: i16,
    pub y: i16,
    /// HP, MP, ST, 魔力, 反應, 攻擊, 防禦, 劍術, 戰技 (record order 0x1b..0x2c).
    pub stats: [i16; 9],
    /// Alignment override (0 = class default) → unit+0x123.
    pub alignment: i16,
    /// AP override (<= 0 = class default).
    pub ap: i16,
    /// Control type: 0,5,6,7 player-controlled; others AI (unit+0x08 = 1);
    /// 3/9 boss (unit+0x2f = 1), 8/10 sub-boss (= 2); 4 hidden reserve (flag 0x40000000).
    pub control: u8,
    /// Initial facing → unit+0x2b.
    pub facing: u8,
    /// Byte 0x33: first turn of the unit's appear window (`FUN_004231be`).
    pub b33: u8,
    /// Byte 0x34: last turn of the appear window (0 = only turn `b33`).
    pub appear_to: u8,
    /// Bonus EXP given to whoever defeats this unit (→ unit+0x119, `FUN_00453608`).
    pub kill_exp: i16,
    /// Items dropped when defeated (→ unit+0x11b).
    pub drops: [i16; 4],
    /// Six equipment/inventory item ids (0 = empty).
    pub items: [i16; 6],
    /// Unit skill flags → unit+0x115.
    pub skill_flags: u32,
    /// Learned-spell bit masks per school (0x4f..0x65 → unit+0xfb; inferred from
    /// the data: casters have bits set in their schools, 志超 has 劍術/合擊).
    pub spells: [i16; 11],
}

impl UnitPlacement {
    fn parse(r: Rec) -> Result<UnitPlacement> {
        let arr = |off: usize, out: &mut [i16]| -> Result<()> {
            for (k, v) in out.iter_mut().enumerate() {
                *v = r.i16(off + 2 * k)?;
            }
            Ok(())
        };
        let mut stats = [0; 9];
        arr(0x1b, &mut stats)?;
        let mut drops = [0; 4];
        arr(0x37, &mut drops)?;
        let mut items = [0; 6];
        arr(0x3f, &mut items)?;
        let mut spells = [0; 11];
        arr(0x4f, &mut spells)?;
        Ok(UnitPlacement {
            character: r.i16(0)?,
            name: r.str(2, 13)?,
            gender: r.u8(0x0f)? & 0x0f,
            class: r.i16(0x10)?,
            level: r.i16(0x12)?,
            ai_mode: r.i8(0x14)?,
            tag: r.i8(0x15)?,
            ai_sub: r.i8(0x16)?,
            x: r.i16(0x17)?,
            y: r.i16(0x19)?,
            stats,
            alignment: r.i16(0x2d)?,
            ap: r.i16(0x2f)?,
            control: r.u8(0x31)?,
            facing: r.u8(0x32)?,
            b33: r.u8(0x33)?,
            appear_to: r.u8(0x34)?,
            kill_exp: r.i16(0x35)?,
            drops,
            items,
            skill_flags: r.u32(0x4b)?,
            spells,
        })
    }
}

/// A decoded stage block.
#[derive(Debug, Clone)]
pub struct Stage {
    pub header: StageHeader,
    pub rewards: Vec<Reward>,
    pub units: Vec<UnitPlacement>,
    pub title: String,
    pub briefing: String,
    pub victory: String,
    pub defeat: String,
    /// Two i16 arrays after the victory text (lengths from the section header; meaning TBD).
    pub list_a: Vec<i16>,
    pub list_b: Vec<i16>,
    /// Character ids whose death loses the stage.
    pub must_survive: Vec<i16>,
    /// Offset within the block where the event script starts (`FUN_00417556`).
    pub script_offset: usize,
    /// The whole block, for the script interpreter.
    pub block: Vec<u8>,
}

/// Parsed stage file.
#[derive(Debug, Clone)]
pub struct StageFile {
    pub headers: Vec<StageHeader>,
    data: Vec<u8>,
}

impl StageFile {
    pub fn parse(data: Vec<u8>) -> Result<StageFile> {
        let n = le::i16(&data, 0)?.max(0) as usize;
        let headers = (0..n)
            .map(|i| StageHeader::parse(Rec::at(&data, 2 + 30 * i, 30, "stage header")?))
            .collect::<Result<_>>()?;
        Ok(StageFile { headers, data })
    }

    pub fn len(&self) -> usize {
        self.headers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }

    /// Decode stage `id` (1-based). Out-of-range ids fall back to 1 like the game.
    pub fn stage(&self, id: usize) -> Result<Stage> {
        const W: &str = "stage";
        let id = if id == 0 || id > self.headers.len() { 1 } else { id };
        let header = self.headers[id - 1].clone();
        let blk = Rec::at(&self.data, header.offset as usize, header.size as usize, W)?.b;

        let groups: Vec<(i16, usize)> = (0..header.reward_groups as usize)
            .map(|g| Ok((le::i16(blk, 4 * g)?, le::i16(blk, 4 * g + 2)?.max(0) as usize)))
            .collect::<Result<_>>()?;
        let mut p = 4 * groups.len();
        let mut take = |n: usize| -> Result<Vec<i16>> {
            let v = (0..n).map(|k| le::i16(blk, p + 2 * k)).collect::<Result<Vec<_>>>()?;
            p += 2 * n;
            Ok(v)
        };
        let mut rewards = Vec::with_capacity(groups.len());
        for &(kind, count) in &groups {
            let v = take(count)?;
            rewards.push(match kind {
                1 => Reward::Join(v),
                2 => Reward::Leave(v),
                3 => Reward::Gold(v.first().copied().unwrap_or(0)),
                4 => Reward::Items(v),
                5 => Reward::Treasure(v.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()),
                k => Reward::Other(k, v),
            });
        }

        let base = header.units_offset as usize;
        let units = (0..header.unit_count as usize)
            .map(|u| UnitPlacement::parse(Rec::at(blk, base + 101 * u, 101, W)?))
            .collect::<Result<_>>()?;

        let s = base + 101 * header.unit_count as usize;
        let lens: Vec<usize> = (0..7).map(|k| le::u16(blk, s + 2 * k).map(|v| v as usize)).collect::<Result<_>>()?;
        let mut q = s + 14;
        let mut text = |n: usize| -> Result<String> {
            let t = multi_str(Rec::at(blk, q, n, W)?.b);
            q += n;
            Ok(t)
        };
        let title = text(lens[0])?;
        let briefing = text(lens[1])?;
        let victory = text(lens[2])?;
        let shorts = |q: usize, n: usize| (0..n).map(|k| le::i16(blk, q + 2 * k)).collect::<Result<Vec<_>>>();
        let list_a = shorts(q, lens[4])?;
        let list_b = shorts(q + 2 * lens[4], lens[5])?;
        q += 2 * (lens[4] + lens[5]);
        let defeat = multi_str(Rec::at(blk, q, lens[3], W)?.b);
        q += lens[3];
        let must_survive = shorts(q, lens[6])?;
        q += 2 * lens[6];
        if q > blk.len() {
            return Err(format_err(W, format!("stage {id}: sections overrun block")));
        }
        Ok(Stage {
            header,
            rewards,
            units,
            title,
            briefing,
            victory,
            defeat,
            list_a,
            list_b,
            must_survive,
            script_offset: q,
            block: blk.to_vec(),
        })
    }

    /// Title of every stage without decoding units (used by the VS stage menu).
    pub fn titles(&self) -> Result<Vec<String>> {
        (1..=self.len()).map(|i| self.stage(i).map(|s| s.title)).collect()
    }
}

/// A text section may hold several NUL-terminated lines; join them with '\n'.
fn multi_str(b: &[u8]) -> String {
    b.split(|&c| c == 0).filter(|s| !s.is_empty()).map(crate::text::big5).collect::<Vec<_>>().join("\n")
}

/// Read just the NUL-terminated title at the start of the text sections.
pub fn read_title(blk: &[u8], off: usize) -> Result<String> {
    cstr_at(blk, off, "stage title").map(|(s, _)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(v: &mut Vec<u8>, x: i16) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    #[test]
    fn synthetic_stage_roundtrip() {
        // block: 1 reward group (gold 500), 1 unit, texts "T","B","V","D", no lists, survive [7]
        let mut blk = Vec::new();
        le16(&mut blk, 3);
        le16(&mut blk, 1);
        le16(&mut blk, 500);
        let units_off = blk.len();
        let mut u = vec![0u8; 101];
        u[0] = 7;
        u[2] = b'A';
        u[0x10] = 81;
        u[0x12] = 1;
        u[0x17] = 13;
        u[0x19] = 21;
        u[0x3f] = 4;
        blk.extend_from_slice(&u);
        for l in [2i16, 2, 2, 2, 0, 0, 1] {
            le16(&mut blk, l);
        }
        blk.extend_from_slice(b"T\0B\0V\0D\0");
        le16(&mut blk, 7);
        blk.extend_from_slice(&[0xAA, 0xBB]);

        let mut f = Vec::new();
        le16(&mut f, 1);
        let mut h = vec![0u8; 30];
        h[0..4].copy_from_slice(&32u32.to_le_bytes());
        h[4..8].copy_from_slice(&(blk.len() as u32).to_le_bytes());
        h[8] = 1;
        h[0x19] = 1;
        h[0x1a..0x1c].copy_from_slice(&(units_off as u16).to_le_bytes());
        f.extend_from_slice(&h);
        f.extend_from_slice(&blk);

        let sf = StageFile::parse(f).unwrap();
        let s = sf.stage(1).unwrap();
        assert_eq!(s.rewards, vec![Reward::Gold(500)]);
        assert_eq!(s.units[0].class, 81);
        assert_eq!((s.units[0].x, s.units[0].y), (13, 21));
        assert_eq!(s.units[0].items[0], 4);
        assert_eq!((s.title.as_str(), s.defeat.as_str()), ("T", "D"));
        assert_eq!(s.must_survive, vec![7]);
        assert_eq!(&s.block[s.script_offset..], &[0xAA, 0xBB]);
    }
}
