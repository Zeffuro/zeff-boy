use super::cpu::VdcPort;
use super::{
    HuC6260, HuC6270, PCE_ACTIVE_FRAME_HEIGHT, PCE_ACTIVE_FRAME_RGBA_BYTES,
    PCE_ACTIVE_FRAME_UNUSED_RGBA, PCE_ACTIVE_FRAME_WIDTH, PceActiveOnlyVideoFrame,
    SpriteScanlineStatus, VDC_SATB_WORDS, VcePixelClock, VcePort, VdcActiveDisplayLine,
    VdcDmaChannel, VdcDmaProgress, VdcRegister, VdcStatus,
};

fn write_vdc(vdc: &mut HuC6270, register: VdcRegister, value: u16) {
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

fn configure_frame(vdc: &mut HuC6270, display: u16, display_end: u16, control: u16) {
    write_vdc(vdc, VdcRegister::Control, 0x0030 | control);
    write_vdc(vdc, VdcRegister::VerticalSync, 0);
    write_vdc(vdc, VdcRegister::VerticalDisplay, display);
    write_vdc(vdc, VdcRegister::VerticalDisplayEnd, display_end);
}

fn next_active(vdc: &mut HuC6270) -> VdcActiveDisplayLine {
    loop {
        if let Some(display) = vdc.advance_scanline_boundary().unwrap().active_display() {
            return display;
        }
    }
}

fn vce_port(offset: u8) -> VcePort {
    VcePort::from_offset(offset)
}

fn write_color(vce: &mut HuC6260, index: u16, raw: u16) {
    vce.write_port(vce_port(2), index as u8);
    vce.write_port(vce_port(3), (index >> 8) as u8);
    vce.write_port(vce_port(4), raw as u8);
    vce.write_port(vce_port(5), (raw >> 8) as u8);
}

fn pixel(frame: &PceActiveOnlyVideoFrame, x: usize, y: usize) -> [u8; 4] {
    let offset = (y * PCE_ACTIVE_FRAME_WIDTH + x) * 4;
    frame.framebuffer()[offset..offset + 4].try_into().unwrap()
}

fn load_satb(vdc: &mut HuC6270, words: &[u16; VDC_SATB_WORDS]) {
    let source = 0x4000;
    vdc.vram_mut()[source..source + VDC_SATB_WORDS].copy_from_slice(words);
    write_vdc(vdc, VdcRegister::SatbSource, source as u16);
    assert!(vdc.start_satb_dma_for_vertical_blank());
    for index in 0..VDC_SATB_WORDS {
        let progress = vdc.service_dma_slot(VdcDmaChannel::Satb).unwrap();
        if index + 1 == VDC_SATB_WORDS {
            assert_eq!(progress, VdcDmaProgress::Complete);
        }
    }
}

fn collision_and_overflow_satb(pattern_code: u16) -> [u16; VDC_SATB_WORDS] {
    let mut satb = [0; VDC_SATB_WORDS];
    for index in 0..17 {
        let base = index * 4;
        satb[base] = 64;
        satb[base + 1] = if index < 2 { 432 } else { 900 };
        satb[base + 2] = pattern_code;
    }
    satb
}

fn rendered_sprite_events(status: SpriteScanlineStatus) -> super::SpriteScanlineEvents {
    let SpriteScanlineStatus::Rendered(events) = status else {
        panic!("sprites should be enabled")
    };
    events
}

#[test]
fn active_width_uses_hdr_and_ignores_hsr() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 1, 1, 0);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 31);
    write_vdc(&mut vdc, VdcRegister::HorizontalSync, 0);

    assert_eq!(next_active(&mut vdc).source_width(), 256);
    write_vdc(&mut vdc, VdcRegister::HorizontalSync, 0x7F1F);
    assert_eq!(next_active(&mut vdc).source_width(), 256);

    let mut maximum = HuC6270::new();
    configure_frame(&mut maximum, 0, 1, 0);
    write_vdc(&mut maximum, VdcRegister::HorizontalDisplay, 0x7F7F);
    assert_eq!(next_active(&mut maximum).source_width(), 1024);
}

#[test]
fn active_only_frame_uses_backdrop_zero_and_host_black_outside_the_row() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 31);
    let display = next_active(&mut vdc);
    let mut vce = HuC6260::new();
    write_color(&mut vce, 0, 0x0038);
    write_color(&mut vce, 0x100, 0x0007);
    let mut frame = PceActiveOnlyVideoFrame::new();

    frame
        .render_active_line(
            &mut vdc,
            &vce,
            display,
            display.display_line(),
            vce.pixel_clock(),
        )
        .unwrap();

    assert_eq!(frame.dimensions(), (1024, 512));
    assert_eq!(frame.framebuffer().len(), PCE_ACTIVE_FRAME_RGBA_BYTES);
    assert_eq!(pixel(&frame, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 255, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 256, 0), PCE_ACTIVE_FRAME_UNUSED_RGBA);
    assert_eq!(pixel(&frame, 0, 1), PCE_ACTIVE_FRAME_UNUSED_RGBA);
    let metadata = frame.row_metadata(0).unwrap();
    assert_eq!(metadata.active_x_origin(), display.source_start());
    assert_eq!(metadata.active_width(), 256);
    assert_eq!(metadata.pixel_clock(), Some(VcePixelClock::DivideByFour));

    frame.begin_frame();
    assert_eq!(pixel(&frame, 0, 0), PCE_ACTIVE_FRAME_UNUSED_RGBA);
    assert_eq!(frame.row_metadata(0).unwrap().active_width(), 0);
    assert_eq!(frame.row_metadata(0).unwrap().pixel_clock(), None);
}

#[test]
fn active_row_records_effective_background_fetch_origin() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0x0080);
    write_vdc(&mut vdc, VdcRegister::MemoryWidth, 0x0050);
    write_vdc(&mut vdc, VdcRegister::BackgroundScrollX, 19);
    write_vdc(&mut vdc, VdcRegister::BackgroundScrollY, 7);
    let display = next_active(&mut vdc);
    let vce = HuC6260::new();
    let mut frame = PceActiveOnlyVideoFrame::new();

    frame
        .render_active_line(
            &mut vdc,
            &vce,
            display,
            display.display_line(),
            vce.pixel_clock(),
        )
        .unwrap();

    let background = frame.row_metadata(0).unwrap().background().unwrap();
    assert_eq!(background.scroll_x(), 19);
    assert_eq!(background.virtual_y(), 7);
    assert_eq!(background.first_bat_word(), 2);
}

#[test]
fn each_active_row_records_the_current_vce_pixel_clock() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 2, 1, 0);
    let mut vce = HuC6260::new();
    let mut frame = PceActiveOnlyVideoFrame::new();

    for (line, (control, clock)) in [
        (0, VcePixelClock::DivideByFour),
        (1, VcePixelClock::DivideByThree),
        (2, VcePixelClock::DivideByTwo),
    ]
    .into_iter()
    .enumerate()
    {
        vce.write_port(vce_port(0), control);
        let display = next_active(&mut vdc);
        assert_eq!(usize::from(display.display_line()), line);
        frame
            .render_active_line(&mut vdc, &vce, display, display.display_line(), clock)
            .unwrap();
        assert_eq!(frame.row_metadata(line).unwrap().pixel_clock(), Some(clock));
    }
}

#[test]
fn pixel_clock_is_boundary_owned_while_palette_lookup_uses_the_current_vce() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0);
    let mut vce = HuC6260::new();
    vce.write_port(vce_port(0), 1);
    let captured_clock = vce.pixel_clock();
    let display = next_active(&mut vdc);
    vce.write_port(vce_port(0), 2);
    write_color(&mut vce, 0, 0x01C0);
    let mut frame = PceActiveOnlyVideoFrame::new();

    frame
        .render_active_line(
            &mut vdc,
            &vce,
            display,
            display.display_line(),
            captured_clock,
        )
        .unwrap();

    assert_eq!(vce.pixel_clock(), VcePixelClock::DivideByTwo);
    assert_eq!(
        frame.row_metadata(0).unwrap().pixel_clock(),
        Some(VcePixelClock::DivideByThree)
    );
    assert_eq!(pixel(&frame, 0, 0), [0, 255, 0, 255]);
}

#[test]
fn full_width_render_latches_off_crop_collision_and_overflow() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0x0043);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 63);
    load_satb(&mut vdc, &collision_and_overflow_satb(2));
    vdc.vram_mut()[0x40] = 0x0080;
    let display = next_active(&mut vdc);

    let mut cropped = vec![None; 256];
    let cropped_events = rendered_sprite_events(
        vdc.render_sprite_scanline(&display.sprites(), 0, &mut cropped)
            .unwrap(),
    );
    assert!(!cropped_events.collision_within_output());
    assert!(cropped_events.overflow());

    let mut frame = PceActiveOnlyVideoFrame::new();
    frame
        .render_active_line(
            &mut vdc,
            &HuC6260::new(),
            display,
            display.display_line(),
            VcePixelClock::DivideByFour,
        )
        .unwrap();
    assert!(vdc.status().contains(VdcStatus::SPRITE_COLLISION));
    assert!(vdc.status().contains(VdcStatus::SPRITE_OVERFLOW));
}

#[test]
fn upper_sprite_patterns_mirror_and_commit_frame_metadata_and_status() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0x0043);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 63);
    load_satb(&mut vdc, &collision_and_overflow_satb(0x0400));
    vdc.vram_mut()[0] = 0x8000;
    let display = next_active(&mut vdc);
    let mut frame = PceActiveOnlyVideoFrame::new();

    assert_eq!(
        frame.render_active_line(
            &mut vdc,
            &HuC6260::new(),
            display,
            display.display_line(),
            VcePixelClock::DivideByFour,
        ),
        Ok(())
    );
    assert_eq!(frame.row_metadata(0).unwrap().active_width(), 512);
    assert!(vdc.status().contains(VdcStatus::SPRITE_COLLISION));
    assert!(vdc.status().contains(VdcStatus::SPRITE_OVERFLOW));
}

#[test]
fn sprite_interrupt_enables_are_captured_at_the_active_boundary() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0x0040);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 63);
    load_satb(&mut vdc, &collision_and_overflow_satb(2));
    vdc.vram_mut()[0x40] = 0x0080;
    let display = next_active(&mut vdc);
    write_vdc(&mut vdc, VdcRegister::Control, 0x0073);

    PceActiveOnlyVideoFrame::new()
        .render_active_line(
            &mut vdc,
            &HuC6260::new(),
            display,
            display.display_line(),
            VcePixelClock::DivideByFour,
        )
        .unwrap();

    assert!(
        !vdc.status()
            .intersects(VdcStatus::SPRITE_COLLISION | VdcStatus::SPRITE_OVERFLOW)
    );
}

#[test]
fn boundary_enabled_sprite_events_latch_after_live_control_is_disabled() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0x0043);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 63);
    load_satb(&mut vdc, &collision_and_overflow_satb(2));
    vdc.vram_mut()[0x40] = 0x0080;
    let display = next_active(&mut vdc);
    write_vdc(&mut vdc, VdcRegister::Control, 0x0070);

    PceActiveOnlyVideoFrame::new()
        .render_active_line(
            &mut vdc,
            &HuC6260::new(),
            display,
            display.display_line(),
            VcePixelClock::DivideByFour,
        )
        .unwrap();

    assert!(vdc.status().contains(VdcStatus::SPRITE_COLLISION));
    assert!(vdc.status().contains(VdcStatus::SPRITE_OVERFLOW));
}

#[test]
fn upper_background_patterns_mirror_into_the_committed_row() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 1, 0x0080);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 0);
    vdc.vram_mut()[0] = 0x0FFF;
    vdc.vram_mut()[0x7FF0] = 0x0080;
    let display = next_active(&mut vdc);
    let mut vce = HuC6260::new();
    write_color(&mut vce, 1, 0x0038);
    let mut frame = PceActiveOnlyVideoFrame::new();

    assert_eq!(
        frame.render_active_line(
            &mut vdc,
            &vce,
            display,
            display.display_line(),
            VcePixelClock::DivideByFour,
        ),
        Ok(())
    );
    assert_eq!(pixel(&frame, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame.row_metadata(0).unwrap().active_width(), 8);
}

#[test]
fn zero_vcr_mwr_latch_flows_into_the_visible_pipeline() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0, 0, 0x0080);
    let first = next_active(&mut vdc);
    assert_eq!(first.background().width_tiles(), 32);
    write_vdc(&mut vdc, VdcRegister::MemoryWidth, 0x0010);
    assert!(
        vdc.advance_scanline_boundary()
            .unwrap()
            .vertical_blank_started()
    );
    let display = next_active(&mut vdc);
    assert_eq!(display.background().width_tiles(), 64);

    vdc.vram_mut()[0] = 1;
    vdc.vram_mut()[0x10] = 0x0080;
    let mut vce = HuC6260::new();
    write_color(&mut vce, 1, 0x01C0);
    let mut frame = PceActiveOnlyVideoFrame::new();
    frame
        .render_active_line(
            &mut vdc,
            &vce,
            display,
            display.display_line(),
            vce.pixel_clock(),
        )
        .unwrap();
    assert_eq!(pixel(&frame, 0, 0), [0, 255, 0, 255]);
}

#[test]
fn maximum_active_line_and_width_fit_the_bounded_frame() {
    let mut vdc = HuC6270::new();
    configure_frame(&mut vdc, 0x01FF, 0, 0);
    write_vdc(&mut vdc, VdcRegister::HorizontalDisplay, 0x007F);
    let mut display = next_active(&mut vdc);
    for expected in 1..PCE_ACTIVE_FRAME_HEIGHT {
        display = next_active(&mut vdc);
        assert_eq!(usize::from(display.display_line()), expected);
    }
    assert_eq!(usize::from(display.source_width()), PCE_ACTIVE_FRAME_WIDTH);
    let mut vce = HuC6260::new();
    write_color(&mut vce, 0, 0x0007);
    let mut frame = PceActiveOnlyVideoFrame::new();

    frame
        .render_active_line(
            &mut vdc,
            &vce,
            display,
            display.display_line(),
            vce.pixel_clock(),
        )
        .unwrap();

    let metadata = frame.row_metadata(PCE_ACTIVE_FRAME_HEIGHT - 1).unwrap();
    assert_eq!(usize::from(metadata.active_width()), PCE_ACTIVE_FRAME_WIDTH);
    assert_eq!(
        pixel(
            &frame,
            PCE_ACTIVE_FRAME_WIDTH - 1,
            PCE_ACTIVE_FRAME_HEIGHT - 1,
        ),
        [0, 0, 255, 255]
    );
    assert_eq!(frame.row_metadata(PCE_ACTIVE_FRAME_HEIGHT), None);
}
