use super::*;
use crate::constants::{SOUND_BUS_REQUEST, SOUND_RESET, VDP_STATUS_FLOAT_MASK};
use crate::hardware::bus::Prefetch;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn read(source: &Emulator, address: u32, lanes: ByteLanes) -> Result<u16, BusError> {
    let mut source = source.clone();
    let access = BusAccess {
        logical_address: address,
        bus_address: address,
        kind: AccessKind::Data,
        lanes,
        function_code: FunctionCode::SupervisorData,
    };
    let clock = source.bus.cpu_clock;
    let transfer = if source.concurrent_sound {
        ScheduledBus::new(
            &mut source.bus,
            &mut source.sound_cpu,
            &mut source.sound_schedule,
        )
        .read_cycle(access, clock)
    } else {
        source.bus.read_cycle(access, clock)
    };
    assert_eq!(transfer.wait_clocks, 0);
    assert_eq!(source.bus.prefetch, Prefetch::Empty);
    transfer.result
}

pub(in crate::emulator) fn assert_reads(source: &Emulator, history: Prefetch) {
    let expected = history.word().ok_or(BusError::UnsupportedAccess {
        address: SOUND_RESET,
        direction: Direction::Read,
    });
    assert_eq!(read(source, SOUND_RESET, ByteLanes::Both), expected);
    let upper = u16::from(history.upper().unwrap_or_default()) << 8;
    assert_eq!(
        read(source, SOUND_BUS_REQUEST, ByteLanes::Upper),
        Ok((upper & !0x100) | (u16::from(!source.bus.sound.granted) << 8))
    );
    assert_eq!(
        read(source, 0xc00004, ByteLanes::Both).unwrap() & VDP_STATUS_FLOAT_MASK,
        upper & VDP_STATUS_FLOAT_MASK
    );
}

#[test]
fn tagged_prefetch_raw_roundtrip_clones_and_continues() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            for history in [
                Prefetch::Empty,
                Prefetch::UpperOnly(0xa7),
                Prefetch::Full(0x00a5),
                Prefetch::Full(0x01a5),
                Prefetch::Full(0xfe5a),
                Prefetch::Full(0xff5a),
            ] {
                let mut source = machine(region, concurrent);
                source.bus.prefetch = history;
                let saved = source.encode_state().unwrap();
                assert_eq!(
                    &saved[8..12],
                    &crate::save_state::STATE_VERSION.to_le_bytes()
                );
                let at = layout::prefetch_offset(&saved);
                let bytes = match history {
                    Prefetch::Empty => vec![0],
                    Prefetch::UpperOnly(value) => vec![1, value],
                    Prefetch::Full(value) => {
                        let [low, high] = value.to_le_bytes();
                        vec![2, low, high]
                    }
                };
                assert_eq!(&saved[at..at + bytes.len()], bytes);
                let mut target = machine(region, concurrent);

                let mut expected = source.clone();
                target.decode_state(&saved).unwrap();
                assert_eq!(
                    (&target.encode_state().unwrap()),
                    (&expected.encode_state().unwrap())
                );
                assert_eq!(target.bus.prefetch, history);
                assert_reads(&source, history);
                assert_reads(&target, history);
                assert_eq!(target.clone().bus.prefetch, history);
                expected.set_sample_rate(expected.sample_rate());
                expected.step_field().unwrap();
                target.step_field().unwrap();
                assert_eq!(
                    (&target.encode_state().unwrap()),
                    (&expected.encode_state().unwrap())
                );
                assert_eq!(target.framebuffer(), expected.framebuffer());
                let pcm = expected.drain_audio_samples();
                assert!(!pcm.is_empty());
                assert_eq!(target.drain_audio_samples(), pcm);
            }
        }
    }
}

#[test]
fn invalid_prefetch_tags_and_partial_payloads_are_atomic_at_bus_and_raw_boundaries() {
    for history in [
        Prefetch::Empty,
        Prefetch::UpperOnly(0xa7),
        Prefetch::Full(0x01a5),
    ] {
        let mut source = machine(Region::NtscUs, false);
        source.bus.prefetch = history;
        let saved = source.encode_state().unwrap();
        let at = layout::prefetch_offset(&saved);
        let (bus_at, _) = layout::section_offsets(&saved);
        let mut rejects = Vec::new();
        for tag in [3, 0x80, 0xff] {
            let mut bad = saved.clone();
            bad[at] = tag;
            rejects.push(bad);
        }
        let payload = match history {
            Prefetch::Empty => 0,
            Prefetch::UpperOnly(_) => 1,
            Prefetch::Full(_) => 2,
        };
        for count in 0..=payload {
            rejects.push(saved[..at + count].to_vec());
        }
        for bad in rejects {
            let mut target = source.clone();
            assert!(target.decode_state(&bad).is_err());
            assert_eq!(target, source);
            let mut bus = source.bus.clone();
            assert!(
                bus.read_state(&mut StateReader::new(&bad[bus_at..]))
                    .is_err()
            );
            assert_eq!(bus, source.bus);
        }
    }
}
