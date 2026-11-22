//! A PSD writer (8-bit RGB with alpha): layers with blend modes, opacity,
//! visibility, layer masks and one tree of groups (section dividers), plus
//! the merged image. Used by File ▸ Export Image/Video ▸ PSD images (beta)
//! (70-advanced/psd-export.md). Channels are PackBits-compressed; the file
//! carries a ResolutionInfo resource (72 dpi), which strict readers expect.

/// A layer's pixels: straight-alpha RGBA, `width` × `height`, placed at
/// (`left`, `top`) in the document.
#[derive(Clone, Debug, Default)]
pub struct PsdLayer {
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub opacity: u8,
    /// Blend mode key (`norm`, `mul `, `lddg`, …).
    pub blend: [u8; 4],
    pub visible: bool,
    /// The "transparency shapes layer" flag (`tsly`).
    pub transparency_shapes: bool,
    /// A layer mask: (left, top, width, height, gray pixels).
    pub mask: Option<(i32, i32, u32, u32, Vec<u8>)>,
}

/// A layer or a group of nodes (first = top-most).
#[derive(Clone, Debug)]
pub enum PsdNode {
    Layer(PsdLayer),
    Group {
        name: String,
        visible: bool,
        children: Vec<PsdNode>,
    },
}

/// PackBits (the PSD RLE) of one row.
fn packbits(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        // A run of equal bytes.
        let mut j = i + 1;
        while j < row.len() && row[j] == row[i] && j - i < 128 {
            j += 1;
        }
        if j - i >= 2 {
            out.push((257 - (j - i)) as u8);
            out.push(row[i]);
            i = j;
            continue;
        }
        // Literal bytes up to the next run of 2 or more.
        let start = i;
        while i < row.len() && i - start < 128 {
            if i + 1 < row.len() && row[i] == row[i + 1] {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&row[start..i]);
    }
    out
}

/// One channel, RLE: compression 1, row byte counts, then the rows.
fn rle_channel(plane: &[u8], width: usize, height: usize) -> Vec<u8> {
    let rows: Vec<Vec<u8>> = (0..height)
        .map(|y| packbits(&plane[y * width..(y + 1) * width]))
        .collect();
    let mut out = 1u16.to_be_bytes().to_vec();
    for r in &rows {
        out.extend_from_slice(&(r.len() as u16).to_be_bytes());
    }
    for r in rows {
        out.extend(r);
    }
    out
}

fn plane(rgba: &[u8], c: usize) -> Vec<u8> {
    rgba.chunks_exact(4).map(|p| p[c]).collect()
}

/// A Pascal string padded so its length (with the length byte) is a
/// multiple of 4.
fn pascal4(s: &str) -> Vec<u8> {
    let b: Vec<u8> = s
        .chars()
        .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
        .take(255)
        .collect();
    let mut out = vec![b.len() as u8];
    out.extend(b);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

/// Additional layer information block.
fn block(key: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(key);
    let mut d = data.to_vec();
    while !d.len().is_multiple_of(2) {
        d.push(0);
    }
    out.extend_from_slice(&(d.len() as u32).to_be_bytes());
    out.extend(d);
    out
}

/// The Unicode layer name (`luni`).
fn luni(name: &str) -> Vec<u8> {
    let u: Vec<u16> = name.encode_utf16().collect();
    let mut d = (u.len() as u32).to_be_bytes().to_vec();
    for c in u {
        d.extend_from_slice(&c.to_be_bytes());
    }
    block(b"luni", &d)
}

struct Record {
    head: Vec<u8>,
    data: Vec<u8>,
}

fn layer_record(l: &PsdLayer, divider: Option<u32>) -> Record {
    let (w, h) = (l.width as usize, l.height as usize);
    let mut chans: Vec<(i16, Vec<u8>)> = if w * h > 0 && l.rgba.len() >= w * h * 4 {
        vec![
            (-1, rle_channel(&plane(&l.rgba, 3), w, h)),
            (0, rle_channel(&plane(&l.rgba, 0), w, h)),
            (1, rle_channel(&plane(&l.rgba, 1), w, h)),
            (2, rle_channel(&plane(&l.rgba, 2), w, h)),
        ]
    } else {
        // Empty layers: zero-size channels.
        vec![
            (-1, 0u16.to_be_bytes().to_vec()),
            (0, 0u16.to_be_bytes().to_vec()),
            (1, 0u16.to_be_bytes().to_vec()),
            (2, 0u16.to_be_bytes().to_vec()),
        ]
    };
    let mut mask_extra = 0u32.to_be_bytes().to_vec();
    if let Some((ml, mt, mw, mh, px)) = &l.mask {
        chans.push((-2, rle_channel(px, *mw as usize, *mh as usize)));
        let mut m = Vec::new();
        for v in [*mt, *ml, mt + *mh as i32, ml + *mw as i32] {
            m.extend_from_slice(&v.to_be_bytes());
        }
        m.extend_from_slice(&[0, 0, 0, 0]); // default colour, flags, padding
        mask_extra = (m.len() as u32).to_be_bytes().to_vec();
        mask_extra.extend(m);
    }
    let (top, left) = (l.top, l.left);
    let (bottom, right) = (top + l.height as i32, left + l.width as i32);
    let mut head = Vec::new();
    for v in [top, left, bottom, right] {
        head.extend_from_slice(&v.to_be_bytes());
    }
    head.extend_from_slice(&(chans.len() as u16).to_be_bytes());
    for (id, d) in &chans {
        head.extend_from_slice(&id.to_be_bytes());
        head.extend_from_slice(&(d.len() as u32).to_be_bytes());
    }
    head.extend_from_slice(b"8BIM");
    head.extend_from_slice(&l.blend);
    // Flags: bit 1 = hidden, bit 3 + bit 4 = "pixel data irrelevant"
    // (group dividers).
    let mut flags = if l.visible { 0u8 } else { 2 };
    if divider.is_some() {
        flags |= 0x18;
    }
    head.extend_from_slice(&[l.opacity, 0, flags, 0]);
    let mut extra = mask_extra;
    extra.extend_from_slice(&0u32.to_be_bytes()); // blending ranges
    extra.extend(pascal4(&l.name));
    extra.extend(luni(&l.name));
    extra.extend(block(b"tsly", &[u8::from(l.transparency_shapes), 0, 0, 0]));
    if let Some(kind) = divider {
        let mut d = kind.to_be_bytes().to_vec();
        if kind != 3 {
            d.extend_from_slice(b"8BIMnorm");
        }
        extra.extend(block(b"lsct", &d));
    }
    head.extend_from_slice(&(extra.len() as u32).to_be_bytes());
    head.extend(extra);
    Record {
        head,
        data: chans.into_iter().flat_map(|c| c.1).collect(),
    }
}

/// Records bottom-to-top: a group is its divider ("</Layer group>"), its
/// children (bottom first), then its header.
fn records(nodes: &[PsdNode], out: &mut Vec<Record>) {
    for n in nodes.iter().rev() {
        match n {
            PsdNode::Layer(l) => out.push(layer_record(l, None)),
            PsdNode::Group {
                name,
                visible,
                children,
            } => {
                let marker = |name: &str, visible: bool| PsdLayer {
                    name: name.into(),
                    opacity: 255,
                    // Normal, not pass-through: some readers refuse `pass`.
                    blend: *b"norm",
                    visible,
                    transparency_shapes: true,
                    ..PsdLayer::default()
                };
                out.push(layer_record(&marker("</Layer group>", true), Some(3)));
                records(children, out);
                out.push(layer_record(&marker(name, *visible), Some(1)));
            }
        }
    }
}

/// The whole file. `merged` is the flattened image (straight-alpha RGBA,
/// `width` × `height`).
pub fn write_psd(width: u32, height: u32, nodes: &[PsdNode], merged: &[u8]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut out = b"8BPS".to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // colour mode data
    let mut res = Vec::new();
    res.extend_from_slice(&(72u32 << 16).to_be_bytes());
    res.extend_from_slice(&[0, 1, 0, 1]);
    res.extend_from_slice(&(72u32 << 16).to_be_bytes());
    res.extend_from_slice(&[0, 1, 0, 1]);
    let mut resources = b"8BIM".to_vec();
    resources.extend_from_slice(&0x03EDu16.to_be_bytes());
    resources.extend_from_slice(&[0, 0]);
    resources.extend_from_slice(&(res.len() as u32).to_be_bytes());
    resources.extend(res);
    out.extend_from_slice(&(resources.len() as u32).to_be_bytes());
    out.extend(resources);

    let mut recs = Vec::new();
    records(nodes, &mut recs);
    let mut info = (-(recs.len() as i16)).to_be_bytes().to_vec();
    for r in &recs {
        info.extend_from_slice(&r.head);
    }
    for r in recs {
        info.extend(r.data);
    }
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut lmi = (info.len() as u32).to_be_bytes().to_vec();
    lmi.extend(info);
    lmi.extend_from_slice(&0u32.to_be_bytes()); // global layer mask
    out.extend_from_slice(&(lmi.len() as u32).to_be_bytes());
    out.extend(lmi);

    // Merged image: RLE, all row counts first, then the planes.
    let full = if merged.len() >= w * h * 4 {
        merged.to_vec()
    } else {
        vec![0; w * h * 4]
    };
    let mut rows = Vec::new();
    for c in 0..4 {
        let p = plane(&full, c);
        for y in 0..h {
            rows.push(packbits(&p[y * w..(y + 1) * w]));
        }
    }
    out.extend_from_slice(&1u16.to_be_bytes());
    for r in &rows {
        out.extend_from_slice(&(r.len() as u16).to_be_bytes());
    }
    for r in rows {
        out.extend(r);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn unpack(data: &[u8], n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while out.len() < n {
            let c = data[i] as i8;
            i += 1;
            if c >= 0 {
                let k = c as usize + 1;
                out.extend_from_slice(&data[i..i + k]);
                i += k;
            } else if c != -128 {
                out.extend(std::iter::repeat_n(data[i], (1 - c as isize) as usize));
                i += 1;
            }
        }
        out
    }

    #[test]
    fn packbits_round_trips() {
        for row in [
            vec![0u8; 300],
            (0..300).map(|i| (i % 7) as u8).collect::<Vec<_>>(),
            vec![1, 2, 2, 3, 3, 3, 4, 5, 5],
            vec![9],
        ] {
            assert_eq!(unpack(&packbits(&row), row.len()), row);
        }
    }

    #[test]
    fn layers_groups_and_masks_parse_back() {
        let px = |w: u32, h: u32, v: u8| vec![v; (w * h * 4) as usize];
        let layer = |name: &str, left: i32| PsdLayer {
            name: name.into(),
            left,
            top: 2,
            width: 3,
            height: 2,
            rgba: px(3, 2, 200),
            opacity: 255,
            blend: *b"mul ",
            visible: true,
            transparency_shapes: true,
            mask: None,
        };
        let mut masked = layer("Masked", 1);
        masked.mask = Some((1, 2, 3, 2, vec![255; 6]));
        let nodes = vec![
            PsdNode::Group {
                name: "Face".into(),
                visible: true,
                children: vec![PsdNode::Layer(layer("Eye", 0)), PsdNode::Layer(masked)],
            },
            PsdNode::Layer(layer("Body", 4)),
        ];
        let bytes = write_psd(8, 6, &nodes, &px(8, 6, 0));
        let psd = crate::psd::Psd::parse(&bytes).unwrap();
        assert_eq!((psd.width, psd.height), (8, 6));
        use crate::psd::PsdNode as N;
        let [N::Group(g), N::Layer(body)] = &psd.nodes[..] else {
            panic!("{:?}", psd.nodes)
        };
        assert_eq!((g.name.as_str(), body.name.as_str()), ("Face", "Body"));
        let [N::Layer(eye), N::Layer(m)] = &g.children[..] else {
            panic!("{:?}", g.children)
        };
        assert_eq!((eye.name.as_str(), m.name.as_str()), ("Eye", "Masked"));
        assert_eq!(&eye.blend, b"mul ");
        let img = psd.pixels(eye).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert!(psd.pixels(m).is_ok());
    }
}
