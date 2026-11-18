//! Keyform's own model template list (Q-0041: not the reference's package
//! format): one folder per template holding `template.json`, the model
//! and a thumbnail.
//!
//! ```text
//! <templates>/<folder>/template.json   {"format":"keyform-template","version":1,
//!                                        "name","description","author",
//!                                        "model":"model.cmo3","thumbnail":"thumbnail.png",
//!                                        "files":[…attached files…]}
//! <templates>/<folder>/model.cmo3
//! <templates>/<folder>/thumbnail.png
//! ```
//! spec: 70-advanced/templates.md §1

use std::path::{Path, PathBuf};

use crate::document::Document;

pub const MANIFEST: &str = "template.json";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub model: String,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Entry {
    pub fn model_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.model)
    }
}

/// The template folder: `KEYFORM_TEMPLATES`, else the user data folder.
pub fn templates_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("KEYFORM_TEMPLATES") {
        return d.into();
    }
    #[cfg(target_os = "windows")]
    if let Some(a) = std::env::var_os("APPDATA") {
        return Path::new(&a).join("Keyform").join("templates");
    }
    #[cfg(target_os = "macos")]
    if let Some(h) = std::env::var_os("HOME") {
        return Path::new(&h).join("Library/Application Support/Keyform/templates");
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return Path::new(&x).join("keyform").join("templates");
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join(".local/share/keyform/templates")
}

/// Templates in `dir`, by name. Folders without a valid manifest are skipped.
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let d = e.path();
            let text = std::fs::read_to_string(d.join(MANIFEST)).ok()?;
            let manifest: Manifest = serde_json::from_str(&text).ok()?;
            (manifest.format == "keyform-template").then_some(Entry { dir: d, manifest })
        })
        .collect();
    v.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    v
}

/// A folder name from a template name (letters, digits, `-`, `_`, space).
fn folder_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim().to_owned();
    if s.is_empty() {
        "template".into()
    } else {
        s
    }
}

/// Export model template: writes the model, a thumbnail and the manifest.
pub fn export(
    dir: &Path,
    doc: &Document,
    name: &str,
    description: &str,
    author: &str,
) -> Result<PathBuf, String> {
    if list(dir).iter().any(|e| e.manifest.name == name) {
        return Err("A Model Template with the same name already exists".into());
    }
    let folder = dir.join(folder_name(name));
    if folder.exists() {
        return Err("A Model Template with the same name already exists".into());
    }
    let bytes = doc.project_bytes()?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::write(folder.join("model.cmo3"), bytes).map_err(|e| e.to_string())?;
    // Thumbnail: the export view scaled so the longer side is ≤ 256 px.
    let side = doc.model.canvas.width.max(doc.model.canvas.height).max(1) as f32;
    let thumb = crate::export_image::render(doc, (25600.0 / side).min(100.0), false)
        .ok()
        .and_then(|img| img.encode_png().ok())
        .and_then(|png| std::fs::write(folder.join("thumbnail.png"), png).ok())
        .map(|_| "thumbnail.png".to_owned());
    let m = Manifest {
        format: "keyform-template".into(),
        version: 1,
        name: name.to_owned(),
        description: description.to_owned(),
        author: author.to_owned(),
        model: "model.cmo3".into(),
        thumbnail: thumb,
        files: Vec::new(),
    };
    let text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
    std::fs::write(folder.join(MANIFEST), text).map_err(|e| e.to_string())?;
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_and_list() {
        let Some(dir) = std::env::var_os("KEYFORM_BB_P5") else {
            return;
        };
        let mut log = crate::log::Log::default();
        let doc = Document::open(&Path::new(&dir).join("bs.cmo3"), &mut log).unwrap();
        let t = std::env::temp_dir().join(format!("kf-templates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let f = export(&t, &doc, "Girl / basic", "A test rig", "Keyform tests").unwrap();
        assert!(f.join("model.cmo3").exists() && f.join("thumbnail.png").exists());
        let l = list(&t);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].manifest.name, "Girl / basic");
        assert!(
            export(&t, &doc, "Girl / basic", "", "").is_err(),
            "same name refused"
        );
        // The exported model loads.
        assert!(keyform_formats::cmo3::Cmo3Document::read(
            &std::fs::read(l[0].model_path()).unwrap()
        )
        .is_ok());
        let _ = std::fs::remove_dir_all(&t);
    }
}
