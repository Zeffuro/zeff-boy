use super::cpu::{LineLevel, VdcPort};
use super::{
    HuC6270, VDC_DMA_PIXELS_PER_WORD, VceFrameLength, VdcExternalVceScanline, VdcHorizontalPhase,
    VdcPortWriteResult, VdcRegister, VdcStatus, VdcVerticalPhase, VdcVramDmaTriggerResult,
};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn select(vdc: &mut HuC6270, register: VdcRegister) {
    let _ = vdc.write_port(VdcPort::SelectOrStatus, register as u8);
}

fn write_register(vdc: &mut HuC6270, register: VdcRegister, value: u16) -> VdcPortWriteResult {
    select(vdc, register);
    let _ = vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8)
}

fn advance_to_deferred_vertical_blank(vdc: &mut HuC6270) {
    write_register(vdc, VdcRegister::Control, 0x000C);
    write_register(vdc, VdcRegister::RasterCounter, 66);
    write_register(vdc, VdcRegister::VerticalSync, 0);
    write_register(vdc, VdcRegister::VerticalDisplay, 0);
    write_register(vdc, VdcRegister::VerticalDisplayEnd, 1);

    for line in 0..5 {
        let boundary = vdc
            .advance_machine_vce_scanline(VdcExternalVceScanline::new(
                1,
                line == 0,
                VceFrameLength::Lines262,
            ))
            .unwrap();
        if line == 4 {
            assert!(boundary.raster_match());
            assert!(boundary.vertical_blank_started());
        }
    }
}

#[test]
fn vertical_blank_follows_same_line_raster_irq_at_display_start() {
    let mut vdc = HuC6270::new();
    advance_to_deferred_vertical_blank(&mut vdc);

    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 4);
    vdc.begin_external_horizontal_line();
    let before = vdc.advance_horizontal_pixels(7).unwrap();
    assert!(!before.vertical_blank_started());
    assert_eq!(vdc.status(), VdcStatus::empty());

    let event = vdc.advance_horizontal_pixels(1).unwrap();
    assert!(event.vertical_blank_started());
    assert!(vdc.frame_burst_enabled());
    assert_eq!(vdc.status(), VdcStatus::VERTICAL_BLANK);
    assert_eq!(vdc.irq_level(), LineLevel::Low);
}

#[test]
fn deferred_machine_vblank_latches_frame_burst_when_horizontal_timing_consumes_it() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x000C);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 1);

    for line in 0..5 {
        vdc.advance_machine_vce_scanline(VdcExternalVceScanline::new(
            1,
            line == 0,
            VceFrameLength::Lines262,
        ))
        .unwrap();
        vdc.begin_external_horizontal_line();
        assert!(!vdc.frame_burst_enabled());
        let horizontal = vdc.advance_horizontal_pixels(8).unwrap();
        if horizontal.vertical_blank_started() {
            assert!(vdc.frame_burst_enabled());
            return;
        }
    }

    panic!("deferred machine vertical blank did not reach horizontal timing");
}

#[test]
fn raster_irq_precedes_next_line_vblank_for_dragon_egg_geometry() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::VerticalSync, 0x0F02);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 0x00EF);
    write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 0x0010);

    for line in 0..259 {
        vdc.advance_machine_vce_scanline(VdcExternalVceScanline::new(
            1,
            line == 0,
            VceFrameLength::Lines263,
        ))
        .unwrap();
    }

    write_register(&mut vdc, VdcRegister::Control, 0x000C);
    write_register(&mut vdc, VdcRegister::RasterCounter, 0x0130);
    write_register(&mut vdc, VdcRegister::HorizontalSync, 0x0202);
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 0x041F);
    vdc.begin_external_horizontal_line();

    vdc.advance_horizontal_pixels(265).unwrap();
    assert_eq!(vdc.status(), VdcStatus::empty());
    vdc.advance_horizontal_pixels(1).unwrap();
    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 4);

    vdc.advance_horizontal_pixels(75).unwrap();
    let boundary = vdc
        .advance_machine_vce_scanline(VdcExternalVceScanline::new(
            1,
            false,
            VceFrameLength::Lines263,
        ))
        .unwrap();
    assert!(boundary.raster_match());
    assert!(vdc.status().is_empty());

    vdc.begin_external_horizontal_line();
    vdc.advance_horizontal_pixels(23).unwrap();
    assert!(vdc.status().is_empty());
    let vblank = vdc.advance_horizontal_pixels(1).unwrap();
    assert!(vblank.vertical_blank_started());
    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 0x20);
}

#[test]
fn raster_compare_falls_back_to_line_end_when_hdw_is_not_reached() {
    let mut vdc = HuC6270::new();
    vdc.advance_machine_vce_scanline(VdcExternalVceScanline::new(
        1,
        true,
        VceFrameLength::Lines263,
    ))
    .unwrap();
    write_register(&mut vdc, VdcRegister::Control, 0x0004);
    write_register(&mut vdc, VdcRegister::RasterCounter, 2);
    write_register(&mut vdc, VdcRegister::HorizontalSync, 0x7F00);

    vdc.begin_external_horizontal_line();
    vdc.advance_horizontal_pixels(341).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::DisplayStart);
    vdc.advance_machine_vce_scanline(VdcExternalVceScanline::new(
        1,
        false,
        VceFrameLength::Lines263,
    ))
    .unwrap();
    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 4);
}

#[test]
fn wrapped_horizontal_line_does_not_repeat_its_raster_match_at_external_boundary() {
    let mut vdc = HuC6270::new();
    vdc.advance_machine_vce_scanline(VdcExternalVceScanline::new(
        1,
        true,
        VceFrameLength::Lines263,
    ))
    .unwrap();
    write_register(&mut vdc, VdcRegister::Control, 0x0004);
    write_register(&mut vdc, VdcRegister::RasterCounter, 2);
    write_register(&mut vdc, VdcRegister::HorizontalSync, 0x0202);
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 0x031F);

    vdc.begin_external_horizontal_line();
    vdc.advance_horizontal_pixels(341).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::DisplayStart);
    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 4);

    let boundary = vdc
        .advance_machine_vce_scanline(VdcExternalVceScanline::new(
            1,
            false,
            VceFrameLength::Lines263,
        ))
        .unwrap();
    assert!(boundary.raster_match());
    assert!(vdc.status().is_empty());
}

#[test]
fn deferred_vertical_blank_survives_vdc_state_round_trip() {
    let mut vdc = HuC6270::new();
    advance_to_deferred_vertical_blank(&mut vdc);
    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 4);

    let mut writer = StateWriter::new();
    vdc.write_state(&mut writer);
    let mut restored = HuC6270::new();
    restored
        .read_state(&mut StateReader::new(&writer.into_bytes()))
        .unwrap();

    restored.begin_external_horizontal_line();
    let event = restored.advance_horizontal_pixels(8).unwrap();
    assert!(event.vertical_blank_started());
    assert_eq!(restored.status(), VdcStatus::VERTICAL_BLANK);
}

#[test]
fn horizontal_fields_latch_only_when_their_phase_starts() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::HorizontalSync, 0x0101);
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 0x0203);
    vdc.begin_external_horizontal_line();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::DisplayStart);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 16);

    vdc.advance_horizontal_pixels(15).unwrap();
    write_register(&mut vdc, VdcRegister::HorizontalSync, 0x0301);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 1);
    vdc.advance_horizontal_pixels(1).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::ActiveDisplay);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 32);

    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 0x0204);
    vdc.advance_horizontal_pixels(32).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::DisplayEnd);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 24);
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 0x0504);
    vdc.advance_horizontal_pixels(24).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::Sync);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 16);
    vdc.advance_horizontal_pixels(16).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::DisplayStart);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 32);
    vdc.advance_horizontal_pixels(32).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::ActiveDisplay);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 40);
    vdc.advance_horizontal_pixels(40).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::DisplayEnd);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 48);
}

#[test]
fn dma_slot_remainder_survives_calls_and_external_line_reset() {
    let mut vdc = HuC6270::new();
    let first = vdc.advance_horizontal_pixels(3).unwrap();
    assert_eq!(first.dma_slots(), 0);
    assert_eq!(vdc.dma_pixel_remainder(), 3);

    vdc.begin_external_horizontal_line();
    assert_eq!(vdc.dma_pixel_remainder(), 3);
    let second = vdc.advance_horizontal_pixels(1).unwrap();
    assert_eq!(second.dma_slots(), 1);
    assert_eq!(vdc.dma_pixel_remainder(), 0);
    assert_eq!(VDC_DMA_PIXELS_PER_WORD, 4);
}

#[test]
fn vram_dma_trigger_is_queued_in_nonburst_active_display() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x00B0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 1);
    while vdc
        .advance_scanline_boundary()
        .unwrap()
        .active_display()
        .is_none()
    {}

    assert_eq!(
        write_register(&mut vdc, VdcRegister::DmaLength, 0),
        VdcPortWriteResult::VramDma(VdcVramDmaTriggerResult::Queued)
    );
    assert!(vdc.pending_vram_dma().is_some());
    assert_eq!(vdc.active_vram_dma(), None);
    assert_eq!(vdc.advance_horizontal_pixels(4).unwrap().vram_words(), 0);
    assert!(vdc.pending_vram_dma().is_some());

    loop {
        let boundary = vdc.advance_scanline_boundary().unwrap();
        if boundary.phase() != VdcVerticalPhase::ActiveDisplay {
            break;
        }
    }
    assert_eq!(vdc.advance_horizontal_pixels(4).unwrap().vram_words(), 1);
    assert_eq!(vdc.pending_vram_dma(), None);
    assert_eq!(vdc.active_vram_dma(), None);
}

#[test]
fn scheduler_gives_satb_every_shared_slot_before_vram_dma() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0] = 0x1111;
    vdc.vram_mut()[0x0100] = 0x2222;
    write_register(&mut vdc, VdcRegister::DmaSource, 0);
    write_register(&mut vdc, VdcRegister::DmaDestination, 1);
    assert_eq!(
        write_register(&mut vdc, VdcRegister::DmaLength, 0),
        VdcPortWriteResult::VramDma(VdcVramDmaTriggerResult::Queued)
    );
    write_register(&mut vdc, VdcRegister::SatbSource, 0x0100);
    assert!(vdc.start_satb_dma_for_vertical_blank());

    let first = vdc.advance_horizontal_pixels(4).unwrap();
    assert_eq!(first.satb_words(), 1);
    assert_eq!(first.vram_words(), 0);
    assert_eq!(vdc.satb()[0], 0x2222);
    assert_eq!(vdc.vram()[1], 0);

    let satb_tail = vdc.advance_horizontal_pixels(255 * 4).unwrap();
    assert_eq!(satb_tail.satb_words(), 255);
    assert_eq!(satb_tail.vram_words(), 0);
    assert_eq!(satb_tail.dma_completions(), 1);
    let vram = vdc.advance_horizontal_pixels(4).unwrap();
    assert_eq!(vram.satb_words(), 0);
    assert_eq!(vram.vram_words(), 1);
    assert_eq!(vram.dma_completions(), 1);
    assert_eq!(vdc.vram()[1], 0x1111);
}

#[test]
fn maximum_length_dma_wraps_and_completes_at_four_pixels_per_word() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0x7FFF] = 0xCAFE;
    write_register(&mut vdc, VdcRegister::DmaSource, 0xFFFF);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x7FFF);
    write_register(&mut vdc, VdcRegister::DmaLength, 0xFFFF);

    let advance = vdc
        .advance_horizontal_pixels(65_536 * u64::from(VDC_DMA_PIXELS_PER_WORD))
        .unwrap();
    assert_eq!(advance.vram_words(), 65_536);
    assert_eq!(advance.dma_completions(), 1);
    assert_eq!(vdc.active_vram_dma(), None);
    assert_eq!(vdc.register(VdcRegister::DmaSource), 0xFFFF);
    assert_eq!(vdc.register(VdcRegister::DmaDestination), 0x7FFF);
    assert_eq!(vdc.register(VdcRegister::DmaLength), 0xFFFF);
    assert_eq!(vdc.vram()[0x7FFF], 0xCAFE);
}

#[test]
fn display_enables_latch_at_hdw_entry_for_the_active_line_snapshot() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x00B0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 1);
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 0);
    vdc.begin_external_horizontal_line();
    vdc.advance_horizontal_pixels(8).unwrap();
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 31);
    write_register(&mut vdc, VdcRegister::Control, 0x0030);
    let first = loop {
        if let Some(display) = vdc.advance_scanline_boundary().unwrap().active_display() {
            break display;
        }
    };
    assert!(first.background().enabled());
    assert_eq!(first.source_width(), 8);

    vdc.begin_external_horizontal_line();
    vdc.advance_horizontal_pixels(8).unwrap();
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 63);
    write_register(&mut vdc, VdcRegister::Control, 0x00B0);
    let second = vdc
        .advance_scanline_boundary()
        .unwrap()
        .active_display()
        .unwrap();
    assert!(!second.background().enabled());
    assert_eq!(second.source_width(), 256);
}

#[test]
fn frame_burst_latch_defers_control_changes_and_prevents_active_abort() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x0030);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplayEnd, 1);
    loop {
        if vdc
            .advance_scanline_boundary()
            .unwrap()
            .vertical_blank_started()
        {
            break;
        }
    }
    assert!(vdc.frame_burst_enabled());

    write_register(&mut vdc, VdcRegister::Control, 0x00B0);
    write_register(&mut vdc, VdcRegister::DmaSource, 0);
    write_register(&mut vdc, VdcRegister::DmaDestination, 1);
    write_register(&mut vdc, VdcRegister::DmaLength, 1);
    let active = loop {
        let boundary = vdc.advance_scanline_boundary().unwrap();
        if boundary.active_display().is_some() {
            break boundary;
        }
    };
    assert!(!active.vram_dma_aborted());
    assert_eq!(vdc.advance_horizontal_pixels(4).unwrap().vram_words(), 1);

    loop {
        if vdc
            .advance_scanline_boundary()
            .unwrap()
            .vertical_blank_started()
        {
            break;
        }
    }
    assert!(!vdc.frame_burst_enabled());
}

#[test]
fn entering_nonburst_active_display_preserves_an_unstarted_transfer() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x00B0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 1);
    write_register(&mut vdc, VdcRegister::DmaLength, 4);
    assert!(vdc.pending_vram_dma().is_some());
    assert!(vdc.active_vram_dma().is_none());

    let active = loop {
        let boundary = vdc.advance_scanline_boundary().unwrap();
        if boundary.active_display().is_some() {
            break boundary;
        }
    };
    assert!(!active.vram_dma_aborted());
    assert!(vdc.pending_vram_dma().is_some());
    assert_eq!(vdc.active_vram_dma(), None);
    assert_eq!(vdc.advance_horizontal_pixels(64).unwrap().vram_words(), 0);

    assert_eq!(
        write_register(&mut vdc, VdcRegister::DmaLength, 1),
        VdcPortWriteResult::VramDma(VdcVramDmaTriggerResult::RejectedWhilePending)
    );
    assert!(vdc.pending_vram_dma().is_some());
}

#[test]
fn dma_trigger_during_an_active_transfer_is_rejected_without_replacement() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::DmaSource, 0x0100);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x0200);
    write_register(&mut vdc, VdcRegister::DmaLength, 3);
    vdc.advance_horizontal_pixels(4).unwrap();
    let active = vdc.active_vram_dma().unwrap();

    assert_eq!(
        write_register(&mut vdc, VdcRegister::DmaLength, 0x1234),
        VdcPortWriteResult::VramDma(VdcVramDmaTriggerResult::RejectedWhileActive)
    );
    assert_eq!(vdc.active_vram_dma(), Some(active));
    assert_eq!(vdc.pending_vram_dma(), None);
}

#[test]
fn dma_retrigger_before_any_pixel_is_rejected_without_replacing_pending_state() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::DmaSource, 0x0100);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x0200);
    write_register(&mut vdc, VdcRegister::DmaLength, 3);
    let pending = vdc.pending_vram_dma().unwrap();

    write_register(&mut vdc, VdcRegister::DmaSource, 0x1111);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x2222);
    assert_eq!(
        write_register(&mut vdc, VdcRegister::DmaLength, 0),
        VdcPortWriteResult::VramDma(VdcVramDmaTriggerResult::RejectedWhilePending)
    );
    assert_eq!(vdc.pending_vram_dma(), Some(pending));
    assert_eq!(vdc.active_vram_dma(), None);
    assert_eq!(vdc.dma_pixel_remainder(), 0);
}

#[test]
fn coincident_phase_transition_and_dma_slot_apply_at_the_same_pixel_boundary() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0] = 0xCAFE;
    write_register(&mut vdc, VdcRegister::Control, 0x0080);
    write_register(&mut vdc, VdcRegister::DmaSource, 0);
    write_register(&mut vdc, VdcRegister::DmaDestination, 1);
    write_register(&mut vdc, VdcRegister::DmaLength, 1);

    vdc.advance_horizontal_pixels(4).unwrap();
    let boundary = vdc.advance_horizontal_pixels(4).unwrap();
    assert_eq!(boundary.phase_transitions(), 1);
    assert_eq!(boundary.dma_slots(), 1);
    assert_eq!(boundary.vram_words(), 1);
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::ActiveDisplay);
    assert_eq!(vdc.vram()[1], 0xCAFE);
}

#[test]
fn sync_width_write_waits_until_the_following_sync_entry() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::HorizontalDisplay, 0);
    write_register(&mut vdc, VdcRegister::HorizontalSync, 1);
    vdc.begin_external_horizontal_line();
    vdc.advance_horizontal_pixels(24).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::Sync);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 16);

    write_register(&mut vdc, VdcRegister::HorizontalSync, 3);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 16);
    vdc.advance_horizontal_pixels(16 + 8 + 8 + 8).unwrap();
    assert_eq!(vdc.horizontal_phase(), VdcHorizontalPhase::Sync);
    assert_eq!(vdc.horizontal_phase_pixels_remaining(), 32);
}

#[test]
fn active_entry_pauses_partial_dma_without_completion_status_or_irq() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x00B0);
    write_register(&mut vdc, VdcRegister::VerticalSync, 0);
    write_register(&mut vdc, VdcRegister::VerticalDisplay, 1);
    write_register(&mut vdc, VdcRegister::DmaControl, 0x0002);
    vdc.vram_mut()[0] = 0xCAFE;
    write_register(&mut vdc, VdcRegister::DmaSource, 0);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x0100);
    write_register(&mut vdc, VdcRegister::DmaLength, 3);
    assert_eq!(vdc.advance_horizontal_pixels(4).unwrap().vram_words(), 1);
    assert_eq!(vdc.register(VdcRegister::DmaSource), 1);
    assert_eq!(vdc.register(VdcRegister::DmaDestination), 0x0101);
    assert_eq!(vdc.register(VdcRegister::DmaLength), 2);

    let active = loop {
        let boundary = vdc.advance_scanline_boundary().unwrap();
        if boundary.active_display().is_some() {
            break boundary;
        }
    };
    assert!(!active.vram_dma_aborted());
    assert_eq!(vdc.pending_vram_dma(), None);
    assert!(vdc.active_vram_dma().is_some());
    assert!(!vdc.status().contains(VdcStatus::VRAM_DMA_COMPLETE));
    assert_eq!(vdc.irq_level(), LineLevel::High);
    assert_eq!(vdc.register(VdcRegister::DmaSource), 1);
    assert_eq!(vdc.register(VdcRegister::DmaDestination), 0x0101);
    assert_eq!(vdc.register(VdcRegister::DmaLength), 2);

    assert_eq!(vdc.advance_horizontal_pixels(64).unwrap().vram_words(), 0);
    assert_eq!(vdc.register(VdcRegister::DmaSource), 1);
    assert_eq!(vdc.register(VdcRegister::DmaDestination), 0x0101);
    assert_eq!(vdc.register(VdcRegister::DmaLength), 2);

    loop {
        let boundary = vdc.advance_scanline_boundary().unwrap();
        if boundary.phase() != VdcVerticalPhase::ActiveDisplay {
            break;
        }
    }
    let resumed = vdc.advance_horizontal_pixels(12).unwrap();
    assert_eq!(resumed.vram_words(), 3);
    assert_eq!(resumed.dma_completions(), 1);
    assert_eq!(vdc.active_vram_dma(), None);
    assert!(vdc.status().contains(VdcStatus::VRAM_DMA_COMPLETE));
    assert_eq!(vdc.irq_level(), LineLevel::Low);
}
