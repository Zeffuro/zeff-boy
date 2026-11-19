use super::*;
use crate::constants::{MAIN_CPU_DIVISOR, SOUND_CPU_DIVISOR, VDP_LINE_TICKS};
use crate::hardware::bus::Prefetch;
use crate::hardware::sound::schedule::{PendingSound, SoundInterval};
use zeff_m68k::timing::CpuClocks;

fn ready(program: &[u8], region: Region) -> Emulator {
    let mut result = machine(&[0x4e71; 8]);
    result.bus.region = region;
    result.bus.audio = crate::hardware::audio::Audio::new(region);
    result.bus.sound.set_reset(false);
    result.bus.sound.ram[..program.len()].copy_from_slice(program);
    result.concurrent_sound = true;
    result
}

fn pending(program: &[u8], at: u64) -> Emulator {
    let mut result = ready(program, Region::NtscUs);
    result.sound_schedule.pending = Some(
        PendingSound::prepare(
            &result.sound_cpu,
            &result.bus.sound,
            &result.bus.cartridge,
            at,
        )
        .unwrap(),
    );
    result
}

fn position(machine: &mut Emulator, at: u64) {
    machine
        .bus
        .advance_machine(at - machine.bus.now.get())
        .unwrap();
    machine.bus.prefetch = Prefetch::Full(0xa55a);
}

fn prepare_epoch(machine: &mut Emulator) {
    machine.sound_schedule.begin_epoch_field();
    let begin = machine.bus.now.get();
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.cartridge,
        &mut machine.bus.ram,
        SoundInterval::new(begin..begin + 1, &mut machine.bus.audio),
    );
    machine.bus.advance_machine(1).unwrap();
    assert!(machine.sound_schedule.epoch.is_some());
    assert!(machine.sound_schedule.pending.is_none());
}

fn pair(
    actual: &mut Emulator,
    reference: &mut Emulator,
    start: CpuClocks,
    clocks: u32,
    occupied: bool,
    acknowledge: bool,
) -> Result<(), BusError> {
    let result = ScheduledBus::new(
        &mut actual.bus,
        &mut actual.sound_cpu,
        &mut actual.sound_schedule,
    )
    .advance_main_cycle_candidate(start, clocks, occupied, acknowledge);
    let expected = ScheduledBus::new(
        &mut reference.bus,
        &mut reference.sound_cpu,
        &mut reference.sound_schedule,
    )
    .advance_main_cycle_reference(start, clocks, occupied, acknowledge);
    assert_eq!(result, expected);
    assert_eq!(actual, reference);
    result
}

#[test]
fn ordinary_local_reads_writes_and_status_ties_cover_all_sound_residues() {
    for phase in 0..SOUND_CPU_DIVISOR {
        for program in [[0x3a, 0, 1], [0x32, 0, 1], [0x3a, 0, 0x40]] {
            for offset in [-1i64, 0, 1] {
                let mut actual = pending(&program, 300 + phase);
                actual.bus.sound.ram[0x100] = 0x5a;
                let deadline = actual.sound_schedule.pending.as_ref().unwrap().deadline;
                let end = deadline.checked_add_signed(offset).unwrap();
                position(&mut actual, end - MAIN_CPU_DIVISOR);
                assert_eq!(actual.sound_schedule.local_pending_after(end), offset < 0);
                let mut reference = actual.clone();
                let start = actual.bus.cpu_clock;
                pair(&mut actual, &mut reference, start, 1, true, false).unwrap();
                assert_eq!(actual.bus.now.get(), end);
                if offset < 0 {
                    assert_eq!(actual.bus.prefetch, Prefetch::Empty);
                    assert_eq!(actual.sound_cpu.cycles(), 0);
                    assert_eq!(actual.bus.sound.ram[0x100], 0x5a);
                } else {
                    assert!(actual.sound_cpu.cycles() > 0);
                    if program[0] == 0x32 {
                        assert_eq!(actual.bus.sound.ram[0x100], 0);
                    }
                }
                let start = actual.bus.cpu_clock;
                pair(&mut actual, &mut reference, start, 30, false, false).unwrap();
            }
        }
    }
}

#[test]
fn prepared_epoch_hits_then_due_misses_preserve_every_internal_record() {
    for phase in 0..SOUND_CPU_DIVISOR {
        for offset in [-1i64, 0, 1] {
            let mut actual = ready(&[0; 16], Region::NtscUs);
            position(&mut actual, 300 + phase);
            let deadline = (300 + phase) / SOUND_CPU_DIVISOR * SOUND_CPU_DIVISOR + 60;
            prepare_epoch(&mut actual);
            let end = deadline.checked_add_signed(offset).unwrap();
            position(&mut actual, end - MAIN_CPU_DIVISOR);
            assert_eq!(actual.sound_schedule.local_pending_after(end), offset < 0);
            let mut reference = actual.clone();
            let start = actual.bus.cpu_clock;
            pair(&mut actual, &mut reference, start, 1, true, false).unwrap();
            let start = actual.bus.cpu_clock;
            pair(&mut actual, &mut reference, start, 70, false, false).unwrap();
            actual.sound_schedule.finish_epoch_field(
                &mut actual.sound_cpu,
                &actual.bus.sound,
                &mut crate::sound_trace::NoSoundCapture,
            );
            reference.sound_schedule.finish_epoch_field(
                &mut reference.sound_cpu,
                &reference.bus.sound,
                &mut crate::sound_trace::NoSoundCapture,
            );
            assert_eq!(actual, reference);
        }
    }
}

#[test]
fn prepared_hit_crosses_irq_edge_then_due_preparation_samples_high_in_each_region() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for frame in [0, 2] {
            let mut actual = ready(&[0; 32], region);
            let edge = (frame * region.frame_lines() + 224) * VDP_LINE_TICKS + 40;
            position(&mut actual, edge - 20);
            prepare_epoch(&mut actual);
            let end = actual.bus.now.get() + 3 * MAIN_CPU_DIVISOR;
            assert!(end >= edge);
            assert!(actual.sound_schedule.local_pending_after(end));
            let mut reference = actual.clone();
            let start = actual.bus.cpu_clock;
            pair(&mut actual, &mut reference, start, 3, false, false).unwrap();
            let start = actual.bus.cpu_clock;
            pair(&mut actual, &mut reference, start, 8, false, false).unwrap();
            assert!(
                actual
                    .sound_schedule
                    .pending
                    .as_ref()
                    .unwrap()
                    .memory
                    .irq_asserted
            );
            assert!(!actual.bus.sound.irq_asserted);
            let start = actual.bus.cpu_clock;
            pair(&mut actual, &mut reference, start, 20, false, true).unwrap();
        }
    }
}

#[test]
fn bank_markers_fault_and_missing_preparation_refuse_admission() {
    for kind in 0..6 {
        let mut actual = pending(&[0x3a, 0, 1], 300);
        match kind {
            0 => actual.sound_schedule.pending = None,
            1 => actual.sound_schedule.fault = Some(SoundFault::ClockOverflow),
            2 => {
                actual
                    .sound_schedule
                    .pending
                    .as_mut()
                    .unwrap()
                    .any_bank_access = true
            }
            3 => {
                actual.sound_schedule.pending.as_mut().unwrap().bank_access =
                    Some(crate::hardware::sound::BankAccess {
                        address: 0xff0000,
                        direction: zeff_m68k::bus::Direction::Read,
                        value: 0,
                    });
            }
            4 => {
                actual.sound_schedule.begin_epoch_field();
                actual.sound_schedule.pending = None;
            }
            _ => {
                actual = ready(&[0; 16], Region::NtscUs);
                position(&mut actual, 300);
                prepare_epoch(&mut actual);
                actual.sound_schedule.pending = pending(&[0x3a, 0, 1], 300).sound_schedule.pending;
            }
        }
        position(&mut actual, 350);
        assert!(!actual.sound_schedule.local_pending_after(357));
        let mut reference = actual.clone();
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 1, true, false).unwrap();
    }
}

#[test]
fn audio_immediate_deferred_disabled_capacity_and_trace_paths_match() {
    for mode in 0..5 {
        let mut actual = pending(&[0x3a, 0, 1], 300);
        position(&mut actual, 350);
        match mode {
            1 => actual.bus.audio.begin_batch(),
            2 => {
                actual.bus.audio.begin_batch();
                actual.bus.audio.set_sample_generation_enabled(false);
            }
            3 => {
                actual.bus.audio.begin_batch();
                actual.sound_schedule.pending.as_mut().unwrap().deadline = 10_000_000;
            }
            4 => {
                let mut traced = machine(&[0x4e71; 8]);
                traced.reset_and_begin_audio_trace(64).unwrap();
                actual.bus.audio.trace = traced.bus.audio.trace;
                assert!(actual.bus.audio.trace.is_enabled());
            }
            _ => {}
        }
        let clocks = if mode == 3 { 100_000 } else { 1 };
        assert!(
            actual
                .sound_schedule
                .local_pending_after(350 + u64::from(clocks) * 7)
        );
        let mut reference = actual.clone();
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, clocks, true, false).unwrap();
        if matches!(mode, 1..=3) {
            actual.bus.audio.finish_batch();
            reference.bus.audio.finish_batch();
        }
        assert_eq!(
            actual.drain_audio_samples(),
            reference.drain_audio_samples()
        );
        assert_eq!(actual, reference);
    }
}

#[test]
fn clock_empty_fault_and_control_boundaries_preserve_original_errors_and_prefetch() {
    for case in 0..9 {
        let mut actual = pending(&[0x32, 0, 1], 300);
        position(&mut actual, 350);
        let mut start = actual.bus.cpu_clock;
        let clocks = match case {
            0 => 0,
            1 => {
                start.0 += 1;
                1
            }
            2 => {
                actual.bus.clock_fault = true;
                1
            }
            3 => {
                actual.bus.now = MasterTicks::new(u64::MAX - 1);
                1
            }
            4 => {
                actual.bus.cpu_clock = CpuClocks(u64::MAX);
                start = actual.bus.cpu_clock;
                1
            }
            5 => {
                actual.bus.sound.request(true);
                1
            }
            6 => {
                actual.bus.sound.set_reset(true);
                1
            }
            7 => {
                actual.bus.sound.request(true);
                actual.bus.sound.complete_boundary();
                1
            }
            _ => {
                actual
                    .bus
                    .vdp
                    .advance_to(MasterTicks::new(358), Region::NtscUs)
                    .unwrap();
                assert!(actual.sound_schedule.local_pending_after(357));
                1
            }
        };
        let saved_audio = actual.bus.audio.clone();
        let mut reference = actual.clone();
        let result = pair(&mut actual, &mut reference, start, clocks, true, false);
        if matches!(case, 1..=4 | 8) {
            assert!(result.is_err());
            if case == 8 {
                assert!(matches!(result, Err(BusError::Vdp(_))));
            }
            assert_eq!(actual.bus.audio, saved_audio);
        } else {
            result.unwrap();
            if case == 0 || actual.bus.sound.granted || actual.bus.sound.reset_asserted {
                assert_eq!(actual.bus.prefetch, Prefetch::Full(0xa55a));
            }
        }
    }
}

#[test]
fn pending_mode_publication_and_due_next_bank_or_fault_preserve_preparation() {
    for (index, value) in [(1u8, 0x24u8), (12, 0x81)] {
        let mut actual = ready(&[0; 32], Region::NtscUs);
        let edge = 2 * Region::NtscUs.frame_lines() * VDP_LINE_TICKS + 224 * VDP_LINE_TICKS + 40;
        position(&mut actual, edge - 20);
        prepare_epoch(&mut actual);
        actual
            .bus
            .vdp
            .write_control(
                0x8000 | (u16::from(index) << 8) | u16::from(value),
                Region::NtscUs,
            )
            .unwrap();
        let write = actual.vdp().registers().pending().unwrap();
        assert!(actual.vdp().registers().pending().is_some());
        let end = actual.bus.now.get() + 3 * MAIN_CPU_DIVISOR;
        assert!(write.publish_at.get() <= end);
        assert!(actual.sound_schedule.local_pending_after(end));
        let mut reference = actual.clone();
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 3, false, false).unwrap();
        assert!(actual.vdp().registers().pending().is_none());
        assert_eq!(
            actual.vdp().registers().registers()[usize::from(index)],
            Some(value)
        );
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 8, false, false).unwrap();
        assert!(
            actual
                .sound_schedule
                .pending
                .as_ref()
                .unwrap()
                .memory
                .irq_asserted
        );
    }
    for high in [0x80, 0x7f] {
        let mut actual = ready(&[0, 0, 0, 0, 0x3a, 0, high, 0x76], Region::NtscUs);
        actual.bus.sound.bank = 0x1ff;
        actual.bus.ram[0x8000] = 0x5a;
        position(&mut actual, 300);
        prepare_epoch(&mut actual);
        assert!(actual.sound_schedule.local_pending_after(308));
        let mut reference = actual.clone();
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 1, true, false).unwrap();
        assert!(actual.sound_schedule.fault.is_none());
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 55, false, false).unwrap();
        assert!(
            !actual
                .sound_schedule
                .local_pending_after(actual.bus.now.get() + 7)
        );
        if high == 0x80 {
            let pending = actual.sound_schedule.pending.as_ref().unwrap();
            assert_eq!((pending.start, pending.deadline), (690, 735));
            assert!(pending.any_bank_access);
            assert!(pending.reservation().is_some());
            assert_eq!(pending.bank_access.unwrap().address, 0xff8000);
        } else {
            assert!(matches!(
                actual.sound_schedule.fault,
                Some(SoundFault::UnsupportedAccess {
                    address: 0x7f00,
                    ..
                })
            ));
        }
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 30, false, false).unwrap();
        if high == 0x80 {
            assert_eq!(
                actual.sound_cpu.regs().a,
                crate::constants::SOUND_DISCONNECTED_BYTE
            );
            assert_eq!(actual.bus.ram[0x8000], 0x5a);
            assert!(actual.sound_schedule.fault.is_none());
        }
    }
}

#[test]
fn actual_external_dma_materializes_prepared_epoch_before_sound_progress() {
    for h40 in [false, true] {
        let mut actual = super::vdp_line::seed(true, 0x4e71);
        for (index, value) in [
            (12, if h40 { 0x81 } else { 0 }),
            (1, 0x54),
            (19, 17),
            (20, 0),
            (21, 0),
            (22, 0xc0),
            (23, 0x7f),
        ] {
            actual.bus.advance_machine(1).unwrap();
            let write = actual
                .bus
                .vdp
                .queue_register(index, value, Region::NtscUs)
                .unwrap();
            actual
                .bus
                .advance_machine(write.publish_at.get() - actual.bus.now.get())
                .unwrap();
        }
        actual.bus.sound.set_reset(false);
        actual.bus.sound.ram[..16].fill(0);
        prepare_epoch(&mut actual);
        actual
            .bus
            .vdp
            .write_control(0x4400, Region::NtscUs)
            .unwrap();
        actual.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
        assert!(
            actual
                .bus
                .vdp
                .external_dma_release(
                    Region::NtscUs,
                    crate::hardware::vdp::VdpDmaSource::new(
                        &actual.bus.cartridge.bytes,
                        &actual.bus.ram
                    ),
                )
                .unwrap()
                .is_some()
        );
        assert!(
            actual
                .sound_schedule
                .local_pending_after(actual.bus.now.get() + 7)
        );
        let mut reference = actual.clone();
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 1, true, false).unwrap();
        assert!(actual.sound_schedule.pending.is_some());
        let start = actual.bus.cpu_clock;
        pair(&mut actual, &mut reference, start, 300, false, false).unwrap();
    }
}
