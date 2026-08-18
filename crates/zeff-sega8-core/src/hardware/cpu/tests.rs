use super::flags::szp_flags;
use super::*;
use crate::hardware::cartridge::{Cartridge, SystemHint};
use crate::hardware::constants::{
    IO_PORT_CONTROLLER_1, IO_PORT_GG_SERIAL_CONTROL, IO_PORT_GG_SERIAL_RX, IO_PORT_PSG,
    IO_PORT_VDP_CONTROL, VDP_CONTROL_REGISTER_WRITE_VALUE, VDP_REG1_FRAME_IRQ_ENABLE,
    VDP_REGISTER_MODE_CONTROL_2, VDP_STATUS_VBLANK, Z80_INTERRUPT_VECTOR_NMI,
};
use crate::hardware::input::ControllerPort;
use crate::hardware::serial::GameGearSerial;

fn cpu_and_bus(program: &[u8]) -> (Cpu, Bus) {
    let cart = Cartridge::load_with_hint(program, SystemHint::MasterSystem)
        .expect("test cartridge should load");
    (Cpu::new(), Bus::new(cart))
}

fn game_gear_cpu_and_bus(program: &[u8]) -> (Cpu, Bus) {
    let cart = Cartridge::load_with_hint(program, SystemHint::GameGear)
        .expect("test cartridge should load");
    (Cpu::new(), Bus::new(cart))
}

fn enable_vdp_frame_interrupt(bus: &mut Bus) {
    bus.io_write(IO_PORT_VDP_CONTROL, VDP_REG1_FRAME_IRQ_ENABLE);
    bus.io_write(
        IO_PORT_VDP_CONTROL,
        VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_MODE_CONTROL_2 as u8,
    );
    bus.vdp_mut().set_status_bits(VDP_STATUS_VBLANK);
}

fn deliver_game_gear_serial_byte(bus: &mut Bus, value: u8, nmi_enabled: bool) {
    let clock_hz = bus.video_standard().clock_hz_approx();
    let local_control = if nmi_enabled { 0x38 } else { 0x30 };
    let mut peer = GameGearSerial::new();
    peer.write_control(0x30);
    peer.write_tx_data(value);
    peer.step_cycles(20_000, clock_hz);
    bus.game_gear_serial_mut().write_control(local_control);
    bus.game_gear_serial_mut()
        .exchange_with_peer(&mut peer, clock_hz);
}

#[test]
fn nop_advances_pc_and_cycles() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0x00]);

    let fetched = cpu.step(&mut bus).expect("NOP should execute");

    assert_eq!(fetched.opcode, 0x00);
    assert_eq!(cpu.regs().pc, 0x0001);
    assert_eq!(cpu.cycles(), 4);
}

#[test]
fn load_immediates_work() {
    let program = [
        0x01, 0x34, 0x12, 0x11, 0x78, 0x56, 0x21, 0xBC, 0x9A, 0x31, 0x00, 0xD0,
    ];
    let (mut cpu, mut bus) = cpu_and_bus(&program);

    for _ in 0..4 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.regs().bc(), 0x1234);
    assert_eq!(cpu.regs().de(), 0x5678);
    assert_eq!(cpu.regs().hl(), 0x9ABC);
    assert_eq!(cpu.regs().sp, 0xD000);
}

#[test]
fn ei_enables_interrupts_after_following_instruction_and_di_clears_them() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xFB, // EI
        0x00, // NOP
        0xF3, // DI
    ]);

    cpu.step(&mut bus);
    assert!(!cpu.interrupts_enabled());
    assert!(!cpu.saved_interrupts_enabled());

    cpu.step(&mut bus);
    assert!(cpu.interrupts_enabled());
    assert!(cpu.saved_interrupts_enabled());

    cpu.step(&mut bus);
    assert!(!cpu.interrupts_enabled());
    assert!(!cpu.saved_interrupts_enabled());
}

#[test]
fn interrupt_mode_opcodes_update_mode_and_retn_restores_iff1_from_iff2() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xED, 0x46, // IM 0
        0xED, 0x56, // IM 1
        0xED, 0x5E, // IM 2
        0xED, 0x45, // RETN
    ]);
    cpu.regs.sp = 0xC000;
    bus.cpu_write(0xC000, 0x34);
    bus.cpu_write(0xC001, 0x12);
    cpu.interrupt_flip_flop_2 = true;

    cpu.step(&mut bus);
    assert_eq!(cpu.interrupt_mode(), InterruptMode::Im0);

    cpu.step(&mut bus);
    assert_eq!(cpu.interrupt_mode(), InterruptMode::Im1);

    cpu.step(&mut bus);
    assert_eq!(cpu.interrupt_mode(), InterruptMode::Im2);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().pc, 0x1234);
    assert!(cpu.interrupts_enabled());
}

#[test]
fn undocumented_ed_retn_aliases_return_like_retn() {
    for opcode in [0x55, 0x5D, 0x65, 0x6D, 0x75, 0x7D] {
        let (mut cpu, mut bus) = cpu_and_bus(&[0xED, opcode]);
        cpu.regs.sp = 0xC000;
        cpu.interrupt_flip_flop_2 = true;
        bus.cpu_write(0xC000, 0x78);
        bus.cpu_write(0xC001, 0x56);

        let fetched = cpu.step(&mut bus).expect("ED RETN alias should execute");

        assert_eq!(fetched.opcode, 0xED);
        assert_eq!(fetched.cycles, CYCLES_RETI_RETN);
        assert_eq!(cpu.regs().pc, 0x5678);
        assert!(cpu.interrupts_enabled(), "opcode {opcode:02X}");
    }
}

#[test]
fn enabled_vblank_interrupt_pushes_pc_and_vectors_to_0038() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x31, 0x00, 0xD0, // LD SP,$D000
        0xFB, // EI
        0x00, // NOP, enables after this instruction
        0x00, // should be interrupted before executing
    ]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    enable_vdp_frame_interrupt(&mut bus);

    let interrupt = cpu.step(&mut bus).expect("interrupt should be serviced");

    assert!(cpu.last_step_was_interrupt());
    assert_eq!(interrupt.opcode, Z80_INTERRUPT_ACK_OPCODE);
    assert_eq!(interrupt.cycles, CYCLES_INTERRUPT_ACK);
    assert_eq!(cpu.regs().pc, Z80_INTERRUPT_VECTOR_IM1);
    assert_eq!(cpu.regs().sp, 0xCFFE);
    assert_eq!(bus.cpu_read(0xCFFE), 0x05);
    assert_eq!(bus.cpu_read(0xCFFF), 0x00);
    assert!(!cpu.interrupts_enabled());
}

#[test]
fn game_gear_serial_rx_nmi_pushes_pc_and_vectors_to_0066() {
    let (mut cpu, mut bus) = game_gear_cpu_and_bus(&[
        0x31, 0x00, 0xD0, // LD SP,$D000
        0xF3, // DI
        0x00, // should be interrupted before executing
    ]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(!cpu.interrupts_enabled());
    deliver_game_gear_serial_byte(&mut bus, 0x5A, true);
    assert!(bus.non_maskable_interrupt_pending());

    let interrupt = cpu.step(&mut bus).expect("NMI should be serviced");

    assert!(cpu.last_step_was_interrupt());
    assert_eq!(interrupt.opcode, Z80_INTERRUPT_ACK_OPCODE);
    assert_eq!(interrupt.cycles, CYCLES_NMI_ACK);
    assert_eq!(cpu.regs().pc, Z80_INTERRUPT_VECTOR_NMI);
    assert_eq!(cpu.regs().sp, 0xCFFE);
    assert_eq!(bus.cpu_read(0xCFFE), 0x04);
    assert_eq!(bus.cpu_read(0xCFFF), 0x00);
    assert!(!bus.non_maskable_interrupt_pending());
    assert_ne!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
    assert_eq!(bus.io_read(IO_PORT_GG_SERIAL_RX), 0x5A);
}

#[test]
fn game_gear_serial_rx_does_not_nmi_when_control_bit_is_clear() {
    let (mut cpu, mut bus) = game_gear_cpu_and_bus(&[0x00]);

    deliver_game_gear_serial_byte(&mut bus, 0x5A, false);
    assert!(!bus.non_maskable_interrupt_pending());

    let fetched = cpu.step(&mut bus).expect("NOP should execute");

    assert_eq!(fetched.opcode, 0x00);
    assert_eq!(cpu.regs().pc, 0x0001);
    assert_ne!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
    assert_eq!(bus.io_read(IO_PORT_GG_SERIAL_RX), 0x5A);
}

#[test]
fn halted_cpu_idles_until_vblank_interrupt_wakes_it() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x31, 0x00, 0xD0, // LD SP,$D000
        0xFB, // EI
        0x76, // HALT, enables interrupts after this instruction
        0x00, // return address after HALT
    ]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.state(), CpuState::Halted);
    assert!(cpu.interrupts_enabled());

    let idle = cpu.step(&mut bus).expect("halted CPU should idle");
    assert_eq!(idle.opcode, 0x76);
    assert_eq!(cpu.state(), CpuState::Halted);

    enable_vdp_frame_interrupt(&mut bus);
    let interrupt = cpu.step(&mut bus).expect("interrupt should wake HALT");

    assert_eq!(interrupt.opcode, Z80_INTERRUPT_ACK_OPCODE);
    assert_eq!(cpu.state(), CpuState::Running);
    assert_eq!(cpu.regs().pc, Z80_INTERRUPT_VECTOR_IM1);
    assert_eq!(bus.cpu_read(0xCFFE), 0x05);
    assert_eq!(bus.cpu_read(0xCFFF), 0x00);
}

#[test]
fn ld_a_n_and_ld_nn_a_write_memory() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0x3E, 0x5A, 0x32, 0x00, 0xC0]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);

    assert_eq!(cpu.regs().a, 0x5A);
    assert_eq!(bus.cpu_read(0xC000), 0x5A);
}

#[test]
fn ld_a_nn_and_register_group_loads_work() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x21, 0x00, 0xC0, // LD HL,$C000
        0x36, 0xA5, // LD (HL),$A5
        0x46, // LD B,(HL)
        0x78, // LD A,B
        0x32, 0x01, 0xC0, // LD ($C001),A
        0x3A, 0x01, 0xC0, // LD A,($C001)
    ]);

    for _ in 0..6 {
        cpu.step(&mut bus);
    }

    assert_eq!(bus.cpu_read(0xC000), 0xA5);
    assert_eq!(bus.cpu_read(0xC001), 0xA5);
    assert_eq!(cpu.regs().b, 0xA5);
    assert_eq!(cpu.regs().a, 0xA5);
}

#[test]
fn indirect_and_16bit_memory_loads_work() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x3E, 0x42, // LD A,$42
        0x01, 0x00, 0xC0, // LD BC,$C000
        0x02, // LD (BC),A
        0xAF, // XOR A
        0x0A, // LD A,(BC)
        0x11, 0x01, 0xC0, // LD DE,$C001
        0x12, // LD (DE),A
        0x21, 0x10, 0xC0, // LD HL,$C010
        0x22, 0x20, 0xC0, // LD ($C020),HL
        0x21, 0x00, 0x00, // LD HL,$0000
        0x2A, 0x20, 0xC0, // LD HL,($C020)
    ]);

    for _ in 0..11 {
        cpu.step(&mut bus);
    }

    assert_eq!(bus.cpu_read(0xC000), 0x42);
    assert_eq!(bus.cpu_read(0xC001), 0x42);
    assert_eq!(bus.cpu_read(0xC020), 0x10);
    assert_eq!(bus.cpu_read(0xC021), 0xC0);
    assert_eq!(cpu.regs().a, 0x42);
    assert_eq!(cpu.regs().hl(), 0xC010);
}

#[test]
fn inc_dec_and_16bit_arithmetic_update_registers_and_flags() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x3E, 0xFF, // LD A,$FF
        0x3C, // INC A
        0x3D, // DEC A
        0x21, 0xFF, 0x0F, // LD HL,$0FFF
        0x01, 0x01, 0x00, // LD BC,$0001
        0x09, // ADD HL,BC
        0x23, // INC HL
        0x2B, // DEC HL
        0xF9, // LD SP,HL
    ]);
    cpu.regs.f = Z80_FLAG_CARRY;

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x00);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_CARRY | Z80_FLAG_ZERO | Z80_FLAG_HALF_CARRY
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0xFF);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_CARRY
            | Z80_FLAG_SIGN
            | Z80_FLAG_BIT_5
            | Z80_FLAG_BIT_3
            | Z80_FLAG_SUBTRACT
            | Z80_FLAG_HALF_CARRY
    );

    for _ in 0..6 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.regs().hl(), 0x1000);
    assert_eq!(cpu.regs().sp, 0x1000);
    assert_eq!(cpu.regs().f & Z80_FLAG_HALF_CARRY, Z80_FLAG_HALF_CARRY);
    assert_eq!(cpu.regs().f & Z80_FLAG_CARRY, 0);
}

#[test]
fn immediate_alu_group_updates_a_and_flags() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x3E, 0x7F, // LD A,$7F
        0xC6, 0x01, // ADD A,$01
        0xFE, 0x80, // CP $80
        0xE6, 0x0F, // AND $0F
        0xF6, 0x80, // OR $80
        0xEE, 0x81, // XOR $81
        0xD6, 0x02, // SUB $02
    ]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x80);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_SIGN | Z80_FLAG_HALF_CARRY | Z80_FLAG_PARITY_OVERFLOW
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x80);
    assert_eq!(cpu.regs().f, Z80_FLAG_ZERO | Z80_FLAG_SUBTRACT);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x00);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW | Z80_FLAG_HALF_CARRY
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x80);
    assert_eq!(cpu.regs().f, Z80_FLAG_SIGN);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x01);
    assert_eq!(cpu.regs().f, 0);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0xFF);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_SIGN
            | Z80_FLAG_BIT_5
            | Z80_FLAG_BIT_3
            | Z80_FLAG_SUBTRACT
            | Z80_FLAG_HALF_CARRY
            | Z80_FLAG_CARRY
    );
}

#[test]
fn jumps_update_pc() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0xC3, 0x05, 0x00, 0x00, 0x00, 0x18, 0xFE]);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().pc, 0x0005);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().pc, 0x0005);
}

#[test]
fn conditional_relative_jumps_and_djnz_follow_flags() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xAF, // XOR A, sets Z
        0x20, 0x02, // JR NZ,+2, not taken
        0x3E, 0x01, // LD A,$01
        0x28, 0x02, // JR Z,+2, taken because LD does not alter F
        0x3E, 0x02, // skipped
        0x06, 0x02, // LD B,$02
        0x10, 0x02, // DJNZ +2, taken
        0x3E, 0x04, // skipped
        0x10, 0x00, // DJNZ +0, not taken
        0x76, // HALT
    ]);

    while !cpu.is_halted() {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.regs().a, 0x01);
    assert_eq!(cpu.regs().b, 0);
    assert_eq!(cpu.regs().pc, 0x0012);
}

#[test]
fn call_ret_push_and_pop_use_stack() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x31, 0x00, 0xD0, // LD SP,$D000
        0xCD, 0x09, 0x00, // CALL $0009
        0x76, // HALT
        0x00, 0x00, // padding
        0x01, 0x34, 0x12, // LD BC,$1234
        0xC5, // PUSH BC
        0x01, 0x00, 0x00, // LD BC,$0000
        0xC1, // POP BC
        0xC9, // RET
    ]);

    while !cpu.is_halted() {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.regs().bc(), 0x1234);
    assert_eq!(cpu.regs().sp, 0xD000);
    assert_eq!(cpu.regs().pc, 0x0007);
    assert_eq!(bus.cpu_read(0xCFFE), 0x06);
    assert_eq!(bus.cpu_read(0xCFFF), 0x00);
}

#[test]
fn cb_rotate_shift_registers_update_result_flags_and_refresh() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x06, 0x81, // LD B,$81
        0xCB, 0x00, // RLC B
        0xCB, 0x38, // SRL B
    ]);

    cpu.step(&mut bus);
    let fetched = cpu.step(&mut bus).expect("CB opcode should execute");

    assert_eq!(fetched.opcode, 0xCB);
    assert_eq!(fetched.cycles, CYCLES_CB_R);
    assert_eq!(cpu.regs().b, 0x03);
    assert_eq!(cpu.regs().f, Z80_FLAG_PARITY_OVERFLOW | Z80_FLAG_CARRY);
    assert_eq!(cpu.regs().r, 3);

    cpu.step(&mut bus);

    assert_eq!(cpu.regs().b, 0x01);
    assert_eq!(cpu.regs().f, Z80_FLAG_CARRY);
    assert_eq!(cpu.regs().r, 5);
}

#[test]
fn cb_bit_res_and_set_work_on_hl_memory() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x21, 0x00, 0xC0, // LD HL,$C000
        0x36, 0x80, // LD (HL),$80
        0xCB, 0x7E, // BIT 7,(HL)
        0xCB, 0xBE, // RES 7,(HL)
        0xCB, 0xC6, // SET 0,(HL)
        0xCB, 0x46, // BIT 0,(HL)
    ]);
    cpu.regs.f = Z80_FLAG_CARRY;

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    let bit = cpu.step(&mut bus).expect("BIT should execute");

    assert_eq!(bit.cycles, CYCLES_CB_BIT_HL);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_CARRY | Z80_FLAG_SIGN | Z80_FLAG_HALF_CARRY
    );

    let res = cpu.step(&mut bus).expect("RES should execute");
    assert_eq!(res.cycles, CYCLES_CB_HL);
    assert_eq!(bus.cpu_read(0xC000), 0x00);

    cpu.step(&mut bus);
    assert_eq!(bus.cpu_read(0xC000), 0x01);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().f, Z80_FLAG_CARRY | Z80_FLAG_HALF_CARRY);
}

#[test]
fn ldir_copies_rom_bytes_to_ram_until_bc_zero() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x21, 0x0C, 0x00, // LD HL,$000C
        0x11, 0x00, 0xC1, // LD DE,$C100
        0x01, 0x03, 0x00, // LD BC,$0003
        0xED, 0xB0, // LDIR
        0x76, // HALT
        0xAA, 0xBB, 0xCC,
    ]);

    while !cpu.is_halted() {
        cpu.step(&mut bus);
    }

    assert_eq!(bus.cpu_read(0xC100), 0xAA);
    assert_eq!(bus.cpu_read(0xC101), 0xBB);
    assert_eq!(bus.cpu_read(0xC102), 0xCC);
    assert_eq!(cpu.regs().hl(), 0x000F);
    assert_eq!(cpu.regs().de(), 0xC103);
    assert_eq!(cpu.regs().bc(), 0);
}

#[test]
fn otir_writes_hl_bytes_to_vdp_data_port() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x3E, 0x00, // LD A,$00
        0xD3, 0xBF, // OUT ($BF),A
        0x3E, 0x40, // LD A,$40
        0xD3, 0xBF, // OUT ($BF),A, VDP write address $0000
        0x21, 0x12, 0x00, // LD HL,$0012
        0x06, 0x03, // LD B,$03
        0x0E, 0xBE, // LD C,$BE
        0xED, 0xB3, // OTIR
        0x76, // HALT
        0x11, 0x22, 0x33,
    ]);

    while !cpu.is_halted() {
        cpu.step(&mut bus);
    }

    assert_eq!(&bus.vdp().vram()[0..3], &[0x11, 0x22, 0x33]);
    assert_eq!(cpu.regs().b, 0);
    assert_eq!(cpu.regs().hl(), 0x0015);
}

#[test]
fn inir_reads_port_bytes_into_hl_memory() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x21,
        0x00,
        0xC2, // LD HL,$C200
        0x06,
        0x02, // LD B,$02
        0x0E,
        IO_PORT_CONTROLLER_1, // LD C,$DC
        0xED,
        0xB2, // INIR
        0x76, // HALT
    ]);
    bus.input_mut()
        .set_controller_raw(ControllerPort::One, 0xF0);

    while !cpu.is_halted() {
        cpu.step(&mut bus);
    }

    assert_eq!(bus.cpu_read(0xC200), 0xF0);
    assert_eq!(bus.cpu_read(0xC201), 0xF0);
    assert_eq!(cpu.regs().b, 0);
    assert_eq!(cpu.regs().hl(), 0xC202);
}

#[test]
fn xor_a_sets_zero_and_halt_stops_execution() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0x3E, 0x80, 0xAF, 0x76]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);

    assert_eq!(cpu.regs().a, 0);
    assert_eq!(cpu.regs().f, szp_flags(0));
    assert_eq!(cpu.state(), CpuState::Halted);
    let idle = cpu
        .step(&mut bus)
        .expect("HALT should idle until interrupt");
    assert_eq!(idle.opcode, 0x76);
    assert_eq!(idle.cycles, CYCLES_HALT);
}

#[test]
fn xor_register_group_updates_szp_flags() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x3E, 0x80, // LD A,$80
        0x06, 0x01, // LD B,$01
        0xA8, // XOR B
    ]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);

    assert_eq!(cpu.regs().a, 0x81);
    assert_eq!(cpu.regs().f, Z80_FLAG_SIGN | Z80_FLAG_PARITY_OVERFLOW);
}

#[test]
fn immediate_io_reads_and_writes_bus_ports() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x3E,
        0x9F, // LD A,$9F
        0xD3,
        IO_PORT_PSG, // OUT ($7F),A
        0xDB,
        IO_PORT_CONTROLLER_1, // IN A,($DC)
    ]);
    bus.input_mut()
        .set_controller_raw(ControllerPort::One, 0xF7);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);

    assert_eq!(bus.apu().last_write(), Some(0x9F));
    assert_eq!(cpu.regs().a, 0xF7);
}

#[test]
fn ed_io_uses_c_register_port() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x0E,
        IO_PORT_PSG, // LD C,$7F
        0x06,
        0x9F, // LD B,$9F
        0xED,
        0x41, // OUT (C),B
        0x0E,
        IO_PORT_CONTROLLER_1, // LD C,$DC
        0xED,
        0x48, // IN C,(C)
    ]);
    bus.input_mut()
        .set_controller_raw(ControllerPort::One, 0xFE);

    for _ in 0..5 {
        cpu.step(&mut bus);
    }

    assert_eq!(bus.apu().last_write(), Some(0x9F));
    assert_eq!(cpu.regs().c, 0xFE);
    assert_eq!(cpu.regs().f, szp_flags(0xFE));
}

#[test]
fn exchange_opcodes_swap_primary_shadow_and_stack_registers() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xE3, // EX (SP),HL
        0xEB, // EX DE,HL
        0xD9, // EXX
        0x08, // EX AF,AF'
    ]);
    cpu.regs.sp = 0xC000;
    cpu.regs.set_hl(0x1234);
    cpu.regs.set_de(0x9ABC);
    cpu.regs.set_bc(0x1111);
    cpu.regs.a = 0x22;
    cpu.regs.f = Z80_FLAG_CARRY;
    cpu.shadow.b = 0x33;
    cpu.shadow.c = 0x44;
    cpu.shadow.d = 0x55;
    cpu.shadow.e = 0x66;
    cpu.shadow.h = 0x77;
    cpu.shadow.l = 0x88;
    cpu.shadow.a = 0x99;
    cpu.shadow.f = Z80_FLAG_ZERO;
    bus.cpu_write(0xC000, 0x78);
    bus.cpu_write(0xC001, 0x56);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().hl(), 0x5678);
    assert_eq!(bus.cpu_read(0xC000), 0x34);
    assert_eq!(bus.cpu_read(0xC001), 0x12);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().de(), 0x5678);
    assert_eq!(cpu.regs().hl(), 0x9ABC);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().bc(), 0x3344);
    assert_eq!(cpu.regs().de(), 0x5566);
    assert_eq!(cpu.regs().hl(), 0x7788);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x99);
    assert_eq!(cpu.regs().f, Z80_FLAG_ZERO);
}

#[test]
fn accumulator_rotates_preserve_szp_and_update_carry() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x07, // RLCA
        0x0F, // RRCA
        0x17, // RLA
        0x1F, // RRA
    ]);
    cpu.regs.a = 0x81;
    cpu.regs.f = Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW;

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x03);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW | Z80_FLAG_CARRY
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x81);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW | Z80_FLAG_CARRY
    );

    cpu.regs.a = 0x80;
    cpu.regs.f = Z80_FLAG_CARRY;
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x01);
    assert_eq!(cpu.regs().f, Z80_FLAG_CARRY);

    cpu.regs.a = 0x01;
    cpu.regs.f = Z80_FLAG_CARRY;
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x80);
    assert_eq!(cpu.regs().f, Z80_FLAG_CARRY);
}

#[test]
fn flag_misc_opcodes_update_accumulator_and_flags() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0x37, // SCF
        0x3F, // CCF
        0x2F, // CPL
        0x3E, 0x09, // LD A,$09
        0xC6, 0x01, // ADD A,$01
        0x27, // DAA
    ]);
    cpu.regs.a = 0x28;
    cpu.regs.f = Z80_FLAG_ZERO | Z80_FLAG_HALF_CARRY | Z80_FLAG_SUBTRACT;

    cpu.step(&mut bus);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_ZERO | Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3 | Z80_FLAG_CARRY
    );

    cpu.step(&mut bus);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_ZERO | Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3 | Z80_FLAG_HALF_CARRY
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0xD7);
    assert_eq!(
        cpu.regs().f,
        Z80_FLAG_ZERO | Z80_FLAG_HALF_CARRY | Z80_FLAG_SUBTRACT
    );

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x10);
}

#[test]
fn ed_16bit_memory_alu_and_special_register_opcodes_work() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xED, 0x53, 0x00, 0xC0, // LD ($C000),DE
        0xED, 0x5B, 0x00, 0xC0, // LD DE,($C000)
        0xED, 0x52, // SBC HL,DE
        0xED, 0x47, // LD I,A
        0xAF, // XOR A
        0xED, 0x57, // LD A,I
        0xED, 0x44, // NEG
    ]);
    cpu.regs.set_de(0x0101);
    cpu.regs.set_hl(0x0203);
    cpu.regs.a = 0x01;
    cpu.regs.f = Z80_FLAG_CARRY;
    cpu.interrupt_flip_flop_2 = true;

    cpu.step(&mut bus);
    assert_eq!(bus.cpu_read(0xC000), 0x01);
    assert_eq!(bus.cpu_read(0xC001), 0x01);

    cpu.regs.set_de(0);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().de(), 0x0101);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().hl(), 0x0101);
    assert_eq!(cpu.regs().f & Z80_FLAG_SUBTRACT, Z80_FLAG_SUBTRACT);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().i, 0x01);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x01);
    assert_eq!(
        cpu.regs().f & Z80_FLAG_PARITY_OVERFLOW,
        Z80_FLAG_PARITY_OVERFLOW
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0xFF);
    assert_eq!(cpu.regs().f & Z80_FLAG_SUBTRACT, Z80_FLAG_SUBTRACT);
    assert_eq!(cpu.regs().f & Z80_FLAG_CARRY, Z80_FLAG_CARRY);
}

#[test]
fn ed_decimal_rotate_nibbles_between_a_and_hl_memory() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xED, 0x67, // RRD
        0xED, 0x6F, // RLD
    ]);
    cpu.regs.a = 0x12;
    cpu.regs.set_hl(0xC000);
    cpu.regs.f = Z80_FLAG_CARRY;
    bus.cpu_write(0xC000, 0xAB);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x1B);
    assert_eq!(bus.cpu_read(0xC000), 0x2A);
    assert_eq!(cpu.regs().f & Z80_FLAG_CARRY, Z80_FLAG_CARRY);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0x12);
    assert_eq!(bus.cpu_read(0xC000), 0xAB);
}

#[test]
fn ed_block_compare_searches_memory_and_updates_bc_hl() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xED, 0xB1, // CPIR
    ]);
    cpu.regs.a = 0x42;
    cpu.regs.set_hl(0xC000);
    cpu.regs.set_bc(3);
    bus.cpu_write(0xC000, 0x10);
    bus.cpu_write(0xC001, 0x42);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().pc, 0);
    assert_eq!(cpu.regs().hl(), 0xC001);
    assert_eq!(cpu.regs().bc(), 2);
    assert_eq!(
        cpu.regs().f & Z80_FLAG_PARITY_OVERFLOW,
        Z80_FLAG_PARITY_OVERFLOW
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().pc, 2);
    assert_eq!(cpu.regs().hl(), 0xC002);
    assert_eq!(cpu.regs().bc(), 1);
    assert_eq!(cpu.regs().f & Z80_FLAG_ZERO, Z80_FLAG_ZERO);
}

#[test]
fn ix_iy_prefixes_handle_indexed_memory_and_stack_forms() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xDD, 0x21, 0x00, 0xC0, // LD IX,$C000
        0xFD, 0x21, 0x00, 0xC1, // LD IY,$C100
        0xDD, 0x36, 0x02, 0x44, // LD (IX+2),$44
        0xFD, 0x46, 0xFE, // LD B,(IY-2)
        0xDD, 0x34, 0x02, // INC (IX+2)
        0xDD, 0x86, 0x02, // ADD A,(IX+2)
        0xDD, 0xE5, // PUSH IX
        0xFD, 0xE1, // POP IY
        0xDD, 0xE3, // EX (SP),IX
        0xFD, 0xF9, // LD SP,IY
    ]);
    cpu.regs.a = 1;
    cpu.regs.sp = 0xC100;
    bus.cpu_write(0xC0FE, 0x78);

    for _ in 0..6 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.cpu_read(0xC002), 0x45);
    assert_eq!(cpu.regs().b, 0x78);
    assert_eq!(cpu.regs().a, 0x46);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().sp, 0xC0FE);
    assert_eq!(bus.cpu_read(0xC0FE), 0x00);
    assert_eq!(bus.cpu_read(0xC0FF), 0xC0);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().iy, 0xC000);
    assert_eq!(cpu.regs().sp, 0xC100);

    bus.cpu_write(0xC100, 0x34);
    bus.cpu_write(0xC101, 0x12);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().ix, 0x1234);
    assert_eq!(bus.cpu_read(0xC100), 0x00);
    assert_eq!(bus.cpu_read(0xC101), 0xC0);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().sp, 0xC000);
}

#[test]
fn ix_iy_prefixes_handle_index_halves_and_indexed_cb() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xDD, 0x21, 0x00, 0xC0, // LD IX,$C000
        0xDD, 0x26, 0x12, // LD IXH,$12
        0xDD, 0x2E, 0x34, // LD IXL,$34
        0x06, 0x56, // LD B,$56
        0xDD, 0x60, // LD IXH,B
        0xDD, 0x24, // INC IXH
        0xDD, 0x84, // ADD A,IXH
        0xDD, 0x21, 0x00, 0xC0, // LD IX,$C000
        0xDD, 0xCB, 0x02, 0x00, // RLC (IX+2),B
        0xFD, 0x21, 0x10, 0xC0, // LD IY,$C010
        0xFD, 0xCB, 0xF0, 0xC6, // SET 0,(IY-16)
    ]);
    cpu.regs.a = 1;

    for _ in 0..7 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs().ix, 0x5734);
    assert_eq!(cpu.regs().a, 0x58);

    let indexed_addr = 0xC002;
    bus.cpu_write(indexed_addr, 0x80);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs().ix, 0xC000);
    cpu.step(&mut bus);
    assert_eq!(bus.cpu_read(indexed_addr), 0x01);
    assert_eq!(cpu.regs().b, 0x01);
    assert_eq!(cpu.regs().f & Z80_FLAG_CARRY, Z80_FLAG_CARRY);

    bus.cpu_write(0xC000, 0);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(bus.cpu_read(0xC000), 0x01);
}

#[test]
fn ignored_index_prefix_can_execute_conditional_return() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xFD, 0xC0, // RET NZ
    ]);
    cpu.regs.sp = 0xC000;
    cpu.regs.f = 0;
    bus.cpu_write(0xC000, 0x34);
    bus.cpu_write(0xC001, 0x12);

    let fetched = cpu.step(&mut bus).expect("prefixed RET should execute");

    assert_eq!(fetched.cycles, CYCLES_RET_CC + Z80_PREFIX_OVERHEAD);
    assert_eq!(cpu.regs().pc, 0x1234);
}

#[test]
fn ignored_index_prefix_can_execute_non_indexed_ld_r_n() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xFD, 0x0E, 0x5A, // LD C,$5A with ignored IY prefix
    ]);

    let fetched = cpu.step(&mut bus).expect("prefixed LD C,n should execute");

    assert_eq!(fetched.cycles, CYCLES_LD_R_N + Z80_PREFIX_OVERHEAD);
    assert_eq!(cpu.regs().c, 0x5A);
    assert_eq!(cpu.regs().iy, 0);
}

#[test]
fn ignored_index_prefix_can_execute_non_indexed_inc_dec_r() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xFD, 0x0C, // INC C with ignored IY prefix
        0xDD, 0x15, // DEC D with ignored IX prefix
    ]);
    cpu.regs.c = 0x0F;
    cpu.regs.d = 0x01;

    let inc = cpu.step(&mut bus).expect("prefixed INC C should execute");
    let dec = cpu.step(&mut bus).expect("prefixed DEC D should execute");

    assert_eq!(inc.cycles, CYCLES_INC_DEC_R + Z80_PREFIX_OVERHEAD);
    assert_eq!(dec.cycles, CYCLES_INC_DEC_R + Z80_PREFIX_OVERHEAD);
    assert_eq!(cpu.regs().c, 0x10);
    assert_eq!(cpu.regs().d, 0x00);
    assert_eq!(cpu.regs().ix, 0);
    assert_eq!(cpu.regs().iy, 0);
    assert_eq!(cpu.regs().f & Z80_FLAG_ZERO, Z80_FLAG_ZERO);
}

#[test]
fn ignored_index_prefix_can_execute_accumulator_rotate() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xFD, 0x07, // RLCA with ignored IY prefix
    ]);
    cpu.regs.a = 0x81;

    let fetched = cpu
        .step(&mut bus)
        .expect("prefixed accumulator rotate should execute");

    assert_eq!(
        fetched.cycles,
        CYCLES_ACCUMULATOR_ROTATE + Z80_PREFIX_OVERHEAD
    );
    assert_eq!(cpu.regs().a, 0x03);
    assert_eq!(cpu.regs().f & Z80_FLAG_CARRY, Z80_FLAG_CARRY);
}

#[test]
fn ignored_and_repeated_index_prefixes_execute_next_supported_opcode() {
    let (mut cpu, mut bus) = cpu_and_bus(&[
        0xFD,
        0xDB,
        IO_PORT_CONTROLLER_1, // IN A,($DC)
        0xFD,
        0xDD,
        0x21,
        0x34,
        0x12, // repeated prefixes; LD IX,$1234
        0xDD,
        0xDF, // RST $18
    ]);
    bus.input_mut()
        .set_controller_raw(ControllerPort::One, 0xA5);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().a, 0xA5);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().ix, 0x1234);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().pc, 0x0018);
}

#[test]
fn undefined_ed_opcodes_act_as_nop_like_instructions() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0xED, 0x81, 0xED, 0xFF, 0x00]);

    let first = cpu.step(&mut bus).expect("prefix fetch should be reported");
    let second = cpu.step(&mut bus).expect("prefix fetch should be reported");

    assert_eq!(first.opcode, 0xED);
    assert_eq!(first.cycles, CYCLES_ED_NOP);
    assert_eq!(second.opcode, 0xED);
    assert_eq!(second.cycles, CYCLES_ED_NOP);
    assert_eq!(cpu.state(), CpuState::Running);
    assert_eq!(cpu.trap(), None);
    assert_eq!(cpu.regs().pc, 0x0004);
    assert_eq!(cpu.regs().r, 4);
}

#[test]
fn undefined_ed_after_ignored_index_prefix_is_still_nop_like() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0xDD, 0xED, 0xFF, 0x00]);

    let fetched = cpu.step(&mut bus).expect("opcode fetch should be reported");

    assert_eq!(fetched.opcode, 0xDD);
    assert_eq!(fetched.cycles, CYCLES_ED_NOP + Z80_PREFIX_OVERHEAD);
    assert_eq!(cpu.state(), CpuState::Running);
    assert_eq!(cpu.trap(), None);
    assert_eq!(cpu.regs().pc, 0x0003);
}

#[test]
fn refresh_register_counts_opcode_fetches_not_operands() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0x3E, 0x12, 0x00]);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().r, 1);

    cpu.step(&mut bus);
    assert_eq!(cpu.regs().r, 2);
}

#[test]
fn prefixed_opcode_fetch_also_increments_refresh_register() {
    let (mut cpu, mut bus) = cpu_and_bus(&[0xED, 0x71]);
    cpu.regs.c = IO_PORT_PSG;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs().r, 2);
    assert_eq!(bus.apu().last_write(), Some(0));
}
