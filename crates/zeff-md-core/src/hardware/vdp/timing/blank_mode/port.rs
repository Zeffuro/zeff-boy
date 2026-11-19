use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) enum PortKind {
    Status,
    Control,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PortCycle {
    pub kind: PortKind,
    pub begin: MasterTicks,
    pub assertion: MasterTicks,
    pub sample: MasterTicks,
    pub end: MasterTicks,
    pub captured: Option<u16>,
    pub staged: bool,
}

impl PortCycle {
    pub fn next_after(self, after: MasterTicks) -> Option<MasterTicks> {
        [self.assertion, self.sample, self.end]
            .into_iter()
            .filter(|at| *at > after)
            .min()
    }
}

impl BlankHistory {
    pub fn begin_port(
        &mut self,
        begin: MasterTicks,
        end: MasterTicks,
        kind: PortKind,
    ) -> Result<(), VdpTimingError> {
        if self.port.is_some()
            || self.data_inflight.is_some()
            || self.control_completed.is_some()
            || begin != self.now
            || end.get()
                != begin
                    .get()
                    .checked_add(28)
                    .ok_or(VdpTimingError::ClockOverflow)?
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        let assertion =
            MasterTicks::new(begin.get() + if kind == PortKind::Status { 7 } else { 14 });
        if !self.port_ready
            && !self.quiet_since.is_some_and(|at| {
                at.get()
                    .checked_add(15)
                    .is_some_and(|ready| assertion.get() >= ready)
            })
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        if self
            .last_release
            .is_some_and(|release| assertion.get() < release.get().saturating_add(10))
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        self.port = Some(PortCycle {
            kind,
            begin,
            assertion,
            sample: MasterTicks::new(end.get() - 3),
            end,
            captured: None,
            staged: self.authoritative(),
        });
        Ok(())
    }

    pub fn retire_port(&mut self) {
        self.port_ready = false;
        self.port = None;
        self.data_inflight = None;
        self.quiet_since = None;
        self.control_completed = None;
        if !self.authoritative() {
            self.eligible = false;
        }
    }
}
