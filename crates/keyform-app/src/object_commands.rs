//! Modeling ▸ Lock, Show/Hide and Clipping ID commands (also used by the
//! Parts palette menu). spec: 50-ui/menus.md (Modeling), palettes.md §1

use std::sync::Arc;

use crate::document::{Document, Selection, SelectionSet};
use crate::palettes::{info, info_mut};

impl Document {
    /// Show all / Hide all: every part and art mesh.
    pub fn set_all_visible(&mut self, v: bool) -> Result<(), String> {
        self.try_edit(if v { "Show all" } else { "Hide all" }, |m| {
            for p in &mut m.parts {
                if p.info.visible != v {
                    Arc::make_mut(p).info.visible = v;
                }
            }
            for a in &mut m.art_meshes {
                if a.info.visible != v {
                    Arc::make_mut(a).info.visible = v;
                }
            }
            Ok(())
        })
    }

    /// Lock all / Unlock all: every part and art mesh.
    pub fn set_all_locked(&mut self, v: bool) -> Result<(), String> {
        self.try_edit(if v { "Lock all" } else { "Unlock all" }, |m| {
            for p in &mut m.parts {
                if p.info.locked != v {
                    Arc::make_mut(p).info.locked = v;
                }
            }
            for a in &mut m.art_meshes {
                if a.info.locked != v {
                    Arc::make_mut(a).info.locked = v;
                }
            }
            Ok(())
        })
    }

    /// Lock/unlock Selected Objects [Ctrl+2]: locks them, or unlocks them
    /// when all are locked already.
    pub fn toggle_selected_locked(&mut self) -> Result<(), String> {
        let objs = self.selection.objects.clone();
        let all = !objs.is_empty()
            && objs
                .iter()
                .all(|&o| info(&self.model, o).is_some_and(|i| i.locked));
        self.try_edit("Lock/unlock Selected Objects", |m| {
            for &o in &objs {
                if let Some(i) = info_mut(m, o) {
                    i.locked = !all;
                }
            }
            Ok(())
        })
    }

    /// Show/Hide Selected Objects [Ctrl+3]: hides them, or shows them when
    /// all are hidden already.
    pub fn toggle_selected_visible(&mut self) -> Result<(), String> {
        let objs = self.selection.objects.clone();
        let all_hidden = !objs.is_empty()
            && objs
                .iter()
                .all(|&o| info(&self.model, o).is_some_and(|i| !i.visible));
        self.try_edit("Show/Hide Selected Objects", |m| {
            for &o in &objs {
                if let Some(i) = info_mut(m, o) {
                    i.visible = all_hidden;
                }
            }
            Ok(())
        })
    }

    /// The art meshes that aren't selected (nor inside a selected part).
    fn unselected_meshes(&self) -> Vec<usize> {
        let sel = &self.selection.objects;
        let in_selected_part = |mut p: Option<usize>| {
            let mut guard = 0;
            while let Some(q) = p {
                if sel.contains(&Selection::Part(q)) {
                    return true;
                }
                p = self.model.parts.get(q).and_then(|x| x.info.parent_part);
                guard += 1;
                if guard > self.model.parts.len() {
                    break;
                }
            }
            false
        };
        (0..self.model.art_meshes.len())
            .filter(|&a| {
                !sel.contains(&Selection::ArtMesh(a))
                    && !in_selected_part(self.model.art_meshes[a].info.parent_part)
            })
            .collect()
    }

    /// Lock the unselected objects [Ctrl+Shift+2].
    pub fn lock_unselected(&mut self) -> Result<(), String> {
        let meshes = self.unselected_meshes();
        self.try_edit("Lock the unselected objects", |m| {
            for a in meshes {
                if !m.art_meshes[a].info.locked {
                    Arc::make_mut(&mut m.art_meshes[a]).info.locked = true;
                }
            }
            Ok(())
        })
    }

    /// Hide the unselected objects [Ctrl+Shift+3].
    pub fn hide_unselected(&mut self) -> Result<(), String> {
        let meshes = self.unselected_meshes();
        self.try_edit("Hide the unselected objects", |m| {
            for a in meshes {
                if m.art_meshes[a].info.visible {
                    Arc::make_mut(&mut m.art_meshes[a]).info.visible = false;
                }
            }
            Ok(())
        })
    }

    /// Clipping ID ▸ Select Clipping Mask: selects the masks of the
    /// selected art meshes.
    pub fn select_clipping_masks(&mut self) {
        let mut out: Vec<Selection> = Vec::new();
        for &o in &self.selection.objects {
            if let Selection::ArtMesh(a) = o {
                for &c in &self.model.art_meshes[a].clip {
                    if !out.contains(&Selection::ArtMesh(c)) {
                        out.push(Selection::ArtMesh(c));
                    }
                }
            }
        }
        let mut set = SelectionSet::of(None);
        set.objects = out;
        self.set_selection(set);
    }

    /// Clipping ID ▸ Reverse Clipping ID Lookup: selects the art meshes that
    /// use the selected ones as a clipping mask.
    pub fn reverse_clip_lookup(&mut self) {
        let mut out: Vec<Selection> = Vec::new();
        for &o in &self.selection.objects {
            if let Selection::ArtMesh(a) = o {
                for u in keyform_edit::objects::clip_users(&self.model, a) {
                    if !out.contains(&Selection::ArtMesh(u)) {
                        out.push(Selection::ArtMesh(u));
                    }
                }
            }
        }
        let mut set = SelectionSet::of(None);
        set.objects = out;
        self.set_selection(set);
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{Document, Selection};

    fn doc() -> Document {
        crate::document::e2e_regressions::character("object-commands").0
    }

    #[test]
    fn lock_and_hide_selected_and_unselected() {
        let mut d = doc();
        let n = d.model.art_meshes.len();
        d.select(Some(Selection::ArtMesh(0)));
        d.toggle_selected_locked().unwrap();
        assert!(d.model.art_meshes[0].info.locked);
        d.toggle_selected_locked().unwrap();
        assert!(
            !d.model.art_meshes[0].info.locked,
            "a second Ctrl+2 unlocks"
        );
        d.hide_unselected().unwrap();
        assert!(d.model.art_meshes[0].info.visible);
        assert_eq!(
            d.model
                .art_meshes
                .iter()
                .filter(|a| !a.info.visible)
                .count(),
            n - 1
        );
        d.set_all_visible(true).unwrap();
        assert!(d.model.art_meshes.iter().all(|a| a.info.visible));
        d.lock_unselected().unwrap();
        d.set_all_locked(false).unwrap();
        assert!(d.model.art_meshes.iter().all(|a| !a.info.locked));
    }
}
