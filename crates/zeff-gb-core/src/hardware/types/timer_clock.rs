use crate::hardware::types::hardware_mode::HardwareMode;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TimerClock {
    Div256, // 00
    Div4,   // 01
    Div16,  // 10
    Div64,  // 11
}

impl TimerClock {
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => TimerClock::Div256,
            1 => TimerClock::Div4,
            2 => TimerClock::Div16,
            3 => TimerClock::Div64,
            _ => unreachable!(),
        }
    }

    pub fn increment_cycles(self, mode: HardwareMode) -> u32 {
        match (self, mode) {
            (TimerClock::Div256, HardwareMode::DMG)
            | (TimerClock::Div256, HardwareMode::SGB2)
            | (TimerClock::Div256, HardwareMode::CGBNormal)
            | (TimerClock::Div256, HardwareMode::CGBDouble) => 1024,

            (TimerClock::Div4, HardwareMode::DMG)
            | (TimerClock::Div4, HardwareMode::SGB2)
            | (TimerClock::Div4, HardwareMode::CGBNormal)
            | (TimerClock::Div4, HardwareMode::CGBDouble) => 16,

            (TimerClock::Div16, HardwareMode::DMG)
            | (TimerClock::Div16, HardwareMode::SGB2)
            | (TimerClock::Div16, HardwareMode::CGBNormal)
            | (TimerClock::Div16, HardwareMode::CGBDouble) => 64,

            (TimerClock::Div64, HardwareMode::DMG)
            | (TimerClock::Div64, HardwareMode::SGB2)
            | (TimerClock::Div64, HardwareMode::CGBNormal)
            | (TimerClock::Div64, HardwareMode::CGBDouble) => 256,

            (TimerClock::Div256, HardwareMode::SGB1) => 1024,
            (TimerClock::Div4, HardwareMode::SGB1) => 16,
            (TimerClock::Div16, HardwareMode::SGB1) => 64,
            (TimerClock::Div64, HardwareMode::SGB1) => 256,
        }
    }
}
