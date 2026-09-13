use std::sync::atomic::AtomicBool;

use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, AudioTraceStart, AudioTraceTiming,
    WonderSwanAudioTrace, WonderSwanResetState, WonderSwanTraceChip, WonderSwanTraceOrigin,
    WonderSwanTraceWrite,
};

use super::{VgmCapture, VgmCaptureSource, encode_wonderswan};

const HEADER_LEN: usize = 0x100;
const PREAMBLE_WRITES: usize = 0x4000 + 28;
const PREAMBLE_BYTES: usize = 0x4000 * 4 + 28 * 3;

fn trace() -> WonderSwanAudioTrace {
    WonderSwanAudioTrace {
        generation: 23,
        cycle_hz: 3_072_000,
        cycle_hz_denominator: 1,
        chip: WonderSwanTraceChip {
            clock_hz: 3_072_000,
            color: true,
            reset: WonderSwanResetState::default(),
        },
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 0,
        events: Vec::new(),
        dropped_events: 0,
        invalidated: None,
    }
}

fn event(cycle: u64, write: WonderSwanTraceWrite) -> AudioTraceEvent<WonderSwanTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc: 0x1234,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: 0x2040,
            bit_reversed: false,
        },
        write,
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

struct Decoded {
    registers: Vec<(u64, u8, u8)>,
    wave_ram: Vec<(u64, u16, u8)>,
    waits: Vec<u16>,
    samples: u64,
}

fn decode(capture: &VgmCapture) -> Decoded {
    let bytes = &capture.bytes;
    assert_eq!(&bytes[..4], b"Vgm ");
    assert_eq!(word(bytes, 4) as usize + 4, bytes.len());
    assert_eq!(word(bytes, 8), 0x171);
    assert_eq!(word(bytes, 0x34), 0xcc);
    assert_eq!(word(bytes, 0xc0), 3_072_000);
    let mut offset = HEADER_LEN;
    let mut decoded = Decoded {
        registers: Vec::new(),
        wave_ram: Vec::new(),
        waits: Vec::new(),
        samples: 0,
    };
    loop {
        match bytes[offset] {
            0xbc => {
                decoded
                    .registers
                    .push((decoded.samples, bytes[offset + 1], bytes[offset + 2]));
                offset += 3;
            }
            0xc6 => {
                decoded.wave_ram.push((
                    decoded.samples,
                    u16::from_be_bytes([bytes[offset + 1], bytes[offset + 2]]),
                    bytes[offset + 3],
                ));
                offset += 4;
            }
            0x61 => {
                let wait = u16::from_le_bytes(bytes[offset + 1..offset + 3].try_into().unwrap());
                assert_ne!(wait, 0);
                decoded.waits.push(wait);
                decoded.samples += u64::from(wait);
                offset += 3;
            }
            0x66 => {
                assert_eq!(offset + 1, bytes.len());
                break;
            }
            opcode => panic!("unexpected VGM opcode: {opcode:02x}"),
        }
    }
    assert_eq!(word(bytes, 0x18), decoded.samples as u32);
    assert_eq!(capture.metadata.total_samples, decoded.samples as u32);
    assert_eq!(
        capture.metadata.command_count as usize,
        decoded.registers.len() + decoded.wave_ram.len() + decoded.waits.len() + 1
    );
    decoded
}

fn encode(trace: &WonderSwanAudioTrace) -> VgmCapture {
    trace.encode_vgm(&AtomicBool::new(false)).unwrap()
}

#[test]
fn reset_preamble_zeroes_all_wave_ram_before_ordinary_sound_ports() {
    let capture = encode(&trace());
    let decoded = decode(&capture);
    assert_eq!(
        capture.metadata.preamble_write_count as usize,
        PREAMBLE_WRITES
    );
    assert_eq!(capture.metadata.guest_write_count, 0);
    assert_eq!(capture.bytes.len(), HEADER_LEN + PREAMBLE_BYTES + 1);
    assert_eq!(decoded.wave_ram.len(), 0x4000);
    assert_eq!(decoded.registers.len(), 28);
    assert_eq!(
        decoded.wave_ram,
        (0..0x4000)
            .map(|address| (0, address, 0))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        decoded.registers,
        (0..28).map(|port| (0, port, 0)).collect::<Vec<_>>()
    );
    let metadata = capture.metadata.wonder_swan.as_ref().unwrap();
    assert_eq!(metadata.model, "wonderswan_color");
    assert_eq!(metadata.master_clock_hz, 3_072_000);
    assert_eq!(metadata.wave_ram_bytes, 0x4000);
    assert!(metadata.canonical_reset.contains("zero"));
    assert!(metadata.hyper_voice.contains("reject"));
    assert!(metadata.sound_test.contains("fast-sweep"));
}

#[test]
fn mixed_commands_use_native_port_offsets_big_endian_memory_and_absolute_time() {
    let mut source = trace();
    source.end_cycle = 3_072_010;
    source.events = vec![
        event(
            0,
            WonderSwanTraceWrite::Register {
                port: 0x80,
                value: 0x12,
                origin: WonderSwanTraceOrigin::Cpu,
            },
        ),
        event(
            0,
            WonderSwanTraceWrite::WaveRam {
                address: 0x1234,
                value: 0x56,
                origin: WonderSwanTraceOrigin::GeneralDma,
            },
        ),
        event(
            3_072_000,
            WonderSwanTraceWrite::Register {
                port: 0x95,
                value: 0x34,
                origin: WonderSwanTraceOrigin::CpuInterrupt,
            },
        ),
        event(
            3_072_000,
            WonderSwanTraceWrite::Register {
                port: 0x89,
                value: 0x78,
                origin: WonderSwanTraceOrigin::SoundDma,
            },
        ),
    ];
    let capture = encode(&source);
    let decoded = decode(&capture);
    assert_eq!(
        &decoded.registers[28..],
        &[(0, 0, 0x12), (44_100, 0x15, 0x34), (44_100, 9, 0x78)]
    );
    assert_eq!(decoded.wave_ram[0x4000], (0, 0x1234, 0x56));
    assert_eq!(decoded.samples, 44_100);
    assert_eq!(capture.metadata.guest_write_count, 4);
    assert_eq!(capture.metadata.timing, "bus_service_boundary");
    let log = crate::vgm::inspect(
        &capture.bytes,
        crate::ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(log.warnings.is_empty());
    assert_eq!(log.samples, 44_100);
    assert_eq!(log.chips[0].name, "wonderswan");
}

#[test]
fn invalid_or_unrepresentable_traces_reject() {
    for invalid in 0..19 {
        let mut source = trace();
        source.events.push(event(
            0,
            WonderSwanTraceWrite::Register {
                port: 0x80,
                value: 0,
                origin: WonderSwanTraceOrigin::Cpu,
            },
        ));
        match invalid {
            0 => source.cycle_hz = 0,
            1 => source.cycle_hz_denominator = 2,
            2 => source.chip.clock_hz = 1,
            3 => source.timing = AudioTraceTiming::IoWriteCompletion,
            4 => source.chip.reset.wave_ram[0] = 1,
            5 => {
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x7f,
                    value: 0,
                    origin: WonderSwanTraceOrigin::Cpu,
                }
            }
            6 => {
                source.events[0].write = WonderSwanTraceWrite::WaveRam {
                    address: 0x4000,
                    value: 0,
                    origin: WonderSwanTraceOrigin::GeneralDma,
                }
            }
            7 => {
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x64,
                    value: 0,
                    origin: WonderSwanTraceOrigin::Cpu,
                }
            }
            8 => {
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x69,
                    value: 0,
                    origin: WonderSwanTraceOrigin::SoundDma,
                }
            }
            9 => {
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x80,
                    value: 0,
                    origin: WonderSwanTraceOrigin::SoundDma,
                }
            }
            10 => {
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x95,
                    value: 0x02,
                    origin: WonderSwanTraceOrigin::Cpu,
                }
            }
            11 => source.dropped_events = 1,
            12 => source.invalidated = Some(AudioTraceInvalidation::StateRestore),
            13 => source.events[0].cycle = 1,
            14 => {
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x96,
                    value: 0,
                    origin: WonderSwanTraceOrigin::Cpu,
                }
            }
            15 => {
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x89,
                    value: 0,
                    origin: WonderSwanTraceOrigin::GeneralDma,
                }
            }
            16 => {
                source.events[0].write = WonderSwanTraceWrite::WaveRam {
                    address: 0,
                    value: 0,
                    origin: WonderSwanTraceOrigin::SoundDma,
                }
            }
            17 => {
                source.chip.color = false;
                source.events[0].write = WonderSwanTraceWrite::Register {
                    port: 0x89,
                    value: 0,
                    origin: WonderSwanTraceOrigin::SoundDma,
                }
            }
            18 => {
                source.chip.color = false;
                source.events[0].write = WonderSwanTraceWrite::WaveRam {
                    address: 0,
                    value: 0,
                    origin: WonderSwanTraceOrigin::GeneralDma,
                }
            }
            _ => unreachable!(),
        }
        assert!(
            encode_wonderswan(&source, &AtomicBool::new(false)).is_err(),
            "invalid case {invalid}"
        );
    }
    assert!(encode_wonderswan(&trace(), &AtomicBool::new(true)).is_err());
}

#[test]
fn hyper_voice_refusals_identify_direct_io_and_sound_dma_separately() {
    for port in 0x64..=0x6b {
        let mut direct = trace();
        direct.events.push(event(
            0,
            WonderSwanTraceWrite::Register {
                port,
                value: 0,
                origin: WonderSwanTraceOrigin::Cpu,
            },
        ));
        assert!(
            encode_wonderswan(&direct, &AtomicBool::new(false))
                .unwrap_err()
                .to_string()
                .contains("HyperVoice I/O")
        );
    }

    let mut sound_dma = trace();
    sound_dma.events.push(event(
        0,
        WonderSwanTraceWrite::Register {
            port: 0x69,
            value: 0,
            origin: WonderSwanTraceOrigin::SoundDma,
        },
    ));
    assert!(
        encode_wonderswan(&sound_dma, &AtomicBool::new(false))
            .unwrap_err()
            .to_string()
            .contains("Sound DMA targeted HyperVoice")
    );
}
