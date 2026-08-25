use super::cpu::{LineLevel, VdcPort};
use super::{
    BackgroundColorMode, DETERMINISTIC_VDC_RESET_LATCHED_MEMORY_WIDTH, HuC6270,
    PROVISIONAL_EXTERNAL_VCE_VERTICAL_PROFILE_LATCHED_AT_VSYNC,
    PROVISIONAL_EXTERNAL_VDW_CAPPED_TO_VCE_FRAME,
    PROVISIONAL_EXTERNAL_VSYNC_MARKER_RESTARTS_VERTICAL_PROGRESSION,
    PROVISIONAL_STOCK_MACHINE_VCE_BOUNDARIES_DRIVE_VDC_HORIZONTAL_AND_VERTICAL_SYNC,
    SpriteColorMode, VDC_SATB_WORDS, VceFrameLength, VdcActiveDisplayLine, VdcDmaChannel,
    VdcDmaProgress, VdcExternalVceScanline, VdcRegister, VdcScanlineAdvanceError,
    VdcScanlineBoundary, VdcScanlineTransition, VdcStatus, VdcVerticalPhase,
};

fn write_register(vdc: &mut HuC6270, register: VdcRegister, value: u16) {
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

fn use_internal_sync(vdc: &mut HuC6270, interrupt_enables: u8) {
    write_register(
        vdc,
        VdcRegister::Control,
        0x0030 | u16::from(interrupt_enables),
    );
}

fn compact_frame(vdc: &mut HuC6270, display_lines_minus_one: u16, display_end: u16) {
    write_register(vdc, VdcRegister::VerticalSync, 0);
    write_register(vdc, VdcRegister::VerticalDisplay, display_lines_minus_one);
    write_register(vdc, VdcRegister::VerticalDisplayEnd, display_end);
}

fn external_frame(vdc: &mut HuC6270, frame_length: VceFrameLength, display_end: u16) {
    let active_lines = frame_length.scanlines() - 3 - display_end;
    write_register(vdc, VdcRegister::Control, 0);
    write_register(vdc, VdcRegister::VerticalSync, 0);
    write_register(vdc, VdcRegister::VerticalDisplay, active_lines - 1);
    write_register(vdc, VdcRegister::VerticalDisplayEnd, display_end);
}

fn external_input(
    boundary_count: u8,
    vsync_started: bool,
    frame_length: VceFrameLength,
) -> VdcExternalVceScanline {
    VdcExternalVceScanline::new(boundary_count, vsync_started, frame_length)
}

fn prepare_scheduler_dma(vdc: &mut HuC6270) {
    write_register(vdc, VdcRegister::RasterCounter, 64);
    write_register(vdc, VdcRegister::DmaSource, 0x0100);
    write_register(vdc, VdcRegister::DmaDestination, 0x0200);
    write_register(vdc, VdcRegister::DmaLength, 3);
    assert!(vdc.activate_pending_vram_dma());
    write_register(vdc, VdcRegister::SatbSource, 0x0300);
}

fn next_active_display(vdc: &mut HuC6270) -> VdcActiveDisplayLine {
    loop {
        if let Some(display) = vdc.advance_scanline_boundary().unwrap().active_display() {
            return display;
        }
    }
}

fn next_vertical_blank(vdc: &mut HuC6270) -> VdcScanlineBoundary {
    loop {
        let boundary = vdc.advance_scanline_boundary().unwrap();
        if boundary.vertical_blank_started() {
            return boundary;
        }
    }
}

#[test]
fn phases_use_the_documented_durations_and_zero_display_end_is_immediate() {
    let mut vdc = HuC6270::new();
    use_internal_sync(&mut vdc, 0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0x0101);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 1);
    write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 0);

    let boundaries = std::array::from_fn::<_, 8, _>(|_| vdc.advance_scanline_boundary().unwrap());
    assert_eq!(
        boundaries.map(|boundary| (
            boundary.phase(),
            boundary.phase_line(),
            boundary.frame_line(),
            boundary.active_display_line(),
        )),
        [
            (VdcVerticalPhase::VerticalSync, 0, 0, None),
            (VdcVerticalPhase::VerticalSync, 1, 1, None),
            (VdcVerticalPhase::DisplayStart, 0, 2, None),
            (VdcVerticalPhase::DisplayStart, 1, 3, None),
            (VdcVerticalPhase::DisplayStart, 2, 4, None),
            (VdcVerticalPhase::ActiveDisplay, 0, 5, Some(0)),
            (VdcVerticalPhase::ActiveDisplay, 1, 6, Some(1)),
            (VdcVerticalPhase::VerticalSync, 0, 0, None),
        ]
    );
    assert!(boundaries[0].frame_started());
    assert_eq!(
        boundaries.map(|boundary| boundary.entered_phase()),
        [
            Some(VdcVerticalPhase::VerticalSync),
            None,
            Some(VdcVerticalPhase::DisplayStart),
            None,
            None,
            Some(VdcVerticalPhase::ActiveDisplay),
            None,
            Some(VdcVerticalPhase::VerticalSync),
        ]
    );
    assert!(boundaries[7].frame_started());
    assert!(boundaries[7].vertical_blank_started());
    assert_eq!(
        boundaries[7].transitions(),
        [
            Some(VdcScanlineTransition::VerticalBlankStarted),
            Some(VdcScanlineTransition::PhaseStarted(
                VdcVerticalPhase::VerticalSync,
            )),
            Some(VdcScanlineTransition::FrameStarted),
        ]
    );
    assert_eq!(boundaries[5].raster_counter(), 64);
    assert_eq!(boundaries[6].raster_counter(), 65);
    assert_eq!(boundaries[7].raster_counter(), 66);
}

#[test]
fn vertical_display_and_end_durations_are_sampled_at_their_period_entries() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 1, 2);
    use_internal_sync(&mut vdc, 0);

    for _ in 0..3 {
        vdc.advance_scanline_boundary().unwrap();
    }
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 4);
    assert_eq!(
        vdc.advance_scanline_boundary()
            .unwrap()
            .active_display_line(),
        Some(0)
    );
    assert_eq!(
        vdc.advance_scanline_boundary()
            .unwrap()
            .active_display_line(),
        Some(1)
    );

    let display_end_zero = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(display_end_zero.phase(), VdcVerticalPhase::DisplayEnd);
    assert_eq!(display_end_zero.phase_line(), 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 0);
    let display_end_one = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(display_end_one.phase(), VdcVerticalPhase::DisplayEnd);
    assert_eq!(display_end_one.phase_line(), 1);

    for _ in 0..3 {
        vdc.advance_scanline_boundary().unwrap();
    }
    for expected_line in 0..5 {
        assert_eq!(
            vdc.advance_scanline_boundary()
                .unwrap()
                .active_display_line(),
            Some(expected_line)
        );
    }
    let next_frame = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(next_frame.phase(), VdcVerticalPhase::VerticalSync);
    assert!(next_frame.vertical_blank_started());
}

#[test]
fn raster_compare_continues_into_non_active_lines() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 0, 2);
    write_register(&mut vdc, VdcRegister::RasterCounter, 66);
    use_internal_sync(&mut vdc, 0x04);

    let active = next_active_display(&mut vdc);
    assert_eq!(active.display_line(), 0);
    assert!(!vdc.status().contains(VdcStatus::RASTER_MATCH));

    let display_end = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(display_end.phase(), VdcVerticalPhase::DisplayEnd);
    assert_eq!(display_end.raster_counter(), 65);
    assert!(display_end.raster_match());
    assert!(vdc.status().contains(VdcStatus::RASTER_MATCH));
}

#[test]
fn raster_compare_is_tested_at_hdw_end_before_the_next_scanline() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 1, 1);
    write_register(&mut vdc, VdcRegister::RasterCounter, 64);
    use_internal_sync(&mut vdc, 0);

    assert!(!vdc.advance_scanline_boundary().unwrap().raster_match());
    assert!(!vdc.advance_scanline_boundary().unwrap().raster_match());
    let before_active = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(before_active.phase(), VdcVerticalPhase::DisplayStart);
    assert!(before_active.raster_match());

    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 1, 1);
    write_register(&mut vdc, VdcRegister::RasterCounter, 65);
    use_internal_sync(&mut vdc, 0);

    for _ in 0..3 {
        assert!(!vdc.advance_scanline_boundary().unwrap().raster_match());
    }
    let first_active = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(first_active.active_display_line(), Some(0));
    assert!(first_active.raster_match());
}

#[test]
fn memory_width_latches_at_vertical_blank_for_the_next_frame() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 0, 1);
    use_internal_sync(&mut vdc, 0);
    write_register(&mut vdc, VdcRegister::MemoryWidth, 0x0010);

    let initial = next_active_display(&mut vdc);
    assert_eq!(initial.background().width_tiles(), 32);
    assert_eq!(initial.background().height_tiles(), 32);
    assert_eq!(initial.sprites().color_mode(), SpriteColorMode::Full);

    next_vertical_blank(&mut vdc);
    write_register(&mut vdc, VdcRegister::MemoryWidth, 0x0064);
    let first_latched = next_active_display(&mut vdc);
    assert_eq!(first_latched.background().width_tiles(), 64);
    assert_eq!(first_latched.background().height_tiles(), 32);
    assert_eq!(
        first_latched.background().color_mode(),
        BackgroundColorMode::Full
    );
    assert_eq!(first_latched.sprites().color_mode(), SpriteColorMode::Full);

    next_vertical_blank(&mut vdc);
    let second_latched = next_active_display(&mut vdc);
    assert_eq!(second_latched.background().width_tiles(), 128);
    assert_eq!(second_latched.background().height_tiles(), 64);
    assert_eq!(
        second_latched.sprites().color_mode(),
        SpriteColorMode::PlanePair
    );
}

#[test]
fn zero_display_end_latches_memory_width_before_the_coalesced_frame_start() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 0, 0);
    use_internal_sync(&mut vdc, 0);

    assert_eq!(next_active_display(&mut vdc).display_line(), 0);
    write_register(&mut vdc, VdcRegister::MemoryWidth, 0x0064);
    let coalesced = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(
        coalesced.transitions(),
        [
            Some(VdcScanlineTransition::VerticalBlankStarted),
            Some(VdcScanlineTransition::PhaseStarted(
                VdcVerticalPhase::VerticalSync,
            )),
            Some(VdcScanlineTransition::FrameStarted),
        ]
    );

    let next_frame = next_active_display(&mut vdc);
    assert_eq!(next_frame.background().width_tiles(), 128);
    assert_eq!(next_frame.background().height_tiles(), 64);
    assert_eq!(
        next_frame.sprites().color_mode(),
        SpriteColorMode::PlanePair
    );
}

#[test]
fn active_lines_sample_scroll_and_layer_enables_at_each_boundary() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 1, 1);
    use_internal_sync(&mut vdc, 0xC0);
    write_register(&mut vdc, VdcRegister::BackgroundScrollX, 5);
    write_register(&mut vdc, VdcRegister::BackgroundScrollY, 7);

    let first = next_active_display(&mut vdc);
    assert_eq!(first.display_line(), 0);
    assert!(first.background().enabled());
    assert!(first.sprites().enabled());
    assert_eq!(first.background().scroll_x(), 5);
    assert_eq!(first.background().scroll_y(), 7);

    use_internal_sync(&mut vdc, 0x40);
    write_register(&mut vdc, VdcRegister::BackgroundScrollX, 9);
    write_register(&mut vdc, VdcRegister::BackgroundScrollY, 11);
    let second = vdc
        .advance_scanline_boundary()
        .unwrap()
        .active_display()
        .unwrap();
    assert_eq!(second.display_line(), 1);
    assert!(!second.background().enabled());
    assert!(second.sprites().enabled());
    assert_eq!(second.background().scroll_x(), 9);
    assert_eq!(second.background().scroll_y(), 11);
}

#[test]
fn active_background_y_reloads_on_every_write_and_wraps_at_nine_bits() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 5, 1);
    use_internal_sync(&mut vdc, 0x80);
    write_register(&mut vdc, VdcRegister::BackgroundScrollY, 0x01FE);

    let first = next_active_display(&mut vdc);
    assert_eq!(first.display_line(), 0);
    assert_eq!(first.background().scroll_y(), 0x01FE);

    let second = vdc
        .advance_scanline_boundary()
        .unwrap()
        .active_display()
        .unwrap();
    assert_eq!(second.display_line(), 1);
    assert_eq!(
        (second.background().scroll_y() + usize::from(second.display_line())) & 0x01FF,
        0x01FF
    );

    let third = vdc
        .advance_scanline_boundary()
        .unwrap()
        .active_display()
        .unwrap();
    assert_eq!(third.display_line(), 2);
    assert_eq!(
        (third.background().scroll_y() + usize::from(third.display_line())) & 0x01FF,
        0
    );

    write_register(&mut vdc, VdcRegister::BackgroundScrollY, 0x01FE);
    let reloaded = vdc
        .advance_scanline_boundary()
        .unwrap()
        .active_display()
        .unwrap();
    assert_eq!(reloaded.display_line(), 3);
    assert_eq!(
        (reloaded.background().scroll_y() + usize::from(reloaded.display_line())) & 0x01FF,
        0x01FF
    );

    let continued = vdc
        .advance_scanline_boundary()
        .unwrap()
        .active_display()
        .unwrap();
    assert_eq!(continued.display_line(), 4);
    assert_eq!(
        (continued.background().scroll_y() + usize::from(continued.display_line())) & 0x01FF,
        0
    );
}

#[test]
fn background_x_write_is_sampled_exactly_on_the_next_active_line() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 2, 1);
    use_internal_sync(&mut vdc, 0x80);
    write_register(&mut vdc, VdcRegister::BackgroundScrollX, 7);

    assert_eq!(next_active_display(&mut vdc).background().scroll_x(), 7);
    write_register(&mut vdc, VdcRegister::BackgroundScrollX, 0x03FF);
    assert_eq!(
        vdc.advance_scanline_boundary()
            .unwrap()
            .active_display()
            .unwrap()
            .background()
            .scroll_x(),
        0x03FF
    );
}

#[test]
fn raster_and_vertical_blank_status_latch_only_when_enabled_at_the_event() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 0, 1);
    write_register(&mut vdc, VdcRegister::RasterCounter, 64);
    use_internal_sync(&mut vdc, 0);

    for _ in 0..2 {
        vdc.advance_scanline_boundary().unwrap();
    }
    let raster = vdc.advance_scanline_boundary().unwrap();
    assert!(raster.raster_match());
    assert!(!vdc.status().contains(VdcStatus::RASTER_MATCH));

    let active = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(active.active_display_line(), Some(0));
    let blank = vdc.advance_scanline_boundary().unwrap();
    assert!(blank.vertical_blank_started());
    assert_eq!(blank.raster_counter(), 65);
    assert!(!vdc.status().contains(VdcStatus::VERTICAL_BLANK));
    assert_eq!(vdc.irq_level(), LineLevel::High);

    use_internal_sync(&mut vdc, 0x0C);
    assert_eq!(vdc.irq_level(), LineLevel::High);
    for _ in 0..3 {
        vdc.advance_scanline_boundary().unwrap();
    }
    assert!(vdc.status().contains(VdcStatus::RASTER_MATCH));
    vdc.advance_scanline_boundary().unwrap();
    let blank = vdc.advance_scanline_boundary().unwrap();
    assert!(blank.vertical_blank_started());
    assert!(vdc.status().contains(VdcStatus::VERTICAL_BLANK));
    assert_eq!(vdc.irq_level(), LineLevel::Low);
}

#[test]
fn vertical_period_durations_are_sampled_when_each_period_begins() {
    let mut vdc = HuC6270::new();
    use_internal_sync(&mut vdc, 0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0x0001);

    assert_eq!(vdc.advance_scanline_boundary().unwrap().phase_line(), 0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0x0300);
    let retained_sync = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(retained_sync.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(retained_sync.phase_line(), 1);

    let display_start = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(display_start.phase(), VdcVerticalPhase::DisplayStart);
    assert_eq!(display_start.phase_line(), 0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0);
    for expected_line in 1..5 {
        let boundary = vdc.advance_scanline_boundary().unwrap();
        assert_eq!(boundary.phase(), VdcVerticalPhase::DisplayStart);
        assert_eq!(boundary.phase_line(), expected_line);
    }
    assert_eq!(
        vdc.advance_scanline_boundary()
            .unwrap()
            .active_display_line(),
        Some(0)
    );
}

#[test]
fn vertical_blank_starts_pending_and_repeating_satb_dma() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 0, 1);
    use_internal_sync(&mut vdc, 0);
    write_register(&mut vdc, VdcRegister::DmaControl, 0x0010);
    write_register(&mut vdc, VdcRegister::SatbSource, 0x0100);

    for _ in 0..4 {
        vdc.advance_scanline_boundary().unwrap();
    }
    let first_blank = vdc.advance_scanline_boundary().unwrap();
    assert!(first_blank.vertical_blank_started());
    assert!(first_blank.satb_dma_started());
    assert!(vdc.pending_satb_dma().is_none());
    assert!(vdc.active_satb_dma().is_some());

    for index in 0..VDC_SATB_WORDS {
        let progress = vdc.service_dma_slot(VdcDmaChannel::Satb).unwrap();
        if index + 1 == VDC_SATB_WORDS {
            assert_eq!(progress, VdcDmaProgress::Complete);
        }
    }
    for _ in 0..4 {
        vdc.advance_scanline_boundary().unwrap();
    }
    let repeated_blank = vdc.advance_scanline_boundary().unwrap();
    assert!(repeated_blank.vertical_blank_started());
    assert!(repeated_blank.satb_dma_started());
    assert!(vdc.active_satb_dma().is_some());
}

#[test]
fn entering_active_display_aborts_only_an_active_vram_dma() {
    let mut vdc = HuC6270::new();
    compact_frame(&mut vdc, 0, 1);
    use_internal_sync(&mut vdc, 0);
    write_register(&mut vdc, VdcRegister::DmaSource, 0x0100);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x0200);
    write_register(&mut vdc, VdcRegister::DmaLength, 3);
    assert!(vdc.activate_pending_vram_dma());

    assert!(!vdc.advance_scanline_boundary().unwrap().vram_dma_aborted());
    assert!(!vdc.advance_scanline_boundary().unwrap().vram_dma_aborted());
    assert!(!vdc.advance_scanline_boundary().unwrap().vram_dma_aborted());
    let active = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(active.active_display_line(), Some(0));
    assert!(active.vram_dma_aborted());
    assert!(vdc.active_vram_dma().is_none());
    assert!(!vdc.status().contains(VdcStatus::VRAM_DMA_COMPLETE));
}

#[test]
fn cr_sync_outputs_do_not_select_input_timing() {
    for (control, horizontal, vertical) in [
        (0x0000, false, false),
        (0x0010, true, false),
        (0x0020, true, true),
        (0x0030, true, true),
    ] {
        let mut vdc = HuC6270::new();
        write_register(&mut vdc, VdcRegister::Control, control);
        assert_eq!(vdc.sync_output().horizontal(), horizontal);
        assert_eq!(vdc.sync_output().vertical(), vertical);
        let first = vdc.advance_scanline_boundary().unwrap();
        assert_eq!(first.phase(), VdcVerticalPhase::VerticalSync);
        assert_eq!(first.frame_line(), 0);
        assert!(first.frame_started());
    }
}

#[test]
fn reset_restores_the_deterministic_first_boundary() {
    let mut vdc = HuC6270::new();
    use_internal_sync(&mut vdc, 0);
    for _ in 0..4 {
        vdc.advance_scanline_boundary().unwrap();
    }
    vdc.reset();
    use_internal_sync(&mut vdc, 0);

    let first = vdc.advance_scanline_boundary().unwrap();
    assert_eq!(first.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(first.phase_line(), 0);
    assert_eq!(first.frame_line(), 0);
    assert_eq!(first.raster_counter(), 0);
    assert!(first.frame_started());

    assert_eq!(DETERMINISTIC_VDC_RESET_LATCHED_MEMORY_WIDTH, 0);
    write_register(&mut vdc, VdcRegister::MemoryWidth, 0x0064);
    let active = next_active_display(&mut vdc);
    assert_eq!(active.background().width_tiles(), 32);
    assert_eq!(active.background().height_tiles(), 32);
    assert_eq!(active.sprites().color_mode(), SpriteColorMode::Full);
}

#[test]
fn external_vce_sync_accepts_complete_262_and_263_line_profiles() {
    for frame_length in [VceFrameLength::Lines262, VceFrameLength::Lines263] {
        let mut vdc = HuC6270::new();
        external_frame(&mut vdc, frame_length, 1);
        let lines = frame_length.scanlines();
        let mut vertical_blank_count = 0;

        for line in 0..lines {
            let boundary = vdc
                .advance_external_vce_scanline(external_input(1, line == 0, frame_length))
                .unwrap();
            assert_eq!(boundary.frame_started(), line == 0);
            assert_eq!(boundary.frame_line(), line);
            vertical_blank_count += u8::from(boundary.vertical_blank_started());
        }
        assert_eq!(vertical_blank_count, 1);

        let next_frame = vdc
            .advance_external_vce_scanline(external_input(1, true, frame_length))
            .unwrap();
        assert!(next_frame.frame_started());
        assert_eq!(next_frame.frame_line(), 0);
    }
}

#[test]
fn provisional_machine_policy_matches_ex00_and_ex11_vertical_results() {
    const { assert!(PROVISIONAL_STOCK_MACHINE_VCE_BOUNDARIES_DRIVE_VDC_HORIZONTAL_AND_VERTICAL_SYNC) };
    let frame_length = VceFrameLength::Lines262;
    let mut ex00 = HuC6270::new();
    let mut ex11 = HuC6270::new();
    external_frame(&mut ex00, frame_length, 1);
    external_frame(&mut ex11, frame_length, 1);
    write_register(&mut ex00, VdcRegister::Control, 0x000C);
    write_register(&mut ex11, VdcRegister::Control, 0x003C);
    prepare_scheduler_dma(&mut ex00);
    prepare_scheduler_dma(&mut ex11);

    for line in 0..=frame_length.scanlines() {
        let input = external_input(1, line % frame_length.scanlines() == 0, frame_length);
        let ex00_boundary = ex00.advance_machine_vce_scanline(input).unwrap();
        let ex11_boundary = ex11.advance_machine_vce_scanline(input).unwrap();
        assert_eq!(ex00_boundary, ex11_boundary);
        assert_eq!(ex00.pending_vram_dma(), ex11.pending_vram_dma());
        assert_eq!(ex00.active_vram_dma(), ex11.active_vram_dma());
        assert_eq!(ex00.pending_satb_dma(), ex11.pending_satb_dma());
        assert_eq!(ex00.active_satb_dma(), ex11.active_satb_dma());
        assert_eq!(ex00.status(), ex11.status());
        assert_eq!(ex00.irq_level(), ex11.irq_level());
    }
}

#[test]
fn provisional_machine_policy_preserves_profile_across_sync_output_changes() {
    let frame_length = VceFrameLength::Lines262;
    let mut reference = HuC6270::new();
    let mut toggled = HuC6270::new();
    external_frame(&mut reference, frame_length, 4);
    external_frame(&mut toggled, frame_length, 4);

    for line in 0..frame_length.scanlines() {
        if line == 40 {
            write_register(&mut toggled, VdcRegister::Control, 0x0030);
        } else if line == 180 {
            write_register(&mut toggled, VdcRegister::Control, 0x0000);
        }
        let input = external_input(1, line == 0, frame_length);
        assert_eq!(
            reference.advance_machine_vce_scanline(input).unwrap(),
            toggled.advance_machine_vce_scanline(input).unwrap()
        );
    }

    write_register(&mut toggled, VdcRegister::Control, 0x0030);
    let marker = toggled
        .advance_machine_vce_scanline(external_input(1, true, frame_length))
        .unwrap();
    assert!(marker.frame_started());
    assert_eq!(marker.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(marker.frame_line(), 0);
}

#[test]
fn provisional_machine_policy_accepts_every_sync_output() {
    let frame_length = VceFrameLength::Lines262;
    for control in [0x0000, 0x0010, 0x0020, 0x0030] {
        let mut vdc = HuC6270::new();
        external_frame(&mut vdc, frame_length, 1);
        let first = vdc
            .advance_machine_vce_scanline(external_input(1, true, frame_length))
            .unwrap();
        assert!(first.frame_started());
        write_register(&mut vdc, VdcRegister::Control, control);
        let continued = vdc
            .advance_machine_vce_scanline(external_input(1, false, frame_length))
            .unwrap();
        assert_eq!(continued.phase(), VdcVerticalPhase::DisplayStart);
        assert_eq!(continued.phase_line(), 0);
        assert_eq!(continued.frame_line(), 1);
    }
}

#[test]
fn provisional_ex11_marker_preserves_overlong_and_zero_vcr_profiles() {
    for (frame_length, display_end_lines) in
        [(VceFrameLength::Lines262, 2), (VceFrameLength::Lines263, 3)]
    {
        let mut vdc = HuC6270::new();
        write_register(&mut vdc, VdcRegister::Control, 0x0038);
        write_register(&mut vdc, VdcRegister::VerticalSync, 0x0F02);
        write_register(&mut vdc, VdcRegister::VerticalDisplay, 0x00EF);
        write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 0x0004);

        let mut active_lines = 0;
        for line in 0..frame_length.scanlines() {
            let boundary = vdc
                .advance_machine_vce_scanline(external_input(1, line == 0, frame_length))
                .unwrap();
            active_lines += u16::from(boundary.active_display().is_some());
            if line >= 260 {
                assert_eq!(boundary.phase(), VdcVerticalPhase::DisplayEnd);
                assert_eq!(boundary.phase_line(), line - 260);
            }
        }
        assert_eq!(active_lines, 240);
        assert_eq!(display_end_lines, frame_length.scanlines() - 260);
        let restarted = vdc
            .advance_machine_vce_scanline(external_input(1, true, frame_length))
            .unwrap();
        assert_eq!(restarted.phase(), VdcVerticalPhase::VerticalSync);
        assert_eq!(restarted.frame_line(), 0);
    }

    let frame_length = VceFrameLength::Lines262;
    let mut vdc = HuC6270::new();
    external_frame(&mut vdc, frame_length, 0);
    write_register(&mut vdc, VdcRegister::Control, 0x0038);
    let mut vblank = None;
    for line in 0..frame_length.scanlines() {
        let boundary = vdc
            .advance_machine_vce_scanline(external_input(1, line == 0, frame_length))
            .unwrap();
        if boundary.vertical_blank_started() {
            vblank = Some(boundary);
        }
    }
    assert_eq!(
        vblank.unwrap().transitions(),
        [
            Some(VdcScanlineTransition::VerticalBlankStarted),
            Some(VdcScanlineTransition::PhaseStarted(
                VdcVerticalPhase::VerticalSync,
            )),
            Some(VdcScanlineTransition::FrameStarted),
        ]
    );
}

#[test]
fn external_and_internal_sync_emit_the_same_vertical_events() {
    let frame_length = VceFrameLength::Lines262;
    let mut external = HuC6270::new();
    let mut internal = HuC6270::new();
    external_frame(&mut external, frame_length, 1);
    external_frame(&mut internal, frame_length, 1);
    write_register(&mut external, VdcRegister::RasterCounter, 64);
    write_register(&mut internal, VdcRegister::RasterCounter, 64);
    write_register(&mut external, VdcRegister::Control, 0x00CC);
    write_register(&mut internal, VdcRegister::Control, 0x00FC);

    for line in 0..=frame_length.scanlines() {
        let external_boundary = external
            .advance_external_vce_scanline(external_input(
                1,
                line % frame_length.scanlines() == 0,
                frame_length,
            ))
            .unwrap();
        assert_eq!(
            external_boundary,
            internal.advance_scanline_boundary().unwrap()
        );
        assert_eq!(external.status(), internal.status());
    }
}

#[test]
fn external_zero_display_end_preserves_coalesced_transition_order() {
    let frame_length = VceFrameLength::Lines262;
    let mut vdc = HuC6270::new();
    external_frame(&mut vdc, frame_length, 0);

    let mut coalesced = None;
    for line in 0..frame_length.scanlines() {
        let boundary = vdc
            .advance_external_vce_scanline(external_input(1, line == 0, frame_length))
            .unwrap();
        if boundary.vertical_blank_started() {
            assert_eq!(line, frame_length.scanlines() - 1);
            coalesced = Some(boundary);
        }
    }
    let coalesced = coalesced.unwrap();
    assert_eq!(
        coalesced.transitions(),
        [
            Some(VdcScanlineTransition::VerticalBlankStarted),
            Some(VdcScanlineTransition::PhaseStarted(
                VdcVerticalPhase::VerticalSync,
            )),
            Some(VdcScanlineTransition::FrameStarted),
        ]
    );

    let marker = vdc
        .advance_external_vce_scanline(external_input(1, true, frame_length))
        .unwrap();
    assert_eq!(
        marker.transitions(),
        [
            Some(VdcScanlineTransition::PhaseStarted(
                VdcVerticalPhase::VerticalSync,
            )),
            Some(VdcScanlineTransition::FrameStarted),
            None,
        ]
    );
}

#[test]
fn external_vsync_marker_preserves_the_uncapped_real_264_line_profile() {
    const { assert!(PROVISIONAL_EXTERNAL_VSYNC_MARKER_RESTARTS_VERTICAL_PROGRESSION) };
    for (frame_length, display_end_lines) in
        [(VceFrameLength::Lines262, 2), (VceFrameLength::Lines263, 3)]
    {
        let mut vdc = HuC6270::new();
        write_register(&mut vdc, VdcRegister::Control, 0x0008);
        write_register(&mut vdc, VdcRegister::VerticalSync, 0x0F02);
        write_register(&mut vdc, VdcRegister::VerticalDisplay, 0x00EF);
        write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 0x0004);

        let first = vdc
            .advance_external_vce_scanline(external_input(1, true, frame_length))
            .unwrap();
        assert_eq!(first.phase(), VdcVerticalPhase::VerticalSync);
        assert_eq!(first.frame_line(), 0);

        let mut active_lines = 0;
        for _ in 1..260 {
            let boundary = vdc
                .advance_external_vce_scanline(external_input(1, false, frame_length))
                .unwrap();
            active_lines += u16::from(boundary.active_display().is_some());
        }
        assert_eq!(active_lines, 240);
        for phase_line in 0..display_end_lines {
            let boundary = vdc
                .advance_external_vce_scanline(external_input(1, false, frame_length))
                .unwrap();
            assert_eq!(boundary.phase(), VdcVerticalPhase::DisplayEnd);
            assert_eq!(boundary.phase_line(), phase_line);
        }

        let restarted = vdc
            .advance_external_vce_scanline(external_input(1, true, frame_length))
            .unwrap();
        assert_eq!(restarted.phase(), VdcVerticalPhase::VerticalSync);
        assert_eq!(restarted.phase_line(), 0);
        assert_eq!(restarted.frame_line(), 0);
        assert!(restarted.frame_started());
    }
}

#[test]
fn external_frame_caps_andre_active_display_before_vblank() {
    const { assert!(PROVISIONAL_EXTERNAL_VDW_CAPPED_TO_VCE_FRAME) };
    for (frame_length, expected_active_lines, vblank_line) in [
        (VceFrameLength::Lines262, 221, 261),
        (VceFrameLength::Lines263, 222, 262),
    ] {
        let mut vdc = HuC6270::new();
        write_register(&mut vdc, VdcRegister::Control, 0x0008);
        write_register(&mut vdc, VdcRegister::VerticalSync, 0x1015);
        write_register(&mut vdc, VdcRegister::VerticalDisplay, 0x00EF);
        write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 0x0004);
        write_register(&mut vdc, VdcRegister::SatbSource, 0x0300);

        let mut active_lines = 0;
        let mut vblank = None;
        for line in 0..frame_length.scanlines() {
            let boundary = vdc
                .advance_external_vce_scanline(external_input(1, line == 0, frame_length))
                .unwrap();
            active_lines += u16::from(boundary.active_display().is_some());
            if boundary.vertical_blank_started() {
                assert!(vblank.replace(boundary).is_none());
            }
        }

        let vblank = vblank.unwrap();
        assert_eq!(active_lines, expected_active_lines);
        assert_eq!(vblank.frame_line(), vblank_line);
        assert_eq!(vblank.phase(), VdcVerticalPhase::DisplayEnd);
        assert_eq!(vblank.phase_line(), 0);
        assert!(vblank.satb_dma_started());
        assert!(vdc.pending_satb_dma().is_none());
        assert!(vdc.active_satb_dma().is_some());
        assert!(vdc.status().contains(VdcStatus::VERTICAL_BLANK));
        assert_eq!(vdc.irq_level(), LineLevel::Low);
    }
}

#[test]
fn external_active_display_cap_is_frozen_until_the_next_marker() {
    let frame_length = VceFrameLength::Lines262;
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::VerticalSync, 0x1015);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 0x00EF);
    write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 0x0004);

    let first = vdc
        .advance_external_vce_scanline(external_input(1, true, frame_length))
        .unwrap();
    assert!(first.frame_started());
    write_register(&mut vdc, VdcRegister::VerticalSync, 0x0F02);

    let mut active_lines = 0;
    for _ in 1..frame_length.scanlines() {
        let boundary = vdc
            .advance_external_vce_scanline(external_input(1, false, frame_length))
            .unwrap();
        active_lines += u16::from(boundary.active_display().is_some());
    }
    assert_eq!(active_lines, 221);

    let mut next_active_lines = 0;
    for line in 0..frame_length.scanlines() {
        let boundary = vdc
            .advance_external_vce_scanline(external_input(1, line == 0, frame_length))
            .unwrap();
        next_active_lines += u16::from(boundary.active_display().is_some());
    }
    assert_eq!(next_active_lines, 240);
}

#[test]
fn external_profile_without_a_vblank_line_is_rejected_transactionally() {
    let frame_length = VceFrameLength::Lines262;
    let mut vdc = HuC6270::new();
    external_frame(&mut vdc, frame_length, 1);
    vdc.advance_external_vce_scanline(external_input(1, true, frame_length))
        .unwrap();
    write_register(&mut vdc, VdcRegister::VerticalSync, 0xFF1F);

    assert_eq!(
        vdc.advance_external_vce_scanline(external_input(1, true, frame_length)),
        Err(VdcScanlineAdvanceError::ExternalVerticalBlankUnavailable {
            frame_lines: 262,
            vertical_sync: 32,
            display_start: 257,
        })
    );
    let continued = vdc
        .advance_external_vce_scanline(external_input(1, false, frame_length))
        .unwrap();
    assert_eq!(continued.phase(), VdcVerticalPhase::DisplayStart);
    assert_eq!(continued.phase_line(), 0);
    assert_eq!(continued.frame_line(), 1);
}

#[test]
fn external_vce_sync_accepts_every_sync_output() {
    let frame_length = VceFrameLength::Lines262;
    for control in [0x0000, 0x0010, 0x0020, 0x0030] {
        let mut vdc = HuC6270::new();
        external_frame(&mut vdc, frame_length, 1);
        write_register(&mut vdc, VdcRegister::Control, control);
        let first = vdc
            .advance_external_vce_scanline(external_input(1, true, frame_length))
            .unwrap();
        assert!(first.frame_started());
        assert_eq!(vdc.status(), VdcStatus::empty());
    }
}

#[test]
fn external_vce_sync_validates_boundary_count_and_initial_marker_transactionally() {
    let frame_length = VceFrameLength::Lines262;
    let mut vdc = HuC6270::new();
    external_frame(&mut vdc, frame_length, 1);

    for count in [0, 2] {
        assert_eq!(
            vdc.advance_external_vce_scanline(external_input(count, true, frame_length)),
            Err(VdcScanlineAdvanceError::InvalidExternalBoundaryCount { count })
        );
    }
    assert_eq!(
        vdc.advance_external_vce_scanline(external_input(1, false, frame_length)),
        Err(VdcScanlineAdvanceError::ExternalProfileNotStarted)
    );

    let first = vdc
        .advance_external_vce_scanline(external_input(1, true, frame_length))
        .unwrap();
    assert!(first.frame_started());
    assert_eq!(first.frame_line(), 0);
}

#[test]
fn external_vertical_profile_freezes_until_the_next_accepted_vsync() {
    const { assert!(PROVISIONAL_EXTERNAL_VCE_VERTICAL_PROFILE_LATCHED_AT_VSYNC) };
    let mut vdc = HuC6270::new();
    external_frame(&mut vdc, VceFrameLength::Lines262, 1);
    vdc.advance_external_vce_scanline(external_input(1, true, VceFrameLength::Lines262))
        .unwrap();

    write_register(&mut vdc, VdcRegister::VerticalSync, 1);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 256);
    let old_profile = vdc
        .advance_external_vce_scanline(external_input(1, false, VceFrameLength::Lines263))
        .unwrap();
    assert_eq!(old_profile.phase(), VdcVerticalPhase::DisplayStart);
    assert_eq!(old_profile.phase_line(), 0);

    let new_profile = vdc
        .advance_external_vce_scanline(external_input(1, true, VceFrameLength::Lines263))
        .unwrap();
    assert_eq!(new_profile.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(new_profile.phase_line(), 0);
    let second_vsync = vdc
        .advance_external_vce_scanline(external_input(1, false, VceFrameLength::Lines263))
        .unwrap();
    assert_eq!(second_vsync.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(second_vsync.phase_line(), 1);
}

#[test]
fn short_external_profile_free_runs_until_an_external_marker() {
    let frame_length = VceFrameLength::Lines262;
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0);

    let phases = std::array::from_fn::<_, 5, _>(|line| {
        vdc.advance_external_vce_scanline(external_input(1, line == 0, frame_length))
            .unwrap()
            .phase()
    });
    assert_eq!(
        phases,
        [
            VdcVerticalPhase::VerticalSync,
            VdcVerticalPhase::DisplayStart,
            VdcVerticalPhase::DisplayStart,
            VdcVerticalPhase::ActiveDisplay,
            VdcVerticalPhase::VerticalSync,
        ]
    );

    let restarted = vdc
        .advance_external_vce_scanline(external_input(1, true, frame_length))
        .unwrap();
    assert_eq!(restarted.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(restarted.phase_line(), 0);
    assert_eq!(restarted.frame_line(), 0);
}

#[test]
fn external_and_internal_sync_match_dma_status_and_irq_for_vcr_zero_and_one() {
    let frame_length = VceFrameLength::Lines262;
    let mut external = HuC6270::new();
    let mut internal = HuC6270::new();
    external_frame(&mut external, frame_length, 1);
    external_frame(&mut internal, frame_length, 1);
    write_register(&mut external, VdcRegister::Control, 0x000C);
    write_register(&mut internal, VdcRegister::Control, 0x003C);
    prepare_scheduler_dma(&mut external);
    prepare_scheduler_dma(&mut internal);

    for line in 0..=frame_length.scanlines() {
        let external_boundary = external
            .advance_external_vce_scanline(external_input(
                1,
                line % frame_length.scanlines() == 0,
                frame_length,
            ))
            .unwrap();
        let internal_boundary = internal.advance_scanline_boundary().unwrap();
        assert_eq!(external_boundary, internal_boundary);
        assert_eq!(external.pending_vram_dma(), internal.pending_vram_dma());
        assert_eq!(external.active_vram_dma(), internal.active_vram_dma());
        assert_eq!(external.pending_satb_dma(), internal.pending_satb_dma());
        assert_eq!(external.active_satb_dma(), internal.active_satb_dma());
        assert_eq!(external.status(), internal.status());
        assert_eq!(external.irq_level(), internal.irq_level());
    }

    let mut external = HuC6270::new();
    let mut internal = HuC6270::new();
    external_frame(&mut external, frame_length, 0);
    external_frame(&mut internal, frame_length, 0);
    write_register(&mut external, VdcRegister::Control, 0x000C);
    write_register(&mut internal, VdcRegister::Control, 0x003C);
    prepare_scheduler_dma(&mut external);
    prepare_scheduler_dma(&mut internal);

    let mut external_vblank = None;
    for line in 0..frame_length.scanlines() {
        let boundary = external
            .advance_external_vce_scanline(external_input(1, line == 0, frame_length))
            .unwrap();
        if boundary.vertical_blank_started() {
            external_vblank = Some(boundary);
        }
    }
    let internal_vblank = next_vertical_blank(&mut internal);
    let external_vblank = external_vblank.unwrap();
    assert_eq!(external_vblank.transitions(), internal_vblank.transitions());
    assert!(external_vblank.satb_dma_started());
    assert!(internal_vblank.satb_dma_started());
    assert_eq!(external.pending_vram_dma(), internal.pending_vram_dma());
    assert_eq!(external.active_vram_dma(), internal.active_vram_dma());
    assert_eq!(external.pending_satb_dma(), internal.pending_satb_dma());
    assert_eq!(external.active_satb_dma(), internal.active_satb_dma());
    assert_eq!(external.status(), internal.status());
    assert_eq!(external.irq_level(), internal.irq_level());
}

#[test]
fn next_external_profile_applies_its_new_vsync_duration_immediately() {
    let frame_length = VceFrameLength::Lines262;
    let mut vdc = HuC6270::new();
    external_frame(&mut vdc, frame_length, 1);

    for line in 0..frame_length.scanlines() {
        vdc.advance_external_vce_scanline(external_input(1, line == 0, frame_length))
            .unwrap();
        if line == 0 {
            write_register(&mut vdc, VdcRegister::VerticalSync, 1);
            write_register(&mut vdc, VdcRegister::VerticalDisplay, 256);
        }
    }

    let vsync_zero = vdc
        .advance_external_vce_scanline(external_input(1, true, frame_length))
        .unwrap();
    assert_eq!(vsync_zero.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(vsync_zero.phase_line(), 0);
    let vsync_one = vdc
        .advance_external_vce_scanline(external_input(1, false, frame_length))
        .unwrap();
    assert_eq!(vsync_one.phase(), VdcVerticalPhase::VerticalSync);
    assert_eq!(vsync_one.phase_line(), 1);
    let display_start = vdc
        .advance_external_vce_scanline(external_input(1, false, frame_length))
        .unwrap();
    assert_eq!(display_start.phase(), VdcVerticalPhase::DisplayStart);
    assert_eq!(display_start.phase_line(), 0);
}
