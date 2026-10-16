use super::*;
use crate::audio_trace::{AudioTraceEvent, AudioTraceSource, AudioTraceTiming};

fn recorder() -> MdAudioTraceRecorder {
    MdAudioTraceRecorder::default()
        .prepare(
            3,
            53_693_175,
            MdTraceChip {
                master_hz: 53_693_175,
                region: MdTraceRegion::NtscUs,
                psg_divisor: 240,
                opn2_divisor: 7,
                reset: MdTraceReset::ZeffPowerOnV1,
            },
            AudioTraceTiming::BusServiceBoundary,
        )
        .unwrap()
}

#[test]
fn same_tick_mixed_chip_and_writer_order_is_retained() {
    let main = MdTraceOrigin {
        writer: MdTraceWriter::M68000,
        boundary_pc: 0x200,
        source: MdTraceSource::EffectiveRom { offset: 0x200 },
    };
    let sound = MdTraceOrigin {
        writer: MdTraceWriter::Z80,
        boundary_pc: 0x40,
        source: MdTraceSource::SoundRam { offset: 0x40 },
    };
    let events = [
        AudioTraceEvent {
            cycle: 420,
            pc: main.boundary_pc,
            instruction_source: AudioTraceSource::Unknown,
            write: MdTraceWrite::Opn2Port {
                origin: main,
                port: 2,
                value: 0x2a,
            },
        },
        AudioTraceEvent {
            cycle: 420,
            pc: sound.boundary_pc,
            instruction_source: AudioTraceSource::Unknown,
            write: MdTraceWrite::Psg {
                origin: sound,
                value: 0x9f,
            },
        },
        AudioTraceEvent {
            cycle: 420,
            pc: main.boundary_pc,
            instruction_source: AudioTraceSource::Unknown,
            write: MdTraceWrite::Opn2Reset { origin: main },
        },
    ];
    let mut recorder = recorder();
    for event in events {
        recorder.record(event);
    }
    let trace = recorder.finish(420).unwrap();
    trace.validate_complete().unwrap();
    assert_eq!(trace.events, events);
    assert_eq!(trace.chip.clock_numerator_hz(), 53_693_175);
    assert_eq!(trace.chip.clock_denominator(), 1);
}

#[test]
fn capture_retains_effective_input_identity_and_clone_ownership() {
    let capture = MdAudioTraceCapture {
        effective_rom_sha256: [0xa5; 32],
        effective_rom_len: 512,
        trace: recorder().finish(0).unwrap(),
    };
    let mut cloned = capture.clone();
    cloned.effective_rom_sha256[0] = 0;
    cloned.effective_rom_len = 1024;
    assert_eq!(capture.effective_rom_sha256, [0xa5; 32]);
    assert_eq!(capture.effective_rom_len, 512);
    assert_ne!(capture, cloned);
}
