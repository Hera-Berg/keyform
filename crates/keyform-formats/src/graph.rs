//! Read access to the XML object graph: shared-object references, fields
//! (attribute or child), scalars, arrays and lists.
//!
//! spec: 10-formats/xml-object-graph.md §2, §4, §6

use std::collections::HashMap;

use crate::error::{FormatError, Result};
use crate::xml::{Element, XmlDocument};

/// Index over a parsed document's `<shared>` objects.
pub struct ObjectGraph<'d> {
    doc: &'d XmlDocument,
    shared: HashMap<&'d str, &'d Element>,
}

impl<'d> ObjectGraph<'d> {
    pub fn new(doc: &'d XmlDocument) -> Result<Self> {
        let mut shared = HashMap::new();
        if let Some(s) = doc.root.children.iter().find(|c| c.name == "shared") {
            for e in &s.children {
                let id = e.attr("xs.id").ok_or_else(|| {
                    FormatError::Invalid(format!("shared <{}> without xs.id", e.name))
                })?;
                shared.insert(id, e);
            }
        }
        Ok(Self { doc, shared })
    }

    pub fn document(&self) -> &'d XmlDocument {
        self.doc
    }

    /// The document root object (first child of `<main>`).
    pub fn main(&self) -> Result<Obj<'_, 'd>> {
        let main = self
            .doc
            .root
            .children
            .iter()
            .find(|c| c.name == "main")
            .and_then(|m| m.children.first())
            .ok_or_else(|| FormatError::Invalid("document has no <main> object".into()))?;
        self.obj(main)
    }

    /// Wraps `e`, following an `xs.ref` to its shared object. A ref to an
    /// unknown id is a hard error (§4).
    pub fn obj<'g>(&'g self, e: &'d Element) -> Result<Obj<'g, 'd>> {
        match e.attr("xs.ref") {
            None => Ok(Obj { g: self, e }),
            Some(id) => self
                .shared
                .get(id)
                .map(|&e| Obj { g: self, e })
                .ok_or_else(|| FormatError::Invalid(format!("ref id {id}: object not found"))),
        }
    }
}

/// A resolved object element.
#[derive(Clone, Copy)]
pub struct Obj<'g, 'd> {
    g: &'g ObjectGraph<'d>,
    e: &'d Element,
}

impl<'g, 'd> Obj<'g, 'd> {
    pub fn element(&self) -> &'d Element {
        self.e
    }

    pub fn tag(&self) -> &'d str {
        &self.e.name
    }

    pub fn is_null(&self) -> bool {
        self.e.name == "null"
    }

    /// Identity key for shared-object detection (stable for one graph).
    pub fn identity(&self) -> *const Element {
        self.e
    }

    pub fn attr(&self, name: &str) -> Option<&'d str> {
        self.e.attr(name)
    }

    /// Child object stored under field `name` on this element only. `<null/>`
    /// values read as `None`.
    pub fn own_field(&self, name: &str) -> Result<Option<Obj<'g, 'd>>> {
        match self.e.child_field(name) {
            None => Ok(None),
            Some(c) => {
                let o = self.g.obj(c)?;
                Ok((!o.is_null()).then_some(o))
            }
        }
    }

    /// The parent-type part (`xs.n="super"`).
    pub fn sup(&self) -> Result<Option<Obj<'g, 'd>>> {
        self.own_field("super")
    }

    /// Field `name` on this element or, failing that, on its `super` chain.
    pub fn field(&self, name: &str) -> Result<Option<Obj<'g, 'd>>> {
        let mut cur = Some(*self);
        let mut depth = 0;
        while let Some(o) = cur {
            if let Some(c) = o.e.child_field(name) {
                let r = self.g.obj(c)?;
                return Ok((!r.is_null()).then_some(r));
            }
            depth += 1;
            if depth > 64 {
                return Err(FormatError::Invalid("super chain too deep".into()));
            }
            cur = o.sup()?;
        }
        Ok(None)
    }

    pub fn req(&self, name: &str) -> Result<Obj<'g, 'd>> {
        self.field(name)?
            .ok_or_else(|| FormatError::Invalid(format!("<{}> lacks field {name:?}", self.e.name)))
    }

    /// Scalar text of field `name`: an attribute on this element or a super
    /// element, else a child scalar element's text (§2.1–2.2).
    pub fn scalar(&self, name: &str) -> Result<Option<&'d str>> {
        let mut cur = Some(*self);
        let mut depth = 0;
        while let Some(o) = cur {
            if let Some(v) = o.e.attr(name) {
                return Ok(Some(v));
            }
            if let Some(c) = o.e.child_field(name) {
                return Ok(if c.name == "null" {
                    None
                } else {
                    Some(c.text.as_str())
                });
            }
            depth += 1;
            if depth > 64 {
                return Err(FormatError::Invalid("super chain too deep".into()));
            }
            cur = o.sup()?;
        }
        Ok(None)
    }

    pub fn string(&self, name: &str) -> Result<Option<String>> {
        Ok(self.scalar(name)?.map(str::to_owned))
    }

    pub fn f32(&self, name: &str) -> Result<Option<f32>> {
        self.scalar(name)?
            .map(|s| parse_f32(s).ok_or_else(|| bad(name, s)))
            .transpose()
    }

    pub fn f64(&self, name: &str) -> Result<Option<f64>> {
        self.scalar(name)?
            .map(|s| parse_f64(s).ok_or_else(|| bad(name, s)))
            .transpose()
    }

    pub fn i64(&self, name: &str) -> Result<Option<i64>> {
        self.scalar(name)?
            .map(|s| s.trim().parse().map_err(|_| bad(name, s)))
            .transpose()
    }

    pub fn i32(&self, name: &str) -> Result<Option<i32>> {
        self.scalar(name)?
            .map(|s| s.trim().parse().map_err(|_| bad(name, s)))
            .transpose()
    }

    pub fn bool(&self, name: &str) -> Result<Option<bool>> {
        Ok(self.scalar(name)?.map(parse_bool))
    }

    /// Enum constant name of field `name` (`<Type xs.n=… v="CONST"/>`).
    pub fn enum_value(&self, name: &str) -> Result<Option<&'d str>> {
        Ok(self.field(name)?.and_then(|o| o.attr("v")))
    }

    /// `uuid` attribute of a GUID-valued field.
    pub fn guid(&self, name: &str) -> Result<Option<&'d str>> {
        Ok(self.field(name)?.and_then(|o| o.attr("uuid")))
    }

    /// `idstr` attribute of an ID-valued field.
    pub fn id(&self, name: &str) -> Result<Option<&'d str>> {
        Ok(self.field(name)?.and_then(|o| o.attr("idstr")))
    }

    /// Items of a list/set/array field, references resolved, nulls dropped (§2.4–2.7).
    pub fn list(&self, name: &str) -> Result<Vec<Obj<'g, 'd>>> {
        match self.field(name)? {
            None => Ok(Vec::new()),
            Some(l) => l.items(),
        }
    }

    /// Children of this collection element, references resolved, nulls dropped.
    pub fn items(&self) -> Result<Vec<Obj<'g, 'd>>> {
        let mut out = Vec::with_capacity(self.e.children.len());
        for c in &self.e.children {
            let o = self.g.obj(c)?;
            if !o.is_null() {
                out.push(o);
            }
        }
        Ok(out)
    }

    /// Text values of a list of boxed scalars (e.g. `array_list` of `<f>`).
    pub fn scalar_items(&self) -> impl Iterator<Item = &'d str> {
        self.e
            .children
            .iter()
            .filter(|c| c.name != "null")
            .map(|c| c.text.as_str())
    }

    pub fn f32_array(&self, name: &str) -> Result<Option<Vec<f32>>> {
        self.prim_array(name, parse_f32)
    }

    pub fn i32_array(&self, name: &str) -> Result<Option<Vec<i32>>> {
        self.prim_array(name, |s| s.parse().ok())
    }

    pub fn i64_array(&self, name: &str) -> Result<Option<Vec<i64>>> {
        self.prim_array(name, |s| s.parse().ok())
    }

    pub fn i16_array(&self, name: &str) -> Result<Option<Vec<i16>>> {
        self.prim_array(name, |s| s.parse().ok())
    }

    pub fn i8_array(&self, name: &str) -> Result<Option<Vec<i8>>> {
        self.prim_array(name, |s| s.parse().ok())
    }

    fn prim_array<T>(
        &self,
        name: &str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<Option<Vec<T>>> {
        let Some(a) = self.field(name)? else {
            return Ok(None);
        };
        let text = a.e.text.as_str();
        let mut out = Vec::new();
        for tok in text.split_ascii_whitespace() {
            out.push(parse(tok).ok_or_else(|| bad(name, tok))?);
        }
        if let Some(n) = a.attr("count").and_then(|c| c.parse::<usize>().ok()) {
            if n != out.len() {
                return Err(FormatError::Invalid(format!(
                    "array {name:?}: count={n} but {} values",
                    out.len()
                )));
            }
        }
        Ok(Some(out))
    }
}

fn bad(field: &str, value: &str) -> FormatError {
    let v: String = value.chars().take(40).collect();
    FormatError::Invalid(format!("field {field:?}: cannot parse {v:?}"))
}

/// Parses Java-style decimal text (§6), including `NaN` and `±Infinity`.
pub fn parse_f64(s: &str) -> Option<f64> {
    match s.trim() {
        "NaN" => Some(f64::NAN),
        "Infinity" | "+Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        t => t.parse().ok().filter(|v: &f64| v.is_finite()),
    }
}

pub fn parse_f32(s: &str) -> Option<f32> {
    match s.trim() {
        "NaN" => Some(f32::NAN),
        "Infinity" | "+Infinity" => Some(f32::INFINITY),
        "-Infinity" => Some(f32::NEG_INFINITY),
        t => t.parse().ok().filter(|v: &f32| v.is_finite()),
    }
}

/// `true` (case-insensitive) is true, anything else false (§6).
pub fn parse_bool(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("true")
}

/// Formats like Java's `Float.toString`: shortest round-trip digits, at least
/// one fractional digit, scientific (`1.0E-5`) outside [1e-3, 1e7). §6
pub fn format_f32(v: f32) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    java_layout(
        &format!("{v:e}"),
        v == 0.0,
        v.abs() >= 1e-3 && v.abs() < 1e7,
    )
}

pub fn format_f64(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    java_layout(
        &format!("{v:e}"),
        v == 0.0,
        v.abs() >= 1e-3 && v.abs() < 1e7,
    )
}

/// `sci` is Rust's shortest `{:e}` form (`-1.2345e-5`).
fn java_layout(sci: &str, zero: bool, plain: bool) -> String {
    let (neg, rest) = match sci.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, sci),
    };
    let sign = if neg { "-" } else { "" };
    if zero {
        return format!("{sign}0.0");
    }
    let (mant, exp) = rest.split_once('e').unwrap_or((rest, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    if plain {
        let point = exp + 1; // digits before the decimal point
        if point <= 0 {
            let zeros = "0".repeat((-point) as usize);
            format!("{sign}0.{zeros}{digits}")
        } else {
            let p = point as usize;
            if digits.len() > p {
                format!("{sign}{}.{}", &digits[..p], &digits[p..])
            } else {
                format!("{sign}{digits}{}.0", "0".repeat(p - digits.len()))
            }
        }
    } else {
        let frac = if digits.len() > 1 { &digits[1..] } else { "0" };
        format!("{sign}{}.{frac}E{exp}", &digits[..1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_float_strings() {
        for (v, s) in [
            (1.0f32, "1.0"),
            (0.1, "0.1"),
            (-0.5, "-0.5"),
            (1.0e-5, "1.0E-5"),
            (1.5e-5, "1.5E-5"),
            (0.001, "0.001"),
            (1.0e7, "1.0E7"),
            (1234567.0, "1234567.0"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (638.0523, "638.0523"),
            (0.068643436, "0.068643436"),
            (2000.0, "2000.0"),
        ] {
            assert_eq!(format_f32(v), s, "{v:e}");
            assert_eq!(parse_f32(s), Some(v));
        }
        assert_eq!(format_f32(f32::NAN), "NaN");
        assert_eq!(format_f32(f32::NEG_INFINITY), "-Infinity");
        assert_eq!(format_f64(0.1), "0.1");
        assert_eq!(format_f64(1e-10), "1.0E-10");
    }

    #[test]
    fn refs_and_fields() {
        let doc = XmlDocument::parse(
            br##"<root><shared><CGuid uuid="u1" xs.id="#0" xs.idx="0" /></shared><main>
            <CThing a="2.5"><CBase xs.n="super"><i xs.n="n">7</i><CGuid xs.n="g" xs.ref="#0" /></CBase>
            <null xs.n="nothing" /><carray_list xs.n="items" count="2"><CGuid xs.ref="#0" /><null /></carray_list>
            <float-array xs.n="p" count="2">1.0 2.0</float-array><CGuid xs.n="bad" xs.ref="#9" />
            </CThing></main></root>"##,
        )
        .unwrap();
        let g = ObjectGraph::new(&doc).unwrap();
        let m = g.main().unwrap();
        assert_eq!(m.f32("a").unwrap(), Some(2.5));
        assert_eq!(m.i32("n").unwrap(), Some(7));
        assert_eq!(m.guid("g").unwrap(), Some("u1"));
        assert!(m.field("nothing").unwrap().is_none());
        assert_eq!(m.list("items").unwrap().len(), 1);
        assert_eq!(m.f32_array("p").unwrap(), Some(vec![1.0, 2.0]));
        assert!(m.field("bad").is_err());
    }
}
