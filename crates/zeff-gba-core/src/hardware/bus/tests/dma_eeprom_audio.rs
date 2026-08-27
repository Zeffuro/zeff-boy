use super::*;

#[test]
fn immediate_dma_copies_words() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write32(0x0200_0000, 0x1122_3344);
    bus.write32(0x0400_00B0, 0x0200_0000);
    bus.write32(0x0400_00B4, 0x0300_0000);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0x8400);

    assert_eq!(bus.read32(0x0300_0000), 0x1122_3344);
}

#[test]
fn immediate_dma_aligns_sram_source_before_reading() {
    let mut bus = Bus::new(sram_cartridge(), 48_000);
    bus.write8(0x0E00_0000, b'G');
    bus.write8(0x0E00_0001, b'a');

    bus.write32(0x0400_00BC, 0x0E00_0001);
    bus.write32(0x0400_00C0, 0x0300_0000);
    bus.write16(0x0400_00C4, 1);
    bus.write16(0x0400_00C6, 0x8000);
    assert_eq!(bus.read16(0x0300_0000), 0x4747);

    bus.write32(0x0400_00BC, 0x0E00_0001);
    bus.write32(0x0400_00C0, 0x0300_0004);
    bus.write16(0x0400_00C4, 1);
    bus.write16(0x0400_00C6, 0x8400);
    assert_eq!(bus.read32(0x0300_0004), 0x4747_4747);
}

#[test]
fn dma0_to_dma2_do_not_write_gamepak_sram() {
    let mut bus = Bus::new(sram_cartridge(), 48_000);
    bus.write8(0x0E00_0000, 0x66);
    bus.write32(0x0200_0000, 0xA5B6_C7D8);

    bus.write32(0x0400_00BC, 0x0200_0000);
    bus.write32(0x0400_00C0, 0x0E00_0000);
    bus.write16(0x0400_00C4, 1);
    bus.write16(0x0400_00C6, 0x8400);

    assert_eq!(bus.read32(0x0E00_0000), 0x6666_6666);
}

#[test]
fn dma3_aligns_gamepak_sram_destination_before_writing() {
    let mut bus = Bus::new(sram_cartridge(), 48_000);
    bus.write8(0x0E00_0000, 0x66);
    bus.write32(0x0200_0000, 0xA5B6_C7D8);

    bus.write32(0x0400_00D4, 0x0200_0000);
    bus.write32(0x0400_00D8, 0x0E00_0001);
    bus.write16(0x0400_00DC, 1);
    bus.write16(0x0400_00DE, 0x8000);

    assert_eq!(bus.read32(0x0E00_0000), 0xD8D8_D8D8);
}

#[test]
fn dma_invalid_source_reads_return_channel_data_latch() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write32(0x0200_0000, 0xFEED_FACE);
    bus.write32(0x0400_00B0, 0x0200_0000);
    bus.write32(0x0400_00B4, 0x0300_0000);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0x8400);

    bus.write32(0x0400_00B0, 0x0800_0000);
    bus.write32(0x0400_00B4, 0x0300_0004);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0x8000);
    assert_eq!(bus.read16(0x0300_0004), 0xFACE);

    bus.write32(0x0400_00B0, 0x0800_0000);
    bus.write32(0x0400_00B4, 0x0300_0008);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0x8400);

    assert_eq!(bus.read32(0x0300_0008), 0xFEED_FACE);
}

#[test]
fn dma0_source_uses_27_bit_address_width() {
    let mut bus = Bus::new(sram_cartridge(), 48_000);
    bus.write8(0x0E00_0000, b'G');
    bus.write16(0x0600_0000, 0x50D0);

    bus.write32(0x0400_00B0, 0x0E00_0000);
    bus.write32(0x0400_00B4, 0x0300_0000);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0x8400);

    assert_eq!(bus.read32(0x0300_0000), 0x0000_50D0);
}

#[test]
fn dma1_gamepak_rom_source_steps_even_when_source_mode_is_fixed() {
    let mut rom = vec![0; 0x100];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(&[
        0xEF, 0xBE, 0xAD, 0xDE, // 0xDEADBEEF
        0xF0, 0xBE, 0xAD, 0xDE, // 0xDEADBEF0
        0xF1, 0xBE, 0xAD, 0xDE, // 0xDEADBEF1
        0xF2, 0xBE, 0xAD, 0xDE, // 0xDEADBEF2
    ]);
    let mut bus = Bus::new(Cartridge::load(&rom).unwrap(), 48_000);

    bus.write32(0x0400_00BC, 0x0800_0100);
    bus.write32(0x0400_00C0, 0x0300_0000);
    bus.write16(0x0400_00C4, 4);
    bus.write16(0x0400_00C6, 0x8540);

    assert_eq!(bus.read32(0x0300_0000), 0xDEAD_BEF2);
}

#[test]
fn immediate_dma_reports_transfer_cycles_once() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write32(0x0200_0000, 0x1122_3344);
    bus.write32(0x0400_00B0, 0x0200_0000);
    bus.write32(0x0400_00B4, 0x0300_0000);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0x8400);

    assert_eq!(bus.take_pending_dma_cycles(), 6);
    assert_eq!(bus.take_pending_dma_cycles(), 0);
}

#[test]
fn dma3_serial_eeprom_writes_and_reads_6_bit_address_pages() {
    let mut bus = Bus::new(eeprom_cartridge(), 48_000);
    let page = 3;
    let payload = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
    let mut write_command = vec![1, 0];
    write_command.extend(eeprom_address_bits(page, 6));
    write_command.extend(eeprom_data_bits(payload));
    write_command.push(0);
    write_halfword_bits(&mut bus, 0x0200_0000, &write_command);

    eeprom_dma_write(&mut bus, 0x0200_0000, write_command.len());
    finish_eeprom_write(&mut bus);

    let mut read_command = vec![1, 1];
    read_command.extend(eeprom_address_bits(page, 6));
    read_command.push(0);
    write_halfword_bits(&mut bus, 0x0200_0100, &read_command);
    eeprom_dma_write(&mut bus, 0x0200_0100, read_command.len());
    eeprom_dma_read(&mut bus, 0x0200_0200, 68);

    let bits = read_halfword_bits(&bus, 0x0200_0200, 68);
    assert_eq!(&bits[0..4], &[0, 0, 0, 0]);
    assert_eq!(bytes_from_eeprom_data_bits(&bits[4..]), payload);
}

#[test]
fn dma3_serial_eeprom_accepts_14_bit_address_commands() {
    let mut bus = Bus::new(eeprom_cartridge(), 48_000);
    let page = 0x123;
    let payload = [0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10];
    let mut write_command = vec![1, 0];
    write_command.extend(eeprom_address_bits(page, 14));
    write_command.extend(eeprom_data_bits(payload));
    write_command.push(0);
    write_halfword_bits(&mut bus, 0x0200_0000, &write_command);

    eeprom_dma_write(&mut bus, 0x0200_0000, write_command.len());
    finish_eeprom_write(&mut bus);

    let mut read_command = vec![1, 1];
    read_command.extend(eeprom_address_bits(page, 14));
    read_command.push(0);
    write_halfword_bits(&mut bus, 0x0200_0100, &read_command);
    eeprom_dma_write(&mut bus, 0x0200_0100, read_command.len());
    eeprom_dma_read(&mut bus, 0x0200_0200, 68);

    let bits = read_halfword_bits(&bus, 0x0200_0200, 68);
    assert_eq!(&bits[0..4], &[0, 0, 0, 0]);
    assert_eq!(bytes_from_eeprom_data_bits(&bits[4..]), payload);
}

#[test]
fn dma3_serial_eeprom_read_accepts_high_padding_bit() {
    let mut bus = Bus::new(eeprom_cartridge(), 48_000);
    let page = 31;
    let payload = [0x4F, 0x52, 0x54, 0x45, 0x24, 0x19, 0x05, 0x08];
    let mut write_command = vec![1, 0];
    write_command.extend(eeprom_address_bits(page, 6));
    write_command.extend(eeprom_data_bits(payload));
    write_command.push(0);
    write_halfword_bits(&mut bus, 0x0200_0000, &write_command);

    eeprom_dma_write(&mut bus, 0x0200_0000, write_command.len());
    finish_eeprom_write(&mut bus);

    let mut read_command = vec![1, 1];
    read_command.extend(eeprom_address_bits(page, 6));
    read_command.push(1);
    write_halfword_bits(&mut bus, 0x0200_0100, &read_command);
    eeprom_dma_write(&mut bus, 0x0200_0100, read_command.len());
    eeprom_dma_read(&mut bus, 0x0200_0200, 68);

    let bits = read_halfword_bits(&bus, 0x0200_0200, 68);
    assert_eq!(&bits[0..4], &[0, 0, 0, 0]);
    assert_eq!(bytes_from_eeprom_data_bits(&bits[4..]), payload);
}

#[test]
fn dma3_serial_eeprom_reports_busy_after_write_until_programming_finishes() {
    let mut bus = Bus::new(eeprom_cartridge(), 48_000);
    let mut write_command = vec![1, 0];
    write_command.extend(eeprom_address_bits(0, 6));
    write_command.extend(eeprom_data_bits([0x55; 8]));
    write_command.push(0);
    write_halfword_bits(&mut bus, 0x0200_0000, &write_command);

    eeprom_dma_write(&mut bus, 0x0200_0000, write_command.len());

    assert_eq!(bus.read16(0x0D00_0000) & 1, 0);
    bus.step_cycles(EEPROM_WRITE_BUSY_CYCLES - 1);
    assert_eq!(bus.read16(0x0D00_0000) & 1, 0);
    bus.step_cycles(2);
    assert_eq!(bus.read16(0x0D00_0000) & 1, 1);
}

#[test]
fn dma3_serial_eeprom_status_poll_clears_during_cpu_time() {
    let mut bus = Bus::new(eeprom_cartridge(), 48_000);
    let mut write_command = vec![1, 0];
    write_command.extend(eeprom_address_bits(0, 6));
    write_command.extend(eeprom_data_bits([0x55; 8]));
    write_command.push(0);
    write_halfword_bits(&mut bus, 0x0200_0000, &write_command);

    eeprom_dma_write(&mut bus, 0x0200_0000, write_command.len());
    let dma_cycles = bus.take_pending_dma_cycles();
    bus.step_cycles(dma_cycles);

    let mut ready = false;
    for _ in 0..2000 {
        if bus.read16(0x0D00_0000) & 1 != 0 {
            ready = true;
            break;
        }
        bus.step_cycles(84);
    }

    assert!(ready, "EEPROM should become ready while polling");
}

#[test]
fn hblank_dma_repeats_with_count_reload_and_destination_reload() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write32(0x0200_0000, 0x1111_2222);
    bus.write32(0x0200_0004, 0x3333_4444);
    bus.write32(0x0400_00B0, 0x0200_0000);
    bus.write32(0x0400_00B4, 0x0300_0000);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0xA660);

    bus.step_cycles(1005);
    assert_eq!(bus.read32(0x0300_0000), 0);

    bus.step_cycles(1);
    assert_eq!(bus.read32(0x0300_0000), 0x1111_2222);
    assert_eq!(bus.dma.channel(0).source, 0x0200_0000);
    assert_eq!(bus.dma.channel(0).active_source, 0x0200_0004);
    assert_eq!(bus.dma.channel(0).destination, 0x0300_0000);
    assert_eq!(bus.dma.channel(0).active_destination, 0x0300_0000);
    assert_eq!(bus.dma.channel(0).count, 1);
    assert_ne!(bus.dma.channel(0).control & 0x8000, 0);

    bus.step_cycles(1232);
    assert_eq!(bus.read32(0x0300_0000), 0x3333_4444);
}

#[test]
fn hblank_dma_does_not_run_during_hidden_vblank_lines() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.step_cycles(1232 * 160);
    assert!(bus.ppu.in_vblank());
    assert!(!bus.ppu.in_hblank());

    bus.write32(0x0200_0000, 0x5566_7788);
    bus.write32(0x0400_00B0, 0x0200_0000);
    bus.write32(0x0400_00B4, 0x0300_0000);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0xA400);

    bus.step_cycles(1006);
    assert!(bus.ppu.in_vblank());
    assert!(bus.ppu.in_hblank());
    assert_eq!(bus.read32(0x0300_0000), 0);
}

#[test]
fn vblank_dma_runs_on_vblank_entry() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write32(0x0200_0000, 0x5566_7788);
    bus.write32(0x0400_00B0, 0x0200_0000);
    bus.write32(0x0400_00B4, 0x0300_0000);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0x9400);

    bus.step_cycles(1232 * 160 - 1);
    assert_eq!(bus.read32(0x0300_0000), 0);

    bus.step_cycles(1);
    assert_eq!(bus.read32(0x0300_0000), 0x5566_7788);
    assert_eq!(bus.dma.channel(0).control & 0x8000, 0);
}

#[test]
fn sound_fifo_dma_refills_on_selected_timer_overflow() {
    let mut bus = Bus::new(cartridge(), 48_000);
    for i in 0..4 {
        bus.write32(0x0200_0000 + i * 4, 0x0403_0201 + i * 0x0404_0404);
    }
    bus.write16(0x0400_0084, 0x0080);
    bus.write16(0x0400_0082, (1 << 2) | (1 << 8) | (1 << 9) | (1 << 11));
    bus.write32(0x0400_00BC, 0x0200_0000);
    bus.write32(0x0400_00C0, 0x0400_00A0);
    bus.write16(0x0400_00C4, 4);
    bus.write16(0x0400_00C6, 0xB600);
    bus.write16(0x0400_0100, 0xFFFF);
    bus.write16(0x0400_0102, 0x0080);

    bus.step_cycles(2);

    assert_eq!(bus.apu.fifo_len(0), 16);
    assert_eq!(bus.dma.channel(1).source, 0x0200_0000);
    assert_eq!(bus.dma.channel(1).active_source, 0x0200_0010);
    assert_ne!(bus.dma.channel(1).control & 0x8000, 0);
}

#[test]
fn sound_fifo_dma_services_multiple_timer_overflows_in_one_step() {
    let mut bus = Bus::new(cartridge(), 48_000);
    for pair in 0..10u16 {
        let lo = pair * 2 + 1;
        let hi = lo + 1;
        bus.write16(0x0400_00A0, lo | (hi << 8));
    }
    for i in 0..4 {
        bus.write32(0x0200_0000 + i * 4, 0xA4A3_A2A1 + i * 0x0404_0404);
    }
    bus.write16(0x0400_0084, 0x0080);
    bus.write16(0x0400_0082, (1 << 2) | (1 << 8) | (1 << 9));
    bus.write32(0x0400_00BC, 0x0200_0000);
    bus.write32(0x0400_00C0, 0x0400_00A0);
    bus.write16(0x0400_00C4, 4);
    bus.write16(0x0400_00C6, 0xB600);
    bus.write16(0x0400_0100, 0xFFFF);
    bus.write16(0x0400_0102, 0x0080);

    bus.step_cycles(6);

    assert_eq!(bus.apu.fifo_len(0), 31);
    assert_eq!(bus.dma.channel(1).source, 0x0200_0000);
    assert_eq!(bus.dma.channel(1).active_source, 0x0200_0010);
}

#[test]
fn sound_fifo_dma_reenable_reloads_active_source_from_initial_register() {
    let mut bus = Bus::new(cartridge(), 48_000);
    for i in 0..8 {
        bus.write32(0x0200_0000 + i * 4, 0x0403_0201 + i * 0x0404_0404);
    }
    bus.write16(0x0400_0084, 0x0080);
    bus.write16(0x0400_0082, (1 << 2) | (1 << 8) | (1 << 9) | (1 << 11));
    bus.write32(0x0400_00BC, 0x0200_0000);
    bus.write32(0x0400_00C0, 0x0400_00A0);
    bus.write16(0x0400_00C4, 4);
    bus.write16(0x0400_00C6, 0xB600);
    bus.write16(0x0400_0100, 0xFFFF);
    bus.write16(0x0400_0102, 0x0080);

    bus.step_cycles(2);
    assert_eq!(bus.dma.channel(1).source, 0x0200_0000);
    assert_eq!(bus.dma.channel(1).active_source, 0x0200_0010);

    bus.write16(0x0400_00C6, 0x0400);
    bus.write16(0x0400_00C6, 0xB600);

    assert_eq!(bus.dma.channel(1).source, 0x0200_0000);
    assert_eq!(bus.dma.channel(1).active_source, 0x0200_0000);
}

#[test]
fn audio_output_is_split_at_direct_sound_timer_overflows() {
    let mut bus = Bus::new(cartridge(), 262_144);
    bus.write16(0x0400_0084, 0x0080);
    bus.write16(0x0400_0088, 0xC200);
    bus.write16(0x0400_0082, (1 << 2) | (1 << 8) | (1 << 9));
    bus.write16(0x0400_00A0, 0x7F80);
    bus.write16(0x0400_0100, 0xFFFF);
    bus.write16(0x0400_0102, 0x0081);

    bus.step_cycles(192);

    let mut samples = Vec::new();
    bus.apu.drain_samples_into(&mut samples);
    assert_eq!(samples.len(), 6);
    assert_eq!(samples[0], 0.0);
    assert_eq!(samples[1], 0.0);
    assert!(samples[2] < -0.10);
    assert!(samples[3] < -0.10);
    assert!(samples[4] > 0.0);
    assert!(samples[5] > 0.0);
}

#[test]
fn gba_psg_registers_drive_gb_compatible_square_channel() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0084, 0x0080);
    bus.write16(0x0400_0080, 0xFF77);
    bus.write16(0x0400_0068, 0xF080);
    bus.write16(0x0400_006C, 0x8700);

    assert_ne!(bus.read8(0x0400_0084) & 0x82, 0);

    bus.step_cycles(8192);
    let mut samples = Vec::new();
    bus.apu.drain_samples_into(&mut samples);

    assert!(!samples.is_empty());
    assert!(samples.iter().any(|&sample| sample != 0.0));
}

#[test]
fn gba_bus_audio_output_rate_matches_configured_sample_rate() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0084, 0x0080);
    bus.write16(0x0400_0080, 0xFF77);
    bus.write16(0x0400_0068, 0xF080);
    bus.write16(0x0400_006C, 0x8700);

    bus.step_cycles(CYCLES_PER_FRAME);

    let mut samples = Vec::new();
    bus.apu.drain_samples_into(&mut samples);
    let stereo_pairs = samples.len() / 2;
    assert!(
        (802..=805).contains(&stereo_pairs),
        "expected about 804 stereo pairs per GBA frame at 48 kHz, got {stereo_pairs}"
    );
}

#[test]
fn gba_bus_audio_output_rate_stays_stable_across_frames() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0084, 0x0080);
    bus.write16(0x0400_0080, 0xFF77);
    bus.write16(0x0400_0068, 0xF080);
    bus.write16(0x0400_006C, 0x8700);

    for _ in 0..60 {
        bus.step_cycles(CYCLES_PER_FRAME);
    }

    let mut samples = Vec::new();
    bus.apu.drain_samples_into(&mut samples);
    let stereo_pairs = samples.len() / 2;
    assert!(
        (48_200..=48_300).contains(&stereo_pairs),
        "expected about 48240 stereo pairs for 60 GBA frames at 48 kHz, got {stereo_pairs}"
    );
}

#[test]
fn soundbias_defaults_to_clean_output_level_and_masks_unwritable_bits() {
    let mut bus = Bus::new(cartridge(), 48_000);

    assert_eq!(bus.read16(0x0400_0088), 0x0200);

    bus.write16(0x0400_0088, 0xFFFF);

    assert_eq!(bus.read16(0x0400_0088), 0xC3FE);
}

#[test]
fn gba_wave_ram_maps_to_psg_wave_ram() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write8(0x0400_0093, 0xAB);

    assert_eq!(bus.read8(0x0400_0093), 0xAB);
}
