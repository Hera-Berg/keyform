//! A new, empty model project: the document skeleton the editor writes for
//! a fresh model (root part, root parameter group, empty object sets and
//! texture manager, default settings). spec: 10-formats/cmo3.md §1–2,
//! 10-formats/caff-archive.md §5; defaults as observed in a new model saved
//! by the reference editor (black box).

use crate::caff::{CaffArchive, CaffEntry, Compression};
use crate::error::Result;
use crate::graph::format_f32;
use crate::image::RgbaImage;
use crate::xml::{Pi, XmlDocument};

use super::Cmo3Document;

/// `<root fileFormatVersion>` for 5.3.04 compatibility (cmo3.md §1).
const FILE_FORMAT_VERSION: &str = "503040001";

/// Version markers of the types the skeleton (and new objects) use.
const VERSIONS: &[&str] = &[
    "CArtMeshSource:5",
    "KeyformGridSource:1",
    "CParameterGroup:4",
    "SerializeFormatVersion:2",
    "CModelSource:15",
    "CFloatColor:1",
    "CPartForm:2",
    "CPartSource:2",
    "CLabelColor:0",
    "CModelImage:3",
];

fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

impl Cmo3Document {
    /// A new model named `name` with a `width` × `height` canvas, targeting
    /// SDK 5.3, with no objects but the root part and root parameter group.
    pub fn new_model(name: &str, width: u32, height: u32) -> Result<Self> {
        let guid = keyform_model::new_guid;
        let mut archive = CaffArchive::new_document();
        // Obfuscation on, with a random key (caff-archive.md §5).
        let k = guid();
        archive.key = i32::from_str_radix(&k.replace('-', "")[..8], 16)
            .unwrap_or(0x5a5a_5a5a)
            .max(1);
        // Blank model icons.
        let mut icons = Vec::new();
        for size in [64u32, 32, 16] {
            let path = format!("image_{size}.png");
            archive.entries.push(CaffEntry {
                path: path.clone(),
                tag: String::new(),
                obfuscated: true,
                compression: Compression::Raw,
                data: RgbaImage::new(size, size).encode_png()?,
            });
            icons.push(path);
        }
        let xml = format!(
            r##"<root fileFormatVersion="{ffv}">
<shared>
<CParameterGroupGuid uuid="{group_guid}" note="Root Parameter Group" xs.id="#0" xs.idx="0" />
<CPartGuid uuid="{part_guid}" note="__RootPart__" xs.id="#1" xs.idx="1" />
<CFormGuid uuid="{form_guid}" note="(no debug info)" xs.id="#2" xs.idx="2" />
<CPartSource xs.id="#3" xs.idx="3">
<ACParameterControllableSource xs.n="super">
<s xs.n="localName">Root Part</s>
<b xs.n="isVisible">true</b>
<b xs.n="isLocked">false</b>
<null xs.n="parentGuid" />
<KeyformGridSource xs.n="keyformGridSource">
<array_list xs.n="keyformsOnGrid" count="1">
<KeyformOnGrid>
<KeyformGridAccessKey xs.n="accessKey">
<array_list xs.n="_keyOnParameterList" count="0" />
</KeyformGridAccessKey>
<CFormGuid xs.n="keyformGuid" xs.ref="#2" />
</KeyformOnGrid>
</array_list>
<array_list xs.n="keyformBindings" count="0" />
</KeyformGridSource>
<KeyFormMorphTargetSet xs.n="keyformMorphTargetSet">
<carray_list xs.n="_morphTargets" count="0" />
<MorphTargetBlendWeightConstraintSet xs.n="blendWeightConstraintSet">
<carray_list xs.n="_constraints" count="0" />
</MorphTargetBlendWeightConstraintSet>
</KeyFormMorphTargetSet>
<carray_list xs.n="_extensions" count="0" />
<null xs.n="internalColor_direct_argb" />
<CLabelColor xs.n="labelColor" customizedColorInt="-1">
<CLabelColorType xs.n="labelType" v="UNDEFINED" />
</CLabelColor>
</ACParameterControllableSource>
<CPartGuid xs.n="guid" xs.ref="#1" />
<CPartId xs.n="id" idstr="__RootPart__" />
<carray_list xs.n="keyforms" count="1">
<CPartForm>
<ACForm xs.n="super">
<CFormGuid xs.n="guid" xs.ref="#2" />
<b xs.n="isAnimatedForm">false</b>
<b xs.n="isLocalAnimatedForm">false</b>
<CPartSource xs.n="_source" xs.ref="#3" />
<null xs.n="name" />
<s xs.n="notes" />
</ACForm>
<i xs.n="drawOrder">500</i>
<f xs.n="opacity">1.0</f>
<CFloatColor xs.n="multiplyColor" red="1.0" green="1.0" blue="1.0" alpha="1.0" />
<CFloatColor xs.n="screenColor" red="0.0" green="0.0" blue="0.0" alpha="1.0" />
</CPartForm>
</carray_list>
<b xs.n="enableDrawOrderGroup">false</b>
<i xs.n="defaultOrder_forEditor">500</i>
<b xs.n="isSketch">false</b>
<CColor xs.n="partsEditColor" />
<carray_list xs.n="_childGuids" count="0" />
<b xs.n="useOffscreen">false</b>
<carray_list xs.n="clipGuidList" count="0" />
<b xs.n="invertClippingMask">false</b>
<ColorComposition xs.n="colorComposition" v="NORMAL" />
<AlphaComposition xs.n="alphaComposition" v="OVER" />
</CPartSource>
<CParameterGroupId idstr="ParamGroupRoot" xs.id="#4" xs.idx="4" />
<CParameterGroup xs.id="#5" xs.idx="5">
<s xs.n="name">Root Parameter Group</s>
<s xs.n="description" />
<b xs.n="folderIsOpened">false</b>
<CParameterGroupGuid xs.n="guid" xs.ref="#0" />
<null xs.n="parentGroupGuid" />
<carray_list xs.n="_childGuids" count="0" />
<CParameterGroupId xs.n="id" xs.ref="#4" />
<CLabelColor xs.n="labelColor" customizedColorInt="-1">
<CLabelColorType xs.n="labelType" v="UNDEFINED" />
</CLabelColor>
</CParameterGroup>
</shared>
<main>
<CModelSource isDefaultKeyformLocked="false">
<CModelGuid xs.n="guid" uuid="{model_guid}" note="(no debug info)" />
<s xs.n="name">{name}</s>
<EditorEdition xs.n="editorEdition">
<i xs.n="edition">11</i>
</EditorEdition>
<CImageCanvas xs.n="canvas">
<i xs.n="pixelWidth">{width}</i>
<i xs.n="pixelHeight">{height}</i>
<CColor xs.n="background" />
</CImageCanvas>
<CParameterSourceSet xs.n="parameterSourceSet">
<carray_list xs.n="_sources" count="0" />
</CParameterSourceSet>
<CTextureManager xs.n="textureManager">
<TextureImageGroup xs.n="textureList">
<carray_list xs.n="children" count="0" />
</TextureImageGroup>
<carray_list xs.n="_rawImages" count="0" />
<carray_list xs.n="_modelImageGroups" count="0" />
<carray_list xs.n="_textureAtlases" count="0" />
<b xs.n="isTextureInputModelImageMode">true</b>
<i xs.n="previewReductionRatio">1</i>
<carray_list xs.n="artPathBrushUsingLayeredImageIds" count="0" />
</CTextureManager>
<b xs.n="useLegacyDrawOrder__testImpl">false</b>
<CDrawableSourceSet xs.n="drawableSourceSet">
<carray_list xs.n="_sources" count="0" />
</CDrawableSourceSet>
<CDeformerSourceSet xs.n="deformerSourceSet">
<carray_list xs.n="_sources" count="0" />
</CDeformerSourceSet>
<CAffecterSourceSet xs.n="affecterSourceSet">
<carray_list xs.n="_sources" count="0" />
</CAffecterSourceSet>
<CPartSourceSet xs.n="partSourceSet">
<carray_list xs.n="_sources" count="1">
<CPartSource xs.ref="#3" />
</carray_list>
</CPartSourceSet>
<CPhysicsSettingsSourceSet xs.n="physicsSettingsSourceSet">
<carray_list xs.n="_sourceCubismPhysics" count="0" />
<null xs.n="selectedCubismPhysics" />
<i xs.n="settingFPS">60</i>
</CPhysicsSettingsSourceSet>
<CPartSource xs.n="rootPart" xs.ref="#3" />
<CParameterGroupSet xs.n="parameterGroupSet">
<carray_list xs.n="_groups" count="1">
<CParameterGroup xs.ref="#5" />
</carray_list>
</CParameterGroupSet>
<CParameterGroup xs.n="rootParameterGroup" xs.ref="#5" />
<CModelInfo xs.n="modelInfo">
<f xs.n="pixelsPerUnit">{ppu}</f>
<CPoint xs.n="originInPixels">
<i xs.n="x">0</i>
<i xs.n="y">0</i>
</CPoint>
<CEffectParameterGroups xs.n="_effectParameterGroups">
<hash_map xs.n="_parameterGroups" count="0" keyType="string" />
</CEffectParameterGroups>
</CModelInfo>
<hash_map xs.n="modelOptions" count="0" keyType="string" />
<CImageIcon xs.n="_icon64">
<CWritableImage xs.n="image" width="64" height="64" type="INT_ARGB">
<file xs.n="image" path="{icon64}" />
</CWritableImage>
</CImageIcon>
<CImageIcon xs.n="_icon32">
<CWritableImage xs.n="image" width="32" height="32" type="INT_ARGB">
<file xs.n="image" path="{icon32}" />
</CWritableImage>
</CImageIcon>
<CImageIcon xs.n="_icon16">
<CWritableImage xs.n="image" width="16" height="16" type="INT_ARGB">
<file xs.n="image" path="{icon16}" />
</CWritableImage>
</CImageIcon>
<CGameMotionSet xs.n="gameMotionSet">
<carray_list xs.n="gameMotions" count="0" />
<carray_list xs.n="gameMotionGroups" count="0" />
</CGameMotionSet>
<ModelViewerSetting xs.n="modelViewerSetting">
<array_list xs.n="trackCursorSettings" count="0" />
</ModelViewerSetting>
<CGuidesSetting xs.n="guides">
<carray_list xs.n="guidesModeling" count="0" />
</CGuidesSetting>
<i xs.n="targetVersionNo">5030000</i>
<i xs.n="latestVersionOfLastModelerNo">5030000</i>
<CArtPathBrushSetting xs.n="artPathBrushesSetting">
<carray_list xs.n="brushes" count="0" />
</CArtPathBrushSetting>
<CRandomPoseSettingManager xs.n="randomPoseSetting">
<array_list xs.n="_settings" count="0" />
<i xs.n="currentIndex">0</i>
</CRandomPoseSettingManager>
<CMotionSyncSettingSourceSet xs.n="motionSyncSettingsSet">
<linked_set xs.n="_settingSourceSetMotionSync" count="0" />
</CMotionSyncSettingSourceSet>
</CModelSource>
</main>
</root>
"##,
            ffv = FILE_FORMAT_VERSION,
            group_guid = guid(),
            part_guid = guid(),
            form_guid = guid(),
            model_guid = guid(),
            name = xml_text(name),
            width = width,
            height = height,
            ppu = format_f32(1.0),
            icon64 = icons[0],
            icon32 = icons[1],
            icon16 = icons[2],
        );
        let mut doc = XmlDocument::parse(xml.as_bytes())?;
        doc.pis = VERSIONS
            .iter()
            .map(|v| Pi {
                target: "version".into(),
                data: (*v).into(),
            })
            .chain(
                super::schema_imports::imports_used(&doc.root)
                    .into_iter()
                    .map(|i| Pi {
                        target: "import".into(),
                        data: i.into(),
                    }),
            )
            .collect();
        archive.entries.insert(
            0,
            CaffEntry {
                path: "main.xml".into(),
                tag: "main_xml".into(),
                obfuscated: true,
                compression: Compression::Fast,
                data: doc.write(),
            },
        );
        let bytes = archive.write()?;
        Cmo3Document::read(&bytes)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn new_model_loads_and_saves() {
        let mut doc = Cmo3Document::new_model("Test & Model", 800, 600).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        let m = &doc.model;
        assert_eq!((m.canvas.width, m.canvas.height), (800, 600));
        assert_eq!(m.name, "Test & Model");
        assert_eq!(m.target_version, 5_030_000);
        assert_eq!(m.parts.len(), 1);
        assert_eq!(m.root_part, Some(0));
        assert!(m.parameters.is_empty() && m.art_meshes.is_empty());
        // An unchanged save round-trips.
        let bytes = doc.save_bytes().unwrap();
        let back = Cmo3Document::read(&bytes).unwrap();
        assert!(back.model.same_data(&doc.model));
    }
}
