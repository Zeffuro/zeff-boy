# Fuzzing

With nightly Rust and `cargo-fuzz` installed, run from the repository root:

```sh
cargo +nightly fuzz run fuzz_audio_rip -- -max_total_time=60 -max_len=262144
```

Audio targets: `fuzz_audio_gba`, `fuzz_audio_tracker`, `fuzz_audio_vgm`,
`fuzz_audio_rip`, `fuzz_audio_natsume`. Inputs start with a control byte for format,
limits and cancellation; see `crates/zeff-audio-discovery/src/fuzzing.rs`.
Targets use the portable library's additive `fuzzing` feature. Limit: 256 KiB.

On Windows, use the Visual Studio x64 Native Tools prompt so the ASAN runtime
is on `PATH`. Keep failure artifacts and logs for reproduction.
