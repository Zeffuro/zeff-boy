use super::*;
use device::{Device, Event};

pub(super) const B: u64 = 4 + u32::MAX as u64;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Idle(u32),
    Read(BusAccess),
    Write(BusAccess, u16),
    Ack(BusAccess),
    An(usize, u32),
    Sr(u16),
    Reserve,
    Commit(u32, [u16; 2]),
}

pub(super) struct Expected {
    pub registers: Registers,
    pub queue: [u16; 2],
    pub device: Device,
    pub result: Result<TimedStep, TimingFault<usize>>,
    pub trace: bool,
}

pub(super) fn registers(supervisor: bool, trace: bool) -> Registers {
    let mut r = Registers::new();
    r.set_status_register(
        0x071f | if supervisor { 0x2000 } else { 0 } | if trace { 0x8000 } else { 0 },
    );
    r.usp = 0x8000;
    r.ssp = 0x9000;
    r.pc = 0x1234_2000;
    r.a = [0x3000, 0x3100, 0x3200, 0x3300, 0x3400, 0x3500, 0x3600];
    r.d = [
        0xa1000010, 0xa2000010, 0xa3000020, 0xa4000030, 0xa5000040, 0xa6000050, 0xa7000060,
        0xa8000070,
    ];
    r
}

pub(super) fn an(r: Registers, reg: usize) -> u32 {
    if reg < 7 {
        r.a[reg]
    } else if r.status_register() & 0x2000 != 0 {
        r.ssp
    } else {
        r.usp
    }
}

pub(super) fn set_an(r: &mut Registers, reg: usize, value: u32) {
    if reg < 7 {
        r.a[reg] = value;
    } else if r.status_register() & 0x2000 != 0 {
        r.ssp = value;
    } else {
        r.usp = value;
    }
}

pub(super) fn access(address: u32, fc: FunctionCode, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_ffff,
        function_code: fc,
        kind,
        lanes: ByteLanes::Both,
    }
}

pub(super) fn read(actions: &mut Vec<Action>, address: u32, fc: FunctionCode, kind: AccessKind) {
    actions.push(Action::Read(access(address, fc, kind)));
}

pub(super) fn write(actions: &mut Vec<Action>, address: u32, value: u16) {
    actions.push(Action::Write(
        access(address, FunctionCode::SupervisorData, AccessKind::Stack),
        value,
    ));
}

pub(super) fn transfers(actions: &[Action]) -> usize {
    actions
        .iter()
        .filter(|a| matches!(a, Action::Read(_) | Action::Write(_, _) | Action::Ack(_)))
        .count()
}

pub(super) fn predict(
    r: Registers,
    queue: [u16; 2],
    input: &Device,
    actions: &[Action],
    exception: Option<u8>,
    trace_allowed: bool,
) -> Expected {
    let mut e = Expected {
        registers: r,
        queue,
        device: input.clone(),
        result: Ok(TimedStep {
            opcode: queue[0],
            clocks: 0,
            exception,
        }),
        trace: false,
    };
    let start = input.clock;
    for &action in actions {
        match action {
            Action::An(reg, value) => set_an(&mut e.registers, reg, value),
            Action::Sr(sr) => e.registers.set_status_register(sr & 0xa71f),
            Action::Commit(pc, words) => {
                e.registers.pc = pc;
                e.queue = words;
            }
            Action::Reserve => {
                if u64::MAX - e.device.clock < B {
                    e.result = Err(TimingFault::ClockOverflow);
                    break;
                }
            }
            Action::Idle(clocks) => {
                let Some(next) = e.device.clock.checked_add(u64::from(clocks)) else {
                    e.result = Err(TimingFault::ClockOverflow);
                    break;
                };
                e.device.events.push(Event::Idle(e.device.clock, clocks));
                e.device.clock = next;
            }
            Action::Read(a) | Action::Write(a, _) | Action::Ack(a) => {
                if u64::MAX - e.device.clock < B {
                    e.result = Err(TimingFault::ClockOverflow);
                    break;
                }
                let index = e.device.count;
                let wait = e.device.waits.get(index).copied().unwrap_or(0);
                let clock = e.device.clock;
                let direction = match action {
                    Action::Read(_) => {
                        let value = e.device.word(a.logical_address);
                        e.device.events.push(Event::Read(a, clock, value, wait));
                        Direction::Read
                    }
                    Action::Write(_, value) => {
                        e.device.put(a.logical_address, value);
                        e.device.events.push(Event::Write(a, clock, value, wait));
                        Direction::Write
                    }
                    Action::Ack(_) => {
                        e.device.events.push(Event::Ack(a, clock, wait));
                        Direction::Read
                    }
                    _ => unreachable!(),
                };
                e.device.count += 1;
                e.device.clock += 4 + u64::from(wait);
                if input.fail == Some(index) {
                    e.result = Err(TimingFault::Bus {
                        access: a,
                        direction,
                        source: index,
                    });
                    break;
                }
            }
        }
    }
    if e.result.is_ok() {
        e.result = Ok(TimedStep {
            opcode: queue[0],
            clocks: e.device.clock - start,
            exception,
        });
        e.trace = trace_allowed && r.status_register() & 0x8000 != 0;
    }
    e
}

pub(super) fn matches(cpu: &mut PrefetchCpu, bus: &Device, e: &Expected, irq: u8) -> bool {
    cpu.registers == e.registers
        && cpu.queued_words() == Some(e.queue)
        && cpu.clock().0 == e.device.clock
        && bus == &e.device
        && bus.valid
        && cpu.trace_pending() == e.trace
        && !cpu.stopped()
        && cpu.interrupt_input().level() == irq
        && cpu.physical_entry_state() == PhysicalEntryState::Idle
        && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues
}

pub(super) fn frozen(cpu: &mut PrefetchCpu, bus: &mut Device) -> bool {
    let c = cpu.clone();
    let b = bus.clone();
    cpu.step_instruction(bus) == Err(TimingFault::Frozen)
        && cpu.step_boundary(bus) == Err(TimingFault::Frozen)
        && cpu.prime(bus) == Err(TimingFault::Frozen)
        && cpu.service_pending_trace(bus) == Err(TimingFault::Frozen)
        && cpu.step(bus) == Err(TimingFault::Frozen)
        && *cpu == c
        && *bus == b
}

pub(super) fn handler(bus: &mut Device, vector: u8) {
    let address = u32::from(vector) * 4;
    bus.put(address, 0x3456);
    bus.put(address + 2, 0x6000);
    bus.put(0x3456_6000, 0x4e71);
    bus.put(0x3456_6002, 0x7201);
    bus.put(0x3456_6004, 0x7402);
}

pub(super) fn entry(r: Registers, vector: u8, irq: bool) -> Vec<Action> {
    let mut a = vec![Action::Idle(if irq { 6 } else { 4 })];
    write(&mut a, r.ssp.wrapping_sub(2), r.pc as u16);
    if irq {
        a.push(Action::Ack(access(
            0xffff_fffe,
            FunctionCode::CpuSpace,
            AccessKind::InterruptAcknowledge,
        )));
        a.push(Action::Idle(4));
    }
    write(&mut a, r.ssp.wrapping_sub(6), r.status_register());
    write(&mut a, r.ssp.wrapping_sub(4), (r.pc >> 16) as u16);
    read(
        &mut a,
        u32::from(vector) * 4,
        FunctionCode::SupervisorData,
        AccessKind::Vector,
    );
    read(
        &mut a,
        u32::from(vector) * 4 + 2,
        FunctionCode::SupervisorData,
        AccessKind::Vector,
    );
    read(
        &mut a,
        0x3456_6000,
        FunctionCode::SupervisorProgram,
        AccessKind::Prefetch,
    );
    a.push(Action::Idle(2));
    read(
        &mut a,
        0x3456_6002,
        FunctionCode::SupervisorProgram,
        AccessKind::Prefetch,
    );
    a.push(Action::Sr(if irq {
        (r.status_register() & !0x8700) | 0x2700
    } else {
        (r.status_register() | 0x2000) & !0x8000
    }));
    a.push(Action::An(7, r.ssp.wrapping_sub(6)));
    a.push(Action::Commit(0x3456_6000, [0x4e71, 0x7201]));
    a
}
