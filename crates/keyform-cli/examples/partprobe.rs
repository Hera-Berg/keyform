#![allow(clippy::unwrap_used)]
//! Lists each part's art meshes with their evaluated opacity at defaults.
use keyform_eval::Evaluator;
use keyform_formats::moc3::{moc3_to_model, Moc3Raw};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = moc3_to_model(&Moc3Raw::read(&std::fs::read(&a[0]).unwrap()).unwrap()).unwrap();
    let ev = Evaluator::new(&m);
    let f = ev.evaluate(&ev.default_values());
    for part in &a[1..] {
        let pi = m.parts.iter().position(|p| &p.info.id == part).unwrap();
        let ops: Vec<String> = m
            .art_meshes
            .iter()
            .enumerate()
            .filter(|(_, x)| x.info.parent_part == Some(pi))
            .map(|(i, x)| {
                format!(
                    "{}:{:.2}{}",
                    x.info.id,
                    f.meshes[i].form_opacity,
                    if f.meshes[i].outside { "(out)" } else { "" }
                )
            })
            .collect();
        println!("{part}: {}", ops.join(" "));
    }
}
