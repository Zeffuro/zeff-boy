use super::*;

fn make_fds() -> Fds {
    let bios = vec![0xFF; FDS_BIOS_SIZE];
    Fds::new(bios, Mirroring::Horizontal)
}

fn side(fill: u8) -> Vec<u8> {
    vec![fill; FDS_SIDE_SIZE]
}

fn clock_until_next_disk_byte(fds: &mut Fds) {
    let cycles = u64::from(fds.disk_media_change_counter)
        + u64::from(fds.disk_lead_in_counter)
        + u64::from(FDS_DISK_BYTE_PERIOD_CPU_CYCLES);
    for _ in 0..cycles {
        fds.clock_cpu();
    }
}

fn clock_until_side_change_settles(fds: &mut Fds) {
    let cycles = u64::from(fds.disk_media_change_counter);
    for _ in 0..cycles {
        fds.clock_cpu();
    }
}

#[test]
fn selecting_disk_side_reports_disk_change_transition() {
    let mut side_a = vec![0; FDS_SIDE_SIZE];
    side_a[0] = 0xA1;
    let mut side_b = vec![0; FDS_SIDE_SIZE];
    side_b[0] = 0xB2;
    let mut image_bytes = side_a;
    image_bytes.extend_from_slice(&side_b);
    let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

    fds.select_side(1).expect("side B should be selectable");

    assert_eq!(
        fds.cpu_read(0x4032) & 0x03,
        0x03,
        "side changes should first look like the disk was removed"
    );

    for _ in 0..FDS_SIDE_CHANGE_EJECT_CPU_CYCLES {
        fds.clock_cpu();
    }
    assert_eq!(
        fds.cpu_read(0x4032) & 0x03,
        0x02,
        "after reinsertion the disk should still need to settle"
    );

    clock_until_side_change_settles(&mut fds);
    assert_eq!(
        fds.cpu_read(0x4032) & 0x03,
        0x00,
        "side changes should eventually report disk present and ready"
    );
}

#[test]
fn disk_image_constructor_initializes_media_slot() {
    let image = FdsImage::parse(&side(0x34)).expect("FDS image should parse");
    let media_id = image.media_object_id();
    let fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

    assert_eq!(fds.disk_image().unwrap().side_count(), 1);
    assert_eq!(fds.media_slot_state().slot.as_ref(), FDS_DRIVE_SLOT_ID);
    assert_eq!(fds.media_slot_state().media_id.as_ref(), Some(&media_id));
    assert_eq!(fds.media_slot_state().side, Some(0));
    assert!(!fds.media_slot_state().write_protected);
}

#[test]
fn media_events_preserve_mutated_disk_across_eject_and_reinsert() {
    let mut bytes = side(0x11);
    bytes.extend_from_slice(&side(0x22));
    let image = FdsImage::parse(&bytes).unwrap();
    let media_id = image.media_object_id();
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.active_side_mut().unwrap()[123] = 0xA5;
    fds.media_slot.record_mutation().unwrap();

    fds.apply_media_event(&MediaEvent::Eject {
        slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
    })
    .unwrap();
    assert!(!fds.media_slot_snapshot().inserted());
    assert_eq!(fds.media_slot.mutation_counter, 1);
    assert_eq!(fds.drive_status(), 0x07);
    let ejected_persistent = fds.dump_persistent_data().unwrap();
    assert!(ejected_persistent.starts_with(&FDS_SAVE_MAGIC));
    let image = FdsImage::parse(&bytes).unwrap();
    let mut reloaded =
        Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    reloaded.load_persistent_data(&ejected_persistent).unwrap();
    assert_eq!(reloaded.disk_image().unwrap().side(0).unwrap()[123], 0xA5);
    assert_eq!(reloaded.media_slot.mutation_counter, 1);
    assert!(
        fds.apply_media_event(&MediaEvent::SelectSide {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
            side: 1,
        })
        .is_err()
    );
    assert!(
        fds.apply_media_event(&MediaEvent::SetWriteProtected {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
            write_protected: true,
        })
        .is_err()
    );

    fds.apply_media_event(&MediaEvent::Insert {
        slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
        media_id,
        side: Some(1),
        write_protected: true,
    })
    .unwrap();
    assert!(fds.media_slot_snapshot().inserted());
    assert_eq!(fds.selected_side(), Some(1));
    assert!(fds.media_slot.write_protected);
    assert_eq!(fds.media_slot.mutation_counter, 1);
    assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[123], 0xA5);
    assert_eq!(fds.drive_status(), 0x07, "inserted media must settle");
}

#[test]
fn media_insert_rejects_wrong_slot_source_and_side() {
    let image = FdsImage::parse(&side(0x11)).unwrap();
    let media_id = image.media_object_id();
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.apply_media_event(&MediaEvent::Eject {
        slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
    })
    .unwrap();

    for event in [
        MediaEvent::Insert {
            slot: MediaSlotId::from("other"),
            media_id: media_id.clone(),
            side: Some(0),
            write_protected: false,
        },
        MediaEvent::Insert {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
            media_id: MediaObjectId::from("sha256:other"),
            side: Some(0),
            write_protected: false,
        },
        MediaEvent::Insert {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
            media_id,
            side: Some(1),
            write_protected: false,
        },
    ] {
        assert!(fds.apply_media_event(&event).is_err());
    }
}

#[test]
fn prg_ram_read_write() {
    let mut fds = make_fds();
    fds.cpu_write(0x6000, 0x42);
    assert_eq!(fds.cpu_peek(0x6000), 0x42);
    fds.cpu_write(0xDFFF, 0xAB);
    assert_eq!(fds.cpu_peek(0xDFFF), 0xAB);
}

#[test]
fn chr_ram_read_write() {
    let mut fds = make_fds();
    fds.chr_write(0x0000, 0x55);
    assert_eq!(fds.chr_read(0x0000), 0x55);
    fds.chr_write(0x1FFF, 0xAA);
    assert_eq!(fds.chr_read(0x1FFF), 0xAA);
}

#[test]
fn irq_counter_fires() {
    let mut fds = make_fds();
    fds.cpu_write(0x4020, 0x02);
    fds.cpu_write(0x4021, 0x00);
    fds.cpu_write(0x4022, 0x02);
    assert!(!fds.irq_pending());
    fds.clock_cpu();
    fds.clock_cpu();
    fds.clock_cpu();
    assert!(fds.irq_pending());
}

#[test]
fn mirroring_toggle() {
    let mut fds = make_fds();
    assert_eq!(fds.mirroring(), Mirroring::Horizontal);
    fds.cpu_write(0x4025, 0x00);
    assert_eq!(fds.mirroring(), Mirroring::Vertical);
    fds.cpu_write(0x4025, 0x08);
    assert_eq!(fds.mirroring(), Mirroring::Horizontal);
}

#[test]
fn inserted_disk_reports_ready_and_present_status() {
    let image = FdsImage::parse(&side(0x34)).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

    assert_eq!(fds.cpu_read(0x4032) & 0x07, 0x00);
}

#[test]
fn selecting_disk_side_switches_active_side_and_rewinds_scan() {
    let mut side_a = vec![0; FDS_SIDE_SIZE];
    side_a[0] = 0xA1;
    let mut side_b = vec![0; FDS_SIDE_SIZE];
    side_b[0] = 0xB2;
    let mut image_bytes = side_a;
    image_bytes.extend_from_slice(&side_b);
    let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0xE5);
    clock_until_next_disk_byte(&mut fds);
    assert_eq!(fds.cpu_read(0x4031), 0xA1);

    fds.select_side(1).expect("side B should be selectable");
    assert_eq!(fds.selected_side(), Some(1));
    clock_until_next_disk_byte(&mut fds);
    assert_eq!(fds.cpu_read(0x4031), 0xB2);
}

#[test]
fn selecting_missing_disk_side_is_rejected() {
    let image = FdsImage::parse(&side(0x34)).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

    let err = fds.select_side(1).unwrap_err().to_string();

    assert!(err.contains("side B is not present"), "{err}");
    assert_eq!(fds.selected_side(), Some(0));
}

#[test]
fn disk_scan_streams_side_bytes_and_raises_transfer_irq() {
    let mut side = vec![0; FDS_SIDE_SIZE];
    side[0] = 0x01;
    side[1] = 0x02;
    let image = FdsImage::parse(&side).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0xE5);
    clock_until_next_disk_byte(&mut fds);

    assert!(fds.irq_pending());
    assert_eq!(fds.cpu_read(0x4030) & 0x80, 0x80);
    assert!(!fds.irq_pending());

    clock_until_next_disk_byte(&mut fds);
    assert!(fds.irq_pending());
    assert_eq!(fds.cpu_read(0x4031), 0x02);
    assert!(!fds.irq_pending());
}

#[test]
fn disk_scan_reset_rewinds_to_start_of_side() {
    let mut side = vec![0; FDS_SIDE_SIZE];
    side[0] = 0xAA;
    side[1] = 0xBB;
    let image = FdsImage::parse(&side).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);

    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0xE5);
    clock_until_next_disk_byte(&mut fds);
    assert_eq!(fds.cpu_read(0x4031), 0xAA);

    fds.cpu_write(0x4025, 0xE4);
    fds.cpu_write(0x4025, 0xE5);
    clock_until_next_disk_byte(&mut fds);
    assert_eq!(fds.cpu_read(0x4031), 0xAA);
}

#[test]
fn audio_wavetable_write_read() {
    let mut fds = make_fds();
    fds.cpu_write(0x4089, 0x80);
    fds.cpu_write(0x4040, 0x20);
    fds.cpu_write(0x4041, 0x3F);
    assert_eq!(fds.cpu_peek(0x4040), 0x20);
    assert_eq!(fds.cpu_peek(0x4041), 0x3F);
}

#[test]
fn battery_data_roundtrip() {
    let mut fds = make_fds();
    fds.cpu_write(0x6000, 0x42);
    fds.cpu_write(0x7FFF, 0xBE);
    let data = fds.dump_battery_data().expect("should dump");
    let mut fds2 = make_fds();
    fds2.load_battery_data(&data).expect("should load");
    assert_eq!(fds2.cpu_peek(0x6000), 0x42);
    assert_eq!(fds2.cpu_peek(0x7FFF), 0xBE);
}

#[test]
fn disk_write_transport_mutates_only_selected_side() {
    let mut side_a = side(0x11);
    side_a[0] = FDS_DISK_INFO_BLOCK_CODE;
    let mut side_b = side(0x22);
    side_b[0] = FDS_DISK_INFO_BLOCK_CODE;
    let mut image_bytes = side_a;
    image_bytes.extend_from_slice(&side_b);
    let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.select_side(1).unwrap();
    clock_until_side_change_settles(&mut fds);
    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0xC1);
    fds.disk_position = 10;
    fds.disk_lead_in_counter = 0;
    fds.disk_cycle_counter = 1;
    fds.disk_next_gap_position = Some(FDS_DISK_INFO_BLOCK_LEN);

    fds.cpu_write(0x4024, 0xA5);
    fds.clock_cpu();

    let image = fds.disk_image().unwrap();
    assert_eq!(image.side(0).unwrap()[10], 0x11);
    assert_eq!(image.side(1).unwrap()[10], 0xA5);
    assert_eq!(fds.disk_position, 11);
    assert_eq!(fds.media_slot_state().mutation_counter, 1);
    assert!(fds.irq_pending());
    assert_eq!(fds.cpu_read(0x4030) & 0x80, 0x80);
}

#[test]
fn write_protected_disk_signals_status_without_mutating() {
    let mut disk = side(0x33);
    disk[0] = FDS_DISK_INFO_BLOCK_CODE;
    let image = FdsImage::parse(&disk).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.media_slot.write_protected = true;
    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0xC1);
    fds.disk_position = 10;
    fds.disk_lead_in_counter = 0;
    fds.disk_cycle_counter = 1;
    fds.disk_next_gap_position = Some(FDS_DISK_INFO_BLOCK_LEN);

    fds.cpu_write(0x4024, 0xA5);
    fds.clock_cpu();

    assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x33);
    assert_eq!(fds.media_slot_state().mutation_counter, 0);
    assert_eq!(fds.disk_position, 11);
    assert_eq!(fds.cpu_read(0x4032) & 0x04, 0x04);
}

#[test]
fn disk_ready_clear_clocks_compact_gap_without_mutation() {
    let mut disk = side(0x66);
    disk[0] = FDS_DISK_INFO_BLOCK_CODE;
    let image = FdsImage::parse(&disk).unwrap();
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0x81);
    fds.disk_position = 10;
    fds.disk_lead_in_counter = 0;
    fds.disk_cycle_counter = 1;
    fds.disk_next_gap_position = Some(FDS_DISK_INFO_BLOCK_LEN);

    fds.cpu_write(0x4024, 0xA5);
    fds.clock_cpu();

    assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x66);
    assert_eq!(fds.disk_position, 10);
    assert_eq!(fds.media_slot_state().mutation_counter, 0);
    assert!(fds.irq_pending());
}

#[test]
fn crc_control_clocks_physical_crc_without_mutating_compact_media() {
    let mut disk = side(0x77);
    disk[0] = FDS_DISK_INFO_BLOCK_CODE;
    let image = FdsImage::parse(&disk).unwrap();
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0xD1);
    fds.disk_position = 10;
    fds.disk_lead_in_counter = 0;
    fds.disk_cycle_counter = 1;
    fds.disk_next_gap_position = Some(10);
    fds.disk_crc_accumulator = 0xBEEF;

    fds.cpu_write(0x4024, 0xA5);
    fds.clock_cpu();

    assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x77);
    assert_eq!(fds.disk_position, 10);
    assert_eq!(fds.disk_crc_accumulator, 0x00BE);
    assert!(fds.disk_write_in_gap);
    assert_eq!(fds.media_slot_state().mutation_counter, 0);
    assert!(!fds.irq_pending());
    assert_eq!(fds.disk_status() & 0x80, 0);
}

#[test]
fn write_gap_and_sync_do_not_overwrite_compact_block_bytes() {
    let mut disk = side(0x66);
    disk[0] = FDS_DISK_INFO_BLOCK_CODE;
    let image = FdsImage::parse(&disk).unwrap();
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.cpu_write(0x4023, 0x01);
    fds.disk_position = 10;
    fds.disk_lead_in_counter = 0;

    for (control, value) in [(0x81, 0x00), (0xC1, 0x80)] {
        fds.cpu_write(0x4025, control);
        fds.cpu_write(0x4024, value);
        fds.disk_lead_in_counter = 0;
        fds.disk_cycle_counter = 1;
        fds.clock_cpu();
        assert_eq!(fds.disk_position, 10);
        assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x66);
    }

    fds.cpu_write(0x4024, 0x02);
    fds.disk_cycle_counter = 1;
    fds.clock_cpu();
    assert_eq!(fds.disk_position, 11);
    assert_eq!(fds.disk_image().unwrap().side(0).unwrap()[10], 0x02);
    assert_eq!(fds.media_slot.mutation_counter, 1);
}

#[test]
fn fds_save_container_roundtrips_mutated_sides_without_volatile_ram() {
    let mut disk = side(0x44);
    disk[0] = FDS_DISK_INFO_BLOCK_CODE;
    let image = FdsImage::parse(&disk).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(
        vec![0xFF; FDS_BIOS_SIZE],
        image.clone(),
        Mirroring::Horizontal,
    );
    fds.cpu_write(0x6000, 0x5A);
    fds.disk_image.as_mut().unwrap().side_mut(0).unwrap()[12] = 0xC7;
    fds.media_slot.record_mutation().unwrap();

    let data = fds.dump_persistent_data().expect("FDS save should dump");
    assert!(data.starts_with(&FDS_SAVE_MAGIC));

    let mut restored =
        Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    restored
        .load_persistent_data(&data)
        .expect("FDS save should load");
    assert_eq!(restored.cpu_peek(0x6000), 0x00);
    assert_eq!(restored.disk_image().unwrap().side(0).unwrap()[12], 0xC7);
    assert_eq!(restored.media_slot_state().mutation_counter, 1);
}

#[test]
fn legacy_raw_fds_ram_save_still_loads() {
    let image = FdsImage::parse(&side(0x44)).unwrap();
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    let mut legacy = vec![0; PRG_RAM_SIZE];
    legacy[0] = 0x42;
    legacy[PRG_RAM_SIZE - 1] = 0xBE;

    fds.load_persistent_data(&legacy).unwrap();

    assert_eq!(fds.cpu_peek(0x6000), 0x42);
    assert_eq!(fds.cpu_peek(0xDFFF), 0xBE);
    assert_eq!(fds.media_slot_state().mutation_counter, 0);
}

#[test]
fn explicit_battery_load_does_not_treat_magic_prefixed_ram_as_a_container() {
    let image = FdsImage::parse(&side(0x44)).unwrap();
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    let mut battery = vec![0; PRG_RAM_SIZE];
    battery[..FDS_SAVE_MAGIC.len()].copy_from_slice(&FDS_SAVE_MAGIC);

    fds.load_battery_data(&battery).unwrap();

    assert_eq!(
        &fds.dump_battery_data().unwrap()[..FDS_SAVE_MAGIC.len()],
        &FDS_SAVE_MAGIC
    );
}

#[test]
fn version_one_fds_container_without_mutation_counter_still_loads() {
    let image = FdsImage::parse(&side(0x44)).unwrap();
    let source = Fds::with_disk_image(
        vec![0xFF; FDS_BIOS_SIZE],
        image.clone(),
        Mirroring::Horizontal,
    );
    let mut version_one = source.dump_persistent_data().unwrap();
    version_one[FDS_SAVE_MAGIC.len()..FDS_SAVE_MAGIC.len() + 4]
        .copy_from_slice(&FDS_SAVE_VERSION_V1.to_le_bytes());
    let media_id_len_offset = FDS_SAVE_MAGIC.len() + 4;
    let media_id_len = u32::from_le_bytes(
        version_one[media_id_len_offset..media_id_len_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let counter_offset = media_id_len_offset + 4 + media_id_len;
    let mut legacy_prg_ram = Vec::with_capacity(4 + PRG_RAM_SIZE);
    legacy_prg_ram.extend_from_slice(&(PRG_RAM_SIZE as u32).to_le_bytes());
    legacy_prg_ram.resize(4 + PRG_RAM_SIZE, 0);
    version_one.splice(counter_offset + 8..counter_offset + 8, legacy_prg_ram);
    version_one.drain(counter_offset..counter_offset + 8);

    let mut restored =
        Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    restored.load_persistent_data(&version_one).unwrap();

    assert_eq!(restored.disk_image().unwrap().side(0).unwrap()[0], 0x44);
    assert_eq!(restored.media_slot_state().mutation_counter, 0);
}

#[test]
fn version_two_fds_container_loads_media_without_restoring_volatile_ram() {
    let image = FdsImage::parse(&side(0x44)).unwrap();
    let source = Fds::with_disk_image(
        vec![0xFF; FDS_BIOS_SIZE],
        image.clone(),
        Mirroring::Horizontal,
    );
    let mut version_two = source.dump_persistent_data().unwrap();
    version_two[FDS_SAVE_MAGIC.len()..FDS_SAVE_MAGIC.len() + 4]
        .copy_from_slice(&FDS_SAVE_VERSION_V2.to_le_bytes());
    let media_id_len_offset = FDS_SAVE_MAGIC.len() + 4;
    let media_id_len = u32::from_le_bytes(
        version_two[media_id_len_offset..media_id_len_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let counter_offset = media_id_len_offset + 4 + media_id_len;
    let mut legacy_prg_ram = Vec::with_capacity(4 + PRG_RAM_SIZE);
    legacy_prg_ram.extend_from_slice(&(PRG_RAM_SIZE as u32).to_le_bytes());
    legacy_prg_ram.resize(4 + PRG_RAM_SIZE, 0x5A);
    version_two.splice(counter_offset + 8..counter_offset + 8, legacy_prg_ram);

    let mut restored =
        Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    restored.load_persistent_data(&version_two).unwrap();

    assert_eq!(restored.cpu_peek(0x6000), 0x00);
    assert_eq!(restored.disk_image().unwrap().side(0).unwrap()[0], 0x44);
}

#[test]
fn fds_save_container_rejects_different_source_media() {
    let source = FdsImage::parse(&side(0x44)).unwrap();
    let other = FdsImage::parse(&side(0x55)).unwrap();
    let source = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], source, Mirroring::Horizontal);
    let data = source.dump_persistent_data().unwrap();
    let mut other = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], other, Mirroring::Horizontal);

    let err = other.load_persistent_data(&data).unwrap_err();
    assert!(err.to_string().contains("different source media"));
}

#[test]
fn audio_produces_output_when_playing() {
    let mut fds = make_fds();
    fds.cpu_write(0x4089, 0x80);
    for i in 0..64u8 {
        fds.cpu_write(0x4040 + i as u16, (i & 0x3F) | 0x20);
    }
    fds.cpu_write(0x4089, 0x00);
    fds.cpu_write(0x4080, 0x80 | 0x1F);
    fds.cpu_write(0x4082, 0xFF);
    fds.cpu_write(0x4083, 0x03);
    for _ in 0..1000 {
        fds.clock_cpu();
    }
    let out = fds.audio_output();
    assert!(out > 0.0, "expected audio output > 0, got {out}");
}

#[test]
fn audio_silent_when_halted() {
    let mut fds = make_fds();
    fds.cpu_write(0x4083, 0x80);
    for _ in 0..100 {
        fds.clock_cpu();
    }
    assert_eq!(fds.audio_output(), 0.0);
}

#[test]
fn irq_repeat_mode() {
    let mut fds = make_fds();
    fds.cpu_write(0x4020, 0x01);
    fds.cpu_write(0x4021, 0x00);
    fds.cpu_write(0x4022, 0x03);
    fds.clock_cpu();
    fds.clock_cpu();
    assert!(fds.irq_pending());
    let _ = fds.cpu_read(0x4030);
    assert!(!fds.irq_pending());
    fds.clock_cpu();
    fds.clock_cpu();
    assert!(fds.irq_pending());
}

#[test]
fn irq_no_repeat_disables_after_fire() {
    let mut fds = make_fds();
    fds.cpu_write(0x4020, 0x01);
    fds.cpu_write(0x4021, 0x00);
    fds.cpu_write(0x4022, 0x02);
    fds.clock_cpu();
    fds.clock_cpu();
    assert!(fds.irq_pending());
    let _ = fds.cpu_read(0x4030);
    assert!(!fds.irq_pending());
    for _ in 0..10 {
        fds.clock_cpu();
    }
    assert!(!fds.irq_pending());
}

#[test]
fn mod_table_writes_only_when_halted() {
    let mut fds = make_fds();
    fds.cpu_write(0x4087, 0x80);
    fds.cpu_write(0x4088, 0x03);
    assert_eq!(fds.cpu_peek(0x4040), 0);
    fds.cpu_write(0x4087, 0x00);
    fds.cpu_write(0x4088, 0x05);
}

#[test]
fn volume_gain_read() {
    let mut fds = make_fds();
    fds.cpu_write(0x4080, 0x80 | 0x20);
    let val = fds.cpu_peek(0x4090);
    assert_eq!(val & 0x3F, 0x20);
}

#[test]
fn save_state_roundtrip() {
    let mut side = vec![0; FDS_SIDE_SIZE];
    side[0] = 0x11;
    side[1] = 0x22;
    let image = FdsImage::parse(&side).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.cpu_write(0x6000, 0xAA);
    fds.cpu_write(0x4023, 0x01);
    fds.cpu_write(0x4025, 0xE5);
    clock_until_next_disk_byte(&mut fds);
    assert_eq!(fds.cpu_read(0x4031), 0x11);
    fds.cpu_write(0x4089, 0x80);
    fds.cpu_write(0x4040, 0x20);
    fds.cpu_write(0x4089, 0x00);
    fds.cpu_write(0x4080, 0x80 | 0x1F);
    fds.cpu_write(0x4082, 0xFF);
    fds.cpu_write(0x4083, 0x03);
    for _ in 0..100 {
        fds.clock_cpu();
    }

    let mut writer = crate::save_state::StateWriter::new();
    fds.write_state(&mut writer);
    let bytes = writer.into_bytes();

    let image = FdsImage::parse(&side).expect("FDS image should parse");
    let mut fds2 = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    let mut reader = crate::save_state::StateReader::new(&bytes);
    fds2.read_state(&mut reader).expect("read_state ok");

    assert_eq!(fds2.cpu_peek(0x6000), 0xAA);
    assert_eq!(fds2.cpu_peek(0x4040), 0x20);
    clock_until_next_disk_byte(&mut fds2);
    assert_eq!(fds2.cpu_read(0x4031), 0x22);
}

#[test]
fn save_state_roundtrip_preserves_selected_side() {
    let mut side_a = vec![0; FDS_SIDE_SIZE];
    side_a[0] = 0xA1;
    let mut side_b = vec![0; FDS_SIDE_SIZE];
    side_b[0] = 0xB2;
    let mut image_bytes = side_a;
    image_bytes.extend_from_slice(&side_b);
    let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
    let mut fds = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    fds.select_side(1).expect("side B should be selectable");

    let mut writer = crate::save_state::StateWriter::new();
    fds.write_state(&mut writer);
    let bytes = writer.into_bytes();

    let image = FdsImage::parse(&image_bytes).expect("FDS image should parse");
    let mut fds2 = Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    let mut reader = crate::save_state::StateReader::new(&bytes);
    fds2.read_state(&mut reader).expect("read_state ok");

    assert_eq!(fds2.selected_side(), Some(1));
    fds2.cpu_write(0x4023, 0x01);
    fds2.cpu_write(0x4025, 0xE5);
    clock_until_next_disk_byte(&mut fds2);
    assert_eq!(fds2.cpu_read(0x4031), 0xB2);
}

#[test]
fn legacy_drive_marker_restores_selected_side_unprotected() {
    let mut bytes = side(0x11);
    bytes.extend_from_slice(&side(0x22));
    let image = FdsImage::parse(&bytes).unwrap();
    let mut source = Fds::with_disk_image(
        vec![0xFF; FDS_BIOS_SIZE],
        image.clone(),
        Mirroring::Horizontal,
    );
    source.select_side(1).unwrap();
    let mut writer = crate::save_state::StateWriter::new();
    source.write_state(&mut writer);
    let mut state = writer.into_bytes();
    let marker = state
        .windows(FDS_DRIVE_STATE_MARKER.len())
        .position(|window| window == FDS_DRIVE_STATE_MARKER)
        .unwrap();
    state[marker..marker + FDS_DRIVE_STATE_MARKER.len()]
        .copy_from_slice(&FDS_DRIVE_STATE_MARKER_V1);

    let inserted_offset = marker + FDS_DRIVE_STATE_MARKER.len() + 26;
    state.remove(inserted_offset);
    let after_side = inserted_offset + 2;
    state.drain(after_side..after_side + 4);

    let mut restored =
        Fds::with_disk_image(vec![0xFF; FDS_BIOS_SIZE], image, Mirroring::Horizontal);
    restored
        .read_state(&mut crate::save_state::StateReader::new(&state))
        .unwrap();
    assert_eq!(restored.selected_side(), Some(1));
    assert!(restored.media_slot.inserted());
    assert!(!restored.media_slot.write_protected);
}
