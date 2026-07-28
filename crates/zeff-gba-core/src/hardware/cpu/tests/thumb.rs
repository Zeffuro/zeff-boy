use super::*;

#[test]
fn thumb_unconditional_branch_uses_pc_plus_4_base() {
    let mut bus = bus_with_rom(&0xE000_u16.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;

    cpu.step(&mut bus);

    assert_eq!(cpu.pc(), RESET_VECTOR + 4);
    assert!(!cpu.next_fetch_sequential);
}

#[test]
fn thumb_conditional_branch_honors_cpsr_flags() {
    let mut bus = bus_with_rom(&0xD100_u16.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.cpsr &= !CPSR_ZERO;

    cpu.step(&mut bus);

    assert_eq!(cpu.pc(), RESET_VECTOR + 4);
    assert!(!cpu.next_fetch_sequential);
}

#[test]
fn thumb_immediate_and_sp_relative_load_store_execute() {
    let mut rom = Vec::new();
    for op in [
        0x2004_u16, // mov r0, #4
        0x3001,     // add r0, #1
        0x9000,     // str r0, [sp]
        0x9900,     // ldr r1, [sp]
    ] {
        rom.extend_from_slice(&op.to_le_bytes());
    }
    let mut bus = bus_with_rom(&rom);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;

    for _ in 0..4 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.regs[0], 5);
    assert_eq!(cpu.regs[1], 5);
}

#[test]
fn thumb_adc_overflow_includes_carry_in() {
    let mut bus = bus_with_rom(&0x4148_u16.to_le_bytes()); // adc r0, r1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB | CPSR_CARRY;
    cpu.regs[0] = 0x7FFF_FFFE;
    cpu.regs[1] = 1;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x8000_0000);
    assert_ne!(cpu.cpsr & CPSR_OVERFLOW, 0);
    assert_ne!(cpu.cpsr & CPSR_NEGATIVE, 0);
    assert_eq!(cpu.cpsr & CPSR_CARRY, 0);
}

#[test]
fn thumb_sp_relative_ldr_misaligned_address_rotates_word() {
    let mut bus = bus_with_rom(&0x9A01_u16.to_le_bytes()); // ldr r2, [sp, #4]
    bus.write32(0x0200_0004, 0x0000_00FF);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[13] = 0x0200_0001;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xFF00_0000);
}

#[test]
fn thumb_ldrh_odd_address_rotates_aligned_halfword() {
    let mut bus = bus_with_rom(&0x5A0A_u16.to_le_bytes()); // ldrh r2, [r1, r0]
    bus.write16(0x0200_0000, 0xBBAA);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[1] = 0x0200_0001;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xAA00_00BB);
}

#[test]
fn thumb_ldrsh_odd_address_sign_extends_addressed_byte() {
    let mut bus = bus_with_rom(&0x5E0A_u16.to_le_bytes()); // ldrsh r2, [r1, r0]
    bus.write16(0x0200_0000, 0x8000);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[1] = 0x0200_0001;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xFFFF_FF80);
}

#[test]
fn thumb_immediate_ldrh_odd_address_rotates_aligned_halfword() {
    let mut bus = bus_with_rom(&0x880A_u16.to_le_bytes()); // ldrh r2, [r1]
    bus.write16(0x0200_0000, 0xBBAA);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[1] = 0x0200_0001;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xAA00_00BB);
}

#[test]
fn thumb_ldmia_with_base_in_list_keeps_loaded_base() {
    let mut bus = bus_with_rom(&0xC916_u16.to_le_bytes()); // ldm r1, {r1, r2, r4}
    bus.write32(0x0200_0000, 0x0400_00D4);
    bus.write32(0x0200_0004, 0x0800_18B4);
    bus.write32(0x0200_0008, 0x8400_0050);

    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[1] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[1], 0x0400_00D4);
    assert_eq!(cpu.regs[2], 0x0800_18B4);
    assert_eq!(cpu.regs[4], 0x8400_0050);
}

#[test]
fn thumb_empty_stmia_stores_visible_pc_and_adds_0x40() {
    let mut bus = bus_with_rom(&0xC000_u16.to_le_bytes()); // stmia r0!, {}
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[0] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(bus.read32(0x0200_0000), RESET_VECTOR + 6);
    assert_eq!(cpu.regs[0], 0x0200_0040);
}

#[test]
fn thumb_stmia_base_in_list_after_first_stores_writeback_value() {
    let mut bus = bus_with_rom(&0xC10F_u16.to_le_bytes()); // stmia r1!, {r0-r3}
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[0] = 0x1111_1111;
    cpu.regs[1] = 0x0200_0000;
    cpu.regs[2] = 0x2222_2222;
    cpu.regs[3] = 0x3333_3333;

    cpu.step(&mut bus);

    assert_eq!(bus.read32(0x0200_0000), 0x1111_1111);
    assert_eq!(bus.read32(0x0200_0004), 0x0200_0010);
    assert_eq!(bus.read32(0x0200_0008), 0x2222_2222);
    assert_eq!(bus.read32(0x0200_000C), 0x3333_3333);
    assert_eq!(cpu.regs[1], 0x0200_0010);
}

#[test]
fn thumb_stmia_base_first_in_list_stores_original_base() {
    let mut bus = bus_with_rom(&0xC11E_u16.to_le_bytes()); // stmia r1!, {r1-r4}
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[1] = 0x0200_0000;
    cpu.regs[2] = 0x2222_2222;
    cpu.regs[3] = 0x3333_3333;
    cpu.regs[4] = 0x4444_4444;

    cpu.step(&mut bus);

    assert_eq!(bus.read32(0x0200_0000), 0x0200_0000);
    assert_eq!(bus.read32(0x0200_0004), 0x2222_2222);
    assert_eq!(bus.read32(0x0200_0008), 0x3333_3333);
    assert_eq!(bus.read32(0x0200_000C), 0x4444_4444);
    assert_eq!(cpu.regs[1], 0x0200_0010);
}

#[test]
fn thumb_long_branch_with_link_sets_target_and_return_address() {
    let mut rom = Vec::new();
    rom.extend_from_slice(&0xF000_u16.to_le_bytes());
    rom.extend_from_slice(&0xF801_u16.to_le_bytes());
    let mut bus = bus_with_rom(&rom);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;

    cpu.step(&mut bus);
    assert_eq!(cpu.regs[14], RESET_VECTOR + 4);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs[14], RESET_VECTOR + 5);
    assert_eq!(cpu.pc(), RESET_VECTOR + 6);
    assert!(!cpu.next_fetch_sequential);
}

#[test]
fn thumb_branch_exchange_can_return_to_arm() {
    let mut bus = bus_with_rom(&0x4770_u16.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[14] = RESET_VECTOR + 8;

    cpu.step(&mut bus);

    assert!(!cpu.thumb_state());
    assert_eq!(cpu.pc(), RESET_VECTOR + 8);
    assert!(!cpu.next_fetch_sequential);
}
