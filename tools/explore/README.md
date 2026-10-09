# tools/explore — one-off exploration scripts

Scripts written while reverse-engineering the data formats. Their findings are
in `docs/formats*.md` and the Rust parsers (`crates/s2data`); they are kept for
reference, not maintained, and several hard-code paths under
`~/games/super2sp/`. They import `s2.py` from this folder (run them with
`tools/.venv/bin/python -I tools/explore/<script>.py`). Data is read from the
user's game copy; nothing from the game is ever executed.

| script | what it was for |
|---|---|
| `s2.py` | shared helpers: archive entries of SUPERSP.CEL/BIN/PRG/DAT, palette, index tables |
| `audio_probe.py` | BIN 17/18 sound archives: entry table and first bytes |
| `audio_probe2.py` | per-id sound durations, compact listing |
| `audio_probe3.py` | BIN 7 records (music / sound cue tables) |
| `audio_probe4.py` | BIN 1 stage headers: music cue per stage |
| `audio_probe5.py` | BIN 10: per-effect sound bytes after the per-spell arrays |
| `audio_probe6.py` | BIN 18 chunk structure (nested sample chunks) |
| `dump_cel1.py` | CEL 1 (spell effect frames) decoded to PNGs |
| `dump_frames_sheet.py` | CEL 1 frames tiled into contact sheets |
| `flow_contact.py` | tile s2run screenshots at 1/4 scale into one sheet |
| `flow_msgs.py` | BIN 5/6 message tables (UI strings) |
| `flow_pcx.py` | PCX decoder for the DAT pictures (title, town backdrops) |
| `flow_screens.py` | per function: UI message ids referenced, their text, callers |
| `flow_sheet.py` | sysimg images into a grid labelled by position |
| `flow_sysimg.py` | dump every sysimg (system UI image) as PNG |
| `flow_thumbs.py` | downscale several PCX DAT entries into one sheet |
| `gfx_big.py` | montage of the large UI images, scaled down 4x |
| `gfx_cel3.py` | CEL 3 frame sets |
| `gfx_crop.py` | crop + scale a region of an indexed PNG |
| `gfx_dat10.py`, `gfx_dat10b.py`, `gfx_dat10c.py` | DAT 7 / DAT 10 picture sets and their palettes |
| `gfx_dat7.py` | DAT 6–10 entry headers |
| `gfx_flc1.py` | FLC movie headers (fixed up) and first frames |
| `gfx_map1.py`, `gfx_map2.py` | PRG 3–5 map tile banks |
| `gfx_montage.py` | CEL 2 UI images as a montage |
| `gfx_palguess.py` | which palette fits which image set |
| `gfx_pcx.py`, `gfx_pcx_montage.py` | PCX entries decoded and tiled |
| `gfx_pngmontage.py` | montage of our own indexed PNG dumps |
| `gfx_prg8.py` | PRG 8 entries |
| `gfx_probe.py`, `gfx_probe2.py`, `gfx_probe3.py` | first looks at the CEL/PRG entry formats |
| `gfx_rawcheck.py` | PRG 3–5 run-length sanity checks |
| `gfx_ui.py`, `gfx_ui2.py` | UI image and small-icon sheets |
| `gfx_units.py` | unit sprite frames from the index + data banks |
| `probe.py` | archive entry 0 first bytes |
| `probe_bin1.py`, `probe_stage.py` | BIN 1 stage records (Big5 texts) |
| `probe_font.py` | BIN 11–14 font index and glyph banks |
| `probe_index.py` | SUPERSP.PAL index tables |
| `tb_atk.py`, `tb_atk2.py` | KINDSP.BIN class names and action ranges |
| `tb_extract.py` | extract raw BIN/PRG/DAT entries to `dump/tables/raw/` |
| `tb_item.py` | ITEMSP.BIN item records |
| `tb_kind.py` | KINDSP.BIN layout (classes, skills, elements) |
| `tb_map.py` | PRG 6/7 battle maps and tile attributes |
| `tb_spell.py` | BIN 2 spell table |
| `tb_stage.py`, `tb_stage2.py` | BIN 1 stage scripts |
| `tb_terrain.py` | DAT 6 terrain table |
| `tb_unitrec.py` | stage unit records against KINDSP classes |
