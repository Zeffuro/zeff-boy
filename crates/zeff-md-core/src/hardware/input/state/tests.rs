use super::*;
use crate::save_state::{MIN_STATE_VERSION, STATE_VERSION};

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
        source.write_state(&mut writer);
        let encoded = writer.into_bytes();
        let mut target = Input::default();
        target.set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
        target
            .read_state(&mut StateReader::new(&encoded), 6, STATE_VERSION)
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
                    .read_state(&mut StateReader::new(&encoded[..length]), 6, STATE_VERSION)
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
    source.write_state(&mut writer);
    let encoded = writer.into_bytes();
    for (offset, value) in [(0, 1), (2, 0xff), (3, 2), (4, 1), (5, 2)] {
        let mut changed = encoded.clone();
        changed[offset] = value;
        let mut target = source.clone();
        assert!(
            target
                .read_state(&mut StateReader::new(&changed), 0, STATE_VERSION)
                .is_err()
        );
        assert_eq!(target, source);
    }
}

#[test]
fn serial_latches_restore_and_legacy_states_reset_them() {
    let mut source = Input::default();
    for port in 0..IO_PORT_COUNT {
        assert!(source.write_serial(port * IO_SERIAL_REGISTERS_PER_PORT, port as u8));
        assert!(source.write_serial(
            port * IO_SERIAL_REGISTERS_PER_PORT + 2,
            IO_SERIAL_CONTROL_MASK
        ));
    }
    let mut writer = StateWriter::new();
    source.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut target = Input::default();
    target
        .read_state(&mut StateReader::new(&bytes), 0, STATE_VERSION)
        .unwrap();
    assert_eq!(target, source);
    for length in bytes.len() - IO_PORT_COUNT * 2..bytes.len() {
        let before = target.clone();
        assert!(
            target
                .read_state(&mut StateReader::new(&bytes[..length]), 0, STATE_VERSION)
                .is_err()
        );
        assert_eq!(target, before);
    }
    let mut invalid = bytes.clone();
    *invalid.last_mut().unwrap() |= IO_SERIAL_MODE_MASK;
    let before = target.clone();
    assert!(
        target
            .read_state(&mut StateReader::new(&invalid), 0, STATE_VERSION)
            .is_err()
    );
    assert_eq!(target, before);
    target
        .read_state(
            &mut StateReader::new(&bytes[..bytes.len() - IO_PORT_COUNT * 2]),
            0,
            MIN_STATE_VERSION,
        )
        .unwrap();
    assert_eq!(target, Input::default());
}
