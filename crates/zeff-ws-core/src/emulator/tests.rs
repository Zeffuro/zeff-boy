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
