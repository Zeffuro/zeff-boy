use super::*;
use crate::hardware::sound::{CandidateBus, SoundFault};
use zeff_m68k::bus::Direction;
use zeff_z80::Z80Bus;

fn arm(m: &mut Emulator, target: u32, code: &[u8]) {
    m.bus.sound.set_reset(false);
    m.bus.sound.bank = (target >> 15) as u16;
    m.bus.sound.ram[..code.len()].copy_from_slice(code);
}

fn pending(m: &mut Emulator, target: u32) {
    for _ in 0..1000 {
        advance(m, 1);
        assert_eq!(m.sound_fault(), None);
        if m.sound_schedule.pending.as_ref().is_some_and(|p| {
            p.bank_access
                .is_some_and(|a| a.address == target && a.direction == Direction::Read)
        }) {
            return;
        }
    }
    panic!("SOUND read not reached {target:x}");
}

fn retire(m: &mut Emulator) {
    let remaining = m.sound_deadline().unwrap().get() - m.bus.now.get();
    assert!(remaining > 0);
    advance(m, remaining);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_speculative_sound_controls_resolve_source_before_overlay_and_sram() {
    for revision in REVISIONS {
        let mut m = machine(revision, Region::NtscUs, true);
        m.install_rom_patches(&patches("001000:12+381000:A5+3C1000:5A"))
            .unwrap();
        let cartridge = m.bus.cartridge.clone();
        let mut memory = m.bus.sound.clone();
        memory.bank = 0;
        let mut bus = CandidateBus::new(&mut memory, &m.bus.cartridge);
        assert_eq!(bus.cpu_read(0x9000), 0x12);
        bus.cpu_write(0x8e00, 0x80);
        assert_eq!(bus.cpu_read(0x9000), 0xa5);
        bus.memory.bank = 0x78;
        assert_eq!(
            bus.cpu_read(0x9000),
            if revision == Revision::HiddenSave {
                0xff
            } else {
                0x5a
            }
        );
        bus.cpu_write(0x9000, 0xc7);
        assert_eq!(
            bus.cpu_read(0x9000),
            if revision == Revision::HiddenSave {
                0xc7
            } else {
                0x5a
            }
        );
        assert_eq!(bus.fault.get(), None);
        assert_eq!(m.bus.cartridge, cartridge);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_pending_sound_v1_uses_receiver_data_and_current_owner_at_deadline() {
    for revision in REVISIONS {
        for region in REGIONS {
            let mut m = machine(revision, region, true);
            control(&mut m, 0x80);
            m.install_rom_patches(&patches("381000:A5")).unwrap();
            arm(&mut m, 0x1000, &[0x3a, 0, 0x90, 0x76]);
            pending(&mut m, 0x1000);
            assert_eq!(
                m.sound_schedule.pending.as_ref().unwrap().cpu.regs().a,
                0xa5
            );
            let raw = m.encode_state().unwrap();
            let front = m.encode_frontend_state().unwrap();
            for current in [Some(0xa5), Some(0x5a), None] {
                for frontend in [false, true] {
                    let mut receiver = m.clone();
                    receiver.clear_rom_patches();
                    if let Some(value) = current {
                        receiver
                            .install_rom_patches(&[patch(0x381000, value)])
                            .unwrap();
                    }
                    restore(&mut receiver, &raw, &front, frontend);
                    let remaining =
                        receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
                    advance(&mut receiver, remaining - 1);
                    assert_eq!(receiver.sound_cpu.regs().a, 0);
                    advance(&mut receiver, 1);
                    assert_eq!(receiver.sound_fault(), None);
                    assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x70));
                }
            }
            let mut disconnected = m.clone();
            control(&mut disconnected, 0x40);
            retire(&mut disconnected);
            assert_eq!(disconnected.sound_fault(), None);
            assert_eq!(disconnected.sound_cpu.regs().a, 0xff);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_sound_consumed_prefix_change_faults_but_operand_refreshes() {
    for revision in REVISIONS {
        for phase in [0, 1] {
            let mut m = machine(revision, Region::NtscUs, true);
            control(&mut m, 0x80);
            m.install_rom_patches(&patches("381000:DD+381001:00+381002:76"))
                .unwrap();
            arm(&mut m, 0x1000, &[0xc3, 0, 0x90]);
            pending(&mut m, 0x1000 + phase);
            assert!(m.sound_schedule.state.next_segment_reads_opcode());
            let raw = m.encode_state().unwrap();
            let front = m.encode_frontend_state().unwrap();
            for frontend in [false, true] {
                let mut changed = m.clone();
                let mut codes = patches("381000:DD+381001:00+381002:76");
                codes.push(patch(0x381000 + phase, if phase == 0 { 0xfd } else { 1 }));
                changed.install_rom_patches(&codes).unwrap();
                restore(&mut changed, &raw, &front, frontend);
                let cpu = changed.sound_cpu.clone();
                retire(&mut changed);
                assert!(matches!(
                    changed.sound_fault(),
                    Some(SoundFault::UnstableCartridgeRead { .. })
                ));
                assert_eq!(changed.sound_cpu, cpu);
            }
        }
        let mut m = machine(revision, Region::NtscUs, true);
        control(&mut m, 0x80);
        m.install_rom_patches(&patches("381000:3E+381001:22+381002:76"))
            .unwrap();
        arm(&mut m, 0x1000, &[0xc3, 0, 0x90]);
        pending(&mut m, 0x1000);
        m.install_rom_patches(&patches("381000:3E+381001:33+381002:76"))
            .unwrap();
        retire(&mut m);
        pending(&mut m, 0x1001);
        retire(&mut m);
        assert_eq!(m.sound_fault(), None);
        assert_eq!(m.sound_cpu.regs().a, 0x33);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_sound_owned_wait_restore_samples_current_source_at_retirement() {
    use crate::hardware::audio::Audio;
    use crate::hardware::bus::ownership::{Requester, Reservation};
    use crate::hardware::sound::SoundMemory;
    use crate::hardware::sound::schedule::{SoundInterval, SoundSchedule};
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    for revision in REVISIONS {
        for requester in [Requester::MAIN, Requester::VDP] {
            let mut m = machine(revision, Region::NtscUs, true);
            control(&mut m, 0x80);
            m.install_rom_patches(&patches("381000:A5")).unwrap();
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.ram[..4].copy_from_slice(&[0x3a, 0, 0x90, 0x76]);
            let mut cpu = zeff_z80::Cpu::new();
            let mut schedule = SoundSchedule::default();
            let mut audio = Audio::default();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut m.bus.cartridge,
                &mut [],
                SoundInterval::new(0..151, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(schedule.pending.as_ref().unwrap().deadline, 195);
            let occupied = [Reservation {
                requester,
                ticks: 150..1500,
            }];
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut m.bus.cartridge,
                &mut [],
                SoundInterval {
                    ticks: 151..1544,
                    occupied: &occupied,
                    audio: &mut audio,
                    sound_irq: None,
                },
            );
            assert_eq!(cpu.regs().a, 0);
            m.install_rom_patches(&patches("381000:5A")).unwrap();
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            let mut other = SoundSchedule::default();
            let mut other_cpu = zeff_z80::Cpu::new();
            other
                .read_state(&mut other_cpu, &mut StateReader::new(&writer.into_bytes()))
                .unwrap();
            other.validate_cartridge(&m.bus.cartridge).unwrap();
            let mut other_memory = memory.clone();
            let mut other_cartridge = m.bus.cartridge.clone();
            let mut other_audio = audio.clone();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut m.bus.cartridge,
                &mut [],
                SoundInterval::new(1544..1545, &mut audio),
            );
            other.advance(
                &mut other_cpu,
                &mut other_memory,
                &mut other_cartridge,
                &mut [],
                SoundInterval::new(1544..1545, &mut other_audio),
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(other.fault, None);
            assert_eq!(cpu.regs().a, 0x5a);
            assert_eq!(cpu, other_cpu);
            assert_eq!(memory, other_memory);
            assert!(schedule == other);
        }
    }
}
