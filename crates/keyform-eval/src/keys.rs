//! Key insertion with keyforms created by interpolation.
//!
//! spec: 35-editing/keys.md, 10-formats/moc3.md §9 step 4

use keyform_model::{KeyformGrid, Parameter};

use crate::interp::{select_keys, Blend};

/// Inserts key `value` into binding `j`, creating the new cells' keyforms by
/// interpolating along that binding. No-op if a key within the parameter's
/// snap epsilon already exists. Returns the key's index.
pub fn insert_key<F: Blend>(
    grid: &mut KeyformGrid<F>,
    j: usize,
    param: &Parameter,
    value: f32,
) -> Option<usize> {
    let b = grid.bindings.get(j)?;
    if let Some(i) = b
        .keys
        .iter()
        .position(|k| (k - value).abs() < param.snap_epsilon)
    {
        return Some(i);
    }
    let old_keys = b.keys.clone();
    let pos = old_keys
        .iter()
        .position(|&k| k > value)
        .unwrap_or(old_keys.len());
    let sel = select_keys(param, &old_keys, value);

    let old_dims: Vec<usize> = grid.bindings.iter().map(|b| b.keys.len()).collect();
    let mut new_dims = old_dims.clone();
    new_dims[j] += 1;
    let cells: usize = new_dims.iter().product();
    let old_cell = |idx: &[usize]| -> usize {
        let mut c = 0;
        let mut stride = 1;
        for (d, &i) in old_dims.iter().zip(idx) {
            c += i * stride;
            stride *= d;
        }
        c
    };

    let mut out = Vec::with_capacity(cells);
    let mut idx = vec![0usize; new_dims.len()];
    for cell in 0..cells {
        // Decode `cell` into per-binding indices (first binding fastest).
        let mut rest = cell;
        for (d, n) in new_dims.iter().enumerate() {
            idx[d] = rest % n;
            rest /= n;
        }
        let ij = idx[j];
        let form = if ij == pos {
            let mut acc: Option<F> = None;
            for (k, w) in sel.iter() {
                let mut o = idx.clone();
                o[j] = k;
                let src = grid.keyforms.get(old_cell(&o))?;
                let a = acc.get_or_insert_with(|| src.zeroed());
                a.add_scaled(src, w);
            }
            let mut f = acc?;
            f.finish();
            f
        } else {
            let mut o = idx.clone();
            o[j] = if ij > pos { ij - 1 } else { ij };
            grid.keyforms.get(old_cell(&o))?.clone()
        };
        out.push(form);
    }
    let b = &mut grid.bindings[j];
    b.keys.insert(pos, value);
    grid.keyforms = out;
    Some(pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyform_model::*;

    fn f(x: f32) -> GlueForm {
        GlueForm { intensity: x }
    }

    #[test]
    fn inserts_interpolated_keyform_in_2d_grid() {
        let p0 = Parameter::new("A", -10.0, 10.0, 0.0);
        let mut g = KeyformGrid {
            bindings: vec![
                Binding::new(0, vec![0.0, 10.0]),
                Binding::new(1, vec![0.0, 1.0]),
            ],
            keyforms: vec![f(0.0), f(1.0), f(2.0), f(3.0)],
        };
        assert_eq!(insert_key(&mut g, 0, &p0, 5.0), Some(1));
        assert_eq!(g.bindings[0].keys, vec![0.0, 5.0, 10.0]);
        let v: Vec<f32> = g.keyforms.iter().map(|k| k.intensity).collect();
        assert_eq!(v, vec![0.0, 0.5, 1.0, 2.0, 2.5, 3.0]);
        // Below the first key: nearest-key fallback copies the first keyform.
        assert_eq!(insert_key(&mut g, 0, &p0, -10.0), Some(0));
        assert_eq!(g.keyforms[0].intensity, 0.0);
        assert!(g.is_consistent());
        // Existing key: no change.
        assert_eq!(insert_key(&mut g, 0, &p0, 5.0005), Some(2));
    }

    #[test]
    fn repeat_parameter_wraps_when_inserting_ends() {
        let mut p = Parameter::new("R", -180.0, 180.0, 0.0);
        p.repeat = true;
        let mut g = KeyformGrid {
            bindings: vec![Binding::new(0, vec![-90.0, 90.0])],
            keyforms: vec![f(0.0), f(1.0)],
        };
        insert_key(&mut g, 0, &p, 180.0);
        // 180 lies halfway between 90 (→1) and 270 ≡ −90 (→0).
        assert!((g.keyforms[2].intensity - 0.5).abs() < 1e-6);
    }
}
