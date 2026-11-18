use super::{Primitive, Slot};
use crate::hardware::vdp::owner::Companion;
use crate::sound_trace::physical::Stamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Driver {
    Vdp,
    Status,
    SoundLow,
    SoundHigh,
    SoundData,
    Undriven,
    Main,
    Protocol,
    Ram,
    Cartridge,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pins {
    pub address: u32,
    pub word: u16,
    pub controls: [bool; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Companion(Companion),
    Drive {
        driver: Driver,
        slot: Slot,
        mask: u32,
        value: u32,
    },
    Arbiter {
        logical: u16,
        bank: u16,
        write: bool,
        high: u16,
        levels: [bool; 3],
    },
    Outputs {
        levels: [bool; 3],
    },
    Resolve {
        released: bool,
        held_strobes: bool,
        held_read: bool,
        pins: Pins,
    },
    Terminal,
    Readback,
    Capture {
        dma: bool,
        address: u32,
        pins: Pins,
    },
    Primitive(Primitive),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Record {
    pub at: Stamp,
    pub event: Event,
}
