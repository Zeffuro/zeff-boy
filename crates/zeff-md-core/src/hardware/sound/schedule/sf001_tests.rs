use super::*;
use crate::cartridge::sf001::{Mapping, Revision};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::Z80Bus;

fn board(revision: Revision) -> Cartridge {
    Cartridge::new(crate::cartridge::sf001::tests::rom(revision)).unwrap()
}

fn memory(program: &[u8], target: u32) -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = (target >> SOUND_BANK_SHIFT) as u16;
    memory.ram[..program.len()].copy_from_slice(program);
    memory
}

fn read_program(target: u32) -> [u8; 4] {
    let address = (0x8000 | (target & SOUND_MAIN_MASK)) as u16;
    let [low, high] = address.to_le_bytes();
    [0x3a, low, high, 0x76]
}

fn restore(cpu: &Cpu, schedule: &SoundSchedule, cartridge: &Cartridge) -> (Cpu, SoundSchedule) {
    let mut writer = StateWriter::new();
    schedule.write_state(cpu, &mut writer).unwrap();
    let bytes = writer.into_bytes();
    let mut other_cpu = Cpu::new();
    let mut other = SoundSchedule::default();
    other
        .read_state(&mut other_cpu, &mut StateReader::new(&bytes))
        .unwrap();
    other.validate_cartridge(cartridge).unwrap();
    assert_eq!((&other_cpu, &other), (cpu, schedule));
    (other_cpu, other)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_speculation_forwards_mapping_and_sram_aliases_without_board_mutation() {
    let mut cartridge = board(Revision::HiddenSave);
    let original = cartridge.clone();
    let mut memory = memory(&[], 0);
    let mut candidate = CandidateBus::new(&mut memory, &cartridge);
    candidate.cpu_write(0x8e00, 0x80);
    candidate.memory.bank = 0x78;
    assert_eq!(candidate.cpu_read(0x8012), 0xff);
    candidate.cpu_write(0x8012, 0x42);
    candidate.memory.bank = 0x7a;
    assert_eq!(candidate.cpu_read(0x8012), 0x42);
    assert_eq!(candidate.fault.get(), None);
    let journal = candidate.sf001_writes;
    candidate.cpu_write(0x8013, 0xa6);
    assert!(matches!(
        candidate.fault.get(),
        Some(SoundFault::UnsupportedAccess {
            direction: Direction::Write,
            ..
        })
    ));
    assert_eq!(cartridge, original);
    journal.publish(&mut cartridge);
    assert_eq!(
        cartridge.sf001.as_ref().unwrap().mapping(),
        Mapping::Switched
    );
    assert_eq!(cartridge.read(0x3f0012), Some(0x42));
    assert_eq!(cartridge.read(0x3f0013), Some(0xff));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_all_cart_reads_use_live_decoder_and_unclaimed_disconnected_are_ff() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut cartridge = board(revision);
        for mode in [0, 0x80, 0x40] {
            cartridge.sf001.as_mut().unwrap().write(0xe00, mode);
            for target in [0, 0x200, 0x380000, 0x3c0012, 0x400000, 0x40ffff, 0x7fffff] {
                let mut memory = memory(&[], target);
                let candidate = CandidateBus::new(&mut memory, &cartridge);
                let address = (0x8000 | (target & SOUND_MAIN_MASK)) as u16;
                assert_eq!(
                    candidate.cpu_read(address),
                    cartridge.read(target).unwrap_or(0xff)
                );
                assert_eq!(candidate.fault.get(), None);
                assert_eq!(candidate.cartridge_read.get(), Some(address));
                assert!(candidate.timed_read.get());
                assert_eq!(candidate.bank_access.get().unwrap().address, target);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_data_reads_sample_mapping_and_sram_after_each_owned_wait_and_restore() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let original = board(revision);
        for target in [0x200, 0x3c0012, 0x400012] {
            for requester in [Requester::MAIN, Requester::VDP, Requester::MAIN_RMW] {
                let mut cartridge = original.clone();
                let mut memory = memory(&read_program(target), target);
                let mut cpu = Cpu::new();
                let mut schedule = SoundSchedule::default();
                let mut audio = Audio::default();
                schedule.advance(
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval::new(0..151, &mut audio),
                );
                assert_eq!(schedule.fault, None);
                assert_eq!(schedule.pending.as_ref().unwrap().deadline, 195);
                assert_eq!(
                    schedule
                        .pending
                        .as_ref()
                        .unwrap()
                        .bank_access
                        .unwrap()
                        .address,
                    target
                );
                cartridge.sf001.as_mut().unwrap().write(0xe00, 0x80);
                cartridge.sf001.as_mut().unwrap().write(0x3c0012, 0xa6);
                cartridge.sf001.as_mut().unwrap().write(0x400012, 0x42);
                memory.bank = 0x1ff;
                let (other_cpu, other_schedule) = restore(&cpu, &schedule, &cartridge);
                let mut other = (
                    other_cpu,
                    memory.clone(),
                    cartridge.clone(),
                    audio.clone(),
                    other_schedule,
                );
                for phase in [151_u64, 400, 1000] {
                    let end = if phase == 151 {
                        400
                    } else if phase == 400 {
                        1000
                    } else {
                        1544
                    };
                    let occupied = [Reservation {
                        requester,
                        ticks: 150..1500,
                    }];
                    for (cpu, memory, cartridge, audio, schedule) in [
                        (
                            &mut cpu,
                            &mut memory,
                            &mut cartridge,
                            &mut audio,
                            &mut schedule,
                        ),
                        (
                            &mut other.0,
                            &mut other.1,
                            &mut other.2,
                            &mut other.3,
                            &mut other.4,
                        ),
                    ] {
                        schedule.advance(
                            cpu,
                            memory,
                            cartridge,
                            &mut [],
                            SoundInterval {
                                ticks: phase..end,
                                occupied: &occupied,
                                sound_irq: None,
                                audio,
                            },
                        );
                        assert_eq!(schedule.fault, None);
                        assert_eq!(cpu.regs().a, 0);
                        assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
                        if requester == Requester::MAIN_RMW && end < 1500 {
                            let mut writer = StateWriter::new();
                            assert!(schedule.write_state(cpu, &mut writer).is_err());
                        } else {
                            if requester == Requester::MAIN_RMW {
                                schedule.finish_rmw(
                                    &occupied[0],
                                    end,
                                    &mut crate::sound_trace::NoSoundCapture,
                                );
                            }
                            restore(cpu, schedule, cartridge);
                        }
                    }
                }
                let expected = cartridge.read(target).unwrap_or(0xff);
                for (cpu, memory, cartridge, audio, schedule) in [
                    (
                        &mut cpu,
                        &mut memory,
                        &mut cartridge,
                        &mut audio,
                        &mut schedule,
                    ),
                    (
                        &mut other.0,
                        &mut other.1,
                        &mut other.2,
                        &mut other.3,
                        &mut other.4,
                    ),
                ] {
                    schedule.advance(
                        cpu,
                        memory,
                        cartridge,
                        &mut [],
                        SoundInterval::new(1544..1545, audio),
                    );
                    assert_eq!(schedule.fault, None);
                    assert_eq!(cpu.regs().a, expected);
                    assert_eq!(memory.bank, (target >> SOUND_BANK_SHIFT) as u16);
                    assert!(schedule.pending.is_none());
                }
                assert_eq!((cpu, memory, cartridge, audio, schedule), other);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_control_and_save_writes_publish_once_after_ownership_with_captured_address() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let original = board(revision);
        let sram = if revision == Revision::HiddenSave {
            0x3c0012
        } else {
            0x400012
        };
        for (target, value) in [(0xe00, 0xa0), (sram, 0x42)] {
            for requester in [Requester::MAIN, Requester::VDP, Requester::MAIN_RMW] {
                let mut cartridge = original.clone();
                cartridge.sf001.as_mut().unwrap().write(0xe00, 0x80);
                let address = (0x8000 | (target & SOUND_MAIN_MASK)) as u16;
                let [low, high] = address.to_le_bytes();
                let mut memory = memory(&[0x3e, value, 0x32, low, high, 0x76], target);
                let mut cpu = Cpu::new();
                let mut audio = Audio::default();
                let mut schedule = SoundSchedule::default();
                schedule.advance(
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval::new(0..256, &mut audio),
                );
                assert_eq!(schedule.fault, None);
                let before = cartridge.clone();
                let pending = schedule.pending.as_ref().unwrap();
                assert_eq!(pending.deadline, 300);
                assert_eq!(
                    pending.bank_access,
                    Some(BankAccess {
                        address: target,
                        direction: Direction::Write,
                        value
                    })
                );
                assert!(!pending.sf001_writes.is_empty());
                memory.bank = 0x1ff;
                let (other_cpu, other_schedule) = restore(&cpu, &schedule, &cartridge);
                let mut other = (
                    other_cpu,
                    memory.clone(),
                    cartridge.clone(),
                    audio.clone(),
                    other_schedule,
                );
                let occupied = [Reservation {
                    requester,
                    ticks: 255..1500,
                }];
                for (cpu, memory, cartridge, audio, schedule) in [
                    (
                        &mut cpu,
                        &mut memory,
                        &mut cartridge,
                        &mut audio,
                        &mut schedule,
                    ),
                    (
                        &mut other.0,
                        &mut other.1,
                        &mut other.2,
                        &mut other.3,
                        &mut other.4,
                    ),
                ] {
                    schedule.advance(
                        cpu,
                        memory,
                        cartridge,
                        &mut [],
                        SoundInterval {
                            ticks: 256..1544,
                            occupied: &occupied,
                            sound_irq: None,
                            audio,
                        },
                    );
                    assert_eq!(*cartridge, before);
                    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
                    if requester == Requester::MAIN_RMW {
                        schedule.finish_rmw(
                            &occupied[0],
                            1544,
                            &mut crate::sound_trace::NoSoundCapture,
                        );
                    }
                    restore(cpu, schedule, cartridge);
                    schedule.advance(
                        cpu,
                        memory,
                        cartridge,
                        &mut [],
                        SoundInterval::new(1544..1545, audio),
                    );
                    assert_eq!(schedule.fault, None);
                    assert!(schedule.pending.is_none());
                    if target == 0xe00 {
                        assert!(cartridge.sf001.as_ref().unwrap().locked());
                    } else {
                        assert_eq!(cartridge.read(target), Some(value));
                        cartridge.sf001.as_mut().unwrap().write(target, 0x17);
                    }
                    schedule.advance(
                        cpu,
                        memory,
                        cartridge,
                        &mut [],
                        SoundInterval::new(1545..1605, audio),
                    );
                    if target != 0xe00 {
                        assert_eq!(cartridge.read(target), Some(0x17));
                    }
                }
                assert_eq!((cpu, memory, cartridge, audio, schedule), other);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_two_write_instruction_applies_lock_before_second_control_and_codec_keeps_order() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut cartridge = board(revision);
        let mut cpu = Cpu::new();
        let mut memory = memory(&[0x21, 0xa0, 0x40, 0x22, 0, 0x8e, 0x76], 0);
        let mut audio = Audio::default();
        let first = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
        first.publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
        let mut pending = PendingSound::prepare(&cpu, &memory, &cartridge, first.deadline).unwrap();
        assert_eq!(cartridge.sf001.as_ref().unwrap().mapping(), Mapping::Linear);
        pending
            .sf001_writes
            .validate(&cartridge, true, pending.bank_access)
            .unwrap();
        let schedule = SoundSchedule {
            pending: Some(pending.clone()),
            ..SoundSchedule::default()
        };
        let (_, restored) = restore(&cpu, &schedule, &cartridge);
        assert_eq!(
            restored.pending.as_ref().unwrap().sf001_writes,
            pending.sf001_writes
        );
        pending
            .reserve_after(&[Reservation {
                requester: Requester::VDP,
                ticks: pending.start..1500,
            }])
            .unwrap();
        pending.publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
        assert_eq!(
            cartridge.sf001.as_ref().unwrap().mapping(),
            Mapping::Switched
        );
        assert!(cartridge.sf001.as_ref().unwrap().locked());
        cartridge.sf001.as_mut().unwrap().write(0xe00, 0);
        assert_eq!(
            cartridge.sf001.as_ref().unwrap().mapping(),
            Mapping::Switched
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_banked_four_cycle_opcode_fetch_replays_and_rejects_changed_shape_transactionally() {
    let mut bytes = crate::cartridge::sf001::tests::rom(Revision::HiddenSave);
    bytes[0x200] = 0;
    bytes[0x380200] = 0x76;
    bytes[0x3ffffe..].fill(0);
    let sum = bytes[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]))
        });
    bytes[0x3ffffe..].copy_from_slice(&0x3e08_u16.wrapping_sub(sum).to_be_bytes());
    let original = Cartridge::new(bytes).unwrap();
    for changed in [false, true] {
        let mut cartridge = original.clone();
        let mut cpu = Cpu::new();
        let mut memory = memory(&[0xc3, 0, 0x82], 0);
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(0..151, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        let pending = schedule.pending.as_ref().unwrap();
        assert_eq!(
            (
                pending.instruction.pc,
                pending.instruction.opcode,
                pending.instruction.cycles
            ),
            (0x8200, 0, 4)
        );
        assert_eq!(pending.bank_access.unwrap().address, 0x200);
        assert_eq!(pending.deadline, 210);
        restore(&cpu, &schedule, &cartridge);
        if changed {
            cartridge.sf001.as_mut().unwrap().write(0xe00, 0x80);
        }
        let before = (
            cpu.clone(),
            memory.clone(),
            cartridge.clone(),
            schedule.state,
        );
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(151..210, &mut audio),
        );
        if changed {
            assert_eq!(
                schedule.fault,
                Some(SoundFault::UnstableCartridgeRead { pc: 0x8200 })
            );
            assert_eq!(
                (
                    cpu.clone(),
                    memory.clone(),
                    cartridge.clone(),
                    schedule.state
                ),
                before
            );
        } else {
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().pc, 0x8201);
        }
        assert!(schedule.pending.is_none());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_journal_state_rejects_holes_bad_addresses_board_and_access_mismatches() {
    let cartridge = board(Revision::HiddenSave);
    for invalid in [0x1000000_u32, 0xffffffff] {
        let mut writer = StateWriter::new();
        writer.write_bool(true);
        writer.write_u32(invalid);
        writer.write_u8(0);
        writer.write_bool(false);
        assert!(Sf001Writes::read_state(&mut StateReader::new(&writer.into_bytes())).is_err());
    }
    let mut writer = StateWriter::new();
    writer.write_bool(false);
    writer.write_bool(true);
    writer.write_u32(0xe00);
    writer.write_u8(0x80);
    assert!(Sf001Writes::read_state(&mut StateReader::new(&writer.into_bytes())).is_err());
    let mut writes = Sf001Writes::default();
    assert!(writes.push(0xe00, 0x80));
    let access = BankAccess {
        address: 0xe00,
        value: 0x80,
        direction: Direction::Write,
    };
    assert!(writes.validate(&cartridge, true, Some(access)).is_ok());
    assert!(writes.validate(&cartridge, false, Some(access)).is_err());
    assert!(
        writes
            .validate(&cartridge, true, Some(BankAccess { value: 0, ..access }))
            .is_err()
    );
    let mut bytes = crate::cartridge::sf001::tests::rom(Revision::HiddenSave);
    bytes[0x180] ^= 1;
    let ordinary = Cartridge::new(bytes).unwrap();
    assert!(writes.validate(&ordinary, true, Some(access)).is_err());
    assert!(
        Sf001Writes::default()
            .validate(&cartridge, true, Some(access))
            .is_err()
    );
    let memory = memory(&read_program(0x200), 0x200);
    let cpu = Cpu::new();
    let before = (cpu.clone(), memory.clone(), cartridge.clone());
    assert!(matches!(
        PendingSound::prepare(&cpu, &memory, &cartridge, 0),
        Err(SoundFault::UnsupportedAccess {
            direction: Direction::Read,
            ..
        })
    ));
    assert_eq!((cpu, memory.clone(), cartridge.clone()), before);
}
