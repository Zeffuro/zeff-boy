use super::*;
use crate::hardware::cartridge::{Cartridge, compute_footer_checksum};

fn rom_with_reset_code(code: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    rom[..code.len()].copy_from_slice(code);
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn bus_with_code(code: &[u8]) -> Bus {
    Bus::new(Cartridge::load(&rom_with_reset_code(code)).unwrap())
}

fn cpu_at_test_code() -> Cpu {
    let mut cpu = Cpu::new();
    cpu.segments[SegmentRegister::Cs.index()] = 0xF000;
    cpu.segments[SegmentRegister::Ss.index()] = 0x0000;
    cpu.ip = 0x0000;
    cpu.regs[REG_SP as usize] = 0x2000;
    cpu.flags = FLAG_FIXED;
    cpu
}

fn enable_serial_tx_irq(bus: &mut Bus, handler_ip: u16) {
    bus.write16(0x20, handler_ip);
    bus.write16(0x22, 0xF000);
    bus.io_write8(0xB0, 0x08);
    bus.io_write8(0xB3, 0x80);
    bus.io_write8(0xB2, 0x01);
}

fn set_brk_vector(bus: &mut Bus, handler_ip: u16) {
    bus.write16(0x04, handler_ip);
    bus.write16(0x06, 0xF000);
}

fn assert_interrupt_service_pushes_ip(cpu: &mut Cpu, bus: &mut Bus, expected_ip: u16) {
    assert!(cpu.step(bus).is_none());
    assert_eq!(cpu.segments[SegmentRegister::Cs.index()], 0xF000);
    assert_eq!(cpu.ip, 0x1234);
    assert_eq!(
        bus.read16(cpu.physical_address(SegmentRegister::Ss, cpu.get_reg16(REG_SP))),
        expected_ip
    );
}

#[test]
fn reset_fetches_from_x86_reset_vector() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[0x90, 0xF4]);
    let fetched = cpu.step(&mut bus).unwrap();
    assert_eq!(fetched.pc, 0xFFFF0);
    assert_eq!(fetched.opcode, 0xEA);
    assert_eq!(cpu.state, CpuState::Running);
    let fetched = cpu.step(&mut bus).unwrap();
    assert_eq!(fetched.pc, 0xF0000);
    assert_eq!(fetched.opcode, 0x90);
    cpu.step(&mut bus);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn wstiming_base_loop_primitives_use_v30mz_fast_path_cycles() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0x90, // nop
        0x90, // nop
        0x90, // nop
        0xEC, // in al, dx
        0x49, // dec cx
        0x75, 0xF9, // jnz $-7
        0xF4, // hlt
    ]);
    cpu.regs[REG_CX as usize] = 2;

    let cycles = (0..12)
        .map(|_| cpu.step(&mut bus).unwrap().cycles)
        .collect::<Vec<_>>();

    assert_eq!(cycles, [1, 1, 1, 5, 1, 5, 1, 1, 1, 5, 1, 3]);
}

#[test]
fn taken_short_branch_to_odd_target_adds_one_cycle() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0x90, // align padding
        0x90, // odd target
        0x49, // dec cx
        0x75, 0xFC, // jnz $-4
        0xF4,
    ]);
    cpu.ip = 1;
    cpu.regs[REG_CX as usize] = 2;

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 1);
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 1);
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 6);
}

#[test]
fn cartridge_start_state_matches_boot_handoff_registers() {
    let mut cpu = Cpu::new();

    cpu.apply_cartridge_start_state(true);

    assert_eq!(cpu.regs[REG_AX as usize], 0xFF87);
    assert_eq!(cpu.regs[REG_BX as usize], 0x0043);
    assert_eq!(cpu.regs[REG_SP as usize], 0x2000);
    assert_eq!(cpu.regs[REG_SI as usize], 0x0457);
    assert_eq!(cpu.regs[REG_DI as usize], 0x040B);
    assert_eq!(cpu.segments[SegmentRegister::Cs.index()], 0xFFFF);
    assert_eq!(cpu.segments[SegmentRegister::Ss.index()], 0x0000);
    assert_eq!(cpu.segments[SegmentRegister::Ds.index()], 0xFE00);
    assert_eq!(cpu.ip, 0x0000);
    assert_eq!(cpu.flags, 0xF086);
}

#[test]
fn hardware_interrupt_vectors_when_enabled() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[0x90, 0xF4]);
    cpu.segments[SegmentRegister::Cs.index()] = 0x8000;
    cpu.segments[SegmentRegister::Ss.index()] = 0x0000;
    cpu.ip = 0x0100;
    cpu.flags = FLAG_FIXED | FLAG_IF;
    cpu.regs[REG_SP as usize] = 0x2000;

    bus.write16(0x98, 0x1234);
    bus.write16(0x9A, 0x8000);
    bus.io_write8(0xB0, 0x20);
    bus.io_write8(0xB2, 0x40);
    bus.step_cycles(crate::hardware::constants::CYCLES_PER_SCANLINE * 144);

    assert!(cpu.step(&mut bus).is_none());
    assert_eq!(cpu.ip, 0x1234);
    assert_eq!(cpu.segments[SegmentRegister::Cs.index()], 0x8000);
    assert_eq!(cpu.regs[REG_SP as usize], 0x1FFA);
    assert_eq!(bus.read16(0x1FFA), 0x0100);
    assert_eq!(bus.read16(0x1FFC), 0x8000);
    assert_eq!(bus.read16(0x1FFE), FLAG_FIXED | FLAG_IF);
    assert_eq!(cpu.flags & FLAG_IF, 0);
}

#[test]
fn sti_defers_pending_hardware_interrupt_for_one_instruction() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0xFB, // sti
        0x90, // nop
        0xF4, // hlt
    ]);
    enable_serial_tx_irq(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xFB);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x90);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0002);
}

#[test]
fn repeated_sti_does_not_extend_pending_hardware_interrupt_deferral() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0xFB, // sti
        0xFB, // sti
        0xF4, // hlt
    ]);
    enable_serial_tx_irq(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xFB);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xFB);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0002);
}

#[test]
fn popf_defers_pending_hardware_interrupt_when_enabling_if() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0x9D, // popf
        0x90, // nop
        0xF4, // hlt
    ]);
    bus.write16(0x2000, FLAG_FIXED | FLAG_IF);
    enable_serial_tx_irq(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x9D);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x90);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0002);
}

#[test]
fn iret_defers_pending_hardware_interrupt_when_enabling_if() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0xCF, // iret
        0x90, // nop
        0xF4, // hlt
    ]);
    bus.write16(0x2000, 0x0001);
    bus.write16(0x2002, 0xF000);
    bus.write16(0x2004, FLAG_FIXED | FLAG_IF);
    enable_serial_tx_irq(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xCF);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x90);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0002);
}

#[test]
fn pop_ss_defers_pending_hardware_interrupt_for_following_instruction() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0xFB, // sti
        0x17, // pop ss
        0x90, // nop
        0xF4, // hlt
    ]);
    bus.write16(0x2000, 0x0000);
    enable_serial_tx_irq(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xFB);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x17);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x90);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0003);
}

#[test]
fn mov_ss_defers_pending_hardware_interrupt_for_following_instruction() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0x31, 0xC0, // xor ax,ax
        0xFB, // sti
        0x8E, 0xD0, // mov ss,ax
        0x90, // nop
        0xF4, // hlt
    ]);
    enable_serial_tx_irq(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x31);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xFB);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x8E);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x90);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0006);
}

#[test]
fn mov_from_ss_does_not_extend_pending_hardware_interrupt_deferral() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0xFB, // sti
        0x8C, 0xD0, // mov ax,ss
        0xF4, // hlt
    ]);
    enable_serial_tx_irq(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xFB);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x8C);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0003);
}

#[test]
fn brk_enabled_by_popf_is_deferred_for_one_instruction() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0x9D, // popf
        0x90, // nop
        0xF4, // hlt
    ]);
    bus.write16(0x2000, FLAG_FIXED | FLAG_BRK);
    set_brk_vector(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x9D);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x90);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0002);
}

#[test]
fn sti_does_not_extend_brk_deferral_from_popf() {
    let mut cpu = cpu_at_test_code();
    let mut bus = bus_with_code(&[
        0x9D, // popf
        0xFB, // sti
        0xF4, // hlt
    ]);
    bus.write16(0x2000, FLAG_FIXED | FLAG_BRK);
    set_brk_vector(&mut bus, 0x1234);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x9D);
    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xFB);
    assert_interrupt_service_pushes_ip(&mut cpu, &mut bus, 0x0002);
}

#[test]
fn pending_interrupt_wakes_halt_even_when_if_is_clear() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[0xF4, 0x90, 0xF4]);
    cpu.segments[SegmentRegister::Cs.index()] = 0xF000;
    cpu.ip = 0x0000;
    cpu.flags = FLAG_FIXED;
    cpu.regs[REG_SP as usize] = 0x2000;

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0xF4);
    assert_eq!(cpu.state, CpuState::Halted);

    bus.io_write8(0xB0, 0x20);
    bus.io_write8(0xB2, 0x40);
    bus.step_cycles(crate::hardware::constants::CYCLES_PER_SCANLINE * 144);

    assert_eq!(cpu.step(&mut bus).unwrap().opcode, 0x90);
    assert_eq!(cpu.state, CpuState::Running);
    assert_eq!(cpu.segments[SegmentRegister::Cs.index()], 0xF000);
    assert_eq!(cpu.ip, 0x0002);
    assert_eq!(cpu.regs[REG_SP as usize], 0x2000);
}

#[test]
fn mov_immediate_and_out_port_update_bank_register() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[0xB0, 0x07, 0xE6, 0xC2, 0xF4]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(bus.cartridge.bank0(), 7);
}

#[test]
fn register_direct_modrm_mov_and_xor_work() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[0xB8, 0x34, 0x12, 0x8B, 0xD8, 0x31, 0xC0, 0xF4]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs[REG_BX as usize], 0x1234);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs[REG_AX as usize], 0);
    assert!(cpu.flags & FLAG_ZF != 0);
}

#[test]
fn rep_movsw_copies_words_and_updates_indices() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB9, 0x02, 0x00, // mov cx,2
        0xBE, 0x00, 0x02, // mov si,0200
        0xBF, 0x00, 0x03, // mov di,0300
        0xF3, 0xA5, // rep movsw
        0xF4,
    ]);
    bus.write16(0x0200, 0x1234);
    bus.write16(0x0202, 0x5678);
    for _ in 0..7 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.read16(0x0300), 0x1234);
    assert_eq!(bus.read16(0x0302), 0x5678);
    assert_eq!(cpu.regs[REG_CX as usize], 0);
    assert_eq!(cpu.regs[REG_SI as usize], 0x0204);
    assert_eq!(cpu.regs[REG_DI as usize], 0x0304);
}

#[test]
fn lea_and_test_immediate_work() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBB, 0x00, 0x10, // mov bx,1000
        0x8D, 0x40, 0x10, // lea ax,[bx+si+10]
        0xA9, 0x0F, 0x00, // test ax,000f
        0xF4,
    ]);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x1010);
    assert!(cpu.flags & FLAG_ZF != 0);
}

#[test]
fn les_and_lds_load_far_pointers_from_memory() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBB, 0x00, 0x02, // mov bx,0200
        0xC4, 0x07, // les ax,[bx]
        0xC5, 0x5F, 0x04, // lds bx,[bx+4]
        0xF4,
    ]);
    bus.write16(0x0200, 0x3456);
    bus.write16(0x0202, 0x1111);
    bus.write16(0x0204, 0x789A);
    bus.write16(0x0206, 0x2222);
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x3456);
    assert_eq!(cpu.segments[SegmentRegister::Es.index()], 0x1111);
    assert_eq!(cpu.regs[REG_BX as usize], 0x789A);
    assert_eq!(cpu.segments[SegmentRegister::Ds.index()], 0x2222);
}

#[test]
fn group_ff_near_call_and_ret_work() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBC, 0x00, 0x20, // mov sp,2000
        0xB8, 0x0A, 0x00, // mov ax,000a
        0xFF, 0xD0, // call ax
        0xF4, // halt after return
        0x90, // pad to offset 000a
        0xB8, 0x34, 0x12, // mov ax,1234
        0xC3, // ret
    ]);
    for _ in 0..7 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x1234);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn loop_decrements_cx_and_branches_until_zero() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB9, 0x03, 0x00, // mov cx,3
        0x40, // inc ax
        0xE2, 0xFD, // loop -3
        0xF4,
    ]);
    for _ in 0..9 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 3);
    assert_eq!(cpu.regs[REG_CX as usize], 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn pusha_popa_restore_registers_except_stack_pointer_slot() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBC, 0x00, 0x20, // mov sp,2000
        0xB8, 0x11, 0x11, // mov ax,1111
        0xBB, 0x22, 0x22, // mov bx,2222
        0x60, // pusha
        0x31, 0xC0, // xor ax,ax
        0x31, 0xDB, // xor bx,bx
        0x61, // popa
        0xF4,
    ]);
    for _ in 0..8 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x1111);
    assert_eq!(cpu.regs[REG_BX as usize], 0x2222);
    assert_eq!(cpu.regs[REG_SP as usize], 0x2000);
}

#[test]
fn shift_rotate_group_updates_result_and_carry() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x01, 0x80, // mov ax,8001
        0xD1, 0xE0, // shl ax,1
        0xF4,
    ]);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x0002);
    assert!(cpu.flags & FLAG_CF != 0);
    assert!(cpu.flags & FLAG_OF != 0);
}

#[test]
fn shift_rotate_count_from_cl_works() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x10, 0x00, // mov ax,0010
        0xB1, 0x02, // mov cl,2
        0xD3, 0xE8, // shr ax,cl
        0xF4,
    ]);
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x0004);
    assert!(cpu.flags & FLAG_CF == 0);
    assert!(cpu.flags & FLAG_ZF == 0);
}

#[test]
fn immediate_shift_fetches_displacement_before_count() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBD, 0xE6, 0x03, // mov bp,03e6
        0xC7, 0x46, 0x04, 0x00, 0x80, // mov word [bp+4],8000
        0xC7, 0x46, 0x08, 0x03, 0x00, // mov word [bp+8],0003
        0xC1, 0x66, 0x08, 0x04, // shl word [bp+8],4
        0xF4,
    ]);
    for _ in 0..6 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.read16(0x03EA), 0x8000);
    assert_eq!(bus.read16(0x03EE), 0x0030);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn aam_splits_al_into_unpacked_digits() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB0, 0x19, // mov al,25
        0xD4, 0x0A, // aam 10
        0xF4,
    ]);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.state, CpuState::Halted);
    assert_eq!(cpu.regs[REG_AX as usize], 0x0205);
}

#[test]
fn aad_combines_unpacked_digits_into_al() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x05, 0x02, // mov ax,0205
        0xD5, 0x0A, // aad 10
        0xF4,
    ]);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.state, CpuState::Halted);
    assert_eq!(cpu.regs[REG_AX as usize], 0x0019);
}

#[test]
fn salc_sets_al_from_carry_without_touching_flags() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xF9, // stc
        0xD6, // salc
        0xF8, // clc
        0xD6, // salc
        0xF4,
    ]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    let flags_after_stc = cpu.flags;
    cpu.step(&mut bus);
    assert_eq!(cpu.get_reg8(REG_AX), 0xFF);
    assert_eq!(cpu.flags, flags_after_stc);

    cpu.step(&mut bus);
    let flags_after_clc = cpu.flags;
    cpu.step(&mut bus);
    assert_eq!(cpu.get_reg8(REG_AX), 0x00);
    assert_eq!(cpu.flags, flags_after_clc);
}

#[test]
fn xlat_loads_al_from_bx_plus_al_and_honors_segment_override() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBB, 0x00, 0x01, // mov bx,0100
        0xB0, 0x04, // mov al,04
        0xD7, // xlat
        0x26, 0xD7, // es:xlat
        0xF4,
    ]);
    cpu.segments[SegmentRegister::Es.index()] = 0x0100;
    bus.write8(0x0104, 0x07);
    bus.write8(0x1107, 0x2A);

    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.get_reg8(REG_AX), 0x07);

    cpu.step(&mut bus);
    assert_eq!(cpu.get_reg8(REG_AX), 0x2A);
}

#[test]
fn aam_zero_base_enters_divide_error_vector() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB0, 0x19, // mov al,25
        0xD4, 0x00, // aam 0
        0xF4,
    ]);

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);

    assert_eq!(cpu.state, CpuState::Running);
    assert_eq!(cpu.segments[SegmentRegister::Cs.index()], 0x0000);
    assert_eq!(cpu.ip, 0x0000);
    assert_eq!(cpu.last_trap, None);
}

#[test]
fn group_f7_imul_and_div_update_ax_dx() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x06, 0x00, // mov ax,0006
        0xBB, 0xFD, 0xFF, // mov bx,fffd
        0xF7, 0xEB, // imul bx
        0xBB, 0x05, 0x00, // mov bx,0005
        0xBA, 0x00, 0x00, // mov dx,0000
        0xB8, 0x17, 0x00, // mov ax,0017
        0xF7, 0xF3, // div bx
        0xF4,
    ]);
    for _ in 0..9 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 4);
    assert_eq!(cpu.regs[REG_DX as usize], 3);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn group_f6_div_and_neg_update_byte_registers() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x15, 0x00, // mov ax,0015
        0xB3, 0x04, // mov bl,04
        0xF6, 0xF3, // div bl
        0xF6, 0xDB, // neg bl
        0xF4,
    ]);
    for _ in 0..7 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.get_reg8(REG_AX), 5);
    assert_eq!(cpu.get_reg8(REG_AX | 0x04), 1);
    assert_eq!(cpu.get_reg8(REG_BX), 0xFC);
    assert!(cpu.flags & FLAG_CF != 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn rep_movsw_runs_one_iteration_per_step_until_count_exhausted() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB9, 0x02, 0x00, // mov cx,2
        0xBE, 0x00, 0x02, // mov si,0200
        0xBF, 0x00, 0x03, // mov di,0300
        0xF3, 0xA5, // rep movsw
        0xF4,
    ]);
    bus.write16(0x0200, 0x1234);
    bus.write16(0x0202, 0x5678);

    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.ip, 0x0009);

    let fetched = cpu.step(&mut bus).unwrap();
    assert_eq!(fetched.opcode, 0xA5);
    assert_eq!(cpu.ip, 0x0009);
    assert_eq!(cpu.regs[REG_CX as usize], 1);
    assert_eq!(bus.read16(0x0300), 0x1234);

    cpu.step(&mut bus);
    assert_eq!(cpu.ip, 0x000B);
    assert_eq!(cpu.regs[REG_CX as usize], 0);
    assert_eq!(bus.read16(0x0302), 0x5678);
}

#[test]
fn group_f7_divide_error_enters_exception_vector() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x01, 0x00, // mov ax,0001
        0x31, 0xDB, // xor bx,bx
        0xF7, 0xF3, // div bx
        0xF4,
    ]);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.state, CpuState::Running);
    assert_eq!(cpu.segments[SegmentRegister::Cs.index()], 0x0000);
    assert_eq!(cpu.ip, 0x0000);
    assert_eq!(cpu.last_trap, None);
}

#[test]
fn test_rm_reg_and_carry_flag_instructions_work() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x01, 0x00, // mov ax,0001
        0xBB, 0x02, 0x00, // mov bx,0002
        0xF9, // stc
        0x85, 0xD8, // test ax,bx
        0xF9, // stc
        0xF5, // cmc
        0xF9, // stc
        0xF8, // clc
        0xF4,
    ]);
    for _ in 0..10 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 1);
    assert_eq!(cpu.regs[REG_BX as usize], 2);
    assert!(cpu.flags & FLAG_ZF != 0);
    assert!(cpu.flags & FLAG_CF == 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn lahf_and_sahf_round_trip_low_flags() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x00, 0x00, // mov ax,0000
        0xF9, // stc
        0x9F, // lahf
        0xF8, // clc
        0x9E, // sahf
        0xF4,
    ]);
    for _ in 0..7 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.get_reg8(REG_AX | 0x04), 0x03);
    assert!(cpu.flags & FLAG_CF != 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn adc_sbb_immediate_group_uses_carry() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0xFF, 0x00, // mov ax,00ff
        0xF9, // stc
        0x83, 0xD0, 0x00, // adc ax,0
        0xF9, // stc
        0x83, 0xD8, 0x01, // sbb ax,1
        0xF4,
    ]);
    for _ in 0..7 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x00FE);
    assert!(cpu.flags & FLAG_CF == 0);
    assert!(cpu.flags & FLAG_AF != 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn opcode_82_behaves_like_byte_immediate_alu_group() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB0, 0x0F, // mov al,0f
        0x82, 0xE8, 0xF0, // sub al,f0
        0xF4,
    ]);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.get_reg8(REG_AX), 0x1F);
    assert_eq!(cpu.last_trap, None);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn wait_is_noop_without_coprocessor() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x34, 0x12, // mov ax,1234
        0x9B, // wait
        0x40, // inc ax
        0xF4,
    ]);
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x1235);
    assert_eq!(cpu.last_trap, None);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn lock_prefix_is_accepted_for_single_cpu_memory_ops() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBB, 0x00, 0x02, // mov bx,0200
        0xF0, 0xFF, 0x07, // lock inc word [bx]
        0xF4,
    ]);
    bus.write16(0x0200, 0x00FF);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.read16(0x0200), 0x0100);
    assert_eq!(cpu.last_trap, None);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn xchg_rm_reg_byte_and_word_forms_work() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB8, 0x34, 0x12, // mov ax,1234
        0xBB, 0x78, 0x56, // mov bx,5678
        0x87, 0xD8, // xchg ax,bx
        0x86, 0xC4, // xchg ah,al
        0xF4,
    ]);
    for _ in 0..6 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0x7856);
    assert_eq!(cpu.regs[REG_BX as usize], 0x1234);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn rep_outsw_writes_words_to_io_port() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBA, 0x20, 0x00, // mov dx,0020
        0xBE, 0x00, 0x02, // mov si,0200
        0xB9, 0x02, 0x00, // mov cx,2
        0xF3, 0x6F, // rep outsw
        0xF4,
    ]);
    bus.write16(0x0200, 0x1234);
    bus.write16(0x0202, 0x5678);
    for _ in 0..8 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.io_peek8(0x20), 0x78);
    assert_eq!(bus.io_peek8(0x21), 0x56);
    assert_eq!(cpu.regs[REG_SI as usize], 0x0204);
    assert_eq!(cpu.regs[REG_CX as usize], 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn insw_reads_word_from_io_port() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBA, 0x30, 0x00, // mov dx,0030
        0xBF, 0x00, 0x03, // mov di,0300
        0x6D, // insw
        0xF4,
    ]);
    bus.io_write16(0x30, 0xCDA5);
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.read16(0x0300), 0xCDA5);
    assert_eq!(cpu.regs[REG_DI as usize], 0x0302);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn daa_and_das_adjust_bcd_values() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xB0, 0x99, // mov al,99
        0x04, 0x01, // add al,01
        0x27, // daa
        0xB0, 0x10, // mov al,10
        0x2C, 0x01, // sub al,01
        0x2F, // das
        0xF4,
    ]);
    for _ in 0..8 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.get_reg8(REG_AX), 0x09);
    assert!(cpu.flags & FLAG_AF != 0);
    assert!(cpu.flags & FLAG_CF == 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn imul_immediate_forms_write_destination_and_flags() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBB, 0xFD, 0xFF, // mov bx,fffd
        0x69, 0xC3, 0x06, 0x00, // imul ax,bx,0006
        0x6B, 0xD3, 0x80, // imul dx,bx,-128
        0xF4,
    ]);
    for _ in 0..5 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_AX as usize], 0xFFEE);
    assert_eq!(cpu.regs[REG_DX as usize], 0x0180);
    assert!(cpu.flags & FLAG_CF == 0);
    assert!(cpu.flags & FLAG_OF == 0);
    assert_eq!(cpu.state, CpuState::Halted);
}

#[test]
fn enter_and_leave_manage_stack_frame() {
    let mut cpu = Cpu::new();
    let mut bus = bus_with_code(&[
        0xBC, 0x00, 0x20, // mov sp,2000
        0xBD, 0x00, 0x10, // mov bp,1000
        0xC8, 0x04, 0x00, 0x00, // enter 4,0
        0xC9, // leave
        0xF4,
    ]);
    for _ in 0..6 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.regs[REG_BP as usize], 0x1000);
    assert_eq!(cpu.regs[REG_SP as usize], 0x2000);
    assert_eq!(bus.read16(0x1FFE), 0x1000);
    assert_eq!(cpu.state, CpuState::Halted);
}
