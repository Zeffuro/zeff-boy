use super::cpu::{Cpu, LineLevel, VdcPort};
use super::{
    BaseBus, BaseBusErrorKind, HuC6202, HuC6260, HuC6270, OPEN_BUS_VALUE,
    PCE_ACTIVE_FRAME_UNUSED_RGBA, PCE_ACTIVE_FRAME_WIDTH, PceActiveOnlyVideoFrame,
    PceCartridgeDescriptor, PceCartridgeHardware, PceDevices, PceHardwareTopology, PceHuCardBoard,
    PceMachine, PceVideoRenderError, PhysicalRegion, PsgRevision, SUPERGRAFX_WORK_RAM_LEN,
    VDC_SATB_WORDS, VceFrameLength, VcePixelClock, VcePort, VdcActiveDisplayLine, VdcDmaChannel,
    VdcExternalVceScanline, VdcRegister, VdcScanlineAdvanceError, VdcStatus, VpcPort, VpcVdc,
    decode_physical_region_for,
};
use crate::hardware::{ControllerPort, PceConsoleWiring};

const RESET_PC: u16 = 0xE000;

fn devices() -> PceDevices {
    PceDevices::with_topology_console_wiring_and_psg_revision(
        PceHardwareTopology::SuperGrafx,
        ControllerPort::default(),
        PceConsoleWiring::PcEngine,
        PsgRevision::HuC6280,
    )
}

fn bus(rom: Vec<u8>) -> BaseBus<PceDevices> {
    BaseBus::with_hucard_and_topology(
        rom,
        PceHuCardBoard::Plain,
        PceHardwareTopology::SuperGrafx,
        devices(),
    )
    .unwrap()
}

fn write_register(vdc: &mut super::HuC6270, register: VdcRegister, value: u16) {
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

fn queue_dma(vdc: &mut super::HuC6270, source: u16, destination: u16, length: u16) {
    write_register(vdc, VdcRegister::DmaSource, source);
    write_register(vdc, VdcRegister::DmaDestination, destination);
    write_register(vdc, VdcRegister::DmaLength, length);
}

fn configure_active_line(vdc: &mut HuC6270, hds: u16, hdw: u16, control: u16) {
    write_register(vdc, VdcRegister::Control, 0x0030 | control);
    write_register(vdc, VdcRegister::HorizontalSync, hds << 8);
    write_register(vdc, VdcRegister::HorizontalDisplay, hdw);
    write_register(vdc, VdcRegister::VerticalSync, 0);
    write_register(vdc, VdcRegister::VerticalDisplay, 0);
    write_register(vdc, VdcRegister::VerticalDisplayEnd, 1);
    vdc.begin_external_horizontal_line();
}

fn next_active(vdc: &mut HuC6270) -> VdcActiveDisplayLine {
    loop {
        if let Some(display) = vdc.advance_scanline_boundary().unwrap().active_display() {
            return display;
        }
    }
}

fn write_color(vce: &mut HuC6260, index: u16, raw: u16) {
    vce.write_port(VcePort::from_offset(2), index as u8);
    vce.write_port(VcePort::from_offset(3), (index >> 8) as u8);
    vce.write_port(VcePort::from_offset(4), raw as u8);
    vce.write_port(VcePort::from_offset(5), (raw >> 8) as u8);
}

fn set_background_pixel(vdc: &mut HuC6270, palette: u16) {
    vdc.vram_mut()[0] = (palette << 12) | 1;
    vdc.vram_mut()[0x10] = 0x0080;
}

fn set_window(vpc: &mut HuC6202, low_offset: u8, value: u16) {
    vpc.write_port(VpcPort::from_offset(low_offset), value as u8);
    vpc.write_port(VpcPort::from_offset(low_offset + 1), (value >> 8) as u8);
}

fn frame_pixel(frame: &PceActiveOnlyVideoFrame, x: usize, y: usize) -> [u8; 4] {
    let offset = (y * PCE_ACTIVE_FRAME_WIDTH + x) * 4;
    frame.framebuffer()[offset..offset + 4].try_into().unwrap()
}

fn load_collision_overflow_satb(vdc: &mut HuC6270) {
    let mut words = [0; VDC_SATB_WORDS];
    for index in 0..17 {
        let base = index * 4;
        words[base] = 64;
        words[base + 1] = if index < 2 { 32 } else { 900 };
        words[base + 2] = 2;
    }
    let source = 0x4000;
    vdc.vram_mut()[source..source + VDC_SATB_WORDS].copy_from_slice(&words);
    write_register(vdc, VdcRegister::SatbSource, source as u16);
    assert!(vdc.start_satb_dma_for_vertical_blank());
    for _ in 0..VDC_SATB_WORDS {
        vdc.service_dma_slot(VdcDmaChannel::Satb).unwrap();
    }
    vdc.vram_mut()[0x40] = 0x8000;
}

fn rom_with_program(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..program.len()].copy_from_slice(program);
    rom[0x1FFE..0x2000].copy_from_slice(&RESET_PC.to_le_bytes());
    rom
}

#[test]
fn bus_rejects_both_device_topology_mismatches() {
    let base_with_supergrafx_devices = BaseBus::new(Vec::new(), devices()).unwrap_err();
    assert_eq!(
        base_with_supergrafx_devices.kind(),
        BaseBusErrorKind::HardwareTopologyMismatch {
            requested: PceHardwareTopology::Base,
            devices: PceHardwareTopology::SuperGrafx,
        }
    );

    let supergrafx_with_base_devices = BaseBus::with_hucard_and_topology(
        Vec::new(),
        PceHuCardBoard::Plain,
        PceHardwareTopology::SuperGrafx,
        (),
    )
    .unwrap_err();
    assert_eq!(
        supergrafx_with_base_devices.kind(),
        BaseBusErrorKind::HardwareTopologyMismatch {
            requested: PceHardwareTopology::SuperGrafx,
            devices: PceHardwareTopology::Base,
        }
    );
}

#[test]
fn topology_decoder_maps_exact_32_byte_video_blocks_and_aliases() {
    let decode = |address| decode_physical_region_for(PceHardwareTopology::SuperGrafx, address);
    assert_eq!(
        decode(0x1F_E000),
        PhysicalRegion::Vdc(VdcPort::SelectOrStatus)
    );
    assert_eq!(
        decode(0x1F_E004),
        PhysicalRegion::Vdc(VdcPort::SelectOrStatus)
    );
    assert_eq!(decode(0x1F_E005), PhysicalRegion::Unmapped);
    assert_eq!(decode(0x1F_E007), PhysicalRegion::Vdc(VdcPort::DataHigh));
    assert_eq!(
        decode(0x1F_E008),
        PhysicalRegion::Vpc(VpcPort::from_offset(0))
    );
    assert_eq!(
        decode(0x1F_E00F),
        PhysicalRegion::Vpc(VpcPort::from_offset(7))
    );
    assert_eq!(
        decode(0x1F_E010),
        PhysicalRegion::Vdc2(VdcPort::SelectOrStatus)
    );
    assert_eq!(
        decode(0x1F_E014),
        PhysicalRegion::Vdc2(VdcPort::SelectOrStatus)
    );
    assert_eq!(decode(0x1F_E015), PhysicalRegion::Unmapped);
    assert_eq!(decode(0x1F_E017), PhysicalRegion::Vdc2(VdcPort::DataHigh));
    assert_eq!(decode(0x1F_E018), PhysicalRegion::Unmapped);
    assert_eq!(decode(0x1F_E01F), PhysicalRegion::Unmapped);
    assert_eq!(
        decode(0x1F_E020),
        PhysicalRegion::Vdc(VdcPort::SelectOrStatus)
    );
    assert_eq!(decode(0x1F_E3FF), PhysicalRegion::Unmapped);
}

#[test]
fn supergrafx_work_ram_keeps_all_four_cer_banks_distinct() {
    let mut bus = bus(Vec::new());
    for (address, value) in [
        (0x1F_0005, 0x11),
        (0x1F_2005, 0x22),
        (0x1F_4005, 0x33),
        (0x1F_6005, 0x44),
    ] {
        bus.write(address, value);
    }
    assert_eq!(bus.read(0x1F_0005), 0x11);
    assert_eq!(bus.read(0x1F_2005), 0x22);
    assert_eq!(bus.read(0x1F_4005), 0x33);
    assert_eq!(bus.read(0x1F_6005), 0x44);
    assert_eq!(bus.mapped_work_ram().len(), SUPERGRAFX_WORK_RAM_LEN);
    assert_eq!(bus.work_ram()[5], 0x11);
}

#[test]
fn mapped_vdc_and_vpc_registers_are_independent_and_mirrored() {
    let mut bus = bus(Vec::new());
    bus.write(0x1F_E000, VdcRegister::Control as u8);
    bus.write(0x1F_E002, 0x05);
    bus.write(0x1F_E003, 0x00);
    bus.write(0x1F_E030, VdcRegister::Control as u8);
    bus.write(0x1F_E032, 0x0A);
    bus.write(0x1F_E033, 0x00);
    bus.write(0x1F_E028, 0xA5);

    assert_eq!(bus.devices().vdc().register(VdcRegister::Control), 0x0005);
    let video = bus.devices().supergrafx_video().unwrap();
    assert_eq!(video.vdc2().register(VdcRegister::Control), 0x000A);
    assert_eq!(video.vpc().priority_control_a(), 0xA5);
    assert_eq!(bus.read(0x1F_E008), 0xA5);
    assert_eq!(bus.read(0x1F_E018), OPEN_BUS_VALUE);
}

#[test]
fn direct_stores_follow_the_vpc_target_without_touching_the_other_vdc() {
    let mut bus = bus(vec![0x03, 0x05, 0x03, 0x0A]);
    let mut cpu = Cpu::new();
    cpu.step(&mut bus).unwrap();
    assert_eq!(bus.devices().vdc().selected_register_id(), 0x05);
    assert_eq!(
        bus.devices()
            .supergrafx_video()
            .unwrap()
            .vdc2()
            .selected_register_id(),
        0
    );

    bus.write(0x1F_E00E, 1);
    cpu.step(&mut bus).unwrap();
    assert_eq!(bus.devices().vdc().selected_register_id(), 0x05);
    assert_eq!(
        bus.devices()
            .supergrafx_video()
            .unwrap()
            .vdc2()
            .selected_register_id(),
        0x0A
    );
}

#[test]
fn aggregate_irq_remains_low_until_both_vdc_sources_clear() {
    let mut devices = devices();
    write_register(devices.vdc_mut(), VdcRegister::Control, 0x0004);
    let video = devices.supergrafx_video_mut().unwrap();
    write_register(video.vdc2_mut(), VdcRegister::Control, 0x0004);
    devices.vdc_mut().latch_status(VdcStatus::RASTER_MATCH);
    devices
        .supergrafx_video_mut()
        .unwrap()
        .vdc2_mut()
        .latch_status(VdcStatus::RASTER_MATCH);
    assert_eq!(devices.vdc_irq_level(), LineLevel::Low);

    devices.vdc_mut().read_port(VdcPort::SelectOrStatus);
    assert_eq!(devices.vdc_irq_level(), LineLevel::Low);
    devices
        .supergrafx_video_mut()
        .unwrap()
        .vdc2_mut()
        .read_port(VdcPort::SelectOrStatus);
    assert_eq!(devices.vdc_irq_level(), LineLevel::High);
}

#[test]
fn reset_restores_both_vdcs_and_vpc_without_clearing_work_ram() {
    let mut bus = bus(Vec::new());
    bus.write(0x1F_6005, 0xA5);
    bus.write(0x1F_E00E, 1);
    bus.write(0x1F_E010, VdcRegister::Control as u8);
    bus.write(0x1F_E012, 0x0F);
    bus.devices_mut().reset();

    assert_eq!(bus.read(0x1F_6005), 0xA5);
    assert_eq!(bus.devices().vdc().selected_register_id(), 0);
    let video = bus.devices().supergrafx_video().unwrap();
    assert_eq!(video.vdc2().selected_register_id(), 0);
    assert_eq!(video.vdc2().register(VdcRegister::Control), 0);
    assert_eq!(video.vpc().direct_vdc_target(), VpcVdc::One);
    assert_eq!(video.vpc().priority_control_a(), 0x11);
}

#[test]
fn dual_vdcs_receive_identical_pixels_and_transactional_line_markers() {
    let mut devices = devices();
    let advance = devices.advance_horizontal_pixels(3).unwrap();
    assert_eq!(advance.0.pixel_clocks(), 3);
    assert_eq!(advance.1.unwrap().pixel_clocks(), 3);
    assert_eq!(devices.vdc().dma_pixel_remainder(), 3);
    assert_eq!(
        devices
            .supergrafx_video()
            .unwrap()
            .vdc2()
            .dma_pixel_remainder(),
        3
    );

    let marker = VdcExternalVceScanline::new(1, true, VceFrameLength::Lines262);
    let first = devices.advance_machine_vce_scanline(marker).unwrap();
    assert_eq!(first.0, first.1.unwrap());
    write_register(
        devices.supergrafx_video_mut().unwrap().vdc2_mut(),
        VdcRegister::Control,
        0x0020,
    );
    let line = VdcExternalVceScanline::new(1, false, VceFrameLength::Lines262);
    assert_eq!(
        devices.advance_machine_vce_scanline(line),
        Err(VdcScanlineAdvanceError::InvalidSyncMode)
    );
    write_register(
        devices.supergrafx_video_mut().unwrap().vdc2_mut(),
        VdcRegister::Control,
        0,
    );
    let next = devices.advance_machine_vce_scanline(line).unwrap();
    assert_eq!(next.0, next.1.unwrap());
}

#[test]
fn supergrafx_renderer_aligns_different_hds_and_records_the_union_origin() {
    let mut one = HuC6270::new();
    let mut two = HuC6270::new();
    configure_active_line(&mut one, 0, 0, 0x0080);
    configure_active_line(&mut two, 1, 0, 0x0080);
    set_background_pixel(&mut one, 0);
    set_background_pixel(&mut two, 1);
    let display_one = next_active(&mut one);
    let display_two = next_active(&mut two);
    assert_eq!(display_one.source_start(), 8);
    assert_eq!(display_two.source_start(), 16);

    let mut vpc = HuC6202::new();
    vpc.write_port(VpcPort::from_offset(1), 0x20);
    let mut vce = HuC6260::new();
    write_color(&mut vce, 0x100, 0x0007);
    write_color(&mut vce, 0x11, 0x01C0);
    let mut frame = PceActiveOnlyVideoFrame::new();
    frame
        .render_supergrafx_active_line(
            &mut one,
            &mut two,
            &vpc,
            &vce,
            Some(display_one),
            Some(display_two),
            0,
            VcePixelClock::DivideByFour,
        )
        .unwrap();

    let metadata = frame.row_metadata(0).unwrap();
    assert_eq!(metadata.active_x_origin(), 8);
    assert_eq!(metadata.active_width(), 16);
    assert_eq!(frame_pixel(&frame, 0, 0), [0, 0, 255, 255]);
    assert_eq!(frame_pixel(&frame, 8, 0), [0, 255, 0, 255]);
}

#[test]
fn supergrafx_renderer_uses_blank_bus_outside_unequal_and_inactive_spans() {
    let mut one = HuC6270::new();
    let mut two = HuC6270::new();
    configure_active_line(&mut one, 0, 0, 0x0080);
    configure_active_line(&mut two, 0, 1, 0x0080);
    set_background_pixel(&mut one, 0);
    let display_one = next_active(&mut one);
    let display_two = next_active(&mut two);
    let mut vpc = HuC6202::new();
    vpc.write_port(VpcPort::from_offset(1), 0x10);
    let mut vce = HuC6260::new();
    write_color(&mut vce, 1, 0x0038);
    write_color(&mut vce, 0x100, 0x0007);
    let mut frame = PceActiveOnlyVideoFrame::new();
    frame
        .render_supergrafx_active_line(
            &mut one,
            &mut two,
            &vpc,
            &vce,
            Some(display_one),
            Some(display_two),
            0,
            VcePixelClock::DivideByFour,
        )
        .unwrap();
    assert_eq!(frame.row_metadata(0).unwrap().active_width(), 16);
    assert_eq!(frame_pixel(&frame, 0, 0), [255, 0, 0, 255]);
    assert_eq!(frame_pixel(&frame, 8, 0), [0, 0, 255, 255]);

    vpc.write_port(VpcPort::from_offset(1), 0x20);
    frame.begin_frame();
    frame
        .render_supergrafx_active_line(
            &mut one,
            &mut two,
            &vpc,
            &vce,
            Some(display_one),
            None,
            0,
            VcePixelClock::DivideByFour,
        )
        .unwrap();
    assert_eq!(frame.row_metadata(0).unwrap().active_width(), 8);
    assert_eq!(frame_pixel(&frame, 0, 0), [0, 0, 255, 255]);
}

#[test]
fn supergrafx_renderer_applies_vpc_windows_in_common_selected_dot_coordinates() {
    let mut one = HuC6270::new();
    let mut two = HuC6270::new();
    configure_active_line(&mut one, 0, 1, 0x0080);
    configure_active_line(&mut two, 0, 1, 0x0080);
    set_background_pixel(&mut one, 0);
    set_background_pixel(&mut two, 1);
    one.vram_mut()[0x10] = 0x00FF;
    two.vram_mut()[0x10] = 0x00FF;
    let display_one = next_active(&mut one);
    let display_two = next_active(&mut two);
    let mut vpc = HuC6202::new();
    vpc.write_port(VpcPort::from_offset(0), 0x02);
    vpc.write_port(VpcPort::from_offset(1), 0x10);
    set_window(&mut vpc, 2, 0x49);
    set_window(&mut vpc, 4, 0x49);
    let mut vce = HuC6260::new();
    write_color(&mut vce, 1, 0x0038);
    write_color(&mut vce, 0x11, 0x01C0);
    let mut frame = PceActiveOnlyVideoFrame::new();

    frame
        .render_supergrafx_active_line(
            &mut one,
            &mut two,
            &vpc,
            &vce,
            Some(display_one),
            Some(display_two),
            0,
            VcePixelClock::DivideByFour,
        )
        .unwrap();

    assert_eq!(frame.row_metadata(0).unwrap().active_x_origin(), 8);
    assert_eq!(frame_pixel(&frame, 0, 0), [0, 255, 0, 255]);
    assert_eq!(frame_pixel(&frame, 1, 0), [0, 255, 0, 255]);
    assert_eq!(frame_pixel(&frame, 2, 0), [255, 0, 0, 255]);
}

#[test]
fn supergrafx_renderer_is_transactional_when_vdc_two_span_is_unavailable() {
    let mut one = HuC6270::new();
    let mut two = HuC6270::new();
    configure_active_line(&mut one, 0, 0, 0x0043);
    configure_active_line(&mut two, 0x7F, 0, 0);
    load_collision_overflow_satb(&mut one);
    let display_one = next_active(&mut one);
    let display_two = next_active(&mut two);
    let mut frame = PceActiveOnlyVideoFrame::new();

    assert_eq!(
        frame.render_supergrafx_active_line(
            &mut one,
            &mut two,
            &HuC6202::new(),
            &HuC6260::new(),
            Some(display_one),
            Some(display_two),
            0,
            VcePixelClock::DivideByFour,
        ),
        Err(PceVideoRenderError::ActiveSpanOutOfBounds {
            vdc: VpcVdc::Two,
            start: 1024,
            width: 8,
        })
    );
    assert_eq!(frame.row_metadata(0).unwrap().pixel_clock(), None);
    assert_eq!(frame_pixel(&frame, 0, 0), PCE_ACTIVE_FRAME_UNUSED_RGBA);
    assert!(
        !one.status()
            .intersects(VdcStatus::SPRITE_COLLISION | VdcStatus::SPRITE_OVERFLOW)
    );
}

#[test]
fn hidden_vdc_sprite_events_latch_after_successful_dual_composition() {
    let mut one = HuC6270::new();
    let mut two = HuC6270::new();
    configure_active_line(&mut one, 0, 0, 0x0043);
    configure_active_line(&mut two, 0, 0, 0);
    load_collision_overflow_satb(&mut one);
    let display_one = next_active(&mut one);
    let display_two = next_active(&mut two);
    let mut vpc = HuC6202::new();
    vpc.write_port(VpcPort::from_offset(1), 0x20);

    PceActiveOnlyVideoFrame::new()
        .render_supergrafx_active_line(
            &mut one,
            &mut two,
            &vpc,
            &HuC6260::new(),
            Some(display_one),
            Some(display_two),
            0,
            VcePixelClock::DivideByFour,
        )
        .unwrap();
    assert!(one.status().contains(VdcStatus::SPRITE_COLLISION));
    assert!(one.status().contains(VdcStatus::SPRITE_OVERFLOW));
    assert_eq!(one.irq_level(), LineLevel::Low);
    assert_eq!(two.irq_level(), LineLevel::High);
}

#[test]
fn public_supergrafx_descriptor_selects_a_revision_and_32k_work_ram() {
    const MADOU_SHA256: [u8; 32] = [
        0x9B, 0x57, 0xCD, 0xF0, 0xD0, 0xB1, 0x10, 0xF4, 0x12, 0x8B, 0x86, 0x34, 0x19, 0xD5, 0xBE,
        0x99, 0xA3, 0x70, 0x8B, 0xFB, 0x11, 0xCF, 0xBE, 0x16, 0x96, 0xF2, 0x54, 0x49, 0xB9, 0x91,
        0x02, 0x6D,
    ];
    let descriptor = PceCartridgeDescriptor::from_sha256(MADOU_SHA256);
    assert_eq!(
        descriptor.required_hardware(),
        PceCartridgeHardware::SuperGrafx
    );
    let machine = PceMachine::with_cartridge(rom_with_program(&[0xEA]), descriptor).unwrap();
    assert_eq!(machine.hardware_topology(), PceHardwareTopology::SuperGrafx);
    assert_eq!(machine.devices().psg().revision(), PsgRevision::HuC6280A);
    assert_eq!(machine.mapped_work_ram().len(), SUPERGRAFX_WORK_RAM_LEN);

    let forced =
        PceCartridgeDescriptor::default().with_required_hardware(PceCartridgeHardware::SuperGrafx);
    assert!(PceMachine::with_cartridge(rom_with_program(&[0xEA]), forced).is_ok());
}

#[test]
fn dual_vram_dma_advances_independently_on_the_same_slot() {
    let mut devices = devices();
    devices.vdc_mut().vram_mut()[0x0100] = 0x1111;
    queue_dma(devices.vdc_mut(), 0x0100, 0x0200, 1);
    let vdc2 = devices.supergrafx_video_mut().unwrap().vdc2_mut();
    vdc2.vram_mut()[0x0100] = 0x2222;
    queue_dma(vdc2, 0x0100, 0x0200, 1);

    let (first, second) = devices.advance_horizontal_pixels(4).unwrap();
    assert_eq!(first.vram_words(), 1);
    assert_eq!(second.unwrap().vram_words(), 1);
    assert_eq!(devices.vdc().vram()[0x0200], 0x1111);
    assert_eq!(
        devices.vdc().active_vram_dma().unwrap().remaining_words(),
        1
    );
    let vdc2 = devices.supergrafx_video().unwrap().vdc2();
    assert_eq!(vdc2.vram()[0x0200], 0x2222);
    assert_eq!(vdc2.active_vram_dma().unwrap().remaining_words(), 1);
}

#[test]
fn dual_vram_dma_completes_with_independent_status_and_irq() {
    let mut devices = devices();
    write_register(devices.vdc_mut(), VdcRegister::DmaControl, 0x0002);
    devices.vdc_mut().vram_mut()[0x0100] = 0x1111;
    queue_dma(devices.vdc_mut(), 0x0100, 0x0200, 0);
    let vdc2 = devices.supergrafx_video_mut().unwrap().vdc2_mut();
    write_register(vdc2, VdcRegister::DmaControl, 0x0002);
    vdc2.vram_mut()[0x0100] = 0x2222;
    queue_dma(vdc2, 0x0100, 0x0200, 0);

    let (first, second) = devices.advance_horizontal_pixels(4).unwrap();
    assert_eq!(first.vram_words(), 1);
    assert_eq!(second.unwrap().vram_words(), 1);
    assert_eq!(devices.vdc().vram()[0x0200], 0x1111);
    assert!(
        devices
            .vdc()
            .status()
            .contains(VdcStatus::VRAM_DMA_COMPLETE)
    );
    let vdc2 = devices.supergrafx_video().unwrap().vdc2();
    assert_eq!(vdc2.vram()[0x0200], 0x2222);
    assert!(vdc2.status().contains(VdcStatus::VRAM_DMA_COMPLETE));
    assert_eq!(devices.vdc_irq_level(), LineLevel::Low);

    devices.vdc_mut().read_port(VdcPort::SelectOrStatus);
    assert_eq!(devices.vdc_irq_level(), LineLevel::Low);
    devices
        .supergrafx_video_mut()
        .unwrap()
        .vdc2_mut()
        .read_port(VdcPort::SelectOrStatus);
    assert_eq!(devices.vdc_irq_level(), LineLevel::High);
}

#[test]
fn machine_direct_vdc_contention_uses_the_vpc_selected_vdc() {
    let mut machine =
        PceMachine::with_supergrafx_substrate_for_test(rom_with_program(&[0x13, 0x34, 0x23, 0x12]))
            .unwrap();
    let video = machine.devices_mut().supergrafx_video_mut().unwrap();
    write_register(video.vdc2_mut(), VdcRegister::VramData, 0);
    video.vdc2_mut().vram_mut()[0x0100..0x0120].fill(0xBEEF);
    write_register(video.vdc2_mut(), VdcRegister::DmaSource, 0x0100);
    write_register(video.vdc2_mut(), VdcRegister::DmaDestination, 0x0200);
    write_register(video.vdc2_mut(), VdcRegister::DmaLength, 31);
    video
        .vdc2_mut()
        .write_port(VdcPort::SelectOrStatus, VdcRegister::VramData as u8);
    video.vpc_mut().write_port(VpcPort::from_offset(6), 1);

    let low = machine.step_boundary().unwrap();
    assert_eq!(low.vram_contention_wait_cycles(), 0);
    let high = machine.step_boundary().unwrap();
    assert!(high.vram_contention_wait_cycles() != 0);
    assert_eq!(
        machine.devices().supergrafx_video().unwrap().vdc2().vram()[1],
        0x1234
    );
    assert_eq!(machine.devices().vdc().vram()[1], 0);
}

#[test]
fn machine_mapped_vdc2_high_write_contends_only_with_vdc2_dma() {
    let mut machine =
        PceMachine::with_supergrafx_substrate_for_test(rom_with_program(&[0x8D, 0x13, 0x00]))
            .unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine.cpu_mut().cpu_mut().registers_mut().a = 0x78;
    let video = machine.devices_mut().supergrafx_video_mut().unwrap();
    write_register(video.vdc2_mut(), VdcRegister::VramData, 0x0056);
    video.vdc2_mut().vram_mut()[0x0100..0x0120].fill(0xBEEF);
    write_register(video.vdc2_mut(), VdcRegister::DmaSource, 0x0100);
    write_register(video.vdc2_mut(), VdcRegister::DmaDestination, 0x0200);
    write_register(video.vdc2_mut(), VdcRegister::DmaLength, 31);
    video
        .vdc2_mut()
        .write_port(VdcPort::SelectOrStatus, VdcRegister::VramData as u8);

    let step = machine.step_boundary().unwrap();
    assert!(step.vram_contention_wait_cycles() != 0);
    assert_eq!(
        machine.devices().supergrafx_video().unwrap().vdc2().vram()[1],
        0x7856
    );
    assert_eq!(machine.devices().vdc().vram()[1], 0);
}

#[test]
fn machine_vdc_access_does_not_contend_with_the_other_vdc_dma() {
    let mut direct =
        PceMachine::with_supergrafx_substrate_for_test(rom_with_program(&[0x23, 0x12])).unwrap();
    write_register(
        direct.devices_mut().vdc_mut(),
        VdcRegister::VramData,
        0x0034,
    );
    direct
        .devices_mut()
        .vdc_mut()
        .write_port(VdcPort::SelectOrStatus, VdcRegister::VramData as u8);
    let vdc2 = direct
        .devices_mut()
        .supergrafx_video_mut()
        .unwrap()
        .vdc2_mut();
    vdc2.vram_mut()[0x0100..0x0120].fill(0xBEEF);
    queue_dma(vdc2, 0x0100, 0x0200, 31);
    let step = direct.step_boundary().unwrap();
    assert_eq!(step.vram_contention_wait_cycles(), 0);
    assert_eq!(direct.devices().vdc().vram()[1], 0x1234);

    let mut mapped =
        PceMachine::with_supergrafx_substrate_for_test(rom_with_program(&[0x8D, 0x13, 0x00]))
            .unwrap();
    mapped.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    mapped.cpu_mut().cpu_mut().registers_mut().a = 0x78;
    let vdc1 = mapped.devices_mut().vdc_mut();
    vdc1.vram_mut()[0x0100..0x0120].fill(0xBEEF);
    queue_dma(vdc1, 0x0100, 0x0200, 31);
    let vdc2 = mapped
        .devices_mut()
        .supergrafx_video_mut()
        .unwrap()
        .vdc2_mut();
    write_register(vdc2, VdcRegister::VramData, 0x0056);
    vdc2.write_port(VdcPort::SelectOrStatus, VdcRegister::VramData as u8);
    let step = mapped.step_boundary().unwrap();
    assert_eq!(step.vram_contention_wait_cycles(), 0);
    assert_eq!(
        mapped.devices().supergrafx_video().unwrap().vdc2().vram()[1],
        0x7856
    );
}

#[test]
fn machine_vce_conversion_feeds_both_vdcs_from_one_fractional_remainder() {
    let mut machine =
        PceMachine::with_supergrafx_substrate_for_test(rom_with_program(&[0xEA])).unwrap();
    assert_eq!(machine.advance_devices_for_test(11).unwrap(), (0, 0));
    assert_eq!(machine.vdc_pixel_clock_remainder(), 3);
    assert_eq!(machine.devices().vdc().dma_pixel_remainder(), 2);
    assert_eq!(
        machine
            .devices()
            .supergrafx_video()
            .unwrap()
            .vdc2()
            .dma_pixel_remainder(),
        2
    );

    assert_eq!(machine.advance_devices_for_test(1).unwrap(), (0, 0));
    assert_eq!(machine.vdc_pixel_clock_remainder(), 0);
    assert_eq!(machine.devices().vdc().dma_pixel_remainder(), 3);
    assert_eq!(
        machine
            .devices()
            .supergrafx_video()
            .unwrap()
            .vdc2()
            .dma_pixel_remainder(),
        3
    );
}
