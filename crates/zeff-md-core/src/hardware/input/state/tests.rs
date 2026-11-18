use super::*;

#[test]
fn six_button_handshake_and_timeout_continue_after_restore() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut source = Input::default();
        source.set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
        source.set_controller_extra_raw(ControllerPort::One, 0x05);
        source.write_control(0, IO_TH, 0, region);
        for edge in 0..6 {
            source.write_data(0, if edge % 2 == 0 { 0 } else { IO_TH }, edge + 1, region);
        }
        let mut writer = StateWriter::new();
        source.write_state(&mut writer, 6).unwrap();
        let encoded = writer.into_bytes();
        let mut target = Input::default();
        target.set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
        target
            .read_state(&mut StateReader::new(&encoded), 6, region)
            .unwrap();
        assert_eq!(target, source);
        for at in [7, region.pad_timeout_ticks() + 6] {
            assert_eq!(
                target.read_data(0, at, region),
                source.read_data(0, at, region)
            );
            target.write_data(0, 0, at, region);
            source.write_data(0, 0, at, region);
            assert_eq!(target, source);
        }
        for length in 0..encoded.len() {
            let before = target.clone();
            assert!(
                target
                    .read_state(&mut StateReader::new(&encoded[..length]), 6, region)
                    .is_err()
            );
            assert_eq!(target, before);
        }
    }
}

#[test]
fn input_rejects_changed_topology_and_invalid_handshake_transactionally() {
    let source = Input::default();
    let mut writer = StateWriter::new();
    source.write_state(&mut writer, 0).unwrap();
    let encoded = writer.into_bytes();
    for (offset, value) in [(0, 1), (2, 0xff), (3, 2), (4, 1), (5, 2)] {
        let mut changed = encoded.clone();
        changed[offset] = value;
        let mut target = source.clone();
        assert!(
            target
                .read_state(&mut StateReader::new(&changed), 0, Region::NtscUs)
                .is_err()
        );
        assert_eq!(target, source);
    }
}

#[test]
fn serial_latches_restore_exactly() {
    output_idle_state_guards();
    let mut source = Input::default();
    for port in 0..IO_PORT_COUNT {
        assert!(source.write_serial(
            port * IO_SERIAL_REGISTERS_PER_PORT,
            port as u8,
            0,
            Region::NtscUs
        ));
        assert!(source.write_serial(
            port * IO_SERIAL_REGISTERS_PER_PORT + 2,
            IO_SERIAL_CONTROL_MASK,
            0,
            Region::NtscUs
        ));
    }
    let mut writer = StateWriter::new();
    source.write_state(&mut writer, 0).unwrap();
    let bytes = writer.into_bytes();
    let mut target = Input::default();
    target
        .read_state(&mut StateReader::new(&bytes), 0, Region::NtscUs)
        .unwrap();
    assert_eq!(target, source);
}

fn output_idle_state_guards() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = Input::cold();
        for port in 0..CONTROLLER_PORT_COUNT {
            assert!(source.write_serial(port * 3 + 2, 0x38, 24_206, region));
        }
        let mut writer = StateWriter::new();
        source.write_state(&mut writer, 24_206).unwrap();
        let bytes = writer.into_bytes();
        let mut target = Input::cold();
        let before = target.clone();
        assert!(
            target
                .read_state(&mut StateReader::new(&bytes), 24_205, region)
                .is_err()
        );
        assert_eq!(target, before);
        target
            .read_state(&mut StateReader::new(&bytes), 24_206, region)
            .unwrap();
        assert_eq!(target, source);
    }
}
