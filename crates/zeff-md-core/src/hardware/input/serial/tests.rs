use super::*;

const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn edge(region: Region, baud: u8, count: u64) -> u64 {
    (region.master_hz() * count).div_ceil(u64::from(BAUD[usize::from(baud)]))
}

#[test]
fn serial_holding_and_shift_boundaries_at_every_baud_and_region() {
    for region in REGIONS {
        for baud in 0..4 {
            let control = (baud << 6) | 0x38;
            let mut serial = Serial::default();
            serial.control(0, control, 0, region);
            serial.transmit(0xff, 0, control, region);
            let start = edge(region, baud, 1);
            assert_eq!(serial.status(start - 1, control, region), TX_FULL);
            assert_eq!(serial.status(start, control, region), 0);
            serial.advance(start, control, region);
            assert_eq!((serial.shift, serial.remaining), (0xff, FRAME_BITS));
            serial.transmit(0xa5, start, control, region);
            let finish = edge(region, baud, 11);
            assert_eq!(serial.status(finish - 1, control, region), TX_FULL);
            assert_eq!(serial.status(finish, control, region), 0);
            serial.advance(finish, control, region);
            assert_eq!((serial.shift, serial.remaining), (0xa5, FRAME_BITS));
            serial.advance(edge(region, baud, 21) - 1, control, region);
            assert_eq!(serial.remaining, 1);
            serial.advance(edge(region, baud, 21), control, region);
            assert_eq!((serial.shift, serial.remaining), (0, 0));
            serial
                .validate(edge(region, baud, 21), control, region)
                .unwrap();
        }
    }
}

#[test]
fn serial_overwrite_disable_and_same_edge_writes_preserve_in_flight_byte() {
    for region in REGIONS {
        let mut serial = Serial::default();
        serial.transmit(0xff, 0, 0, region);
        serial.transmit(0x5a, 1, 0, region);
        let start = edge(region, 0, 1);
        serial.transmit(0x12, start, 0, region);
        assert_eq!(
            (serial.shift, serial.remaining, serial.holding),
            (0x5a, 10, Some(0x12))
        );
        serial.control(0, 0x38, start + 1, region);
        serial.transmit(0xa5, start + 2, 0x38, region);
        serial.control(0x38, 0, start + 3, region);
        assert_eq!(
            (serial.shift, serial.remaining, serial.holding),
            (0x5a, 10, Some(0xa5))
        );
        serial.advance(edge(region, 0, 11), 0, region);
        assert_eq!(
            (serial.shift, serial.remaining, serial.holding),
            (0xa5, 10, None)
        );
        serial.advance(edge(region, 0, 21), 0, region);
        assert_eq!(serial.remaining, 0);
        serial.transmit(0xff, edge(region, 0, 21), 0, region);
        assert_eq!(serial.status(edge(region, 0, 21), 0, region), TX_FULL);
    }
}

#[test]
fn serial_baud_changes_defer_until_frame_completion_and_idle_changes_restart_phase() {
    for region in REGIONS {
        let mut serial = Serial::default();
        serial.transmit(0x5a, 0, 0x38, region);
        let start = edge(region, 0, 1);
        serial.transmit(0xa5, start, 0x38, region);
        serial.control(0x38, 0xf8, start + 17, region);
        assert_eq!(serial.baud, 0);
        let finish = edge(region, 0, 11);
        assert_eq!(serial.status(finish - 1, 0xf8, region), TX_FULL);
        serial.advance(finish, 0xf8, region);
        assert_eq!(
            (serial.baud, serial.phase, serial.remaining, serial.shift),
            (3, 0, 10, 0xa5)
        );
        serial.advance(finish + edge(region, 3, 10) - 1, 0xf8, region);
        assert_eq!(serial.remaining, 1);
        let done = finish + edge(region, 3, 10);
        serial.advance(done, 0xf8, region);
        serial.control(0xf8, 0x78, done + 5, region);
        serial.transmit(0x34, done + 5, 0x78, region);
        assert_eq!(
            serial.status(done + 5 + edge(region, 1, 1) - 1, 0x78, region),
            TX_FULL
        );
        assert_eq!(
            serial.status(done + 5 + edge(region, 1, 1), 0x78, region),
            0
        );
    }
}

#[test]
fn serial_access_frequency_and_huge_idle_intervals_do_not_change_results() {
    for region in REGIONS {
        let mut bulk = Serial::default();
        bulk.transmit(0x5a, 0, 0x38, region);
        bulk.transmit(0xa5, edge(region, 0, 1), 0x38, region);
        bulk.control(0x38, 0xb8, edge(region, 0, 2), region);
        let mut scalar = bulk.clone();
        let end = edge(region, 2, 30);
        for at in (scalar.clock..end).step_by(43) {
            scalar.advance(at, 0xb8, region);
        }
        scalar.advance(end, 0xb8, region);
        bulk.advance(end, 0xb8, region);
        assert_eq!(bulk, scalar);
        bulk.advance(u64::MAX, 0xb8, region);
        scalar.advance(u64::MAX - 1, 0xb8, region);
        scalar.advance(u64::MAX, 0xb8, region);
        assert_eq!(bulk, scalar);
        assert_eq!(bulk.status(u64::MAX, 0xb8, region), 0);
        bulk.validate(u64::MAX, 0xb8, region).unwrap();
    }
}

#[test]
fn serial_pending_states_roundtrip_and_reject_invalid_clocks_and_frames() {
    for region in REGIONS {
        let mut serial = Serial::default();
        serial.transmit(0xff, 0, 0x38, region);
        for at in [0, edge(region, 0, 1), edge(region, 0, 5)] {
            serial.advance(at, 0x38, region);
            serial.transmit(0xff, at, 0x38, region);
            let mut writer = StateWriter::new();
            serial.write_state(&mut writer);
            let bytes = writer.into_bytes();
            let mut restored = Serial::read_state(&mut StateReader::new(&bytes)).unwrap();
            restored.validate(at, 0x38, region).unwrap();
            assert_eq!(restored, serial);
            let end = edge(region, 0, 30);
            restored.advance(end, 0x38, region);
            let mut expected = serial.clone();
            expected.advance(end, 0x38, region);
            assert_eq!(restored, expected);
            for (offset, value) in [(7, 0xff), (11, 0xff), (12, 4), (13, 11), (15, 2)] {
                let mut changed = bytes.clone();
                changed[offset] = value;
                assert!(
                    Serial::read_state(&mut StateReader::new(&changed))
                        .and_then(|value| value.validate(at, 0x38, region))
                        .is_err()
                );
            }
        }
    }
}
