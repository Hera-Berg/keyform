//! Image tracks as scene layers: which are visible at a frame, where they
//! sit, and compositing them with the model track.
//! spec: 60-animation/animation-model.md §2, §5.1

use std::sync::Arc;

use keyform_anim::scene::Placement;
use keyform_formats::image::RgbaImage;
use keyform_model::animation::TrackKind;

use crate::document::Document;

/// A visible image track at one frame.
pub struct ImageLayer {
    pub guid: String,
    pub image: Arc<RgbaImage>,
    pub placement: Placement,
    /// Drawn in front of the model track.
    pub front: bool,
    /// Rendered per frame (a nested scene): its texture must be refreshed.
    pub dynamic: bool,
}

impl Document {
    /// The image tracks visible at scene `frame`, back to front: tracks are
    /// drawn depth-first in timeline order with the top track in front, so
    /// one listed above the model track is drawn in front of it.
    /// spec: animation-model.md §5.1 (drawing order)
    pub fn image_layers(&self, frame: f64, nested_k: f32) -> Vec<ImageLayer> {
        self.anim.as_ref().map_or_else(Vec::new, |a| {
            self.image_layers_in(a.scene, frame, nested_k, 0)
        })
    }

    /// [`Self::image_layers`] for scene `si`; nested scenes are rendered at
    /// `nested_k` pixels per nested-scene pixel (frame mapping §5.1), at
    /// most 4 levels deep.
    pub fn image_layers_in(
        &self,
        si: usize,
        frame: f64,
        nested_k: f32,
        depth: u32,
    ) -> Vec<ImageLayer> {
        let Some(st) = self.anim.as_ref() else {
            return Vec::new();
        };
        let Some(s) = st.doc.animation.scenes.get(si) else {
            return Vec::new();
        };
        let model = st.model_track_in(si).map_or(usize::MAX, |(i, _)| i);
        let mut out = Vec::new();
        for (i, t) in s.tracks.iter().enumerate().rev() {
            let in_time =
                frame >= f64::from(t.start) && frame < f64::from(t.start + t.duration.max(1));
            if !t.visible || t.guide || !in_time {
                continue;
            }
            let x = frame - f64::from(t.start) + f64::from(t.offset);
            match &t.kind {
                TrackKind::Image(it) => {
                    let Some(img) = it.resource.as_ref().and_then(|g| st.images.get(g)) else {
                        continue;
                    };
                    out.push(ImageLayer {
                        guid: t.guid.clone(),
                        image: img.clone(),
                        placement: keyform_anim::scene::placement(t, x),
                        front: i < model,
                        dynamic: false,
                    });
                }
                TrackKind::Scene(sc) => {
                    if depth >= 4 {
                        continue;
                    }
                    let Some(ni) = sc
                        .scene
                        .as_ref()
                        .and_then(|g| st.doc.animation.scenes.iter().position(|n| &n.guid == g))
                        .filter(|&ni| ni != si)
                    else {
                        continue;
                    };
                    let nf =
                        keyform_anim::scene::nested_frame(t, sc, &st.doc.animation.scenes[ni], x);
                    let Ok(img) =
                        crate::export_media::render_scene(self, ni, nf, nested_k, false, depth + 1)
                    else {
                        continue;
                    };
                    // The image is the nested canvas at `nested_k`: scale the
                    // placement back to nested-canvas pixels.
                    let mut pl = keyform_anim::scene::placement(t, x);
                    pl.anchor = [pl.anchor[0] * nested_k, pl.anchor[1] * nested_k];
                    pl.scale = [pl.scale[0] / nested_k, pl.scale[1] / nested_k];
                    out.push(ImageLayer {
                        guid: t.guid.clone(),
                        image: Arc::new(img),
                        placement: pl,
                        front: i < model,
                        dynamic: true,
                    });
                }
                _ => {}
            }
        }
        out
    }
}

/// Source-over of a straight-alpha colour onto a straight-alpha pixel.
fn over(dst: &mut [u8], s: [f32; 4]) {
    let d = [
        f32::from(dst[0]) / 255.0,
        f32::from(dst[1]) / 255.0,
        f32::from(dst[2]) / 255.0,
        f32::from(dst[3]) / 255.0,
    ];
    let a = s[3] + d[3] * (1.0 - s[3]);
    if a <= 0.0 {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    for i in 0..3 {
        let c = (s[i] * s[3] + d[i] * d[3] * (1.0 - s[3])) / a;
        dst[i] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    dst[3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

/// Bilinear sample of a straight-alpha image (premultiplied blend, result
/// straight), transparent outside.
fn sample(img: &RgbaImage, x: f32, y: f32) -> [f32; 4] {
    let (w, h) = (img.width as i64, img.height as i64);
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let mut acc = [0.0f32; 4];
    for (dx, dy, wgt) in [
        (0, 0, (1.0 - tx) * (1.0 - ty)),
        (1, 0, tx * (1.0 - ty)),
        (0, 1, (1.0 - tx) * ty),
        (1, 1, tx * ty),
    ] {
        let (px, py) = (x0 + dx, y0 + dy);
        if px < 0 || py < 0 || px >= w || py >= h {
            continue;
        }
        let o = 4 * (py * w + px) as usize;
        let a = f32::from(img.data[o + 3]) / 255.0;
        for i in 0..3 {
            acc[i] += wgt * a * f32::from(img.data[o + i]) / 255.0;
        }
        acc[3] += wgt * a;
    }
    let a = acc[3];
    if a > 0.0 {
        for c in acc.iter_mut().take(3) {
            *c /= a;
        }
    }
    acc
}

/// Draws an image layer onto `dst` (scene rendered at `k` pixels per scene
/// pixel), with its group opacity.
pub fn composite_image(dst: &mut RgbaImage, layer: &ImageLayer, k: f32) {
    let op = layer.placement.opacity.clamp(0.0, 1.0);
    if op <= 0.0 {
        return;
    }
    let w = dst.width as usize;
    for (i, px) in dst.data.chunks_exact_mut(4).enumerate() {
        let p = [((i % w) as f32 + 0.5) / k, ((i / w) as f32 + 0.5) / k];
        let Some(q) = layer.placement.unapply(p) else {
            return;
        };
        let mut s = sample(&layer.image, q[0], q[1]);
        if s[3] <= 0.0 {
            continue;
        }
        s[3] *= op;
        over(px, s);
    }
}

/// Draws a straight-alpha image of the same size over `dst`.
pub fn composite_over(dst: &mut RgbaImage, src: &RgbaImage) {
    for (d, s) in dst.data.chunks_exact_mut(4).zip(src.data.chunks_exact(4)) {
        if s[3] == 0 {
            continue;
        }
        over(
            d,
            [
                f32::from(s[0]) / 255.0,
                f32::from(s[1]) / 255.0,
                f32::from(s[2]) / 255.0,
                f32::from(s[3]) / 255.0,
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2 red image placed at (10, 10), scaled ×2: it covers scene
    /// pixels 9…11 and is half transparent with opacity 0.5.
    #[test]
    fn image_layer_composites() {
        let img = RgbaImage {
            width: 2,
            height: 2,
            data: [255u8, 0, 0, 255].repeat(4),
        };
        let layer = ImageLayer {
            guid: String::new(),
            image: Arc::new(img),
            placement: Placement {
                xy: [10.0, 10.0],
                anchor: [1.0, 1.0],
                scale: [2.0, 2.0],
                rotate: 0.0,
                shear: 0.0,
                opacity: 0.5,
            },
            front: true,
            dynamic: false,
        };
        let mut dst = RgbaImage {
            width: 20,
            height: 20,
            data: vec![0; 20 * 20 * 4],
        };
        composite_image(&mut dst, &layer, 1.0);
        let at = |x: usize, y: usize| &dst.data[4 * (y * 20 + x)..4 * (y * 20 + x) + 4];
        assert_eq!(at(10, 10), &[255, 0, 0, 128]);
        assert_eq!(at(2, 2), &[0, 0, 0, 0]);
    }
}

#[cfg(test)]
mod doc_tests {
    use crate::document::Document;
    use crate::log::Log;
    use keyform_model::animation::TrackKind;
    use std::path::Path;

    /// A new image track: centred, 100 %, drawn behind the model (listed
    /// below it); it saves, reloads with its image, and undoes.
    #[test]
    fn image_track_round_trip() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kf-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("bg.png");
        let img = keyform_formats::image::RgbaImage {
            width: 4,
            height: 2,
            data: [0u8, 128, 255, 255].repeat(8),
        };
        std::fs::write(&png, img.encode_png().unwrap()).unwrap();
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        doc.anim_structure("Add Image Track", |d, _| {
            d.add_image_track(0, &png.to_string_lossy(), (4, 2))
        })
        .unwrap();
        let a = doc.anim.as_ref().unwrap();
        let s = a.scene().unwrap();
        let t = s.tracks.last().unwrap();
        assert!(matches!(t.kind, TrackKind::Image(_)));
        let layers = doc.image_layers(0.0, 1.0);
        assert_eq!(layers.len(), 1);
        assert!(!layers[0].front);
        assert_eq!(
            layers[0].placement.xy,
            [s.canvas.0 as f32 / 2.0, s.canvas.1 as f32 / 2.0]
        );
        assert_eq!(layers[0].placement.anchor, [2.0, 1.0]);
        // The export draws it: the scene centre pixel shows it or the model.
        let out = dir.join("img.can3");
        doc.save_animation_to(Some(&out)).unwrap();
        let back = Document::open_animation(&out, &mut log).unwrap();
        assert_eq!(back.image_layers(0.0, 1.0).len(), 1, "{:?}", log.entries);
        assert_eq!(
            doc.anim.as_mut().unwrap().step(false).as_deref(),
            Some("Add Image Track")
        );
        assert!(doc.image_layers(0.0, 1.0).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod scene_track_tests {
    use crate::document::Document;
    use crate::log::Log;
    use keyform_model::animation::TrackKind;
    use std::path::Path;

    /// A scene track nesting a second scene: the nested duration, a layer
    /// rendered at the nested canvas size, drawn in the export; saved and
    /// reopened. spec: animation-model.md §5.1
    #[test]
    fn scene_track_nests_a_scene() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = root.join("hiyori_pro_t11.cmo3");
        if !model.exists() {
            return;
        }
        let mut log = Log::default();
        let mut doc = Document::new_animation(&model, &mut log).unwrap();
        doc.anim_structure("Add Scene", |d, m| {
            d.add_scene("Inner", m, 40, 30.0).map(|_| ())
        })
        .unwrap();
        doc.anim.as_mut().unwrap().scene = 0;
        doc.anim_structure("Add Scene Track", |d, _| d.add_scene_track(0, 1))
            .unwrap();
        let a = doc.anim.as_ref().unwrap();
        let s = &a.doc.animation.scenes[0];
        let t = s.tracks.last().unwrap();
        assert!(matches!(t.kind, TrackKind::Scene(_)));
        assert_eq!(t.duration, a.doc.animation.scenes[1].duration);
        let layers = doc.image_layers(0.0, 0.1);
        assert_eq!(layers.len(), 1);
        assert!(layers[0].dynamic && !layers[0].front);
        let inner = &doc.anim.as_ref().unwrap().doc.animation.scenes[1];
        assert_eq!(
            layers[0].image.width,
            (inner.canvas.0 as f32 * 0.1).floor() as u32
        );
        assert!(
            layers[0].image.data.chunks_exact(4).any(|p| p[3] > 0),
            "nested scene drawn"
        );
        crate::export_media::render_frame(&doc, 0.0, 10.0, true).unwrap();
        let dir = std::env::temp_dir().join(format!("kf-nest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("nest.can3");
        doc.save_animation_to(Some(&out)).unwrap();
        let back = Document::open_animation(&out, &mut log).unwrap();
        assert_eq!(back.image_layers(0.0, 0.1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
