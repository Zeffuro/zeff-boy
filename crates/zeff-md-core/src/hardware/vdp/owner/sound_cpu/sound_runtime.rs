use super::super::sound_semantics::CopyPhase;
use super::*;
use crate::sound_trace::physical::{Binding, HalfCapture, Shape as TraceShape};

impl SoundOwner {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance(
        &mut self,
        half: u64,
        pass: u8,
        zclk: bool,
        response: SoundResponse,
        audio: &Audio,
        irq: bool,
    ) -> Result<()> {
        self.advance_capturing(
            AdvanceInput {
                half,
                pass,
                zclk,
                response,
                audio,
                irq,
            },
            &mut (),
        )
    }

    pub(crate) fn advance_capturing<C: HalfCapture>(
        &mut self,
        input: AdvanceInput<'_>,
        capture: &mut C,
    ) -> Result<()> {
        self.advance_inner(input, capture)
            .map_err(|error| {
                anyhow::anyhow!(
                    "{error}; SOUND candidate first_pc={:04x} first_opcode={:02x} bytes={:02x?} m1={} regs={:?} state={:?} bank={:03x} bus={:?}",
                    self.semantic.cpu.last_opcode_pc(),
                    self.semantic.cpu.last_opcode(),
                    self.semantic.cpu.instruction_bytes(),
                    self.semantic.cpu.last_m1_fetch_count(),
                    self.semantic.cpu.regs(),
                    self.semantic.state,
                    self.semantic.memory.bank,
                    self.bus
                )
            })
    }

    fn advance_inner<C: HalfCapture>(
        &mut self,
        input: AdvanceInput<'_>,
        capture: &mut C,
    ) -> Result<()> {
        let AdvanceInput {
            half,
            pass,
            zclk,
            response,
            audio,
            irq,
        } = input;
        let edge = self.clock.observe(half, pass, zclk)?;
        let request = self.request_low;
        let wait = self.wait_high;
        let irq_sample = self.irq_low;
        if response.reset && !self.semantic.memory.reset_asserted {
            ensure!(
                !self.semantic.state.indexed_memory_pending()
                    && !self.cpu().indexed_cb_cycle_pending(&self.semantic.state)
                    && !self.semantic.state.ed_word_pending(),
                "RESET during indexed memory is not qualified"
            );
            ensure!(
                self.djnz_tail.is_none() && self.bus.plan.is_none_or(|p| p.djnz.is_none()),
                "RESET during DJNZ is not qualified"
            );
            ensure!(
                self.word_tail.is_none(),
                "RESET during pending ADD16 tail is not qualified"
            );
            ensure!(
                self.jr_tail.is_none(),
                "RESET during pending JR tail is not qualified"
            );
            ensure!(self.reset.is_none(), "RESET reasserted during release");
            let late_repeat = !response.request
                && !self.semantic.memory.requested
                && !self.request_low
                && self
                    .copy_history
                    .late_entry(&self.bus, &self.semantic.cpu, self.semantic.state);
            let reset = if late_repeat {
                SoundReset::late_repeat(&self.clock)?
            } else if self.stable_grant
                && self.bus.phase == Phase::Granted
                && self.bus.plan.is_none()
                && self.semantic.memory.requested
                && self.request_low
            {
                SoundReset::granted(&self.clock)?
            } else {
                ensure!(
                    !response.request && !self.semantic.memory.requested && !self.request_low,
                    "running RESET conflicts with BUSREQ"
                );
                let plan = self
                    .bus
                    .plan
                    .ok_or_else(|| anyhow::anyhow!("RESET lacks next M1"))?;
                plan.reset_compatible(
                    &self.semantic.cpu,
                    self.semantic.state,
                    &self.semantic.memory,
                )?;
                let running = self.m1_history.begin(&self.bus, plan)?;
                SoundReset::running(&self.clock, running)?
            };
            self.reset = Some(reset);
            if !late_repeat {
                capture.reset();
                self.bus.plan = None;
                self.bus.phase = Phase::Ready;
                self.copy_history.clear();
            }
            self.stable_grant = false;
            self.m1_history.clear();
            self.effects.push(AudioEffect::Reset {
                tick: half.div_ceil(2),
            });
        }
        if response.request && !self.semantic.memory.requested && self.reset.is_none() {
            ensure!(
                !matches!(self.bus.phase, Phase::ReturnHigh | Phase::ReturnLow),
                "SOUND request reassertion during accepted bus return"
            );
            ensure!(
                self.bus.plan.is_none_or(|p| !p.opaque_tail),
                "BUSREQ during an unclassified local SOUND aggregate"
            );
        }
        if (response.request, response.reset) != self.observed_controls() {
            let memory = &mut Arc::make_mut(&mut self.semantic).memory;
            memory.request(response.request);
            memory.set_reset(response.reset);
        }
        if zclk {
            self.wait_high = response.wait;
        } else {
            self.request_low = response.request;
            self.irq_low = irq || self.semantic.memory.irq_asserted;
        }
        if let Some(reset) = &mut self.reset {
            self.bus.high = zclk;
            if reset.observe(&self.clock, response.request, response.reset, response.wait)? {
                let semantic = Arc::make_mut(&mut self.semantic);
                semantic
                    .cpu
                    .apply_qualified_reset(&mut semantic.state)
                    .map_err(|e| anyhow::anyhow!("invalid qualified SOUND reset: {e:?}"))?;
                capture.reset();
                self.released = true;
                self.boundary_irq = false;
                self.copy_history.clear();
            }
            let ready = reset.ready();
            let waiting_repeat = reset.waiting_repeat();
            let from_repeat = reset.is_repeat_origin();
            if !waiting_repeat {
                self.refresh_hold = reset.pins().refresh;
                self.data_hold = reset.pins().data;
                if !from_repeat {
                    self.copy_history.clear();
                }
            }
            self.sync_grant();
            if !ready && !waiting_repeat {
                return Ok(());
            }
            if ready {
                self.reset = None;
                self.bus.phase = Phase::ResetReady;
                self.bus.held_address = self.semantic.cpu.regs().pc;
                self.m1_history.release(self.wait_high);
            }
        }
        if self.bus.plan.is_none()
            && matches!(
                self.bus.phase,
                Phase::Ready | Phase::ReturnLow | Phase::ResetReady
            )
        {
            self.bus.plan = Some(self.prepare_plan(audio, half / 2)?);
        }
        let memory_predecessor = self
            .bus
            .plan
            .is_some_and(|p| p.reset_predecessor(&self.semantic.cpu, self.semantic.state));
        let previous_phase = self.bus.phase;
        let progress = self.bus.edge(edge, wait, request)?;
        if previous_phase != Phase::Active && self.bus.phase == Phase::Active {
            let plan = self.bus.plan.expect("active SOUND plan");
            capture.activated(Binding {
                address: plan.address,
                bank: plan.bank,
                external: plan.external,
                shape: match plan.shape {
                    Shape::Fetch => TraceShape::Fetch,
                    Shape::Read => TraceShape::Read,
                    Shape::Write => TraceShape::Write,
                    Shape::Internal => TraceShape::Internal,
                    Shape::InterruptAck => TraceShape::InterruptAck,
                },
            });
        }
        if edge != Edge::None {
            let driven = self.bus.pins(false).data;
            if driven.is_some() || edge == Edge::Rise {
                self.data_hold = driven;
            }
        }
        if edge == Edge::Rise {
            self.refresh_hold = self.bus.pins(false).refresh;
        }
        self.copy_history.observe(
            edge,
            &self.bus,
            &self.semantic.cpu,
            self.semantic.state,
            response.wait || wait || response.request || request,
        );
        self.m1_history.observe(
            edge,
            &self.bus,
            response.wait,
            response.request || request,
            memory_predecessor,
        );
        if edge == Edge::Rise
            && self.bus.phase == Phase::Active
            && !self.bus.waiting
            && self
                .bus
                .plan
                .is_some_and(|p| p.finishes && self.bus.t == p.cycles)
        {
            self.boundary_irq = irq_sample;
        }
        if progress.sample {
            let plan = self
                .bus
                .plan
                .ok_or_else(|| anyhow::anyhow!("missing SOUND read plan"))?;
            let value = if plan.external {
                ensure!(
                    response.known == 255,
                    "unknown physical SOUND read at {:04x}",
                    plan.address
                );
                response.data
            } else {
                local_byte(&self.semantic.memory, audio, half / 2, plan.address)?
            };
            self.bus.sample = Some(value);
            capture.sampled(value);
        }
        if progress.write {
            let plan = self
                .bus
                .plan
                .ok_or_else(|| anyhow::anyhow!("missing SOUND write plan"))?;
            if !plan.external {
                self.local_write(plan.address, plan.value, half.div_ceil(2))?;
                capture.local_write(plan.address, plan.value, half.div_ceil(2));
            } else {
                capture.external_write(plan.address, plan.bank, plan.value);
            }
        }
        if progress.complete {
            let plan = self
                .bus
                .plan
                .ok_or_else(|| anyhow::anyhow!("missing completed SOUND plan"))?;
            let (cpu, state) = self.retire_plan(plan, audio, half / 2, capture)?;
            let semantic = Arc::make_mut(&mut self.semantic);
            semantic.cpu = cpu;
            semantic.state = state;
            let slot = usize::from(plan.external);
            self.completed[slot] = self.completed[slot]
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("SOUND completion count overflow"))?;
            if plan.external {
                let value = if plan.shape == Shape::Write {
                    plan.value
                } else {
                    self.bus
                        .sample
                        .ok_or_else(|| anyhow::anyhow!("missing external SOUND receipt"))?
                };
                self.last_external = Some(ExternalReceipt {
                    address: plan.address,
                    bank: plan.bank,
                    write: plan.shape == Shape::Write,
                    value,
                    half,
                    waits: self.bus.waits,
                });
            }
            if plan.shape == Shape::InterruptAck {
                self.boundary_irq = false;
            }
            self.bus.clear_completed();
            if self.reset.as_ref().is_some_and(|r| r.waiting_repeat()) {
                ensure!(
                    plan.copy == Some(CopyPhase::Repeat) && self.bus.phase == Phase::Ready,
                    "late RESET completed another cycle"
                );
                let next = self.prepare_plan(audio, half / 2)?;
                next.reset_after_copy(
                    &self.semantic.cpu,
                    self.semantic.state,
                    &self.semantic.memory,
                )?;
                let running = self.copy_history.running(next)?;
                self.reset
                    .as_mut()
                    .expect("pending repeat reset")
                    .finish_repeat(running)?;
                self.m1_history.clear();
                self.refresh_hold = self.reset.as_ref().unwrap().pins().refresh;
                self.data_hold = None;
            } else if plan.copy == Some(CopyPhase::Repeat) {
                self.copy_history.clear();
            }
            if self.bus.phase == Phase::Ready && self.reset.is_none() {
                self.bus.plan = Some(self.prepare_plan(audio, half / 2)?);
            }
        }
        self.sync_grant();
        if self.bus.phase != Phase::Granted {
            self.stable_grant = false;
        } else if !zclk && self.pins().bus_ack {
            self.stable_grant = true;
        }
        if edge == Edge::Fall
            && self.bus.phase == Phase::ReturnLow
            && self.bus.plan.is_none()
            && !self.semantic.memory.requested
        {
            self.bus.plan = Some(self.prepare_plan(audio, half / 2)?);
        }
        self.validate_irq()?;
        ensure!(self.effects.len() <= 4, "physical SOUND effect capacity");
        Ok(())
    }
}
