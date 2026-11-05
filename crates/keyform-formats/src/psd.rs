//! Photoshop `.psd` reader for PSD import: the layer tree and per-layer
//! RGBA pixels. Only what the import uses is read (spec: 70-advanced/
//! psd-import.md §3); the file format follows Adobe's public "Photoshop
//! File Formats Specification". Input is untrusted: every size is checked
//! and layer pixels are decoded one layer at a time, on request.

use crate::error::{FormatError, Result};
use crate::image::RgbaImage;

/// Largest document side accepted (the PSD format's own limit).
const MAX_SIDE: u32 = 30_000;
/// Largest layer area decoded (pixels).
const MAX_LAYER_PIXELS: u64 = 400_000_000;

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(FormatError::Invalid(format!("psd: {}", msg.into())))
}

/// Big-endian cursor over a byte slice.
struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8], pos: usize) -> Self {
        Self { b, pos }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.b.len());
        let Some(end) = end else {
            return Err(FormatError::UnexpectedEof {
                offset: self.pos,
                needed: n,
            });
        };
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    fn tag(&mut self) -> Result<[u8; 4]> {
        let s = self.take(4)?;
        Ok([s[0], s[1], s[2], s[3]])
    }
}

/// A layer rectangle in document pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn read(r: &mut Rd) -> Result<Self> {
        let (top, left, bottom, right) = (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
        let w = i64::from(right) - i64::from(left);
        let h = i64::from(bottom) - i64::from(top);
        if !(0..=i64::from(MAX_SIDE)).contains(&w) || !(0..=i64::from(MAX_SIDE)).contains(&h) {
            return invalid("layer rectangle out of range");
        }
        Ok(Self {
            left,
            top,
            width: w as u32,
            height: h as u32,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// One channel of a layer record and where its data is in the file.
#[derive(Clone, Debug)]
struct Channel {
    id: i16,
    /// Compression word followed by the data.
    data: std::ops::Range<usize>,
}

/// A pixel layer. spec: psd-import.md §3 step 3
#[derive(Clone, Debug)]
pub struct PsdLayer {
    pub name: String,
    /// `lyid` as four upper-case hex bytes joined by `-`.
    pub id: Option<String>,
    pub bounds: Rect,
    pub opacity: u8,
    pub visible: bool,
    pub clipping: bool,
    /// Blend mode key (`norm`, `mul `, `lddg`, …).
    pub blend: [u8; 4],
    /// Transparency shapes layer (`tsly`), true when absent.
    pub transparency_shapes: bool,
    channels: Vec<Channel>,
    mask: Option<Rect>,
}

/// A layer group (a folder in the Layers panel).
#[derive(Clone, Debug)]
pub struct PsdGroup {
    pub name: String,
    pub id: Option<String>,
    pub visible: bool,
    pub opacity: u8,
    pub blend: [u8; 4],
    pub children: Vec<PsdNode>,
}

#[derive(Clone, Debug)]
pub enum PsdNode {
    Layer(PsdLayer),
    Group(PsdGroup),
}

/// A parsed PSD: document size and layer tree (top to bottom), with the
/// file bytes for decoding layer pixels.
pub struct Psd<'a> {
    pub width: u32,
    pub height: u32,
    pub nodes: Vec<PsdNode>,
    data: &'a [u8],
}

/// What a layer record is in the Layers panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Pixel,
    /// A folder (opens a group, walking top to bottom).
    Open,
    /// A bounding section divider (closes the group).
    Close,
}

impl<'a> Psd<'a> {
    /// Parses the header and the layer records (no pixels yet).
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let mut r = Rd::new(data, 0);
        if r.take(4)? != b"8BPS" {
            return Err(FormatError::BadMagic { expected: "8BPS" });
        }
        match r.u16()? {
            1 => {}
            2 => {
                return Err(FormatError::Unsupported(
                    "PSB File loading is not supported.".into(),
                ))
            }
            v => return invalid(format!("version {v}")),
        }
        r.take(6)?;
        let _channels = r.u16()?;
        let height = r.u32()?;
        let width = r.u32()?;
        let depth = r.u16()?;
        let mode = r.u16()?;
        if depth != 8 {
            return Err(FormatError::Unsupported(
                "Failed to read PSD. Not compatible with non-8bit PSDs.".into(),
            ));
        }
        if mode != 3 {
            return Err(FormatError::Unsupported(
                "Failed to read PSD. Only RGB documents are supported.".into(),
            ));
        }
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return invalid("document size out of range");
        }
        // Colour mode data and image resources are skipped.
        let n = r.u32()? as usize;
        r.take(n)?;
        let n = r.u32()? as usize;
        r.take(n)?;
        // Layer and mask information.
        let lm_len = r.u32()? as usize;
        let lm_end = r.pos.checked_add(lm_len).filter(|&e| e <= data.len());
        let Some(_lm_end) = lm_end else {
            return invalid("layer and mask section past the end");
        };
        let mut records = Vec::new();
        if lm_len >= 4 {
            let li_len = r.u32()? as usize;
            let li_end = r
                .pos
                .checked_add(li_len)
                .filter(|&e| e <= data.len())
                .ok_or_else(|| FormatError::Invalid("psd: layer info past the end".into()))?;
            if li_len >= 2 {
                let count = (r.u16()? as i16).unsigned_abs() as usize;
                for _ in 0..count {
                    records.push(read_record(&mut r)?);
                }
                // Channel image data follows the records, in record order
                // (groups and dividers have channels too).
                for (_, rec) in &mut records {
                    for c in &mut rec.channels {
                        let len = c.data.end;
                        let start = r.pos;
                        r.take(len)?;
                        c.data = start..start + len;
                    }
                }
            }
            let _ = li_end;
        }
        let nodes = build_tree(records);
        Ok(Self {
            width,
            height,
            nodes,
            data,
        })
    }

    /// Every pixel layer, depth-first in Layers-panel order.
    pub fn layers(&self) -> Vec<&PsdLayer> {
        fn walk<'b>(nodes: &'b [PsdNode], out: &mut Vec<&'b PsdLayer>) {
            for n in nodes {
                match n {
                    PsdNode::Layer(l) => out.push(l),
                    PsdNode::Group(g) => walk(&g.children, out),
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.nodes, &mut out);
        out
    }

    /// The layer's straight-alpha RGBA pixels (its bounds' size), with the
    /// user mask applied. spec: psd-import.md §3 step 4
    pub fn pixels(&self, l: &PsdLayer) -> Result<RgbaImage> {
        let (w, h) = (l.bounds.width as usize, l.bounds.height as usize);
        if (w as u64) * (h as u64) > MAX_LAYER_PIXELS {
            return Err(FormatError::LimitExceeded("psd layer too large".into()));
        }
        if w == 0 || h == 0 {
            return Ok(RgbaImage::new(l.bounds.width, l.bounds.height));
        }
        // Before allocating: the colour channels' data must be able to hold
        // the pixels (raw: 1 byte each; PackBits: at most 128 per byte), so
        // a corrupt rectangle can't make us allocate far beyond the file.
        let n = (w as u64) * (h as u64);
        for c in l.channels.iter().filter(|c| (0..=2).contains(&c.id)) {
            let d = &self.data[c.data.clone()];
            let body = d.len().saturating_sub(2) as u64;
            let ok = match d.get(..2) {
                Some([0, 0]) => body >= n,
                Some([0, 1]) => body.saturating_mul(128) >= n,
                _ => true,
            };
            if !ok {
                return invalid(format!("layer {:?}: channel data too short", l.name));
            }
        }
        let mut img = RgbaImage::new(l.bounds.width, l.bounds.height);
        let has = |id: i16| l.channels.iter().any(|c| c.id == id);
        if !(has(0) && has(1) && has(2)) {
            return Err(FormatError::Unsupported(format!(
                "psd layer {:?}: not an RGB layer",
                l.name
            )));
        }
        let alpha = has(-1);
        if !alpha {
            for p in img.data.chunks_exact_mut(4) {
                p[3] = 255;
            }
        }
        for c in &l.channels {
            let (slot, rect) = match c.id {
                0 => (0, l.bounds),
                1 => (1, l.bounds),
                2 => (2, l.bounds),
                -1 => (3, l.bounds),
                -2 => {
                    // The user mask multiplies alpha; outside its rectangle
                    // the mask is 0.
                    let Some(m) = l.mask else { continue };
                    let plane = decode_channel(&self.data[c.data.clone()], m)?;
                    let mut mask = vec![0u8; w * h];
                    for y in 0..h {
                        let dy = i64::from(l.bounds.top) + y as i64 - i64::from(m.top);
                        if dy < 0 || dy >= i64::from(m.height) {
                            continue;
                        }
                        for x in 0..w {
                            let dx = i64::from(l.bounds.left) + x as i64 - i64::from(m.left);
                            if dx >= 0 && dx < i64::from(m.width) {
                                mask[y * w + x] =
                                    plane[dy as usize * m.width as usize + dx as usize];
                            }
                        }
                    }
                    for (p, &mv) in img.data.chunks_exact_mut(4).zip(&mask) {
                        p[3] = (u16::from(p[3]) * u16::from(mv) / 255) as u8;
                    }
                    continue;
                }
                _ => continue,
            };
            let plane = decode_channel(&self.data[c.data.clone()], rect)?;
            for (p, &v) in img.data.chunks_exact_mut(4).zip(&plane) {
                p[slot] = v;
            }
        }
        Ok(img)
    }
}

/// Decodes one channel (compression word + data) of `rect`'s size.
fn decode_channel(d: &[u8], rect: Rect) -> Result<Vec<u8>> {
    let (w, h) = (rect.width as usize, rect.height as usize);
    let n = w * h;
    if d.len() < 2 {
        return Ok(vec![0; n]);
    }
    let comp = u16::from_be_bytes([d[0], d[1]]);
    let body = &d[2..];
    match comp {
        0 => {
            let mut out = body[..body.len().min(n)].to_vec();
            out.resize(n, 0);
            Ok(out)
        }
        1 => {
            // PackBits: row byte counts (u16 each), then the rows.
            if body.len() < 2 * h {
                return invalid("RLE row table truncated");
            }
            let mut pos = 2 * h;
            let mut out = vec![0u8; n];
            for y in 0..h {
                let len = usize::from(u16::from_be_bytes([body[2 * y], body[2 * y + 1]]));
                let end = (pos + len).min(body.len());
                unpack_bits(&body[pos..end], &mut out[y * w..(y + 1) * w]);
                pos = end;
            }
            Ok(out)
        }
        _ => Err(FormatError::Unsupported(
            "psd: ZIP-compressed layer channels".into(),
        )),
    }
}

/// PackBits into `out` (stops when either side runs out).
fn unpack_bits(src: &[u8], out: &mut [u8]) {
    let (mut i, mut o) = (0, 0);
    while i < src.len() && o < out.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let k = (n as usize + 1).min(src.len() - i).min(out.len() - o);
            out[o..o + k].copy_from_slice(&src[i..i + k]);
            i += n as usize + 1;
            o += k;
        } else if n != -128 {
            let k = (1 - n as isize) as usize;
            let Some(&v) = src.get(i) else { break };
            i += 1;
            let k = k.min(out.len() - o);
            out[o..o + k].fill(v);
            o += k;
        }
    }
}

fn read_pascal(r: &mut Rd) -> Result<String> {
    let n = usize::from(r.u8()?);
    let s = r.take(n)?;
    // Padded to a multiple of 4 including the length byte.
    let pad = (4 - (n + 1) % 4) % 4;
    r.take(pad.min(r.b.len() - r.pos))?;
    Ok(s.iter().map(|&c| char::from(c)).collect())
}

fn read_record(r: &mut Rd) -> Result<(Kind, PsdLayer)> {
    let bounds = Rect::read(r)?;
    let nch = usize::from(r.u16()?);
    if nch > 56 {
        return invalid("too many channels");
    }
    let mut channels = Vec::with_capacity(nch);
    for _ in 0..nch {
        let id = r.u16()? as i16;
        let len = r.u32()? as usize;
        // The range is filled in later; keep the length in `end`.
        channels.push(Channel { id, data: 0..len });
    }
    if r.take(4)? != b"8BIM" {
        return invalid("layer blend signature");
    }
    let blend = r.tag()?;
    let opacity = r.u8()?;
    let clipping = r.u8()? != 0;
    let flags = r.u8()?;
    r.u8()?;
    let extra = r.u32()? as usize;
    let extra_end = r
        .pos
        .checked_add(extra)
        .filter(|&e| e <= r.b.len())
        .ok_or_else(|| FormatError::Invalid("psd: layer extra data past the end".into()))?;
    // Layer mask data: the first rectangle is the user mask's.
    let mask_len = r.u32()? as usize;
    let mut mask = None;
    if mask_len >= 18 {
        let m = Rect::read(r)?;
        mask = (!m.is_empty()).then_some(m);
        r.take(mask_len - 16)?;
    } else {
        r.take(mask_len)?;
    }
    let ranges = r.u32()? as usize;
    r.take(ranges)?;
    let mut name = read_pascal(r)?;
    let (mut id, mut section, mut tsl) = (None, None, true);
    // Additional layer information blocks.
    while r.pos + 12 <= extra_end {
        let sig = r.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            // Resynchronise on writers that pad blocks oddly.
            r.pos -= 3;
            continue;
        }
        let key = r.tag()?;
        let len = r.u32()? as usize;
        let start = r.pos;
        let end = start.checked_add(len).filter(|&e| e <= extra_end);
        let Some(end) = end else { break };
        let mut b = Rd::new(&r.b[..end], start);
        match &key {
            b"luni" => {
                let n = b.u32()? as usize;
                let n = n.min(len.saturating_sub(4) / 2);
                let units: Vec<u16> = (0..n).map(|_| b.u16()).collect::<Result<_>>()?;
                let s = String::from_utf16_lossy(&units);
                name = s.trim_end_matches('\0').to_owned();
            }
            b"lyid" => {
                let v = b.u32()?.to_be_bytes();
                id = Some(format!(
                    "{:02X}-{:02X}-{:02X}-{:02X}",
                    v[0], v[1], v[2], v[3]
                ));
            }
            b"lsct" | b"lsdk" => {
                let t = b.u32()?;
                // lsdk (nested sections) opens a group whatever its type.
                section = Some(if &key == b"lsdk" && t != 3 { 1 } else { t });
            }
            b"tsly" => tsl = b.u8()? != 0,
            _ => {}
        }
        r.pos = end;
    }
    r.pos = extra_end;
    // Bit 1 set means hidden.
    let visible = flags & 0x02 == 0;
    let kind = match section {
        Some(3) => Kind::Close,
        Some(1 | 2) => Kind::Open,
        None if name == "</Layer group>" || name == "</Layer set>" => Kind::Close,
        _ => Kind::Pixel,
    };
    Ok((
        kind,
        PsdLayer {
            name,
            id,
            bounds,
            opacity,
            visible,
            clipping,
            blend,
            transparency_shapes: tsl,
            channels,
            mask,
        },
    ))
}

/// Builds the tree from file-order records (bottom first): walking them in
/// reverse, a folder record opens a group and a divider closes it. Empty
/// pixel layers are skipped. spec: psd-import.md §3 step 2
fn build_tree(records: Vec<(Kind, PsdLayer)>) -> Vec<PsdNode> {
    fn push(stack: &mut [PsdGroup], top: &mut Vec<PsdNode>, n: PsdNode) {
        match stack.last_mut() {
            Some(parent) => parent.children.push(n),
            None => top.push(n),
        }
    }
    let mut stack: Vec<PsdGroup> = Vec::new();
    let mut top: Vec<PsdNode> = Vec::new();
    for (kind, l) in records.into_iter().rev() {
        match kind {
            Kind::Open => stack.push(PsdGroup {
                name: l.name,
                id: l.id,
                visible: l.visible,
                opacity: l.opacity,
                blend: l.blend,
                children: Vec::new(),
            }),
            Kind::Close => {
                if let Some(done) = stack.pop() {
                    push(&mut stack, &mut top, PsdNode::Group(done));
                }
            }
            Kind::Pixel if !l.bounds.is_empty() => {
                push(&mut stack, &mut top, PsdNode::Layer(l));
            }
            Kind::Pixel => {}
        }
    }
    // Unclosed groups (malformed files) are closed at the end.
    while let Some(done) = stack.pop() {
        push(&mut stack, &mut top, PsdNode::Group(done));
    }
    top
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn packbits() {
        let mut out = [0u8; 8];
        // 3 literal bytes, then 5 repeats of 9.
        unpack_bits(&[2, 1, 2, 3, 0xFC, 9], &mut out);
        assert_eq!(out, [1, 2, 3, 9, 9, 9, 9, 9]);
        // Truncated input stops without panicking.
        let mut out = [7u8; 4];
        unpack_bits(&[5, 1], &mut out);
        assert_eq!(out, [1, 7, 7, 7]);
    }

    /// A minimal RGB PSD with one 2×2 RLE layer and one raw layer.
    fn tiny() -> Vec<u8> {
        let mut d = b"8BPS".to_vec();
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&[0; 6]);
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&4u32.to_be_bytes());
        d.extend_from_slice(&4u32.to_be_bytes());
        d.extend_from_slice(&8u16.to_be_bytes());
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        let mut li = Vec::new();
        li.extend_from_slice(&2i16.to_be_bytes());
        let mut chans = Vec::new();
        for raw in [false, true] {
            for v in [0i32, 0, 2, 2] {
                li.extend_from_slice(&v.to_be_bytes());
            }
            li.extend_from_slice(&4u16.to_be_bytes());
            for id in [0i16, 1, 2, -1] {
                let data: Vec<u8> = if raw {
                    let mut c = 0u16.to_be_bytes().to_vec();
                    c.extend_from_slice(&[200; 4]);
                    c
                } else {
                    // RLE: two rows of "2 × value".
                    let mut c = 1u16.to_be_bytes().to_vec();
                    c.extend_from_slice(&2u16.to_be_bytes());
                    c.extend_from_slice(&2u16.to_be_bytes());
                    c.extend_from_slice(&[0xFF, 77, 0xFF, 77]);
                    c
                };
                li.extend_from_slice(&id.to_be_bytes());
                li.extend_from_slice(&(data.len() as u32).to_be_bytes());
                chans.push(data);
            }
            li.extend_from_slice(b"8BIMnorm");
            li.extend_from_slice(&[255, 0, 0, 0]);
            let name = b"\x01L\0\0";
            li.extend_from_slice(&(8 + name.len() as u32).to_be_bytes());
            li.extend_from_slice(&0u32.to_be_bytes());
            li.extend_from_slice(&0u32.to_be_bytes());
            li.extend_from_slice(name);
        }
        for c in chans {
            li.extend_from_slice(&c);
        }
        let mut lm = (li.len() as u32).to_be_bytes().to_vec();
        lm.extend_from_slice(&li);
        d.extend_from_slice(&(lm.len() as u32).to_be_bytes());
        d.extend_from_slice(&lm);
        d
    }

    #[test]
    fn tiny_psd_and_corruptions() {
        let d = tiny();
        let psd = Psd::parse(&d).unwrap();
        let layers = psd.layers();
        assert_eq!(layers.len(), 2);
        assert_eq!(psd.pixels(layers[1]).unwrap().data[..4], [77, 77, 77, 77]);
        assert_eq!(
            psd.pixels(layers[0]).unwrap().data[..4],
            [200, 200, 200, 200]
        );
        // Truncations and byte flips never panic.
        for n in 0..d.len() {
            if let Ok(p) = Psd::parse(&d[..n]) {
                for l in p.layers() {
                    let _ = p.pixels(l);
                }
            }
        }
        for i in 0..d.len() {
            let mut e = d.clone();
            e[i] ^= 0xA5;
            if let Ok(p) = Psd::parse(&e) {
                for l in p.layers() {
                    let _ = p.pixels(l);
                }
            }
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(Psd::parse(b"").is_err());
        assert!(Psd::parse(b"8BPS\x00\x02").is_err());
        let mut h = b"8BPS\x00\x01\0\0\0\0\0\0\x00\x03".to_vec();
        h.extend_from_slice(&10u32.to_be_bytes());
        h.extend_from_slice(&10u32.to_be_bytes());
        h.extend_from_slice(&16u16.to_be_bytes());
        h.extend_from_slice(&3u16.to_be_bytes());
        assert!(matches!(Psd::parse(&h), Err(FormatError::Unsupported(_))));
    }
}
