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
