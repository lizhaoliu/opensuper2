//! Cutscenes: Autodesk FLC/FLI animations with an obfuscated header.
//!
//! BIN entry 7 lists the cutscenes (`FUN_004593dd`):
//! `u16 n; { u32 events_offset; char name[13]; u8 pad[2]; i16 event_count }[n]`
//! and, at each `events_offset`, `(i16 frame, i16 music, i16 sound)[count]`
//! triggers (frame is 1-based; music > 0 plays a track, -1 stops; sound > 0
//! plays a sample, -1 stops, < -1 loops sample `-sound`).
//!
//! | id | name | DAT entry | size |
//! |----|------|-----------|------|
//! | 1 | LOGO.FLC | 21 | 640x360, 53 frames |
//! | 2 | SPMARK.FLC | 22 | 640x480, 658 frames (opening) |
//! | 3 | SPOVER.FLC | 23 | 640x480, 1142 frames |
//! | 4 | END-1.FLC | 24 | 640x480, 136 frames |
//! | 5 | END-2.FLC | 25 | 640x480, 95 frames |
//!
//! Header obfuscation (`FUN_0045ac5a`, undone in place): swap the dwords at
//! 0x00 and 0x50, swap the words at 0x06 and 0x0e, then subtract 0x923 from
//! the magic, 0x38 from width, 0x0b from height and 8 from depth.
//!
//! Player `FUN_00459566`: frames start at `oframe1`; exactly `frames` frames
//! are shown (no ring frame). Chunk handling differs from Autodesk's in two
//! places that are reproduced here: COLOR_64 is shifted `>> 2` like COLOR_256
//! in FLC files, DELTA_FLC word runs fill with the word's low byte only, and
//! COPY always reads width*height bytes (END-1.FLC has two short COPY chunks).
//! FLI (320x200) output is pixel- and line-doubled to 640x400 at row 40.
//! Frame delay in centiseconds: FLC `speed*10/48`, FLI `speed*4/3`.

use crate::sprite::Image;
use crate::{format_err, le, Archives, Kind, Result};

pub const MAGIC_FLI: u16 = 0xaf11;
pub const MAGIC_FLC: u16 = 0xaf12;
/// Cutscene `id` lives in DAT entry `id + CUTSCENE_DAT_BASE`.
pub const CUTSCENE_DAT_BASE: usize = 0x14;

/// Undo the header obfuscation of a 128-byte FLC header.
pub fn deobfuscate_header(h: &[u8]) -> Result<[u8; 128]> {
    let mut o: [u8; 128] = h.get(..128).ok_or_else(|| format_err("flc", "short header"))?.try_into().unwrap();
    for i in 0..4 {
        o.swap(i, 0x50 + i);
    }
    for i in 0..2 {
        o.swap(6 + i, 0x0e + i);
    }
    for (off, d) in [(4usize, 0x923u16), (8, 0x38), (10, 0x0b), (12, 8)] {
        let v = u16::from_le_bytes([o[off], o[off + 1]]).wrapping_sub(d);
        o[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    Ok(o)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlcHeader {
    pub size: u32,
    pub magic: u16,
    pub frames: u16,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub flags: u16,
    pub speed: u32,
    pub oframe1: u32,
    pub oframe2: u32,
}

impl FlcHeader {
    pub fn parse(h: &[u8; 128]) -> Result<FlcHeader> {
        Ok(FlcHeader {
            size: le::u32(h, 0)?,
            magic: le::u16(h, 4)?,
            frames: le::u16(h, 6)?,
            width: le::u16(h, 8)?,
            height: le::u16(h, 10)?,
            depth: le::u16(h, 12)?,
            flags: le::u16(h, 14)?,
            speed: le::u32(h, 16)?,
            oframe1: le::u32(h, 0x50)?,
            oframe2: le::u32(h, 0x54)?,
        })
    }
}

/// A decoder that steps through the frames of one animation.
pub struct Flc {
    pub header: FlcHeader,
    data: Vec<u8>,
    next: usize,
    frame: usize,
    /// Current frame, `width * height` bytes.
    pub pixels: Vec<u8>,
    /// Current palette, 6-bit.
    pub palette6: [u8; 768],
    /// Set when the last decoded frame changed the palette.
    pub palette_changed: bool,
}

impl Flc {
    /// `data` is the whole archive entry (obfuscated header included).
    pub fn open(data: Vec<u8>) -> Result<Flc> {
        let header = FlcHeader::parse(&deobfuscate_header(&data)?)?;
        if header.magic != MAGIC_FLC && header.magic != MAGIC_FLI {
            return Err(format_err("flc", format!("bad magic {:#06x}", header.magic)));
        }
        if header.width == 0 || header.height == 0 || header.width > 2048 || header.height > 2048 {
            return Err(format_err("flc", "bad size"));
        }
        let next = if header.magic == MAGIC_FLC { header.oframe1 as usize } else { 128 };
        let n = header.width as usize * header.height as usize;
        Ok(Flc { header, data, next, frame: 0, pixels: vec![0; n], palette6: [0; 768], palette_changed: false })
    }

    pub fn is_fli(&self) -> bool {
        self.header.magic == MAGIC_FLI
    }

    pub fn frame_count(&self) -> usize {
        self.header.frames as usize
    }

    /// Index of the next frame to decode (0-based).
    pub fn position(&self) -> usize {
        self.frame
    }

    pub fn delay_cs(&self) -> u32 {
        if self.is_fli() { self.header.speed * 4 / 3 } else { self.header.speed * 10 / 48 }
    }

    /// Decode the next frame; returns false once all frames were shown.
    pub fn next_frame(&mut self) -> Result<bool> {
        if self.frame >= self.frame_count() {
            return Ok(false);
        }
        self.palette_changed = false;
        let f = self.next;
        let size = le::u32(&self.data, f)? as usize;
        if size < 16 || f + size > self.data.len() {
            return Err(format_err("flc", format!("frame {} has bad size {size}", self.frame)));
        }
        let chunks = le::i16(&self.data, f + 6)?.max(0) as usize;
        let mut c = f + 16;
        for _ in 0..chunks {
            let csize = le::u32(&self.data, c)? as usize;
            let ctype = le::u16(&self.data, c + 4)?;
            let body = if ctype == 16 {
                // The game memcpy()s width*height bytes whatever the chunk
                // size says; two COPY chunks in END-1.FLC are 2 bytes short.
                let n = self.pixels.len();
                let end = (c + 6 + n).min(self.data.len());
                self.data[c + 6..end].to_vec()
            } else {
                self.data.get(c + 6..c + csize.max(6)).ok_or_else(|| format_err("flc", "chunk beyond frame"))?.to_vec()
            };
            self.chunk(ctype, &body)?;
            if csize == 0 {
                break;
            }
            c += csize;
        }
        self.next = f + size;
        self.frame += 1;
        Ok(true)
    }

    fn chunk(&mut self, ctype: u16, b: &[u8]) -> Result<()> {
        let w = self.header.width as usize;
        let h = self.header.height as usize;
        let short = move || format_err("flc", format!("chunk {ctype} truncated"));
        let byte = |p: usize| b.get(p).copied().ok_or_else(short);
        match ctype {
            4 | 11 => {
                let shift = if self.is_fli() { 0 } else { 2 };
                let packets = byte(0)? as usize;
                let (mut p, mut idx) = (2usize, 0usize);
                for _ in 0..packets {
                    idx += byte(p)? as usize * 3;
                    let mut n = byte(p + 1)? as usize;
                    if n == 0 {
                        n = 256;
                    }
                    p += 2;
                    for _ in 0..n * 3 {
                        if idx < 768 {
                            self.palette6[idx] = byte(p)? >> shift;
                        }
                        idx += 1;
                        p += 1;
                    }
                }
                self.palette_changed = true;
            }
            7 => {
                let mut lines = le::u16(b, 0)? as i16;
                let mut p = 2;
                let mut y = 0usize;
                'lines: while lines > 0 {
                    let packets;
                    loop {
                        let op = le::u16(b, p)?;
                        p += 2;
                        if (op as i16) >= 0 {
                            packets = op as usize;
                            break;
                        }
                        if op & 0x4000 != 0 {
                            y = (y as isize - op as i16 as isize) as usize;
                        } else {
                            if y < h {
                                self.pixels[y * w + w - 1] = op as u8;
                            }
                            let next = le::u16(b, p)?;
                            p += 2;
                            if next != 0 {
                                packets = next as usize;
                                break;
                            }
                            y += 1;
                            lines -= 1;
                            continue 'lines;
                        }
                    }
                    let mut x = 0usize;
                    for _ in 0..packets {
                        x += byte(p)? as usize;
                        let count = byte(p + 1)? as i8 as isize;
                        p += 2;
                        if count >= 0 {
                            let n = 2 * count as usize;
                            let src = b.get(p..p + n).ok_or_else(short)?;
                            self.put_row(y, x, src);
                            p += n;
                            x += n;
                        } else {
                            let n = (-2 * count) as usize;
                            let v = byte(p)?;
                            self.fill_row(y, x, n, v);
                            p += 2;
                            x += n;
                        }
                    }
                    y += 1;
                    lines -= 1;
                }
            }
            12 => {
                let first = le::u16(b, 0)? as usize;
                let lines = le::u16(b, 2)? as usize;
                let mut p = 4;
                for line in 0..lines {
                    let y = first + line;
                    let packets = byte(p)? as usize;
                    p += 1;
                    let mut x = 0usize;
                    for _ in 0..packets {
                        x += byte(p)? as usize;
                        let count = byte(p + 1)? as i8 as isize;
                        p += 2;
                        if count > 0 {
                            let src = b.get(p..p + count as usize).ok_or_else(short)?;
                            self.put_row(y, x, src);
                            p += count as usize;
                            x += count as usize;
                        } else if count < 0 {
                            let v = byte(p)?;
                            self.fill_row(y, x, (-count) as usize, v);
                            p += 1;
                            x += (-count) as usize;
                        }
                    }
                }
            }
            13 => self.pixels.fill(0),
            15 => {
                let mut p = 0;
                for y in 0..h {
                    p += 1;
                    let mut x = 0usize;
                    while x < w {
                        let count = byte(p)? as i8 as isize;
                        p += 1;
                        if count < 0 {
                            let n = (-count) as usize;
                            let src = b.get(p..p + n).ok_or_else(short)?;
                            self.put_row(y, x, src);
                            p += n;
                            x += n;
                        } else if count > 0 {
                            let v = byte(p)?;
                            self.fill_row(y, x, count as usize, v);
                            p += 1;
                            x += count as usize;
                        }
                    }
                }
            }
            16 => {
                let n = b.len().min(self.pixels.len());
                self.pixels[..n].copy_from_slice(&b[..n]);
            }
            _ => {}
        }
        Ok(())
    }

    fn put_row(&mut self, y: usize, x: usize, src: &[u8]) {
        let w = self.header.width as usize;
        let h = self.header.height as usize;
        // The game writes through a flat pointer, so runs may spill into the next row.
        let start = y.min(h) * w + x;
        let end = (start + src.len()).min(self.pixels.len());
        if start < end {
            self.pixels[start..end].copy_from_slice(&src[..end - start]);
        }
    }

    fn fill_row(&mut self, y: usize, x: usize, n: usize, v: u8) {
        let w = self.header.width as usize;
        let h = self.header.height as usize;
        let start = y.min(h) * w + x;
        let end = (start + n).min(self.pixels.len());
        if start < end {
            self.pixels[start..end].fill(v);
        }
    }

    /// The current frame as drawn on the 640x480 screen, with its position.
    pub fn screen_frame(&self) -> (Image, usize, usize) {
        let (w, h) = (self.header.width as usize, self.header.height as usize);
        if self.is_fli() {
            let mut img = Image::new(w * 2, h * 2);
            for y in 0..h * 2 {
                for x in 0..w * 2 {
                    img.pixels[y * w * 2 + x] = self.pixels[(y / 2) * w + x / 2];
                }
            }
            (img, 0, 40)
        } else {
            let off = if h < 480 { (480 - h) * 320 } else { 0 };
            (Image { width: w, height: h, pixels: self.pixels.clone() }, off % 640, off / 640)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutsceneEvent {
    pub frame: i16,
    pub music: i16,
    pub sound: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cutscene {
    pub id: usize,
    pub name: String,
    pub events: Vec<CutsceneEvent>,
}

/// Parse the cutscene table (BIN entry 7).
pub fn parse_cutscenes(b: &[u8]) -> Result<Vec<Cutscene>> {
    let n = le::u16(b, 0)? as usize;
    (0..n)
        .map(|i| {
            let r = 2 + 21 * i;
            let off = le::u32(b, r)? as usize;
            let name_bytes = b.get(r + 4..r + 17).ok_or_else(|| format_err("cutscenes", "truncated"))?;
            let name = String::from_utf8_lossy(&name_bytes[..name_bytes.iter().position(|&c| c == 0).unwrap_or(13)]).into_owned();
            let count = le::i16(b, r + 19)?.max(0) as usize;
            let events = (0..count)
                .map(|k| {
                    let e = off + 6 * k;
                    Ok(CutsceneEvent { frame: le::i16(b, e)?, music: le::i16(b, e + 2)?, sound: le::i16(b, e + 4)? })
                })
                .collect::<Result<_>>()?;
            Ok(Cutscene { id: i + 1, name, events })
        })
        .collect()
}

pub fn load_cutscenes(arcs: &Archives) -> Result<Vec<Cutscene>> {
    parse_cutscenes(&arcs.read(Kind::Bin, 7)?)
}

/// Open cutscene `id` (1-based).
pub fn open_cutscene(arcs: &Archives, id: usize) -> Result<Flc> {
    Flc::open(arcs.read(Kind::Dat, id + CUTSCENE_DAT_BASE)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obfuscate(h: &mut [u8; 128]) {
        for (off, d) in [(4usize, 0x923u16), (8, 0x38), (10, 0x0b), (12, 8)] {
            let v = u16::from_le_bytes([h[off], h[off + 1]]).wrapping_add(d);
            h[off..off + 2].copy_from_slice(&v.to_le_bytes());
        }
        for i in 0..2 {
            h.swap(6 + i, 0x0e + i);
        }
        for i in 0..4 {
            h.swap(i, 0x50 + i);
        }
    }

    fn tiny_flc() -> Vec<u8> {
        let mut h = [0u8; 128];
        h[4..6].copy_from_slice(&MAGIC_FLC.to_le_bytes());
        h[6..8].copy_from_slice(&1u16.to_le_bytes());
        h[8..10].copy_from_slice(&4u16.to_le_bytes());
        h[10..12].copy_from_slice(&2u16.to_le_bytes());
        h[12..14].copy_from_slice(&8u16.to_le_bytes());
        h[16..20].copy_from_slice(&48u32.to_le_bytes());
        h[0x50..0x54].copy_from_slice(&128u32.to_le_bytes());
        obfuscate(&mut h);
        let mut v = h.to_vec();
        // BYTE_RUN: row 0 = fill 4x7; row 1 = literal 1,2,3,4
        let body = [0u8, 4, 7, 0, 0xfc, 1, 2, 3, 4];
        let chunk_len = 6 + body.len();
        let frame_len = 16 + chunk_len;
        v.extend_from_slice(&(frame_len as u32).to_le_bytes());
        v.extend_from_slice(&0xf1fau16.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&[0; 8]);
        v.extend_from_slice(&(chunk_len as u32).to_le_bytes());
        v.extend_from_slice(&15u16.to_le_bytes());
        v.extend_from_slice(&body);
        v
    }

    #[test]
    fn header_roundtrip_and_byte_run() {
        let mut f = Flc::open(tiny_flc()).unwrap();
        assert_eq!((f.header.width, f.header.height, f.header.frames), (4, 2, 1));
        assert_eq!(f.delay_cs(), 10);
        assert!(f.next_frame().unwrap());
        assert_eq!(f.pixels, vec![7, 7, 7, 7, 1, 2, 3, 4]);
        assert!(!f.next_frame().unwrap());
        let (_, x, y) = f.screen_frame();
        assert_eq!((x, y), (0, 239));
    }
}
