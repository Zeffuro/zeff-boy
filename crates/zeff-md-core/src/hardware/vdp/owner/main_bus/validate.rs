use super::*;

impl MainBus {
    pub fn validate(&self) -> Result<(), Error> {
        let s = self.state;
        let phase = s
            .last_edge_half
            .checked_sub(s.origin_rise_half)
            .ok_or(Error::InvalidState)?
            % PERIOD;
        if phase != if s.clock_high { 0 } else { HIGH }
            || s.now_half < s.last_edge_half
            || s.now_half > self.next_edge_half()?
            || (s.phase == Phase::Idle) != s.pending.is_none()
            || s.pending.is_some() && s.completed.is_some()
        {
            return Err(Error::InvalidState);
        }
        s.enable.validate(s)?;
        if let Some(p) = s.pending {
            p.request.validate()?;
            if !self.rise(p.start_half)
                || p.request.access.function_code != s.function_code
                || s.clock_high != s.phase.high()
            {
                return Err(Error::InvalidState);
            }
            if let Some(sync) = p.vpa {
                self.validate_vpa_pending(p, sync)?;
            } else {
                let (base, waits) = match s.phase {
                    Phase::S0 => (0, 0),
                    Phase::S1 => (8, 0),
                    Phase::S2 => (14, 0),
                    Phase::S3 => (22, 0),
                    Phase::S4 => (28, 0),
                    Phase::WaitLow => (36, p.waits.checked_sub(1).ok_or(Error::InvalidState)?),
                    Phase::WaitHigh => (42, p.waits.checked_sub(1).ok_or(Error::InvalidState)?),
                    Phase::S5 => (36, p.waits),
                    Phase::S6 => (42, p.waits),
                    Phase::S7 => (50, p.waits),
                    Phase::VpaLow | Phase::VpaHigh => return Err(Error::InvalidState),
                    Phase::Idle => return Err(Error::InvalidState),
                };
                let accepted = matches!(s.phase, Phase::S5 | Phase::S6 | Phase::S7);
                let ack = if accepted {
                    Some(offset(p.start_half, 36, p.waits)?)
                } else {
                    None
                };
                if s.last_edge_half != offset(p.start_half, base, waits)?
                    || !accepted
                        && !matches!(s.phase, Phase::WaitLow | Phase::WaitHigh)
                        && p.waits != 0
                    || p.sampled_ack_half != ack
                    || p.read_word.is_some() != (s.phase == Phase::S7 && p.request.reads())
                    || accepted && p.request.operation == Operation::InterruptAcknowledge
                {
                    return Err(Error::InvalidState);
                }
            }
        }
        if let Some(r) = s.completed {
            r.request.validate()?;
            if !self.rise(r.start_half)
                || r.request.access.function_code != s.function_code
                || r.end_half != offset(r.start_half, 56, r.waits)?
                || r.end_half > s.last_edge_half
            {
                return Err(Error::InvalidState);
            }
            if let Some(sync) = r.vpa {
                sync.validate(s.origin_rise_half, r.start_half, r.waits)?;
                if r.sampled_ack_half != sync.recognition_half
                    || r.read_word.is_some()
                    || r.request.interrupt_level().map(|level| 24 + level) != r.autovector
                    || r.autovector.is_none()
                {
                    return Err(Error::InvalidState);
                }
            } else if r.autovector.is_some()
                || r.request.operation == Operation::InterruptAcknowledge
                || r.sampled_ack_half != offset(r.start_half, 36, r.waits)?
                || r.read_word.is_some() != r.request.reads()
            {
                return Err(Error::InvalidState);
            }
        }
        Ok(())
    }

    fn validate_vpa_pending(&self, p: Pending, sync: VpaSync) -> Result<(), Error> {
        let s = self.state;
        sync.validate(s.origin_rise_half, p.start_half, p.waits)?;
        if p.request.interrupt_level().is_none()
            || p.read_word.is_some()
            || p.sampled_ack_half != Some(sync.recognition_half)
        {
            return Err(Error::InvalidState);
        }
        let valid = match s.phase {
            Phase::VpaLow => {
                s.last_edge_half >= sync.recognition_half
                    && s.last_edge_half <= sync.fall_half - PERIOD
            }
            Phase::VpaHigh => {
                s.last_edge_half > sync.recognition_half && s.last_edge_half < sync.fall_half - HIGH
            }
            Phase::S6 => s.last_edge_half == sync.fall_half - HIGH,
            Phase::S7 => s.last_edge_half == sync.fall_half,
            _ => false,
        };
        if !valid {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
}
