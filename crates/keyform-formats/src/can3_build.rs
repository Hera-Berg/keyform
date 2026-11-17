//! Building `.can3` object graphs: new animations, scenes and model
//! tracks, laid out as the reference saves them (5.3.04 samples).
//! spec: 10-formats/can3-schema.md, 60-animation/animation-model.md §1–2,
//! 10-formats/xml-object-graph.md

use keyform_model::Model;

use crate::error::{FormatError, Result};
use crate::graph::{format_f32, format_f64};
use crate::xml::{Element, Pi, XmlDocument};

/// Appends shared objects to a document under fresh ids.
pub(crate) struct Graph<'x> {
    xml: &'x mut XmlDocument,
    next_id: u64,
    next_idx: u64,
}

fn num_attr(e: &Element, k: &str) -> Option<u64> {
    e.attr(k)?.trim_start_matches('#').parse().ok()
}

impl<'x> Graph<'x> {
    pub(crate) fn on(xml: &'x mut XmlDocument) -> Result<Self> {
        let sh =
            shared(xml).ok_or_else(|| FormatError::Invalid("document has no <shared>".into()))?;
        let (mut id, mut idx) = (0, 0);
        for e in &xml.root.children[sh].children {
            id = id.max(num_attr(e, "xs.id").map_or(0, |n| n + 1));
            idx = idx.max(num_attr(e, "xs.idx").map_or(0, |n| n + 1));
        }
        Ok(Self {
            xml,
            next_id: id,
            next_idx: idx,
        })
    }

    /// Reserves a shared object `<tag>` and returns its id.
    fn reserve(&mut self, tag: &str) -> String {
        let id = format!("#{}", self.next_id);
        self.next_id += 1;
        let e = Element::new(tag)
            .with_attr("xs.id", &id)
            .with_attr("xs.idx", self.next_idx.to_string());
        self.next_idx += 1;
        let sh = shared(self.xml).expect("shared");
        self.xml.root.children[sh].children.push(e);
        id
    }

    /// Fills the reserved object `id` (children and extra attributes).
    fn fill(&mut self, id: &str, children: Vec<Element>, attrs: &[(&str, String)]) {
        let sh = shared(self.xml).expect("shared");
        if let Some(e) = self.xml.root.children[sh]
            .children
            .iter_mut()
            .find(|c| c.attr("xs.id") == Some(id))
        {
            e.children = children;
            for (k, v) in attrs {
                e.set_attr(k, v.clone());
            }
        }
    }

    fn add(&mut self, tag: &str, children: Vec<Element>, attrs: &[(&str, String)]) -> String {
        let id = self.reserve(tag);
        self.fill(&id, children, attrs);
        id
    }

    fn guid(&mut self, tag: &str, note: &str) -> String {
        let id = self.reserve(tag);
        let sh = shared(self.xml).expect("shared");
        if let Some(e) = self.xml.root.children[sh]
            .children
            .iter_mut()
            .find(|c| c.attr("xs.id") == Some(id.as_str()))
        {
            e.attrs
                .insert(0, ("uuid".into(), keyform_model::new_guid()));
            e.attrs.insert(1, ("note".into(), note.into()));
        }
        id
    }

    /// The shared GUID object of `uuid` (tag `tag`), creating one.
    fn guid_of(&mut self, tag: &str, uuid: &str, note: &str) -> String {
        let sh = shared(self.xml).expect("shared");
        if let Some(id) = self.xml.root.children[sh]
            .children
            .iter()
            .find(|c| c.name == tag && c.attr("uuid") == Some(uuid))
            .and_then(|c| c.attr("xs.id"))
        {
            return id.to_owned();
        }
        let id = self.reserve(tag);
        let sh = shared(self.xml).expect("shared");
        if let Some(e) = self.xml.root.children[sh]
            .children
            .iter_mut()
            .find(|c| c.attr("xs.id") == Some(id.as_str()))
        {
            e.attrs.insert(0, ("uuid".into(), uuid.into()));
            e.attrs.insert(1, ("note".into(), note.into()));
        }
        id
    }
}

fn shared(xml: &XmlDocument) -> Option<usize> {
    xml.root.children.iter().position(|c| c.name == "shared")
}

// ------------------------------------------------------------ elements

fn named(tag: &str, name: &str) -> Element {
    Element::new(tag).with_attr("xs.n", name)
}
fn text(tag: &str, name: &str, t: String) -> Element {
    let mut e = named(tag, name);
    e.text = t;
    e
}
fn s(name: &str, v: &str) -> Element {
    text("s", name, v.to_owned())
}
fn i(name: &str, v: i64) -> Element {
    text("i", name, v.to_string())
}
fn d(name: &str, v: f64) -> Element {
    text("d", name, format_f64(v))
}
fn f(name: &str, v: f32) -> Element {
    text("f", name, format_f32(v))
}
fn b(name: &str, v: bool) -> Element {
    text("b", name, v.to_string())
}
fn null(name: &str) -> Element {
    named("null", name)
}
fn r(tag: &str, name: &str, id: &str) -> Element {
    named(tag, name).with_attr("xs.ref", id)
}
fn item(tag: &str, id: &str) -> Element {
    Element::new(tag).with_attr("xs.ref", id)
}
fn list(tag: &str, name: &str, items: Vec<Element>) -> Element {
    let mut l = named(tag, name).with_attr("count", items.len().to_string());
    l.children = items;
    l
}
fn typed_array(name: &str, ty: &str, items: Vec<Element>) -> Element {
    let mut l = named("array", name)
        .with_attr("count", items.len().to_string())
        .with_attr("type", ty);
    l.children = items;
    l
}
fn string_map(name: &str, items: Vec<Element>) -> Element {
    let mut m = named("hash_map", name)
        .with_attr("count", items.len().to_string())
        .with_attr("keyType", "string");
    m.children = items;
    m
}
fn entry_map(name: &str, entries: Vec<(Element, Element)>) -> Element {
    let mut m = named("hash_map", name).with_attr("count", entries.len().to_string());
    if entries.is_empty() {
        m.set_attr("keyType", "string");
    }
    for (k, v) in entries {
        let mut e = Element::new("entry");
        e.children = vec![k, v];
        m.children.push(e);
    }
    m
}
fn vec2(name: &str, x: f32, y: f32) -> Element {
    let mut e = named("GVector2", name);
    e.children = vec![f("x", x), f("y", y)];
    e
}
fn rect(name: &str, w: f32, h: f32) -> Element {
    let mut e = named("GRectF", name);
    e.children = vec![f("x", 0.0), f("y", 0.0), f("width", w), f("height", h)];
    e
}
fn attr_id(name: &str, id: &str) -> Element {
    named("CAttrId", name).with_attr("idstr", id)
}

/// The common attribute part (`ICMvAttr`).
#[allow(clippy::too_many_arguments)]
fn attr_super(
    id: &str,
    name: &str,
    guid: Element,
    adapt: &str,
    opts: Vec<Element>,
    track_tag: &str,
    track: &str,
) -> Element {
    let mut e = named("ICMvAttr", "super");
    e.children = vec![
        b("isShyMode", false),
        b("isFreezeMode", false),
        attr_id("id", id),
        s("name", name),
        guid,
        b("isActive", true),
        r("AdaptType", "adaptType", adapt),
        string_map("optionParam", opts),
        r(track_tag, "track", track),
    ];
    e
}

/// Settings of a new float attribute.
struct FloatAttr<'a> {
    id: &'a str,
    name: &'a str,
    guid: Element,
    adapt: &'a str,
    opts: Vec<Element>,
    base: f64,
    range: (f64, f64),
    repeat: bool,
    fixed: bool,
}

/// A new `CMvAttrF` with an empty curve at its base value.
fn float_attr(g: &mut Graph, a: FloatAttr, track_tag: &str, track: &str) -> String {
    let id = g.reserve("CMvAttrF");
    let (lo, hi) = a.range;
    let shown = |v: f64, alt: f64| {
        if v.is_finite() && v.abs() < 1e300 {
            v
        } else {
            alt
        }
    };
    let value = if a.fixed {
        let mut v = named("CFixedSequence", "valueData");
        v.children = vec![
            d("baseValue", a.base),
            d("curMax", a.base),
            d("curMin", a.base),
            r("CMvAttrF", "attr", &id),
        ];
        v
    } else {
        let mut sup = named("ACValueSequence", "super");
        sup.children = vec![
            d("curMin", shown(lo, -100.0)),
            d("curMax", shown(hi, 100.0)),
            i("posStart", 0),
            named("int-array", "keyPts2").with_attr("count", "0"),
            i("keyMin", 0),
            i("keyMax", 0),
            d("lastValue", a.base),
            i("lastPos", 0),
            r("CMvAttrF", "attr", &id),
            d("baseValue", a.base),
        ];
        let mut v = named("CMutableSequence", "valueData");
        v.children = vec![
            sup,
            typed_array("points", "CBezierPt", Vec::new()),
            list("carray_list", "curveTypes", Vec::new()),
        ];
        v
    };
    let children = vec![
        attr_super(a.id, a.name, a.guid, a.adapt, a.opts, track_tag, track),
        value,
        d("rangeMin", lo),
        d("rangeMax", hi),
        b("isRepeat", a.repeat),
        d("repeatMin", if a.repeat { lo } else { f64::MIN }),
        d("repeatMax", if a.repeat { hi } else { f64::MAX }),
        null("linked_keyFormsForObject"),
    ];
    g.fill(&id, children, &[]);
    id
}

/// A new `CMvAttrPt` (position-like) holding one point.
fn point_attr(
    g: &mut Graph,
    id_s: &str,
    name: &str,
    adapt: &str,
    xy: (f32, f32),
    track_tag: &str,
    track: &str,
) -> String {
    let id = g.reserve("CMvAttrPt");
    let mut pt = Element::new("CPtTNS");
    pt.children = vec![
        vec2("super", xy.0, xy.1),
        b("isCorner", false),
        vec2("value", xy.0, xy.1),
        i("pos", 0),
    ];
    let mut seq = named("CXY_TNSSequence", "valueDataXY");
    seq.children = vec![
        typed_array("points", "CPtTNS", vec![pt]),
        vec2("basePt", 0.0, 0.0),
        r("CMvAttrPt", "attr", &id),
    ];
    g.fill(
        &id,
        vec![
            attr_super(
                id_s,
                name,
                null("guid"),
                adapt,
                Vec::new(),
                track_tag,
                track,
            ),
            seq,
        ],
        &[],
    );
    id
}

/// A new `CMvAttrI` (frame step).
fn int_attr(
    g: &mut Graph,
    id_s: &str,
    name: &str,
    adapt: &str,
    base: i32,
    track_tag: &str,
    track: &str,
) -> String {
    let id = g.reserve("CMvAttrI");
    let mut sup = named("ACValueSequence", "super");
    sup.children = vec![
        d("curMin", 0.0),
        d("curMax", 0.0),
        i("posStart", 0),
        named("int-array", "keyPts2").with_attr("count", "0"),
        i("keyMin", i64::from(i32::MAX)),
        i("keyMax", i64::from(i32::MIN)),
        d("lastValue", f64::NAN),
        i("lastPos", -1),
        r("CMvAttrI", "attr", &id),
        d("baseValue", f64::from(base)),
    ];
    let mut v = named("CIntSequence", "valueData");
    v.children = vec![sup, typed_array("points", "CSeqPt", Vec::new())];
    g.fill(
        &id,
        vec![
            attr_super(
                id_s,
                name,
                null("guid"),
                adapt,
                Vec::new(),
                track_tag,
                track,
            ),
            v,
            i("rangeMin", 0),
            i("rangeMax", 100),
        ],
        &[],
    );
    id
}

/// The `ICMvEffect` part of an effect.
fn effect_super(
    effect_id: &str,
    can_delete: bool,
    attrs: &[(String, String, String)],
    track_tag: &str,
    track: &str,
) -> Element {
    // attrs: (element tag, shared id, attribute ID)
    let mut e = named("ICMvEffect", "super");
    e.children = vec![
        r("CEffectId", "id", effect_id),
        b("isActive", true),
        b("canDelete", can_delete),
        typed_array(
            "attrList",
            "ICMvAttr",
            attrs.iter().map(|(t, id, _)| item(t, id)).collect(),
        ),
        entry_map(
            "attrMap",
            attrs
                .iter()
                .map(|(t, id, a)| (attr_id("key", a), r(t, "value", id)))
                .collect(),
        ),
        r(track_tag, "track", track),
    ];
    e
}

/// Shared ids of the per-document objects effects refer to.
struct Common {
    relative: String,
    absolute: String,
    visual: String,
    eye_blink: String,
    lip_sync: String,
    params: String,
    parts: String,
}

fn common(g: &mut Graph) -> Common {
    let adapt = |g: &mut Graph, t: &str| g.add("AdaptType", vec![s("text", t)], &[]);
    let eid = |g: &mut Graph, t: &str| {
        let id = g.reserve("CEffectId");
        let sh = shared(g.xml).expect("shared");
        if let Some(e) = g.xml.root.children[sh]
            .children
            .iter_mut()
            .find(|c| c.attr("xs.id") == Some(id.as_str()))
        {
            e.attrs.insert(0, ("idstr".into(), t.into()));
        }
        id
    };
    Common {
        relative: adapt(g, "Relative"),
        absolute: adapt(g, "Absolute"),
        visual: eid(g, "VisualDefault"),
        eye_blink: eid(g, "Effects:EyeBlink"),
        lip_sync: eid(g, "Effects:LipSync"),
        params: eid(g, "Effects:Live2DParam"),
        parts: eid(g, "live2DPartsOpacity:"),
    }
}

/// Builds a Live2D model track for `model` (linked as resource
/// `resource`) in scene `scene` under the root group `parent`. Returns
/// (track id, track GUID object id). spec: animation-model.md §2
#[allow(clippy::too_many_arguments)]
fn model_track(
    g: &mut Graph,
    c: &Common,
    model: &Model,
    name: &str,
    scene: &str,
    parent_guid: &str,
    resource: &str,
    frames: i32,
    scene_size: (i32, i32),
) -> (String, String) {
    const T: &str = "CMvTrack_Live2DModel_Source";
    let guid = g.guid("CTrackGuid", &format!("{name} CMvTrack_Live2DModel_Source"));
    let track = g.reserve(T);
    // Visual effect: the model centred in the scene, scaled to fit.
    let (mw, mh) = (model.canvas.width as f32, model.canvas.height as f32);
    let (sw, sh) = (scene_size.0 as f32, scene_size.1 as f32);
    let scale = if mw > 0.0 && mh > 0.0 {
        (sw / mw).min(sh / mh) * 100.0
    } else {
        100.0
    };
    let xy = point_attr(
        g,
        "xy",
        "Position",
        &c.relative,
        (sw / 2.0, sh / 2.0),
        T,
        &track,
    );
    let fa = |g: &mut Graph, id: &str, name: &str, base: f64, range: (f64, f64)| {
        float_attr(
            g,
            FloatAttr {
                id,
                name,
                guid: null("guid"),
                adapt: &c.relative,
                opts: Vec::new(),
                base,
                range,
                repeat: false,
                fixed: false,
            },
            T,
            &track,
        )
    };
    let unbounded = (f64::MIN, f64::MAX);
    let sx = fa(g, "scalex", "Scale X", f64::from(scale), unbounded);
    let sy = fa(g, "scaley", "Scale Y", f64::from(scale), unbounded);
    let rot = fa(g, "rotate", "Rotation (degrees)", 0.0, unbounded);
    let shear = fa(g, "shear", "Shear", 0.0, unbounded);
    let anchor = point_attr(
        g,
        "anchor",
        "Anchor Point",
        &c.relative,
        (mw / 2.0, mh / 2.0),
        T,
        &track,
    );
    let op = fa(g, "opacity", "Opacity", 100.0, (0.0, 100.0));
    let fs = int_attr(g, "frameStep", "Frame Step", &c.relative, 1, T, &track);
    let apw = fa(
        g,
        "artPathWidth",
        "ArtPath line-width magnification",
        100.0,
        unbounded,
    );
    let vattrs: Vec<(String, String, String)> = [
        ("CMvAttrPt", &xy, "xy"),
        ("CMvAttrF", &sx, "scalex"),
        ("CMvAttrF", &sy, "scaley"),
        ("CMvAttrF", &rot, "rotate"),
        ("CMvAttrF", &shear, "shear"),
        ("CMvAttrPt", &anchor, "anchor"),
        ("CMvAttrF", &op, "opacity"),
        ("CMvAttrI", &fs, "frameStep"),
        ("CMvAttrF", &apw, "artPathWidth"),
    ]
    .iter()
    .map(|(t, id, a)| (t.to_string(), id.to_string(), a.to_string()))
    .collect();
    let visual = g.add(
        "CMvEffect_VisualDefault",
        vec![
            effect_super(&c.visual, false, &vattrs, T, &track),
            r("CMvAttrPt", "attrXY", &xy),
            r("CMvAttrF", "attrScaleX", &sx),
            r("CMvAttrF", "attrScaleY", &sy),
            r("CMvAttrF", "attrRotate", &rot),
            r("CMvAttrPt", "attrAnchorXY", &anchor),
            r("CMvAttrF", "attrShear", &shear),
            r("CMvAttrF", "attrOpacity", &op),
            r("CMvAttrI", "attrFrameStep", &fs),
            r("CMvAttrF", "attrArtPathWidth", &apw),
        ],
        &[],
    );
    // Eye blink and lip sync on the model's parameter lists.
    let ids = |list: &[usize]| -> Vec<Element> {
        list.iter()
            .filter_map(|&pi| model.parameters.get(pi))
            .map(|p| Element::new("CAttrId").with_attr("idstr", format!("live2dParam_{}", p.id)))
            .collect()
    };
    let rel = |g: &mut Graph, id: &str, name: &str, base: f64, range: (f64, f64), fixed: bool| {
        float_attr(
            g,
            FloatAttr {
                id,
                name,
                guid: null("guid"),
                adapt: &c.relative,
                opts: Vec::new(),
                base,
                range,
                repeat: false,
                fixed,
            },
            T,
            &track,
        )
    };
    let eye_open = rel(g, "eyeOpen", "Blink", 1.0, (0.0, 1.0), false);
    let eye_level = rel(g, "effectLevel", "Effect", 100.0, (0.0, 100.0), false);
    let ea = vec![
        (
            "CMvAttrF".to_string(),
            eye_open.clone(),
            "eyeOpen".to_string(),
        ),
        (
            "CMvAttrF".to_string(),
            eye_level.clone(),
            "effectLevel".to_string(),
        ),
    ];
    let eye = g.add(
        "CMvEffect_EyeBlink",
        vec![
            effect_super(&c.eye_blink, true, &ea, T, &track),
            list(
                "carray_list",
                "effectParameterAttrIds",
                ids(&model.eye_blink_parameters),
            ),
            b("invert", false),
            b("relative", true),
        ],
        &[],
    );
    let sl = rel(g, "soundLevel", "Volume", 0.0, (0.0, 1.0), false);
    let sc = rel(g, "lipSyncScale", "Scale", 2.0, (-4.0, 4.0), true);
    let bs = rel(g, "lipSyncBase", "Base Value", 0.0, (0.0, 1.0), true);
    let lv = rel(g, "lipSyncLevel", "Effect", 100.0, (0.0, 100.0), false);
    let la: Vec<(String, String, String)> = [
        (&sl, "soundLevel"),
        (&sc, "lipSyncScale"),
        (&bs, "lipSyncBase"),
        (&lv, "lipSyncLevel"),
    ]
    .iter()
    .map(|(id, a)| ("CMvAttrF".to_string(), id.to_string(), a.to_string()))
    .collect();
    let lip = g.add(
        "CMvEffect_LipSync",
        vec![
            effect_super(&c.lip_sync, true, &la, T, &track),
            list(
                "carray_list",
                "effectParameterAttrIds",
                ids(&model.lip_sync_parameters),
            ),
            null("syncTrackGuid"),
            b("isInvert", false),
            b("isRelative", true),
        ],
        &[],
    );
    // Parameters, in model order.
    let mut pattrs = Vec::new();
    for p in &model.parameters {
        let pg = g.guid_of("CParameterGuid", &p.guid, &p.id);
        let aid = format!("live2dParam_{}", p.id);
        let name = if p.name.is_empty() {
            p.id.clone()
        } else {
            p.name.clone()
        };
        let id = float_attr(
            g,
            FloatAttr {
                id: &aid,
                name: &name,
                guid: r("CParameterGuid", "guid", &pg),
                adapt: &c.absolute,
                opts: vec![i("KEY_ATTR_FADE_OUT", -1), i("KEY_ATTR_FADE_IN", -1)],
                base: f64::from(p.default),
                range: (f64::from(p.min), f64::from(p.max)),
                repeat: p.repeat,
                fixed: false,
            },
            T,
            &track,
        );
        pattrs.push(("CMvAttrF".to_string(), id, aid));
    }
    let mut groups = Vec::new();
    for gr in &model.parameter_groups {
        let gg = g.guid_of("CParameterGroupGuid", &gr.guid, &gr.id);
        let mut e = Element::new("CMvParameter_Group");
        e.children = vec![
            r("CParameterGroupGuid", "guid", &gg),
            b("isShyMode", false),
            b("isFreezeMode", false),
            named("CMvParameterGroupId", "id").with_attr("idstr", &gr.id),
            null("attrFrameStep"),
        ];
        groups.push(e);
    }
    let params = g.add(
        "CMvEffect_Live2DParameter",
        vec![
            effect_super(&c.params, false, &pattrs, T, &track),
            list("carray_list", "parameterGroupList", groups),
        ],
        &[],
    );
    // Part opacities.
    let mut qattrs = Vec::new();
    for p in &model.parts {
        let pg = g.guid_of("CPartGuid", &p.info.guid, &p.info.id);
        let aid = format!("live2DPartsOpacity_{}", p.info.id);
        let id = float_attr(
            g,
            FloatAttr {
                id: &aid,
                name: p.info.display_name(),
                guid: r("CPartGuid", "guid", &pg),
                adapt: &c.absolute,
                opts: vec![s("KEY_PARTS_VISIBLE_ID", &p.info.id)],
                base: 1.0,
                range: (0.0, 1.0),
                repeat: false,
                fixed: false,
            },
            T,
            &track,
        );
        qattrs.push(("CMvAttrF".to_string(), id, aid));
    }
    let parts = g.add(
        "CMvEffect_Live2DPartsVisible",
        vec![effect_super(&c.parts, false, &qattrs, T, &track)],
        &[],
    );
    // The track.
    let mut src = track_source(
        name,
        &guid,
        frames,
        T,
        &track,
        Some(&visual),
        Some(parent_guid),
        scene,
    );
    src.children[16] = {
        let mut m = named("CMvEffectManager", "effectManager");
        m.children = vec![typed_array(
            "effectList",
            "ICMvEffect",
            vec![
                item("CMvEffect_EyeBlink", &eye),
                item("CMvEffect_LipSync", &lip),
                item("CMvEffect_Live2DParameter", &params),
                item("CMvEffect_Live2DPartsVisible", &parts),
                item("CMvEffect_VisualDefault", &visual),
            ],
        )];
        m
    };
    let mut linked = named("ICMvTrack_Linked", "super");
    linked.children = vec![src, r("CResourceGuid", "_resourceGuid", resource)];
    let mut fas = named("FormAnimationSet", "formAnimationSet");
    fas.children = vec![
        string_map("formMapOnGlobal", Vec::new()),
        string_map("formMapOnLocal", Vec::new()),
        r(T, "trackSource", &track),
    ];
    let mut labels = named("ParameterBookmarkLabelSet", "parameterBookmarkLabelSet");
    labels.children = vec![list("carray_list", "labels", Vec::new())];
    g.fill(
        &track,
        vec![
            linked,
            r("CMvEffect_Live2DParameter", "keyParamEffect", &params),
            r("CMvEffect_Live2DPartsVisible", "partsVisibleEffect", &parts),
            r("CMvEffect_LipSync", "lipSyncEffect", &lip),
            r("CMvEffect_EyeBlink", "eyeBlinkEffect", &eye),
            null("formEditEffect"),
            fas,
            rect("bounds", mw, mh),
            labels,
        ],
        &[],
    );
    (track, guid)
}

/// The `ICMvTrack_Source` part of a track, in the reference's field order
/// (index 16 = effectManager).
#[allow(clippy::too_many_arguments)]
fn track_source(
    name: &str,
    guid: &str,
    frames: i32,
    tag: &str,
    me: &str,
    visual: Option<&str>,
    parent: Option<&str>,
    scene: &str,
) -> Element {
    let mut vh = named("CVisualHandler", "visualHandler");
    vh.children = vec![r(tag, "track", me)];
    let mut sh = named("CSoundHandler", "soundHandler");
    sh.children = vec![r(tag, "track", me)];
    let mut em = named("CMvEffectManager", "effectManager");
    em.children = vec![typed_array("effectList", "ICMvEffect", Vec::new())];
    let mut e = named("ICMvTrack_Source", "super");
    e.children = vec![
        s("name", name),
        b("isUserRenamed", false),
        r("CTrackGuid", "guid", guid),
        i("start", 0),
        i("internalOffset", 0),
        i("duration", i64::from(frames)),
        b("editable", true),
        b("visible", true),
        b("mute", false),
        b("isGuide", false),
        b("isRepeat", false),
        b("soloSwitch", false),
        vh,
        sh,
        null("soundEffect"),
        match visual {
            Some(v) => r("CMvEffect_VisualDefault", "visualEffect", v),
            None => null("visualEffect"),
        },
        em,
        match parent {
            Some(p) => r("CTrackGuid", "parentGuid", p),
            None => null("parentGuid"),
        },
        r("CSceneSource", "_sceneSource", scene),
        string_map("userData", Vec::new()),
        null("keys"),
    ];
    e
}

/// Builds a scene with a root group and one model track. Returns the
/// scene id. spec: animation-model.md §1
#[allow(clippy::too_many_arguments)]
fn scene(
    g: &mut Graph,
    c: &Common,
    anim: &str,
    name: &str,
    model: &Model,
    model_name: &str,
    resource: &str,
    frames: i32,
    fps: f64,
    size: (i32, i32),
) -> String {
    let sg = g.guid("CSceneGuid", "(no debug info)");
    let scene = g.reserve("CSceneSource");
    let rg = g.guid("CTrackGuid", "Root CMvTrack_Group_Source");
    let root = g.reserve("CMvTrack_Group_Source");
    let (track, tg) = model_track(g, c, model, model_name, &scene, &rg, resource, frames, size);
    let rsrc = track_source(
        "Root",
        &rg,
        frames,
        "CMvTrack_Group_Source",
        &root,
        None,
        None,
        &scene,
    );
    g.fill(
        &root,
        vec![
            rsrc,
            list(
                "carray_list",
                "_childTrackGuids",
                vec![item("CTrackGuid", &tg)],
            ),
            rect("bounds", size.0 as f32, size.1 as f32),
        ],
        &[],
    );
    let mut canvas = named("CImageCanvas", "canvas");
    canvas.children = vec![
        i("pixelWidth", i64::from(size.0)),
        i("pixelHeight", i64::from(size.1)),
        named("CColor", "background"),
    ];
    let mut tss = named("CTrackSourceSet", "trackSourceSet");
    tss.children = vec![list(
        "carray_list",
        "_sources",
        vec![
            item("CMvTrack_Group_Source", &root),
            item("CMvTrack_Live2DModel_Source", &track),
        ],
    )];
    let mut mi = named("CMvMovieInfo", "movieInfo");
    mi.children = vec![
        i("width", i64::from(size.0)),
        i("height", i64::from(size.1)),
        i("duration", i64::from(frames)),
        d("fps", fps),
        i("workspaceStart", 0),
        i("workspaceEnd", i64::from(frames - 1)),
        named("CColor", "background"),
        i("fadeInMSec", -1),
        i("fadeOutMSec", -1),
        b("isBezierRestricted", false),
        b("isLoopMotion", false),
        i("startFrame", 0),
        named("CFrameIndexType", "frameIndexType").with_attr("v", "ZERO_INDEX"),
    ];
    let mut bookmarks = named(
        "ParameterBookmarkLabelCarrierTrackSet",
        "parameterBookmarkCarrierTrackSet",
    );
    bookmarks.children = vec![
        r("CSceneSource", "_owner", &scene),
        list("carray_list", "_trackSortInfo", Vec::new()),
    ];
    g.fill(
        &scene,
        vec![
            s("sceneName", name),
            canvas,
            r("CSceneGuid", "guid", &sg),
            s("tag", ""),
            tss,
            r("CMvTrack_Group_Source", "rootTrack", &root),
            mi,
            r("CAnimation", "_animation", anim),
            string_map("marker", Vec::new()),
            named("CCurveType", "defaultParameterCurveType").with_attr("v", "SMOOTH"),
            named("CCurveType", "defaultPartCurveType").with_attr("v", "STEP"),
            b("fixAspect", true),
            named("Animation", "targetVersion").with_attr("v", "FOR_SDK"),
            b("lockMarker", false),
            list("array_list", "onionSkinMarker", Vec::new()),
            b("lockOnionSkinMarker", false),
            bookmarks,
        ],
        &[("exportMotionFile", "true".into())],
    );
    scene
}

/// The CAnimation object (shared id) and its `_scenes` list.
fn animation_obj(xml: &XmlDocument) -> Option<(usize, usize)> {
    let sh = shared(xml)?;
    let main = xml.root.children.iter().find(|c| c.name == "main")?;
    let id = main.children.first()?.attr("xs.ref")?;
    let k = xml.root.children[sh]
        .children
        .iter()
        .position(|c| c.attr("xs.id") == Some(id))?;
    Some((sh, k))
}

/// The first linked model resource's GUID object id.
fn first_resource(xml: &XmlDocument) -> Option<String> {
    let sh = shared(xml)?;
    xml.root.children[sh]
        .children
        .iter()
        .find(|c| c.name == "CResourceData")
        .and_then(|c| {
            let mut st = vec![c];
            while let Some(e) = st.pop() {
                if e.name == "CResourceGuid" && e.field_name() == Some("guid") {
                    return e.attr("xs.ref").map(str::to_owned);
                }
                st.extend(e.children.iter());
            }
            None
        })
}

/// Adds the `<?import?>` / `<?version?>` markers the document's types
/// need (versions first, imports sorted).
pub(crate) fn fix_markers(xml: &mut XmlDocument) {
    let mut tags = std::collections::BTreeSet::new();
    let mut st = vec![&xml.root];
    while let Some(e) = st.pop() {
        tags.insert(e.name.clone());
        // Arrays name their element class too (`type="CBezierPt"`), even
        // when empty.
        if let Some(t) = e.attr("type") {
            tags.insert(t.to_owned());
        }
        st.extend(e.children.iter());
    }
    for t in &tags {
        if let Some(v) = crate::can3_schema::version_of(t) {
            let ty = v.split(':').next().unwrap_or(v);
            if !xml
                .pis
                .iter()
                .any(|p| p.target == "version" && p.data.split(':').next() == Some(ty))
            {
                let at = xml
                    .pis
                    .iter()
                    .rposition(|p| p.target == "version")
                    .map_or(0, |i| i + 1);
                xml.pis.insert(
                    at,
                    Pi {
                        target: "version".into(),
                        data: v.into(),
                    },
                );
            }
        }
        if let Some(imp) = crate::can3_schema::import_of(t) {
            if !xml
                .pis
                .iter()
                .any(|p| p.target == "import" && p.data == imp)
            {
                let at = xml
                    .pis
                    .iter()
                    .position(|p| p.target == "import" && p.data.as_str() > imp)
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
                        data: imp.into(),
                    },
                );
            }
        }
    }
}

/// A new animation document linking `model` (file `model_path`) with one
/// scene `scene_name` of `frames` frames at `fps`, sized like the model
/// canvas scaled to fit 1280 px. spec: animation-model.md §1
pub fn new_animation_xml(
    name: &str,
    file: &str,
    model: &Model,
    model_path: &str,
    scene_name: &str,
    frames: i32,
    fps: f64,
) -> Result<XmlDocument> {
    let mut xml =
        XmlDocument::new(Element::new("root").with_attr("fileFormatVersion", "402030000"));
    xml.root.children = vec![Element::new("shared"), Element::new("main")];
    xml.pis.push(Pi {
        target: "version".into(),
        data: "SerializeFormatVersion:2".into(),
    });
    let model_name = std::path::Path::new(model_path).file_name().map_or_else(
        || model_path.to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut g = Graph::on(&mut xml)?;
    let c = common(&mut g);
    let anim = g.reserve("CAnimation");
    let rm = g.reserve("CResourceManager");
    let rgg = g.guid("CResourceGroupGuid", "Root Resource Group");
    let res = g.guid("CResourceGuid", &model_name);
    let mut rf = named("ACResource_File", "super");
    rf.children = vec![
        text("file", "srcFile", model_path.to_owned()),
        r("CResourceGuid", "guid", &res),
        s("name", &model_name),
    ];
    let mut link = named("CResource_Linked_Model", "resourceRef");
    link.children = vec![rf];
    let mut entry = named("ACResourceEntry", "super");
    entry.children = vec![
        r("CResourceGroupGuid", "parentGuid", &rgg),
        r("CResourceManager", "_resourceManager", &rm),
    ];
    let rdata = g.add("CResourceData", vec![entry, null("customName"), link], &[]);
    let size = {
        let (w, h) = (
            model.canvas.width.max(1) as f64,
            model.canvas.height.max(1) as f64,
        );
        let k = (1280.0 / w.max(h)).min(1.0);
        (
            ((w * k).round() as i32).max(16),
            ((h * k).round() as i32).max(16),
        )
    };
    let mn = model_name.trim_end_matches(".cmo3").to_owned();
    let sc = scene(
        &mut g, &c, &anim, scene_name, model, &mn, &res, frames, fps, size,
    );
    let mut root_group = named("CResourceGroup", "rootGroup");
    let mut rge = named("ACResourceEntry", "super");
    rge.children = vec![
        null("parentGuid"),
        r("CResourceManager", "_resourceManager", &rm),
    ];
    root_group.children = vec![
        rge,
        r("CResourceGroupGuid", "guid", &rgg),
        list(
            "carray_list",
            "_childGuids",
            vec![item("CResourceGuid", &res)],
        ),
        s("name", "Resources"),
    ];
    g.fill(
        &rm,
        vec![
            root_group,
            list(
                "carray_list",
                "_resourceRefList",
                vec![item("CResourceData", &rdata)],
            ),
            entry_map(
                "resourceGuidMap",
                vec![(
                    r("CResourceGuid", "key", &res),
                    r("CResourceData", "value", &rdata),
                )],
            ),
            list("carray_list", "_resourceGroupList", Vec::new()),
            string_map("resourceGroupGuidMap", Vec::new()),
            r("CSceneSource", "_sceneSource", &sc),
        ],
        &[],
    );
    let mut ed = named("EditorEdition", "editorEdition");
    ed.children = vec![i("edition", 15)];
    let mut blend = named("CSceneBlendingSettingsSource", "sceneBlendingSettings");
    blend.children = vec![
        named("CSceneBlendingSettingsGuid", "guid")
            .with_attr("uuid", keyform_model::new_guid())
            .with_attr("note", "(no debug info)"),
        list("carray_list", "playlists", Vec::new()),
    ];
    g.fill(
        &anim,
        vec![
            s("name", name),
            text("file", "file", file.to_owned()),
            list("carray_list", "_scenes", vec![item("CSceneSource", &sc)]),
            r("CSceneSource", "currentScene", &sc),
            r("CResourceManager", "resourceManager", &rm),
            ed,
            blend,
            b("hideAbsolutePathIfLinkError", false),
            named("Animation", "targetVersion").with_attr("v", "FOR_SDK"),
            b("isSmoothCurveLegacyMode", false),
        ],
        &[],
    );
    if let Some(m) = xml.root.children.iter_mut().find(|c| c.name == "main") {
        m.children
            .push(Element::new("CAnimation").with_attr("xs.ref", &anim));
    }
    fix_markers(&mut xml);
    Ok(xml)
}

/// Adds a scene with a model track for `model` (the document's first
/// linked model resource) to an animation document.
pub fn add_scene_xml(
    xml: &mut XmlDocument,
    name: &str,
    model: &Model,
    frames: i32,
    fps: f64,
    size: (i32, i32),
) -> Result<()> {
    let res = first_resource(xml)
        .ok_or_else(|| FormatError::Unsupported("the animation links no model".into()))?;
    let (sh, k) = animation_obj(xml).ok_or_else(|| FormatError::Invalid("no CAnimation".into()))?;
    let anim = xml.root.children[sh].children[k]
        .attr("xs.id")
        .unwrap_or_default()
        .to_owned();
    let mn = {
        // The resource's name, without the extension.
        let mut n = String::new();
        let mut st = vec![&xml.root.children[sh]];
        while let Some(e) = st.pop() {
            if e.name == "ACResource_File" {
                if let Some(x) = e.child_field("name") {
                    n = x.text.trim_end_matches(".cmo3").to_owned();
                }
            }
            st.extend(e.children.iter());
        }
        n
    };
    let mut g = Graph::on(xml)?;
    let c = common(&mut g);
    let sc = scene(&mut g, &c, &anim, name, model, &mn, &res, frames, fps, size);
    let (sh, k) = animation_obj(xml).ok_or_else(|| FormatError::Invalid("no CAnimation".into()))?;
    let a = &mut xml.root.children[sh].children[k];
    if let Some(l) = a
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("_scenes"))
    {
        l.children.push(item("CSceneSource", &sc));
        l.set_attr("count", l.children.len().to_string());
    }
    fix_markers(xml);
    Ok(())
}

/// Removes scene `index` from the animation's scene list (its objects
/// stay unreferenced and are dropped by the next structural save).
pub fn delete_scene_xml(xml: &mut XmlDocument, index: usize) -> Result<()> {
    let (sh, k) = animation_obj(xml).ok_or_else(|| FormatError::Invalid("no CAnimation".into()))?;
    let a = &mut xml.root.children[sh].children[k];
    let l = a
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("_scenes"))
        .ok_or_else(|| FormatError::Invalid("no _scenes".into()))?;
    if index >= l.children.len() || l.children.len() <= 1 {
        return Err(FormatError::Unsupported(
            "an animation keeps at least one scene".into(),
        ));
    }
    let gone = l.children.remove(index);
    l.set_attr("count", l.children.len().to_string());
    let first = l.children[0].attr("xs.ref").map(str::to_owned);
    // currentScene must stay valid.
    if let (Some(cur), Some(first)) = (
        a.children
            .iter_mut()
            .find(|c| c.field_name() == Some("currentScene")),
        first,
    ) {
        if cur.attr("xs.ref") == gone.attr("xs.ref") {
            cur.set_attr("xs.ref", first);
        }
    }
    Ok(())
}

/// Renames scene `index`.
pub fn rename_scene_xml(xml: &mut XmlDocument, index: usize, name: &str) -> Result<()> {
    let (sh, k) = animation_obj(xml).ok_or_else(|| FormatError::Invalid("no CAnimation".into()))?;
    let id = xml.root.children[sh].children[k]
        .children
        .iter()
        .find(|c| c.field_name() == Some("_scenes"))
        .and_then(|l| l.children.get(index))
        .and_then(|e| e.attr("xs.ref"))
        .map(str::to_owned)
        .ok_or_else(|| FormatError::Invalid(format!("no scene {index}")))?;
    let sc = xml.root.children[sh]
        .children
        .iter_mut()
        .find(|c| c.attr("xs.id") == Some(id.as_str()))
        .ok_or_else(|| FormatError::Invalid("scene not found".into()))?;
    if let Some(n) = sc
        .children
        .iter_mut()
        .find(|c| c.field_name() == Some("sceneName"))
    {
        n.text = name.to_owned();
    }
    Ok(())
}
