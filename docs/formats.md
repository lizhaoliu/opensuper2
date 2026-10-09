# SuperSp (超時空英雄傳說2 北方密使) data formats

Reverse-engineered from the Windows "特殊版" v1.4 (2003, DirectX 7) executable
`WSUPERSP.EXE` (PECompact2-packed; unpack with `tools/unpack_pec2.py`).
Function names like `FUN_0046dedd` refer to the Ghidra decompilation of the
unpacked image (image base 0x400000, OEP 0x47b11f).

All multi-byte values are little-endian. Text is Big5.

## Files in the game directory

| File | Role |
|------|------|
| `SUPERSP.PAL` | master palette (768 bytes, 6-bit VGA) + obfuscated archive index |
| `SUPERSP.BIN` | archive "SuperSp_B" v1.3, 18 entries |
| `SUPERSP.CEL` | archive "SuperSp_C" v1.1, 5 entries (sprite/effect frame sets) |
| `SUPERSP.PRG` | archive "SuperSp_P" v1.1, 9 entries (installed copy, used when `DAT_004944f0 != 0`) |
| `SUPERSP\SUPERSP.PRG` | disc copy of PRG (differs in size; TBD) |
| `SUPERSP\SUPERSP.DAT` | archive "SuperSp_W" v1.1, 41 entries; index has a disc-1 and a disc-2 layout |
| `FSTAGE.BIN`, `KINDSP.BIN`, `ITEMSP.BIN`, `ITEMHLSP.BIN` | loose tables (copies also inside DAT) |
| `SUPERSP.CFG` | text config: `DEVICE=`, `VOLUME=`, `CDDA=`, `SET=`, `Version=` |
| `SAVE\UJnn.SAV` | save slots |
| `DISK.ID` | which CD is "inserted" (ASCII digit) |
| `midisp.bin` | MIDI music bank (referenced by the exe; absent from the repack) |

Music was CD audio (Miles `AIL_redbook_*`) or MIDI (`AIL_init_sequence`).
Sound effects are Miles samples (`AIL_set_sample_file`).

## Archive header (28 bytes)

```
u32  data_len      file length - 28
[4]  "U&J\0"
[10] "SuperSp_X\0"  X = B/C/P/W
[4]  version "1.1\0" / "1.3\0"
[2]  pad
u32  entry count
```

## Archive index (SUPERSP.PAL + 0x300) — `FUN_004217b9`

`u32` words. Five tables in this order with obfuscation bases:

| k | table | base |
|---|-------|------|
| 0 | CEL | 0x38 |
| 1 | BIN | 0x0b |
| 2 | DAT (disc 1) | 0x08 |
| 3 | PRG | 0x0a |
| 4 | DAT (disc 2) | 0x14 |

- `word[k] - base_k` = entry count of table k
- `word[5+k] - base_k` = expected `data_len` of that archive
- from `word[10]` on: the tables, each entry `(offset, size)` with `base_k + i`
  added to both words (i = entry index).

Entries are addressed 1-based: `FUN_0046dedd(kind, id)` loads a whole entry,
`FUN_0046e13b(kind, id, &span)` opens the file positioned at it. kind: 0=BIN,
1=CEL, 2=PRG, 3=DAT.

## RLE frame — decoder `FUN_00420e5c`, blitter `FUN_0046af47`

```
i16 width-1, i16 height-1
stream (row-major, runs cross rows):
  0x00      transparent pixel
  0x80|n    n literal bytes
  n         next byte repeated n times
```

Screen is 640x480, 8-bit indexed; index 0 is transparent.

## Frame set (CEL entry 1 sub-entries) — `FUN_0045e9c1`

CEL entry 1 starts with `u16 n; (u32 off, u32 size)[n]` (offsets relative
to the entry). 133 frame sets = spell/effect animations. Each set:

```
i16 frame_count
i16 anchor_x, anchor_y   (-1,-1: full-screen; 0,0: centered on unit; else offset)
i16 width, height
i16 remap_count          then remap_from[n], remap_to[n]
i16 sequence_len         >0: u8 frame order[n] (1-based); <=0: -n extra loops
u16 frame_delay
(u32 offset, u32 size)[frame_count]   relative to frame data start
frame data (RLE frames)
```

## Open questions

- CEL entries 2–5, PRG entries, BIN entries, DAT entries.
- Font, scripts, maps, unit tables, save format.
