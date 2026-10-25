#![no_main]
use keyform_eval::Evaluator;
use keyform_formats::moc3::{moc3_to_model, Moc3Raw};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(raw) = Moc3Raw::read(data) {
        let _ = raw.write();
        if let Ok(m) = moc3_to_model(&raw) {
            let ev = Evaluator::new(&m);
            let _ = ev.evaluate(&ev.default_values());
        }
    }
});
