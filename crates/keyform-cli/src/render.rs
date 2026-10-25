//! `kf render` / `kf sweep`: headless rendering to PNG.

use std::path::Path;

use keyform_eval::Evaluator;
use keyform_model::Model;
use keyform_render::{CpuRenderer, RenderOptions, RgbaImage, View};

struct Args {
    size: u32,
    ss: u32,
    background: bool,
    steps: usize,
    values: Vec<(String, f32)>,
}

fn parse(rest: &[&str]) -> Result<Args, Box<dyn std::error::Error>> {
    let mut a = Args {
        size: 1024,
        ss: 2,
        background: false,
        steps: 5,
        values: Vec::new(),
    };
    let mut it = rest.iter();
    while let Some(&t) = it.next() {
        match t {
            "--size" => a.size = it.next().ok_or("--size N")?.parse()?,
            "--ss" => a.ss = it.next().ok_or("--ss N")?.parse()?,
            "--bg" => a.background = true,
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
    RenderOptions {
        view: View::fit_canvas(m, a.size),
        background: a.background.then_some([1.0, 1.0, 1.0, 1.0]),
        supersample: a.ss,
    }
}

pub fn render_cmd(model: &Path, out: &Path, rest: &[&str]) -> super::R {
    let a = parse(rest)?;
    let m = super::load_any(model)?;
    let r = CpuRenderer::new(&m);
    for w in &r.warnings {
        eprintln!("warning: {w}");
    }
    let frame = Evaluator::new(&m).evaluate(&values(&m, &a.values)?);
    let t = std::time::Instant::now();
    let img = r.render(&frame, &options(&m, &a))?;
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
