use crate::hardware::ppu::PPU;
use crate::hardware::joypad::Joypad;
use crate::hardware::sgb::SgbState;
use crate::hardware::serial::Serial;
use crate::hardware::timer::Timer;
use crate::hardware::apu::Apu;

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
}