//! Lossless XML document model for `main.xml` (and other object-graph XML).
//!
//! The reference writes one element per line, no indentation, scalar text
//! inline, empty elements as `<tag a="v" />`, CRLF line ends, and a blank line
//! after `</root>`. [`XmlDocument::write`] reproduces exactly that layout, so a
//! document read from a reference file is written back byte for byte.
//! spec: 10-formats/xml-object-graph.md §1–2

use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{FormatError, Result};

/// Nesting deeper than this is rejected (protects recursive consumers).
pub const MAX_DEPTH: usize = 1000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub name: String,
    /// Attributes in document order (order matters for byte-exact output).
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    /// Text content. Only meaningful for elements without children;
    /// whitespace between child elements is not kept.
    pub text: String,
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.attrs.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => self.attrs.push((key.to_owned(), value)),
        }
    }

    pub fn with_attr(mut self, key: &str, value: impl Into<String>) -> Self {
        self.set_attr(key, value);
        self
    }

    /// The field name under which this value is stored in its parent (`xs.n`).
    pub fn field_name(&self) -> Option<&str> {
        self.attr("xs.n")
    }

    /// First direct child stored under field name `name` (no ref resolution).
    pub fn child_field(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.field_name() == Some(name))
    }

    pub fn child_field_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.children
            .iter_mut()
            .find(|c| c.field_name() == Some(name))
    }
}

/// A processing instruction before the root element, e.g. `<?version CModelSource:15?>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pi {
    pub target: String,
    pub data: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct XmlDocument {
    /// Contents of the `<?xml …?>` declaration.
    pub declaration: String,
    /// Processing instructions in document order.
    pub pis: Vec<Pi>,
    pub root: Element,
}

impl XmlDocument {
    pub fn new(root: Element) -> Self {
        Self {
            declaration: r#"xml version="1.0" encoding="UTF-8""#.to_owned(),
            pis: Vec::new(),
            root,
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let xerr = |e: quick_xml::Error| FormatError::Xml(e.to_string());
        let mut reader = Reader::from_reader(data);
        reader.config_mut().trim_text(false);
        reader.config_mut().check_end_names = true;

        let mut declaration = None;
        let mut pis = Vec::new();
        let mut stack: Vec<Element> = Vec::new();
        let mut root = None;
        let mut buf = Vec::new();

        fn start(e: &quick_xml::events::BytesStart) -> Result<Element> {
            let name = std::str::from_utf8(e.name().as_ref())
                .map_err(|_| FormatError::Xml("element name is not UTF-8".into()))?
                .to_owned();
            let mut el = Element::new(name);
            for a in e.attributes() {
                let a = a.map_err(|e| FormatError::Xml(e.to_string()))?;
                let k = std::str::from_utf8(a.key.as_ref())
                    .map_err(|_| FormatError::Xml("attribute name is not UTF-8".into()))?
                    .to_owned();
                // Attribute-value normalisation: literal whitespace → space.
                let raw = std::str::from_utf8(&a.value)
                    .map_err(|_| FormatError::Xml("attribute value is not UTF-8".into()))?;
                let raw = normalize_line_ends(raw).replace(['\n', '\t'], " ");
                let v = unescape(&raw).map_err(|e| FormatError::Xml(e.to_string()))?;
                el.attrs.push((k, v.into_owned()));
            }
            Ok(el)
        }

        loop {
            match reader.read_event_into(&mut buf).map_err(xerr)? {
                Event::Decl(d) => {
                    declaration = Some(String::from_utf8_lossy(&d).trim().to_owned());
                }
                Event::PI(p) => {
                    if stack.is_empty() && root.is_none() {
                        let target = String::from_utf8_lossy(p.target()).into_owned();
                        let data = String::from_utf8_lossy(p.content()).trim().to_owned();
                        pis.push(Pi { target, data });
                    }
                }
                Event::Start(e) => {
                    if stack.len() >= MAX_DEPTH {
                        return Err(FormatError::LimitExceeded("XML nesting depth".into()));
                    }
                    if stack.is_empty() && root.is_some() {
                        return Err(FormatError::Xml("more than one root element".into()));
                    }
                    stack.push(start(&e)?);
                }
                Event::Empty(e) => {
                    let el = start(&e)?;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(el),
                        None if root.is_none() => root = Some(el),
                        None => return Err(FormatError::Xml("more than one root element".into())),
                    }
                }
                Event::End(_) => {
                    let mut el = stack
                        .pop()
                        .ok_or_else(|| FormatError::Xml("unbalanced end tag".into()))?;
                    if !el.children.is_empty() {
                        el.text.clear();
                    }
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(el),
                        None => root = Some(el),
                    }
                }
                Event::Text(t) => {
                    if let Some(top) = stack.last_mut() {
                        let raw = std::str::from_utf8(&t)
                            .map_err(|_| FormatError::Xml("text is not UTF-8".into()))?;
                        let norm = normalize_line_ends(raw);
                        let s = unescape(&norm).map_err(|e| FormatError::Xml(e.to_string()))?;
                        top.text.push_str(&s);
                    }
                }
                Event::CData(t) => {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&String::from_utf8_lossy(&t));
                    }
                }
                Event::Comment(_) | Event::DocType(_) => {}
                Event::Eof => break,
            }
            buf.clear();
        }
        if !stack.is_empty() {
            return Err(FormatError::Xml("unexpected end of document".into()));
        }
        let root = root.ok_or_else(|| FormatError::Xml("no root element".into()))?;
        Ok(Self {
            declaration: declaration
                .unwrap_or_else(|| r#"xml version="1.0" encoding="UTF-8""#.into()),
            pis,
            root,
        })
    }

    /// Serializes with the reference layout (spec: xml-object-graph.md §6.1).
    pub fn write(&self) -> Vec<u8> {
        let nl = NL;
        let mut out = Vec::with_capacity(1 << 20);
        out.extend_from_slice(b"<?");
        out.extend_from_slice(self.declaration.as_bytes());
        out.extend_from_slice(b"?>");
        out.extend_from_slice(nl);
        for pi in &self.pis {
            out.extend_from_slice(b"<?");
            out.extend_from_slice(pi.target.as_bytes());
            if !pi.data.is_empty() {
                out.push(b' ');
                out.extend_from_slice(pi.data.as_bytes());
            }
            out.extend_from_slice(b"?>");
            out.extend_from_slice(nl);
        }
        write_element(&mut out, &self.root, nl);
        out.extend_from_slice(nl);
        out
    }

    /// Values of `<?version Key:N?>` PIs.
    pub fn versions(&self) -> impl Iterator<Item = (&str, u32)> {
        self.pis
            .iter()
            .filter(|p| p.target == "version")
            .filter_map(|p| {
                let (k, v) = p.data.rsplit_once(':')?;
                Some((k.trim(), v.trim().parse().ok()?))
            })
    }

    pub fn version_of(&self, key: &str) -> Option<u32> {
        self.versions().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}

fn write_element(out: &mut Vec<u8>, root: &Element, nl: &[u8]) {
    // Iterative to keep stack use flat for deep documents.
    enum Step<'a> {
        Open(&'a Element),
        Close(&'a Element),
    }
    let mut work = vec![Step::Open(root)];
    while let Some(step) = work.pop() {
        match step {
            Step::Open(e) => {
                out.push(b'<');
                out.extend_from_slice(e.name.as_bytes());
                for (k, v) in &e.attrs {
                    out.push(b' ');
                    out.extend_from_slice(k.as_bytes());
                    out.extend_from_slice(b"=\"");
                    escape_into(out, v, true);
                    out.push(b'"');
                }
                if e.children.is_empty() {
                    if e.text.is_empty() {
                        out.extend_from_slice(b" />");
                    } else {
                        out.push(b'>');
                        escape_into(out, &e.text, false);
                        out.extend_from_slice(b"</");
                        out.extend_from_slice(e.name.as_bytes());
                        out.push(b'>');
                    }
                    out.extend_from_slice(nl);
                } else {
                    out.push(b'>');
                    out.extend_from_slice(nl);
                    work.push(Step::Close(e));
                    work.extend(e.children.iter().rev().map(Step::Open));
                }
            }
            Step::Close(e) => {
                out.extend_from_slice(b"</");
                out.extend_from_slice(e.name.as_bytes());
                out.push(b'>');
                out.extend_from_slice(nl);
            }
        }
    }
    // The last element's newline is part of the layout; the document adds one more.
}

/// Line end written everywhere (CRLF on every platform, §6.1).
const NL: &[u8] = b"\r\n";

/// XML 1.0 line-end handling: CRLF and lone CR read as LF.
fn normalize_line_ends(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\r') {
        std::borrow::Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Characters legal in XML 1.0. Others can't be written (the reference's
/// save fails on them); Keyform drops them.
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// Escaping per spec xml-object-graph.md §6.1. Text is trimmed of XML
/// whitespace; LF in text is written as the line separator.
fn escape_into(out: &mut Vec<u8>, s: &str, attr: bool) {
    let s = if attr {
        s
    } else {
        s.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
    };
    let mut buf = [0u8; 4];
    for c in s.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '"' if attr => out.extend_from_slice(b"&quot;"),
            '\t' if attr => out.extend_from_slice(b"&#x9;"),
            '\n' if attr => out.extend_from_slice(b"&#xA;"),
            '\n' => out.extend_from_slice(NL),
            '\r' => out.extend_from_slice(b"&#xD;"),
            c if is_xml_char(c) => out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
        <?version CModelSource:15?>\r\n\
        <?import com.example.CFoo?>\r\n\
        <root fileFormatVersion=\"503040001\">\r\n\
        <shared>\r\n\
        <CFoo xs.id=\"#0\" xs.idx=\"3\" a=\"x &amp; &lt;y&gt; &quot;z&quot;\" />\r\n\
        </shared>\r\n\
        <main>\r\n\
        <CBar>\r\n\
        <s xs.n=\"name\">角度 X &amp; more</s>\r\n\
        <s xs.n=\"memo\" />\r\n\
        <float-array xs.n=\"p\" count=\"2\">1.0 -0.5</float-array>\r\n\
        <CFoo xs.n=\"foo\" xs.ref=\"#0\" />\r\n\
        </CBar>\r\n\
        </main>\r\n\
        </root>\r\n\r\n";

    #[test]
    fn byte_exact_roundtrip() {
        let d = XmlDocument::parse(DOC.as_bytes()).unwrap();
        assert_eq!(d.version_of("CModelSource"), Some(15));
        let main = &d.root.children[1].children[0];
        assert_eq!(main.child_field("name").unwrap().text, "角度 X & more");
        assert_eq!(
            d.root.children[0].children[0].attr("a"),
            Some("x & <y> \"z\"")
        );
        assert_eq!(String::from_utf8(d.write()).unwrap(), DOC);
    }

    #[test]
    fn escaping_follows_reference_layout() {
        // spec: xml-object-graph.md §6.1
        let mut e = Element::new("CFoo");
        e.set_attr("a", "x\ty\nz\r'\"<&>");
        let mut t = Element::new("s");
        t.text = "  a\r\nb <&> \u{1}'\"\t\n".into();
        e.children.push(t);
        let doc = XmlDocument::new(e);
        let out = String::from_utf8(doc.write()).unwrap();
        assert!(
            out.contains("a=\"x&#x9;y&#xA;z&#xD;'&quot;&lt;&amp;&gt;\""),
            "{out}"
        );
        assert!(
            out.contains("<s>a&#xD;\r\nb &lt;&amp;&gt; '\"</s>\r\n"),
            "{out}"
        );
        // Reading back restores the values (text was trimmed; LF via CRLF).
        let back = XmlDocument::parse(out.as_bytes()).unwrap();
        assert_eq!(back.root.attr("a"), Some("x\ty\nz\r'\"<&>"));
        assert_eq!(back.root.children[0].text, "a\r\nb <&> '\"");
        // LF-only input is accepted and written with CRLF.
        let lf = DOC.replace("\r\n", "\n");
        let d = XmlDocument::parse(lf.as_bytes()).unwrap();
        assert_eq!(String::from_utf8(d.write()).unwrap(), DOC);
    }

    #[test]
    fn malformed_is_error() {
        for bad in [
            "",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "<a>&bogus;</a>",
            "<a x=\"1></a>",
        ] {
            assert!(XmlDocument::parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
        let deep = "<a>".repeat(MAX_DEPTH + 1);
        assert!(XmlDocument::parse(deep.as_bytes()).is_err());
    }
}
