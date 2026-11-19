use super::super::timing::blank_mode::BlankHistory;
use super::*;

impl VdpInterrupts {
    pub(crate) fn m5_vertical(&self) -> bool {
        self.timing.blank.is_some_and(|b| b.vertical.is_some())
    }

    pub(in crate::hardware::vdp) fn vertical_change_supported(
        &self,
        registers: &VdpRegisterPipeline,
        value: u8,
        region: Region,
    ) -> bool {
        value & (MODE5_ENABLE | VRAM_128K_ENABLE) == MODE5_ENABLE
            && self.timing.history.is_none()
            && self
                .timing
                .blank
                .is_some_and(|b| b.vertical_supported(registers, region))
    }
    pub(crate) fn has_port_history(&self) -> bool {
        self.timing.blank.is_some()
    }

    pub(in crate::hardware::vdp) fn blank_history(&self) -> Option<BlankHistory> {
        self.timing.blank
    }
    pub(in crate::hardware::vdp) fn set_blank_history(&mut self, history: Option<BlankHistory>) {
        self.timing.blank = history.filter(|b| b.authoritative() || b.eligible);
    }
    pub(in crate::hardware::vdp) fn blank_transition_supported(
        &self,
        registers: &VdpRegisterPipeline,
        value: u8,
    ) -> bool {
        value & (DISPLAY_ENABLE | V30_ENABLE | VRAM_128K_ENABLE) == 0
            && self.timing.blank.is_none_or(|b| b.vertical.is_none())
            && self.timing.blank.is_some_and(|b| b.supported(registers))
    }
    pub(in crate::hardware::vdp) fn track_software_origin(&mut self) {
        self.timing.blank = Some(BlankHistory::new(self.now));
    }

    pub(in crate::hardware::vdp) fn staged_blank(&self) -> bool {
        self.timing.blank.is_some_and(|b| b.active)
    }

    pub(in crate::hardware::vdp) fn advance_blank_pair(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &mut VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<Option<VdpEvent>, VdpScheduleError> {
        if at < self.now {
            return Err(VdpInterruptError::TimeReversal {
                current: self.now,
                requested: at,
            }
            .into());
        }
        if self
            .timing
            .blank
            .is_some_and(|b| b.port.is_some_and(|p| at > p.end))
        {
            return Err(VdpInterruptError::from(VdpTimingError::UnqualifiedDisplayHistory).into());
        }
        let mut candidate = *self;
        let mut regs = *registers;
        candidate
            .timing
            .blank
            .unwrap()
            .check_data_publication(at, &regs)
            .map_err(VdpInterruptError::from)?;
        loop {
            let mut blank = candidate.timing.blank.unwrap();
            let next = blank
                .next_event(&regs)
                .map_err(VdpInterruptError::from)?
                .min(at);
            let before = regs;
            let old_core = blank.core;
            let activates =
                blank.activates(&regs, next) || blank.activates_vertical(&regs, next, region);
            if blank.port.is_some_and(|p| {
                p.kind == super::super::timing::blank_mode::PortKind::Status
                    && p.sample == next
                    && p.captured.is_none()
            }) {
                let sample = MasterTicks::new(next.get().saturating_sub(1));
                let word = if blank.authoritative() {
                    blank
                        .status(region, &before)
                        .map_err(VdpInterruptError::from)?
                } else {
                    let mut old = candidate;
                    old.timing.blank = None;
                    let r = before.published_state().1;
                    FIFO_EMPTY_STATUS
                        | old.status_bits()
                        | if region == Region::Pal { PAL_STATUS } else { 0 }
                        | if r.registers()[1].unwrap() & DISPLAY_ENABLE == 0
                            || old
                                .timing
                                .vblank_in_mode(sample, region, &r, false)
                                .map_err(VdpInterruptError::from)?
                        {
                            VBLANK_STATUS
                        } else {
                            0
                        }
                        | if old
                            .timing
                            .hblank_flag_at(sample, &r)
                            .map_err(VdpInterruptError::from)?
                        {
                            HBLANK_STATUS
                        } else {
                            0
                        }
                };
                blank.capture(word);
            }
            let mut event;
            let write;
            if !blank.authoritative() {
                let mut old = candidate;
                old.timing.blank = None;
                let mut old_regs = if activates {
                    regs.published_state().1
                } else {
                    regs
                };
                event = old.advance_pair(next, region, &mut old_regs, require_display)?;
                blank.core.hp = old.horizontal_pending;
                blank.core.vp = old.vertical_pending;
                candidate = old;
                write = regs.advance_to(next)?;
            } else {
                write = regs.advance_to(next)?;
                event = write.map(VdpEvent::Register);
            }
            blank
                .step(next, region, &before, &regs, write)
                .map_err(VdpInterruptError::from)?;
            if blank.authoritative() {
                let vertical = blank.core.h && blank.core.q && !(old_core.h && old_core.q);
                if blank.core.v != old_core.v || vertical {
                    event = Some(VdpEvent::Interrupt(VdpInterruptEdge {
                        at: next,
                        clock: if vertical {
                            VdpInterruptClock::Vertical
                        } else {
                            VdpInterruptClock::HorizontalCounter
                        },
                    }));
                }
                candidate.horizontal_pending = blank.core.hp;
                candidate.vertical_pending = blank.core.vp;
                candidate.line_counter = !blank.core.c;
                candidate.timing.field.start = blank.core.field_start;
                candidate.timing.field.id = blank.core.field_id;
                candidate.timing.field.updated_at = next;
                candidate.timing.field.mode = super::super::InterlaceMode::Progressive;
                candidate.timing.field.active_mode = super::super::InterlaceMode::Progressive;
                candidate.timing.field.odd = false;
                candidate.timing.field.active_odd = false;
            }
            candidate.now = next;
            candidate.timing.blank = Some(blank).filter(|b| b.authoritative() || b.eligible);
            if event.is_some() || next == at || candidate.timing.blank.is_none() {
                *self = candidate;
                *registers = regs;
                return Ok(event);
            }
        }
    }
}
