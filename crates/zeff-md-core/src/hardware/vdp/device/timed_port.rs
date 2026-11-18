use super::*;
use crate::hardware::vdp::timing::blank_mode::PortKind;
use crate::hardware::vdp::{VdpInterruptError, VdpTimingError};

impl Vdp {
    pub(crate) fn requires_timed_ports(&self) -> bool {
        if self.owned.is_some() {
            return true;
        }
        self.state.interrupts.staged_blank()
    }
    pub(crate) fn has_timed_status(&self) -> bool {
        if self.owned.is_some() {
            return false;
        }
        self.state
            .interrupts
            .blank_history()
            .is_some_and(|b| b.port.is_some_and(|p| p.kind == PortKind::Status))
    }
    pub(crate) fn has_timed_control(&self) -> bool {
        if self.owned.is_some() {
            return false;
        }
        self.state.interrupts.blank_history().is_some_and(|b| {
            b.control_completed.is_some() || b.port.is_some_and(|p| p.kind == PortKind::Control)
        })
    }
    pub(crate) fn field_presentation_supported(&self) -> bool {
        if self.owned.is_some() {
            return false;
        }
        self.state
            .interrupts
            .blank_history()
            .is_none_or(|b| !b.authoritative() || b.core.coverage)
    }
    pub(crate) fn retire_timed_port_history(&mut self) {
        if self.owned.is_some() {
            return;
        }
        if self.state.interrupts.blank_history().is_none() {
            return;
        }
        self.invalidate_interval();
        if let Some(mut blank) = self.state.interrupts.blank_history() {
            blank.retire_port();
            self.state.interrupts.set_blank_history(Some(blank));
        }
    }
    pub(crate) fn retire_unready_main_port(&mut self) -> bool {
        if self.owned.is_some() {
            return false;
        }
        if self.state.interrupts.blank_history().is_some_and(|b| {
            !b.active
                && !b.port_ready
                && b.port.is_none()
                && b.data_inflight.is_none()
                && b.control_completed.is_none()
        }) && !self
            .state
            .registers
            .pending()
            .is_some_and(|w| w.index == MODE_REGISTER && w.value & MODE5_ENABLE == 0)
        {
            self.retire_timed_port_history();
            return true;
        }
        false
    }
    fn begin_timed_port(
        &mut self,
        begin: MasterTicks,
        end: MasterTicks,
        kind: PortKind,
    ) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        if self.state.interrupts.blank_history().is_none() {
            return Ok(());
        }
        if self.now() != begin {
            return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
        }
        self.preflight_main_port()?;
        if let Some(mut blank) = self.state.interrupts.blank_history() {
            if kind == PortKind::Status
                && blank.vertical.is_none()
                && (!self.state.fifo.is_empty()
                    || self.state.cache.is_ready()
                    || self.state.read_phase.is_some()
                    || self.state.dma.is_some())
            {
                if !blank.active
                    && !self
                        .state
                        .registers
                        .pending()
                        .is_some_and(|w| w.index == MODE_REGISTER && w.value & MODE5_ENABLE == 0)
                {
                    self.retire_timed_port_history();
                    return Ok(());
                }
                return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
            }
            if kind == PortKind::Status && !self.state.timed_status_known(end) {
                return Err(VdpPortError::Status(VdpStatusError::UnknownHistory));
            }
            blank
                .begin_port(begin, end, kind)
                .map_err(|_| VdpPortError::Status(VdpStatusError::UnknownBusHistory))?;
            self.invalidate_interval();
            self.state.interrupts.set_blank_history(Some(blank));
        }
        Ok(())
    }
    pub(crate) fn begin_timed_status(
        &mut self,
        begin: MasterTicks,
        end: MasterTicks,
        _region: Region,
    ) -> Result<(), VdpPortError> {
        self.begin_timed_port(begin, end, PortKind::Status)
    }
    pub(crate) fn preflight_main_status(&self) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        self.preflight_main_port()?;
        let refines = self.state.interrupts.blank_history().is_some_and(|b| {
            b.active
                || self
                    .state
                    .registers
                    .pending()
                    .is_some_and(|w| w.index == MODE_REGISTER && w.value & MODE5_ENABLE == 0)
        });
        if refines && !self.state.status_known {
            return Err(VdpPortError::Status(VdpStatusError::UnknownHistory));
        }
        if refines
            && (!self.state.fifo.is_empty()
                || self.state.cache.is_ready()
                || self.state.read_phase.is_some()
                || self.state.dma.is_some())
        {
            return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
        }
        Ok(())
    }
    pub(crate) fn preflight_main_port(&self) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        if self.state.interrupts.blank_history().is_some_and(|b| {
            b.port.is_some() || b.data_inflight.is_some() || b.control_completed.is_some()
        }) {
            return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
        }
        Ok(())
    }
    pub(crate) fn begin_timed_control(
        &mut self,
        begin: MasterTicks,
        end: MasterTicks,
        _region: Region,
    ) -> Result<(), VdpPortError> {
        self.begin_timed_port(begin, end, PortKind::Control)
    }
    pub(crate) fn finish_timed_status(
        &mut self,
        end: MasterTicks,
        region: Region,
    ) -> Result<u16, VdpPortError> {
        self.require_legacy_port()?;
        let mut blank = self
            .state
            .interrupts
            .blank_history()
            .ok_or(VdpPortError::Status(VdpStatusError::UnknownBusHistory))?;
        let port = blank
            .port
            .filter(|p| p.kind == PortKind::Status && p.end == end && end == self.now())
            .ok_or(VdpPortError::Status(VdpStatusError::UnknownBusHistory))?;
        if !self.state.timed_status_known(end) {
            return Err(VdpPortError::Status(VdpStatusError::UnknownHistory));
        }
        let word = if port.staged || blank.authoritative() {
            port.captured
                .ok_or(VdpPortError::Status(VdpStatusError::UnknownBusHistory))?
        } else {
            self.state.read_cpu_status_internal(region)?
        };
        self.state.control.cancel_pending();
        blank.port = None;
        blank.port_ready = true;
        blank.quiet_since = None;
        blank.last_release = Some(port.sample);
        self.state.interrupts.set_blank_history(Some(blank));
        Ok(word)
    }
    pub(crate) fn finish_timed_control(
        &mut self,
        end: MasterTicks,
        _region: Region,
    ) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        let mut blank = self
            .state
            .interrupts
            .blank_history()
            .ok_or(VdpPortError::Status(VdpStatusError::UnknownBusHistory))?;
        if end != self.now() {
            return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
        }
        if blank.control_completed != Some(end) {
            blank
                .complete_control(end)
                .map_err(|_| VdpPortError::Status(VdpStatusError::UnknownBusHistory))?;
        }
        blank.control_completed = None;
        self.state.interrupts.set_blank_history(Some(blank));
        Ok(())
    }
}

impl State {
    pub(in crate::hardware::vdp) fn timed_status_known(&self, end: MasterTicks) -> bool {
        self.interrupts.m5_vertical()
            || self.status_known
            || self.interrupts.blank_history().is_none_or(|b| {
                !b.active
                    && !self.registers.pending().is_some_and(|w| {
                        w.index == MODE_REGISTER
                            && w.value & MODE5_ENABLE == 0
                            && w.publish_at <= end
                    })
            })
    }

    pub(in crate::hardware::vdp) fn pending_status_known(&self) -> bool {
        self.interrupts
            .blank_history()
            .and_then(|b| b.port)
            .filter(|p| p.kind == PortKind::Status)
            .is_none_or(|p| {
                self.interrupts.m5_vertical()
                    || self.fifo.is_empty()
                        && !self.cache.is_ready()
                        && self.read_phase.is_none()
                        && self.dma.is_none()
                        && self.timed_status_known(p.end)
            })
    }
}

fn timing(error: VdpTimingError) -> VdpPortError {
    VdpPortError::Schedule(VdpInterruptError::from(error).into())
}

impl Vdp {
    pub(crate) fn has_main_data(&self) -> bool {
        if self.owned.is_some() {
            return false;
        }
        self.state
            .interrupts
            .blank_history()
            .is_some_and(|b| b.data_inflight.is_some())
    }

    pub(crate) fn preflight_main_data(&self) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        let Some(b) = self.state.interrupts.blank_history() else {
            return Ok(());
        };
        let registers = &self.state.registers;
        if b.vertical.is_some() {
            if !b.vertical_supported(registers, Region::NtscUs)
                || b.port.is_some()
                || b.data_inflight.is_some()
                || b.control_completed.is_some()
            {
                return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
            }
            return Ok(());
        }
        if b.active
            || !b.eligible
            || b.port.is_some()
            || b.data_inflight.is_some()
            || b.control_completed.is_some()
            || registers.registers()[0].is_none_or(|r| r & 1 != 0)
            || registers.registers()[1]
                .is_none_or(|r| r & MODE5_ENABLE == 0 || r & (V30_ENABLE | VRAM_128K_ENABLE) != 0)
            || registers.registers()[12] != Some(0)
            || registers
                .pending()
                .is_some_and(|w| w.index == MODE_REGISTER && w.value & MODE5_ENABLE == 0)
        {
            return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
        }
        Ok(())
    }

    pub(crate) fn begin_main_data(
        &mut self,
        begin: MasterTicks,
        write: bool,
    ) -> Result<(), VdpPortError> {
        self.preflight_main_data()?;
        let Some(mut b) = self.state.interrupts.blank_history() else {
            return Ok(());
        };
        b.begin_data(begin, write).map_err(timing)?;
        self.invalidate_interval();
        self.state.interrupts.set_blank_history(Some(b));
        Ok(())
    }

    pub(crate) fn abort_main_data(&mut self) {
        if self.owned.is_some() {
            return;
        }
        let Some(mut b) = self.state.interrupts.blank_history() else {
            return;
        };
        if b.data_inflight.take().is_some() {
            b.quiet_since = None;
            self.invalidate_interval();
            self.state.interrupts.set_blank_history(Some(b));
        }
    }

    pub(crate) fn read_main_data(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.require_legacy_port()?;
        let Some(b) = self.state.interrupts.blank_history() else {
            return self.read_data(region);
        };
        let mut completed = b;
        completed.finish_data(self.now(), false).map_err(timing)?;
        let mut candidate = self.state.clone();
        let word = candidate.read_data(region, &self.memory)?;
        candidate.interrupts.set_blank_history(Some(completed));
        self.invalidate_interval();
        self.state = candidate;
        Ok(word)
    }

    pub(crate) fn write_main_data(
        &mut self,
        word: u16,
        region: Region,
    ) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        let Some(b) = self.state.interrupts.blank_history() else {
            return self.write_data(word, region);
        };
        let mut completed = b;
        completed.finish_data(self.now(), true).map_err(timing)?;
        let mut candidate = self.state.clone();
        candidate.write_data(word, region, &self.memory)?;
        candidate.interrupts.set_blank_history(Some(completed));
        self.invalidate_interval();
        self.state = candidate;
        Ok(())
    }
}
