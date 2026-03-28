use super::*;

fn make_mbc7() -> Mbc7 {
    let mut mbc7 = Mbc7::new(vec![0; 0x8000]);
    mbc7.write_rom(0x0000, 0x0A);
    mbc7.write_rom(0x4000, SENSOR_UNLOCK);
    mbc7
}

#[test]
fn accelerometer_returns_center_after_latch() {
    let mut mbc7 = make_mbc7();

    let x_before = (mbc7.read_ram(0xA031) as u16) << 8 | mbc7.read_ram(0xA020) as u16;
    assert_eq!(x_before, 0x8000);

    mbc7.write_ram(0xA000, ACCEL_LATCH_VALUE);
    let x = (mbc7.read_ram(0xA031) as u16) << 8 | mbc7.read_ram(0xA020) as u16;
    assert_eq!(x, 0x8000);
}

#[test]
fn rom_bank_0_maps_to_1() {
    let mut mbc7 = Mbc7::new(vec![0; 0x8000]);
    mbc7.write_rom(0x2000, 0x00);
    assert_eq!(mbc7.rom_bank, 1);
}

#[test]
fn ram_disabled_returns_ff() {
    let mbc7 = Mbc7::new(vec![0; 0x8000]);
    assert_eq!(mbc7.read_ram(0xA080), 0xFF);
}

#[test]
fn eeprom_read_write_roundtrip() {
    let mut mbc7 = make_mbc7();

    eeprom_send_command(&mut mbc7, CMD_EXTENDED, 0b11_000000, None);
    eeprom_send_command(&mut mbc7, CMD_WRITE, 0x00, Some(0x1234));

    let val = eeprom_read(&mut mbc7, 0x00);
    assert_eq!(val, 0x1234);
}

#[test]
fn host_tilt_maps_to_sensor_range_and_clamps() {
    let mut mbc7 = make_mbc7();

    mbc7.set_host_tilt(0.0, 0.0);
    mbc7.write_ram(0xA000, ACCEL_LATCH_VALUE);
    mbc7.write_ram(0xA010, ACCEL_RESET_VALUE);
    let center_x = (mbc7.read_ram(0xA031) as u16) << 8 | mbc7.read_ram(0xA020) as u16;
    assert_eq!(center_x, ACCEL_CENTER);

    mbc7.set_host_tilt(10.0, -10.0);
    mbc7.write_ram(0xA000, ACCEL_LATCH_VALUE);
    mbc7.write_ram(0xA010, ACCEL_RESET_VALUE);
    let x = (mbc7.read_ram(0xA031) as u16) << 8 | mbc7.read_ram(0xA020) as u16;
    let y = (mbc7.read_ram(0xA051) as u16) << 8 | mbc7.read_ram(0xA040) as u16;
    assert_eq!(x, (ACCEL_CENTER as i32 - ACCEL_MAX_DELTA) as u16);
    assert_eq!(y, (ACCEL_CENTER as i32 - ACCEL_MAX_DELTA) as u16);
}

fn eeprom_reset(mbc7: &mut Mbc7) {
    mbc7.write_ram(0xA080, 0x00);
    mbc7.write_ram(0xA080, SPI_CS);
}

fn eeprom_send_command(mbc7: &mut Mbc7, cmd: u8, addr: u8, data: Option<u16>) {
    eeprom_reset(mbc7);

    clock_bit(mbc7, true);

    clock_bit(mbc7, cmd & 0x02 != 0);
    clock_bit(mbc7, cmd & 0x01 != 0);

    for i in (0..ADDR_BITS).rev() {
        clock_bit(mbc7, addr & (1 << i) != 0);
    }

    if let Some(d) = data {
        for i in (0..DATA_BITS).rev() {
            clock_bit(mbc7, d & (1 << i) != 0);
        }
    }

    mbc7.write_ram(0xA080, 0x00);
}

fn clock_bit(mbc7: &mut Mbc7, di: bool) {
    let di_bit = if di { SPI_DI } else { 0x00 };
    mbc7.write_ram(0xA080, SPI_CS | di_bit);
    mbc7.write_ram(0xA080, SPI_CS | SPI_CLK | di_bit);
}

fn eeprom_read(mbc7: &mut Mbc7, addr: u8) -> u16 {
    eeprom_reset(mbc7);

    clock_bit(mbc7, true);

    clock_bit(mbc7, true);
    clock_bit(mbc7, false);

    for i in (0..ADDR_BITS).rev() {
        clock_bit(mbc7, addr & (1 << i) != 0);
    }

    clock_bit(mbc7, false);

    let mut result: u16 = 0;
    for _ in 0..DATA_BITS {
        mbc7.write_ram(0xA080, SPI_CS | SPI_CLK);
        mbc7.write_ram(0xA080, SPI_CS);
        result = (result << 1) | (mbc7.read_ram(0xA080) as u16 & 1);
    }

    mbc7.write_ram(0xA080, 0x00);
    result
}

