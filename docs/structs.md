# Runtime data structures (battle)

Reverse-engineered from the unpacked `WSUPERSP.EXE` (image base 0x400000).
`FUN_xxxxxxxx` = function address, `DAT_xxxxxxxx` = global. All offsets are
byte offsets; the original code is MSVC with packed structs (many fields are
unaligned). Field names are ours. "?" = not yet understood; "(guess)" = inferred
from usage only.

Companion docs: `battle.md` (rules and algorithms), `ai.md` (enemy AI),
`formats-tables.md` / `rules.md` (TABLES sub-task: on-disk tables).

## Globals

| Global | Type | Meaning |
|---|---|---|
| `DAT_0049f9f8` | `Army` | player side (side 0) |
| `DAT_004976c8` | `Army` | enemy side (side 1) |
| `DAT_00497a68` | `BattleMap` | the battle map |
| `DAT_004a2b48` | `MoveRange` | movement-range scratch object (one global instance) |
| `DAT_00492b08` | `Duel` | combat forecast/resolution for the main target |
| `DAT_004a1f58` | `SpellCaster` | magic/technique/summon executor |
| `DAT_00492a68` | `SpellTarget` | spell target/area selector (0x401000–0x406000 code) |
| `DAT_00492af0` | `AiCtl` | per-unit AI controller (`FUN_0040a443`) |
| `DAT_00492bd0` | script VM object (TEXT sub-task) |
| `DAT_0048eaa0/a4` | i32 | map cursor cell x / y |
| `DAT_00494674` | i32 | turn number (1-based; ≥100 = defeat) |
| `DAT_00494668` | i32 | 1 = two-player VS mode |
| `DAT_004944e0` | i32 | VS: which side is acting |
| `DAT_00494684` | i8 | difficulty 1..5 (1 easiest; set to 3 by `FUN_00424450`, 5 by `FUN_0040f67f`; shown in settings) |
| `DAT_00497b54` | i32 | chosen protagonist: 0 → char 0xb, else char 1 |
| `DAT_00494510/11` | u8 | stage seals: 1 = player army can't use magic / techniques |
| `DAT_0049450b/0c` | u8 | script-forced victory / defeat |
| `DAT_004944fc` | ptr | current stage header (victory/defeat condition counts at +6..+0xc) |
| `DAT_00494750/54` | i16* | victory / defeat condition lists |
| `DAT_00494598` | i16[4*20] | event-point "reached" flags (8 bytes per point) |
| `DAT_0049478c` | i32 | number of event points on this map |
| `DAT_004952cc` | i32 | which victory group was satisfied (1/2) |
| `DAT_00494758/5c, DAT_004946a1` | | victory reward list (type,count)×n and values |
| `DAT_0049456c` | `i8*` | **rules constants** table (see below) |
| `DAT_00494580` | `ClassRec*` | class table, 0x25 bytes per class (KINDSP) |
| `DAT_00494590` | `AtkSkill*` | class attack-skill table, 0x17 bytes each (KINDSP) |
| `DAT_00494558` | `(i16 first, i16 count)*` | per-class attack-skill list (index = kind) |
| `DAT_00494660` | `u8[8]*` | per-class promotion targets (index = kind) |
| `DAT_00494594`, `DAT_004946b0` | `i8*`, i32 | class-type affinity matrix (n×n, n = DAT_004946b0) |
| `DAT_004946a8` | `ItemRec*` | item table, 0x26 bytes each; `DAT_00494788` = item count |
| `DAT_004a2560` | `MagicRec*` | magic/technique table, 0x25 bytes each (BIN 2) |
| `DAT_004a2564` | `u8*` | BIN 2 header: [1]=#categories (11), [2]=#magic cats (8), [3]=#technique cats (3), [7]=#skills, [8]=#status names |
| `DAT_004a2570` | `(u8 first, u8 count)[11]` | skills per category (first is 0-based index into MagicRec) |
| `DAT_004a1f38` | `char*[42]` | status names: 0..26 = `Unit.status` bits, 27..41 = `Unit.buffs` bits |
| `DAT_004a1f54` | `u8*` | per-spell array (BIN 10); value 2 = "no cost / summon-kill" special |
| `DAT_00494560` | `i8[3]*` | terrain effect triples (DAT 6), see `BattleMap` |
| `DAT_00494768` | `char*[]` | system strings (BIN 5/6) |
| `DAT_004a2690` | u8[100] | RNG permutation table (see battle.md §RNG) |
| `DAT_004a2af8` | i32 | RNG reseed counter |
| `DAT_0048fc10` | u32 | MSVC `rand()` state |

## Army (0x2b3+ bytes) — methods in 0x43a000–0x446000, 0x44c000–0x45d000

```rust
struct Army {
    /*000*/ count: i32,
    /*004*/ units: [*mut Unit; 50],       // 1-based ids: units[id-1]
    /*0cc*/ gold: i32,                    // victory reward type 3 adds here (FUN_0045cfad)
    /*0d0*/ inventory: *mut u8,           // count per item id-1 (cap 99), DAT_00494788 entries
    /*0d4*/ _d4: i16,                     // saved, ?
    /*0d6*/ _d6: i32,                     // saved, ?
    /*0da*/ side: i32,                    // 0 = player, 1 = enemy
    /*0de*/ flags: u32,                   // aggregated by FUN_0043abc2, see below
    /*0e2*/ _e2: i32,                     // saved, ?
    /*0e8*/ portrait_index: [(u32 off, u32 size); ..], // portrait blobs, base ptr at +0x27e
    /*27e*/ portrait_data: *mut u8,
    /*282*/ opponent: *mut Army,
    /*286*/ summons: [(u8 active, i16 kind); 11],  // 0x21 bytes, saved
    /*2ad*/ summon_sprite_off: i32,
    /*2b3*/ summon_anim_table: [u8; 0xc8],         // frame table for summoned creatures
    /*37b*/ summon_anim_table2: [u8; ..],
    /*399*/ summon_sprite_data: *mut u8,
}
```

`Army.flags` (recomputed by `FUN_0043abc2(army, mode)`; bits 0x7e cleared first):

| bit | set when a living, deployed unit has… |
|---|---|
| 0x01 | (set/cleared by spells: `FUN_0043c94a`, cleared at phase upkeep) "reveal invisible" |
| 0x02 | ability 0x8 |
| 0x04 | item ability 0x3a, or ability 0x4000000 (sets 0x0c) |
| 0x08 | item ability 0x1f, or ability 0x4000000 |
| 0x10 | ability 0x20000 |
| 0x20 | non-NPC with ability 0x40000 → NPC allies become player-controllable |
| 0x40 | non-NPC with ability 0x80000000 → treasure search after victory |

Opponent flag `& 9` disables invisibility (`FUN_0043aad7`); flag `& 2` on the
defender's army disables critical hits against it (`FUN_0040927b`).

Save format (`FUN_00442ef8`): `count`, each unit 0x16a raw, then +0xcc(4),
+0xde(4), +0xd6(4), +0xd4(2), +0xe2(4), inventory (item count bytes), +0x286 (0x21).

## Unit (0x16a bytes)

Created zeroed (`FUN_0043d787`, `FUN_0043d8b5`, `FUN_0043d9e9`), filled from a
0x65-byte definition record by `FUN_0043e018`. Two stat blocks exist: the
"max/base" block at 0xb5.. (from the definition, plus equipment) and the
"current" block at 0x124.. (buffs/damage apply here).

```rust
struct Unit {
    /*000*/ slot: i16,            // 1-based index in army
    /*002*/ char_id: i16,         // persistent character id; 0 = generic; 1 and 0xb = protagonists
    /*004*/ kind: i16,            // class id (1-based, ClassRec)
    /*006*/ tag: i16,             // def+0x15 (i8): group tag; ai_mode 31..100 waits for the unit with tag m-30 (FUN_00457644)
    /*008*/ npc: i16,             // 1 = AI-controlled ally / AI unit (def.ctrl ∉ {0,5,6,7})
    /*00a*/ ai_mode: i16,         // def+0x14; see ai.md (1 attack, 2 standby, 3 hold, 4 keep away,
                                  //  5 retreat, 6/7 go-to-point, 31..100 wake when unit #(m-30) dies,
                                  //  101..150 wake on turn m-100)
    /*00c*/ ai_sub: i16,          // def+0x16; 1 = "wait until a target is reachable"
    /*00e*/ ai_state: i16,        // 5 = retreating (cleared when HP ≥ 90%)
    /*010*/ name: [u8; 15],       // Big5, from def+2 (strcpy)
    /*01f*/ kills: i16,
    /*021*/ deaths: i16,
    /*023*/ x: i32,               // cell column
    /*027*/ y: i32,               // cell row (staggered rows, see BattleMap)
    /*02b*/ facing: u8,           // bits0-1 = direction (0 down-left,1 down-right,2 up-right,3 up-left),
                                  // bits2.. = animation set * 4 (0 idle, 1 active, skill anim id)
    /*02c*/ frame: u8,            // current animation frame index
    /*02d*/ act: u8,              // 0 ready, 1 selected, 2 done, 3 walking, 4 walked, 5 in combat
    /*02e*/ sex: u8,              // def+0xf & 0xf; 0/1/other → item restriction mask 8/0x10/0x20 (guess: gender)
    /*02f*/ rank: u8,             // 1 = boss (ctrl 3/9), 2 = sub-boss (ctrl 8/10); `rank & 3 != 0` = "important"
    /*030*/ flags: u32,           // see below
    /*034*/ turn_flags: u32,      // reset to 0 at phase end; see below
    /*038*/ buffs: u32,           // 15 bits, names DAT_004a1f38[27+i]
    /*03c*/ status: u32,          // 27 bits, names DAT_004a1f38[i]
    /*040*/ render_flags: u32,    // 0xc0000000 = hit flash while being struck
    /*048*/ buff_a: [(u8 turns, i16 delta); 9],      // buffs bits 0..8
    /*063*/ buff_b: [(u8 turns, i16 d_atk, i16 d_def); 5], // buffs bits 9..13
    /*07c*/ buff_c: (u8 turns, i16 d_ap, i16 d_atk, i16 d_def, i16 _), // buffs bit 14 (9 bytes)
    /*085*/ status_turns: [i8; 27],  // -1 = permanent; only >0 counts down
    /*0a0*/ sprite: u16,          // unit graphic id (DAT_004975f8 table)
    /*0a2*/ portrait: u16,
    /*0a4*/ sprite_w: i16, /*0a6*/ sprite_h: i16,
    /*0a8*/ anim_timer: i32,      // centisecond tick of last frame
    /*0ac*/ remap: i8,            // palette remap for current frame
    /*0ad*/ sprite_buf: *mut u8,  /*0b1*/ frame_buf: *mut u8,
    /*0b5*/ level: i16,           // 1..99
    /*0b7*/ hp_max: i16,          // ≤ 5000
    /*0b9*/ mp_max: i16,          // ≤ 999 (all other stats too)
    /*0bb*/ st_max: i16,          // ST = 精力 (technique fuel)
    /*0bd*/ mag: i16,             // 魔力 incl. equipment
    /*0bf*/ ap_max: i16,          // AP = 行動點 per turn (≤ 110)
    /*0c1*/ react: i16,           // 反應 (evasion / crit defence)
    /*0c3*/ atk: i16,             // 攻擊 incl. equipment
    /*0c5*/ def: i16,             // 防禦 incl. equipment
    /*0c7*/ sword: i16,           // 劍術 (technique power)
    /*0c9*/ skill: i16,           // 戰技 (hit / crit)
    /*0cb*/ equip: [u16; 6],      // item ids; [0] = weapon
    /*0d7*/ prof: [u16; 18],      // [0..6] weapon type 1..6, [6..17] magic/tech category 1..11, [17] other
                                  // (stored ×10; cap 1500 for weapons via FUN_00452da5, 15000 for categories)
    /*0fb*/ learned: [u16; 11],   // bitmask per category: bit i = skill (first+i) known
    /*111*/ exp_next: u16,        // from ClassRec+0xe
    /*113*/ exp: u16,             // ≤ 999
    /*115*/ abilities: u32,       // 隱技 bitmask, names DAT_00494778[i+1]
    /*119*/ bounty: u16,          // EXP awarded to the killer
    /*11b*/ drops: [u16; 4],      // items dropped on death
    /*123*/ charm: u8,            // def+0x2d or ClassRec+0x1a; class change adds/subtracts. = alignment (TABLES): FUN_0043cf34 maps <31/<70/≥70 to item restriction bits 1/2/4; holy classes 80–95, undead 5–25
    /*124*/ hp: i16, /*126*/ mp: i16, /*128*/ st: i16,
    /*12a*/ mag_cur: i16,
    /*12c*/ ap_base: i16,         // AP refilled to this each turn (incl. equipment & buffs)
    /*12e*/ ap: i16,              // current AP
    /*130*/ ap_carry: i16,        // AP carried into next turn (≤ rules[0xed]); clamped to ap
    /*132*/ hp_before_hit: i16,   // display only; also used by power rating mode 1
    /*134*/ atk_cur: i16, /*136*/ def_cur: i16, /*138*/ skill_cur: i16,
    /*13a*/ eq_atk: i16, /*13c*/ eq_def: i16, /*13e*/ eq_mag: i16, /*140*/ eq_ap: i16, // equipment totals
    /*142*/ affinity: [u8; 40],   // affection toward char_id 1..40; > 'E' (69) both ways enables couple combos
}
```

`Unit.flags` (0x30):

| bit | meaning |
|---|---|
| 0x80000000 | deployed / on the map |
| 0x40000000 | hidden reserve (def.ctrl 4; joins later) |
| 0x20000000 | dead or retreated |
| 0x10000000 | summoned creature (uses the army summon sprite tables) |
| 0x08000000 | being summoned (animation) |
| 0x04000000 | movement type 5 override (see `move_type`) |
| 0x02000000 | still occupies its cell while 0x20000000 is set |
| 0x00000001 | bonus growth: +1 to every stat on level up |

`Unit.turn_flags` (0x34): 0x01 moved, 0x02 attacked, 0x04 used magic, 0x08 used
technique, 0x10 used item, 0x20 changed equipment, 0x80 end-of-action processed.

### Status (0x3c) and buffs (0x38)

Bit i of `status` ↔ name `DAT_004a1f38[i]`, bit i of `buffs` ↔ `[27+i]`.
Effects listed are what the battle code does with them (battle.md has the full rules).

| status bit | name | effect in code |
|---|---|---|
| 0x1 | 飛行 flying | move type 4 |
| 0x2 | 飄浮 levitate | move type 1 |
| 0x4 | 祝福 blessing | incoming hit/crit −10% |
| 0x8 | 反彈 reflect | |
| 0x10 | 隱身 invisible | hidden from enemy (see `FUN_0043aad7`) |
| 0x20 | 神護 | physical damage −150 (min 1) |
| 0x40 | 神佑 | damage −180 (min 1) |
| 0x80 | 靈擊 | own attacks ignore DEF |
| 0x100 | 靈盾 | category-5 spells deal half |
| 0x200 | 再生 regen | +15% HP (≥10) at upkeep |
| 0x400 | 輕功 | ignore height limits when moving |
| 0x800 | 無我 | takes 0 physical damage, 0 from damage spells |
| 0x1000 | 失明 blind | counter hit /10; can't counter |
| 0x2000 | 凍結 frozen | can't act/counter; is auto-hit |
| 0x4000 | 禁錮 sealed | can't cast magic |
| 0x8000 | 痲庳 paralysis | can't act/counter; auto-hit; 71% removed when hit |
| 0x10000 | 詛咒 curse | no healing, no rest, no items |
| 0x20000 | 睡眠 sleep | can't act/counter; auto-hit; removed when hit |
| 0x40000 | 衰弱 weak | own damage /2 |
| 0x80000 | 恐懼 fear | 49% chance to lose its turn |
| 0x100000 | 屏障 barrier | immune to inflicted statuses |
| 0x200000..0x2000000 | 土/火/水/暗/神盾 | (elemental shields; checked via masks) |
| 0x4000000 | 光盾 | damage −1/3, immune to crits/instant death |

| buff bit | name | storage | effect |
|---|---|---|---|
| 0x1 | 防降 | buff_a[0] | DEF −delta |
| 0x2 | 狂暴 berserk | buff_a[1] | ATK +delta; uncontrollable, AI attacks anyone |
| 0x4 | 嗜血 | buff_a[2] | ATK +delta |
| 0x8 | 噁心 | buff_a[3] | ATK −delta% |
| 0x10 | 護甲 | buff_a[4] | DEF +delta |
| 0x20 | 魔強 | buff_a[5] | MAG +delta |
| 0x40 | 加速 haste | buff_a[6] | AP +delta |
| 0x80 | 減速 slow | buff_a[7] | AP −delta |
| 0x100 | 激勵 | buff_a[8] | 戰技 +delta |
| 0x200 | 石化 petrified | buff_b[0] | ATK/DEF −1/2; can't act; auto-hit |
| 0x400 | 中毒 poison | buff_b[1] | ATK/DEF −1/5; −20% HP (≤200) per upkeep, can kill |
| 0x800 | 附體 | buff_b[2] | ATK/DEF +1/2 |
| 0x1000 | 運功 | buff_b[3] | incoming damage ×0.8 |
| 0x2000 | 發功 | buff_b[4] | incoming damage ×0.7 |
| 0x4000 | 衰竭 | buff_c | AP, ATK −10%, DEF −10% |

Masks used by the code: `0x2b000` (frozen|paralysis|sleep|blind) and `buffs&0x200`
= "incapacitated" (`FUN_0043c727`/`FUN_0043c76c`); `0x2a000` = auto-hit;
"cleanse" = `buffs &= 0xffffb974` (or `0xffffbb74`), `status &= 0xfef00fff`;
"dispel positive" = `buffs &= 0xffffc68b`, `status &= 0xf90ff000`.
`FUN_0043c32d` recomputes current stats from buffs after any change.

### Helpers on units

| fn | meaning |
|---|---|
| `FUN_0043aa26(u)` | **move type**: 1 levitate (class move 7 or status 0x2), 2 (class move 8 or item ability 0xf), 3 (class move 6), 4 flying (status 0x1 or item ability 0x11), 5 (flags 0x4000000), else 0 ground |
| `FUN_0043ab32(u, a)` | has an equipped item (type field +0xf == 0) with item ability +0x19 == a |
| `FUN_0043c727(u)` | can act: alive and not incapacitated |
| `FUN_0043aad7(army,u)` | invisible to the opponent: (ability 0x80 or status 0x10) and no adjacent enemy (`FUN_0043b9ff`) and `opponent.flags & 9 == 0` |
| `FUN_0043a7ea(u)` | can move at all: `ap − rules[0xe8] − (move_type==0 ? rules[0xec] : rules[0xe9]) ≥ 0` |
| `FUN_0043a522(u)` | promotion available (level ≥ 11, or level 10 and exp ≥ next) and class has targets; else clamps exp to next |
| `FUN_0043ce32(army,u,k)` | knows any skill of kind k (1 magic cats [0,b2), 2 technique cats [b2,b2+b3), 3 any), honouring stage seals for the player |
| `FUN_0045781c(u,mode,part)` | **power rating**, see battle.md §Power rating |
| `FUN_004576f7(u,x,y)` | facing toward cell |
| `FUN_00457578(army,char_id,mode)` | find unit by char id (mode 0 deployed, 1 any alive, 2 any incl. dead, 3 not dead/hidden, 4 any) |

## BattleMap (`DAT_00497a68`)

Isometric diamond tiles 64×32 px on **staggered rows**: cell (x, y) is drawn
at `px = x*64 + (y odd ? 32 : 0)`, `py = y*16 − (height−1)*16`. Rows are
`0 .. 2*h−1`; row 0 is invalid; on even rows columns 0 and w−1 are invalid.

```rust
struct BattleMap {
    /*00*/ w: i16,                 // cells per row
    /*02*/ h: i16,                 // half the row count (rows = 2h)
    /*04*/ ncells: i16,
    /*0e*/ nsprites: i16,          // map object list (+0x24, 6 bytes each, sorted by row; +0x74 row index)
    /*1c*/ cells: *mut [u8; 5],    // per cell: u8 height, i16 tile (1-based), i16 overlay (1-based)
    /*24*/ objs: *mut [i16; 3],
    /*28..*/ tile graphics tables (GRAPHICS sub-task)
    /*4c*/ tile_terrain: *mut u16, // per tile id: hi byte = terrain class, lo byte = terrain type (1-based)
    /*50*/ overlay_terrain: *mut u16, // per overlay id: hi byte = overlay class, lo byte = type
    /*6c*/ scroll_x: i32,          // in half cells; visible columns = scroll_x/2 .. +10
    /*70*/ scroll_y: i32,          // in rows; visible rows scroll_y .. +30
    /*89*/ nspecial: i32, /*8d*/ special: [(i16 x, i16 y); 20],   // tiles with overlay class 0x23
}
```

- `FUN_004255da(x,y)` height (0 = invalid cell). `FUN_00427a10` cell→screen,
  `FUN_00425879` screen→cell (handles height), `FUN_0042681e` centre view on a
  cell, `FUN_00426fdb` scroll by key code.
- `FUN_00425fbe(x,y,&code)`: map object at cell; `code & 0x8000` = blocking object.
- `FUN_00427e1f(cell, unit, out[3])` → **terrain query**, returns 0 impassable,
  1 passable, 2 pass-through only (cannot stop); `out = (move cost, atk mod, def mod)`.
  See battle.md §Terrain.
- `FUN_004256db(x,y) >> 8` = overlay class of the cell (end-of-phase effects, exits = 6).
- `FUN_004281d7(x,y,u)` = non-flying unit may not stop on bridge-like tiles (class 1/2 with tile id > 4).

Terrain effect table (`DAT_00494728` = DAT entry 6): `i16 stride, i16 overlay_base,
i16 overlay_count`, then `[class(1..) ][stride] (i8 cost, i8 atk, i8 def)`
(`DAT_00494560`); per-terrain 11-byte info follows at `DAT_0049457c`.

## MoveRange (`DAT_004a2b48`) — `FUN_004709dc` … `FUN_00472c10`

```rust
struct MoveRange {
    /*00*/ nodes: *mut Node,       // 2466 nodes (0xbb2e bytes), memset -1
    /*04*/ count: i16,
    /*06*/ path: *mut u16, /*0a*/ path_len: i16,  // dest .. root (root = node 0)
    /*0c,10*/ scroll px snapshot, /*20,24,28,2a,2c*/ highlight blink timers
    /*14*/ unit: *mut Unit, /*18*/ army: *mut Army, /*1c*/ enemy: *mut Army,
    /*2e*/ order: *mut u16,        // node indices in creation order (0x2768 bytes)
    /*32*/ free: *mut u16, /*36*/ nfree: i32,
    /*3a*/ highlight tile buffer,
    /*42*/ flags: u16,             // 1 | arg; 2 if unit knows any magic/technique; 0x8000 = ignore units
    /*44*/ move_type: i32,         // FUN_0043aa26
    /*48*/ spell_cost: i32,        // rules[0xef] (+rules[0xf3] if magic/tech already used)
    /*4c*/ attack_cost: i32,       // rules[0xee] (−4 with ability 0x20000000, +rules[0xf3] if attacked w/o ability 0x8000)
}
struct Node { /*0*/ x: i16, /*2*/ y: i16, /*4*/ budget: i16, /*6*/ height: u8,
              /*7*/ can_stop: u8, /*8*/ parent: i16, /*a*/ child: [i16; 4],
              /*12*/ mark: u8 }       // 0 nothing left, 1 can attack after, 2 can cast after
```

## TargetList (stack object, 0x44 bytes) — `FUN_004069d1`, `FUN_00406a36`

```rust
struct TargetList {
    /*00*/ army: *mut Army, /*04*/ enemy: *mut Army, /*08*/ unit: *mut Unit,
    /*0c*/ capacity: i32, /*10*/ shape: i32,      // 2 = ring/area, else 4 rays (3 = piercing line)
    /*14*/ ncells: i32,
    /*18*/ ray_len: [i32; 4], /*28*/ ray_start: [i32; 4],
    /*38*/ cells: *mut (i32, i32),
    /*3c*/ targets: *mut (i32 unit_id, i32 x, i32 y, i32 facing),
    /*40*/ ntargets: i32,
}
```

## Duel (0x4a bytes) — `FUN_0040805a` (init), `FUN_00408078` (setup)

```rust
struct Duel {
    /*00*/ att_army: *mut Army, /*04*/ def_army: *mut Army,
    /*08*/ att: *mut Unit, /*0c*/ def: *mut Unit,
    /*10*/ skill: i32,          // AtkSkill index of the attack
    /*14*/ counter_skill: i32,  // -1 = defender can't counter
    /*18*/ fc_hit: i32, /*1c*/ fc_dmg: i32,      // forecast for the attack
    /*20*/ fc_chit: i32, /*24*/ fc_cdmg: i32,    // forecast for the counter
    /*28*/ att_prof: i32, /*2c*/ def_prof: i32,  // weapon proficiency of each side
    /*30*/ att_hp_lost: i32, /*34*/ def_hp_lost: i32,  // % of max HP missing
    /*38*/ att_acc: i32, /*3c*/ def_acc: i32,    // 戰技 after desperation roll (FUN_00409e98)
    /*40*/ facing: i32,         // 1 front, 2 side, 3 back (attacker relative to defender)
    /*44*/ att_terrain: [i8; 3], /*47*/ def_terrain: [i8; 3],  // (cost, atk, def)
}
```

## SpellCaster (`DAT_004a1f58`, ~0x604 bytes) — 0x45e000–0x46d000

| off | meaning |
|---|---|
| 0x00 / 0x04 | own / enemy army |
| 0x08 | caster |
| 0x0c | combo partner |
| 0x10 | MagicRec id (1-based) |
| 0x14 / 0x18 | target cell x / y |
| 0x1c / 0x20 | EXP accumulated for caster / partner |
| 0x24 | proficiency gain |
| 0x28 | item drop accumulator (`u8[items]`) |
| 0x38 | 1 = AI is evaluating (skip UI) |
| 0x3c | 1 = roll random (0 = expected value for AI) |
| 0x574 | number of categories in the menu |
| 0x575 | 0 magic, 1 technique |
| 0x576 | 1 = cast from an item/scroll |
| 0x577 / 0x580 / 0x58c | menu: category ids, per-category counts, skill indices |
| 0x5fc | effect animation buffer (BIN 9) |
| 0x600 | slot chosen by "destroy equipment" spells (0x22/0x26) |

## Rules constants (`DAT_0049456c`, 272 bytes in BIN entry 10)

Read as signed bytes unless noted. Offsets used by the battle code:

| off | use |
|---|---|
| 0x00–0x11 | nine variance pairs `(base, spread)`: value × (base + rand(spread+3)) / 10; forecasts use base + (spread+3)/2. 0/1 physical damage, 2/3 physical hit, 4/5 spell damage, 6/7 spell hit, 8/9 support amount, 0xa/0xb support hit, 0xc/0xd heal amount, 0xe/0xf ?, 0x10/0x11 EXP |
| 0x12–0xd5 | spell power formula constants (battle.md §Spell power) |
| 0xd6–0xdf | power rating divisors |
| 0xe0 | NPC magic proficiency factor |
| 0xe1, 0xe2, 0xe5 | EXP formula |
| 0xe8 | AP overhead of a move (and reserve) |
| 0xe9 | flyer AP cost per cell |
| 0xea | flyer AP cost per height step climbed |
| 0xeb | walker AP cost per height step climbed |
| 0xec | nominal AP per cell (AI reach estimates) |
| 0xed | max AP carried over / AI keep-AP margin |
| 0xee | AP cost of an attack |
| 0xef | AP cost of magic / technique |
| 0xf0 | AP cost of resting (and AI move margin) |
| 0xf1 | AP cost of using an item |
| 0xf2 | AP cost of changing equipment |
| 0xf3 | extra AP for repeating the same kind of action in a turn |
| 0xf4–0xf8 | enemy-side damage/hit scaling % by difficulty 1..5 (`0xf3 + difficulty`) |
| 0xfc / 0xfd | victory / defeat animation ids |
| 0x100 + d (u8) | AI weight multiplier for attacking a VIP, difficulty d |
| 0x105 + d (u8) | AI weight multiplier for a likely kill |
| 0x10b, 0x10c (u8) | AI score threshold = [0x10b]*100 + [0x10c]*turn |
| 0x10d (u8) | AI spell value % bonus |

## Static record fields read by the battle code

Field meanings the battle code depends on (TABLES owns the full layouts).

ClassRec (0x25): +0x0e u16 exp_next; +0x11 i8 class type (affinity row; 5, 6, 7, 0x0e "undead", 0x13, 0x14, 3, 4, 0x17 used by AI);
+0x12 u8 move class (1..5 ground, 6..8 air, 9 water, 10 amphibious; 7 → levitate,
8 → type 2, 6 → type 3); +0x13 u8 ability granted (bit index+1); +0x14 u8 weapon/special proficiency floor factor for AI units (FUN_00452af0 writes prof[kind−2] and prof[17], not school slots);
+0x15 i8 default AP; +0x16 u16 element resist mask (÷10); +0x18 u16 element weakness mask (×3);
+0x1a u8 charm (alignment); +0x1b i16 promotion item; +0x1d u32 promotion ability mask; +0x21 u8 power-rating factor.

AtkSkill (0x17, `DAT_00494590`): +0x00 i8 type (1..7 weapon types, 5 = bow, 0xb/0xc = "use magic/technique"
placeholders, 0x17 鷹襲, 0x20 魔法劍, 0x24 魔擊, ≥0x15 magic-based); +0x01 name; +0x0a i8 animation set;
+0x0b i8 power ×10; +0x0c i8 hit bonus; +0x0d i8 min range; +0x0e i8 max range; +0x11 i8 on-hit effect;
+0x12 i8 on-hit chance; +0x13 i8 on-hit duration; +0x16 i8 shape.

ItemRec (0x26): +0x02 name; +0x0e i8 type (1..6 weapon kinds, 7..11 armour slots, 12 consumable);
+0x0f i8 0 = equipped-effect item; +0x10 i8 range code; +0x11 i16 ATK / effect amount;
+0x13 i8 hit; +0x14 i8 crit; +0x15 i8 DEF; +0x16 i8 MAG; +0x17 i8 evasion; +0x18 i8 AP;
+0x19 i8 ability code (see battle.md §Item abilities); +0x1e i8 shape override;
+0x1f u32 class-type mask; +0x23 u16 restriction flags.

MagicRec (0x25, `DAT_004a2560`): +0x00 u8 category (1-based); +0x01 u8 index in category (1-based);
+0x02 name; +0x0d i16 cost; +0x0f i16 required proficiency (×10 units); +0x11 u8 targeting mode;
+0x12 u8 area; +0x13 u8 effect type; +0x14 u8 power formula; +0x15 i16 base power;
+0x17 i16 base chance; +0x19 i16 duration code; +0x1b u16 element mask; +0x1d u16 flags;
+0x1f u8 target side (0 self, 1 ally, 2 enemy, 3 all); +0x20 u8 (&7 partners needed, &6 couple combo,
&0x80 special learn rule); +0x21 u8 (1 MP/MAG, 2 ST/劍術, 4 HP/fixed, 0x10/0x20/0x40 facing table, 0x80 height);
+0x23 i16 AI value.

Unit definition record (0x65; stage scripts / BIN tables): +0x00 char id; +0x02 name; +0x0f flags (low nibble → sex);
+0x10 kind; +0x12 level; +0x14 ai_mode; +0x15; +0x16 ai_sub; +0x17/+0x19 x/y; +0x1b HP, +0x1d MP, +0x1f ST,
+0x21 MAG, +0x23 反應, +0x25 ATK, +0x27 DEF, +0x29 劍術, +0x2b 戰技; +0x2d charm; +0x2f AP; +0x31 control type;
+0x32 facing; +0x34 appear turn; +0x35 bounty; +0x37 drops[4]; +0x3f equipment[6]; +0x4b abilities; +0x4f learned[11].
