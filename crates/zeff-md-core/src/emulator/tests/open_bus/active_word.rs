use super::*;
use crate::constants::{MAIN_CPU_DIVISOR, SOUND_CPU_DIVISOR, TRANSFER_CLOCKS};
use crate::diagnostic::{BusRecord, Callback, Capture, Observation, Outcome, Report};
use crate::hardware::bus::Prefetch;
use sha2::{Digest, Sha256};
use zeff_m68k::bus::Direction;

#[path = "active_word/continuation.rs"]
mod continuation;
#[path = "active_word/interference.rs"]
mod interference;
#[path = "active_word/rig.rs"]
mod rig;
use rig::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_real_move_crossing_and_service_status() {
    ed_block_memory_read_crossing();
    single_memory_read_crossing();
    let mut hash = Sha256::new();
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for phase in [
            Phase::Crossing,
            Phase::Pending,
            Phase::Granted,
            Phase::Reset,
        ] {
            let mut residues = [false; SOUND_CPU_DIVISOR as usize];
            for padding in 1..=SOUND_CPU_DIVISOR as usize {
                let mut actual = fixture(region, phase, padding);
                let mut control = actual.clone();
                let expected = control.step_instruction();
                let capture = Capture::begin().unwrap();
                let result = actual.step_instruction();
                let report = capture.finish(&actual).unwrap();
                assert_eq!(result, expected);
                let read = busreq_record(&report);
                residues[(read.entry.bus.master_ticks % SOUND_CPU_DIVISOR) as usize] = true;
                if !matches!(read.outcome, Some(Outcome::Read(_))) {
                    emit_record(phase, padding, read);
                    assert!(matches!(result, Err(MachineFault::Main(TimingFault::Bus {
                        access,
                        direction: Direction::Read,
                        source: BusError::UnsupportedAccess { address: SOUND_BUS_REQUEST, direction: Direction::Read },
                    })) if access == super::access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both)));
                }
                assert_real_read(&report, &actual, phase);
                result.unwrap();
                assert_eq!(
                    actual.registers().d[0],
                    0x1234_0000 | u32::from(value(phase))
                );
                compare(&mut actual, &mut control, &mut hash);
                hash.update(format!("{region:?}{phase:?}{read:?}").as_bytes());
                cases += 1;
            }
            assert!(residues.into_iter().all(|seen| seen));
        }
    }
    emit("busreq-active-real-v1", hash, cases);
}

fn memory_read_fixture(single: bool) -> Emulator {
    use crate::hardware::sound::{CandidateBus, schedule::SoundInterval};
    let mut image = vec![0; 0x40000];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in [0x31f9_u16, 0x00a1, 0x1100, 0x91c8, 0x60fe, 0x4e71]
        .into_iter()
        .enumerate()
    {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    image[0x39c01] = 6;
    image[0x3a567] = 0x0f;
    let mut machine = Emulator::new(
        image,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 7;
    let program: &[u8] = if single {
        &[0x11, 0x67, 0xa5, 0x1a, 0x13, 0x76]
    } else {
        &[
            0x21, 1, 0x9c, 0x11, 0xf0, 0x1d, 0x01, 1, 0, 0xed, 0xa0, 0x76,
        ]
    };
    machine.bus.sound.ram[..program.len()].copy_from_slice(program);
    let setup_instructions = if single { 1 } else { 3 };
    for _ in 0..setup_instructions {
        let mut bus = CandidateBus::new(&mut machine.bus.sound, &machine.bus.cartridge);
        machine.sound_cpu.step_with_bus(&mut bus).unwrap();
    }
    assert_eq!(machine.sound_cpu.cycles(), if single { 10 } else { 30 });
    let begin = if single { 1001 } else { 1000 };
    machine
        .bus
        .advance_machine(begin - machine.bus.now.get())
        .unwrap();
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.cartridge,
        &mut machine.bus.ram,
        SoundInterval::new(960..begin, &mut machine.bus.audio),
    );
    assert_eq!(machine.sound_schedule.fault, None);
    let pending = machine.sound_schedule.pending.as_ref().unwrap();
    assert_eq!((pending.start, pending.deadline), (960, 1020));
    assert!(!pending.any_bank_access);
    assert_eq!(machine.queued_words(), Some([0x31f9, 0x00a1]));
    machine
}

fn single_memory_read_crossing() {
    single_unreserved_state();
    let mut actual = memory_read_fixture(true);
    let before = actual.encode_frontend_state().unwrap();
    let mut control = actual.clone();
    control.decode_frontend_state(&before).unwrap();
    assert_eq!(control.encode_frontend_state().unwrap(), before);
    let expected = control.step_instruction();
    let capture = Capture::begin().unwrap();
    let result = actual.step_instruction();
    let report = capture.finish(&actual).unwrap();
    assert_eq!(result, expected);
    result.unwrap();
    assert!(report.complete);
    let records: Vec<_> = report.history.iter().flatten().collect();
    let first = records[0];
    assert_eq!(first.wait_clocks, 0);
    assert_eq!(first.outcome, Some(Outcome::Read(0x1100)));
    let pending = first.returned.unwrap().schedule.unwrap().pending.unwrap();
    assert_eq!(pending.bank_access, Some((0x3a567, Direction::Read, 0x0f)));
    assert!(pending.any_bank_access);
    assert_eq!((pending.start, pending.deadline), (1035, 1080));
    let slot = pending.reservation.unwrap();
    assert_eq!((slot.start, slot.end), (1035, 1080));
    let source = records[1];
    assert_eq!(source.entry.bus.master_ticks, 1029);
    assert_eq!(source.wait_clocks, 8);
    assert_eq!(source.service.unwrap().master_ticks, 1113);
    assert_eq!(source.access.unwrap().bus_address, 0x106);
    assert_eq!(source.access.unwrap().kind, AccessKind::Prefetch);
    assert_eq!(source.outcome, Some(Outcome::Read(0x91c8)));
    assert_eq!(
        source.returned.unwrap().bus.prefetch,
        Observation::Full(0x91c8)
    );
    let read = busreq_record(&report);
    assert_eq!(read.sequence, 2);
    assert_eq!(read.entry.bus.master_ticks, 1113);
    assert_eq!(read.service.unwrap().master_ticks, 1141);
    assert_eq!(read.wait_clocks, 0);
    assert_eq!(read.entry.bus.prefetch, Observation::Full(0x91c8));
    assert_eq!(read.service.unwrap().prefetch, Observation::Full(0x91c8));
    assert_eq!(read.outcome, Some(Outcome::Read(0x91c8)));
    assert_eq!(read.returned.unwrap().bus.prefetch, Observation::Empty);
    let pending = read.entry.schedule.unwrap().pending.unwrap();
    assert!(!pending.any_bank_access);
    assert!(pending.bank_access.is_none());
    assert_eq!((pending.start, pending.deadline), (1080, 1170));
    assert_eq!(&actual.bus.ram[0x91c8..0x91ca], &0x91c8_u16.to_be_bytes());
    assert_eq!(actual.sound_cpu.regs().a, 0x0f);
    assert_eq!(actual.sound_cpu.regs().de(), 0xa568);
    assert_eq!(report.first_failed_sequence, None);
    assert_eq!(actual, control);
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        control.encode_frontend_state().unwrap()
    );
}

fn single_unreserved_state() {
    use crate::hardware::sound::schedule::SoundInterval;
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    let mut actual = memory_read_fixture(true);
    actual.sound_schedule.advance(
        &mut actual.sound_cpu,
        &mut actual.bus.sound,
        &mut actual.bus.cartridge,
        &mut actual.bus.ram,
        SoundInterval::new(1001..1029, &mut actual.bus.audio),
    );
    let pending = actual.sound_schedule.pending.as_mut().unwrap();
    assert_eq!((pending.start, pending.deadline), (1020, 1065));
    assert_eq!(pending.instruction.cycles, 3);
    assert!(pending.any_bank_access && pending.bank_access.is_some());
    pending.bank_access = None;
    actual.bus.advance_machine(28).unwrap();
    let mut writer = StateWriter::new();
    actual
        .sound_schedule
        .write_state(&actual.sound_cpu, &mut writer)
        .unwrap();
    let bytes = writer.into_bytes();
    let mut restored = actual.clone();
    let mut reader = StateReader::new(&bytes);
    restored
        .sound_schedule
        .read_state(&mut restored.sound_cpu, &mut reader)
        .unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(actual, restored);
    for machine in [&mut actual, &mut restored] {
        let pending = machine.sound_schedule.pending.as_ref().unwrap();
        assert!(pending.any_bank_access);
        assert!(pending.bank_access.is_none());
        assert!(pending.reservation().is_none());
        assert_eq!(
            super::read(
                machine,
                true,
                access(0x106, AccessKind::Prefetch, ByteLanes::Both)
            ),
            Ok(0x91c8)
        );
        assert_eq!(machine.bus.prefetch, Prefetch::Full(0x91c8));
        assert_eq!(
            super::read(
                machine,
                true,
                access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both)
            ),
            Err(BusError::UnsupportedAccess {
                address: SOUND_BUS_REQUEST,
                direction: Direction::Read,
            })
        );
        assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        assert_eq!(machine.sound_cpu.regs().a, 0x0f);
    }
    assert_eq!(actual, restored);
}

fn ed_block_memory_read_crossing() {
    use crate::hardware::bus::ownership::{Requester, Reservation};
    use crate::hardware::sound::schedule::SoundInterval;
    for end in [1094, 1095, 1096, 1140, 1141] {
        let mut actual = memory_read_fixture(false);
        actual.sound_schedule.advance(
            &mut actual.sound_cpu,
            &mut actual.bus.sound,
            &mut actual.bus.cartridge,
            &mut actual.bus.ram,
            SoundInterval::new(1000..1081, &mut actual.bus.audio),
        );
        let pending = actual.sound_schedule.pending.as_mut().unwrap();
        assert_eq!((pending.start, pending.deadline), (1080, 1125));
        assert!(pending.bank_access.is_some());
        pending
            .reserve_after(&[Reservation {
                requester: Requester::MAIN,
                ticks: 1080..1095,
            }])
            .unwrap();
        assert_eq!((pending.start, pending.deadline), (1095, 1140));
        let mut control = actual.clone();
        let local = actual.sound_schedule.advance_observed(
            &mut actual.sound_cpu,
            &mut actual.bus.sound,
            &mut actual.bus.cartridge,
            &mut actual.bus.ram,
            SoundInterval::new(1081..end, &mut actual.bus.audio),
        );
        control.sound_schedule.advance(
            &mut control.sound_cpu,
            &mut control.bus.sound,
            &mut control.bus.cartridge,
            &mut control.bus.ram,
            SoundInterval::new(1081..end, &mut control.bus.audio),
        );
        assert_eq!(local, end <= 1095);
        assert_eq!(actual, control);
    }
    let mut actual = memory_read_fixture(false);
    let before = actual.encode_frontend_state().unwrap();
    let mut control = actual.clone();
    control.decode_frontend_state(&before).unwrap();
    assert_eq!(control.encode_frontend_state().unwrap(), before);
    let expected = control.step_instruction();
    let capture = Capture::begin().unwrap();
    let result = actual.step_instruction();
    let report = capture.finish(&actual).unwrap();
    assert_eq!(result, expected);
    result.unwrap();
    let read = busreq_record(&report);
    assert_eq!(read.sequence, 2);
    assert_eq!(read.entry.bus.master_ticks, 1056);
    assert_eq!(read.service.unwrap().master_ticks, 1084);
    assert_eq!(read.entry.bus.prefetch, Observation::Full(0x91c8));
    assert_eq!(read.service.unwrap().prefetch, Observation::Full(0x91c8));
    assert_eq!(read.wait_clocks, 0);
    assert_eq!(read.outcome, Some(Outcome::Read(0x91c8)));
    let returned = read.returned.unwrap();
    assert_eq!(returned.bus.prefetch, Observation::Empty);
    let pending = returned.schedule.unwrap().pending.unwrap();
    assert_eq!(pending.bank_access, Some((0x39c01, Direction::Read, 6)));
    assert!(pending.any_bank_access);
    assert_eq!((pending.start, pending.deadline), (1095, 1140));
    let slot = pending.reservation.unwrap();
    assert_eq!((slot.start, slot.end), (1095, 1140));
    let next = report
        .history
        .iter()
        .flatten()
        .find(|record| record.sequence == 3)
        .unwrap();
    assert_eq!(next.wait_clocks, 8);
    assert_eq!(&actual.bus.ram[0x91c8..0x91ca], &0x91c8_u16.to_be_bytes());
    assert_eq!(report.first_failed_sequence, None);
    assert_eq!(actual, control);
    for _ in 0..2 {
        assert_eq!(actual.step_instruction(), control.step_instruction());
    }
    assert_eq!(actual.bus.sound.ram[0x1df0], 6);
    assert_eq!(actual.sound_cpu.regs().hl(), 0x9c02);
    assert_eq!(actual.sound_cpu.regs().de(), 0x1df1);
    assert_eq!(actual.sound_cpu.regs().bc(), 0);
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        control.encode_frontend_state().unwrap()
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_actual_field_observer_neutrality() {
    let mut hash = Sha256::new();
    for phase in [
        Phase::Crossing,
        Phase::Pending,
        Phase::Granted,
        Phase::Reset,
    ] {
        let mut actual = fixture(Region::NtscUs, phase, 1);
        let mut control = actual.clone();
        let expected = control.step_field();
        let capture = Capture::begin().unwrap();
        let result = actual.step_field();
        let report = capture.finish(&actual).unwrap();
        assert_eq!(result, expected);
        assert!(report.complete);
        assert_eq!(report.terminal, actual.diagnostic_snapshot());
        compare(&mut actual, &mut control, &mut hash);
        result.unwrap();
        assert_eq!(
            actual.registers().d[0],
            0x1234_0000 | u32::from(value(phase))
        );
    }
    emit("busreq-active-field-v1", hash, 4);
}
