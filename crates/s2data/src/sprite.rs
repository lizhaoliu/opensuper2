//! 8-bit indexed frames (RLE, raw and "flagged") and the banks that hold them.
//!
//! Every frame starts with `i16 width-1, i16 height-1`. Three bodies exist:
//!
//! * **RLE** (`FUN_00420e5c` decoder, `FUN_0046af47` clipped blitter): a byte
//!   stream filling the rectangle row-major (runs may cross rows):
//!   - `0x00`        one transparent pixel (index 0)
//!   - `0x80 | n`    n literal pixel bytes follow
//!   - `n` (1..=127) the next byte repeated n times
//! * **raw**: `width * height` pixel bytes (map tiles, cursors). Drawn by
//!   `FUN_0046f629`, which skips index 0 unless asked to copy opaquely.
//! * **flagged**: one leading byte, `0` = RLE frame follows, else a raw frame
//!   follows (`FUN_0046ed81`). Used by portraits, PRG8 backdrops and DAT7.
//!
//! Index 0 is transparent everywhere.

use crate::archive::{offset_table, Span};
use crate::{format_err, le, Result};

/// A decoded 8-bit indexed image; index 0 is transparent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn new(width: usize, height: usize) -> Image {
        Image { width, height, pixels: vec![0; width * height] }
    }

    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.pixels[y * self.width + x]
    }

    /// Copy `src` onto self at (x, y), skipping transparent pixels.
    pub fn blit(&mut self, src: &Image, x: isize, y: isize) {
        self.blit_impl(src, x, y, false);
    }

    /// Copy `src` onto self at (x, y) including index-0 pixels.
    pub fn blit_opaque(&mut self, src: &Image, x: isize, y: isize) {
        self.blit_impl(src, x, y, true);
    }

    fn blit_impl(&mut self, src: &Image, x: isize, y: isize, opaque: bool) {
        let x0 = x.max(0);
        let x1 = (x + src.width as isize).min(self.width as isize);
        if x0 >= x1 {
            return;
        }
        for sy in 0..src.height {
            let dy = y + sy as isize;
            if dy < 0 || dy >= self.height as isize {
                continue;
            }
            let srow = &src.pixels[sy * src.width..(sy + 1) * src.width];
            let drow = &mut self.pixels[dy as usize * self.width..(dy as usize + 1) * self.width];
            for dx in x0..x1 {
                let c = srow[(dx - x) as usize];
                if opaque || c != 0 {
                    drow[dx as usize] = c;
                }
            }
        }
    }
}

fn frame_size(b: &[u8], what: &str) -> Result<(usize, usize)> {
    let w = le::i16(b, 0)? as isize + 1;
    let h = le::i16(b, 2)? as isize + 1;
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
        return Err(format_err(what, format!("bad size {w}x{h}")));
    }
    Ok((w as usize, h as usize))
}

/// Decode one RLE frame. Returns the image and the number of bytes consumed.
pub fn decode_rle(b: &[u8]) -> Result<(Image, usize)> {
    let (w, h) = frame_size(b, "rle frame")?;
    let n = w * h;
    let mut out = Vec::with_capacity(n);
    let mut p = 4;
    let byte = |p: usize| b.get(p).copied().ok_or_else(|| format_err("rle frame", "truncated"));
    while out.len() < n {
        let c = byte(p)?;
        p += 1;
        if c == 0 {
            out.push(0);
        } else if c & 0x80 != 0 {
            let k = (c & 0x7f) as usize;
            let lit = b.get(p..p + k).ok_or_else(|| format_err("rle frame", "truncated literal"))?;
            out.extend_from_slice(lit);
            p += k;
        } else {
            let v = byte(p)?;
            p += 1;
            out.extend(std::iter::repeat_n(v, c as usize));
        }
    }
    out.truncate(n);
    Ok((Image { width: w, height: h, pixels: out }, p))
}

/// Decode one raw frame (header + `w*h` bytes). Returns the image and bytes consumed.
pub fn decode_raw(b: &[u8]) -> Result<(Image, usize)> {
    let (w, h) = frame_size(b, "raw frame")?;
    let pixels = b.get(4..4 + w * h).ok_or_else(|| format_err("raw frame", "truncated"))?.to_vec();
    Ok((Image { width: w, height: h, pixels }, 4 + w * h))
}

/// Decode a flagged frame: `u8 flag` (0 = RLE, else raw) then the frame.
pub fn decode_flagged(b: &[u8]) -> Result<(Image, usize)> {
    let flag = *b.first().ok_or_else(|| format_err("flagged frame", "empty"))?;
    let (img, n) = if flag == 0 { decode_rle(&b[1..])? } else { decode_raw(&b[1..])? };
    Ok((img, n + 1))
}

/// How the frames of an [`ImageBank`] are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Rle,
    Raw,
    Flagged,
}

impl FrameKind {
    pub fn decode(self, b: &[u8]) -> Result<Image> {
        match self {
            FrameKind::Rle => decode_rle(b),
            FrameKind::Raw => decode_raw(b),
            FrameKind::Flagged => decode_flagged(b),
        }
        .map(|(img, _)| img)
    }
}

/// An archive entry laid out as `u16 n; (u32 offset, u32 size)[n]; frames`
/// where every frame has the same encoding.
#[derive(Debug, Clone)]
pub struct ImageBank {
    pub kind: FrameKind,
    spans: Vec<Span>,
    data: Vec<u8>,
}

impl ImageBank {
    pub fn parse(data: Vec<u8>, kind: FrameKind) -> Result<ImageBank> {
        let spans = offset_table(&data)?;
        if let Some(bad) = spans.iter().find(|s| s.offset as usize + s.size as usize > data.len()) {
            return Err(format_err("image bank", format!("span {bad:?} beyond entry")));
        }
        Ok(ImageBank { kind, spans, data })
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Encoded bytes of image `i` (0-based).
    pub fn bytes(&self, i: usize) -> Result<&[u8]> {
        let s = self.spans.get(i).ok_or_else(|| format_err("image bank", format!("no image {i}")))?;
        Ok(&self.data[s.offset as usize..(s.offset + s.size) as usize])
    }

    /// Decode image `i` (0-based).
    pub fn image(&self, i: usize) -> Result<Image> {
        self.kind.decode(self.bytes(i)?)
    }
}

/// A set of frames with a shared 16-byte header (CEL entry 1: spell effects).
///
/// ```text
/// i16 frame_count
/// i16 anchor_x, anchor_y   (-1,-1 = full screen; 0,0 = centered on the unit's
///                           tile, bottom-aligned; else offset of the frame
///                           origin from the tile origin)
/// i16 width, height        bounding size
/// i16 remap_count          then remap_from[n] and remap_to[n]: translucent
///                          colours and their opacity in percent (see
///                          `gfx_palette::ColorTree::blend`)
/// i16 sequence_len         >0: a 1-based frame-order table of this many bytes
///                          follows; <0: -n = number of extra loops
/// u16 frame_delay          centiseconds per frame
/// ```
/// then `frame_count` (offset, size) pairs relative to the frame data start.
/// The effect player is `FUN_00466507`; pixels go through `FUN_0046b5aa`.
/// Placement relative to the target cell's screen position (`FUN_00427a10`,
/// i.e. diamond top-left, elevation applied) is given by [`FrameSet::origin`].
/// Anchor (0, 0) sets are also occluded like a unit standing on the cell.
#[derive(Debug, Clone)]
pub struct FrameSet {
    pub frame_count: usize,
    pub anchor: (i16, i16),
    pub size: (i16, i16),
    pub remap_from: Vec<u8>,
    pub remap_to: Vec<u8>,
    pub sequence: Vec<u8>,
    pub loops: i16,
    pub delay: u16,
    frames: Vec<(u32, u32)>,
    data: Vec<u8>,
    data_start: usize,
}

impl FrameSet {
    pub fn parse(b: Vec<u8>) -> Result<FrameSet> {
        let frame_count = le::i16(&b, 0)?.max(0) as usize;
        let anchor = (le::i16(&b, 2)?, le::i16(&b, 4)?);
        let size = (le::i16(&b, 6)?, le::i16(&b, 8)?);
        let remap = le::i16(&b, 10)?.max(0) as usize;
        let seq = le::i16(&b, 12)?;
        let delay = le::u16(&b, 14)?;
        let mut p = 16;
        let take = |p: usize, n: usize| {
            b.get(p..p + n).map(|s| s.to_vec()).ok_or_else(|| format_err("frame set", "truncated header"))
        };
        let remap_from = take(p, remap)?;
        let remap_to = take(p + remap, remap)?;
        p += 2 * remap;
        let (sequence, loops) = if seq > 0 {
            let s = take(p, seq as usize)?;
            p += seq as usize;
            (s, 0)
        } else {
            (Vec::new(), -seq)
        };
        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            frames.push((le::u32(&b, p + 8 * i)?, le::u32(&b, p + 8 * i + 4)?));
        }
        let data_start = p + 8 * frame_count;
        Ok(FrameSet { frame_count, anchor, size, remap_from, remap_to, sequence, loops, delay, frames, data: b, data_start })
    }

    pub fn frame_bytes(&self, i: usize) -> Result<&[u8]> {
        let (o, s) = *self.frames.get(i).ok_or_else(|| format_err("frame set", format!("no frame {i}")))?;
        let start = self.data_start + o as usize;
        self.data.get(start..start + s as usize).ok_or_else(|| format_err("frame set", "frame beyond data"))
    }

    pub fn frame(&self, i: usize) -> Result<Image> {
        decode_rle(self.frame_bytes(i)?).map(|(img, _)| img)
    }

    /// Frame indices in playback order (the sequence table is 1-based).
    pub fn playback_order(&self) -> Vec<usize> {
        if self.sequence.is_empty() {
            (0..self.frame_count).collect()
        } else {
            self.sequence.iter().map(|&s| (s as i8 as isize - 1).max(0) as usize).collect()
        }
    }

    /// Where frames are drawn for a target cell whose diamond top-left is at
    /// (cell_x, cell_y); `None` means full screen at (0, 0).
    pub fn origin(&self, cell_x: i32, cell_y: i32) -> Option<(i32, i32)> {
        match self.anchor {
            (-1, -1) => None,
            (0, 0) => Some((cell_x + 32 - self.size.0 as i32 / 2, cell_y + 32 - self.size.1 as i32)),
            (ax, ay) => Some((cell_x + 32 - ax as i32, cell_y + 32 - ay as i32)),
        }
    }

    /// Opacity percent for a colour if it is translucent in this set.
    pub fn remap_opacity(&self, color: u8) -> Option<u8> {
        self.remap_from.iter().position(|&c| c == color).map(|k| self.remap_to[k])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdr(w: u16, h: u16) -> Vec<u8> {
        let mut v = (w - 1).to_le_bytes().to_vec();
        v.extend_from_slice(&(h - 1).to_le_bytes());
        v
    }

    #[test]
    fn rle_runs_cross_rows() {
        let mut b = hdr(3, 2);
        b.extend_from_slice(&[0x00, 0x82, 5, 6, 0x03, 9]);
        let (img, used) = decode_rle(&b).unwrap();
        assert_eq!(img.pixels, vec![0, 5, 6, 9, 9, 9]);
        assert_eq!(used, b.len());
    }

    #[test]
    fn raw_and_flagged() {
        let mut raw = hdr(2, 2);
        raw.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(decode_raw(&raw).unwrap().0.pixels, vec![1, 2, 3, 4]);
        let mut fl = vec![1u8];
        fl.extend_from_slice(&raw);
        let (img, used) = decode_flagged(&fl).unwrap();
        assert_eq!((img.width, img.height, used), (2, 2, 9));
        let mut rle = vec![0u8];
        rle.extend_from_slice(&hdr(2, 1));
        rle.extend_from_slice(&[0x02, 7]);
        assert_eq!(decode_flagged(&rle).unwrap().0.pixels, vec![7, 7]);
    }

    #[test]
    fn bank_and_blit() {
        let mut b = vec![1, 0];
        b.extend_from_slice(&10u32.to_le_bytes());
        let mut f = hdr(2, 1);
        f.extend_from_slice(&[0, 3]);
        b.extend_from_slice(&(f.len() as u32).to_le_bytes());
        b.extend_from_slice(&f);
        let bank = ImageBank::parse(b, FrameKind::Raw).unwrap();
        let img = bank.image(0).unwrap();
        let mut dst = Image { width: 3, height: 1, pixels: vec![8, 8, 8] };
        dst.blit(&img, 1, 0);
        assert_eq!(dst.pixels, vec![8, 8, 3]);
        dst.blit_opaque(&img, -1, 0);
        assert_eq!(dst.pixels, vec![3, 8, 3]);
    }
}
