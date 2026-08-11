use super::Emulator;
use zeff_emu_common::debug::WatchType;
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
