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

use std::path::Path;
use std::sync::Arc;

use keyform_model::*;

mod grid;

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
        let mut root = self.xml.root.clone();
        let s = &self.sources;
        let (old, new) = (&self.saved, &self.model);
        let mut ctx = Ctx::new(&root, &new.parameters, &s.parameters);
        let ctx = &mut ctx;
        sync_list(
            &mut root,
            &old.parameters,
            &new.parameters,
            |root, i, o, n| {
                let path = s.parameters.get(i).cloned().flatten();
                write_parameter(element_at(root, path.as_deref())?, o, n)
            },
        )?;
        sync_list(&mut root, &old.parts, &new.parts, |root, i, o, n| {
            write_part(root, ctx, source(&s.parts, i)?, o, n)
        })?;
        sync_list(
            &mut root,
            &old.deformers,
            &new.deformers,
            |root, i, o, n| write_deformer(root, ctx, source(&s.deformers, i)?, o, n),
        )?;
        sync_list(
            &mut root,
            &old.art_meshes,
            &new.art_meshes,
            |root, i, o, n| write_art_mesh(root, ctx, source(&s.art_meshes, i)?, o, n),
        )?;
        sync_list(&mut root, &old.glues, &new.glues, |root, i, o, n| {
            write_glue(root, ctx, source(&s.glues, i)?, o, n)
        })?;
        if !ctx.structural {
            self.xml.root = root;
            self.saved = self.model.clone();
            return Ok(());
        }
        grid::remove_orphans(&mut root, &ctx.orphans)?;
        let mut xml = self.xml.clone();
        xml.root = root;
        for name in grid::GRID_IMPORTS {
            ensure_import(&mut xml, name);
        }
        // Debugging aid: keep the candidate document.
        if let Some(p) = std::env::var_os("KEYFORM_DEBUG_SAVE") {
            let _ = std::fs::write(p, xml.write());
        }
        // The rewritten document must load back as exactly the edited model.
        let mut warnings = Vec::new();
        let (loaded, sources) = super::load_model(&self.archive, &xml, &mut warnings)?;
        if !loaded.same_data(&self.model) {
            return invalid(format!(
                "the rewritten document does not reload as the edited model ({})",
                first_difference(&loaded, &self.model)
            ));
        }
        self.xml = xml;
        self.sources = sources;
        self.saved = self.model.clone();
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
        .unwrap_or_else(|| "model-level data".into())
}

/// Adds `<?import name?>` unless present, keeping the imports sorted.
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
    let same_textures = old.textures.len() == new.textures.len()
        && old
            .textures
            .iter()
            .zip(&new.textures)
            .all(|(a, b)| Arc::ptr_eq(a, b) || a == b);
    let same_groups = old.parameter_groups.len() == new.parameter_groups.len()
        && old
            .parameter_groups
            .iter()
            .zip(&new.parameter_groups)
            .all(|(a, b)| Arc::ptr_eq(a, b) || a == b);
    let checks = [
        (same_textures, "textures"),
        (same_groups, "parameter groups"),
        (
            old.root_parameter_entries == new.root_parameter_entries,
            "parameter order",
        ),
        (old.root_part == new.root_part, "root part"),
        (old.physics == new.physics, "physics settings"),
        (old.export == new.export, "export settings"),
        (old.canvas == new.canvas, "canvas"),
        (old.name == new.name, "model name"),
        (old.guid == new.guid, "model GUID"),
        (
            old.eye_blink_parameters == new.eye_blink_parameters
                && old.lip_sync_parameters == new.lip_sync_parameters,
            "effect parameter groups",
        ),
        (old.target_version == new.target_version, "SDK target"),
        (
            old.parameters.len() == new.parameters.len(),
            "parameter count",
        ),
        (old.parts.len() == new.parts.len(), "part count"),
        (old.deformers.len() == new.deformers.len(), "deformer count"),
        (
            old.art_meshes.len() == new.art_meshes.len(),
            "art mesh count",
        ),
        (old.glues.len() == new.glues.len(), "glue count"),
    ];
    match checks.iter().find(|(ok, _)| !ok) {
        Some((_, what)) => unsupported(*what),
        None => Ok(()),
    }
}

fn sync_list<T: PartialEq>(
    root: &mut Element,
    old: &[Arc<T>],
    new: &[Arc<T>],
    mut write: impl FnMut(&mut Element, usize, &T, &T) -> Result<()>,
) -> Result<()> {
    for (i, (o, n)) in old.iter().zip(new).enumerate() {
        if !Arc::ptr_eq(o, n) && **o != **n {
            write(root, i, o, n)?;
        }
    }
    Ok(())
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
    c.set_attr("count", values.len().to_string());
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

fn write_info(f: &mut Fields, id_tag: &str, o: &ObjectInfo, n: &ObjectInfo) -> Result<()> {
    if o.guid != n.guid || o.parent_part != n.parent_part {
        return unsupported(format!("{}: moving to another part", o.id));
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
        if o.positions.len() != n.positions.len() {
            return unsupported("keyform vertex count");
        }
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
fn write_forms<F: FormFields>(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    what: &str,
    (og, ob): (&KeyformGrid<F>, &BlendShapes<F>),
    (ng, nb): (&KeyformGrid<F>, &BlendShapes<F>),
) -> Result<()> {
    if og.bindings != ng.bindings || og.guids != ng.guids || og.keyforms.len() != ng.keyforms.len()
    {
        // Keys changed: the grid is rewritten (blend shapes must stay as they are).
        if ob != nb {
            return unsupported(format!("{what}: blend shapes together with keys"));
        }
        return grid::rebuild_grid(root, ctx, src, what, og, ng);
    }
    let same_shapes = ob.constraints == nb.constraints
        && ob.shapes.len() == nb.shapes.len()
        && ob
            .shapes
            .iter()
            .zip(&nb.shapes)
            .all(|(a, b)| a.parameter == b.parameter && a.key == b.key);
    if !same_shapes {
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
        return unsupported(format!("part {}: children or clipping", o.info.id));
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
    if o.target != n.target {
        return unsupported(format!("deformer {}: parent deformer", o.info.id));
    }
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
    let rest_same = ArtMesh {
        info: o.info.clone(),
        invert_clip: o.invert_clip,
        culling: o.culling,
        color_composition: o.color_composition,
        alpha_composition: o.alpha_composition,
        user_data: o.user_data.clone(),
        grid: o.grid.clone(),
        blend_shapes: o.blend_shapes.clone(),
        ..n.clone()
    } == *o;
    if !rest_same {
        return unsupported(format!(
            "art mesh {}: mesh, texture, clipping or parent deformer",
            o.info.id
        ));
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
    write_info(&mut f, "CDrawableId", &o.info, &n.info)?;
    f.bool("culling", o.culling, n.culling);
    f.bool("invertClippingMask", o.invert_clip, n.invert_clip);
    f.string("userData", &o.user_data, &n.user_data);
    write_compositions(
        &mut f,
        (o.color_composition, o.alpha_composition),
        (n.color_composition, n.alpha_composition),
    );
    Ok(())
}

fn write_glue(
    root: &mut Element,
    ctx: &mut Ctx,
    src: &ObjectSource,
    o: &Glue,
    n: &Glue,
) -> Result<()> {
    let rest_same = Glue {
        info: o.info.clone(),
        grid: o.grid.clone(),
        blend_shapes: o.blend_shapes.clone(),
        ..n.clone()
    } == *o;
    if !rest_same {
        return unsupported(format!("glue {}: meshes or vertex pairs", o.info.id));
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
