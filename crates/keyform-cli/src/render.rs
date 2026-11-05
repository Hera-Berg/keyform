//! `kf render` / `kf sweep`: headless rendering to PNG.

use std::path::Path;

use keyform_eval::Evaluator;
use keyform_model::Model;
use keyform_render::{CpuRenderer, RenderOptions, RgbaImage, View};

struct Args {
    size: u32,
    /// Fixed scale (output = ⌊canvas × scale⌋, like the reference's image
    /// export) instead of fitting into `size`.
    scale: Option<f32>,
    /// `--ss` given explicitly (turns off the export's MSAA-style sampling).
    ss_given: Option<u32>,
    ss: u32,
    background: bool,
    gpu: bool,
    steps: usize,
    values: Vec<(String, f32)>,
}

fn parse(rest: &[&str]) -> Result<Args, Box<dyn std::error::Error>> {
    let mut a = Args {
        size: 1024,
        scale: None,
        ss_given: None,
        ss: 2,
        background: false,
        gpu: false,
        steps: 5,
        values: Vec::new(),
    };
    let mut it = rest.iter();
    while let Some(&t) = it.next() {
        match t {
            "--size" => a.size = it.next().ok_or("--size N")?.parse()?,
            "--scale" => a.scale = Some(it.next().ok_or("--scale S")?.parse()?),
            "--ss" => {
                a.ss = it.next().ok_or("--ss N")?.parse()?;
                a.ss_given = Some(a.ss);
            }
            "--bg" => a.background = true,
            "--gpu" => a.gpu = true,
            _ => match t.split_once('=') {
                Some((id, v)) => a.values.push((id.to_owned(), v.parse()?)),
                None => {
                    a.steps = t
                        .parse()
                        .map_err(|_| format!("unexpected argument {t:?}"))?
                }
            },
        }
    }
    Ok(a)
}

fn values(m: &Model, set: &[(String, f32)]) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut v: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    for (id, x) in set {
        let i = m
            .parameters
            .iter()
            .position(|p| &p.id == id)
            .ok_or_else(|| format!("no parameter {id:?}"))?;
        v[i] = *x;
    }
    Ok(v)
}

fn options(m: &Model, a: &Args) -> RenderOptions {
    // --scale renders like the reference's image export (dialogs.md §20):
    // its camera, and 4 samples per pixel used like 4× MSAA.
    let export = a.scale.and_then(|s| View::for_export(m, s));
    RenderOptions {
        msaa: export.is_some() && a.ss_given.is_none(),
        view: export.unwrap_or_else(|| View::fit_canvas(m, a.size)),
        background: a.background.then_some([1.0, 1.0, 1.0, 1.0]),
        supersample: match (export, a.ss_given) {
            (Some(_), None) => 2,
            _ => a.ss,
        },
    }
}

pub fn render_cmd(model: &Path, out: &Path, rest: &[&str]) -> super::R {
    let m = super::load_any(model)?;
    render_model(&m, out, rest)
}

/// Renders an in-memory model (render options as for `kf render`).
pub fn render_model(m: &keyform_model::Model, out: &Path, rest: &[&str]) -> super::R {
    let a = parse(rest)?;
    let m = m.clone();
    let frame = Evaluator::new(&m).evaluate(&values(&m, &a.values)?);
    let t = std::time::Instant::now();
    let img = if a.gpu {
        let r = keyform_render::gpu::GpuRenderer::new(&m)?;
        for w in &r.warnings {
            eprintln!("warning: {w}");
        }
        eprintln!("gpu: {}", r.adapter_info);
        r.render(&m, &frame, &options(&m, &a))?
    } else {
        let r = CpuRenderer::new(&m);
        for w in &r.warnings {
            eprintln!("warning: {w}");
        }
        r.render(&frame, &options(&m, &a))?
    };
    std::fs::write(out, img.encode_png()?)?;
    println!(
        "wrote {out:?} ({}×{}, {:.0} ms)",
        img.width,
        img.height,
        t.elapsed().as_secs_f64() * 1e3
    );
    Ok(())
}

pub fn sweep_cmd(model: &Path, out: &Path, id: &str, rest: &[&str]) -> super::R {
    let a = parse(rest)?;
    let m = super::load_any(model)?;
    let p = m
        .parameters
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("no parameter {id:?}"))?;
    let steps = a.steps.max(2);
    let r = CpuRenderer::new(&m);
    let ev = Evaluator::new(&m);
    let opts = options(&m, &a);
    let (cw, ch) = (opts.view.width as usize, opts.view.height as usize);
    let mut sheet = RgbaImage::new((cw * steps) as u32, ch as u32);
    for s in 0..steps {
        let v = p.min + (p.max - p.min) * s as f32 / (steps - 1) as f32;
        let mut set = a.values.clone();
        set.push((id.to_owned(), v));
        let img = r.render(&ev.evaluate(&values(&m, &set)?), &opts)?;
        for y in 0..ch {
            let src = &img.data[y * cw * 4..(y + 1) * cw * 4];
            let dst = (y * cw * steps + s * cw) * 4;
            sheet.data[dst..dst + cw * 4].copy_from_slice(src);
        }
    }
    std::fs::write(out, sheet.encode_png()?)?;
    println!(
        "wrote {out:?}: {id} from {} to {} in {steps} steps",
        p.min, p.max
    );
    Ok(())
}

/// Renders the same frame on CPU and GPU and reports the differences;
/// writes an amplified difference image.
pub fn gpudiff_cmd(model: &Path, out: &Path, rest: &[&str]) -> super::R {
    let a = parse(rest)?;
    let m = super::load_any(model)?;
    let frame = Evaluator::new(&m).evaluate(&values(&m, &a.values)?);
    let opts = options(&m, &a);
    let cpu = CpuRenderer::new(&m).render(&frame, &opts)?;
    let g = keyform_render::gpu::GpuRenderer::new(&m)?;
    let gpu = g.render(&m, &frame, &opts)?;
    let mut diff = RgbaImage::new(cpu.width, cpu.height);
    let (mut sum, mut max, mut over8) = (0u64, 0u8, 0usize);
    for (i, (p, q)) in cpu
        .data
        .chunks_exact(4)
        .zip(gpu.data.chunks_exact(4))
        .enumerate()
    {
        // Compare premultiplied values: straight colour is meaningless at
        // near-zero alpha.
        let pm = |c: &[u8]| [0, 1, 2].map(|i| (u16::from(c[i]) * u16::from(c[3]) / 255) as u8);
        let (pp, qq) = (pm(p), pm(q));
        let d = pp
            .iter()
            .zip(&qq)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0)
            .max(p[3].abs_diff(q[3]));
        sum += u64::from(d);
        max = max.max(d);
        if d > 8 {
            over8 += 1;
        }
        let v = d.saturating_mul(8);
        diff.data[4 * i..4 * i + 4].copy_from_slice(&[v, v, v, 255]);
    }
    let n = (cpu.width * cpu.height) as f64;
    std::fs::write(out, diff.encode_png()?)?;
    println!(
        "{}: mean |Δ| {:.3}, max {max}, {:.3}% of pixels differ by > 8 (diff ×8 in {out:?})",
        g.adapter_info,
        sum as f64 / n,
        100.0 * over8 as f64 / n
    );
    Ok(())
}
