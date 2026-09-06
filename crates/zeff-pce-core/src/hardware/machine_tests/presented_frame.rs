use super::*;

#[test]
fn reset_presented_frame_is_empty_and_describes_fixed_storage() {
    let machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    let frame = machine.presented_frame();

    assert_eq!(
        frame.storage_dimensions(),
        (PCE_ACTIVE_FRAME_WIDTH, PCE_ACTIVE_FRAME_HEIGHT)
    );
    assert_eq!(frame.rgba().len(), PCE_ACTIVE_FRAME_RGBA_BYTES);
    assert_eq!(frame.rows().len(), PCE_ACTIVE_FRAME_HEIGHT);
    assert!(frame.rows().iter().all(|row| !row.is_active()));
    assert_eq!(frame.active_bounds(), None);
    assert_eq!(frame.signal_bounds().first_row(), PCE_SIGNAL_FIRST_ROW);
    assert_eq!(frame.signal_bounds().row_end(), PCE_SIGNAL_ROW_END);
    assert_eq!(frame.signal_bounds().height(), 242);
    assert!(
        frame
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == PCE_ACTIVE_FRAME_UNUSED_RGBA)
    );
}

#[test]
fn absolute_vce_rows_preserve_224_239_and_full_240_signal_placement() {
    for vce_control in [0, 0x04] {
        for (vertical_sync, vertical_display, vertical_end, expected) in [
            (0x1702, 0x00DF, 0x000A, (28, 252, 224)),
            (0x0F02, 0x00EF, 0x0004, (20, 260, 239)),
            (0x0E02, 0x00EF, 0x0004, (19, 259, 240)),
        ] {
            let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
            machine
                .devices_mut()
                .vce_mut()
                .write_port(VcePort::from_offset(0), vce_control);
            write_vdc_register(
                machine.devices_mut(),
                VdcRegister::VerticalSync,
                vertical_sync,
            );
            write_vdc_register(
                machine.devices_mut(),
                VdcRegister::VerticalDisplay,
                vertical_display,
            );
            write_vdc_register(
                machine.devices_mut(),
                VdcRegister::VerticalDisplayEnd,
                vertical_end,
            );

            machine.run_until_frame().unwrap();

            let frame = machine.presented_frame();
            let active = frame.active_bounds().unwrap();
            assert_eq!(
                (active.first_row(), active.row_end()),
                (expected.0, expected.1)
            );
            let signal = frame.signal_bounds();
            assert_eq!(
                (signal.first_row(), signal.row_end()),
                (PCE_SIGNAL_FIRST_ROW, PCE_SIGNAL_ROW_END)
            );
            let visible_start = active.first_row().max(signal.first_row());
            let visible_end = active.row_end().min(signal.row_end());
            assert_eq!(visible_end - visible_start, expected.2);
        }
    }
}

#[test]
fn presented_frame_reports_variable_active_widths_clocks_and_bounds() {
    let mut machine = PceMachine::new(high_speed_loop_rom()).unwrap();
    configure_external_262(machine.devices_mut(), 0);
    write_vdc_register(machine.devices_mut(), VdcRegister::HorizontalDisplay, 31);
    advance_to_vce_line(&mut machine, 4);
    write_vdc_register(machine.devices_mut(), VdcRegister::HorizontalDisplay, 63);
    machine
        .devices_mut()
        .vce_mut()
        .write_port(VcePort::from_offset(0), 1);
    machine.run_until_frame().unwrap();

    let frame = machine.presented_frame();
    assert_eq!(frame.rows()[3].active_width(), 256);
    assert_eq!(
        frame.rows()[3].pixel_clock(),
        Some(VcePixelClock::DivideByFour)
    );
    assert_eq!(frame.rows()[4].active_width(), 512);
    assert_eq!(
        frame.rows()[4].pixel_clock(),
        Some(VcePixelClock::DivideByThree)
    );
    let bounds = frame.active_bounds().unwrap();
    assert_eq!(bounds.first_row(), 3);
    assert_eq!(bounds.row_end(), 261);
    assert_eq!(bounds.height(), 258);
    assert_eq!(bounds.maximum_width(), 512);
}
