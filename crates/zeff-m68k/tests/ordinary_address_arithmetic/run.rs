use super::*;
use device::Event;

pub(super) fn seed(case: Case) -> (PrefetchCpu, Device, oracle::Plan) {
    let r = case.initial();
    let memory = case.memory(r);
    let plan = oracle::plan(case, &memory);
    let cpu =
        PrefetchCpu::from_prefetched(r, [case.opcode(), case.retained()], CpuClocks(case.start))
            .unwrap();
    (cpu, Device::new(case, memory), plan)
}
pub(super) fn events(
    case: Case,
    p: &oracle::Plan,
    waits: &[u32; 5],
    count: usize,
    tails: u64,
) -> Vec<Event> {
    let mut clock = case.start;
    let mut result = Vec::new();
    if case.idle() != 0 {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (at, t) in p.phases.iter().take(count).enumerate() {
        result.push(Event::Read(t.access, clock));
        clock += 4 + u64::from(waits[at]);
    }
    for _ in 0..tails {
        result.push(Event::Idle(clock, 2));
        clock += 2;
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
pub(super) fn run(case: Case) -> bool {
    let (mut cpu, mut bus, plan) = seed(case);
    let r = case.initial();
    let source = plan.phases[case.extensions()].access.logical_address;
    let input_good = source & 1 == 0
        && (case.mutate.is_some()
            || plan.source
                == if case.long {
                    case.value
                } else {
                    case.value & 65535
                });
    let result = cpu.step_instruction(&mut bus);
    let count = case.failure.map_or(case.count(), |at| at + 1);
    let clocks = case.idle()
        + count as u64 * (4 + u64::from(case.wait))
        + if case.failure.is_none() {
            case.tail()
        } else {
            0
        };
    let state = if let Some(at) = case.failure {
        result
            == Err(TimingFault::Bus {
                access: plan.phases[at].access,
                direction: Direction::Read,
                source: at,
            })
            && cpu.registers == plan.phases[at].registers
            && cpu.queued_words() == Some([case.opcode(), case.retained()])
            && !cpu.trace_pending()
            && frozen(&mut cpu, &mut bus)
    } else {
        result
            == Ok(TimedStep {
                opcode: case.opcode(),
                clocks,
                exception: None,
            })
            && cpu.registers == plan.full
            && cpu.queued_words() == Some(plan.queue)
            && !cpu.trace_pending()
    };
    let mut shadow = case.memory(r);
    if let Some((at, word)) = case.mutate.filter(|(at, _)| *at < count) {
        put(&mut shadow, plan.phases[at].access.logical_address, word);
    }
    input_good
        && state
        && bus.valid
        && bus.words == shadow
        && bus.count == count
        && cpu.clock().0 == case.start + clocks
        && bus.clock == cpu.clock().0
        && bus.events
            == events(
                case,
                &plan,
                &bus.waits,
                count,
                if case.failure.is_none() {
                    case.tail() / 2
                } else {
                    0
                },
            )
}
