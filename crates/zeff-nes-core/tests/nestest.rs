use std::path::PathBuf;

fn nestest_rom_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../test-roms/nes-test-roms/other/nestest.nes");
    path
}

#[test]
fn nestest_official_opcodes_pass() {
    let rom_path = nestest_rom_path();
    if !rom_path.exists() {
        eprintln!(
            "Skipping nestest: ROM not found at {}",
            rom_path.display()
        );
        return;
    }

    let rom_data = std::fs::read(&rom_path).expect("failed to read nestest.nes");
    let mut emu = zeff_nes_core::emulator::Emulator::new(&rom_data, 48000.0)
        .expect("failed to create emulator");

    emu.set_cpu_pc(0xC000);

    for _ in 0..30_000 {
        emu.step_instruction();

        if emu.cpu_pc() == emu.last_opcode_pc() {
            break;
        }
    }

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
    let rom_path = nestest_rom_path();
    if !rom_path.exists() {
        eprintln!(
            "Skipping nestest: ROM not found at {}",
            rom_path.display()
        );
        return;
    }

    let rom_data = std::fs::read(&rom_path).expect("failed to read nestest.nes");
    let mut emu = zeff_nes_core::emulator::Emulator::new(&rom_data, 48000.0)
        .expect("failed to create emulator");

    emu.set_cpu_pc(0xC000);

    for _ in 0..30_000 {
        emu.step_instruction();
        if emu.cpu_pc() == emu.last_opcode_pc() {
            break;
        }
    }

    let unofficial_result = emu.bus().ram[0x03];
    assert_eq!(
        unofficial_result, 0x00,
        "nestest unofficial opcode tests failed with error code: {:#04X}. \
         See nestest.txt for failure code meanings.",
        unofficial_result
    );
}
