//! `kf set`: apply field edits to a project and save it (exercises the
//! `.cmo3` write-back; used for black-box checks in the reference editor).
//!
//! Edits are `KIND:ID.FIELD=VALUE` with KIND one of `param`, `part`, `mesh`,
//! `deformer`, `glue`:
//! - every object kind: `id`, `name`, `visible`, `locked`;
//! - `param`: `min`, `max`, `default`, `repeat`, `description`;
//!   `create=MIN,MAX,DEFAULT` (new top-level parameter), `delete=1`;
//! - `part`, `mesh`, `deformer`: `opacity` (every keyform);
//! - `mesh`, warp `deformer`: `move=DX,DY` (every keyform position);
//! - rotation `deformer`: `angle+=DEG` (every keyform);
//! - `mesh`: `culling`;
//! - every kind with keyforms: `addkey=PARAM_ID:VALUE`, `removekey=PARAM_ID:VALUE`
//! - mesh: `movekeyform=VALUE:dx,dy` (single binding)
//!   (keys.md §1–2);
//! - `mesh` topology (re-triangulated, keyforms remapped): `addpoint=X,Y`
//!   (canvas pixels; on an edge it splits the edge), `movepoint=K:DX,DY`,
//!   `deletepoint=K`;
//! - `mesh`: `delete=1` (its glue goes too), `clip=ID,ID…` (empty clears),
//!   `glueto=OTHER_ID:VA:VB` (glue vertex VA to OTHER's VB);
//!   `glue`: `delete=1`.

use std::path::Path;
use std::sync::Arc;

use keyform_model::{DeformerKind, Model, ObjectInfo, Parameter};

type R = Result<(), Box<dyn std::error::Error>>;

fn find<T>(list: &[Arc<T>], id: &str, get: impl Fn(&T) -> &str) -> Result<usize, String> {
    list.iter()
        .position(|x| get(x) == id)
        .ok_or_else(|| format!("no object {id:?}"))
}

fn info_field(
    info: &mut ObjectInfo,
    field: &str,
    v: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    match field {
        "id" => info.id = v.to_owned(),
        "name" => info.name = v.to_owned(),
        "visible" => info.visible = v.parse()?,
        "locked" => info.locked = v.parse()?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn pair(v: &str) -> Result<[f32; 2], Box<dyn std::error::Error>> {
    let (x, y) = v.split_once(',').ok_or("expected DX,DY")?;
    Ok([x.trim().parse()?, y.trim().parse()?])
}

fn shift(positions: &mut [f32], d: [f32; 2]) {
    for p in positions.chunks_exact_mut(2) {
        p[0] += d[0];
        p[1] += d[1];
    }
}

/// `addkey` / `removekey` on any object with keyforms.
fn key_edit(
    m: &mut Model,
    kind: &str,
    id: &str,
    field: &str,
    value: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    use keyform_edit::ObjectRef;
    if field != "addkey" && field != "removekey" {
        return Ok(false);
    }
    let o = match kind {
        "part" => ObjectRef::Part(find(&m.parts, id, |p| &p.info.id)?),
        "mesh" => ObjectRef::ArtMesh(find(&m.art_meshes, id, |a| &a.info.id)?),
        "deformer" => ObjectRef::Deformer(find(&m.deformers, id, |d| &d.info.id)?),
        "glue" => ObjectRef::Glue(find(&m.glues, id, |g| &g.info.id)?),
        _ => return Err(format!("{kind}: no keyforms").into()),
    };
    let (pid, v) = value.split_once(':').ok_or("expected PARAM_ID:VALUE")?;
    let p = find(&m.parameters, pid, |p| &p.id)?;
    let v: f32 = v.parse()?;
    let changed = if field == "addkey" {
        keyform_edit::add_key(m, o, p, v)?
    } else {
        keyform_edit::remove_key(m, o, p, v)?
    };
    if !changed {
        return Err("no change".into());
    }
    Ok(true)
}

/// Structural edits: `mesh:ID.wrapwarp=NAME`, `mesh:ID.wraprotation=NAME`
/// (a new deformer adopting the mesh, in its part), `part:ID.newpart=NAME`.
fn structural(
    m: &mut Model,
    kind: &str,
    id: &str,
    field: &str,
    value: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    use keyform_edit::objects::{
        create_part, create_rotation_deformer, create_warp_deformer, NewDeformer,
    };
    use keyform_edit::ObjectRef;
    let defaults: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    match (kind, field) {
        ("mesh", "wrapwarp" | "wraprotation") => {
            let a = find(&m.art_meshes, id, |x| &x.info.id)?;
            let children = [ObjectRef::ArtMesh(a)];
            let nd = NewDeformer {
                name: value.to_owned(),
                part: m.art_meshes[a].info.parent_part,
                parent: m.art_meshes[a].target,
                children: &children,
                values: &defaults,
            };
            if field == "wrapwarp" {
                create_warp_deformer(m, &nd, 5, 5, (2, 2))?;
            } else {
                create_rotation_deformer(m, &nd)?;
            }
            Ok(true)
        }
        ("mesh", "addpoint" | "movepoint" | "deletepoint") => {
            use keyform_edit::mesh::{set_mesh, MeshGraph};
            let a = find(&m.art_meshes, id, |x| &x.info.id)?;
            let mut g = MeshGraph::of(&m.art_meshes[a]);
            let xy = |v: &str| -> Result<[f32; 2], Box<dyn std::error::Error>> {
                let (x, y) = v.split_once(',').ok_or("expected X,Y")?;
                Ok([x.trim().parse()?, y.trim().parse()?])
            };
            match field {
                "addpoint" => {
                    let q = xy(value)?;
                    let on_edge = (0..g.mesh.edges.len()).find(|&e| {
                        let [p0, p1] = g.mesh.edges[e].map(|v| g.point(v as usize));
                        let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
                        let len = dx.hypot(dy);
                        let cross = ((q[0] - p0[0]) * dy - (q[1] - p0[1]) * dx).abs();
                        let t = ((q[0] - p0[0]) * dx + (q[1] - p0[1]) * dy) / (len * len);
                        len > 0.0 && cross / len <= 1.0 && (0.0..=1.0).contains(&t)
                    });
                    match on_edge {
                        Some(e) => g.split_edge(e, q),
                        None => g.add_point(q),
                    };
                }
                "movepoint" => {
                    let (k, d) = value.split_once(':').ok_or("expected K:DX,DY")?;
                    let k: usize = k.parse()?;
                    let d = xy(d)?;
                    if k >= g.len() {
                        return Err(format!("{id}: no vertex {k}").into());
                    }
                    let p = g.point(k);
                    g.set_point(k, [p[0] + d[0], p[1] + d[1]]);
                }
                _ => {
                    let k: usize = value.parse()?;
                    g.delete_points(&std::collections::HashSet::from([k]));
                }
            }
            let t = set_mesh(m, a, &g)?;
            if t.unresolved > 0 {
                eprintln!("{id}: {} crossing edges left out", t.unresolved);
            }
            Ok(true)
        }
        ("mesh", "delete") => {
            let a = find(&m.art_meshes, id, |x| &x.info.id)?;
            for g in keyform_edit::objects::delete_art_mesh(m, a)? {
                eprintln!("{id}: glue {g} deleted with it");
            }
            Ok(true)
        }
        ("glue", "delete") => {
            let g = find(&m.glues, id, |x| &x.info.id)?;
            keyform_edit::objects::delete_glue(m, g)?;
            Ok(true)
        }
        ("mesh", "clip") => {
            let a = find(&m.art_meshes, id, |x| &x.info.id)?;
            let ids: Vec<&str> = value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect();
            let dropped = keyform_edit::objects::set_clip_ids(m, a, &ids);
            if !dropped.is_empty() {
                eprintln!(
                    "{id}: {} ({})",
                    keyform_edit::objects::CLIP_IDS_CANCELLED,
                    dropped.join(", ")
                );
            }
            Ok(true)
        }
        ("mesh", "glueto") => {
            let a = find(&m.art_meshes, id, |x| &x.info.id)?;
            let mut it = value.split(':');
            let (Some(other), Some(va), Some(vb)) = (it.next(), it.next(), it.next()) else {
                return Err("expected OTHER_ID:VA:VB".into());
            };
            let b = find(&m.art_meshes, other, |x| &x.info.id)?;
            keyform_edit::objects::bind_vertices(m, (a, va.parse()?), (b, vb.parse()?))?;
            Ok(true)
        }
        ("part", "newpart") => {
            let p = find(&m.parts, id, |x| &x.info.id)?;
            create_part(m, value, Some(p));
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn apply(m: &mut Model, edit: &str) -> R {
    let (target, value) = edit.split_once('=').ok_or("expected KIND:ID.FIELD=VALUE")?;
    let (kind, rest) = target.split_once(':').ok_or("expected KIND:ID.FIELD")?;
    let (id, field) = rest.rsplit_once('.').ok_or("expected ID.FIELD")?;
    if key_edit(m, kind, id, field, value)? || structural(m, kind, id, field, value)? {
        return Ok(());
    }
    let unknown = || format!("{kind}: unknown field {field:?}");
    match kind {
        "param" if field == "create" => {
            // create=MIN,MAX,DEFAULT
            let v: Vec<f32> = value
                .split(',')
                .map(|x| x.trim().parse())
                .collect::<Result<_, _>>()?;
            let [min, max, default] = v[..] else {
                return Err("create=MIN,MAX,DEFAULT".into());
            };
            keyform_edit::objects::add_parameter(m, Parameter::new(id, min, max, default), None);
        }
        "param" if field == "delete" => {
            let i = find(&m.parameters, id, |p| &p.id)?;
            keyform_edit::objects::remove_parameter(m, i);
        }
        "param" => {
            let i = find(&m.parameters, id, |p| &p.id)?;
            let p = Arc::make_mut(&mut m.parameters[i]);
            match field {
                "id" => p.id = value.to_owned(),
                "name" => p.name = value.to_owned(),
                "description" => p.description = value.to_owned(),
                "min" => p.min = value.parse()?,
                "max" => p.max = value.parse()?,
                "default" => p.default = value.parse()?,
                "repeat" => p.repeat = value.parse()?,
                _ => return Err(unknown().into()),
            }
        }
        "part" => {
            let i = find(&m.parts, id, |p| &p.info.id)?;
            let p = Arc::make_mut(&mut m.parts[i]);
            if !info_field(&mut p.info, field, value)? {
                match field {
                    "opacity" => {
                        let o = value.parse()?;
                        p.grid.keyforms.iter_mut().for_each(|f| f.opacity = o);
                    }
                    _ => return Err(unknown().into()),
                }
            }
        }
        "mesh" => {
            let i = find(&m.art_meshes, id, |a| &a.info.id)?;
            let a = Arc::make_mut(&mut m.art_meshes[i]);
            if !info_field(&mut a.info, field, value)? {
                match field {
                    "opacity" => {
                        let o = value.parse()?;
                        a.grid.keyforms.iter_mut().for_each(|f| f.opacity = o);
                    }
                    "move" => {
                        let d = pair(value)?;
                        a.grid
                            .keyforms
                            .iter_mut()
                            .for_each(|f| shift(&mut f.positions, d));
                    }
                    "culling" => a.culling = value.parse()?,
                    "movekeyform" => {
                        // VALUE:dx,dy — the keyform at key VALUE of a
                        // single-binding mesh.
                        let (k, d) = value.split_once(':').ok_or("expected VALUE:dx,dy")?;
                        let k: f32 = k.parse()?;
                        let d = pair(d)?;
                        let b = a.grid.bindings.first().ok_or("the mesh has no keys")?;
                        if a.grid.bindings.len() != 1 {
                            return Err("movekeyform needs a mesh with one binding".into());
                        }
                        let i = b
                            .keys
                            .iter()
                            .position(|x| (x - k).abs() < 1e-4)
                            .ok_or("no such key")?;
                        shift(&mut a.grid.keyforms[i].positions, d);
                    }
                    "multiply" | "screen" => {
                        let c: Vec<f32> = value
                            .split(',')
                            .map(|x| x.trim().parse())
                            .collect::<Result<_, _>>()?;
                        let [r, g, b] = c[..] else {
                            return Err("expected r,g,b".into());
                        };
                        for f in &mut a.grid.keyforms {
                            let col = if field == "multiply" {
                                &mut f.multiply
                            } else {
                                &mut f.screen
                            };
                            *col = [r, g, b, col[3]];
                        }
                    }
                    "blend" => {
                        a.color_composition = keyform_model::ColorComposition::from_name(value)
                            .ok_or("unknown blend name (e.g. MULTIPLY_R2)")?;
                    }
                    "alpha" => {
                        a.alpha_composition = keyform_model::AlphaComposition::from_name(value)
                            .ok_or("unknown alpha name (OVER, ATOP, …)")?;
                    }
                    _ => return Err(unknown().into()),
                }
            }
        }
        "deformer" => {
            let i = find(&m.deformers, id, |d| &d.info.id)?;
            let d = Arc::make_mut(&mut m.deformers[i]);
            if !info_field(&mut d.info, field, value)? {
                match (&mut d.kind, field) {
                    (DeformerKind::Warp(w), "opacity") => {
                        let o = value.parse()?;
                        w.grid.keyforms.iter_mut().for_each(|f| f.opacity = o);
                    }
                    (DeformerKind::Rotation(r), "opacity") => {
                        let o = value.parse()?;
                        r.grid.keyforms.iter_mut().for_each(|f| f.opacity = o);
                    }
                    (DeformerKind::Warp(w), "move") => {
                        let d = pair(value)?;
                        w.grid
                            .keyforms
                            .iter_mut()
                            .for_each(|f| shift(&mut f.positions, d));
                    }
                    (DeformerKind::Rotation(r), "angle+") => {
                        let a: f32 = value.parse()?;
                        r.grid.keyforms.iter_mut().for_each(|f| f.angle += a);
                    }
                    _ => return Err(unknown().into()),
                }
            }
        }
        "glue" => {
            let i = find(&m.glues, id, |g| &g.info.id)?;
            let g = Arc::make_mut(&mut m.glues[i]);
            if !info_field(&mut g.info, field, value)? {
                return Err(unknown().into());
            }
        }
        _ => return Err(format!("unknown kind {kind:?}").into()),
    }
    Ok(())
}

pub fn set_cmd(input: &Path, output: &Path, edits: &[&str]) -> R {
    let mut doc = super::load(input)?;
    for e in edits {
        apply(&mut doc.model, e).map_err(|err| format!("{e}: {err}"))?;
    }
    let edited = doc.model.clone();
    doc.save_to(output)?;
    let back = super::load(output)?;
    if !back.model.same_data(&edited) {
        return Err("re-read model differs from the edited model".into());
    }
    println!(
        "wrote {output:?}: {} edits, model identical after re-read",
        edits.len()
    );
    Ok(())
}
