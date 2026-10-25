#![allow(clippy::unwrap_used)]
//! Prints the physics3.json Keyform would export for a project.
use keyform_formats::cmo3::Cmo3Document;
use keyform_physics::{json3, to_json_string};
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let doc = Cmo3Document::read(&std::fs::read(p).unwrap()).unwrap();
    let (rig, msgs) = json3::verify(&doc.model.physics, &doc.model.parameters);
    eprintln!("{msgs:?}");
    println!(
        "{}",
        to_json_string(&json3::to_json(&rig, &doc.model.parameters, true).unwrap())
    );
}
