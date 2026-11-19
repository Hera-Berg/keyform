//! Render order from interpolated draw orders. spec: 30-algorithms/draw-order.md

use keyform_model::{MeshIndex, Model, PartChild, PartIndex};

use crate::{MeshState, PartState};

enum Entry {
    Mesh(MeshIndex),
    ArtPath(usize),
    Alias(usize),
    Group(PartIndex, Vec<Entry>),
}

/// A drawn object in render order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawItem {
    Mesh(MeshIndex),
    ArtPath(usize),
    /// Alias `k` drawing mesh `mi` again.
    AliasMesh(usize, MeshIndex),
    /// Alias `k` drawing art path `p` again.
    AliasArtPath(usize, usize),
    /// Alias placeholder before expansion (not drawn).
    Alias(usize),
}

/// Render order of art meshes and art paths, back to front. Art paths sort
/// with the objects of their parent part by draw order; on ties they are
/// drawn on top (Q-0046). spec: draw-order.md, art-path.md
pub fn render_items(
    m: &Model,
    parts: &[PartState],
    meshes: &[MeshState],
    art_orders: &[f32],
    alias_orders: &[f32],
) -> Vec<DrawItem> {
    let Some(root) = m.root_part.filter(|&r| r < m.parts.len()) else {
        let mut v: Vec<DrawItem> = (0..m.art_meshes.len()).map(DrawItem::Mesh).collect();
        v.extend((0..m.art_paths.len().min(art_orders.len())).map(DrawItem::ArtPath));
        v.extend((0..m.aliases.len().min(alias_orders.len())).map(DrawItem::Alias));
        return v;
    };
    let mut paths_of: Vec<Vec<Entry0>> = vec![Vec::new(); m.parts.len()];
    for (k, a) in m.art_paths.iter().enumerate().take(art_orders.len()) {
        let p = a
            .info
            .parent_part
            .filter(|&p| p < m.parts.len())
            .unwrap_or(root);
        paths_of[p].push(Entry0::ArtPath(k));
    }
    for (k, a) in m.aliases.iter().enumerate().take(alias_orders.len()) {
        let p = a
            .info
            .parent_part
            .filter(|&p| p < m.parts.len())
            .unwrap_or(root);
        paths_of[p].push(Entry0::Alias(k));
    }
    let cx = Cx {
        m,
        paths_of: &paths_of,
    };
    let mut visited = vec![false; m.parts.len()];
    let mut top = Vec::new();
    fill(&cx, root, &mut top, &mut visited);
    let keys = Keys {
        parts,
        meshes,
        art_orders,
        alias_orders,
    };
    sort_group(&mut top, &keys);
    let mut out = Vec::with_capacity(meshes.len());
    flatten(&top, &mut out);
    out
}

/// An art path or alias attached to a part.
#[derive(Clone, Copy)]
enum Entry0 {
    ArtPath(usize),
    Alias(usize),
}

struct Cx<'a> {
    m: &'a Model,
    paths_of: &'a [Vec<Entry0>],
}

struct Keys<'a> {
    parts: &'a [PartState],
    meshes: &'a [MeshState],
    art_orders: &'a [f32],
    alias_orders: &'a [f32],
}

/// Builds the group tree (§1), stable-sorts every group by draw order (§2)
/// and flattens it (§3). Returns art meshes back to front.
pub fn render_order(m: &Model, parts: &[PartState], meshes: &[MeshState]) -> Vec<MeshIndex> {
    meshes_of(&render_items(m, parts, meshes, &[], &[]))
}

/// The art meshes of a render-item list.
pub fn meshes_of(items: &[DrawItem]) -> Vec<MeshIndex> {
    items
        .iter()
        .filter_map(|i| match *i {
            DrawItem::Mesh(mi) => Some(mi),
            _ => None,
        })
        .collect()
}

/// Appends `part`'s children to `group`, inlining parts without a draw-order
/// group, then reverses the group's own list (palette top first → bottom first).
fn fill(cx: &Cx, part: PartIndex, group: &mut Vec<Entry>, visited: &mut [bool]) {
    let start = group.len();
    collect(cx, part, group, visited);
    group[start..].reverse();
}

fn collect(cx: &Cx, part: PartIndex, group: &mut Vec<Entry>, visited: &mut [bool]) {
    let m = cx.m;
    if visited[part] {
        return;
    }
    visited[part] = true;
    // Art paths first: the group is reversed, so they win ties.
    group.extend(
        cx.paths_of
            .get(part)
            .into_iter()
            .flatten()
            .map(|&e| match e {
                Entry0::ArtPath(k) => Entry::ArtPath(k),
                Entry0::Alias(k) => Entry::Alias(k),
            }),
    );
    for &c in &m.parts[part].children {
        match c {
            PartChild::ArtMesh(i) if i < m.art_meshes.len() => group.push(Entry::Mesh(i)),
            PartChild::Part(p) if p < m.parts.len() => {
                if m.parts[p].draw_order_group {
                    let mut sub = Vec::new();
                    fill(cx, p, &mut sub, visited);
                    group.push(Entry::Group(p, sub));
                } else {
                    // Inlined in palette order; the enclosing group reverses once.
                    collect(cx, p, group, visited);
                }
            }
            _ => {}
        }
    }
}

fn key(e: &Entry, k: &Keys) -> f32 {
    match e {
        Entry::Mesh(i) => k.meshes.get(*i).map_or(0.0, |s| s.draw_order),
        Entry::ArtPath(i) => k.art_orders.get(*i).copied().unwrap_or(0.0),
        Entry::Alias(i) => k.alias_orders.get(*i).copied().unwrap_or(0.0),
        Entry::Group(p, _) => k.parts.get(*p).map_or(0.0, |s| s.form.draw_order),
    }
}

fn sort_group(g: &mut [Entry], k: &Keys) {
    // Stable ascending sort: ties keep the reversed palette order, so the
    // entry higher in the palette is drawn on top.
    g.sort_by(|a, b| key(a, k).total_cmp(&key(b, k)));
    for e in g.iter_mut() {
        if let Entry::Group(_, sub) = e {
            sort_group(sub, k);
        }
    }
}

fn flatten(g: &[Entry], out: &mut Vec<DrawItem>) {
    for e in g {
        match e {
            Entry::Mesh(i) => out.push(DrawItem::Mesh(*i)),
            Entry::ArtPath(i) => out.push(DrawItem::ArtPath(*i)),
            Entry::Alias(i) => out.push(DrawItem::Alias(*i)),
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
            clip_missing: false,
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
            mesh: EditableMesh::default(),
            model_image: None,
            generator: Default::default(),
            grid: KeyformGrid::single(ArtMeshForm::default()),
            blend_shapes: BlendShapes::default(),
            paths: Vec::new(),
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
