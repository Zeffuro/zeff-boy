use super::cpu::{Cpu, LineLevel, VdcPort};
use super::vdc::{
    DETERMINISTIC_VDC_RESET_HORIZONTAL_DISPLAY, DETERMINISTIC_VDC_RESET_VERTICAL_DISPLAY,
};
use super::{
    BaseBus, DETERMINISTIC_VDC_INITIAL_VRAM_WORD, DETERMINISTIC_VDC_RESET_PRESERVES_VRAM,
    DETERMINISTIC_VDC_RESET_VALUE, HuC6270, VDC_UNAVAILABLE_READ_VALUE, VdcRegister, VdcStatus,
};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn select(vdc: &mut HuC6270, register: VdcRegister) {
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
}

fn write_register(vdc: &mut HuC6270, register: VdcRegister, value: u16) {
    select(vdc, register);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

#[test]
fn unused_port_reads_zero_without_clearing_status_or_changing_selection() {
    let mut vdc = HuC6270::new();
    select(&mut vdc, VdcRegister::Control);
    vdc.latch_status(VdcStatus::VERTICAL_BLANK);

    assert_eq!(vdc.read_port(VdcPort::Unused), 0);
    assert_eq!(vdc.status(), VdcStatus::VERTICAL_BLANK);

    vdc.write_port(VdcPort::Unused, 0xFF);
    assert_eq!(vdc.selected_register(), Some(VdcRegister::Control));
    assert_eq!(
        vdc.read_port(VdcPort::SelectOrStatus),
        VdcStatus::VERTICAL_BLANK.bits()
    );
}

#[test]
fn register_selection_masks_the_id_and_invalid_registers_are_unavailable() {
    let mut vdc = HuC6270::new();
    let writable_registers = [
        (VdcRegister::Control, 0x1FFF),
        (VdcRegister::RasterCounter, 0x03FF),
        (VdcRegister::BackgroundScrollX, 0x03FF),
        (VdcRegister::BackgroundScrollY, 0x01FF),
        (VdcRegister::MemoryWidth, 0x00FF),
        (VdcRegister::HorizontalSync, 0x7F1F),
        (VdcRegister::HorizontalDisplay, 0x7F7F),
        (VdcRegister::VerticalSync, 0xFF1F),
        (VdcRegister::VerticalDisplay, 0x01FF),
        (VdcRegister::VerticalDisplayEnd, 0x00FF),
        (VdcRegister::DmaControl, 0x001F),
        (VdcRegister::DmaSource, 0xFFFF),
        (VdcRegister::DmaDestination, 0xFFFF),
        (VdcRegister::DmaLength, 0xFFFF),
        (VdcRegister::SatbSource, 0xFFFF),
    ];

    for (register, mask) in writable_registers {
        vdc.write_port(VdcPort::SelectOrStatus, 0xE0 | register as u8);
        assert_eq!(vdc.selected_register(), Some(register));
        vdc.write_port(VdcPort::DataLow, 0xAA);
        assert_eq!(vdc.register(register), 0x00AA & mask);
        vdc.write_port(VdcPort::DataHigh, 0xFF);
        assert_eq!(vdc.register(register), 0xFFAA & mask);
        vdc.write_port(VdcPort::DataLow, 0x55);
        assert_eq!(vdc.register(register), 0xFF55 & mask);
        assert_eq!(vdc.read_port(VdcPort::DataLow), VDC_UNAVAILABLE_READ_VALUE);
        assert_eq!(vdc.read_port(VdcPort::DataHigh), VDC_UNAVAILABLE_READ_VALUE);
    }

    let control = vdc.register(VdcRegister::Control);
    for id in [0x03, 0x04, 0x14, 0x1F] {
        vdc.write_port(VdcPort::SelectOrStatus, id);
        assert_eq!(vdc.selected_register_id(), id);
        assert_eq!(vdc.selected_register(), None);
        vdc.write_port(VdcPort::DataLow, 0x12);
        vdc.write_port(VdcPort::DataHigh, 0x34);
        assert_eq!(vdc.register(VdcRegister::Control), control);
        assert_eq!(vdc.read_port(VdcPort::DataLow), VDC_UNAVAILABLE_READ_VALUE);
        assert_eq!(vdc.read_port(VdcPort::DataHigh), VDC_UNAVAILABLE_READ_VALUE);
    }
}

#[test]
fn vram_write_latch_commits_and_increments_only_on_high_byte_writes() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::MemoryAddressWrite, 0x1234);
    select(&mut vdc, VdcRegister::VramData);

    vdc.write_port(VdcPort::DataLow, 0x34);
    assert_eq!(vdc.vram()[0x1234], 0);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressWrite), 0x1234);

    vdc.write_port(VdcPort::DataHigh, 0x12);
    assert_eq!(vdc.vram()[0x1234], 0x1234);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressWrite), 0x1235);

    vdc.write_port(VdcPort::DataHigh, 0xAB);
    assert_eq!(vdc.vram()[0x1235], 0xAB34);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressWrite), 0x1236);
    vdc.write_port(VdcPort::DataLow, 0xCD);
    assert_eq!(vdc.vram()[0x1236], 0);
}

#[test]
fn marr_high_prefetches_then_increments_and_vrr_high_fetches_the_next_word() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0x1234] = 0xA1B2;
    vdc.vram_mut()[0x1235] = 0xC3D4;

    select(&mut vdc, VdcRegister::MemoryAddressRead);
    vdc.write_port(VdcPort::DataLow, 0x34);
    assert_eq!(vdc.vram_read_buffer(), 0);
    vdc.write_port(VdcPort::DataHigh, 0x12);
    assert_eq!(vdc.vram_read_buffer(), 0xA1B2);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressRead), 0x1235);

    select(&mut vdc, VdcRegister::VramData);
    assert_eq!(vdc.read_port(VdcPort::DataLow), 0xB2);
    assert_eq!(vdc.read_port(VdcPort::DataLow), 0xB2);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressRead), 0x1235);
    assert_eq!(vdc.read_port(VdcPort::DataHigh), 0xA1);
    assert_eq!(vdc.vram_read_buffer(), 0xC3D4);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressRead), 0x1236);
    assert_eq!(vdc.read_port(VdcPort::DataLow), 0xD4);
    assert_eq!(vdc.read_port(VdcPort::DataHigh), 0xC3);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressRead), 0x1237);
}

#[test]
fn control_register_selects_all_four_address_increments() {
    for (increment_bits, increment) in [(0, 1), (1, 0x20), (2, 0x40), (3, 0x80)] {
        let mut vdc = HuC6270::new();
        write_register(&mut vdc, VdcRegister::Control, increment_bits << 11);
        write_register(&mut vdc, VdcRegister::MemoryAddressWrite, 0x0100);
        write_register(&mut vdc, VdcRegister::VramData, 0xCAFE);
        assert_eq!(
            vdc.register(VdcRegister::MemoryAddressWrite),
            0x0100 + increment
        );

        vdc.vram_mut()[0x0200] = 0x1122;
        vdc.vram_mut()[0x0200 + usize::from(increment)] = 0x3344;
        write_register(&mut vdc, VdcRegister::MemoryAddressRead, 0x0200);
        assert_eq!(vdc.vram_read_buffer(), 0x1122);
        assert_eq!(
            vdc.register(VdcRegister::MemoryAddressRead),
            0x0200 + increment
        );
        select(&mut vdc, VdcRegister::VramData);
        assert_eq!(vdc.read_port(VdcPort::DataHigh), 0x11);
        assert_eq!(vdc.vram_read_buffer(), 0x3344);
        assert_eq!(
            vdc.register(VdcRegister::MemoryAddressRead),
            0x0200 + 2 * increment
        );
    }
}

#[test]
fn upper_logical_vram_reads_mirror_and_writes_are_dropped_while_addresses_advance() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0] = 0x1122;
    vdc.vram_mut()[1] = 0x3344;

    write_register(&mut vdc, VdcRegister::MemoryAddressRead, 0x8000);
    assert_eq!(vdc.vram_read_buffer(), 0x1122);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressRead), 0x8001);
    select(&mut vdc, VdcRegister::VramData);
    assert_eq!(vdc.read_port(VdcPort::DataLow), 0x22);
    assert_eq!(vdc.read_port(VdcPort::DataHigh), 0x11);
    assert_eq!(vdc.vram_read_buffer(), 0x3344);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressRead), 0x8002);

    write_register(&mut vdc, VdcRegister::MemoryAddressWrite, 0x8000);
    write_register(&mut vdc, VdcRegister::VramData, 0xDEAD);
    assert_eq!(vdc.vram()[0], 0x1122);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressWrite), 0x8001);
    write_register(&mut vdc, VdcRegister::MemoryAddressWrite, 0xFFFF);
    write_register(&mut vdc, VdcRegister::VramData, 0xBEEF);
    assert_eq!(vdc.vram()[0], 0x1122);
    assert_eq!(vdc.register(VdcRegister::MemoryAddressWrite), 0);
}

#[test]
fn status_read_clears_events_but_keeps_busy_and_updates_irq_level() {
    let mut vdc = HuC6270::new();
    vdc.latch_status(VdcStatus::all());
    assert!(!vdc.status().contains(VdcStatus::BUSY));
    vdc.set_busy(true);
    assert_eq!(vdc.irq_level(), LineLevel::High);
    write_register(&mut vdc, VdcRegister::Control, 0x000F);
    assert_eq!(vdc.irq_level(), LineLevel::Low);

    assert_eq!(vdc.read_port(VdcPort::DataLow), VDC_UNAVAILABLE_READ_VALUE);
    assert_eq!(vdc.irq_level(), LineLevel::Low);
    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 0x7F);
    assert_eq!(vdc.status(), VdcStatus::BUSY);
    assert_eq!(vdc.irq_level(), LineLevel::High);
    assert_eq!(vdc.read_port(VdcPort::SelectOrStatus), 0x40);

    for (event, enable) in [
        (VdcStatus::SPRITE_COLLISION, 0x01),
        (VdcStatus::SPRITE_OVERFLOW, 0x02),
        (VdcStatus::RASTER_MATCH, 0x04),
        (VdcStatus::VERTICAL_BLANK, 0x08),
    ] {
        let mut source = HuC6270::new();
        source.latch_status(event);
        assert_eq!(source.irq_level(), LineLevel::High);
        write_register(&mut source, VdcRegister::Control, enable);
        assert_eq!(source.irq_level(), LineLevel::Low);
    }

    for (event, enable) in [
        (VdcStatus::SATB_DMA_COMPLETE, 0x01),
        (VdcStatus::VRAM_DMA_COMPLETE, 0x02),
    ] {
        let mut source = HuC6270::new();
        source.latch_status(event);
        assert_eq!(source.irq_level(), LineLevel::High);
        write_register(&mut source, VdcRegister::DmaControl, enable);
        assert_eq!(source.irq_level(), LineLevel::Low);
        assert_eq!(source.read_port(VdcPort::SelectOrStatus), event.bits());
        assert_eq!(source.irq_level(), LineLevel::High);
    }
}

#[test]
fn reset_uses_named_internal_and_external_vram_policies() {
    let mut vdc = HuC6270::new();
    assert_eq!(
        vdc.register(VdcRegister::HorizontalDisplay),
        DETERMINISTIC_VDC_RESET_HORIZONTAL_DISPLAY
    );
    assert_eq!(
        vdc.register(VdcRegister::VerticalDisplay),
        DETERMINISTIC_VDC_RESET_VERTICAL_DISPLAY
    );
    assert_eq!(vdc.vram()[0], DETERMINISTIC_VDC_INITIAL_VRAM_WORD);
    assert_eq!(
        vdc.vram()[vdc.vram().len() - 1],
        DETERMINISTIC_VDC_INITIAL_VRAM_WORD
    );
    vdc.vram_mut()[0x1234] = 0xABCD;
    write_register(&mut vdc, VdcRegister::Control, 0x1FFF);
    vdc.latch_status(VdcStatus::VERTICAL_BLANK);
    vdc.set_busy(true);
    vdc.write_port(VdcPort::SelectOrStatus, 0x1F);

    vdc.reset();

    assert_eq!(
        vdc.vram()[0x1234] == 0xABCD,
        DETERMINISTIC_VDC_RESET_PRESERVES_VRAM
    );
    assert_eq!(vdc.selected_register_id(), 0);
    assert_eq!(
        vdc.selected_register(),
        Some(VdcRegister::MemoryAddressWrite)
    );
    assert_eq!(
        vdc.register(VdcRegister::Control),
        DETERMINISTIC_VDC_RESET_VALUE
    );
    assert_eq!(
        vdc.register(VdcRegister::HorizontalDisplay),
        DETERMINISTIC_VDC_RESET_HORIZONTAL_DISPLAY
    );
    assert_eq!(
        vdc.register(VdcRegister::VerticalDisplay),
        DETERMINISTIC_VDC_RESET_VERTICAL_DISPLAY
    );
    for register in [
        VdcRegister::MemoryAddressWrite,
        VdcRegister::MemoryAddressRead,
        VdcRegister::VramData,
        VdcRegister::Control,
        VdcRegister::RasterCounter,
        VdcRegister::BackgroundScrollX,
        VdcRegister::BackgroundScrollY,
        VdcRegister::MemoryWidth,
        VdcRegister::HorizontalSync,
        VdcRegister::VerticalSync,
        VdcRegister::VerticalDisplayEnd,
        VdcRegister::DmaControl,
        VdcRegister::DmaSource,
        VdcRegister::DmaDestination,
        VdcRegister::DmaLength,
        VdcRegister::SatbSource,
    ] {
        assert_eq!(vdc.register(register), DETERMINISTIC_VDC_RESET_VALUE);
    }
    assert_eq!(vdc.vram_read_buffer(), DETERMINISTIC_VDC_RESET_VALUE);
    assert_eq!(vdc.status(), VdcStatus::empty());
    assert_eq!(vdc.irq_level(), LineLevel::High);
}

#[test]
fn reset_register_state_round_trip_is_byte_deterministic() {
    let reset = HuC6270::new();
    let mut writer = StateWriter::new();
    reset.write_state(&mut writer);
    let reset_state = writer.into_bytes();

    let mut restored = HuC6270::new();
    write_register(&mut restored, VdcRegister::HorizontalDisplay, 0);
    write_register(&mut restored, VdcRegister::VerticalDisplay, 0);
    restored
        .read_state(&mut StateReader::new(&reset_state))
        .unwrap();
    assert_eq!(
        restored.register(VdcRegister::HorizontalDisplay),
        DETERMINISTIC_VDC_RESET_HORIZONTAL_DISPLAY
    );
    assert_eq!(
        restored.register(VdcRegister::VerticalDisplay),
        DETERMINISTIC_VDC_RESET_VERTICAL_DISPLAY
    );

    let mut restored_writer = StateWriter::new();
    restored.write_state(&mut restored_writer);
    assert_eq!(restored_writer.into_bytes(), reset_state);
}

#[test]
fn mirrored_base_bus_ports_route_to_the_vdc() {
    let mut bus = BaseBus::new(Vec::new(), HuC6270::new()).unwrap();
    bus.write(0x1F_E3FC, VdcRegister::MemoryAddressWrite as u8);
    bus.write(0x1F_E102, 0x34);
    bus.write(0x1F_E003, 0x12);
    bus.write(0x1F_E200, VdcRegister::VramData as u8);
    bus.write(0x1F_E3FE, 0xCD);
    bus.write(0x1F_E0FF, 0xAB);

    assert_eq!(bus.devices().vram()[0x1234], 0xABCD);
    assert_eq!(
        bus.devices().register(VdcRegister::MemoryAddressWrite),
        0x1235
    );
    bus.devices_mut().latch_status(VdcStatus::VERTICAL_BLANK);
    assert_eq!(bus.read(0x1F_E301), 0);
    assert_eq!(bus.devices().status(), VdcStatus::VERTICAL_BLANK);
    assert_eq!(bus.read(0x1F_E300), VdcStatus::VERTICAL_BLANK.bits());
    assert_eq!(bus.devices().status(), VdcStatus::empty());
}

#[test]
fn direct_vdc_store_instructions_drive_the_cpu_facing_ports() {
    let rom = vec![
        0x03,
        VdcRegister::MemoryAddressWrite as u8,
        0x13,
        0x01,
        0x23,
        0x00,
        0x03,
        VdcRegister::VramData as u8,
        0x13,
        0xCD,
        0x23,
        0xAB,
    ];
    let mut bus = BaseBus::new(rom, HuC6270::new()).unwrap();
    let mut cpu = Cpu::new();

    for _ in 0..6 {
        cpu.step(&mut bus).unwrap();
    }

    assert_eq!(bus.devices().vram()[1], 0xABCD);
    assert_eq!(bus.devices().register(VdcRegister::MemoryAddressWrite), 2);
}
