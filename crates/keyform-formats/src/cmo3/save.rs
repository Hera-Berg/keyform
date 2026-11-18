//! Writing model edits back into the loaded document.
//!
//! Only objects whose `Arc` changed since the last load or save are visited,
//! and only fields whose value changed are rewritten, in place, where the
//! loader found them (an attribute or a child on the `super` chain). Every
//! other byte of `main.xml` stays as it was. A change this module cannot
//! write yet (adding or removing objects, re-parenting, mesh topology…)
//! fails the whole sync with `Unsupported` and leaves the document
//! untouched, so an edit is never silently dropped. Grids whose keys changed
//! are rewritten by [`grid`]; after such structural changes the document is
//! reloaded and must give back exactly the edited model before it is kept.
//! spec: 10-formats/xml-object-graph.md §2, §6

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use keyform_model::*;

mod extended;
mod grid;
mod mesh;
mod motion_sync;
mod objects;
mod params;
mod paths;
mod random_pose;
mod textures;

use grid::Ctx;

use super::{Cmo3Document, ElementPath, ObjectSource};
use crate::error::{invalid, FormatError, Result};
use crate::graph::format_f32;
use crate::xml::Element;

/// Strings at least this long are written as `<s>` children, never as
/// attributes. spec: xml-object-graph.md §2.2
const MAX_ATTR_STRING: usize = 66;

/// Longest `super` chain followed when looking for a field.
const MAX_SUPER_DEPTH: usize = 64;

/// What saved deletions removed from the document, so that an object that
/// comes back (undo after save) can be written as it was.
#[derive(Clone, Debug, Default)]
pub(crate) struct Retired {
    /// Removed shared elements by `xs.id`.
    elements: HashMap<String, Element>,
    /// Deleted objects by GUID: (set list, tag, element id, set-list
    /// position).
    objects: HashMap<String, (String, String, String, usize)>,
    /// Ids are never reused: new ones start above these.
    next_id: u64,
    next_idx: u64,
}

impl Cmo3Document {
    /// True if `model` has changes not yet written to the document.
    pub fn has_unsynced_changes(&self) -> bool {
        !self.model.same_objects(&self.saved)
    }

    /// Writes the changes of `model` (relative to `saved`) into `xml`. On
    /// error nothing is changed.
    pub fn sync(&mut self) -> Result<()> {
        if self.model.same_objects(&self.saved) {
            return Ok(());
        }
        check_model_level(&self.saved, &self.model)?;
        self.revive()?;
        let mut root = self.xml.root.clone();
        let s = &self.sources;
        let (old, new) = (&self.saved, &self.model);
        let mut ctx = Ctx::new(&root, old, new, s);
        ctx.next_id = ctx.next_id.max(self.retired.next_id);
        ctx.next_idx = ctx.next_idx.max(self.retired.next_idx);
        let ctx = &mut ctx;
        params::sync_parameters(&mut root, ctx, s, old, new)?;
        random_pose::sync_random_pose(&mut root, ctx, old, new)?;
        // New layered images, model images and atlas pages (art meshes
        // created below refer to them).
        textures::sync_textures(&mut root, ctx, &self.archive, old, new)?;
        params::sync_physics(&mut root, ctx, old, new)?;
        motion_sync::sync_motion_sync(&mut root, ctx, old, new)?;
        // Parts and deformers are matched by GUID: new ones first (others
        // may refer to them), then edits, then deletions.
        let old_parts = objects::index_by_guid(&old.parts, |p| &p.info.guid);
        let old_defs = objects::index_by_guid(&old.deformers, |d| &d.info.guid);
        for p in &new.parts {
            if !old_parts.contains_key(&p.info.guid) {
                objects::create_part(&mut root, ctx, new, p)?;
                ctx.structural = true;
            }
        }
        for d in &new.deformers {
            if !old_defs.contains_key(&d.info.guid) {
                objects::create_deformer(&mut root, ctx, new, d)?;
                ctx.structural = true;
            }
        }
        for n in &new.parts {
            if let Some(&i) = old_parts.get(&n.info.guid) {
                let o = &old.parts[i];
                if !Arc::ptr_eq(o, n) && **o != **n {
                    write_part(&mut root, ctx, source(&s.parts, i)?, o, n)?;
                }
            }
        }
        for n in &new.deformers {
            if let Some(&i) = old_defs.get(&n.info.guid) {
                let o = &old.deformers[i];
                if !Arc::ptr_eq(o, n) && **o != **n {
                    write_deformer(&mut root, ctx, source(&s.deformers, i)?, o, n)?;
                }
            }
        }
        let new_parts = objects::index_by_guid(&new.parts, |p| &p.info.guid);
        let new_defs = objects::index_by_guid(&new.deformers, |d| &d.info.guid);
        for (i, o) in old.parts.iter().enumerate() {
            if !new_parts.contains_key(&o.info.guid) {
                let path = source(&s.parts, i)?.element.clone();
                objects::delete_object(
                    &mut root,
                    ctx,
                    &o.info.guid,
                    path.as_deref().unwrap_or_default(),
                    "partSourceSet",
                )?;
            }
        }
        for (i, o) in old.deformers.iter().enumerate() {
            if !new_defs.contains_key(&o.info.guid) {
                let path = source(&s.deformers, i)?.element.clone();
                objects::delete_object(
                    &mut root,
                    ctx,
                    &o.info.guid,
                    path.as_deref().unwrap_or_default(),
                    "deformerSourceSet",
                )?;
            }
        }
        // Art meshes and glues are matched by GUID too.
        let old_meshes = objects::index_by_guid(&old.art_meshes, |a| &a.info.guid);
        let new_meshes = objects::index_by_guid(&new.art_meshes, |a| &a.info.guid);
        for n in &new.art_meshes {
            let Some(&i) = old_meshes.get(&n.info.guid) else {
                let path = objects::create_art_mesh(&mut root, ctx, &self.archive, new, n)?;
                if !n.paths.is_empty() {
                    ctx.pending_paths.push((path, n.info.guid.clone()));
                }
                ctx.structural = true;
                continue;
            };
            let o = &old.art_meshes[i];
            if !Arc::ptr_eq(o, n) && **o != **n {
                write_art_mesh(&mut root, ctx, source(&s.art_meshes, i)?, o, n)?;
            }
        }
        for (i, o) in old.art_meshes.iter().enumerate() {
            if !new_meshes.contains_key(&o.info.guid) {
                let path = source(&s.art_meshes, i)?.element.clone();
                objects::delete_object(
                    &mut root,
                    ctx,
                    &o.info.guid,
                    path.as_deref().unwrap_or_default(),
                    "drawableSourceSet",
                )?;
            }
        }
        let old_glues = objects::index_by_guid(&old.glues, |g| &g.info.guid);
        let new_glues = objects::index_by_guid(&new.glues, |g| &g.info.guid);
        for n in &new.glues {
            match old_glues.get(&n.info.guid) {
                Some(&i) => {
                    let o = &old.glues[i];
                    if !Arc::ptr_eq(o, n) && **o != **n {
                        write_glue(&mut root, ctx, source(&s.glues, i)?, o, n)?;
                    }
                }
                None => {
                    objects::create_glue(&mut root, ctx, new, n)?;
                    ctx.structural = true;
                }
            }
        }
        for (i, o) in old.glues.iter().enumerate() {
            if !new_glues.contains_key(&o.info.guid) {
                let path = source(&s.glues, i)?.element.clone();
                objects::delete_object(
                    &mut root,
                    ctx,
                    &o.info.guid,
                    path.as_deref().unwrap_or_default(),
                    "affecterSourceSet",
                )?;
            }
        }
        for (path, guid) in std::mem::take(&mut ctx.pending_paths) {
            let empty = Vec::new();
            let o = old
                .art_meshes
                .iter()
                .find(|a| a.info.guid == guid)
                .map_or(&empty, |a| &a.paths);
            if let Some(n) = new.art_meshes.iter().find(|a| a.info.guid == guid) {
                paths::write_paths(&mut root, ctx, &path, o, n)?;
            }
        }
        paths::relink_targets(&mut root, ctx)?;
        if !ctx.structural {
            self.xml.root = root;
            upgrade_alpha_composition(&mut self.xml);
            self.saved = self.model.clone();
            return Ok(());
        }
        let removed = grid::remove_orphans(&mut root, &ctx.orphans)?;
        let deleted = std::mem::take(&mut ctx.deleted);
        let (next_id, next_idx) = (ctx.next_id, ctx.next_idx);
        let mut xml = self.xml.clone();
        xml.root = root;
        for name in grid::GRID_IMPORTS {
            ensure_import(&mut xml, name);
        }
        let new_images = !ctx.tex.imports.is_empty();
        for name in std::mem::take(&mut ctx.tex.imports) {
            ensure_import(&mut xml, name);
        }
        // A `null` placeholder must stay empty (a filled-in field keeps its
        // type's tag).
        {
            let mut stack = vec![&xml.root];
            while let Some(e) = stack.pop() {
                if e.name == "null"
                    && (!e.children.is_empty() || e.attrs.iter().any(|(k, _)| k != "xs.n"))
                {
                    return invalid(format!(
                        "a null field {:?} carries a value",
                        e.field_name().unwrap_or_default()
                    ));
                }
                stack.extend(e.children.iter());
            }
        }
        // Every element type the document now uses has its import.
        for name in super::schema_imports::imports_used(&xml.root) {
            ensure_import(&mut xml, name);
        }
        // Version markers of custom-encoded types that are new to the
        // document (a type it already had keeps its marker, or none).
        let had: std::collections::HashSet<&str> = {
            let mut set = std::collections::HashSet::new();
            let mut stack = vec![&self.xml.root];
            while let Some(e) = stack.pop() {
                set.insert(e.name.as_str());
                stack.extend(e.children.iter());
            }
            set
        };
        for v in super::schema_imports::versions_used(&xml.root) {
            let ty = v.split(':').next().unwrap_or(v);
            let marked = xml
                .pis
                .iter()
                .any(|p| p.target == "version" && p.data.split(':').next() == Some(ty));
            let short = ty.rsplit('.').next().unwrap_or(ty);
            if !marked && !had.contains(short) {
                ensure_pi(&mut xml, "version", v);
            }
        }
        upgrade_alpha_composition(&mut xml);
        let _ = new_images;
        let mut archive = self.archive.clone();
        textures::add_entries(&mut archive, &mut ctx.tex, &xml.root);
        if new
            .glues
            .iter()
            .any(|g| !old_glues.contains_key(&g.info.guid))
        {
            for name in objects::GLUE_IMPORTS {
                ensure_import(&mut xml, name);
            }
        }
        // Debugging aid: keep the candidate document.
        if let Some(p) = std::env::var_os("KEYFORM_DEBUG_SAVE") {
            let _ = std::fs::write(p, xml.write());
        }
        // The rewritten document must load back as exactly the edited model.
        let mut warnings = Vec::new();
        let (loaded, sources) = super::load_model(&archive, &xml, &mut warnings)?;
        // Physics enable flags are editor preview state, not stored.
        let mut want = self.model.clone();
        want.physics = want.physics.as_stored();
        if !loaded.same_data(&want) {
            return invalid(format!(
                "the rewritten document does not reload as the edited model ({})",
                first_difference(&loaded, &want)
            ));
        }
        self.xml = xml;
        self.archive = archive;
        self.sources = sources;
        self.saved = self.model.clone();
        let r = &mut self.retired;
        for e in removed {
            if let Some(id) = e.attr("xs.id") {
                r.elements.insert(id.to_owned(), e.clone());
            }
        }
        for (guid, set, tag, id, pos) in deleted {
            r.objects.insert(guid, (set, tag, id, pos));
        }
        r.next_id = r.next_id.max(next_id);
        r.next_idx = r.next_idx.max(next_idx);
        Ok(())
    }

    /// Puts back the elements of retired objects that are in the model
    /// again, at their old set-list positions, and reloads, so that the
    /// sync can treat them as existing objects.
    fn revive(&mut self) -> Result<()> {
        let old = &self.saved;
        let known = |g: &str| {
            old.parts.iter().any(|x| x.info.guid == g)
                || old.deformers.iter().any(|x| x.info.guid == g)
                || old.art_meshes.iter().any(|x| x.info.guid == g)
                || old.glues.iter().any(|x| x.info.guid == g)
        };
        let m = &self.model;
        let mut back: Vec<(String, (String, String, String, usize))> = m
            .parts
            .iter()
            .map(|x| &x.info.guid)
            .chain(m.deformers.iter().map(|x| &x.info.guid))
            .chain(m.art_meshes.iter().map(|x| &x.info.guid))
            .chain(m.glues.iter().map(|x| &x.info.guid))
            .filter(|g| !known(g))
            .filter_map(|g| Some((g.clone(), self.retired.objects.get(g)?.clone())))
            .collect();
        if back.is_empty() {
            return Ok(());
        }
        back.sort_by_key(|(_, r)| r.3);
        let mut root = self.xml.root.clone();
        for (_, (set, tag, id, pos)) in &back {
            let mut stack = vec![id.clone()];
            while let Some(x) = stack.pop() {
                if grid::get_shared(&root, &x).is_some() {
                    continue;
                }
                let Some(e) = self.retired.elements.get(&x) else {
                    return invalid(format!("revived object needs {x}, which is gone"));
                };
                objects::refs_in(e, &mut stack);
                grid::shared_mut(&mut root)?.children.push(e.clone());
            }
            objects::register_at(&mut root, set, tag, id, *pos)?;
        }
        let mut xml = self.xml.clone();
        xml.root = root;
        let mut warnings = Vec::new();
        let (loaded, sources) = super::load_model(&self.archive, &xml, &mut warnings)?;
        self.xml = xml;
        self.saved = loaded;
        self.sources = sources;
        for (g, _) in back {
            self.retired.objects.remove(&g);
        }
        Ok(())
    }

    /// Syncs and serializes the project.
    pub fn save_bytes(&mut self) -> Result<Vec<u8>> {
        self.sync()?;
        self.write()
    }

    /// Syncs and writes the project to `path` atomically: the bytes go to a
    /// temporary file in the same directory, which then replaces `path`.
    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        let bytes = self.save_bytes()?;
        write_atomic(path, &bytes)?;
        Ok(())
    }
}

/// Names the first object that differs between two models (diagnostics).
fn first_difference(a: &Model, b: &Model) -> String {
    fn diff<T: PartialEq + std::fmt::Debug>(
        kind: &str,
        x: &[Arc<T>],
        y: &[Arc<T>],
        id: impl Fn(&T) -> String,
    ) -> Option<String> {
        if x.len() != y.len() {
            return Some(format!("{kind} count"));
        }
        let (p, q) = x.iter().zip(y).find(|(p, q)| p != q)?;
        // Where the debug texts part, with a little context.
        let (dp, dq) = (format!("{p:?}"), format!("{q:?}"));
        let at = dp
            .char_indices()
            .zip(dq.chars())
            .find(|((_, a), b)| a != b)
            .map_or(dp.len().min(dq.len()), |((i, _), _)| i);
        let lo = dp[..at].char_indices().rev().nth(60).map_or(0, |(i, _)| i);
        let snippet = |t: &str| t[lo..].chars().take(140).collect::<String>();
        Some(format!(
            "{kind} {}: reloaded …{}… vs edited …{}…",
            id(p),
            snippet(&dp),
            snippet(&dq)
        ))
    }
    diff("parameter", &a.parameters, &b.parameters, |p| p.id.clone())
        .or_else(|| diff("part", &a.parts, &b.parts, |p| p.info.id.clone()))
        .or_else(|| {
            diff("deformer", &a.deformers, &b.deformers, |p| {
                p.info.id.clone()
            })
        })
        .or_else(|| {
            diff("art mesh", &a.art_meshes, &b.art_meshes, |p| {
                p.info.id.clone()
            })
        })
        .or_else(|| diff("glue", &a.glues, &b.glues, |p| p.info.id.clone()))
        .or_else(|| diff("texture", &a.textures, &b.textures, |p| p.name.clone()))
        .or_else(|| {
            diff("model image", &a.model_images, &b.model_images, |p| {
                p.name.clone()
            })
        })
        .or_else(|| {
            diff(
                "model image group",
                &a.model_image_groups,
                &b.model_image_groups,
                |p| p.name.clone(),
            )
        })
        .or_else(|| {
            diff("layered image", &a.layered_images, &b.layered_images, |p| {
                p.name.clone()
            })
        })
        .unwrap_or_else(|| "model-level data".into())
}

/// Adds `<?import name?>` unless present, keeping the imports sorted.
/// Documents with `CArtMeshSource` below version 5 have no
/// `alphaComposition` field, so the reference ignores a written one. When
/// an art mesh gets one there, the type is upgraded to its current version
/// 5: the marker is raised and every art mesh source gets the field (OVER
/// where it had none). spec: 10-formats/xml-object-graph.md §5,
/// 10-formats/cmo3-schema.md (CArtMeshSource)
fn upgrade_alpha_composition(xml: &mut crate::xml::XmlDocument) {
    let Some(k) = xml.pis.iter().position(|p| {
        p.target == "version"
            && p.data
                .split(':')
                .next()
                .is_some_and(|t| t.ends_with("CArtMeshSource"))
    }) else {
        return;
    };
    let (name, v) = match xml.pis[k].data.split_once(':') {
        Some((n, v)) => (n.to_owned(), v.trim().parse::<u32>().unwrap_or(0)),
        None => return,
    };
    if v >= 5 {
        return;
    }
    fn walk(e: &Element, f: &mut dyn FnMut(&Element) -> bool) -> bool {
        f(e) || e.children.iter().any(|c| walk(c, f))
    }
    let uses = walk(&xml.root, &mut |e| {
        e.name == "CArtMeshSource"
            && e.children
                .iter()
                .any(|c| c.field_name() == Some("alphaComposition"))
    });
    if !uses {
        return;
    }
    fn fill(e: &mut Element) {
        if e.name == "CArtMeshSource"
            && e.attr("xs.ref").is_none()
            && !e.children.is_empty()
            && !e
                .children
                .iter()
                .any(|c| c.field_name() == Some("alphaComposition"))
        {
            e.children.push(
                Element::new("AlphaComposition")
                    .with_attr("xs.n", "alphaComposition")
                    .with_attr("v", "OVER"),
            );
        }
        for c in &mut e.children {
            fill(c);
        }
    }
    fill(&mut xml.root);
    xml.pis[k].data = format!("{name}:5");
    if let Some(imp) = super::schema_imports::import_of("AlphaComposition") {
        ensure_import(xml, imp);
    }
}

/// Adds a `<?target data?>` processing instruction if missing (after the
/// last one of the same target).
fn ensure_pi(xml: &mut crate::xml::XmlDocument, target: &str, data: &str) {
    use crate::xml::Pi;
    if xml.pis.iter().any(|p| p.target == target && p.data == data) {
        return;
    }
    let at = xml
        .pis
        .iter()
        .rposition(|p| p.target == target)
        .map_or(xml.pis.len(), |i| i + 1);
    xml.pis.insert(
        at,
        Pi {
            target: target.into(),
            data: data.into(),
        },
    );
}

fn ensure_import(xml: &mut crate::xml::XmlDocument, name: &str) {
    use crate::xml::Pi;
    if xml
        .pis
        .iter()
        .any(|p| p.target == "import" && p.data == name)
    {
        return;
    }
    let at = xml
        .pis
        .iter()
        .position(|p| p.target == "import" && p.data.as_str() > name)
        .or_else(|| {
            xml.pis
                .iter()
                .rposition(|p| p.target == "import")
                .map(|i| i + 1)
        })
        .unwrap_or(xml.pis.len());
    xml.pis.insert(
        at,
        Pi {
            target: "import".into(),
            data: name.into(),
        },
    );
}

/// Writes `bytes` to `path` through a temporary sibling file and a rename,
/// so a failed save never leaves a truncated project behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let tmp = path.with_file_name(format!(".{name}.keyform-save"));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn unsupported<T>(what: impl Into<String>) -> Result<T> {
    Err(FormatError::Unsupported(format!(
        "saving this change is not supported yet: {}",
        what.into()
    )))
}

/// Model-wide data other than the object lists must be unchanged.
fn check_model_level(old: &Model, new: &Model) -> Result<()> {
    // Groups: only their child lists may change (rewritten with parameters).
    let same_groups = old.parameter_groups.len() == new.parameter_groups.len()
        && old
            .parameter_groups
            .iter()
            .zip(&new.parameter_groups)
            .all(|(a, b)| {
                Arc::ptr_eq(a, b)
                    || ParameterGroup {
                        children: Vec::new(),
                        ..(**a).clone()
                    } == ParameterGroup {
                        children: Vec::new(),
                        ..(**b).clone()
                    }
            });
    let checks = [
        (same_groups, "parameter groups"),
        (
            old.root_part.map(|p| &old.parts[p].info.guid)
                == new.root_part.map(|p| &new.parts[p].info.guid),
            "root part",
        ),
        (old.export == new.export, "export settings"),
        (old.canvas == new.canvas, "canvas"),
        (old.name == new.name, "model name"),
        (old.guid == new.guid, "model GUID"),
        (old.target_version == new.target_version, "SDK target"),
    ];
    match checks.iter().find(|(ok, _)| !ok) {
        Some((_, what)) => unsupported(*what),
        None => Ok(()),
    }
}

fn source(list: &[ObjectSource], i: usize) -> Result<&ObjectSource> {
    list.get(i)
        .ok_or_else(|| FormatError::Invalid(format!("no source record for object {i}")))
}

/// The element at `path` (child indices from the document root).
fn element_at<'e>(root: &'e mut Element, path: Option<&[u32]>) -> Result<&'e mut Element> {
    let Some(path) = path else {
        return invalid("edited object has no element in the document");
    };
    let mut cur = root;
    for &i in path {
        cur = match cur.children.get_mut(i as usize) {
            Some(c) => c,
            None => return invalid("stale element path"),
        };
    }
    Ok(cur)
}

// ---- field writers ----------------------------------------------------------

/// Depth on the `super` chain of the element holding field `name`.
fn holder_depth(e: &Element, name: &str) -> Option<usize> {
    let mut cur = e;
    for depth in 0..MAX_SUPER_DEPTH {
        if cur.attr(name).is_some() || cur.child_field(name).is_some() {
            return Some(depth);
        }
        cur = cur.child_field("super")?;
    }
    None
}

fn level_mut(e: &mut Element, depth: usize) -> &mut Element {
    let mut cur = e;
    for _ in 0..depth {
        // `holder_depth` walked this chain, so every level exists.
        match cur
            .children
            .iter()
            .position(|c| c.field_name() == Some("super"))
        {
            Some(i) => cur = &mut cur.children[i],
            None => break,
        }
    }
    cur
}

fn child_index(e: &Element, name: &str) -> Option<usize> {
    e.children.iter().position(|c| c.field_name() == Some(name))
}

fn new_child(tag: &str, name: &str) -> Element {
    Element::new(tag).with_attr("xs.n", name)
}

/// Sets scalar field `name` to `text`. An existing attribute or child keeps
/// its form; a missing field is appended as a `<tag>` child.
fn set_scalar(e: &mut Element, name: &str, tag: &str, text: String) {
    match holder_depth(e, name) {
        Some(d) => {
            let h = level_mut(e, d);
            if h.attr(name).is_some() {
                if tag == "s" && !attr_string_ok(&text) {
                    h.attrs.retain(|(k, _)| k != name);
                    let mut c = new_child("s", name);
                    c.text = text;
                    h.children.push(c);
                } else {
                    h.set_attr(name, text);
                }
            } else if let Some(i) = child_index(h, name) {
                let c = &mut h.children[i];
                if c.name == "null" {
                    c.name = tag.to_owned();
                }
                c.attrs.retain(|(k, _)| k != "xs.ref");
                c.children.clear();
                c.text = text;
            }
        }
        None => {
            let mut c = new_child(tag, name);
            c.text = text;
            e.children.push(c);
        }
    }
}

/// spec: xml-object-graph.md §2.2 (short and XML-legal).
fn attr_string_ok(s: &str) -> bool {
    s.chars().count() < MAX_ATTR_STRING && s.chars().all(xml_legal)
}

fn xml_legal(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}')
        || c >= '\u{10000}'
}

/// Characters XML 1.0 cannot hold are dropped, so the document stays
/// loadable. spec: xml-object-graph.md §6.1
fn xml_text(s: &str) -> String {
    s.chars().filter(|&c| xml_legal(c)).collect()
}

/// Sets attribute `attr` of the object stored under field `name` (enums,
/// IDs, colours). A missing field is appended as `<tag attr=…/>`.
fn set_field_attrs(e: &mut Element, name: &str, tag: &str, attrs: &[(&str, String)]) {
    let field = match holder_depth(e, name) {
        Some(d) => {
            let h = level_mut(e, d);
            match child_index(h, name) {
                Some(i) => &mut h.children[i],
                None => {
                    h.attrs.retain(|(k, _)| k != name);
                    h.children.push(new_child(tag, name));
                    h.children.last_mut().expect("just pushed")
                }
            }
        }
        None => {
            e.children.push(new_child(tag, name));
            e.children.last_mut().expect("just pushed")
        }
    };
    if field.name == "null" {
        field.name = tag.to_owned();
    }
    // A shared value changed for this object only: write it inline.
    field.attrs.retain(|(k, _)| k != "xs.ref");
    for (k, v) in attrs {
        field.set_attr(k, v.clone());
    }
}

fn set_array(e: &mut Element, name: &str, tag: &str, values: &[f32]) {
    let text = values
        .iter()
        .map(|v| format_f32(*v))
        .collect::<Vec<_>>()
        .join(" ");
    set_array_text(e, name, tag, values.len(), text);
}

/// Sets array field `name` to `count` space-separated values.
fn set_array_text(e: &mut Element, name: &str, tag: &str, count: usize, text: String) {
    let d = holder_depth(e, name);
    let h = match d {
        Some(d) => level_mut(e, d),
        None => e,
    };
    let i = match child_index(h, name) {
        Some(i) => i,
        None => {
            h.children.push(new_child(tag, name));
            h.children.len() - 1
        }
    };
    let c = &mut h.children[i];
    c.name = tag.to_owned();
    // Arrays can be shared (`xs.ref`); the new values belong to this object.
    c.attrs.retain(|(k, _)| k != "xs.ref");
    c.set_attr("count", count.to_string());
    c.text = text;
}

/// Writes fields whose old and new values differ.
struct Fields<'e> {
    e: &'e mut Element,
}

impl Fields<'_> {
    fn f32(&mut self, name: &str, old: f32, new: f32) {
        if old.to_bits() != new.to_bits() {
            set_scalar(self.e, name, "f", format_f32(new));
        }
    }

    fn i32(&mut self, name: &str, old: i32, new: i32) {
        if old != new {
            set_scalar(self.e, name, "i", new.to_string());
        }
    }

    fn bool(&mut self, name: &str, old: bool, new: bool) {
        if old != new {
            set_scalar(self.e, name, "b", new.to_string());
        }
    }

    fn string(&mut self, name: &str, old: &str, new: &str) {
        if old != new {
            set_scalar(self.e, name, "s", xml_text(new));
        }
    }

    fn enumv(&mut self, name: &str, tag: &str, old: &str, new: &str) {
        if old != new {
            set_field_attrs(self.e, name, tag, &[("v", new.to_owned())]);
        }
    }

    fn id(&mut self, name: &str, tag: &str, old: &str, new: &str) {
        if old != new {
            set_field_attrs(self.e, name, tag, &[("idstr", xml_text(new))]);
        }
    }

    fn color(&mut self, name: &str, old: &Color, new: &Color) {
        if old.iter().zip(new).any(|(a, b)| a.to_bits() != b.to_bits()) {
            let attrs = ["red", "green", "blue", "alpha"]
                .iter()
                .zip(new)
                .map(|(k, v)| (*k, format_f32(*v)))
                .collect::<Vec<_>>();
            set_field_attrs(self.e, name, "CFloatColor", &attrs);
        }
    }

    fn positions(&mut self, name: &str, old: &[f32], new: &[f32]) {
        if old.len() != new.len() || old.iter().zip(new).any(|(a, b)| a.to_bits() != b.to_bits()) {
            set_array(self.e, name, "float-array", new);
        }
    }

    /// Keyform draw orders are stored as integers.
    fn draw_order(&mut self, old: f32, new: f32) {
        self.i32("drawOrder", old.round() as i32, new.round() as i32);
    }
}

// ---- objects ----------------------------------------------------------------

fn write_parameter(e: &mut Element, o: &Parameter, n: &Parameter) -> Result<()> {
    let rest_same = Parameter {
        id: o.id.clone(),
        name: o.name.clone(),
        description: o.description.clone(),
        min: o.min,
        max: o.max,
        default: o.default,
        repeat: o.repeat,
        kind: o.kind,
        decimal_places: o.decimal_places,
        snap_epsilon: o.snap_epsilon,
        combined: o.combined,
        ..n.clone()
    } == *o;
    if !rest_same {
        return unsupported(format!("parameter {}: grouping", o.id));
    }
    let mut f = Fields { e };
    f.id("id", "CParameterId", &o.id, &n.id);
    f.string("name", &o.name, &n.name);
    f.string("description", &o.description, &n.description);
    f.f32("minValue", o.min, n.min);
    f.f32("maxValue", o.max, n.max);
    f.f32("defaultValue", o.default, n.default);
    f.bool("isRepeat", o.repeat, n.repeat);
    f.i32("decimalPlaces", o.decimal_places, n.decimal_places);
    f.f32("snapEpsilon", o.snap_epsilon, n.snap_epsilon);
    f.bool("combined", o.combined, n.combined);
    let kind = |k: ParameterKind| match k {
        ParameterKind::Normal => "NORMAL",
        ParameterKind::BlendShape => "MORPH_TARGET",
    };
    f.enumv("paramType", "Type", kind(o.kind), kind(n.kind));
    Ok(())
}

fn part_guid(m: &Model, p: Option<usize>) -> Option<&str> {
    p.and_then(|p| m.parts.get(p)).map(|x| x.info.guid.as_str())
}

fn deformer_guid(m: &Model, d: Option<usize>) -> Option<&str> {
    d.and_then(|d| m.deformers.get(d))
        .map(|x| x.info.guid.as_str())
}

/// A part's children as (kind, GUID): comparable across renumbering.
fn child_guids<'m>(m: &'m Model, part: &Part) -> Vec<(u8, Option<&'m str>)> {
    part.children
        .iter()
        .map(|c| match *c {
            PartChild::Part(i) => (0, m.parts.get(i).map(|x| x.info.guid.as_str())),
            PartChild::Deformer(i) => (1, m.deformers.get(i).map(|x| x.info.guid.as_str())),
            PartChild::ArtMesh(i) => (2, m.art_meshes.get(i).map(|x| x.info.guid.as_str())),
            PartChild::Glue(i) => (3, m.glues.get(i).map(|x| x.info.guid.as_str())),
            PartChild::Other => (4, None),
        })
        .collect()
}

/// Parent part and parent deformer changes (compared by GUID).
fn write_parents(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    (op, np): (Option<usize>, Option<usize>),
    target: Option<(Option<usize>, Option<usize>)>,
) -> Result<()> {
    let (old, new) = (ctx.old_model, ctx.new_model);
    let Some(path) = src.element.clone() else {
        return invalid("object has no element");
    };
    if part_guid(old, op) != part_guid(new, np) {
        objects::write_parent_part(root, ctx, &path, new, np)?;
        ctx.structural = true;
    }
    if let Some((ot, nt)) = target {
        if deformer_guid(old, ot) != deformer_guid(new, nt) {
            objects::write_target(root, ctx, &path, new, nt)?;
            ctx.structural = true;
        }
    }
    Ok(())
}

fn write_info(f: &mut Fields, id_tag: &str, o: &ObjectInfo, n: &ObjectInfo) -> Result<()> {
    if o.guid != n.guid {
        return unsupported(format!("{}: object identity", o.id));
    }
    f.id("id", id_tag, &o.id, &n.id);
    f.string("localName", &o.name, &n.name);
    f.bool("isVisible", o.visible, n.visible);
    f.bool("isLocked", o.locked, n.locked);
    Ok(())
}

/// A keyform type whose fields can be written in place.
trait FormFields: Clone + PartialEq {
    fn write(f: &mut Fields, o: &Self, n: &Self) -> Result<()>;
}

impl FormFields for PartForm {
    fn write(f: &mut Fields, o: &Self, n: &Self) -> Result<()> {
        f.draw_order(o.draw_order, n.draw_order);
        f.f32("opacity", o.opacity, n.opacity);
        f.color("multiplyColor", &o.multiply, &n.multiply);
        f.color("screenColor", &o.screen, &n.screen);
        Ok(())
    }
}

impl FormFields for ArtMeshForm {
    fn write(f: &mut Fields, o: &Self, n: &Self) -> Result<()> {
        // A changed vertex count comes with a topology change, which
        // `write_art_mesh` checks.
        f.positions("positions", &o.positions, &n.positions);
        f.draw_order(o.draw_order, n.draw_order);
        f.f32("opacity", o.opacity, n.opacity);
        f.color("multiplyColor", &o.multiply, &n.multiply);
        f.color("screenColor", &o.screen, &n.screen);
        Ok(())
    }
}

impl FormFields for WarpForm {
    fn write(f: &mut Fields, o: &Self, n: &Self) -> Result<()> {
        if o.positions.len() != n.positions.len() {
            return unsupported("warp grid size");
        }
        f.positions("positions", &o.positions, &n.positions);
        f.f32("opacity", o.opacity, n.opacity);
        f.color("multiplyColor", &o.multiply, &n.multiply);
        f.color("screenColor", &o.screen, &n.screen);
        Ok(())
    }
}

impl FormFields for RotationForm {
    fn write(f: &mut Fields, o: &Self, n: &Self) -> Result<()> {
        f.f32("angle", o.angle, n.angle);
        f.f32("originX", o.origin[0], n.origin[0]);
        f.f32("originY", o.origin[1], n.origin[1]);
        f.f32("scale", o.scale, n.scale);
        f.bool("isReflectX", o.reflect_x, n.reflect_x);
        f.bool("isReflectY", o.reflect_y, n.reflect_y);
        f.f32("opacity", o.opacity, n.opacity);
        f.color("multiplyColor", &o.multiply, &n.multiply);
        f.color("screenColor", &o.screen, &n.screen);
        Ok(())
    }
}

impl FormFields for GlueForm {
    fn write(f: &mut Fields, o: &Self, n: &Self) -> Result<()> {
        f.f32("intensity", o.intensity, n.intensity);
        Ok(())
    }
}

/// Writes changed keyform contents. The grid's bindings and the blend
/// shapes' parameters and keys must be unchanged.
fn write_forms<F: FormFields + keyform_eval::Blend + keyform_eval::extended::Points>(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    what: &str,
    (og, ob): (&KeyformGrid<F>, &BlendShapes<F>),
    (ng, nb): (&KeyformGrid<F>, &BlendShapes<F>),
) -> Result<()> {
    write_grid_forms(root, ctx, src, what, (og, ob), (ng, nb))?;
    // Extended interpolation follows the grid and the blend-shape settings
    // (extended-interpolation.md §4).
    extended::sync_extension(root, ctx, src, what, (og, ob), (ng, nb))
}

fn write_grid_forms<F: FormFields>(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    what: &str,
    (og, ob): (&KeyformGrid<F>, &BlendShapes<F>),
    (ng, nb): (&KeyformGrid<F>, &BlendShapes<F>),
) -> Result<()> {
    if !grid::same_bindings(ctx, &og.bindings, &ng.bindings)
        || og.guids != ng.guids
        || og.keyforms.len() != ng.keyforms.len()
    {
        // Keys changed: the grid is rewritten (blend shapes must stay as they are).
        if !grid::same_blend(ctx, ob, nb, true) {
            return unsupported(format!("{what}: blend shapes together with keys"));
        }
        return grid::rebuild_grid(root, ctx, src, what, og, ng);
    }
    if !grid::same_blend(ctx, ob, nb, false) {
        return unsupported(format!("{what}: blend shapes"));
    }
    let forms = og
        .keyforms
        .iter()
        .zip(&ng.keyforms)
        .zip(src.cells.iter().chain(std::iter::repeat(&None)))
        .chain(
            ob.shapes
                .iter()
                .map(|s| &s.form)
                .zip(nb.shapes.iter().map(|s| &s.form))
                .zip(src.blend.iter().chain(std::iter::repeat(&None))),
        );
    // One stored keyform may serve several cells; it can only take one value.
    let mut written: Vec<(&ElementPath, &F)> = Vec::new();
    for ((o, n), path) in forms {
        if o == n {
            continue;
        }
        let Some(path) = path else {
            return unsupported(format!("{what}: keyform of an empty grid cell"));
        };
        if let Some((_, prev)) = written.iter().find(|(p, _)| *p == path) {
            if *prev != n {
                return unsupported(format!("{what}: shared keyform edited differently"));
            }
            continue;
        }
        let e = element_at(root, Some(path))?;
        F::write(&mut Fields { e }, o, n)?;
        written.push((path, n));
    }
    Ok(())
}

fn write_part(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    o: &Part,
    n: &Part,
) -> Result<()> {
    let rest_same = Part {
        info: o.info.clone(),
        children: o.children.clone(),
        draw_order_group: o.draw_order_group,
        sketch: o.sketch,
        offscreen: o.offscreen,
        invert_clip: o.invert_clip,
        color_composition: o.color_composition,
        alpha_composition: o.alpha_composition,
        default_order: o.default_order,
        grid: o.grid.clone(),
        blend_shapes: o.blend_shapes.clone(),
        ..n.clone()
    } == *o;
    if !rest_same {
        return unsupported(format!("part {}: clipping", o.info.id));
    }
    write_parents(
        root,
        ctx,
        src,
        (o.info.parent_part, n.info.parent_part),
        None,
    )?;
    let (old, new) = (ctx.old_model, ctx.new_model);
    if child_guids(old, o) != child_guids(new, n) {
        let Some(path) = src.element.clone() else {
            return invalid("part has no element");
        };
        objects::write_children(root, ctx, &path, new, n)?;
        ctx.structural = true;
    }
    write_forms(
        root,
        ctx,
        src,
        &o.info.id,
        (&o.grid, &o.blend_shapes),
        (&n.grid, &n.blend_shapes),
    )?;
    let mut f = Fields {
        e: element_at(root, src.element.as_deref())?,
    };
    write_info(&mut f, "CPartId", &o.info, &n.info)?;
    f.bool(
        "enableDrawOrderGroup",
        o.draw_order_group,
        n.draw_order_group,
    );
    f.bool("isSketch", o.sketch, n.sketch);
    f.bool("useOffscreen", o.offscreen, n.offscreen);
    f.bool("invertClippingMask", o.invert_clip, n.invert_clip);
    f.i32("defaultOrder_forEditor", o.default_order, n.default_order);
    write_compositions(
        &mut f,
        (o.color_composition, o.alpha_composition),
        (n.color_composition, n.alpha_composition),
    );
    Ok(())
}

fn write_compositions(
    f: &mut Fields,
    o: (ColorComposition, AlphaComposition),
    n: (ColorComposition, AlphaComposition),
) {
    f.enumv(
        "colorComposition",
        "ColorComposition",
        o.0.name(),
        n.0.name(),
    );
    f.enumv(
        "alphaComposition",
        "AlphaComposition",
        o.1.name(),
        n.1.name(),
    );
}

fn write_deformer(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    o: &Deformer,
    n: &Deformer,
) -> Result<()> {
    write_parents(
        root,
        ctx,
        src,
        (o.info.parent_part, n.info.parent_part),
        Some((o.target, n.target)),
    )?;
    let id = &o.info.id;
    match (&o.kind, &n.kind) {
        (DeformerKind::Warp(ow), DeformerKind::Warp(nw)) => {
            if (ow.cols, ow.rows) != (nw.cols, nw.rows) || ow.bezier_nets != nw.bezier_nets {
                return unsupported(format!("deformer {id}: warp grid size"));
            }
            write_forms(
                root,
                ctx,
                src,
                id,
                (&ow.grid, &ow.blend_shapes),
                (&nw.grid, &nw.blend_shapes),
            )?;
            let mut f = Fields {
                e: element_at(root, src.element.as_deref())?,
            };
            write_info(&mut f, "CDeformerId", &o.info, &n.info)?;
            f.bool("isQuadTransform", ow.quad, nw.quad);
        }
        (DeformerKind::Rotation(or), DeformerKind::Rotation(nr)) => {
            write_forms(
                root,
                ctx,
                src,
                id,
                (&or.grid, &or.blend_shapes),
                (&nr.grid, &nr.blend_shapes),
            )?;
            let mut f = Fields {
                e: element_at(root, src.element.as_deref())?,
            };
            write_info(&mut f, "CDeformerId", &o.info, &n.info)?;
            f.f32("baseAngle", or.base_angle, nr.base_angle);
            f.f32("handleLengthOnCanvas", or.handle_length, nr.handle_length);
            f.f32("circleRadiusOnCanvas", or.circle_radius, nr.circle_radius);
        }
        _ => return unsupported(format!("deformer {id}: deformer type")),
    }
    Ok(())
}

fn write_art_mesh(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    o: &ArtMesh,
    n: &ArtMesh,
) -> Result<()> {
    // Errors name the mesh.
    write_art_mesh_fields(root, ctx, src, o, n).map_err(|e| match e {
        FormatError::Invalid(m) if !m.contains(&*n.info.id) => {
            FormatError::Invalid(format!("art mesh {}: {m}", n.info.id))
        }
        e => e,
    })
}

fn write_art_mesh_fields(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    o: &ArtMesh,
    n: &ArtMesh,
) -> Result<()> {
    let rest_same = ArtMesh {
        info: o.info.clone(),
        target: o.target,
        invert_clip: o.invert_clip,
        culling: o.culling,
        color_composition: o.color_composition,
        alpha_composition: o.alpha_composition,
        user_data: o.user_data.clone(),
        grid: o.grid.clone(),
        blend_shapes: o.blend_shapes.clone(),
        positions: o.positions.clone(),
        uvs: o.uvs.clone(),
        indices: o.indices.clone(),
        point_uids: o.point_uids.clone(),
        mesh: o.mesh.clone(),
        generator: o.generator,
        clip: o.clip.clone(),
        clip_missing: o.clip_missing,
        texture: o.texture,
        paths: o.paths.clone(),
        ..n.clone()
    } == *o;
    if !rest_same {
        return unsupported(format!("art mesh {}: texture", o.info.id));
    }
    if mesh::texture_input_changed(ctx, o, n) {
        let Some(path) = src.element.clone() else {
            return invalid("art mesh has no element");
        };
        mesh::write_texture_input(root, ctx, &path, o, n)?;
    }
    if mesh::topology_changed(o, n) {
        let Some(path) = src.element.clone() else {
            return invalid("art mesh has no element");
        };
        mesh::write_topology(root, &path, n)?;
    }
    let (om, nm) = (ctx.old_model, ctx.new_model);
    if mesh::clip_guids(om, o) != mesh::clip_guids(nm, n) || o.clip_missing != n.clip_missing {
        let Some(path) = src.element.clone() else {
            return invalid("art mesh has no element");
        };
        mesh::write_clip_list(root, ctx, &path, o, n)?;
    }
    if o.generator != n.generator {
        let Some(path) = src.element.clone() else {
            return invalid("art mesh has no element");
        };
        mesh::write_generator(root, &path, n)?;
    }
    write_parents(
        root,
        ctx,
        src,
        (o.info.parent_part, n.info.parent_part),
        Some((o.target, n.target)),
    )?;
    write_forms(
        root,
        ctx,
        src,
        &o.info.id,
        (&o.grid, &o.blend_shapes),
        (&n.grid, &n.blend_shapes),
    )?;
    let mut f = Fields {
        e: element_at(root, src.element.as_deref())?,
    };
    write_info(&mut f, "CDrawableId", &o.info, &n.info)?;
    f.bool("culling", o.culling, n.culling);
    f.bool("invertClippingMask", o.invert_clip, n.invert_clip);
    f.string("userData", &o.user_data, &n.user_data);
    write_compositions(
        &mut f,
        (o.color_composition, o.alpha_composition),
        (n.color_composition, n.alpha_composition),
    );
    // Deferred: sharing keyforms in place would move elements other
    // writers address by path.
    if o.paths != n.paths {
        let Some(path) = src.element.clone() else {
            return invalid("art mesh has no element");
        };
        ctx.pending_paths.push((path, n.info.guid.clone()));
    }
    Ok(())
}

fn write_glue(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    o: &Glue,
    n: &Glue,
) -> Result<()> {
    let (om, nm) = (ctx.old_model, ctx.new_model);
    let mesh_guid = |m: &Model, k: Option<usize>| {
        k.and_then(|k| m.art_meshes.get(k))
            .map(|a| a.info.guid.clone())
    };
    if mesh_guid(om, o.mesh_a) != mesh_guid(nm, n.mesh_a)
        || mesh_guid(om, o.mesh_b) != mesh_guid(nm, n.mesh_b)
    {
        return unsupported(format!("glue {}: changing its meshes", o.info.id));
    }
    // Pairs compare by vertex UID (vertex indices move with mesh edits).
    if objects::glue_uids(om, o) != objects::glue_uids(nm, n) || o.weights != n.weights {
        let Some(path) = src.element.clone() else {
            return invalid("glue has no element");
        };
        objects::write_glue_pairs(root, &path, nm, n)?;
    }
    write_parents(
        root,
        ctx,
        src,
        (o.info.parent_part, n.info.parent_part),
        None,
    )?;
    write_forms(
        root,
        ctx,
        src,
        &o.info.id,
        (&o.grid, &o.blend_shapes),
        (&n.grid, &n.blend_shapes),
    )?;
    let mut f = Fields {
        e: element_at(root, src.element.as_deref())?,
    };
    write_info(&mut f, "CAffecterId", &o.info, &n.info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::XmlDocument;

    fn parse(s: &str) -> Element {
        XmlDocument::parse(s.as_bytes()).unwrap().root
    }

    #[test]
    fn scalar_kept_where_it_was() {
        let mut e = parse(
            r#"<CPartSource><ACParameterControllableSource xs.n="super"><b xs.n="isVisible">true</b></ACParameterControllableSource><x isLocked="false" /></CPartSource>"#,
        );
        set_scalar(&mut e, "isVisible", "b", "false".into());
        set_scalar(&mut e, "opacity", "f", "0.5".into());
        assert_eq!(e.children[0].children[0].text, "false");
        assert_eq!(e.children.len(), 3);
        assert_eq!(e.children[2].name, "f");
        assert_eq!(e.children[2].field_name(), Some("opacity"));

        let mut a = parse(r#"<CFoo name="short" />"#);
        set_scalar(&mut a, "name", "s", "still short".into());
        assert_eq!(a.attr("name"), Some("still short"));
        // Too long for an attribute: becomes an <s> child (§2.2).
        set_scalar(&mut a, "name", "s", "x".repeat(66));
        assert_eq!(a.attr("name"), None);
        assert_eq!(a.children[0].name, "s");
        assert_eq!(a.children[0].text.len(), 66);
    }

    #[test]
    fn field_attrs_and_arrays() {
        let mut e = parse(
            r#"<K><CPartId xs.n="id" idstr="A" /><null xs.n="multiplyColor" /><float-array xs.n="positions" count="2">0.0 1.0</float-array></K>"#,
        );
        let mut f = Fields { e: &mut e };
        f.id("id", "CPartId", "A", "B");
        f.color("multiplyColor", &WHITE, &[0.5, 1.0, 1.0, 1.0]);
        f.positions("positions", &[0.0, 1.0], &[2.5, -1.0e-5]);
        f.f32("opacity", 1.0, 1.0);
        assert_eq!(e.children[0].attr("idstr"), Some("B"));
        assert_eq!(e.children[1].name, "CFloatColor");
        assert_eq!(e.children[1].attr("red"), Some("0.5"));
        assert_eq!(e.children[2].text, "2.5 -1.0E-5");
        assert_eq!(e.children[2].attr("count"), Some("2"));
        assert_eq!(e.children.len(), 3, "unchanged values are not written");
    }

    #[test]
    fn illegal_characters_are_dropped() {
        assert_eq!(xml_text("a\u{1}b\tc\u{FFFF}"), "ab\tc");
    }

    #[test]
    fn atomic_write_replaces_file() {
        let dir = std::env::temp_dir().join(format!("keyform-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("m.cmo3");
        std::fs::write(&p, b"old").unwrap();
        write_atomic(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
