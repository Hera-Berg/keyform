#![allow(clippy::unwrap_used)]
//! Runtime animation files from locally available samples (never committed):
//! every `*.motion3.json`, `*.exp3.json` and `*.pose3.json` found (recursively)
//! under `$KEYFORM_SAMPLES` must parse, and motions must re-serialize to the
//! identical text.

use std::path::{Path, PathBuf};

use keyform_anim::{json, Expression, Motion, Pose};

fn files(suffix: &str) -> Vec<PathBuf> {
    fn walk(d: &Path, suffix: &str, out: &mut Vec<PathBuf>, depth: usize) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 4 {
                walk(&p, suffix, out, depth + 1);
            } else if p.to_string_lossy().ends_with(suffix) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(v) = std::env::var("KEYFORM_SAMPLES") {
        for d in std::env::split_paths(&v) {
            walk(&d, suffix, &mut out, 0);
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn motions_roundtrip_byte_exact() {
    for p in files(".motion3.json") {
        let text = std::fs::read_to_string(&p).unwrap();
        let m = Motion::parse(&text).unwrap_or_else(|e| panic!("{p:?}: {e}"));
        let out = json::to_string(&m.to_json());
        let src = text.trim_start_matches('\u{feff}');
        if out != src {
            let at = out
                .bytes()
                .zip(src.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            panic!(
                "{p:?} differs at {at}:\n--- file\n{}\n--- ours\n{}",
                &src[at.saturating_sub(80)..(at + 80).min(src.len())],
                &out[at.saturating_sub(80)..(at + 80).min(out.len())]
            );
        }
        eprintln!("{p:?}: {} curves byte-exact", m.curves.len());
    }
}

#[test]
fn expressions_and_poses_parse() {
    for p in files(".exp3.json") {
        Expression::parse(&std::fs::read_to_string(&p).unwrap())
            .unwrap_or_else(|e| panic!("{p:?}: {e}"));
    }
    for p in files(".pose3.json") {
        let pose = Pose::parse(&std::fs::read_to_string(&p).unwrap())
            .unwrap_or_else(|e| panic!("{p:?}: {e}"));
        assert!(!pose.groups.is_empty(), "{p:?}");
    }
}
