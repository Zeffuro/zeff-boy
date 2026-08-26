use std::path::PathBuf;

const FIXTURE_PRG_LEN: usize = 16 * 1024;
const FIXTURE_PRG_BASE: u16 = 0xC000;
const FIXTURE_RESULT_ADDR: usize = 0x00FA;

/// Builds a small, legal NROM image instead of embedding a third-party ROM.
///
/// This is deliberately an integration floor, not a replacement for nestest's
/// exhaustive opcode corpus below. It exercises the public emulator through
/// cartridge loading, reset-vector fetch, CPU instruction execution, and CPU
/// RAM access with a bounded pass-byte oracle.
fn source_generated_cpu_integration_rom() -> (Vec<u8>, u16) {
    macro_rules! emit {
        ($program:expr; $($byte:expr),+ $(,)?) => {
            $program.extend_from_slice(&[$($byte),+]);
        };
    }

    fn patch_absolute(program: &mut [u8], operand: usize, target: usize) {
        let address = FIXTURE_PRG_BASE
            .checked_add(u16::try_from(target).expect("fixture target fits in PRG ROM"))
            .expect("fixture target fits in CPU address space");
        program[operand..operand + 2].copy_from_slice(&address.to_le_bytes());
    }

    fn patch_relative(program: &mut [u8], operand: usize, target: usize) {
        let next_instruction = isize::try_from(operand + 1).expect("fixture offset");
        let target = isize::try_from(target).expect("fixture target");
        program[operand] =
            i8::try_from(target - next_instruction).expect("fixture branch is in range") as u8;
    }

    let mut program = Vec::new();
    let mut fail_branches = Vec::new();
    let mut failure_jumps = Vec::new();

    macro_rules! bne_fail {
        () => {{
            emit!(program; 0xD0, 0x00); // BNE failure
            fail_branches.push(program.len() - 1);
        }};
    }

    // Setup and arithmetic via zero-page RAM.
    emit!(program; 0x78, 0xD8); // SEI; CLD
    emit!(program; 0xA2, 0xFD, 0x9A); // LDX #$FD; TXS
    emit!(program; 0xA2, 0x03, 0x86, 0x10); // LDX #3; STX $10
    emit!(program; 0xA9, 0x20, 0x18, 0x65, 0x10, 0xC9, 0x23); // 0x20 + $10
    bne_fail!();
    emit!(program; 0x38, 0xE9, 0x03, 0xC9, 0x20); // SEC; SBC #3; CMP #$20
    bne_fail!();

    // Absolute,Y and (zero-page),Y addressing both resolve to CPU RAM.
    emit!(program; 0xA0, 0x02, 0xA9, 0x5A, 0x99, 0x00, 0x02); // STA $0200,Y
    emit!(program; 0xB9, 0x00, 0x02, 0xC9, 0x5A); // LDA $0200,Y; CMP #$5A
    bne_fail!();
    emit!(program; 0xA9, 0x00, 0x85, 0x20, 0xA9, 0x03, 0x85, 0x21); // pointer $0300
    emit!(program; 0xA0, 0x00, 0xA9, 0xA5, 0x91, 0x20); // STA ($20),Y
    emit!(program; 0xB1, 0x20, 0xC9, 0xA5); // LDA ($20),Y; CMP #$A5
    bne_fail!();

    // Stack, register transfers, and flag-dependent branches.
    emit!(program; 0x48, 0xA9, 0x00, 0x68, 0xC9, 0xA5); // PHA; LDA #0; PLA
    bne_fail!();
    emit!(program; 0xAA, 0xE8, 0x8A, 0xC9, 0xA6); // TAX; INX; TXA; CMP #$A6
    bne_fail!();
    emit!(program; 0xA9, 0x40, 0x85, 0x11, 0xA9, 0x00, 0x24, 0x11); // BIT $11
    emit!(program; 0x50, 0x00); // BVC failure (V must be set)
    fail_branches.push(program.len() - 1);
    emit!(program; 0xD0, 0x00); // BNE failure (Z must be set)
    fail_branches.push(program.len() - 1);
    emit!(program; 0xA9, 0x01, 0xD0, 0x00); // LDA #1; BNE branch_taken
    let branch_taken = program.len() - 1;
    emit!(program; 0x4C, 0x00, 0x00); // unreachable JMP failure
    failure_jumps.push(program.len() - 2);

    let branch_target = program.len();
    emit!(program; 0x20, 0x00, 0x00, 0xC0, 0x01); // JSR subroutine; CPY #1
    let subroutine_call = program.len() - 4;
    bne_fail!();
    emit!(program; 0xA9, 0x00, 0x85, FIXTURE_RESULT_ADDR as u8, 0x4C, 0x00, 0x00);
    let success_jump = program.len() - 2;

    let failure = program.len();
    emit!(program; 0xA9, 0xFF, 0x85, FIXTURE_RESULT_ADDR as u8, 0x4C, 0x00, 0x00);
    let failure_terminal_jump = program.len() - 2;

    let subroutine = program.len();
    emit!(program; 0xC8, 0x60); // INY; RTS

    let terminal = program.len();
    emit!(program; 0x4C, 0x00, 0x00); // JMP terminal
    let terminal_jump = program.len() - 2;

    for branch in fail_branches {
        patch_relative(&mut program, branch, failure);
    }
    patch_relative(&mut program, branch_taken, branch_target);
    for jump in failure_jumps {
        patch_absolute(&mut program, jump, failure);
    }
    patch_absolute(&mut program, subroutine_call, subroutine);
    patch_absolute(&mut program, success_jump, terminal);
    patch_absolute(&mut program, failure_terminal_jump, terminal);
    patch_absolute(&mut program, terminal_jump, terminal);

    let mut rom = vec![0u8; 16 + FIXTURE_PRG_LEN];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1; // One 16 KiB NROM PRG bank, mirrored at $8000..=$FFFF.
    rom[16..16 + program.len()].copy_from_slice(&program);
    let vectors = 16 + 0x3FFA;
    rom[vectors..vectors + 6].copy_from_slice(&[
        0x00, 0xC0, // NMI -> $C000
        0x00, 0xC0, // RESET -> $C000
        0x00, 0xC0, // IRQ -> $C000
    ]);
    (
        rom,
        FIXTURE_PRG_BASE + u16::try_from(terminal).expect("terminal fits in PRG ROM"),
    )
}

#[test]
fn source_generated_cpu_integration_floor_passes() {
    let (rom, terminal_pc) = source_generated_cpu_integration_rom();
    let mut emu = zeff_nes_core::emulator::Emulator::new(&rom, 48_000.0)
        .expect("source-generated NROM fixture should load");

    let reached_terminal = (0..1_000).any(|_| {
        emu.step_instruction();
        emu.last_opcode_pc() == terminal_pc
    });

    assert!(
        reached_terminal,
        "fixture did not reach its bounded terminal loop"
    );
    assert_eq!(
        emu.bus().ram[FIXTURE_RESULT_ADDR],
        0,
        "fixture reported a CPU/cartridge/bus integration failure"
    );
}

fn nestest_rom_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../test-roms/nes-test-roms/other/nestest.nes");
    path
}

fn run_nestest_automation() -> zeff_nes_core::emulator::Emulator {
    let rom_path = nestest_rom_path();
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
            return emu;
        }
    }

    panic!("nestest automation did not reach its final RTS");
}

#[test]
#[ignore = "requires test-roms/nes-test-roms/other/nestest.nes"]
fn nestest_official_opcodes_pass() {
    let emu = run_nestest_automation();

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
#[ignore = "requires test-roms/nes-test-roms/other/nestest.nes"]
fn nestest_unofficial_opcodes_pass() {
    let emu = run_nestest_automation();

    let unofficial_result = emu.bus().ram[0x03];
    assert_eq!(
        unofficial_result, 0x00,
        "nestest unofficial opcode tests failed with error code: {:#04X}. \
         See nestest.txt for failure code meanings.",
        unofficial_result
    );
}
