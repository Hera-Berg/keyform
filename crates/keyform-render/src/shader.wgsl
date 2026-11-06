// Keyform drawable shader. spec: 40-rendering/appearance.md §4,
// blending.md §1–3, clipping.md §2.

struct Draw {
    // Premultiplied base colour (α, α, α, α for white).
    base: vec4<f32>,
    mul: vec4<f32>,
    scr: vec4<f32>,
    // x: has mask, y: invert mask, z: final opacity (advanced path)
    flags: vec4<f32>,
    // x: colour composition, y: alpha composition (moc3 codes)
    modes: vec4<u32>,
    // xy: UV scale onto the unpadded texture area (appearance.md §5)
    uvs: vec4<f32>,
};

@group(0) @binding(0) var<uniform> d: Draw;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var mask: texture_2d<f32>;
@group(0) @binding(4) var dst_copy: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(pos, 0.0, 1.0);
    o.uv = uv;
    return o;
}

// Texture sample (appearance.md §5): the sampler does bilinear, trilinear
// or anisotropic filtering over the premultiplied mip chain; outside the
// texture is transparent where the device supports border clamping.
fn sample_tex(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(tex, samp, uv * d.uvs.xy);
}

// Premultiplied texel with multiply and screen colour applied (appearance.md §4).
fn shaded(uv: vec2<f32>) -> vec4<f32> {
    let c = sample_tex(uv);
    let rgb = c.rgb * d.mul.rgb;
    return vec4<f32>(rgb + d.scr.rgb * c.a - rgb * d.scr.rgb, c.a);
}

fn mask_factor(p: vec4<f32>) -> f32 {
    if (d.flags.x == 0.0) {
        return 1.0;
    }
    var k = textureLoad(mask, vec2<i32>(p.xy), 0).a;
    if (d.flags.y != 0.0) {
        k = 1.0 - k;
    }
    return k;
}

@fragment
fn fs_simple(in: VsOut) -> @location(0) vec4<f32> {
    // The "illegal" look (blending.md §5): texture × (1, 0, 0, 0.5).
    if (d.flags.w != 0.0) {
        return sample_tex(in.uv) * vec4<f32>(1.0, 0.0, 0.0, 0.5);
    }
    var o = shaded(in.uv) * d.base;
    o = vec4<f32>(min(o.rgb, vec3<f32>(o.a)), o.a);
    return o * mask_factor(in.pos);
}

// Clip-source pass: texture alpha only, accumulated with premultiplied over.
@fragment
fn fs_mask(in: VsOut) -> @location(0) vec4<f32> {
    let a = sample_tex(in.uv).a;
    return vec4<f32>(a);
}

const EPS: f32 = 1e-6;

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.30, 0.59, 0.11));
}

fn clip_color(c0: vec3<f32>) -> vec3<f32> {
    var c = c0;
    let l = lum(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    if (n < 0.0 && abs(l - n) > EPS) {
        c = l + (c - l) * l / (l - n);
    }
    if (x > 1.0 && abs(x - l) > EPS) {
        c = l + (c - l) * (1.0 - l) / (x - l);
    }
    return c;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_color(c + (l - lum(c)));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mx = max(max(c.r, c.g), c.b);
    let mn = min(min(c.r, c.g), c.b);
    if (mx - mn <= EPS) {
        return vec3<f32>(0.0);
    }
    return (c - mn) * s / (mx - mn);
}

fn burn(s: f32, d: f32) -> f32 {
    if (abs(d - 1.0) < EPS) { return 1.0; }
    if (abs(s) < EPS) { return 0.0; }
    return 1.0 - min(1.0, (1.0 - d) / s);
}

fn dodge(s: f32, d: f32) -> f32 {
    if (d <= 0.0) { return 0.0; }
    if (s >= 1.0) { return 1.0; }
    return min(1.0, d / (1.0 - s));
}

fn overlay1(s: f32, d: f32) -> f32 {
    if (d < 0.5) { return 2.0 * s * d; }
    return 1.0 - 2.0 * (1.0 - s) * (1.0 - d);
}

fn soft1(s: f32, d: f32) -> f32 {
    if (s <= 0.5) { return d - (1.0 - 2.0 * s) * d * (1.0 - d); }
    if (d <= 0.25) { return d + (2.0 * s - 1.0) * d * ((16.0 * d - 12.0) * d + 3.0); }
    return d + (2.0 * s - 1.0) * (sqrt(d) - d);
}

fn linear_light1(s: f32, d: f32) -> f32 {
    if (s < 0.5) { return max(0.0, 2.0 * s + d - 1.0); }
    return min(1.0, 2.0 * (s - 0.5) + d);
}

// Blend function B(Cs, Cd) on straight colours (blending.md §1).
fn blend_fn(mode: u32, cs: vec3<f32>, cd: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 3u: { return min(cs + cd, vec3<f32>(1.0)); }
        case 4u: { return cs + cd; }
        case 5u: { return min(cs, cd); }
        case 6u: { return cs * cd; }
        case 7u: { return vec3<f32>(burn(cs.r, cd.r), burn(cs.g, cd.g), burn(cs.b, cd.b)); }
        case 8u: { return max(vec3<f32>(0.0), cs + cd - 1.0); }
        case 9u: { return max(cs, cd); }
        case 10u: { return cs + cd - cs * cd; }
        case 11u: { return vec3<f32>(dodge(cs.r, cd.r), dodge(cs.g, cd.g), dodge(cs.b, cd.b)); }
        case 12u: { return vec3<f32>(overlay1(cs.r, cd.r), overlay1(cs.g, cd.g), overlay1(cs.b, cd.b)); }
        case 13u: { return vec3<f32>(soft1(cs.r, cd.r), soft1(cs.g, cd.g), soft1(cs.b, cd.b)); }
        case 14u: { return vec3<f32>(overlay1(cd.r, cs.r), overlay1(cd.g, cs.g), overlay1(cd.b, cs.b)); }
        case 15u: { return vec3<f32>(linear_light1(cs.r, cd.r), linear_light1(cs.g, cd.g), linear_light1(cs.b, cd.b)); }
        case 16u: {
            let s = clamp(cs, vec3<f32>(0.0), vec3<f32>(1.0));
            let dd = clamp(cd, vec3<f32>(0.0), vec3<f32>(1.0));
            return set_lum(set_sat(s, sat(dd)), lum(dd));
        }
        case 17u: { return set_lum(cs, lum(cd)); }
        default: { return cs; }
    }
}

// Advanced compositing (blending.md §2): replaces the destination; the
// 8-bit target quantises the result.
@fragment
fn fs_advanced(in: VsOut) -> @location(0) vec4<f32> {
    let src = shaded(in.uv);
    let dst = textureLoad(dst_copy, vec2<i32>(in.pos.xy), 0);
    var cs = vec3<f32>(0.0);
    if (abs(src.a) >= EPS) { cs = src.rgb / src.a; }
    var cd = vec3<f32>(0.0);
    if (abs(dst.a) >= EPS) { cd = dst.rgb / dst.a; }
    let sa = src.a * d.flags.z * mask_factor(in.pos);
    let da = dst.a;
    var p = vec3<f32>(0.0);
    switch d.modes.y {
        case 1u: { p = vec3<f32>(sa * da, 0.0, da * (1.0 - sa)); }
        case 2u: { p = vec3<f32>(0.0, 0.0, da * (1.0 - sa)); }
        case 3u: { p = vec3<f32>(min(sa, da), max(sa - da, 0.0), max(da - sa, 0.0)); }
        case 4u: { p = vec3<f32>(max(sa + da - 1.0, 0.0), min(sa, 1.0 - da), min(da, 1.0 - sa)); }
        default: { p = vec3<f32>(sa * da, sa * (1.0 - da), da * (1.0 - sa)); }
    }
    let b = blend_fn(d.modes.x, cs, cd);
    let rgb = b * p.x + cs * p.y + cd * p.z;
    return clamp(vec4<f32>(rgb, p.x + p.y + p.z), vec4<f32>(0.0), vec4<f32>(1.0));
}
