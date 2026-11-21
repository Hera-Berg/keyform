//! Writes the Modeling view's ruler guides (`CGuidesSetting.guidesModeling`)
//! when they changed, in the layout the reference saves (observed,
//! 2026-11-21): `CGuide_Modeling` with guid, type, position, colour
//! components and memo. spec: 50-ui/view-aids.md §1

use keyform_model::{Guide, Model};

use super::element_at;
use super::grid::{field_path, scalar, set_count, Ctx};
use crate::error::{invalid, Result};
use crate::graph::format_f32;
use crate::xml::Element;

fn guide_element(g: &Guide) -> Element {
    let mut e = Element::new("CGuide_Modeling");
    e.children.push(
        Element::new("CGuideGuid")
            .with_attr("xs.n", "guid")
            .with_attr("uuid", g.guid.as_str())
            .with_attr("note", "(no debug info)"),
    );
    e.children.push(
        Element::new("GuideType")
            .with_attr("xs.n", "type")
            .with_attr("v", if g.vertical { "VERTICAL" } else { "HORIZONTAL" }),
    );
    e.children
        .push(scalar("f", "position", format_f32(g.position)));
    for (name, v) in ["colorRed", "colorGreen", "colorBlue", "colorAlpha"]
        .iter()
        .zip(g.color)
    {
        e.children.push(scalar("f", name, format_f32(v)));
    }
    e.children.push(scalar("s", "memo", g.memo.as_str()));
    e
}

pub(super) fn sync_guides(
    root: &mut Element,
    ctx: &mut Ctx,
    old: &Model,
    new: &Model,
) -> Result<()> {
    if old.guides == new.guides {
        return Ok(());
    }
    let main = match root.children.iter().position(|c| c.name == "main") {
        Some(i) => vec![i as u32, 0],
        None => return invalid("document has no <main>"),
    };
    let Ok(setting) = field_path(root, &main, "guides") else {
        return invalid("this document has no guide settings (CGuidesSetting)");
    };
    let list = field_path(root, &setting, "guidesModeling")?;
    let l = element_at(root, Some(&list))?;
    l.children = new.guides.iter().map(guide_element).collect();
    set_count(l);
    ctx.structural = true;
    Ok(())
}
