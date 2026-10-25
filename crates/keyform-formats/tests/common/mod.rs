#![allow(dead_code)]
use std::path::{Path, PathBuf};

/// Sample files with extension `ext` from `$KEYFORM_SAMPLES` (colon-separated
/// directories), else the workspace root.
pub fn sample_files(ext: &str) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = match std::env::var("KEYFORM_SAMPLES") {
        Ok(v) => std::env::split_paths(&v).collect(),
        Err(_) => vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")],
    };
    let mut out: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
        .collect();
    out.sort();
    out
}
