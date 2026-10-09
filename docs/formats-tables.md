# Rule tables, stages, battle maps and saves

Readers: `crates/s2data/src/{tables,tbl_stage,map,save}.rs`; dumper:
`s2tables --game <dir> all <outdir>`. Function names refer to the Ghidra
decompilation of the unpacked exe (see `docs/formats.md`). "Inferred" marks
meanings taken from the data rather than from code that reads the field.

All ids the game passes around are **1-based** (class 1 = 蠻人, item 1 = 短劍,
spell 1 = 治療).

## Where the tables live

| Table | Source | Loader | Global |
|-------|--------|--------|--------|
| classes/actions/type matrix | `KINDSP.BIN`, else DAT 4 | `FUN_00423737` | `DAT_00494774` |
| items | `ITEMSP.BIN`, else DAT 1 | `FUN_004239c9` | `DAT_00494784` (+2 = `DAT_004946a8`) |
| item help text | `ITEMHLSP.BIN`, else DAT 3 | `FUN_004242f2` | `DAT_00494760` |
| terrain | DAT 6 | `FUN_00423a58` | `DAT_00494780` |
| spells | BIN 2 | `FUN_0045e4cf` | `DAT_004a2564` |
| growth / class base stats | BIN 8 | `FUN_0042345b`, `FUN_004233d1` | read on demand |
| formula constants | BIN 10 | `FUN_0042422b` | `DAT_0049456c` |
| world-map stage positions | BIN 4 | `FUN_0040fd3c` | `DAT_00492bc4` |
| campaign stages | BIN 1 | `FUN_00422b8e` | `DAT_00494738` |
| two-player stages | `FSTAGE.BIN` (loose only) | `FUN_0040f418` | same |
| battle maps | PRG 6 | `FUN_00429336`, `FUN_004297f5` | map object `DAT_00497a68` |
| tile attributes | PRG 7 | `FUN_00429070` | map+0x4c/0x50/0x54 |

A loose file next to the exe overrides the archive copy (`FUN_00489fce` =
`_access`). The shipped loose `ITEMHLSP.BIN` (21823 B) is newer than DAT 3
(21763 B).

Other entries met while tracing (not owned here): BIN 5/6 UI strings
(`FUN_00424122`, picked by `DAT_004944f4`), BIN 7 FLC video list, BIN 9 spell
help (`FUN_0045f57c`, 6-byte `(u32 off, u16 len)` entries), BIN 0x11/0x12
sound banks (`FUN_0041a01c`), DAT 5 + CEL 2 font (`FUN_00421f14`), DAT 7
generic image bank (`FUN_00422551`), DAT 11..17 PCX world-map images
(`FUN_00420fab(-id, pal, 3, 0)`), PRG 3/4/5 tile graphics.

## KINDSP (16417 bytes)

Header:

| off | type | meaning |
|-----|------|---------|
| 0x00 | i32 | offset of promotion lists (8 bytes per class) |
| 0x04 | i16 | class count (136) |
| 0x06 | i32, 0x0a i16 | offset/count of skill names (33, NUL-separated; 0 = 無) |
| 0x0c | i32, 0x10 i16 | offset/count of element names (16) |
| 0x12 | i32, 0x16 i16 | offset/count of class records (37 bytes) |
| 0x18 | i32, 0x1c i16 | offset/size n of the n x n type matrix (24); followed by n 9-byte type names |
| 0x1e | i32, 0x22 i16 | offset of per-class `(u16 first, u16 count)` action ranges, total action count (378); the 23-byte action records follow the 136 ranges |

Class record (37 bytes, `DAT_00494580 + (id-1)*0x25`):

| off | type | meaning | evidence |
|-----|------|---------|----------|
| 0x00 | char[11] | name | |
| 0x0b, 0x0c | u8 | always 0x1e, 0x2f | unused so far |
| 0x0d | u8 | battle sprite set ("man cell"); characters with id 1..199 use their own | `FUN_0043ff..` (line 30379) |
| 0x0e | u16 | EXP per level → unit+0x111 | `FUN_00455d95`, `FUN_0043e018` |
| 0x10 | u8 | always 5 | unused so far |
| 0x11 | u8 | unit type 1..24 (兵種, row/col of the type matrix) | `FUN_0040a080`, class info screen |
| 0x12 | u8 | movement type 1..10 | `FUN_00427e1f` |
| 0x13 | u8 | innate skill 隱技 (skill index; sets unit flag `1<<(v-1)`) | `FUN_00455d95` |
| 0x14 | u8 | weapon/special proficiency floor factor for AI units (sets prof[kind−2] for action kinds 2..7 and prof[17]) | `FUN_00452af0` |
| 0x15 | i8 | base AP (行動) | `FUN_0043e018` |
| 0x16 | u16 | 生魔 element mask: spell damage /10 | `FUN_00468d53` |
| 0x18 | u16 | 剋魔 element mask: spell damage x3 | `FUN_00468d53` |
| 0x1a | u8 | base alignment → unit+0x123 (cap 110): bands <31 / <70 / ≥70 give item-restriction bits 1/2/4 (`FUN_0043cf34`); holy classes 80–95, undead/evil 5–25 | `FUN_00455d95`, `FUN_0043cf34` |
| 0x1b | u16 | item required (and consumed) to change into this class | `FUN_00455c85` |
| 0x1d | u32 | unit+0x115 flags, any of which is required to change into this class | `FUN_00455c85` |
| 0x21 | u8 | strength-rating multiplier (/10) | `FUN_0045781c` |
| 0x22 | u8 | tier 1..5 (0 = monster/NPC) | |
| 0x23 | u16 | spell schools allowed (bit k = school k+1) | class change screen |

Promotion list: 8 class ids per class (0 = empty) — the classes reachable by
轉職. E.g. 蠻人 → 狂戰士, 劍術師, 拳法家, 傭兵, 格鬥者, 警衛, 弓兵.

Action record (23 bytes, `DAT_00494590 + idx*0x17`):

| off | meaning |
|-----|---------|
| 0x00 | kind: 1 natural, 2 sword, 3 blade, 4 staff, 5 bow/gun, 6 axe, 7 spear/whip, 11 magic, 12 sword/martial art, 13 brainwash, 40 special move |
| 0x01 | name (9 bytes) |
| 0x0a | animation set (`<< 2` into `unit.facing`) |
| 0x0b | power x/10 (`FUN_0040a080`) |
| 0x0c | hit %: kinds < 21 multiply the weapon's hit (`hit_w * v / 100`), others add v (`FUN_00409f0b`) |
| 0x0d, 0x0e | min/max range; 0,0 = use the weapon item's reach |
| 0x0f, 0x10 | motion/icon id, 0 (unconfirmed) |
| 0x11, 0x12, 0x13 | status effect inflicted, chance %, duration (`FUN_004083c4` AI checks the effect type) |
| 0x14 | hit animation (weapon item+0x1c overrides when > 0) |
| 0x15 | sound effect (weapon item+0x1d overrides) |
| 0x16 | shape: 1 single, 2 ranged, 3 line (weapon item+0x1e overrides) |

Type matrix: `matrix[atk_type-1][def_type-1]`, signed percent added to damage
(halved when the defender has skill flag 0x800).

## ITEMSP (11478 bytes): `i16 count` + 302 x 38-byte records

| off | type | meaning |
|-----|------|---------|
| 0x00 | u16 | id |
| 0x02 | char[12] | name |
| 0x0e | u8 | kind: 1..6 weapon (sword, blade, staff, bow, axe, spear = action kind − 1; proficiency index kind−1), 7..11 armour for equipment slot kind−6, 12 stackable consumable |
| 0x0f | u8 | 0 = equipment whose 0x19 is a passive ability |
| 0x10 | u8 | reach: max = v % 10, min = 1 if v == 1 or v >= 10 else 2 |
| 0x11 | i16 | 攻擊 bonus |
| 0x13 | u8 | weapon hit % (unarmed = 90) |
| 0x14 | i8 | critical bonus (summed over equipment, `FUN_0040927b`) |
| 0x15 | i8 | 防禦 bonus |
| 0x16 | i8 | 魔力 bonus |
| 0x17 | i8 | evasion: subtracted from hit chances against the wearer |
| 0x18 | i8 | AP bonus |
| 0x19 | u8 | passive ability id (`FUN_0043ab32(unit, id)`) |
| 0x1a | u16 | price |
| 0x1c | i8 | animation override |
| 0x1d | u8 | sound override |
| 0x1e | i8 | shape override |
| 0x1f | u32 | equippable unit types (bit type−1) |
| 0x23 | u16 | restrictions: 0x40 only character 11 (志超), 0x80 only 22, 0x100 only 1; low bits vs gender/tier masks of `FUN_0043cf34` |
| 0x25 | u8 | rank for auto-equip (higher wins) |

Passive ability ids seen in code: 2 combo boost, 3 and 7 ignore-defence
checks, 0xb half elemental damage, 0xd −1/3 damage, 0xe halves status
chance, 0x10 −10% hit taken, 0x12 +20%/+50% magic, 0x19 revive at full HP,
0x1c revive once (item consumed), 0x1d 30% double counter, 0x1e counter x3,
0x40 +10 alignment.

ITEMHLSP: `i16 count` + NUL-terminated strings; **string 0 is a header and
string i is the help of item id i** (`DAT_00494760[id]`).

## Terrain (DAT 6, 22632 bytes)

```
i16 total (54), ground (24), objects (30)
per class (136) x per terrain (54): i8 cost, i8 atk, i8 def   (cost -1 = impassable)
54 x char[11] names: 道路 平地 草地 坡地 荒地 岩地 濕地 沙漠 雪地 地磚 木板 石地 地毯 炎漿 淺水 深水 橋 水草 破壞地 障礙物 農地 屋頂 梯道 保留 | 捅子 木頭 瓶罐 岩石 大岩石 巨岩 草叢 樹 寶箱 欄杆 鐵欄杆 藥材 花朵 船身 我方軍旗 敵方軍旗 障礙物 植物 傢俱 墓碑 石雕 雕像 屋頂 火燄 我方脫離點 敵方脫離點 水草 雜物 傳送點 保留
```

Ground tiles index the first 24 entries, object tiles index 24 + (t − 1).

## Spells (BIN 2, 4657 bytes)

```
u8[9]  hdr: [1] = schools (11), [7] = spells (115), [8] = status names (42)
(u8 first, u8 count)[schools]
SpellRec[spells] (37 bytes)
u16 offsets[schools + statuses], strings
```
Schools: 神術 巫術 魔術 妖術 幻術 法術 喚術 魔合 武術 劍術 合擊.

| off | type | meaning |
|-----|------|---------|
| 0x00 | u8 | school 1..11 |
| 0x01 | u8 | index within school |
| 0x02 | char[11] | name |
| 0x0d | i16 | cost: MP if 0x21&1, ST if 0x21&2 (both for both); halved by skill flag 0x20 |
| 0x0f | i16 | required school level (school EXP / 10) |
| 0x11 | i8 | range/size parameter (area = v*4+1 cells etc. per shape) |
| 0x12 | u8 | area shape (1 = single target; 5/8 special in targeting code) |
| 0x13 | u8 | effect code (1 = damage, 0x12/0x5a heal, 0x15..0x1f summons, others statuses) |
| 0x14 | u8 | power formula (see rules.md) |
| 0x15 | i16 | power |
| 0x17 | i16 | hit % |
| 0x19 | i16 | duration: <= 100 turns; 101..200 caster 魔力/(v−100); 201..254 (caster−target 魔力)/(v−200); >= 255 permanent (`FUN_0046ae25`) |
| 0x1b | u16 | element mask |
| 0x1d | u16 | flags (0x4/0x8/0x10 terrain bonuses, 0x40 skip modifiers, 0x80 no damage) |
| 0x1f | u8 | target side (1 own, 2 enemy, 3 all) |
| 0x20 | u8 | & 7: combo needing a partner (合擊); 0x80: requires `FUN_00457301` condition to learn |
| 0x21 | u8 | 1: uses 魔力/MP; 2: uses 劍術/ST; 0x10/0x20/0x40 height falloff classes |
| 0x22..0x24 | u8 | status icon/help ids (unconfirmed) |

Status names (index = status id): 飛行 飄浮 祝福 反彈 隱身 神護 神佑 靈擊 靈盾 再生
輕功 無我 失明 凍結 禁錮 痲庳 詛咒 睡眠 衰弱 恐懼 屏障 土盾 火盾 水盾 暗盾 神盾 光盾
防降 狂暴 嗜血 噁心 護甲 魔強 加速 減速 激勵 石化 中毒 附體 運功 發功 衰竭.

## Growth (BIN 8, 21490 bytes)

```
i16 id_limit (272)
136 x 108 bytes: 9 growth descriptors (6 x i16) in stat order
136 x 50 bytes : base record
```
Stat order: HP, MP, ST, 攻擊, 防禦, 反應, 戰技, 魔力, 劍術.
Base record: the 9 stats, AP, 4 default equipment ids, 11 learned-spell
masks (one per school).

## Formula constants (BIN 10, 871 bytes)

```
i16 n (115 = spells); u8[n] x 4 (per-spell presentation: DAT_004a255c, _1f54, _1f3c, _1f28)
i16 m (133 = CEL1 effect sets); u8[m] sound per effect (DAT_004a1f50)
u8[2] (skipped: FUN_0042422b sets DAT_0049456c = DAT_004a1f50 + m + 2)
i8 P[272]  (DAT_0049456c)
```
`P` indices are listed in rules.md.

## World map stage records (BIN 4, 21440 bytes)

`i16 n (168)` then n x 14 bytes: `u32 text_offset, u16 text_len, u16 x, u16 y,
u8 choice, u8 seg[3]`; `x, y` is the stage's pixel position on the world map
(the view is centred there, `FUN_0040fd3c`). `choice`/`seg` are inferred
(route choice and text segment lengths).

## Stage files (BIN 1: 168 stages; FSTAGE.BIN: 10 two-player stages)

`i16 count`, then 30-byte headers (copied to globals `0x494688..0x4946a6`):

| off | global | meaning |
|-----|--------|---------|
| 0x00 | 494688 | u32 block offset |
| 0x04 | 49468c | u32 block size |
| 0x08 | 494690 | u8 unit placement count |
| 0x09 | 494691 | u8 stage number shown |
| 0x0a | 494692 | u8 music track |
| 0x0c | 494694 | i16 next stage |
| 0x12..0x14 | 49469a.. | flags (→ `DAT_00494508/9/a`) |
| 0x15 | 49469d | stage kind: 1 battle, 3 story only (no EXP/proficiency) |
| 0x16 | 49469e | battle map id (PRG 6) |
| 0x19 | 4946a1 | u8 reward group count |
| 0x1a | 4946a2 | u16 offset of unit records in the block |

Block:

```
(i16 kind, i16 count)[reward groups]; i16 values[sum of counts]
UnitPlacement[unit count] at units_offset, 101 bytes each
u16 len[7]
char title[len0]; char briefing[len1]; char victory[len2];
i16 a[len4]; i16 b[len5]; char defeat[len3]; i16 must_survive[len6]
event script (FUN_00417556 — owned by the script sub-task)
```
Reward kinds (戰果報告, `FUN_0045a9..` lines 42690ff): 1 夥伴加入 (character
ids), 2 夥伴離開, 3 得到金錢 (one value), 4 得到物品 (item ids), 5 搜索寶物:
groups of 4 item ids, one per treasure spot index; at the end of the stage
each unopened spot of that index yields `ids[rand % 4]` (`FUN_0042b097`,
needs army flag 0x40).

Unit placement (101 bytes; `FUN_0043e018` copies it into a unit):

| off | type | → unit | meaning |
|-----|------|--------|---------|
| 0x00 | i16 | +0x02 | character id (portrait/sprite; 0 generic) |
| 0x02 | char[13] | +0x10 | name |
| 0x0f | u8 | +0x2e | low nibble gender (1 = female) |
| 0x10 | i16 | +0x04 | class id |
| 0x12 | i16 | +0xb5 | level (0 → 1) |
| 0x14 | i8 | +0x0a | AI mode (101..150 = wake on turn m−100, 31..100 = wake when unit m−30 dies; docs/ai.md) |
| 0x15 | i8 | +0x06 | group tag |
| 0x16 | i8 | +0x0c | AI sub-mode |
| 0x17, 0x19 | i16 | +0x23, +0x27 | position (col, row) |
| 0x1b.. | 9 x i16 | | HP, MP, ST, 魔力, 反應, 攻擊, 防禦, 劍術, 戰技 (max and current) |
| 0x2d | i16 | +0x123 | alignment (0 = class default) |
| 0x2f | i16 | +0x12e | AP (<= 0 = class default) |
| 0x31 | u8 | | control type: 0/5/6/7 player-controlled (unit+0x08 = 0), others AI; 3/9 boss (unit+0x2f = 1); 8/10 sub-boss (= 2); 4 hidden reserve (flag 0x40000000); 7 clears it |
| 0x32 | u8 | +0x2b | initial facing |
| 0x33 | u8 | | first turn of the appear window (`FUN_004231be`) |
| 0x34 | u8 | | last turn of the window (0 = only turn 0x33) |
| 0x35 | i16 | +0x119 | bonus EXP for defeating this unit (`FUN_00453608`) |
| 0x37 | 4 x i16 | +0x11b | items dropped when defeated |
| 0x3f | 6 x i16 | +0xcb | equipment / carried items (consumables counted into the army) |
| 0x4b | u32 | +0x115 | skill flags |
| 0x4f | 11 x i16 | +0xfb | learned-spell masks per school (inferred) |

## Battle maps (PRG 6, 132 maps) and tile attributes (PRG 7)

PRG 6: `u16 count; (u32 off, u32 size)[count]`, offsets from the entry start.
Map:

```
i16 width, height        grid is width x 2*height staggered rows
i16 cells (= width*2*height), tile_w 64, tile_h 32, row_step 16
i16 objects, spots
i16 extra[6]             (e.g. 3,15,0,0,100,300; TBD: cursor start, bg offset, scale)
Cell[cells]              u8 height, u16 ground tile, u16 object tile (0 = none)
Object[objects]          i16 col, row, pixel w, h, large tile id
Spot[spots]              i16 col, row, u16 flags (low byte treasure index, 0x100 taken, 0x8000 TBD)
3 x { u16 used; i16 remap[global tiles of bank]; (u32 off, u32 size)[used] }   ground, object, large
```
Cell (col, row) is at pixel `(col*64 + (row odd ? 32 : 0), row*16)`. Row 0,
the last row, and columns 0 and width−1 on even rows are off-map
(`FUN_004255da`).

PRG 7: `u16 n_ground (4284), n_object (896), n_large (360)` then one u16 per
tile per bank: `(passability << 8) | terrain` (terrain 1-based; 0 = none).
Passability (high byte) per bank, as used by `FUN_00427e1f`:

- ground: 0,1,2,4 land; 3 water-like (fliers only); 5 never passable
- object: 1 blocks; 2 fly-only; 10 fly-only unless levitating fails → blocked; 0xc excluded from treasure search

## Unit struct (0x16a = 362 bytes, also the save-file record)

| off | type | meaning |
|-----|------|---------|
| 0x02 | i16 | character id |
| 0x04 | i16 | class id |
| 0x08 | i16 | 1 = AI controlled |
| 0x0a, 0x0c | i16 | deployment/action state |
| 0x10 | char[] | name |
| 0x1f | i16 | kill count |
| 0x23, 0x27 | i32 | col, row |
| 0x2b | u8 | AI mode |
| 0x2d | u8 | action/display state |
| 0x2e | u8 | gender |
| 0x2f | u8 | boss class (1, 2) |
| 0x30 | u32 | flags: 0x80000000 deployed, 0x40000000 guest, 0x20000000 dead, 1 = +1 to every level-up gain |
| 0x38 | u32 | buff mask (bit k ↔ buff record at 0x48+3k: u8 turns, i16 value; 0x7c+9k for 3-stat buffs) |
| 0x3c | u32 | status-effect mask |
| 0xb5 | i16 | level (max 99) |
| 0xb7..0xc9 | i16 | base/max: HP, MP, ST, 魔力, AP, 反應, 攻擊, 防禦, 劍術, 戰技 (0xb7, b9, bb, bd, bf, c1, c3, c5, c7, c9) |
| 0xcb | 6 x u16 | equipment (slot 0 weapon, 1..5 armour) |
| 0xd7 | 18 x u16 | proficiency: [0..6) weapon kinds 1..6 (cap 1500), [6..17) school EXP (level = /10, cap 15000), [17] special moves |
| 0xfb | 11 x u16 | learned-spell masks per school |
| 0x111 | u16 | EXP per level; 0x113 u16 EXP |
| 0x115 | u32 | skill flags (innate skills `1<<(skill-1)` and more) |
| 0x119 | u16 | bonus EXP when defeated; 0x11b 4 x u16 drops |
| 0x123 | u8 | alignment (see class 0x1a) |
| 0x124..0x130 | i16 | current HP, MP, ST, 魔力, AP refill (0x12c), AP (0x12e), AP carry (0x130) |
| 0x132 | i16 | HP before the last hit |
| 0x134, 0x136, 0x138 | i16 | current 攻擊, 防禦, 戰技 (base + equipment + buffs) |
| 0x13a..0x140 | i16 | equipment totals: 攻擊, 防禦, 魔力, AP |

## Saves (`SAVE\UJnn.SAV`, nn = 00..52) — writer `FUN_004312c4`

```
"北方密使\0"                      9 bytes (Big5)
description                       50 bytes ("%s %d.%s" + padding, shown in the load menu)
copyright                         255 bytes ("The SuperSp Game V1.4 Copyright (c) 1997 ...")
globals 0x494508                  0x50
globals 0x494638                  0x1f
globals 0x494668                  0x1d   (+0 two-player flag, +4 mode, +8 stage, +0x10 previous stage, +0x1c difficulty)
u32 treasure count; i16[80] treasure candidates of the current stage
globals 0x497b48                  0x1c
army x 2 (player 0x49f9f8, enemy 0x4976c8), FUN_00442ef8:
    u32 n; unit[n] (362 bytes each)
    u32 gold (army+0xcc), u32 flags (+0xde), u32 (+0xd6), u16 (+0xd4), i32 stat scale % (+0xe2)
    u8 item_count[302] (+0xd0), u8[0x21] (+0x286)
if mode (globals 0x49466c) == 0: u16 n; i16 script_flags[n]   (FUN_0041745a)
```
`SaveFile` keeps the global blocks raw and writes them back byte-for-byte.
No original save was available to test against; the reader is validated on
synthetic data only.
