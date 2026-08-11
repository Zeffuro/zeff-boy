use super::Emulator;
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::cartridge::compute_footer_checksum;
use crate::hardware::cpu::CpuState;
use zeff_emu_common::debug::WatchType;
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
