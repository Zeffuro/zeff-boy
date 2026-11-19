use super::*;
use crate::sound_trace::{AttemptCapture, CapturedBus, NoAttempt};

impl PendingSound {
    pub fn prepare(
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        now: u64,
    ) -> Result<Self, SoundFault> {
        Self::prepare_with(
            cpu,
            memory,
            cartridge,
            Preparation {
                now,
                state: BusState::default(),
                bounded: false,
                opn2_status: 0,
            },
            memory.irq_asserted,
            None,
        )
    }

    pub(super) fn prepare_with(
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        preparation: Preparation,
        irq_asserted: bool,
        legacy_relative: Option<RelativeReplay>,
    ) -> Result<Self, SoundFault> {
        let mut pending = None;
        Self::prepare_into_captured::<NoAttempt>(
            &mut pending,
            cpu,
            memory,
            cartridge,
            preparation,
            PreparationPolicy {
                irq_asserted,
                legacy_relative,
            },
        )?;
        Ok(pending.unwrap())
    }

    pub(crate) fn prepare_captured<A: AttemptCapture>(
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        preparation: Preparation,
        policy: PreparationPolicy,
    ) -> Result<(Self, A), SoundFault> {
        let mut pending = None;
        let attempt = Self::prepare_into_captured::<A>(
            &mut pending,
            cpu,
            memory,
            cartridge,
            preparation,
            policy,
        )?;
        Ok((pending.unwrap(), attempt))
    }

    pub(super) fn prepare_into_captured<A: AttemptCapture>(
        destination: &mut Option<Self>,
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        preparation: Preparation,
        policy: PreparationPolicy,
    ) -> Result<A, SoundFault> {
        Self::prepare_into_history_captured::<A>(
            destination,
            cpu,
            memory,
            (cartridge, None),
            preparation,
            policy,
        )
    }

    pub(super) fn prepare_into_history<'a>(
        destination: &mut Option<Self>,
        cpu: &Cpu,
        memory: &SoundMemory,
        source: (
            &'a Cartridge,
            Option<&'a [super::super::svp_reads::RomRead]>,
        ),
        preparation: Preparation,
        policy: PreparationPolicy,
    ) -> Result<(), SoundFault> {
        let (cartridge, historical_rom) = source;
        Self::prepare_into_history_captured::<NoAttempt>(
            destination,
            cpu,
            memory,
            (cartridge, historical_rom),
            preparation,
            policy,
        )
        .map(drop)
    }

    pub(super) fn prepare_into_history_captured<'a, A: AttemptCapture>(
        destination: &mut Option<Self>,
        cpu: &Cpu,
        memory: &SoundMemory,
        source: (
            &'a Cartridge,
            Option<&'a [super::super::svp_reads::RomRead]>,
        ),
        preparation: Preparation,
        policy: PreparationPolicy,
    ) -> Result<A, SoundFault> {
        let (cartridge, historical_rom) = source;
        let mut attempt = A::begin(cpu, preparation.state);
        let PreparationPolicy {
            irq_asserted,
            legacy_relative,
        } = policy;
        #[cfg(feature = "sound-shadow-observer")]
        let mut observation = observer::Attempt::begin();
        let Preparation {
            now,
            mut state,
            bounded,
            opn2_status,
        } = preparation;
        // Prepare in the final slot to keep the RAM snapshot from moving.
        *destination = Some(Self {
            cpu: cpu.clone(),
            memory: memory.snapshot(),
            instruction: FetchedInstruction {
                pc: 0,
                opcode: 0,
                cycles: 0,
            },
            deadline: 0,
            state,
            writes: WorkWrites::default(),
            register_writes: RegisterWrites::default(),
            sf001_writes: Sf001Writes::default(),
            sf002_writes: Sf002Writes::default(),
            psg_writes: PsgWrites::default(),
            opn2_writes: Opn2Writes::default(),
            bank_access: None,
            observed_bank_access: None,
            svp_history: None,
            any_bank_access: false,
            start: now / SOUND_CPU_DIVISOR * SOUND_CPU_DIVISOR,
            replay: None,
            trace_origin: None,
            #[cfg(feature = "sound-shadow-observer")]
            observer: observer::Token::empty(),
        });
        let pending = destination.as_mut().unwrap();
        #[cfg(feature = "sound-shadow-observer")]
        {
            pending.observer = observer::image(&pending.memory.ram.1, None);
        }
        let memory_cycle = bounded.then(|| state.next_memory_cycle()).flatten();
        let source_memory = cartridge.svp.as_ref().map(|_| memory.snapshot());
        let mut candidate = CandidateBus::new(&mut pending.memory, cartridge);
        candidate.historical_rom = historical_rom;
        candidate.memory.irq_asserted = irq_asserted;
        candidate.opn2_status = opn2_status;
        let (instruction, complete) = if bounded {
            let step = match legacy_relative {
                Some(RelativeReplay::Jr) => pending.cpu.step_bus_segment_legacy_jr(
                    &mut CapturedBus {
                        inner: &mut candidate,
                        attempt: &mut attempt,
                    },
                    &mut state,
                ),
                Some(RelativeReplay::Djnz) => pending.cpu.step_bus_segment_legacy_djnz(
                    &mut CapturedBus {
                        inner: &mut candidate,
                        attempt: &mut attempt,
                    },
                    &mut state,
                ),
                None => pending.cpu.step_bus_segment(
                    &mut CapturedBus {
                        inner: &mut candidate,
                        attempt: &mut attempt,
                    },
                    &mut state,
                ),
            };
            let step = step.expect("owned sound CPU cannot be suspended");
            (step.instruction, step.complete)
        } else {
            let step = pending
                .cpu
                .step_bounded(
                    &mut CapturedBus {
                        inner: &mut candidate,
                        attempt: &mut attempt,
                    },
                    &mut PrefixState::default(),
                )
                .expect("owned sound CPU cannot be suspended");
            if !step.complete {
                #[cfg(feature = "first-failure-diagnostic")]
                crate::diagnostic::preparation_failed(
                    cpu,
                    &pending.cpu,
                    (preparation.state, state),
                    step.instruction,
                    &candidate,
                    crate::diagnostic::SoundPreparationContext {
                        now,
                        bounded,
                        irq_asserted,
                        opn2_status,
                    },
                );
                if let Some(address) = candidate.cartridge_read.get() {
                    return Err(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    });
                }
                return Err(SoundFault::UnsupportedOpcode {
                    pc: step.instruction.pc,
                    opcode: step.instruction.opcode,
                });
            }
            (step.instruction, step.complete)
        };
        if let Some(fault) = candidate.fault.get() {
            #[cfg(feature = "first-failure-diagnostic")]
            crate::diagnostic::preparation_failed(
                cpu,
                &pending.cpu,
                (preparation.state, state),
                instruction,
                &candidate,
                crate::diagnostic::SoundPreparationContext {
                    now,
                    bounded,
                    irq_asserted,
                    opn2_status,
                },
            );
            return Err(fault);
        }
        if let Some(trap) = pending.cpu.trap() {
            return Err(SoundFault::CpuTrap(trap));
        }
        let expected = cpu
            .cycles()
            .checked_add(u64::from(instruction.cycles))
            .ok_or(SoundFault::ClockOverflow)?;
        if pending.cpu.cycles() != expected {
            return Err(SoundFault::ClockOverflow);
        }
        let deadline = (now / SOUND_CPU_DIVISOR)
            .checked_add(u64::from(instruction.cycles))
            .and_then(|cycles| cycles.checked_mul(SOUND_CPU_DIVISOR))
            .ok_or(SoundFault::ClockOverflow)?;
        if let Some(history) = historical_rom
            && candidate.historical_cursor.get() != history.len()
        {
            return Err(SoundFault::UnsupportedAccess {
                address: instruction.pc,
                direction: Direction::Read,
            });
        }
        let CandidateBus {
            svp_reads,
            writes,
            register_writes,
            sf001_writes,
            sf002_writes,
            psg_writes,
            opn2_writes,
            bank_access,
            accesses,
            timed_read,
            cartridge_read,
            ..
        } = candidate;
        let bank_access = bank_access.get();
        let accesses = accesses.get();
        let sf001_phase = (cartridge.sf001.is_some() || cartridge.sf002.is_some())
            && matches!(instruction.cycles, 3..=5);
        let lock_on_phase = bank_access
            .is_some_and(|access| cartridge.mode_sensitive_read(access.address))
            && matches!(instruction.cycles, 3..=5);
        let cartridge_phase = cartridge_read.get().is_some()
            && bounded
            && accesses == 1
            && (sf001_phase
                || lock_on_phase
                || instruction.cycles == CARTRIDGE_READ_CYCLES
                    && (cartridge.sf001.is_some()
                        || cartridge.sf002.is_some()
                        || instruction.pc <= SOUND_RAM_END))
            && bank_access.is_some_and(|access| access.direction == Direction::Read);
        if let Some(address) = cartridge_read.get()
            && !cartridge_phase
        {
            return Err(SoundFault::UnsupportedAccess {
                address,
                direction: Direction::Read,
            });
        }
        let sf001_write_phase = cartridge.sf001.is_some()
            && bounded
            && accesses == 1
            && matches!(instruction.cycles, 3..=5)
            && !sf001_writes.is_empty()
            && bank_access.is_some_and(|access| access.direction == Direction::Write);
        pending.instruction = instruction;
        pending.deadline = deadline;
        pending.state = state;
        pending.writes = writes;
        pending.register_writes = register_writes;
        pending.sf001_writes = sf001_writes;
        pending.sf002_writes = sf002_writes;
        pending.psg_writes = psg_writes;
        pending.opn2_writes = opn2_writes;
        let sf002_write_phase = cartridge.sf002.is_some()
            && bounded
            && accesses == 1
            && matches!(instruction.cycles, 3..=5)
            && !sf002_writes.is_empty()
            && bank_access.is_some_and(|access| access.direction == Direction::Write);
        if !sf002_writes.is_empty() && !sf002_write_phase {
            return Err(SoundFault::UnsupportedAccess {
                address: instruction.pc,
                direction: Direction::Write,
            });
        }
        let download_write = cartridge.download.is_some()
            && bank_access.is_some_and(|access| {
                crate::cartridge::download::memory(access.address)
                    && access.direction == Direction::Write
            });
        if download_write
            && !(bounded
                && accesses == 1
                && instruction.cycles == 3
                && memory_cycle == Some(3)
                && instruction.pc <= SOUND_RAM_END)
        {
            return Err(SoundFault::UnsupportedAccess {
                address: instruction.pc,
                direction: Direction::Write,
            });
        }
        pending.bank_access = if cartridge_phase || sf001_write_phase || sf002_write_phase {
            bank_access
        } else {
            memory_cycle
                .filter(|cycles| *cycles == instruction.cycles && accesses == 1)
                .and(bank_access)
        };
        pending.any_bank_access = bank_access.is_some();
        pending.observed_bank_access = if cartridge.svp.is_some()
            || cartridge.bounded_split()
            || cartridge.download.is_some()
            || cartridge
                .registers
                .as_ref()
                .is_some_and(|board| board.is_banked())
            || cartridge.lock_on_save.is_some()
            || cartridge.ssf2.is_some()
            || cartridge.sf002.is_some()
            || cartridge.sf004.is_some()
            || cartridge.extended_ssf.is_some()
            || cartridge.realtec.is_some()
        {
            bank_access
        } else {
            None
        };
        pending.replay = timed_read.get().then_some(ReadReplay {
            preparation,
            bank: memory.bank,
            irq_asserted,
            cycles: instruction.cycles,
            accesses,
            bank_identity: bank_access.map(|access| (access.address, access.direction)),
        });
        let reads = svp_reads.into_inner();
        if reads.changed() || (pending.replay.is_some() && reads.records[0].is_some()) {
            pending.svp_history = Some(Box::new(svp_history::SoundHistory {
                preparation,
                irq_asserted,
                legacy_relative,
                source_memory: source_memory.unwrap(),
                reads: reads.values(),
                reads_settled: false,
                reservation_waits: 0,
            }));
        }
        pending.extend_bus_segment(0)?;
        #[cfg(feature = "sound-shadow-observer")]
        observation.succeed();
        attempt.finish(instruction, complete);
        Ok(attempt)
    }
}
