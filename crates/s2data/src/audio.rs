//! Sound effects, music and the tables that choose them.
//!
//! - Sound effects: BIN entry 17, `u16 n; (u32 off, u32 size)[n]`, each a
//!   complete RIFF WAVE file (PCM 8/16-bit, mono/stereo, 11/22/44 kHz).
//!   Ids are 1-based, 1..=245 (`FUN_0041a01c`, player `FUN_00419b1d`).
//! - Music: BIN entry 18, same table layout, each an XMIDI file
//!   (`FORM XDIR` + `CAT XMID`). Ids 1..=37 (`FUN_0041a152`, `FUN_0041999a`).
//! - With `CDDA=ON` the game first maps a music id to a CD audio track through
//!   a per-disc table in the exe (`FUN_004198d4`); ids without a track, and
//!   all movie/ending music, play the MIDI version.
//!
//! See `docs/formats-audio.md` for the cue logic.

use crate::archive::{offset_table, Span};
use crate::{format_err, le, Archives, Kind, Result};

pub const SFX_ENTRY: usize = 17;
pub const MUSIC_ENTRY: usize = 18;
pub const MOVIE_CUE_ENTRY: usize = 7;
pub const EFFECT_SOUND_ENTRY: usize = 10;

/// Highest sound id the player accepts (`0xf5`).
pub const MAX_SFX_ID: u16 = 245;
/// Highest music id the player accepts (`< 0x26`).
pub const MAX_MUSIC_ID: u16 = 37;
/// Mixer voices the game allocates.
pub const SFX_VOICES: usize = 16;

/// An offset-table bank inside one BIN entry; members are read on demand.
#[derive(Debug, Clone)]
pub struct Bank {
    entry: usize,
    spans: Vec<Span>,
}

impl Bank {
    pub fn open(arcs: &Archives, entry: usize) -> Result<Bank> {
        let n = le::u16(&arcs.read_within(Kind::Bin, entry, 0, 2)?, 0)? as u32;
        let table = arcs.read_within(Kind::Bin, entry, 0, 2 + 8 * n)?;
        Ok(Bank { entry, spans: offset_table(&table)? })
    }

    pub fn sounds(arcs: &Archives) -> Result<Bank> {
        Bank::open(arcs, SFX_ENTRY)
    }

    pub fn music(arcs: &Archives) -> Result<Bank> {
        Bank::open(arcs, MUSIC_ENTRY)
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Raw bytes of member `id` (1-based, as the game numbers them).
    pub fn raw(&self, arcs: &Archives, id: usize) -> Result<Vec<u8>> {
        let span = id
            .checked_sub(1)
            .and_then(|i| self.spans.get(i))
            .ok_or_else(|| format_err("audio bank", format!("id {id} out of range 1..={}", self.spans.len())))?;
        arcs.read_within(Kind::Bin, self.entry, span.offset, span.size)
    }
}

/// A parsed RIFF WAVE file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wave {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub block_align: u16,
    /// Sample data; 8-bit is unsigned, 16-bit signed little-endian.
    pub data: Vec<u8>,
}

impl Wave {
    pub fn parse(b: &[u8]) -> Result<Wave> {
        if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
            return Err(format_err("wave", "not a RIFF WAVE file"));
        }
        let mut fmt = None;
        let mut data = None;
        let mut p = 12;
        while p + 8 <= b.len() {
            let len = le::u32(b, p + 4)? as usize;
            let body = b.get(p + 8..p + 8 + len).ok_or_else(|| format_err("wave", "chunk past end of file"))?;
            match &b[p..p + 4] {
                b"fmt " => fmt = Some(body),
                b"data" => data = Some(body),
                _ => {}
            }
            p += 8 + len + (len & 1);
        }
        let fmt = fmt.ok_or_else(|| format_err("wave", "no fmt chunk"))?;
        let data = data.ok_or_else(|| format_err("wave", "no data chunk"))?;
        let w = Wave {
            format_tag: le::u16(fmt, 0)?,
            channels: le::u16(fmt, 2)?,
            sample_rate: le::u32(fmt, 4)?,
            block_align: le::u16(fmt, 12)?,
            bits_per_sample: le::u16(fmt, 14)?,
            data: data.to_vec(),
        };
        if w.format_tag != 1 || w.channels == 0 || w.block_align == 0 {
            return Err(format_err("wave", format!("unsupported format tag {} / {} ch", w.format_tag, w.channels)));
        }
        Ok(w)
    }

    pub fn frames(&self) -> usize {
        self.data.len() / self.block_align as usize
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }

    /// Interleaved samples as f32 in -1.0..1.0.
    pub fn samples_f32(&self) -> Vec<f32> {
        match self.bits_per_sample {
            8 => self.data.iter().map(|&s| (s as f32 - 128.0) / 128.0).collect(),
            _ => self.data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
        }
    }
}

/// One XMIDI sequence (the game's files hold exactly one each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xmidi {
    /// (patch, bank) pairs from the TIMB chunk.
    pub timbres: Vec<(u8, u8)>,
    /// Raw EVNT chunk payload.
    pub events: Vec<u8>,
}

/// XMIDI timing is fixed at 120 ticks per second.
pub const XMIDI_TICKS_PER_SEC: u32 = 120;

fn be32(b: &[u8], off: usize) -> Result<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format_err("xmidi", "truncated chunk header"))
}

impl Xmidi {
    pub fn parse(b: &[u8]) -> Result<Xmidi> {
        let form = find_xmid_form(b, 0, b.len())?.ok_or_else(|| format_err("xmidi", "no FORM XMID"))?;
        let (start, end) = form;
        let mut timbres = Vec::new();
        let mut events = None;
        let mut p = start;
        while p + 8 <= end {
            let len = be32(b, p + 4)? as usize;
            let body = b.get(p + 8..p + 8 + len).ok_or_else(|| format_err("xmidi", "chunk past end"))?;
            match &b[p..p + 4] {
                b"TIMB" => {
                    let n = le::u16(body, 0)? as usize;
                    for i in 0..n {
                        let e = body.get(2 + 2 * i..4 + 2 * i).ok_or_else(|| format_err("xmidi", "short TIMB"))?;
                        timbres.push((e[0], e[1]));
                    }
                }
                b"EVNT" => events = Some(body.to_vec()),
                _ => {}
            }
            p += 8 + len + (len & 1);
        }
        let events = events.ok_or_else(|| format_err("xmidi", "no EVNT chunk"))?;
        Ok(Xmidi { timbres, events })
    }

    /// Convert to a type-0 Standard MIDI File (60 PPQN at 500000 us/quarter,
    /// i.e. 120 ticks/s like XMIDI). XMIDI tempo events are dropped because
    /// XMIDI delays are already in real time; AIL-specific controllers
    /// 110..=120 (channel locking, FOR/NEXT loops, callbacks) are dropped too.
    pub fn to_smf(&self) -> Result<Vec<u8>> {
        let ev = &self.events;
        let err = || format_err("xmidi", "truncated event");
        let byte = |p: usize| ev.get(p).copied().ok_or_else(err);
        let vlq = |p: &mut usize| -> Result<u32> {
            let mut v = 0u32;
            for _ in 0..4 {
                let c = byte(*p)?;
                *p += 1;
                v = (v << 7) | (c & 0x7f) as u32;
                if c & 0x80 == 0 {
                    return Ok(v);
                }
            }
            Err(format_err("xmidi", "variable-length value too long"))
        };
        // (absolute time, MIDI message); stable sort keeps note-offs that were
        // scheduled earlier ahead of later events at the same tick.
        let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut t = 0u32;
        let mut p = 0;
        while p < ev.len() {
            let status = byte(p)?;
            if status < 0x80 {
                t += status as u32;
                p += 1;
                continue;
            }
            p += 1;
            match status & 0xf0 {
                0x90 => {
                    let (note, vel) = (byte(p)?, byte(p + 1)?);
                    p += 2;
                    let dur = vlq(&mut p)?;
                    out.push((t, vec![status, note, vel]));
                    out.push((t + dur, vec![0x80 | (status & 0x0f), note, 0x40]));
                }
                0x80 | 0xa0 | 0xe0 => {
                    out.push((t, vec![status, byte(p)?, byte(p + 1)?]));
                    p += 2;
                }
                0xb0 => {
                    let (ctl, val) = (byte(p)?, byte(p + 1)?);
                    p += 2;
                    if !(110..=120).contains(&ctl) {
                        out.push((t, vec![status, ctl, val]));
                    }
                }
                0xc0 | 0xd0 => {
                    out.push((t, vec![status, byte(p)?]));
                    p += 1;
                }
                _ if status == 0xff => {
                    let kind = byte(p)?;
                    p += 1;
                    let len = vlq(&mut p)? as usize;
                    let data = ev.get(p..p + len).ok_or_else(err)?;
                    p += len;
                    if kind == 0x2f {
                        break;
                    }
                    if kind != 0x51 {
                        let mut m = vec![0xff, kind];
                        push_vlq(&mut m, len as u32);
                        m.extend_from_slice(data);
                        out.push((t, m));
                    }
                }
                _ if status == 0xf0 || status == 0xf7 => {
                    let len = vlq(&mut p)? as usize;
                    let data = ev.get(p..p + len).ok_or_else(err)?;
                    p += len;
                    let mut m = vec![status];
                    push_vlq(&mut m, len as u32);
                    m.extend_from_slice(data);
                    out.push((t, m));
                }
                _ => return Err(format_err("xmidi", format!("unexpected status {status:#04x} at {:#x}", p - 1))),
            }
        }
        out.sort_by_key(|(time, _)| *time);
        let mut track = vec![0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20];
        let mut last = 0;
        for (time, msg) in &out {
            push_vlq(&mut track, time - last);
            track.extend_from_slice(msg);
            last = *time;
        }
        track.extend_from_slice(&[0x00, 0xff, 0x2f, 0x00]);
        let mut smf = b"MThd\0\0\0\x06\0\0\0\x01\0\x3c".to_vec();
        smf.extend_from_slice(b"MTrk");
        smf.extend_from_slice(&(track.len() as u32).to_be_bytes());
        smf.extend_from_slice(&track);
        Ok(smf)
    }

    /// Length in seconds (time of the last delay before end of track).
    pub fn duration_secs(&self) -> f64 {
        let ev = &self.events;
        let (mut t, mut p) = (0u32, 0usize);
        while p < ev.len() {
            let s = ev[p];
            p += 1;
            if s < 0x80 {
                t += s as u32;
                continue;
            }
            let skip_vlq = |p: &mut usize| -> usize {
                let mut v = 0usize;
                while let Some(&c) = ev.get(*p) {
                    *p += 1;
                    v = (v << 7) | (c & 0x7f) as usize;
                    if c & 0x80 == 0 {
                        break;
                    }
                }
                v
            };
            match s & 0xf0 {
                0x90 => {
                    p += 2;
                    skip_vlq(&mut p);
                }
                0x80 | 0xa0 | 0xb0 | 0xe0 => p += 2,
                0xc0 | 0xd0 => p += 1,
                _ if s == 0xff => {
                    let kind = ev.get(p).copied().unwrap_or(0x2f);
                    p += 1;
                    let len = skip_vlq(&mut p);
                    if kind == 0x2f {
                        break;
                    }
                    p += len;
                }
                _ => {
                    let len = skip_vlq(&mut p);
                    p += len;
                }
            }
        }
        t as f64 / XMIDI_TICKS_PER_SEC as f64
    }
}

/// Find the first `FORM XMID` body (start, end) inside IFF data.
fn find_xmid_form(b: &[u8], mut p: usize, end: usize) -> Result<Option<(usize, usize)>> {
    while p + 12 <= end {
        let len = be32(b, p + 4)? as usize;
        let body_end = (p + 8 + len).min(end);
        let id = &b[p..p + 4];
        let sub = &b[p + 8..p + 12];
        if id == b"FORM" && sub == b"XMID" {
            return Ok(Some((p + 12, body_end)));
        }
        if id == b"FORM" || id == b"CAT " {
            if let Some(found) = find_xmid_form(b, p + 12, body_end)? {
                return Ok(Some(found));
            }
        }
        p += 8 + len + (len & 1);
    }
    Ok(None)
}

fn push_vlq(out: &mut Vec<u8>, mut v: u32) {
    let mut buf = [0u8; 5];
    let mut i = buf.len() - 1;
    buf[i] = (v & 0x7f) as u8;
    v >>= 7;
    while v > 0 {
        i -= 1;
        buf[i] = 0x80 | (v & 0x7f) as u8;
        v >>= 7;
    }
    out.extend_from_slice(&buf[i..]);
}

/// Which game disc's CD-audio layout to use (`DAT_00497b58`: 0 = disc 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disc {
    One,
    Two,
}

/// Music id (1..=37) -> table value for disc 1, from the exe at 0x48e304.
const CD_DISC1: [u8; 37] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0, 0, 0, 0, 14, 15, 0, 0, 0, 0, 0, 16, 0, 17, 0, 0, 18, 19, 20, 0, 21,
    0, 22, 23,
];
/// Music id (1..=37) -> table value for disc 2, from the exe at 0x48e398.
const CD_DISC2: [u8; 37] = [
    17, 0, 0, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 0, 1, 4, 5, 6, 7, 8, 9, 19, 10, 11, 12, 13, 0, 14, 15, 0, 16, 0,
    20, 21,
];

/// The CD audio track the game plays for `music_id` when `CDDA=ON`, or
/// `None` when it falls back to the MIDI version (`FUN_004198d4`,
/// `FUN_00407678`: track = table value + 1, track 1 being the data track).
pub fn cd_track(disc: Disc, music_id: u16) -> Option<u8> {
    let table = match disc {
        Disc::One => &CD_DISC1,
        Disc::Two => &CD_DISC2,
    };
    let v = *table.get((music_id as usize).checked_sub(1)?)?;
    (v > 0).then_some(v + 1)
}

/// One sound cue inside a movie (BIN entry 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovieCue {
    /// 1-based frame number the cue fires on.
    pub frame: i16,
    /// Positive: play music id (always the MIDI version); -1: stop music; 0: none.
    pub music: i16,
    /// Positive: play sound id; -1: stop all sounds; below -1: sound `-sound`
    /// re-triggered every frame while the movie plays with flag 0x400; 0: none.
    pub sound: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movie {
    /// FLC file name inside DAT (movie n is DAT entry 20 + n).
    pub name: String,
    pub cues: Vec<MovieCue>,
}

/// Parse BIN entry 7: `i16 n; record[n]` with 21-byte records
/// `u32 cue_offset; char name[15]; i16 cue_count`, cues being three i16s
/// (`FUN_004593dd`, consumed by `FUN_00459566`).
pub fn parse_movies(b: &[u8]) -> Result<Vec<Movie>> {
    let n = le::i16(b, 0)?.max(0) as usize;
    (0..n)
        .map(|i| {
            let r = 2 + 21 * i;
            let off = le::u32(b, r)? as usize;
            let name_bytes = b.get(r + 4..r + 19).ok_or_else(|| format_err("movie table", "truncated record"))?;
            let count = le::i16(b, r + 19)?.max(0) as usize;
            let cues = (0..count)
                .map(|k| {
                    let c = off + 6 * k;
                    Ok(MovieCue { frame: le::i16(b, c)?, music: le::i16(b, c + 2)?, sound: le::i16(b, c + 4)? })
                })
                .collect::<Result<_>>()?;
            Ok(Movie { name: crate::text::big5(name_bytes), cues })
        })
        .collect()
}

/// Sound id played with each spell/effect animation (CEL entry 1 set k uses
/// element k-1; 0 = silent). Stored in BIN entry 10 after four arrays of
/// `n` bytes (`FUN_0042422b`, used by `FUN_00466507`).
pub fn parse_effect_sounds(b: &[u8]) -> Result<Vec<u8>> {
    let n = le::i16(b, 0)?.max(0) as usize;
    let p = 2 + 4 * n;
    let m = le::i16(b, p)?.max(0) as usize;
    b.get(p + 2..p + 2 + m)
        .map(|s| s.to_vec())
        .ok_or_else(|| format_err("effect sound table", "truncated"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_encoding() {
        let mut v = Vec::new();
        push_vlq(&mut v, 0);
        push_vlq(&mut v, 0x7f);
        push_vlq(&mut v, 0x80);
        push_vlq(&mut v, 0x3fff);
        assert_eq!(v, [0x00, 0x7f, 0x81, 0x00, 0xff, 0x7f]);
    }

    #[test]
    fn xmidi_note_gets_note_off() {
        // delay 10, note on ch0 C4 vel 100 dur 30, delay 40, end of track
        let x = Xmidi { timbres: vec![], events: vec![10, 0x90, 60, 100, 30, 40, 0xff, 0x2f, 0x00] };
        let smf = x.to_smf().unwrap();
        let trk = &smf[22..];
        assert_eq!(
            trk,
            [0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, 10, 0x90, 60, 100, 30, 0x80, 60, 0x40, 0x00, 0xff, 0x2f, 0x00]
        );
        assert!((x.duration_secs() - 50.0 / 120.0).abs() < 1e-9);
    }

    #[test]
    fn cd_tracks_skip_data_track() {
        assert_eq!(cd_track(Disc::One, 1), Some(2));
        assert_eq!(cd_track(Disc::One, 14), None);
        assert_eq!(cd_track(Disc::Two, 18), Some(2));
        assert_eq!(cd_track(Disc::Two, 0), None);
        assert_eq!(cd_track(Disc::Two, 38), None);
    }
}
