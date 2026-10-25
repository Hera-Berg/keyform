//! Evaluates two models at identical parameter values and reports the
//! largest differences per art mesh (matched by ID).

use std::collections::HashMap;
use std::path::Path;

use keyform_eval::{Evaluator, Frame};
use keyform_model::Model;

/// Small deterministic PRNG (xorshift64*), so runs are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32) / (1u64 << 24) as f32
    }
}

pub fn run(a: &Path, b: &Path, samples: usize) -> super::R {
    let ma = super::load_any(a)?;
    let mb = super::load_any(b)?;
    let ea = Evaluator::new(&ma);
    let eb = Evaluator::new(&mb);

    // Parameter sets keyed by ID: defaults, each parameter at min and max, random.
    let mut sets: Vec<(String, HashMap<String, f32>)> = vec![("defaults".into(), HashMap::new())];
    for p in &ma.parameters {
        for (tag, v) in [("min", p.min), ("max", p.max)] {
            sets.push((
                format!("{}={tag}", p.id),
                HashMap::from([(p.id.clone(), v)]),
            ));
        }
    }
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for i in 0..samples {
        let vals = ma
            .parameters
            .iter()
            .map(|p| (p.id.clone(), p.min + (p.max - p.min) * rng.next()))
            .collect();
        sets.push((format!("random#{i}"), vals));
    }

    let values = |m: &Model, set: &HashMap<String, f32>| -> Vec<f32> {
        m.parameters
            .iter()
            .map(|p| set.get(&p.id).copied().unwrap_or(p.default))
            .collect()
    };
    let index_b: HashMap<&str, usize> = mb
        .art_meshes
        .iter()
        .enumerate()
        .map(|(i, a)| (a.info.id.as_str(), i))
        .collect();
    let only_a: Vec<&str> = ma
        .art_meshes
        .iter()
        .map(|a| a.info.id.as_str())
        .filter(|id| !index_b.contains_key(id))
        .collect();
    if !only_a.is_empty() {
        println!(
            "{} meshes only in A (not exported?): {}",
            only_a.len(),
            only_a.join(" ")
        );
    }

    if std::env::var_os("KF_VERBOSE").is_some() {
        let fa = ea.evaluate(&values(&ma, &sets[0].1));
        let fb = eb.evaluate(&values(&mb, &sets[0].1));
        let mut rows = Vec::new();
        for (ia, am) in ma.art_meshes.iter().enumerate() {
            let Some(&ib) = index_b.get(am.info.id.as_str()) else {
                continue;
            };
            let d = fa.meshes[ia]
                .positions
                .chunks(2)
                .zip(fb.meshes[ib].positions.chunks(2))
                .map(|(p, q)| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt())
                .fold(0.0f32, f32::max);
            let mut chain = Vec::new();
            let mut t = am.target;
            while let Some(d) = t {
                let k = match ma.deformers[d].kind {
                    keyform_model::DeformerKind::Warp(_) => "W",
                    _ => "R",
                };
                chain.push(format!("{k}:{}", ma.deformers[d].info.id));
                t = ma.deformers[d].target;
            }
            rows.push((d, am.info.id.clone(), chain.join(" < ")));
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        let n = rows.len();
        for (d, id, chain) in rows
            .iter()
            .take(12)
            .chain(rows.iter().skip(n.saturating_sub(16)))
        {
            println!("  {d:10.4} {id}  {chain}");
        }
        let ok = rows.iter().filter(|r| r.0 < 0.01).count();
        println!("  {ok}/{} meshes within 0.01 px at defaults", rows.len());
    }
    let mut worst_pos = (0.0f32, String::new(), String::new());
    let mut worst_op = (0.0f32, String::new(), String::new());
    let mut drawn_mismatch = 0usize;
    let mut order_mismatch = 0usize;
    let mut first_order_issue = None;
    for (name, set) in &sets {
        let fa: Frame = ea.evaluate(&values(&ma, set));
        let fb: Frame = eb.evaluate(&values(&mb, set));
        for (ia, am) in ma.art_meshes.iter().enumerate() {
            let Some(&ib) = index_b.get(am.info.id.as_str()) else {
                continue;
            };
            let (sa, sb) = (&fa.meshes[ia], &fb.meshes[ib]);
            if sa.is_drawn() != sb.is_drawn() {
                drawn_mismatch += 1;
                continue;
            }
            if !sa.is_drawn() {
                continue;
            }
            for (p, q) in sa.positions.chunks(2).zip(sb.positions.chunks(2)) {
                let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
                if d > worst_pos.0 || d.is_nan() {
                    worst_pos = (d, am.info.id.clone(), name.clone());
                }
            }
            let d = (sa.opacity - sb.opacity).abs();
            if d > worst_op.0 {
                worst_op = (d, am.info.id.clone(), name.clone());
            }
        }
        // Relative render order of meshes drawn in both.
        let ids = |m: &Model, f: &Frame| -> Vec<String> {
            f.render_order
                .iter()
                .filter(|&&i| f.meshes[i].is_drawn())
                .map(|&i| m.art_meshes[i].info.id.clone())
                .collect()
        };
        let (oa, ob) = (ids(&ma, &fa), ids(&mb, &fb));
        let common: Vec<&String> = oa.iter().filter(|id| ob.contains(id)).collect();
        let common_b: Vec<&String> = ob.iter().filter(|id| oa.contains(id)).collect();
        if common != common_b {
            order_mismatch += 1;
            first_order_issue.get_or_insert_with(|| name.clone());
        }
    }
    println!("{} parameter sets compared", sets.len());
    println!(
        "max vertex distance: {:.5} px ({} at {})",
        worst_pos.0, worst_pos.1, worst_pos.2
    );
    println!(
        "max opacity difference: {:.5} ({} at {})",
        worst_op.0, worst_op.1, worst_op.2
    );
    println!("drawn/hidden mismatches: {drawn_mismatch}");
    println!(
        "render order mismatches: {order_mismatch}{}",
        first_order_issue
            .map(|n| format!(" (first at {n})"))
            .unwrap_or_default()
    );
    Ok(())
}
