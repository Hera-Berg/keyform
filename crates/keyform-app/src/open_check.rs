//! Hidden `--check-open FILE`: loads a file the way the editor opens it,
//! prints `OK …` or the error, and exits 0 / 1 without a window. Used by the
//! release workflow to check the installed / bundled binary on each platform.

use std::path::Path;

use crate::document::Document;
use crate::log::Log;

/// Loads `path` like File ▸ Open (a `.can3` with its linked model, a PSD
/// through the import reader, anything else as a model).
pub fn check(path: &Path) -> Result<String, String> {
    let mut log = Log::default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "psd" | "psb" => {
            crate::psd_dialog::read(path)?;
            Ok(format!("OK {}: PSD", path.display()))
        }
        "can3" => {
            let d = Document::open_animation(path, &mut log)?;
            let scenes = d.anim.as_ref().map_or(0, |a| a.doc.animation.scenes.len());
            Ok(format!(
                "OK {}: animation, {scenes} scenes, model with {} parameters",
                path.display(),
                d.model.parameters.len()
            ))
        }
        _ => {
            let d = Document::open(path, &mut log)?;
            Ok(format!(
                "OK {}: {} parameters, {} parts, {} art meshes, {} deformers",
                path.display(),
                d.model.parameters.len(),
                d.model.parts.len(),
                d.model.art_meshes.len(),
                d.model.deformers.len()
            ))
        }
    }
}

/// Runs the check and returns the process exit code.
pub fn run(path: &Path) -> i32 {
    match check(path) {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(e) => {
            eprintln!("ERROR {}: {e}", path.display());
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_bad_files_fail() {
        assert!(check(Path::new("/nonexistent/x.cmo3")).is_err());
        let dir = std::env::temp_dir().join(format!("kf-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.cmo3");
        std::fs::write(&bad, b"not a project").unwrap();
        assert!(check(&bad).is_err());
        assert_eq!(run(&bad), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
