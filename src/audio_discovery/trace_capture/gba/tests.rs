use super::*;
use zeff_emu_common::audio_trace::*;

fn fixture() -> (Vec<u8>, GbaAudioTrace, Value) {
    let mut source = vec![0; 0x400];
    source[0xb2] = 0x96;
    source[0x200..0x204].copy_from_slice(&[0xff, 0x80, 1, 0x7f]);
    let origin = GbaAudioTraceOrigin::Dma(GbaAudioTraceDma {
        channel: 1,
        kind: GbaAudioTraceDmaKind::Fifo,
        requested_source: 0x0800_0200,
        aligned_source: 0x0800_0200,
        width: 4,
        value: 0x7f01_80ff,
        source_latched: false,
        source_lanes: std::array::from_fn(|lane| GbaAudioTraceSource::Rom {
            offset: 0x200 + lane as u32,
        }),
    });
    let event = |cycle, write| AudioTraceEvent {
        cycle,
        pc: 0,
        instruction_source: AudioTraceSource::Unknown,
        write,
    };
    let mut events = Vec::new();
    for lane in [0, 2] {
        events.push(event(
            1,
            GbaAudioTraceWrite::FifoHalfword {
                fifo: GbaDirectSoundFifo::A,
                value: (0x7f01_80ffu32 >> (lane * 8)) as u16,
                access: GbaAudioTraceAccess {
                    address: 0x0400_00a0,
                    width: 4,
                    halfword_lane: lane as u8,
                },
                origin,
            },
        ));
    }
    for (index, value) in [-1i8, -128, 1, 127, 0].into_iter().enumerate() {
        events.push(event(
            index as u64 + 2,
            GbaAudioTraceWrite::FifoPop {
                fifo: GbaDirectSoundFifo::A,
                timer: 0,
                effective_soundcnt_h: 0x300,
                before_len: 4u8.saturating_sub(index as u8),
                after_len: 3u8.saturating_sub(index as u8),
                value,
                underflow: index == 4,
                origin: GbaAudioTraceOrigin::Timer { timer: 0 },
            },
        ));
    }
    let empty = GbaAudioTraceFifoState {
        queue: [0; 32],
        len: 0,
        current: 0,
    };
    events.push(event(
        7,
        GbaAudioTraceWrite::Terminal {
            fifo_a: empty,
            fifo_b: empty,
            origin: GbaAudioTraceOrigin::Unknown,
        },
    ));
    let digest = zeff_firmware::sha256_hex(&source);
    let trace = GbaAudioTrace {
        generation: 1,
        cycle_hz: GBA_AUDIO_TRACE_CLOCK_HZ,
        cycle_hz_denominator: 1,
        chip: GbaAudioTraceChip {
            clock_hz: GBA_AUDIO_TRACE_CLOCK_HZ,
            reset: GbaAudioTraceReset::PostBiosV1,
            source_sha256: const_hex::decode_to_array(&digest).unwrap(),
        },
        timing: AudioTraceTiming::BusServiceBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 7,
        events,
        dropped_events: 0,
        invalidated: None,
    };
    let context = json!({"system":"gba", "firmware":null,
        "settings":{"system_start":"post_bios_core_reset"},
        "source":{"loaded_media":{"sha256":digest,"byte_len":source.len()}}});
    (source, trace, context)
}

#[test]
fn raw_fifo_export_preserves_signed_bytes_and_pop_provenance() -> Result<()> {
    let (source, trace, context) = fixture();
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("feed.zip");
    let cancel = AtomicBool::new(false);
    write_gba_new(&path, &trace, &source, context.clone(), &cancel)?;
    let bytes = std::fs::read(&path)?;
    let member = super::super::tests::member;
    assert_eq!(member(&bytes, "fifo-a.s8"), [0xff, 0x80, 1, 0x7f, 0]);
    assert!(member(&bytes, "fifo-b.s8").is_empty());
    let feed: Value = serde_json::from_slice(&member(&bytes, "feed.json"))?;
    assert_eq!(feed["pops"][2]["source"], json!({"event":1,"lane":0}));
    assert!(feed["pops"][4]["source"].is_null());
    assert!(write_gba_new(&path, &trace, &source, context, &cancel).is_err());
    assert_eq!(std::fs::read(path)?, bytes);
    Ok(())
}

#[test]
fn fifo_export_rejects_tampered_provenance_loss_and_cancellation() {
    let (source, trace, _) = fixture();
    let cancel = AtomicBool::new(false);
    for variant in 0..8 {
        let mut changed = trace.clone();
        match variant {
            0 => changed.events.swap(0, 1),
            1 => changed.dropped_events = 1,
            2 => {
                changed.events.pop();
            }
            3 => changed.events[2].cycle = 0,
            4 => changed.invalidated = Some(AudioTraceInvalidation::ExternalMutation),
            5 => {
                if let GbaAudioTraceWrite::FifoHalfword {
                    origin: GbaAudioTraceOrigin::Dma(ref mut dma),
                    ..
                } = changed.events[0].write
                {
                    dma.source_lanes[0] = GbaAudioTraceSource::Rom { offset: 0x201 };
                }
            }
            6 => {
                if let GbaAudioTraceWrite::FifoPop { ref mut value, .. } = changed.events[2].write {
                    *value = 17;
                }
            }
            7 => {
                for event in &mut changed.events[..2] {
                    if let GbaAudioTraceWrite::FifoHalfword {
                        origin: GbaAudioTraceOrigin::Dma(ref mut dma),
                        ..
                    } = event.write
                    {
                        dma.source_latched = true;
                        dma.kind = GbaAudioTraceDmaKind::Normal;
                        dma.source_lanes = [GbaAudioTraceSource::Latch; 4];
                    }
                }
            }
            _ => unreachable!(),
        }
        assert!(
            replay::replay(&changed, &source, &cancel).is_err(),
            "variant {variant}"
        );
    }
    let mut changed_source = source.clone();
    changed_source[0x200] ^= 1;
    assert!(replay::replay(&trace, &changed_source, &cancel).is_err());
    assert!(replay::replay(&trace, &source, &AtomicBool::new(true)).is_err());
}

#[test]
fn fifo_replay_evicts_oldest_bytes_and_requires_reset_causality() {
    let (source, mut trace, _) = fixture();
    let origin = GbaAudioTraceOrigin::Cpu {
        active_pc: 0x0800_00c0,
    };
    let access = GbaAudioTraceAccess {
        address: 0x0400_00a0,
        width: 2,
        halfword_lane: 0,
    };
    let cpu_event = |write| AudioTraceEvent {
        cycle: 1,
        pc: 0x0800_00c0,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: 0xc0,
            bit_reversed: false,
        },
        write,
    };
    trace.events.clear();
    for value in 0..20u16 {
        trace
            .events
            .push(cpu_event(GbaAudioTraceWrite::FifoHalfword {
                fifo: GbaDirectSoundFifo::A,
                value: value | (value << 8),
                access,
                origin,
            }));
    }
    trace.events.push(AudioTraceEvent {
        cycle: 2,
        pc: 0,
        instruction_source: AudioTraceSource::Unknown,
        write: GbaAudioTraceWrite::FifoPop {
            fifo: GbaDirectSoundFifo::A,
            timer: 0,
            effective_soundcnt_h: 0x300,
            before_len: 32,
            after_len: 31,
            value: 4,
            underflow: false,
            origin: GbaAudioTraceOrigin::Timer { timer: 0 },
        },
    });
    let reset_access = GbaAudioTraceAccess {
        address: 0x0400_0082,
        ..access
    };
    for write in [
        GbaAudioTraceWrite::Control {
            address: 0x82,
            raw_value: 0x800,
            io_value: 0,
            access: reset_access,
            origin,
        },
        GbaAudioTraceWrite::FifoReset {
            fifo: GbaDirectSoundFifo::A,
            access: reset_access,
            origin,
        },
    ] {
        let mut event = cpu_event(write);
        event.cycle = 3;
        trace.events.push(event);
    }
    let empty = GbaAudioTraceFifoState {
        queue: [0; 32],
        len: 0,
        current: 0,
    };
    trace.events.push(AudioTraceEvent {
        cycle: 7,
        pc: 0,
        instruction_source: AudioTraceSource::Unknown,
        write: GbaAudioTraceWrite::Terminal {
            fifo_a: empty,
            fifo_b: empty,
            origin: GbaAudioTraceOrigin::Unknown,
        },
    });
    let cancel = AtomicBool::new(false);
    let feed = replay::replay(&trace, &source, &cancel).unwrap();
    assert_eq!(feed.samples[0], [4]);
    assert_eq!(feed.metadata["evicted_bytes"], json!([8, 0]));
    assert_eq!(
        feed.metadata["pops"][0]["source"],
        json!({"event":4,"lane":0})
    );
    trace.events.remove(21);
    assert!(replay::replay(&trace, &source, &cancel).is_err());
}
