use super::{
    DETERMINISTIC_VPC_RESET_REGISTERS, HuC6202, PROVISIONAL_VPC_PRIORITY_MODE_POLICY,
    PROVISIONAL_VPC_WINDOW_ORIGIN_AND_THRESHOLD, VpcPixelSelection, VpcPixelSource, VpcPort,
    VpcPriorityModePolicy, VpcVdc, VpcVdcPixel, VpcWindow, VpcWindowRegion,
};

fn port(offset: u8) -> VpcPort {
    VpcPort::from_offset(offset)
}

fn set_window(vpc: &mut HuC6202, window: VpcWindow, width: u16) {
    let offset = match window {
        VpcWindow::One => 2,
        VpcWindow::Two => 4,
    };
    vpc.write_port(port(offset), width as u8);
    vpc.write_port(port(offset + 1), (width >> 8) as u8);
}

fn pixel(index: u16, source: VpcPixelSource) -> VpcVdcPixel {
    let source_bit = match source {
        VpcPixelSource::Background => 0,
        VpcPixelSource::Sprite => 0x100,
    };
    VpcVdcPixel::new(source_bit | (index & 0xFF))
}

#[test]
fn registers_mask_window_high_bytes_and_reset_deterministically() {
    let mut vpc = HuC6202::new();
    for (offset, expected) in DETERMINISTIC_VPC_RESET_REGISTERS.into_iter().enumerate() {
        assert_eq!(vpc.read_port(port(offset as u8)), expected);
    }
    assert_eq!(vpc.direct_vdc_target(), VpcVdc::One);

    vpc.write_port(port(0), 0xA5);
    vpc.write_port(port(1), 0x5A);
    set_window(&mut vpc, VpcWindow::One, 0xFFFF);
    set_window(&mut vpc, VpcWindow::Two, 0xBEEF);
    vpc.write_port(port(6), 0xFF);
    vpc.write_port(port(7), 0xFF);

    assert_eq!(vpc.priority_control_a(), 0xA5);
    assert_eq!(vpc.priority_control_b(), 0x5A);
    assert_eq!(vpc.window_width(VpcWindow::One), 0x03FF);
    assert_eq!(vpc.window_width(VpcWindow::Two), 0x02EF);
    assert_eq!(vpc.read_port(port(3)), 3);
    assert_eq!(vpc.read_port(port(5)), 2);
    assert_eq!(vpc.direct_vdc_target(), VpcVdc::Two);
    assert_eq!(vpc.read_port(port(6)), 0);
    assert_eq!(vpc.read_port(port(7)), 0);

    vpc.reset();
    for (offset, expected) in DETERMINISTIC_VPC_RESET_REGISTERS.into_iter().enumerate() {
        assert_eq!(vpc.read_port(port(offset as u8)), expected);
    }
    assert_eq!(vpc.direct_vdc_target(), VpcVdc::One);
}

#[test]
fn window_comparator_covers_threshold_origin_and_maximum_width() {
    const { assert!(PROVISIONAL_VPC_WINDOW_ORIGIN_AND_THRESHOLD) };
    let mut vpc = HuC6202::new();

    set_window(&mut vpc, VpcWindow::One, 0x3F);
    assert_eq!(vpc.window_region(0), VpcWindowRegion::Neither);
    set_window(&mut vpc, VpcWindow::One, 0x40);
    assert_eq!(vpc.window_region(0), VpcWindowRegion::WindowOne);
    assert_eq!(vpc.window_region(1), VpcWindowRegion::Neither);
    set_window(&mut vpc, VpcWindow::One, 0x41);
    assert_eq!(vpc.window_region(1), VpcWindowRegion::WindowOne);
    assert_eq!(vpc.window_region(2), VpcWindowRegion::Neither);
    set_window(&mut vpc, VpcWindow::One, 0x3FF);
    assert_eq!(vpc.window_region(0x3BF), VpcWindowRegion::WindowOne);
    assert_eq!(vpc.window_region(0x3C0), VpcWindowRegion::Neither);
}

#[test]
fn priority_register_nibbles_map_to_all_four_window_regions() {
    let mut vpc = HuC6202::new();
    vpc.write_port(port(0), 0x21);
    vpc.write_port(port(1), 0x84);
    let one = pixel(0x001, VpcPixelSource::Background);
    let two = pixel(0x102, VpcPixelSource::Background);

    set_window(&mut vpc, VpcWindow::One, 0x40);
    set_window(&mut vpc, VpcWindow::Two, 0x40);
    assert_eq!(vpc.window_region(0), VpcWindowRegion::Both);
    assert_eq!(
        vpc.select_pixel(0, one, two).selected_vdc(),
        Some(VpcVdc::One)
    );

    set_window(&mut vpc, VpcWindow::One, 0x3F);
    assert_eq!(vpc.window_region(0), VpcWindowRegion::WindowTwo);
    assert_eq!(
        vpc.select_pixel(0, one, two).selected_vdc(),
        Some(VpcVdc::Two)
    );

    set_window(&mut vpc, VpcWindow::One, 0x40);
    set_window(&mut vpc, VpcWindow::Two, 0x3F);
    assert_eq!(vpc.window_region(0), VpcWindowRegion::WindowOne);
    assert_eq!(vpc.select_pixel(0, one, two), VpcPixelSelection::Backdrop);

    set_window(&mut vpc, VpcWindow::One, 0x3F);
    assert_eq!(vpc.window_region(0), VpcWindowRegion::Neither);
    assert_eq!(vpc.select_pixel(0, one, two), VpcPixelSelection::Backdrop);
}

#[test]
fn priority_modes_match_each_explicit_source_pair() {
    let source_pairs = [
        (VpcPixelSource::Background, VpcPixelSource::Background),
        (VpcPixelSource::Background, VpcPixelSource::Sprite),
        (VpcPixelSource::Sprite, VpcPixelSource::Background),
        (VpcPixelSource::Sprite, VpcPixelSource::Sprite),
    ];
    let expected = [
        [VpcVdc::One, VpcVdc::One, VpcVdc::One, VpcVdc::One],
        [VpcVdc::One, VpcVdc::Two, VpcVdc::One, VpcVdc::One],
        [VpcVdc::One, VpcVdc::One, VpcVdc::Two, VpcVdc::One],
        [VpcVdc::One, VpcVdc::One, VpcVdc::One, VpcVdc::One],
    ];

    for (mode, expected) in expected.into_iter().enumerate() {
        let mut vpc = HuC6202::new();
        vpc.write_port(port(0), 3 | ((mode as u8) << 2));
        set_window(&mut vpc, VpcWindow::One, 0x40);
        set_window(&mut vpc, VpcWindow::Two, 0x40);
        for ((one_source, two_source), expected) in source_pairs.into_iter().zip(expected) {
            let selected = vpc.select_pixel(0, pixel(1, one_source), pixel(2, two_source));
            assert_eq!(selected.selected_vdc(), Some(expected));
        }
    }
}

#[test]
fn selector_exhausts_modes_enables_transparency_and_source_pairs() {
    assert_eq!(
        PROVISIONAL_VPC_PRIORITY_MODE_POLICY,
        VpcPriorityModePolicy::GeargrafxMameCompatibility
    );
    for mode in 0..4 {
        for enables in 0..4 {
            for opaque in 0..4 {
                for one_source in [VpcPixelSource::Background, VpcPixelSource::Sprite] {
                    for two_source in [VpcPixelSource::Background, VpcPixelSource::Sprite] {
                        let mut vpc = HuC6202::new();
                        vpc.write_port(port(0), enables | (mode << 2));
                        set_window(&mut vpc, VpcWindow::One, 0x40);
                        set_window(&mut vpc, VpcWindow::Two, 0x40);
                        let one = pixel(if opaque & 1 == 0 { 0x010 } else { 0x011 }, one_source);
                        let two = pixel(if opaque & 2 == 0 { 0x120 } else { 0x122 }, two_source);
                        let one_transparent = opaque & 1 == 0;
                        let two_transparent = opaque & 2 == 0;
                        let expected = match enables {
                            0 => None,
                            1 => Some(VpcVdc::One),
                            2 => Some(VpcVdc::Two),
                            _ if one_transparent && !two_transparent => Some(VpcVdc::Two),
                            _ if !one_transparent && two_transparent => Some(VpcVdc::One),
                            _ if mode == 1
                                && one_source == VpcPixelSource::Background
                                && two_source == VpcPixelSource::Sprite =>
                            {
                                Some(VpcVdc::Two)
                            }
                            _ if mode == 2
                                && one_source == VpcPixelSource::Sprite
                                && two_source == VpcPixelSource::Background =>
                            {
                                Some(VpcVdc::Two)
                            }
                            _ => Some(VpcVdc::One),
                        };
                        let selected = vpc.select_pixel(0, one, two);
                        assert_eq!(selected.selected_vdc(), expected);
                        assert_eq!(
                            selected.palette_index(),
                            match expected {
                                None => 0,
                                Some(VpcVdc::One) => one.palette_index(),
                                Some(VpcVdc::Two) => two.palette_index(),
                            }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn transparent_bus_values_follow_enable_and_underlying_priority_rules() {
    let background_zero = VpcVdcPixel::new(0x000);
    let sprite_zero = VpcVdcPixel::new(0x100);
    let opaque = VpcVdcPixel::new(0x001);
    let mut vpc = HuC6202::new();
    set_window(&mut vpc, VpcWindow::One, 0x40);
    set_window(&mut vpc, VpcWindow::Two, 0x40);

    vpc.write_port(port(0), 0);
    assert_eq!(
        vpc.select_pixel(0, sprite_zero, opaque),
        VpcPixelSelection::Backdrop
    );
    vpc.write_port(port(0), 1);
    assert_eq!(
        vpc.select_pixel(0, sprite_zero, opaque),
        VpcPixelSelection::VdcOne(sprite_zero)
    );
    vpc.write_port(port(0), 2);
    assert_eq!(
        vpc.select_pixel(0, opaque, sprite_zero),
        VpcPixelSelection::VdcTwo(sprite_zero)
    );
    vpc.write_port(port(0), 3);
    assert_eq!(
        vpc.select_pixel(0, background_zero, opaque),
        VpcPixelSelection::VdcTwo(opaque)
    );
    assert_eq!(
        vpc.select_pixel(0, opaque, sprite_zero),
        VpcPixelSelection::VdcOne(opaque)
    );
    assert_eq!(
        vpc.select_pixel(0, background_zero, sprite_zero),
        VpcPixelSelection::VdcOne(background_zero)
    );
    vpc.write_port(port(0), 7);
    assert_eq!(
        vpc.select_pixel(0, background_zero, sprite_zero),
        VpcPixelSelection::VdcTwo(sprite_zero)
    );
}
