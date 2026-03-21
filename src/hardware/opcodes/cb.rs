use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;
use crate::hardware::opcodes::cycles::CB_CYCLE_TABLE;

pub(crate) fn unimplemented_cb_prefix_handler(cpu: &mut CPU, opcode: u8) {
    log::warn!(
        "Unimplemented CB opcode CB {:02X} at PC={:04X}",
        opcode,
        cpu.pc.wrapping_sub(1)
    );
}

pub(crate) fn execute_cb_prefix(cpu: &mut CPU, bus: &mut Bus) {
    let opcode = cpu.fetch8_timed(bus);

    match opcode {
        // RLC
        0x00 => crate::hardware::opcodes::bitwise::rlc_b(cpu, bus),
        0x01 => crate::hardware::opcodes::bitwise::rlc_c(cpu, bus),
        0x02 => crate::hardware::opcodes::bitwise::rlc_d(cpu, bus),
        0x03 => crate::hardware::opcodes::bitwise::rlc_e(cpu, bus),
        0x04 => crate::hardware::opcodes::bitwise::rlc_h(cpu, bus),
        0x05 => crate::hardware::opcodes::bitwise::rlc_l(cpu, bus),
        0x06 => crate::hardware::opcodes::bitwise::rlc_hl(cpu, bus),
        0x07 => crate::hardware::opcodes::bitwise::rlc_a(cpu, bus),

        // RRC
        0x08 => crate::hardware::opcodes::bitwise::rrc_b(cpu, bus),
        0x09 => crate::hardware::opcodes::bitwise::rrc_c(cpu, bus),
        0x0A => crate::hardware::opcodes::bitwise::rrc_d(cpu, bus),
        0x0B => crate::hardware::opcodes::bitwise::rrc_e(cpu, bus),
        0x0C => crate::hardware::opcodes::bitwise::rrc_h(cpu, bus),
        0x0D => crate::hardware::opcodes::bitwise::rrc_l(cpu, bus),
        0x0E => crate::hardware::opcodes::bitwise::rrc_hl(cpu, bus),
        0x0F => crate::hardware::opcodes::bitwise::rrc_a(cpu, bus),

        // RL
        0x10 => crate::hardware::opcodes::bitwise::rl_b(cpu, bus),
        0x11 => crate::hardware::opcodes::bitwise::rl_c(cpu, bus),
        0x12 => crate::hardware::opcodes::bitwise::rl_d(cpu, bus),
        0x13 => crate::hardware::opcodes::bitwise::rl_e(cpu, bus),
        0x14 => crate::hardware::opcodes::bitwise::rl_h(cpu, bus),
        0x15 => crate::hardware::opcodes::bitwise::rl_l(cpu, bus),
        0x16 => crate::hardware::opcodes::bitwise::rl_hl(cpu, bus),
        0x17 => crate::hardware::opcodes::bitwise::rl_a(cpu, bus),

        // RR
        0x18 => crate::hardware::opcodes::bitwise::rr_b(cpu, bus),
        0x19 => crate::hardware::opcodes::bitwise::rr_c(cpu, bus),
        0x1A => crate::hardware::opcodes::bitwise::rr_d(cpu, bus),
        0x1B => crate::hardware::opcodes::bitwise::rr_e(cpu, bus),
        0x1C => crate::hardware::opcodes::bitwise::rr_h(cpu, bus),
        0x1D => crate::hardware::opcodes::bitwise::rr_l(cpu, bus),
        0x1E => crate::hardware::opcodes::bitwise::rr_hl(cpu, bus),
        0x1F => crate::hardware::opcodes::bitwise::rr_a(cpu, bus),

        // SLA
        0x20 => crate::hardware::opcodes::bitwise::sla_b(cpu, bus),
        0x21 => crate::hardware::opcodes::bitwise::sla_c(cpu, bus),
        0x22 => crate::hardware::opcodes::bitwise::sla_d(cpu, bus),
        0x23 => crate::hardware::opcodes::bitwise::sla_e(cpu, bus),
        0x24 => crate::hardware::opcodes::bitwise::sla_h(cpu, bus),
        0x25 => crate::hardware::opcodes::bitwise::sla_l(cpu, bus),
        0x26 => crate::hardware::opcodes::bitwise::sla_hl(cpu, bus),
        0x27 => crate::hardware::opcodes::bitwise::sla_a(cpu, bus),

        // SRA
        0x28 => crate::hardware::opcodes::bitwise::sra_b(cpu, bus),
        0x29 => crate::hardware::opcodes::bitwise::sra_c(cpu, bus),
        0x2A => crate::hardware::opcodes::bitwise::sra_d(cpu, bus),
        0x2B => crate::hardware::opcodes::bitwise::sra_e(cpu, bus),
        0x2C => crate::hardware::opcodes::bitwise::sra_h(cpu, bus),
        0x2D => crate::hardware::opcodes::bitwise::sra_l(cpu, bus),
        0x2E => crate::hardware::opcodes::bitwise::sra_hl(cpu, bus),
        0x2F => crate::hardware::opcodes::bitwise::sra_a(cpu, bus),

        // SWAP
        0x30 => crate::hardware::opcodes::bitwise::swap_b(cpu, bus),
        0x31 => crate::hardware::opcodes::bitwise::swap_c(cpu, bus),
        0x32 => crate::hardware::opcodes::bitwise::swap_d(cpu, bus),
        0x33 => crate::hardware::opcodes::bitwise::swap_e(cpu, bus),
        0x34 => crate::hardware::opcodes::bitwise::swap_h(cpu, bus),
        0x35 => crate::hardware::opcodes::bitwise::swap_l(cpu, bus),
        0x36 => crate::hardware::opcodes::bitwise::swap_hl(cpu, bus),
        0x37 => crate::hardware::opcodes::bitwise::swap_a(cpu, bus),

        // SRL
        0x38 => crate::hardware::opcodes::bitwise::srl_b(cpu, bus),
        0x39 => crate::hardware::opcodes::bitwise::srl_c(cpu, bus),
        0x3A => crate::hardware::opcodes::bitwise::srl_d(cpu, bus),
        0x3B => crate::hardware::opcodes::bitwise::srl_e(cpu, bus),
        0x3C => crate::hardware::opcodes::bitwise::srl_h(cpu, bus),
        0x3D => crate::hardware::opcodes::bitwise::srl_l(cpu, bus),
        0x3E => crate::hardware::opcodes::bitwise::srl_hl(cpu, bus),
        0x3F => crate::hardware::opcodes::bitwise::srl_a(cpu, bus),

        // BIT 0
        0x40 => crate::hardware::opcodes::bitwise::bit_0_b(cpu, bus),
        0x41 => crate::hardware::opcodes::bitwise::bit_0_c(cpu, bus),
        0x42 => crate::hardware::opcodes::bitwise::bit_0_d(cpu, bus),
        0x43 => crate::hardware::opcodes::bitwise::bit_0_e(cpu, bus),
        0x44 => crate::hardware::opcodes::bitwise::bit_0_h(cpu, bus),
        0x45 => crate::hardware::opcodes::bitwise::bit_0_l(cpu, bus),
        0x46 => crate::hardware::opcodes::bitwise::bit_0_hl(cpu, bus),
        0x47 => crate::hardware::opcodes::bitwise::bit_0_a(cpu, bus),

        // BIT 1
        0x48 => crate::hardware::opcodes::bitwise::bit_1_b(cpu, bus),
        0x49 => crate::hardware::opcodes::bitwise::bit_1_c(cpu, bus),
        0x4A => crate::hardware::opcodes::bitwise::bit_1_d(cpu, bus),
        0x4B => crate::hardware::opcodes::bitwise::bit_1_e(cpu, bus),
        0x4C => crate::hardware::opcodes::bitwise::bit_1_h(cpu, bus),
        0x4D => crate::hardware::opcodes::bitwise::bit_1_l(cpu, bus),
        0x4E => crate::hardware::opcodes::bitwise::bit_1_hl(cpu, bus),
        0x4F => crate::hardware::opcodes::bitwise::bit_1_a(cpu, bus),

        // BIT 2
        0x50 => crate::hardware::opcodes::bitwise::bit_2_b(cpu, bus),
        0x51 => crate::hardware::opcodes::bitwise::bit_2_c(cpu, bus),
        0x52 => crate::hardware::opcodes::bitwise::bit_2_d(cpu, bus),
        0x53 => crate::hardware::opcodes::bitwise::bit_2_e(cpu, bus),
        0x54 => crate::hardware::opcodes::bitwise::bit_2_h(cpu, bus),
        0x55 => crate::hardware::opcodes::bitwise::bit_2_l(cpu, bus),
        0x56 => crate::hardware::opcodes::bitwise::bit_2_hl(cpu, bus),
        0x57 => crate::hardware::opcodes::bitwise::bit_2_a(cpu, bus),

        // BIT 3
        0x58 => crate::hardware::opcodes::bitwise::bit_3_b(cpu, bus),
        0x59 => crate::hardware::opcodes::bitwise::bit_3_c(cpu, bus),
        0x5A => crate::hardware::opcodes::bitwise::bit_3_d(cpu, bus),
        0x5B => crate::hardware::opcodes::bitwise::bit_3_e(cpu, bus),
        0x5C => crate::hardware::opcodes::bitwise::bit_3_h(cpu, bus),
        0x5D => crate::hardware::opcodes::bitwise::bit_3_l(cpu, bus),
        0x5E => crate::hardware::opcodes::bitwise::bit_3_hl(cpu, bus),
        0x5F => crate::hardware::opcodes::bitwise::bit_3_a(cpu, bus),

        // BIT 4
        0x60 => crate::hardware::opcodes::bitwise::bit_4_b(cpu, bus),
        0x61 => crate::hardware::opcodes::bitwise::bit_4_c(cpu, bus),
        0x62 => crate::hardware::opcodes::bitwise::bit_4_d(cpu, bus),
        0x63 => crate::hardware::opcodes::bitwise::bit_4_e(cpu, bus),
        0x64 => crate::hardware::opcodes::bitwise::bit_4_h(cpu, bus),
        0x65 => crate::hardware::opcodes::bitwise::bit_4_l(cpu, bus),
        0x66 => crate::hardware::opcodes::bitwise::bit_4_hl(cpu, bus),
        0x67 => crate::hardware::opcodes::bitwise::bit_4_a(cpu, bus),

        // BIT 5
        0x68 => crate::hardware::opcodes::bitwise::bit_5_b(cpu, bus),
        0x69 => crate::hardware::opcodes::bitwise::bit_5_c(cpu, bus),
        0x6A => crate::hardware::opcodes::bitwise::bit_5_d(cpu, bus),
        0x6B => crate::hardware::opcodes::bitwise::bit_5_e(cpu, bus),
        0x6C => crate::hardware::opcodes::bitwise::bit_5_h(cpu, bus),
        0x6D => crate::hardware::opcodes::bitwise::bit_5_l(cpu, bus),
        0x6E => crate::hardware::opcodes::bitwise::bit_5_hl(cpu, bus),
        0x6F => crate::hardware::opcodes::bitwise::bit_5_a(cpu, bus),

        // BIT 6
        0x70 => crate::hardware::opcodes::bitwise::bit_6_b(cpu, bus),
        0x71 => crate::hardware::opcodes::bitwise::bit_6_c(cpu, bus),
        0x72 => crate::hardware::opcodes::bitwise::bit_6_d(cpu, bus),
        0x73 => crate::hardware::opcodes::bitwise::bit_6_e(cpu, bus),
        0x74 => crate::hardware::opcodes::bitwise::bit_6_h(cpu, bus),
        0x75 => crate::hardware::opcodes::bitwise::bit_6_l(cpu, bus),
        0x76 => crate::hardware::opcodes::bitwise::bit_6_hl(cpu, bus),
        0x77 => crate::hardware::opcodes::bitwise::bit_6_a(cpu, bus),

        // BIT 7
        0x78 => crate::hardware::opcodes::bitwise::bit_7_b(cpu, bus),
        0x79 => crate::hardware::opcodes::bitwise::bit_7_c(cpu, bus),
        0x7A => crate::hardware::opcodes::bitwise::bit_7_d(cpu, bus),
        0x7B => crate::hardware::opcodes::bitwise::bit_7_e(cpu, bus),
        0x7C => crate::hardware::opcodes::bitwise::bit_7_h(cpu, bus),
        0x7D => crate::hardware::opcodes::bitwise::bit_7_l(cpu, bus),
        0x7E => crate::hardware::opcodes::bitwise::bit_7_hl(cpu, bus),
        0x7F => crate::hardware::opcodes::bitwise::bit_7_a(cpu, bus),

        // RES 0
        0x80 => crate::hardware::opcodes::bitwise::res_0_b(cpu, bus),
        0x81 => crate::hardware::opcodes::bitwise::res_0_c(cpu, bus),
        0x82 => crate::hardware::opcodes::bitwise::res_0_d(cpu, bus),
        0x83 => crate::hardware::opcodes::bitwise::res_0_e(cpu, bus),
        0x84 => crate::hardware::opcodes::bitwise::res_0_h(cpu, bus),
        0x85 => crate::hardware::opcodes::bitwise::res_0_l(cpu, bus),
        0x86 => crate::hardware::opcodes::bitwise::res_0_hl(cpu, bus),
        0x87 => crate::hardware::opcodes::bitwise::res_0_a(cpu, bus),

        // RES 1
        0x88 => crate::hardware::opcodes::bitwise::res_1_b(cpu, bus),
        0x89 => crate::hardware::opcodes::bitwise::res_1_c(cpu, bus),
        0x8A => crate::hardware::opcodes::bitwise::res_1_d(cpu, bus),
        0x8B => crate::hardware::opcodes::bitwise::res_1_e(cpu, bus),
        0x8C => crate::hardware::opcodes::bitwise::res_1_h(cpu, bus),
        0x8D => crate::hardware::opcodes::bitwise::res_1_l(cpu, bus),
        0x8E => crate::hardware::opcodes::bitwise::res_1_hl(cpu, bus),
        0x8F => crate::hardware::opcodes::bitwise::res_1_a(cpu, bus),

        // RES 2
        0x90 => crate::hardware::opcodes::bitwise::res_2_b(cpu, bus),
        0x91 => crate::hardware::opcodes::bitwise::res_2_c(cpu, bus),
        0x92 => crate::hardware::opcodes::bitwise::res_2_d(cpu, bus),
        0x93 => crate::hardware::opcodes::bitwise::res_2_e(cpu, bus),
        0x94 => crate::hardware::opcodes::bitwise::res_2_h(cpu, bus),
        0x95 => crate::hardware::opcodes::bitwise::res_2_l(cpu, bus),
        0x96 => crate::hardware::opcodes::bitwise::res_2_hl(cpu, bus),
        0x97 => crate::hardware::opcodes::bitwise::res_2_a(cpu, bus),

        // RES 3
        0x98 => crate::hardware::opcodes::bitwise::res_3_b(cpu, bus),
        0x99 => crate::hardware::opcodes::bitwise::res_3_c(cpu, bus),
        0x9A => crate::hardware::opcodes::bitwise::res_3_d(cpu, bus),
        0x9B => crate::hardware::opcodes::bitwise::res_3_e(cpu, bus),
        0x9C => crate::hardware::opcodes::bitwise::res_3_h(cpu, bus),
        0x9D => crate::hardware::opcodes::bitwise::res_3_l(cpu, bus),
        0x9E => crate::hardware::opcodes::bitwise::res_3_hl(cpu, bus),
        0x9F => crate::hardware::opcodes::bitwise::res_3_a(cpu, bus),

        // RES 4
        0xA0 => crate::hardware::opcodes::bitwise::res_4_b(cpu, bus),
        0xA1 => crate::hardware::opcodes::bitwise::res_4_c(cpu, bus),
        0xA2 => crate::hardware::opcodes::bitwise::res_4_d(cpu, bus),
        0xA3 => crate::hardware::opcodes::bitwise::res_4_e(cpu, bus),
        0xA4 => crate::hardware::opcodes::bitwise::res_4_h(cpu, bus),
        0xA5 => crate::hardware::opcodes::bitwise::res_4_l(cpu, bus),
        0xA6 => crate::hardware::opcodes::bitwise::res_4_hl(cpu, bus),
        0xA7 => crate::hardware::opcodes::bitwise::res_4_a(cpu, bus),

        // RES 5
        0xA8 => crate::hardware::opcodes::bitwise::res_5_b(cpu, bus),
        0xA9 => crate::hardware::opcodes::bitwise::res_5_c(cpu, bus),
        0xAA => crate::hardware::opcodes::bitwise::res_5_d(cpu, bus),
        0xAB => crate::hardware::opcodes::bitwise::res_5_e(cpu, bus),
        0xAC => crate::hardware::opcodes::bitwise::res_5_h(cpu, bus),
        0xAD => crate::hardware::opcodes::bitwise::res_5_l(cpu, bus),
        0xAE => crate::hardware::opcodes::bitwise::res_5_hl(cpu, bus),
        0xAF => crate::hardware::opcodes::bitwise::res_5_a(cpu, bus),

        // RES 6
        0xB0 => crate::hardware::opcodes::bitwise::res_6_b(cpu, bus),
        0xB1 => crate::hardware::opcodes::bitwise::res_6_c(cpu, bus),
        0xB2 => crate::hardware::opcodes::bitwise::res_6_d(cpu, bus),
        0xB3 => crate::hardware::opcodes::bitwise::res_6_e(cpu, bus),
        0xB4 => crate::hardware::opcodes::bitwise::res_6_h(cpu, bus),
        0xB5 => crate::hardware::opcodes::bitwise::res_6_l(cpu, bus),
        0xB6 => crate::hardware::opcodes::bitwise::res_6_hl(cpu, bus),
        0xB7 => crate::hardware::opcodes::bitwise::res_6_a(cpu, bus),

        // RES 7
        0xB8 => crate::hardware::opcodes::bitwise::res_7_b(cpu, bus),
        0xB9 => crate::hardware::opcodes::bitwise::res_7_c(cpu, bus),
        0xBA => crate::hardware::opcodes::bitwise::res_7_d(cpu, bus),
        0xBB => crate::hardware::opcodes::bitwise::res_7_e(cpu, bus),
        0xBC => crate::hardware::opcodes::bitwise::res_7_h(cpu, bus),
        0xBD => crate::hardware::opcodes::bitwise::res_7_l(cpu, bus),
        0xBE => crate::hardware::opcodes::bitwise::res_7_hl(cpu, bus),
        0xBF => crate::hardware::opcodes::bitwise::res_7_a(cpu, bus),

        // SET 0
        0xC0 => crate::hardware::opcodes::bitwise::set_0_b(cpu, bus),
        0xC1 => crate::hardware::opcodes::bitwise::set_0_c(cpu, bus),
        0xC2 => crate::hardware::opcodes::bitwise::set_0_d(cpu, bus),
        0xC3 => crate::hardware::opcodes::bitwise::set_0_e(cpu, bus),
        0xC4 => crate::hardware::opcodes::bitwise::set_0_h(cpu, bus),
        0xC5 => crate::hardware::opcodes::bitwise::set_0_l(cpu, bus),
        0xC6 => crate::hardware::opcodes::bitwise::set_0_hl(cpu, bus),
        0xC7 => crate::hardware::opcodes::bitwise::set_0_a(cpu, bus),

        // SET 1
        0xC8 => crate::hardware::opcodes::bitwise::set_1_b(cpu, bus),
        0xC9 => crate::hardware::opcodes::bitwise::set_1_c(cpu, bus),
        0xCA => crate::hardware::opcodes::bitwise::set_1_d(cpu, bus),
        0xCB => crate::hardware::opcodes::bitwise::set_1_e(cpu, bus),
        0xCC => crate::hardware::opcodes::bitwise::set_1_h(cpu, bus),
        0xCD => crate::hardware::opcodes::bitwise::set_1_l(cpu, bus),
        0xCE => crate::hardware::opcodes::bitwise::set_1_hl(cpu, bus),
        0xCF => crate::hardware::opcodes::bitwise::set_1_a(cpu, bus),

        // SET 2
        0xD0 => crate::hardware::opcodes::bitwise::set_2_b(cpu, bus),
        0xD1 => crate::hardware::opcodes::bitwise::set_2_c(cpu, bus),
        0xD2 => crate::hardware::opcodes::bitwise::set_2_d(cpu, bus),
        0xD3 => crate::hardware::opcodes::bitwise::set_2_e(cpu, bus),
        0xD4 => crate::hardware::opcodes::bitwise::set_2_h(cpu, bus),
        0xD5 => crate::hardware::opcodes::bitwise::set_2_l(cpu, bus),
        0xD6 => crate::hardware::opcodes::bitwise::set_2_hl(cpu, bus),
        0xD7 => crate::hardware::opcodes::bitwise::set_2_a(cpu, bus),

        // SET 3
        0xD8 => crate::hardware::opcodes::bitwise::set_3_b(cpu, bus),
        0xD9 => crate::hardware::opcodes::bitwise::set_3_c(cpu, bus),
        0xDA => crate::hardware::opcodes::bitwise::set_3_d(cpu, bus),
        0xDB => crate::hardware::opcodes::bitwise::set_3_e(cpu, bus),
        0xDC => crate::hardware::opcodes::bitwise::set_3_h(cpu, bus),
        0xDD => crate::hardware::opcodes::bitwise::set_3_l(cpu, bus),
        0xDE => crate::hardware::opcodes::bitwise::set_3_hl(cpu, bus),
        0xDF => crate::hardware::opcodes::bitwise::set_3_a(cpu, bus),

        // SET 4
        0xE0 => crate::hardware::opcodes::bitwise::set_4_b(cpu, bus),
        0xE1 => crate::hardware::opcodes::bitwise::set_4_c(cpu, bus),
        0xE2 => crate::hardware::opcodes::bitwise::set_4_d(cpu, bus),
        0xE3 => crate::hardware::opcodes::bitwise::set_4_e(cpu, bus),
        0xE4 => crate::hardware::opcodes::bitwise::set_4_h(cpu, bus),
        0xE5 => crate::hardware::opcodes::bitwise::set_4_l(cpu, bus),
        0xE6 => crate::hardware::opcodes::bitwise::set_4_hl(cpu, bus),
        0xE7 => crate::hardware::opcodes::bitwise::set_4_a(cpu, bus),

        // SET 5
        0xE8 => crate::hardware::opcodes::bitwise::set_5_b(cpu, bus),
        0xE9 => crate::hardware::opcodes::bitwise::set_5_c(cpu, bus),
        0xEA => crate::hardware::opcodes::bitwise::set_5_d(cpu, bus),
        0xEB => crate::hardware::opcodes::bitwise::set_5_e(cpu, bus),
        0xEC => crate::hardware::opcodes::bitwise::set_5_h(cpu, bus),
        0xED => crate::hardware::opcodes::bitwise::set_5_l(cpu, bus),
        0xEE => crate::hardware::opcodes::bitwise::set_5_hl(cpu, bus),
        0xEF => crate::hardware::opcodes::bitwise::set_5_a(cpu, bus),

        // SET 6
        0xF0 => crate::hardware::opcodes::bitwise::set_6_b(cpu, bus),
        0xF1 => crate::hardware::opcodes::bitwise::set_6_c(cpu, bus),
        0xF2 => crate::hardware::opcodes::bitwise::set_6_d(cpu, bus),
        0xF3 => crate::hardware::opcodes::bitwise::set_6_e(cpu, bus),
        0xF4 => crate::hardware::opcodes::bitwise::set_6_h(cpu, bus),
        0xF5 => crate::hardware::opcodes::bitwise::set_6_l(cpu, bus),
        0xF6 => crate::hardware::opcodes::bitwise::set_6_hl(cpu, bus),
        0xF7 => crate::hardware::opcodes::bitwise::set_6_a(cpu, bus),

        // SET 7
        0xF8 => crate::hardware::opcodes::bitwise::set_7_b(cpu, bus),
        0xF9 => crate::hardware::opcodes::bitwise::set_7_c(cpu, bus),
        0xFA => crate::hardware::opcodes::bitwise::set_7_d(cpu, bus),
        0xFB => crate::hardware::opcodes::bitwise::set_7_e(cpu, bus),
        0xFC => crate::hardware::opcodes::bitwise::set_7_h(cpu, bus),
        0xFD => crate::hardware::opcodes::bitwise::set_7_l(cpu, bus),
        0xFE => crate::hardware::opcodes::bitwise::set_7_hl(cpu, bus),
        0xFF => crate::hardware::opcodes::bitwise::set_7_a(cpu, bus),
    }

    let expected_total = CB_CYCLE_TABLE[opcode as usize] as u64;
    if cpu.timed_cycles_accounted < expected_total {
        cpu.tick_internal_timed(bus, expected_total - cpu.timed_cycles_accounted);
    }
}
