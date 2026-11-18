//! Saves motion-sync settings (`motionSyncSettingsSet`) in the layout a
//! reference-made file uses (5.3.04, test-data/blackbox/p5/ms.cmo3):
//! inline settings; parameters by ID string; all audio elements refer to
//! one shared null `SyncAudioParamGuid`; mappings as linked maps keyed by
//! audio ID and parameter ID. spec: 70-advanced/motion-sync.md §1,
//! 10-formats/cmo3-schema.md (CMotionSyncSettingSource, MotionSyncMappingSetting)

use std::collections::HashMap;

use keyform_model::{Model, MotionSyncSetting};

use super::element_at;
use super::grid::{field_path, get_shared, scalar, set_count, stub, uuid_of, Ctx};
use super::objects::guid_id;
use super::params::main_path;
use super::unsupported;
use crate::error::Result;
use crate::graph::format_f32;
use crate::xml::Element;

pub(super) fn sync_motion_sync(
    root: &mut Element,
    ctx: &mut Ctx,
    old: &Model,
    new: &Model,
) -> Result<()> {
    if old.motion_sync == new.motion_sync {
        return Ok(());
    }
    let main = main_path(root)?;
    let Ok(set) = field_path(root, &main, "motionSyncSettingsSet") else {
        // TODO(Q-0039): adding the set to an older document.
        return unsupported("motion-sync settings in a model file saved before Cubism 5");
    };
    let list = field_path(root, &set, "_settingSourceSetMotionSync")?;
    ctx.structural = true;
    let snapshot = element_at(root, Some(&list))?.clone();
    let mut by_uuid: HashMap<String, Element> = HashMap::new();
    for c in &snapshot.children {
        let content = match c.attr("xs.ref") {
            Some(id) => get_shared(root, id).cloned(),
            None => Some(c.clone()),
        };
        if let Some(u) = content
            .and_then(|e| e.child_field("guid").cloned())
            .and_then(|g| uuid_of(root, &g))
        {
            by_uuid.insert(u, c.clone());
        }
    }
    let mut children = Vec::new();
    for s in &new.motion_sync {
        let before = old.motion_sync.iter().find(|o| o.guid == s.guid);
        match by_uuid.remove(&s.guid) {
            Some(child) if before == Some(s) => children.push(child),
            Some(child) => {
                if let Some(id) = child.attr("xs.ref") {
                    ctx.orphans.push(id.to_owned());
                }
                children.push(setting(root, ctx, s)?);
            }
            None => children.push(setting(root, ctx, s)?),
        }
    }
    for child in by_uuid.into_values() {
        if let Some(id) = child.attr("xs.ref") {
            ctx.orphans.push(id.to_owned());
        }
    }
    let l = element_at(root, Some(&list))?;
    l.children = children;
    set_count(l);
    Ok(())
}

fn el(tag: &str, name: &str) -> Element {
    Element::new(tag).with_attr("xs.n", name)
}

fn guid_el(tag: &str, name: &str, uuid: &str) -> Element {
    el(tag, name)
        .with_attr("uuid", uuid)
        .with_attr("note", "(no debug info)")
}

fn text(tag: &str, name: &str, v: &str) -> Element {
    let mut e = el(tag, name);
    e.text = super::xml_text(v);
    e
}

fn setting(root: &mut Element, ctx: &mut Ctx, s: &MotionSyncSetting) -> Result<Element> {
    let f = |v: f32| format_f32(v);
    let mut pp = el("MotionSyncPostProcSetting", "postProcessingSetting");
    pp.children = vec![
        scalar("f", "blendPercentage", f(s.blend)),
        scalar("i", "smoothAmount", s.smoothing.to_string()),
        scalar("f", "sampleRate", f(s.sample_rate)),
    ];
    let mut cubism = el("array_list", "cubismParameters");
    for c in &s.cubism {
        let mut e = Element::new("SyncCubismParam");
        e.children = vec![
            guid_el("SyncCubismParamGuid", "guid", &c.guid),
            text("s", "parameterId", &c.parameter_id),
            text("s", "name", &c.name),
            scalar("f", "min", f(c.min)),
            scalar("f", "max", f(c.max)),
            scalar("f", "damper", f(c.damper)),
            scalar("i", "smooth", c.smooth.to_string()),
        ];
        cubism.children.push(e);
    }
    set_count(&mut cubism);
    let mut audio = el("array_list", "audioParameters");
    for a in &s.audio {
        let gid = guid_id(root, ctx, &a.guid, "SyncAudioParamGuid", "(no debug info)")?;
        let mut e = Element::new("SyncAudioParam");
        e.children = vec![
            stub("SyncAudioParamGuid", Some("guid"), &gid),
            text("s", "audioId", &a.audio_id),
            text("s", "name", &a.name),
            scalar("f", "min", f(a.min)),
            scalar("f", "max", f(a.max)),
            scalar("f", "scale", f(a.scale)),
            scalar("b", "enabled", a.enabled.to_string()),
        ];
        audio.children.push(e);
    }
    set_count(&mut audio);
    let mut map = el("linked_map", "mappingMap").with_attr("keyType", "string");
    for x in &s.mappings {
        let mut details = el("linked_map", "details").with_attr("keyType", "string");
        for (pid, v) in &x.targets {
            let mut sup = el("AMappingDetail", "super");
            sup.children = vec![text("s", "_audioId", &x.audio_id)];
            let mut d = el("ShapeMappingDetail", pid);
            d.children = vec![sup, scalar("f", "target", f(*v))];
            details.children.push(d);
        }
        set_count(&mut details);
        let mut item = el("MappingItem", &x.audio_id);
        item.children = vec![
            text("s", "audioId", &x.audio_id),
            el("SettingType", "type").with_attr("v", x.kind.clone()),
            details,
        ];
        map.children.push(item);
    }
    set_count(&mut map);
    let mut mapping = el("MotionSyncMappingSetting", "mapping");
    mapping.children = vec![
        el("SyncType", "syncType").with_attr("v", s.sync_type.clone()),
        cubism,
        audio,
        map,
    ];
    let mut e = Element::new("CMotionSyncSettingSource");
    e.children = vec![
        el("CMotionSyncSettingId", "id").with_attr("idstr", s.id.clone()),
        text("s", "name", &s.name),
        el("CEMotionSyncVersion", "motionSyncVersion").with_attr(
            "v",
            if s.version >= 1 {
                "MOTION_SYNC_1"
            } else {
                "MOTION_SYNC_0"
            },
        ),
        guid_el("CMotionSyncSettingGuid", "guid", &s.guid),
        pp,
        mapping,
    ];
    Ok(e)
}
