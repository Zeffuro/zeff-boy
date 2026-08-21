use super::{
    BaseBus, DETERMINISTIC_VCE_INITIAL_COLOR, DETERMINISTIC_VCE_RESET_PRESERVES_PALETTE,
    DETERMINISTIC_VCE_RESET_VALUE, HuC6260, VCE_UNAVAILABLE_READ_VALUE, VceColor, VceFrameLength,
    VcePixelClock, VcePort,
};

fn port(offset: u8) -> VcePort {
    VcePort::from_offset(offset)
}

fn set_address(vce: &mut HuC6260, address: u16) {
    vce.write_port(port(2), address as u8);
    vce.write_port(port(3), (address >> 8) as u8);
}

#[test]
fn control_masks_reserved_bits_and_exposes_all_clock_modes() {
    let mut vce = HuC6260::new();
    for (raw, clock, divisor) in [
        (0, VcePixelClock::DivideByFour, 4),
        (1, VcePixelClock::DivideByThree, 3),
        (2, VcePixelClock::DivideByTwo, 2),
        (3, VcePixelClock::DivideByTwo, 2),
    ] {
        vce.write_port(port(0), raw);
        assert_eq!(vce.control(), raw);
        assert_eq!(vce.pixel_clock(), clock);
        assert_eq!(vce.pixel_clock().divisor(), divisor);
        assert_eq!(
            vce.frame_length(),
            if raw & 0x04 == 0 {
                VceFrameLength::Lines262
            } else {
                VceFrameLength::Lines263
            }
        );
    }

    vce.write_port(port(0), 0xFF);
    assert_eq!(vce.control(), 0x87);
    assert!(vce.blur_enabled());
    assert!(vce.monochrome_enabled());
    assert_eq!(vce.frame_length(), VceFrameLength::Lines263);
}

#[test]
fn address_bytes_update_independently_and_mask_to_nine_bits() {
    let mut vce = HuC6260::new();
    vce.write_port(port(3), 0xFF);
    assert_eq!(vce.color_table_address(), 0x100);
    vce.write_port(port(2), 0xA5);
    assert_eq!(vce.color_table_address(), 0x1A5);
    vce.write_port(port(3), 0xFE);
    assert_eq!(vce.color_table_address(), 0x0A5);
    vce.write_port(port(2), 0x5A);
    assert_eq!(vce.color_table_address(), 0x05A);
}

#[test]
fn palette_writes_are_immediate_and_high_byte_writes_increment() {
    let mut vce = HuC6260::new();
    set_address(&mut vce, 0x1FF);
    vce.write_port(port(4), 0xA5);
    assert_eq!(vce.palette()[0x1FF].raw(), 0x0A5);
    assert_eq!(vce.color_table_address(), 0x1FF);

    vce.write_port(port(5), 0xFF);
    assert_eq!(vce.palette()[0x1FF].raw(), 0x1A5);
    assert_eq!(vce.color_table_address(), 0);

    vce.write_port(port(4), 0x3C);
    vce.write_port(port(5), 0xFE);
    assert_eq!(vce.palette()[0].raw(), 0x03C);
    assert_eq!(vce.color_table_address(), 1);
}

#[test]
fn repeated_high_byte_writes_preserve_each_destination_low_byte() {
    let mut vce = HuC6260::new();
    vce.write_port(port(4), 0x12);
    vce.write_port(port(5), 0);
    vce.write_port(port(4), 0x34);
    vce.write_port(port(5), 0);

    set_address(&mut vce, 0);
    vce.write_port(port(5), 1);
    vce.write_port(port(5), 1);

    assert_eq!(vce.palette()[0].raw(), 0x112);
    assert_eq!(vce.palette()[1].raw(), 0x134);
    assert_eq!(vce.color_table_address(), 2);
}

#[test]
fn palette_write_stream_crosses_the_address_high_bit() {
    let mut vce = HuC6260::new();
    set_address(&mut vce, 0x0FF);
    vce.write_port(port(4), 0xA5);
    vce.write_port(port(5), 1);
    vce.write_port(port(4), 0x5A);
    vce.write_port(port(5), 0);

    assert_eq!(vce.palette()[0x0FF].raw(), 0x1A5);
    assert_eq!(vce.palette()[0x100].raw(), 0x05A);
    assert_eq!(vce.color_table_address(), 0x101);
}

#[test]
fn palette_reads_increment_only_after_the_high_byte() {
    let mut vce = HuC6260::new();
    set_address(&mut vce, 0x1FF);
    vce.write_port(port(4), 0x7B);
    vce.write_port(port(5), 1);
    vce.write_port(port(4), 0x24);
    vce.write_port(port(5), 0);

    set_address(&mut vce, 0x1FF);
    assert_eq!(vce.read_port(port(4)), 0x7B);
    assert_eq!(vce.read_port(port(4)), 0x7B);
    assert_eq!(vce.color_table_address(), 0x1FF);
    assert_eq!(vce.read_port(port(5)), 0xFF);
    assert_eq!(vce.color_table_address(), 0);
    assert_eq!(vce.read_port(port(4)), 0x24);
    assert_eq!(vce.read_port(port(5)), 0xFE);
    assert_eq!(vce.color_table_address(), 1);

    set_address(&mut vce, 0x1FF);
    assert_eq!(vce.read_port(port(5)), 0xFF);
    assert_eq!(vce.color_table_address(), 0);
    assert_eq!(vce.read_port(port(4)), 0x24);
}

#[test]
fn unavailable_ports_use_the_named_policy_without_side_effects() {
    let mut vce = HuC6260::new();
    set_address(&mut vce, 0x123);
    for offset in [0, 1, 2, 3, 6, 7] {
        assert_eq!(vce.read_port(port(offset)), VCE_UNAVAILABLE_READ_VALUE);
        assert_eq!(vce.color_table_address(), 0x123);
    }
    for offset in [1, 6, 7] {
        vce.write_port(port(offset), 0xFF);
    }
    assert_eq!(vce.control(), 0);
    assert_eq!(vce.color_table_address(), 0x123);
    assert_eq!(vce.palette()[0x123], DETERMINISTIC_VCE_INITIAL_COLOR);
}

#[test]
fn colors_mask_to_nine_bits_and_convert_green_red_blue_components() {
    assert_eq!(VceColor::new(0xFFFF).raw(), 0x01FF);
    assert_eq!(VceColor::new(0).rgb8(), [0, 0, 0]);
    assert_eq!(VceColor::new(0x0038).rgb8(), [255, 0, 0]);
    assert_eq!(VceColor::new(0x01C0).rgb8(), [0, 255, 0]);
    assert_eq!(VceColor::new(0x0007).rgb8(), [0, 0, 255]);
    assert_eq!(VceColor::new(0x00D1).rgb8(), [72, 109, 36]);
}

#[test]
fn reset_uses_named_internal_and_palette_policies() {
    let mut vce = HuC6260::new();
    assert_eq!(vce.palette()[0], DETERMINISTIC_VCE_INITIAL_COLOR);
    set_address(&mut vce, 0x123);
    vce.write_port(port(4), 0xAB);
    vce.write_port(port(5), 1);
    vce.write_port(port(0), 0x87);

    vce.reset();

    assert_eq!(vce.control(), DETERMINISTIC_VCE_RESET_VALUE as u8);
    assert_eq!(vce.color_table_address(), DETERMINISTIC_VCE_RESET_VALUE);
    assert_eq!(
        vce.palette()[0x123].raw() == 0x1AB,
        DETERMINISTIC_VCE_RESET_PRESERVES_PALETTE
    );
}

#[test]
fn base_bus_routes_sampled_eight_byte_vce_mirrors() {
    let mut bus = BaseBus::new(Vec::new(), HuC6260::new()).unwrap();
    bus.write(0x1F_E7F8, 0xFF);
    bus.write(0x1F_E4FA, 0x34);
    bus.write(0x1F_E6FB, 0x01);
    bus.write(0x1F_E57C, 0xCD);
    bus.write(0x1F_E67D, 0x01);

    assert_eq!(bus.devices().control(), 0x87);
    assert_eq!(bus.devices().palette()[0x134].raw(), 0x1CD);
    assert_eq!(bus.devices().color_table_address(), 0x135);
    bus.write(0x1F_E402, 0x34);
    bus.write(0x1F_E403, 0x01);
    assert_eq!(bus.read(0x1F_E6FC), 0xCD);
    assert_eq!(bus.read(0x1F_E7FD), 0xFF);
    assert_eq!(bus.devices().color_table_address(), 0x135);
    assert_eq!(bus.read(0x1F_E7FF), VCE_UNAVAILABLE_READ_VALUE);
}
