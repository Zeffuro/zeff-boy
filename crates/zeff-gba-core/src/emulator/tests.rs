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
