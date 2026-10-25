#![allow(clippy::unwrap_used)]
//! Ad-hoc inspection of a project: object order, visibility, sketch flags.
use keyform_formats::cmo3::Cmo3Document;

fn main() {
    let path = std::env::args().nth(1).expect("path");
    let doc = Cmo3Document::read(&std::fs::read(path).unwrap()).unwrap();
    let m = &doc.model;
    let pname = |p: Option<usize>| p.map_or("-".to_string(), |p| m.parts[p].info.id.clone());
    println!("PARTS (model order):");
    for p in &m.parts {
        println!(
            "  {} vis={} sketch={} group={} parent={} default_order={} kf={}",
            p.info.id,
            p.info.visible,
            p.sketch,
            p.draw_order_group,
            pname(p.info.parent_part),
            p.default_order,
            p.grid.keyforms.len()
        );
    }
    println!("ART MESHES not visible / in hidden or sketch parts:");
    for a in &m.art_meshes {
        let mut hidden_anc = false;
        let mut sketch = false;
        let mut t = a.info.parent_part;
        while let Some(p) = t {
            hidden_anc |= !m.parts[p].info.visible;
            sketch |= m.parts[p].sketch;
            t = m.parts[p].info.parent_part;
        }
        if !a.info.visible || hidden_anc || sketch || a.texture.is_none() {
            println!(
                "  {} vis={} hidden_anc={} sketch_anc={} tex={:?}",
                a.info.id, a.info.visible, hidden_anc, sketch, a.texture
            );
        }
    }
    println!(
        "DEFORMERS (model order): {}",
        m.deformers
            .iter()
            .map(|d| format!(
                "{}<{}",
                d.info.id,
                d.target
                    .map_or("ROOT".into(), |t| m.deformers[t].info.id.clone())
            ))
            .collect::<Vec<_>>()
            .join(" ")
    );
}
