use super::*;
use crate::emulator::Emulator;

const COUNTERS: u32 = 0x0200_0000;
const BIOS_FLAGS: u32 = 0x0300_7FF8;

fn fixture(thumb: bool, discard: bool, mask: u16, old: u16) -> Emulator {
    let mut rom = vec![0; 0xC0];
    rom[0xB2] = 0x96;
    if thumb {
        rom[..6].copy_from_slice(&[0x04, 0xDF, 0x01, 0x35, 0xFE, 0xE7]);
    } else {
        for (slot, word) in
            rom.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip([0xEF04_0000u32, 0xE285_5001, 0xEAFF_FFFE])
        {
            slot.copy_from_slice(&word.to_le_bytes());
        }
    }
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.cpu.regs[0] = u32::from(discard);
    emu.cpu.regs[1] = u32::from(mask);
    if thumb {
        emu.cpu.cpsr |= CPSR_THUMB;
    }
    // The authored IRQ records and acknowledges its sources, then updates the BIOS mirror.
    for (index, word) in [
        0xE59F_0030,
        0xE1D0_10B2,
        0xE1C0_10B2,
        0xE59F_2028,
        0xE592_3000,
        0xE283_3001,
        0xE582_3000,
        0xE582_1004,
        0xE59F_0018,
        0xE1D0_20B0,
        0xE182_2001,
        0xE1C0_20B0,
        0xE12F_FF1E,
        0xE1A0_0000,
        0x0400_0200,
        COUNTERS,
        BIOS_FLAGS,
    ]
    .into_iter()
    .enumerate()
    {
        emu.bus.write32(0x0300_0000 + index as u32 * 4, word);
    }
    emu.bus.write32(0x0300_7FFC, 0x0300_0000);
    emu.bus.write16(0x0400_0200, 0x3FFF);
    emu.bus.write16(0x0400_0208, 0);
    emu.bus.write16(BIOS_FLAGS, old);
    emu
}

fn finish_wake(emu: &mut Emulator) {
    for _ in 0..1_000 {
        if emu.cpu.regs[5] != 0
            || (emu.cpu.state == CpuState::Halted
                && emu.bus.read16(0x0400_0200) & emu.bus.read16(0x0400_0202) == 0)
        {
            return;
        }
        emu.step_instruction();
    }
    panic!("interrupt handler did not return to its wait");
}

fn request(emu: &mut Emulator, flags: u16) {
    emu.bus.request_interrupt(flags);
    emu.bus.step_cycles(7);
    emu.cpu.cycles += 7;
    finish_wake(emu);
}

#[test]
fn unrelated_irqs_rehalt_until_a_requested_bios_flag_is_set() {
    for thumb in [false, true] {
        for mask in [1, 8, 0x200] {
            let mut emu = fixture(thumb, true, mask, 0);
            emu.step_instruction();
            let return_pc = emu.cpu.swi_wait_return_pc;
            assert_eq!(emu.cpu.state, CpuState::Halted);
            for count in 1..=3 {
                request(&mut emu, 0x400);
                assert_eq!(emu.cpu.regs[5], 0);
                assert_eq!(emu.cpu.state, CpuState::Halted);
                assert_eq!(emu.cpu.swi_wait_return_pc, return_pc);
                assert_eq!(emu.cpu.swi_wait_mask, mask);
                assert_eq!(emu.bus.read32(COUNTERS), count);
                assert_eq!(emu.bus.bios_irq_flags(), 0x400);
            }
            request(&mut emu, mask);
            assert_eq!(emu.cpu.regs[5], 1);
            assert_eq!(emu.bus.bios_irq_flags(), 0x400);
            assert_eq!(emu.cpu.swi_wait_mask, 0);
            assert_eq!(emu.cpu.swi_wait_return_pc, None);

            emu.cpu.set_pc(return_pc.unwrap());
            emu.step_instruction();
            assert_eq!(emu.cpu.regs[5], 2);
            assert_eq!(emu.cpu.state, CpuState::Running);
        }
    }
}

#[test]
fn old_flags_discard_and_combined_sources_preserve_unrequested_bits() {
    for thumb in [false, true] {
        let mut old = fixture(thumb, false, 9, 0x208);
        old.step_instruction();
        assert_eq!(old.cpu.state, CpuState::Running);
        assert_eq!(old.cpu.swi_wait_return_pc, None);
        assert_eq!(old.bus.bios_irq_flags(), 0x200);
        assert_eq!(old.bus.read16(0x0400_0208), 1);
        assert_eq!(old.bus.read32(COUNTERS), 0);

        let mut discarded = fixture(thumb, true, 9, 0x209);
        discarded.step_instruction();
        assert_eq!(discarded.cpu.state, CpuState::Halted);
        assert_eq!(discarded.bus.bios_irq_flags(), 0x200);
        request(&mut discarded, 0x408);
        assert_eq!(discarded.cpu.regs[5], 1);
        assert_eq!(discarded.bus.bios_irq_flags(), 0x600);
        assert_eq!(discarded.bus.read32(COUNTERS), 1);
        assert_eq!(discarded.bus.read32(COUNTERS + 4), 0x408);
    }
}

#[test]
fn a_pending_hardware_flag_requires_its_irq_handler_before_completion() {
    for thumb in [false, true] {
        let mut emu = fixture(thumb, false, 8, 0);
        emu.bus.request_interrupt(8);
        emu.step_instruction();
        assert_eq!(emu.cpu.state, CpuState::Halted);
        assert_eq!(emu.cpu.regs[5], 0);
        assert_eq!(emu.bus.bios_irq_flags(), 0);
        finish_wake(&mut emu);
        assert_eq!(emu.bus.read32(COUNTERS), 1);
        assert_eq!(emu.cpu.regs[5], 1);
        assert_eq!(emu.bus.bios_irq_flags(), 0);
    }
}

#[test]
fn zero_mask_services_irqs_without_completing_the_wait() {
    for thumb in [false, true] {
        let mut emu = fixture(thumb, false, 0, 0x209);
        emu.step_instruction();
        assert_eq!(emu.cpu.state, CpuState::Halted);
        for flags in [8, 0x200, 0x400] {
            request(&mut emu, flags);
            assert_eq!(emu.cpu.regs[5], 0);
            assert_eq!(emu.cpu.state, CpuState::Halted);
            assert!(emu.cpu.swi_wait_return_pc.is_some());
        }
        assert_eq!(emu.bus.read32(COUNTERS), 3);
        assert_eq!(emu.bus.bios_irq_flags(), 0x609);
    }
}

#[test]
fn native_state_restores_a_rehalted_wait_and_adjacent_requested_irq() {
    for thumb in [false, true] {
        let mut source = fixture(thumb, true, 1, 0);
        source.step_instruction();
        request(&mut source, 0x200);
        source.bus.request_interrupt(1);
        let saved = source.encode_state().unwrap();
        let mut restored = fixture(thumb, true, 1, 0);
        restored.load_state(&saved).unwrap();
        finish_wake(&mut source);
        finish_wake(&mut restored);
        assert_eq!(source.cpu.regs[5], 1);
        assert_eq!(
            source.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
    }
}

#[test]
fn failed_bios_flag_check_takes_the_rehalt_path_without_clearing_flags() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.swi_wait_mask = 1;
    cpu.swi_wait_return_pc = Some(RESET_VECTOR);
    bus.write16(BIOS_FLAGS, 8);
    assert!(!cpu.complete_swi_wait(&mut bus));
    assert_eq!(cpu.cycles, 22);
    assert_eq!(cpu.state, CpuState::Halted);
    assert_eq!(cpu.swi_wait_return_pc, Some(RESET_VECTOR));
    assert_eq!(cpu.swi_wait_mask, 1);
    assert_eq!(bus.bios_irq_flags(), 8);
}

#[test]
fn vblank_loop_advances_once_per_frame_despite_frequent_timer_irqs() {
    let mut emu = fixture(false, true, 1, 0);
    emu.bus.write32(0x0300_0100, 0xEF05_0000);
    emu.bus.write32(0x0300_0104, 0xE285_5001);
    emu.bus.write32(0x0300_0108, 0xEAFF_FFFC);
    emu.cpu.set_pc(0x0300_0100);
    emu.bus.write16(0x0400_0200, 9);
    emu.bus.write16(0x0400_0004, 8);
    emu.bus.write16(0x0400_0100, 0xF000);
    emu.bus.write16(0x0400_0102, 0xC0);
    for expected in 0..8 {
        emu.step_frame();
        assert_eq!(emu.cpu.regs[5], expected, "frame {expected}");
        assert!(emu.bus.read32(COUNTERS) > 30 + expected * 50);
    }
}
