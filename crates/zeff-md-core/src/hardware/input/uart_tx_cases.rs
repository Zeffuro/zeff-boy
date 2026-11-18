use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(super) fn checks() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let (rx, tx, period) = if region == Region::Pal {
            (30_041, 38_805, 11_088)
        } else {
            (30_349, 39_197, 11_200)
        };
        for port in 0..CONTROLLER_PORT_COUNT {
            let base = port * IO_SERIAL_REGISTERS_PER_PORT;
            let mut input = Input::cold();
            assert!(input.write_serial(base + 2, 0x38, 24_206, region));
            let clean = input.clone();
            for (begin, end) in [
                (24_205, 24_233),
                (30_000, 30_027),
                (rx - 14, rx + 14),
                (tx - 14, tx + 14),
            ] {
                assert!(!input.write_serial_at_range(base, 0x53, begin, end, region));
                assert_eq!(input, clean);
            }
            assert!(input.write_serial_at_range(base, 0x53, 30_000, 30_028, region));
            assert!(!input.serial_read_allowed(base + 2));
            assert!(!input.data_read_allowed(port));
            assert_eq!(
                input.read_serial_at_range(base, 31_000, 31_028, region),
                None
            );
            assert_eq!(
                input.read_serial_at_range(base + 1, 31_000, 31_028, region),
                None
            );
            let first = input.clone();
            assert_eq!(
                input.read_serial_at_range(base + 2, rx, rx + 28, region),
                Some(0x39)
            );
            assert_eq!(
                input.read_serial_at_range(base + 2, tx - 50, tx - 22, region),
                Some(0x39)
            );
            for (begin, end, value) in [
                (tx - 28, tx, 0x39),
                (tx - 27, tx + 1, 0x39),
                (tx - 26, tx + 2, 0x39),
                (tx - 24, tx + 4, 0x38),
                (tx, tx + 28, 0x38),
            ] {
                assert_eq!(
                    input.read_serial_at_range(base + 2, begin, end, region),
                    Some(value)
                );
            }
            assert_eq!(
                input.read_serial_at_range(base + 2, tx - 25, tx + 3, region),
                Some(0x39)
            );
            assert_eq!(
                input.read_serial_at_range(base + 2, 31_000, 31_027, region),
                None
            );
            assert_eq!(
                input.read_serial_at_range(base + 2, 30_000, 30_028, region),
                None
            );
            assert_eq!(
                input.read_serial_at_range(base + 2, tx + 1, tx + 29, region),
                Some(0x38)
            );
            assert_eq!(input, first);
            sample_state_check(&first, base, tx, region);
            assert!(!input.write_serial_at_range(base, 0x53, 31_000, 31_028, region));
            assert_eq!(input, first);
            for now in [31_000, tx, tx + 1] {
                assert!(input.write_data(port, 0x7f, now, region));
                assert!(input.write_control(port, 0, now, region));
                assert_eq!(input, first);
            }
            assert!(!input.write_data(port, 0x7e, tx + 1, region));
            assert!(!input.write_control(port, 0x10, tx + 1, region));
            assert!(!input.write_serial(base + 2, 0x20, tx + 1, region));
            assert!(!input.write_serial(8, 0, tx + 1, region));
            assert_eq!(input, first);
            assert!(!input.write_serial_at_range(
                base,
                0x53,
                tx + 10 * period - 14,
                tx + 10 * period + 14,
                region
            ));
            assert_eq!(input, first);
            state_checks(&input, base, region);
            inactive_host_checks(&first, port, tx, period, region);

            let mut repeated_control = clean.clone();
            assert!(repeated_control.write_control(port, 0x10, 29_999, region));
            assert!(repeated_control.write_serial_at_range(base, 0x53, 30_000, 30_028, region));
            let repeated_before = repeated_control.clone();
            assert!(repeated_control.write_control(port, 0x10, tx + 1, region));
            assert!(repeated_control.write_serial(base + 2, 0x38, tx + 1, region));
            assert_eq!(repeated_control, repeated_before);
            assert!(!repeated_control.write_control(port, 0x90, tx + 1, region));
            assert_eq!(repeated_control, repeated_before);

            let mut settled = first.clone();
            assert!(settled.write_serial_at_range(base, 0x4d, tx, tx + 28, region));
            let settled_slot = tx + 11 * period;
            assert_eq!(
                settled.read_serial_at_range(
                    base + 2,
                    settled_slot - 50,
                    settled_slot - 22,
                    region
                ),
                Some(0x39)
            );
            assert_eq!(
                settled.read_serial_at_range(base + 2, settled_slot + 1, settled_slot + 29, region),
                Some(0x38)
            );

            let interior_rx = tx - 448;
            let mut sampled = clean.clone();
            assert!(sampled.write_serial_at_range(
                base,
                0x53,
                interior_rx - 20,
                interior_rx + 8,
                region
            ));
            assert_eq!(
                sampled.read_serial_at_range(base + 2, tx - 50, tx - 22, region),
                Some(0x39)
            );
            assert_eq!(
                sampled.read_serial_at_range(base + 2, tx + 1, tx + 29, region),
                Some(0x38)
            );

            let mut non_load = clean.clone();
            assert!(non_load.write_serial_at_range(base, 0x53, tx - 20, tx + 8, region));
            assert_eq!(
                non_load.read_serial_at_range(base + 2, tx + 1, tx + 29, region),
                None
            );
            assert_eq!(
                non_load.read_serial_at_range(base + 2, tx + 8, tx + 36, region),
                Some(0x39)
            );
            assert_eq!(
                non_load.read_serial_at_range(base + 2, tx + period + 1, tx + period + 29, region),
                Some(0x38)
            );

            let pulse = tx + 10 * period;
            let mut preset = first.clone();
            assert!(preset.write_serial_at_range(base, 0x4d, pulse - 20, pulse + 8, region));
            let slot = tx + 11 * period;
            assert_eq!(
                preset.read_serial_at_range(base + 2, slot - 50, slot - 22, region),
                Some(0x39)
            );
            assert_eq!(
                preset.read_serial_at_range(base + 2, slot + 1, slot + 29, region),
                Some(0x38)
            );
            prior_load_check(&preset, port, pulse, period, region);

            assert!(input.write_serial_at_range(base, 0x4d, tx + 100, tx + 128, region));
            let queued = tx + 11 * period;
            assert_eq!(
                input.read_serial_at_range(base + 2, queued - 50, queued - 22, region),
                Some(0x39)
            );
            assert_eq!(
                input.read_serial_at_range(base + 2, queued + 1, queued + 29, region),
                Some(0x38)
            );
            let queued_state = input.clone();
            assert_eq!(
                input.read_serial_at_range(base + 2, u64::MAX - 28, u64::MAX, region),
                Some(0x38)
            );
            assert_eq!(input, queued_state);
            assert!(!input.write_serial_at_range(base, 0, u64::MAX - 28, u64::MAX, region));
            assert_eq!(input, queued_state);

            let mut late = first.clone();
            assert!(late.write_serial_at_range(base, 0x4d, queued - 100, queued - 72, region));
            assert_eq!(
                late.read_serial_at_range(base + 2, queued + 1, queued + 29, region),
                Some(0x39)
            );
            assert_eq!(
                late.read_serial_at_range(
                    base + 2,
                    queued + period + 1,
                    queued + period + 29,
                    region
                ),
                Some(0x38)
            );

            let mut host = first.clone();
            let key = if port == 0 {
                ControllerPort::One
            } else {
                ControllerPort::Two
            };
            host.set_controller_raw_at(key, 0xff, tx);
            assert_eq!(host, first);
            host.set_controller_extra_at(key, 0x0e, tx);
            assert!(host.fault().is_some());
            let faulted = host.clone();
            assert!(!host.write_data(port, 0x7f, tx + 1, region));
            assert!(!host.write_control(port, 0, tx + 1, region));
            assert_eq!(host, faulted);
            assert_eq!(
                host.read_serial_at_range(base + 2, tx + 1, tx + 29, region),
                None
            );

            let mut retired = clean.clone();
            assert!(retired.write_serial(base + 2, 0, 24_207, region));
            assert!(retired.write_serial(base + 2, 0x40, 24_208, region));
            assert!(retired.write_serial(base + 2, 0, 24_209, region));
            assert!(retired.write_serial(base + 2, 0x38, 24_210, region));
            assert!(!retired.write_serial_at_range(base, 0, 30_000, 30_028, region));
        }
        let mut shared = Input::cold();
        for port in 0..CONTROLLER_PORT_COUNT {
            assert!(shared.write_serial(port * 3 + 2, 0x38, 24_206, region));
        }
        assert!(shared.write_serial_at_range(0, 0x53, 30_000, 30_028, region));
        assert!(shared.write_serial_at_range(3, 0x4d, 30_042, 30_070, region));
        for register in [2, 5] {
            assert_eq!(
                shared.read_serial_at_range(register, tx - 50, tx - 22, region),
                Some(0x39)
            );
            assert_eq!(
                shared.read_serial_at_range(register, tx + 1, tx + 29, region),
                Some(0x38)
            );
        }
    }
}

fn encoded(input: &Input, now: u64) -> Vec<u8> {
    let mut writer = StateWriter::new();
    input.write_state(&mut writer, now).unwrap();
    writer.into_bytes()
}

fn host_change(input: &mut Input, index: usize, change: InputChange, now: u64) {
    let port = [ControllerPort::One, ControllerPort::Two][index];
    match change {
        InputChange::Buttons => {
            input.set_controller_raw_at(port, input.pads[index].buttons ^ 1, now)
        }
        InputChange::ExtraButtons => {
            input.set_controller_extra_at(port, input.pads[index].extra ^ 1, now)
        }
        InputChange::ControllerKind => {
            let kind = if input.pads[index].kind == ControllerKind::ThreeButton {
                ControllerKind::SixButton
            } else {
                ControllerKind::ThreeButton
            };
            input.set_controller_kind_at(port, kind, now);
        }
    }
}

fn inactive_host_checks(first: &Input, used: usize, tx: u64, period: u64, region: Region) {
    let other = 1 - used;
    let base = used * IO_SERIAL_REGISTERS_PER_PORT;
    let other_base = other * IO_SERIAL_REGISTERS_PER_PORT;
    let key = [ControllerPort::One, ControllerPort::Two][other];
    for stage in 0..3 {
        let mut source = first.clone();
        if stage == 2 {
            assert!(source.write_serial_at_range(base, 0x4d, tx + 100, tx + 128, region));
        }
        let now = [31_000, tx + 29, tx + 128][stage];
        let load = if stage == 2 { tx + 11 * period } else { tx };
        for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
            source.pads[other].set_kind(kind);
            for change in [
                InputChange::Buttons,
                InputChange::ExtraButtons,
                InputChange::ControllerKind,
            ] {
                let mut target = source.clone();
                target.set_controller_raw_at(key, target.pads[other].buttons, now);
                target.set_controller_extra_at(key, target.pads[other].extra, now);
                target.set_controller_kind_at(key, kind, now);
                assert_eq!(target, source);
                host_change(&mut target, other, change, now);
                assert_eq!(target.fault(), None);
                let mut expected = source.clone();
                expected.pads[other] = target.pads[other];
                expected.uart_tx.retire_port(other);
                expected.uart_idle.history[other] = uart_idle::History::LegacyUnknown;
                assert_eq!(target, expected);
                let before = target.clone();
                assert!(!target.write_serial(other_base + 2, 0x38, now, region));
                assert_eq!(target, before);
                let bytes = encoded(&target, now);
                let mut restored = Input::default();
                restored.pads[other].set_kind(target.pads[other].kind);
                restored
                    .read_state(&mut StateReader::new(&bytes), now, region)
                    .unwrap();
                assert_eq!(restored, target);
                assert_eq!(encoded(&restored, now), bytes);
                for input in [&target, &restored] {
                    host_future_status(input, base, load, now, region);
                }
                let next = load + 11 * period;
                for input in [&mut target, &mut restored] {
                    assert!(input.write_serial_at_range(
                        base,
                        0x54,
                        load + 100,
                        load + 128,
                        region
                    ));
                    host_future_status(input, base, next, load + 128, region);
                }
                assert_eq!(target, restored);
                assert_eq!(encoded(&target, next + 29), encoded(&restored, next + 29));
                let mut same = source.clone();
                host_change(&mut same, used, change, now);
                let fault = same.fault().unwrap();
                assert_eq!(fault.port, [ControllerPort::One, ControllerPort::Two][used]);
                assert_eq!(fault.change, change);
                assert_eq!(same.uart_tx, source.uart_tx);
                host_change(&mut same, other, change, now + 1);
                assert_eq!(same.fault(), Some(fault));
                assert_eq!(same.uart_tx, source.uart_tx);
                assert_eq!(same.uart_idle.history, source.uart_idle.history);
            }
        }
        for mode in [0x20, 0x38] {
            let mut active = source.clone();
            assert!(active.write_serial(other_base + 2, mode, now, region));
            host_change(&mut active, other, InputChange::Buttons, now + 1);
            assert!(active.fault().is_some());
            assert_eq!(active.uart_tx, source.uart_tx);
        }
    }
}

fn host_future_status(input: &Input, base: usize, load: u64, now: u64, region: Region) {
    let before = input.clone();
    for (begin, end, status) in [
        (load - 50, load - 22, 0x39),
        (load - 25, load + 3, 0x39),
        (load + 1, load + 29, 0x38),
    ] {
        if begin >= now {
            assert_eq!(
                input.read_serial_at_range(base + 2, begin, end, region),
                Some(status)
            );
        }
    }
    assert_eq!(input, &before);
}

fn owner_length(input: &Input) -> usize {
    let mut writer = StateWriter::new();
    input.uart_tx.write_state(&mut writer);
    writer.into_bytes().len() + 4
}

fn sample_state_check(input: &Input, base: usize, load: u64, region: Region) {
    let now = load + 3;
    let bytes = encoded(input, now);
    let mut restored = Input::default();
    restored
        .read_state(&mut StateReader::new(&bytes), now, region)
        .unwrap();
    assert_eq!(&restored, input);
    assert_eq!(
        restored.read_serial_at_range(base + 2, load - 25, now, region),
        Some(0x39)
    );
    assert_eq!(
        restored.read_serial_at_range(base + 2, load - 24, now + 1, region),
        Some(0x38)
    );
    assert_eq!(&restored, input);
    assert_eq!(encoded(&restored, now), bytes);
}

fn state_checks(input: &Input, base: usize, region: Region) {
    let bytes = encoded(input, 30_028);
    let mut restored = Input::default();
    restored
        .read_state(&mut StateReader::new(&bytes), 30_028, region)
        .unwrap();
    assert_eq!(&restored, input);
    for length in 0..bytes.len() {
        let before = restored.clone();
        assert!(
            restored
                .read_state(&mut StateReader::new(&bytes[..length]), 30_028, region)
                .is_err()
        );
        assert_eq!(restored, before);
    }
    let payload = bytes.len() - owner_length(input) + 4;
    for (offset, value) in [(payload, 0), (payload + 1, 0), (payload + 1, 3)] {
        let mut changed = bytes.clone();
        changed[offset] = value;
        let before = restored.clone();
        assert!(
            restored
                .read_state(&mut StateReader::new(&changed), 30_028, region)
                .is_err()
        );
        assert_eq!(restored, before);
    }
    let other = if region == Region::Pal {
        Region::NtscUs
    } else {
        Region::Pal
    };
    let before = restored.clone();
    assert!(
        restored
            .read_state(&mut StateReader::new(&bytes), 30_028, other)
            .is_err()
    );
    assert_eq!(restored, before);
    assert_eq!(
        restored.read_serial_at_range(base + 2, 31_000, 31_028, region),
        Some(0x39)
    );
}

fn prior_load_check(input: &Input, port: usize, pulse: u64, period: u64, region: Region) {
    let now = pulse + 8;
    let bytes = encoded(input, now);
    let mut target = Input::default();
    target
        .read_state(&mut StateReader::new(&bytes), now, region)
        .unwrap();
    assert_eq!(&target, input);
    let payload = bytes.len() - owner_length(input) + 4;
    let mut reader = StateReader::new(&bytes[payload..]);
    reader.read_bool().unwrap();
    reader.read_u8().unwrap();
    let mut offsets = None;
    for index in 0..CONTROLLER_PORT_COUNT {
        reader.read_bool().unwrap();
        reader.read_bool().unwrap();
        let last_at = if reader.read_bool().unwrap() {
            let at = payload + reader.position();
            reader.read_u64().unwrap();
            Some(at)
        } else {
            None
        };
        if reader.read_bool().unwrap() {
            reader.read_u64().unwrap();
            let load_at = payload + reader.position();
            reader.read_u64().unwrap();
            reader.read_u8().unwrap();
            if index == port {
                offsets = Some((last_at.unwrap(), load_at));
            }
        }
    }
    assert!(reader.is_exhausted());
    let (last_at, load_at) = offsets.unwrap();
    let mut changed = bytes.clone();
    changed[last_at..last_at + 8].copy_from_slice(&pulse.to_le_bytes());
    changed[load_at..load_at + 8].copy_from_slice(&(pulse + 11 * period).to_le_bytes());
    let before = target.clone();
    assert!(
        target
            .read_state(&mut StateReader::new(&changed), now, region)
            .is_err()
    );
    assert_eq!(target, before);
}
