//! CAFF archive container used by `.cmo3` / `.can3` documents.
//!
//! spec: 10-formats/caff-archive.md

use std::io::{Read, Write};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use crate::bytes::Cursor;
use crate::error::{invalid, FormatError, Result};

const MAGIC: &[u8; 4] = b"CAFF";
const GUARD: [u8; 2] = [0x62, 0x63];
const HEADER_LEN: usize = 54;
/// Bytes of a table entry besides its two strings (start, size, flags, reserved).
const ENTRY_FIXED_LEN: usize = 8 + 4 + 1 + 1 + 8;

/// Payload compression code stored per entry. spec: caff-archive.md §3.1
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    /// 16: stored as is.
    Raw,
    /// 33: ZIP stream, deflate level 1.
    Fast,
    /// 37: ZIP stream, deflate level 5.
    Small,
    /// Any other code; treated as compressed on read and written back verbatim.
    Other(u8),
}

impl Compression {
    pub fn code(self) -> u8 {
        match self {
            Compression::Raw => 16,
            Compression::Fast => 33,
            Compression::Small => 37,
            Compression::Other(c) => c,
        }
    }
    pub fn from_code(c: u8) -> Self {
        match c {
            16 => Compression::Raw,
            33 => Compression::Fast,
            37 => Compression::Small,
            c => Compression::Other(c),
        }
    }
    fn level(self) -> u32 {
        match self {
            Compression::Small => 5,
            _ => 1,
        }
    }
}

/// Preview image header fields (the PNG itself is the entry tagged `preview`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewInfo {
    /// 0 = not set, 1 = PNG, 127 = none.
    pub format: u8,
    /// 0 = not set, 1 = ARGB, 2 = RGB, 127 = none.
    pub color_type: u8,
    pub width: i16,
    pub height: i16,
}

impl PreviewInfo {
    pub const NONE: PreviewInfo = PreviewInfo {
        format: 127,
        color_type: 127,
        width: 0,
        height: 0,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaffEntry {
    pub path: String,
    pub tag: String,
    pub obfuscated: bool,
    pub compression: Compression,
    /// Decoded (de-obfuscated, inflated) file contents.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaffArchive {
    pub archive_version: [u8; 3],
    pub format_id: [u8; 4],
    pub format_version: [u8; 3],
    /// Obfuscation key; 0 = none. spec: caff-archive.md §1
    pub key: i32,
    pub preview: PreviewInfo,
    pub entries: Vec<CaffEntry>,
    /// False if the trailing guard bytes were missing (a warning, not an error; §3.3).
    pub guard_ok: bool,
}

/// Limits applied while reading untrusted archives.
#[derive(Clone, Copy, Debug)]
pub struct ReadLimits {
    /// Maximum decoded size of one entry.
    pub max_entry_size: usize,
    /// Maximum total decoded size of all entries.
    pub max_total_size: usize,
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            max_entry_size: 1 << 30,
            max_total_size: 4 << 30,
        }
    }
}

/// The obfuscation key in its 8/16/32/64-bit forms.
///
/// The 64-bit form is the 32-bit key in the high half OR'd with the key
/// **sign-extended** to 64 bits, so for a negative key the high half reads all
/// ones. spec: caff-archive.md §1
#[derive(Clone, Copy)]
struct Key(i32);

impl Key {
    fn b(self) -> u8 {
        self.0 as u8
    }
    fn k32(self) -> u32 {
        self.0 as u32
    }
    fn k64(self) -> u64 {
        ((self.0 as u32 as u64) << 32) | (self.0 as i64 as u64)
    }
}

fn read_varint(c: &mut Cursor, key: Key) -> Result<u32> {
    // spec: caff-archive.md §2 — big-endian base-128, MSB group first, ≤ 4 bytes.
    let mut n: u32 = 0;
    for _ in 0..4 {
        let byte = c.u8()? ^ key.b();
        n = (n << 7) | u32::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(n);
        }
    }
    invalid("varint longer than 4 bytes")
}

fn write_varint(out: &mut Vec<u8>, n: u32, key: Key) -> Result<()> {
    if n >= 1 << 28 {
        return Err(FormatError::LimitExceeded(format!(
            "varint value {n} ≥ 2^28"
        )));
    }
    let groups = match n {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        0x4000..=0x1f_ffff => 3,
        _ => 4,
    };
    for g in (0..groups).rev() {
        let mut b = ((n >> (7 * g)) & 0x7f) as u8;
        if g != 0 {
            b |= 0x80;
        }
        out.push(b ^ key.b());
    }
    Ok(())
}

fn read_string(c: &mut Cursor, key: Key) -> Result<String> {
    let len = read_varint(c, key)? as usize;
    let raw: Vec<u8> = c.take(len)?.iter().map(|b| b ^ key.b()).collect();
    String::from_utf8(raw).or_else(|_| invalid("string is not UTF-8"))
}

fn write_string(out: &mut Vec<u8>, s: &str, key: Key) -> Result<()> {
    let len = u32::try_from(s.len()).map_err(|_| FormatError::LimitExceeded("string".into()))?;
    write_varint(out, len, key)?;
    out.extend(s.bytes().map(|b| b ^ key.b()));
    Ok(())
}

impl CaffArchive {
    /// An empty archive with the values the reference editor writes for
    /// `.cmo3` / `.can3` (spec: caff-archive.md §5), unobfuscated.
    pub fn new_document() -> Self {
        Self {
            archive_version: [0; 3],
            format_id: *b"----",
            format_version: [0; 3],
            key: 0,
            preview: PreviewInfo::NONE,
            entries: Vec::new(),
            guard_ok: true,
        }
    }

    pub fn read(data: &[u8]) -> Result<Self> {
        Self::read_with_limits(data, ReadLimits::default())
    }

    pub fn read_with_limits(data: &[u8], limits: ReadLimits) -> Result<Self> {
        let mut c = Cursor::new(data);
        if &c.array::<4>()? != MAGIC {
            return Err(FormatError::BadMagic { expected: "CAFF" });
        }
        let archive_version = c.array::<3>()?;
        let format_id = c.array::<4>()?;
        let format_version = c.array::<3>()?;
        let key = Key(c.u32_be()? as i32);
        c.take(8)?;
        let preview_format = c.u8()?;
        let preview_color = c.u8()?;
        c.take(2)?;
        let pw = c.u16_be()? as i16;
        let ph = c.u16_be()? as i16;
        let _preview_start = c.u64_be()?;
        let _preview_size = c.u32_be()?;
        c.take(8)?;
        debug_assert_eq!(c.pos(), HEADER_LEN);

        let count = (c.u32_be()? ^ key.k32()) as usize;
        // Every entry needs at least two 1-byte varints plus the fixed part.
        if count > data.len() / (ENTRY_FIXED_LEN + 2) {
            return invalid(format!("file count {count} too large for archive size"));
        }

        struct Raw {
            path: String,
            tag: String,
            start: u64,
            size: u32,
            obfuscated: bool,
            compression: Compression,
        }
        let mut table = Vec::with_capacity(count);
        for _ in 0..count {
            let path = read_string(&mut c, key)?;
            let tag = read_string(&mut c, key)?;
            let start = c.u64_be()? ^ key.k64();
            let size = c.u32_be()? ^ key.k32();
            let obfuscated = (c.u8()? ^ key.b()) != 0;
            let compression = Compression::from_code(c.u8()? ^ key.b());
            c.take(8)?;
            table.push(Raw {
                path,
                tag,
                start,
                size,
                obfuscated,
                compression,
            });
        }

        let mut total = 0usize;
        let mut entries = Vec::with_capacity(count);
        for r in table {
            let start = usize::try_from(r.start).or_else(|_| invalid("entry offset"))?;
            let stored = Cursor::at(data, start)
                .and_then(|mut c| c.take(r.size as usize))
                .map_err(|_| {
                    FormatError::Invalid(format!("entry {:?} lies outside the archive", r.path))
                })?;
            let plain: Vec<u8> = if r.obfuscated {
                stored.iter().map(|b| b ^ key.b()).collect()
            } else {
                stored.to_vec()
            };
            let decoded = match r.compression {
                Compression::Raw => plain,
                _ => read_zip_stream(&plain, limits.max_entry_size)
                    .map_err(|e| FormatError::Invalid(format!("entry {:?}: {e}", r.path)))?,
            };
            if decoded.len() > limits.max_entry_size {
                return Err(FormatError::LimitExceeded(format!(
                    "entry {:?} too large",
                    r.path
                )));
            }
            total = total.saturating_add(decoded.len());
            if total > limits.max_total_size {
                return Err(FormatError::LimitExceeded("archive total size".into()));
            }
            entries.push(CaffEntry {
                path: r.path,
                tag: r.tag,
                obfuscated: r.obfuscated,
                compression: r.compression,
                data: decoded,
            });
        }

        let guard_ok = data.len() >= 2 && data[data.len() - 2..] == GUARD;
        Ok(Self {
            archive_version,
            format_id,
            format_version,
            key: key.0,
            preview: PreviewInfo {
                format: preview_format,
                color_type: preview_color,
                width: pw,
                height: ph,
            },
            entries,
            guard_ok,
        })
    }

    pub fn entry(&self, path: &str) -> Option<&CaffEntry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// The document: the first entry tagged `main_xml` (§6 step 4).
    pub fn main_xml(&self) -> Option<&CaffEntry> {
        self.entries.iter().find(|e| e.tag == "main_xml")
    }

    /// Returns `path` made unique among the archive's entries by inserting
    /// `_0`, `_1`, … before the extension (§4).
    pub fn unique_path(&self, path: &str) -> String {
        let path = path.replace('\\', "/");
        if self.entry(&path).is_none() {
            return path;
        }
        let (stem, ext) = match path.rfind('.') {
            Some(i) if i > path.rfind('/').map_or(0, |s| s + 1) => path.split_at(i),
            _ => (path.as_str(), ""),
        };
        (0..)
            .map(|n| format!("{stem}_{n}{ext}"))
            .find(|p| self.entry(p).is_none())
            .unwrap_or_default()
    }

    /// Serializes the archive. With a non-zero key every entry except the
    /// preview is obfuscated, whatever its flag says (§ Edge cases).
    pub fn write(&self) -> Result<Vec<u8>> {
        let key = Key(self.key);
        let mut payloads = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            let is_preview = e.tag == "preview";
            let obfuscate = self.key != 0 && !is_preview;
            let mut bytes = match e.compression {
                Compression::Raw => e.data.clone(),
                c => write_zip_stream(&e.data, c.level())?,
            };
            if obfuscate {
                bytes.iter_mut().for_each(|b| *b ^= key.b());
            }
            payloads.push((bytes, obfuscate));
        }

        // Table with placeholder offsets first, to learn its length.
        let table_len = {
            let mut t = Vec::new();
            for e in &self.entries {
                write_string(&mut t, &e.path, key)?;
                write_string(&mut t, &e.tag, key)?;
            }
            t.len() + self.entries.len() * ENTRY_FIXED_LEN
        };
        let mut offset = (HEADER_LEN + 4 + table_len) as u64;
        let mut starts = Vec::with_capacity(payloads.len());
        for (p, _) in &payloads {
            starts.push(offset);
            offset += p.len() as u64;
        }

        let mut out = Vec::with_capacity(offset as usize + 2);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.archive_version);
        out.extend_from_slice(&self.format_id);
        out.extend_from_slice(&self.format_version);
        out.extend_from_slice(&(self.key as u32).to_be_bytes());
        out.extend_from_slice(&[0; 8]);
        let preview_idx = self.entries.iter().position(|e| e.tag == "preview");
        let preview = if preview_idx.is_some() {
            self.preview
        } else {
            PreviewInfo::NONE
        };
        out.push(preview.format);
        out.push(preview.color_type);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&(preview.width as u16).to_be_bytes());
        out.extend_from_slice(&(preview.height as u16).to_be_bytes());
        let (ps, pl) = preview_idx.map_or((0, 0), |i| (starts[i], payloads[i].0.len() as u32));
        out.extend_from_slice(&ps.to_be_bytes());
        out.extend_from_slice(&pl.to_be_bytes());
        out.extend_from_slice(&[0; 8]);

        let count = u32::try_from(self.entries.len())
            .map_err(|_| FormatError::LimitExceeded("entry count".into()))?;
        out.extend_from_slice(&(count ^ key.k32()).to_be_bytes());
        for ((e, (p, obf)), start) in self.entries.iter().zip(&payloads).zip(&starts) {
            write_string(&mut out, &e.path, key)?;
            write_string(&mut out, &e.tag, key)?;
            let size = u32::try_from(p.len())
                .map_err(|_| FormatError::LimitExceeded(format!("entry {:?}", e.path)))?;
            out.extend_from_slice(&(start ^ key.k64()).to_be_bytes());
            out.extend_from_slice(&(size ^ key.k32()).to_be_bytes());
            out.push(u8::from(*obf) ^ key.b());
            out.push(e.compression.code() ^ key.b());
            out.extend_from_slice(&[0; 8]);
        }
        for (p, _) in &payloads {
            out.extend_from_slice(p);
        }
        out.extend_from_slice(&GUARD);
        Ok(out)
    }
}

const LOCAL_HEADER_SIG: u32 = 0x0403_4b50;
const DESCRIPTOR_SIG: u32 = 0x0807_4b50;

/// Reads the single entry of a ZIP *stream* (local header, data, optional
/// data descriptor; the reference writes no central directory). spec: §3.2
fn read_zip_stream(data: &[u8], max_size: usize) -> Result<Vec<u8>> {
    let mut c = Cursor::new(data);
    if c.u32_le()? != LOCAL_HEADER_SIG {
        return invalid("payload is not a ZIP stream");
    }
    let _version = c.u16_le()?;
    let flags = c.u16_le()?;
    let method = c.u16_le()?;
    let _time = c.u32_le()?;
    let mut crc = c.u32_le()?;
    let csize = c.u32_le()? as usize;
    let _usize = c.u32_le()?;
    let name_len = c.u16_le()? as usize;
    let extra_len = c.u16_le()? as usize;
    c.take(name_len + extra_len)?;
    let body = &data[c.pos()..];
    let has_descriptor = flags & 0x08 != 0;

    let (out, consumed) = match method {
        0 => {
            if has_descriptor {
                return invalid("stored ZIP entry with data descriptor");
            }
            let s = Cursor::new(body).take(csize)?;
            (s.to_vec(), csize)
        }
        8 => {
            let src = if has_descriptor {
                body
            } else {
                Cursor::new(body).take(csize)?
            };
            let mut dec = DeflateDecoder::new(src);
            let mut out = Vec::new();
            (&mut dec).take(max_size as u64 + 1).read_to_end(&mut out)?;
            if out.len() > max_size {
                return Err(FormatError::LimitExceeded("inflated entry".into()));
            }
            (out, dec.total_in() as usize)
        }
        m => return Err(FormatError::Unsupported(format!("ZIP method {m}"))),
    };
    if has_descriptor {
        let mut d = Cursor::at(body, consumed)?;
        let mut first = d.u32_le()?;
        if first == DESCRIPTOR_SIG {
            first = d.u32_le()?;
        }
        crc = first;
    }
    if crc32fast::hash(&out) != crc {
        return invalid("ZIP entry CRC mismatch");
    }
    Ok(out)
}

/// Writes `data` as a ZIP stream holding one entry named `contents`, laid out
/// like the reference output: local header with deferred sizes, deflate data,
/// data descriptor, no central directory.
fn write_zip_stream(data: &[u8], level: u32) -> Result<Vec<u8>> {
    let mut enc = DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
    enc.write_all(data)?;
    let deflated = enc.finish()?;
    let name = b"contents";
    let mut out = Vec::with_capacity(deflated.len() + 64);
    out.extend_from_slice(&LOCAL_HEADER_SIG.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0x0808u16.to_le_bytes()); // data descriptor + UTF-8 names
    out.extend_from_slice(&8u16.to_le_bytes()); // deflate
    out.extend_from_slice(&0x0021_0000u32.to_le_bytes()); // DOS time 0, date 1980-01-01
    out.extend_from_slice(&[0; 12]); // crc, sizes (in descriptor)
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&deflated);
    let size = |n: usize| {
        u32::try_from(n).map_err(|_| FormatError::LimitExceeded("ZIP entry over 4 GiB".into()))
    };
    out.extend_from_slice(&DESCRIPTOR_SIG.to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
    out.extend_from_slice(&size(deflated.len())?.to_le_bytes());
    out.extend_from_slice(&size(data.len())?.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(key: i32) -> CaffArchive {
        let mut a = CaffArchive::new_document();
        a.key = key;
        a.entries.push(CaffEntry {
            path: "imageFileBuf.png".into(),
            tag: String::new(),
            obfuscated: key != 0,
            compression: Compression::Raw,
            data: vec![0x89, b'P', b'N', b'G', 1, 2, 3],
        });
        a.entries.push(CaffEntry {
            path: "main.xml".into(),
            tag: "main_xml".into(),
            obfuscated: key != 0,
            compression: Compression::Fast,
            data: b"<?xml version=\"1.0\"?><root/>".repeat(50),
        });
        a
    }

    #[test]
    fn varint_examples() {
        // spec: caff-archive.md §2
        for (n, bytes) in [
            (0u32, vec![0x00]),
            (0x7f, vec![0x7f]),
            (0x80, vec![0x81, 0x00]),
            (300, vec![0x82, 0x2c]),
            (0x3fff, vec![0xff, 0x7f]),
            (0x4000, vec![0x81, 0x80, 0x00]),
        ] {
            let mut out = Vec::new();
            write_varint(&mut out, n, Key(0)).unwrap();
            assert_eq!(out, bytes, "encode {n}");
            assert_eq!(read_varint(&mut Cursor::new(&bytes), Key(0)).unwrap(), n);
        }
        assert!(write_varint(&mut Vec::new(), 1 << 28, Key(0)).is_err());
    }

    #[test]
    fn key64_sign_extends() {
        assert_eq!(Key(0x1234_5678).k64(), 0x1234_5678_1234_5678);
        assert_eq!(Key(0xb3f9_7dc4_u32 as i32).k64(), 0xffff_ffff_b3f9_7dc4);
    }

    #[test]
    fn roundtrip_plain_and_obfuscated() {
        for key in [0, 0x5a5a_5a5a, 0xb3f9_7dc4_u32 as i32] {
            let a = sample(key);
            let bytes = a.write().unwrap();
            assert_eq!(&bytes[bytes.len() - 2..], &GUARD);
            let b = CaffArchive::read(&bytes).unwrap();
            assert_eq!(a, b, "key {key:#x}");
        }
    }

    #[test]
    fn unique_paths() {
        let a = sample(0);
        assert_eq!(a.unique_path("foo.png"), "foo.png");
        assert_eq!(a.unique_path("imageFileBuf.png"), "imageFileBuf_0.png");
        assert_eq!(a.unique_path("dir\\x.png"), "dir/x.png");
    }

    #[test]
    fn truncated_inputs_never_panic() {
        let bytes = sample(0x1357_9bdf).write().unwrap();
        for n in 0..bytes.len() {
            let _ = CaffArchive::read(&bytes[..n]);
        }
        let mut flipped = bytes.clone();
        for i in 0..flipped.len() {
            flipped[i] ^= 0xff;
            let _ = CaffArchive::read(&flipped);
            flipped[i] ^= 0xff;
        }
    }
}
