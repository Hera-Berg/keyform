//! `.can3` animation documents: CAFF archive + XML object graph, read into
//! [`keyform_model::animation::Animation`].
//! spec: 60-animation/animation-model.md §1–3, 10-formats/can3-schema.md,
//! 10-formats/xml-object-graph.md

use std::collections::HashMap;

use keyform_model::animation::{
    Animation, Attr, Curve, CurveType, EyeBlinkEffect, Handle, IntCurve, Key, LipSyncEffect,
    ModelTrack, ParamGroup, Resource, Scene, Track, TrackKind,
};

use crate::caff::CaffArchive;
use crate::error::{FormatError, Result};
use crate::graph::{Obj, ObjectGraph};
use crate::xml::XmlDocument;

type O<'g, 'd> = Obj<'g, 'd>;

/// Which list of a model track an attribute is in.
pub const PARAMS: u8 = 0;
pub const PARTS: u8 = 1;
/// Eye blink: 0 = eyeOpen, 1 = effectLevel.
pub const EYE_BLINK: u8 = 2;
/// Lip sync: 0 = soundLevel, 1 = scale, 2 = base, 3 = level.
pub const LIP_SYNC: u8 = 3;
/// A track's visual effect (any track kind): 0 = scalex, 1 = scaley,
/// 2 = rotate, 3 = shear, 4 = opacity.
pub const VISUAL: u8 = 4;
/// A track's visual point attributes: 0 = xy, 1 = anchor.
pub const VISUAL_PT: u8 = 5;

/// A curve attribute's place: (scene, track, list, index in the list).
pub type AttrKey = (usize, usize, u8, usize);

/// A loaded `.can3`.
pub struct Can3Document {
    pub archive: CaffArchive,
    pub xml: XmlDocument,
    pub animation: Animation,
    pub warnings: Vec<String>,
    /// The shared element id of each model-track attribute.
    pub sources: HashMap<AttrKey, String>,
    /// The animation as last loaded or saved.
    pub saved: Animation,
    /// Scenes or tracks were added or removed since the last save.
    structural: bool,
}

/// Everything a structural edit changes, for undo.
#[derive(Clone)]
pub struct Can3Snapshot {
    xml: XmlDocument,
    animation: Animation,
    sources: HashMap<AttrKey, String>,
    saved: Animation,
}

impl Can3Document {
    /// The document's state (for undoing a structural edit).
    pub fn snapshot(&self) -> Can3Snapshot {
        Can3Snapshot {
            xml: self.xml.clone(),
            animation: self.animation.clone(),
            sources: self.sources.clone(),
            saved: self.saved.clone(),
        }
    }

    /// Puts back a state taken by [`Self::snapshot`].
    pub fn restore(&mut self, s: Can3Snapshot) {
        self.xml = s.xml;
        self.animation = s.animation;
        self.sources = s.sources;
        self.saved = s.saved;
        // The XML now differs from what was last written either way.
        self.structural = true;
    }

    pub fn read(data: &[u8]) -> Result<Self> {
        let archive = CaffArchive::read(data)?;
        let xml = XmlDocument::parse(
            &archive
                .main_xml()
                .ok_or_else(|| FormatError::Invalid("archive has no main.xml".into()))?
                .data,
        )?;
        let mut warnings = Vec::new();
        let mut sources = HashMap::new();
        let animation = load(&xml, &mut warnings, &mut sources)?;
        Ok(Self {
            archive,
            xml,
            saved: animation.clone(),
            animation,
            warnings,
            sources,
            structural: false,
        })
    }

    /// Writes the edited curves into the document (in place). The result
    /// must load back as exactly the edited animation.
    fn sync(&mut self) -> Result<()> {
        let mut xml = self.xml.clone();
        // Form animation keys can't be written yet: their FramePosAndForm
        // encoding is unknown (Q-0033).
        let forms = |a: &Animation| -> Vec<keyform_model::animation::FormAnimation> {
            a.scenes
                .iter()
                .flat_map(|s| s.tracks.iter())
                .filter_map(|t| match &t.kind {
                    TrackKind::Model(m) => Some(m.form.clone()),
                    _ => None,
                })
                .collect()
        };
        if forms(&self.saved) != forms(&self.animation) {
            return Err(FormatError::Unsupported(
                "form animation edits can't be saved yet (the form key encoding is unknown, Q-0033)".into(),
            ));
        }
        let changed = changed_attrs(&self.saved, &self.animation);
        // Scene settings and track user data (events) changed in place.
        let mut other = false;
        let without_tracks =
            |s: &keyform_model::animation::Scene| keyform_model::animation::Scene {
                tracks: Vec::new(),
                ..s.clone()
            };
        for (si, s) in self.animation.scenes.iter().enumerate() {
            let Some(old) = self.saved.scenes.get(si) else {
                continue;
            };
            if without_tracks(old) != without_tracks(s) {
                crate::can3_build::write_scene_settings(&mut xml, si, s)?;
                other = true;
            }
            for (ti, t) in s.tracks.iter().enumerate() {
                if old.tracks.get(ti).map(|o| &o.user_data) != Some(&t.user_data) {
                    crate::can3_build::write_user_data(&mut xml, si, ti, &t.user_data)?;
                    other = true;
                }
            }
        }
        if changed.is_empty() && !other {
            return Ok(());
        }
        for key in changed {
            let Some(id) = self.sources.get(&key) else {
                return Err(FormatError::Unsupported(format!(
                    "attribute {key:?}: no stored element"
                )));
            };
            if key.2 == VISUAL_PT {
                let p = point_at(&self.animation, key)
                    .ok_or_else(|| FormatError::Invalid(format!("attribute {key:?} missing")))?;
                write_point_curve(&mut xml, id, p)?;
                continue;
            }
            let attr = attr_at(&self.animation, key)
                .ok_or_else(|| FormatError::Invalid(format!("attribute {key:?} missing")))?;
            write_curve(&mut xml, id, attr)?;
        }
        let mut warnings = Vec::new();
        let mut sources = HashMap::new();
        let back = load(&xml, &mut warnings, &mut sources)?;
        if back != self.animation {
            return Err(FormatError::Invalid(
                "the rewritten animation does not reload as the edited one".into(),
            ));
        }
        self.xml = xml;
        self.sources = sources;
        self.saved = self.animation.clone();
        Ok(())
    }

    /// Re-reads the typed animation after a structural change of the XML.
    fn reload(&mut self) -> Result<()> {
        let mut warnings = Vec::new();
        let mut sources = HashMap::new();
        self.animation = load(&self.xml, &mut warnings, &mut sources)?;
        self.sources = sources;
        self.saved = self.animation.clone();
        self.warnings.extend(warnings);
        self.structural = true;
        Ok(())
    }

    /// Writes the edited curves into the document and returns the file.
    /// Changed attributes get a rebuilt `valueData`; after structural
    /// changes, objects no longer reachable from the root are dropped.
    /// spec: can3-schema.md (CMutableSequence, CBezierPt), xml-object-graph.md
    pub fn save_bytes(&mut self) -> Result<Vec<u8>> {
        self.sync()?;
        if self.structural {
            drop_unreachable(&mut self.xml);
            self.reload()?;
            self.structural = false;
        }
        // Rewritten curves may bring types the document didn't use yet.
        crate::can3_build::fix_markers(&mut self.xml);
        let mut archive = self.archive.clone();
        match archive.entries.iter_mut().find(|e| e.tag == "main_xml") {
            Some(e) => e.data = self.xml.write(),
            None => return Err(FormatError::Invalid("archive has no main.xml".into())),
        }
        let bytes = archive.write()?;
        self.archive = archive;
        Ok(bytes)
    }

    /// A new animation linking `model` (file `model_path`) with one scene.
    /// spec: animation-model.md §1–2
    pub fn new_animation(
        name: &str,
        file: &str,
        model: &keyform_model::Model,
        model_path: &str,
        scene: &str,
        frames: i32,
        fps: f64,
    ) -> Result<Self> {
        let xml = crate::can3_build::new_animation_xml(
            name, file, model, model_path, scene, frames, fps,
        )?;
        let mut archive = CaffArchive::new_document();
        archive.entries.push(crate::caff::CaffEntry {
            path: "main.xml".into(),
            tag: "main_xml".into(),
            obfuscated: true,
            compression: crate::caff::Compression::Fast,
            data: xml.write(),
        });
        let bytes = archive.write()?;
        Self::read(&bytes)
    }

    /// Adds a scene (with a model track for `model`, the linked model).
    pub fn add_scene(
        &mut self,
        name: &str,
        model: &keyform_model::Model,
        frames: i32,
        fps: f64,
    ) -> Result<usize> {
        self.sync()?;
        let size = self
            .animation
            .scenes
            .first()
            .map_or((1280, 720), |s| (s.width, s.height));
        crate::can3_build::add_scene_xml(&mut self.xml, name, model, frames, fps, size)?;
        self.reload()?;
        Ok(self.animation.scenes.len() - 1)
    }

    pub fn delete_scene(&mut self, index: usize) -> Result<()> {
        self.sync()?;
        crate::can3_build::delete_scene_xml(&mut self.xml, index)?;
        self.reload()
    }

    /// Adds a model track for `model` (the linked model) to scene `index`.
    pub fn add_model_track(
        &mut self,
        index: usize,
        model: &keyform_model::Model,
        name: &str,
    ) -> Result<()> {
        self.sync()?;
        crate::can3_build::add_model_track_xml(&mut self.xml, index, model, name)?;
        self.reload()
    }

    /// Deletes track `t` of scene `index` (not the root group).
    pub fn delete_track(&mut self, index: usize, t: usize) -> Result<()> {
        self.sync()?;
        crate::can3_build::delete_track_xml(&mut self.xml, index, t)?;
        self.reload()
    }

    /// Sets the document's own stored path (`CAnimation.file`, which the
    /// linked models' paths are resolved against).
    pub fn set_file(&mut self, path: &str) -> Result<()> {
        self.sync()?;
        let main = self
            .xml
            .root
            .children
            .iter()
            .find(|c| c.name == "main")
            .and_then(|m| m.children.first())
            .and_then(|c| c.attr("xs.ref"))
            .map(str::to_owned)
            .ok_or_else(|| FormatError::Invalid("no CAnimation".into()))?;
        let sh = self
            .xml
            .root
            .children
            .iter_mut()
            .find(|c| c.name == "shared")
            .ok_or_else(|| FormatError::Invalid("no <shared>".into()))?;
        let a = sh
            .children
            .iter_mut()
            .find(|c| c.attr("xs.id") == Some(main.as_str()))
            .ok_or_else(|| FormatError::Invalid("no CAnimation".into()))?;
        match a.child_field_mut("file") {
            Some(f) => f.text = path.to_owned(),
            None => {
                return Err(FormatError::Unsupported(
                    "the animation has no file field".into(),
                ))
            }
        }
        Ok(())
    }

    /// Adds a sound track playing the WAV `path` to scene `index`, as the
    /// lip-sync source of the scene's model track.
    /// Adds an image track for the image at `path` (`size` pixels).
    pub fn add_image_track(&mut self, index: usize, path: &str, size: (u32, u32)) -> Result<()> {
        self.sync()?;
        crate::can3_build::add_image_track_xml(&mut self.xml, index, path, size)?;
        self.reload()
    }

    /// Adds a sound track for the WAV at `path`, `seconds` long.
    pub fn add_sound_track(&mut self, index: usize, path: &str, seconds: f64) -> Result<()> {
        self.sync()?;
        crate::can3_build::add_sound_track_xml(&mut self.xml, index, path, seconds)?;
        self.reload()
    }

    pub fn rename_scene(&mut self, index: usize, name: &str) -> Result<()> {
        self.sync()?;
        crate::can3_build::rename_scene_xml(&mut self.xml, index, name)?;
        self.reload()
    }
}

/// Removes shared objects nothing reachable from `<main>` refers to.
fn drop_unreachable(xml: &mut XmlDocument) {
    use std::collections::HashSet;
    let Some(sh) = xml.root.children.iter().position(|c| c.name == "shared") else {
        return;
    };
    let by_id: HashMap<String, usize> = xml.root.children[sh]
        .children
        .iter()
        .enumerate()
        .filter_map(|(k, c)| c.attr("xs.id").map(|id| (id.to_owned(), k)))
        .collect();
    let mut live: HashSet<usize> = HashSet::new();
    let mut work: Vec<&crate::xml::Element> = xml
        .root
        .children
        .iter()
        .filter(|c| c.name != "shared")
        .collect();
    let shared_children = &xml.root.children[sh].children;
    while let Some(e) = work.pop() {
        if let Some(k) = e.attr("xs.ref").and_then(|r| by_id.get(r)) {
            if live.insert(*k) {
                work.push(&shared_children[*k]);
            }
        }
        work.extend(e.children.iter());
    }
    let mut k = 0;
    xml.root.children[sh].children.retain(|_| {
        let keep = live.contains(&k);
        k += 1;
        keep
    });
}

/// The attribute at `key`.
pub fn attr_at(a: &Animation, (s, t, list, k): AttrKey) -> Option<&Attr> {
    let tr = a.scenes.get(s)?.tracks.get(t)?;
    if list == VISUAL {
        let v = tr.visual.as_ref();
        return match k {
            0 => v.map(|v| &v.scale_x),
            1 => v.map(|v| &v.scale_y),
            2 => v.map(|v| &v.rotate),
            3 => v.map(|v| &v.shear),
            4 => tr.opacity.as_ref(),
            _ => None,
        };
    }
    let TrackKind::Model(m) = &tr.kind else {
        return None;
    };
    match list {
        PARAMS => m.params.get(k),
        PARTS => m.parts.get(k),
        EYE_BLINK => {
            let e = m.eye_blink.as_ref()?;
            [&e.eye_open, &e.level].get(k).copied()
        }
        LIP_SYNC => {
            let e = m.lip_sync.as_ref()?;
            [&e.sound_level, &e.scale, &e.base, &e.level]
                .get(k)
                .copied()
        }
        _ => None,
    }
}

/// A track's visual point attribute at `key` (list [`VISUAL_PT`]).
pub fn point_at(
    a: &Animation,
    (s, t, _, k): AttrKey,
) -> Option<&keyform_model::animation::PointCurve> {
    let v = a.scenes.get(s)?.tracks.get(t)?.visual.as_ref()?;
    match k {
        0 => Some(&v.xy),
        1 => Some(&v.anchor),
        _ => None,
    }
}

/// Attributes whose curves differ between `a` and `b`.
fn changed_attrs(a: &Animation, b: &Animation) -> Vec<AttrKey> {
    let mut out = Vec::new();
    for (si, s) in b.scenes.iter().enumerate() {
        for (ti, t) in s.tracks.iter().enumerate() {
            for k in 0..5 {
                let key = (si, ti, VISUAL, k);
                if let Some(at) = attr_at(b, key) {
                    if attr_at(a, key).map(|x| &x.curve) != Some(&at.curve) {
                        out.push(key);
                    }
                }
            }
            for k in 0..2 {
                let key = (si, ti, VISUAL_PT, k);
                if let Some(p) = point_at(b, key) {
                    if point_at(a, key) != Some(p) {
                        out.push(key);
                    }
                }
            }
            let TrackKind::Model(m) = &t.kind else {
                continue;
            };
            let mut lists: Vec<(u8, Vec<&Attr>)> = vec![
                (PARAMS, m.params.iter().collect()),
                (PARTS, m.parts.iter().collect()),
            ];
            if let Some(e) = &m.eye_blink {
                lists.push((EYE_BLINK, vec![&e.eye_open, &e.level]));
            }
            if let Some(e) = &m.lip_sync {
                lists.push((LIP_SYNC, vec![&e.sound_level, &e.scale, &e.base, &e.level]));
            }
            for (list, attrs) in lists {
                for (k, at) in attrs.into_iter().enumerate() {
                    if attr_at(a, (si, ti, list, k)).map(|x| &x.curve) != Some(&at.curve) {
                        out.push((si, ti, list, k));
                    }
                }
            }
        }
    }
    out
}

fn scalar_el(tag: &str, name: &str, text: String) -> crate::xml::Element {
    let mut e = crate::xml::Element::new(tag).with_attr("xs.n", name);
    e.text = text;
    e
}

/// Rebuilds the key points of the shared point attribute `id`
/// (`CXY_TNSSequence` of `CPtTNS`). spec: can3-schema.md, animation-model.md §5.1
fn write_point_curve(
    xml: &mut XmlDocument,
    id: &str,
    c: &keyform_model::animation::PointCurve,
) -> Result<()> {
    use crate::graph::format_f32;
    use crate::xml::Element;
    let shared = xml
        .root
        .children
        .iter_mut()
        .find(|c| c.name == "shared")
        .ok_or_else(|| FormatError::Invalid("document has no <shared>".into()))?;
    let el = shared
        .children
        .iter_mut()
        .find(|c| c.attr("xs.id") == Some(id))
        .ok_or_else(|| FormatError::Invalid(format!("ref id {id}: not found")))?;
    let vd = el
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("valueDataXY"))
        .ok_or_else(|| FormatError::Invalid(format!("{id}: no valueDataXY")))?;
    let v2 = |name: &str, p: [f32; 2]| {
        let mut e = Element::new("GVector2").with_attr("xs.n", name);
        e.children = vec![
            scalar_el("f", "x", format_f32(p[0])),
            scalar_el("f", "y", format_f32(p[1])),
        ];
        e
    };
    let mut points = Element::new("array")
        .with_attr("xs.n", "points")
        .with_attr("count", c.keys.len().to_string())
        .with_attr("type", "CPtTNS");
    for (i, (f, p)) in c.keys.iter().enumerate() {
        let mut e = Element::new("CPtTNS");
        e.children = vec![
            v2("super", *p),
            scalar_el(
                "b",
                "isCorner",
                c.corners.get(i).copied().unwrap_or(false).to_string(),
            ),
            v2("value", *p),
            scalar_el("i", "pos", f.to_string()),
        ];
        points.children.push(e);
    }
    match vd
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("points"))
    {
        Some(old) => *old = points,
        None => vd.children.insert(0, points),
    }
    Ok(())
}

/// Rebuilds the `valueData` of the shared attribute `id` from `a`'s curve.
fn write_curve(xml: &mut XmlDocument, id: &str, a: &Attr) -> Result<()> {
    use crate::graph::{format_f32, format_f64};
    use crate::xml::Element;
    let shared = xml
        .root
        .children
        .iter_mut()
        .find(|c| c.name == "shared")
        .ok_or_else(|| FormatError::Invalid("document has no <shared>".into()))?;
    let el = shared
        .children
        .iter_mut()
        .find(|c| c.attr("xs.id") == Some(id))
        .ok_or_else(|| FormatError::Invalid(format!("ref id {id}: not found")))?;
    let attr_tag = el.name.clone();
    let vd = el
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("valueData"))
        .ok_or_else(|| FormatError::Invalid(format!("{id}: no valueData")))?;
    if vd.name != "CMutableSequence" {
        return Err(FormatError::Unsupported(format!(
            "{}: keys on a fixed sequence",
            a.id
        )));
    }
    let c = &a.curve;
    let d = |v: f64| format_f64(v);
    let (cur_min, cur_max) = c
        .keys
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), k| {
            (lo.min(k.value), hi.max(k.value))
        });
    let first = c.keys.first();
    let mut keypts = Element::new("int-array")
        .with_attr("xs.n", "keyPts2")
        .with_attr("count", c.keys.len().to_string());
    keypts.text = c
        .keys
        .iter()
        .map(|k| k.frame.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let mut sup = Element::new("ACValueSequence").with_attr("xs.n", "super");
    sup.children = vec![
        scalar_el(
            "d",
            "curMin",
            d(if c.keys.is_empty() { 0.0 } else { cur_min }),
        ),
        scalar_el(
            "d",
            "curMax",
            d(if c.keys.is_empty() { 0.0 } else { cur_max }),
        ),
        scalar_el("i", "posStart", "0".into()),
        keypts,
        scalar_el(
            "i",
            "keyMin",
            first.map_or(i32::MAX, |k| k.frame).to_string(),
        ),
        scalar_el(
            "i",
            "keyMax",
            c.keys.last().map_or(i32::MIN, |k| k.frame).to_string(),
        ),
        scalar_el("d", "lastValue", first.map_or("NaN".into(), |k| d(k.value))),
        scalar_el("i", "lastPos", first.map_or(-1, |k| k.frame).to_string()),
        Element::new(attr_tag.as_str())
            .with_attr("xs.n", "attr")
            .with_attr("xs.ref", id),
        scalar_el("d", "baseValue", d(c.base)),
    ];
    let handle = |name: &str,
                  h: Option<keyform_model::animation::Handle>,
                  k: &keyform_model::animation::Key| {
        let h = h.unwrap_or(keyform_model::animation::Handle {
            frame: k.frame as f32,
            value: k.value,
        });
        let mut e = Element::new("CBezierCtrlPt").with_attr("xs.n", name);
        e.children = vec![
            scalar_el("f", "posF", format_f32(h.frame)),
            scalar_el("i", "pos", "0".into()),
            scalar_el("d", "doubleValue", d(h.value)),
            scalar_el("b", "isPosOptimized", "false".into()),
        ];
        e
    };
    let mut points = Element::new("array")
        .with_attr("xs.n", "points")
        .with_attr("count", c.keys.len().to_string())
        .with_attr("type", "CBezierPt");
    for k in &c.keys {
        let mut an = Element::new("CSeqPt").with_attr("xs.n", "anchor");
        an.children = vec![
            scalar_el("b", "isCorner", k.corner.to_string()),
            scalar_el("i", "pos", k.frame.to_string()),
            scalar_el("d", "doubleValue", d(k.value)),
        ];
        let mut bp = Element::new("CBezierPt");
        bp.children = vec![an, handle("next", k.next, k), handle("prev", k.prev, k)];
        points.children.push(bp);
    }
    let mut types = Element::new("carray_list")
        .with_attr("xs.n", "curveTypes")
        .with_attr("count", c.keys.len().to_string());
    for i in 0..c.keys.len() {
        types
            .children
            .push(Element::new("CCurveType").with_attr("v", c.type_at(i).name()));
    }
    vd.children = vec![sup, points, types];
    Ok(())
}

/// Reads the typed animation from a parsed `.can3` document.
pub fn load_animation(xml: &XmlDocument, warnings: &mut Vec<String>) -> Result<Animation> {
    load(xml, warnings, &mut HashMap::new())
}

fn load(
    xml: &XmlDocument,
    warnings: &mut Vec<String>,
    sources: &mut HashMap<AttrKey, String>,
) -> Result<Animation> {
    let g = ObjectGraph::new(xml)?;
    let root = g.main()?;
    if root.tag() != "CAnimation" {
        return Err(FormatError::Invalid(format!(
            "root object is <{}>, expected <CAnimation>",
            root.tag()
        )));
    }
    let mut a = Animation {
        name: root.string("name")?.unwrap_or_default(),
        // A missing flag reads as legacy mode (Q-0028).
        smooth_legacy: root.bool("isSmoothCurveLegacyMode")?.unwrap_or(true),
        ..Default::default()
    };
    if let Some(rm) = root.field("resourceManager")? {
        for r in rm.list("_resourceRefList")? {
            let Some(f) = r.field("resourceRef")? else {
                continue;
            };
            a.resources.push(Resource {
                guid: f.guid("guid")?.unwrap_or_default().to_owned(),
                name: f.string("name")?.unwrap_or_default(),
                path: f.string("srcFile")?.unwrap_or_default(),
            });
        }
    }
    for s in root.list("_scenes")? {
        let si = a.scenes.len();
        let mut ids = Vec::new();
        match scene(&s, warnings, &mut ids) {
            Ok(sc) => {
                a.scenes.push(sc);
                for (t, part, k, id) in ids {
                    sources.insert((si, t, part, k), id);
                }
            }
            Err(e) => warnings.push(format!("scene skipped: {e}")),
        }
    }
    Ok(a)
}

fn curve_type(o: Option<&str>) -> CurveType {
    o.and_then(CurveType::from_name).unwrap_or_default()
}

/// Attribute element ids found while reading: (track, part?, index, id).
type AttrIds = Vec<(usize, u8, usize, String)>;

fn scene(s: &O, warnings: &mut Vec<String>, ids: &mut AttrIds) -> Result<Scene> {
    let mi = s
        .field("movieInfo")?
        .ok_or_else(|| FormatError::Invalid("scene without movieInfo".into()))?;
    let mut sc = Scene {
        name: s.string("sceneName")?.unwrap_or_default(),
        guid: s.guid("guid")?.unwrap_or_default().to_owned(),
        width: mi.i32("width")?.unwrap_or(0),
        height: mi.i32("height")?.unwrap_or(0),
        canvas: match s.field("canvas")? {
            Some(c) => (
                c.i32("pixelWidth")?.unwrap_or(0),
                c.i32("pixelHeight")?.unwrap_or(0),
            ),
            None => (0, 0),
        },
        duration: mi.i32("duration")?.unwrap_or(0),
        fps: mi.f64("fps")?.unwrap_or(30.0),
        work_start: mi.i32("workspaceStart")?.unwrap_or(0),
        work_end: mi.i32("workspaceEnd")?.unwrap_or(0),
        fade_in_ms: mi.i32("fadeInMSec")?.unwrap_or(-1),
        fade_out_ms: mi.i32("fadeOutMSec")?.unwrap_or(-1),
        bezier_restricted: mi.bool("isBezierRestricted")?.unwrap_or(false),
        loop_motion: mi.bool("isLoopMotion")?.unwrap_or(false),
        start_frame: mi.i32("startFrame")?.unwrap_or(0),
        export_motion: s.bool("exportMotionFile")?.unwrap_or(true),
        default_param_curve: curve_type(s.enum_value("defaultParameterCurveType")?),
        default_part_curve: s
            .enum_value("defaultPartCurveType")?
            .and_then(CurveType::from_name)
            .unwrap_or(CurveType::Step),
        target: s.enum_value("targetVersion")?.unwrap_or("").to_owned(),
        tracks: Vec::new(),
    };
    // Tracks in tree order from the root; parents by `parentGuid`.
    let sources = match s.field("trackSourceSet")? {
        Some(set) => set.list("_sources")?,
        None => Vec::new(),
    };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut parents: Vec<Option<String>> = Vec::new();
    for t in &sources {
        let mut own = Vec::new();
        match track(t, &mut own) {
            Ok((tr, parent)) => {
                let ti = sc.tracks.len();
                ids.extend(own.into_iter().map(|(p, k, id)| (ti, p, k, id)));
                index.insert(tr.guid.clone(), sc.tracks.len());
                parents.push(parent);
                sc.tracks.push(tr);
            }
            Err(e) => warnings.push(format!("{}: track skipped: {e}", sc.name)),
        }
    }
    for (t, p) in sc.tracks.iter_mut().zip(parents) {
        t.parent = p.and_then(|g| index.get(&g).copied());
    }
    Ok(sc)
}

fn track(t: &O, ids: &mut Vec<(u8, usize, String)>) -> Result<(Track, Option<String>)> {
    let src = source_of(t)?;
    let mut tr = Track {
        name: src.string("name")?.unwrap_or_default(),
        guid: src.guid("guid")?.unwrap_or_default().to_owned(),
        parent: None,
        start: src.i32("start")?.unwrap_or(0),
        offset: src.i32("internalOffset")?.unwrap_or(0),
        duration: src.i32("duration")?.unwrap_or(0),
        visible: src.bool("visible")?.unwrap_or(true),
        mute: src.bool("mute")?.unwrap_or(false),
        guide: src.bool("isGuide")?.unwrap_or(false),
        repeat: src.bool("isRepeat")?.unwrap_or(false),
        kind: TrackKind::Other(t.tag().to_owned()),
        opacity: None,
        frame_step: None,
        visual: None,
        user_data: Vec::new(),
    };
    let parent = src.guid("parentGuid")?.map(str::to_owned);
    if let Some(ud) = src.field("userData")? {
        for e in ud.items()? {
            // Entry form (integer keys): <entry><int key/><s value/></entry>.
            let frame = e.scalar("key")?.and_then(|s| s.trim().parse().ok());
            let text = e.string("value")?;
            if let (Some(f), Some(t)) = (frame, text) {
                tr.user_data.push((f, t));
            }
        }
        tr.user_data.sort_by_key(|u| u.0);
    }
    if let Some(v) = src.field("visualEffect")?.filter(|v| !v.is_null()) {
        let mut vis = keyform_model::animation::Visual::default();
        for at in effect_attrs(&v)? {
            let slot = match attr_id(&at)?.as_str() {
                "scalex" => Some((VISUAL, 0)),
                "scaley" => Some((VISUAL, 1)),
                "rotate" => Some((VISUAL, 2)),
                "shear" => Some((VISUAL, 3)),
                "opacity" => Some((VISUAL, 4)),
                "xy" => Some((VISUAL_PT, 0)),
                "anchor" => Some((VISUAL_PT, 1)),
                _ => None,
            };
            if let (Some((list, k)), Some(id)) = (slot, at.attr("xs.id")) {
                ids.push((list, k, id.to_owned()));
            }
            match attr_id(&at)?.as_str() {
                "opacity" => tr.opacity = Some(float_attr(&at)?),
                "frameStep" => tr.frame_step = int_curve(&at)?,
                "scalex" => vis.scale_x = float_attr(&at)?,
                "scaley" => vis.scale_y = float_attr(&at)?,
                "rotate" => vis.rotate = float_attr(&at)?,
                "shear" => vis.shear = float_attr(&at)?,
                "xy" => vis.xy = point_curve(&at)?,
                "anchor" => vis.anchor = point_curve(&at)?,
                _ => {}
            }
        }
        tr.visual = Some(vis);
    }
    tr.kind = match t.tag() {
        "CMvTrack_Group_Source" => TrackKind::Group,
        "CMvTrack_Live2DModel_Source" => TrackKind::Model(Box::new(model_track(t, ids)?)),
        "CMvTrack_JavaSoundTrack_Source" => {
            let volume = match src.field("soundEffect")?.filter(|e| !e.is_null()) {
                Some(e) => match e.field("attrVolume")? {
                    Some(a) => Some(float_attr(&a)?),
                    None => None,
                },
                None => None,
            };
            TrackKind::Sound(Box::new(keyform_model::animation::SoundTrack {
                resource: t.guid("_resourceGuid")?.map(str::to_owned),
                speed: t.f64("speed")?.unwrap_or(100.0),
                start_frame: t.i32("startFrame")?.unwrap_or(0),
                volume,
            }))
        }
        "CMvTrack_Image_Source" => {
            TrackKind::Image(Box::new(keyform_model::animation::ImageTrack {
                resource: t.guid("_resourceGuid")?.map(str::to_owned),
            }))
        }
        "CMvTrack_Scene_Source" => {
            let mut frame_no = None;
            if let Some(em) = src.field("effectManager")? {
                for e in em.list("effectList")? {
                    if e.tag() == "CMvEffect_SceneTrackDefault" {
                        if let Some(a) = e.field("attrFrameNo")?.filter(|a| !a.is_null()) {
                            frame_no = Some(float_attr(&a)?);
                        }
                    }
                }
            }
            TrackKind::Scene(Box::new(keyform_model::animation::SceneTrack {
                scene: t.guid("resourceGuid")?.map(str::to_owned),
                work_area: t.bool("isWorkAreaReferenced")?.unwrap_or(false),
                start_frame: t.i32("startFrame")?.unwrap_or(0),
                frame_no,
            }))
        }
        other => TrackKind::Other(other.to_owned()),
    };
    Ok((tr, parent))
}

/// The `ICMvTrack_Source` level of a track (on its `super` chain).
fn source_of<'g, 'd>(t: &O<'g, 'd>) -> Result<O<'g, 'd>> {
    let mut cur = *t;
    for _ in 0..4 {
        if cur.tag() == "ICMvTrack_Source" {
            return Ok(cur);
        }
        match cur.sup()? {
            Some(s) => cur = s,
            None => break,
        }
    }
    // Fields resolve through `super` anyway.
    Ok(*t)
}

fn effect_attrs<'g, 'd>(e: &O<'g, 'd>) -> Result<Vec<O<'g, 'd>>> {
    e.list("attrList")
}

fn attr_id(a: &O) -> Result<String> {
    Ok(a.field("id")?
        .and_then(|i| i.attr("idstr"))
        .unwrap_or_default()
        .to_owned())
}

fn float_attr(a: &O) -> Result<Attr> {
    let mut at = Attr {
        id: attr_id(a)?,
        name: a.string("name")?.unwrap_or_default(),
        guid: a.guid("guid")?.map(str::to_owned),
        active: a.bool("isActive")?.unwrap_or(true),
        shy: a.bool("isShyMode")?.unwrap_or(false),
        freeze: a.bool("isFreezeMode")?.unwrap_or(false),
        range: (
            a.f64("rangeMin")?.unwrap_or(f64::MIN),
            a.f64("rangeMax")?.unwrap_or(f64::MAX),
        ),
        fade_in_ms: -1,
        fade_out_ms: -1,
        ..Default::default()
    };
    if a.bool("isRepeat")?.unwrap_or(false) {
        at.repeat = Some((
            a.f64("repeatMin")?.unwrap_or(at.range.0),
            a.f64("repeatMax")?.unwrap_or(at.range.1),
        ));
    }
    if let Some(op) = a.field("optionParam")? {
        at.fade_in_ms = op.i32("KEY_ATTR_FADE_IN")?.unwrap_or(-1);
        at.fade_out_ms = op.i32("KEY_ATTR_FADE_OUT")?.unwrap_or(-1);
    }
    if let Some(v) = a.field("valueData")? {
        at.curve = curve(&v)?;
    }
    Ok(at)
}

fn handle(h: Option<O>) -> Result<Option<Handle>> {
    let Some(h) = h else { return Ok(None) };
    if h.is_null() {
        return Ok(None);
    }
    Ok(Some(Handle {
        frame: h.f32("posF")?.unwrap_or(0.0),
        value: h.f64("doubleValue")?.unwrap_or(0.0),
    }))
}

fn curve(v: &O) -> Result<Curve> {
    let base = v.f64("baseValue")?.unwrap_or(0.0);
    match v.tag() {
        "CFixedSequence" => Ok(Curve {
            base,
            fixed: true,
            ..Default::default()
        }),
        _ => {
            let mut c = Curve::constant(base);
            for p in v.list("points")? {
                let Some(an) = p.field("anchor")? else {
                    continue;
                };
                c.keys.push(Key {
                    frame: an.i32("pos")?.unwrap_or(0),
                    value: an.f64("doubleValue")?.unwrap_or(0.0),
                    corner: an.bool("isCorner")?.unwrap_or(false),
                    prev: handle(p.field("prev")?)?,
                    next: handle(p.field("next")?)?,
                });
            }
            for t in v.list("curveTypes")? {
                c.types.push(curve_type(t.attr("v")));
            }
            Ok(c)
        }
    }
}

/// A `CMvAttrPt`'s `CXY_TNSSequence`: (pos, value) points.
fn point_curve(a: &O) -> Result<keyform_model::animation::PointCurve> {
    let mut c = keyform_model::animation::PointCurve::default();
    let Some(v) = a.field("valueDataXY")? else {
        return Ok(c);
    };
    let xy = |o: Option<O>| -> Result<[f32; 2]> {
        Ok(match o {
            Some(p) => [p.f32("x")?.unwrap_or(0.0), p.f32("y")?.unwrap_or(0.0)],
            None => [0.0, 0.0],
        })
    };
    c.base = xy(v.field("basePt")?)?;
    let mut pts = Vec::new();
    for p in v.list("points")? {
        pts.push((
            p.i32("pos")?.unwrap_or(0),
            xy(p.field("value")?)?,
            p.bool("isCorner")?.unwrap_or(false),
        ));
    }
    pts.sort_by_key(|k| k.0);
    c.keys = pts.iter().map(|k| (k.0, k.1)).collect();
    c.corners = pts.iter().map(|k| k.2).collect();
    Ok(c)
}

fn int_curve(a: &O) -> Result<Option<IntCurve>> {
    let Some(v) = a.field("valueData")? else {
        return Ok(None);
    };
    let mut c = IntCurve {
        base: v.f64("baseValue")?.unwrap_or(0.0) as i32,
        keys: Vec::new(),
    };
    for p in v.list("points")? {
        c.keys.push((
            p.i32("pos")?.unwrap_or(0),
            p.f64("doubleValue")?.unwrap_or(0.0).round() as i32,
        ));
    }
    Ok(Some(c))
}

fn id_list(o: &O, name: &str) -> Result<Vec<String>> {
    Ok(o.list(name)?
        .iter()
        .filter_map(|i| i.attr("idstr").map(str::to_owned))
        .collect())
}

/// One form-animation map (`formMapOnGlobal` / `formMapOnLocal`):
/// entries GUID → `KeyFormsForObject`. spec: form-animation-and-controllers.md §1
fn form_map(m: Option<O>) -> Result<Vec<keyform_model::animation::FormObject>> {
    use keyform_model::animation::{FormKey, FormObject};
    let mut out = Vec::new();
    let Some(m) = m else { return Ok(out) };
    for e in m.items()? {
        let (Some(k), Some(v)) = (e.field("key")?, e.field("value")?) else {
            continue;
        };
        let mut o = FormObject {
            guid: k.attr("uuid").unwrap_or_default().to_owned(),
            ..Default::default()
        };
        for fk in v.list("framePosKeyformList")? {
            let pos = |f: Option<O>| -> Result<Vec<f32>> {
                Ok(match f {
                    Some(f) => f.f32_array("positions")?.unwrap_or_default(),
                    None => Vec::new(),
                })
            };
            o.keys.push(FormKey {
                frame: fk.i32("framePos")?.unwrap_or(0),
                original: pos(fk.field("originalForm")?)?,
                target: pos(fk.field("targetForm")?)?,
            });
        }
        o.keys.sort_by_key(|k| k.frame);
        if let Some(a) = v.field("attrTimeRemapping")?.filter(|a| !a.is_null()) {
            o.time_remap = Some(float_attr(&a)?);
        }
        out.push(o);
    }
    Ok(out)
}

fn model_track(t: &O, ids: &mut Vec<(u8, usize, String)>) -> Result<ModelTrack> {
    let mut m = ModelTrack {
        resource: t.guid("_resourceGuid")?.map(str::to_owned),
        ..Default::default()
    };
    if let Some(e) = t.field("keyParamEffect")? {
        for a in effect_attrs(&e)? {
            if a.tag() == "CMvAttrF" {
                if let Some(id) = a.attr("xs.id") {
                    ids.push((PARAMS, m.params.len(), id.to_owned()));
                }
                m.params.push(float_attr(&a)?);
            }
        }
        for gr in e.list("parameterGroupList")? {
            m.groups.push(ParamGroup {
                id: gr
                    .field("id")?
                    .and_then(|i| i.attr("idstr"))
                    .unwrap_or_default()
                    .to_owned(),
                guid: gr.guid("guid")?.unwrap_or_default().to_owned(),
                frame_step: match gr.field("attrFrameStep")? {
                    Some(f) if !f.is_null() => int_curve(&f)?,
                    _ => None,
                },
                shy: gr.bool("isShyMode")?.unwrap_or(false),
                freeze: gr.bool("isFreezeMode")?.unwrap_or(false),
            });
        }
    }
    if let Some(fa) = t.field("formAnimationSet")? {
        m.form.global = form_map(fa.field("formMapOnGlobal")?)?;
        m.form.local = form_map(fa.field("formMapOnLocal")?)?;
    }
    m.form.apply = true;
    if let Some(e) = t.field("formEditEffect")?.filter(|e| !e.is_null()) {
        m.form.apply = e.bool("isApplyEditResult")?.unwrap_or(true);
        m.form.local_mode = e.bool("isLocalFormAnimation")?.unwrap_or(false);
    }
    if let Some(e) = t.field("partsVisibleEffect")? {
        for a in effect_attrs(&e)? {
            if a.tag() == "CMvAttrF" {
                if let Some(id) = a.attr("xs.id") {
                    ids.push((PARTS, m.parts.len(), id.to_owned()));
                }
                m.parts.push(float_attr(&a)?);
            }
        }
    }
    if let Some(e) = t.field("eyeBlinkEffect")?.filter(|e| !e.is_null()) {
        let mut eb = EyeBlinkEffect {
            active: e.bool("isActive")?.unwrap_or(true),
            targets: id_list(&e, "effectParameterAttrIds")?,
            invert: e.bool("invert")?.unwrap_or(false),
            relative: e.bool("relative")?.unwrap_or(false),
            ..Default::default()
        };
        for a in effect_attrs(&e)? {
            let at = float_attr(&a)?;
            let slot = match at.id.as_str() {
                "eyeOpen" => 0,
                "effectLevel" => 1,
                _ => continue,
            };
            if let Some(id) = a.attr("xs.id") {
                ids.push((EYE_BLINK, slot, id.to_owned()));
            }
            if slot == 0 {
                eb.eye_open = at;
            } else {
                eb.level = at;
            }
        }
        m.eye_blink = Some(eb);
    }
    if let Some(e) = t.field("lipSyncEffect")?.filter(|e| !e.is_null()) {
        let mut ls = LipSyncEffect {
            active: e.bool("isActive")?.unwrap_or(true),
            targets: id_list(&e, "effectParameterAttrIds")?,
            sync_track: e.guid("syncTrackGuid")?.map(str::to_owned),
            invert: e.bool("isInvert")?.unwrap_or(false),
            relative: e.bool("isRelative")?.unwrap_or(false),
            ..Default::default()
        };
        for a in effect_attrs(&e)? {
            let at = float_attr(&a)?;
            let slot = match at.id.as_str() {
                "soundLevel" => 0,
                "lipSyncScale" => 1,
                "lipSyncBase" => 2,
                "lipSyncLevel" => 3,
                _ => continue,
            };
            if let Some(id) = a.attr("xs.id") {
                ids.push((LIP_SYNC, slot, id.to_owned()));
            }
            match slot {
                0 => ls.sound_level = at,
                1 => ls.scale = at,
                2 => ls.base = at,
                _ => ls.level = at,
            }
        }
        m.lip_sync = Some(ls);
    }
    Ok(m)
}
