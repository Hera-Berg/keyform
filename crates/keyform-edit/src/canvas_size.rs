//! Canvas Settings and Resize Model (File menu): a new canvas size placed
//! by a 3×3 anchor with the model moved to keep its place, and scaling the
//! model. spec: 50-ui/dialogs.md §3, §22

use std::sync::Arc;

use keyform_model::{DeformerKind, Model};

/// Scales the whole model by `scale` about canvas (0, 0), then moves it by
/// `offset` (dialogs.md §22 steps 1–5, done directly): wrapping every
/// root-level object in a rotation deformer at (0, 0), setting its scale
/// and origin and unwrapping again bakes p → p·scale + offset into their
/// keyforms (and blend-shape keyforms, which blend shapes compare with the
/// base keyform); a root rotation deformer also takes the scale into its
/// own keyform scale, which carries everything below it. Children of warp
/// deformers are in the warp's normalised space and don't change. Only
/// keyforms change: rest positions, model images, atlases, UVs and guides
/// stay put.
pub fn transform_model(m: &mut Model, scale: f32, offset: [f32; 2]) {
    let map = |p: &mut [f32]| {
        for c in p.chunks_mut(2) {
            c[0] = c[0] * scale + offset[0];
            if let Some(y) = c.get_mut(1) {
                *y = *y * scale + offset[1];
            }
        }
    };
    for a in &mut m.art_meshes {
        if a.target.is_some() {
            continue;
        }
        let a = Arc::make_mut(a);
        for f in &mut a.grid.keyforms {
            map(&mut f.positions);
        }
        for s in &mut a.blend_shapes.shapes {
            map(&mut s.form.positions);
        }
    }
    for d in &mut m.deformers {
        if d.target.is_some() {
            continue;
        }
        match &mut Arc::make_mut(d).kind {
            DeformerKind::Warp(w) => {
                for f in &mut w.grid.keyforms {
                    map(&mut f.positions);
                }
                for s in &mut w.blend_shapes.shapes {
                    map(&mut s.form.positions);
                }
            }
            DeformerKind::Rotation(r) => {
                for f in r
                    .grid
                    .keyforms
                    .iter_mut()
                    .chain(r.blend_shapes.shapes.iter_mut().map(|s| &mut s.form))
                {
                    map(&mut f.origin);
                    f.scale *= scale;
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
                map(&mut q.position);
            }
        }
    }
}

/// Moves the whole model by (dx, dy) canvas pixels (Canvas Settings).
pub fn move_model(m: &mut Model, dx: f32, dy: f32) {
    transform_model(m, 1.0, [dx, dy]);
}

/// File ▸ Resize Model: scales every keyform by `percent` / 100 about
/// canvas (0, 0); the canvas size doesn't change. spec: dialogs.md §22
pub fn resize_model(m: &mut Model, percent: f32) -> Result<(), String> {
    if !(1.0..=5000.0).contains(&percent) {
        return Err("The resize factor must be between 1 and 5000 %.".into());
    }
    transform_model(m, percent / 100.0, [0.0, 0.0]);
    Ok(())
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
        assert!(resize_model(&mut m, 0.5).is_err());
        assert!(resize_model(&mut m, 6000.0).is_err());
    }

    #[test]
    fn resize_scales_keyforms_about_the_origin() {
        use keyform_model::{KeyformGrid, RotationDeformer, RotationForm};
        let mut m = Model::default();
        m.deformers.push(Arc::new(keyform_model::Deformer {
            info: keyform_model::ObjectInfo::new("R"),
            target: None,
            kind: DeformerKind::Rotation(RotationDeformer {
                handle_length: 10.0,
                circle_radius: 5.0,
                base_angle: 0.0,
                grid: KeyformGrid::single(RotationForm {
                    origin: [100.0, 40.0],
                    ..RotationForm::default()
                }),
                blend_shapes: Default::default(),
            }),
        }));
        resize_model(&mut m, 50.0).unwrap();
        let DeformerKind::Rotation(r) = &m.deformers[0].kind else {
            panic!()
        };
        assert_eq!(r.grid.keyforms[0].origin, [50.0, 20.0]);
        assert_eq!(
            r.grid.keyforms[0].scale, 0.5,
            "the scale carries the children"
        );
    }
}
