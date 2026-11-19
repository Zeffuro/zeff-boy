use super::*;
use device::{Device, Event};
use fixture::Case;

pub(super) fn seed(c: Case) -> (PrefetchCpu, Device, oracle::Plan) {
    let r = c.initial();
    let memory = c.memory(r);
    let plan = oracle::plan(c, &memory);
    let cpu =
        PrefetchCpu::from_prefetched(r, [c.opcode(), c.retained()], CpuClocks(c.start)).unwrap();
    let mut bus = Device::new(memory, c.start, c.failure);
    bus.waits = [c.wait; 5];
    bus.mutate = c.mutate;
    (cpu, bus, plan)
}
pub(super) fn events(
    c: Case,
    p: &oracle::Plan,
    waits: &[u32; 5],
    count: usize,
    tail: bool,
) -> Vec<Event> {
    let mut clock = c.start;
    let mut result = Vec::new();
    if c.idle() != 0 {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (at, t) in p.phases.iter().take(count).enumerate() {
        result.push(Event::Read(t.access, clock, t.word));
        clock += 4 + u64::from(waits[at]);
    }
    if tail && c.long {
        result.push(Event::Idle(clock, 2));
    }
    result
}
pub(super) fn frozen(cpu: &mut PrefetchCpu, bus: &mut Device) -> bool {
    let before = cpu.clone();
    let device = bus.clone();
    cpu.prime(bus) == Err(TimingFault::Frozen)
        && cpu.step_instruction(bus) == Err(TimingFault::Frozen)
        && cpu.step(bus) == Err(TimingFault::Frozen)
        && cpu.step_boundary(bus) == Err(TimingFault::Frozen)
        && cpu.service_pending_trace(bus) == Err(TimingFault::Frozen)
        && *cpu == before
        && *bus == device
}
pub(super) fn check(c: Case) -> bool {
    let (mut cpu, mut bus, p) = seed(c);
    let source = p.phases[c.extensions()].access.logical_address;
    let input = source & 1 == 0
        && (c.mutate.is_some() || p.source == if c.long { c.value } else { c.value & 65535 });
    let result = cpu.step_instruction(&mut bus);
    let count = c.failure.map_or(c.count(), |at| at + 1);
    let clocks = c.idle()
        + count as u64 * (4 + u64::from(c.wait))
        + if c.failure.is_none() { c.tail() } else { 0 };
    let state = if let Some(at) = c.failure {
        result
            == Err(TimingFault::Bus {
                access: p.phases[at].access,
                direction: Direction::Read,
                source: at,
            })
            && cpu.registers == p.phases[at].registers
            && cpu.queued_words() == Some([c.opcode(), c.retained()])
            && frozen(&mut cpu, &mut bus)
    } else {
        result
            == Ok(TimedStep {
                opcode: c.opcode(),
                clocks,
                exception: None,
            })
            && cpu.registers == p.full
            && cpu.queued_words() == Some(p.queue)
    };
    let mut shadow = c.memory(c.initial());
    if let Some((at, word)) = c.mutate.filter(|(at, _)| *at < count) {
        put(&mut shadow, p.phases[at].access.logical_address, word);
    }
    input
        && state
        && !cpu.trace_pending()
        && bus.valid
        && bus.words == shadow
        && bus.count == count
        && cpu.clock().0 == c.start + clocks
        && bus.clock == cpu.clock().0
        && bus.events == events(c, &p, &bus.waits, count, c.failure.is_none())
}
