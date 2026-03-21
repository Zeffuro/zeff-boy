use crate::hardware::ppu::PPU;
use crate::hardware::serial::Serial;
use crate::hardware::timer::Timer;

pub(crate) struct IO {
    pub(crate) serial: Serial,
    pub(crate) timer: Timer,
    pub(crate) ppu: PPU,
}

impl IO {
    pub(crate) fn new() -> Self {
        Self {
            serial: Serial::new(),
            timer: Timer::new(),
            ppu: PPU::new(),
        }
    }
}