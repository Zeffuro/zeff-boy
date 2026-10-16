use crate::device::{Datum, Device, Event, Reply, Script};
use crate::entry_axes::{Api, EntryAxis, IRQ, TRACE};
use crate::fixed::Fixed;
use crate::literal::Value;
use crate::oracle::{self, Expected as Primary};
use crate::seed::{self, LIMIT};
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{TimedStep, TimingFault};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completion {
    Origin(TimedStep),
    Trace(Option<u64>),
    Instruction(TimedStep),
    Interrupt { level: u8, vector: u8, clocks: u64 },
}

#[derive(Clone, Debug)]
pub struct Expected {
    pub axis: EntryAxis,
    pub origin: Primary,
    pub initial_device: Device,
    pub origin_device: Device,
    pub device: Device,
    pub r: Registers,
    pub queue: [u16; 2],
    pub clock: u64,
    pub level: u8,
    pub edge: bool,
    pub trace: bool,
    pub error: Option<TimingFault<usize>>,
    pub completion: Option<Completion>,
    pub attempts: Fixed<&'static str>,
    pub callbacks: Fixed<&'static str>,
    pub fault: Option<&'static str>,
    pub blocked: Option<&'static str>,
}

fn configure(device: &mut Device, axis: &EntryAxis, accepted: bool) {
    device.entry_mode = Some(axis.trace);
    device.reply = axis.reply;
    device.entry_fail = axis
        .value
        .find("entry_fail_phase")
        .filter(|value| !matches!(value, Value::Null))
        .map(Value::text);
    device.entry_failed_deposit = axis
        .value
        .find("entry_failed_write_deposit")
        .is_some_and(Value::boolean);
    let mut extra = Vec::new();
    if axis.trace {
        extra.extend(TRACE);
    } else {
        if !accepted {
            extra.push("IQ_NF");
        }
        extra.extend(IRQ);
    }
    for phase in extra {
        device.phases[device.phase_count] = Some(phase);
        device.phase_count += 1;
    }
    if let Some(Value::Object(fields)) = axis.value.find("entry_waits") {
        for (name, wait) in fields {
            let wait = wait.number() as u32;
            if let Some(slot) = device.wait_config.slots[..device.wait_config.len]
                .iter_mut()
                .find(|slot| slot.is_some_and(|(old, _)| old == *name))
            {
                *slot = Some((*name, wait));
            } else {
                device.wait_config.push((*name, wait));
            }
            let position = device.phases[..device.phase_count]
                .iter()
                .position(|phase| *phase == Some(*name))
                .unwrap();
            device.waits[position] = wait;
        }
    }
}

impl Expected {
    pub fn result(&self) -> Result<Completion, TimingFault<usize>> {
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(self.completion.clone().unwrap()),
        }
    }
    pub fn cpu_image(&self) -> Primary {
        let mut image = self.origin.clone();
        image.r = self.r;
        image.queue = self.queue;
        image.device.clock = self.clock;
        image.initial.level = self.level;
        image.initial.edge = self.edge;
        image.trace = self.trace;
        image.error = self.error.clone();
        image
    }
    fn fail(&mut self, phase: &'static str, error: TimingFault<usize>) {
        self.fault = Some(phase);
        self.error = Some(error);
    }
    fn idle(&mut self, phase: &'static str, clocks: u32) -> bool {
        let Some(end) = self.clock.checked_add(u64::from(clocks)) else {
            self.blocked = Some(phase);
            self.fail(phase, TimingFault::ClockOverflow);
            return false;
        };
        self.device.events.push(Event::Idle {
            phase,
            start: self.clock,
            clocks,
        });
        self.device.entry_idle_count += 1;
        self.clock = end;
        self.device.clock += u64::from(clocks);
        true
    }
    fn transfer(
        &mut self,
        phase: &'static str,
        address: u32,
        direction: Direction,
        payload: u16,
        route: (FunctionCode, AccessKind),
    ) -> Option<Datum> {
        let (fc, kind) = route;
        if address & 1 != 0 {
            self.fail(phase, TimingFault::AddressError { address });
            return None;
        }
        self.attempts.push(phase);
        if self.clock > LIMIT {
            self.blocked = Some(phase);
            self.fail(phase, TimingFault::ClockOverflow);
            return None;
        }
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK,
            function_code: fc,
            kind,
            lanes: ByteLanes::Both,
        };
        let index = self.device.transfers;
        let wait = self.device.waits[self.device.phases[..self.device.phase_count]
            .iter()
            .position(|name| *name == Some(phase))
            .unwrap()];
        let acknowledge = kind == AccessKind::InterruptAcknowledge;
        let failed =
            self.device.entry_fail == Some(phase) || acknowledge && self.axis.reply == Reply::Host;
        let sampled = if acknowledge {
            Datum::Reply(self.axis.reply)
        } else {
            Datum::Word(self.device.words.sample(address))
        };
        let deposited =
            direction == Direction::Write && (!failed || self.device.entry_failed_deposit);
        if deposited {
            self.device.words.insert(address, payload);
        }
        self.device.events.push(Event::Transfer {
            phase,
            access,
            direction,
            value: if direction == Direction::Write {
                Datum::Word(payload)
            } else {
                sampled
            },
            start: self.clock,
            wait,
            elapsed: 4 + u64::from(wait),
            failed,
            deposited,
            index,
        });
        self.device.scripts.push(Script {
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
        self.device.transfers += 1;
        self.callbacks.push(phase);
        self.device.clock += 4 + u64::from(wait);
        if acknowledge && self.axis.reply == Reply::Unsupported && !failed {
            self.fail(phase, TimingFault::InterruptAcknowledgeUnsupported);
            return None;
        }
        self.clock += 4 + u64::from(wait);
        if failed {
            self.fail(
                phase,
                TimingFault::Bus {
                    access,
                    direction,
                    source: index,
                },
            );
            None
        } else {
            Some(sampled)
        }
    }
    fn read(
        &mut self,
        phase: &'static str,
        address: u32,
        kind: AccessKind,
        fc: FunctionCode,
    ) -> Option<u16> {
        match self.transfer(phase, address, Direction::Read, 0, (fc, kind))? {
            Datum::Word(value) => Some(value),
            _ => panic!("word"),
        }
    }
    fn write(&mut self, phase: &'static str, address: u32, value: u16) -> bool {
        self.transfer(
            phase,
            address,
            Direction::Write,
            value,
            (FunctionCode::SupervisorData, AccessKind::Stack),
        )
        .is_some()
    }
    fn execute(&mut self, accepted: bool) {
        let start = self.clock;
        if self.axis.trace && self.axis.api == Api::Instruction {
            self.completion = Some(Completion::Origin(self.origin.result().unwrap()));
            return;
        }
        if !self.axis.trace {
            self.level = self.axis.irq;
            self.edge = self.axis.edge;
            if !accepted {
                assert_eq!(self.queue[0], 0x4e71);
                let fc = if self.r.status_register() & 0x2000 != 0 {
                    FunctionCode::SupervisorProgram
                } else {
                    FunctionCode::UserProgram
                };
                let Some(next) =
                    self.read("IQ_NF", self.r.pc.wrapping_add(4), AccessKind::Prefetch, fc)
                else {
                    return;
                };
                self.r.pc = self.r.pc.wrapping_add(2);
                self.queue = [self.queue[1], next];
                self.completion = Some(Completion::Instruction(TimedStep {
                    opcode: 0x4e71,
                    clocks: self.clock - start,
                    exception: None,
                }));
                return;
            }
        }
        let (pc, sr, sp) = (self.r.pc, self.r.status_register(), self.r.ssp);
        let trace = self.axis.trace;
        let prefix = if trace { "TR_PCL" } else { "IQ_PCL" };
        if !self.idle(
            if trace { "TraceI4" } else { "InterruptI6" },
            if trace { 4 } else { 6 },
        ) {
            return;
        }
        if !self.write(prefix, sp.wrapping_sub(2), pc as u16) {
            return;
        }
        let mut vector = 9u8;
        let irq = if self.edge { 7 } else { self.level };
        if !trace {
            let Some(Datum::Reply(reply)) = self.transfer(
                "IQ_ACK",
                0xfffffff0 | u32::from(irq) << 1,
                Direction::Read,
                0,
                (FunctionCode::CpuSpace, AccessKind::InterruptAcknowledge),
            ) else {
                return;
            };
            vector = match reply {
                Reply::Autovector => 24 + irq,
                Reply::Spurious => 24,
                Reply::Vector64 => 64,
                _ => unreachable!(),
            };
            if irq == 7 {
                self.edge = false;
            }
            if !self.idle("InterruptI4", 4) {
                return;
            }
        }
        let phases = if trace {
            ["TR_SR", "TR_PCH", "TR_VH", "TR_VL", "TR_H0", "TR_H1"]
        } else {
            ["IQ_SR", "IQ_PCH", "IQ_VH", "IQ_VL", "IQ_H0", "IQ_H1"]
        };
        if !self.write(phases[0], sp.wrapping_sub(6), sr)
            || !self.write(phases[1], sp.wrapping_sub(4), (pc >> 16) as u16)
        {
            return;
        }
        let address = u32::from(vector) * 4;
        let Some(high) = self.read(
            phases[2],
            address,
            AccessKind::Vector,
            FunctionCode::SupervisorData,
        ) else {
            return;
        };
        let Some(low) = self.read(
            phases[3],
            address + 2,
            AccessKind::Vector,
            FunctionCode::SupervisorData,
        ) else {
            return;
        };
        let target = u32::from(high) << 16 | u32::from(low);
        let Some(first) = self.read(
            phases[4],
            target,
            AccessKind::Prefetch,
            FunctionCode::SupervisorProgram,
        ) else {
            return;
        };
        if !self.idle(if trace { "TraceI2" } else { "InterruptI2" }, 2) {
            return;
        }
        let Some(second) = self.read(
            phases[5],
            target.wrapping_add(2),
            AccessKind::Prefetch,
            FunctionCode::SupervisorProgram,
        ) else {
            return;
        };
        self.r.ssp = sp.wrapping_sub(6);
        self.r.pc = target;
        self.r.set_status_register(if trace {
            (sr | 0x2000) & !0x8000
        } else {
            (sr | 0x2000) & !0x8700 | u16::from(irq) << 8
        });
        self.queue = [first, second];
        if trace {
            self.trace = false;
            if self.axis.api == Api::Service {
                self.completion = Some(Completion::Trace(Some(self.clock - start)));
            } else {
                let mut step = self.origin.result().unwrap();
                step.clocks = self.clock - self.origin.initial.device.clock;
                step.exception = Some(9);
                self.completion = Some(Completion::Instruction(step));
            }
        } else {
            self.completion = Some(Completion::Interrupt {
                level: irq,
                vector,
                clocks: self.clock - start,
            });
        }
    }
}

pub fn predict(axis: EntryAxis) -> Expected {
    let origin = oracle::predict(seed::build(&axis.primary));
    assert!(origin.importable && origin.error.is_none());
    let accepted = axis.edge || axis.irq > (origin.r.status_register() >> 8 & 7) as u8;
    let mut initial_device = origin.initial.device.clone();
    let mut device = origin.device.clone();
    configure(&mut initial_device, &axis, accepted);
    configure(&mut device, &axis, accepted);
    let mut expected = Expected {
        r: origin.r,
        queue: origin.queue,
        clock: origin.device.clock,
        level: origin.initial.level,
        edge: origin.initial.edge,
        trace: origin.trace,
        origin,
        axis,
        initial_device,
        origin_device: device.clone(),
        device,
        error: None,
        completion: None,
        attempts: Fixed::default(),
        callbacks: Fixed::default(),
        fault: None,
        blocked: None,
    };
    expected.execute(accepted);
    expected
}
