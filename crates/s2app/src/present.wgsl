// Presentation shaders: the 640x480 game frame (already converted to RGBA)
// drawn into the view rectangle with one of the frontend filters. None of
// this is part of the original; `fs_nearest` shows the frame as it is.

struct Params {
    // View rectangle in framebuffer pixels: x, y, w, h.
    view: vec4<f32>,
    // Source size (640, 480) and output pixels per source pixel.
    src: vec2<f32>,
    scale: vec2<f32>,
    // 1 when the target is an sRGB format (shader output is linear then).
    out_srgb: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var frame: texture_2d<f32>;
@group(0) @binding(2) var linear_clamp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
};

// One triangle covering the viewport.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var o: VsOut;
    o.pos = vec4<f32>(x, y, 0.0, 1.0);
    return o;
}

// Position in source pixels (0..640, 0..480) of a framebuffer pixel.
fn src_pos(frag: vec2<f32>) -> vec2<f32> {
    return (frag - P.view.xy) / P.view.zw * P.src;
}

fn texel(p: vec2<i32>) -> vec3<f32> {
    let q = clamp(p, vec2<i32>(0, 0), vec2<i32>(P.src) - vec2<i32>(1, 1));
    return textureLoad(frame, q, 0).rgb;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, x <= vec3<f32>(0.0031308));
}

// Shader results are sRGB-encoded colours; an sRGB target encodes itself.
fn finish(c: vec3<f32>) -> vec4<f32> {
    if P.out_srgb != 0u {
        return vec4<f32>(srgb_to_linear(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
    }
    return vec4<f32>(c, 1.0);
}

// ---------------------------------------------------------------------------
// nearest: each output pixel shows the source pixel it falls in.

@fragment
fn fs_nearest(v: VsOut) -> @location(0) vec4<f32> {
    let p = src_pos(v.pos.xy);
    return finish(texel(vec2<i32>(floor(p))));
}

// ---------------------------------------------------------------------------
// sharp: nearest inside each source pixel, a one-output-pixel linear blend
// at its edges ("sharp bilinear"), so non-integer scales keep every source
// pixel the same size without the blur of plain bilinear.

@fragment
fn fs_sharp(v: VsOut) -> @location(0) vec4<f32> {
    let p = src_pos(v.pos.xy);
    let base = floor(p - vec2<f32>(0.5));
    let f = p - vec2<f32>(0.5) - base;
    let s = max(P.scale, vec2<f32>(1.0));
    let t = clamp((f - vec2<f32>(0.5)) * s + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let uv = (base + vec2<f32>(0.5) + t) / P.src;
    return finish(textureSampleLevel(frame, linear_clamp, uv, 0.0).rgb);
}

// ---------------------------------------------------------------------------
// xbr: edge-directed smoothing after Hyllian's xBR level 2 (shallow, 45°
// and steep edges), evaluated per output pixel so any scale works.
//
//         A1 B1 C1
//      A0  A  B  C C4
//      D0  D  E  F F4
//      G0  G  H  I I4
//         G5 H5 I5

fn ycbcr(c: vec3<f32>) -> vec3<f32> {
    // BT.2020 luma, as xBRZ compares colours.
    let y = 0.2627 * c.r + 0.6780 * c.g + 0.0593 * c.b;
    return vec3<f32>(y, (c.b - y) * (0.5 / (1.0 - 0.0593)), (c.r - y) * (0.5 / (1.0 - 0.2627)));
}

fn dist(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return length(ycbcr(a) - ycbcr(b));
}

fn same(a: vec3<f32>, b: vec3<f32>) -> bool {
    return dist(a, b) < 0.11;
}

// The bottom-right corner of E (towards I), in a frame where it is the
// bottom-right one; returns the colour to blend in and how much.
fn xbr_corner(fp: vec2<f32>, aa: f32,
              e: vec3<f32>, b: vec3<f32>, c: vec3<f32>, d: vec3<f32>, f: vec3<f32>, g: vec3<f32>,
              h: vec3<f32>, i: vec3<f32>, f4: vec3<f32>, i4: vec3<f32>, h5: vec3<f32>, i5: vec3<f32>) -> vec4<f32> {
    if same(e, f) || same(e, h) {
        return vec4<f32>(e, 0.0);
    }
    let keep = (!same(f, b) && !same(h, d)) || (same(e, i) && !same(f, i4) && !same(h, i5)) || same(e, g) || same(e, c);
    if !keep {
        return vec4<f32>(e, 0.0);
    }
    let across = dist(e, c) + dist(e, g) + dist(i, h5) + dist(i, f4) + 4.0 * dist(h, f);
    let along = dist(h, d) + dist(h, i5) + dist(f, i4) + dist(f, b) + 4.0 * dist(e, i);
    if !(across < along) {
        return vec4<f32>(e, 0.0);
    }
    let fg = dist(f, g);
    let hc = dist(h, c);
    let shallow = 2.0 * fg <= hc && !same(e, g) && !same(d, g);
    let steep = fg >= 2.0 * hc && !same(e, c) && !same(b, c);
    var amount = smoothstep(1.5 - aa, 1.5 + aa, fp.x + fp.y);
    if shallow {
        amount = max(amount, smoothstep(1.0 - aa, 1.0 + aa, fp.y + 0.5 * fp.x));
    }
    if steep {
        amount = max(amount, smoothstep(1.0 - aa, 1.0 + aa, fp.x + 0.5 * fp.y));
    }
    let col = select(h, f, dist(e, f) <= dist(e, h));
    return vec4<f32>(col, amount);
}

@fragment
fn fs_xbr(v: VsOut) -> @location(0) vec4<f32> {
    let p = src_pos(v.pos.xy);
    let q = vec2<i32>(floor(p));
    let fp = p - floor(p);
    let aa = 1.0 / max(min(P.scale.x, P.scale.y), 1.0);

    let a1 = texel(q + vec2<i32>(-1, -2));
    let b1 = texel(q + vec2<i32>(0, -2));
    let c1 = texel(q + vec2<i32>(1, -2));
    let a0 = texel(q + vec2<i32>(-2, -1));
    let a = texel(q + vec2<i32>(-1, -1));
    let b = texel(q + vec2<i32>(0, -1));
    let c = texel(q + vec2<i32>(1, -1));
    let c4 = texel(q + vec2<i32>(2, -1));
    let d0 = texel(q + vec2<i32>(-2, 0));
    let d = texel(q + vec2<i32>(-1, 0));
    let e = texel(q);
    let f = texel(q + vec2<i32>(1, 0));
    let f4 = texel(q + vec2<i32>(2, 0));
    let g0 = texel(q + vec2<i32>(-2, 1));
    let g = texel(q + vec2<i32>(-1, 1));
    let h = texel(q + vec2<i32>(0, 1));
    let i = texel(q + vec2<i32>(1, 1));
    let i4 = texel(q + vec2<i32>(2, 1));
    let g5 = texel(q + vec2<i32>(-1, 2));
    let h5 = texel(q + vec2<i32>(0, 2));
    let i5 = texel(q + vec2<i32>(1, 2));

    // The four corners, each rotated into the bottom-right frame.
    let br = xbr_corner(fp, aa, e, b, c, d, f, g, h, i, f4, i4, h5, i5);
    let bl = xbr_corner(vec2<f32>(fp.y, 1.0 - fp.x), aa, e, f, i, b, h, a, d, g, h5, g5, d0, g0);
    let tl = xbr_corner(vec2<f32>(1.0 - fp.x, 1.0 - fp.y), aa, e, h, g, f, d, c, b, a, d0, a0, b1, a1);
    let tr = xbr_corner(vec2<f32>(1.0 - fp.y, fp.x), aa, e, d, a, h, b, i, f, c, b1, c1, f4, c4);

    var best = br;
    if bl.w > best.w { best = bl; }
    if tl.w > best.w { best = tl; }
    if tr.w > best.w { best = tr; }
    return finish(mix(e, best.rgb, best.w));
}

// ---------------------------------------------------------------------------
// crt: a 640x480 monitor of the time: Gaussian scanlines whose beam widens
// with brightness, a soft horizontal response, an aperture-grille mask and a
// little glow. Computed in linear light.

fn lin(p: vec2<i32>) -> vec3<f32> {
    return srgb_to_linear(texel(p));
}

// Catmull-Rom along one scanline at source x `x`.
fn scanline(x: f32, row: i32) -> vec3<f32> {
    let xb = floor(x - 0.5);
    let t = x - 0.5 - xb;
    let xi = i32(xb);
    let t2 = t * t;
    let t3 = t2 * t;
    let w0 = -0.5 * t3 + t2 - 0.5 * t;
    let w1 = 1.5 * t3 - 2.5 * t2 + 1.0;
    let w2 = -1.5 * t3 + 2.0 * t2 + 0.5 * t;
    let w3 = 0.5 * t3 - 0.5 * t2;
    let c = lin(vec2<i32>(xi - 1, row)) * w0 + lin(vec2<i32>(xi, row)) * w1
        + lin(vec2<i32>(xi + 1, row)) * w2 + lin(vec2<i32>(xi + 2, row)) * w3;
    return max(c, vec3<f32>(0.0));
}

@fragment
fn fs_crt(v: VsOut) -> @location(0) vec4<f32> {
    let p = src_pos(v.pos.xy);
    // Scanlines need about three output rows each to read as lines rather
    // than beat against the pixel grid; fade the effect in from 2x to 3x.
    let strength = clamp(P.scale.y - 2.0, 0.0, 1.0);

    let y = p.y - 0.5;
    let y0 = floor(y);
    let fy = y - y0;
    var beam = vec3<f32>(0.0);
    var flat = vec3<f32>(0.0);
    for (var k = -1; k <= 2; k++) {
        let c = scanline(p.x, i32(y0) + k);
        let dy = fy - f32(k);
        let l = max(c.r, max(c.g, c.b));
        let sigma = mix(0.24, 0.38, l);
        // Not normalised: a bright line (wider beam) carries more light.
        let w = exp(-0.5 * dy * dy / (sigma * sigma));
        beam += c * w;
        if k == 0 || k == 1 {
            flat += c * select(fy, 1.0 - fy, k == 0);
        }
    }
    var col = mix(flat, beam * 1.2, strength);

    // Glow: a wide, faint blur of the neighbourhood.
    let q = vec2<i32>(floor(p));
    var glow = vec3<f32>(0.0);
    for (var gy = -2; gy <= 2; gy++) {
        for (var gx = -2; gx <= 2; gx++) {
            let w = exp(-0.35 * f32(gx * gx + gy * gy));
            glow += lin(q + vec2<i32>(gx, gy)) * w;
        }
    }
    glow /= 9.16;
    col += glow * 0.06 * strength;

    // Aperture grille: one bright channel per output column, the others dimmed.
    let m = i32(floor(v.pos.x)) % 3;
    var mask = vec3<f32>(0.72);
    if m == 0 { mask.r = 1.0; } else if m == 1 { mask.g = 1.0; } else { mask.b = 1.0; }
    let mask_on = clamp(P.scale.x - 2.0, 0.0, 1.0);
    col *= mix(vec3<f32>(1.0), mask * 1.22, mask_on);

    return finish(linear_to_srgb(col));
}
