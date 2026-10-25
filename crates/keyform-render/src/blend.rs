//! Color and alpha composition on premultiplied RGBA.
//!
//! spec: 40-rendering/blending.md

use keyform_model::{AlphaComposition, ColorComposition};

pub type Px = [f32; 4];

/// The fixed-function modes (§3): no destination read needed.
pub fn is_simple(c: ColorComposition, a: AlphaComposition) -> bool {
    matches!(c, ColorComposition::Add | ColorComposition::Multiply)
        || (c == ColorComposition::Normal && a == AlphaComposition::Over)
}

/// §3 simple modes. `src`, `dst` premultiplied.
pub fn blend_simple(c: ColorComposition, src: Px, dst: Px) -> Px {
    match c {
        ColorComposition::Add => [dst[0] + src[0], dst[1] + src[1], dst[2] + src[2], dst[3]],
        ColorComposition::Multiply => {
            let k = 1.0 - src[3];
            [
                src[0] * dst[0] + dst[0] * k,
                src[1] * dst[1] + dst[1] * k,
                src[2] * dst[2] + dst[2] * k,
                dst[3],
            ]
        }
        _ => {
            let k = 1.0 - src[3];
            [
                src[0] + dst[0] * k,
                src[1] + dst[1] * k,
                src[2] + dst[2] * k,
                src[3] + dst[3] * k,
            ]
        }
    }
}

const EPS: f32 = 1e-6;

fn lum(c: [f32; 3]) -> f32 {
    0.30 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        let d = l - n;
        if d.abs() > EPS {
            c = c.map(|ch| l + (ch - l) * l / d);
        }
    }
    if x > 1.0 {
        let d = x - l;
        if d.abs() > EPS {
            c = c.map(|ch| l + (ch - l) * (1.0 - l) / d);
        }
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let (lo, mid, hi) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0; 3];
    if c[hi] - c[lo] > EPS {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}

/// Blend function B(Cs, Cd) on straight colours (§1 table).
pub fn blend_fn(mode: ColorComposition, cs: [f32; 3], cd: [f32; 3]) -> [f32; 3] {
    use ColorComposition::*;
    let per = |f: &dyn Fn(f32, f32) -> f32| [f(cs[0], cd[0]), f(cs[1], cd[1]), f(cs[2], cd[2])];
    match mode {
        Normal | Add | Multiply => cs,
        AddR2Tsl => per(&|s, d| (s + d).min(1.0)),
        AddR2 => per(&|s, d| s + d),
        Darken => per(&|s, d| s.min(d)),
        MultiplyR2 => per(&|s, d| s * d),
        ColorBurnTsl => per(&|s, d| {
            if (d - 1.0).abs() < EPS {
                1.0
            } else if s.abs() < EPS {
                0.0
            } else {
                1.0 - ((1.0 - d) / s).min(1.0)
            }
        }),
        LinearBurnTsl => per(&|s, d| (s + d - 1.0).max(0.0)),
        Lighten => per(&|s, d| s.max(d)),
        Screen => per(&|s, d| s + d - s * d),
        ColorDodgeTsl => per(&|s, d| {
            if d <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (d / (1.0 - s)).min(1.0)
            }
        }),
        Overlay => per(&|s, d| {
            if d < 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }),
        SoftLight => per(&|s, d| {
            if s <= 0.5 {
                d - (1.0 - 2.0 * s) * d * (1.0 - d)
            } else if d <= 0.25 {
                d + (2.0 * s - 1.0) * d * ((16.0 * d - 12.0) * d + 3.0)
            } else {
                d + (2.0 * s - 1.0) * (d.sqrt() - d)
            }
        }),
        HardLight => per(&|s, d| {
            if s < 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }),
        LinearLightTsl => per(&|s, d| {
            if s < 0.5 {
                (2.0 * s + d - 1.0).max(0.0)
            } else {
                (2.0 * (s - 0.5) + d).min(1.0)
            }
        }),
        HslHue => {
            let c01 = |c: [f32; 3]| c.map(|x| x.clamp(0.0, 1.0));
            let (s, d) = (c01(cs), c01(cd));
            set_lum(set_sat(s, sat(d)), lum(d))
        }
        HslColor => set_lum(cs, lum(cd)),
    }
}

/// (p_both, p_src, p_dst) for an alpha composition (§1 second table).
pub fn alpha_terms(a: AlphaComposition, sa: f32, da: f32) -> (f32, f32, f32) {
    match a {
        AlphaComposition::Over => (sa * da, sa * (1.0 - da), da * (1.0 - sa)),
        AlphaComposition::Atop => (sa * da, 0.0, da * (1.0 - sa)),
        AlphaComposition::Out => (0.0, 0.0, da * (1.0 - sa)),
        AlphaComposition::Conjoint => (sa.min(da), (sa - da).max(0.0), (da - sa).max(0.0)),
        AlphaComposition::Disjoint => {
            ((sa + da - 1.0).max(0.0), sa.min(1.0 - da), da.min(1.0 - sa))
        }
    }
}

fn quantize(x: f32) -> f32 {
    (x.clamp(0.0, 1.0) * 255.0).round() / 255.0
}

/// §2 advanced compositing. `src` is the premultiplied texel after
/// multiply/screen, `alpha` = final opacity × clip factor. Returns the new
/// destination pixel (premultiplied), quantised to 8 bits.
pub fn blend_advanced(
    c: ColorComposition,
    a: AlphaComposition,
    src: Px,
    alpha: f32,
    dst: Px,
) -> Px {
    let straight = |p: Px| -> ([f32; 3], f32) {
        if p[3].abs() < EPS {
            ([0.0; 3], p[3])
        } else {
            ([p[0] / p[3], p[1] / p[3], p[2] / p[3]], p[3])
        }
    };
    let (cs, sa) = straight(src);
    let (cd, da) = straight(dst);
    let sa = sa * alpha;
    let b = blend_fn(c, cs, cd);
    let (pb, ps, pd) = alpha_terms(a, sa, da);
    // Spec §2 step 5 writes straight colours weighted by the coverage terms,
    // giving a premultiplied result.
    let out_a = pb + ps + pd;
    let rgb = [0, 1, 2].map(|i| b[i] * pb + cs[i] * ps + cd[i] * pd);
    [
        quantize(rgb[0]),
        quantize(rgb[1]),
        quantize(rgb[2]),
        quantize(out_a),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_over_matches_advanced_path() {
        // Normal+Over through the advanced path equals premultiplied "over" (up to quantisation).
        let src = [0.4, 0.2, 0.1, 0.5];
        let dst = [0.3, 0.6, 0.9, 1.0];
        let simple = blend_simple(ColorComposition::Normal, src, dst);
        let adv = blend_advanced(
            ColorComposition::Normal,
            AlphaComposition::Over,
            src,
            1.0,
            dst,
        );
        for i in 0..4 {
            assert!(
                (simple[i] - adv[i]).abs() <= 0.5 / 255.0 + 1e-6,
                "{simple:?} {adv:?}"
            );
        }
    }

    #[test]
    fn separable_modes() {
        let m = |c, s: f32, d: f32| blend_fn(c, [s; 3], [d; 3])[0];
        assert_eq!(m(ColorComposition::MultiplyR2, 0.5, 0.5), 0.25);
        assert_eq!(m(ColorComposition::Screen, 0.5, 0.5), 0.75);
        assert_eq!(m(ColorComposition::AddR2Tsl, 0.7, 0.7), 1.0);
        assert!((m(ColorComposition::AddR2, 0.7, 0.7) - 1.4).abs() < 1e-6);
        assert_eq!(m(ColorComposition::Overlay, 1.0, 0.25), 0.5);
        assert_eq!(m(ColorComposition::ColorDodgeTsl, 0.5, 0.25), 0.5);
        assert_eq!(m(ColorComposition::ColorBurnTsl, 0.0, 0.5), 0.0);
    }

    #[test]
    fn hue_and_color_keep_destination_luminance() {
        let cs = [1.0, 0.0, 0.0];
        let cd = [0.2, 0.4, 0.6];
        for mode in [ColorComposition::HslColor, ColorComposition::HslHue] {
            let r = blend_fn(mode, cs, cd);
            assert!((lum(r) - lum(cd)).abs() < 1e-5, "{mode:?} {r:?}");
        }
    }

    #[test]
    fn legacy_add_keeps_alpha() {
        let r = blend_simple(
            ColorComposition::Add,
            [0.5, 0.5, 0.5, 0.5],
            [0.2, 0.2, 0.2, 0.4],
        );
        assert_eq!(r[3], 0.4);
        assert!((r[0] - 0.7).abs() < 1e-6);
    }
}
