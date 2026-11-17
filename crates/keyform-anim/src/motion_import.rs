//! motion3.json import into an animation scene: segments back to keys.
//! spec: 60-animation/motion3-json.md §4

use keyform_model::animation::{Curve, CurveType, Handle, Key};

use crate::motion::{Motion, Segment, Target};

/// One imported curve: its target, ID, keyframe curve and per-curve fades
/// (ms, −1 = unset).
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedCurve {
    pub target: Target,
    pub id: String,
    pub curve: Curve,
    pub fade_in_ms: i32,
    pub fade_out_ms: i32,
}

/// The keys of `m`'s curves at `fps`: every segment end point (and the
/// first point) becomes a key at frame round(time × fps); a key takes the
/// type of the segment that follows it (the last key SMOOTH); a Bézier
/// segment's control points become the start key's next and the end key's
/// prev handle. User data → (frame, text) with frame = (int)(time × fps).
pub fn import_motion(m: &Motion, fps: f64) -> (Vec<ImportedCurve>, Vec<(i32, String)>) {
    let frame = |t: f32| (f64::from(t) * fps).round() as i32;
    let mut out = Vec::new();
    for c in &m.curves {
        let mut keys = vec![Key {
            frame: frame(c.start[0]),
            value: f64::from(c.start[1]),
            ..Default::default()
        }];
        let mut types = Vec::new();
        for s in &c.segments {
            let (t, end) = match *s {
                Segment::Linear(p) => (CurveType::Linear, p),
                Segment::Stepped(p) => (CurveType::Step, p),
                Segment::InverseStepped(p) => (CurveType::InverseStep, p),
                Segment::Bezier { c1, c2, end } => {
                    if let Some(k) = keys.last_mut() {
                        k.next = Some(Handle {
                            frame: (f64::from(c1[0]) * fps) as f32,
                            value: f64::from(c1[1]),
                        });
                    }
                    let mut k = Key {
                        frame: frame(end[0]),
                        value: f64::from(end[1]),
                        ..Default::default()
                    };
                    k.prev = Some(Handle {
                        frame: (f64::from(c2[0]) * fps) as f32,
                        value: f64::from(c2[1]),
                    });
                    types.push(CurveType::Bezier);
                    if keys.last().is_some_and(|l| l.frame == k.frame) {
                        keys.pop();
                        types.pop();
                    }
                    keys.push(k);
                    continue;
                }
            };
            types.push(t);
            let k = Key {
                frame: frame(end[0]),
                value: f64::from(end[1]),
                ..Default::default()
            };
            // Coincident frames (sub-frame segments): the later key wins.
            if keys.last().is_some_and(|l| l.frame == k.frame) {
                keys.pop();
                types.pop();
            }
            keys.push(k);
        }
        types.push(CurveType::Smooth);
        types.truncate(keys.len());
        while types.len() < keys.len() {
            types.push(CurveType::Smooth);
        }
        let ms = |f: Option<f32>| f.map_or(-1, |s| (s * 1000.0).round() as i32);
        out.push(ImportedCurve {
            target: c.target,
            id: c.id.clone(),
            curve: Curve {
                base: f64::from(c.start[1]),
                keys,
                types,
                fixed: false,
            },
            fade_in_ms: ms(c.fade_in),
            fade_out_ms: ms(c.fade_out),
        });
    }
    let events = m
        .user_data
        .iter()
        .map(|(t, v)| ((f64::from(*t) * fps) as i32, v.clone()))
        .collect();
    (out, events)
}
