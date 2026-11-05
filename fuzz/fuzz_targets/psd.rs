#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(psd) = keyform_formats::psd::Psd::parse(data) {
        for l in psd.layers().into_iter().take(16) {
            let _ = psd.pixels(l);
        }
    }
});
