use super::logic::{read_bits, write_bits};
use super::main_bus::Phase;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, bail, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum State {
    Idle,
    GrantFirst,
    GrantHold,
    ReleaseFirst,
    ReleaseHold,
    RegrantFirst,
    RegrantHold,
}

impl State {
    fn next(self, request: bool, acknowledge: bool) -> Self {
        match self {
            Self::Idle => {
                if acknowledge {
                    Self::ReleaseHold
                } else if request {
                    Self::GrantFirst
                } else {
                    Self::Idle
                }
            }
            Self::GrantFirst => Self::GrantHold,
            Self::GrantHold => {
                if acknowledge {
                    Self::ReleaseFirst
                } else if request {
                    Self::GrantHold
                } else {
                    Self::Idle
                }
            }
            Self::ReleaseFirst => {
                if !request && !acknowledge {
                    Self::Idle
                } else {
                    Self::ReleaseHold
                }
            }
            Self::ReleaseHold => {
                if request {
                    Self::RegrantFirst
                } else if acknowledge {
                    Self::ReleaseHold
                } else {
                    Self::Idle
                }
            }
            Self::RegrantFirst => Self::RegrantHold,
            Self::RegrantHold => match (request, acknowledge) {
                (true, true) => Self::RegrantHold,
                (true, false) => Self::GrantHold,
                (false, true) => Self::ReleaseFirst,
                (false, false) => Self::Idle,
            },
        }
    }

    fn read(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(match reader.read_u8()? {
            0 => Self::Idle,
            1 => Self::GrantFirst,
            2 => Self::GrantHold,
            3 => Self::ReleaseFirst,
            4 => Self::ReleaseHold,
            5 => Self::RegrantFirst,
            6 => Self::RegrantHold,
            _ => bail!("invalid MAIN arbitration node"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grant {
    state: State,
    prepared: State,
    sampled: [bool; 2],
    valid: [bool; 2],
}

impl Grant {
    pub const fn software_origin() -> Self {
        Self {
            state: State::Idle,
            prepared: State::Idle,
            sampled: [false; 2],
            valid: [false; 2],
        }
    }

    pub fn edge(&mut self, high: bool, phase: Phase, request: bool, acknowledge: bool) {
        if high {
            self.state = self.prepared;
        } else {
            self.valid = self.sampled;
            self.sampled = [request, acknowledge];
            self.prepared = if matches!(phase, Phase::S0 | Phase::S1) {
                self.state
            } else {
                self.state.next(self.valid[0], self.valid[1])
            };
        }
    }

    pub fn granted(self) -> bool {
        matches!(
            self.state,
            State::GrantFirst | State::GrantHold | State::RegrantFirst | State::RegrantHold
        )
    }

    pub fn tristate(self) -> bool {
        self.state != State::Idle
    }

    pub fn write_state(self, writer: &mut StateWriter) {
        writer.write_u8(self.state as u8);
        writer.write_u8(self.prepared as u8);
        write_bits(writer, self.sampled);
        write_bits(writer, self.valid);
    }

    pub fn read_state(reader: &mut StateReader<'_>, high: bool, phase: Phase) -> Result<Self> {
        let value = Self {
            state: State::read(reader)?,
            prepared: State::read(reader)?,
            sampled: read_bits(reader)?,
            valid: read_bits(reader)?,
        };
        value.validate(high, phase)?;
        Ok(value)
    }

    pub fn validate(self, high: bool, phase: Phase) -> Result<()> {
        let expected = if high || matches!(phase, Phase::S0 | Phase::S1) {
            self.state
        } else {
            self.state.next(self.valid[0], self.valid[1])
        };
        ensure!(
            self.prepared == expected,
            "invalid MAIN arbitration preparation"
        );
        ensure!(
            !matches!(phase, Phase::S0 | Phase::S1) || !self.tristate(),
            "MAIN cycle offered while bus relinquished"
        );
        Ok(())
    }
}
