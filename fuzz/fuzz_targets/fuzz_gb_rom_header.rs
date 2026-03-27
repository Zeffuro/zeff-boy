#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = zeff_gb_core::hardware::rom_header::RomHeader::from_rom(data);
});

