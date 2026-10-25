#![allow(clippy::unwrap_used)]
//! Prints selected moc3 sections by name: `mocdump <file> <section>...`
use keyform_formats::moc3::{Moc3Raw, SectionData};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let raw = Moc3Raw::read(&std::fs::read(&args[0]).unwrap()).unwrap();
    println!("version {} counts {:?}", raw.version, raw.counts);
    for name in &args[1..] {
        match raw.sections.get(name.as_str()) {
            Some(SectionData::Ids(v)) => println!("{name}: {}", v.join(" ")),
            Some(SectionData::I32(v)) => println!("{name}: {v:?}"),
            Some(SectionData::F32(v)) => println!("{name}: {v:?}"),
            Some(SectionData::I16(v)) => println!("{name}: {v:?}"),
            Some(SectionData::U8(v)) => println!("{name}: {v:?}"),
            Some(SectionData::Runtime(n)) => println!("{name}: runtime × {n}"),
            None => println!("{name}: (absent)"),
        }
    }
}
