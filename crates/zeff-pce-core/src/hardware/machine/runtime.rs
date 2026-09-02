use super::*;

impl PceMachine {
    pub fn step_boundary(&mut self) -> Result<PceMachineStep, PceMachineError> {
        self.step_boundary_faulting()
    }

    pub fn run_until_frame(&mut self) -> Result<PceFrameRun, PceMachineError> {
        let starting_ticks = self.master_ticks;
        let mut cpu_boundaries = 0_u64;
        loop {
            if self.is_cpu_suspended() {
                return Ok(PceFrameRun {
                    cpu_boundaries,
                    master_ticks: self.master_ticks - starting_ticks,
                    frames_published: 0,
                });
            }
            if self.skip_breakpoint_once {
                self.skip_breakpoint_once = false;
            } else if !self.suspend_after_instruction {
                let pc = Address::from(self.cpu.cpu().registers().pc);
                if self.debug.should_break(pc) {
                    self.execution_state = PceExecutionState::Suspended;
                    return Ok(PceFrameRun {
                        cpu_boundaries,
                        master_ticks: self.master_ticks - starting_ticks,
                        frames_published: 0,
                    });
                }
            }
            let step = self.step_boundary_faulting()?;
            cpu_boundaries += 1;
            if self.suspend_after_instruction && matches!(step.action, PceCpuAction::Instruction(_))
            {
                self.execution_state = PceExecutionState::Suspended;
                self.suspend_after_instruction = false;
                return Ok(PceFrameRun {
                    cpu_boundaries,
                    master_ticks: self.master_ticks - starting_ticks,
                    frames_published: step.frames_published,
                });
            }
            if step.frames_published != 0 {
                return Ok(PceFrameRun {
                    cpu_boundaries,
                    master_ticks: self.master_ticks - starting_ticks,
                    frames_published: step.frames_published,
                });
            }
        }
    }

    #[inline]
    pub const fn cpu(&self) -> &HuC6280 {
        &self.cpu
    }

    #[inline]
    pub fn cpu_mut(&mut self) -> &mut HuC6280 {
        &mut self.cpu
    }

    #[inline]
    pub fn devices(&self) -> &PceDevices {
        self.bus.devices()
    }

    #[inline]
    pub fn devices_mut(&mut self) -> &mut PceDevices {
        self.bus.devices_mut()
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.bus.devices_mut().set_sample_rate(sample_rate);
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus
            .devices_mut()
            .set_sample_generation_enabled(enabled);
    }

    pub fn set_channel_mutes(&mut self, mutes: &[bool]) {
        self.bus.devices_mut().set_channel_mutes(mutes);
    }

    pub fn drain_audio_samples_into(&mut self, output: &mut Vec<f32>) {
        self.bus.devices_mut().drain_audio_samples_into(output);
    }

    #[inline]
    pub fn framebuffer(&self) -> &[u8] {
        self.front_video.framebuffer()
    }

    #[inline]
    pub fn presented_frame(&self) -> PcePresentedFrame<'_> {
        self.front_video.presented_frame()
    }

    #[inline]
    pub const fn master_ticks(&self) -> u64 {
        self.master_ticks
    }

    #[inline]
    pub const fn vce_line_accumulator(&self) -> u64 {
        self.vce_line_accumulator
    }

    #[inline]
    pub const fn vdc_pixel_clock_remainder(&self) -> u8 {
        self.vdc_pixel_clock_remainder
    }

    #[inline]
    pub const fn vce_line_index(&self) -> u16 {
        self.vce_line_index
    }

    #[inline]
    pub const fn vce_frame_length(&self) -> VceFrameLength {
        self.vce_frame_length
    }

    pub const fn frame_count(&self) -> u64 {
        self.trace_frame
    }

    #[inline]
    pub const fn faulted(&self) -> bool {
        self.faulted
    }

    pub(super) fn step_boundary_faulting(&mut self) -> Result<PceMachineStep, PceMachineError> {
        self.step_boundary_faulting_with(|cpu, bus| {
            Ok(match cpu.service_interrupt_boundary(bus) {
                Some(step) => PceCpuAction::Interrupt(step),
                None => PceCpuAction::Instruction(cpu.step_instruction(bus)?),
            })
        })
    }

    pub(super) fn step_boundary_faulting_with(
        &mut self,
        execute: impl FnOnce(&mut HuC6280, &mut TimedMachineBus<'_>) -> Result<PceCpuAction, CpuTrap>,
    ) -> Result<PceMachineStep, PceMachineError> {
        if self.faulted {
            return Err(PceMachineError::FaultedUntilReset);
        }
        let result = self.step_boundary_inner_with(execute);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn step_boundary_inner_with(
        &mut self,
        execute: impl FnOnce(&mut HuC6280, &mut TimedMachineBus<'_>) -> Result<PceCpuAction, CpuTrap>,
    ) -> Result<PceMachineStep, PceMachineError> {
        let logical_pc = self.cpu.cpu().registers().pc;
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_physical_pc =
            trace_enabled.then(|| self.cpu.cpu().logical_to_physical(logical_pc));
        let trace_registers_before = trace_enabled.then(|| pce_trace_registers(&self.cpu));
        if trace_enabled {
            self.trace_scratch.clear();
        }
        let trace_frame = self.trace_frame;
        let trace_cycle = self.master_ticks;
        self.refresh_vdc_irq1();
        self.refresh_cdrom2_irq2();
        let entering_speed = self.cpu.cpu().speed_mode();
        let master_ticks_per_cycle = master_ticks_per_cpu_cycle(entering_speed);
        let (
            action,
            trap,
            error,
            wait_cycles,
            contention_wait_cycles,
            master_ticks,
            vce_lines,
            frames_published,
            dma_completed,
        ) = {
            let mut bus = TimedMachineBus::new(
                &mut self.bus,
                &mut self.front_video,
                &mut self.back_video,
                &mut self.vce_line_accumulator,
                &mut self.vdc_pixel_clock_remainder,
                &mut self.vce_line_index,
                &mut self.vce_frame_length,
                master_ticks_per_cycle,
                trace_enabled.then_some(&mut self.trace_scratch),
                &mut self.debug,
            );
            let mut action = None;
            let mut trap = None;
            let mut error = None;
            match execute(&mut self.cpu, &mut bus) {
                Ok(completed) => match bus.advance_remaining(completed.cycles()) {
                    Ok(()) => action = Some(completed),
                    Err(failure) => error = Some(failure),
                },
                Err(cpu_trap) => trap = Some(cpu_trap),
            }
            self.cpu
                .advance_master_ticks(bus.take_elapsed_master_ticks());
            error = bus.fault.or(error);
            (
                action,
                trap,
                error,
                bus.video_wait_cycles,
                bus.vram_contention_wait_cycles,
                bus.elapsed_master_ticks,
                bus.vce_lines,
                bus.frames_published,
                bus.dma_completed,
            )
        };
        self.commit_elapsed_cpu_time(master_ticks)?;
        if error.is_none() && trap.is_none() {
            self.cpu.sample_interrupts_after_action();
        }
        if let Some(error) = error {
            return Err(error);
        }
        if let Some(trap) = trap {
            return Err(PceMachineError::CpuTrap(trap));
        }
        let action = action.expect("successful CPU action is present");
        if trace_enabled {
            let physical_pc = match action {
                PceCpuAction::Instruction(step) => step.physical_pc,
                PceCpuAction::Interrupt(_) => trace_physical_pc.expect("trace physical PC"),
            };
            let trace_rom_offset = self.bus.hucard_rom_offset(physical_pc);
            let trace = &self.trace_scratch;
            let mut record = InstructionTraceRecord::new(
                TraceExecMode::HuC6280,
                u32::from(logical_pc),
                trace_rom_offset.map(u64::from),
                trace_frame,
                trace_cycle,
                if matches!(action, PceCpuAction::Interrupt(_)) {
                    &[]
                } else {
                    &trace.instruction_bytes[..usize::from(trace.instruction_byte_len)]
                },
            );
            record.bank = Some(physical_pc >> 13);
            if matches!(action, PceCpuAction::Interrupt(_)) {
                record.event = Some(DebugEvent::Interrupt);
            } else if dma_completed {
                record.event = Some(DebugEvent::Dma);
            }
            for write in &trace.trace_writes[..usize::from(trace.trace_write_len)] {
                record.push_write(*write);
            }
            record.write_overflow = trace.trace_write_overflow;
            push_pce_register_deltas(
                &mut record,
                &trace_registers_before.expect("trace state is present"),
                &pce_trace_registers(&self.cpu),
            );
            self.instruction_trace.push(record);
        }
        if let PceCpuAction::Instruction(step) = action {
            self.opcode_history.push(PceOpcodeHistoryEntry {
                logical_pc: step.pc,
                physical_pc: step.physical_pc,
                opcode: step.opcode,
                master_ticks: self.master_ticks,
            });
        }
        let hit_event = matches!(action, PceCpuAction::Interrupt(_))
            && self.debug.check_event(DebugEvent::Interrupt)
            || dma_completed && self.debug.check_event(DebugEvent::Dma);
        if hit_event {
            self.execution_state = PceExecutionState::Suspended;
            self.suspend_after_instruction = false;
        }
        if self.debug.hit_watchpoint.is_some() {
            self.execution_state = PceExecutionState::Suspended;
            self.suspend_after_instruction = false;
        }
        self.trace_frame = self.trace_frame.wrapping_add(frames_published);
        Ok(PceMachineStep {
            action,
            entering_speed,
            wait_cycles,
            vram_contention_wait_cycles: contention_wait_cycles,
            master_ticks,
            vce_lines,
            frames_published,
        })
    }

    fn commit_elapsed_cpu_time(&mut self, master_ticks: u64) -> Result<(), PceMachineError> {
        let next_master_ticks = checked_clock_add(
            self.master_ticks,
            master_ticks,
            PceClockCounter::MasterTicks,
        )?;
        self.master_ticks = next_master_ticks;
        self.refresh_vdc_irq1();
        self.refresh_cdrom2_irq2();
        Ok(())
    }

    #[cfg(test)]
    pub(in super::super) fn force_unsupported_opcode_trap_after_fetch(
        &mut self,
    ) -> Result<PceMachineStep, PceMachineError> {
        self.step_boundary_faulting_with(|cpu, bus| {
            let pc = cpu.cpu().registers().pc;
            let opcode = bus.read(cpu.cpu().logical_to_physical(pc));
            cpu.cpu_mut().registers_mut().pc = pc.wrapping_add(1);
            Err(CpuTrap::UnsupportedOpcode { pc, opcode })
        })
    }

    #[cfg(test)]
    pub(in super::super) fn advance_devices_for_test(
        &mut self,
        master_ticks: u64,
    ) -> Result<(u64, u64), PceMachineError> {
        let (error, elapsed, lines, frames) = {
            let mut bus = TimedMachineBus::new(
                &mut self.bus,
                &mut self.front_video,
                &mut self.back_video,
                &mut self.vce_line_accumulator,
                &mut self.vdc_pixel_clock_remainder,
                &mut self.vce_line_index,
                &mut self.vce_frame_length,
                1,
                None,
                &mut self.debug,
            );
            bus.advance_devices(master_ticks);
            (
                bus.fault,
                bus.elapsed_master_ticks,
                bus.vce_lines,
                bus.frames_published,
            )
        };
        self.cpu.advance_master_ticks(elapsed);
        self.commit_elapsed_cpu_time(elapsed)?;
        if let Some(error) = error {
            return Err(error);
        }
        Ok((lines, frames))
    }

    #[inline]
    pub(super) fn refresh_vdc_irq1(&mut self) {
        self.cpu.set_irq1_line(self.bus.devices().vdc_irq_level());
    }

    #[inline]
    pub(super) fn refresh_cdrom2_irq2(&mut self) {
        self.cpu
            .set_irq2_line(self.bus.devices().cdrom2_irq_level());
    }
}

fn pce_trace_registers(cpu: &HuC6280) -> [u32; 15] {
    let registers = cpu.cpu().registers();
    let mapping = cpu.cpu().mapping_registers();
    [
        u32::from(registers.a),
        u32::from(registers.x),
        u32::from(registers.y),
        u32::from(registers.sp),
        u32::from(registers.pc),
        u32::from(registers.status.bits()),
        u32::from(mapping[0]),
        u32::from(mapping[1]),
        u32::from(mapping[2]),
        u32::from(mapping[3]),
        u32::from(mapping[4]),
        u32::from(mapping[5]),
        u32::from(mapping[6]),
        u32::from(mapping[7]),
        u32::from(matches!(cpu.cpu().speed_mode(), SpeedMode::High)),
    ]
}

fn push_pce_register_deltas(
    record: &mut InstructionTraceRecord,
    before: &[u32; 15],
    after: &[u32; 15],
) {
    for (register, (&before, &after)) in before.iter().zip(after).enumerate() {
        if before != after {
            record.push_register_delta(RegisterDelta {
                register: register as u8,
                value: after,
            });
        }
    }
}

#[inline]
const fn master_ticks_per_cpu_cycle(speed: SpeedMode) -> u64 {
    match speed {
        SpeedMode::Low => PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
        SpeedMode::High => PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
    }
}
