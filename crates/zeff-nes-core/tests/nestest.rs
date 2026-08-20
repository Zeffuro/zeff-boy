use std::path::PathBuf;

fn nestest_rom_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../test-roms/nes-test-roms/other/nestest.nes");
    path
}

fn run_nestest_automation() -> Option<zeff_nes_core::emulator::Emulator> {
    let rom_path = nestest_rom_path();
    if !rom_path.exists() {
        eprintln!("Skipping nestest: ROM not found at {}", rom_path.display());
        return None;
    }

    let rom_data = std::fs::read(&rom_path).expect("failed to read nestest.nes");
    let mut emu = zeff_nes_core::emulator::Emulator::new(&rom_data, 48000.0)
        .expect("failed to create emulator");

    // nestest's automation mode treats zero as the passing default and only
    // writes a non-zero failure code. Real NES work RAM has no fixed power-on
    // value, so establish the test ROM's documented harness precondition.
    emu.bus_mut().ram.fill(0);
    emu.set_cpu_pc(0xC000);

    for _ in 0..30_000 {
        emu.step_instruction();
        if emu.last_opcode_pc() == 0xC66E {
            return Some(emu);
        }
    }

    panic!("nestest automation did not reach its final RTS");
}

#[test]
fn nestest_official_opcodes_pass() {
    let Some(emu) = run_nestest_automation() else {
        return;
    };

    let official_result = emu.bus().ram[0x02];
    let unofficial_result = emu.bus().ram[0x03];

    assert_eq!(
        official_result, 0x00,
        "nestest official opcode tests failed with error code: {:#04X}. \
         See nestest.txt for failure code meanings.",
        official_result
    );

    if unofficial_result != 0x00 {
        eprintln!(
            "nestest unofficial opcode tests returned: {:#04X} (non-zero may indicate \
             edge-case differences in unofficial opcode behavior)",
            unofficial_result
        );
    }
}

#[test]
fn nestest_unofficial_opcodes_pass() {
    let Some(emu) = run_nestest_automation() else {
        return;
    };

    let unofficial_result = emu.bus().ram[0x03];
    assert_eq!(
        unofficial_result, 0x00,
        "nestest unofficial opcode tests failed with error code: {:#04X}. \
         See nestest.txt for failure code meanings.",
        unofficial_result
    );
}
