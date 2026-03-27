#![no_main]
use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

fuzz_target!(|data: &[u8]| {
    let _ = zeff_nes_core::hardware::cartridge::Cartridge::load(data);
});

