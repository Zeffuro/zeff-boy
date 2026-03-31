use super::*;
use crate::hardware::cartridge::Mirroring;

fn write_ctrl(m: &mut BandaiFcg16, scl: bool, sda: bool, read_en: bool) {
    let mut v = 0u8;
    if scl {
        v |= 0x80;
    }
    if sda {
        v |= 0x40;
    }
    if read_en {
        v |= 0x20;
    }
    m.cpu_write(0x800D, v);
}

fn i2c_start(m: &mut BandaiFcg16, read_en: bool) {
    write_ctrl(m, true, true, read_en);
    write_ctrl(m, true, false, read_en);
    write_ctrl(m, false, false, read_en);
}

fn i2c_stop(m: &mut BandaiFcg16, read_en: bool) {
    write_ctrl(m, false, false, read_en);
    write_ctrl(m, true, false, read_en);
    write_ctrl(m, true, true, read_en);
}

fn i2c_write_byte(m: &mut BandaiFcg16, b: u8) {
    for bit in (0..8).rev() {
        let sda = (b >> bit) & 1 != 0;
        write_ctrl(m, false, sda, false);
        write_ctrl(m, true, sda, false);
    }
    write_ctrl(m, false, true, false);
    write_ctrl(m, true, true, false);
    write_ctrl(m, false, true, false);
}

fn i2c_read_byte_nack(m: &mut BandaiFcg16) -> u8 {
    let mut out = 0u8;
    for _ in 0..8 {
        write_ctrl(m, false, true, true);
        write_ctrl(m, true, true, true);
        out = (out << 1) | u8::from(m.cpu_read(0x6000) & 0x10 != 0);
    }
    write_ctrl(m, false, true, true);
    write_ctrl(m, true, true, true);
    write_ctrl(m, false, true, true);
    out
}

#[test]
fn mapper16_switches_prg_bank_at_8000() {
    let mut prg = vec![0u8; 3 * 0x4000];
    for bank in 0..3usize {
        prg[bank * 0x4000] = bank as u8;
    }
    let chr = vec![0u8; 0x2000];

    let mut mapper = BandaiFcg16::new(prg, chr, Mirroring::Horizontal, 4, false);
    mapper.cpu_write(0x6008, 0x01);

    assert_eq!(mapper.cpu_read(0x8000), 1);
}

#[test]
fn mapper16_eeprom_write_then_read_random_access() {
    let prg = vec![0u8; 2 * 0x4000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = BandaiFcg16::new(prg, chr, Mirroring::Horizontal, 5, true);

    i2c_start(&mut mapper, false);
    i2c_write_byte(&mut mapper, 0xA0);
    i2c_write_byte(&mut mapper, 0x12);
    i2c_write_byte(&mut mapper, 0xAB);
    i2c_stop(&mut mapper, false);

    i2c_start(&mut mapper, false);
    i2c_write_byte(&mut mapper, 0xA0);
    i2c_write_byte(&mut mapper, 0x12);
    i2c_start(&mut mapper, true);
    i2c_write_byte(&mut mapper, 0xA1);
    let read_back = i2c_read_byte_nack(&mut mapper);
    i2c_stop(&mut mapper, true);

    assert_eq!(read_back, 0xAB);
}
