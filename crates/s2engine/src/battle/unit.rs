//! The 0x16a-byte unit record (`docs/structs.md` "Unit").
//!
//! Field order and offsets follow the original struct so saves round-trip
//! byte-for-byte through [`Unit::from_bytes`] / [`Unit::to_bytes`]. The two
//! runtime pointers at 0x0ad/0x0b1 (sprite and frame buffers) are owned by the
//! battle view and are always written as zero.

/// Size of a unit record in memory and in save files.
pub const UNIT_SIZE: usize = 0x16a;

/// `Unit::flags` (0x030).
pub mod flag {
    /// Deployed on the map.
    pub const DEPLOYED: u32 = 0x8000_0000;
    /// Hidden reserve (placement control type 4); joins later.
    pub const RESERVE: u32 = 0x4000_0000;
    /// Dead or retreated.
    pub const DEAD: u32 = 0x2000_0000;
    /// Summoned creature.
    pub const SUMMON: u32 = 0x1000_0000;
    /// Summon animation in progress.
    pub const SUMMONING: u32 = 0x0800_0000;
    /// Movement type 5 override.
    pub const MOVE5: u32 = 0x0400_0000;
    /// Still occupies its cell although dead.
    pub const CORPSE: u32 = 0x0200_0000;
    /// +1 to every stat on level up.
    pub const BONUS_GROWTH: u32 = 0x0000_0001;
}

/// `Unit::turn_flags` (0x034).
pub mod turn {
    pub const MOVED: u32 = 0x01;
    pub const ATTACKED: u32 = 0x02;
    pub const MAGIC: u32 = 0x04;
    pub const TECHNIQUE: u32 = 0x08;
    pub const ITEM: u32 = 0x10;
    pub const EQUIP: u32 = 0x20;
    pub const END_DONE: u32 = 0x80;
}

/// `Unit::status` (0x03c) bits; names are `DAT_004a1f38[bit index]`.
pub mod status {
    pub const FLYING: u32 = 0x1;
    pub const LEVITATE: u32 = 0x2;
    pub const BLESSING: u32 = 0x4;
    pub const REFLECT: u32 = 0x8;
    pub const INVISIBLE: u32 = 0x10;
    pub const GUARD150: u32 = 0x20;
    pub const GUARD180: u32 = 0x40;
    pub const SPIRIT_STRIKE: u32 = 0x80;
    pub const SPIRIT_SHIELD: u32 = 0x100;
    pub const REGEN: u32 = 0x200;
    pub const LIGHT_BODY: u32 = 0x400;
    pub const SELFLESS: u32 = 0x800;
    pub const BLIND: u32 = 0x1000;
    pub const FROZEN: u32 = 0x2000;
    pub const SEALED: u32 = 0x4000;
    pub const PARALYSIS: u32 = 0x8000;
    pub const CURSE: u32 = 0x10000;
    pub const SLEEP: u32 = 0x20000;
    pub const WEAK: u32 = 0x40000;
    pub const FEAR: u32 = 0x80000;
    pub const BARRIER: u32 = 0x100000;
    pub const LIGHT_SHIELD: u32 = 0x4000000;
    /// frozen | paralysis | sleep | blind: cannot act or counter.
    pub const INCAPACITATED: u32 = 0x2b000;
    /// frozen | paralysis | sleep: attacks auto-hit.
    pub const HELPLESS: u32 = 0x2a000;
}

/// `Unit::buffs` (0x038) bits; names are `DAT_004a1f38[27 + bit index]`.
pub mod buff {
    pub const DEF_DOWN: u32 = 0x1;
    pub const BERSERK: u32 = 0x2;
    pub const BLOODLUST: u32 = 0x4;
    pub const NAUSEA: u32 = 0x8;
    pub const ARMOR: u32 = 0x10;
    pub const MAGIC_UP: u32 = 0x20;
    pub const HASTE: u32 = 0x40;
    pub const SLOW: u32 = 0x80;
    pub const INSPIRE: u32 = 0x100;
    pub const PETRIFIED: u32 = 0x200;
    pub const POISON: u32 = 0x400;
    pub const POSSESSED: u32 = 0x800;
    pub const CHI: u32 = 0x1000;
    pub const CHI_RELEASE: u32 = 0x2000;
    pub const EXHAUSTION: u32 = 0x4000;
}

/// One of the nine 3-byte buff slots at 0x048 (buff bits 0..8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuffA {
    pub turns: i8,
    pub delta: i16,
}

/// One of the five 5-byte buff slots at 0x063 (buff bits 9..13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuffB {
    pub turns: i8,
    pub d_atk: i16,
    pub d_def: i16,
}

/// The 9-byte slot at 0x07c (buff bit 14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuffC {
    pub turns: i8,
    pub d_ap: i16,
    pub d_atk: i16,
    pub d_def: i16,
    pub _rest: [u8; 2],
}

/// A unit (0x16a bytes). See `docs/structs.md` for field semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// 0x000 1-based index in its army.
    pub slot: i16,
    /// 0x002 persistent character id (0 = generic; 1 and 0xb = protagonists).
    pub char_id: i16,
    /// 0x004 class id (1-based).
    pub kind: i16,
    /// 0x006 group tag (placement +0x15); `ai_mode` 31..100 waits on it.
    pub tag: i16,
    /// 0x008 1 = AI-controlled (NPC ally or computer unit).
    pub npc: i16,
    /// 0x00a AI mode (placement +0x14).
    pub ai_mode: i16,
    /// 0x00c AI sub-mode (placement +0x16).
    pub ai_sub: i16,
    /// 0x00e AI state (5 = retreating).
    pub ai_state: i16,
    /// 0x010 Big5 name, NUL padded.
    pub name: [u8; 15],
    /// 0x01f
    pub kills: i16,
    /// 0x021
    pub deaths: i16,
    /// 0x023 cell column.
    pub x: i32,
    /// 0x027 cell row.
    pub y: i32,
    /// 0x02b bits 0-1 facing (0 down-left, 1 down-right, 2 up-right, 3 up-left), bits 2.. animation set.
    pub facing: u8,
    /// 0x02c animation frame index.
    pub frame: u8,
    /// 0x02d 0 ready, 1 selected, 2 done, 3 walking, 4 walked, 5 in combat.
    pub act: u8,
    /// 0x02e gender (item restriction mask).
    pub sex: u8,
    /// 0x02f 1 boss, 2 sub-boss.
    pub rank: u8,
    /// 0x030 see [`flag`].
    pub flags: u32,
    /// 0x034 see [`turn`].
    pub turn_flags: u32,
    /// 0x038 see [`buff`].
    pub buffs: u32,
    /// 0x03c see [`status`].
    pub status: u32,
    /// 0x040 render flags (hit flash).
    pub render_flags: u32,
    /// 0x044 unknown.
    pub _044: [u8; 4],
    /// 0x048
    pub buff_a: [BuffA; 9],
    /// 0x063
    pub buff_b: [BuffB; 5],
    /// 0x07c
    pub buff_c: BuffC,
    /// 0x085 turns left per status bit (-1 permanent).
    pub status_turns: [i8; 27],
    /// 0x0a0 battle sprite id.
    pub sprite: u16,
    /// 0x0a2 portrait id.
    pub portrait: u16,
    /// 0x0a4
    pub sprite_w: i16,
    /// 0x0a6
    pub sprite_h: i16,
    /// 0x0a8 centisecond tick of the last animation frame.
    pub anim_timer: i32,
    /// 0x0ac palette remap of the current frame.
    pub remap: i8,
    /// 0x0b5
    pub level: i16,
    /// 0x0b7 ≤ 5000
    pub hp_max: i16,
    /// 0x0b9
    pub mp_max: i16,
    /// 0x0bb
    pub st_max: i16,
    /// 0x0bd 魔力 incl. equipment.
    pub mag: i16,
    /// 0x0bf AP per turn (≤ 110).
    pub ap_max: i16,
    /// 0x0c1 反應.
    pub react: i16,
    /// 0x0c3 攻擊 incl. equipment.
    pub atk: i16,
    /// 0x0c5 防禦 incl. equipment.
    pub def: i16,
    /// 0x0c7 劍術.
    pub sword: i16,
    /// 0x0c9 戰技.
    pub skill: i16,
    /// 0x0cb equipped item ids; [0] = weapon, [1..6] = item kinds 7..11.
    pub equip: [u16; 6],
    /// 0x0d7 proficiency ×10: [0..6] weapon kinds 1..6, [6..17] spell schools 1..11, [17] other.
    pub prof: [u16; 18],
    /// 0x0fb learned spells, bit mask per school.
    pub learned: [u16; 11],
    /// 0x111
    pub exp_next: u16,
    /// 0x113 ≤ 999
    pub exp: u16,
    /// 0x115 隱技 bits.
    pub abilities: u32,
    /// 0x119 EXP for whoever kills this unit.
    pub bounty: u16,
    /// 0x11b items dropped on death.
    pub drops: [u16; 4],
    /// 0x123 magic resistance / charm.
    pub charm: u8,
    /// 0x124
    pub hp: i16,
    /// 0x126
    pub mp: i16,
    /// 0x128
    pub st: i16,
    /// 0x12a
    pub mag_cur: i16,
    /// 0x12c AP refilled to this each turn.
    pub ap_base: i16,
    /// 0x12e
    pub ap: i16,
    /// 0x130 AP carried into the next turn.
    pub ap_carry: i16,
    /// 0x132 HP before the last hit.
    pub hp_before_hit: i16,
    /// 0x134
    pub atk_cur: i16,
    /// 0x136
    pub def_cur: i16,
    /// 0x138
    pub skill_cur: i16,
    /// 0x13a equipment ATK total.
    pub eq_atk: i16,
    /// 0x13c
    pub eq_def: i16,
    /// 0x13e
    pub eq_mag: i16,
    /// 0x140
    pub eq_ap: i16,
    /// 0x142 affinity toward character ids 1..=40.
    pub affinity: [u8; 40],
}

impl Default for Unit {
    fn default() -> Self {
        Unit::from_bytes(&[0; UNIT_SIZE])
    }
}

/// Little-endian cursor used for (de)serialising the record.
struct Rd<'a>(&'a [u8], usize);

impl Rd<'_> {
    fn u8(&mut self) -> u8 {
        let v = self.0[self.1];
        self.1 += 1;
        v
    }
    fn i8(&mut self) -> i8 {
        self.u8() as i8
    }
    fn u16(&mut self) -> u16 {
        let v = u16::from_le_bytes([self.0[self.1], self.0[self.1 + 1]]);
        self.1 += 2;
        v
    }
    fn i16(&mut self) -> i16 {
        self.u16() as i16
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.0[self.1..self.1 + 4].try_into().unwrap());
        self.1 += 4;
        v
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    fn arr<const N: usize>(&mut self) -> [u8; N] {
        let v = self.0[self.1..self.1 + N].try_into().unwrap();
        self.1 += N;
        v
    }
    fn skip(&mut self, n: usize) {
        self.1 += n;
    }
}

struct Wr(Vec<u8>);

impl Wr {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
}

impl Unit {
    /// Decode a raw record (e.g. from a save file).
    pub fn from_bytes(b: &[u8; UNIT_SIZE]) -> Unit {
        let mut r = Rd(b, 0);
        let slot = r.i16();
        let char_id = r.i16();
        let kind = r.i16();
        let tag = r.i16();
        let npc = r.i16();
        let ai_mode = r.i16();
        let ai_sub = r.i16();
        let ai_state = r.i16();
        let name = r.arr::<15>();
        let kills = r.i16();
        let deaths = r.i16();
        let x = r.i32();
        let y = r.i32();
        let facing = r.u8();
        let frame = r.u8();
        let act = r.u8();
        let sex = r.u8();
        let rank = r.u8();
        let flags = r.u32();
        let turn_flags = r.u32();
        let buffs = r.u32();
        let status = r.u32();
        let render_flags = r.u32();
        let _044 = r.arr::<4>();
        let buff_a = std::array::from_fn(|_| BuffA { turns: r.i8(), delta: r.i16() });
        let buff_b = std::array::from_fn(|_| BuffB { turns: r.i8(), d_atk: r.i16(), d_def: r.i16() });
        let buff_c = BuffC { turns: r.i8(), d_ap: r.i16(), d_atk: r.i16(), d_def: r.i16(), _rest: r.arr::<2>() };
        let status_turns = r.arr::<27>().map(|v| v as i8);
        debug_assert_eq!(r.1, 0xa0);
        let sprite = r.u16();
        let portrait = r.u16();
        let sprite_w = r.i16();
        let sprite_h = r.i16();
        let anim_timer = r.i32();
        let remap = r.i8();
        r.skip(8); // 0x0ad, 0x0b1: runtime buffer pointers
        let level = r.i16();
        let hp_max = r.i16();
        let mp_max = r.i16();
        let st_max = r.i16();
        let mag = r.i16();
        let ap_max = r.i16();
        let react = r.i16();
        let atk = r.i16();
        let def = r.i16();
        let sword = r.i16();
        let skill = r.i16();
        let equip = std::array::from_fn(|_| r.u16());
        let prof = std::array::from_fn(|_| r.u16());
        let learned = std::array::from_fn(|_| r.u16());
        let exp_next = r.u16();
        let exp = r.u16();
        let abilities = r.u32();
        let bounty = r.u16();
        let drops = std::array::from_fn(|_| r.u16());
        let charm = r.u8();
        debug_assert_eq!(r.1, 0x124);
        let hp = r.i16();
        let mp = r.i16();
        let st = r.i16();
        let mag_cur = r.i16();
        let ap_base = r.i16();
        let ap = r.i16();
        let ap_carry = r.i16();
        let hp_before_hit = r.i16();
        let atk_cur = r.i16();
        let def_cur = r.i16();
        let skill_cur = r.i16();
        let eq_atk = r.i16();
        let eq_def = r.i16();
        let eq_mag = r.i16();
        let eq_ap = r.i16();
        let affinity = r.arr::<40>();
        debug_assert_eq!(r.1, UNIT_SIZE);
        Unit {
            slot, char_id, kind, tag, npc, ai_mode, ai_sub, ai_state, name, kills, deaths, x, y, facing,
            frame, act, sex, rank, flags, turn_flags, buffs, status, render_flags, _044, buff_a, buff_b,
            buff_c, status_turns, sprite, portrait, sprite_w, sprite_h, anim_timer, remap, level, hp_max,
            mp_max, st_max, mag, ap_max, react, atk, def, sword, skill, equip, prof, learned, exp_next,
            exp, abilities, bounty, drops, charm, hp, mp, st, mag_cur, ap_base, ap, ap_carry,
            hp_before_hit, atk_cur, def_cur, skill_cur, eq_atk, eq_def, eq_mag, eq_ap, affinity,
        }
    }

    /// Encode as the original 0x16a-byte record (buffer pointers zeroed).
    pub fn to_bytes(&self) -> [u8; UNIT_SIZE] {
        let mut w = Wr(Vec::with_capacity(UNIT_SIZE));
        for v in [self.slot, self.char_id, self.kind, self.tag, self.npc, self.ai_mode, self.ai_sub, self.ai_state] {
            w.u16(v as u16);
        }
        w.bytes(&self.name);
        w.u16(self.kills as u16);
        w.u16(self.deaths as u16);
        w.u32(self.x as u32);
        w.u32(self.y as u32);
        for v in [self.facing, self.frame, self.act, self.sex, self.rank] {
            w.u8(v);
        }
        for v in [self.flags, self.turn_flags, self.buffs, self.status, self.render_flags] {
            w.u32(v);
        }
        w.bytes(&self._044);
        for b in &self.buff_a {
            w.u8(b.turns as u8);
            w.u16(b.delta as u16);
        }
        for b in &self.buff_b {
            w.u8(b.turns as u8);
            w.u16(b.d_atk as u16);
            w.u16(b.d_def as u16);
        }
        let c = &self.buff_c;
        w.u8(c.turns as u8);
        w.u16(c.d_ap as u16);
        w.u16(c.d_atk as u16);
        w.u16(c.d_def as u16);
        w.bytes(&c._rest);
        for &t in &self.status_turns {
            w.u8(t as u8);
        }
        w.u16(self.sprite);
        w.u16(self.portrait);
        w.u16(self.sprite_w as u16);
        w.u16(self.sprite_h as u16);
        w.u32(self.anim_timer as u32);
        w.u8(self.remap as u8);
        w.bytes(&[0; 8]);
        for v in [
            self.level, self.hp_max, self.mp_max, self.st_max, self.mag, self.ap_max, self.react, self.atk,
            self.def, self.sword, self.skill,
        ] {
            w.u16(v as u16);
        }
        for &v in self.equip.iter().chain(&self.prof).chain(&self.learned) {
            w.u16(v);
        }
        w.u16(self.exp_next);
        w.u16(self.exp);
        w.u32(self.abilities);
        w.u16(self.bounty);
        for &v in &self.drops {
            w.u16(v);
        }
        w.u8(self.charm);
        for v in [
            self.hp, self.mp, self.st, self.mag_cur, self.ap_base, self.ap, self.ap_carry, self.hp_before_hit,
            self.atk_cur, self.def_cur, self.skill_cur, self.eq_atk, self.eq_def, self.eq_mag, self.eq_ap,
        ] {
            w.u16(v as u16);
        }
        w.bytes(&self.affinity);
        w.0.try_into().expect("unit record is 0x16a bytes")
    }

    /// Name as text.
    pub fn name_str(&self) -> String {
        s2data::text::big5(&self.name)
    }

    pub fn is_deployed(&self) -> bool {
        self.flags & flag::DEPLOYED != 0
    }

    pub fn is_dead(&self) -> bool {
        self.flags & flag::DEAD != 0
    }

    /// Deployed and alive.
    pub fn on_map(&self) -> bool {
        self.flags & (flag::DEPLOYED | flag::DEAD) == flag::DEPLOYED
    }

    /// Port of FUN_0043c727: alive and not incapacitated.
    pub fn can_act(&self) -> bool {
        self.flags & flag::DEAD == 0 && self.buffs & buff::PETRIFIED == 0 && self.status & status::INCAPACITATED == 0
    }

    /// Port of FUN_0043c76c: may counter-attack.
    pub fn can_counter(&self) -> bool {
        self.buffs & buff::PETRIFIED == 0 && self.status & status::INCAPACITATED == 0
    }

    /// `rank & 3 != 0` or a protagonist: the AI's "VIP" test.
    pub fn is_vip(&self) -> bool {
        self.char_id == 1 || self.char_id == 0xb || self.rank & 3 != 0
    }

    /// Port of FUN_0043c32d: recompute current stats from base values and buffs.
    pub fn recompute_buffs(&mut self) {
        self.mag_cur = self.mag;
        self.ap_base = self.ap_max;
        self.atk_cur = self.atk;
        self.def_cur = self.def;
        self.skill_cur = self.skill;
        if self.buffs == 0 {
            return;
        }
        let b = self.buffs;
        let a = &self.buff_a;
        if b & 0x1 != 0 {
            self.def_cur += a[0].delta;
        }
        if b & 0x2 != 0 {
            self.atk_cur += a[1].delta;
        }
        if b & 0x4 != 0 {
            self.atk_cur += a[2].delta;
        }
        if b & 0x8 != 0 {
            self.atk_cur += a[3].delta;
        }
        if b & 0x10 != 0 {
            self.def_cur += a[4].delta;
        }
        if b & 0x20 != 0 {
            self.mag_cur += a[5].delta;
        }
        if b & 0x40 != 0 {
            self.ap_base += a[6].delta;
        }
        if b & 0x80 != 0 {
            self.ap_base += a[7].delta;
        }
        if b & 0x100 != 0 {
            self.skill_cur += a[8].delta;
        }
        for (k, bit) in [0x200u32, 0x400, 0x800, 0x1000, 0x2000].into_iter().enumerate() {
            if b & bit != 0 {
                self.atk_cur += self.buff_b[k].d_atk;
                self.def_cur += self.buff_b[k].d_def;
            }
        }
        if b & 0x4000 != 0 {
            self.ap_base += self.buff_c.d_ap;
            self.atk_cur += self.buff_c.d_atk;
            self.def_cur += self.buff_c.d_def;
        }
        self.ap_base = self.ap_base.max(0);
        self.atk_cur = self.atk_cur.max(0);
        self.def_cur = self.def_cur.max(0);
    }

    /// Port of FUN_004585cc: reset battle state at stage start.
    pub fn reset_for_stage(&mut self) {
        self.flags &= 0x73ff_ffff;
        self.turn_flags = 0;
        self.buffs = 0;
        self.status = 0;
        self.act = 0;
        // memset(0x124, 0, 0x16): hp..skill_cur
        self.hp = self.hp_max;
        self.mp = self.mp_max;
        self.st = self.st_max;
        self.mag_cur = self.mag;
        if self.ap_max < 0x6f {
            self.ap = self.ap_max;
            self.ap_base = self.ap;
        } else {
            self.ap = 0x6e;
            self.ap_base = 0x6e;
        }
        self.ap_carry = 0;
        self.hp_before_hit = 0;
        self.atk_cur = self.atk;
        self.def_cur = self.def;
        self.skill_cur = self.skill;
    }

    /// Port of FUN_0043c8bb: set status `bit` for `turns`; returns true if newly set.
    pub fn apply_status(&mut self, turns: i32, bit: u32) -> bool {
        let i = bit_index(bit, 27);
        if self.status & bit == 0 {
            self.status |= bit;
            self.status_turns[i] = turns as i8;
            true
        } else {
            let t = &mut self.status_turns[i];
            if (*t as i32) < turns && *t != -1 {
                *t = turns as i8;
            }
            false
        }
    }

    /// Port of FUN_0043cb09: 5-byte buff (bits 0x200..0x2000) with ATK/DEF deltas.
    pub fn apply_buff_b(&mut self, turns: i32, d_atk: i16, d_def: i16, bit: u32) -> bool {
        let i = bit_index(bit >> 9, 5);
        if self.buffs & bit == 0 {
            self.buffs |= bit;
            let s = &mut self.buff_b[i];
            s.d_atk = d_atk;
            self.atk_cur += d_atk;
            s.d_def = d_def;
            self.def_cur += d_def;
            s.turns = turns as i8;
            true
        } else {
            let t = &mut self.buff_b[i].turns;
            if (*t as i32) < turns && *t != -1 {
                *t = turns as i8;
            }
            false
        }
    }

    /// Port of FUN_0043cbf0: poison (ATK/DEF −1/5).
    pub fn apply_poison(&mut self, turns: i32) -> bool {
        let (a, d) = ((-(self.atk_cur as i32) / 5) as i16, (-(self.def_cur as i32) / 5) as i16);
        self.apply_buff_b(turns, a, d, buff::POISON)
    }

    /// Port of FUN_0043cc3c: petrify (ATK/DEF −1/2).
    pub fn apply_petrify(&mut self, turns: i32) -> bool {
        let (a, d) = ((-(self.atk_cur as i32) / 2) as i16, (-(self.def_cur as i32) / 2) as i16);
        self.apply_buff_b(turns, a, d, buff::PETRIFIED)
    }

    /// Port of FUN_0043ccc5: berserk with ATK +`delta`.
    pub fn apply_berserk(&mut self, delta: i16, turns: i32) -> bool {
        let i = 1; // FUN_0043c7a1(2)
        if self.buffs & buff::BERSERK == 0 {
            self.atk_cur += delta;
            self.buffs |= buff::BERSERK;
            self.buff_a[i].delta = delta;
            self.buff_a[i].turns = turns as i8;
            true
        } else {
            let t = &mut self.buff_a[i].turns;
            if (*t as i32) < turns && *t != -1 {
                *t = turns as i8;
            }
            false
        }
    }

    /// Port of FUN_0043cc82: remove petrification.
    pub fn cure_petrify(&mut self) -> bool {
        let had = self.buffs & buff::PETRIFIED != 0;
        if had {
            self.buffs ^= buff::PETRIFIED;
            self.recompute_buffs();
        }
        had
    }

    /// Cleanse negative effects (`buffs &= 0xffffb974; status &= 0xfef00fff`) and recompute.
    pub fn cleanse(&mut self) {
        self.buffs &= 0xffff_b974;
        self.status &= 0xfef0_0fff;
        self.recompute_buffs();
    }

    /// HP percentage (`hp*100/hp_max`, 0 if no max).
    pub fn hp_pct(&self) -> i32 {
        if self.hp_max > 0 {
            self.hp as i32 * 100 / self.hp_max as i32
        } else {
            0
        }
    }
}

/// Index of the lowest set bit of `v` scanned over `n` bits (FUN_0043c7a1 family); 0 if none.
pub fn bit_index(v: u32, n: usize) -> usize {
    (0..n).find(|&i| v >> i & 1 != 0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_round_trips() {
        let mut b = [0u8; UNIT_SIZE];
        for (i, v) in b.iter_mut().enumerate() {
            *v = (i * 7 + 3) as u8;
        }
        b[0xad..0xb5].fill(0);
        let u = Unit::from_bytes(&b);
        assert_eq!(u.to_bytes(), b);
        assert_eq!(u.x, i32::from_le_bytes(b[0x23..0x27].try_into().unwrap()));
        assert_eq!(u.hp, i16::from_le_bytes([b[0x124], b[0x125]]));
        assert_eq!(u.affinity[39], b[0x169]);
        assert_eq!(u.level, i16::from_le_bytes([b[0xb5], b[0xb6]]));
    }

    #[test]
    fn status_durations_extend_only() {
        let mut u = Unit::default();
        assert!(u.apply_status(3, status::PARALYSIS));
        assert!(!u.apply_status(2, status::PARALYSIS));
        assert_eq!(u.status_turns[15], 3);
        assert!(!u.apply_status(5, status::PARALYSIS));
        assert_eq!(u.status_turns[15], 5);
    }

    #[test]
    fn poison_lowers_and_recompute_keeps() {
        let mut u = Unit { atk: 50, def: 20, ..Unit::default() };
        u.recompute_buffs();
        u.apply_poison(3);
        assert_eq!((u.atk_cur, u.def_cur), (40, 16));
        u.recompute_buffs();
        assert_eq!((u.atk_cur, u.def_cur), (40, 16));
        u.buffs = 0;
        u.recompute_buffs();
        assert_eq!((u.atk_cur, u.def_cur), (50, 20));
    }
}
