#![no_main]
//! Feeds the input as an unobfuscated main.xml inside a CAFF archive.
use keyform_eval::Evaluator;
use keyform_formats::caff::{CaffArchive, CaffEntry, Compression};
use keyform_formats::cmo3::Cmo3Document;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut a = CaffArchive::new_document();
    a.entries.push(CaffEntry {
        path: "main.xml".into(),
        tag: "main_xml".into(),
        obfuscated: false,
        compression: Compression::Raw,
        data: data.to_vec(),
    });
    if let Ok(bytes) = a.write() {
        if let Ok(doc) = Cmo3Document::read(&bytes) {
            let ev = Evaluator::new(&doc.model);
            let _ = ev.evaluate(&ev.default_values());
        }
    }
});
