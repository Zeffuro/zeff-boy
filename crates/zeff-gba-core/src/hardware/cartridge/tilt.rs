use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const LATCH_ADDR: u32 = 0x8000;
const SAMPLE_ADDR: u32 = 0x8100;
const X_LOW_ADDR: u32 = 0x8200;
const X_HIGH_ADDR: u32 = 0x8300;
const Y_LOW_ADDR: u32 = 0x8400;
const Y_HIGH_ADDR: u32 = 0x8500;
const SAMPLE_CENTER: i32 = 0x3A0;
const SAMPLE_DELTA: f32 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TiltState {
    pub host_x_bits: u32,
    pub host_y_bits: u32,
    pub x_latch: u16,
    pub y_latch: u16,
    pub latch_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TiltSensor {
    host_x_bits: u32,
    host_y_bits: u32,
    x_latch: u16,
    y_latch: u16,
    latch_ready: bool,
}

impl Default for TiltSensor {
    fn default() -> Self {
        Self {
            host_x_bits: 0,
            host_y_bits: 0,
            x_latch: 0x0FFF,
            y_latch: 0x0FFF,
            latch_ready: false,
        }
    }
}

impl TiltSensor {
    pub(super) fn set_host_input(&mut self, x: f32, y: f32) {
        self.host_x_bits = x.to_bits();
        self.host_y_bits = y.to_bits();
    }

    pub(super) fn state(&self) -> TiltState {
        TiltState {
            host_x_bits: self.host_x_bits,
            host_y_bits: self.host_y_bits,
            x_latch: self.x_latch,
            y_latch: self.y_latch,
            latch_ready: self.latch_ready,
        }
    }

    pub(super) fn reset_execution_state(&mut self) {
        let host_x_bits = self.host_x_bits;
        let host_y_bits = self.host_y_bits;
        *self = Self::default();
        self.host_x_bits = host_x_bits;
        self.host_y_bits = host_y_bits;
    }

    fn from_state(state: TiltState) -> Result<Self> {
        ensure!(
            state.x_latch <= 0x0FFF && state.y_latch <= 0x0FFF,
            "invalid GBA tilt sample"
        );
        Ok(Self {
            host_x_bits: state.host_x_bits,
            host_y_bits: state.host_y_bits,
            x_latch: state.x_latch,
            y_latch: state.y_latch,
            latch_ready: state.latch_ready,
        })
    }

    pub(super) fn read8(&self, addr: u32) -> Option<u8> {
        Some(match addr & 0xFFFF {
            X_LOW_ADDR => self.x_latch as u8,
            X_HIGH_ADDR => ((self.x_latch >> 8) as u8 & 0x0F) | 0x80,
            Y_LOW_ADDR => self.y_latch as u8,
            Y_HIGH_ADDR => (self.y_latch >> 8) as u8 & 0x0F,
            _ => return None,
        })
    }

    pub(super) fn write8(&mut self, addr: u32, value: u8) -> bool {
        match addr & 0xFFFF {
            LATCH_ADDR => {
                if value == 0x55 {
                    self.latch_ready = true;
                }
                true
            }
            SAMPLE_ADDR => {
                if value == 0xAA && self.latch_ready {
                    self.latch_ready = false;
                    self.x_latch = map_component(f32::from_bits(self.host_x_bits));
                    self.y_latch = map_component(f32::from_bits(self.host_y_bits));
                }
                true
            }
            X_LOW_ADDR | X_HIGH_ADDR | Y_LOW_ADDR | Y_HIGH_ADDR => true,
            _ => false,
        }
    }
}

impl super::Cartridge {
    pub(crate) fn write_tilt_execution_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.tilt.is_some());
        if let Some(tilt) = &self.tilt {
            let state = tilt.state();
            writer.write_u32(state.host_x_bits);
            writer.write_u32(state.host_y_bits);
            writer.write_u16(state.x_latch);
            writer.write_u16(state.y_latch);
            writer.write_bool(state.latch_ready);
        }
    }

    pub(crate) fn read_tilt_execution_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(
            reader.read_bool()? == self.tilt.is_some(),
            "GBA tilt state does not match the cartridge"
        );
        let Some(_) = self.tilt else {
            return Ok(());
        };
        let state = TiltState {
            host_x_bits: reader.read_u32()?,
            host_y_bits: reader.read_u32()?,
            x_latch: reader.read_u16()?,
            y_latch: reader.read_u16()?,
            latch_ready: reader.read_bool()?,
        };
        self.tilt = Some(TiltSensor::from_state(state)?);
        Ok(())
    }

    pub(crate) fn reset_tilt_execution_state(&mut self) {
        if self.tilt.is_some() {
            self.tilt = Some(TiltSensor::default());
        }
    }

    pub(crate) fn reset_tilt_hardware(&mut self) {
        if let Some(tilt) = &mut self.tilt {
            tilt.reset_execution_state();
        }
    }
}

fn map_component(value: f32) -> u16 {
    let value = if value.is_nan() { 0.0 } else { value };
    let offset = (value.clamp(-1.0, 1.0) * SAMPLE_DELTA).round() as i32;
    (SAMPLE_CENTER - offset).clamp(0, 0x0FFF) as u16
}

#[cfg(test)]
mod tests {
    use super::super::{Cartridge, SensorKind};
    use super::*;

    fn cartridge(game_code: &[u8; 4]) -> Cartridge {
        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xAC..0xB0].copy_from_slice(game_code);
        rom[0xB0..0xB2].copy_from_slice(b"01");
        rom[0xB2] = 0x96;
        rom.extend_from_slice(b"EEPROM_V122");
        Cartridge::load(&rom).unwrap()
    }

    fn read_sample(sensor: &TiltSensor) -> (u16, u16) {
        let x = u16::from(sensor.read8(X_LOW_ADDR).unwrap())
            | (u16::from(sensor.read8(X_HIGH_ADDR).unwrap() & 0x0F) << 8);
        let y = u16::from(sensor.read8(Y_LOW_ADDR).unwrap())
            | (u16::from(sensor.read8(Y_HIGH_ADDR).unwrap()) << 8);
        (x, y)
    }

    #[test]
    fn requires_ordered_latch_sequence_and_maps_finite_input() {
        let mut sensor = TiltSensor::default();
        sensor.set_host_input(1.0, -1.0);

        assert!(sensor.write8(SAMPLE_ADDR, 0xAA));
        assert_eq!(read_sample(&sensor), (0x0FFF, 0x0FFF));
        assert!(sensor.write8(LATCH_ADDR, 0x55));
        assert!(sensor.write8(SAMPLE_ADDR, 0xAA));
        assert_eq!(read_sample(&sensor), (0x02A0, 0x04A0));
        assert_eq!(sensor.read8(X_HIGH_ADDR), Some(0x82));
    }

    #[test]
    fn clamps_input_and_canonicalizes_non_finite_values() {
        let mut sensor = TiltSensor::default();
        sensor.set_host_input(f32::INFINITY, f32::from_bits(0x7FC0_1234));
        assert!(sensor.write8(LATCH_ADDR, 0x55));
        assert!(sensor.write8(SAMPLE_ADDR, 0xAA));
        assert_eq!(read_sample(&sensor), (0x02A0, 0x03A0));

        sensor.set_host_input(2.0, -2.0);
        assert!(sensor.write8(LATCH_ADDR, 0x55));
        assert!(sensor.write8(SAMPLE_ADDR, 0xAA));
        assert_eq!(read_sample(&sensor), (0x02A0, 0x04A0));
    }

    #[test]
    fn known_game_codes_select_only_the_tilt_device() {
        for code in [b"KHPJ", b"KYGJ", b"KYGE", b"KYGP"] {
            assert_eq!(cartridge(code).sensor_kind(), SensorKind::Tilt);
        }

        let mut plain = cartridge(b"ABCD");
        assert_eq!(plain.sensor_kind(), SensorKind::None);
        assert!(!plain.set_tilt_input(0.25, -0.5));
    }

    #[test]
    fn cartridge_routes_tilt_registers_without_touching_eeprom() {
        let mut cartridge = cartridge(b"KYGE");
        assert!(cartridge.set_tilt_input(0.25, -0.5));
        cartridge.backup_write8(0x0E00_8000, 0x55);
        cartridge.backup_write8(0x0E00_8100, 0xAA);

        assert_eq!(cartridge.backup_read8(0x0E00_8200), 0x60);
        assert_eq!(cartridge.backup_read8(0x0E00_8300), 0x83);
        assert_eq!(cartridge.backup_read8(0x0E00_8400), 0x20);
        assert_eq!(cartridge.backup_read8(0x0E00_8500), 0x04);
        assert_eq!(cartridge.backup_read8(0x0E00_8600), 0xFF);
    }
}
