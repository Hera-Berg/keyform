#![allow(clippy::unwrap_used)]
//! Malformed-input robustness: mutated and truncated sample files must make
//! the readers return errors, never panic. Deterministic (fixed seeds); longer
//! runs live in `fuzz/` (cargo-fuzz).

use std::panic::{catch_unwind, AssertUnwindSafe};

use keyform_eval::Evaluator;
use keyform_formats::caff::CaffArchive;
use keyform_formats::cmo3::Cmo3Document;
use keyform_formats::moc3::{moc3_to_model, Moc3Raw};
use keyform_formats::xml::XmlDocument;

mod common;
use common as samples;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Mutation kinds aimed at structured binary data.
fn mutate(rng: &mut Rng, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    if d.is_empty() {
        return d;
    }
    match rng.below(5) {
        0 => {
            let i = rng.below(d.len());
            d[i] ^= 1 << rng.below(8);
        }
        1 => {
            // Overwrite an aligned 4-byte word with an extreme value.
            let i = rng.below(d.len() / 4) * 4;
            let v: [u8; 4] =
                [[0xff; 4], [0x7f, 0xff, 0xff, 0xff], [0x80, 0, 0, 0], [0; 4]][rng.below(4)];
            for (k, b) in v.iter().enumerate() {
                if let Some(x) = d.get_mut(i + k) {
                    *x = *b;
                }
            }
        }
        2 => d.truncate(rng.below(d.len())),
        3 => {
            let i = rng.below(d.len());
            d[i] = rng.next() as u8;
        }
        _ => {
            // Small signed nudge of a 32-bit little-endian integer.
            let i = rng.below(d.len() / 4) * 4;
            if i + 4 <= d.len() {
                let v = i32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
                let n = v.wrapping_add(rng.below(5) as i32 - 2);
                d[i..i + 4].copy_from_slice(&n.to_le_bytes());
            }
        }
    }
    d
}

fn no_panic(what: &str, f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        panic!("panic on malformed input: {what}");
    }
}

#[test]
fn moc3_mutations_never_panic() {
    for path in samples::sample_files("moc3") {
        let data = std::fs::read(&path).unwrap();
        let mut rng = Rng(0x1234_5678_9abc_def1);
        for i in 0..3000 {
            let d = mutate(&mut rng, &data);
            no_panic(&format!("{path:?} mutation {i}"), || {
                if let Ok(raw) = Moc3Raw::read(&d) {
                    if let Ok(m) = moc3_to_model(&raw) {
                        let ev = Evaluator::new(&m);
                        let _ = ev.evaluate(&ev.default_values());
                    }
                }
            });
        }
    }
}

#[test]
fn xml_mutations_never_panic() {
    // Use the smallest sample project's document to keep the test fast.
    let mut files = samples::sample_files("cmo3");
    files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
    let Some(path) = files.first() else { return };
    let archive = CaffArchive::read(&std::fs::read(path).unwrap()).unwrap();
    let xml = archive.main_xml().unwrap().data.clone();
    let mut rng = Rng(0x0dd_ba11_cafe_f00d);
    for i in 0..60 {
        // Mutate a window of the text, keeping it mostly well-formed.
        let mut d = xml.clone();
        for _ in 0..8 {
            let at = rng.below(d.len());
            match rng.below(4) {
                0 => d[at] = b"<>\"/=#0123456789-. "[rng.below(19)],
                1 => {
                    d.remove(at);
                }
                2 => d.insert(at, b'9'),
                _ => d[at] = b'"',
            }
        }
        no_panic(&format!("xml mutation {i}"), || {
            if let Ok(doc) = XmlDocument::parse(&d) {
                let mut a = archive.clone();
                a.key = 0;
                if let Some(e) = a.entries.iter_mut().find(|e| e.tag == "main_xml") {
                    e.data = doc.write();
                }
                if let Ok(bytes) = a.write() {
                    if let Ok(doc) = Cmo3Document::read(&bytes) {
                        let ev = Evaluator::new(&doc.model);
                        let _ = ev.evaluate(&ev.default_values());
                    }
                }
            }
        });
    }
}
