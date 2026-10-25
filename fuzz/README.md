# Fuzzing

Long-running fuzzers for the untrusted-input parsers (needs nightly Rust and
`cargo install cargo-fuzz`):

    cd fuzz
    cargo +nightly fuzz run moc3   -- -max_len=4000000
    cargo +nightly fuzz run caff
    cargo +nightly fuzz run xml
    cargo +nightly fuzz run cmo3

Seed corpora: copy sample files into `fuzz/corpus/<target>/` locally (never
commit licensed samples). The deterministic mutation tests in
`crates/keyform-formats/tests/robustness.rs` run on stable with `cargo test`.
