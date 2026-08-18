use super::Emulator;
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::cartridge::compute_footer_checksum;
use crate::hardware::cpu::CpuState;
use zeff_emu_common::debug::{DebugEvent, WatchType};
use zeff_emu_common::save_ram::SaveRamKind;

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

fn large_linear_bank_prefetch_rom(old_bank_code: &[u8], new_bank_code: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xFF; 2 * 1024 * 1024];
    let old_bank_base = 0x1F_0000;
    let new_bank_base = 0x0F_0000;
    rom[old_bank_base..old_bank_base + old_bank_code.len()].copy_from_slice(old_bank_code);
    rom[new_bank_base..new_bank_base + new_bank_code.len()].copy_from_slice(new_bank_code);
    rom[0x1F_FFF0..0x1F_FFF5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);

    let footer = rom.len() - 10;
    rom[footer + 4] = 0x04;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn code_with_handler(prefix: &[u8], handler_offset: usize, handler: &[u8]) -> Vec<u8> {
    let mut code = vec![0xF4; handler_offset + handler.len()];
    code[..prefix.len()].copy_from_slice(prefix);
    code[handler_offset..handler_offset + handler.len()].copy_from_slice(handler);
    code
}

#[test]
fn loads_and_steps_minimal_rom() {
    let rom = rom_with_reset_code(&[0x90, 0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    assert_eq!(emu.framebuffer_dimensions(), (224, 144));
    assert_eq!(emu.cpu_pc(), 0xFFFF0);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0x90);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xF4);
    assert_eq!(emu.cpu_state(), CpuState::Halted);
}

#[test]
fn guest_call_returns_to_suspended_context() {
    let mut code = vec![0x90; 0x14];
    code[0x10..0x14].copy_from_slice(&[0xB8, 0x2A, 0x00, 0xC3]);
    let rom = rom_with_reset_code(&code);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.step_instruction();
    emu.debug_suspend();
    let pc = emu.cpu_pc();
    let sp = emu.cpu_registers()[4];

    assert_eq!(emu.debug_execute_guest_call(0xF0010, 10), Ok(2));
    assert_eq!(emu.cpu_registers()[0], 0x2A);
    assert_eq!(emu.cpu_pc(), pc);
    assert_eq!(emu.cpu_registers()[4], sp);
    assert_eq!(emu.cpu_state(), CpuState::Suspended);
}

#[test]
fn interrupt_event_breakpoint_suspends_emulator() {
    let rom = rom_with_reset_code(&[0x90, 0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.cpu.flags |= 0x0200;
    emu.bus.write16(0x98, 0x1234);
    emu.bus.write16(0x9A, 0xF000);
    emu.bus.io_write8(0xB0, 0x20);
    emu.bus.io_write8(0xB2, 0x40);
    emu.bus
        .step_cycles(crate::hardware::constants::CYCLES_PER_SCANLINE * 144);
    emu.set_event_breakpoint(DebugEvent::Interrupt, true);

    emu.step_instruction();

    assert_eq!(emu.debug_hit_event(), Some(DebugEvent::Interrupt));
    assert_eq!(emu.cpu_state(), CpuState::Suspended);
}

#[test]
fn opcode_history_records_executed_instructions_when_enabled() {
    let rom = rom_with_reset_code(&[0x90, 0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.set_opcode_log_enabled(true);

    let fetched = emu
        .step_instruction()
        .expect("WonderSwan CPU should fetch one instruction");
    let recent = emu.recent_opcodes(4);

    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].pc, fetched.pc);
    assert_eq!(recent[0].cs, fetched.cs);
    assert_eq!(recent[0].ip, fetched.ip);
    assert_eq!(recent[0].opcode, fetched.opcode);
    assert_eq!(recent[0].cycles, fetched.cycles);

    emu.reset();
    assert!(emu.recent_opcodes(4).is_empty());
}

#[test]
fn instruction_trace_captures_v30_bytes_and_mapping() {
    let rom = rom_with_reset_code(&[0x90, 0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.set_instruction_trace_enabled(true);

    emu.step_instruction();

    let entry = emu.instruction_trace().iter().next().unwrap();
    assert_eq!(entry.pc, 0xFFFF0);
    assert_eq!(entry.physical_rom_offset, Some(0xFFF0));
    assert_eq!(entry.instruction[0], 0xEA);
}

#[test]
fn step_frame_produces_framebuffer() {
    let rom = rom_with_reset_code(&[0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.step_frame();
    assert!(emu.frame_ready());
    assert_eq!(
        emu.framebuffer().len(),
        crate::hardware::constants::FRAMEBUFFER_LEN
    );
    assert_eq!(emu.system_ram().len(), emu.video_ram_snapshot().len());
    assert_eq!(emu.save_ram_kind(), SaveRamKind::none());
    assert_eq!(emu.frame_count, 1);
}

#[test]
fn bus_trace_records_instruction_fetches_and_io() {
    let rom = rom_with_reset_code(&[0xB0, 0x04, 0xE6, 0xC2, 0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.step_instruction();
    emu.step_instruction();
    let (_, trace) = emu.step_instruction_with_bus_trace();
    assert!(trace.iter().any(|event| {
        matches!(
            event,
            DebugTraceEvent::IoWrite {
                port: 0x00C2,
                new_value: 4,
                ..
            }
        )
    }));
}

#[test]
fn breakpoints_suspend_and_debug_step_executes_one_instruction() {
    let rom = rom_with_reset_code(&[0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    let start_pc = emu.cpu_pc();

    emu.add_breakpoint(start_pc);

    assert_eq!(emu.step_instruction(), None);
    assert!(emu.is_cpu_suspended());
    assert_eq!(emu.debug_hit_breakpoint(), Some(start_pc));

    emu.debug_step();

    assert!(emu.is_cpu_suspended());
    assert_eq!(emu.debug_hit_breakpoint(), None);
    assert_ne!(emu.cpu_pc(), start_pc);

    emu.debug_continue();

    assert!(!emu.is_cpu_suspended());
}

#[test]
fn watchpoints_record_debuggable_reads_and_writes() {
    let rom = rom_with_reset_code(&[0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();

    emu.add_watchpoint(0x0000, WatchType::Write);
    assert_eq!(emu.debug_watchpoints().len(), 1);

    emu.cpu_write8(0x0000, 0x5A);

    let hit = emu
        .debug_hit_watchpoint()
        .expect("write watchpoint should hit");
    assert_eq!(hit.address, 0x0000);
    assert_eq!(hit.new_value, 0x5A);
    assert_eq!(hit.watch_type, WatchType::Write);

    emu.debug_continue();
    emu.add_watchpoint(0x0000, WatchType::Read);
    assert_eq!(emu.cpu_read8_debuggable(0x0000), 0x5A);

    let hit = emu
        .debug_hit_watchpoint()
        .expect("read watchpoint should hit");
    assert_eq!(hit.address, 0x0000);
    assert_eq!(hit.new_value, 0x5A);
    assert_eq!(hit.watch_type, WatchType::Read);
}

#[test]
fn input_press_raises_enabled_keypad_interrupt() {
    let rom = rom_with_reset_code(&[0xF4]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.io_write8(0xB0, 0x20);
    emu.io_write8(0xB2, 0x02);

    emu.set_input(0x01, 0x00);

    assert_eq!(emu.io_peek8(0xB4) & 0x02, 0x02);
}

#[test]
fn linear_bank_write_keeps_one_old_bank_instruction_prefetched() {
    let rom = large_linear_bank_prefetch_rom(
        &[
            0xB0, 0x00, // mov al,00
            0xE6, 0xC0, // out c0,al
            0xB0, 0x42, // mov al,42
            0xB0, 0x11, // mov al,11
            0xF4, // hlt
        ],
        &[
            0xB0, 0xEE, // mov al,ee
            0xF4, // hlt
            0x90, // nop
            0xB0, 0x77, // mov al,77
            0xB0, 0x99, // mov al,99
            0xF4, // hlt
        ],
    );
    let mut emu = Emulator::from_rom_data(&rom).unwrap();

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xE6);
    assert_eq!(emu.bus.cartridge.linear_bank(), 1);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x42);
    assert_eq!(emu.bus.cartridge.linear_bank(), 0);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x99);
}

#[test]
fn taken_branch_after_linear_bank_write_fetches_target_from_new_bank() {
    let rom = large_linear_bank_prefetch_rom(
        &[
            0xB0, 0x00, // mov al,00
            0xE6, 0xC0, // out c0,al
            0xEB, 0x04, // jmp +4
            0xB0, 0x42, // mov al,42
            0xF4, // hlt
            0xB0, 0x55, // mov al,55
            0xF4, // hlt
        ],
        &[
            0xB0, 0xEE, // mov al,ee
            0xF4, // hlt
            0x90, // nop
            0xB0, 0x77, // mov al,77
            0xF4, // hlt
            0xF4, // hlt
            0xF4, // hlt
            0xF4, // hlt
            0xB0, 0x99, // mov al,99
            0xF4, // hlt
        ],
    );
    let mut emu = Emulator::from_rom_data(&rom).unwrap();

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xE6);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEB);
    assert_eq!(emu.cpu_pc(), 0xF000A);
    assert_eq!(emu.bus.cartridge.linear_bank(), 0);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x99);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xF4);
    assert_eq!(emu.cpu_state(), CpuState::Halted);
}

#[test]
fn far_jump_after_linear_bank_write_fetches_target_from_new_bank() {
    let old_bank = code_with_handler(
        &[
            0xB0, 0x00, // mov al,00
            0xE6, 0xC0, // out c0,al
            0xEA, 0x10, 0x00, 0x00, 0xF0, // jmp f000:0010
        ],
        0x10,
        &[
            0xB0, 0x11, // mov al,11
            0xF4, // hlt
        ],
    );
    let new_bank = code_with_handler(
        &[
            0xB0, 0xEE, // mov al,ee
            0xF4, // hlt
        ],
        0x10,
        &[
            0xB0, 0x77, // mov al,77
            0xF4, // hlt
        ],
    );
    let rom = large_linear_bank_prefetch_rom(&old_bank, &new_bank);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xE6);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.cpu_pc(), 0xF0010);
    assert_eq!(emu.bus.cartridge.linear_bank(), 0);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x77);
}

#[test]
fn far_call_and_return_after_linear_bank_write_stay_on_new_bank() {
    let old_bank = code_with_handler(
        &[
            0xB0, 0x00, // mov al,00
            0xE6, 0xC0, // out c0,al
            0x9A, 0x10, 0x00, 0x00, 0xF0, // call f000:0010
            0xB0, 0x11, // mov al,11
            0xF4, // hlt
        ],
        0x10,
        &[
            0xB0, 0x22, // mov al,22
            0xCB, // retf
        ],
    );
    let new_bank = code_with_handler(
        &[
            0xB0, 0xEE, // mov al,ee
            0xF4, // hlt
            0xF4, 0xF4, 0xF4, 0xF4, 0xF4, 0xF4, // padding to return address 0009
            0xB0, 0x99, // mov al,99
            0xF4, // hlt
        ],
        0x10,
        &[
            0xB0, 0x66, // mov al,66
            0xCB, // retf
        ],
    );
    let rom = large_linear_bank_prefetch_rom(&old_bank, &new_bank);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xE6);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0x9A);
    assert_eq!(emu.cpu_pc(), 0xF0010);
    assert_eq!(emu.bus.cartridge.linear_bank(), 0);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x66);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xCB);
    assert_eq!(emu.cpu_pc(), 0xF0009);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x99);
}

#[test]
fn software_interrupt_after_linear_bank_write_fetches_handler_from_new_bank() {
    let old_bank = code_with_handler(
        &[
            0xB0, 0x00, // mov al,00
            0xE6, 0xC0, // out c0,al
            0xCD, 0x20, // int 20h
            0xB0, 0x11, // mov al,11
            0xF4, // hlt
        ],
        0x10,
        &[
            0xB0, 0x22, // mov al,22
            0xF4, // hlt
        ],
    );
    let new_bank = code_with_handler(
        &[
            0xB0, 0xEE, // mov al,ee
            0xF4, // hlt
        ],
        0x10,
        &[
            0xB0, 0x88, // mov al,88
            0xF4, // hlt
        ],
    );
    let rom = large_linear_bank_prefetch_rom(&old_bank, &new_bank);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.bus.write16(0x80, 0x0010);
    emu.bus.write16(0x82, 0xF000);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xE6);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xCD);
    assert_eq!(emu.cpu_pc(), 0xF0010);
    assert_eq!(emu.bus.cartridge.linear_bank(), 0);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x88);
}

#[test]
fn hardware_interrupt_after_linear_bank_write_flushes_deferred_bank() {
    let old_bank = code_with_handler(
        &[
            0xFB, // sti
            0xB0, 0x00, // mov al,00
            0xE6, 0xC0, // out c0,al
            0x90, // nop that would be the stale prefetched instruction
            0xF4, // hlt
        ],
        0x10,
        &[
            0xB0, 0x33, // mov al,33
            0xF4, // hlt
        ],
    );
    let new_bank = code_with_handler(
        &[
            0xB0, 0xEE, // mov al,ee
            0xF4, // hlt
        ],
        0x10,
        &[
            0xB0, 0x99, // mov al,99
            0xF4, // hlt
        ],
    );
    let rom = large_linear_bank_prefetch_rom(&old_bank, &new_bank);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();
    emu.bus.write16(0x84, 0x0010);
    emu.bus.write16(0x86, 0xF000);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xFB);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xE6);
    assert_eq!(emu.bus.cartridge.linear_bank(), 1);

    emu.io_write8(0xB0, 0x20);
    emu.io_write8(0xB2, 0x02);
    emu.set_input(0x01, 0x00);

    assert_eq!(emu.step_instruction(), None);
    assert_eq!(emu.cpu_pc(), 0xF0010);
    assert_eq!(emu.bus.cartridge.linear_bank(), 0);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x99);
}

#[test]
fn self_modified_ram_target_is_visible_after_far_jump_flush() {
    let rom = rom_with_reset_code(&[
        0xB8, 0x00, 0x00, // mov ax,0000
        0x8E, 0xD8, // mov ds,ax
        0xC6, 0x06, 0x00, 0x01, 0xB0, // mov byte [0100],b0
        0xC6, 0x06, 0x01, 0x01, 0x5A, // mov byte [0101],5a
        0xC6, 0x06, 0x02, 0x01, 0xF4, // mov byte [0102],f4
        0xEA, 0x00, 0x01, 0x00, 0x00, // jmp 0000:0100
        0xF4, // hlt
    ]);
    let mut emu = Emulator::from_rom_data(&rom).unwrap();

    for expected in [0xEA, 0xB8, 0x8E, 0xC6, 0xC6, 0xC6, 0xEA] {
        assert_eq!(emu.step_instruction().unwrap().opcode, expected);
    }
    assert_eq!(emu.cpu_pc(), 0x00100);

    assert_eq!(emu.step_instruction().unwrap().opcode, 0xB0);
    assert_eq!(emu.cpu_registers()[0] & 0x00FF, 0x5A);
    assert_eq!(emu.step_instruction().unwrap().opcode, 0xF4);
    assert_eq!(emu.cpu_state(), CpuState::Halted);
}
