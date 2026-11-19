//! `.cmo3` model projects: CAFF archive + `main.xml` object graph → [`Model`].
//!
//! The parsed XML document is kept alongside the typed model, so saving an
//! unmodified project reproduces the original document exactly.
//! spec: 10-formats/cmo3.md, 10-formats/cmo3-schema.md, 20-model/object-model.md

use std::collections::HashMap;

use keyform_model::*;

use crate::caff::CaffArchive;
use crate::error::{invalid, FormatError, Result};
use crate::graph::{parse_f32, Obj, ObjectGraph};
use crate::xml::XmlDocument;

/// Highest root schema version this reader knows. spec: cmo3.md §2
pub const CMODEL_SOURCE_VERSION: u32 = 15;
/// spec: 20-model/object-model.md §1 — ROOT, NOT INITIALIZED and unresolved
/// target GUIDs all mean "attached to the canvas" on read.
pub use keyform_model::{NOT_INITIALIZED_GUID as NULL_GUID, ROOT_DEFORMER_GUID};

mod new_model;
mod save;
mod schema_imports;
pub use save::write_atomic;

/// Path of an element in the document: child indices from the root.
pub type ElementPath = Vec<u32>;

/// Where each object of the loaded model lives in the document.
#[derive(Clone, Debug, Default)]
pub struct ObjectSource {
    /// The object's own element.
    pub element: Option<ElementPath>,
    /// Keyform element per grid cell (`None` for cells the file left empty).
    pub cells: Vec<Option<ElementPath>>,
    /// Keyform element per blend shape.
    pub blend: Vec<Option<ElementPath>>,
}

#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    pub parameters: Vec<Option<ElementPath>>,
    pub parts: Vec<ObjectSource>,
    pub deformers: Vec<ObjectSource>,
    pub art_meshes: Vec<ObjectSource>,
    pub glues: Vec<ObjectSource>,
    pub art_paths: Vec<ObjectSource>,
    pub aliases: Vec<ObjectSource>,
}

/// A loaded project: archive, lossless document and the typed model.
#[derive(Clone)]
pub struct Cmo3Document {
    pub archive: CaffArchive,
    pub xml: XmlDocument,
    pub model: Model,
    /// Non-fatal inconsistencies found while loading.
    pub warnings: Vec<String>,
    /// Element locations of the objects of `saved`.
    pub sources: SourceMap,
    /// The model as last loaded or saved: objects that are still the same
    /// `Arc` need no writing.
    pub saved: Model,
    /// Elements removed by saved deletions, for objects that come back
    /// (undo after save).
    pub(crate) retired: save::Retired,
}

/// Loads the typed model and its element sources from a parsed document.
pub(crate) fn load_model(
    archive: &CaffArchive,
    xml: &XmlDocument,
    warnings: &mut Vec<String>,
) -> Result<(Model, SourceMap)> {
    let g = ObjectGraph::new(xml)?;
    let paths = element_paths(&xml.root);
    let mut l = Loader {
        g: &g,
        archive,
        warnings,
        idx: Index::default(),
        paths: &paths,
        sources: SourceMap::default(),
        pending: ObjectSource::default(),
    };
    let m = l.load()?;
    Ok((m, l.sources))
}

/// Maps every element (by address) to its path, for recording sources.
fn element_paths(root: &crate::xml::Element) -> HashMap<*const crate::xml::Element, ElementPath> {
    let mut map = HashMap::new();
    let mut stack: Vec<(&crate::xml::Element, ElementPath)> = vec![(root, Vec::new())];
    while let Some((e, path)) = stack.pop() {
        for (i, c) in e.children.iter().enumerate() {
            let mut p = path.clone();
            p.push(i as u32);
            stack.push((c, p));
        }
        map.insert(e as *const _, path);
    }
    map
}

impl Cmo3Document {
    pub fn read(data: &[u8]) -> Result<Self> {
        let archive = CaffArchive::read(data)?;
        let xml_bytes = &archive
            .main_xml()
            .ok_or_else(|| FormatError::Invalid("archive has no main.xml".into()))?
            .data;
        let xml = XmlDocument::parse(xml_bytes)?;
        let mut warnings = Vec::new();
        if !archive.guard_ok {
            warnings.push("archive guard bytes missing".into());
        }
        let (model, sources) = load_model(&archive, &xml, &mut warnings)?;
        Ok(Self {
            archive,
            xml,
            saved: model.clone(),
            model,
            warnings,
            sources,
            retired: Default::default(),
        })
    }

    /// Serializes the document as it stands. Edits to `model` are written
    /// into it by [`Cmo3Document::sync`] (or [`Cmo3Document::save_bytes`]).
    pub fn write(&self) -> Result<Vec<u8>> {
        let mut archive = self.archive.clone();
        let xml = self.xml.write();
        match archive.entries.iter_mut().find(|e| e.tag == "main_xml") {
            Some(e) => e.data = xml,
            None => return invalid("archive has no main.xml"),
        }
        archive.write()
    }
}

/// moc3 header version of an export-dialog `FormatVersion` constant.
/// spec: 70-advanced/export-dialog.md §1.1 (`UNSELECTED` is replaced by the
/// default, so it maps to `None` here).
fn moc_version_of(name: &str) -> Option<u8> {
    Some(match name.strip_prefix("VERSION_")? {
        "30" => 1,
        "33" => 2,
        "40" => 3,
        "42" => 4,
        "50" => 5,
        "53" => 6,
        _ => return None,
    })
}

#[derive(Default)]
struct Index {
    params: HashMap<String, ParamIndex>,
    groups: HashMap<String, usize>,
    parts: HashMap<String, PartIndex>,
    deformers: HashMap<String, DeformerIndex>,
    meshes: HashMap<String, MeshIndex>,
    glues: HashMap<String, GlueIndex>,
    /// CImageResource identity → texture index.
    texture_images: HashMap<*const crate::xml::Element, usize>,
    textures_by_guid: HashMap<String, usize>,
    model_images_by_guid: HashMap<String, usize>,
    /// Sampling of each texture page, from the first mesh that uses it.
    sampling: HashMap<usize, TextureSampling>,
}

struct Loader<'a, 'd> {
    g: &'a ObjectGraph<'d>,
    archive: &'a CaffArchive,
    warnings: &'a mut Vec<String>,
    idx: Index,
    paths: &'a HashMap<*const crate::xml::Element, ElementPath>,
    sources: SourceMap,
    /// Keyform element paths recorded by the last `keyforms` call.
    pending: ObjectSource,
}

type O<'g, 'd> = Obj<'g, 'd>;

/// PSD blend keys and the editor's blend classes (`CBlend_<class>`).
/// spec: 70-advanced/psd-import.md §5.4
pub(crate) const PSD_BLEND_CLASSES: [(&[u8; 4], &str); 28] = [
    (b"norm", "Normal"),
    (b"mul ", "Multiply"),
    (b"scrn", "Screen"),
    (b"lddg", "LinearDodge"),
    (b"dark", "Darken"),
    (b"lite", "Lighten"),
    (b"idiv", "ColorBurn"),
    (b"div ", "ColorDodge"),
    (b"lbrn", "LinearBurn"),
    (b"over", "Overray"),
    (b"sLit", "SoftLight"),
    (b"hLit", "HardLight"),
    (b"lLit", "LinearLight"),
    (b"vLit", "VividLight"),
    (b"pLit", "PinLight"),
    (b"hMix", "HardMix"),
    (b"diff", "Difference"),
    (b"smud", "Exclusion"),
    (b"fsub", "Subtract"),
    (b"fdiv", "Divide"),
    (b"hue ", "Hue"),
    (b"sat ", "Saturation"),
    (b"colr", "Color"),
    (b"lum ", "Luminosity"),
    (b"diss", "Dissolve"),
    (b"dkCl", "DarkerColor"),
    (b"lgCl", "LighterColor"),
    (b"pass", "PassThrough"),
];

/// `GTexture2D` filter settings. spec: 40-rendering/appearance.md §5
fn texture_sampling(t: &O) -> Result<TextureSampling> {
    let min = match t.field("filterMode")? {
        Some(f) => f.enum_value("minFilter")?,
        None => None,
    };
    Ok(TextureSampling {
        mipmaps: min.is_none_or(|v| v.contains("MIPMAP")),
        anisotropy: t.enum_value("anisotropy")?.is_none_or(|v| v == "ON"),
        pad_to: t.i32("mipmapLevel")?.unwrap_or(0).clamp(0, 4096) as u32,
    })
}

fn color(o: Option<O>, default: Color) -> Result<Color> {
    let Some(o) = o else { return Ok(default) };
    Ok([
        o.f32("red")?.unwrap_or(default[0]),
        o.f32("green")?.unwrap_or(default[1]),
        o.f32("blue")?.unwrap_or(default[2]),
        o.f32("alpha")?.unwrap_or(default[3]),
    ])
}

const IDENTITY_AFFINE: [f32; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// A `CAffine` as row-major [m00, m01, m02, m10, m11, m12].
fn affine(a: &O) -> [f32; 6] {
    let mut out = IDENTITY_AFFINE;
    for (k, name) in ["m00", "m01", "m02", "m10", "m11", "m12"]
        .iter()
        .enumerate()
    {
        if let Some(v) = a.attr(name).and_then(parse_f32) {
            out[k] = v;
        }
    }
    out
}

/// A `GTransform2` (position, scale, euler angle in degrees) as an affine:
/// translate · rotate · scale.
fn gtransform(t: &O) -> Result<[f32; 6]> {
    let v = |name: &str| -> Result<[f32; 2]> {
        Ok(match t.field(name)? {
            Some(g) => [g.f32("x")?.unwrap_or(0.0), g.f32("y")?.unwrap_or(0.0)],
            None => [0.0, 0.0],
        })
    };
    let (p, sc) = (v("position")?, v("scale")?);
    let (s, c) = t.f32("eulerAngle")?.unwrap_or(0.0).to_radians().sin_cos();
    Ok([c * sc[0], -s * sc[1], p[0], s * sc[0], c * sc[1], p[1]])
}

fn affine_mul(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}

fn affine_inverse(a: [f32; 6]) -> Option<[f32; 6]> {
    let det = a[0] * a[4] - a[1] * a[3];
    if det.abs() < 1e-12 {
        return None;
    }
    let (i00, i01, i10, i11) = (a[4] / det, -a[1] / det, -a[3] / det, a[0] / det);
    Some([
        i00,
        i01,
        -(i00 * a[2] + i01 * a[5]),
        i10,
        i11,
        -(i10 * a[2] + i11 * a[5]),
    ])
}

/// The layered image and layer a model image is cut from: its current
/// image GUID and the first layer input mapped to it.
/// spec: psd-import.md §5.1
/// (layered image, layer) entries of a model image's layer map.
type LayerMapEntry = (Guid, Guid);

fn model_image_source(mi: &O) -> Result<(Option<LayerMapEntry>, Vec<LayerMapEntry>)> {
    let Some(env) = mi.field("inputFilterEnv")? else {
        return Ok((None, Vec::new()));
    };
    let (mut current, mut layers) = (None, Vec::new());
    for entry in env.list("envValues")? {
        let Some(v) = entry.field("value")? else {
            continue;
        };
        let Some(value) = v.field("value")? else {
            continue;
        };
        match value.tag() {
            "CLayeredImageGuid" => current = value.attr("uuid").map(str::to_owned),
            "CLayerSelectorMap" => {
                for e in value.list("_imageToLayerInput")? {
                    let Some(img) = e
                        .field("key")?
                        .and_then(|k| k.attr("uuid").map(str::to_owned))
                    else {
                        continue;
                    };
                    let first = e
                        .field("value")?
                        .map(|l| l.items())
                        .transpose()?
                        .and_then(|items| items.into_iter().next());
                    if let Some(input) = first {
                        if let Some(layer) = input.field("layer")? {
                            if let Some(g) = layer.guid("guid")? {
                                layers.push((img, g.to_owned()));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let cur = match &current {
        Some(c) => layers.iter().position(|(i, _)| i == c),
        None => (!layers.is_empty()).then_some(0),
    };
    Ok(match cur {
        Some(k) => {
            let main = layers.remove(k);
            (Some(main), layers)
        }
        None => (None, layers),
    })
}

/// Generator settings; out-of-range values are clamped to the UI ranges.
/// Older files' single `polygonDensity` is ignored, as the reference does:
/// both intervals then take their default. spec: 70-advanced/auto-mesh.md §2
fn mesh_generate_setting(g: &O) -> Result<MeshGenerateSetting> {
    let d = MeshGenerateSetting::default();
    let int = |name: &str, lo: i32, hi: i32, def: i32| -> Result<i32> {
        Ok(g.i32(name)?.unwrap_or(def).clamp(lo, hi))
    };
    Ok(MeshGenerateSetting {
        outer_density: int("polygonOuterDensity", 10, 200, d.outer_density)?,
        inner_density: int("polygonInnerDensity", 10, 200, d.inner_density)?,
        margin: int("polygonMargin", 0, 50, d.margin)?,
        inner_margin: int("polygonInnerMargin", 0, 50, d.inner_margin)?,
        min_margin: int("polygonMinMargin", 1, 10, d.min_margin)?,
        min_bounds_pt: int("polygonMinBoundsPt", 3, 20, d.min_bounds_pt)?,
        threshold_alpha: int("thresholdAlpha", 0, 254, d.threshold_alpha)?,
    })
}

impl<'a, 'd> Loader<'a, 'd> {
    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    fn load(&mut self) -> Result<Model> {
        let doc = self.g.document();
        let root = self.g.main()?;
        if root.tag() != "CModelSource" {
            return invalid(format!(
                "root object is <{}>, expected <CModelSource>",
                root.tag()
            ));
        }
        if let Some(v) = doc.version_of("CModelSource") {
            if v > CMODEL_SOURCE_VERSION {
                self.warn(format!(
                    "CModelSource version {v} is newer than supported ({CMODEL_SOURCE_VERSION})"
                ));
            }
        }

        let mut m = Model {
            guid: root.guid("guid")?.unwrap_or_default().to_owned(),
            name: root.string("name")?.unwrap_or_default(),
            ..Model::default()
        };
        if let Some(c) = root.field("canvas")? {
            let w = c.i32("pixelWidth")?.unwrap_or(1280);
            let h = c.i32("pixelHeight")?.unwrap_or(720);
            if !(1..=30000).contains(&w) || !(1..=30000).contains(&h) {
                return invalid(format!("canvas size {w}×{h} out of range"));
            }
            m.canvas = Canvas {
                width: w as u32,
                height: h as u32,
            };
        }
        m.target_version = root.i32("targetVersionNo")?.unwrap_or(0);

        self.load_textures(&root, &mut m)?;
        self.load_parameters(&root, &mut m)?;

        // Index every object by GUID first: references go in all directions.
        let part_objs = self.sources(&root, "partSourceSet")?;
        let deformer_objs = self.sources(&root, "deformerSourceSet")?;
        let drawable_objs = self.sources(&root, "drawableSourceSet")?;
        let affecter_objs = self.sources(&root, "affecterSourceSet")?;

        for (i, p) in part_objs.iter().enumerate() {
            self.idx
                .parts
                .insert(p.guid("guid")?.unwrap_or_default().to_owned(), i);
        }
        for (i, d) in deformer_objs.iter().enumerate() {
            self.idx
                .deformers
                .insert(d.guid("guid")?.unwrap_or_default().to_owned(), i);
        }
        let meshes: Vec<O> = drawable_objs
            .iter()
            .copied()
            .filter(|d| {
                let keep = d.tag() == "CArtMeshSource";
                keep
            })
            .collect();
        let art_path_objs: Vec<O> = drawable_objs
            .iter()
            .copied()
            .filter(|d| d.tag() == "CArtPathSource")
            .collect();
        let skipped = drawable_objs.len() - meshes.len() - art_path_objs.len();
        if skipped > 0 {
            self.warn(format!("{skipped} drawables of unsupported kinds skipped"));
        }
        for (i, d) in meshes.iter().enumerate() {
            self.idx
                .meshes
                .insert(d.guid("guid")?.unwrap_or_default().to_owned(), i);
        }
        let glues: Vec<O> = affecter_objs
            .iter()
            .copied()
            .filter(|a| a.tag() == "CGlueSource")
            .collect();
        for (i, a) in glues.iter().enumerate() {
            self.idx
                .glues
                .insert(a.guid("guid")?.unwrap_or_default().to_owned(), i);
        }

        for p in &part_objs {
            m.parts.push(std::sync::Arc::new(self.load_part(p, &m)?));
            let src = self.take_source(p);
            self.sources.parts.push(src);
        }
        m.root_part = match root.field("rootPart")? {
            Some(rp) => rp
                .guid("guid")?
                .and_then(|g| self.idx.parts.get(g).copied()),
            None => None,
        };
        for d in &deformer_objs {
            m.deformers
                .push(std::sync::Arc::new(self.load_deformer(d, &m)?));
            let src = self.take_source(d);
            self.sources.deformers.push(src);
        }
        for o in deformer_objs.iter().chain(meshes.iter()) {
            self.load_rotate3d(o, &mut m)?;
        }
        // Kept sorted by object GUID (one extension per object).
        m.rotate3d.sort_by(|a, b| a.0.cmp(&b.0));
        for d in &art_path_objs {
            let a = self.load_art_path(d, &m)?;
            // Kept as stored; only the object info is written back.
            let src = self.take_source(d);
            self.sources.art_paths.push(src);
            m.art_paths.push(std::sync::Arc::new(a));
        }
        for d in &meshes {
            m.art_meshes
                .push(std::sync::Arc::new(self.load_art_mesh(d, &m)?));
            let src = self.take_source(d);
            self.sources.art_meshes.push(src);
        }
        self.load_aliases(&root, &art_path_objs, &mut m)?;
        for a in &glues {
            m.glues.push(std::sync::Arc::new(self.load_glue(a, &m)?));
            let src = self.take_source(a);
            self.sources.glues.push(src);
        }
        self.check_deformer_cycles(&mut m);
        for (&ti, &s) in &self.idx.sampling {
            if let Some(t) = m.textures.get_mut(ti) {
                std::sync::Arc::make_mut(t).sampling = s;
            }
        }

        self.load_physics(&root, &mut m)?;
        self.load_motion_sync(&root, &mut m)?;
        // Random Pose enable flags of the current setting (generators.md §4).
        if let Some(mgr) = root.field("randomPoseSetting")? {
            let i = mgr.i32("currentIndex")?.unwrap_or(0).max(0) as usize;
            if let Some(s) = mgr.list("_settings")?.get(i) {
                let keys = s.list("parameters.keys")?;
                let vals = s.list("parameters.values")?;
                for (k, v) in keys.iter().zip(&vals) {
                    let id = k.attr("idstr").unwrap_or_default();
                    if v.bool("isEnable")? == Some(false) {
                        if let Some(p) = m.parameters.iter().position(|q| q.id == id) {
                            m.random_pose_off.push(p);
                        }
                    }
                }
            }
        }

        if let Some(opts) = root.field("modelOptions")? {
            if let Some(e) = opts.own_field("MODEL_EXPORT_SETTING_KEY")? {
                m.export = ExportSettings {
                    moc_version: e.enum_value("version")?.and_then(moc_version_of),
                    origin_normalized: [
                        e.f32("normalOriginX")?.unwrap_or(0.5),
                        e.f32("normalOriginY")?.unwrap_or(0.5),
                    ],
                    pixels_per_unit: e.f32("pixelsPerUnit")?.filter(|v| *v > 0.0),
                    export_invisible_parts: e.bool("exportInvisiblePart")?.unwrap_or(false),
                    export_invisible_art_meshes: e.bool("exportInvisibleArtMesh")?.unwrap_or(false),
                    export_sketch_parts: e.bool("exportSketchPart")?.unwrap_or(false),
                    extend_edge_color: e.bool("enableExtendEdgeColor")?.unwrap_or(true),
                    physics_fps: !e.bool("ignoreFPSOnPhysicsJson")?.unwrap_or(false),
                };
            }
        }
        if let Some(info) = root.field("modelInfo")? {
            if let Some(groups) = info
                .field("_effectParameterGroups")?
                .map(|e| e.field("_parameterGroups"))
                .transpose()?
                .flatten()
            {
                for entry in groups.items()? {
                    let kind = entry.enum_value("key")?.unwrap_or_default();
                    let list: Vec<ParamIndex> = entry
                        .list("value")?
                        .iter()
                        .filter_map(|p| {
                            p.attr("uuid").and_then(|u| self.idx.params.get(u).copied())
                        })
                        .collect();
                    match kind {
                        "EYE_BLINK" => m.eye_blink_parameters = list,
                        "LIPSYNC" => m.lip_sync_parameters = list,
                        _ => {}
                    }
                }
            }
        }
        Ok(m)
    }

    /// Physics settings. spec: 70-advanced/physics.md §1; the document stores
    /// no gravity, wind or enable flag, so their defaults apply.
    fn load_physics(&mut self, root: &O<'a, 'd>, m: &mut Model) -> Result<()> {
        let Some(set) = root.field("physicsSettingsSourceSet")? else {
            return Ok(());
        };
        m.physics.fps = set.f32("settingFPS")?.filter(|f| *f > 0.0);
        let kind = |o: &O<'a, 'd>| -> Result<PhysicsKind> {
            Ok(match o.enum_value("type")? {
                Some("SRC_TO_Y") => PhysicsKind::Y,
                Some("SRC_TO_G_ANGLE") => PhysicsKind::Angle,
                _ => PhysicsKind::X,
            })
        };
        let vec2 = |o: Option<O<'a, 'd>>| -> Result<[f32; 2]> {
            Ok(match o {
                Some(v) => [v.f32("x")?.unwrap_or(0.0), v.f32("y")?.unwrap_or(0.0)],
                None => [0.0, 0.0],
            })
        };
        for s in set.list("_sourceCubismPhysics")? {
            let id = s.id("id")?.unwrap_or_default().to_owned();
            let mut inputs = Vec::new();
            for i in s.list("inputs")? {
                let Some(&parameter) = i.guid("source")?.and_then(|g| self.idx.params.get(g))
                else {
                    self.warn(format!("physics {id}: input with unknown parameter"));
                    continue;
                };
                inputs.push(PhysicsInput {
                    parameter,
                    weight: i.f32("weight")?.unwrap_or(100.0),
                    kind: kind(&i)?,
                    reflect: i.bool("isReverse")?.unwrap_or(false),
                });
            }
            let mut outputs = Vec::new();
            for o in s.list("outputs")? {
                let Some(&parameter) = o.guid("destination")?.and_then(|g| self.idx.params.get(g))
                else {
                    self.warn(format!("physics {id}: output with unknown parameter"));
                    continue;
                };
                outputs.push(PhysicsOutput {
                    parameter,
                    vertex_index: o.i32("vertexIndex")?.unwrap_or(1).max(0) as usize,
                    translation_scale: vec2(o.field("translationScale")?)?,
                    angle_scale: o.f32("angleScale")?.unwrap_or(0.0),
                    weight: o.f32("weight")?.unwrap_or(100.0),
                    kind: kind(&o)?,
                    reflect: o.bool("isReverse")?.unwrap_or(false),
                });
            }
            let mut vertices = Vec::new();
            for v in s.list("vertices")? {
                vertices.push(PhysicsVertex {
                    position: vec2(v.field("position")?)?,
                    mobility: v.f32("mobility")?.unwrap_or(1.0),
                    delay: v.f32("delay")?.unwrap_or(1.0),
                    acceleration: v.f32("acceleration")?.unwrap_or(1.0),
                    radius: v.f32("radius")?.unwrap_or(0.0),
                });
            }
            let norm = |min: &str, def: &str, max: &str| -> Result<Normalization> {
                Ok(Normalization {
                    minimum: s.f32(min)?.unwrap_or(-10.0),
                    default: s.f32(def)?.unwrap_or(0.0),
                    maximum: s.f32(max)?.unwrap_or(10.0),
                })
            };
            m.physics.settings.push(PhysicsSetting {
                guid: s.guid("guid")?.unwrap_or_default().to_owned(),
                name: s.string("name")?.unwrap_or_default(),
                id,
                enabled: true,
                inputs,
                outputs,
                vertices,
                position: norm(
                    "normalizedPositionValueMin",
                    "normalizedPositionDefaultValue",
                    "normalizedPositionValueMax",
                )?,
                angle: norm(
                    "normalizedAngleValueMin",
                    "normalizedAngleDefaultValue",
                    "normalizedAngleValueMax",
                )?,
            });
        }
        Ok(())
    }

    /// Motion-sync settings (`motionSyncSettingsSet`). Layout observed in a
    /// reference-made file (5.3.04): parameters by ID string, mappings as a
    /// map audio ID → `MappingItem` whose details map parameter ID →
    /// `ShapeMappingDetail.target`. spec: 70-advanced/motion-sync.md §1
    fn load_motion_sync(&mut self, root: &O<'a, 'd>, m: &mut Model) -> Result<()> {
        use keyform_model::{MotionSyncSetting, SyncAudioParam, SyncCubismParam, SyncMapping};
        let Some(set) = root.field("motionSyncSettingsSet")? else {
            return Ok(());
        };
        for s in set.list("_settingSourceSetMotionSync")? {
            let pp = s.field("postProcessingSetting")?;
            let f = |o: &Option<O<'a, 'd>>, k: &str, d: f32| -> Result<f32> {
                Ok(match o {
                    Some(o) => o.f32(k)?.unwrap_or(d),
                    None => d,
                })
            };
            let mut st = MotionSyncSetting {
                guid: s.guid("guid")?.unwrap_or_default().to_owned(),
                id: s.id("id")?.unwrap_or_default().to_owned(),
                name: s.string("name")?.unwrap_or_default(),
                // A missing value means version 0 (motion-sync.md §2).
                version: u8::from(s.enum_value("motionSyncVersion")? == Some("MOTION_SYNC_1")),
                sync_type: "CRI".into(),
                blend: f(&pp, "blendPercentage", 0.0)?,
                smoothing: match &pp {
                    Some(o) => o.i32("smoothAmount")?.unwrap_or(0),
                    None => 0,
                },
                sample_rate: f(&pp, "sampleRate", 30.0)?,
                cubism: Vec::new(),
                audio: Vec::new(),
                mappings: Vec::new(),
            };
            if let Some(mp) = s.field("mapping")? {
                if let Some(t) = mp.enum_value("syncType")? {
                    st.sync_type = t.to_owned();
                }
                for c in mp.list("cubismParameters")? {
                    st.cubism.push(SyncCubismParam {
                        guid: c.guid("guid")?.unwrap_or_default().to_owned(),
                        parameter_id: c.string("parameterId")?.unwrap_or_default(),
                        name: c.string("name")?.unwrap_or_default(),
                        min: c.f32("min")?.unwrap_or(0.0),
                        max: c.f32("max")?.unwrap_or(1.0),
                        damper: c.f32("damper")?.unwrap_or(0.0),
                        smooth: c.i32("smooth")?.unwrap_or(0),
                    });
                }
                for a in mp.list("audioParameters")? {
                    st.audio.push(SyncAudioParam {
                        guid: a.guid("guid")?.unwrap_or_default().to_owned(),
                        audio_id: a.string("audioId")?.unwrap_or_default(),
                        name: a.string("name")?.unwrap_or_default(),
                        min: a.f32("min")?.unwrap_or(0.0),
                        max: a.f32("max")?.unwrap_or(1.0),
                        scale: a.f32("scale")?.unwrap_or(1.0),
                        enabled: a.bool("enabled")?.unwrap_or(true),
                    });
                }
                for it in mp.list("mappingMap")? {
                    let mut targets = Vec::new();
                    for d in it.list("details")? {
                        let key = d.attr("xs.n").unwrap_or_default().to_owned();
                        targets.push((key, d.f32("target")?.unwrap_or(0.0)));
                    }
                    st.mappings.push(SyncMapping {
                        audio_id: it.string("audioId")?.unwrap_or_default(),
                        kind: it.enum_value("type")?.unwrap_or("Shape").to_owned(),
                        targets,
                    });
                }
            }
            m.motion_sync.push(st);
        }
        Ok(())
    }

    /// An art path (`CArtPathSource`; layout observed in a reference-made
    /// file). spec: 70-advanced/art-path.md §1
    fn load_art_path(&mut self, o: &O<'a, 'd>, m: &Model) -> Result<keyform_model::ArtPath> {
        use keyform_model::{ArtPathForm, ArtPathPoint, LineAlgorithm, LineAlignment};
        let info = self.info(o)?;
        let target = self.target_deformer(o, &info.id)?;
        let (grid, blend_shapes) = self.keyforms(o, m, |f| {
            let mut points = Vec::new();
            for p in f.list("positions")? {
                let pos = p
                    .field("curvePointPosition")?
                    .and_then(|c| c.field("point").ok().flatten());
                let xy = match pos {
                    Some(v) => [v.f32("x")?.unwrap_or(0.0), v.f32("y")?.unwrap_or(0.0)],
                    None => [0.0, 0.0],
                };
                points.push(ArtPathPoint {
                    position: xy,
                    width: p.f32("width")?.unwrap_or(1.0),
                    opacity: p.f32("opacity")?.unwrap_or(1.0),
                    color: [
                        p.f32("colorRed")?.unwrap_or(0.0),
                        p.f32("colorGreen")?.unwrap_or(0.0),
                        p.f32("colorBlue")?.unwrap_or(0.0),
                        p.f32("colorAlpha")?.unwrap_or(1.0),
                    ],
                    corner: p.bool("isCorner")?.unwrap_or(false),
                });
            }
            Ok(ArtPathForm {
                points,
                draw_order: f.i32("drawOrder")?.unwrap_or(500) as f32,
                opacity: f.f32("opacity")?.unwrap_or(1.0),
                multiply: color(f.field("multiplyColor")?, WHITE)?,
                screen: color(f.field("screenColor")?, SCREEN_DEFAULT)?,
            })
        })?;
        Ok(keyform_model::ArtPath {
            info,
            target,
            grid,
            blend_shapes,
            div_num: o.i32("divNum")?.unwrap_or(20).clamp(1, 200) as u32,
            algorithm: match o.enum_value("lineAlgorithm")? {
                Some("SNS") => LineAlgorithm::Sns,
                _ => LineAlgorithm::Lcns,
            },
            alignment: match o.enum_value("lineAlignment")? {
                Some("LEFT_CCW") => LineAlignment::LeftCcw,
                Some("RIGHT_CCW") => LineAlignment::RightCcw,
                _ => LineAlignment::Center,
            },
            closed: o.bool("isClosed")?.unwrap_or(false),
            // The same `clipGuidList` field as art meshes (ACDrawableSource);
            // drawing with it is TODO(Q-0046).
            clip: self.clip_list(o)?.0,
            invert_clip: o.bool("invertClippingMask")?.unwrap_or(false),
            brush: o
                .field("brushGuid")?
                .and_then(|b| b.attr("uuid"))
                .unwrap_or_default()
                .to_owned(),
            user_data: o.string("userData")?.unwrap_or_default(),
        })
    }

    /// Aliases: a `CAliasSourceSet` found by tag under `CModelSource` (its
    /// field name isn't known, TODO(Q-0046)); `CAliasSource` fields per the
    /// schema. Kept as stored on save. spec: 70-advanced/alias.md
    fn load_aliases(
        &mut self,
        root: &O<'a, 'd>,
        art_paths: &[O<'a, 'd>],
        m: &mut Model,
    ) -> Result<()> {
        use keyform_model::{Alias, AliasForm, AliasRef};
        let mut sets = Vec::new();
        let mut cur = Some(*root);
        while let Some(o) = cur {
            for c in &o.element().children {
                if c.name == "CAliasSourceSet" {
                    sets.push(self.g.obj(c)?);
                }
            }
            cur = o.sup()?;
        }
        let mut objs = Vec::new();
        for set in sets {
            objs.extend(set.list("_sources")?);
        }
        if objs.is_empty() {
            return Ok(());
        }
        let mut path_of = HashMap::new();
        for (k, a) in art_paths.iter().enumerate() {
            path_of.insert(a.guid("guid")?.unwrap_or_default().to_owned(), k);
        }
        let mut alias_of = HashMap::new();
        for (k, a) in objs.iter().enumerate() {
            alias_of.insert(a.guid("guid")?.unwrap_or_default().to_owned(), k);
        }
        for o in &objs {
            let info = self.info(o)?;
            let target = self.target_deformer(o, &info.id)?;
            let (grid, blend_shapes) = self.keyforms(o, m, |f| {
                Ok(AliasForm {
                    draw_order: f.i32("drawOrder")?.unwrap_or(500) as f32,
                    opacity: f.f32("opacity")?.unwrap_or(1.0),
                    multiply: color(f.field("multiplyColor")?, WHITE)?,
                    screen: color(f.field("screenColor")?, SCREEN_DEFAULT)?,
                })
            })?;
            let r = o.guid("referenceObjectGuid")?.unwrap_or_default();
            let reference = if let Some(&p) = self.idx.parts.get(r) {
                AliasRef::Part(p)
            } else if let Some(&i) = self.idx.meshes.get(r) {
                AliasRef::Mesh(i)
            } else if let Some(&k) = path_of.get(r) {
                AliasRef::ArtPath(k)
            } else if let Some(&k) = alias_of.get(r) {
                AliasRef::Alias(k)
            } else {
                self.warn(format!("{}: The reference object does not exist.", info.id));
                AliasRef::Missing
            };
            let src = self.take_source(o);
            self.sources.aliases.push(src);
            m.aliases.push(std::sync::Arc::new(Alias {
                info,
                target,
                grid,
                blend_shapes,
                reference,
                offscreen: o.bool("useOffscreen")?.unwrap_or(false),
                clip: self.clip_list(o)?.0,
                invert_clip: o.bool("invertClippingMask")?.unwrap_or(false),
                color_composition: o
                    .enum_value("colorComposition")?
                    .and_then(ColorComposition::from_name)
                    .unwrap_or_default(),
                alpha_composition: o
                    .enum_value("alphaComposition")?
                    .and_then(AlphaComposition::from_name)
                    .unwrap_or_default(),
            }));
        }
        Ok(())
    }

    /// `CRotate3DExtension` of a deformer or art mesh (generators.md §1
    /// "Storage", declarative fields).
    fn load_rotate3d(&mut self, o: &O<'a, 'd>, m: &mut Model) -> Result<()> {
        use keyform_model::{Rotate3dConfig, Rotate3dDepth};
        let Some(guid) = o.guid("guid")? else {
            return Ok(());
        };
        let method = |e: &O<'a, 'd>| -> Result<u8> {
            Ok(match e.enum_value("rotationMethod")? {
                Some("XY_ORDER") => 0,
                Some("YX_ORDER") => 1,
                _ => 2,
            })
        };
        for ext in o.list("_extensions")? {
            if ext.tag() != "CRotate3DExtension" {
                continue;
            }
            let f = |k: &str, d: f32| -> Result<f32> { Ok(ext.f32(k)?.unwrap_or(d)) };
            let p = |k: &str| -> Result<Option<usize>> {
                Ok(ext.guid(k)?.and_then(|g| self.idx.params.get(g).copied()))
            };
            let depth = match ext.field("depthEstimationData")? {
                Some(d) => Some(Rotate3dDepth {
                    buffer: d.f32_array("depthBuffer")?.unwrap_or_default(),
                    row: d.i32("row")?.unwrap_or(0),
                    column: d.i32("column")?.unwrap_or(0),
                    camera: [
                        d.f32("cameraX")?.unwrap_or(0.0),
                        d.f32("cameraY")?.unwrap_or(0.0),
                    ],
                    centre: [
                        d.f32("centerX")?.unwrap_or(0.0),
                        d.f32("centerY")?.unwrap_or(0.0),
                    ],
                    range: [
                        d.f32("rotationRangeX")?.unwrap_or(30.0),
                        d.f32("rotationRangeY")?.unwrap_or(30.0),
                    ],
                    z_offset: d.f32("zOffset")?.unwrap_or(0.0),
                    perspective: d.f32("perspective")?.unwrap_or(1.0),
                    method: method(&d)?,
                    reference_form: d.guid("referenceFormGuid")?.unwrap_or_default().to_owned(),
                }),
                None => None,
            };
            let c = Rotate3dConfig {
                camera: [f("cameraX", 0.0)?, f("cameraY", 0.0)?],
                centre: [f("centerX", 0.0)?, f("centerY", 0.0)?],
                range: [f("rotationRangeX", 30.0)?, f("rotationRangeY", 30.0)?],
                symmetry: [
                    ext.bool("isSymmetryX")?.unwrap_or(false),
                    ext.bool("isSymmetryY")?.unwrap_or(false),
                ],
                z_offset: f("zOffset", 0.0)?,
                perspective: f("perspective", 1.0)?,
                method: method(&ext)?,
                parameters: [p("mappedParameterGuid0")?, p("mappedParameterGuid1")?],
                depth_enabled: ext.bool("isDepthEstimationEnabled")?.unwrap_or(false),
                depth,
                filter: match ext.enum_value("keyformFilterKind")? {
                    Some("X_DIRECTION") => 1,
                    Some("Y_DIRECTION") => 2,
                    Some("CORNERS") => 3,
                    _ => 0,
                },
            };
            m.rotate3d.push((guid.to_owned(), c));
        }
        Ok(())
    }

    fn path_of(&self, o: &O<'a, 'd>) -> Option<ElementPath> {
        self.paths.get(&o.identity()).cloned()
    }

    /// The source record of the object just loaded.
    fn take_source(&mut self, o: &O<'a, 'd>) -> ObjectSource {
        let mut s = std::mem::take(&mut self.pending);
        s.element = self.path_of(o);
        s
    }

    fn sources(&self, root: &O<'a, 'd>, set: &str) -> Result<Vec<O<'a, 'd>>> {
        match root.field(set)? {
            Some(s) => s.list("_sources"),
            None => Ok(Vec::new()),
        }
    }

    fn load_textures(&mut self, root: &O<'a, 'd>, m: &mut Model) -> Result<()> {
        let Some(tm) = root.field("textureManager")? else {
            return Ok(());
        };
        m.preview_reduction = tm.i32("previewReductionRatio")?.unwrap_or(0).max(0) as u32;
        // Layered images (imported PSDs). spec: psd-import.md §8
        for w in tm.list("_rawImages")? {
            let Some(img) = w.field("image")? else {
                continue;
            };
            m.layered_images.push(std::sync::Arc::new(LayeredImage {
                guid: img.guid("guid")?.unwrap_or_default().to_owned(),
                name: img.string("name")?.unwrap_or_default(),
                width: img.i32("width")?.unwrap_or(0).max(0) as u32,
                height: img.i32("height")?.unwrap_or(0).max(0) as u32,
                path: img.scalar("psdFile")?.unwrap_or_default().to_owned(),
                imported_ms: w.i64("importedTimeMSec")?.unwrap_or(0),
                modified_ms: w.i64("lastModifiedTimeMSec")?.unwrap_or(0),
                replaced: w.bool("isReplaced")?.unwrap_or(false),
                tree: self.layer_tree(&img)?,
            }));
        }
        // Model images (per-mesh source rasters), grouped per PSD.
        for group in tm.list("_modelImageGroups")? {
            let gi = m.model_image_groups.len();
            m.model_image_groups
                .push(std::sync::Arc::new(ModelImageGroup {
                    name: group.string("groupName")?.unwrap_or_default(),
                    linked: group
                        .list("_linkedRawImageGuids")?
                        .iter()
                        .filter_map(|g| g.attr("uuid").map(str::to_owned))
                        .collect(),
                }));
            for mi in group.list("_modelImages")? {
                let Some(guid) = mi.guid("guid")? else {
                    continue;
                };
                let guid = guid.to_owned();
                let png = match mi.field("_filteredImage")? {
                    Some(img) => self.image_payload(&img)?,
                    None => None,
                };
                let local_to_canvas = match mi.field("_materialLocalToCanvasTransform")? {
                    Some(a) => affine(&a),
                    None => IDENTITY_AFFINE,
                };
                self.idx
                    .model_images_by_guid
                    .insert(guid.clone(), m.model_images.len());
                let sources = model_image_source(&mi)?;
                m.model_images.push(std::sync::Arc::new(ModelImage {
                    guid,
                    name: mi.string("name")?.unwrap_or_default(),
                    png,
                    local_to_canvas,
                    group: Some(gi),
                    source: sources.0,
                    other_sources: sources.1,
                }));
            }
        }
        for atlas in tm.list("_textureAtlases")? {
            let guid = atlas.guid("guid")?.unwrap_or_default().to_owned();
            let image = atlas.field("cachedAtlasImage")?;
            let png = match image {
                Some(img) => self.image_payload(&img)?,
                None => None,
            };
            let ti = m.textures.len();
            if let Some(img) = image {
                self.idx.texture_images.insert(img.identity(), ti);
            }
            self.idx.textures_by_guid.insert(guid.clone(), ti);
            let mut entries = Vec::new();
            for e in atlas.list("modelImages")? {
                let Some(k) = e
                    .guid("modelImageGuid")?
                    .and_then(|g| self.idx.model_images_by_guid.get(g).copied())
                else {
                    continue;
                };
                let atlas_to_canvas = match e.field("atlasLocalToCanvasTransform")? {
                    Some(a) => affine(&a),
                    // Version 1: material-local → atlas; convert.
                    // spec: texture-atlas.md §1
                    None => match e.field("materialLocalToAtlasTransform")? {
                        Some(t) => {
                            let to_atlas = gtransform(&t)?;
                            match affine_inverse(to_atlas) {
                                Some(inv) => affine_mul(m.model_images[k].local_to_canvas, inv),
                                None => continue,
                            }
                        }
                        None => continue,
                    },
                };
                entries.push(AtlasEntry {
                    model_image: k,
                    atlas_to_canvas,
                });
            }
            m.textures.push(std::sync::Arc::new(Texture {
                sampling: Default::default(),
                guid,
                name: atlas.string("name")?.unwrap_or_default(),
                width: atlas.i32("width")?.unwrap_or(0).max(0) as u32,
                height: atlas.i32("height")?.unwrap_or(0).max(0) as u32,
                png,
                locked: atlas.bool("lockCachedAtlasImage")?.unwrap_or(false),
                entries,
            }));
        }
        Ok(())
    }

    /// PNG bytes of a `CImageResource`. spec: cmo3.md §3
    /// The stored layer tree of a layered image (`_rootLayer`'s children).
    /// spec: 70-advanced/psd-import.md §8
    fn layer_tree(&mut self, img: &O<'a, 'd>) -> Result<Option<std::sync::Arc<Vec<LayerEntry>>>> {
        let Some(root) = img.field("_rootLayer")? else {
            return Ok(None);
        };
        Ok(Some(std::sync::Arc::new(self.layer_entries(&root, 0)?)))
    }

    fn layer_entries(&mut self, group: &O<'a, 'd>, depth: usize) -> Result<Vec<LayerEntry>> {
        if depth > 256 {
            return invalid("layer groups nested too deeply");
        }
        let mut out = Vec::new();
        for c in group.list("_children")? {
            let blend = c
                .field("blend")?
                .and_then(|b| b.tag().strip_prefix("CBlend_"))
                .and_then(|class| PSD_BLEND_CLASSES.iter().find(|(_, n)| *n == class))
                .map_or(*b"norm", |(k, _)| **k);
            let common = LayerCommon {
                guid: c.guid("guid")?.unwrap_or_default().to_owned(),
                name: c.string("name")?.unwrap_or_default(),
                visible: c.bool("isVisible")?.unwrap_or(true),
                clipping: c.bool("isClipping")?.unwrap_or(false),
                opacity: c.i32("opacity255")?.unwrap_or(255).clamp(0, 255) as u8,
                blend,
                layer_id: match c.field("layerIdentifier")? {
                    Some(id) => id.string("layerId")?,
                    None => None,
                },
            };
            match c.tag() {
                "CLayerGroup" => {
                    let children = self.layer_entries(&c, depth + 1)?;
                    out.push(LayerEntry::Group(LayerGroup { common, children }));
                }
                "CLayer" => {
                    let png = match c.field("imageResource")? {
                        Some(r) => self.image_payload(&r)?,
                        None => None,
                    };
                    let mut bounds = [0; 4];
                    if let Some(b) = c.field("boundsOnImageDoc")? {
                        for (k, f) in ["x", "y", "width", "height"].iter().enumerate() {
                            bounds[k] = b.i32(f)?.unwrap_or(0);
                        }
                    }
                    out.push(LayerEntry::Layer(ImageLayer {
                        common,
                        bounds,
                        png: std::sync::Arc::new(png.unwrap_or_default()),
                    }));
                }
                _ => {}
            }
        }
        Ok(out)
    }

    fn image_payload(&mut self, img: &O<'a, 'd>) -> Result<Option<Vec<u8>>> {
        let Some(path) = img.field("imageFileBuf")?.and_then(|f| f.attr("path")) else {
            return Ok(None);
        };
        match self.archive.entry(path) {
            Some(e) => Ok(Some(e.data.clone())),
            None => {
                self.warn(format!("image payload {path:?} missing from archive"));
                Ok(None)
            }
        }
    }

    fn load_parameters(&mut self, root: &O<'a, 'd>, m: &mut Model) -> Result<()> {
        let params = match root.field("parameterSourceSet")? {
            Some(s) => s.list("_sources")?,
            None => Vec::new(),
        };
        for p in &params {
            let guid = p.guid("guid")?.unwrap_or_default().to_owned();
            let id = p.id("id")?.unwrap_or_default().to_owned();
            let min = p.f32("minValue")?.unwrap_or(0.0);
            let max = p.f32("maxValue")?.unwrap_or(1.0);
            let default = p.f32("defaultValue")?.unwrap_or(0.0);
            let decimal_places = p.i32("decimalPlaces")?.unwrap_or(3);
            let snap_epsilon = p
                .f32("snapEpsilon")?
                .filter(|e| *e > 0.0)
                .unwrap_or_else(|| 10f32.powi(-decimal_places.clamp(0, 10)));
            let kind = match p.enum_value("paramType")? {
                Some("MORPH_TARGET") => ParameterKind::BlendShape,
                _ => ParameterKind::Normal,
            };
            self.idx.params.insert(guid.clone(), m.parameters.len());
            self.sources.parameters.push(self.path_of(p));
            m.parameters.push(std::sync::Arc::new(Parameter {
                guid,
                name: p.string("name")?.unwrap_or_else(|| id.clone()),
                id,
                description: p.string("description")?.unwrap_or_default(),
                min,
                max,
                default,
                repeat: p.bool("isRepeat")?.unwrap_or(false),
                kind,
                decimal_places,
                snap_epsilon,
                combined: p.bool("combined")?.unwrap_or(false),
                group: None,
            }));
        }

        // Groups: folders of the Parameter palette.
        let groups = match root.field("parameterGroupSet")? {
            Some(s) => s.list("_groups")?,
            None => Vec::new(),
        };
        let root_group = root.field("rootParameterGroup")?;
        let root_guid = match root_group {
            Some(r) => r.guid("guid")?.map(str::to_owned),
            None => None,
        };
        let mut group_objs = Vec::new();
        for gr in &groups {
            let guid = gr.guid("guid")?.unwrap_or_default().to_owned();
            if Some(&guid) == root_guid.as_ref() {
                continue;
            }
            self.idx
                .groups
                .insert(guid.clone(), m.parameter_groups.len());
            m.parameter_groups.push(std::sync::Arc::new(ParameterGroup {
                guid,
                id: gr.id("id")?.unwrap_or_default().to_owned(),
                name: gr.string("name")?.unwrap_or_default(),
                parent: None,
                children: Vec::new(),
            }));
            group_objs.push(*gr);
        }
        for (gi, gr) in group_objs.iter().enumerate() {
            let children = self.param_entries(gr)?;
            for c in &children {
                match *c {
                    ParamEntry::Parameter(p) => {
                        std::sync::Arc::make_mut(&mut m.parameters[p]).group = Some(gi)
                    }
                    ParamEntry::Group(g) => {
                        std::sync::Arc::make_mut(&mut m.parameter_groups[g]).parent = Some(gi)
                    }
                }
            }
            std::sync::Arc::make_mut(&mut m.parameter_groups[gi]).children = children;
        }
        m.root_parameter_entries = match root_group {
            Some(r) => self.param_entries(&r)?,
            None => (0..m.parameters.len()).map(ParamEntry::Parameter).collect(),
        };
        Ok(())
    }

    fn param_entries(&self, group: &O<'a, 'd>) -> Result<Vec<ParamEntry>> {
        let mut out = Vec::new();
        for c in group.list("_childGuids")? {
            let Some(uuid) = c.attr("uuid") else { continue };
            match c.tag() {
                "CParameterGuid" => {
                    if let Some(&p) = self.idx.params.get(uuid) {
                        out.push(ParamEntry::Parameter(p));
                    }
                }
                "CParameterGroupGuid" => {
                    if let Some(&g) = self.idx.groups.get(uuid) {
                        out.push(ParamEntry::Group(g));
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    fn info(&mut self, o: &O<'a, 'd>) -> Result<ObjectInfo> {
        let parent_part = match o.guid("parentGuid")? {
            Some(g) => self.idx.parts.get(g).copied(),
            None => None,
        };
        Ok(ObjectInfo {
            guid: o.guid("guid")?.unwrap_or_default().to_owned(),
            id: o.id("id")?.unwrap_or_default().to_owned(),
            name: o.string("localName")?.unwrap_or_default(),
            parent_part,
            visible: o.bool("isVisible")?.unwrap_or(true),
            locked: o.bool("isLocked")?.unwrap_or(false),
        })
    }

    fn target_deformer(&mut self, o: &O<'a, 'd>, what: &str) -> Result<Option<DeformerIndex>> {
        let Some(g) = o.guid("targetDeformerGuid")? else {
            return Ok(None);
        };
        if g == NULL_GUID || g == ROOT_DEFORMER_GUID {
            return Ok(None);
        }
        match self.idx.deformers.get(g) {
            Some(&d) => Ok(Some(d)),
            None => {
                self.warn(format!(
                    "{what}: target deformer {g} not found; attached to root"
                ));
                Ok(None)
            }
        }
    }

    /// Keyforms of an object: grid (one form per cell) and blend shapes.
    /// spec: object-model.md §5–6
    fn keyforms<F: Clone>(
        &mut self,
        o: &O<'a, 'd>,
        m: &Model,
        parse: impl Fn(&O<'a, 'd>) -> Result<F>,
    ) -> Result<(KeyformGrid<F>, BlendShapes<F>)> {
        let what = o.id("id")?.unwrap_or("?").to_owned();
        let mut forms: HashMap<String, F> = HashMap::new();
        let mut form_paths: HashMap<String, ElementPath> = HashMap::new();
        let mut first = None;
        for f in o.list("keyforms")? {
            let parsed = parse(&f)?;
            if first.is_none() {
                first = Some(parsed.clone());
            }
            if let Some(g) = f.guid("guid")? {
                forms.insert(g.to_owned(), parsed);
                if let Some(p) = self.path_of(&f) {
                    form_paths.insert(g.to_owned(), p);
                }
            }
        }
        let Some(first) = first else {
            return invalid(format!("{what}: object has no keyforms"));
        };

        let mut bindings = Vec::new();
        let mut binding_ids = Vec::new();
        let grid_src = o.field("keyformGridSource")?;
        if let Some(gs) = grid_src {
            for b in gs.list("keyformBindings")? {
                let pg = b.guid("parameterGuid")?.unwrap_or_default();
                let Some(&param) = self.idx.params.get(pg) else {
                    return invalid(format!("{what}: binding to unknown parameter {pg}"));
                };
                let keys: Vec<f32> = match b.field("keys")? {
                    Some(k) => k
                        .scalar_items()
                        .map(|s| {
                            parse_f32(s).ok_or_else(|| {
                                FormatError::Invalid(format!("{what}: bad key {s:?}"))
                            })
                        })
                        .collect::<Result<_>>()?,
                    None => Vec::new(),
                };
                if keys.is_empty() || !keys.windows(2).all(|w| w[0] < w[1]) {
                    return invalid(format!("{what}: binding keys not strictly ascending"));
                }
                let extended = match b.enum_value("extendedInterpolationType")? {
                    Some("ELLIPSE") => ExtendedInterpolationKind::Ellipse,
                    Some("SNS_CURVE") => ExtendedInterpolationKind::SnsCurve,
                    _ => ExtendedInterpolationKind::Linear,
                };
                binding_ids.push(b.identity());
                bindings.push(Binding {
                    parameter: param,
                    keys,
                    interpolation: b
                        .enum_value("interpolationType")?
                        .unwrap_or("LINEAR")
                        .to_owned(),
                    extended,
                    insert_point_count: b.i32("insertPointCount")?.unwrap_or(1),
                    extended_scale: b.f32("extendedInterpolationScale")?.unwrap_or(1.0),
                });
            }
        }
        let mut grid = KeyformGrid {
            guids: Vec::new(),
            bindings,
            keyforms: Vec::new(),
        };
        let cells = grid.cell_count();
        if cells > 1 << 20 {
            return Err(FormatError::LimitExceeded(format!(
                "{what}: keyform grid of {cells} cells"
            )));
        }
        let mut slots: Vec<Option<F>> = vec![None; cells];
        let mut cell_paths: Vec<Option<ElementPath>> = vec![None; cells];
        let mut cell_guids: Vec<Guid> = vec![String::new(); cells];
        if let Some(gs) = grid_src {
            for kog in gs.list("keyformsOnGrid")? {
                let mut idx = vec![usize::MAX; grid.bindings.len()];
                if let Some(ak) = kog.field("accessKey")? {
                    for kop in ak.list("_keyOnParameterList")? {
                        let Some(b) = kop.field("binding")? else {
                            continue;
                        };
                        let Some(bi) = binding_ids.iter().position(|&id| id == b.identity()) else {
                            return invalid(format!("{what}: access key names a foreign binding"));
                        };
                        idx[bi] = kop.i32("keyIndex")?.unwrap_or(-1).max(-1) as usize;
                    }
                }
                let Some(cell) = grid.cell_index(&idx) else {
                    return invalid(format!("{what}: access key {idx:?} outside the grid"));
                };
                let fg = kog.guid("keyformGuid")?.unwrap_or_default();
                cell_paths[cell] = form_paths.get(fg).cloned();
                cell_guids[cell] = fg.to_owned();
                match forms.get(fg) {
                    Some(f) => slots[cell] = Some(f.clone()),
                    None => return invalid(format!("{what}: grid names unknown keyform {fg}")),
                }
            }
        }
        let missing = slots.iter().filter(|s| s.is_none()).count();
        if missing > 0 {
            self.warn(format!(
                "{what}: {missing} grid cells without keyform (filled with the first)"
            ));
        }
        grid.keyforms = slots
            .into_iter()
            .map(|s| s.unwrap_or_else(|| first.clone()))
            .collect();
        grid.guids = cell_guids;

        let mut blend = BlendShapes::default();
        let mut blend_paths = Vec::new();
        if let Some(set) = o.field("keyformMorphTargetSet")? {
            for mt in set.list("_morphTargets")? {
                let pg = mt.guid("parameterGuid")?.unwrap_or_default();
                let Some(&param) = self.idx.params.get(pg) else {
                    self.warn(format!("{what}: blend shape on unknown parameter"));
                    continue;
                };
                let fg = mt.guid("keyformGuid")?.unwrap_or_default();
                let Some(form) = forms.get(fg) else {
                    self.warn(format!("{what}: blend shape names unknown keyform"));
                    continue;
                };
                // keyValue is a nullable Float; unset = parameter max (blend-shapes.md §1).
                let key = match mt.field("keyValue")? {
                    Some(k) => parse_f32(&k.element().text),
                    None => mt.f32("keyValue")?,
                };
                blend.shapes.push(BlendShape {
                    parameter: param,
                    key,
                    form: form.clone(),
                });
                blend_paths.push(form_paths.get(fg).cloned());
            }
            if let Some(cs) = set.field("blendWeightConstraintSet")? {
                for c in cs.list("_constraints")? {
                    let p = |n: &str| -> Result<Option<ParamIndex>> {
                        Ok(c.guid(n)?.and_then(|g| self.idx.params.get(g).copied()))
                    };
                    let (Some(bp), Some(cp)) = (
                        p("morphTargetParameterGuid")?,
                        p("constraintParameterGuid")?,
                    ) else {
                        continue;
                    };
                    blend.constraints.push(BlendConstraint {
                        blend_parameter: bp,
                        constraint_parameter: cp,
                        value: c.f32("constraintParameterValue")?.unwrap_or(0.0),
                        weight: c.f32("blendWeight")?.unwrap_or(1.0),
                    });
                }
            }
        }
        // Extended blend-shape settings (extended-interpolation.md §4):
        // `extendedInterpolationSettings`, a map CParameterGuid →
        // ExtendedInterpolationSetting (entry form, xml-object-graph.md §2.6).
        for ext in o.list("_extensions")? {
            if ext.tag() != "CExtendedInterpolationExtension" {
                continue;
            }
            let Some(set) = ext.field("_extendedKeyformMorphTargetSet")? else {
                continue;
            };
            let Some(map) = set.field("extendedInterpolationSettings")? else {
                continue;
            };
            for entry in map.items()? {
                let Some(key) = entry.field("key")? else {
                    continue;
                };
                let Some(&param) = key.attr("uuid").and_then(|g| self.idx.params.get(g)) else {
                    continue;
                };
                let Some(v) = entry.field("value")? else {
                    continue;
                };
                // LINEAR entries are stored too (Q-0027).
                let kind = match v.enum_value("type")? {
                    Some("ELLIPSE") => ExtendedInterpolationKind::Ellipse,
                    Some("SNS_CURVE") => ExtendedInterpolationKind::SnsCurve,
                    _ => ExtendedInterpolationKind::Linear,
                };
                blend.extended.push(keyform_model::BlendExtended {
                    parameter: param,
                    kind,
                    points: v.i32("insertPointCount")?.unwrap_or(1),
                    scale: v.f32("extendedInterpolationScale")?.unwrap_or(1.0),
                });
            }
        }
        blend.extended.sort_by_key(|x| x.parameter);
        let _ = m;
        self.pending.cells = cell_paths;
        self.pending.blend = blend_paths;
        Ok((grid, blend))
    }

    fn load_part(&mut self, o: &O<'a, 'd>, m: &Model) -> Result<Part> {
        let info = self.info(o)?;
        let (grid, blend_shapes) = self.keyforms(o, m, |f| {
            Ok(PartForm {
                draw_order: f.i32("drawOrder")?.unwrap_or(500) as f32,
                opacity: f.f32("opacity")?.unwrap_or(1.0),
                multiply: color(f.field("multiplyColor")?, WHITE)?,
                screen: color(f.field("screenColor")?, SCREEN_DEFAULT)?,
            })
        })?;
        let mut children = Vec::new();
        for c in o.list("_childGuids")? {
            let Some(uuid) = c.attr("uuid") else { continue };
            let child = match c.tag() {
                "CPartGuid" => self.idx.parts.get(uuid).map(|&i| PartChild::Part(i)),
                "CDeformerGuid" => self
                    .idx
                    .deformers
                    .get(uuid)
                    .map(|&i| PartChild::Deformer(i)),
                "CDrawableGuid" => self.idx.meshes.get(uuid).map(|&i| PartChild::ArtMesh(i)),
                "CAffecterGuid" => self.idx.glues.get(uuid).map(|&i| PartChild::Glue(i)),
                _ => None,
            };
            children.push(child.unwrap_or(PartChild::Other));
        }
        let (clip, clip_missing) = self.clip_list(o)?;
        Ok(Part {
            clip_missing,
            info,
            children,
            draw_order_group: o.bool("enableDrawOrderGroup")?.unwrap_or(false),
            sketch: o.bool("isSketch")?.unwrap_or(false),
            offscreen: o.bool("useOffscreen")?.unwrap_or(false),
            clip,
            invert_clip: o.bool("invertClippingMask")?.unwrap_or(false),
            color_composition: o
                .enum_value("colorComposition")?
                .and_then(ColorComposition::from_name)
                .unwrap_or_default(),
            alpha_composition: o
                .enum_value("alphaComposition")?
                .and_then(AlphaComposition::from_name)
                .unwrap_or_default(),
            default_order: o.i32("defaultOrder_forEditor")?.unwrap_or(500),
            grid,
            blend_shapes,
        })
    }

    /// Clip masks, and whether the list names drawables that don't exist.
    fn clip_list(&self, o: &O<'a, 'd>) -> Result<(Vec<MeshIndex>, bool)> {
        let mut found = Vec::new();
        let mut missing = false;
        for c in o.list("clipGuidList")? {
            match c.attr("uuid").and_then(|u| self.idx.meshes.get(u).copied()) {
                Some(i) => found.push(i),
                None => missing = true,
            }
        }
        Ok((found, missing))
    }

    fn load_deformer(&mut self, o: &O<'a, 'd>, m: &Model) -> Result<Deformer> {
        let info = self.info(o)?;
        let target = self.target_deformer(o, &info.id)?;
        let kind = match o.tag() {
            "CWarpDeformerSource" => {
                let cols = o.i32("col")?.unwrap_or(0);
                let rows = o.i32("row")?.unwrap_or(0);
                if !(1..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
                    return invalid(format!("{}: warp grid {cols}×{rows}", info.id));
                }
                let n = 2 * (cols as usize + 1) * (rows as usize + 1);
                let id = info.id.clone();
                let (grid, blend_shapes) = self.keyforms(o, m, |f| {
                    let positions = f.f32_array("positions")?.unwrap_or_default();
                    if positions.len() != n {
                        return invalid(format!(
                            "{id}: warp keyform has {} coords, expected {n}",
                            positions.len()
                        ));
                    }
                    Ok(WarpForm {
                        positions,
                        opacity: f.f32("opacity")?.unwrap_or(1.0),
                        multiply: color(f.field("multiplyColor")?, WHITE)?,
                        screen: color(f.field("screenColor")?, SCREEN_DEFAULT)?,
                    })
                })?;
                let mut bezier_nets = Vec::new();
                let mut sway = Vec::new();
                for ext in o.list("_extensions")? {
                    if ext.tag() == "CAutoYureConfigExtension" {
                        // generators.md §2 "Storage" (observed layout).
                        for e in ext.list("paramToConfigMap")? {
                            let Some(&parameter) =
                                e.guid("key")?.and_then(|g| self.idx.params.get(g))
                            else {
                                continue;
                            };
                            let Some(v) = e.field("value")? else { continue };
                            let side = |n: &str| -> Result<keyform_model::SwaySideConfig> {
                                let d = keyform_model::SwaySideConfig::default();
                                Ok(match v.field(n)? {
                                    Some(c) => keyform_model::SwaySideConfig {
                                        scale_x: c.f32("scalePercentX")?.unwrap_or(d.scale_x),
                                        scale_y: c.f32("scalePercentY")?.unwrap_or(d.scale_y),
                                        expand: c.f32("expandScale")?.unwrap_or(d.expand),
                                        decay: c.f64("decayLevel")?.unwrap_or(d.decay),
                                    },
                                    None => d,
                                })
                            };
                            sway.push(keyform_model::SwayConfig {
                                parameter,
                                left: side("leftYureConfig")?,
                                right: side("rightYureConfig")?,
                                sync: v.bool("syncLeftRight")?.unwrap_or(true),
                                root: match v.enum_value("rootDirection")? {
                                    Some("RIGHT") => 1,
                                    Some("BOTTOM") => 2,
                                    Some("LEFT") => 3,
                                    _ => 0,
                                },
                                flip: v.bool("isFlip")?.unwrap_or(false),
                            });
                        }
                    }
                    if ext.tag() == "CWarpDeformerBezierExtension" {
                        let level = ext.i32("editLevel")?.unwrap_or(2).clamp(1, 3) as u8;
                        let c = ext.i32("bezierCol")?.unwrap_or(1).clamp(1, 1000) as u32;
                        let r = ext.i32("bezierRow")?.unwrap_or(1).clamp(1, 1000) as u32;
                        bezier_nets.push((level, c, r));
                    }
                }
                DeformerKind::Warp(WarpDeformer {
                    bezier_nets,
                    sway,
                    cols: cols as u32,
                    rows: rows as u32,
                    quad: o.bool("isQuadTransform")?.unwrap_or(false),
                    grid,
                    blend_shapes,
                })
            }
            "CRotationDeformerSource" => {
                let (grid, blend_shapes) = self.keyforms(o, m, |f| {
                    Ok(RotationForm {
                        angle: f.f32("angle")?.unwrap_or(0.0),
                        origin: [
                            f.f32("originX")?.unwrap_or(0.0),
                            f.f32("originY")?.unwrap_or(0.0),
                        ],
                        scale: f.f32("scale")?.unwrap_or(1.0),
                        reflect_x: f.bool("isReflectX")?.unwrap_or(false),
                        reflect_y: f.bool("isReflectY")?.unwrap_or(false),
                        opacity: f.f32("opacity")?.unwrap_or(1.0),
                        multiply: color(f.field("multiplyColor")?, WHITE)?,
                        screen: color(f.field("screenColor")?, SCREEN_DEFAULT)?,
                    })
                })?;
                DeformerKind::Rotation(RotationDeformer {
                    handle_length: o.f32("handleLengthOnCanvas")?.unwrap_or(100.0),
                    circle_radius: o.f32("circleRadiusOnCanvas")?.unwrap_or(30.0),
                    base_angle: o.f32("baseAngle")?.unwrap_or(0.0),
                    grid,
                    blend_shapes,
                })
            }
            t => return Err(FormatError::Unsupported(format!("deformer kind <{t}>"))),
        };
        Ok(Deformer { info, target, kind })
    }

    fn load_art_mesh(&mut self, o: &O<'a, 'd>, m: &Model) -> Result<ArtMesh> {
        let info = self.info(o)?;
        let target = self.target_deformer(o, &info.id)?;
        let positions = o.f32_array("positions")?.unwrap_or_default();
        let nv = positions.len() / 2;
        if positions.len() % 2 != 0 {
            return invalid(format!("{}: odd position count", info.id));
        }
        let uvs = o.f32_array("uvs")?.unwrap_or_default();
        if uvs.len() != positions.len() {
            return invalid(format!(
                "{}: {} UVs for {nv} vertices",
                info.id,
                uvs.len() / 2
            ));
        }
        let indices: Vec<u32> = o
            .i32_array("indices")?
            .unwrap_or_default()
            .into_iter()
            .map(|i| u32::try_from(i).ok().filter(|&i| (i as usize) < nv))
            .collect::<Option<_>>()
            .ok_or_else(|| {
                FormatError::Invalid(format!("{}: triangle index out of range", info.id))
            })?;
        if !indices.len().is_multiple_of(3) {
            return invalid(format!("{}: index count not a multiple of 3", info.id));
        }
        let id = info.id.clone();
        let (grid, blend_shapes) = self.keyforms(o, m, |f| {
            let positions = f.f32_array("positions")?.unwrap_or_default();
            if positions.len() != 2 * nv {
                return invalid(format!(
                    "{id}: keyform has {} coords, expected {}",
                    positions.len(),
                    2 * nv
                ));
            }
            Ok(ArtMeshForm {
                positions,
                draw_order: f.i32("drawOrder")?.unwrap_or(500) as f32,
                opacity: f.f32("opacity")?.unwrap_or(1.0),
                multiply: color(f.field("multiplyColor")?, WHITE)?,
                screen: color(f.field("screenColor")?, SCREEN_DEFAULT)?,
            })
        })?;

        let mut point_uids = Vec::new();
        let mut mesh = EditableMesh::default();
        let mut observed = false;
        let mut texture = None;
        let mut model_image = None;
        let mut generator = MeshGenerateSetting::default();
        let mut paths = Vec::new();
        for ext in o.list("_extensions")? {
            match ext.tag() {
                "CEditableMeshExtension" => {
                    if let Some(em) = ext.field("editableMesh")? {
                        point_uids = em
                            .i32_array("pointUid")?
                            .unwrap_or_default()
                            .into_iter()
                            .map(i64::from)
                            .collect();
                        mesh = self.editable_mesh(&em, &info.id, nv)?;
                    }
                }
                "CTopologyObserverExtension" => {
                    // Deform paths (own or as a sub mesh) are handled by
                    // the mesh editor (deform-paths.md §1); other observers
                    // block topology edits.
                    observed |= ext
                        .list("observers")?
                        .iter()
                        .any(|o| o.tag() != "CControllerExtension");
                }
                "CTextureInputExtension" => {
                    if let Some(cur) = ext.field("currentTextureInputData")? {
                        if let Some(g) = cur.guid("textureAtlasGuid")? {
                            texture = self.idx.textures_by_guid.get(g).copied();
                        }
                    }
                    for input in ext.list("_textureInputs")? {
                        if let Some(g) = input.guid("_modelImageGuid")? {
                            model_image = self.idx.model_images_by_guid.get(g).copied();
                        }
                    }
                }
                "CMeshGeneratorExtension" => {
                    if let Some(g) = ext.field("meshGenerateSetting")? {
                        generator = mesh_generate_setting(&g)?;
                    }
                }
                "CControllerExtension" => match deform_path(&ext) {
                    Ok(p) => paths.push(p),
                    Err(e) => self.warn(format!("{}: deform path skipped: {e}", info.id)),
                },
                _ => {}
            }
        }
        mesh.observed = observed;
        if point_uids.len() != nv {
            if !point_uids.is_empty() {
                self.warn(format!(
                    "{}: editable mesh has {} points for {nv} vertices",
                    info.id,
                    point_uids.len()
                ));
            }
            point_uids = (0..nv as i64).collect();
        }
        if texture.is_none() {
            if let Some(img) = o
                .field("texture")?
                .map(|t| t.field("srcImageResource"))
                .transpose()?
                .flatten()
            {
                texture = self.idx.texture_images.get(&img.identity()).copied();
            }
        }
        if let (Some(ti), Some(t)) = (texture, o.field("texture")?) {
            if let std::collections::hash_map::Entry::Vacant(e) = self.idx.sampling.entry(ti) {
                e.insert(texture_sampling(&t)?);
            }
        }
        let _ = m;
        let (clip, clip_missing) = self.clip_list(o)?;
        Ok(ArtMesh {
            clip,
            clip_missing,
            invert_clip: o.bool("invertClippingMask")?.unwrap_or(false),
            info,
            target,
            positions,
            uvs,
            indices,
            texture,
            culling: o.bool("culling")?.unwrap_or(false),
            color_composition: o
                .enum_value("colorComposition")?
                .and_then(ColorComposition::from_name)
                .unwrap_or_default(),
            alpha_composition: o
                .enum_value("alphaComposition")?
                .and_then(AlphaComposition::from_name)
                .unwrap_or_default(),
            user_data: o.string("userData")?.unwrap_or_default(),
            point_uids,
            mesh,
            model_image,
            generator,
            grid,
            blend_shapes,
            paths,
        })
    }

    /// The mesh-editor graph; malformed parts are dropped with a warning.
    /// spec: 10-formats/cmo3.md §4
    fn editable_mesh(&mut self, em: &O<'a, 'd>, id: &str, nv: usize) -> Result<EditableMesh> {
        let mut mesh = EditableMesh::default();
        if let Some(v) = em.attr("nextPointUid") {
            mesh.next_point_uid = v.trim().parse().unwrap_or(mesh.next_point_uid);
        }
        if let Some(v) = em.attr("useDelaunayTriangulation") {
            mesh.delaunay = crate::graph::parse_bool(v);
        }
        let points = em.i8_array("pointPriority")?.unwrap_or_default();
        if points.len() == nv {
            mesh.point_priority = points;
        }
        let edges = em.i16_array("edge")?.unwrap_or_default();
        let mut priority = em.i8_array("edgePriority")?.unwrap_or_default();
        priority.resize(edges.len() / 2, EDGE_NORMAL);
        let mut dropped = 0;
        for (e, &p) in edges.chunks_exact(2).zip(&priority) {
            // Stored as signed shorts; indices past 32767 wrap.
            let (a, b) = (u32::from(e[0] as u16), u32::from(e[1] as u16));
            if a as usize >= nv || b as usize >= nv || a == b {
                dropped += 1;
                continue;
            }
            mesh.edges.push([a, b]);
            mesh.edge_priority.push(p);
        }
        if dropped > 0 {
            self.warn(format!(
                "{id}: {dropped} editable-mesh edges are out of range"
            ));
        }
        Ok(mesh)
    }

    fn load_glue(&mut self, o: &O<'a, 'd>, m: &Model) -> Result<Glue> {
        let info = self.info(o)?;
        let mesh = |n: &str, l: &Self| -> Result<Option<MeshIndex>> {
            Ok(o.guid(n)?.and_then(|g| l.idx.meshes.get(g).copied()))
        };
        let mesh_a = mesh("targetArtMeshA_guid", self)?;
        let mesh_b = mesh("targetArtMeshB_guid", self)?;
        let uids = o.i64_array("bindVertexUids")?.unwrap_or_default();
        let w = o.f32_array("weights")?.unwrap_or_default();
        if uids.len() % 2 != 0 || w.len() != uids.len() {
            return invalid(format!(
                "{}: {} vertex UIDs, {} weights",
                info.id,
                uids.len(),
                w.len()
            ));
        }
        // Pairs are interleaved (uidA, uidB); weights likewise (wA, wB). spec: glue.md
        let resolve =
            |mi: Option<MeshIndex>, uid: i64| mi.and_then(|i| m.art_meshes[i].vertex_of_uid(uid));
        let pairs = uids
            .chunks(2)
            .map(|p| (resolve(mesh_a, p[0]), resolve(mesh_b, p[1])))
            .collect();
        let weights = w.chunks(2).map(|p| [p[0], p[1]]).collect();
        let (grid, blend_shapes) = self.keyforms(o, m, |f| {
            Ok(GlueForm {
                intensity: f.f32("intensity")?.unwrap_or(1.0),
            })
        })?;
        Ok(Glue {
            info,
            mesh_a,
            mesh_b,
            pairs,
            weights,
            grid,
            blend_shapes,
        })
    }

    /// Breaks target-deformer cycles (malformed input) by detaching.
    fn check_deformer_cycles(&mut self, m: &mut Model) {
        for start in 0..m.deformers.len() {
            let mut cur = m.deformers[start].target;
            let mut steps = 0;
            while let Some(d) = cur {
                if d == start || steps > m.deformers.len() {
                    self.warn(format!(
                        "{}: target deformer cycle broken",
                        m.deformers[start].info.id
                    ));
                    std::sync::Arc::make_mut(&mut m.deformers[start]).target = None;
                    break;
                }
                cur = m.deformers[d].target;
                steps += 1;
            }
        }
    }
}

fn vec2(o: Option<O>) -> Result<[f32; 2]> {
    Ok(match o {
        Some(v) => [v.f32("x")?.unwrap_or(0.0), v.f32("y")?.unwrap_or(0.0)],
        None => [0.0, 0.0],
    })
}

/// A deform path (`CControllerExtension`). spec: 35-editing/deform-paths.md §1
fn deform_path(ext: &O) -> Result<DeformPath> {
    let guid = ext.guid("guid")?.unwrap_or_default().to_owned();
    let mut p = DeformPath::new(guid, ext.i32("editLevel")?.unwrap_or(2).clamp(1, 3) as u8);
    p.max_bind = ext.i32("maxBindCount")?.unwrap_or(3);
    p.bind_method = ext
        .enum_value("bindMethod")?
        .and_then(BindMethod::from_name)
        .unwrap_or_default();
    p.sub_meshes = ext
        .list("subArtMeshGuids")?
        .iter()
        .filter_map(|g| g.attr("uuid").map(str::to_owned))
        .collect();
    let points = ext.list("controlPoints")?;
    let curves = ext.list("controlCurves")?;
    let point_index = |o: &O| points.iter().position(|q| q.identity() == o.identity());
    let curve_index = |o: &O| curves.iter().position(|q| q.identity() == o.identity());
    let mut curve_guids = Vec::new();
    for c in &curves {
        let cg = c.guid("curveId")?.unwrap_or_default().to_owned();
        curve_guids.push(cg.clone());
        let pts = c
            .list("_curvePoints")?
            .iter()
            .filter_map(&point_index)
            .collect();
        let mut curve = ControlCurve::new(cg, pts);
        curve.open = c.bool("isOpen")?.unwrap_or(true);
        curve.width = c.f32("lineWidth")?.unwrap_or(200.0);
        curve.hardness = c.f32("lineHardnessPercent")?.unwrap_or(50.0);
        p.curves.push(curve);
    }
    for q in &points {
        let mut anchor = PointInTriangle::default();
        if let Some(t) = q.field("pointInTriangle")? {
            for k in 0..3 {
                anchor.index[k] = t.i32(&format!("ptIndex{}", k + 1))?.unwrap_or(0);
                anchor.weight[k] = t.f32(&format!("weight{}", k + 1))?.unwrap_or(0.0);
            }
        }
        p.points.push(ControlPoint {
            guid: q.guid("ctrlPtId")?.unwrap_or_default().to_owned(),
            curve: q.field("assignedCurve")?.and_then(|c| curve_index(&c)),
            corner: q.bool("isCorner")?.unwrap_or(false),
            anchor,
            pos_default: vec2(q.field("posOnLocalOfDefaultKeyform")?)?,
            recovery: vec2(q.field("pointOnCanvasForRecovery")?)?,
            total_effect: q.f32("totalEffectToPointInTriangle")?.unwrap_or(0.0),
        });
    }
    for t in ext.list("targetPoints")? {
        let Some(r) = t.field("_point")? else {
            continue;
        };
        let mut effects = Vec::new();
        for e in t.list("effects")? {
            let w = e.f32("weight")?.unwrap_or(0.0);
            let Some(ep) = e.field("effectorPt")? else {
                continue;
            };
            let effector = match ep.tag() {
                "PointOnCurve" => {
                    let cg = ep.guid("curveId")?.unwrap_or_default();
                    let Some(curve) = curve_guids.iter().position(|g| g == cg) else {
                        continue;
                    };
                    Effector::Curve {
                        curve,
                        t: ep.f32("totalT")?.unwrap_or(0.0),
                        distance: ep.f32("distance")?.unwrap_or(0.0),
                        pos: vec2(ep.field("_posOnLocal")?)?,
                    }
                }
                _ => match point_index(&ep) {
                    Some(k) => Effector::Point(k),
                    None => continue,
                },
            };
            effects.push(Effect {
                effector,
                weight: w,
            });
        }
        p.targets.push(TargetPoint {
            mesh: r.guid("artMeshGuid")?.unwrap_or_default().to_owned(),
            uid: r.i64("pointUid")?.unwrap_or(-1),
            index: r.i32("_index")?.unwrap_or(0).max(0) as usize,
            form: match r.field("keyForm")? {
                Some(f) => f.guid("guid")?.unwrap_or_default().to_owned(),
                None => String::new(),
            },
            effects,
        });
    }
    Ok(p)
}
