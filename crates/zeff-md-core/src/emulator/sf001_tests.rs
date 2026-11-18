use super::*;
use crate::cartridge::sf001::{Mapping, Revision};

mod state;

fn rom(revision: Revision, words: &[u16]) -> Vec<u8> {
    let mut bytes = crate::cartridge::sf001::tests::rom(revision);
    for base in [0x200, 0x380200] {
        for (index, word) in words.iter().enumerate() {
            bytes[base + index * 2..base + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    bytes[0x3ffffe..].fill(0);
    let sum = bytes[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]))
        });
    let expected = u16::from_be_bytes(bytes[0x18e..0x190].try_into().unwrap());
    bytes[0x3ffffe..].copy_from_slice(&expected.wrapping_sub(sum).to_be_bytes());
    bytes
}

fn machine(revision: Revision, words: &[u16], region: Region) -> Emulator {
    Emulator::new(
        rom(revision, words),
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap()
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
    assert_eq!(machine.sound_fault(), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_real_main_instructions_switch_map_and_use_both_save_lanes() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let high = if revision == Revision::HiddenSave {
            0x003c
        } else {
            0x0040
        };
        let words = [
            0x13fc, 0x0080, 0, 0x0e00, 0x13fc, 0x00a5, high, 0, 0x1039, high, 0, 0x13fc, 0x005a,
            high, 1, 0x3239, high, 0, 0x4e72, 0x2700,
        ];
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            let mut source = machine(revision, &words, region);
            for _ in 0..6 {
                source.step_instruction().unwrap();
            }
            assert!(source.cpu.stopped());
            assert_eq!(source.cpu.registers.d[0] & 0xff, 0xa5);
            assert_eq!(source.cpu.registers.d[1] & 0xffff, 0xa55a);
            let board = source.bus.cartridge.sf001.as_ref().unwrap();
            assert_eq!(board.mapping(), Mapping::Switched);
            assert_eq!(&board.save_bytes()[..2], &[0xa5, 0x5a]);
            let _ = source.drain_audio_samples();
            let saved = source.encode_state().unwrap();
            assert_eq!(
                &saved[8..12],
                &crate::save_state::STATE_VERSION.to_le_bytes()
            );
            let mut restored = machine(revision, &words, region);
            restored.decode_state(&saved).unwrap();
            for _ in 0..3 {
                assert_eq!(source.step_instruction(), restored.step_instruction());
                assert_eq!(
                    (&source.encode_state().unwrap()),
                    (&restored.encode_state().unwrap())
                );
                assert_eq!(source.framebuffer(), restored.framebuffer());
                assert_eq!(source.drain_audio_samples(), restored.drain_audio_samples());
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_host_save_and_power_on_preserve_cells_while_cpu_reset_retains_mapping() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut source = machine(revision, &[0x4e70, 0x60fc], Region::NtscUs);
        let cells: Vec<_> = (0..0x10000)
            .map(|index| (index ^ (index >> 8)) as u8)
            .collect();
        source.load_save_ram(&cells).unwrap();
        assert_eq!(source.dump_save_ram(), Some(cells.clone()));
        assert_eq!(
            source.save_ram_kind(),
            zeff_emu_common::save_ram::SaveRamKind::known_nonvolatile(0x10000)
        );
        for size in [0, 0x7fff, 0x8000, 0xffff, 0x10001] {
            let before = source.clone();
            assert!(source.load_save_ram(&vec![0; size]).is_err());
            assert_eq!(source, before);
        }
        source
            .bus
            .cartridge
            .sf001
            .as_mut()
            .unwrap()
            .control(0xe00, 0xa0);
        source.reset_cpu().unwrap();
        source.step_instruction().unwrap();
        let board = source.bus.cartridge.sf001.as_ref().unwrap();
        assert_eq!(board.mapping(), Mapping::Switched);
        assert!(board.locked());
        assert_eq!(board.save_bytes(), cells);
        assert_eq!(source.reset_pulses(), 1);
        source.reset();
        let board = source.bus.cartridge.sf001.as_ref().unwrap();
        assert_eq!(board.mapping(), Mapping::Linear);
        assert!(!board.locked());
        assert_eq!(board.save_bytes(), cells);
        assert_eq!(source.reset_pulses(), 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_failed_boundary_restart_restores_a_mapper_write_published_by_sound() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut source = machine(revision, &[0x60fe], Region::NtscUs);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 0;
        source.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x60, 0x32, 0, 0x8e, 0x76]);
        let mut found = false;
        for _ in 0..600 {
            advance(&mut source, 1);
            if source
                .sound_schedule
                .pending
                .as_ref()
                .and_then(|pending| pending.bank_access)
                .is_some_and(|access| {
                    access.address == 0xe00 && access.direction == zeff_m68k::bus::Direction::Write
                })
            {
                found = true;
                break;
            }
        }
        assert!(found);
        let deadline = source.sound_deadline().unwrap().get();
        let now = source.bus.now.get();
        advance(&mut source, deadline - now - 1);
        let before = source.clone();
        assert!(source.reset_cpu().is_err());
        assert_eq!(source.bus.cartridge, before.bus.cartridge);
        assert_eq!(source.bus.now, before.bus.now);
        assert_eq!(source.bus.cpu_clock, before.bus.cpu_clock);
        assert_eq!(source.bus.ram, before.bus.ram);
        assert_eq!(source.bus.sound, before.bus.sound);
        assert_eq!(source.sound_cpu, before.sound_cpu);
        assert_eq!(source.sound_schedule, before.sound_schedule);
        assert_eq!(source.cpu, before.cpu);
    }
}
