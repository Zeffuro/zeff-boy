use super::{Binding, Occurrence, Stamp};

#[cfg(test)]
pub(crate) mod raw;
mod shadow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Address,
    Data,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Foreign,
    Unknown,
    Sound {
        logical: u16,
        bank: u16,
        write: bool,
    },
    ArbiterHigh,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mux {
    pub released: bool,
    pub held_strobes: bool,
    pub held_read: bool,
    pub source: Source,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Primitive {
    pub ordinal: u8,
    pub address: u32,
    pub bus_address: u32,
    pub word: u16,
    pub lanes: [bool; 2],
    pub main: [bool; 3],
    pub intake: [bool; 3],
    pub ack: [bool; 3],
    pub psg: [bool; 2],
    pub load: Option<(u16, [bool; 2])>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Drive {
        slot: Slot,
        mask: u32,
        source: Source,
    },
    Arbiter(Source),
    Outputs,
    Resolve(Mux),
    Terminal,
    Readback,
    Capture {
        dma: bool,
    },
    Primitive(Primitive),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Data,
    ControlFirst,
    ControlSecond,
    DisconnectedPsg,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Acceptance {
    pub primitive: u8,
    pub source: Stamp,
    pub binding: Binding,
    pub kind: Kind,
    pub address: u32,
    pub bus_address: u32,
    pub word: u16,
    pub lanes: [bool; 2],
}
pub(crate) trait Capture {
    fn vdp(&mut self, event: Event);
    #[cfg(test)]
    fn vdp_raw(&mut self, _event: raw::Event) {}
}
impl Capture for () {
    fn vdp(&mut self, _: Event) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token {
    id: Occurrence,
    binding: Binding,
    stamp: Stamp,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Origin {
    #[default]
    Unknown,
    Foreign,
    Fixed,
    Sound(Token),
}
#[derive(Clone, Default)]
struct Bus {
    address: [Origin; 24],
    data: [Origin; 16],
    controls: [Origin; 4],
}
#[derive(Clone, Default)]
pub(super) struct Shadow {
    address: [Origin; 24],
    data: [Origin; 16],
    held: [Origin; 3],
    arbiter: Origin,
    outputs: Origin,
    input: Bus,
    wiring: Bus,
}
