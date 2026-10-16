use crate::device::{Device, Effect, Event, Script};
use crate::fixed::Fixed;
use crate::seed::{self, LIMIT, Seed};
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{TimedStep, TimingFault};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    pub phase: &'static str,
    pub registers: Registers,
}

#[derive(Clone, Debug)]
pub struct Expected {
    pub initial: Seed,
    pub r: Registers,
    pub queue: [u16; 2],
    pub device: Device,
    pub error: Option<TimingFault<usize>>,
    pub trace: bool,
    pub images: Fixed<Image>,
    pub attempts: Fixed<&'static str>,
    pub cached: Option<u32>,
    pub latched: Option<u32>,
    pub calculated: Option<u32>,
    pub fault_phase: Option<&'static str>,
    pub blocked_phase: Option<&'static str>,
    pub importable: bool,
}

pub fn flags(old: u16, bits: u32, result: u32) -> u16 {
    let mask = if bits == 32 {
        u32::MAX
    } else {
        (1 << bits) - 1
    };
    let value = result & mask;
    old & 0xfff0
        | if value == 0 { 4 } else { 0 }
        | if value & (1 << (bits - 1)) != 0 { 8 } else { 0 }
}

impl Expected {
    pub fn result(&self) -> Result<TimedStep, TimingFault<usize>> {
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(TimedStep {
                opcode: self.initial.axis.word,
                clocks: self.device.clock - self.initial.device.clock,
                exception: None,
            }),
        }
    }

    pub fn debug(&self, initial: bool) -> String {
        let (r, queue, clock, frozen, trace) = if initial {
            (
                &self.initial.r,
                self.initial.queue,
                self.initial.device.clock,
                false,
                false,
            )
        } else {
            (
                &self.r,
                self.queue,
                self.device.clock,
                self.error.is_some(),
                self.trace,
            )
        };
        format!(
            "PrefetchCpu {{ registers: {:?}, clock: CpuClocks({}), pipeline: Some(Pipeline {{ address: {}, words: {:?} }}), frozen: {}, stopped: false, interrupts: InterruptState {{ level: {}, level7_edge: {} }}, pending_trace: {}, physical_state: Idle, physical_inspection: DiagnosticCompleteValues, imported_boundary: None }}",
            r, clock, r.pc, queue, frozen, self.initial.level, self.initial.edge, trace
        )
    }

    fn reserve(&mut self, phase: &'static str) -> bool {
        self.attempts.push(phase);
        if self.device.clock > LIMIT {
            self.error = Some(TimingFault::ClockOverflow);
            self.fault_phase = Some(phase);
            self.blocked_phase = Some(phase);
            false
        } else {
            true
        }
    }

    fn idle(&mut self, phase: &'static str, clocks: u32) -> bool {
        if let Some(end) = self.device.clock.checked_add(u64::from(clocks)) {
            self.device.events.push(Event::Idle {
                phase,
                start: self.device.clock,
                clocks,
            });
            self.images.push(Image {
                phase,
                registers: self.r,
            });
            self.device.clock = end;
            true
        } else {
            self.error = Some(TimingFault::ClockOverflow);
            self.fault_phase = Some(phase);
            self.blocked_phase = Some(phase);
            false
        }
    }

    fn mutate(&mut self, phase: &'static str, after: bool) {
        if let Some(change) = self
            .device
            .mutation
            .filter(|change| change.phase == phase && change.after == after)
        {
            let before = self.device.words.sample(change.address);
            self.device.words.insert(change.address, change.value);
            self.device.effects.push(Effect {
                mutation: change,
                before,
                clock: self.device.clock,
            });
        }
    }

    fn transfer(
        &mut self,
        phase: &'static str,
        address: u32,
        direction: Direction,
        payload: u16,
    ) -> Option<u16> {
        if !self.reserve(phase) {
            return None;
        }
        let data = matches!(phase, "RH" | "W");
        let supervisor = self.r.status_register() & 0x2000 != 0;
        let lanes = if !data {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        };
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK & !1,
            function_code: match (data, supervisor) {
                (true, true) => FunctionCode::SupervisorData,
                (true, false) => FunctionCode::UserData,
                (false, true) => FunctionCode::SupervisorProgram,
                (false, false) => FunctionCode::UserProgram,
            },
            kind: if data {
                AccessKind::Data
            } else {
                AccessKind::Prefetch
            },
            lanes,
        };
        self.mutate(phase, false);
        let index = self.device.transfers;
        let sampled = self.device.words.sample(access.bus_address);
        let wait = self.device.waits[self.initial.device.phases[..self.initial.device.phase_count]
            .iter()
            .position(|name| *name == Some(phase))
            .unwrap()];
        let failed = self.device.fail == Some(phase);
        let deposited =
            direction == Direction::Write && (!failed || self.device.failed_write_deposit);
        if deposited {
            let value = if address & 1 == 0 {
                sampled & 0xff | payload & 0xff00
            } else {
                sampled & 0xff00 | payload & 0xff
            };
            self.device.words.insert(access.bus_address, value);
        }
        self.device.events.push(Event::Transfer {
            phase,
            access,
            direction,
            value: if direction == Direction::Write {
                payload
            } else {
                sampled
            },
            start: self.device.clock,
            wait,
            elapsed: 4 + u64::from(wait),
            failed,
            deposited,
            index,
        });
        self.images.push(Image {
            phase,
            registers: self.r,
        });
        self.device.scripts.push(Script::Ordinary {
            phase,
            access,
            direction,
            wait,
            result: if failed {
                Err(index)
            } else {
                Ok((direction == Direction::Read).then_some(sampled))
            },
        });
        self.mutate(phase, true);
        self.device.clock = self.device.clock.checked_add(4 + u64::from(wait)).unwrap();
        self.device.transfers += 1;
        if failed {
            self.error = Some(TimingFault::Bus {
                access,
                direction,
                source: index,
            });
            self.fault_phase = Some(phase);
            None
        } else {
            Some(sampled)
        }
    }

    fn read(&mut self, phase: &'static str, address: u32) -> Option<u16> {
        self.transfer(phase, address, Direction::Read, 0)
    }

    fn logic(&self, source: u32, mask: u32) -> u32 {
        if self.initial.axis.word & 0xff00 == 0x4200 {
            0
        } else {
            !source & mask
        }
    }

    fn execute(&mut self) {
        if !self.importable {
            return;
        }
        let seed = &self.initial;
        let (mode, reg, bits, count) = (seed.mode, seed.register, seed.bits, seed.extensions);
        let (pc, retained) = (self.r.pc, self.queue[1]);
        let initial_trace = self.r.status_register() & 0x8000 != 0;
        if mode == 0 {
            let old = self.r.d[reg];
            let mask = if bits == 32 {
                u32::MAX
            } else {
                (1 << bits) - 1
            };
            self.latched = Some(old & mask);
            let result = self.logic(old & mask, mask);
            self.calculated = Some(result);
            if !self.reserve("F") {
                return;
            }
            self.r
                .set_status_register(flags(self.r.status_register(), bits, result));
            let publish = if bits == 32 { 65535 } else { mask };
            self.r.d[reg] = old & !publish | result & publish;
            let Some(fetched) = self.read("F", pc.wrapping_add(4)) else {
                return;
            };
            if bits == 32 {
                self.r.d[reg] = result;
                if !self.idle("LongTailI2", 2) {
                    return;
                }
            }
            self.r.d[reg] = old & !mask | result;
            self.r.pc = pc.wrapping_add(2);
            self.queue = [retained, fetched];
            self.trace = initial_trace;
            return;
        }
        let base = seed::active(&self.r, reg);
        if matches!(mode, 4 | 6) && !self.idle("InstructionI2", 2) {
            return;
        }
        if mode == 4 {
            let address = base.wrapping_sub(if reg == 7 { 2 } else { 1 });
            self.cached = Some(address);
            seed::set_active(&mut self.r, reg, address);
        }
        let retained_next = if mode >= 5 {
            let Some(first) = self.read(if count == 2 { "E1" } else { "E" }, pc.wrapping_add(4))
            else {
                return;
            };
            if count == 2 {
                let Some(next) = self.read("E2", pc.wrapping_add(6)) else {
                    return;
                };
                self.cached = Some(u32::from(retained) << 16 | u32::from(first));
                next
            } else {
                self.cached = Some(match mode {
                    5 => base.wrapping_add_signed(i32::from(retained as i16)),
                    6 => seed::indexed(&self.r, base, retained),
                    _ => retained as i16 as i32 as u32,
                });
                first
            }
        } else {
            self.cached = Some(self.cached.unwrap_or(base));
            retained
        };
        if mode == 3 {
            seed::set_active(
                &mut self.r,
                reg,
                base.wrapping_add(if reg == 7 { 2 } else { 1 }),
            );
        }
        let address = self.cached.unwrap();
        let Some(word) = self.read("RH", address) else {
            return;
        };
        let source = u32::from(if address & 1 == 0 {
            word >> 8
        } else {
            word & 255
        });
        self.latched = Some(source);
        let result = self.logic(source, 255);
        self.calculated = Some(result);
        if !self.reserve("F") {
            return;
        }
        self.r
            .set_status_register(flags(self.r.status_register(), 8, result));
        let anchor = pc.wrapping_add(2 + count as u32 * 2);
        let Some(fetched) = self.read("F", anchor.wrapping_add(2)) else {
            return;
        };
        if self
            .transfer(
                "W",
                address,
                Direction::Write,
                if address & 1 == 0 {
                    (result << 8) as u16
                } else {
                    result as u16
                },
            )
            .is_none()
        {
            return;
        }
        self.r.pc = anchor;
        self.queue = [retained_next, fetched];
        self.trace = initial_trace;
    }
}

pub fn predict(seed: Seed) -> Expected {
    let mut expected = Expected {
        r: seed.r,
        queue: seed.queue,
        device: seed.device.clone(),
        error: None,
        trace: false,
        images: Fixed::default(),
        attempts: Fixed::default(),
        cached: None,
        latched: None,
        calculated: None,
        fault_phase: None,
        blocked_phase: None,
        importable: seed.r.pc & 1 == 0,
        initial: seed,
    };
    expected.execute();
    expected
}
