use super::cpu::{LineLevel, VdcPort};
use super::{
    BaseBus, DETERMINISTIC_VDC_INITIAL_SATB_WORD, DETERMINISTIC_VDC_RESET_CLEARS_SATB, HuC6270,
    VDC_SATB_WORDS, VdcDmaChannel, VdcDmaDirection, VdcDmaProgress, VdcRegister, VdcStatus,
};

fn select(vdc: &mut HuC6270, register: VdcRegister) {
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
}

fn write_register(vdc: &mut HuC6270, register: VdcRegister, value: u16) {
    select(vdc, register);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

#[test]
fn dma_commands_queue_only_on_high_byte_and_snapshot_the_transfer() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::DmaControl, 0x000C);
    write_register(&mut vdc, VdcRegister::DmaSource, 0x1234);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x5678);

    select(&mut vdc, VdcRegister::DmaLength);
    vdc.write_port(VdcPort::DataLow, 0x02);
    assert_eq!(vdc.pending_vram_dma(), None);
    vdc.write_port(VdcPort::DataHigh, 0x00);

    let pending = vdc.pending_vram_dma().unwrap();
    assert_eq!(pending.source(), 0x1234);
    assert_eq!(pending.destination(), 0x5678);
    assert_eq!(pending.remaining_words(), 3);
    assert_eq!(pending.source_direction(), VdcDmaDirection::Decrement);
    assert_eq!(pending.destination_direction(), VdcDmaDirection::Decrement);

    write_register(&mut vdc, VdcRegister::DmaSource, 0x1111);
    assert_eq!(vdc.pending_vram_dma(), Some(pending));
    assert!(vdc.activate_pending_vram_dma());
    assert!(!vdc.activate_pending_vram_dma());
    assert_eq!(vdc.pending_vram_dma(), None);
    assert_eq!(vdc.active_vram_dma(), Some(pending));

    let mut maximum = HuC6270::new();
    write_register(&mut maximum, VdcRegister::DmaLength, 0xFFFF);
    assert_eq!(
        maximum.pending_vram_dma().unwrap().remaining_words(),
        65_536
    );
}

#[test]
fn vram_dma_services_one_word_per_slot_and_updates_final_registers() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0x0100..=0x0102].copy_from_slice(&[0x1111, 0x2222, 0x3333]);
    write_register(&mut vdc, VdcRegister::DmaControl, 0x000A);
    write_register(&mut vdc, VdcRegister::DmaSource, 0x0100);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x0202);
    write_register(&mut vdc, VdcRegister::DmaLength, 2);
    assert!(vdc.activate_pending_vram_dma());

    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Transferred { remaining_words: 2 })
    );
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Transferred { remaining_words: 1 })
    );
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Complete)
    );
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Idle)
    );

    assert_eq!(vdc.vram()[0x0200..=0x0202], [0x3333, 0x2222, 0x1111]);
    assert_eq!(vdc.register(VdcRegister::DmaSource), 0x0103);
    assert_eq!(vdc.register(VdcRegister::DmaDestination), 0x01FF);
    assert_eq!(vdc.register(VdcRegister::DmaLength), 0xFFFF);
    assert_eq!(vdc.active_vram_dma(), None);
    assert!(vdc.status().contains(VdcStatus::VRAM_DMA_COMPLETE));
    assert_eq!(vdc.irq_level(), LineLevel::Low);
    assert_eq!(
        vdc.read_port(VdcPort::SelectOrStatus),
        VdcStatus::VRAM_DMA_COMPLETE.bits()
    );
    assert_eq!(vdc.irq_level(), LineLevel::High);
}

#[test]
fn overlapping_vram_dma_reads_each_word_before_writing_it() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0x0100..=0x0103].copy_from_slice(&[1, 2, 3, 4]);
    write_register(&mut vdc, VdcRegister::DmaSource, 0x0100);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x0101);
    write_register(&mut vdc, VdcRegister::DmaLength, 2);
    assert!(vdc.activate_pending_vram_dma());

    for _ in 0..3 {
        vdc.service_dma_slot(VdcDmaChannel::Vram).unwrap();
    }

    assert_eq!(vdc.vram()[0x0100..=0x0103], [1, 1, 1, 1]);
}

#[test]
fn zero_length_means_one_word_and_addresses_wrap() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0] = 0xCAFE;
    write_register(&mut vdc, VdcRegister::DmaControl, 0x000C);
    write_register(&mut vdc, VdcRegister::DmaSource, 0);
    write_register(&mut vdc, VdcRegister::DmaDestination, 1);
    write_register(&mut vdc, VdcRegister::DmaLength, 0);
    assert!(vdc.activate_pending_vram_dma());

    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Complete)
    );
    assert_eq!(vdc.vram()[1], 0xCAFE);
    assert_eq!(vdc.register(VdcRegister::DmaSource), 0xFFFF);
    assert_eq!(vdc.register(VdcRegister::DmaDestination), 0);
    assert_eq!(vdc.register(VdcRegister::DmaLength), 0xFFFF);
    assert_eq!(vdc.status(), VdcStatus::empty());

    write_register(&mut vdc, VdcRegister::DmaControl, 0x0002);
    assert_eq!(vdc.irq_level(), LineLevel::High);
}

#[test]
fn upper_vram_dma_sources_mirror_and_destinations_are_ignored_while_registers_advance() {
    let mut source_mirror = HuC6270::new();
    source_mirror.vram_mut()[0] = 0x1111;
    write_register(&mut source_mirror, VdcRegister::DmaControl, 0x0002);
    write_register(&mut source_mirror, VdcRegister::DmaSource, 0x8000);
    write_register(&mut source_mirror, VdcRegister::DmaDestination, 1);
    write_register(&mut source_mirror, VdcRegister::DmaLength, 0);
    assert!(source_mirror.activate_pending_vram_dma());
    assert_eq!(
        source_mirror.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Complete)
    );
    assert_eq!(source_mirror.vram()[1], 0x1111);
    assert_eq!(source_mirror.register(VdcRegister::DmaSource), 0x8001);
    assert_eq!(source_mirror.register(VdcRegister::DmaDestination), 2);
    assert_eq!(source_mirror.register(VdcRegister::DmaLength), 0xFFFF);
    assert!(
        source_mirror
            .status()
            .contains(VdcStatus::VRAM_DMA_COMPLETE)
    );

    let mut ignored_destination = HuC6270::new();
    ignored_destination.vram_mut()[0] = 0x2222;
    write_register(&mut ignored_destination, VdcRegister::DmaSource, 0);
    write_register(
        &mut ignored_destination,
        VdcRegister::DmaDestination,
        0x8001,
    );
    write_register(&mut ignored_destination, VdcRegister::DmaLength, 0);
    assert!(ignored_destination.activate_pending_vram_dma());
    assert_eq!(
        ignored_destination.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Complete)
    );
    assert_eq!(ignored_destination.vram()[1], 0);
    assert_eq!(ignored_destination.register(VdcRegister::DmaSource), 1);
    assert_eq!(
        ignored_destination.register(VdcRegister::DmaDestination),
        0x8002
    );
    assert_eq!(ignored_destination.register(VdcRegister::DmaLength), 0xFFFF);
}

#[test]
fn active_display_abort_is_separate_and_has_no_completion_event() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0x10] = 0xAAAA;
    vdc.vram_mut()[0x11] = 0xBBBB;
    write_register(&mut vdc, VdcRegister::DmaControl, 0x0002);
    write_register(&mut vdc, VdcRegister::DmaSource, 0x10);
    write_register(&mut vdc, VdcRegister::DmaDestination, 0x20);
    write_register(&mut vdc, VdcRegister::DmaLength, 1);
    assert!(vdc.activate_pending_vram_dma());
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Transferred { remaining_words: 1 })
    );

    assert!(vdc.abort_vram_dma_for_active_display());
    assert!(!vdc.abort_vram_dma_for_active_display());
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Idle)
    );
    assert_eq!(vdc.vram()[0x20], 0xAAAA);
    assert_eq!(vdc.vram()[0x21], 0);
    assert_eq!(vdc.register(VdcRegister::DmaLength), 0);
    assert_eq!(vdc.status(), VdcStatus::empty());
}

#[test]
fn satb_dma_waits_for_vertical_blank_and_auto_repeats() {
    let mut vdc = HuC6270::new();
    for index in 0..VDC_SATB_WORDS {
        vdc.vram_mut()[0x0100 + index] = index as u16;
    }
    write_register(&mut vdc, VdcRegister::DmaControl, 0x0010);
    select(&mut vdc, VdcRegister::SatbSource);
    vdc.write_port(VdcPort::DataLow, 0x00);
    assert_eq!(vdc.pending_satb_dma(), None);
    vdc.write_port(VdcPort::DataHigh, 0x01);
    assert_eq!(vdc.pending_satb_dma().unwrap().source(), 0x0100);
    assert!(vdc.start_satb_dma_for_vertical_blank());
    assert!(!vdc.start_satb_dma_for_vertical_blank());

    for remaining in (1..VDC_SATB_WORDS).rev() {
        assert_eq!(
            vdc.service_dma_slot(VdcDmaChannel::Satb),
            Ok(VdcDmaProgress::Transferred {
                remaining_words: remaining as u32,
            })
        );
    }
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Satb),
        Ok(VdcDmaProgress::Complete)
    );
    assert_eq!(vdc.satb().as_slice(), &(0..256).collect::<Vec<u16>>());
    assert_eq!(vdc.status(), VdcStatus::empty());
    assert_eq!(vdc.pending_satb_dma(), None);

    write_register(&mut vdc, VdcRegister::DmaControl, 0x0011);
    assert_eq!(vdc.irq_level(), LineLevel::High);
    vdc.vram_mut()[0x0100] = 0xCAFE;
    assert!(vdc.start_satb_dma_for_vertical_blank());
    assert_eq!(vdc.pending_satb_dma(), None);
    assert_eq!(vdc.active_satb_dma().unwrap().source(), 0x0100);
    for _ in 0..VDC_SATB_WORDS - 1 {
        assert!(matches!(
            vdc.service_dma_slot(VdcDmaChannel::Satb),
            Ok(VdcDmaProgress::Transferred { .. })
        ));
    }
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Satb),
        Ok(VdcDmaProgress::Complete)
    );
    assert_eq!(vdc.satb()[0], 0xCAFE);
    assert!(vdc.status().contains(VdcStatus::SATB_DMA_COMPLETE));
    assert_eq!(vdc.irq_level(), LineLevel::Low);
}

#[test]
fn satb_dma_mirrors_upper_vram_sources() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0x7FFF] = 0xCAFE;
    vdc.vram_mut()[0] = 0xBEEF;
    write_register(&mut vdc, VdcRegister::DmaControl, 0x0001);
    write_register(&mut vdc, VdcRegister::SatbSource, 0x7FFF);
    assert!(vdc.start_satb_dma_for_vertical_blank());
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Satb),
        Ok(VdcDmaProgress::Transferred {
            remaining_words: 255,
        })
    );
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Satb),
        Ok(VdcDmaProgress::Transferred {
            remaining_words: 254,
        })
    );
    assert_eq!(vdc.satb()[0], 0xCAFE);
    assert_eq!(vdc.satb()[1], 0xBEEF);
    assert_eq!(vdc.active_satb_dma().unwrap().next_word(), 2);
    assert_eq!(vdc.status(), VdcStatus::empty());
    assert_eq!(vdc.register(VdcRegister::SatbSource), 0x7FFF);
}

#[test]
fn scheduler_channel_selection_services_only_the_chosen_dma() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0] = 0x1111;
    vdc.vram_mut()[0x0100] = 0x2222;
    write_register(&mut vdc, VdcRegister::DmaSource, 0);
    write_register(&mut vdc, VdcRegister::DmaDestination, 1);
    write_register(&mut vdc, VdcRegister::DmaLength, 0);
    assert!(vdc.activate_pending_vram_dma());
    write_register(&mut vdc, VdcRegister::SatbSource, 0x0100);
    assert!(vdc.start_satb_dma_for_vertical_blank());

    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Satb),
        Ok(VdcDmaProgress::Transferred {
            remaining_words: 255,
        })
    );
    assert_eq!(vdc.satb()[0], 0x2222);
    assert_eq!(vdc.vram()[1], 0);
    assert_eq!(vdc.active_vram_dma().unwrap().remaining_words(), 1);

    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Vram),
        Ok(VdcDmaProgress::Complete)
    );
    assert_eq!(vdc.vram()[1], 0x1111);
    assert_eq!(vdc.active_satb_dma().unwrap().remaining_words(), 255);
}

#[test]
fn reset_clears_satb_and_all_transfer_state_by_named_policy() {
    let mut vdc = HuC6270::new();
    vdc.vram_mut()[0] = 0xABCD;
    write_register(&mut vdc, VdcRegister::DmaSource, 0);
    write_register(&mut vdc, VdcRegister::DmaDestination, 1);
    write_register(&mut vdc, VdcRegister::DmaLength, 1);
    assert!(vdc.activate_pending_vram_dma());
    write_register(&mut vdc, VdcRegister::SatbSource, 0);
    assert!(vdc.start_satb_dma_for_vertical_blank());
    assert!(matches!(
        vdc.service_dma_slot(VdcDmaChannel::Satb),
        Ok(VdcDmaProgress::Transferred { .. })
    ));
    assert_eq!(vdc.satb()[0], 0xABCD);

    vdc.reset();

    assert_eq!(
        vdc.satb()
            .iter()
            .all(|word| *word == DETERMINISTIC_VDC_INITIAL_SATB_WORD),
        DETERMINISTIC_VDC_RESET_CLEARS_SATB
    );
    assert_eq!(vdc.pending_vram_dma(), None);
    assert_eq!(vdc.active_vram_dma(), None);
    assert_eq!(vdc.pending_satb_dma(), None);
    assert_eq!(vdc.active_satb_dma(), None);
}

#[test]
fn mirrored_base_bus_ports_queue_dma_commands() {
    let mut bus = BaseBus::new(Vec::new(), HuC6270::new()).unwrap();
    bus.write(0x1F_E3FC, VdcRegister::DmaLength as u8);
    bus.write(0x1F_E102, 0x34);
    assert_eq!(bus.devices().pending_vram_dma(), None);
    bus.write(0x1F_E003, 0x12);

    assert_eq!(
        bus.devices().pending_vram_dma().unwrap().remaining_words(),
        0x1235
    );
}
