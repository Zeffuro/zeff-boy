use super::*;
use crate::audio_trace::{AudioTraceEvent, AudioTraceTiming};

fn chip() -> GbaAudioTraceChip {
    GbaAudioTraceChip {
        clock_hz: GBA_AUDIO_TRACE_CLOCK_HZ,
        reset: GbaAudioTraceReset::PostBiosV1,
        source_sha256: [0xA5; 32],
    }
}

#[test]
fn terminal_state_is_retained_in_cycle_order() {
    let mut recorder = GbaAudioTraceRecorder::default()
        .prepare(
            2,
            GBA_AUDIO_TRACE_CLOCK_HZ,
            chip(),
            AudioTraceTiming::BusServiceBoundary,
        )
        .unwrap();
    recorder.record(AudioTraceEvent {
        cycle: 4,
        pc: 0,
        instruction_source: crate::audio_trace::AudioTraceSource::Unknown,
        write: GbaAudioTraceWrite::Terminal {
            fifo_a: GbaAudioTraceFifoState {
                queue: [0; 32],
                len: 0,
                current: 3,
            },
            fifo_b: GbaAudioTraceFifoState {
                queue: [0; 32],
                len: 0,
                current: -2,
            },
            origin: GbaAudioTraceOrigin::Unknown,
        },
    });
    let trace = recorder.finish(4).unwrap();
    trace.validate_complete().unwrap();
    assert_eq!(trace.chip.source_sha256, [0xA5; 32]);
    assert_eq!(trace.events.len(), 1);
}

#[test]
fn fixed_clock_and_dma_shape_are_explicit() {
    let origin = GbaAudioTraceOrigin::Dma(GbaAudioTraceDma {
        channel: 1,
        kind: GbaAudioTraceDmaKind::Fifo,
        requested_source: 0x0200_0003,
        aligned_source: 0x0200_0000,
        width: 4,
        value: 0x807F_0201,
        source_latched: false,
        source_lanes: [
            GbaAudioTraceSource::Ewram { offset: 0 },
            GbaAudioTraceSource::Ewram { offset: 1 },
            GbaAudioTraceSource::Ewram { offset: 2 },
            GbaAudioTraceSource::Ewram { offset: 3 },
        ],
    });
    assert_eq!(GBA_AUDIO_TRACE_CLOCK_HZ, 16_777_216);
    assert!(matches!(origin, GbaAudioTraceOrigin::Dma(_)));
}
