use super::*;

#[test]
fn arm_fetch_reads_32_bits_and_advances_pc() {
    let bus = bus_with_rom(&[0x78, 0x56, 0x34, 0xEA]);
    let mut cpu = Cpu::new();
    cpu.reset();
    let fetched = cpu.fetch_decode_stub(&bus);
    assert_eq!(fetched.instruction_set, InstructionSet::Arm);
    assert_eq!(fetched.width_bytes, 4);
    assert_eq!(fetched.raw, 0xEA34_5678);
    assert_eq!(cpu.pc(), RESET_VECTOR + 4);
    assert!(matches!(
        fetched.decoded,
        DecodedInstruction::Arm {
            class: ArmInstructionClass::Branch,
            ..
        }
    ));
}

#[test]
fn thumb_fetch_reads_16_bits_and_advances_pc() {
    let bus = bus_with_rom(&[0x00, 0xE0]);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    let fetched = cpu.fetch_decode_stub(&bus);
    assert_eq!(fetched.instruction_set, InstructionSet::Thumb);
    assert_eq!(fetched.width_bytes, 2);
    assert_eq!(fetched.raw, 0xE000);
    assert_eq!(cpu.pc(), RESET_VECTOR + 2);
    assert!(matches!(
        fetched.decoded,
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::UnconditionalBranch
        }
    ));
}

#[test]
fn cpu_data_reads_from_bios_outside_bios_return_post_startup_latch() {
    let mut bus = bus_with_rom(&[0x00, 0x00, 0xA0, 0xE1]); // mov r0, r0
    let mut cpu = Cpu::new();
    cpu.reset();

    cpu.fetch_decode_stub(&bus);

    assert_eq!(bus.read32(0x0000_0000), 0);
    assert_eq!(
        cpu.cpu_read32(&mut bus, 0x0000_0000),
        POST_STARTUP_BIOS_READ_LATCH
    );
    assert_eq!(
        cpu.cpu_read16(&mut bus, 0x0000_0002),
        (POST_STARTUP_BIOS_READ_LATCH >> 16) as u16
    );
    assert_eq!(
        cpu.cpu_read8(&mut bus, 0x0000_0001),
        (POST_STARTUP_BIOS_READ_LATCH >> 8) as u8
    );
}

#[test]
fn cpu_data_reads_from_bios_while_inside_bios_see_bios_stub() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.last_fetch = Some(FetchedInstruction {
        pc: 0x0000_0018,
        raw: bus.read32(0x0000_0018),
        instruction_set: InstructionSet::Arm,
        width_bytes: 4,
        fetch_cycles: 1,
        decoded: DecodedInstruction::Arm {
            condition: 0xE,
            class: ArmInstructionClass::Branch,
        },
    });

    let expected = bus.read32(0x0000_0018);
    assert_eq!(cpu.cpu_read32(&mut bus, 0x0000_0018), expected);
}

#[test]
fn sequential_pipeline_fetch_uses_current_waitcnt() {
    let rom = [0xE3A0_0001u32, 0xE3A0_1002, 0xE3A0_2003, 0xE3A0_3004]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let mut bus = bus_with_rom(&rom);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.step(&mut bus);
    bus.write16(0x0400_0204, 1 << 4);

    let fetched = cpu.step(&mut bus).unwrap();
    let expected = crate::hardware::timing::instruction_fetch_cycles_with_waitcnt(
        RESET_VECTOR + 12,
        4,
        true,
        bus.waitcnt(),
    );

    assert_eq!(fetched.pc, RESET_VECTOR + 4);
    assert_eq!(fetched.fetch_cycles, expected);
    assert_eq!(cpu.pipeline_state().entries[1].pc, RESET_VECTOR + 12);
}

#[cfg(feature = "profiling")]
#[test]
fn instruction_fetch_profiling_classifies_descriptor_opportunities() {
    let mut bus = bus_with_rom(&vec![0; 0x100]);
    let mut cpu = Cpu::new();
    let cases = [
        (0x0000_0000, InstructionSet::Arm, 4, false),
        (0x0200_0000, InstructionSet::Arm, 4, true),
        (0x0300_0000, InstructionSet::Thumb, 2, false),
        (0x0800_0000, InstructionSet::Arm, 4, true),
        (0x0A00_0000, InstructionSet::Thumb, 2, true),
        (0x0C00_0000, InstructionSet::Arm, 4, false),
        (0x0400_0000, InstructionSet::Arm, 4, true),
        (0x0800_0100, InstructionSet::Arm, 4, true),
    ];
    for (pc, instruction_set, width, sequential) in cases {
        cpu.profile_instruction_fetch(&bus, pc, instruction_set, width, sequential);
    }
    bus.debug_trace_enabled = true;
    bus.debug_trace_reads = true;
    cpu.profile_instruction_fetch(&bus, 0x0800_0000, InstructionSet::Arm, 4, true);

    assert_eq!(cpu.profiling.instruction_fetches, 9);
    assert_eq!(cpu.profiling.instruction_fetch_modes, [7, 2]);
    assert_eq!(cpu.profiling.instruction_fetch_accesses, [3, 6]);
    assert_eq!(
        cpu.profiling.instruction_fetch_regions,
        [1, 1, 1, 3, 1, 1, 1]
    );
    assert_eq!(cpu.profiling.instruction_fetch_descriptor_compatible, 5);
    assert_eq!(cpu.profiling.instruction_fetch_fallbacks, [0, 0, 1, 2, 1]);
    assert_eq!(cpu.profiling.instruction_fetch_waitcnt_changes, 0);
}

#[cfg(feature = "profiling")]
#[test]
fn instruction_fetch_profiling_classifies_cartridge_fallbacks_and_waitcnt_changes() {
    let mut eeprom_rom = vec![0; 0xD0];
    eeprom_rom[0x20..0x28].copy_from_slice(b"EEPROM_V");
    eeprom_rom[0xA0..0xA4].copy_from_slice(b"TEST");
    eeprom_rom[0xB2] = 0x96;
    let eeprom_bus = Bus::new(Cartridge::load(&eeprom_rom).unwrap(), 48_000);

    let mut rtc_rom = vec![0; 0xD0];
    rtc_rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rtc_rom[0xAC..0xAF].copy_from_slice(b"BPE");
    rtc_rom[0xB2] = 0x96;
    let mut rtc_bus = Bus::new(Cartridge::load(&rtc_rom).unwrap(), 48_000);

    let mut cpu = Cpu::new();
    cpu.profile_instruction_fetch(&eeprom_bus, 0x0D00_0000, InstructionSet::Thumb, 2, true);
    rtc_bus.write16(0x0400_0204, 1 << 4);
    cpu.profile_instruction_fetch(&rtc_bus, 0x0800_00C4, InstructionSet::Arm, 4, true);
    rtc_bus.write16(0x0400_0204, (1 << 14) | (1 << 4));
    cpu.profile_instruction_fetch(&rtc_bus, 0x0800_00C4, InstructionSet::Arm, 4, true);

    assert_eq!(cpu.profiling.instruction_fetches, 3);
    assert_eq!(cpu.profiling.instruction_fetch_modes, [2, 1]);
    assert_eq!(cpu.profiling.instruction_fetch_accesses, [0, 3]);
    assert_eq!(
        cpu.profiling.instruction_fetch_regions,
        [0, 0, 0, 2, 0, 1, 0]
    );
    assert_eq!(cpu.profiling.instruction_fetch_descriptor_compatible, 0);
    assert_eq!(cpu.profiling.instruction_fetch_fallbacks, [1, 2, 0, 0, 0]);
    assert_eq!(cpu.profiling.instruction_fetch_waitcnt_changes, 1);
}
