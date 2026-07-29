use super::*;

#[test]
fn swi_vblank_intr_wait_returns_if_vblank_already_pending() {
    let mut bus = bus_with_rom(&0xDF05_u16.to_le_bytes()); // swi 5
    bus.write16(0x0400_0200, 1);
    bus.io[0x202] = 1;
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;

    cpu.step(&mut bus);

    assert_eq!(cpu.state, CpuState::Running);
    assert_eq!(cpu.pc(), RESET_VECTOR + 2);
}

#[test]
fn swi_vblank_intr_wait_discards_old_bios_flag_then_halts() {
    let mut bus = bus_with_rom(&0xDF05_u16.to_le_bytes()); // swi 5
    bus.write16(0x0300_7FF8, 1);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;

    cpu.step(&mut bus);

    assert_eq!(bus.read16(0x0300_7FF8), 0);
    assert_eq!(bus.read16(0x0400_0208), 1);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn swi_sound_bias_sets_level_and_preserves_pwm_bits() {
    let mut bus = bus_with_rom(&[]);
    bus.write16(0x0400_0088, 0x8000);
    let mut cpu = Cpu::new();
    cpu.reset();

    cpu.regs[0] = 1;
    cpu.execute_software_interrupt(&mut bus, 0x19);
    assert_eq!(bus.read16(0x0400_0088), 0x8200);

    cpu.regs[0] = 0;
    cpu.execute_software_interrupt(&mut bus, 0x19);
    assert_eq!(bus.read16(0x0400_0088), 0x8000);
}

#[test]
fn swi_div_by_zero_matches_bios_scratch_registers() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0;
    cpu.regs[1] = 0;

    cpu.execute_software_interrupt(&mut bus, 0x06);

    assert_eq!(cpu.regs[0], 1);
    assert_eq!(cpu.regs[1], 0);
    assert_eq!(cpu.regs[3], 1);
}

#[test]
fn swi_div_int_min_by_negative_one_matches_bios_scratch_registers() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x8000_0000;
    cpu.regs[1] = 0xFFFF_FFFF;

    cpu.execute_software_interrupt(&mut bus, 0x06);

    assert_eq!(cpu.regs[0], 0x8000_0000);
    assert_eq!(cpu.regs[1], 0);
    assert_eq!(cpu.regs[3], 0x8000_0000);
}

#[test]
fn swi_arc_tan_matches_bios_polynomial_and_scratch_registers() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x4000;

    cpu.execute_software_interrupt(&mut bus, 0x09);

    assert_eq!(cpu.regs[0], 0x2000);
    assert_eq!(cpu.regs[1], 0xFFFF_C000);
    assert_eq!(cpu.regs[3], 0x8000);
}

#[test]
fn swi_arc_tan2_matches_bios_quadrants_and_scratch_registers() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0xFFFF_FFFF;
    cpu.regs[1] = 1;

    cpu.execute_software_interrupt(&mut bus, 0x0A);

    assert_eq!(cpu.regs[0], 0x6000);
    assert_eq!(cpu.regs[1], 0xFFFF_C000);
    assert_eq!(cpu.regs[3], 0x170);
}

#[test]
fn swi_obj_affine_set_writes_contiguous_identity_params() {
    let mut bus = bus_with_rom(&[]);
    bus.write16(0x0200_0000, 0x0100);
    bus.write16(0x0200_0002, 0x0100);
    bus.write16(0x0200_0004, 0x0000);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = 1;
    cpu.regs[3] = 2;

    cpu.execute_software_interrupt(&mut bus, 0x0F);

    assert_eq!(bus.read16(0x0300_0000), 0x0100);
    assert_eq!(bus.read16(0x0300_0002), 0x0000);
    assert_eq!(bus.read16(0x0300_0004), 0x0000);
    assert_eq!(bus.read16(0x0300_0006), 0x0100);
    assert_eq!(cpu.regs[0], 0x0200_0006);
    assert_eq!(cpu.regs[1], 0x0300_0008);
}

#[test]
fn swi_obj_affine_set_respects_oam_spacing_and_ignores_angle_fraction() {
    let mut bus = bus_with_rom(&[]);
    bus.write16(0x0200_0000, 0x0100);
    bus.write16(0x0200_0002, 0x0100);
    bus.write16(0x0200_0004, 0x40FF);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = 1;
    cpu.regs[3] = 8;

    cpu.execute_software_interrupt(&mut bus, 0x0F);

    assert_eq!(bus.read16(0x0300_0000), 0x0000);
    assert_eq!(bus.read16(0x0300_0008), 0xFF00);
    assert_eq!(bus.read16(0x0300_0010), 0x0100);
    assert_eq!(bus.read16(0x0300_0018), 0x0000);
    assert_eq!(cpu.regs[0], 0x0200_0006);
    assert_eq!(cpu.regs[1], 0x0300_0020);
}

#[test]
fn swi_bg_affine_set_writes_identity_params_and_origin_start() {
    let mut bus = bus_with_rom(&[]);
    bus.write32(0x0200_0000, 120 << 8);
    bus.write32(0x0200_0004, 80 << 8);
    bus.write16(0x0200_0008, 120);
    bus.write16(0x0200_000A, 80);
    bus.write16(0x0200_000C, 0x0100);
    bus.write16(0x0200_000E, 0x0100);
    bus.write16(0x0200_0010, 0x0000);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = 1;

    cpu.execute_software_interrupt(&mut bus, 0x0E);

    assert_eq!(bus.read16(0x0300_0000), 0x0100);
    assert_eq!(bus.read16(0x0300_0002), 0x0000);
    assert_eq!(bus.read16(0x0300_0004), 0x0000);
    assert_eq!(bus.read16(0x0300_0006), 0x0100);
    assert_eq!(bus.read32(0x0300_0008), 0);
    assert_eq!(bus.read32(0x0300_000C), 0);
    assert_eq!(cpu.regs[0], 0x0200_0014);
    assert_eq!(cpu.regs[1], 0x0300_0010);
}

#[test]
fn swi_cpu_set_copies_halfwords() {
    let mut bus = bus_with_rom(&[]);
    bus.write16(0x0200_0000, 0x1234);
    bus.write16(0x0200_0002, 0x5678);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = 2;

    cpu.execute_software_interrupt(&mut bus, 0x0B);

    assert_eq!(bus.read16(0x0300_0000), 0x1234);
    assert_eq!(bus.read16(0x0300_0002), 0x5678);
}

#[test]
fn swi_cpu_set_halfword_from_odd_source_copies_zero_extended_odd_bytes() {
    let mut bus = bus_with_rom(&[]);
    bus.write32(0x0200_0000, 0xCAFE_BABE);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0001;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = 2;

    cpu.execute_software_interrupt(&mut bus, 0x0B);

    assert_eq!(bus.read32(0x0300_0000), 0x00CA_00BA);
    assert_eq!(cpu.regs[0], 0x0200_0005);
    assert_eq!(cpu.regs[1], 0x0300_0004);
}

#[test]
fn swi_cpu_set_invalid_halfword_source_reads_zero() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0100_0000;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = 2;

    cpu.execute_software_interrupt(&mut bus, 0x0B);

    assert_eq!(bus.read32(0x0300_0000), 0);
}

#[test]
fn swi_cpu_set_invalid_word_source_reads_zero() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0100_0000;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = (1 << 26) | 1;

    cpu.execute_software_interrupt(&mut bus, 0x0B);

    assert_eq!(bus.read32(0x0300_0000), 0);
}

#[test]
fn swi_cpu_fast_set_invalid_word_source_reads_zero() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0100_0000;
    cpu.regs[1] = 0x0300_0000;
    cpu.regs[2] = 1;

    cpu.execute_software_interrupt(&mut bus, 0x0C);

    for offset in (0..32).step_by(4) {
        assert_eq!(bus.read32(0x0300_0000 + offset), 0);
    }
}

#[test]
fn swi_lz77_uncomp_expands_backrefs() {
    let mut bus = bus_with_rom(&[]);
    let data = [0x10, 0x06, 0x00, 0x00, 0x10, b'A', b'B', b'C', 0x00, 0x02];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;

    cpu.execute_software_interrupt(&mut bus, 0x11);

    let out: Vec<u8> = (0..6).map(|i| bus.read8(0x0300_0000 + i)).collect();
    assert_eq!(out, b"ABCABC");
}

#[test]
fn arm_swi_dispatch_uses_gba_function_byte() {
    let mut bus = bus_with_rom(&0xEF11_0000_u32.to_le_bytes());
    let data = [0x10, 0x03, 0x00, 0x00, 0x00, b'N', b'E', b'S'];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_7400;

    cpu.step(&mut bus);

    let out: Vec<u8> = (0..3).map(|i| bus.read8(0x0300_7400 + i)).collect();
    assert_eq!(out, b"NES");
    assert_eq!(cpu.regs[0], 0x0200_0008);
    assert_eq!(cpu.regs[1], 0x0300_7403);
}

#[test]
fn swi_lz77_vram_variant_writes_halfwords() {
    let mut bus = bus_with_rom(&[]);
    let data = [0x10, 0x06, 0x00, 0x00, 0x10, b'A', b'B', b'C', 0x00, 0x02];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0600_0000;

    cpu.execute_software_interrupt(&mut bus, 0x12);

    let out: Vec<u8> = (0..6).map(|i| bus.read8(0x0600_0000 + i)).collect();
    assert_eq!(out, b"ABCABC");
}

#[test]
fn swi_rl_uncomp_expands_runs() {
    let mut bus = bus_with_rom(&[]);
    let data = [0x30, 0x04, 0x00, 0x00, 0x81, 0x7F];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;

    cpu.execute_software_interrupt(&mut bus, 0x14);

    let out: Vec<u8> = (0..4).map(|i| bus.read8(0x0300_0000 + i)).collect();
    assert_eq!(out, [0x7F; 4]);
}

#[test]
fn swi_rl_vram_variant_writes_halfwords() {
    let mut bus = bus_with_rom(&[]);
    let data = [0x30, 0x04, 0x00, 0x00, 0x03, 1, 2, 3, 4];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0600_0000;

    cpu.execute_software_interrupt(&mut bus, 0x15);

    let out: Vec<u8> = (0..4).map(|i| bus.read8(0x0600_0000 + i)).collect();
    assert_eq!(out, [1, 2, 3, 4]);
}

#[test]
fn swi_bit_unpack_writes_words() {
    let mut bus = bus_with_rom(&[]);
    for (i, value) in [1, 2, 3, 4].into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    bus.write16(0x0200_0100, 4);
    bus.write8(0x0200_0102, 8);
    bus.write8(0x0200_0103, 8);
    bus.write32(0x0200_0104, 0);
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0600_0000;
    cpu.regs[2] = 0x0200_0100;

    cpu.execute_software_interrupt(&mut bus, 0x10);

    let out: Vec<u8> = (0..4).map(|i| bus.read8(0x0600_0000 + i)).collect();
    assert_eq!(out, [1, 2, 3, 4]);
}

#[test]
fn swi_huff_uncomp_decodes_terminal_tree() {
    let mut bus = bus_with_rom(&[]);
    let data = [
        0x28, 0x03, 0x00, 0x00, // 8-bit Huffman, 3 output bytes
        0x01, // four-byte tree table
        0xC0, b'A', b'B', // root with terminal child 0/1
        0x00, 0x00, 0x00, 0x40, // bits 0,1,0 in MSB-first order
    ];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;

    cpu.execute_software_interrupt(&mut bus, 0x13);

    let out: Vec<u8> = (0..3).map(|i| bus.read8(0x0300_0000 + i)).collect();
    assert_eq!(out, b"ABA");
}

#[test]
fn swi_diff_8bit_unfilter_expands_wrapping_deltas() {
    let mut bus = bus_with_rom(&[]);
    let data = [0x81, 0x04, 0x00, 0x00, 10, 1, 0xFF, 2];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;

    cpu.execute_software_interrupt(&mut bus, 0x16);

    let out: Vec<u8> = (0..4).map(|i| bus.read8(0x0300_0000 + i)).collect();
    assert_eq!(out, [10, 11, 10, 12]);
}

#[test]
fn swi_diff_8bit_vram_variant_writes_halfwords() {
    let mut bus = bus_with_rom(&[]);
    let data = [0x81, 0x04, 0x00, 0x00, 10, 1, 0xFF, 2];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0600_0000;

    cpu.execute_software_interrupt(&mut bus, 0x17);

    let out: Vec<u8> = (0..4).map(|i| bus.read8(0x0600_0000 + i)).collect();
    assert_eq!(out, [10, 11, 10, 12]);
}

#[test]
fn swi_diff_16bit_unfilter_expands_halfword_deltas() {
    let mut bus = bus_with_rom(&[]);
    let data = [
        0x82, 0x06, 0x00, 0x00, // 16-bit diff, 6 output bytes
        0x00, 0x10, 0x01, 0x00, 0xFF, 0xFF,
    ];
    for (i, value) in data.into_iter().enumerate() {
        bus.write8(0x0200_0000 + i as u32, value);
    }
    let mut cpu = Cpu::new();
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0300_0000;

    cpu.execute_software_interrupt(&mut bus, 0x18);

    assert_eq!(bus.read16(0x0300_0000), 0x1000);
    assert_eq!(bus.read16(0x0300_0002), 0x1001);
    assert_eq!(bus.read16(0x0300_0004), 0x1000);
}

#[test]
fn hle_swi_sets_protected_bios_latch_for_following_game_reads() {
    let mut bus = bus_with_rom(&[0x00, 0x00, 0xA0, 0xE1]); // mov r0, r0
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.fetch_decode_stub(&bus);

    cpu.execute_software_interrupt(&mut bus, 0x06);

    assert_eq!(cpu.cpu_read32(&bus, 0x0000_0000), POST_SWI_BIOS_READ_LATCH);
    assert_eq!(cpu.cpu_read16(&bus, 0x0000_0002), 0xE3A0);
}

#[test]
fn hle_swi_wait_restores_post_swi_latch_at_return_pc_after_irq_path() {
    let mut bus = bus_with_rom(&[
        0x05, 0xDF, // swi 5
        0xC0, 0x46, // nop
    ]);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;

    cpu.step(&mut bus);
    assert_eq!(cpu.state, CpuState::Halted);
    let return_pc = cpu.swi_wait_return_pc.expect("wait return PC");

    cpu.bios_protected_read_latch = 0xE55E_C002;
    cpu.set_pc(return_pc);
    cpu.resume();
    cpu.fetch_decode_stub(&bus);

    assert_eq!(cpu.cpu_read32(&bus, 0x0000_0000), POST_SWI_BIOS_READ_LATCH);
}
