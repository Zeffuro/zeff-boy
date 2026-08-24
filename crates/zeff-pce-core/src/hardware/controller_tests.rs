use super::{
    BaseBus, BaseBusDevices, ControllerDevice, ControllerPort,
    DETERMINISTIC_SIX_BUTTON_RESET_PHASE, FivePortMultitap, MULTITAP_EXHAUSTED_NIBBLE,
    MultitapDevice, MultitapPort, PROVISIONAL_MOUSE_CLEAR_TIMEOUT_MASTER_TICKS,
    PROVISIONAL_MOUSE_SELECT_TIMEOUT_MASTER_TICKS, PadButtons, SixButtonExtraButtons, SixButtonPad,
    SixButtonPhase, TwoButtonPad,
};

fn mouse_scan_nibble(port: &mut ControllerPort) -> (u8, u8) {
    port.write_lines(true, true);
    port.write_lines(true, false);
    let movement = port.read_nibble();
    port.write_lines(false, false);
    let buttons = port.read_nibble();
    (movement, buttons)
}

#[test]
fn mouse_latches_motion_once_and_returns_xy_high_then_low() {
    let mut port = ControllerPort::mouse();
    let mouse = port.mouse_mut().unwrap();
    mouse.accumulate_motion(0x2D, -0x17);
    mouse.set_buttons(PadButtons::I | PadButtons::RUN);
    port.write_lines(true, false);

    assert_eq!(mouse_scan_nibble(&mut port), (0x02, 0x06));
    assert_eq!(mouse_scan_nibble(&mut port), (0x0D, 0x06));
    assert_eq!(mouse_scan_nibble(&mut port), (0x0E, 0x06));
    assert_eq!(mouse_scan_nibble(&mut port), (0x09, 0x06));
    assert_eq!(mouse_scan_nibble(&mut port), (0x00, 0x06));
}

#[test]
fn mouse_repeated_reads_do_not_advance_and_motion_accumulates_for_next_scan() {
    let mut port = ControllerPort::mouse();
    port.mouse_mut().unwrap().accumulate_motion(1, 2);
    port.write_lines(true, false);
    port.write_lines(true, true);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0);
    assert_eq!(port.read_nibble(), 0);
    port.mouse_mut().unwrap().accumulate_motion(3, 4);

    port.advance_master_ticks(PROVISIONAL_MOUSE_CLEAR_TIMEOUT_MASTER_TICKS);
    assert_eq!(mouse_scan_nibble(&mut port).0, 0);
    assert_eq!(mouse_scan_nibble(&mut port).0, 3);
    assert_eq!(mouse_scan_nibble(&mut port).0, 0);
    assert_eq!(mouse_scan_nibble(&mut port).0, 4);
}

#[test]
fn mouse_both_timeout_paths_realign_to_x_high() {
    for timeout in [
        PROVISIONAL_MOUSE_SELECT_TIMEOUT_MASTER_TICKS,
        PROVISIONAL_MOUSE_CLEAR_TIMEOUT_MASTER_TICKS,
    ] {
        let mut port = ControllerPort::mouse();
        port.mouse_mut().unwrap().accumulate_motion(0x12, 0x34);
        port.write_lines(true, false);
        assert_eq!(mouse_scan_nibble(&mut port).0, 1);
        assert_eq!(mouse_scan_nibble(&mut port).0, 2);
        port.mouse_mut().unwrap().accumulate_motion(0x56, 0x78);
        port.advance_master_ticks(timeout);
        assert_eq!(mouse_scan_nibble(&mut port).0, 5);
    }
}

struct Devices {
    controller: ControllerPort,
    upper_input_bits: u8,
}

impl BaseBusDevices for Devices {
    fn read_controller(&mut self) -> u8 {
        self.upper_input_bits & 0xF0 | self.controller.read_nibble()
    }

    fn write_controller(&mut self, value: u8) {
        self.controller.write_lines(value & 1 != 0, value & 2 != 0);
    }
}

#[test]
fn two_button_pad_selects_active_low_direction_and_button_nibbles() {
    let mut port = ControllerPort::two_button();
    port.two_button_pad_mut()
        .unwrap()
        .set_buttons(PadButtons::UP | PadButtons::LEFT | PadButtons::I | PadButtons::RUN);

    assert_eq!(port.read_nibble(), 0);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x06);
    port.write_lines(false, false);
    assert_eq!(port.read_nibble(), 0x06);

    port.two_button_pad_mut()
        .unwrap()
        .set_buttons(PadButtons::empty());
    assert_eq!(port.read_nibble(), 0x0F);
}

#[test]
fn every_standard_pad_button_uses_the_documented_return_bit() {
    let cases = [
        (PadButtons::UP, true, 0x0E),
        (PadButtons::RIGHT, true, 0x0D),
        (PadButtons::DOWN, true, 0x0B),
        (PadButtons::LEFT, true, 0x07),
        (PadButtons::I, false, 0x0E),
        (PadButtons::II, false, 0x0D),
        (PadButtons::SELECT, false, 0x0B),
        (PadButtons::RUN, false, 0x07),
    ];

    for (button, select_high, expected) in cases {
        let mut pad = TwoButtonPad::new();
        pad.set_button(button, true);
        let mut port = ControllerPort::new(ControllerDevice::TwoButton(pad));
        port.write_lines(select_high, false);
        assert_eq!(port.read_nibble(), expected);
    }
}

#[test]
fn clear_high_forces_connected_pad_outputs_low() {
    let mut port = ControllerPort::two_button();
    port.two_button_pad_mut()
        .unwrap()
        .set_buttons(PadButtons::UP | PadButtons::I);

    port.write_lines(true, true);
    assert_eq!(port.read_nibble(), 0);
    port.write_lines(false, true);
    assert_eq!(port.read_nibble(), 0);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x0E);
}

#[test]
fn reset_restores_high_output_lines_without_clearing_input_state() {
    let mut port = ControllerPort::two_button();
    port.two_button_pad_mut()
        .unwrap()
        .set_buttons(PadButtons::LEFT);
    port.write_lines(false, false);

    port.reset();

    assert!(port.select_high());
    assert!(port.clear_high());
    assert_eq!(port.read_nibble(), 0);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x07);
}

#[test]
fn disconnected_port_stays_pulled_high_for_all_line_states() {
    let mut port = ControllerPort::default();

    for select_high in [false, true] {
        for clear_high in [false, true] {
            port.write_lines(select_high, clear_high);
            assert_eq!(port.read_nibble(), 0x0F);
        }
    }

    port.set_device(ControllerDevice::TwoButton(TwoButtonPad::new()));
    assert_eq!(port.read_nibble(), 0);
    port.set_device(ControllerDevice::Disconnected);
    assert_eq!(port.read_nibble(), 0x0F);
}

#[test]
fn test_machine_aggregate_routes_all_controller_window_mirrors() {
    let mut controller = ControllerPort::two_button();
    controller
        .two_button_pad_mut()
        .unwrap()
        .set_buttons(PadButtons::UP | PadButtons::II);
    let devices = Devices {
        controller,
        upper_input_bits: 0xD0,
    };
    let mut bus = BaseBus::new(Vec::new(), devices).unwrap();

    bus.write(0x1F_F3FF, 0xFD);
    assert_eq!(bus.read(0x1F_F000), 0xDE);
    assert_eq!(bus.read(0x1F_F2A5), 0xDE);

    bus.write(0x1F_F123, 0xFC);
    assert_eq!(bus.read(0x1F_F3FF), 0xDD);

    bus.write(0x1F_F000, 0xFF);
    assert_eq!(bus.read(0x1F_F200), 0xD0);
}

#[test]
fn six_button_pad_changes_phase_only_on_clear_rising_edges() {
    let mut port = ControllerPort::new(ControllerDevice::SixButton(SixButtonPad::with_phase(
        SixButtonPhase::Standard,
    )));
    let pad = port.six_button_pad_mut().unwrap();
    pad.standard_pad_mut()
        .set_buttons(PadButtons::UP | PadButtons::I);
    pad.set_extra_buttons(SixButtonExtraButtons::III | SixButtonExtraButtons::VI);

    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x0E);
    port.write_lines(false, false);
    assert_eq!(port.read_nibble(), 0x0E);
    assert_eq!(port.read_nibble(), 0x0E);
    port.write_lines(true, false);
    assert_eq!(
        port.six_button_pad_mut().unwrap().phase(),
        SixButtonPhase::Standard
    );

    port.write_lines(true, true);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0);
    assert_eq!(port.read_nibble(), 0);
    port.write_lines(false, false);
    assert_eq!(port.read_nibble(), 0x06);
    assert_eq!(port.read_nibble(), 0x06);
    assert_eq!(
        port.six_button_pad_mut().unwrap().phase(),
        SixButtonPhase::Extended
    );

    port.write_lines(true, false);
    port.write_lines(true, true);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x0E);
}

#[test]
fn every_six_button_extra_button_uses_the_documented_return_bit() {
    for (button, expected) in [
        (SixButtonExtraButtons::III, 0x0E),
        (SixButtonExtraButtons::IV, 0x0D),
        (SixButtonExtraButtons::V, 0x0B),
        (SixButtonExtraButtons::VI, 0x07),
    ] {
        let mut pad = SixButtonPad::with_phase(SixButtonPhase::Extended);
        pad.set_extra_button(button, true);
        let mut port = ControllerPort::new(ControllerDevice::SixButton(pad));
        port.write_lines(false, false);
        assert_eq!(port.read_nibble(), expected);
        assert_eq!(port.read_nibble(), expected);
    }

    let mut pad = SixButtonPad::with_phase(SixButtonPhase::Extended);
    pad.standard_pad_mut().set_button(PadButtons::UP, true);
    let mut port = ControllerPort::new(ControllerDevice::SixButton(pad));
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0);
    port.write_lines(true, true);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x0E);
}

#[test]
fn controller_reset_applies_deterministic_six_button_phase_policy_from_low_lines() {
    let mut port = ControllerPort::six_button();
    port.six_button_pad_mut()
        .unwrap()
        .standard_pad_mut()
        .set_button(PadButtons::LEFT, true);
    port.write_lines(true, false);
    port.write_lines(true, true);
    port.write_lines(false, false);
    assert_eq!(
        port.six_button_pad_mut().unwrap().phase(),
        SixButtonPhase::Extended
    );

    port.reset();

    assert!(port.select_high());
    assert!(port.clear_high());
    assert_eq!(
        port.six_button_pad_mut().unwrap().phase(),
        DETERMINISTIC_SIX_BUTTON_RESET_PHASE
    );
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x07);
}

#[test]
fn multitap_resets_to_port_one_and_advances_on_select_rising_edges() {
    let pad = |buttons| {
        let mut pad = TwoButtonPad::new();
        pad.set_buttons(buttons);
        MultitapDevice::TwoButton(pad)
    };
    let multitap = FivePortMultitap::new([
        pad(PadButtons::UP | PadButtons::II),
        pad(PadButtons::RIGHT | PadButtons::SELECT),
        pad(PadButtons::DOWN | PadButtons::RUN),
        pad(PadButtons::LEFT | PadButtons::I),
        pad(PadButtons::UP | PadButtons::RIGHT | PadButtons::II | PadButtons::RUN),
    ]);
    let mut port = ControllerPort::multitap(multitap);

    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), MULTITAP_EXHAUSTED_NIBBLE);
    port.write_lines(true, true);
    assert_eq!(
        port.multitap_mut().unwrap().active_port(),
        Some(MultitapPort::One)
    );
    assert_eq!(port.read_nibble(), 0);
    port.write_lines(true, false);

    for (index, (active, expected_high, expected_low)) in [
        (MultitapPort::One, 0x0E, 0x0D),
        (MultitapPort::Two, 0x0D, 0x0B),
        (MultitapPort::Three, 0x0B, 0x07),
        (MultitapPort::Four, 0x07, 0x0E),
        (MultitapPort::Five, 0x0C, 0x05),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(port.multitap_mut().unwrap().active_port(), Some(active));
        assert_eq!(port.read_nibble(), expected_high);
        port.write_lines(false, false);
        assert_eq!(port.read_nibble(), expected_low);
        assert_eq!(port.read_nibble(), expected_low);
        if index != 4 {
            port.write_lines(true, false);
        }
    }

    port.write_lines(true, false);
    assert_eq!(port.multitap_mut().unwrap().active_port(), None);
    assert_eq!(port.read_nibble(), MULTITAP_EXHAUSTED_NIBBLE);
}

#[test]
fn multitap_no_port_is_zero_but_a_disconnected_socket_is_pulled_high() {
    let mut port = ControllerPort::multitap(FivePortMultitap::default());
    assert_eq!(port.read_nibble(), MULTITAP_EXHAUSTED_NIBBLE);

    port.write_lines(true, false);
    port.write_lines(true, true);
    port.write_lines(true, false);
    assert_eq!(
        port.multitap_mut().unwrap().active_port(),
        Some(MultitapPort::One)
    );
    assert_eq!(port.read_nibble(), 0x0F);

    for _ in 0..5 {
        port.write_lines(false, false);
        port.write_lines(true, false);
    }
    assert_eq!(port.multitap_mut().unwrap().active_port(), None);
    assert_eq!(port.read_nibble(), MULTITAP_EXHAUSTED_NIBBLE);
}

#[test]
fn multitap_clear_reset_requires_select_high() {
    let mut port = ControllerPort::multitap(FivePortMultitap::default());
    port.write_lines(true, false);
    port.write_lines(true, true);
    assert_eq!(port.read_nibble(), 0x0F);
    port.write_lines(true, false);
    port.write_lines(false, false);
    port.write_lines(true, false);
    assert_eq!(
        port.multitap_mut().unwrap().active_port(),
        Some(MultitapPort::Two)
    );

    port.write_lines(false, false);
    port.write_lines(false, true);
    assert_eq!(
        port.multitap_mut().unwrap().active_port(),
        Some(MultitapPort::Two)
    );
    port.write_lines(false, false);
    port.write_lines(true, true);
    assert_eq!(
        port.multitap_mut().unwrap().active_port(),
        Some(MultitapPort::One)
    );
}

#[test]
fn multitap_advances_only_the_phase_of_the_pad_leaving_active_service() {
    let mut first_pad = SixButtonPad::new();
    first_pad
        .standard_pad_mut()
        .set_button(PadButtons::UP, true);
    let six = MultitapDevice::SixButton(SixButtonPad::new());
    let mut port = ControllerPort::multitap(FivePortMultitap::new([
        MultitapDevice::SixButton(first_pad),
        six,
        six,
        MultitapDevice::Disconnected,
        MultitapDevice::Disconnected,
    ]));
    let phases = |port: &mut ControllerPort| {
        let multitap = port.multitap_mut().unwrap();
        [MultitapPort::One, MultitapPort::Two, MultitapPort::Three].map(|tap_port| match *multitap
            .port(tap_port)
        {
            MultitapDevice::SixButton(pad) => pad.phase(),
            _ => unreachable!(),
        })
    };

    port.write_lines(true, false);
    port.write_lines(true, true);
    port.write_lines(true, false);
    assert_eq!(phases(&mut port), [SixButtonPhase::Standard; 3]);

    port.write_lines(false, false);
    port.write_lines(true, false);
    assert_eq!(
        phases(&mut port),
        [
            SixButtonPhase::Extended,
            SixButtonPhase::Standard,
            SixButtonPhase::Standard,
        ]
    );
    assert_eq!(port.read_nibble(), 0x0F);
    assert_eq!(port.read_nibble(), 0x0F);
    assert_eq!(
        phases(&mut port),
        [
            SixButtonPhase::Extended,
            SixButtonPhase::Standard,
            SixButtonPhase::Standard,
        ]
    );

    port.write_lines(false, false);
    port.write_lines(true, false);
    assert_eq!(
        phases(&mut port),
        [
            SixButtonPhase::Extended,
            SixButtonPhase::Extended,
            SixButtonPhase::Standard,
        ]
    );

    port.write_lines(false, false);
    port.reset();
    assert_eq!(port.multitap_mut().unwrap().active_port(), None);
    assert_eq!(phases(&mut port), [SixButtonPhase::Standard; 3]);
    assert!(port.select_high());
    assert!(port.clear_high());
    port.write_lines(true, false);
    port.write_lines(true, true);
    port.write_lines(true, false);
    assert_eq!(port.read_nibble(), 0x0E);
}
