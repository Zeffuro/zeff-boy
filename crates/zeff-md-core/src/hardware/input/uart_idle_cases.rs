use super::*;

pub(super) fn checks() {
    output_checks();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for port in 0..CONTROLLER_PORT_COUNT {
            let base = port * IO_SERIAL_REGISTERS_PER_PORT;
            let key = if port == 0 {
                ControllerPort::One
            } else {
                ControllerPort::Two
            };
            let mut input = Input::cold();
            assert!(input.write_serial(base + 2, 0x27, 0, region));
            for at in [0, 1, region.pad_timeout_ticks(), u64::MAX] {
                assert_eq!(input.read_serial(base + 2, at, region), 0x20);
                assert_ne!(input.read_data(port, at, region) & 0x20, 0);
            }
            assert!(!input.serial_read_allowed(base));
            assert!(!input.serial_read_allowed(base + 1));
            for (register, value) in [(base, 0xff), (base + 1, 0), (base + 2, 0x38)] {
                let before = input.clone();
                assert!(!input.write_serial(register, value, 1, region));
                assert_eq!(input, before);
            }
            assert!(input.write_control(port, 0xfc, 1, region));
            assert!(input.write_data(port, 0x5f, 1, region));
            assert_ne!(input.read_data(port, 1, region) & 0x20, 0);
            assert!(input.write_serial(base + 2, 0, 1, region));
            assert!(!input.write_serial(base + 2, 0x20, 1, region));
            assert!(input.write_data(port, 0x7f, 1, region));
            input.set_controller_raw_at(key, 0x7f, 2);
            assert!(input.write_control(port, IO_TH, 2, region));
            assert!(input.write_serial(base + 2, 0x20, 2, region));
            let before = input.clone();
            assert!(!input.write_data(port, 0, 3, region));
            assert_eq!(input, before);
            input.set_controller_raw_at(key, 0xdf, 4);
            let fault = input.fault().unwrap();
            assert_eq!(fault.port, key);
            assert_eq!(fault.at, 4);
            input.set_controller_raw_at(key, 0xff, 5);
            assert_eq!(input.fault(), Some(fault));
            assert!(!input.write_serial(base + 2, 0, 5, region));
            assert!(input.validate_serial(5, region).is_err());

            let mut prior = Input::cold();
            assert!(prior.write_serial(base, 0xff, 0, region));
            assert!(!prior.write_serial(base + 2, 0x20, 1, region));
            assert!(prior.write_serial((1 - port) * 3 + 2, 0x20, 1, region));
            let mut unknown = Input::default();
            assert!(!unknown.write_serial(base + 2, 0x20, 0, region));
            let mut low = Input::cold();
            low.set_controller_raw_at(key, 0xdf, 0);
            assert!(!low.write_serial(base + 2, 0x20, 0, region));
            assert!(low.write_control(port, 0x20, 0, region));
            assert!(!low.write_serial(base + 2, 0x20, 0, region));
            let mut driven_low = Input::cold();
            assert!(driven_low.write_data(port, 0x5f, 0, region));
            assert!(driven_low.write_control(port, 0x20, 0, region));
            assert!(!driven_low.write_serial(base + 2, 0x20, 0, region));
        }
    }
}

fn output_checks() {
    const SETTLED: u64 = 24_206;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for port in 0..CONTROLLER_PORT_COUNT {
            let base = port * IO_SERIAL_REGISTERS_PER_PORT;
            let key = if port == 0 {
                ControllerPort::One
            } else {
                ControllerPort::Two
            };
            let mut input = Input::cold();
            let before = input.clone();
            assert!(!input.write_serial(base + 2, 0x38, SETTLED - 1, region));
            assert_eq!(input, before);
            for (at, baud) in [(0, 0xc0), (10, 0x80), (100, 0x40), (200, 0)] {
                assert!(input.write_serial(base + 2, baud, at, region));
            }
            assert!(input.write_serial(base + 2, 0x20, 200, region));
            let before = input.clone();
            assert!(!input.write_serial(base + 2, 0x38, SETTLED - 1, region));
            assert_eq!(input, before);
            assert!(input.write_serial(base + 2, 0x38, SETTLED, region));
            assert!(!input.serial_read_allowed(base));
            assert!(!input.serial_read_allowed(base + 1));
            for register in [base, base + 1] {
                let before = input.clone();
                assert!(!input.write_serial(register, 0xff, SETTLED, region));
                assert_eq!(input, before);
            }
            for at in [SETTLED, SETTLED + region.pad_timeout_ticks(), u64::MAX] {
                assert_eq!(input.read_serial(base + 2, at, region), 0x38);
                assert_eq!(input.read_data(port, at, region) & 0x30, 0x30);
            }
            for value in [0x28, 0x30, 0x10, 0x78, 0xb8, 0xf8] {
                let before = input.clone();
                assert!(!input.write_serial(base + 2, value, SETTLED, region));
                assert_eq!(input, before);
            }
            assert!(input.write_control(port, 0x10, SETTLED, region));
            assert!(input.write_data(port, 0, SETTLED, region));
            assert_eq!(input.read_data(port, SETTLED, region) & 0x30, 0x30);
            for value in [0, 0x20] {
                let before = input.clone();
                assert!(!input.write_serial(base + 2, value, SETTLED, region));
                assert_eq!(input, before);
            }
            assert!(input.write_data(port, 0x10, SETTLED, region));
            assert!(input.write_serial(base + 2, 0x20, SETTLED, region));
            assert!(input.write_serial(base + 2, 0x38, SETTLED, region));
            assert!(input.write_serial(base + 2, 0, SETTLED, region));
            assert!(input.write_serial(base + 2, 0x38, SETTLED, region));
            input.set_controller_extra_at(key, 0, SETTLED);
            input.set_controller_kind_at(key, ControllerKind::SixButton, SETTLED);
            assert_eq!(input.fault(), None);
            assert_eq!(
                input.read_data(port, SETTLED + region.pad_timeout_ticks(), region) & 0x30,
                0x30
            );
            assert!(input.validate_serial(SETTLED, region).is_ok());
            assert!(input.validate_serial(SETTLED - 1, region).is_err());
            let mut writer = zeff_emu_common::save_state::StateWriter::new();
            assert!(input.write_state(&mut writer, SETTLED - 1).is_err());
            assert!(writer.into_bytes().is_empty());

            for (control, data, buttons) in [
                (0, 0x7f, 0xef),
                (IO_TH, 0, 0xbf),
                (0, 0x7f, 0xdf),
                (IO_TH, 0, 0x7f),
            ] {
                let mut host = Input::cold();
                assert!(host.write_control(port, control, 0, region));
                assert!(host.write_data(port, data, 0, region));
                assert!(host.write_serial(base + 2, 0x38, SETTLED, region));
                host.set_controller_raw_at(key, buttons, SETTLED + 1);
                let fault = host.fault().unwrap();
                assert_eq!((fault.port, fault.at), (key, SETTLED + 1));
                host.set_controller_raw_at(key, 0xff, SETTLED + 2);
                host.set_controller_extra_at(key, 0xf, SETTLED + 2);
                host.set_controller_kind_at(key, ControllerKind::SixButton, SETTLED + 2);
                assert_eq!(host.fault(), Some(fault));
                assert!(!host.write_serial(base + 2, 0, SETTLED + 2, region));
                assert!(host.validate_serial(SETTLED + 2, region).is_err());
                let mut low = Input::cold();
                assert!(low.write_control(port, control, 0, region));
                assert!(low.write_data(port, data, 0, region));
                low.set_controller_raw_at(key, buttons, 0);
                let before = low.clone();
                assert!(!low.write_serial(base + 2, 0x38, SETTLED, region));
                assert_eq!(low, before);
            }
            for buttons in [0xbf, 0x7f] {
                let mut th = Input::cold();
                th.set_controller_raw_at(key, buttons, 0);
                assert!(th.write_serial(base + 2, 0x38, SETTLED, region));
                assert!(th.write_control(port, IO_TH, SETTLED, region));
                let before = th.clone();
                assert!(!th.write_data(port, 0, SETTLED, region));
                assert_eq!(th, before);
            }
            for mut rejected in [Input::default(), Input::cold()] {
                if rejected.uart_idle.history[port] == uart_idle::History::KnownIdleNoTx {
                    assert!(rejected.write_serial(base, 0xff, 0, region));
                }
                let before = rejected.clone();
                assert!(!rejected.write_serial(base + 2, 0x38, SETTLED, region));
                assert_eq!(rejected, before);
            }
            let mut driven_low = Input::cold();
            assert!(driven_low.write_control(port, 0x10, 0, region));
            assert!(driven_low.write_data(port, 0, 0, region));
            let before = driven_low.clone();
            assert!(!driven_low.write_serial(base + 2, 0x38, SETTLED, region));
            assert_eq!(driven_low, before);
        }
    }
}
