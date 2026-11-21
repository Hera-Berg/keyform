//! Edit ▸ Cut / Copy / Paste of objects and Edit ▸ Copy / Paste Parameter
//! Value. spec: 35-editing/object-clipboard.md (§1 commands, §3 captured
//! data, §4 paste, §4.1 numbering); 50-ui/menus.md "Behaviour of simple
//! commands".
//!
//! Not yet: art paths, aliases and art-path brushes aren't copied (§3), and
//! atlases brought from another document aren't repacked (§4 step 5: the
//! source page is copied with only the copied images' entries).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use keyform_model::{
    new_guid, ArtMesh, Deformer, DeformerIndex, DeformerKind, Glue, Guid, LayeredImage, Model,
    ModelImage, ModelImageGroup, Parameter, Part, PartChild, PartIndex, Texture,
};

use crate::keys::ObjectRef;

/// One captured object with the identities it refers to (indices change
/// between copy and paste, and differ between documents).
#[derive(Clone, Debug)]
pub enum CopiedObject {
    Part(Arc<Part>),
    ArtMesh {
        mesh: Arc<ArtMesh>,
        /// Clipping masks (GUIDs).
        clips: Vec<Guid>,
        /// The mesh's model image and atlas page, for pasting into another
        /// document (§3 modelImages / textureAtlases / rawImages).
        image: Option<Arc<ModelImage>>,
        group: Option<ModelImageGroup>,
        layered: Option<Arc<LayeredImage>>,
        texture: Option<Arc<Texture>>,
    },
    Deformer(Arc<Deformer>),
    Glue {
        glue: Arc<Glue>,
        meshes: [Option<Guid>; 2],
    },
}

/// A captured object with its parents (GUIDs).
#[derive(Clone, Debug)]
pub struct Copied {
    pub object: CopiedObject,
    pub parent_part: Option<Guid>,
    /// Target deformer (deformers and art meshes).
    pub target: Option<Guid>,
}

impl Copied {
    fn guid(&self) -> &Guid {
        match &self.object {
            CopiedObject::Part(p) => &p.info.guid,
            CopiedObject::ArtMesh { mesh, .. } => &mesh.info.guid,
            CopiedObject::Deformer(d) => &d.info.guid,
            CopiedObject::Glue { glue, .. } => &glue.info.guid,
        }
    }

    pub fn display_name(&self) -> &str {
        match &self.object {
            CopiedObject::Part(p) => p.info.display_name(),
            CopiedObject::ArtMesh { mesh, .. } => mesh.info.display_name(),
            CopiedObject::Deformer(d) => d.info.display_name(),
            CopiedObject::Glue { glue, .. } => glue.info.display_name(),
        }
    }
}

/// What Edit ▸ Copy captured (`PartExportData`, §3).
#[derive(Clone, Debug)]
pub struct ObjectClipboard {
    /// The source document instance (decides same-document paste).
    pub source: u64,
    /// The source model's parameters (bindings refer to these indices).
    pub parameters: Vec<Arc<Parameter>>,
    /// The copied objects in model tree order.
    pub objects: Vec<Copied>,
    /// Parent deformers that weren't copied, with their own targets.
    pub target_deformers: Vec<(Arc<Deformer>, Option<Guid>)>,
}

impl ObjectClipboard {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

// ------------------------------------------------------------------ order

/// Every object in Parts-palette order (depth first from the root part, a
/// part before its children), then anything outside the part tree.
pub fn palette_order(m: &Model) -> Vec<ObjectRef> {
    fn walk(m: &Model, p: PartIndex, out: &mut Vec<ObjectRef>, guard: &mut usize) {
        *guard += 1;
        if *guard > m.parts.len() + 1 {
            return;
        }
        out.push(ObjectRef::Part(p));
        for c in &m.parts[p].children {
            match *c {
                PartChild::Part(q) if q < m.parts.len() => walk(m, q, out, guard),
                PartChild::Deformer(d) => out.push(ObjectRef::Deformer(d)),
                PartChild::ArtMesh(a) => out.push(ObjectRef::ArtMesh(a)),
                PartChild::Glue(g) => out.push(ObjectRef::Glue(g)),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    if let Some(r) = m.root_part {
        let mut guard = 0;
        walk(m, r, &mut out, &mut guard);
    }
    let seen: HashSet<ObjectRef> = out.iter().copied().collect();
    let rest = (0..m.parts.len())
        .map(ObjectRef::Part)
        .chain((0..m.deformers.len()).map(ObjectRef::Deformer))
        .chain((0..m.art_meshes.len()).map(ObjectRef::ArtMesh))
        .chain((0..m.glues.len()).map(ObjectRef::Glue))
        .filter(|o| !seen.contains(o))
        .collect::<Vec<_>>();
    out.extend(rest);
    out
}

/// Every object below part `p` (all depths).
fn descendants(m: &Model, p: PartIndex) -> Vec<ObjectRef> {
    let order = palette_order(m);
    let Some(start) = order.iter().position(|o| *o == ObjectRef::Part(p)) else {
        return Vec::new();
    };
    let inside = |o: ObjectRef| {
        let mut part = parent_part(m, o);
        let mut guard = 0;
        while let Some(q) = part {
            if q == p {
                return true;
            }
            part = m.parts.get(q).and_then(|x| x.info.parent_part);
            guard += 1;
            if guard > m.parts.len() {
                break;
            }
        }
        false
    };
    order[start + 1..]
        .iter()
        .copied()
        .filter(|&o| inside(o))
        .collect()
}

fn parent_part(m: &Model, o: ObjectRef) -> Option<PartIndex> {
    match o {
        ObjectRef::Part(i) => m.parts.get(i)?.info.parent_part,
        ObjectRef::Deformer(i) => m.deformers.get(i)?.info.parent_part,
        ObjectRef::ArtMesh(i) => m.art_meshes.get(i)?.info.parent_part,
        ObjectRef::Glue(i) => m.glues.get(i)?.info.parent_part,
    }
}

// ------------------------------------------------------------------- copy

/// Whether Copy must ask "Do you want to copy part's child objects?": a
/// selected part has descendants that aren't selected (§1).
pub fn needs_children_prompt(m: &Model, selection: &[ObjectRef]) -> bool {
    selection.iter().any(|&o| match o {
        ObjectRef::Part(p) => descendants(m, p).iter().any(|d| !selection.contains(d)),
        _ => false,
    })
}

/// Edit ▸ Copy (§1): the selection (plus, with `with_children`, the
/// descendants of selected parts; selected glue adds both of its meshes)
/// in model tree order. `None` when nothing can be copied.
pub fn copy_objects(
    m: &Model,
    selection: &[ObjectRef],
    source: u64,
    with_children: bool,
) -> Option<ObjectClipboard> {
    let mut set: HashSet<ObjectRef> = selection.iter().copied().collect();
    for &o in selection {
        match o {
            ObjectRef::Part(p) if with_children => set.extend(descendants(m, p)),
            ObjectRef::Glue(g) => {
                if let Some(x) = m.glues.get(g) {
                    set.extend(x.mesh_a.map(ObjectRef::ArtMesh));
                    set.extend(x.mesh_b.map(ObjectRef::ArtMesh));
                }
            }
            _ => {}
        }
    }
    if let Some(r) = m.root_part {
        set.remove(&ObjectRef::Part(r));
    }
    let order: Vec<ObjectRef> = palette_order(m)
        .into_iter()
        .filter(|o| set.contains(o))
        .collect();
    let dguid = |d: Option<DeformerIndex>| {
        d.and_then(|d| m.deformers.get(d))
            .map(|x| x.info.guid.clone())
    };
    let pguid = |p: Option<PartIndex>| p.and_then(|p| m.parts.get(p)).map(|x| x.info.guid.clone());
    let mguid = |a: Option<usize>| {
        a.and_then(|a| m.art_meshes.get(a))
            .map(|x| x.info.guid.clone())
    };
    let mut objects = Vec::new();
    let mut copied_defs = HashSet::new();
    for &o in &order {
        let c = match o {
            ObjectRef::Part(p) => {
                let x = &m.parts[p];
                Copied {
                    object: CopiedObject::Part(x.clone()),
                    parent_part: pguid(x.info.parent_part),
                    target: None,
                }
            }
            ObjectRef::Deformer(d) => {
                let x = &m.deformers[d];
                copied_defs.insert(d);
                Copied {
                    object: CopiedObject::Deformer(x.clone()),
                    parent_part: pguid(x.info.parent_part),
                    target: dguid(x.target),
                }
            }
            ObjectRef::ArtMesh(a) => {
                let x = &m.art_meshes[a];
                let image = x.model_image.and_then(|i| m.model_images.get(i)).cloned();
                let group = image
                    .as_ref()
                    .and_then(|i| i.group)
                    .and_then(|g| m.model_image_groups.get(g))
                    .map(|g| (**g).clone());
                let layered = image
                    .as_ref()
                    .and_then(|i| i.source.as_ref())
                    .and_then(|(l, _)| m.layered_images.iter().find(|x| x.guid == *l))
                    .cloned();
                Copied {
                    object: CopiedObject::ArtMesh {
                        mesh: x.clone(),
                        clips: x.clip.iter().filter_map(|&c| mguid(Some(c))).collect(),
                        image,
                        group,
                        layered,
                        texture: x.texture.and_then(|t| m.textures.get(t)).cloned(),
                    },
                    parent_part: pguid(x.info.parent_part),
                    target: dguid(x.target),
                }
            }
            ObjectRef::Glue(g) => {
                let x = &m.glues[g];
                Copied {
                    object: CopiedObject::Glue {
                        glue: x.clone(),
                        meshes: [mguid(x.mesh_a), mguid(x.mesh_b)],
                    },
                    parent_part: pguid(x.info.parent_part),
                    target: None,
                }
            }
        };
        objects.push(c);
    }
    if objects.is_empty() {
        return None;
    }
    // Parent deformers that weren't copied (every uncopied ancestor).
    let mut target_deformers = Vec::new();
    let mut seen = HashSet::new();
    for &o in &order {
        let mut t = match o {
            ObjectRef::Deformer(d) => m.deformers[d].target,
            ObjectRef::ArtMesh(a) => m.art_meshes[a].target,
            _ => None,
        };
        let mut guard = 0;
        while let Some(d) = t {
            if !copied_defs.contains(&d) && seen.insert(d) {
                let x = &m.deformers[d];
                target_deformers.push((x.clone(), dguid(x.target)));
            }
            t = m.deformers.get(d).and_then(|x| x.target);
            guard += 1;
            if guard > m.deformers.len() {
                break;
            }
        }
    }
    Some(ObjectClipboard {
        source,
        parameters: m.parameters.clone(),
        objects,
        target_deformers,
    })
}

// -------------------------------------------------------------- numbering

/// Splits off the trailing decimal digits.
fn split_number(s: &str) -> (&str, &str) {
    let cut = s
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map_or(s.len(), |(i, _)| i);
    s.split_at(cut)
}

/// ID numbering (§4.1): `id` if free; else the stem plus a counter that
/// starts at the ID's own trailing number (or 2), keeps its digit width
/// and grows only on overflow.
pub fn copy_id<'a>(id: &str, taken: impl Iterator<Item = &'a str>) -> String {
    let used: HashSet<&str> = taken.collect();
    if !id.is_empty() && !used.contains(id) {
        return id.to_owned();
    }
    let (stem, digits) = split_number(id);
    let (mut n, width): (u64, usize) = match digits.parse() {
        Ok(v) => (v, digits.len()),
        Err(_) => (2, 1),
    };
    loop {
        let c = format!("{stem}{n:0width$}");
        if !used.contains(c.as_str()) {
            return c;
        }
        n += 1;
    }
}

/// Name numbering (§4.1, with the paste-numbering setting): `name` if
/// free; else the trailing number + 1 (or 2), keeping its zero padding.
pub fn number_name<'a>(name: &str, taken: impl Iterator<Item = &'a str>) -> String {
    let used: HashSet<&str> = taken.collect();
    if !used.contains(name) {
        return name.to_owned();
    }
    let (stem, digits) = split_number(name);
    let (mut n, width): (u64, usize) = match digits.parse::<u64>() {
        Ok(v) => (v + 1, digits.len()),
        Err(_) => (2, 1),
    };
    loop {
        let c = format!("{stem}{n:0width$}");
        if !used.contains(c.as_str()) {
            return c;
        }
        n += 1;
    }
}

/// Old-style parameter IDs are equivalent to new ones (§4 step 4): an ID
/// without lowercase letters is split on `_`, each word capitalised and
/// joined (leading `@`s kept). `PARAM_ANGLE_X` ≡ `ParamAngleX`.
pub fn normalise_param_id(id: &str) -> String {
    if id.chars().any(|c| c.is_lowercase()) {
        return id.to_owned();
    }
    let at = id.len() - id.trim_start_matches('@').len();
    let mut out = id[..at].to_owned();
    for w in id[at..].split('_').filter(|w| !w.is_empty()) {
        let mut ch = w.chars();
        if let Some(f) = ch.next() {
            out.extend(f.to_uppercase());
            out.push_str(&ch.as_str().to_lowercase());
        }
    }
    out
}

// ------------------------------------------------------------------ paste

/// Remaps every parameter index of a grid and blend-shape set.
macro_rules! remap_params {
    ($grid:expr, $blend:expr, $f:expr) => {{
        for b in &mut $grid.bindings {
            b.parameter = $f(b.parameter);
        }
        for s in &mut $blend.shapes {
            s.parameter = $f(s.parameter);
        }
        for c in &mut $blend.constraints {
            c.blend_parameter = $f(c.blend_parameter);
            c.constraint_parameter = $f(c.constraint_parameter);
        }
        for x in &mut $blend.extended {
            x.parameter = $f(x.parameter);
        }
    }};
}

/// The parameters (source indices) an object's keyforms use.
fn params_of(o: &CopiedObject, out: &mut BTreeSet<usize>) {
    macro_rules! collect {
        ($grid:expr, $blend:expr) => {{
            out.extend($grid.bindings.iter().map(|b| b.parameter));
            out.extend($blend.shapes.iter().map(|s| s.parameter));
            for c in &$blend.constraints {
                out.insert(c.blend_parameter);
                out.insert(c.constraint_parameter);
            }
        }};
    }
    match o {
        CopiedObject::Part(p) => collect!(p.grid, p.blend_shapes),
        CopiedObject::ArtMesh { mesh, .. } => collect!(mesh.grid, mesh.blend_shapes),
        CopiedObject::Glue { glue, .. } => collect!(glue.grid, glue.blend_shapes),
        CopiedObject::Deformer(d) => deformer_params(d, out),
    }
}

fn deformer_params(d: &Deformer, out: &mut BTreeSet<usize>) {
    match &d.kind {
        DeformerKind::Warp(w) => {
            out.extend(w.grid.bindings.iter().map(|b| b.parameter));
            out.extend(w.blend_shapes.shapes.iter().map(|s| s.parameter));
            out.extend(w.sway.iter().map(|s| s.parameter));
        }
        DeformerKind::Rotation(r) => {
            out.extend(r.grid.bindings.iter().map(|b| b.parameter));
            out.extend(r.blend_shapes.shapes.iter().map(|s| s.parameter));
        }
    }
}

fn remap_deformer(d: &mut Deformer, f: &dyn Fn(usize) -> usize) {
    match &mut d.kind {
        DeformerKind::Warp(w) => {
            remap_params!(w.grid, w.blend_shapes, f);
            for s in &mut w.sway {
                s.parameter = f(s.parameter);
            }
            w.grid.guids = w.grid.guids.iter().map(|_| new_guid()).collect();
        }
        DeformerKind::Rotation(r) => {
            remap_params!(r.grid, r.blend_shapes, f);
            r.grid.guids = r.grid.guids.iter().map(|_| new_guid()).collect();
        }
    }
}

#[cfg(test)]
fn index_of(o: ObjectRef) -> usize {
    match o {
        ObjectRef::Part(i)
        | ObjectRef::Deformer(i)
        | ObjectRef::ArtMesh(i)
        | ObjectRef::Glue(i) => i,
    }
}

/// The result of a paste: the new objects (in clipboard order) and objects
/// that couldn't be pasted.
#[derive(Debug, Default)]
pub struct Pasted {
    pub objects: Vec<ObjectRef>,
    pub warnings: Vec<String>,
}

/// Edit ▸ Paste (§4) into `m` (document instance `dest`), anchored at the
/// top-most object of `selection` in palette order. `numbered`: the
/// paste-numbering setting (names).
pub fn paste_objects(
    m: &mut Model,
    clip: &ObjectClipboard,
    selection: &[ObjectRef],
    dest: u64,
    numbered: bool,
) -> Result<Pasted, String> {
    if clip.objects.is_empty() {
        return Err("Unable to copy".into());
    }
    let same_doc = clip.source == dest;
    let mut warnings = Vec::new();

    // Insertion point: the top-most selected object in palette order.
    let order = palette_order(m);
    let anchor = order.iter().copied().find(|o| selection.contains(o));
    let (ins_part, mut ins_index) = match anchor {
        Some(ObjectRef::Part(p)) => (Some(p), 0),
        Some(o) => {
            let p = parent_part(m, o);
            let child = match o {
                ObjectRef::Deformer(i) => PartChild::Deformer(i),
                ObjectRef::ArtMesh(i) => PartChild::ArtMesh(i),
                ObjectRef::Glue(i) => PartChild::Glue(i),
                ObjectRef::Part(i) => PartChild::Part(i),
            };
            let at = p
                .and_then(|p| m.parts.get(p))
                .and_then(|x| x.children.iter().position(|c| *c == child))
                .unwrap_or(0);
            (p.or(m.root_part), at)
        }
        None => (m.root_part, 0),
    };

    // Art meshes that can't bring their image along are left out (§4
    // step 5), with glue that needs them.
    let mut skip: HashSet<Guid> = HashSet::new();
    if !same_doc {
        for c in &clip.objects {
            if let CopiedObject::ArtMesh {
                mesh,
                image,
                texture,
                ..
            } = &c.object
            {
                let reusable = image.as_ref().is_some_and(|i| {
                    i.source.is_some() && m.model_images.iter().any(|d| d.source == i.source)
                });
                // Without an atlas page the mesh draws from its model image.
                let carried = image.as_ref().is_some_and(|i| i.png.is_some())
                    && texture.as_ref().is_none_or(|t| t.png.is_some());
                if image.is_some() && !reusable && !carried {
                    warnings.push(format!(
                        "{} cannot be pasted. Duplicating image material between different \
                         models is not supported.",
                        mesh.info.display_name()
                    ));
                    skip.insert(mesh.info.guid.clone());
                }
            }
        }
        for c in &clip.objects {
            if let CopiedObject::Glue { glue, meshes } = &c.object {
                if meshes.iter().flatten().any(|g| skip.contains(g)) {
                    skip.insert(glue.info.guid.clone());
                }
            }
        }
    }
    let objects: Vec<&Copied> = clip
        .objects
        .iter()
        .filter(|c| !skip.contains(c.guid()))
        .collect();
    if objects.is_empty() {
        return Ok(Pasted {
            objects: Vec::new(),
            warnings,
        });
    }

    // Parameters (§4 step 4): those of the copies, then those of captured
    // parent deformers (added as TEMPORARY_PARAM_n when missing).
    let mut used = BTreeSet::new();
    for c in &objects {
        params_of(&c.object, &mut used);
    }
    let mut target_only = BTreeSet::new();
    for (d, _) in &clip.target_deformers {
        deformer_params(d, &mut target_only);
    }
    let target_only: Vec<usize> = target_only.difference(&used).copied().collect();
    let mut pmap: HashMap<usize, usize> = HashMap::new();
    let mut temp_params = Vec::new();
    for &sp in &used {
        let Some(p) = clip.parameters.get(sp) else {
            return Err("A parameter of the copied objects is missing.".into());
        };
        let by_id = m.parameters.iter().position(|q| q.id == p.id);
        let by_guid = m.parameters.iter().position(|q| q.guid == p.guid);
        let n = normalise_param_id(&p.id);
        let by_equiv = m
            .parameters
            .iter()
            .position(|q| normalise_param_id(&q.id) == n);
        let i = match (by_id.or(by_equiv), by_guid) {
            (Some(i), _) => i,
            (None, Some(_)) => {
                let mut q = (**p).clone();
                q.guid = new_guid();
                q.group = None;
                crate::objects::add_parameter(m, q, None)
            }
            (None, None) => {
                let mut q = (**p).clone();
                q.group = None;
                crate::objects::add_parameter(m, q, None)
            }
        };
        pmap.insert(sp, i);
    }
    for (n, &sp) in target_only.iter().enumerate() {
        let Some(p) = clip.parameters.get(sp) else {
            continue;
        };
        if let Some(i) = m.parameters.iter().position(|q| q.id == p.id) {
            pmap.insert(sp, i);
            continue;
        }
        let mut q = (**p).clone();
        q.guid = new_guid();
        q.group = None;
        q.id = copy_id(
            &format!("TEMPORARY_PARAM_{n}"),
            m.parameters.iter().map(|x| x.id.as_str()),
        );
        let i = crate::objects::add_parameter(m, q, None);
        temp_params.push(i);
        pmap.insert(sp, i);
    }
    let pf = |i: usize| pmap.get(&i).copied().unwrap_or(i);

    // New indices of the copies (appended), by source GUID.
    let mut new_ref: HashMap<Guid, ObjectRef> = HashMap::new();
    let (mut np, mut nd, mut na, mut ng) = (
        m.parts.len(),
        m.deformers.len(),
        m.art_meshes.len(),
        m.glues.len(),
    );
    for c in &objects {
        let r = match c.object {
            CopiedObject::Part(_) => {
                np += 1;
                ObjectRef::Part(np - 1)
            }
            CopiedObject::Deformer(_) => {
                nd += 1;
                ObjectRef::Deformer(nd - 1)
            }
            CopiedObject::ArtMesh { .. } => {
                na += 1;
                ObjectRef::ArtMesh(na - 1)
            }
            CopiedObject::Glue { .. } => {
                ng += 1;
                ObjectRef::Glue(ng - 1)
            }
        };
        new_ref.insert(c.guid().clone(), r);
    }

    // Target deformers (§4 step 2): a copy, else the same GUID here, else
    // a deformer here with the captured parent's ID, else the captured
    // parent itself, added temporarily.
    let first_temp = nd;
    let mut temps: Vec<(usize, Option<Guid>)> = Vec::new();
    let mut temp_index: HashMap<Guid, DeformerIndex> = HashMap::new();
    let mut resolve = |g: &Option<Guid>,
                       m: &Model,
                       temps: &mut Vec<(usize, Option<Guid>)>|
     -> Option<DeformerIndex> {
        let g = g.as_ref()?;
        if let Some(ObjectRef::Deformer(d)) = new_ref.get(g) {
            return Some(*d);
        }
        if let Some(d) = m.deformers.iter().position(|x| x.info.guid == *g) {
            return Some(d);
        }
        if let Some(&d) = temp_index.get(g) {
            return Some(d);
        }
        let k = clip
            .target_deformers
            .iter()
            .position(|(d, _)| d.info.guid == *g)?;
        let captured = &clip.target_deformers[k].0;
        if let Some(d) = m
            .deformers
            .iter()
            .position(|x| x.info.id == captured.info.id)
        {
            return Some(d);
        }
        let d = first_temp + temps.len();
        temps.push((k, clip.target_deformers[k].1.clone()));
        temp_index.insert(g.clone(), d);
        Some(d)
    };
    let mut targets: Vec<Option<DeformerIndex>> = Vec::new();
    for c in &objects {
        targets.push(match c.object {
            CopiedObject::Deformer(_) | CopiedObject::ArtMesh { .. } => {
                resolve(&c.target, m, &mut temps)
            }
            _ => None,
        });
    }
    // Temporary deformers' own targets (they may add further temporaries).
    let mut temp_targets: Vec<Option<DeformerIndex>> = Vec::new();
    let mut k = 0;
    while k < temps.len() {
        let t = temps[k].1.clone();
        let r = resolve(&t, m, &mut temps);
        temp_targets.push(r);
        k += 1;
    }

    // Insert the copies (§4 steps 3 and 5).
    let mut made = Vec::new();
    let mut new_textures: HashMap<Guid, usize> = HashMap::new();
    let mut top_level: Vec<PartChild> = Vec::new();
    for (c, &target) in objects.iter().zip(&targets) {
        let in_copied_part = match c.parent_part.as_ref().and_then(|g| new_ref.get(g)) {
            Some(ObjectRef::Part(p)) => Some(*p),
            _ => None,
        };
        let part = in_copied_part.or(ins_part);
        let child = match &c.object {
            CopiedObject::Part(src) => {
                let mut p = (**src).clone();
                p.children.clear();
                p.info.id = copy_id(&p.info.id, m.parts.iter().map(|x| x.info.id.as_str()));
                if m.parts.iter().any(|x| x.info.guid == p.info.guid) {
                    p.info.guid = new_guid();
                }
                if numbered {
                    p.info.name =
                        number_name(&p.info.name, m.parts.iter().map(|x| x.info.name.as_str()));
                }
                p.info.parent_part = part;
                if !same_doc {
                    p.clip.clear();
                }
                remap_params!(p.grid, p.blend_shapes, pf);
                p.grid.guids = p.grid.guids.iter().map(|_| new_guid()).collect();
                m.parts.push(Arc::new(p));
                PartChild::Part(m.parts.len() - 1)
            }
            CopiedObject::Deformer(src) => {
                let mut d = (**src).clone();
                d.info.id = copy_id(&d.info.id, m.deformers.iter().map(|x| x.info.id.as_str()));
                if m.deformers.iter().any(|x| x.info.guid == d.info.guid) {
                    d.info.guid = new_guid();
                }
                if numbered {
                    d.info.name = number_name(
                        &d.info.name,
                        m.deformers.iter().map(|x| x.info.name.as_str()),
                    );
                }
                d.info.parent_part = part;
                d.target = target;
                remap_deformer(&mut d, &pf);
                m.deformers.push(Arc::new(d));
                PartChild::Deformer(m.deformers.len() - 1)
            }
            CopiedObject::ArtMesh {
                mesh,
                clips,
                image,
                group,
                layered,
                texture,
            } => {
                let mut a = (**mesh).clone();
                a.info.id = copy_id(&a.info.id, m.art_meshes.iter().map(|x| x.info.id.as_str()));
                if m.art_meshes.iter().any(|x| x.info.guid == a.info.guid) {
                    a.info.guid = new_guid();
                }
                if numbered {
                    a.info.name = number_name(
                        &a.info.name,
                        m.art_meshes.iter().map(|x| x.info.name.as_str()),
                    );
                }
                a.info.parent_part = part;
                a.target = target;
                a.clip = clips
                    .iter()
                    .filter_map(|g| match new_ref.get(g) {
                        Some(ObjectRef::ArtMesh(i)) => Some(*i),
                        _ => m.art_meshes.iter().position(|x| x.info.guid == *g),
                    })
                    .collect();
                if !same_doc {
                    bring_image(m, &mut a, image, group, layered, texture, &mut new_textures);
                } else if a.texture.is_some_and(|t| t >= m.textures.len())
                    || a.model_image.is_some_and(|t| t >= m.model_images.len())
                {
                    return Err(format!(
                        "{} cannot be pasted: its image no longer exists.",
                        a.info.display_name()
                    ));
                }
                remap_params!(a.grid, a.blend_shapes, pf);
                a.grid.guids = a.grid.guids.iter().map(|_| new_guid()).collect();
                m.art_meshes.push(Arc::new(a));
                PartChild::ArtMesh(m.art_meshes.len() - 1)
            }
            CopiedObject::Glue { glue, meshes } => {
                let mut g = (**glue).clone();
                g.info.id = copy_id(&g.info.id, m.glues.iter().map(|x| x.info.id.as_str()));
                if m.glues.iter().any(|x| x.info.guid == g.info.guid) {
                    g.info.guid = new_guid();
                }
                if numbered {
                    g.info.name =
                        number_name(&g.info.name, m.glues.iter().map(|x| x.info.name.as_str()));
                }
                g.info.parent_part = part;
                let mesh = |x: &Option<Guid>| -> Option<usize> {
                    let x = x.as_ref()?;
                    match new_ref.get(x) {
                        Some(ObjectRef::ArtMesh(i)) => Some(*i),
                        _ => m.art_meshes.iter().position(|y| y.info.guid == *x),
                    }
                };
                g.mesh_a = mesh(&meshes[0]);
                g.mesh_b = mesh(&meshes[1]);
                remap_params!(g.grid, g.blend_shapes, pf);
                g.grid.guids = g.grid.guids.iter().map(|_| new_guid()).collect();
                m.glues.push(Arc::new(g));
                PartChild::Glue(m.glues.len() - 1)
            }
        };
        match in_copied_part {
            Some(p) => Arc::make_mut(&mut m.parts[p]).children.push(child),
            None => top_level.push(child),
        }
        made.push(match child {
            PartChild::Part(i) => ObjectRef::Part(i),
            PartChild::Deformer(i) => ObjectRef::Deformer(i),
            PartChild::ArtMesh(i) => ObjectRef::ArtMesh(i),
            PartChild::Glue(i) => ObjectRef::Glue(i),
            _ => continue,
        });
    }
    // Temporary deformers (§4 step 6), at the insertion point.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_millis());
    let mut temp_defs = Vec::new();
    for (n, ((k, _), t)) in temps.iter().zip(&temp_targets).enumerate() {
        let mut d = (*clip.target_deformers[*k].0).clone();
        d.info.id = copy_id(
            &format!("temporaryDeformer_{n}_{stamp}"),
            m.deformers.iter().map(|x| x.info.id.as_str()),
        );
        if m.deformers.iter().any(|x| x.info.guid == d.info.guid) {
            d.info.guid = new_guid();
        }
        d.info.parent_part = ins_part;
        d.target = *t;
        remap_deformer(&mut d, &pf);
        m.deformers.push(Arc::new(d));
        let i = m.deformers.len() - 1;
        temp_defs.push(i);
        top_level.push(PartChild::Deformer(i));
    }
    // The top-level block at the insertion point, in copied order.
    if let Some(p) = ins_part {
        let part = Arc::make_mut(&mut m.parts[p]);
        ins_index = ins_index.min(part.children.len());
        for c in top_level {
            part.children.insert(ins_index, c);
            ins_index += 1;
        }
    }

    // "Deformer of position adjusting" (§4 step 7): only when every chain
    // of targets inside the pasted set ends at the root.
    let inside: HashSet<DeformerIndex> = made
        .iter()
        .filter_map(|o| match o {
            ObjectRef::Deformer(d) => Some(*d),
            _ => None,
        })
        .chain(temp_defs.iter().copied())
        .collect();
    let target_of = |m: &Model, o: ObjectRef| match o {
        ObjectRef::Deformer(d) => m.deformers[d].target,
        ObjectRef::ArtMesh(a) => m.art_meshes[a].target,
        _ => None,
    };
    let chained: Vec<ObjectRef> = made
        .iter()
        .copied()
        .filter(|o| matches!(o, ObjectRef::Deformer(_) | ObjectRef::ArtMesh(_)))
        .chain(temp_defs.iter().map(|&d| ObjectRef::Deformer(d)))
        .collect();
    let mut broken = false;
    for &o in &chained {
        let mut t = target_of(m, o);
        let mut guard = 0;
        while let Some(d) = t {
            if !inside.contains(&d) || guard > m.deformers.len() {
                broken = true;
                break;
            }
            t = m.deformers[d].target;
            guard += 1;
        }
    }
    let tops: Vec<ObjectRef> = chained
        .iter()
        .copied()
        .filter(|&o| target_of(m, o).is_none())
        .collect();
    let single = tops.len() == 1 && keyform_count(m, tops[0]) == 1;
    if !broken && !tops.is_empty() && !single {
        let values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
        let longer = m.canvas.width.max(m.canvas.height) as f32;
        let nd = crate::objects::NewDeformer {
            name: "Deformer of position adjusting".into(),
            part: ins_part,
            parent: None,
            children: &tops,
            values: &values,
        };
        let bounds = crate::objects::children_bounds(m, &nd)?;
        let r = crate::objects::create_rotation_deformer(m, &nd)?;
        if let (Some((lo, hi)), DeformerKind::Rotation(rot)) =
            (bounds, &mut Arc::make_mut(&mut m.deformers[r]).kind)
        {
            let l = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(0.05 * longer);
            rot.circle_radius = l * 0.5;
            rot.handle_length = l * 0.6;
        }
    }

    // Clean-up (§4 step 8): temporary deformers (children re-parented with
    // conversion) and temporary parameters.
    let values: Vec<f32> = m.parameters.iter().map(|p| p.default).collect();
    for &d in temp_defs.iter().rev() {
        crate::objects::delete_deformer(m, d, &values)?;
        for o in &mut made {
            if let ObjectRef::Deformer(x) = o {
                if *x > d {
                    *x -= 1;
                }
            }
        }
    }
    for &p in temp_params.iter().rev() {
        crate::objects::remove_parameter(m, p);
    }
    Ok(Pasted {
        objects: made,
        warnings,
    })
}

fn keyform_count(m: &Model, o: ObjectRef) -> usize {
    match o {
        ObjectRef::ArtMesh(a) => m.art_meshes[a].grid.keyforms.len(),
        ObjectRef::Deformer(d) => match &m.deformers[d].kind {
            DeformerKind::Warp(w) => w.grid.keyforms.len(),
            DeformerKind::Rotation(r) => r.grid.keyforms.len(),
        },
        _ => 1,
    }
}

/// §4 step 5, another document: reuse a model image cut from the same
/// layer, else add the captured model image (with its layered image and
/// group) and a copy of its atlas page holding only the copied images'
/// entries; the mesh's UVs are mapped to the page it ends up on.
fn bring_image(
    m: &mut Model,
    a: &mut ArtMesh,
    image: &Option<Arc<ModelImage>>,
    group: &Option<ModelImageGroup>,
    layered: &Option<Arc<LayeredImage>>,
    texture: &Option<Arc<Texture>>,
    new_textures: &mut HashMap<Guid, usize>,
) {
    let Some(img) = image else {
        a.model_image = None;
        a.texture = None;
        return;
    };
    let src_index = a.model_image;
    let src_entry = texture.as_ref().and_then(|t| {
        t.entries
            .iter()
            .find(|e| src_index == Some(e.model_image))
            .map(|e| (e.atlas_to_canvas, t.width, t.height))
    });
    if let Some(mi) = img.source.as_ref().and_then(|s| {
        m.model_images
            .iter()
            .position(|d| d.source.as_ref() == Some(s))
    }) {
        let dest = m.textures.iter().enumerate().find_map(|(ti, t)| {
            t.entries
                .iter()
                .find(|e| e.model_image == mi)
                .map(|e| (ti, e.atlas_to_canvas, t.width, t.height))
        });
        if let (Some((ti, d_a2c, dw, dh)), Some((s_a2c, sw, sh))) = (dest, src_entry) {
            remap_uvs(&mut a.uvs, (s_a2c, sw, sh), (d_a2c, dw, dh));
            a.texture = Some(ti);
        }
        a.model_image = Some(mi);
        return;
    }
    if let Some(l) = layered {
        if !m.layered_images.iter().any(|x| x.guid == l.guid) {
            m.layered_images.push(l.clone());
        }
    }
    let mut mi = (**img).clone();
    if m.model_images.iter().any(|x| x.guid == mi.guid) {
        mi.guid = new_guid();
    }
    mi.group =
        group.as_ref().map(
            |g| match m.model_image_groups.iter().position(|x| x.name == g.name) {
                Some(i) => i,
                None => {
                    m.model_image_groups.push(Arc::new(g.clone()));
                    m.model_image_groups.len() - 1
                }
            },
        );
    m.model_images.push(Arc::new(mi));
    let new_mi = m.model_images.len() - 1;
    a.model_image = Some(new_mi);
    let Some(t) = texture else {
        a.texture = None;
        return;
    };
    let ti = match new_textures.get(&t.guid) {
        Some(&ti) => ti,
        None => {
            let mut page = (**t).clone();
            page.guid = new_guid();
            page.entries.clear();
            m.textures.push(Arc::new(page));
            let ti = m.textures.len() - 1;
            new_textures.insert(t.guid.clone(), ti);
            ti
        }
    };
    if let Some(e) = t.entries.iter().find(|e| Some(e.model_image) == src_index) {
        let mut e = *e;
        e.model_image = new_mi;
        Arc::make_mut(&mut m.textures[ti]).entries.push(e);
    }
    a.texture = Some(ti);
}

/// UVs from one atlas placement to another: source UV → source page pixel
/// → canvas → destination page pixel → destination UV.
fn remap_uvs(uvs: &mut [f32], src: ([f32; 6], u32, u32), dst: ([f32; 6], u32, u32)) {
    let (s, sw, sh) = src;
    let (d, dw, dh) = dst;
    let det = d[0] * d[4] - d[1] * d[3];
    if det.abs() < 1e-12 || sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return;
    }
    for uv in uvs.chunks_mut(2) {
        let (px, py) = (uv[0] * sw as f32, uv[1] * sh as f32);
        let cx = s[0] * px + s[1] * py + s[2];
        let cy = s[3] * px + s[4] * py + s[5];
        let (rx, ry) = (cx - d[2], cy - d[5]);
        uv[0] = (d[4] * rx - d[1] * ry) / det / dw as f32;
        uv[1] = (-d[3] * rx + d[0] * ry) / det / dh as f32;
    }
}

// ------------------------------------------------------- parameter values

/// Edit ▸ Copy Parameter Value: every parameter's value, by ID.
pub fn copy_parameter_values(m: &Model, values: &[f32]) -> Vec<(String, f32)> {
    m.parameters
        .iter()
        .zip(values)
        .map(|(p, &v)| (p.id.clone(), v))
        .collect()
}

/// Edit ▸ Paste Parameter Value: sets every parameter found by ID (clamped
/// to its range); others are left alone. Returns how many were set.
pub fn paste_parameter_values(m: &Model, values: &mut [f32], copied: &[(String, f32)]) -> usize {
    let mut n = 0;
    for (id, v) in copied {
        if let Some(i) = m.parameters.iter().position(|p| p.id == *id) {
            if let Some(slot) = values.get_mut(i) {
                let p = &m.parameters[i];
                *slot = v.clamp(p.min.min(p.max), p.max.max(p.min));
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::{ArtMeshForm, Binding, EditableMesh, KeyformGrid, ObjectInfo, PartForm};

    fn mesh(id: &str, part: usize) -> ArtMesh {
        ArtMesh {
            clip_missing: false,
            info: ObjectInfo {
                guid: new_guid(),
                name: id.into(),
                parent_part: Some(part),
                ..ObjectInfo::new(id)
            },
            target: None,
            positions: vec![0.0, 0.0, 10.0, 0.0, 0.0, 10.0],
            uvs: vec![0.0; 6],
            indices: vec![0, 1, 2],
            texture: None,
            clip: Vec::new(),
            invert_clip: false,
            culling: false,
            color_composition: Default::default(),
            alpha_composition: Default::default(),
            user_data: String::new(),
            point_uids: vec![0, 1, 2],
            mesh: EditableMesh::default(),
            model_image: None,
            generator: Default::default(),
            grid: KeyformGrid {
                bindings: vec![Binding::new(0, vec![-1.0, 1.0])],
                keyforms: vec![
                    ArtMeshForm {
                        positions: vec![0.0, 0.0, 10.0, 0.0, 0.0, 10.0],
                        ..Default::default()
                    },
                    ArtMeshForm {
                        positions: vec![1.0, 0.0, 11.0, 0.0, 1.0, 10.0],
                        ..Default::default()
                    },
                ],
                guids: vec![new_guid(), new_guid()],
            },
            blend_shapes: Default::default(),
            paths: Vec::new(),
        }
    }

    /// Root part [BrowR, Sub [C], BrowL], meshes bound to X.
    fn model() -> Model {
        let mut root = Part::new("Root");
        root.info.guid = new_guid();
        root.grid = KeyformGrid::single(PartForm::default());
        root.children = vec![
            PartChild::ArtMesh(0),
            PartChild::Part(1),
            PartChild::ArtMesh(1),
        ];
        let mut sub = Part::new("Sub");
        sub.info.guid = new_guid();
        sub.info.name = "Sub".into();
        sub.info.parent_part = Some(0);
        sub.grid = KeyformGrid::single(PartForm::default());
        sub.children = vec![PartChild::ArtMesh(2)];
        let mut m = Model {
            guid: new_guid(),
            parameters: vec![
                Arc::new(Parameter::new("X", -1.0, 1.0, 0.0)),
                Arc::new(Parameter::new("Y", 0.0, 10.0, 0.0)),
            ],
            art_meshes: vec![
                Arc::new(mesh("BrowR", 0)),
                Arc::new(mesh("BrowL", 0)),
                Arc::new(mesh("C", 1)),
            ],
            parts: vec![Arc::new(root), Arc::new(sub)],
            root_part: Some(0),
            ..Model::default()
        };
        m.canvas.width = 100;
        m.canvas.height = 100;
        m
    }

    #[test]
    fn id_and_name_numbering() {
        let t = ["BrowR", "BrowR2"];
        assert_eq!(copy_id("BrowR", t.iter().copied()), "BrowR3");
        let t = ["BrowR", "BrowR3"];
        assert_eq!(
            copy_id("BrowR3", t.iter().copied()),
            "BrowR4",
            "starts at its own number"
        );
        let w = ["Warp0", "Warp1", "Warp2", "Warp3", "Warp4", "Warp5"];
        assert_eq!(copy_id("Warp3", w.iter().copied()), "Warp6");
        assert_eq!(copy_id("A09", ["A09"].into_iter()), "A10");
        assert_eq!(copy_id("A99", ["A99"].into_iter()), "A100");
        assert_eq!(
            copy_id("Free", ["X"].into_iter()),
            "Free",
            "a free ID is kept"
        );
        assert_eq!(number_name("Eye", ["Eye"].into_iter()), "Eye2");
        assert_eq!(number_name("Eye01", ["Eye01"].into_iter()), "Eye02");
        assert_eq!(number_name("Eye9", ["Eye9"].into_iter()), "Eye10");
        assert_eq!(
            number_name("New", ["Eye"].into_iter()),
            "New",
            "unused names stay"
        );
        assert_eq!(normalise_param_id("PARAM_ANGLE_X"), "ParamAngleX");
        assert_eq!(normalise_param_id("@@PARAM_EYE"), "@@ParamEye");
        assert_eq!(normalise_param_id("ParamAngleX"), "ParamAngleX");
    }

    #[test]
    fn paste_block_goes_above_the_top_most_selected_object() {
        let mut m = model();
        let clip = copy_objects(
            &m,
            &[ObjectRef::ArtMesh(1), ObjectRef::ArtMesh(0)],
            1,
            false,
        )
        .unwrap();
        assert_eq!(clip.objects[0].display_name(), "BrowR", "tree order");
        // Palette order is BrowR, Sub, C, BrowL: of BrowL and C, C is top-most.
        let r = paste_objects(
            &mut m,
            &clip,
            &[ObjectRef::ArtMesh(1), ObjectRef::ArtMesh(2)],
            1,
            false,
        )
        .unwrap();
        assert_eq!(
            r.objects,
            vec![ObjectRef::ArtMesh(3), ObjectRef::ArtMesh(4)]
        );
        // The block in copied order directly above C (root-level copies with
        // keyforms also get the adjusting deformer, §4 step 7).
        let meshes: Vec<PartChild> = m.parts[1]
            .children
            .iter()
            .copied()
            .filter(|c| matches!(c, PartChild::ArtMesh(_)))
            .collect();
        assert_eq!(
            meshes,
            vec![
                PartChild::ArtMesh(3),
                PartChild::ArtMesh(4),
                PartChild::ArtMesh(2)
            ],
        );
        assert_eq!(m.art_meshes[3].info.id, "BrowR2");
        assert_eq!(m.art_meshes[3].info.parent_part, Some(1));
        // A selected part: the top of that part (after the adjusting
        // deformer, which goes in first).
        let r = paste_objects(&mut m, &clip, &[ObjectRef::Part(1)], 1, false).unwrap();
        assert!(m.parts[1].children[..3].contains(&PartChild::ArtMesh(index_of(r.objects[0]))));
        // Nothing selected: the top of the root part, in order.
        let r = paste_objects(&mut m, &clip, &[], 1, false).unwrap();
        let top: Vec<PartChild> = m.parts[0]
            .children
            .iter()
            .copied()
            .filter(|c| matches!(c, PartChild::ArtMesh(_)))
            .take(2)
            .collect();
        assert_eq!(
            top,
            vec![
                PartChild::ArtMesh(index_of(r.objects[0])),
                PartChild::ArtMesh(index_of(r.objects[1]))
            ]
        );
    }

    #[test]
    fn parts_copy_with_children_and_numbered_names() {
        let mut m = model();
        assert!(needs_children_prompt(&m, &[ObjectRef::Part(1)]));
        assert!(!needs_children_prompt(
            &m,
            &[ObjectRef::Part(1), ObjectRef::ArtMesh(2)]
        ));
        let clip = copy_objects(&m, &[ObjectRef::Part(1)], 1, true).unwrap();
        assert_eq!(clip.objects.len(), 2, "the part and its child");
        let r = paste_objects(&mut m, &clip, &[], 1, true).unwrap();
        let ObjectRef::Part(p) = r.objects[0] else {
            panic!()
        };
        assert_eq!(m.parts[p].children, vec![PartChild::ArtMesh(3)]);
        assert_eq!(m.art_meshes[3].info.parent_part, Some(p));
        assert_eq!(
            m.parts[p].info.name, "Sub2",
            "numbered (the name was in use)"
        );
        assert_eq!(m.art_meshes[3].info.name, "C2");
        let clip = copy_objects(&m, &[ObjectRef::Part(1)], 1, false).unwrap();
        assert_eq!(clip.objects.len(), 1, "without children: only the part");
    }

    #[test]
    fn paste_into_another_document_matches_parameters_by_id() {
        let src = model();
        let clip = copy_objects(&src, &[ObjectRef::ArtMesh(0)], 1, false).unwrap();
        let mut dst = model();
        dst.parameters = vec![
            Arc::new(Parameter::new("Z", 0.0, 1.0, 0.0)),
            Arc::new(Parameter::new("X", -1.0, 1.0, 0.0)),
        ];
        let r = paste_objects(&mut dst, &clip, &[], 2, false).unwrap();
        let ObjectRef::ArtMesh(i) = r.objects[0] else {
            panic!()
        };
        assert_eq!(
            dst.art_meshes[i].grid.bindings[0].parameter, 1,
            "matched by ID"
        );
        assert_eq!(dst.art_meshes[i].info.id, "BrowR2");
        // One top-level copy with two keyforms: the adjusting deformer.
        let t = dst.art_meshes[i].target.unwrap();
        assert_eq!(dst.deformers[t].info.name, "Deformer of position adjusting");
    }

    #[test]
    fn parameter_values_round_trip_by_id() {
        let m = model();
        let copied = copy_parameter_values(&m, &[0.5, 7.0]);
        let mut values = vec![-1.0, 0.0];
        assert_eq!(paste_parameter_values(&m, &mut values, &copied), 2);
        assert_eq!(values, vec![0.5, 7.0]);
        let mut values = vec![0.0, 0.0];
        paste_parameter_values(&m, &mut values, &[("Y".into(), 99.0)]);
        assert_eq!(values, vec![0.0, 10.0], "clamped to the range");
    }
}
