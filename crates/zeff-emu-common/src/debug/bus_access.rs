use super::{TraceWriteKind, TraceWriteWidth};
use crate::time::MasterTicks;

pub type BusAccessSpace = TraceWriteKind;
pub type BusAccessWidth = TraceWriteWidth;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusAccessEvent {
    Read {
        at: Option<MasterTicks>,
        space: BusAccessSpace,
        addr: u32,
        value: u32,
        width: BusAccessWidth,
        mapped_addr: Option<u32>,
    },
    Write {
        at: Option<MasterTicks>,
        space: BusAccessSpace,
        addr: u32,
        old_value: u32,
        written_value: u32,
        new_value: u32,
        width: BusAccessWidth,
        mapped_addr: Option<u32>,
    },
}

impl BusAccessEvent {
    pub const fn at(self) -> Option<MasterTicks> {
        match self {
            Self::Read { at, .. } | Self::Write { at, .. } => at,
        }
    }

    pub const fn addr(self) -> u32 {
        match self {
            Self::Read { addr, .. } | Self::Write { addr, .. } => addr,
        }
    }

    pub const fn width(self) -> BusAccessWidth {
        match self {
            Self::Read { width, .. } | Self::Write { width, .. } => width,
        }
    }

    pub const fn space(self) -> BusAccessSpace {
        match self {
            Self::Read { space, .. } | Self::Write { space, .. } => space,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_metadata_is_available_without_matching_the_variant() {
        let event = BusAccessEvent::Read {
            at: Some(MasterTicks::new(12)),
            space: TraceWriteKind::Memory,
            addr: 0xC000,
            value: 0x42,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        };

        assert_eq!(event.at(), Some(MasterTicks::new(12)));
        assert_eq!(event.addr(), 0xC000);
        assert_eq!(event.width(), TraceWriteWidth::Byte);
        assert_eq!(event.space(), TraceWriteKind::Memory);
    }
}
