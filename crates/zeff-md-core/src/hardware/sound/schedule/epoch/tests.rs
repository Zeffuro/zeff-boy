use super::*;
use crate::hardware::vdp::Vdp;
use zeff_emu_common::save_state::StateWriter;

mod callbacks;
#[cfg(feature = "sound-shadow-observer")]
mod observer;

#[derive(Clone)]
struct Rig {
    cpu: Cpu,
    memory: SoundMemory,
    schedule: SoundSchedule,
    cartridge: Cartridge,
    audio: Audio,
    ram: Vec<u8>,
}

impl Rig {
    fn new(program: &[u8], batched: bool) -> Self {
        let mut rom = vec![0; 0x4000];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
        let mut memory = SoundMemory::default();
        memory.set_reset(false);
        memory.ram[..program.len()].copy_from_slice(program);
        let mut schedule = SoundSchedule::default();
        if batched {
            schedule.begin_epoch_field();
        }
        let mut audio = Audio::default();
        audio.begin_batch();
        Self {
            cpu: Cpu::new(),
            memory,
            schedule,
            cartridge: Cartridge::new(rom).unwrap(),
            audio,
            ram: vec![0; WORK_RAM_SIZE],
        }
    }

    fn advance(&mut self, begin: u64, end: u64, irq: Option<SoundInterrupt>) {
        let mut interval = SoundInterval::new(begin..end, &mut self.audio);
        interval.sound_irq = irq.as_ref();
        self.schedule.advance(
            &mut self.cpu,
            &mut self.memory,
            &mut self.cartridge,
            &mut self.ram,
            interval,
        );
    }

    fn canonical(&self) -> Self {
        let mut result = self.clone();
        result.schedule.finish_epoch_field(
            &mut result.cpu,
            &result.memory,
            &mut crate::sound_trace::NoSoundCapture,
        );
        result
    }
}

fn equal(actual: &Rig, expected: &Rig) {
    let actual = actual.canonical();
    assert_eq!(actual.cpu, expected.cpu);
    assert_eq!(actual.memory, expected.memory);
    assert_eq!(actual.schedule, expected.schedule);
    assert_eq!(actual.ram, expected.ram);
    assert_eq!(actual.audio, expected.audio);
    if actual.schedule.fault.is_none() {
        let mut a = StateWriter::new();
        let mut b = StateWriter::new();
        actual.schedule.write_state(&actual.cpu, &mut a).unwrap();
        expected
            .schedule
            .write_state(&expected.cpu, &mut b)
            .unwrap();
        assert_eq!(a.into_bytes(), b.into_bytes());
    }
}

#[test]
fn real_segments_match_at_every_tick_and_continuation_cut() {
    for program in [
        &[0, 0, 0, 0, 0, 0, 0, 0][..],
        &[0x21, 0x00, 0x01, 0x7e, 0x3c, 0x18, 0xfc][..],
        &[0xdd, 0xdd, 0x21, 0x00, 0x01, 0xdd, 0x7e, 0x00][..],
    ] {
        let mut actual = Rig::new(program, true);
        let mut expected = Rig::new(program, false);
        for tick in 0..720 {
            actual.advance(tick, tick + 1, None);
            expected.advance(tick, tick + 1, None);
            equal(&actual, &expected);
            if tick == 0 {
                assert!(actual.schedule.epoch_deadline().is_some());
                assert!(actual.schedule.pending.is_none());
            }
        }
    }
}

#[test]
fn future_bank_read_prepares_at_original_cursor_before_deadline() {
    let program = [0, 0, 0, 0, 0x3a, 0x00, 0x80, 0x76];
    let mut actual = Rig::new(&program, true);
    let mut expected = Rig::new(&program, false);
    for (begin, end) in [(0, 1), (1, 389), (389, 391)] {
        actual.advance(begin, end, None);
        expected.advance(begin, end, None);
        equal(&actual, &expected);
    }
    let pending = actual.schedule.pending.as_ref().unwrap();
    assert!(pending.any_bank_access);
    assert_eq!((pending.start, pending.deadline), (390, 435));
    assert_eq!(pending.reservation().unwrap().ticks, 390..435);
}

#[test]
fn speculative_access_fault_is_latched_only_at_actual_preparation() {
    let program = [0, 0, 0, 0, 0x3a, 0x00, 0x7f];
    let mut actual = Rig::new(&program, true);
    let mut expected = Rig::new(&program, false);
    actual.advance(0, 389, None);
    expected.advance(0, 389, None);
    equal(&actual, &expected);
    assert!(actual.schedule.fault.is_none());
    assert!(actual.schedule.epoch_deadline().is_some());
    actual.advance(389, 391, None);
    expected.advance(389, 391, None);
    equal(&actual, &expected);
    assert!(matches!(
        actual.schedule.fault,
        Some(SoundFault::UnsupportedAccess {
            address: 0x7f00,
            ..
        })
    ));
    assert_eq!(actual.cpu.cycles(), 26);
}

#[test]
fn future_local_write_executes_once_at_ordinary_publication() {
    let program = [0x21, 0x00, 0x01, 0x3e, 0x73, 0, 0, 0, 0, 0x77, 0x76];
    let mut actual = Rig::new(&program, true);
    let mut expected = Rig::new(&program, false);
    for (begin, end) in [(0, 554), (554, 556), (556, 599), (599, 600)] {
        actual.advance(begin, end, None);
        expected.advance(begin, end, None);
        equal(&actual, &expected);
        assert_eq!(actual.memory.ram[0x100], if end == 600 { 0x73 } else { 0 });
    }
}

#[test]
fn rising_irq_uses_actual_preparation_sample_and_pending_capture() {
    let vdp = Vdp::bootstrap_mode5();
    let irq = vdp
        .interrupts()
        .sound_interrupt(vdp.registers(), crate::Region::NtscUs);
    let edge = 224 * crate::constants::VDP_LINE_TICKS + 40;
    let begin = edge - 240;
    let mut actual = Rig::new(&[0; 16], true);
    let mut expected = Rig::new(&[0; 16], false);
    actual.audio.advance_to(begin);
    expected.audio.advance_to(begin);
    for (from, to) in [(begin, begin + 1), (begin + 1, edge), (edge, edge + 80)] {
        actual.advance(from, to, Some(irq));
        expected.advance(from, to, Some(irq));
        equal(&actual, &expected);
    }
    assert!(
        actual
            .schedule
            .pending
            .as_ref()
            .unwrap()
            .memory
            .irq_asserted
    );
    assert!(!actual.memory.irq_asserted);
}

#[test]
fn bounded_cache_stops_before_ninth_segment_and_over_budget_instruction() {
    let mut eight = Rig::new(&[0; 16], true);
    eight.advance(0, 1, None);
    let epoch = eight.schedule.epoch.as_ref().unwrap();
    assert_eq!(epoch.len, 8);
    assert_eq!(epoch.records[7].as_ref().unwrap().deadline, 480);
    let mut capped = Rig::new(&[0, 0, 0x18, 0xfe], true);
    capped.advance(0, 1, None);
    let epoch = capped.schedule.epoch.as_ref().unwrap();
    assert_eq!(epoch.len, 6);
    assert_eq!(
        epoch.records[..epoch.len]
            .iter()
            .map(|record| record.as_ref().unwrap().deadline)
            .collect::<Vec<_>>(),
        [60, 120, 180, 300, 360, 480]
    );
}

#[test]
fn dma_interval_materializes_existing_local_pending_before_fallback() {
    let mut actual = Rig::new(&[0; 16], true);
    let mut expected = Rig::new(&[0; 16], false);
    actual.advance(0, 1, None);
    expected.advance(0, 1, None);
    let reservation = [Reservation {
        requester: Requester::VDP,
        ticks: 1..90,
    }];
    for rig in [&mut actual, &mut expected] {
        let mut interval = SoundInterval::new(1..70, &mut rig.audio);
        interval.occupied = &reservation;
        rig.schedule.advance(
            &mut rig.cpu,
            &mut rig.memory,
            &mut rig.cartridge,
            &mut rig.ram,
            interval,
        );
    }
    equal(&actual, &expected);
    assert!(actual.schedule.pending.is_some());
    assert!(actual.schedule.epoch_deadline().is_none());
}

mod trace;

mod trace_lifecycle;
