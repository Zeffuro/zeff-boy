use super::*;
use crate::hardware::cartridge::rtc::{
    RTC_DAY_HIGH, RTC_DAY_LOW, RTC_DH_CARRY_BIT, RTC_DH_DAY_HIGH_BIT, RTC_HOURS, RTC_MINUTES,
    RTC_SECONDS,
};

#[test]
fn rtc_registers_are_latched_on_every_latch_write() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
    mbc3.write_rom(0x0000, 0x0A);

    mbc3.write_rom(0x4000, 0x08);
    mbc3.write_ram(0xA000, 12);
    mbc3.write_rom(0x4000, 0x09);
    mbc3.write_ram(0xA000, 34);
    mbc3.write_rom(0x4000, 0x0A);
    mbc3.write_ram(0xA000, 56);

    mbc3.write_rom(0x6000, 0xA5);

    mbc3.write_rom(0x4000, 0x08);
    assert_eq!(mbc3.read_ram(0xA000), 12);
    mbc3.write_rom(0x4000, 0x09);
    assert_eq!(mbc3.read_ram(0xA000), 34);
    mbc3.write_rom(0x4000, 0x0A);
    assert_eq!(mbc3.read_ram(0xA000), 24);

    mbc3.write_rom(0x4000, 0x08);
    mbc3.write_ram(0xA000, 45);
    mbc3.write_rom(0x6000, 0xA5);
    assert_eq!(mbc3.read_ram(0xA000), 45);
}

#[test]
fn rtc_mapper_uses_four_select_bits_and_leaves_invalid_banks_open() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x8000, true);
    mbc3.write_rom(0x0000, 0x0A);

    mbc3.write_rom(0x4000, 0x00);
    mbc3.write_ram(0xA000, 0x42);
    mbc3.write_rom(0x4000, 0x04);
    mbc3.write_ram(0xA000, 0x99);
    assert_eq!(mbc3.read_ram(0xA000), 0xFF);

    mbc3.write_rom(0x4000, 0x10);
    assert_eq!(mbc3.read_ram(0xA000), 0x42);
    mbc3.write_rom(0x4000, 0x1D);
    assert_eq!(mbc3.read_ram(0xA000), 0xFF);
}

#[test]
fn rtc_overflow_sets_carry_and_wraps_day_counter() {
    let rtc = &mut Rtc {
        internal: [59, 59, 23, 0xFF, 0x01],
        latched: [0; RTC_REG_COUNT],
        subsecond_cycles: 0,
    };
    rtc.advance_cycles(GB_T_CYCLES_PER_SECOND);

    assert_eq!(rtc.internal[RTC_SECONDS], 0);
    assert_eq!(rtc.internal[RTC_MINUTES], 0);
    assert_eq!(rtc.internal[RTC_HOURS], 0);
    assert_eq!(rtc.internal[RTC_DAY_LOW], 0);
    assert_eq!(rtc.internal[RTC_DAY_HIGH] & RTC_DH_DAY_HIGH_BIT, 0);
    assert_ne!(rtc.internal[RTC_DAY_HIGH] & RTC_DH_CARRY_BIT, 0);
}

#[test]
fn rtc_sram_footer_44_byte_format_is_loaded() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 2, true);
    let mut bytes = vec![0xAA, 0xBB];
    let regs: [u8; 10] = [1, 2, 3, 4, 0x41, 5, 6, 7, 8, 0x80];
    for reg in regs {
        bytes.extend_from_slice(&(reg as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&1234u32.to_le_bytes());

    mbc3.load_sram(&bytes);

    assert_eq!(mbc3.ram, vec![0xAA, 0xBB]);
    let rtc = &mbc3.rtc;
    assert_eq!(rtc.internal, [1, 2, 3, 4, 0x41]);
    assert_eq!(rtc.latched, [5, 6, 7, 8, 0x80]);
}

#[test]
fn rtc_explicit_time_sidecar_io_is_deterministic() {
    let epoch = 946_684_800u64;
    let mut bytes = vec![0xAA, 0xBB];
    for reg in [1u8, 2, 3, 4, 0, 5, 6, 7, 8, 0] {
        bytes.extend_from_slice(&(reg as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&(epoch - 5).to_le_bytes());

    let mut first = Mbc3::new(vec![0; 0x8000], 2, true);
    let mut second = Mbc3::new(vec![0; 0x8000], 2, true);
    first.load_sram_at_time(&bytes, epoch);
    second.load_sram_at_time(&bytes, epoch);

    assert_eq!(first.rtc_state(), second.rtc_state());
    assert_eq!(first.rtc.internal[RTC_SECONDS], 6);
    assert_eq!(first.rtc.subsecond_cycles, 0);
    let persisted = first.dump_sram_at_time(epoch);
    assert_eq!(&persisted[persisted.len() - 8..], &epoch.to_le_bytes());
}

#[test]
fn rtc_subsecond_extension_roundtrips_complete_clock_state() {
    let epoch = 946_684_800u64;
    let mut source = Mbc3::new(vec![0; 0x8000], 2, true);
    source.ram.copy_from_slice(&[0xA5, 0x5A]);
    source.rtc.internal = [17, 23, 5, 9, 0];
    source.rtc.latched = [16, 22, 4, 8, 0];
    source.step(GB_T_CYCLES_PER_SECOND / 3);
    let expected = source.rtc_state();

    let standard = source.dump_sram_at_time(epoch);
    let extended = source.dump_sram_with_rtc_subsecond_at_time(epoch);
    assert_eq!(standard.len(), source.ram.len() + 48);
    assert_eq!(extended.len(), source.ram.len() + 64);

    let mut restored = Mbc3::new(vec![0; 0x8000], 2, true);
    restored.load_sram_at_time(&extended, epoch);
    assert_eq!(restored.ram, source.ram);
    assert_eq!(restored.rtc_state(), expected);

    let mut caught_up = Mbc3::new(vec![0; 0x8000], 2, true);
    caught_up.load_sram_at_time(&extended, epoch + 5);
    assert_eq!(caught_up.rtc.internal[RTC_SECONDS], 22);
    assert_eq!(caught_up.rtc.subsecond_cycles, source.rtc.subsecond_cycles);
}

#[test]
fn rtc_subsecond_extension_rejects_an_out_of_range_phase() {
    let epoch = 946_684_800u64;
    let source = Mbc3::new(vec![0; 0x8000], 2, true);
    let mut bytes = source.dump_sram_with_rtc_subsecond_at_time(epoch);
    let end = bytes.len();
    bytes[end - 8..].copy_from_slice(&GB_T_CYCLES_PER_SECOND.to_le_bytes());

    let mut restored = Mbc3::new(vec![0; 0x8000], 2, true);
    restored.rtc.internal[RTC_SECONDS] = 31;
    restored.load_sram_at_time(&bytes, epoch);
    assert_eq!(restored.ram, source.ram);
    assert_eq!(restored.rtc.internal[RTC_SECONDS], 31);
    assert_eq!(restored.rtc.subsecond_cycles, 0);
}

#[test]
fn rtc_catchup_handles_a_zero_timestamp_without_per_second_work() {
    let mut rtc = Rtc::new();
    rtc.catchup_seconds(946_684_800);

    assert_eq!(rtc.internal, [0, 0, 0, 205, RTC_DH_CARRY_BIT]);
}

#[test]
fn rtc_tick_based_on_t_cycles() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
    mbc3.write_rom(0x0000, 0x0A);
    mbc3.write_rom(0x4000, 0x08);
    mbc3.write_ram(0xA000, 0); // seconds = 0

    // Advance just under 1 second - should not tick
    mbc3.step(GB_T_CYCLES_PER_SECOND - 1);
    mbc3.write_rom(0x6000, 0x00);
    mbc3.write_rom(0x6000, 0x01);
    assert_eq!(mbc3.read_ram(0xA000), 0);

    // Advance 1 more cycle - should tick to 1
    mbc3.step(1);
    mbc3.write_rom(0x6000, 0x00);
    mbc3.write_rom(0x6000, 0x01);
    assert_eq!(mbc3.read_ram(0xA000), 1);
}

#[test]
fn rtc_subsecond_preserved_on_non_seconds_write() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
    mbc3.write_rom(0x0000, 0x0A);

    // Advance 500ms worth of cycles
    let half_second = GB_T_CYCLES_PER_SECOND / 2;
    mbc3.step(half_second);

    // Write to minutes register (should preserve sub-second)
    mbc3.write_rom(0x4000, 0x09);
    mbc3.write_ram(0xA000, 5);

    // Advance another 500ms - should now tick
    mbc3.step(half_second);
    mbc3.write_rom(0x6000, 0x00);
    mbc3.write_rom(0x6000, 0x01);
    mbc3.write_rom(0x4000, 0x08);
    assert_eq!(mbc3.read_ram(0xA000), 1);
}

#[test]
fn rtc_subsecond_reset_on_seconds_write() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
    mbc3.write_rom(0x0000, 0x0A);

    // Advance 500ms worth of cycles
    let half_second = GB_T_CYCLES_PER_SECOND / 2;
    mbc3.step(half_second);

    // Write to seconds register (should reset sub-second)
    mbc3.write_rom(0x4000, 0x08);
    mbc3.write_ram(0xA000, 10);

    // Advance another 500ms - should NOT tick (sub-second was reset)
    mbc3.step(half_second);
    mbc3.write_rom(0x6000, 0x00);
    mbc3.write_rom(0x6000, 0x01);
    assert_eq!(mbc3.read_ram(0xA000), 10);

    // Need full 1s from the seconds write to tick
    mbc3.step(half_second);
    mbc3.write_rom(0x6000, 0x00);
    mbc3.write_rom(0x6000, 0x01);
    assert_eq!(mbc3.read_ram(0xA000), 11);
}
