# Text, fonts and stage scripts

Reader code: `crates/s2data/src/{text,font,msg,script}.rs`. Dumper:
`s2text` (`stages`, `strings`, `font-sheet`, `dialogue`, `markup`).
Addresses are functions in the unpacked `WSUPERSP.EXE`.

Strings are Big5. The game's walkers treat any byte < 0x81 as a one-byte
character and anything else as the lead of a two-byte one; strings end at
NUL or `\n`. `*` is the line break inside long texts.

The `#F066`/`#E072` strings in the exe are only error-location tags passed
to the error box `FUN_0046cf8f` (the global at 0x48e600); they are not
message ids.

## SUPERSP.BIN entries

| id | size | contents | loader / owner |
|----|------|----------|----------------|
| 1 | 625274 | stage scenarios (168 stages) | `FUN_00422b8e`; this doc |
| 2 | 4657 | unit class (兵種) table, 0x25-byte records | `FUN_0045e4cf`; TABLES |
| 3 | 39736 | ending narration, credits, 8 epilogue variants | `FUN_0041b5dc`; this doc |
| 4 | 21440 | world-map locations (one per stage): position, shops, gossip | `FUN_0040fd3c`; this doc |
| 5 | 4944 | 275 UI strings | `FUN_00424122` |
| 6 | 4942 | UI strings, 簡體版 wording (存盤, 內存 …), used if `DAT_004944f4` | same |
| 7 | 335 | FLC movie list (`LOGO.FLC`, `SPMARK.FLC` …), 0x15-byte records | `FUN_004593dd`; GRAPHICS |
| 8 | 21490 | 0x6c- and 0x32-byte stat records | `FUN_004233d1`, `FUN_0042345b`; TABLES |
| 9 | 7488 | 115 spell descriptions | `FUN_0045f57c` |
| 10 | 871 | per-effect tables for the 115 spells/effects (sound, type …) | `FUN_0042422b`; TABLES/GRAPHICS |
| 11 | 30284 | Big5 glyph index | `FUN_0041eb94` |
| 12 | 71968 | 2249 glyphs 16x16 | same |
| 13 | 161928 | 2249 glyphs 24x24 | same |
| 14 | 71968 | 2249 glyphs 16x16, simplified shapes (簡體版) | same |
| 15 | 3840 | 256 ASCII glyphs 8x15 | `FUN_0041de58` |
| 16 | 12288 | 256 ASCII glyphs 16x24 | same |
| 17 | 9981804 | sound effects ("soundsp") | `FUN_0041a01c`; AUDIO |
| 18 | 212214 | MIDI music ("midisp") | `FUN_0041a152`; AUDIO |

`DAT_004944f4` is the 繁體版/簡體版 switch offered by the original installer
(DAT entry 8 holds the installer's strings). The shipped exe forces it to 0
in the config loader (`FUN_00423b00`), so BIN 6 and 14 are unused.

Text in DAT: entry 2/3 = item help (same format as ITEMHLSP.BIN, which the
game prefers when present), entry 8/9 = installer strings.

## Fonts — `FUN_0041df60`, `FUN_0041f748`, `FUN_0041f87c`, `FUN_0041fa6c`

Glyph lookup for a Big5 code `hi lo` (`FUN_0041ecd6`):

```
if hi == 0xc6 and lo > 0xa0: slot = lo + 0x7f60
else:
  a1..a3: base 0x8400, row = hi-0xa1      a4..c6: base 0x8800, row = hi-0xa4
  c7..c8: base 0x805f, row = hi-0xc7      c9..f9: base 0x9d19, row = hi-0xc9
  col = lo-0x40 (40..7e) or lo-0x62 (a1..fe); otherwise unmapped
  slot = row*0x9d + col + base
glyph_no = u16 at BIN11[slot*2 - 0x10000]      (1-based)
bitmap   = BIN12[(glyph_no-1)*32]  or  BIN13[(glyph_no-1)*72]
```

ASCII glyph = byte value (bytes < 0x1e drawn as 0x1e) into BIN 15 (15 B) or BIN 16 (48 B).

Bitmaps are 1bpp, MSB first. Each glyph is expanded into a cell and blitted
with index 0 transparent:

| kind | cell | bitmap at (col,row) | used by |
|------|------|--------------------|---------|
| Big5 16 | 18x18 | (1,0) | compact text `FUN_0041df60` (advance 16) |
| Big5 24 | 26x26 | (1,0) | normal text `FUN_0041f748` (advance 24) |
| ASCII 8x15 | 10x18 | (1,1) | compact text (advance 8) |
| ASCII 16x24 | 18x26 | (1,1) | normal text, drawn at x+2 (advance 16) |

Styles (`font+0x10`): 2 = plain main color; 1 = shadow (second color at
+1,+1, then main); 0 = outline: horizontal passes then vertical passes set
empty pixels next to main-color pixels to the second color (4-neighbour).
Pass ranges differ per cell and are reproduced in `font.rs`.

`FUN_0041f87c` (boxed text): `lines` rows of up to `cols*2-1` bytes; each
character takes a 28 px slot, ASCII is pulled 8 px left per preceding ASCII
char; `*` ends a row (ignored at row start); after the last row it wraps to
the top and keeps drawing.

## UI and help strings

- BIN 5/6: `i16 n; (u32 off, u32 len)[n]; blob` (offsets relative to the
  blob). The exe indexes them as `DAT_00494768[i]` (decompiled as `+ i*4`).
  `s2text strings` → `ui.txt`.
- BIN 9: `(u32 off, u16 len)[]`, absolute offsets; count = first offset / 6.
- ITEMHLSP.BIN / DAT 3: `i16 n` then n NUL-terminated strings (entry 0 is a title).
- BIN 3 endings:
  ```
  i16 n_pages (3), i16 n_variants (8)
  (u32 off, u16 len)[n_pages]                 narration ×2, credits
  (u32 off, u16 count, u16 size)[n_variants]
    at off: (i16 char_id, u32 text_off, u16 len)[count]   per-character epilogue
  ```
- BIN 4 locations, one per stage id:
  ```
  i16 n (168)
  (u32 off, u16 len, u16 x, u16 y, u8 kind, u8 shop_count[3])[n]
  at off: name\0, i16 item_id[sum(shop_count)], gossip\0, gossip\0
  ```

## Dialogue markup — `FUN_00418e73`

Plain text is typed out one character per delay tick. Control blocks
`@@ … @@` are only recognised where the next character would be printed:

| command | effect |
|---------|--------|
| `Wcc,ss` | text color `cc`, second color `ss` (hex) |
| `Ddd` | per-character delay (hex) |
| `RW` | color back to 0x2a/0 |
| `RD` | delay back to the configured speed `DAT_0049463c` |

Quirks kept by `msg::parse_dialogue`: one character is always printed right
after a block; an unknown command ends the block and printing resumes at
that byte (`R?` one byte later). Data uses colors 0xa7/0xa3 and delays
0x10..0x6d (31 lines). Layout constants are in `msg::layout`. The box
origin is `(6, 0x7d)` unless the speaker is on the map; text starts at
(+0x8f, +0x1d), 25 px lines, wrap at x ≥ +0x23f, page full at y ≥ +0xb2
(the "more" arrow, UI image 0xec, sits at +0x244,+0xb7), the speaker name
is centered on +0x4a at +0xa8 (color 0x36, outline 0).

## Stages (BIN entry 1) — `FUN_00422b8e`

```
i16 count (168)
StageHeader[count]  (30 bytes each, copied to DAT_00494688)
stage blocks
```

StageHeader:

| off | type | field |
|-----|------|-------|
| 0x00 | u32 | block offset in BIN 1 |
| 0x04 | u32 | block size |
| 0x08 | u8 | deployment record count |
| 0x09 | u8 | battle map id (PRG entry 6, `FUN_00429336`) |
| 0x0a | u16 | music track at stage start |
| 0x0c | u16 | next stage |
| 0x0e | u16 | unused |
| 0x10 | u8 | stage taken via hidden exit tile 0xff (0 = none) |
| 0x11 | u8 | stage taken via hidden exit tile 0xfe |
| 0x12 | u8[3] | initial story flags 1..3 |
| 0x15 | u8 | kind: 1 battle, 2 branch point, 3 no battle phase |
| 0x16 | u8 | chapter/location number (not read by stage code) |
| 0x17 | u8 | enemy scaling base level |
| 0x18 | u8 | enemy scaling step (`FUN_0040d729`: enemy power ±25%/−10% per step, ±4 steps, plus (difficulty−3)×5%) |
| 0x19 | u8 | reward group count |
| 0x1a | u32 | deployment table offset in block |

Stage block:

```
(i16 kind, i16 count)[reward_groups]     1 join, 2 leave, 3 gold, 4 items, 5 treasure spots
i16 values[...]                          consumed in order (gold uses one)
Deploy[unit_count]                       0x65 bytes: i16 char_id, char name[14],
                                         i16 class @0x10, i16 x @0x17, i16 y @0x19, … (army code)
u16 len[7]
name[len0]  briefing[len1]  win_text[len2]
i16 win_set1[len4]  i16 win_set2[len5]
lose_text[len3]  i16 lose_codes[len6]
script                                   (rest of the block)
```

Win (`FUN_00433257`, either set, every code in it must hold; the winning
set number goes to `DAT_004952cc`): 0 all enemies down; 1..20 treasure
spot n taken; 0x1f protagonist on an exit tile (map attr 6) with flag 1;
0x20 any active non-leader on an exit tile; 0x21 all enemy leaders
(`unit+0x2f == 1`) down; 0x22 all other enemies down; anything else never
holds (0x23 is used as "the script decides"). Flag 4 forces a win (set 0).

Lose (`FUN_00433161`, any code): 0 every main-party unit down; 1..0x2b that
character dies; 101..190 turn (code−100) reached; turn 100 always loses;
flag 5 forces a loss. The protagonist is char 11 (白志超) or 1 (娜塔莉亞)
depending on `DAT_00497b54`.

## Script and events

```
i16 count, u16 size
Event[count]             20 bytes each
line tables + text pools (one per event, contiguous, addressed from script start)
```

Event:

| off | type | field |
|-----|------|-------|
| 0x00 | u8 | type (0 = consumed) |
| 0x01 | i16 | sub |
| 0x03 | i16 | a (usually the precondition flag 1..80) |
| 0x05 | i16 | b |
| 0x07 | i16 | c |
| 0x09 | i16 | d |
| 0x0b | i8 | flag to set when fired (1..80) |
| 0x0c | u32 | line table offset |
| 0x10 | u16 | size of line table + text pool |
| 0x12 | i16 | line count |

Line (0x17 bytes): `i16 speaker, i16 music_before, i16 music_after,
char name[13], i16 text_off (into the pool after the table), i16 text_len`.
The speaker id picks the portrait (CEL entry 3, frame `id*2-2`) and the unit
the box is drawn next to.

All 168 scripts parse with every byte accounted for (`s2text stages`):
2364 lines; events by type 1:245 2:74 3:101 4:0 5:98 6:35 7:29 8:39 9:199
10:30 11:153 12:179 13:9 14:100 15:8 16:35.

### Firing

When an event fires: if it has lines they are shown (`FUN_004188ca`), its
type byte is zeroed, the flag at +0x0b is set and `FUN_0041be7a(flag)`
runs (flag 1 reveals the attr-6 exit tile and its name, flag 3 the attr-0xb
tile, flag 7 calls `FUN_0045ceb9` on the enemy army). Then type-9/sub-0
events whose flag is now set are run in a chain (`FUN_00418203`).

Poll points (`DAT_00492bd0` = script object):

| function | when | types |
|----------|------|-------|
| `FUN_00417a1d` | start of each turn | 1 |
| `FUN_004178fa` | after actions, after each fired event | 4–16 via `FUN_00417587` |
| `FUN_00417b74` | after victory | 2 |
| `FUN_00417cbd` | after the enemy phase | 3/1 |
| `FUN_00417f26` | unit defeated (`FUN_0044d14b`) | 3/2 |
| `FUN_00417d8e` | before a battle | 3/3 |

Events are skipped entirely in two-player mode (`DAT_00494668 == 1`).

### Event table

`[a]` = requires story flag `a` (1..80) to be set, otherwise the event is
not checked. "fires" = condition handler returns true (the event then
fires as above).

| type | sub | meaning | handler |
|------|-----|---------|---------|
| 1 | 0 | start of turn `a` | `FUN_00417a1d` |
| 2 | 0 / n | victory (any set / set n) | `FUN_00417b74` |
| 3 | 1 | all enemies down after the enemy phase | `FUN_00417cbd` |
| 3 | 2 | character `a` defeated (last words; returns 0x12) | `FUN_00417f26` |
| 3 | 3 | battle: attacker char `a` vs defender char `b` | `FUN_00417d8e` |
| 4 | 0 | chars `a` and `b` adjacent (height diff < 4); no flag check | `FUN_0041c267` → `FUN_00441326` |
| 5 | 0 | [a] choice menu: line 0 prompt, lines 1.. options; result → flag-12 byte | `FUN_0041929b` |
| 5 | 1..4 | [a] last choice == sub (clears flag a) | `FUN_00417587` |
| 6 | 0/1/2/3 | [a] deploy / withdraw chars b,c,d (animated / animated / silent / silent) | `FUN_0041c302` |
| 7 | 0,1 | [a] move char b to (c,d) (mode 0/1) | `FUN_00458703` |
| 7 | 2,3 | [a] move char b to char c's tile | |
| 7 | 4 | [a] warp char b to (c,d) | `FUN_00459131` |
| 7 | 5 | [a] warp char b to char c | |
| 8 | 0/1 | [a] camera center / scroll to (b,c) | `FUN_0041c437` |
| 8 | 2/3 | [a] camera center / scroll to char b | |
| 8 | 4 | [a] center on (b,c) (or char b if c = 0) then screen effect | |
| 8 | 5 | [a] effect animation b on char c | `FUN_0046cd1c` |
| 8 | 6 | [a] effect animation b at (c,d) | `FUN_0046cd78` |
| 8 | 7 | [a] wait b | `FUN_0046d652` |
| 8 | 8 | [a] redraw | |
| 9 | 0 | [a] flag a set (also chained after other events) | `FUN_0041c81c` |
| 9 | 1/2 | [a] flags b,c,d all set (2: then clear them) | |
| 9 | 3/4 | [a] any of b,c,d set (4: then clear them) | |
| 9 | 5 | [a] clear flags b,c,d | |
| 9 | 6 | [a] next stage = b (`DAT_00494678`) | |
| 9 | 7 | `a` is in the list at `DAT_00492cbc` (no flag check) | |
| 10 | 0 | [a] give item b ×c | `FUN_0041cb9a` |
| 10 | 1/2 | [a] party has / lacks item b | |
| 10 | 3 | [a] enemy army has item b | |
| 10 | 4/5 | [a] char c holds / lacks item b | |
| 10 | 6 | [a] take item b from char c (needs party char d) | |
| 10 | 7 | [a] `FUN_00446c6b(party, b)` | |
| 11 | 0..6 | [a] char b is dead / alive / in roster / not in party / retreated / on map / neither | `FUN_0041ce89` |
| 11 | 7 | [a] `FUN_0045d922(char b, c)` | |
| 11 | 8,9 | [a] `FUN_0045d8b2(party, b, 0/1)` | |
| 11 | 10 | [a] `FUN_00442eb7(party)` | |
| 11 | 11/12 | [a] 名聲 fame `DAT_00497b48` / 評價 rating `DAT_00497b4c` += b | |
| 11 | 13 | [a] `FUN_00441038(party)` | |
| 11 | 14 | [a] party setup screen (`FUN_00433d8f`) | |
| 12 | op | [a] action on char b (ops 0,1 always; 10..28 only if the unit is on the map) | `FUN_0045bc6a` |
| 13 | 0 | [a] difficulty (`DAT_00494684`) `<` / `==` / `>` c for b = 1/2/3 | `FUN_0041d280` |
| 13 | 1/2 | [a] play sound / music b (0 = stop) | |
| 13 | 3 | [a] show picture (b,c,d), then resume music | `FUN_0041b963` |
| 13 | 4/6 | [a] flags 27..29 = b,c,d (picture) / flag 30 = b (movie), shown after a won stage by `FUN_0041bc3b` | |
| 13 | 5 | [a] play movie b | `FUN_0041a631` |
| 14 | 0 | [a] char b stat[c] += d (c 1..40, clamped to i8) | `FUN_0041d484` |
| 14 | 1 | [a] `FUN_0045d9d1(party, b, c)` | |
| 14 | 2/3/4 | [a] char b stat[c] `>` / `==` / `<` d | |
| 15 | 0 / 3 | [a] group walk, party / enemy, b members; members follow as type 15 with the same `a`: sub 1/4 `(char b → c, facing d)`, terminated by sub 2/5 | `FUN_0041d6b9` |
| 16 | 0 / 4 | [a] group move, party / enemy, b members, mode c; members sub 1/5 `(char b → c,d)`, sub 2/6 `(char b, c)`, end sub 3/7 | `FUN_0041d9ec` |

Type 12 ops (`FUN_0045bc6a`): 0 reset to deployable; 1 mark retreated;
10 face/step (`FUN_0044ebb2(d, c<<2)`); 11 set +0x2d=2; 12 `FUN_0040a443`;
13/14/15 turn toward c (4 / 8 / class-dependent steps); 16 set facing c;
17 reset+defeat; 18 defeat; 19 reset+remove; 20 remove; 21 move to the
other army; 22 rejoin; 23 damage c; 24 damage c% of max HP; 25 set +0x0a=c;
26 full restore; 27/28 `FUN_0044bfb2`. Others: no effect.

Sub-ops a handler has no case for still count as fired ("fires" with no
effect) for types 6–8 and 10–14; for 1, 3, 5, 9 they never fire. Type 3/2's
handler also tests subs 1/3/4 after requiring sub == 2; those branches are
dead in the shipped exe.

### Story flags (`DAT_00494507 + n`, n = 1..80)

Flags 1..40 are per stage: cleared when a stage ends (`FUN_00423aea`, in
`FUN_0040d108`) and 1..3 seeded from the header when it loads. Flags 41..80
are campaign-wide and only cleared on new game / reset (`FUN_00423ad4`).
Some low flags have engine meaning:

| flag | meaning |
|------|---------|
| 1 | exit tiles (attr 6) usable; required by win code 0x1f |
| 2 | attr-5 tiles usable; skip closing box animation on the last line |
| 3 | set by event → reveal attr-0xb tile |
| 4 | attr-0xb tiles usable; forces victory |
| 5 | forces defeat |
| 6, 8 | one-shot notices in battle (`FUN_0044fae0`) |
| 7 | set by event → `FUN_0045ceb9(enemy army)` |
| 9, 10 | stage seals: player army can't use magic / techniques |
| 27..29, 30 | picture / movie to show after a won stage (`FUN_0041bc3b`; set by type 13/4, 13/6) |
| 11 | abort the current event poll (cleared by `FUN_00417f26`) |
| 12 | holds the last choice result (1..4), not a boolean |

Save files store only each event's type byte (`FUN_0041745a` /
`FUN_00417369`): a 0 marks it consumed.
