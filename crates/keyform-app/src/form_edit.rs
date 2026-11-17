//! Form Edit in the Animation workspace: form keys on art meshes, made by
//! dragging vertices on the canvas, in Adjustment Mode 1 (global: added to
//! the final form) or Mode 2 (local: added before the deformer).
//! spec: 60-animation/form-animation-and-controllers.md §1

use eframe::egui::{self, Pos2, Rect};
use keyform_model::animation::{FormKey, FormObject, TrackKind};

use crate::animation::{FrameOpts, UndoStep};
use crate::document::Document;

/// A vertex drag in progress.
pub struct FormDrag {
    pub mesh: usize,
    pub vertex: usize,
    /// Last pointer position (scene pixels).
    pub last: [f32; 2],
}

/// Pick radius in screen pixels.
const PICK: f32 = 8.0;

fn solve2(m: [[f32; 2]; 2], d: [f32; 2]) -> Option<[f32; 2]> {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if det.abs() < 1e-12 {
        return None;
    }
    Some([
        (d[0] * m[1][1] - d[1] * m[0][1]) / det,
        (m[0][0] * d[1] - m[1][0] * d[0]) / det,
    ])
}

impl Document {
    fn form_track(&self, ti: usize) -> Option<&keyform_model::animation::Track> {
        self.anim.as_ref()?.scene()?.tracks.get(ti)
    }

    /// The model track's index and its frame-stepped frame at the playhead.
    fn form_frame(&self) -> Option<(usize, f64, i32)> {
        let st = self.anim.as_ref()?;
        let (ti, _) = st.model_track()?;
        let t = &st.scene()?.tracks[ti];
        let x = st.frame - f64::from(t.start) + f64::from(t.offset);
        let fx = keyform_anim::scene::step_frame(x, t.frame_step.as_ref());
        Some((ti, st.frame, fx.round() as i32))
    }

    /// Makes sure art mesh `mi` has a form key at the playhead in the
    /// current Adjustment Mode's map. A new key's original form is what the
    /// mesh shows now without form animation (Mode 1: final form; Mode 2:
    /// interpolated local form); its target keeps the current difference,
    /// so nothing moves. Returns false when there is no model track.
    pub fn create_form_key(&mut self, mi: usize) -> bool {
        let Some((ti, frame, f)) = self.form_frame() else {
            return false;
        };
        let Some(am) = self.model.art_meshes.get(mi) else {
            return false;
        };
        let guid = am.info.guid.clone();
        let local = match self.form_track(ti).map(|t| &t.kind) {
            Some(TrackKind::Model(m)) => m.form.local_mode,
            _ => return false,
        };
        let original: Vec<f32> = if local {
            let Some((values, _)) = self.anim_values_at(frame) else {
                return false;
            };
            match keyform_eval::interpolate(
                &self.model.parameters,
                &values,
                &am.grid,
                &am.blend_shapes,
            ) {
                Some(r) => r.form.positions,
                None => return false,
            }
        } else {
            let o = FrameOpts {
                unplaced: true,
                no_form: true,
                ..Default::default()
            };
            match self.anim_frame_opts(frame, &o) {
                Some(fr) => fr.meshes[mi].positions.clone(),
                None => return false,
            }
        };
        let Some(st) = self.anim.as_mut() else {
            return false;
        };
        let s = st.scene;
        let TrackKind::Model(m) = &mut st.doc.animation.scenes[s].tracks[ti].kind else {
            return false;
        };
        let map = if local {
            &mut m.form.local
        } else {
            &mut m.form.global
        };
        let oi = match map.iter().position(|o| o.guid == guid) {
            Some(i) => i,
            None => {
                map.push(FormObject {
                    guid,
                    keys: Vec::new(),
                    time_remap: None,
                });
                map.len() - 1
            }
        };
        let obj = &mut map[oi];
        if obj.keys.iter().any(|k| k.frame == f) {
            return true;
        }
        let d = keyform_anim::scene::form_difference(obj, f64::from(f))
            .filter(|d| d.len() == original.len())
            .unwrap_or_else(|| vec![0.0; original.len()]);
        let target = original.iter().zip(&d).map(|(a, b)| a + b).collect();
        let at = obj.keys.partition_point(|k| k.frame < f);
        obj.keys.insert(
            at,
            FormKey {
                frame: f,
                original,
                target,
            },
        );
        m.form.apply = true;
        st.dirty = true;
        true
    }

    /// Moves vertex `vi` of art mesh `mi` by `delta` scene pixels in the
    /// form key at the playhead (made if needed). The delta is taken back
    /// through the track placement (Mode 1) or, numerically, through the
    /// deformers and placement (Mode 2).
    pub fn move_form_vertex(&mut self, mi: usize, vi: usize, delta: [f32; 2]) {
        if !self.create_form_key(mi) {
            return;
        }
        let Some((ti, frame, f)) = self.form_frame() else {
            return;
        };
        let local = matches!(
            self.form_track(ti).map(|t| &t.kind),
            Some(TrackKind::Model(m)) if m.form.local_mode
        );
        // d(scene vertex) / d(key value), by finite differences.
        let h = if local { 1e-3 } else { 1.0 };
        let probe = |extra: Option<[f32; 2]>, me: &Document| -> Option<[f32; 2]> {
            let o = FrameOpts {
                extra_local: extra.map(|e| {
                    let n = me.model.art_meshes[mi].vertex_count();
                    let mut v = vec![0.0; 2 * n];
                    v[2 * vi] = e[0];
                    v[2 * vi + 1] = e[1];
                    (mi, v)
                }),
                ..Default::default()
            };
            let fr = me.anim_frame_opts(frame, &o)?;
            let p = fr.meshes.get(mi)?.positions.get(2 * vi..2 * vi + 2)?;
            Some([p[0], p[1]])
        };
        let jac = if local {
            (|| {
                let b = probe(None, self)?;
                let x = probe(Some([h, 0.0]), self)?;
                let y = probe(Some([0.0, h]), self)?;
                Some([
                    [(x[0] - b[0]) / h, (y[0] - b[0]) / h],
                    [(x[1] - b[1]) / h, (y[1] - b[1]) / h],
                ])
            })()
        } else {
            // The placement's linear part.
            let Some(t) = self.form_track(ti) else {
                return;
            };
            let x = frame - f64::from(t.start) + f64::from(t.offset);
            let pl = keyform_anim::scene::placement(t, x);
            let a = pl.anchor;
            let o = pl.apply(a);
            let ex = pl.apply([a[0] + 1.0, a[1]]);
            let ey = pl.apply([a[0], a[1] + 1.0]);
            Some([[ex[0] - o[0], ey[0] - o[0]], [ex[1] - o[1], ey[1] - o[1]]])
        };
        let Some(dk) = jac.and_then(|j| solve2(j, delta)) else {
            return;
        };
        let guid = self.model.art_meshes[mi].info.guid.clone();
        let Some(st) = self.anim.as_mut() else {
            return;
        };
        let s = st.scene;
        let TrackKind::Model(m) = &mut st.doc.animation.scenes[s].tracks[ti].kind else {
            return;
        };
        let map = if local {
            &mut m.form.local
        } else {
            &mut m.form.global
        };
        if let Some(k) = map
            .iter_mut()
            .find(|o| o.guid == guid)
            .and_then(|o| o.keys.iter_mut().find(|k| k.frame == f))
        {
            if let Some(t) = k.target.get_mut(2 * vi..2 * vi + 2) {
                t[0] += dk[0];
                t[1] += dk[1];
            }
        }
        st.dirty = true;
    }

    /// Removes art mesh `mi`'s form animation (both maps) as one undo step
    /// ("Delete Form Edit").
    pub fn delete_form_edit(&mut self, mi: usize) {
        let Some((ti, _, _)) = self.form_frame() else {
            return;
        };
        let guid = self.model.art_meshes[mi].info.guid.clone();
        let Some(st) = self.anim.as_mut() else {
            return;
        };
        let before = st.doc.animation.clone();
        let s = st.scene;
        if let TrackKind::Model(m) = &mut st.doc.animation.scenes[s].tracks[ti].kind {
            m.form.global.retain(|o| o.guid != guid);
            m.form.local.retain(|o| o.guid != guid);
        }
        if st.doc.animation != before {
            st.undo
                .push(("Delete Form Edit".into(), UndoStep::Anim(before)));
            st.redo.clear();
            st.dirty = true;
        }
    }
}

/// The Form Edit tool's canvas input: press on a vertex of a drawn art
/// mesh (the selected one first) and drag it.
pub fn input(ui: &egui::Ui, response: &egui::Response, doc: &mut Document, rect: Rect, ppp: f32) {
    let view = crate::canvas::view_of(doc, rect, ppp);
    let to_canvas = |p: Pos2| view.to_canvas([(p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp]);
    let primary = egui::PointerButton::Primary;
    if response.drag_started_by(primary) {
        let Some(press) = ui.input(|i| i.pointer.press_origin()) else {
            return;
        };
        let q = to_canvas(press);
        let r = PICK / doc.view.zoom.max(1e-6);
        let Some(frame) = doc.frame.as_ref() else {
            return;
        };
        let mut best: Option<(f32, usize, usize)> = None;
        for &mi in frame.render_order.iter().rev() {
            let ms = &frame.meshes[mi];
            if !ms.is_drawn() {
                continue;
            }
            for (vi, p) in ms.positions.chunks_exact(2).enumerate() {
                let d = (p[0] - q[0]).hypot(p[1] - q[1]);
                if d <= r && best.is_none_or(|b| d < b.0) {
                    best = Some((d, mi, vi));
                }
            }
        }
        if let Some((_, mi, vi)) = best {
            if let Some(st) = doc.anim.as_mut() {
                let before = st.doc.animation.clone();
                st.undo.push(("Form Edit".into(), UndoStep::Anim(before)));
                st.redo.clear();
                st.form_drag = Some(FormDrag {
                    mesh: mi,
                    vertex: vi,
                    last: q,
                });
                st.form_mesh = Some(mi);
            }
        }
    } else if response.dragged_by(primary) {
        let Some(p) = ui.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        let q = to_canvas(p);
        let Some((mi, vi, last)) = doc
            .anim
            .as_ref()
            .and_then(|a| a.form_drag.as_ref())
            .map(|d| (d.mesh, d.vertex, d.last))
        else {
            return;
        };
        let delta = [q[0] - last[0], q[1] - last[1]];
        if delta != [0.0, 0.0] {
            doc.move_form_vertex(mi, vi, delta);
            if let Some(d) = doc.anim.as_mut().and_then(|a| a.form_drag.as_mut()) {
                d.last = q;
            }
        }
    } else if response.drag_stopped() {
        if let Some(a) = doc.anim.as_mut() {
            a.form_drag = None;
        }
    }
}

/// Draws the vertices of the art mesh being form-edited.
pub fn draw(
    painter: &egui::Painter,
    doc: &Document,
    to_screen: &dyn Fn([f32; 2]) -> Pos2,
    color: egui::Color32,
) {
    let (Some(mi), Some(frame)) = (
        doc.anim.as_ref().and_then(|a| a.form_mesh),
        doc.frame.as_ref(),
    ) else {
        return;
    };
    let Some(ms) = frame.meshes.get(mi) else {
        return;
    };
    for p in ms.positions.chunks_exact(2) {
        painter.circle_stroke(to_screen([p[0], p[1]]), 3.0, egui::Stroke::new(1.0, color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Log;
    use std::path::Path;

    /// Dragging a vertex in either Adjustment Mode moves it by the drag in
    /// the scene; Mode 2 edits sit under the deformer; saving is refused
    /// until the encoding is known (Q-0033).
    #[test]
    fn form_edit_both_modes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        // An art mesh inside a deformer, drawn.
        let f0 = doc.anim_frame_at(0.0).unwrap();
        let mi = (0..doc.model.art_meshes.len())
            .find(|&i| doc.model.art_meshes[i].target.is_some() && f0.meshes[i].is_drawn())
            .unwrap();
        let at =
            |doc: &Document| doc.anim_frame_at(0.0).unwrap().meshes[mi].positions[0..2].to_vec();
        for local in [false, true] {
            {
                let a = doc.anim.as_mut().unwrap();
                let (ti, _) = a.model_track().unwrap();
                let s = a.scene;
                let TrackKind::Model(m) = &mut a.doc.animation.scenes[s].tracks[ti].kind else {
                    panic!()
                };
                m.form.local_mode = local;
            }
            let p0 = at(&doc);
            doc.move_form_vertex(mi, 0, [10.0, -5.0]);
            let p1 = at(&doc);
            assert!(
                (p1[0] - p0[0] - 10.0).abs() < 0.05 && (p1[1] - p0[1] + 5.0).abs() < 0.05,
                "local {local}: {p0:?} → {p1:?}"
            );
        }
        let a = doc.anim.as_ref().unwrap();
        let (_, m) = a.model_track().unwrap();
        assert_eq!((m.form.global.len(), m.form.local.len()), (1, 1));
        // Saved (§3.1) and read back equal.
        let form = m.form.clone();
        let out = std::env::temp_dir().join(format!("kf-form-{}.can3", std::process::id()));
        let model_copy = out.with_file_name("hiyori_pro_t11.cmo3");
        let _ = std::fs::copy(&model, &model_copy);
        doc.save_animation_to(Some(&out)).unwrap();
        if let Ok(dir) = std::env::var("KEYFORM_BB_DIR") {
            let _ = std::fs::copy(&out, Path::new(&dir).join("form_kf.can3"));
        }
        let back = Document::open_animation(&out, &mut log).unwrap();
        let (_, m2) = back.anim.as_ref().unwrap().model_track().unwrap();
        assert_eq!(m2.form, form);
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&model_copy);
        doc.delete_form_edit(mi);
        let (_, m) = doc.anim.as_ref().unwrap().model_track().unwrap();
        assert!(m.form.global.is_empty() && m.form.local.is_empty());
    }
}
