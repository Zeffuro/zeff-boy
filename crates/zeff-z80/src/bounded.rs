use super::*;

mod state;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrefixState {
    pending: Option<IndexPrefix>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IndexPrefix {
    pc: u16,
    selected: u8,
}

impl PrefixState {
    pub(super) fn index(pc: u16, selected: u8) -> Self {
        Self {
            pending: Some(IndexPrefix { pc, selected }),
        }
    }

    pub(super) fn selected_index(&self) -> Option<(u16, u8)> {
        self.pending.map(|prefix| (prefix.pc, prefix.selected))
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedStep {
    pub instruction: FetchedInstruction,
    pub complete: bool,
}

impl CpuStep for BoundedStep {
    fn cpu_cycles(&self) -> Option<NonZeroU64> {
        self.instruction.cpu_cycles()
    }
}

pub(super) enum StepStart {
    Blocked,
    Handled(FetchedInstruction),
    Interrupt(interrupt::Service),
    Opcode,
}

impl Cpu {
    pub(super) fn begin_step<B: Z80Bus>(&mut self, bus: &mut B) -> StepStart {
        if self.state == CpuState::Suspended {
            return StepStart::Blocked;
        }
        self.clear_step_observation();
        if let Some(service) = interrupt::Service::poll(self, bus) {
            return StepStart::Interrupt(service);
        }
        if self.state == CpuState::Halted {
            bus.cpu_read(self.regs.pc);
            return StepStart::Handled(self.complete_halt_fetch());
        }
        StepStart::Opcode
    }

    pub(super) fn clear_step_observation(&mut self) {
        self.instruction_bytes = [0; 4];
        self.instruction_byte_count = 0;
        self.last_m1_fetch_count = 0;
        self.last_step_was_interrupt = false;
    }

    pub(super) fn complete_halt_fetch(&mut self) -> FetchedInstruction {
        self.increment_refresh_register();
        self.cycles = self.cycles.wrapping_add(u64::from(CYCLES_HALT));
        FetchedInstruction {
            pc: self.regs.pc,
            opcode: 0x76,
            cycles: CYCLES_HALT,
        }
    }

    /// Pair CPU and prefix state when cloning/resetting; do not mix APIs or codecs while pending.
    /// Cycles and I/O offsets describe this call, including the terminal call after prefixes.
    pub fn step_bounded<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        state: &mut PrefixState,
    ) -> Option<BoundedStep> {
        self.step_bounded_inner(bus, state, None, None)
    }

    pub(super) fn step_bounded_inner<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        state: &mut PrefixState,
        transfer: Option<&mut Option<bus_segment::Transfer>>,
        legacy_relative: Option<bus_segment::LegacyRelative>,
    ) -> Option<BoundedStep> {
        if let Some(mut prefix) = state.pending {
            if self.state == CpuState::Suspended {
                return None;
            }
            let opcode_pc = self.regs.pc;
            let prior_fetches = self.last_m1_fetch_count;
            let opcode = self.fetch_u8(bus);
            if matches!(opcode, Z80_PREFIX_DD | Z80_PREFIX_FD) {
                self.increment_refresh_register();
                prefix.selected = opcode;
                state.pending = Some(prefix);
                self.cycles = self.cycles.wrapping_add(u64::from(CYCLES_M1));
                return Some(BoundedStep {
                    instruction: FetchedInstruction {
                        pc: prefix.pc,
                        opcode: self.last_opcode,
                        cycles: CYCLES_M1,
                    },
                    complete: false,
                });
            }
            self.last_m1_fetch_count = 0;
            self.increment_refresh_register();
            if self.defer_transfer(
                transfer,
                prefix.pc,
                opcode,
                Some(prefix.selected),
                legacy_relative,
            ) {
                self.last_m1_fetch_count = prior_fetches.saturating_add(self.last_m1_fetch_count);
                return Some(BoundedStep {
                    instruction: FetchedInstruction {
                        pc: prefix.pc,
                        opcode: self.last_opcode,
                        cycles: bus_segment::fetch_cycles(opcode),
                    },
                    complete: false,
                });
            }
            let cycles = if opcode == Z80_PREFIX_ED {
                self.execute_ed(bus, opcode_pc)
            } else {
                self.execute_index_terminal(bus, prefix.pc, prefix.selected, opcode)
                    - Z80_PREFIX_OVERHEAD
            };
            self.last_m1_fetch_count = prior_fetches.saturating_add(self.last_m1_fetch_count);
            state.pending = None;
            self.finish_instruction(cycles);
            return Some(BoundedStep {
                instruction: FetchedInstruction {
                    pc: prefix.pc,
                    opcode: self.last_opcode,
                    cycles,
                },
                complete: true,
            });
        }
        match self.begin_step(bus) {
            StepStart::Blocked => return None,
            StepStart::Handled(instruction) => {
                return Some(BoundedStep {
                    instruction,
                    complete: true,
                });
            }
            StepStart::Opcode => {}
            StepStart::Interrupt(service) => {
                let instruction = if let Some(slot) = transfer {
                    let (next, cycles) = service.step(self, bus);
                    *slot = next.map(bus_segment::Transfer::interrupt);
                    self.cycles = self.cycles.wrapping_add(u64::from(cycles));
                    return Some(BoundedStep {
                        instruction: FetchedInstruction {
                            pc: service.pc(),
                            opcode: Z80_INTERRUPT_ACK_OPCODE,
                            cycles,
                        },
                        complete: next.is_none(),
                    });
                } else {
                    service.execute(self, bus)
                };
                return Some(BoundedStep {
                    instruction,
                    complete: true,
                });
            }
        }
        let pc = self.regs.pc;
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();
        self.last_opcode_pc = pc;
        self.last_opcode = opcode;
        if matches!(opcode, Z80_PREFIX_DD | Z80_PREFIX_FD) {
            state.pending = Some(IndexPrefix {
                pc,
                selected: opcode,
            });
            self.cycles = self.cycles.wrapping_add(u64::from(CYCLES_M1));
            Some(BoundedStep {
                instruction: FetchedInstruction {
                    pc,
                    opcode,
                    cycles: CYCLES_M1,
                },
                complete: false,
            })
        } else if self.defer_transfer(transfer, pc, opcode, None, legacy_relative) {
            Some(BoundedStep {
                instruction: FetchedInstruction {
                    pc,
                    opcode,
                    cycles: bus_segment::fetch_cycles(opcode),
                },
                complete: false,
            })
        } else {
            let cycles = self.execute_unprefixed(bus, pc, opcode);
            self.finish_instruction(cycles);
            Some(BoundedStep {
                instruction: FetchedInstruction { pc, opcode, cycles },
                complete: true,
            })
        }
    }
}
