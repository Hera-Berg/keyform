//! moc3 → [`Model`], for viewing runtime files and cross-checking exports.
//!
//! Every index and range from the file is validated; malformed files give an
//! error, never a panic. spec: 10-formats/moc3.md §8, §11

use std::ops::Range;

use keyform_model::*;

use super::Moc3Raw;
use crate::error::{invalid, FormatError, Result};

fn range(what: &str, begin: i32, count: i32, len: usize) -> Result<Range<usize>> {
    // Empty ranges may carry any begin (unused parameters store −1).
    if count == 0 {
        return Ok(0..0);
    }
    if begin < 0 || count < 0 {
        return invalid(format!("{what}: negative range {begin}+{count}"));
    }
    let (b, c) = (begin as usize, count as usize);
    match b.checked_add(c) {
        Some(e) if e <= len => Ok(b..e),
        _ => invalid(format!("{what}: range {b}+{c} exceeds pool of {len}")),
    }
}

fn at<T: Copy>(what: &str, v: &[T], i: usize) -> Result<T> {
    v.get(i)
        .copied()
        .ok_or_else(|| FormatError::Invalid(format!("{what}: index {i} out of range")))
}

fn opt_index(what: &str, v: i32, len: usize) -> Result<Option<usize>> {
    match v {
        -1 => Ok(None),
        i if i >= 0 && (i as usize) < len => Ok(Some(i as usize)),
        _ => invalid(format!("{what}: index {v} out of range (len {len})")),
    }
}

struct Ctx<'a> {
    r: &'a Moc3Raw,
    /// Parameter owning each parameter binding.
    binding_param: Vec<ParamIndex>,
    ppu: f32,
    origin: [f32; 2],
}

impl Ctx<'_> {
    /// Bindings of keyform binding `kb`.
    fn bindings(&self, what: &str, kb: i32) -> Result<Vec<Binding>> {
        let r = self.r;
        let begins = r.i32s("keyform_bindings.begin");
        let kb = usize::try_from(kb)
            .ok()
            .filter(|&k| k < begins.len())
            .ok_or_else(|| {
                FormatError::Invalid(format!("{what}: keyform binding {kb} out of range"))
            })?;
        let idx = r.i32s("param_binding_indices");
        let rg = range(
            what,
            begins[kb],
            at(what, r.i32s("keyform_bindings.count"), kb)?,
            idx.len(),
        )?;
        let keys = r.f32s("keys");
        let mut out = Vec::new();
        for &pb in &idx[rg] {
            let pb = usize::try_from(pb)
                .ok()
                .filter(|&p| p < self.binding_param.len())
                .ok_or_else(|| {
                    FormatError::Invalid(format!("{what}: parameter binding {pb} out of range"))
                })?;
            let kr = range(
                what,
                at(what, r.i32s("param_bindings.keys_begin"), pb)?,
                at(what, r.i32s("param_bindings.keys_count"), pb)?,
                keys.len(),
            )?;
            let k = keys[kr].to_vec();
            if k.is_empty() || !k.windows(2).all(|w| w[0] < w[1]) {
                return invalid(format!("{what}: binding keys not ascending"));
            }
            out.push(Binding::new(self.binding_param[pb], k));
        }
        Ok(out)
    }

    fn keyform_range(
        &self,
        what: &str,
        bindings: &[Binding],
        begin: i32,
        count: i32,
        pool: usize,
    ) -> Result<Range<usize>> {
        let rg = range(what, begin, count, pool)?;
        let cells: usize = bindings.iter().map(|b| b.keys.len()).product();
        if rg.len() != cells {
            return invalid(format!(
                "{what}: {} keyforms for a grid of {cells} cells",
                rg.len()
            ));
        }
        Ok(rg)
    }

    fn to_canvas(&self, xy: &mut [f32]) {
        for p in xy.as_chunks_mut::<2>().0 {
            p[0] = p[0] * self.ppu + self.origin[0];
            p[1] = p[1] * self.ppu + self.origin[1];
        }
    }

    /// Multiply / screen colors of keyform `k` (`base` = object's color begin, v4.2+).
    fn colors(
        &self,
        kind: &str,
        k: usize,
        base: Option<i32>,
        local: usize,
    ) -> Result<(Color, Color)> {
        let r = self.r;
        if r.version < 4 {
            return Ok((WHITE, SCREEN_DEFAULT));
        }
        let (mi, si) = if r.version >= 5 {
            (
                at(kind, r.i32s(&format!("{kind}_keyforms.multiply_color")), k)?,
                at(kind, r.i32s(&format!("{kind}_keyforms.screen_color")), k)?,
            )
        } else {
            let b = base.unwrap_or(0) + local as i32;
            (b, b)
        };
        let get = |pre: &str, i: i32| -> Result<Color> {
            let i = usize::try_from(i)
                .map_err(|_| FormatError::Invalid(format!("{kind}: color index {i}")))?;
            Ok([
                at(kind, r.f32s(&format!("{pre}.r")), i)?,
                at(kind, r.f32s(&format!("{pre}.g")), i)?,
                at(kind, r.f32s(&format!("{pre}.b")), i)?,
                1.0,
            ])
        };
        Ok((get("multiply_colors", mi)?, get("screen_colors", si)?))
    }
}

/// Converts a moc3 file to a model in canvas pixels.
pub fn moc3_to_model(r: &Moc3Raw) -> Result<Model> {
    use super::layout::c;
    let cv = r.canvas;
    let finite = [
        cv.pixels_per_unit,
        cv.origin_x,
        cv.origin_y,
        cv.width,
        cv.height,
    ];
    if finite.iter().any(|v| !v.is_finite()) || cv.pixels_per_unit <= 0.0 {
        return invalid("bad canvas info");
    }
    if !(1.0..=30000.0).contains(&cv.width) || !(1.0..=30000.0).contains(&cv.height) {
        return invalid("canvas size out of range");
    }
    let mut m = Model {
        canvas: Canvas {
            width: cv.width as u32,
            height: cv.height as u32,
        },
        ..Model::default()
    };
    m.export.pixels_per_unit = Some(cv.pixels_per_unit);
    m.export.origin_normalized = [cv.origin_x / cv.width, cv.origin_y / cv.height];

    // Parameters and the binding → parameter map.
    let n_params = r.count(c::PARAMETERS);
    let n_bindings = r.count(c::PARAM_BINDINGS);
    let mut binding_param = vec![usize::MAX; n_bindings];
    let ids = r.ids("parameters.id");
    for p in 0..n_params {
        let what = "parameter";
        let mut param = Parameter::new(
            &ids[p],
            at(what, r.f32s("parameters.min"), p)?,
            at(what, r.f32s("parameters.max"), p)?,
            at(what, r.f32s("parameters.default"), p)?,
        );
        param.repeat = at(what, r.i32s("parameters.repeat"), p)? != 0;
        param.decimal_places = at(what, r.i32s("parameters.decimal_places"), p)?;
        param.snap_epsilon = 10f32.powi(-param.decimal_places.clamp(0, 10));
        if r.version >= 4 && at(what, r.i32s("parameters.type"), p)? == 1 {
            param.kind = ParameterKind::BlendShape;
        }
        let rg = range(
            what,
            at(what, r.i32s("parameters.binding_begin"), p)?,
            at(what, r.i32s("parameters.binding_count"), p)?,
            n_bindings,
        )?;
        for b in rg {
            binding_param[b] = p;
        }
        m.parameters.push(std::sync::Arc::new(param));
        m.root_parameter_entries.push(ParamEntry::Parameter(p));
    }
    if let Some(b) = binding_param.iter().position(|&p| p == usize::MAX) {
        return invalid(format!("parameter binding {b} owned by no parameter"));
    }
    let ctx = Ctx {
        r,
        binding_param,
        ppu: cv.pixels_per_unit,
        origin: [cv.origin_x, cv.origin_y],
    };

    // Parts (plus a synthetic root part at the end for the draw-order tree).
    let n_parts = r.count(c::PARTS);
    let part_ids = r.ids("parts.id");
    let draw_orders = r.f32s("part_keyforms.draw_order");
    for p in 0..n_parts {
        let what = &part_ids[p];
        let bindings = ctx.bindings(what, at(what, r.i32s("parts.keyform_binding"), p)?)?;
        let kr = ctx.keyform_range(
            what,
            &bindings,
            at(what, r.i32s("parts.keyform_begin"), p)?,
            at(what, r.i32s("parts.keyform_count"), p)?,
            draw_orders.len(),
        )?;
        let mut part = Part::new(what);
        part.info.visible = at(what, r.i32s("parts.visible"), p)? != 0;
        part.info.parent_part =
            opt_index(what, at(what, r.i32s("parts.parent_part"), p)?, n_parts)?;
        part.grid = KeyformGrid {
            guids: Vec::new(),
            bindings,
            keyforms: draw_orders[kr]
                .iter()
                .map(|&d| PartForm {
                    draw_order: d,
                    ..PartForm::default()
                })
                .collect(),
        };
        if r.version >= 6 && at(what, r.i32s("parts.offscreen"), p)? >= 0 {
            part.offscreen = true;
        }
        m.parts.push(std::sync::Arc::new(part));
    }
    let root = m.parts.len();
    m.parts.push(std::sync::Arc::new(Part::new("__RootPart__")));
    for p in 0..n_parts {
        if m.parts[p].info.parent_part.is_none() {
            std::sync::Arc::make_mut(&mut m.parts[p]).info.parent_part = Some(root);
        }
    }
    m.root_part = Some(root);

    // Deformers.
    let n_def = r.count(c::DEFORMERS);
    let def_ids = r.ids("deformers.id");
    let positions = r.f32s("keyform_positions.xy");
    for d in 0..n_def {
        let what = &def_ids[d];
        let mut info = ObjectInfo::new(what);
        info.visible = at(what, r.i32s("deformers.visible"), d)? != 0;
        info.parent_part =
            opt_index(what, at(what, r.i32s("deformers.parent_part"), d)?, n_parts)?.or(Some(root));
        let target = opt_index(
            what,
            at(what, r.i32s("deformers.parent_deformer"), d)?,
            n_def,
        )?;
        if target.is_some_and(|t| t >= d) {
            return invalid(format!("{what}: parent deformer must precede its child"));
        }
        let si = at(what, r.i32s("deformers.specific_index"), d)?;
        let si =
            usize::try_from(si).map_err(|_| FormatError::Invalid(format!("{what}: index {si}")))?;
        let kind = match at(what, r.i32s("deformers.type"), d)? {
            0 => {
                let w = |n: &str| at(what, r.i32s(n), si);
                let (cols, rows) = (w("warps.cols")?, w("warps.rows")?);
                if !(1..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
                    return invalid(format!("{what}: warp grid {cols}×{rows}"));
                }
                let npts = (cols as usize + 1) * (rows as usize + 1);
                if w("warps.vertex_count")? as usize != npts {
                    return invalid(format!("{what}: warp vertex count mismatch"));
                }
                let bindings = ctx.bindings(what, w("warps.keyform_binding")?)?;
                let kr = ctx.keyform_range(
                    what,
                    &bindings,
                    w("warps.keyform_begin")?,
                    w("warps.keyform_count")?,
                    r.count(c::WARP_KEYFORMS),
                )?;
                let color_base = if r.version >= 4 {
                    Some(w("warps.color_begin")?)
                } else {
                    None
                };
                let mut keyforms = Vec::with_capacity(kr.len());
                for (local, k) in kr.enumerate() {
                    let pb = at(what, r.i32s("warp_keyforms.position_begin"), k)?;
                    let pr = range(what, pb, 2 * npts as i32, positions.len())?;
                    let mut pos = positions[pr].to_vec();
                    if target.is_none() {
                        ctx.to_canvas(&mut pos);
                    }
                    let (multiply, screen) = ctx.colors("warp", k, color_base, local)?;
                    keyforms.push(WarpForm {
                        positions: pos,
                        opacity: at(what, r.f32s("warp_keyforms.opacity"), k)?,
                        multiply,
                        screen,
                    });
                }
                DeformerKind::Warp(WarpDeformer {
                    bezier_nets: Vec::new(),
                    cols: cols as u32,
                    rows: rows as u32,
                    quad: r.version >= 2 && w("warps.quad")? != 0,
                    grid: KeyformGrid {
                        bindings,
                        keyforms,
                        guids: Vec::new(),
                    },
                    blend_shapes: BlendShapes::default(),
                })
            }
            1 => {
                let w = |n: &str| at(what, r.i32s(n), si);
                let bindings = ctx.bindings(what, w("rotations.keyform_binding")?)?;
                let kr = ctx.keyform_range(
                    what,
                    &bindings,
                    w("rotations.keyform_begin")?,
                    w("rotations.keyform_count")?,
                    r.count(c::ROTATION_KEYFORMS),
                )?;
                let color_base = if r.version >= 4 {
                    Some(w("rotations.color_begin")?)
                } else {
                    None
                };
                // The export wraps the model in a 1/ppu rotation; warps pass total
                // scale through, so the first rotation deformer on each chain carries
                // the factor. spec: moc3.md §8.10
                let first_rotation = {
                    let mut t = target;
                    let mut has_rotation_ancestor = false;
                    while let Some(p) = t {
                        if matches!(m.deformers[p].kind, DeformerKind::Rotation(_)) {
                            has_rotation_ancestor = true;
                            break;
                        }
                        t = m.deformers[p].target;
                    }
                    !has_rotation_ancestor
                };
                let mut keyforms = Vec::with_capacity(kr.len());
                for (local, k) in kr.enumerate() {
                    let f = |n: &str| at(what, r.f32s(n), k);
                    let mut origin = [
                        f("rotation_keyforms.origin_x")?,
                        f("rotation_keyforms.origin_y")?,
                    ];
                    let mut scale = f("rotation_keyforms.scale")?;
                    if target.is_none() {
                        ctx.to_canvas(&mut origin);
                    }
                    if first_rotation {
                        scale *= ctx.ppu;
                    }
                    let (multiply, screen) = ctx.colors("rotation", k, color_base, local)?;
                    keyforms.push(RotationForm {
                        angle: f("rotation_keyforms.angle")?,
                        origin,
                        scale,
                        reflect_x: at(what, r.i32s("rotation_keyforms.reflect_x"), k)? != 0,
                        reflect_y: at(what, r.i32s("rotation_keyforms.reflect_y"), k)? != 0,
                        opacity: f("rotation_keyforms.opacity")?,
                        multiply,
                        screen,
                    });
                }
                DeformerKind::Rotation(RotationDeformer {
                    handle_length: 100.0,
                    circle_radius: 30.0,
                    base_angle: at(what, r.f32s("rotations.base_angle"), si)?,
                    grid: KeyformGrid {
                        bindings,
                        keyforms,
                        guids: Vec::new(),
                    },
                    blend_shapes: BlendShapes::default(),
                })
            }
            t => return invalid(format!("{what}: deformer type {t}")),
        };
        m.deformers
            .push(std::sync::Arc::new(Deformer { info, target, kind }));
    }

    // Art meshes.
    let n_mesh = r.count(c::ART_MESHES);
    let mesh_ids = r.ids("art_meshes.id");
    let uvs = r.f32s("uvs");
    let vidx = r.i16s("vertex_indices");
    let clips = r.i32s("clip_masks");
    for a in 0..n_mesh {
        let what = &mesh_ids[a];
        let g = |n: &str| at(what, r.i32s(n), a);
        let mut info = ObjectInfo::new(what);
        info.visible = g("art_meshes.visible")? != 0;
        info.parent_part = opt_index(what, g("art_meshes.parent_part")?, n_parts)?.or(Some(root));
        let target = opt_index(what, g("art_meshes.parent_deformer")?, n_def)?;
        let nv = g("art_meshes.vertex_count")?;
        let uvr = range(
            what,
            g("art_meshes.uv_begin")?,
            nv.saturating_mul(2),
            uvs.len(),
        )?;
        let ir = range(
            what,
            g("art_meshes.index_begin")?,
            g("art_meshes.index_count")?,
            vidx.len(),
        )?;
        if ir.len() % 3 != 0 {
            return invalid(format!("{what}: index count not a multiple of 3"));
        }
        let indices: Vec<u32> = vidx[ir]
            .iter()
            .map(|&i| u32::try_from(i).ok().filter(|&i| (i as i32) < nv))
            .collect::<Option<_>>()
            .ok_or_else(|| FormatError::Invalid(format!("{what}: vertex index out of range")))?;
        let clip: Vec<MeshIndex> = clips[range(
            what,
            g("art_meshes.clip_begin")?,
            g("art_meshes.clip_count")?,
            clips.len(),
        )?]
        .iter()
        .map(|&ci| {
            opt_index(what, ci, n_mesh)?
                .ok_or_else(|| FormatError::Invalid(format!("{what}: clip index")))
        })
        .collect::<Result<_>>()?;
        let flags = at(what, r.u8s("art_meshes.flags"), a)?;
        let (mut cc, mut ac) = match flags & 3 {
            1 => (ColorComposition::Add, AlphaComposition::Over),
            2 => (ColorComposition::Multiply, AlphaComposition::Over),
            _ => (ColorComposition::Normal, AlphaComposition::Over),
        };
        if r.version >= 6 {
            let bm = g("art_meshes.blend_mode")?;
            cc = ColorComposition::from_moc3_code((bm & 0xff) as u8).unwrap_or(cc);
            ac = AlphaComposition::from_moc3_code(((bm >> 8) & 0xff) as u8).unwrap_or(ac);
        }

        let bindings = ctx.bindings(what, g("art_meshes.keyform_binding")?)?;
        let kr = ctx.keyform_range(
            what,
            &bindings,
            g("art_meshes.keyform_begin")?,
            g("art_meshes.keyform_count")?,
            r.count(c::ART_MESH_KEYFORMS),
        )?;
        let color_base = if r.version >= 4 {
            Some(g("art_meshes.color_begin")?)
        } else {
            None
        };
        let mut keyforms = Vec::with_capacity(kr.len());
        for (local, k) in kr.enumerate() {
            let pb = at(what, r.i32s("art_mesh_keyforms.position_begin"), k)?;
            let mut pos =
                positions[range(what, pb, nv.saturating_mul(2), positions.len())?].to_vec();
            if target.is_none() {
                ctx.to_canvas(&mut pos);
            }
            let (multiply, screen) = ctx.colors("art_mesh", k, color_base, local)?;
            keyforms.push(ArtMeshForm {
                positions: pos,
                draw_order: at(what, r.f32s("art_mesh_keyforms.draw_order"), k)?,
                opacity: at(what, r.f32s("art_mesh_keyforms.opacity"), k)?,
                multiply,
                screen,
            });
        }
        let nvu = nv as usize;
        m.art_meshes.push(std::sync::Arc::new(ArtMesh {
            clip_missing: false,
            info,
            target,
            positions: keyforms
                .first()
                .map(|k| k.positions.clone())
                .unwrap_or_default(),
            uvs: uvs[uvr].to_vec(),
            indices,
            texture: usize::try_from(g("art_meshes.texture")?).ok(),
            clip,
            invert_clip: r.version >= 3 && flags & 8 != 0,
            culling: flags & 4 == 0,
            color_composition: cc,
            alpha_composition: ac,
            user_data: String::new(),
            point_uids: (0..nvu as i64).collect(),
            mesh: EditableMesh::default(),
            model_image: None,
            generator: Default::default(),
            grid: KeyformGrid {
                bindings,
                keyforms,
                guids: Vec::new(),
            },
            blend_shapes: BlendShapes::default(),
        }));
    }

    // Glue.
    let n_glue = r.count(c::GLUES);
    let glue_ids = r.ids("glues.id");
    let pw = r.f32s("glue_pairs.weight");
    let pv = r.i16s("glue_pairs.vertex");
    let gk = r.f32s("glue_keyforms.intensity");
    for gi in 0..n_glue {
        let what = &glue_ids[gi];
        let g = |n: &str| at(what, r.i32s(n), gi);
        let mesh_a = opt_index(what, g("glues.mesh_a")?, n_mesh)?;
        let mesh_b = opt_index(what, g("glues.mesh_b")?, n_mesh)?;
        let pr = range(
            what,
            g("glues.pair_begin")?,
            g("glues.pair_count")?,
            pv.len().min(pw.len()),
        )?;
        if pr.len() % 2 != 0 {
            return invalid(format!("{what}: odd glue entry count"));
        }
        let (mut pairs, mut weights) = (Vec::new(), Vec::new());
        for e in pr.step_by(2) {
            let va = usize::try_from(pv[e]).ok();
            let vb = usize::try_from(pv[e + 1]).ok();
            pairs.push((va, vb));
            weights.push([pw[e], pw[e + 1]]);
        }
        let bindings = ctx.bindings(what, g("glues.keyform_binding")?)?;
        let kr = ctx.keyform_range(
            what,
            &bindings,
            g("glues.keyform_begin")?,
            g("glues.keyform_count")?,
            gk.len(),
        )?;
        m.glues.push(std::sync::Arc::new(Glue {
            info: ObjectInfo::new(what),
            mesh_a,
            mesh_b,
            pairs,
            weights,
            grid: KeyformGrid {
                guids: Vec::new(),
                bindings,
                keyforms: gk[kr].iter().map(|&i| GlueForm { intensity: i }).collect(),
            },
            blend_shapes: BlendShapes::default(),
        }));
    }

    build_draw_order_tree(r, &mut m, root)?;
    if r.version >= 4 && r.count(c::BS_KEYFORM_BINDINGS) > 0 {
        // TODO: import blend shapes (moc3.md §8.4); evaluation then lacks them.
        return Err(FormatError::Unsupported(
            "moc3 blend shapes not imported yet".into(),
        ));
    }
    Ok(m)
}

/// Rebuilds part child lists from the draw-order groups (§8.7) so the editor
/// draw-order algorithm reproduces the runtime order.
fn build_draw_order_tree(r: &Moc3Raw, m: &mut Model, root: PartIndex) -> Result<()> {
    use super::layout::c;
    let n_groups = r.count(c::DRAW_ORDER_GROUPS);
    let items = r.count(c::DRAW_ORDER_GROUP_ITEMS);
    if n_groups == 0 {
        return Ok(());
    }
    let ty = r.i32s("draw_order_group_items.type");
    let ix = r.i32s("draw_order_group_items.index");
    let own = r.i32s("draw_order_group_items.own_group");
    // Group owner part for groups ≥ 1.
    let mut owner = vec![None; n_groups];
    owner[0] = Some(root);
    for i in 0..items {
        if at("group item", ty, i)? == 1 {
            let g = opt_index("group item", at("group item", own, i)?, n_groups)?;
            let p = opt_index("group item", at("group item", ix, i)?, root)?;
            if let (Some(g), Some(p)) = (g, p) {
                owner[g] = Some(p);
            }
        }
    }
    for g in 0..n_groups {
        let Some(p) = owner[g] else { continue };
        let rg = range(
            "draw order group",
            at("group", r.i32s("draw_order_groups.item_begin"), g)?,
            at("group", r.i32s("draw_order_groups.item_count"), g)?,
            items,
        )?;
        let mut children = Vec::with_capacity(rg.len());
        for i in rg.rev() {
            // Items are bottom first; part children are palette (top-first) order.
            match ty[i] {
                0 => children.push(PartChild::ArtMesh(
                    opt_index("group item", ix[i], m.art_meshes.len())?.unwrap_or(0),
                )),
                1 => {
                    if let Some(pi) = opt_index("group item", ix[i], root)? {
                        std::sync::Arc::make_mut(&mut m.parts[pi]).draw_order_group = true;
                        children.push(PartChild::Part(pi));
                    }
                }
                _ => children.push(PartChild::Other),
            }
        }
        if p != root || g == 0 {
            std::sync::Arc::make_mut(&mut m.parts[p]).children = children;
        }
    }
    Ok(())
}
