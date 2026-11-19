use super::*;
use crate::sound_trace::SoundCapture;

impl SoundSchedule {
    pub(crate) fn advance_owned<C: SoundCapture>(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut SoundMemory,
        cartridge: &mut Cartridge,
        ram: &mut [u8],
        interval: (SoundInterval<'_>, &mut C),
    ) -> bool {
        let (interval, capture) = interval;
        let SoundInterval {
            ticks,
            occupied,
            audio,
            sound_irq,
        } = interval;
        let mut local = !occupied.iter().any(|slot| slot.requester == Requester::VDP);
        if audio.trace.is_enabled() || occupied.iter().any(|slot| slot.requester == Requester::VDP)
        {
            self.materialize_epoch(cpu, memory, capture);
        }
        if self.fault.is_some() {
            self.materialize_epoch(cpu, memory, capture);
            audio.request_advance_to(ticks.end);
            return false;
        }
        let mut cursor = ticks.start;
        while cursor < ticks.end {
            if let Some(deadline) = self.epoch_deadline() {
                if deadline > ticks.end {
                    break;
                }
                self.publish_epoch(memory, audio, capture);
                cursor = deadline;
                continue;
            }
            if self.pending.is_none() {
                if memory.reset_asserted || memory.requested {
                    self.materialize_epoch(cpu, memory, capture);
                    memory.complete_boundary();
                    break;
                }
                let irq = match sound_irq.map(|irq| irq.asserted_at(cursor)).transpose() {
                    Ok(irq) => memory.irq_asserted || irq.unwrap_or(false),
                    Err(error) => {
                        self.materialize_epoch(cpu, memory, capture);
                        self.fault = Some(SoundFault::VdpTiming(error));
                        break;
                    }
                };
                if !occupied.iter().any(|slot| slot.requester == Requester::VDP)
                    && !audio.trace.is_enabled()
                    && self.prepare_epoch(cpu, memory, cursor, irq, capture)
                {
                    continue;
                }
                match PendingSound::prepare_into_captured::<C::Attempt>(
                    &mut self.pending,
                    cpu,
                    memory,
                    cartridge,
                    Preparation {
                        now: cursor,
                        state: self.state,
                        bounded: true,
                        opn2_status: 0,
                    },
                    PreparationPolicy {
                        irq_asserted: irq,
                        legacy_relative: None,
                    },
                ) {
                    Ok(attempt) => capture.prepared(attempt),
                    Err(fault) => {
                        self.pending = None;
                        capture.discard_pending();
                        self.fault = Some(fault);
                        break;
                    }
                }
                if audio.trace.is_enabled() {
                    self.pending.as_mut().unwrap().trace_origin =
                        Some(crate::hardware::audio::trace::sound_origin(
                            cpu.regs().pc,
                            memory.bank,
                            cartridge,
                        ));
                }
            }
            if let Err(fault) = self.reserve_pending(occupied, capture) {
                self.pending = None;
                capture.discard_pending();
                self.fault = Some(fault);
                break;
            }
            let pending = self.pending.as_ref().unwrap();
            local &= if pending.bank_access.is_some() {
                pending.start >= ticks.end
            } else {
                !pending.any_bank_access
            };
            let deadline = pending.deadline;
            if deadline > ticks.end {
                break;
            }
            let completed = self.pending.as_mut().unwrap();
            if completed.replay.is_some() {
                audio.flush_pending();
            }
            if let Err(fault) =
                completed.refresh_reads_captured(cpu, memory, cartridge, audio, capture)
            {
                self.pending = None;
                capture.discard_pending();
                self.fault = Some(fault);
                break;
            }
            if completed
                .sf002_writes
                .validate(cartridge, completed.any_bank_access, completed.bank_access)
                .is_err()
            {
                self.pending = None;
                capture.discard_pending();
                self.fault = Some(SoundFault::UnsupportedAccess {
                    address: cpu.regs().pc,
                    direction: Direction::Write,
                });
                break;
            }
            self.state = completed.state;
            completed.publish(cpu, memory, ram, audio, cartridge);
            capture.publish(cpu);
            self.pending = None;
            capture.discard_pending();
            cursor = deadline;
        }
        if self.fault.is_some() {
            capture.fault();
        }
        audio.request_advance_to(ticks.end);
        local && self.fault.is_none()
    }
}
