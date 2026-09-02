use crate::hardware::apu::Apu;
use crate::hardware::joypad::Joypad;
use crate::hardware::ppu::PPU;
use crate::hardware::serial::{Serial, SerialDevicePort};
use crate::hardware::sgb::SgbState;
use crate::hardware::timer::Timer;
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;
use std::fmt;

pub struct IO {
    pub(super) joypad: Joypad,
    pub(super) serial: Serial,
    pub(super) timer: Timer,
    pub(super) ppu: PPU,
    pub(super) apu: Apu,
    pub(super) sgb: SgbState,
    pub(super) serial_device: SerialDevicePort,
}

impl fmt::Debug for IO {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IO")
            .field("joypad", &self.joypad)
            .field("serial", &self.serial)
            .field("timer", &self.timer)
            .field("ppu", &self.ppu)
            .field("apu", &self.apu)
            .field("sgb", &self.sgb)
            .field("serial_device", &self.serial_device)
            .finish()
    }
}

impl IO {
    pub fn new() -> Self {
        Self {
            joypad: Joypad::new(),
            serial: Serial::new(),
            timer: Timer::new(),
            ppu: PPU::new(),
            apu: Apu::new(),
            sgb: SgbState::new(),
            serial_device: SerialDevicePort::new(),
        }
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        self.joypad.write_state(writer);
        self.serial.write_state(writer);
        self.timer.write_state(writer);
        self.ppu.write_state(writer);
        self.apu.write_state(writer);
        self.sgb.write_state(writer);
        self.serial_device.write_state(writer);
    }

    pub fn read_state(reader: &mut StateReader<'_>, format_version: u32) -> Result<Self> {
        Ok(Self {
            joypad: Joypad::read_state(reader)?,
            serial: Serial::read_state(reader)?,
            timer: Timer::read_state(reader)?,
            ppu: PPU::read_state(reader, format_version)?,
            apu: Apu::read_state(reader, format_version)?,
            sgb: SgbState::read_state(reader, format_version)?,
            serial_device: SerialDevicePort::read_state(reader, format_version)?,
        })
    }

    pub(super) fn write_sgb_native_continuation(&self, writer: &mut StateWriter) {
        self.sgb.write_native_continuation(writer);
    }

    pub(super) fn read_sgb_native_continuation(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<()> {
        self.sgb.read_native_continuation(reader)
    }
}
