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

/// A loaded `.can3`.
pub struct Can3Document {
    pub archive: CaffArchive,
    pub xml: XmlDocument,
    pub animation: Animation,
    pub warnings: Vec<String>,
}

impl Can3Document {
    pub fn read(data: &[u8]) -> Result<Self> {
        let archive = CaffArchive::read(data)?;
        let xml = XmlDocument::parse(
            &archive
                .main_xml()
                .ok_or_else(|| FormatError::Invalid("archive has no main.xml".into()))?
                .data,
        )?;
        let mut warnings = Vec::new();
        let animation = load_animation(&xml, &mut warnings)?;
        Ok(Self {
            archive,
            xml,
            animation,
            warnings,
        })
    }
}

/// Reads the typed animation from a parsed `.can3` document.
pub fn load_animation(xml: &XmlDocument, warnings: &mut Vec<String>) -> Result<Animation> {
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
        smooth_legacy: root.bool("isSmoothCurveLegacyMode")?.unwrap_or(false),
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
        match scene(&s, warnings) {
            Ok(sc) => a.scenes.push(sc),
            Err(e) => warnings.push(format!("scene skipped: {e}")),
        }
    }
    Ok(a)
}

fn curve_type(o: Option<&str>) -> CurveType {
    o.and_then(CurveType::from_name).unwrap_or_default()
}

fn scene(s: &O, warnings: &mut Vec<String>) -> Result<Scene> {
    let mi = s
        .field("movieInfo")?
        .ok_or_else(|| FormatError::Invalid("scene without movieInfo".into()))?;
    let mut sc = Scene {
        name: s.string("sceneName")?.unwrap_or_default(),
        guid: s.guid("guid")?.unwrap_or_default().to_owned(),
        width: mi.i32("width")?.unwrap_or(0),
        height: mi.i32("height")?.unwrap_or(0),
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
        match track(t) {
            Ok((tr, parent)) => {
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

fn track(t: &O) -> Result<(Track, Option<String>)> {
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
    if let Some(v) = src.field("visualEffect")? {
        for at in effect_attrs(&v)? {
            match attr_id(&at)?.as_str() {
                "opacity" => tr.opacity = Some(float_attr(&at)?),
                "frameStep" => tr.frame_step = int_curve(&at)?,
                _ => {}
            }
        }
    }
    tr.kind = match t.tag() {
        "CMvTrack_Group_Source" => TrackKind::Group,
        "CMvTrack_Live2DModel_Source" => TrackKind::Model(Box::new(model_track(t)?)),
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

fn ids(o: &O, name: &str) -> Result<Vec<String>> {
    Ok(o.list(name)?
        .iter()
        .filter_map(|i| i.attr("idstr").map(str::to_owned))
        .collect())
}

fn model_track(t: &O) -> Result<ModelTrack> {
    let mut m = ModelTrack {
        resource: t.guid("_resourceGuid")?.map(str::to_owned),
        ..Default::default()
    };
    if let Some(e) = t.field("keyParamEffect")? {
        for a in effect_attrs(&e)? {
            if a.tag() == "CMvAttrF" {
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
    if let Some(e) = t.field("partsVisibleEffect")? {
        for a in effect_attrs(&e)? {
            if a.tag() == "CMvAttrF" {
                m.parts.push(float_attr(&a)?);
            }
        }
    }
    if let Some(e) = t.field("eyeBlinkEffect")?.filter(|e| !e.is_null()) {
        let mut eb = EyeBlinkEffect {
            active: e.bool("isActive")?.unwrap_or(true),
            targets: ids(&e, "effectParameterAttrIds")?,
            invert: e.bool("invert")?.unwrap_or(false),
            relative: e.bool("relative")?.unwrap_or(false),
            ..Default::default()
        };
        for a in effect_attrs(&e)? {
            let at = float_attr(&a)?;
            match at.id.as_str() {
                "effectLevel" => eb.level = at,
                "eyeOpen" => eb.eye_open = at,
                _ => {}
            }
        }
        m.eye_blink = Some(eb);
    }
    if let Some(e) = t.field("lipSyncEffect")?.filter(|e| !e.is_null()) {
        let mut ls = LipSyncEffect {
            active: e.bool("isActive")?.unwrap_or(true),
            targets: ids(&e, "effectParameterAttrIds")?,
            sync_track: e.guid("syncTrackGuid")?.map(str::to_owned),
            invert: e.bool("isInvert")?.unwrap_or(false),
            relative: e.bool("isRelative")?.unwrap_or(false),
            ..Default::default()
        };
        for a in effect_attrs(&e)? {
            let at = float_attr(&a)?;
            match at.id.as_str() {
                "soundLevel" => ls.sound_level = at,
                "lipSyncScale" => ls.scale = at,
                "lipSyncBase" => ls.base = at,
                "lipSyncLevel" => ls.level = at,
                _ => {}
            }
        }
        m.lip_sync = Some(ls);
    }
    Ok(m)
}
