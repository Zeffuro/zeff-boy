use crate::hardware::apu::Apu;
use crate::hardware::joypad::Joypad;
use crate::hardware::ppu::PPU;
use crate::hardware::serial::Serial;
use crate::hardware::sgb::SgbState;
use crate::hardware::timer::Timer;
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

pub(crate) struct IO {
    pub(crate) joypad: Joypad,
    pub(crate) serial: Serial,
    pub(crate) timer: Timer,
    pub(crate) ppu: PPU,
    pub(crate) apu: Apu,
    pub(crate) sgb: SgbState,
}

impl IO {
    pub(crate) fn new() -> Self {
        Self {
            joypad: Joypad::new(),
            serial: Serial::new(),
            timer: Timer::new(),
            ppu: PPU::new(),
            apu: Apu::new(),
            sgb: SgbState::new(),
        }
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        self.joypad.write_state(writer);
        self.serial.write_state(writer);
        self.timer.write_state(writer);
        self.ppu.write_state(writer);
        self.apu.write_state(writer);
        self.sgb.write_state(writer);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            joypad: Joypad::read_state(reader)?,
            serial: Serial::read_state(reader)?,
            timer: Timer::read_state(reader)?,
            ppu: PPU::read_state(reader)?,
            apu: Apu::read_state(reader)?,
            sgb: SgbState::read_state(reader)?,
        })
    }
}
