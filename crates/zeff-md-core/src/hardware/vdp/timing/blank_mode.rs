use super::*;

mod bridge;
mod core;
mod port;
mod projection;
mod state;
mod vertical;
pub(in crate::hardware::vdp) use port::{PortCycle, PortKind};
pub(in crate::hardware::vdp) use vertical::VerticalClock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct BlankHistory {
    pub active: bool,
    pub eligible: bool,
    pub port_ready: bool,
    pub last_release: Option<MasterTicks>,
    pub port: Option<PortCycle>,
    pub data_inflight: Option<(MasterTicks, bool)>,
    pub quiet_since: Option<MasterTicks>,
    pub control_completed: Option<MasterTicks>,
    pub cancel_control: bool,
    pub now: MasterTicks,
    pub next_clock: MasterTicks,
    pub core: core::Core,
    pub vertical: Option<VerticalClock>,
}

impl BlankHistory {
    pub fn new(epoch: MasterTicks) -> Self {
        Self {
            active: false,
            eligible: true,
            port_ready: true,
            last_release: None,
            port: None,
            data_inflight: None,
            quiet_since: None,
            control_completed: None,
            cancel_control: false,
            now: epoch,
            next_clock: MasterTicks::new(epoch.get().saturating_add(H32_DOT_TICKS)),
            core: core::Core::new(epoch),
            vertical: None,
        }
    }

    pub fn next_event(
        &self,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        let next = [
            Some(self.next_clock),
            registers.pending().map(|w| w.publish_at),
            self.port.and_then(|p| p.next_after(self.now)),
        ]
        .into_iter()
        .flatten()
        .filter(|at| *at > self.now)
        .min();
        next.ok_or(VdpTimingError::ClockOverflow)
    }

    pub fn activates(&self, registers: &VdpRegisterPipeline, at: MasterTicks) -> bool {
        !self.active
            && registers.pending().is_some_and(|w| {
                w.publish_at == at && w.index == MODE_REGISTER && w.value & MODE5_ENABLE == 0
            })
    }

    pub fn supported(&self, registers: &VdpRegisterPipeline) -> bool {
        self.eligible
            && self.port_ready
            && self.data_inflight.is_none()
            && [0, 1, 10, 12]
                .into_iter()
                .all(|i| registers.registers()[i].is_some())
            && registers.registers()[0].is_some_and(|r| r & 1 == 0)
            && registers.registers()[1].is_some_and(|r| r & (V30_ENABLE | VRAM_128K_ENABLE) == 0)
            && registers.registers()[12] == Some(0)
    }

    pub fn advance_projection(
        mut self,
        at: MasterTicks,
        region: Region,
        registers: &mut VdpRegisterPipeline,
    ) -> Result<Self, VdpTimingError> {
        if at < self.now {
            return Err(VdpTimingError::BeforeEpoch {
                epoch: self.now,
                requested: at,
            });
        }
        if self.port.is_some_and(|p| at > p.end) {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        self.check_data_publication(at, registers)?;
        loop {
            let next = self.next_event(registers)?.min(at);
            let before = *registers;
            if self
                .port
                .is_some_and(|p| p.sample == next && p.captured.is_none())
            {
                self.capture(self.status(region, &before)?);
            }
            let write = registers
                .advance_to(next)
                .map_err(|_| VdpTimingError::PendingModeChange)?;
            self.step(next, region, &before, registers, write)?;
            if next == at {
                return Ok(self);
            }
        }
    }

    pub fn capture(&mut self, word: u16) {
        if let Some(port) = &mut self.port
            && port.kind == PortKind::Status
            && port.captured.is_none()
        {
            port.captured = Some(word);
        }
    }

    pub fn step(
        &mut self,
        at: MasterTicks,
        region: Region,
        before: &VdpRegisterPipeline,
        registers: &VdpRegisterPipeline,
        write: Option<super::super::VdpRegisterWrite>,
    ) -> Result<(), VdpTimingError> {
        let clock = at == self.next_clock;
        if clock {
            if let Some(vertical) = self.vertical {
                self.core.commit_at(at, vertical.counter(at)?)?;
            } else {
                self.core.commit(at, region)?;
            }
        }
        if self.activates_vertical(before, at, region) {
            if !clock {
                return Err(VdpTimingError::UnqualifiedDisplayHistory);
            }
            self.vertical = Some(VerticalClock::new(
                self.core,
                before.registers()[1].unwrap(),
                before.registers()[10].unwrap(),
                at,
            ));
            self.eligible = false;
            self.core.coverage = false;
            self.core.canonical = false;
        }
        if let Some(write) = write {
            if self.vertical.is_some()
                && ((write.index == MODE_REGISTER
                    && before.registers()[1].is_some_and(|old| {
                        (old ^ write.value) & (DISPLAY_ENABLE | V30_ENABLE) != 0
                    }))
                    || write.index == DISPLAY_SIZE_REGISTER
                        && before.registers()[12] != Some(write.value))
            {
                self.core.coverage = false;
                self.core.canonical = false;
            }
            if matches!(write.index, 0 | 1 | 12) {
                let unsafe_change = match write.index {
                    0 => write.value & 1 != 0,
                    1 => write.value & (V30_ENABLE | VRAM_128K_ENABLE) != 0,
                    12 => before.registers()[12] != Some(write.value) || write.value != 0,
                    _ => false,
                };
                if self.vertical.is_none() {
                    self.eligible &= !unsafe_change;
                }
            }
            if write.index == MODE_REGISTER && write.value & MODE5_ENABLE == 0 {
                self.active = true;
                self.core.coverage = false;
                self.core.canonical = false;
            }
        }
        let mode = registers.registers()[1].ok_or(VdpTimingError::UnknownRegister(1))?;
        if self.vertical.is_some() && !self.vertical_supported(registers, region) {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        self.core.settle(mode);
        let asserted = self
            .port
            .is_some_and(|p| p.kind == PortKind::Status && p.assertion <= at && at < p.sample);
        let authoritative = self.authoritative();
        if let Some(port) = &mut self.port {
            port.staged |= authoritative;
        }
        self.cancel_control |= self.active && asserted;
        if asserted {
            self.core.t6 = true;
        } else if self.core.b {
            self.core.t6 = false;
        }
        if clock {
            let dot = if let Some(mut vertical) = self.vertical {
                vertical.finish(at, self.core, registers)?;
                self.core.prepare_clock(
                    region,
                    registers,
                    vertical.counter(at)?,
                    Some(vertical.clock),
                );
                vertical.prepared_mode = mode;
                vertical.prepared_hint = registers.registers()[10].unwrap();
                vertical.prepared_at = at;
                self.vertical = Some(vertical);
                vertical.dot()
            } else {
                self.core
                    .prepare(region, registers, self.core.h_counter(at));
                H32_DOT_TICKS
            };
            self.next_clock = MasterTicks::new(
                at.get()
                    .checked_add(dot)
                    .ok_or(VdpTimingError::ClockOverflow)?,
            );
        }
        self.now = at;
        self.core.updated_at = at;
        Ok(())
    }

    pub fn field_position(&self, region: Region) -> FieldPosition {
        FieldPosition {
            id: self.core.field_id,
            start: self.core.field_start,
            lines: region.frame_lines(),
            line: self.core.row,
            line_start: self.core.line_start,
            line_ticks: H32_LINE_TICKS,
            projected_end: None,
            mode: InterlaceMode::Progressive,
            odd: false,
            active_mode: InterlaceMode::Progressive,
            active_odd: false,
        }
    }
}
