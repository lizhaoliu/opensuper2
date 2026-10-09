# Audio: sound effects, music, cues

Reader: `crates/s2data/src/audio.rs`. Dumper: `s2audio list | dump <dir> | cues`.

## Where the data is

| What | Location | Format |
|------|----------|--------|
| Sound effects / voices | BIN entry 17 | `u16 n; (u32 off, u32 size)[n]` (offsets from entry start), 245 complete RIFF WAVE files |
| Music | BIN entry 18 | same table, 37 XMIDI files (`FORM XDIR`/`INFO` count=1, `CAT XMID` > `FORM XMID` > `TIMB`, `EVNT`) |
| Movie sound cues | BIN entry 7 | see below |
| Effect animation -> sound | BIN entry 10 | see below |
| Stage music | BIN entry 1, byte 10 of each 30-byte stage record (`DAT_00494692`) | music id |
| CD track tables | exe `.data` 0x48e300 (disc 1), 0x48e394 (disc 2) | `i32[38]`, indexed by music id |

DAT entries hold no audio (PCX images, tables, FLC movies). The FLC movies
have no audio track; their sound comes entirely from the BIN 7 cues.

The strings `midisp.bin` / `soundsp` in the exe are leftovers of a loose-file
loader that is never used; the shipped game reads BIN 17/18.

### Samples

PCM only (format tag 1): 198 are 11025 Hz mono 8-bit; others 11/22/44.1 kHz,
mono/stereo, 8/16-bit. 578.7 s total. Ids 244 (64.7 s) and 245 (14.4 s)
are the digitized music of the LOGO / SPMARK opening movies. A few ids are
near-silent stubs (≤ 0.05 s: 102, 103, 150, 159, 192–199).

### Music

XMIDI delays are 120 Hz ticks summed from bytes < 0x80; note-on carries a
VLQ duration instead of a note-off. Each song has one tempo meta event,
which is informational (MIDIFORM already converted to real time).
Controllers used: 1, 7, 10, 64 only — no AIL loop/branch controllers. The
game loops whole songs with `AIL_set_sequence_loop_count(0)`. `Xmidi::to_smf`
converts to a type-0 SMF at 60 PPQN / 500000 µs/qn (= 120 ticks/s). All 37
convert cleanly (26.7–84.0 s per loop), GM layout with drums on channel 10.

## Playback logic (`FUN_00419890` init, `FUN_00419b1d`, `FUN_004198d4`)

Init (`FUN_00419d3a`): opens the Miles waveOut digital driver at 44.1, 22.05,
then 11.025 kHz, 16-bit stereo, retrying the three rates with
`AIL_set_preference(15, 1)` if all fail; allocates up to 16 sample voices;
opens one MIDI sequence handle on the default MIDI out.

### Sound effects — `play_sound(id, slot)` = `FUN_00419b1d`

```
if no driver || sfx_volume == 0 || id !in 1..=245: return 0
if slot in 1..=voices && voice[slot-1] loaded:          # re-trigger a held voice
    last = (id, slot-1); if voice done: restart it; return slot
if id == last.id && voice[last.slot] loaded && done:    # same sound again: no reload
    restart it; return last.slot+1
v = first voice whose status is DONE, else return 0     # no free voice: dropped
reload v with sound id, volume = sfx_volume (0..100 passed raw to Miles' 0..127), start
last = (id, v); return v+1
```

Callers that pass back the returned slot every frame get a looping sound
(it restarts whenever it finishes). `FUN_00419b08` stops all voices.

### Music — `play_music(id, loop, mode)` = `FUN_004198d4`

```
if id !in 1..=37: return
if music already playing && current == id && music_active: return
if !cdda_music || mode == 2:  play MIDI song id (BIN 18)
else:
    t = cd_table[disc][id]        # disc = DAT_00497b58: 0 = disc 1
    if t < 1: play MIDI song id
    else:     play CD track t+1   # track 1 is the data track
current = id                      # _DAT_00494653, restored after cut-ins
```

`loop == -1` sets the loop flag (MIDI loop count 0; CD: the per-frame poll
`FUN_00407782` replays the track when its status turns STOPPED). Volumes:
`VOLUME=midi,cd,sfx` (0..100); MIDI/CD are scaled ×127/100. `FUN_00419a7b`
stops all music. If CD mode is on but no CD drive opened, CD-mapped ids are
silent (the call is a no-op), which is why the no-CD repack ships `CDDA=OFF`.

### Music id -> CD track (`s2data::audio::cd_track`)

| id | 1–13 | 14–17 | 18 | 19 | 20–24 | 25 | 26 | 27 | 28–29 | 30 | 31 | 32 | 33 | 34 | 35 | 36 | 37 |
|----|------|-------|----|----|-------|----|----|----|-------|----|----|----|----|----|----|----|----|
| disc 1 | 2–14 | MIDI | 15 | 16 | MIDI | 17 | MIDI | 18 | MIDI | 19 | 20 | 21 | MIDI | 22 | MIDI | 23 | 24 |

| id | 1 | 2–3 | 4 | 5–14 | 15 | 16 | 17 | 18 | 19–24 | 25 | 26–29 | 30 | 31 | 32 | 33 | 34 | 35 | 36 | 37 |
|----|---|-----|---|------|----|----|----|----|-------|----|-------|----|----|----|----|----|----|----|----|
| disc 2 | 18 | MIDI | 19 | MIDI | 3 | 4 | MIDI | 2 | 5–10 | 20 | 11–14 | MIDI | 15 | 16 | MIDI | 17 | MIDI | 21 | 22 |

So disc 1 has 23 audio tracks (2–24) and disc 2 has 21 (2–22). Ids 14, 17,
33 and 35 exist only as MIDI.

### Where music ids come from

| Source | Call site |
|--------|-----------|
| stage record byte 10, at stage start and after cut-ins | `FUN_0040d108`, `FUN_0041a583`, `FUN_00453986` (resumes `_DAT_00494653`) |
| 25: title/main menu, load screen, quit to title | `FUN_0040d7e1`, `FUN_0040de96`, `FUN_00444bea` |
| 4: intermission menu between stages | `FUN_00410763`, `FUN_004109b3` |
| 37: intermission menu option 3 sub-menu | `FUN_00410c21` |
| 36: intermission map/selection screen | `FUN_00414b17` |
| 13, 20: ending sequence (mode 2 = always MIDI) | `FUN_0041abf4`, `FUN_0041b489` (from `FUN_0041a779`) |
| event script op 1 = sound (`0` stops all), op 2 = music (`0` stops), op 3 = show CG then play music, op 5 = movie then resume music | `FUN_0041d280` (`+5` = arg) |
| dialogue line record (0x17 bytes): `+2` music before the line, `+4` music after | `FUN_004188ca` |
| movie cues (mode 2 = always MIDI) | `FUN_00459566` |

### Movie cues — BIN entry 7 (`FUN_004593dd`)

`i16 n;` then `n` 21-byte records: `u32 cue_offset; char name[15]; i16
cue_count`. Cues at `cue_offset` (entry-relative) are `i16 frame (1-based),
i16 music, i16 sound`: music >0 play (MIDI), -1 stop; sound >0 play, -1 stop
all, < -1 play `-sound` re-triggered every frame (loops) when the movie is
played with flag 0x400. Movie n's FLC data is DAT entry 20+n.

| # | file | cues |
|---|------|------|
| 1 | LOGO.FLC | looped sound 245 |
| 2 | SPMARK.FLC | looped sound 244 + 17 timed effects |
| 3 | SPOVER.FLC | music 32 + 16 timed effects |
| 4, 5 | END-1.FLC, END-2.FLC | music 14 |

Movies 1–2 play at startup (`FUN_0041a370`) only if the 2nd `CDDA=` field is
ON (`DAT_004944ec`), as do the ending movies 4–5.

### Effect animation -> sound — BIN entry 10 (`FUN_0042422b`)

`i16 n; u8 a[n], b[n], c[n], d[n]; i16 m; u8 sound[m]; ...`. `sound[k-1]` is
the sound id played when CEL entry 1 frame set k (spell/effect) starts
(`FUN_00466507`); 0 = silent. m = 133 = number of effect sets.

## SUPERSP.CFG

| Line | Fields |
|------|--------|
| `DEVICE=a,b,d` | a: PRG installed next to the exe (`DAT_004944f0`); b: `DAT_004944f4` (selects BIN 5 vs 6; not audio); d: CD drive letter, `0` = no CD. The repack uses `.` so that `d:\SUPERSP\` becomes `.\SUPERSP\` |
| `VOLUME=midi,cd,sfx` | 0..100 each |
| `CDDA=m,v` | m: music from CD audio (ON) or MIDI (OFF); v: play FLC movies (ON/OFF) |

The current disc `DAT_00497b58` (0 = disc 1, 1 = disc 2) comes from
`DISK.ID` ("1"/"2", `FUN_00407490`) in CD mode and is stored in save files
(offset 0x10 of the 0x1c-byte block at 0x497b48).

## This repack

- Has all 245 samples and all 37 MIDI songs. Ships `CDDA=OFF,ON`, so it
  plays MIDI music, with the movies on.
- Has no CD audio. MSS32.DLL is stock Miles 4.0g (1998-06-11) with no MP3 codec,
  and redbook playback goes to MCI `cdaudio`. Neither the exe nor the DLL has
  any MP3, track-file or ASI references, so the readme's "MP3背景音樂" isn't
  in this package. It would have needed an external CD-audio emulator and
  ripped tracks.

## Proposed engine music convention (design only)

For each music request `(id, mode)`, try these in order:

1. `music/M{id:02}.{ogg,flac,mp3,wav}`: a per-id override that doesn't depend on
   the disc. This is the recommended layout for user-made rips.
2. If CD music is enabled and `mode != 2`: `music/disc{1,2}/Track{NN:02}.{ogg,flac,mp3,wav}`,
   with NN = `cd_track(disc, id)`. This lets raw CD rips of either disc
   drop in unchanged.
3. Otherwise, or if no file is found: the MIDI song from BIN 18, converted with
   `Xmidi::to_smf` and rendered by a General MIDI SoundFont synth
   (e.g. a user-supplied `.sf2` file). Recorded audio files loop end to end;
   MIDI loops whole songs.

Keep the original rules: a repeated request for the current song is a no-op;
`current` is remembered so the game can resume after cut-ins; mode 2
(movies and the ending) uses MIDI unless a file from step 1 exists. Volume
sliders keep the 0..100 scale.
