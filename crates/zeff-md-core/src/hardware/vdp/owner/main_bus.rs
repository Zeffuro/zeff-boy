use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, FunctionCode};

mod state;
mod validate;
mod vpa;

pub(crate) use vpa::{EnableClock, VpaSync};

const PERIOD: u64 = 14;
const HIGH: u64 = 8;
const LOW: u64 = PERIOD - HIGH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Read,
    Write(u16),
    InterruptAcknowledge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub access: BusAccess,
    pub operation: Operation,
}

impl Request {
    fn validate(self) -> Result<(), Error> {
        let address = self.access.logical_address & BUS_ADDRESS_MASK;
        let lanes_match = match self.access.lanes {
            ByteLanes::Both | ByteLanes::Upper => address & 1 == 0,
            ByteLanes::Lower => address & 1 != 0,
        };
        let iack = self.operation == Operation::InterruptAcknowledge;
        if self.access.bus_address != address & !1
            || !lanes_match
            || iack != (self.access.kind == AccessKind::InterruptAcknowledge)
            || iack != (self.access.function_code == FunctionCode::CpuSpace)
        {
            return Err(Error::InvalidRequest);
        }
        if iack && self.interrupt_level().is_none() {
            return Err(Error::InvalidRequest);
        }
        if let Operation::Write(word) = self.operation
            && self.access.lanes != ByteLanes::Both
            && word as u8 != (word >> 8) as u8
        {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }

    fn reads(self) -> bool {
        !matches!(self.operation, Operation::Write(_))
    }

    fn interrupt_level(self) -> Option<u8> {
        let level = ((self.access.bus_address >> 1) & 7) as u8;
        (self.operation == Operation::InterruptAcknowledge
            && level != 0
            && self.access.lanes == ByteLanes::Both
            && self.access.logical_address == (0xffff_fff0 | (u32::from(level) << 1))
            && self.access.bus_address == (0x00ff_fff0 | (u32::from(level) << 1)))
            .then_some(level)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Phase {
    Idle = 0,
    S0 = 1,
    S1 = 2,
    S2 = 3,
    S3 = 4,
    S4 = 5,
    WaitLow = 6,
    WaitHigh = 7,
    S5 = 8,
    S6 = 9,
    S7 = 10,
    VpaLow = 11,
    VpaHigh = 12,
}

impl Phase {
    fn high(self) -> bool {
        matches!(
            self,
            Self::S0 | Self::S2 | Self::S4 | Self::WaitHigh | Self::S6 | Self::VpaHigh
        )
    }

    fn strobes(self) -> bool {
        matches!(
            self,
            Self::S2
                | Self::S3
                | Self::S4
                | Self::WaitLow
                | Self::WaitHigh
                | Self::S5
                | Self::S6
                | Self::VpaLow
                | Self::VpaHigh
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pending {
    pub request: Request,
    pub start_half: u64,
    pub waits: u32,
    pub sampled_ack_half: Option<u64>,
    pub read_word: Option<u16>,
    pub vpa: Option<VpaSync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub request: Request,
    pub start_half: u64,
    pub end_half: u64,
    pub sampled_ack_half: u64,
    pub waits: u32,
    pub read_word: Option<u16>,
    pub vpa: Option<VpaSync>,
    pub autovector: Option<u8>,
}

impl Receipt {
    pub fn end_tick(self) -> u64 {
        ceil_tick(self.end_half)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub origin_rise_half: u64,
    pub now_half: u64,
    pub last_edge_half: u64,
    pub clock_high: bool,
    pub enable: EnableClock,
    pub function_code: FunctionCode,
    pub phase: Phase,
    pub pending: Option<Pending>,
    pub completed: Option<Receipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MainBus {
    state: Snapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sample {
    pub half: u64,
    pub clock_high: bool,
    pub dtack: bool,
    pub vpa: bool,
    pub berr: bool,
    pub read_word: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pins {
    pub function_code: FunctionCode,
    pub address: Option<u32>,
    pub read: bool,
    pub address_strobe: bool,
    pub upper_strobe: bool,
    pub lower_strobe: bool,
    pub write_word: Option<u16>,
    pub write_lanes: Option<ByteLanes>,
    pub interrupt_acknowledge: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    InvalidRequest,
    InvalidState,
    ClockReversal,
    ClockGeometry,
    ClockOverflow,
    NotReady,
    ReceiptPending,
    MissingReadData,
    UnsupportedTermination,
}

pub(crate) fn ceil_tick(half: u64) -> u64 {
    half / 2 + half % 2
}

fn offset(start: u64, base: u64, waits: u32) -> Result<u64, Error> {
    start
        .checked_add(base)
        .and_then(|at| at.checked_add(u64::from(waits) * PERIOD))
        .ok_or(Error::ClockOverflow)
}

impl MainBus {
    // The caller owns the complete fresh clock prefix ending at this actual rise.
    pub fn at_origin(rise_half: u64, function_code: FunctionCode) -> Result<Self, Error> {
        if function_code == FunctionCode::CpuSpace {
            return Err(Error::InvalidRequest);
        }
        Self::from_snapshot(Snapshot {
            origin_rise_half: rise_half,
            now_half: rise_half,
            last_edge_half: rise_half,
            clock_high: true,
            enable: EnableClock::software_origin(),
            function_code,
            phase: Phase::Idle,
            pending: None,
            completed: None,
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state
    }

    pub fn from_snapshot(state: Snapshot) -> Result<Self, Error> {
        let result = Self { state };
        result.validate()?;
        Ok(result)
    }

    pub fn next_edge_half(&self) -> Result<u64, Error> {
        self.state
            .last_edge_half
            .checked_add(if self.state.clock_high { HIGH } else { LOW })
            .ok_or(Error::ClockOverflow)
    }

    pub fn ready_tick(&self) -> Option<u64> {
        let s = self.state;
        (s.phase == Phase::Idle
            && s.completed.is_none()
            && s.clock_high
            && s.now_half - s.last_edge_half <= s.last_edge_half % 2)
            .then_some(ceil_tick(s.last_edge_half))
    }

    pub(crate) fn origin_start_valid(&self, start_half: u64) -> bool {
        start_half <= self.state.now_half && self.rise(start_half)
    }

    pub fn offer(&mut self, request: Request) -> Result<(), Error> {
        request.validate()?;
        if self.state.completed.is_some() {
            return Err(Error::ReceiptPending);
        }
        if self.ready_tick().is_none() {
            return Err(Error::NotReady);
        }
        self.state.function_code = request.access.function_code;
        self.state.phase = Phase::S0;
        self.state.pending = Some(Pending {
            request,
            start_half: self.state.last_edge_half,
            waits: 0,
            sampled_ack_half: None,
            read_word: None,
            vpa: None,
        });
        Ok(())
    }

    pub fn take_completed(&mut self) -> Option<Receipt> {
        self.state.completed.take()
    }

    pub fn pins(&self) -> Pins {
        let s = self.state;
        let mut pins = Pins {
            function_code: s.function_code,
            address: None,
            read: true,
            address_strobe: false,
            upper_strobe: false,
            lower_strobe: false,
            write_word: None,
            write_lanes: None,
            interrupt_acknowledge: s.function_code == FunctionCode::CpuSpace,
        };
        let Some(p) = s.pending else {
            return pins;
        };
        if s.phase != Phase::S0 {
            pins.address = Some(p.request.access.bus_address);
        }
        pins.address_strobe = s.phase.strobes();
        let data_strobe =
            pins.address_strobe && (p.request.reads() || !matches!(s.phase, Phase::S2 | Phase::S3));
        pins.upper_strobe = data_strobe && p.request.access.lanes != ByteLanes::Lower;
        pins.lower_strobe = data_strobe && p.request.access.lanes != ByteLanes::Upper;
        if let Operation::Write(word) = p.request.operation {
            pins.read = matches!(s.phase, Phase::S0 | Phase::S1);
            if !matches!(s.phase, Phase::S0 | Phase::S1 | Phase::S2) {
                pins.write_word = Some(word);
                pins.write_lanes = Some(p.request.access.lanes);
            }
        }
        pins
    }

    // Inputs are the resolved pre-edge bus levels; no same-edge ACK is resampled.
    pub fn advance(&mut self, sample: Sample) -> Result<(), Error> {
        let mut candidate = self.clone();
        candidate.advance_inner(sample)?;
        *self = candidate;
        Ok(())
    }

    fn advance_inner(&mut self, sample: Sample) -> Result<(), Error> {
        if sample.half < self.state.now_half {
            return Err(Error::ClockReversal);
        }
        self.check_termination(sample)?;
        let edge = self.next_edge_half()?;
        if sample.clock_high == self.state.clock_high {
            if sample.half > edge {
                return Err(Error::ClockGeometry);
            }
            self.state.now_half = sample.half;
            return Ok(());
        }
        if sample.half != edge {
            return Err(Error::ClockGeometry);
        }
        self.state.now_half = edge;
        self.state.last_edge_half = edge;
        self.state.clock_high = sample.clock_high;
        if !sample.clock_high {
            self.state.enable.falling(edge);
        }
        let Some(mut p) = self.state.pending else {
            return Ok(());
        };
        self.state.phase = match self.state.phase {
            Phase::S0 => Phase::S1,
            Phase::S1 => Phase::S2,
            Phase::S2 => Phase::S3,
            Phase::S3 => Phase::S4,
            Phase::S4 | Phase::WaitHigh => {
                if sample.vpa {
                    let sync = VpaSync::select(edge, self.state.enable.phase)?;
                    let end = sync.end_half()?;
                    p.waits = u32::try_from((end - p.start_half - 56) / PERIOD)
                        .map_err(|_| Error::ClockOverflow)?;
                    p.vpa = Some(sync);
                    p.sampled_ack_half = Some(edge);
                    Phase::VpaLow
                } else if sample.dtack {
                    p.sampled_ack_half = Some(edge);
                    Phase::S5
                } else {
                    p.waits = p.waits.checked_add(1).ok_or(Error::ClockOverflow)?;
                    Phase::WaitLow
                }
            }
            Phase::WaitLow => Phase::WaitHigh,
            Phase::VpaLow => {
                if edge == p.vpa.ok_or(Error::InvalidState)?.fall_half - HIGH {
                    Phase::S6
                } else {
                    Phase::VpaHigh
                }
            }
            Phase::VpaHigh => Phase::VpaLow,
            Phase::S5 => Phase::S6,
            Phase::S6 => {
                if p.request.reads() && p.vpa.is_none() {
                    p.read_word = Some(sample.read_word.ok_or(Error::MissingReadData)?);
                }
                Phase::S7
            }
            Phase::S7 => {
                self.state.completed = Some(Receipt {
                    request: p.request,
                    start_half: p.start_half,
                    end_half: edge,
                    sampled_ack_half: p.sampled_ack_half.ok_or(Error::InvalidState)?,
                    waits: p.waits,
                    read_word: p.read_word,
                    vpa: p.vpa,
                    autovector: if p.vpa.is_some() {
                        Some(24 + p.request.interrupt_level().ok_or(Error::InvalidState)?)
                    } else {
                        None
                    },
                });
                self.state.pending = None;
                self.state.phase = Phase::Idle;
                return Ok(());
            }
            Phase::Idle => return Err(Error::InvalidState),
        };
        self.state.pending = Some(p);
        Ok(())
    }

    fn rise(&self, half: u64) -> bool {
        half.checked_sub(self.state.origin_rise_half)
            .is_some_and(|elapsed| elapsed.is_multiple_of(PERIOD))
    }
}
