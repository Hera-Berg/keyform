//! Writes a changed canvas size (`CImageCanvas` pixelWidth / pixelHeight),
//! for Canvas Settings (50-ui/dialogs.md §3).

use keyform_model::Model;

use super::element_at;
use super::grid::{field_path, Ctx};
use crate::error::{invalid, Result};
use crate::xml::Element;

pub(super) fn sync_canvas(
    root: &mut Element,
    _ctx: &mut Ctx,
    old: &Model,
    new: &Model,
) -> Result<()> {
    if old.canvas == new.canvas {
        return Ok(());
    }
    let main = match root.children.iter().position(|c| c.name == "main") {
        Some(i) => vec![i as u32, 0],
        None => return invalid("document has no <main>"),
    };
    let canvas = field_path(root, &main, "canvas")?;
    let e = element_at(root, Some(&canvas))?;
    for (name, v) in [
        ("pixelWidth", new.canvas.width),
        ("pixelHeight", new.canvas.height),
    ] {
        match e.children.iter_mut().find(|c| c.field_name() == Some(name)) {
            Some(c) => c.text = v.to_string(),
            None => return invalid(format!("canvas has no {name}")),
        }
    }
    Ok(())
}
