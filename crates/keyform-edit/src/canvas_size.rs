//! Canvas Settings (Modeling ▸ Model ▸ Canvas Settings; File ▸ Canvas
//! Settings… in menus.md): a new canvas size placed by a 3×3 anchor, the
//! model moved to keep its place relative to the anchor.
//! spec: 50-ui/dialogs.md §3

use std::sync::Arc;

use keyform_model::{DeformerKind, Model};

/// Moves the whole model by (dx, dy) canvas pixels: every root-level form
/// (art meshes, warp and rotation deformers, art paths without a target
/// deformer), with blend-shape keyforms (blend shapes add the difference to
/// the base keyform, so both move). Art-mesh rest positions move too.
/// TODO(Q-0051): whether model images, atlas entries and guides move as well.
pub fn move_model(m: &mut Model, dx: f32, dy: f32) {
    let shift = |p: &mut [f32]| {
        for c in p.chunks_mut(2) {
            c[0] += dx;
            if let Some(y) = c.get_mut(1) {
                *y += dy;
            }
        }
    };
    for a in &mut m.art_meshes {
        if a.target.is_some() {
            continue;
        }
        let a = Arc::make_mut(a);
        shift(&mut a.positions);
        for f in &mut a.grid.keyforms {
            shift(&mut f.positions);
        }
        for s in &mut a.blend_shapes.shapes {
            shift(&mut s.form.positions);
        }
    }
    for d in &mut m.deformers {
        if d.target.is_some() {
            continue;
        }
        match &mut Arc::make_mut(d).kind {
            DeformerKind::Warp(w) => {
                for f in &mut w.grid.keyforms {
                    shift(&mut f.positions);
                }
                for s in &mut w.blend_shapes.shapes {
                    shift(&mut s.form.positions);
                }
            }
            DeformerKind::Rotation(r) => {
                for f in &mut r.grid.keyforms {
                    shift(&mut f.origin);
                }
                for s in &mut r.blend_shapes.shapes {
                    shift(&mut s.form.origin);
                }
            }
        }
    }
    for p in &mut m.art_paths {
        if p.target.is_some() {
            continue;
        }
        let p = Arc::make_mut(p);
        for f in p
            .grid
            .keyforms
            .iter_mut()
            .chain(p.blend_shapes.shapes.iter_mut().map(|s| &mut s.form))
        {
            for q in &mut f.points {
                q.position[0] += dx;
                q.position[1] += dy;
            }
        }
    }
}

/// The offset of the old canvas inside the new one for an anchor column
/// (0 = W, 1 = centre, 2 = E) or row (0 = N, 1 = centre, 2 = S).
fn offset(anchor: u8, old: u32, new: u32) -> f32 {
    let d = new as f32 - old as f32;
    match anchor {
        0 => 0.0,
        1 => d / 2.0,
        _ => d,
    }
}

/// Canvas Settings: sets the canvas to `width` × `height` and moves the
/// model so it keeps its place relative to the anchor `(column, row)`.
pub fn change_canvas_size(
    m: &mut Model,
    width: u32,
    height: u32,
    anchor: (u8, u8),
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("The canvas size must be at least 1 × 1.".into());
    }
    let dx = offset(anchor.0, m.canvas.width, width);
    let dy = offset(anchor.1, m.canvas.height, height);
    m.canvas.width = width;
    m.canvas.height = height;
    if dx != 0.0 || dy != 0.0 {
        move_model(m, dx, dy);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_move_the_model() {
        let mut m = Model::default();
        m.canvas.width = 100;
        m.canvas.height = 200;
        // Centre anchor: half the growth; W/N: none; E/S: all of it.
        let mut c = m.clone();
        change_canvas_size(&mut c, 140, 260, (1, 1)).unwrap();
        assert_eq!((offset(1, 100, 140), offset(1, 200, 260)), (20.0, 30.0));
        assert_eq!((c.canvas.width, c.canvas.height), (140, 260));
        assert_eq!(offset(0, 100, 140), 0.0);
        assert_eq!(offset(2, 100, 60), -40.0);
        assert!(change_canvas_size(&mut m, 0, 10, (0, 0)).is_err());
    }
}
