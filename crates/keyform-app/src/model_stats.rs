//! File ▸ Model Statistics…: a text report on the current model.
//! spec: 50-ui/dialogs.md §19 (layout as the reference prints it).

use eframe::egui;
use keyform_model::{DeformerKind, Model};

use crate::document::{Document, Selection};

/// The counts of the report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub parts: usize,
    pub warps: usize,
    pub rotations: usize,
    pub deformer_keyforms: usize,
    pub one_draw: usize,
    pub one_deformer: usize,
    pub empty: Vec<usize>,
    pub meshes: usize,
    pub mesh_keyforms: usize,
    pub vertices: usize,
    pub polygons: usize,
    pub advanced_blend: usize,
    pub art_paths: usize,
    pub art_path_points: usize,
    pub offscreen_parts: usize,
    pub offscreen_blend: usize,
    pub offscreen_depth: usize,
    pub clippings: usize,
    pub mask_types: usize,
    pub moc3_issues: usize,
}

pub fn stats(m: &Model) -> Stats {
    let mut s = Stats {
        parts: m
            .parts
            .len()
            .saturating_sub(usize::from(m.root_part.is_some())),
        meshes: m.art_meshes.len(),
        art_paths: m.art_paths.len(),
        ..Stats::default()
    };
    for d in &m.deformers {
        match &d.kind {
            DeformerKind::Warp(w) => {
                s.warps += 1;
                s.deformer_keyforms += w.grid.keyforms.len();
            }
            DeformerKind::Rotation(r) => {
                s.rotations += 1;
                s.deformer_keyforms += r.grid.keyforms.len();
            }
        }
    }
    // Direct children of each deformer: deformers and drawables.
    for d in 0..m.deformers.len() {
        let defs = m.deformers.iter().filter(|x| x.target == Some(d)).count();
        let draws = m.art_meshes.iter().filter(|x| x.target == Some(d)).count()
            + m.art_paths.iter().filter(|x| x.target == Some(d)).count();
        match (defs, draws) {
            (0, 1) => s.one_draw += 1,
            (1, 0) => s.one_deformer += 1,
            (0, 0) => s.empty.push(d),
            _ => {}
        }
    }
    let mut masks: Vec<Vec<usize>> = Vec::new();
    for a in &m.art_meshes {
        s.mesh_keyforms += a.grid.keyforms.len();
        s.vertices += a.vertex_count();
        s.polygons += a.indices.len() / 3;
        if a.color_composition != Default::default() || a.alpha_composition != Default::default() {
            s.advanced_blend += 1;
        }
        if !a.clip.is_empty() {
            s.clippings += 1;
            let mut k = a.clip.clone();
            k.sort_unstable();
            if !masks.contains(&k) {
                masks.push(k);
            }
        }
        if a.clip_missing || m.illegal_look(a) {
            s.moc3_issues += 1;
        }
    }
    s.mask_types = masks.len();
    s.art_path_points = m
        .art_paths
        .iter()
        .map(|p| p.grid.keyforms.first().map_or(0, |f| f.points.len()))
        .sum();
    // Offscreen parts and their nesting depth.
    let depth = |mut p: Option<usize>| {
        let mut n = 0;
        let mut guard = 0;
        while let Some(q) = p {
            if m.parts.get(q).is_some_and(|x| x.offscreen) {
                n += 1;
            }
            p = m.parts.get(q).and_then(|x| x.info.parent_part);
            guard += 1;
            if guard > m.parts.len() {
                break;
            }
        }
        n
    };
    for (i, p) in m.parts.iter().enumerate() {
        if p.offscreen {
            s.offscreen_parts += 1;
            if p.color_composition != Default::default()
                || p.alpha_composition != Default::default()
            {
                s.offscreen_blend += 1;
            }
            s.offscreen_depth = s.offscreen_depth.max(depth(Some(i)));
        }
    }
    s
}

/// The report text, headed by the file name and the time.
pub fn report(m: &Model, title: &str, time: &str) -> String {
    let s = stats(m);
    format!(
        "{title}  [ {time} ]\n\n\
         << Common Data >>\n  Number of Parts : {}\n\n\
         << Deformer Data >>\n  Number of Warp Deformers : {}\n  Number of Rotation Deformers : {}\n  \
         Number of Interpolations in Deformers : {}\n\n  \
         Number of Deformers with Only One Draw Element : {}\n  \
         Number of Deformers with Only One Deformer : {}\n  \
         Number of Deformers with No Content : {}\n\n\
         << ArtMesh Related  >>\n  Number of ArtMesh  : {}\n  Number of Interpolation ArtMesh : {}\n  \
         Number of Vertices : {}\n  Number of Polygons   : {}\n  \
         Number of advanced blend mode uses: {}\n  Number of ArtPath:{}\n  \
         Number of ArtPath's control points:{}\n\n\
         <<Offscreen Drawing Related>>\n  Number of parts to draw: {}\n  \
         Number of advanced blend mode uses: {}\n  Maximum depth : {}\n\n\
         <<Clipping Mask Related>>\n  Clippings used: {}\n  Mask ID permutation types: {}\n  \
         Objects with moc3 output issues: {}\n",
        s.parts,
        s.warps,
        s.rotations,
        s.deformer_keyforms,
        s.one_draw,
        s.one_deformer,
        s.empty.len(),
        s.meshes,
        s.mesh_keyforms,
        s.vertices,
        s.polygons,
        s.advanced_blend,
        s.art_paths,
        s.art_path_points,
        s.offscreen_parts,
        s.offscreen_blend,
        s.offscreen_depth,
        s.clippings,
        s.mask_types,
        s.moc3_issues,
    )
}

/// The Model Statistics window (non-modal).
#[derive(Default)]
pub struct StatsWindow {
    pub open: bool,
    text: String,
    /// Auto Update (remembered with the app settings by the caller).
    pub auto: bool,
    stale: bool,
}

impl StatsWindow {
    pub fn show(&mut self) {
        self.open = true;
        self.stale = true;
    }
}

fn now() -> String {
    // Local time is not available without a time-zone crate: UTC.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}/{m:02}/{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

pub fn window(ctx: &egui::Context, st: &mut StatsWindow, doc: Option<&mut Document>) {
    if !st.open {
        return;
    }
    let Some(doc) = doc else {
        st.open = false;
        return;
    };
    if st.stale || st.auto {
        st.text = report(&doc.model, &doc.title, &now());
        st.stale = false;
    }
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Model Statistics"))
        .default_size([400.0, 600.0])
        .open(&mut open)
        .show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(crate::i18n::tr("Select"), |ui| {
                    let empty = stats(&doc.model).empty;
                    if ui
                        .add_enabled(
                            !empty.is_empty(),
                            egui::Button::new(crate::i18n::tr("Select Empty Deformers")),
                        )
                        .clicked()
                    {
                        let mut set = crate::document::SelectionSet::of(None);
                        set.objects = empty.into_iter().map(Selection::Deformer).collect();
                        doc.set_selection(set);
                        ui.close();
                    }
                });
            });
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 30.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut st.text.as_str())
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Update")).clicked() {
                    st.stale = true;
                }
                ui.checkbox(&mut st.auto, crate::i18n::tr("Auto Update"));
            });
        });
    if !open {
        st.open = false;
    }
}

#[cfg(test)]
mod tests {
    /// The reference's report for test-data/blackbox/q48/copy.cmo3 (a local
    /// black-box file; skipped when absent).
    #[test]
    fn matches_the_reference_report() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("test-data/blackbox/q48/copy.cmo3");
        let Ok(data) = std::fs::read(&path) else {
            return;
        };
        let doc = keyform_formats::cmo3::Cmo3Document::read(&data).unwrap();
        let s = super::stats(&doc.model);
        assert_eq!(
            (s.parts, s.warps, s.rotations, s.deformer_keyforms),
            (0, 7, 3, 57)
        );
        assert_eq!((s.one_draw, s.one_deformer, s.empty.len()), (5, 0, 1));
        assert_eq!(
            (s.meshes, s.mesh_keyforms, s.vertices, s.polygons),
            (21, 72, 241, 290)
        );
        assert_eq!((s.clippings, s.mask_types, s.moc3_issues), (3, 3, 0));
    }
}
