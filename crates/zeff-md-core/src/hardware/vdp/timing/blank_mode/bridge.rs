use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter, read_optional, write_optional};
use anyhow::{Result, ensure};

impl BlankHistory {
    pub fn check_data_publication(
        &self,
        at: MasterTicks,
        registers: &VdpRegisterPipeline,
    ) -> Result<(), VdpTimingError> {
        if self.data_inflight.is_some()
            && (self.active
                || registers.pending().is_some_and(|w| {
                    w.index == MODE_REGISTER && w.value & MODE5_ENABLE == 0 && w.publish_at <= at
                }))
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        Ok(())
    }

    pub fn begin_data(&mut self, begin: MasterTicks, write: bool) -> Result<(), VdpTimingError> {
        if self.active
            || (!self.eligible && self.vertical.is_none())
            || begin != self.now
            || self.port.is_some()
            || self.data_inflight.is_some()
            || self.control_completed.is_some()
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        begin
            .get()
            .checked_add(28)
            .ok_or(VdpTimingError::ClockOverflow)?;
        self.data_inflight = Some((begin, write));
        self.port_ready = false;
        self.quiet_since = None;
        self.last_release = None;
        Ok(())
    }

    pub fn finish_data(&mut self, end: MasterTicks, write: bool) -> Result<(), VdpTimingError> {
        let Some((begin, direction)) = self.data_inflight else {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        };
        if self.active
            || direction != write
            || end != self.now
            || !begin
                .get()
                .checked_add(28)
                .is_some_and(|first| end.get() >= first)
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        end.get()
            .checked_add(15)
            .ok_or(VdpTimingError::ClockOverflow)?;
        self.data_inflight = None;
        self.quiet_since = Some(end);
        Ok(())
    }

    pub fn complete_control(&mut self, end: MasterTicks) -> Result<(), VdpTimingError> {
        let Some(port) = self
            .port
            .filter(|p| p.kind == PortKind::Control && p.end == end && end == self.now)
        else {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        };
        self.port = None;
        self.port_ready = true;
        self.quiet_since = None;
        self.last_release = Some(port.sample);
        self.control_completed = Some(end);
        Ok(())
    }

    pub fn write_bridge(self, w: &mut StateWriter) {
        write_optional(w, self.data_inflight, |w, (at, write)| {
            w.write_u64(at.get());
            w.write_bool(write);
        });
        write_optional(w, self.quiet_since, |w, at| w.write_u64(at.get()));
        write_optional(w, self.control_completed, |w, at| w.write_u64(at.get()));
    }

    pub fn read_bridge(&mut self, r: &mut StateReader<'_>) -> Result<()> {
        self.data_inflight =
            read_optional(r, |r| Ok((MasterTicks::new(r.read_u64()?), r.read_bool()?)))?;
        self.quiet_since = read_optional(r, |r| Ok(MasterTicks::new(r.read_u64()?)))?;
        self.control_completed = read_optional(r, |r| Ok(MasterTicks::new(r.read_u64()?)))?;
        Ok(())
    }

    pub fn validate_bridge(&self, now: MasterTicks) -> Result<()> {
        if let Some((begin, _)) = self.data_inflight {
            ensure!(
                !self.active
                    && (self.eligible || self.vertical.is_some())
                    && !self.port_ready
                    && self.core.epoch <= begin
                    && begin <= now
                    && begin.get().checked_add(28).is_some()
                    && self.port.is_none()
                    && self.quiet_since.is_none()
                    && self.last_release.is_none()
                    && self.control_completed.is_none(),
                "invalid VDP data lifetime"
            );
        }
        if let Some(at) = self.quiet_since {
            ensure!(
                self.core
                    .epoch
                    .get()
                    .checked_add(28)
                    .is_some_and(|first| at.get() >= first)
                    && at <= now
                    && at.get().checked_add(15).is_some()
                    && !self.port_ready
                    && self.data_inflight.is_none()
                    && self.last_release.is_none()
                    && self.control_completed.is_none(),
                "invalid VDP quiet certificate"
            );
            if let Some(p) = self.port {
                ensure!(
                    at <= p.begin
                        && at
                            .get()
                            .checked_add(15)
                            .is_some_and(|ready| p.assertion.get() >= ready),
                    "invalid VDP recovery assertion"
                );
            }
        }
        if let Some(end) = self.control_completed {
            ensure!(
                self.core
                    .epoch
                    .get()
                    .checked_add(28)
                    .is_some_and(|first| end.get() >= first)
                    && end <= now
                    && self.port_ready
                    && self.port.is_none()
                    && self.data_inflight.is_none()
                    && self.quiet_since.is_none()
                    && end.get().checked_sub(3) == self.last_release.map(|at| at.get()),
                "invalid VDP completed control receipt"
            );
        }
        Ok(())
    }
}
