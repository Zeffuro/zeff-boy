use super::*;
use crate::hardware::cartridge::rtc::{
    RTC_DAY_HIGH, RTC_DAY_LOW, RTC_DH_CARRY_BIT, RTC_DH_DAY_HIGH_BIT, RTC_HOURS, RTC_MINUTES,
    RTC_SECONDS,
};

#[test]
fn rtc_registers_are_latched_on_00_to_01_transition() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
    mbc3.write_rom(0x0000, 0x0A);

    mbc3.write_rom(0x4000, 0x08);
    mbc3.write_ram(0xA000, 12);
    mbc3.write_rom(0x4000, 0x09);
    mbc3.write_ram(0xA000, 34);
    mbc3.write_rom(0x4000, 0x0A);
    mbc3.write_ram(0xA000, 56);

    mbc3.write_rom(0x6000, 0x00);
    mbc3.write_rom(0x6000, 0x01);

    mbc3.write_rom(0x4000, 0x08);
    assert_eq!(mbc3.read_ram(0xA000), 12);
    mbc3.write_rom(0x4000, 0x09);
    assert_eq!(mbc3.read_ram(0xA000), 34);
    mbc3.write_rom(0x4000, 0x0A);
    assert_eq!(mbc3.read_ram(0xA000), 24);
}

#[test]
fn rtc_overflow_sets_carry_and_wraps_day_counter() {
    let rtc = &mut Rtc {
        internal: [59, 59, 23, 0xFF, 0x01],
        latched: [0; RTC_REG_COUNT],
        subsecond_cycles: 0,
    };
    rtc.advance_cycles(T_CYCLES_PER_SECOND);

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
fn rtc_tick_based_on_t_cycles() {
    let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
    mbc3.write_rom(0x0000, 0x0A);
    mbc3.write_rom(0x4000, 0x08);
    mbc3.write_ram(0xA000, 0); // seconds = 0

    // Advance just under 1 second - should not tick
    mbc3.step(T_CYCLES_PER_SECOND - 1);
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
    let half_second = T_CYCLES_PER_SECOND / 2;
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
    let half_second = T_CYCLES_PER_SECOND / 2;
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
