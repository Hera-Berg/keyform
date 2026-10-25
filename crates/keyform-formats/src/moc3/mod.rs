//! moc3 runtime model format: raw section container ([`Moc3Raw`]) plus
//! conversion to and from [`keyform_model::Model`].
//!
//! spec: 10-formats/moc3.md

mod export;
mod import;
pub mod layout;

pub use export::{export_moc3, ExportOptions, ExportReport};
pub use import::moc3_to_model;

use std::collections::HashMap;

use crate::bytes::Cursor;
use crate::error::{invalid, FormatError, Result};
use layout::{Kind, SECTIONS};

/// Latest header version byte (Cubism 5.3). spec §1
pub const LATEST_VERSION: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasInfo {
    pub pixels_per_unit: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub width: f32,
    pub height: f32,
    /// 0 = normal coordinates (always written), 1 = Y-reversed.
    pub flags: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SectionData {
    /// Element count of a zero-filled runtime section.
    Runtime(usize),
    Ids(Vec<String>),
    I32(Vec<i32>),
    F32(Vec<f32>),
    I16(Vec<i16>),
    U8(Vec<u8>),
}

impl SectionData {
    pub fn len(&self) -> usize {
        match self {
            SectionData::Runtime(n) => *n,
            SectionData::Ids(v) => v.len(),
            SectionData::I32(v) => v.len(),
            SectionData::F32(v) => v.len(),
            SectionData::I16(v) => v.len(),
            SectionData::U8(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn empty(kind: Kind, n: usize) -> Self {
        match kind {
            Kind::Runtime => SectionData::Runtime(n),
            Kind::Ids => SectionData::Ids(vec![String::new(); n]),
            Kind::I32 => SectionData::I32(vec![0; n]),
            Kind::F32 => SectionData::F32(vec![0.0; n]),
            Kind::I16 => SectionData::I16(vec![0; n]),
            Kind::U8 => SectionData::U8(vec![0; n]),
        }
    }
}

/// A moc3 file as its count block, canvas info and sections, without
/// semantic interpretation.
#[derive(Clone, Debug, PartialEq)]
pub struct Moc3Raw {
    pub version: u8,
    /// Count-info values present for `version` (spec §4, 0-based).
    pub counts: Vec<i32>,
    pub canvas: CanvasInfo,
    /// Section name → data, for every section present in `version`.
    pub sections: HashMap<&'static str, SectionData>,
}

impl Moc3Raw {
    /// An empty file of `version` with all counts zero.
    pub fn new(version: u8, canvas: CanvasInfo) -> Self {
        let mut sections = HashMap::new();
        for s in SECTIONS.iter().filter(|s| s.since <= version) {
            sections.insert(s.name, SectionData::empty(s.kind, 0));
        }
        Self {
            version,
            counts: vec![0; layout::c::present(version)],
            canvas,
            sections,
        }
    }

    pub fn count(&self, idx: usize) -> usize {
        self.counts.get(idx).map_or(0, |&c| c.max(0) as usize)
    }

    pub fn i32s(&self, name: &str) -> &[i32] {
        match self.sections.get(name) {
            Some(SectionData::I32(v)) => v,
            _ => &[],
        }
    }
    pub fn f32s(&self, name: &str) -> &[f32] {
        match self.sections.get(name) {
            Some(SectionData::F32(v)) => v,
            _ => &[],
        }
    }
    pub fn i16s(&self, name: &str) -> &[i16] {
        match self.sections.get(name) {
            Some(SectionData::I16(v)) => v,
            _ => &[],
        }
    }
    pub fn u8s(&self, name: &str) -> &[u8] {
        match self.sections.get(name) {
            Some(SectionData::U8(v)) => v,
            _ => &[],
        }
    }
    pub fn ids(&self, name: &str) -> &[String] {
        match self.sections.get(name) {
            Some(SectionData::Ids(v)) => v,
            _ => &[],
        }
    }

    pub fn set(&mut self, name: &'static str, data: SectionData) {
        self.sections.insert(name, data);
    }

    pub fn read(data: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(data);
        if &c.array::<4>()? != b"MOC3" {
            return Err(FormatError::BadMagic { expected: "MOC3" });
        }
        let version = c.u8()?;
        if !(1..=LATEST_VERSION).contains(&version) {
            return Err(FormatError::Unsupported(format!("moc3 version {version}")));
        }
        if c.u8()? != 0 {
            return Err(FormatError::Unsupported("big-endian moc3".into()));
        }
        let n_entries = layout::table_entries(version);
        let mut table = Vec::with_capacity(n_entries);
        let mut tc = Cursor::at(data, layout::HEADER_LEN)?;
        for _ in 0..n_entries {
            table.push(u32::from_le_bytes(tc.array()?) as usize);
        }

        let n_counts = layout::c::present(version);
        let mut cc = Cursor::at(data, table[0])?;
        let mut counts = Vec::with_capacity(n_counts);
        for i in 0..n_counts {
            let v = i32::from_le_bytes(cc.array()?);
            if v < 0 {
                return invalid(format!("negative count #{i}"));
            }
            counts.push(v);
        }
        let mut cv = Cursor::at(data, table[1])?;
        let f = |c: &mut Cursor| -> Result<f32> { Ok(f32::from_le_bytes(c.array()?)) };
        let canvas = CanvasInfo {
            pixels_per_unit: f(&mut cv)?,
            origin_x: f(&mut cv)?,
            origin_y: f(&mut cv)?,
            width: f(&mut cv)?,
            height: f(&mut cv)?,
            flags: cv.u8()?,
        };

        let mut sections = HashMap::new();
        let mut prev_end =
            table[1] + layout::CANVAS_INFO_LEN.min(data.len() - table[1].min(data.len()));
        for s in SECTIONS.iter().filter(|s| s.since <= version) {
            let n = counts[s.count] as usize;
            let off = table[s.row - 1];
            let size = n
                .checked_mul(s.kind.elem_size())
                .ok_or_else(|| FormatError::Invalid(format!("{}: size overflow", s.name)))?;
            if n > 0 && off < prev_end {
                return invalid(format!(
                    "{}: section offset {off} overlaps previous data",
                    s.name
                ));
            }
            let mut sc = Cursor::at(data, off).map_err(|_| {
                FormatError::Invalid(format!("{}: offset {off} outside file", s.name))
            })?;
            let bytes = sc.take(size).map_err(|_| {
                FormatError::Invalid(format!("{}: {n} elements exceed the file", s.name))
            })?;
            if n > 0 {
                prev_end = off + size;
            }
            let d = match s.kind {
                Kind::Runtime => SectionData::Runtime(n),
                Kind::Ids => SectionData::Ids(
                    bytes
                        .as_chunks::<64>()
                        .0
                        .iter()
                        .map(|slot| {
                            let end = slot.iter().position(|&b| b == 0).ok_or_else(|| {
                                FormatError::Invalid(format!("{}: ID not zero-terminated", s.name))
                            })?;
                            Ok(String::from_utf8_lossy(&slot[..end]).into_owned())
                        })
                        .collect::<Result<_>>()?,
                ),
                Kind::I32 => SectionData::I32(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect(),
                ),
                Kind::F32 => SectionData::F32(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect(),
                ),
                Kind::I16 => SectionData::I16(
                    bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| i16::from_le_bytes([b[0], b[1]]))
                        .collect(),
                ),
                Kind::U8 => SectionData::U8(bytes.to_vec()),
            };
            sections.insert(s.name, d);
        }
        Ok(Self {
            version,
            counts,
            canvas,
            sections,
        })
    }

    /// Serializes with the reference layout: every section 64-aligned except
    /// ID sections, which directly follow the preceding (runtime) section;
    /// file length padded to 64. spec §2–3
    pub fn write(&self) -> Result<Vec<u8>> {
        let v = self.version;
        if !(1..=LATEST_VERSION).contains(&v) {
            return Err(FormatError::Unsupported(format!("moc3 version {v}")));
        }
        let mut table = vec![0u32; layout::table_entries(v)];
        let mut out = vec![0u8; layout::data_base(v)];
        out[..4].copy_from_slice(b"MOC3");
        out[4] = v;

        let to_u32 = |n: usize| {
            u32::try_from(n).map_err(|_| FormatError::LimitExceeded("moc3 over 4 GiB".into()))
        };
        table[0] = to_u32(out.len())?;
        let mut counts = vec![0u8; layout::count_info_len(v)];
        for (i, &n) in self.counts.iter().take(layout::c::present(v)).enumerate() {
            counts[4 * i..4 * i + 4].copy_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&counts);
        table[1] = to_u32(out.len())?;
        let cv = &self.canvas;
        for f in [
            cv.pixels_per_unit,
            cv.origin_x,
            cv.origin_y,
            cv.width,
            cv.height,
        ] {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.push(cv.flags);
        out.resize(table[1] as usize + layout::CANVAS_INFO_LEN, 0);

        for s in SECTIONS.iter().filter(|s| s.since <= v) {
            let n = self.count(s.count);
            let d = self.sections.get(s.name);
            let len = d.map_or(0, SectionData::len);
            if len != n {
                return invalid(format!("{}: {len} elements but count says {n}", s.name));
            }
            if s.kind != Kind::Ids {
                out.resize(layout::align64(out.len()), 0);
            }
            table[s.row - 1] = to_u32(out.len())?;
            match d {
                None | Some(SectionData::Runtime(_)) => out.resize(out.len() + 8 * n, 0),
                Some(SectionData::Ids(ids)) => {
                    for id in ids {
                        if id.len() >= 64 {
                            return invalid(format!("ID {id:?} is 64 bytes or longer"));
                        }
                        let start = out.len();
                        out.extend_from_slice(id.as_bytes());
                        out.resize(start + 64, 0);
                    }
                }
                Some(SectionData::I32(x)) => x
                    .iter()
                    .for_each(|e| out.extend_from_slice(&e.to_le_bytes())),
                Some(SectionData::F32(x)) => x
                    .iter()
                    .for_each(|e| out.extend_from_slice(&e.to_le_bytes())),
                Some(SectionData::I16(x)) => x
                    .iter()
                    .for_each(|e| out.extend_from_slice(&e.to_le_bytes())),
                Some(SectionData::U8(x)) => out.extend_from_slice(x),
            }
        }
        out.resize(layout::align64(out.len()), 0);
        for (i, off) in table.iter().enumerate() {
            let p = layout::HEADER_LEN + 4 * i;
            out[p..p + 4].copy_from_slice(&off.to_le_bytes());
        }
        Ok(out)
    }
}
