use super::CartridgeDebugInfo;
use super::{build_debug_info, is_ram_enable, load_ram_into, read_banked_rom, read_fixed_rom};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

mod eeprom;
mod sensor;

#[cfg(test)]
use sensor::{ACCEL_CENTER, ACCEL_LATCH_VALUE, ACCEL_MAX_DELTA, ACCEL_RESET_VALUE};

const EEPROM_SIZE: usize = 256;
const EEPROM_WORD_COUNT: usize = 128;

const CMD_BITS: u8 = 2;
const ADDR_BITS: u8 = 8;
const DATA_BITS: u8 = 16;

const CMD_EXTENDED: u8 = 0b00;
const CMD_WRITE: u8 = 0b01;
const CMD_READ: u8 = 0b10;
const CMD_ERASE: u8 = 0b11;

const EXT_EWDS: u8 = 0b00;
const EXT_WRAL: u8 = 0b01;
const EXT_ERAL: u8 = 0b10;
const EXT_EWEN: u8 = 0b11;

const SPI_CS: u8 = 0x80;
const SPI_CLK: u8 = 0x40;
const SPI_DI: u8 = 0x02;

const STATE_IDLE: u8 = 0;
const STATE_COMMAND: u8 = 1;
const STATE_ADDRESS: u8 = 2;
const STATE_DATA: u8 = 3;
const STATE_SHIFT_OUT: u8 = 4;
const STATE_WRITE_PENDING: u8 = 5;

const SENSOR_UNLOCK: u8 = 0x40;

fn register_group(addr: u16) -> u8 {
    ((addr >> 4) & 0x0F) as u8
}

const REG_EEPROM: u8 = 0x8;

pub struct Mbc7 {
    rom: Vec<u8>,
    eeprom: [u8; EEPROM_SIZE],
    ram_enable: bool,
    rom_bank: usize,
    ram_bank_select: u8,

    host_x: f32,
    host_y: f32,
    x_latch: u16,
    y_latch: u16,
    latch_ready: bool,

    cs: bool,
    clk: bool,
    idle: bool,
    state: u8,
    buffer: u16,
    count: u8,
    command: u8,
    address: u8,
    write_enable: bool,
    do_value: u8,

    rumble_active: bool,
}

impl Mbc7 {
    pub fn new(rom: Vec<u8>) -> Self {
        Self {
            rom,
            eeprom: [0xFF; EEPROM_SIZE],
            ram_enable: false,
            rom_bank: 1,
            ram_bank_select: 0,
            host_x: 0.0,
            host_y: 0.0,
            x_latch: 0x8000,
            y_latch: 0x8000,
            latch_ready: false,
            cs: false,
            clk: false,
            idle: false,
            state: STATE_IDLE,
            buffer: 0,
            count: 0,
            command: 0,
            address: 0,
            write_enable: false,
            do_value: 1,
            rumble_active: false,
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => read_fixed_rom(&self.rom, addr),
            0x4000..=0x7FFF => read_banked_rom(&self.rom, self.rom_bank, addr),
            _ => 0xFF,
        }
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enable = is_ram_enable(value),
            0x2000..=0x3FFF => {
                let bank = (value & 0x7F) as usize;
                self.rom_bank = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => self.ram_bank_select = value,
            _ => {}
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable || self.ram_bank_select != SENSOR_UNLOCK {
            return 0xFF;
        }

        let reg = register_group(addr);
        if let Some(value) = self.read_accel_register(reg) {
            return value;
        }

        match reg {
            REG_EEPROM => self.do_value,
            _ => 0xFF,
        }
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable || self.ram_bank_select != SENSOR_UNLOCK {
            return;
        }

        let reg = register_group(addr);
        if self.write_accel_register(reg, value) {
            return;
        }

        if reg == REG_EEPROM { self.write_eeprom(value) }
    }

    pub fn set_host_tilt(&mut self, x: f32, y: f32) {
        self.host_x = x;
        self.host_y = y;
    }

    pub fn rumble_active(&self) -> bool {
        self.rumble_active
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info("MBC7", self.rom_bank, 0, self.ram_enable, None)
    }

    pub fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        self.rom = rom;
    }

    pub(super) fn ram_bytes(&self) -> &[u8] {
        &self.eeprom
    }

    pub(super) fn load_ram_bytes(&mut self, bytes: &[u8]) {
        load_ram_into(&mut self.eeprom, bytes);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.eeprom);
        writer.write_bool(self.ram_enable);
        writer.write_u64(self.rom_bank as u64);
        writer.write_u8(self.ram_bank_select);
        writer.write_bool(self.write_enable);
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2000, (self.rom_bank & 0x7F) as u8),
            (0x4000, self.ram_bank_select),
        ]
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut eeprom = [0u8; EEPROM_SIZE];
        reader.read_exact(&mut eeprom)?;
        let ram_enable = reader.read_bool()?;
        let rom_bank = reader.read_u64()? as usize;
        let ram_bank_select = reader.read_u8()?;
        let write_enable = reader.read_bool()?;

        Ok(Self {
            rom: Vec::new(),
            eeprom,
            ram_enable,
            rom_bank,
            ram_bank_select,
            host_x: 0.0,
            host_y: 0.0,
            x_latch: 0x8000,
            y_latch: 0x8000,
            latch_ready: false,
            cs: false,
            clk: false,
            idle: false,
            state: STATE_IDLE,
            buffer: 0,
            count: 0,
            command: 0,
            address: 0,
            write_enable,
            do_value: 1,
            rumble_active: false,
        })
    }
}

#[cfg(test)]
mod tests {
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
}
