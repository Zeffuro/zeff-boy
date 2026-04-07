#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut minimal_rom = vec![0u8; 16 + 16384];
    minimal_rom[0..4].copy_from_slice(b"NES\x1A");
    minimal_rom[4] = 1;
    minimal_rom[5] = 0;

    let emu = zeff_nes_core::emulator::Emulator::new(
        &minimal_rom,
        48000.0,
    );
    if let Ok(mut emu) = emu {
        let _ = emu.load_state(data);
    }
});

