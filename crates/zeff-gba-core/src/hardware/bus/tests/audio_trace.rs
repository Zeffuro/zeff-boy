use super::*;
use zeff_emu_common::audio_trace::{
    GbaAudioTraceDmaKind, GbaAudioTraceOrigin, GbaAudioTraceSource, GbaAudioTraceWrite,
    GbaDirectSoundFifo,
};

#[test]
fn audio_trace_keeps_applied_word_lanes_reset_causality_and_terminal_state() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.begin_audio_trace(16, [0x11; 32]).unwrap();
    bus.set_audio_trace_cpu_active_pc(0x0800_0000);
    bus.write32(0x0400_00A0, 0x807F_0201);
    bus.write16(0x0400_0082, 1 << 11);
    let trace = bus.finish_audio_trace().unwrap();

    assert_eq!(trace.chip.source_sha256, [0x11; 32]);
    assert!(matches!(
        trace.events[0].write,
        GbaAudioTraceWrite::FifoHalfword {
            fifo: GbaDirectSoundFifo::A,
            value: 0x0201,
            access,
            origin: GbaAudioTraceOrigin::Cpu {
                active_pc: 0x0800_0000
            },
        } if access.address == 0x0400_00A0 && access.width == 4 && access.halfword_lane == 0
    ));
    assert!(matches!(
        trace.events[1].write,
        GbaAudioTraceWrite::FifoHalfword {
            value: 0x807F,
            access,
            ..
        } if access.halfword_lane == 2
    ));
    assert!(matches!(
        trace.events[2].write,
        GbaAudioTraceWrite::Control {
            address: 0x82,
            raw_value: 0x0800,
            io_value: 0,
            ..
        }
    ));
    assert!(matches!(
        trace.events[3].write,
        GbaAudioTraceWrite::FifoReset {
            fifo: GbaDirectSoundFifo::A,
            ..
        }
    ));
    assert!(matches!(
        trace.events.last().unwrap().write,
        GbaAudioTraceWrite::Terminal { .. }
    ));
    trace.validate_complete().unwrap();
}

#[test]
fn audio_trace_records_pop_before_fifo_dma_refill_with_ram_lanes() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.begin_audio_trace(64, [0x22; 32]).unwrap();
    bus.set_audio_trace_cpu_active_pc(0x0800_0000);
    for index in 0..4 {
        bus.write32(0x0200_0000 + index * 4, 0x0403_0201 + index * 0x0404_0404);
    }
    bus.write16(0x0400_0082, (1 << 2) | (1 << 8) | (1 << 9) | (1 << 11));
    bus.write32(0x0400_00BC, 0x0200_0000);
    bus.write32(0x0400_00C0, 0x0400_00A0);
    bus.write16(0x0400_00C4, 4);
    bus.write16(0x0400_00C6, 0xB600);
    bus.write16(0x0400_0100, 0xFFFF);
    bus.write16(0x0400_0102, 0x0080);
    bus.clear_audio_trace_cpu_origin();
    bus.step_cycles(2);
    let trace = bus.finish_audio_trace().unwrap();

    let pop = trace
        .events
        .iter()
        .position(|event| matches!(event.write, GbaAudioTraceWrite::FifoPop { .. }))
        .unwrap();
    let refill = trace
        .events
        .iter()
        .position(|event| {
            matches!(
                event.write,
                GbaAudioTraceWrite::FifoHalfword {
                    origin: GbaAudioTraceOrigin::Dma(_),
                    ..
                }
            )
        })
        .unwrap();
    assert!(pop < refill);
    assert!(matches!(
        trace.events[pop].write,
        GbaAudioTraceWrite::FifoPop {
            fifo: GbaDirectSoundFifo::A,
            timer: 0,
            before_len: 0,
            after_len: 0,
            value: 0,
            underflow: true,
            ..
        }
    ));
    assert!(matches!(
        trace.events[refill].write,
        GbaAudioTraceWrite::FifoHalfword {
            value: 0x0201,
            access,
            origin: GbaAudioTraceOrigin::Dma(dma),
            ..
        } if dma.kind == GbaAudioTraceDmaKind::Fifo
            && dma.channel == 1
            && dma.requested_source == 0x0200_0000
            && dma.aligned_source == 0x0200_0000
            && dma.width == 4
            && !dma.source_latched
            && dma.source_lanes[0] == GbaAudioTraceSource::Ewram { offset: 0 }
            && access.halfword_lane == 0
    ));
    trace.validate_complete().unwrap();
}

#[test]
fn audio_trace_invalidates_before_master_cycle_wraps() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.begin_audio_trace(16, [0x33; 32]).unwrap();
    bus.master_cycles = u64::MAX;
    bus.step_cycles(1);

    assert_eq!(
        bus.finish_audio_trace().unwrap().invalidated,
        Some(zeff_emu_common::audio_trace::AudioTraceInvalidation::ClockOverflow)
    );
}
