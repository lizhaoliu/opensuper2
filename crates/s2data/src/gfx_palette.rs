//! Palette operations exactly as the game performs them.
//!
//! * The game expands 6-bit VGA components with a plain `<< 2` (max 252),
//!   see `FUN_0046d390`.
//! * Fades (`FUN_0046d442` → `FUN_004758c3`) scale entries 1..=255 by
//!   `level / 255` with `level = step << 2`, step 0..=63; entry 0 is left alone.
//! * Translucency and tints pick the nearest palette colour with a k-d tree
//!   (`FUN_0045daaf` build, `FUN_0045dc15` insert, `FUN_0045dd6e` search). The
//!   search only looks inside a ±`radius` box and only at colour indices in
//!   the tree's allowed range; ties go to the first colour found, so the
//!   traversal is ported verbatim.
//! * At start-up the master palette (with entry 0 forced to black) builds the
//!   tree with range 1..=255; `FUN_0045e3c7` then narrows the range to
//!   0x20..=0xfe for the rest of the session and builds 13 tint tables.

/// Expand a 768-byte 6-bit palette to 8-bit RGB the way the game does.
pub fn vga_to_rgb(pal6: &[u8]) -> [[u8; 3]; 256] {
    let mut out = [[0u8; 3]; 256];
    for (i, c) in out.iter_mut().enumerate() {
        for (j, v) in c.iter_mut().enumerate() {
            *v = pal6.get(i * 3 + j).copied().unwrap_or(0).wrapping_shl(2);
        }
    }
    out
}

/// Palette at fade `step` (0 = black, 63 = the brightest the fade reaches).
pub fn fade_rgb(pal6: &[u8], step: u8) -> [[u8; 3]; 256] {
    let level = (step.min(63) as u32) << 2;
    let mut out = vga_to_rgb(pal6);
    for c in out.iter_mut().skip(1) {
        for v in c.iter_mut() {
            *v = (*v as u32 * level / 0xff) as u8;
        }
    }
    out
}

const NIL: u16 = 0x101;

#[derive(Debug, Clone, Copy)]
struct Node {
    /// Components in the game's axis order: G, R, B (6-bit).
    c: [u8; 3],
    index: u16,
    left: u16,
    right: u16,
}

/// Nearest-colour k-d tree over a 6-bit palette.
#[derive(Debug, Clone)]
pub struct ColorTree {
    nodes: Vec<Node>,
    /// The 6-bit palette the tree was built from (entry 0 as given).
    pal6: [u8; 768],
    range: (u16, u16),
}

impl ColorTree {
    /// Build like `FUN_0045daaf`. The game passes the master palette with
    /// entry 0 already zeroed.
    pub fn build(pal6: &[u8]) -> ColorTree {
        let mut p = [0u8; 768];
        for (d, s) in p.iter_mut().zip(pal6) {
            *d = *s;
        }
        let empty = Node { c: [0; 3], index: 0xffff, left: NIL, right: NIL };
        let mut t = ColorTree { nodes: vec![empty; NIL as usize + 1], pal6: p, range: (1, 0xff) };
        let start = nearest_to_grey(&p);
        let order = (0..=start).rev().chain(start + 1..256);
        for (slot, i) in order.enumerate() {
            let c = [p[i * 3 + 1], p[i * 3], p[i * 3 + 2]];
            t.insert(c, i as u16, slot as u16 + 1);
        }
        t
    }

    /// The master-palette tree in its steady state (after `FUN_0045e3c7`).
    pub fn for_master(pal6: &[u8]) -> ColorTree {
        let mut p = pal6[..768.min(pal6.len())].to_vec();
        p.resize(768, 0);
        p[..3].fill(0);
        let mut t = ColorTree::build(&p);
        t.set_range(0x20, 0xfe);
        t
    }

    /// Restrict results to palette indices `lo..=hi` (`FUN_0045e0fa`).
    pub fn set_range(&mut self, lo: u16, hi: u16) {
        self.range = (lo, hi);
    }

    fn insert(&mut self, c: [u8; 3], index: u16, slot: u16) {
        self.nodes[slot as usize] = Node { c, index, left: NIL, right: NIL };
        let mut cur = self.nodes[0].right;
        let mut parent = 0u16;
        let mut axis: isize = -1;
        let mut went_right = true;
        while cur != NIL {
            parent = cur;
            axis = if axis == 2 { 0 } else { axis + 1 };
            let less = c[axis as usize] < self.nodes[cur as usize].c[axis as usize];
            cur = if less { self.nodes[cur as usize].left } else { self.nodes[cur as usize].right };
            went_right = !less;
        }
        if went_right {
            self.nodes[parent as usize].right = slot;
        } else {
            self.nodes[parent as usize].left = slot;
        }
    }

    /// Nearest allowed colour to 6-bit (r, g, b) within ±`radius`; 0xff if none.
    pub fn nearest(&self, r: i32, g: i32, b: i32, radius: i32) -> u8 {
        // Window per axis (G, R, B): a node is visited when lo < v <= hi.
        let lo = [g - radius, r - radius, b - radius];
        let hi = [g + radius, r + radius, b + radius];
        let mut best_d = 0x2e83i32;
        let mut best = NIL;
        let mut stack: Vec<(isize, u16)> = vec![(-1, 0)];
        while best_d != 0 {
            let Some((mut axis, node)) = stack.pop() else { break };
            let mut cur = self.nodes[node as usize].right;
            while cur != NIL && best_d != 0 {
                axis = if axis == 2 { 0 } else { axis + 1 };
                let n = self.nodes[cur as usize];
                let v = n.c[axis as usize] as i32;
                if lo[axis as usize] < v {
                    if v <= hi[axis as usize] {
                        stack.push((axis, cur));
                        if self.range.0 <= n.index && n.index <= self.range.1 {
                            let (nr, ng, nb) = (n.c[1] as i32, n.c[0] as i32, n.c[2] as i32);
                            let d = (nr - r) * (nr - r) + (ng - g) * (ng - g) + (nb - b) * (nb - b);
                            if d < best_d {
                                best = cur;
                                best_d = d;
                            }
                        }
                    }
                    cur = n.left;
                } else {
                    cur = n.right;
                }
            }
        }
        self.nodes[best as usize].index as u8
    }

    fn rgb6(&self, i: u8) -> [i32; 3] {
        let k = i as usize * 3;
        [self.pal6[k] as i32, self.pal6[k + 1] as i32, self.pal6[k + 2] as i32]
    }

    /// `FUN_0045e123`: mix `src` over `dst` at `pct` percent opacity.
    pub fn blend(&self, dst: u8, src: u8, pct: i32) -> u8 {
        if pct >= 100 {
            return src;
        }
        if pct < 1 {
            return dst;
        }
        let (d, s) = (self.rgb6(dst), self.rgb6(src));
        let mix = |k: usize| (s[k] * pct + d[k] * (100 - pct)) / 100;
        self.nearest(mix(0), mix(1), mix(2), 0x20)
    }

    /// `FUN_0045e238`: table mapping every colour toward `color` by `pct` percent.
    pub fn tint_table(&self, color: u8, pct: i32) -> [u8; 256] {
        let mut t = [0u8; 256];
        for (c, v) in t.iter_mut().enumerate() {
            *v = if pct < 1 {
                c as u8
            } else if pct < 100 {
                self.blend(c as u8, color, pct)
            } else {
                color
            };
        }
        t
    }
}

fn nearest_to_grey(p: &[u8; 768]) -> usize {
    let mut best = 0;
    let mut best_d = 0x200;
    for i in 0..256 {
        let d: i32 = (0..3).map(|k| (p[i * 3 + k] as i32 - 0x1f).pow(2)).sum();
        if d < best_d {
            best = i;
            best_d = d;
        }
    }
    best
}

/// The 13 tint tables built at start-up by `FUN_0045e3c7` (colour, percent).
pub const TINTS: [(u8, i32); 13] = [
    (0x21, 0),
    (0x21, 10),
    (0x21, 20),
    (0x21, 30),
    (0x21, 40),
    (0x20, 60),
    (0x20, 50),
    (0xea, 65),
    (0x20, 25),
    (0xbd, 60),
    (0x24, 15),
    (0x3c, 15),
    (0x44, 80),
];

/// Build all start-up tint tables from a tree in its steady state.
pub fn tint_tables(tree: &ColorTree) -> Vec<[u8; 256]> {
    TINTS.iter().map(|&(c, p)| tree.tint_table(c, p)).collect()
}

/// Lazily filled translucency lookup for one frame set (`FUN_0046b5aa`):
/// one 256-entry table per translucent colour, 0 = not computed yet.
#[derive(Debug, Clone)]
pub struct BlendCache {
    from: Vec<u8>,
    pct: Vec<u8>,
    tables: Vec<[u8; 256]>,
}

impl BlendCache {
    pub fn new(remap_from: &[u8], remap_to: &[u8]) -> BlendCache {
        BlendCache { from: remap_from.to_vec(), pct: remap_to.to_vec(), tables: vec![[0; 256]; remap_from.len()] }
    }

    /// Draw `src` onto `dst` at (x, y): index 0 is transparent, translucent
    /// colours blend with what is underneath (`FUN_0046af47` + `FUN_0046b5aa`;
    /// `FUN_0046b6c1` / `FUN_0046b4dd` per pixel and `FUN_0046b3e8` for a run).
    pub fn blit(&mut self, tree: &ColorTree, dst: &mut crate::sprite::Image, src: &crate::sprite::Image, x: i32, y: i32) {
        for sy in 0..src.height as i32 {
            let dy = y + sy;
            if dy < 0 || dy >= dst.height as i32 {
                continue;
            }
            for sx in 0..src.width as i32 {
                let dx = x + sx;
                if dx < 0 || dx >= dst.width as i32 {
                    continue;
                }
                let s = src.pixels[sy as usize * src.width + sx as usize];
                if s != 0 {
                    let d = &mut dst.pixels[dy as usize * dst.width + dx as usize];
                    *d = self.apply(tree, *d, s);
                }
            }
        }
    }

    /// Resulting pixel when effect colour `src` lands on `dst`.
    pub fn apply(&mut self, tree: &ColorTree, dst: u8, src: u8) -> u8 {
        // The game maps colour -> slot with a 256-byte table where later
        // entries overwrite earlier ones, so search from the back.
        let Some(k) = self.from.iter().rposition(|&c| c == src) else { return src };
        let cached = self.tables[k][dst as usize];
        if cached != 0 {
            return cached;
        }
        let v = tree.blend(dst, src, self.pct[k] as i32);
        self.tables[k][dst as usize] = v;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> Vec<u8> {
        // Grey ramp: entry i = (i/4, i/4, i/4).
        (0..768).map(|k| ((k / 3) / 4) as u8).collect()
    }

    #[test]
    fn expansion_and_fade() {
        let p = ramp();
        assert_eq!(vga_to_rgb(&p)[255], [252, 252, 252]);
        assert_eq!(fade_rgb(&p, 0)[255], [0, 0, 0]);
        assert_eq!(fade_rgb(&p, 63)[255], [(252 * 252 / 255) as u8; 3]);
    }

    #[test]
    fn nearest_matches_brute_force_inside_window() {
        let t = ColorTree::build(&ramp());
        for v in [3, 17, 40, 63] {
            let got = t.nearest(v, v, v, 0x20) as i32;
            // Several indices share a grey level; any with that level is fine.
            assert_eq!(got / 4, v.min(63));
        }
    }

    #[test]
    fn blend_extremes() {
        let t = ColorTree::build(&ramp());
        assert_eq!(t.blend(10, 200, 100), 200);
        assert_eq!(t.blend(10, 200, 0), 10);
        let mid = t.blend(0, 200, 50) as i32;
        assert_eq!(mid / 4, 25);
    }
}
