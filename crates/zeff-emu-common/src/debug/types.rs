use crate::address::Address;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugEvent {
    Interrupt,
    Dma,
    Exception,
}

impl DebugEvent {
    pub const ALL: [Self; 3] = [Self::Interrupt, Self::Dma, Self::Exception];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Interrupt => "IRQ / NMI",
            Self::Dma => "DMA transfer",
            Self::Exception => "CPU exception",
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Interrupt => 0,
            Self::Dma => 1,
            Self::Exception => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchType {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakpointHitCondition {
    pub address: Address,
    pub target_hits: u64,
    pub hits: u64,
}

#[derive(Clone, Debug)]
pub struct AddressWatchpoint {
    pub address: Address,
    pub end_address: Address,
    pub watch_type: WatchType,
    pub last_value: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct AddressWatchHit {
    pub address: Address,
    pub old_value: u8,
    pub new_value: u8,
    pub watch_type: WatchType,
}

#[derive(Clone, Debug)]
pub struct Watchpoint {
    pub address: u16,
    pub end_address: u16,
    pub watch_type: WatchType,
    pub last_value: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct WatchHit {
    pub address: u16,
    pub old_value: u8,
    pub new_value: u8,
    pub watch_type: WatchType,
}
