use super::*;
use crate::constants::PAD_SIX_TIMEOUT_TICKS;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_mode_serial_registers_preserve_pad_inputs() {
    upper_write_lanes();
    idle_cases::checks();
    uart_tx_cases::checks();
    crate::hardware::bus::owned::uart_tx_cases::checks();
    let mut input = Input::default();
    input.set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
    input.set_controller_raw(ControllerPort::One, 0x12);
    for port in 0..CONTROLLER_PORT_COUNT {
        let base = port * IO_SERIAL_REGISTERS_PER_PORT;
        assert_eq!(
            input.read_serial(base, 0, Region::NtscUs),
            IO_SERIAL_TX_RESET
        );
        assert_eq!(input.read_serial(base + 1, 0, Region::NtscUs), 0);
        assert_eq!(input.read_serial(base + 2, 0, Region::NtscUs), 0);
        let data = input.read_data(port, 0, Region::NtscUs);
        for value in [0, 0x5a, 0xff] {
            assert!(input.write_serial(base, value, 0, Region::NtscUs));
            assert_eq!(input.read_serial(base, 0, Region::NtscUs), value);
            assert!(input.write_serial(base + 1, value, 0, Region::NtscUs));
            assert_eq!(input.read_serial(base + 1, 0, Region::NtscUs), 0);
        }
        for value in [0, 7, 8, 0x40, 0x80, 0xcf] {
            assert!(input.write_serial(base + 2, value, 0, Region::NtscUs));
            assert_eq!(
                input.read_serial(base + 2, 0, Region::NtscUs),
                value & IO_SERIAL_CONTROL_MASK
            );
            assert_eq!(input.read_data(port, 0, Region::NtscUs), data);
        }
        for value in [0x10, 0x20, 0x30, 0xff] {
            let before = input.clone();
            assert!(!input.write_serial(base + 2, value, 0, Region::NtscUs));
            assert_eq!(input, before);
        }
    }
    input.power_on();
    for port in 0..IO_PORT_COUNT {
        assert_eq!(
            input.read_serial(port * IO_SERIAL_REGISTERS_PER_PORT, 0, Region::NtscUs),
            IO_SERIAL_TX_RESET
        );
        assert_eq!(
            input.read_serial(port * IO_SERIAL_REGISTERS_PER_PORT + 2, 0, Region::NtscUs),
            0
        );
    }
    assert_eq!(
        input.controller_kind(ControllerPort::One),
        ControllerKind::SixButton
    );
}

fn upper_write_lanes() {
    use crate::cartridge::Cartridge;
    use crate::hardware::bus::Bus;
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;

    let mut bytes = vec![0; 4096];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xf0, 0, 0, 0, 2, 0]);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut bus = Bus::new(Cartridge::new(bytes.clone()).unwrap(), region);
        assert!(bus.input.write_serial(2, 0x20, 0, region));
        let input = bus.input.clone();
        let access = BusAccess {
            logical_address: 0xa10002,
            bus_address: 0xa10002,
            function_code: FunctionCode::SupervisorData,
            kind: AccessKind::Data,
            lanes: ByteLanes::Upper,
        };
        for address in (0xa10000..=0xa1001e).step_by(2) {
            for function_code in [FunctionCode::UserData, FunctionCode::SupervisorData] {
                let request = BusAccess {
                    logical_address: address,
                    bus_address: address,
                    function_code,
                    ..access
                };
                assert!(
                    bus.write_cycle(request, 0xff00, bus.cpu_clock)
                        .result
                        .is_ok()
                );
                assert_eq!(bus.input, input);
            }
        }
        for request in [
            BusAccess {
                logical_address: 0xa10003,
                ..access
            },
            BusAccess {
                logical_address: 0xa10003,
                bus_address: 0xa10003,
                ..access
            },
            BusAccess {
                logical_address: 0xa10020,
                bus_address: 0xa10020,
                ..access
            },
            BusAccess {
                kind: AccessKind::Prefetch,
                ..access
            },
            BusAccess {
                function_code: FunctionCode::SupervisorProgram,
                ..access
            },
            BusAccess {
                function_code: FunctionCode::CpuSpace,
                ..access
            },
        ] {
            assert!(
                bus.write_cycle(request, 0xff00, bus.cpu_clock)
                    .result
                    .is_err()
            );
            assert_eq!(bus.input, input);
        }
        bus.input.set_controller_raw(ControllerPort::One, 0xdf);
        assert!(bus.input.fault().is_some());
        let input = bus.input.clone();
        assert!(bus.write_cycle(access, 0, bus.cpu_clock).result.is_err());
        assert_eq!(bus.input, input);
    }
}

fn six() -> Input {
    let mut input = Input::default();
    input.set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
    input.set_controller_extra_raw(ControllerPort::One, 5);
    input.write_control(0, IO_TH, 0, Region::NtscUs);
    input
}

fn extended(input: &mut Input, start: u64) -> u64 {
    extended_with_region(input, start, Region::NtscUs)
}

fn extended_with_region(input: &mut Input, start: u64, region: Region) -> u64 {
    for edge in 0..6 {
        input.write_data(
            0,
            if edge % 2 == 0 { 0 } else { IO_TH },
            start + edge,
            region,
        );
    }
    start + 5
}

#[test]
fn pal_timeout_boundary_preserves_edges_and_read_only_state() {
    let region = Region::Pal;
    assert_eq!(region.pad_timeout_ticks(), 79_806);
    assert_eq!(Region::NtscUs.pad_timeout_ticks(), 80_540);
    let mut input = six();
    let last = extended_with_region(&mut input, 1, region);
    let saved = input.clone();
    assert_eq!(input.read_data(0, last + 79_805, region), 0x75);
    assert_eq!(input.read_data(0, last + 79_806, region), 0x7f);
    assert_eq!(input, saved);
    input.write_data(0, IO_TH, last + 79_805, region);
    input.write_control(0, IO_TH | 1, last + 79_805, region);
    assert_eq!(input.read_data(0, last + 79_806, region), 0x7e);
    input.write_control(0, IO_TH, last + 79_806, region);
    input.write_data(0, 0, last + 79_806, region);
    assert_eq!(input.read_data(0, last + 79_806, region), 0x33);

    let last = extended_with_region(&mut input, last + 79_807, region);
    input.write_data(0, 0, last + 1, region);
    assert_eq!(input.read_data(0, last + 79_806, region), 0x3f);
    assert_eq!(input.read_data(0, last + 79_807, region), 0x33);
    input.write_data(0, IO_TH, last + 79_807, region);
    input.write_data(0, 0, last + 79_808, region);
    assert_eq!(input.read_data(0, last + 79_808, region), 0x33);
    input.write_data(0, IO_TH, last + 79_809, region);
    input.write_data(0, 0, last + 79_810, region);
    assert_eq!(input.read_data(0, last + 79_810, region), 0x30);
    input.power_on();
    assert_eq!(
        input.controller_kind(ControllerPort::One),
        ControllerKind::SixButton
    );
    assert_eq!(input.read_data(0, 0, region), 0x7f);
}

#[test]
fn timeout_boundary_does_not_mutate_on_reads_or_refresh_on_same_level() {
    let mut input = six();
    let last = extended(&mut input, 1);
    let saved = input.clone();
    assert_eq!(
        input.read_data(0, last + PAD_SIX_TIMEOUT_TICKS - 1, Region::NtscUs),
        0x75
    );
    assert_eq!(
        input.read_data(0, last + PAD_SIX_TIMEOUT_TICKS, Region::NtscUs),
        0x7f
    );
    assert_eq!(input, saved);
    input.write_data(0, IO_TH, last + PAD_SIX_TIMEOUT_TICKS - 1, Region::NtscUs);
    input.write_control(
        0,
        IO_TH | 1,
        last + PAD_SIX_TIMEOUT_TICKS - 1,
        Region::NtscUs,
    );
    assert_eq!(
        input.read_data(0, last + PAD_SIX_TIMEOUT_TICKS, Region::NtscUs),
        0x7e
    );
    input.write_control(0, IO_TH, last + PAD_SIX_TIMEOUT_TICKS, Region::NtscUs);
    input.write_data(0, 0, last + PAD_SIX_TIMEOUT_TICKS, Region::NtscUs);
    assert_eq!(
        input.read_data(0, last + PAD_SIX_TIMEOUT_TICKS, Region::NtscUs),
        0x33
    );
}

#[test]
fn low_timeout_restarts_rising_count_and_keeps_ports_independent() {
    let mut input = six();
    input.set_controller_kind(ControllerPort::Two, ControllerKind::SixButton);
    let last = extended(&mut input, 1);
    input.write_data(0, 0, last + 1, Region::NtscUs);
    let expiry = last + 1 + PAD_SIX_TIMEOUT_TICKS;
    assert_eq!(input.read_data(0, expiry - 1, Region::NtscUs), 0x3f);
    assert_eq!(input.read_data(0, expiry, Region::NtscUs), 0x33);
    input.write_control(1, IO_TH, expiry, Region::NtscUs);
    for edge in 0..6 {
        input.write_data(
            1,
            if edge % 2 == 0 { 0 } else { IO_TH },
            expiry + edge,
            Region::NtscUs,
        );
    }
    assert_eq!(input.read_data(0, expiry + 5, Region::NtscUs), 0x33);
    input.write_data(0, IO_TH, expiry + 6, Region::NtscUs);
    input.write_data(0, 0, expiry + 7, Region::NtscUs);
    assert_eq!(input.read_data(0, expiry + 7, Region::NtscUs), 0x33);
    input.write_data(0, IO_TH, expiry + 8, Region::NtscUs);
    input.write_data(0, 0, expiry + 9, Region::NtscUs);
    assert_eq!(input.read_data(0, expiry + 9, Region::NtscUs), 0x30);
}

#[test]
fn effective_th_and_output_pin_masks() {
    let mut input = six();
    input.write_control(0, 0, 0, Region::NtscUs);
    for tick in 1..20 {
        input.write_data(
            0,
            if tick % 2 == 0 { IO_TH } else { 0 },
            tick,
            Region::NtscUs,
        );
    }
    input.write_control(0, IO_TH, 20, Region::NtscUs);
    input.write_control(0, 0, 21, Region::NtscUs);
    input.write_control(0, IO_TH, 22, Region::NtscUs);
    assert_eq!(input.read_data(0, 22, Region::NtscUs), 0x33);
    input.write_control(0, 0, 23, Region::NtscUs);
    input.write_control(0, IO_TH, 24, Region::NtscUs);
    assert_eq!(input.read_data(0, 24, Region::NtscUs), 0x30);
    input.write_control(0, 0, 25, Region::NtscUs);
    assert_eq!(input.read_data(0, 25, Region::NtscUs), 0x75);
    input.write_data(0, 0x80, 26, Region::NtscUs);
    input.write_control(0, 0x21, 27, Region::NtscUs);
    assert_eq!(input.read_data(0, 27, Region::NtscUs), 0xd4);
}

#[test]
fn timeout_comparison_does_not_overflow_at_end_of_clock() {
    let mut input = six();
    extended(&mut input, u64::MAX - 10);
    assert_eq!(input.read_data(0, u64::MAX, Region::NtscUs), 0x75);
}

#[test]
fn completed_sequence_saturates_until_timeout() {
    let mut input = six();
    let last = extended(&mut input, 1);
    input.write_data(0, 0, last + 1, Region::NtscUs);
    assert_eq!(input.read_data(0, last + 1, Region::NtscUs), 0x3f);
    input.write_data(0, IO_TH, last + 2, Region::NtscUs);
    for edge in 0..12 {
        let high = edge % 2 != 0;
        input.write_data(
            0,
            if high { IO_TH } else { 0 },
            last + 3 + edge,
            Region::NtscUs,
        );
        assert_eq!(
            input.read_data(0, last + 3 + edge, Region::NtscUs),
            if high { 0x7f } else { 0x33 }
        );
    }
    let restart = last + 15 + PAD_SIX_TIMEOUT_TICKS;
    extended(&mut input, restart);
    assert_eq!(input.read_data(0, restart + 5, Region::NtscUs), 0x75);
}

#[path = "uart_idle_cases.rs"]
mod idle_cases;

#[path = "uart_tx_cases.rs"]
mod uart_tx_cases;
