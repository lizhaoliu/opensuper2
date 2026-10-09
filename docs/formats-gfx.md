# SuperSp graphics formats

Companion to `formats.md`. Code: `crates/s2data/src/{sprite,anim,gfx_*}.rs`,
dumper `crates/s2tool/src/bin/s2gfx.rs` (`s2gfx --game DIR dump-all OUT`).
All entries below were decoded completely and checked visually.

## Frame encodings (`sprite.rs`)

Every frame starts with `i16 w-1, i16 h-1`. Index 0 is transparent.

| encoding | body | decoder in the exe |
|----------|------|--------------------|
| RLE | `00` = 1 transparent px; `80|n` = n literal bytes; `n` = next byte n times; runs cross rows | `FUN_00420e5c`, blit `FUN_0046af47` |
| raw | `w*h` bytes | blit `FUN_0046f629` (param 1 = opaque) |
| flagged | `u8 flag` then RLE (flag 0) or raw frame | `FUN_0046ed81` |

Most banks are `u16 n; (u32 offset, u32 size)[n]; frames` (offsets relative
to the entry) → `ImageBank`.

## Where everything lives

| archive entry | content | encoding | module |
|---------------|---------|----------|--------|
| CEL 1 | 133 spell/effect frame sets | RLE, `FrameSet` | `sprite.rs` |
| CEL 2 | 653 UI images, ids 143..795 | RLE bank | `gfx_ui.rs` |
| CEL 3 | 167 portraits x 2 (dialogue 100x124, battle 92x111) | flagged bank | `gfx_portrait.rs` |
| CEL 4 / CEL 5 | summon sprite index / frames (11 types) | see units | `gfx_unit.rs` |
| PRG 1 / PRG 2 | unit sprite index / frames (136 types, 4417 frames) | see units | `gfx_unit.rs` |
| PRG 3 | 4284 ground tiles 64x48 | raw bank | `gfx_map.rs` |
| PRG 4 | 896 overlay tiles 64x96 | raw bank | `gfx_map.rs` |
| PRG 5 | 360 map objects (64x96 .. 256x256) | raw bank | `gfx_map.rs` |
| PRG 6 | 132 battle maps | see maps | `gfx_map.rs` |
| PRG 7 (= PRG 9) | tile attributes | `u16 n3,n4,n5; u16[n3+n4+n5]` | `gfx_map.rs` |
| PRG 8 | 50 map backdrops, up to 1920x1280 | flagged bank | `gfx_map.rs` |
| DAT 5 | 142 UI images, ids 1..142 (cursors, range diamonds, 16-way projectiles) | raw bank | `gfx_ui.rs` |
| DAT 7 | 11 UI screens/windows (status screen, lists, buttons) | flagged bank | `gfx_ui.rs` |
| DAT 10 | 14 images, unreferenced in v1.4, palette unknown | RLE bank | `gfx_ui.rs` |
| DAT 11..20, 26..40 | full-screen PCX pictures with palettes | PCX | `gfx_pcx.rs` |
| DAT 21..25 | cutscenes (FLC) | FLC | `anim.rs` |
| BIN 7 | cutscene table (names + per-frame audio events) | | `anim.rs` |

`SUPERSP\SUPERSP.PRG` (disc copy) has entries 1..8 byte-identical to the
installed `SUPERSP.PRG`; its tail holds a second copy of PRG 6 instead of the
PRG 7 copy (entry 9 is never read). `invsoke.cel`/`invoke.ind`/`man cell`/
`man index` are only error-message names for CEL 5/CEL 4/PRG 2/PRG 1.

## Palettes (`gfx_palette.rs`)

* Master palette = first 768 bytes of SUPERSP.PAL, 6-bit. The game expands
  with `<< 2` (not `(x<<2)|(x>>4)`): `palette.rs::Palette::from_vga` differs
  slightly; use `gfx_palette::vga_to_rgb` for exact output.
* PCX pictures carry 8-bit palettes (game uses `>> 2`); FLC frames set their
  own (COLOR_256 `>> 2`). DAT 20 uses DAT 19's palette.
* Fade (`FUN_0046d442`): 64 steps, entries 1..255 scaled by `(step<<2)/255`.
* Nearest colour: k-d tree over the master palette (entry 0 forced black),
  axes G,R,B, windowed search ±32, index range 1..255 at build and 0x20..0xfe
  after `FUN_0045e3c7`. Ported verbatim (`ColorTree`).
* Blend `FUN_0045e123`: `src*p + dst*(100-p)` per 6-bit channel, nearest.
* 13 tint tables (`TINTS`, `FUN_0045e3c7`): toward 0x21 at 0..40%, 0x20 at
  60/50/25%, 0xea 65%, 0xbd 60%, 0x24 15%, 0x3c 15%, 0x44 80%. Table 6 and 4
  implement the shadow colours 0x20 / 0x21 of overlays/objects.

## Spell effects (CEL 1 frame sets)

16-byte header: frame count, anchor (x, y), size (w, h), remap count,
sequence length (>0 = frame order table, <=0 = extra loops), delay (cs). Remap
tables list translucent colours and their opacity percent; drawing blends
them with the destination through the colour tree and caches a 256-byte LUT
per colour (`BlendCache`). Placement: anchor (-1,-1) = full screen at (0,0);
(0,0) = bottom-centred on the target tile (`x+32-w/2, y+32-h`) and occluded
like a unit; otherwise `(x+32-ax, y+32-ay)` where (x, y) is the cell's screen
position (`FUN_00427a10`). Player: `FUN_00466507`.

## Unit sprites (`gfx_unit.rs`)

Index (PRG 1 / CEL 4): `u16 n; (u16 frames, u16 first, u32 bytes)[n];
u8 actions[n][200]`. Action blocks are NUL-separated 1-based frame-number
strings: actions 0-3 stand, 4-7 walk, 8-11 attack, then more groups of four
(one per facing). Frames (PRG 2 / CEL 5): `u16 n; (u32 off, u16 size, i8 dx)[n]`,
RLE, mostly 128x160. Summon types are ids 0x71.. (`unit+0x30 & 0x10000000`).
Draw position: `BattleMap::unit_position` (`FUN_0042665a`).

## Battle maps (`gfx_map.rs`)

PRG 6 map: 14-word header (W, H, cells, 64, 32, 16, objects, flags, backdrop
id/x/y, foreground id/x/y), cells `(u8 height, i16 ground, i16 overlay)`,
objects `(col, row, w, h, image)`, flags `(col, row, u16)` (0x4000 = overlay
stays in front of objects; 0x8000 and low byte are stage logic), then tile
usage tables. Rows are 16 px apart, odd rows offset 32 px; even rows lose
their first/last cell. Background build (`MapRenderer::render_background`,
`FUN_00424d81`): backdrop opaque → per row: objects whose `row*16+h` is
reached (each followed by re-drawing flagged overlays in front of it), then
each cell's ground tile stacked `height` times (ids < 6 drawn once) and its
overlay → objects past the last row → foreground. Overlay category (PRG 7
high byte) 0xc = doors (left to stage logic), 5/6/0xb = optional decor.
Sprite occlusion (`MapRenderer::occlude`, `FUN_00426417`): clears sprite
pixels covered by the 12 rings of cells in front, overlays and objects; mode
1 = checkerboard.

## Full-screen pictures (`gfx_pcx.rs`)

PCX v5 8-bit. DAT 11 world map 1280x907; 12..17 town/shop/temple scenes; 18
world map (dark); 19 title; 20 title dimmed; 26 sea; 27..39 character art;
40 world map (darker). Positive ids are relative: DAT entry = id + 0x19.

## Cutscenes (`anim.rs`)

FLC with an obfuscated header (swap 0x00/0x50 dwords and 0x06/0x0e words;
subtract 0x923 magic, 0x38 width, 0x0b height, 8 depth). DAT 21 LOGO (640x360),
22 SPMARK (opening), 23 SPOVER, 24 END-1, 25 END-2. Delay `speed*10/48` cs.
Quirks kept: COLOR_64 also `>> 2`, DELTA_FLC word runs fill with the low byte,
COPY reads w*h bytes even when the chunk is shorter (END-1 frames 84, 100).
BIN 7 gives per-frame music/sound triggers.

## Timing

`FUN_0046d5d6` returns centiseconds; all frame delays above are in cs.

## Notes for the other sub-tasks

* AUDIO: BIN 17 = 245 RIFF WAV samples (`FUN_0041a01c`); BIN 18 = 37 XMIDI
  songs, `FORM XDIR` (`FUN_0041a152`) — the MIDI music is in the data after
  all; cutscene sound/music events in BIN 7.
* TEXT: BIN 11..16 look like font bitmaps (BIN 15 = 3840 bytes, 8x15 ASCII).
* TABLES: PRG 7 ground attribute low byte = terrain type (name table
  `DAT_0049457c`, 11-byte records); map flag records (0x8000, low byte = door
  index checked against `DAT_00494598`); BIN 2 spell table (`FUN_0045e4cf`)
  and BIN 9 per-spell effect scripts (`FUN_0045f57c`, records `u32 off, u16 len`);
  DAT 6 (`FUN_00423a58`), DAT 8/9 unidentified small tables.
