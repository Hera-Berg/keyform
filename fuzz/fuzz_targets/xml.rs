#![no_main]
use keyform_formats::graph::ObjectGraph;
use keyform_formats::xml::XmlDocument;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = XmlDocument::parse(data) {
        let _ = doc.write();
        if let Ok(g) = ObjectGraph::new(&doc) {
            let _ = g.main();
        }
    }
});
