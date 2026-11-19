use super::*;
use crate::hardware::audio::{
    Audio,
    trace::{main_origin, trace_chip},
};
use zeff_emu_common::audio_trace::{AudioTraceInvalidation, AudioTraceTiming};

pub(super) fn fixture(region: Region, program: &[u8], cut: u64, epoch: bool, rate: u32) -> Rig {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0x80, 0, 0, 0, 1, 0]);
    for word in image[0x100..].as_chunks_mut::<2>().0 {
        word.copy_from_slice(&0x4e71u16.to_be_bytes());
    }
    let mut bus = Bus::new(Cartridge::new(image).unwrap(), region);
    std::sync::Arc::make_mut(&mut bus.cartridge.bytes)[4..8]
        .copy_from_slice(&0x00ff0200u32.to_be_bytes());
    bus.ram[0x200..0x20c].copy_from_slice(&[
        0x52, 0x79, 0, 0xff, 3, 0, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71,
    ]);
    bus.ram[0x300..0x302].copy_from_slice(&0x1234u16.to_be_bytes());
    bus.sound.set_reset(false);
    bus.sound.ram[..program.len()].copy_from_slice(program);
    bus.audio = Audio::new(region);
    bus.audio.set_sample_rate(rate);
    for value in [0x84, 1, 0x90] {
        bus.audio.write_psg(0, value);
    }
    for (port, value) in [(0, 0x2a), (1, 0xd3), (0, 0x2b), (1, 0x80)] {
        bus.audio.write_opn2(0, port, value);
    }
    let mut rig = Rig {
        bus,
        sound: Cpu::new(),
        schedule: SoundSchedule::default(),
    };
    if epoch {
        rig.schedule.begin_epoch_field();
    }
    rig.schedule.advance(
        &mut rig.sound,
        &mut rig.bus.sound,
        &mut rig.bus.cartridge,
        &mut rig.bus.ram,
        SoundInterval::new(0..cut, &mut rig.bus.audio),
    );
    assert_eq!(rig.schedule.fault, None);
    rig.bus.advance_machine(cut).unwrap();
    rig.bus.cpu_clock = CpuClocks(23);
    rig.bus.irq_level = 3;
    rig
}

pub(super) fn trace(rig: &mut Rig, enabled: bool) {
    if enabled {
        rig.bus.audio.trace = rig
            .bus
            .audio
            .trace
            .prepare_with_clock(
                32,
                rig.bus.rate(),
                trace_chip(rig.bus.region),
                AudioTraceTiming::BusServiceBoundary,
            )
            .unwrap();
    }
    rig.bus.audio.trace_origin = Some(main_origin(0x100, &rig.bus.cartridge));
    rig.bus.audio.write_psg(rig.bus.now.get(), 0x91);
}

pub(super) fn entered(outcome: Outcome) -> Report {
    match outcome {
        Outcome::Entered(report) => *report,
        other => panic!("{other:?}"),
    }
}

pub(super) fn clean(rig: &mut Rig, enabled: bool, reason: AudioTraceInvalidation) {
    assert!(!rig.bus.audio.batch_active());
    assert!(rig.schedule.epoch.is_none());
    assert!(rig.bus.audio.trace_origin.is_none());
    let mut recorder = rig.bus.audio.trace.clone();
    let result = recorder.finish(rig.bus.now.get());
    assert_eq!(result.is_some(), enabled);
    if let Some(trace) = result {
        assert_eq!(trace.invalidated, Some(reason));
        assert_eq!(trace.events.len(), 1);
    }
    let before = rig.clone();
    rig.bus.audio.begin_batch();
    rig.bus.audio.finish_batch();
    assert_eq!(*rig, before);
}

pub(super) fn callback_control(rig: &mut Rig, concurrent: bool) {
    fn apply(bus: &mut impl ClockedBus<Error = BusError>, mut clock: CpuClocks) {
        bus.idle(clock, 14);
        clock.0 += 14;
        for (index, address) in [0, 2, 4, 6, 0xff0200, 0xff0202].into_iter().enumerate() {
            if index == 5 {
                bus.idle(clock, 2);
                clock.0 += 2;
            }
            let response = bus.read_cycle(
                BusAccess {
                    logical_address: address,
                    bus_address: address,
                    function_code: FunctionCode::SupervisorProgram,
                    kind: if index < 4 {
                        AccessKind::ResetVector
                    } else {
                        AccessKind::Prefetch
                    },
                    lanes: ByteLanes::Both,
                },
                clock,
            );
            assert_eq!(response.wait_clocks, 0);
            assert_eq!(
                response.result,
                Ok([0x00ff, 0x8000, 0x00ff, 0x0200, 0x5279, 0x00ff][index])
            );
            clock.0 += 4;
        }
    }
    rig.bus
        .audio
        .invalidate_trace(AudioTraceInvalidation::ExternalMutation);
    rig.schedule.finish_epoch_field(
        &mut rig.sound,
        &rig.bus.sound,
        &mut crate::sound_trace::NoSoundCapture,
    );
    let clock = rig.bus.cpu_clock;
    if concurrent {
        rig.bus.audio.begin_batch();
        apply(&mut rig.scheduled(), clock);
        rig.bus.audio.finish_batch();
    } else {
        apply(&mut rig.bus, clock);
    }
    assert!(rig.schedule.epoch.is_none());
    assert!(!rig.bus.audio.batch_active());
    rig.bus.audio.trace_origin = None;
}
