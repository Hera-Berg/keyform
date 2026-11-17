//! Editing rules of the Physics Settings window that don't depend on the
//! UI: new and duplicated groups, pendulum rows, caution colouring, output
//! magnification and "similar changes".
//! spec: 70-advanced/physics-editor.md

use std::borrow::Borrow;

use keyform_model::{
    new_guid, Normalization, Parameter, PhysicsInput, PhysicsKind, PhysicsOutput, PhysicsRig,
    PhysicsSetting, PhysicsVertex,
};

/// A new pendulum row with the "New-row default" values (§6).
pub fn default_pendulum() -> PhysicsVertex {
    PhysicsVertex {
        position: [0.0, 0.0],
        mobility: 0.95,
        delay: 1.0,
        acceleration: 1.0,
        radius: 10.0,
    }
}

/// The root particle every group starts with (observed in the samples:
/// position (0, 0), mobility, delay and acceleration 1, radius 0).
pub fn root_vertex() -> PhysicsVertex {
    PhysicsVertex {
        position: [0.0, 0.0],
        mobility: 1.0,
        delay: 1.0,
        acceleration: 1.0,
        radius: 0.0,
    }
}

/// Field ranges of the pendulum table (§6): radius, mobility, delay,
/// acceleration as (min, max).
pub const RADIUS: (f32, f32) = (0.1, 100.0);
pub const MOBILITY: (f32, f32) = (0.01, 1.0);
pub const DELAY: (f32, f32) = (0.01, 10.0);
pub const ACCELERATION: (f32, f32) = (0.01, 10.0);
/// Output scale range (§5.2).
pub const SCALE: (f32, f32) = (0.0, 999.999);

/// Rest positions follow the radii: particle i sits at (0, Σ radius₁..ᵢ)
/// (physics.md §2; matches the samples' stored positions).
pub fn relayout(s: &mut PhysicsSetting) {
    let mut y = 0.0;
    for (i, v) in s.vertices.iter_mut().enumerate() {
        if i > 0 {
            y += v.radius;
        }
        v.position = [0.0, y];
    }
}

/// The next free `PhysicsSetting<n>` id (physics.md §4).
pub fn next_id(rig: &PhysicsRig) -> String {
    let used = |n: usize| {
        rig.settings
            .iter()
            .any(|s| s.id == format!("PhysicsSetting{n}"))
    };
    let n = (1..).find(|&n| !used(n)).unwrap_or(1);
    format!("PhysicsSetting{n}")
}

/// A new group named `name`: no inputs or outputs, default normalisation,
/// the root particle and one pendulum.
// TODO(Q-0038): the reference's new group without presets.
pub fn new_setting(rig: &PhysicsRig, name: &str) -> PhysicsSetting {
    let mut s = PhysicsSetting {
        guid: new_guid(),
        id: next_id(rig),
        name: name.to_owned(),
        enabled: true,
        inputs: Vec::new(),
        outputs: Vec::new(),
        vertices: vec![root_vertex(), default_pendulum()],
        position: Normalization::default(),
        angle: Normalization::default(),
    };
    relayout(&mut s);
    s
}

/// A unique copy name for duplicating `name` (§4 "unique copy name").
// TODO(Q-0038): the reference's copy-name format.
pub fn copy_name(rig: &PhysicsRig, name: &str) -> String {
    let taken = |n: &str| rig.settings.iter().any(|s| s.name == n);
    let first = format!("{name} copy");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|k| format!("{name} copy {k}"))
        .find(|n| !taken(n))
        .unwrap_or(first)
}

/// Duplicate of group `i` with a new GUID, id and copy name, inserted after it.
pub fn duplicate(rig: &mut PhysicsRig, i: usize) -> Option<usize> {
    let mut s = rig.settings.get(i)?.clone();
    s.guid = new_guid();
    s.id = next_id(rig);
    s.name = copy_name(rig, &s.name);
    rig.settings.insert(i + 1, s);
    Some(i + 1)
}

/// Type of a newly added input (§5.1): Angle if both the parameter's name
/// and its ID end in `Z`, otherwise Position X.
pub fn default_input_kind(p: &Parameter) -> PhysicsKind {
    if p.name.ends_with('Z') && p.id.ends_with('Z') {
        PhysicsKind::Angle
    } else {
        PhysicsKind::X
    }
}

/// A new input row for parameter `parameter` (weight 0, no reflect, §5.1).
pub fn new_input(params: &[impl Borrow<Parameter>], parameter: usize) -> Option<PhysicsInput> {
    let p = params.get(parameter)?.borrow();
    Some(PhysicsInput {
        parameter,
        weight: 0.0,
        kind: default_input_kind(p),
        reflect: false,
    })
}

/// A new output row on the last pendulum.
// TODO(Q-0038): the reference's new output row.
pub fn new_output(s: &PhysicsSetting, parameter: usize) -> PhysicsOutput {
    PhysicsOutput {
        parameter,
        vertex_index: s.vertices.len().saturating_sub(1).max(1),
        translation_scale: [1.0, 1.0],
        angle_scale: 1.0,
        weight: 100.0,
        kind: PhysicsKind::Angle,
        reflect: false,
    }
}

/// Sets the scale used by the output's type (the one written as `Scale`).
pub fn set_scale(o: &mut PhysicsOutput, v: f32) {
    match o.kind {
        PhysicsKind::X => o.translation_scale[0] = v,
        PhysicsKind::Y => o.translation_scale[1] = v,
        PhysicsKind::Angle => o.angle_scale = v,
    }
}

/// Number of pendulums of a group: its particles without the root
/// (observed in the reference's "Edit Physics Group" table).
pub fn pendulum_count(s: &PhysicsSetting) -> usize {
    s.vertices.len().saturating_sub(1)
}

/// Caution state of a cell or parameter (§4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Caution {
    #[default]
    None,
    /// Also an output of another group.
    Duplicate,
    /// Order problem: read before it is produced, or feeds back.
    Warning,
}

/// Caution of each input and output of each group, in priority order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupCaution {
    pub inputs: Vec<Caution>,
    pub outputs: Vec<Caution>,
}

impl GroupCaution {
    /// The colour of the input cell for inputs of `kind` (X or Angle).
    pub fn input_cell(&self, s: &PhysicsSetting, kind: PhysicsKind) -> Caution {
        s.inputs
            .iter()
            .zip(&self.inputs)
            .filter(|(i, _)| i.kind == kind)
            .map(|(_, c)| *c)
            .max_by_key(|c| *c as u8)
            .unwrap_or_default()
    }

    pub fn output_cell(&self) -> Caution {
        self.outputs
            .iter()
            .copied()
            .max_by_key(|c| *c as u8)
            .unwrap_or_default()
    }
}

/// Caution colouring of the group table (§4): an input warns when its
/// parameter is an output of this group or a later one; an output warns
/// when its parameter is an input of this group or an earlier one, and is
/// a duplicate when another group outputs it too.
pub fn caution(rig: &PhysicsRig) -> Vec<GroupCaution> {
    let s = &rig.settings;
    let outputs_of = |g: usize, p: usize| s[g].outputs.iter().any(|o| o.parameter == p);
    let inputs_of = |g: usize, p: usize| s[g].inputs.iter().any(|i| i.parameter == p);
    (0..s.len())
        .map(|g| GroupCaution {
            inputs: s[g]
                .inputs
                .iter()
                .map(|i| {
                    if (g..s.len()).any(|h| outputs_of(h, i.parameter)) {
                        Caution::Warning
                    } else {
                        Caution::None
                    }
                })
                .collect(),
            outputs: s[g]
                .outputs
                .iter()
                .map(|o| {
                    if (0..=g).any(|h| inputs_of(h, o.parameter)) {
                        Caution::Warning
                    } else if (0..s.len()).any(|h| h != g && outputs_of(h, o.parameter)) {
                        Caution::Duplicate
                    } else {
                        Caution::None
                    }
                })
                .collect(),
        })
        .collect()
}

/// Output Maximum (%) from the recorded extremes [lowest, highest] (§5.2);
/// `None` before anything was recorded.
pub fn output_max_percent(extremes: [f32; 2], p: &Parameter) -> Option<f32> {
    if !(extremes[0].is_finite() && extremes[1].is_finite()) {
        return None;
    }
    let range = p.min.abs().max(p.max.abs());
    if range <= 0.0 {
        return None;
    }
    Some(extremes[0].abs().max(extremes[1].abs()) / range * 100.0)
}

/// Which outputs "Increase Output" / "Decrease Output" adjust (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adjust {
    /// Ratio under 100 %.
    Increase,
    /// Ratio over 100 %.
    Decrease,
    /// Both ("…to fit in parameter range?").
    Fit,
}

/// Rounds up to 3 decimals (scale results, §5.2).
pub fn ceil3(v: f32) -> f32 {
    // Guard against values that are already on a 0.001 step.
    let x = f64::from(v) * 1000.0;
    let r = if (x - x.round()).abs() < 1e-6 {
        x.round()
    } else {
        x.ceil()
    };
    (r / 1000.0) as f32
}

/// Adjusts the scales of `s`'s outputs from their recorded extremes and
/// resets those extremes to the parameter range (so they show 100 %).
/// Returns the number of outputs changed.
pub fn adjust_outputs(
    s: &mut PhysicsSetting,
    extremes: &mut [[f32; 2]],
    params: &[impl Borrow<Parameter>],
    how: Adjust,
) -> usize {
    let mut n = 0;
    for (o, e) in s.outputs.iter_mut().zip(extremes.iter_mut()) {
        let Some(p) = params.get(o.parameter).map(Borrow::borrow) else {
            continue;
        };
        let Some(pct) = output_max_percent(*e, p) else {
            continue;
        };
        let r = pct / 100.0;
        let hit = match how {
            Adjust::Increase => r < 1.0,
            Adjust::Decrease => r > 1.0,
            Adjust::Fit => r != 1.0,
        };
        if !hit || r <= 0.0 {
            continue;
        }
        let v = ceil3((o.scale() / r).clamp(SCALE.0, SCALE.1));
        set_scale(o, v);
        *e = [p.min, p.max];
        n += 1;
    }
    n
}

/// Rescales recorded extremes when an output's scale changes (§5.2).
pub fn rescale_extremes(e: &mut [f32; 2], old: f32, new: f32) {
    if old != 0.0 && e[0].is_finite() {
        let f = new / old;
        e[0] *= f;
        e[1] *= f;
        if e[0] > e[1] {
            e.swap(0, 1);
        }
    }
}

// ---------------------------------------------------------------- Similar changes

/// Splits an ID into words from the left (§5.2): letter words
/// `[A-Za-z][a-z]*`, digit runs and runs of other characters.
pub fn id_words(id: &str) -> Vec<String> {
    let c: Vec<char> = id.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let start = i;
        if c[i].is_ascii_alphabetic() {
            i += 1;
            while i < c.len() && c[i].is_ascii_lowercase() {
                i += 1;
            }
        } else if c[i].is_ascii_digit() {
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
        } else {
            while i < c.len() && !c[i].is_ascii_alphanumeric() {
                i += 1;
            }
        }
        out.push(c[start..i].iter().collect());
    }
    out
}

/// One recorded word edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordEdit {
    Delete(String),
    Insert(String),
    Replace(String, String),
}

fn is_num(w: &str) -> bool {
    w.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// The word edits turning ID `a` into ID `b` (§5.2), equal words omitted.
pub fn word_edits(a: &str, b: &str) -> Vec<WordEdit> {
    let (a, b) = (id_words(a), id_words(b));
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        let both_num = is_num(&a[i]) && is_num(&b[j]);
        let in_a_later =
            a[i + 1..].contains(&b[j]) || (both_num && a[i + 1..].iter().any(|w| is_num(w)));
        let in_b_later =
            b[j + 1..].contains(&a[i]) || (both_num && b[j + 1..].iter().any(|w| is_num(w)));
        if in_a_later {
            out.push(WordEdit::Delete(a[i].clone()));
            i += 1;
        } else if in_b_later {
            out.push(WordEdit::Insert(b[j].clone()));
            j += 1;
        } else {
            out.push(WordEdit::Replace(a[i].clone(), b[j].clone()));
            i += 1;
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|w| WordEdit::Delete(w.clone())));
    out.extend(b[j..].iter().map(|w| WordEdit::Insert(w.clone())));
    out
}

/// Similar change candidates (§5.2) after output `changed` of group `s`
/// went from parameter `from` to `to`: other outputs whose parameter can
/// be renamed the same way into an existing parameter. Returns
/// (output index, new parameter).
pub fn similar_changes(
    s: &PhysicsSetting,
    params: &[impl Borrow<Parameter>],
    changed: usize,
    from: usize,
    to: usize,
) -> Vec<(usize, usize)> {
    let id = |p: usize| params.get(p).map(|q| q.borrow().id.clone());
    let (Some(a), Some(b)) = (id(from), id(to)) else {
        return Vec::new();
    };
    let edits = word_edits(&a, &b);
    if edits.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (k, o) in s.outputs.iter().enumerate() {
        if k == changed {
            continue;
        }
        let Some(x) = id(o.parameter) else { continue };
        let target = (0..params.len())
            .find(|&y| y != o.parameter && id(y).is_some_and(|yid| word_edits(&x, &yid) == edits));
        if let Some(y) = target {
            out.push((k, y));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::ParameterKind;
    use std::sync::Arc;

    fn param(id: &str, name: &str) -> Arc<Parameter> {
        let mut p = Parameter::new(id, -30.0, 30.0, 0.0);
        p.name = name.into();
        p.kind = ParameterKind::Normal;
        Arc::new(p)
    }

    #[test]
    fn words_and_similar_changes() {
        assert_eq!(id_words("ParamHairFront2"), ["Param", "Hair", "Front", "2"]);
        assert_eq!(
            id_words("Param_ABc12x"),
            ["Param", "_", "A", "Bc", "12", "x"]
        );
        assert_eq!(
            word_edits("ParamHairFront", "ParamHairBack"),
            [WordEdit::Replace("Front".into(), "Back".into())]
        );
        assert_eq!(
            word_edits("ParamHairFront", "ParamFront"),
            [WordEdit::Delete("Hair".into())]
        );
        assert_eq!(
            word_edits("ParamFront", "ParamHairFront"),
            [WordEdit::Insert("Hair".into())]
        );
        let params = vec![
            param("ParamHairFront", "a"),
            param("ParamHairBack", "b"),
            param("ParamHairFront2", "c"),
            param("ParamHairBack2", "d"),
            param("ParamHairSide", "e"),
        ];
        let mut s = new_setting(&PhysicsRig::default(), "g");
        for p in [1, 2, 4] {
            s.outputs.push(new_output(&s, p));
        }
        // Output 0 changed from ParamHairFront to ParamHairBack.
        assert_eq!(similar_changes(&s, &params, 0, 0, 1), vec![(1, 3)]);
    }

    #[test]
    fn caution_rules() {
        let mut rig = PhysicsRig::default();
        let mut a = new_setting(&rig, "a");
        a.inputs.push(PhysicsInput {
            parameter: 0,
            weight: 50.0,
            kind: PhysicsKind::X,
            reflect: false,
        });
        a.outputs.push(new_output(&a, 1));
        rig.settings.push(a);
        let mut b = new_setting(&rig, "b");
        // Reads a's output (fine: produced earlier), outputs a's input
        // (warning: a read it before) and a's output (duplicate).
        b.inputs.push(PhysicsInput {
            parameter: 1,
            weight: 50.0,
            kind: PhysicsKind::Angle,
            reflect: false,
        });
        b.outputs.push(new_output(&b, 0));
        b.outputs.push(new_output(&b, 2));
        rig.settings.push(b);
        let mut c = new_setting(&rig, "c");
        c.outputs.push(new_output(&c, 1));
        rig.settings.push(c);
        let k = caution(&rig);
        // a's input 0 is output later by b → warning.
        assert_eq!(k[0].inputs, [Caution::Warning]);
        // a's output 1 is also output by c → duplicate.
        assert_eq!(k[0].outputs, [Caution::Duplicate]);
        // b's input 1 is output later by c → warning.
        assert_eq!(k[1].inputs, [Caution::Warning]);
        assert_eq!(k[1].outputs, [Caution::Warning, Caution::None]);
        assert_eq!(
            k[1].input_cell(&rig.settings[1], PhysicsKind::Angle),
            Caution::Warning
        );
        assert_eq!(
            k[1].input_cell(&rig.settings[1], PhysicsKind::X),
            Caution::None
        );
        assert_eq!(next_id(&rig), "PhysicsSetting4");
    }

    #[test]
    fn output_adjust_and_layout() {
        let params = vec![param("ParamA", "a")];
        let mut s = new_setting(&PhysicsRig::default(), "g");
        s.outputs.push(new_output(&s, 0));
        s.outputs.push(new_output(&s, 0));
        set_scale(&mut s.outputs[0], 2.0);
        // Output 0 reached 15 of 30 (50 %), output 1 reached 45 (150 %).
        let mut ext = vec![[-15.0, 10.0], [-5.0, 45.0]];
        assert_eq!(output_max_percent(ext[0], &params[0]), Some(50.0));
        assert_eq!(
            adjust_outputs(&mut s, &mut ext, &params, Adjust::Increase),
            1
        );
        assert_eq!(s.outputs[0].scale(), 4.0);
        assert_eq!(ext[0], [-30.0, 30.0]);
        assert_eq!(
            adjust_outputs(&mut s, &mut ext, &params, Adjust::Decrease),
            1
        );
        // 1 / 1.5 = 0.6667 rounded up.
        assert_eq!(s.outputs[1].scale(), 0.667);
        assert_eq!(ceil3(0.5), 0.5);
        s.vertices.push(PhysicsVertex {
            radius: 5.0,
            ..default_pendulum()
        });
        relayout(&mut s);
        assert_eq!(s.vertices[2].position, [0.0, 15.0]);
        assert_eq!(pendulum_count(&s), 2);
        let p = param("ParamAngleZ", "Angle Z");
        assert_eq!(default_input_kind(&p), PhysicsKind::Angle);
        assert_eq!(
            default_input_kind(&param("ParamAngleZ", "Angle z")),
            PhysicsKind::X
        );
    }
}
