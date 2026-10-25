//! Render order from interpolated draw orders. spec: 30-algorithms/draw-order.md

use keyform_model::{MeshIndex, Model, PartChild, PartIndex};

use crate::{MeshState, PartState};

enum Entry {
    Mesh(MeshIndex),
    Group(PartIndex, Vec<Entry>),
}

/// Builds the group tree (§1), stable-sorts every group by draw order (§2)
/// and flattens it (§3). Returns art meshes back to front.
pub fn render_order(m: &Model, parts: &[PartState], meshes: &[MeshState]) -> Vec<MeshIndex> {
    let Some(root) = m.root_part.filter(|&r| r < m.parts.len()) else {
        // No part tree: model order.
        return (0..m.art_meshes.len()).collect();
    };
    let mut visited = vec![false; m.parts.len()];
    let mut top = Vec::new();
    fill(m, root, &mut top, &mut visited);
    sort_group(&mut top, parts, meshes);
    let mut out = Vec::with_capacity(meshes.len());
    flatten(&top, &mut out);
    out
}

/// Appends `part`'s children to `group`, inlining parts without a draw-order
/// group, then reverses the group's own list (palette top first → bottom first).
fn fill(m: &Model, part: PartIndex, group: &mut Vec<Entry>, visited: &mut [bool]) {
    let start = group.len();
    collect(m, part, group, visited);
    group[start..].reverse();
}

fn collect(m: &Model, part: PartIndex, group: &mut Vec<Entry>, visited: &mut [bool]) {
    if visited[part] {
        return;
    }
    visited[part] = true;
    for &c in &m.parts[part].children {
        match c {
            PartChild::ArtMesh(i) if i < m.art_meshes.len() => group.push(Entry::Mesh(i)),
            PartChild::Part(p) if p < m.parts.len() => {
                if m.parts[p].draw_order_group {
                    let mut sub = Vec::new();
                    fill(m, p, &mut sub, visited);
                    group.push(Entry::Group(p, sub));
                } else {
                    // Inlined in palette order; the enclosing group reverses once.
                    collect(m, p, group, visited);
                }
            }
            _ => {}
        }
    }
}

fn key(e: &Entry, parts: &[PartState], meshes: &[MeshState]) -> f32 {
    match e {
        Entry::Mesh(i) => meshes.get(*i).map_or(0.0, |s| s.draw_order),
        Entry::Group(p, _) => parts.get(*p).map_or(0.0, |s| s.form.draw_order),
    }
}

fn sort_group(g: &mut [Entry], parts: &[PartState], meshes: &[MeshState]) {
    // Stable ascending sort: ties keep the reversed palette order, so the
    // entry higher in the palette is drawn on top.
    g.sort_by(|a, b| key(a, parts, meshes).total_cmp(&key(b, parts, meshes)));
    for e in g.iter_mut() {
        if let Entry::Group(_, sub) = e {
            sort_group(sub, parts, meshes);
        }
    }
}

fn flatten(g: &[Entry], out: &mut Vec<MeshIndex>) {
    for e in g {
        match e {
            Entry::Mesh(i) => out.push(*i),
            Entry::Group(_, sub) => flatten(sub, out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::*;

    fn mesh(id: &str, d: f32) -> (ArtMesh, MeshState) {
        let a = ArtMesh {
            info: ObjectInfo::new(id),
            target: None,
            positions: vec![],
            uvs: vec![],
            indices: vec![],
            texture: None,
            clip: vec![],
            invert_clip: false,
            culling: false,
            color_composition: ColorComposition::Normal,
            alpha_composition: AlphaComposition::Over,
            user_data: String::new(),
            point_uids: vec![],
            grid: KeyformGrid::single(ArtMeshForm::default()),
            blend_shapes: BlendShapes::default(),
        };
        let s = MeshState {
            positions: vec![],
            draw_order: d,
            form_opacity: 1.0,
            opacity: 1.0,
            multiply: [1.0; 3],
            screen: [0.0; 3],
            outside: false,
            hidden: false,
        };
        (a, s)
    }

    #[test]
    fn ties_put_palette_top_in_front_and_groups_move_as_block() {
        let mut m = Model::default();
        let mut states = Vec::new();
        for (id, d) in [
            ("top", 500.0),
            ("mid", 500.0),
            ("low", 400.0),
            ("g1", 900.0),
            ("g2", 100.0),
        ] {
            let (a, s) = mesh(id, d);
            m.art_meshes.push(a.into());
            states.push(s);
        }
        let mut root = Part::new("__RootPart__");
        let mut inl = Part::new("Inline");
        let mut grp = Part::new("Group");
        grp.draw_order_group = true;
        // Group part at draw order 450: between "low" (400) and the 500s.
        grp.grid = KeyformGrid::single(PartForm {
            draw_order: 450.0,
            ..PartForm::default()
        });
        root.children = vec![
            PartChild::ArtMesh(0),
            PartChild::Part(1),
            PartChild::Part(2),
        ];
        inl.children = vec![PartChild::ArtMesh(1), PartChild::ArtMesh(2)];
        grp.children = vec![PartChild::ArtMesh(3), PartChild::ArtMesh(4)];
        m.parts = vec![root.into(), inl.into(), grp.into()];
        m.root_part = Some(0);
        let parts: Vec<PartState> = m
            .parts
            .iter()
            .map(|p| PartState {
                form: p.grid.keyforms[0].clone(),
                outside: false,
                visible: true,
            })
            .collect();
        let order = render_order(&m, &parts, &states);
        // Back to front: low(400), [group 450: g2(100), g1(900)], mid(500), top(500, palette top).
        assert_eq!(order, vec![2, 4, 3, 1, 0]);
    }
}
