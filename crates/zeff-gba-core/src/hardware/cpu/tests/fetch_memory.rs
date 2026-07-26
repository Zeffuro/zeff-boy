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
    let bus = bus_with_rom(&[0x00, 0x00, 0xA0, 0xE1]); // mov r0, r0
    let mut cpu = Cpu::new();
    cpu.reset();

    cpu.fetch_decode_stub(&bus);

    assert_eq!(bus.read32(0x0000_0000), 0);
    assert_eq!(
        cpu.cpu_read32(&bus, 0x0000_0000),
        POST_STARTUP_BIOS_READ_LATCH
    );
    assert_eq!(
        cpu.cpu_read16(&bus, 0x0000_0002),
        (POST_STARTUP_BIOS_READ_LATCH >> 16) as u16
    );
    assert_eq!(
        cpu.cpu_read8(&bus, 0x0000_0001),
        (POST_STARTUP_BIOS_READ_LATCH >> 8) as u8
    );
}

#[test]
fn cpu_data_reads_from_bios_while_inside_bios_see_bios_stub() {
    let bus = bus_with_rom(&[]);
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

    assert_eq!(cpu.cpu_read32(&bus, 0x0000_0018), bus.read32(0x0000_0018));
}
