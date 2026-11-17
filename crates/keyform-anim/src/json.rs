//! JSON helpers shared by the runtime animation formats.

use serde_json::{Number, Value};

/// A number rounded to at most `decimals` fraction digits; integral values
/// are written without a decimal point (`-8`, `0.067`), like the reference
/// files. spec: 60-animation/motion3-json.md §2, 70-advanced/viewer.md §1
pub fn num(v: f64, decimals: i32) -> Value {
    if !v.is_finite() {
        return Value::from(0);
    }
    // Decimal rounding of the exact binary value, ties to even (0.995 →
    // 0.99, 0.8125 → 0.812), as the reference's files show.
    let r: f64 = format!("{:.*}", decimals.max(0) as usize, v)
        .parse()
        .unwrap_or(0.0);
    let r = if r == 0.0 { 0.0 } else { r };
    if r == r.trunc() && r.abs() < 9.0e15 {
        Value::from(r as i64)
    } else {
        Number::from_f64(r).map_or(Value::from(0), Value::Number)
    }
}

/// Pretty JSON with tab indentation and no trailing newline.
pub fn to_string(v: &Value) -> String {
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    if serde::Serialize::serialize(v, &mut ser).is_err() {
        return String::new();
    }
    String::from_utf8(out).unwrap_or_default()
}

pub fn f32_of(v: &Value) -> Option<f32> {
    v.as_f64().filter(|x| x.is_finite()).map(|x| x as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_like_reference() {
        assert_eq!(num(-8.0, 3).to_string(), "-8");
        assert_eq!(num(0.0666667, 3).to_string(), "0.067");
        assert_eq!(num(1.005, 2).to_string(), "1");
        assert_eq!(num(0.995, 2).to_string(), "0.99");
        assert_eq!(num(0.8125, 3).to_string(), "0.812");
        assert_eq!(num(-0.0625, 3).to_string(), "-0.062");
        assert_eq!(num(30.25, 1).to_string(), "30.2");
        assert_eq!(num(f64::NAN, 3).to_string(), "0");
    }
}
