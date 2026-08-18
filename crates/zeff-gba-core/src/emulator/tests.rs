use super::Emulator;
use zeff_emu_common::debug::{DebugEvent, WatchType};
use zeff_emu_common::save_ram::SaveRamKind;

fn minimal_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    rom
}

#[test]
fn breakpoint_suspends_stub_cpu() {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.add_breakpoint(0x0800_0000);
    assert_eq!(emu.save_ram_kind(), SaveRamKind::none());
    assert_eq!(emu.video_ram_snapshot().len(), emu.vram_snapshot().len());
    let (ewram, iwram) = emu.system_ram();
    assert_eq!(
        ewram.len() + iwram.len(),
        crate::hardware::constants::EWRAM_SIZE + crate::hardware::constants::IWRAM_SIZE
    );
    emu.step_frame();
    assert!(emu.is_cpu_suspended());
    assert_eq!(emu.debug_hit_breakpoint(), Some(0x0800_0000));
}

#[test]
fn watchpoint_hits_on_debug_write() {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.add_watchpoint(0x0200_0000, WatchType::Write);
    emu.cpu_write8(0x0200_0000, 0x5A);
    let hit = emu.debug_hit_watchpoint().expect("watchpoint should hit");
    assert_eq!(hit.address, 0x0200_0000);
    assert_eq!(hit.new_value, 0x5A);
}

#[test]
fn event_breakpoints_stop_on_irq_and_dma() {
    let mut rom = minimal_rom();
    rom[..4].copy_from_slice(&0xE1A0_0000_u32.to_le_bytes());
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.cpu_write32(0x03FF_FFFC, 0x0800_0000);
    emu.cpu_write16(0x0400_0200, 1);
    emu.cpu_write16(0x0400_0208, 1);
    emu.bus.request_interrupt(1);
    emu.set_event_breakpoint(DebugEvent::Interrupt, true);

    for _ in 0..4 {
        emu.step_instruction();
        if emu.debug_hit_event().is_some() {
            break;
        }
    }
    assert_eq!(emu.debug_hit_event(), Some(DebugEvent::Interrupt));
    assert!(emu.is_cpu_suspended());

    emu.set_event_breakpoint(DebugEvent::Interrupt, false);
    emu.set_event_breakpoint(DebugEvent::Dma, true);
    emu.debug_continue();
    emu.cpu_write32(0x0400_00B0, 0x0200_0000);
    emu.cpu_write32(0x0400_00B4, 0x0300_0000);
    emu.cpu_write16(0x0400_00B8, 1);
    emu.cpu_write16(0x0400_00BA, 0x8400);

    emu.step_instruction();
    assert_eq!(emu.debug_hit_event(), Some(DebugEvent::Dma));
    assert!(emu.is_cpu_suspended());
}

#[test]
fn public_cpu_peek_does_not_enter_bus_trace() {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();

    emu.bus.debug_trace_enabled = true;
    emu.bus.debug_trace_reads = true;
    assert_eq!(emu.cpu_peek8(0x0200_0000), 0x00);

    assert!(emu.bus.debug_trace_events.borrow().is_empty());
}

#[test]
fn opcode_history_records_executed_instructions_when_enabled() {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.set_opcode_log_enabled(true);

    let fetched = emu
        .step_instruction()
        .expect("stub CPU should fetch one instruction");
    let recent = emu.recent_opcodes(4);

    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].pc, fetched.pc);
    assert_eq!(recent[0].raw, fetched.raw);
    assert_eq!(recent[0].instruction_set, fetched.instruction_set);
    assert_eq!(recent[0].width_bytes, fetched.width_bytes);

    emu.reset();
    assert!(emu.recent_opcodes(4).is_empty());
}

#[test]
fn instruction_trace_captures_arm_bytes_and_mapping() {
    let mut rom = minimal_rom();
    rom[..4].copy_from_slice(&0xE3A0_0042_u32.to_le_bytes());
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.set_instruction_trace_enabled(true);

    emu.step_instruction();

    let entry = emu.instruction_trace().iter().next().unwrap();
    assert_eq!(entry.pc, 0x0800_0000);
    assert_eq!(entry.physical_rom_offset, Some(0));
    assert_eq!(&entry.instruction[..4], &0xE3A0_0042_u32.to_le_bytes());
}

#[test]
fn guest_call_returns_across_thumb_mode() {
    let mut rom = minimal_rom();
    rom[4..6].copy_from_slice(&0x202A_u16.to_le_bytes());
    rom[6..8].copy_from_slice(&0x4770_u16.to_le_bytes());
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.debug_suspend();
    let pc = emu.cpu_pc();

    assert_eq!(emu.debug_execute_guest_call(0x0800_0004, true, 10), Ok(2));
    assert_eq!(emu.cpu_registers()[0], 0x2A);
    assert_eq!(emu.cpu_pc(), pc);
    assert!(!emu.cpu_thumb_state());
    assert!(emu.is_cpu_suspended());
}

#[test]
fn halted_cpu_wakes_on_exact_hblank_interrupt_cycle() {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.cpu_write16(0x0400_0004, 1 << 4);
    emu.cpu_write16(0x0400_0200, 1 << 1);
    emu.cpu_write16(0x0400_0208, 1);
    emu.cpu.state = crate::hardware::cpu::CpuState::Halted;

    while emu.cpu.state == crate::hardware::cpu::CpuState::Halted {
        emu.step_instruction();
    }

    assert_eq!(emu.cpu_cycles(), 1013);
}
