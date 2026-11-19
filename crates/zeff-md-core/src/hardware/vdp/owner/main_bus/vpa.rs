use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EnableClock {
    pub phase: u8,
    pub last_fall_half: Option<u64>,
}

impl EnableClock {
    pub fn software_origin() -> Self {
        // Software origin starts low; only observed CPU falling edges advance E.
        Self {
            phase: 0,
            last_fall_half: None,
        }
    }

    pub fn falling(&mut self, half: u64) {
        self.phase = (self.phase + 1) % 10;
        self.last_fall_half = Some(half);
    }

    pub fn validate(self, state: Snapshot) -> Result<(), Error> {
        let elapsed = state
            .last_edge_half
            .checked_sub(state.origin_rise_half)
            .ok_or(Error::InvalidState)?;
        let falls = elapsed / PERIOD + u64::from(!state.clock_high);
        let last = if falls == 0 {
            None
        } else {
            Some(
                state
                    .origin_rise_half
                    .checked_add((falls - 1) * PERIOD)
                    .and_then(|half| half.checked_add(HIGH))
                    .ok_or(Error::ClockOverflow)?,
            )
        };
        if self.phase != (falls % 10) as u8 || self.last_fall_half != last {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VpaSync {
    pub recognition_half: u64,
    pub rise_half: u64,
    pub fall_half: u64,
}

impl VpaSync {
    pub fn select(recognition_half: u64, phase: u8) -> Result<Self, Error> {
        if phase >= 10 {
            return Err(Error::InvalidState);
        }
        let mut clocks = (16 - u64::from(phase)) % 10;
        if clocks < 3 {
            clocks += 10;
        }
        let rise_half = recognition_half
            .checked_add(clocks * PERIOD)
            .ok_or(Error::ClockOverflow)?;
        let fall_half = rise_half
            .checked_add(4 * PERIOD)
            .ok_or(Error::ClockOverflow)?;
        let sync = Self {
            recognition_half,
            rise_half,
            fall_half,
        };
        sync.end_half()?;
        Ok(sync)
    }

    pub fn end_half(self) -> Result<u64, Error> {
        self.fall_half.checked_add(LOW).ok_or(Error::ClockOverflow)
    }

    pub fn validate(self, origin: u64, start: u64, waits: u32) -> Result<(), Error> {
        let elapsed = self
            .recognition_half
            .checked_sub(origin)
            .ok_or(Error::InvalidState)?;
        let before = self
            .recognition_half
            .checked_sub(start)
            .ok_or(Error::InvalidState)?;
        if elapsed % PERIOD != HIGH || before < 36 || (before - 36) % PERIOD != 0 {
            return Err(Error::InvalidState);
        }
        let phase = ((elapsed / PERIOD + 1) % 10) as u8;
        if self != Self::select(self.recognition_half, phase)?
            || self.end_half()? != offset(start, 56, waits)?
        {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
}

impl MainBus {
    pub(crate) fn check_termination(&self, sample: Sample) -> Result<(), Error> {
        let Some(pending) = self.state.pending else {
            return Ok(());
        };
        let iack = pending.request.operation == Operation::InterruptAcknowledge;
        if sample.berr
            || sample.vpa && (!iack || sample.dtack)
            || iack && sample.dtack
            || pending.vpa.is_some() && self.state.phase != Phase::S7 && !sample.vpa
        {
            return Err(Error::UnsupportedTermination);
        }
        Ok(())
    }
}
