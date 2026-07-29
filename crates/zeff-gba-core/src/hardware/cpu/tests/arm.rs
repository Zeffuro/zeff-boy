use super::*;

#[test]
fn arm_branch_uses_pc_plus_8_base_and_refills_pipeline() {
    let mut bus = bus_with_rom(&0xEA00_0000_u32.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();

    cpu.step(&mut bus);

    assert_eq!(cpu.pc(), RESET_VECTOR + 8);
    assert!(!cpu.next_fetch_sequential);
}

#[test]
fn arm_branch_with_link_sets_lr_to_following_instruction() {
    let mut bus = bus_with_rom(&0xEB00_0001_u32.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[14], RESET_VECTOR + 4);
    assert_eq!(cpu.pc(), RESET_VECTOR + 12);
}

#[test]
fn arm_stm_writeback_base_in_list_stores_new_base_when_not_first_register() {
    let mut bus = bus_with_rom(&0xE8A2_0005_u32.to_le_bytes()); // stmia r2!, {r0, r2}
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x1111_2222;
    cpu.regs[2] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(bus.read32(0x0200_0000), 0x1111_2222);
    assert_eq!(bus.read32(0x0200_0004), 0x0200_0008);
    assert_eq!(cpu.regs[2], 0x0200_0008);
}

#[test]
fn arm_ldm_writeback_is_suppressed_when_base_is_loaded() {
    let mut bus = bus_with_rom(&0xE8B2_0005_u32.to_le_bytes()); // ldmia r2!, {r0, r2}
    bus.write32(0x0200_0000, 0x1111_2222);
    bus.write32(0x0200_0004, 0x3333_4444);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[2] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x1111_2222);
    assert_eq!(cpu.regs[2], 0x3333_4444);
}

#[test]
fn arm_empty_ldmia_loads_pc_and_adds_0x40_to_base() {
    let mut bus = bus_with_rom(&0xE8B0_0000_u32.to_le_bytes()); // ldmia r0!, {}
    bus.write32(0x0200_0000, RESET_VECTOR + 0x20);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.pc(), RESET_VECTOR + 0x20);
    assert_eq!(cpu.regs[0], 0x0200_0040);
}

#[test]
fn arm_empty_stmdb_stores_pc_and_subtracts_0x40_from_base() {
    let mut bus = bus_with_rom(&0xE920_0000_u32.to_le_bytes()); // stmdb r0!, {}
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x0200_0040;

    cpu.step(&mut bus);

    assert_eq!(bus.read32(0x0200_0000), RESET_VECTOR + 12);
    assert_eq!(cpu.regs[0], 0x0200_0000);
}

#[test]
fn arm_condition_can_skip_branch() {
    let mut bus = bus_with_rom(&0x0A00_0000_u32.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr &= !CPSR_ZERO;

    cpu.step(&mut bus);

    assert_eq!(cpu.pc(), RESET_VECTOR + 4);
    assert!(cpu.next_fetch_sequential);
}

#[test]
fn arm_data_processing_and_load_store_execute() {
    let mut rom = Vec::new();
    for op in [
        0xE3A0_0001_u32, // mov r0, #1
        0xE280_1002,     // add r1, r0, #2
        0xE58D_1000,     // str r1, [sp]
        0xE59D_2000,     // ldr r2, [sp]
        0xE352_0003,     // cmp r2, #3
    ] {
        rom.extend_from_slice(&op.to_le_bytes());
    }
    let mut bus = bus_with_rom(&rom);
    let mut cpu = Cpu::new();
    cpu.reset();

    for _ in 0..5 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.regs[0], 1);
    assert_eq!(cpu.regs[1], 3);
    assert_eq!(cpu.regs[2], 3);
    assert_ne!(cpu.cpsr & CPSR_ZERO, 0);
}

#[test]
fn arm_adc_overflow_includes_carry_in() {
    let mut bus = bus_with_rom(&0xE2B0_0001_u32.to_le_bytes()); // adcs r0, r0, #1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x7FFF_FFFE;
    cpu.cpsr |= CPSR_CARRY;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x8000_0000);
    assert_ne!(cpu.cpsr & CPSR_OVERFLOW, 0);
    assert_ne!(cpu.cpsr & CPSR_NEGATIVE, 0);
    assert_eq!(cpu.cpsr & CPSR_CARRY, 0);
}

#[test]
fn arm_sbc_overflow_uses_original_rhs_not_borrow_adjusted_rhs() {
    let mut bus = bus_with_rom(&0xE0D0_0001_u32.to_le_bytes()); // sbcs r0, r0, r1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0;
    cpu.regs[1] = 0x7FFF_FFFF;
    cpu.cpsr &= !CPSR_CARRY;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x8000_0000);
    assert_ne!(cpu.cpsr & CPSR_NEGATIVE, 0);
    assert_eq!(cpu.cpsr & CPSR_OVERFLOW, 0);
    assert_eq!(cpu.cpsr & CPSR_CARRY, 0);
}

#[test]
fn arm_sbc_carry_uses_full_borrow_adjusted_subtrahend() {
    let mut bus = bus_with_rom(&0xE0D0_0001_u32.to_le_bytes()); // sbcs r0, r0, r1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0;
    cpu.regs[1] = 0xFFFF_FFFF;
    cpu.cpsr &= !CPSR_CARRY;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0);
    assert_ne!(cpu.cpsr & CPSR_ZERO, 0);
    assert_eq!(cpu.cpsr & CPSR_CARRY, 0);
}

#[test]
fn arm_smull_writes_signed_64_bit_product() {
    let mut bus = bus_with_rom(&0xE0C0_2091_u32.to_le_bytes()); // smull r2, r0, r1, r0
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0xFFFF_FFFE;
    cpu.regs[1] = 0x0001_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xFFFE_0000);
    assert_eq!(cpu.regs[0], 0xFFFF_FFFF);
}

#[test]
fn arm_umlal_accumulates_unsigned_64_bit_product() {
    let mut bus = bus_with_rom(&0xE0A3_2190_u32.to_le_bytes()); // umlal r2, r3, r0, r1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0xFFFF_FFFF;
    cpu.regs[1] = 2;
    cpu.regs[2] = 3;
    cpu.regs[3] = 4;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 1);
    assert_eq!(cpu.regs[3], 6);
}

#[test]
fn arm_umulls_sets_arm7tdmi_carry_side_effect() {
    let mut bus = bus_with_rom(&0xE093_2190_u32.to_le_bytes()); // umulls r2, r3, r0, r1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0xFFFF_FFFF;
    cpu.regs[1] = 0xFFFF_FFFF;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 1);
    assert_eq!(cpu.regs[3], 0xFFFF_FFFE);
    assert_ne!(cpu.cpsr & CPSR_NEGATIVE, 0);
    assert_ne!(cpu.cpsr & CPSR_CARRY, 0);
    assert_eq!(cpu.cpsr & CPSR_ZERO, 0);
}

#[test]
fn arm_smulls_zero_result_can_set_arm7tdmi_carry_side_effect() {
    let mut bus = bus_with_rom(&0xE0D3_2190_u32.to_le_bytes()); // smulls r2, r3, r0, r1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0;
    cpu.regs[1] = 0x8000_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0);
    assert_eq!(cpu.regs[3], 0);
    assert_ne!(cpu.cpsr & CPSR_ZERO, 0);
    assert_ne!(cpu.cpsr & CPSR_CARRY, 0);
}

#[test]
fn arm_swp_exchanges_word_with_memory() {
    let mut bus = bus_with_rom(&0xE101_2090_u32.to_le_bytes()); // swp r2, r0, [r1]
    bus.write32(0x0200_0000, 0x1122_3344);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0xAABB_CCDD;
    cpu.regs[1] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0x1122_3344);
    assert_eq!(bus.read32(0x0200_0000), 0xAABB_CCDD);
}

#[test]
fn arm_swpb_exchanges_byte_and_zero_extends_loaded_value() {
    let mut bus = bus_with_rom(&0xE141_2090_u32.to_le_bytes()); // swpb r2, r0, [r1]
    bus.write32(0x0200_0000, 0x1122_3344);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0xAABB_CC99;
    cpu.regs[1] = 0x0200_0001;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0x33);
    assert_eq!(bus.read32(0x0200_0000), 0x1122_9944);
}

#[test]
fn arm_ldr_with_writeback_to_same_register_keeps_loaded_value() {
    let mut bus = bus_with_rom(&0xE5B0_0004_u32.to_le_bytes()); // ldr r0, [r0, #4]!
    bus.write32(0x0200_0004, 0x1234_5678);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x1234_5678);
}

#[test]
fn arm_post_index_ldr_with_writeback_to_same_register_keeps_loaded_value() {
    let mut bus = bus_with_rom(&0xE490_0004_u32.to_le_bytes()); // ldr r0, [r0], #4
    bus.write32(0x0200_0000, 0x1234_5678);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x1234_5678);
}

#[test]
fn arm_ldrh_odd_address_rotates_aligned_halfword() {
    let mut bus = bus_with_rom(&0xE1D1_20B0_u32.to_le_bytes()); // ldrh r2, [r1]
    bus.write16(0x0200_0000, 0xBBAA);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[1] = 0x0200_0001;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xAA00_00BB);
}

#[test]
fn arm_ldrh_with_writeback_to_same_register_keeps_loaded_value() {
    let mut bus = bus_with_rom(&0xE1F0_00B4_u32.to_le_bytes()); // ldrh r0, [r0, #4]!
    bus.write16(0x0200_0004, 0x1234);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x0200_0000;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x1234);
}

#[test]
fn arm_ldrsh_odd_address_sign_extends_addressed_byte() {
    let mut bus = bus_with_rom(&0xE1D1_20F0_u32.to_le_bytes()); // ldrsh r2, [r1]
    bus.write16(0x0200_0000, 0x8000);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[1] = 0x0200_0001;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xFFFF_FF80);
}

#[test]
fn arm_ldm_with_s_bit_and_pc_restores_cpsr_from_spsr() {
    let mut bus = bus_with_rom(&0xE8FD_8001_u32.to_le_bytes()); // ldmia sp!, {r0, pc}^
    bus.write32(0x0300_7FE0, 0x1234_5678);
    bus.write32(0x0300_7FE4, 0x0800_1001);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.set_cpsr(CPSR_IRQ_DISABLE | 0x13);
    cpu.spsr = CPSR_THUMB | CPSR_IRQ_DISABLE | 0x1F;
    cpu.regs[13] = 0x0300_7FE0;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x1234_5678);
    assert!(cpu.thumb_state());
    assert_eq!(cpu.mode(), CpuMode::System);
    assert_eq!(cpu.pc(), 0x0800_1000);
    assert_eq!(cpu.banked_sp[BANK_SUPERVISOR], 0x0300_7FE8);
}

#[test]
fn arm_bad_cmp_with_rd_pc_restores_cpsr_from_spsr_without_branching() {
    let mut bus = bus_with_rom(&0xE15F_F000_u32.to_le_bytes()); // cmp pc, pc, r0
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[8] = 32;
    cpu.set_cpsr(CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x11);
    cpu.regs[0] = 1;
    cpu.regs[8] = 64;
    cpu.spsr = CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x1F;

    cpu.step(&mut bus);

    assert_eq!(cpu.mode(), CpuMode::System);
    assert_eq!(cpu.regs[8], 32);
    assert_eq!(cpu.pc(), RESET_VECTOR + 4);
    assert!(cpu.next_fetch_sequential);
}

#[test]
fn arm_bad_cmp_with_rd_pc_without_spsr_does_not_flush_pipeline() {
    let mut bus = bus_with_rom(&0xE15F_F000_u32.to_le_bytes()); // cmp pc, pc, r0
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 1;

    cpu.step(&mut bus);

    assert_eq!(cpu.mode(), CpuMode::System);
    assert_eq!(cpu.pc(), RESET_VECTOR + 4);
    assert!(cpu.next_fetch_sequential);
}

#[test]
fn fiq_mode_banks_r8_to_r12() {
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[8] = 32;
    cpu.regs[9] = 33;

    cpu.set_cpsr(CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x11);
    cpu.regs[8] = 64;
    cpu.regs[9] = 65;

    cpu.set_cpsr(CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x1F);
    assert_eq!(cpu.regs[8], 32);
    assert_eq!(cpu.regs[9], 33);

    cpu.set_cpsr(CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x11);
    assert_eq!(cpu.regs[8], 64);
    assert_eq!(cpu.regs[9], 65);
}

#[test]
fn arm_stm_with_s_bit_reads_user_sp_lr_bank() {
    let mut bus = bus_with_rom(&0xE8C0_6000_u32.to_le_bytes()); // stmia r0, {sp, lr}^
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.set_cpsr(CPSR_IRQ_DISABLE | 0x13);
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[13] = 0x0300_7FE0;
    cpu.regs[14] = 0xDEAD_BEEF;
    cpu.banked_sp[BANK_USER_SYSTEM] = 0x0300_1234;
    cpu.banked_lr[BANK_USER_SYSTEM] = 0x0800_5678;

    cpu.step(&mut bus);

    assert_eq!(bus.read32(0x0200_0000), 0x0300_1234);
    assert_eq!(bus.read32(0x0200_0004), 0x0800_5678);
}

#[test]
fn arm_stm_with_s_bit_reads_user_r8_bank_from_fiq_mode() {
    let mut bus = bus_with_rom(&0xE8C0_0100_u32.to_le_bytes()); // stmia r0, {r8}^
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[8] = 0x1111_2222;
    cpu.set_cpsr(CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x11);
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[8] = 0x3333_4444;

    cpu.step(&mut bus);

    assert_eq!(bus.read32(0x0200_0000), 0x1111_2222);
    assert_eq!(cpu.regs[8], 0x3333_4444);
}

#[test]
fn arm_branch_exchange_switches_to_thumb() {
    let mut bus = bus_with_rom(&0xE12F_FF1E_u32.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[14] = RESET_VECTOR + 9;

    cpu.step(&mut bus);

    assert!(cpu.thumb_state());
    assert_eq!(cpu.pc(), RESET_VECTOR + 8);
    assert!(!cpu.next_fetch_sequential);
}

#[test]
fn arm_prefetch_keeps_instruction_when_code_self_modifies_two_words_ahead() {
    let mut bus = bus_with_rom(&[]);
    let code_base = 0x0600_0260;
    let instructions = [
        0xE3A0_1000, // mov r1, #0
        0xE28F_E008, // add lr, pc, #8
        0xE51F_0010, // ldr r0, [pc, #-0x10]
        0xE58E_0000, // str r0, [lr]
        0xE3A0_10FF, // mov r1, #255
        0xE3A0_10FF, // mov r1, #255; overwritten while already prefetched
    ];
    for (index, instruction) in instructions.into_iter().enumerate() {
        bus.write32(code_base + index as u32 * 4, instruction);
    }
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.set_pc(code_base);

    for _ in 0..instructions.len() {
        cpu.step(&mut bus);
    }

    assert_eq!(bus.read32(code_base + 0x14), 0xE3A0_1000);
    assert_eq!(cpu.regs[1], 0xFF);
}

#[test]
fn arm_register_shift_reads_pc_operand_as_pc_plus_12() {
    let mut bus = bus_with_rom(&0xE1A0_011F_u32.to_le_bytes()); // mov r0, pc, lsl r1
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[1] = 0;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x0800_000C);
}

#[test]
fn arm_register_shift_reads_pc_shift_amount_as_pc_plus_12() {
    let mut bus = bus_with_rom(&0xE1A0_0F12_u32.to_le_bytes()); // mov r0, r2, lsl pc
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[2] = 1;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], 0x1000);
}

#[test]
fn arm_register_shift_reads_pc_operand1_as_pc_plus_12() {
    let mut bus = bus_with_rom(&0xE08F_0010_u32.to_le_bytes()); // add r0, pc, r0, lsl r0
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[0], RESET_VECTOR + 12);
}

#[test]
fn arm_mrs_msr_can_clear_irq_disable() {
    let mut rom = Vec::new();
    rom.extend_from_slice(&0xE10F_0000_u32.to_le_bytes()); // mrs r0, cpsr
    rom.extend_from_slice(&0xE3C0_0080_u32.to_le_bytes()); // bic r0, r0, #0x80
    rom.extend_from_slice(&0xE121_F000_u32.to_le_bytes()); // msr cpsr_c, r0
    let mut bus = bus_with_rom(&rom);
    let mut cpu = Cpu::new();
    cpu.reset();

    for _ in 0..3 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.cpsr & CPSR_IRQ_DISABLE, 0);
}
