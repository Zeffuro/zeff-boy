use super::*;

impl Plan {
    #[allow(clippy::too_many_arguments)]
    pub fn retire(
        self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        audio: &Audio,
        tick: u64,
        sample: Option<u8>,
        waits: u32,
    ) -> Result<(Cpu, BusState)> {
        self.retire_capturing(
            RetirementState { cpu, state },
            RetirementMemory {
                memory,
                audio,
                tick,
            },
            RetirementInput { sample, waits },
            &mut (),
        )
    }

    pub fn retire_capturing<C: crate::sound_trace::physical::HalfCapture>(
        self,
        before: RetirementState<'_>,
        context: RetirementMemory<'_>,
        input: RetirementInput,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        let RetirementState { cpu, state } = before;
        let RetirementMemory {
            memory,
            audio,
            tick,
        } = context;
        let RetirementInput { sample, waits } = input;
        if self.ed_word.is_some() {
            return if C::ENABLED {
                self.retire_ed_word_capturing(cpu, state, sample, waits, capture)
            } else {
                self.retire_ed_word(cpu, state, sample, waits)
            };
        }
        if self.indexed.is_some() {
            if cpu.indexed_cb_cycle_pending(&state) {
                return if C::ENABLED {
                    self.retire_indexed_cb_capturing(
                        cpu,
                        state,
                        memory,
                        RetirementInput { sample, waits },
                        capture,
                    )
                } else {
                    self.retire_indexed_cb(cpu, state, memory, sample, waits)
                };
            }
            return if C::ENABLED {
                self.retire_indexed_capturing(
                    cpu,
                    state,
                    memory,
                    RetirementInput { sample, waits },
                    capture,
                )
            } else {
                self.retire_indexed(cpu, state, memory, sample, waits)
            };
        }
        if self.copy.is_some() {
            return if C::ENABLED {
                self.retire_copy_capturing(
                    cpu,
                    state,
                    memory,
                    RetirementInput { sample, waits },
                    capture,
                )
            } else {
                self.retire_copy(cpu, state, memory, sample, waits)
            };
        }
        ensure!(self.djnz.is_none(), "DJNZ requires its caller-owned tail");
        ensure!(self.jr.is_none(), "JR requires its caller-owned tail");
        if self.im1.is_some() {
            return if C::ENABLED {
                self.retire_im1_capturing(
                    cpu,
                    state,
                    memory,
                    RetirementInput { sample, waits },
                    capture,
                )
            } else {
                self.retire_im1(cpu, state, memory, sample, waits)
            };
        }
        if self.external {
            let request = cpu
                .next_physical_cycle(&state)
                .map_err(|e| anyhow::anyhow!("SOUND cycle request: {e:?}"))?;
            let sample = CycleSample {
                address: self.address,
                m1: false,
                write: self.shape == Shape::Write,
                value: if self.shape == Shape::Write {
                    self.value
                } else {
                    sample.ok_or_else(|| anyhow::anyhow!("missing SOUND external byte"))?
                },
                known_mask: 255,
            };
            let completion = request
                .complete(
                    sample,
                    waits,
                    CycleInterrupts {
                        maskable_pending: self.irq,
                        nmi_pending: false,
                    },
                )
                .map_err(|e| anyhow::anyhow!("SOUND cycle completion: {e:?}"))?;
            let mut next = cpu.clone();
            let mut state = state;
            let step = crate::sound_trace::physical::retirement::physical(
                &mut next, &mut state, completion, capture,
            )
            .map_err(|e| anyhow::anyhow!("SOUND cycle retirement: {e:?}"))?;
            ensure!(
                step.instruction.pc == self.instruction.pc
                    && step.instruction.opcode == self.instruction.opcode,
                "SOUND external instruction identity differs"
            );
            return Ok((next, state));
        }
        let replay = if matches!(self.shape, Shape::Fetch | Shape::Read) {
            Some((
                self.address,
                sample.ok_or_else(|| anyhow::anyhow!("missing frozen local SOUND byte"))?,
            ))
        } else {
            None
        };
        let starts = !state.pending() && !cpu.is_halted();
        let run = if C::ENABLED {
            reader::run_capturing::<true>(
                cpu,
                state,
                reader::ReadContext {
                    memory,
                    audio: Some(audio),
                    tick,
                    irq: self.irq,
                },
                replay,
                self.steps,
            )?
        } else {
            reader::run_capturing::<false>(
                cpu,
                state,
                reader::ReadContext {
                    memory,
                    audio: Some(audio),
                    tick,
                    irq: self.irq,
                },
                replay,
                self.steps,
            )?
        };
        let reader::Run {
            mut next,
            state,
            instruction,
            accesses,
            complete,
            consumed,
            input_lost,
        } = run;
        ensure!(
            instruction == self.instruction,
            "SOUND replay changed segment identity or duration"
        );
        if self.shape == Shape::Internal {
            ensure!(
                accesses.is_empty(),
                "SOUND internal segment gained a callback"
            );
        } else if !(cpu.is_halted() && accesses.is_empty()) {
            ensure!(accesses.len() == 1, "SOUND callback count changed");
            let a = accesses[0];
            ensure!(
                a.address == self.address
                    && a.write == (self.shape == Shape::Write)
                    && (!a.write || a.value == self.value),
                "SOUND callback identity changed"
            );
        }
        let mut instruction = instruction;
        ensure!(
            next.extend_bus_segment(&mut instruction, waits),
            "SOUND cycle counter overflow"
        );
        if C::ENABLED {
            capture.cpu_snapshot(crate::sound_trace::physical::CpuSnapshot::new(
                cpu,
                &next,
                starts && consumed.is_some(),
            ));
        }
        capture.classified_retired(crate::sound_trace::physical::ClassifiedRetirement {
            complete,
            consumed,
            cpu_before: cpu.cycles(),
            cpu_after: next.cycles(),
            input_lost,
            kind: if cpu.is_halted() {
                zeff_z80::RetirementKind::HaltPoll
            } else {
                zeff_z80::RetirementKind::Instruction
            },
        });
        Ok((next, state))
    }
}
